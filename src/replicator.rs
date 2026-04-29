// Couchbase Lite replicator API
//
// Copyright (c) 2020 Couchbase, Inc All rights reserved.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//

#![allow(non_upper_case_globals)]

use std::{
    ptr,
    collections::{HashMap, HashSet},
    sync::mpsc::channel,
    time::Duration,
};
#[cfg(feature = "enterprise")]
use crate::Database;
use crate::{
    CblRef, Dict, Document, Error, ListenerToken, MutableDict, Result, check_error, release,
    slice::{from_str, self},
    c_api::{
        CBLListener_Remove, CBLAuth_CreatePassword, CBLAuth_CreateSession, CBLAuthenticator,
        CBLDocument, CBLDocumentFlags, CBLEndpoint, CBLEndpoint_CreateWithURL, CBLError,
        CBLProxySettings, CBLProxyType, CBLReplicatedDocument, CBLReplicator,
        CBLReplicatorConfiguration, CBLReplicatorStatus, CBLReplicatorType,
        CBLReplicator_AddChangeListener, CBLReplicator_AddDocumentReplicationListener,
        CBLReplicator_Create, CBLReplicator_IsDocumentPending, CBLReplicator_PendingDocumentIDs,
        CBLReplicator_SetHostReachable, CBLReplicator_SetSuspended, CBLReplicator_Start,
        CBLReplicator_Status, CBLReplicator_Stop, FLDict, FLString, kCBLDocumentFlagsAccessRemoved,
        kCBLDocumentFlagsDeleted, kCBLProxyHTTP, kCBLProxyHTTPS, kCBLReplicatorBusy,
        kCBLReplicatorConnecting, kCBLReplicatorIdle, kCBLReplicatorOffline, kCBLReplicatorStopped,
        kCBLReplicatorTypePull, kCBLReplicatorTypePush, kCBLReplicatorTypePushAndPull,
        CBLReplicationCollection, kCBLDefaultReplicatorAcceptParentCookies,
        kCBLDefaultReplicatorContinuous, kCBLDefaultReplicatorDisableAutoPurge,
        kCBLDefaultReplicatorHeartbeat, kCBLDefaultReplicatorMaxAttemptsSingleShot,
        kCBLDefaultReplicatorMaxAttemptsWaitTime, kCBLDefaultReplicatorType,
    },
    MutableArray, Listener,
    collection::Collection,
};
#[cfg(feature = "enterprise")]
use crate::{
    CouchbaseLiteError, ErrorCode,
    c_api::{
        CBLEndpoint_CreateWithLocalDB, FLSlice, FLSliceResult, FLSliceResult_New, FLSlice_Copy,
        FLStringResult,
    },
    slice::from_bytes,
};

//======== CONFIGURATION

/** Represents the location of a database to replicate with. */
#[derive(Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub(crate) cbl_ref: *mut CBLEndpoint,
    pub url: Option<String>,
}

impl CblRef for Endpoint {
    type Output = *mut CBLEndpoint;
    fn get_ref(&self) -> Self::Output {
        self.cbl_ref
    }
}

impl Endpoint {
    pub fn new_with_url(url: &str) -> Result<Self> {
        unsafe {
            let mut error = CBLError::default();
            let endpoint: *mut CBLEndpoint =
                CBLEndpoint_CreateWithURL(from_str(url).get_ref(), std::ptr::addr_of_mut!(error));

            check_error(&error).map(|_| Self {
                cbl_ref: endpoint,
                url: Some(url.to_string()),
            })
        }
    }

    #[cfg(feature = "enterprise")]
    pub fn new_with_local_db(db: &Database) -> Self {
        unsafe {
            Self {
                cbl_ref: CBLEndpoint_CreateWithLocalDB(db.get_ref()),
                url: None,
            }
        }
    }
}

impl Clone for Endpoint {
    fn clone(&self) -> Self {
        Self {
            cbl_ref: self.cbl_ref,
            url: self.url.clone(),
        }
    }
}

impl Drop for Endpoint {
    fn drop(&mut self) {
        unsafe {
            release(self.get_ref());
        }
    }
}

/** An opaque object representing authentication credentials for a remote server. */
#[derive(Debug, PartialEq, Eq)]
pub struct Authenticator {
    pub(crate) cbl_ref: *mut CBLAuthenticator,
}

impl CblRef for Authenticator {
    type Output = *mut CBLAuthenticator;
    fn get_ref(&self) -> Self::Output {
        self.cbl_ref
    }
}

impl Authenticator {
    pub fn create_password(username: &str, password: &str) -> Self {
        unsafe {
            Self {
                cbl_ref: CBLAuth_CreatePassword(
                    from_str(username).get_ref(),
                    from_str(password).get_ref(),
                ),
            }
        }
    }

    pub fn create_session(session_id: &str, cookie_name: &str) -> Self {
        unsafe {
            Self {
                cbl_ref: CBLAuth_CreateSession(
                    from_str(session_id).get_ref(),
                    from_str(cookie_name).get_ref(),
                ),
            }
        }
    }
}

impl Clone for Authenticator {
    fn clone(&self) -> Self {
        Self {
            cbl_ref: self.cbl_ref,
        }
    }
}

/** Direction of replication: push, pull, or both. */
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplicatorType {
    PushAndPull,
    Push,
    Pull,
}

impl From<CBLReplicatorType> for ReplicatorType {
    fn from(repl_type: CBLReplicatorType) -> Self {
        match u32::from(repl_type) {
            kCBLReplicatorTypePushAndPull => Self::PushAndPull,
            kCBLReplicatorTypePush => Self::Push,
            kCBLReplicatorTypePull => Self::Pull,
            _ => unreachable!(),
        }
    }
}
impl From<ReplicatorType> for CBLReplicatorType {
    fn from(repl_type: ReplicatorType) -> Self {
        match repl_type {
            ReplicatorType::PushAndPull => kCBLReplicatorTypePushAndPull as Self,
            ReplicatorType::Push => kCBLReplicatorTypePush as Self,
            ReplicatorType::Pull => kCBLReplicatorTypePull as Self,
        }
    }
}

/** Types of proxy servers, for CBLProxySettings. */
#[derive(Debug, PartialEq, Eq)]
pub enum ProxyType {
    HTTP,
    HTTPS,
}

impl From<CBLProxyType> for ProxyType {
    fn from(proxy_type: CBLProxyType) -> Self {
        match u32::from(proxy_type) {
            kCBLProxyHTTP => Self::HTTP,
            kCBLProxyHTTPS => Self::HTTPS,
            _ => unreachable!(),
        }
    }
}
impl From<ProxyType> for CBLProxyType {
    fn from(proxy_type: ProxyType) -> Self {
        match proxy_type {
            ProxyType::HTTP => kCBLProxyHTTP as Self,
            ProxyType::HTTPS => kCBLProxyHTTPS as Self,
        }
    }
}

/** Proxy settings for the replicator. */
#[derive(Debug)]
pub struct ProxySettings {
    /// Proxy server hostname or IP address. Mandatory
    pub hostname: String,
    /// Username for proxy auth (optional, per CBLReplicator.h).
    pub username: Option<String>,
    /// Password for proxy auth (only meaningful when `username` is set).
    pub password: Option<String>,
    cbl: CBLProxySettings,
}

impl ProxySettings {
    pub fn new(
        proxy_type: ProxyType,
        hostname: String,
        port: u16,
        username: Option<String>,
        password: Option<String>,
    ) -> Self {
        let cbl = CBLProxySettings {
            type_: proxy_type.into(),
            hostname: from_str(&hostname).get_ref(),
            port,
            username: username
                .as_ref()
                .map_or(slice::NULL_SLICE, |s| from_str(s).get_ref()),
            password: password
                .as_ref()
                .map_or(slice::NULL_SLICE, |s| from_str(s).get_ref()),
        };

        Self {
            hostname,
            username,
            password,
            cbl,
        }
    }
}

impl CblRef for ProxySettings {
    type Output = *const CBLProxySettings;
    fn get_ref(&self) -> Self::Output {
        std::ptr::addr_of!(self.cbl)
    }
}

//======== CALLBACKS

/// Identifies a collection by `(scope_name, collection_name)`, used to look up the
/// per-collection filter or conflict resolver from the context inside the C callback.
type CollectionKey = (String, String);
/// Returns the `(scope_name, collection_name)` pair for the document's collection, or
/// `None` if the document has not been saved (which shouldn't reach a replicator callback).
fn document_collection_key(doc: &Document) -> Option<CollectionKey> {
    doc.collection().map(|c| (c.scope().name(), c.name()))
}

/** A callback that can decide whether a particular document should be pushed or pulled. */
pub type ReplicationFilter = Box<dyn Fn(&Document, bool, bool) -> bool>;

unsafe extern "C" fn c_replication_push_filter(
    context: *mut ::std::os::raw::c_void,
    document: *mut CBLDocument,
    flags: CBLDocumentFlags,
) -> bool {
    unsafe {
        let doc = Document::reference(document);
        // Default behaviour matches CBL's "no filter installed": replicate the document.
        // Reached only if key derivation or the per-collection lookup unexpectedly misses.
        let Some(key) = document_collection_key(&doc) else {
            return true;
        };
        let repl_conf_context = &*(context as *const ReplicationConfigurationContext);
        let (is_deleted, is_access_removed) = read_document_flags(flags);

        repl_conf_context
            .push_filters
            .get(&key)
            .is_none_or(|callback| callback(&doc, is_deleted, is_access_removed))
    }
}
unsafe extern "C" fn c_replication_pull_filter(
    context: *mut ::std::os::raw::c_void,
    document: *mut CBLDocument,
    flags: CBLDocumentFlags,
) -> bool {
    unsafe {
        let doc = Document::reference(document);
        let Some(key) = document_collection_key(&doc) else {
            return true;
        };
        let repl_conf_context = &*(context as *const ReplicationConfigurationContext);
        let (is_deleted, is_access_removed) = read_document_flags(flags);

        repl_conf_context
            .pull_filters
            .get(&key)
            .is_none_or(|callback| callback(&doc, is_deleted, is_access_removed))
    }
}
fn read_document_flags(flags: CBLDocumentFlags) -> (bool, bool) {
    (flags & DELETED != 0, flags & ACCESS_REMOVED != 0)
}

/** Conflict-resolution callback for use in replications. This callback will be invoked
when the replicator finds a newer server-side revision of a document that also has local
changes. The local and remote changes must be resolved before the document can be pushed
to the server. */
pub type ConflictResolver =
    Box<dyn Fn(&str, Option<Document>, Option<Document>) -> Option<Document>>;

unsafe extern "C" fn c_replication_conflict_resolver(
    context: *mut ::std::os::raw::c_void,
    document_id: FLString,
    local_document: *const CBLDocument,
    remote_document: *const CBLDocument,
) -> *const CBLDocument {
    // Fallback used when the per-collection resolver lookup unexpectedly misses or the
    // doc has no collection. Mirrors `CBLDefaultConflictResolver`: local wins, falling
    // back to remote if local is absent. Returning null would *delete* the document
    // (CBLReplicator.h:136) so it is not a safe default.
    let default_resolution = if !local_document.is_null() {
        local_document
    } else {
        remote_document
    };

    unsafe {
        let local_document = if local_document.is_null() {
            None
        } else {
            Some(Document::reference(local_document as *mut CBLDocument))
        };
        let remote_document = if remote_document.is_null() {
            None
        } else {
            Some(Document::reference(remote_document as *mut CBLDocument))
        };

        let key_doc = local_document.as_ref().or(remote_document.as_ref());
        let Some(key) = key_doc.and_then(document_collection_key) else {
            return default_resolution;
        };
        let repl_conf_context = &*(context as *const ReplicationConfigurationContext);
        let doc_id = document_id.to_string().unwrap_or_default();

        repl_conf_context
            .conflict_resolvers
            .get(&key)
            .map_or(default_resolution, |callback| {
                callback(&doc_id, local_document, remote_document)
                    .map_or(ptr::null(), |d| d.get_ref() as *const CBLDocument)
            })
    }
}

#[derive(Debug, PartialEq)]
pub enum EncryptionError {
    Temporary, // The replicator will stop the replication when encountering this error, then restart and try encrypting/decrypting the document again
    Permanent, // The replicator will bypass the document and not try encrypting/decrypting the document until a new revision is created
}

/// Translates an [`EncryptionError`] returned by a user closure into the [`Error`] we
/// report back to CBL through the `cbl_error` out-pointer. Shared by the encryptor and
/// decryptor C callbacks.
#[cfg(feature = "enterprise")]
fn encryption_error_to_cbl(err: EncryptionError) -> Error {
    match err {
        // Transient: CBL stops the replication and retries the document later.
        // WebSocket 503 is the agreed-upon signal for "temporary, please retry".
        EncryptionError::Temporary => Error {
            code: ErrorCode::WebSocket(503),
            internal_info: None,
        },
        // Permanent: bypass this revision until a new one is created.
        EncryptionError::Permanent => Error::cbl_error(CouchbaseLiteError::Crypto),
    }
}

/** Callback that encrypts encryptable properties in documents pushed by the replicator.
\note   If a null result or an error is returned, the document will be failed to
        replicate with the kCBLErrorCrypto error. For security reason, the encryption
        cannot be skipped. */
#[cfg(feature = "enterprise")]
pub type CollectionPropertyEncryptor = fn(
    scope: Option<String>,
    collection: Option<String>,
    document_id: Option<String>,
    properties: Dict,
    key_path: Option<String>,
    input: Vec<u8>,
    algorithm: Option<String>,
    kid: Option<String>,
    error: &Error,
) -> std::result::Result<Vec<u8>, EncryptionError>;
#[cfg(feature = "enterprise")]
extern "C" fn c_collection_property_encryptor(
    context: *mut ::std::os::raw::c_void,
    scope: FLString,
    collection: FLString,
    document_id: FLString,
    properties: FLDict,
    key_path: FLString,
    input: FLSlice,
    algorithm: *mut FLStringResult,
    kid: *mut FLStringResult,
    cbl_error: *mut CBLError,
) -> FLSliceResult {
    unsafe {
        let ctx = context as *const ReplicationConfigurationContext;

        // No encryptor registered: leave the property untouched.
        let Some(callback) = (*ctx).collection_property_encryptor else {
            return FLSliceResult_New(0);
        };

        // Invalid input bytes: encryption is not skippable for security reasons (see
        // header doc), so signal a permanent crypto error.
        let Some(plaintext) = input.to_vec() else {
            *cbl_error = Error::cbl_error(CouchbaseLiteError::Crypto).as_cbl_error();
            return FLSliceResult::null();
        };

        let result = callback(
            scope.to_string(),
            collection.to_string(),
            document_id.to_string(),
            Dict::wrap(properties, &properties),
            key_path.to_string(),
            plaintext,
            algorithm.as_ref().and_then(|s| s.clone().to_string()),
            kid.as_ref().and_then(|s| s.clone().to_string()),
            &Error::default(),
        );

        match result {
            Ok(ciphertext) => FLSlice_Copy(from_bytes(&ciphertext[..]).get_ref()),
            Err(err) => {
                *cbl_error = encryption_error_to_cbl(err).as_cbl_error();
                FLSliceResult::null()
            }
        }
    }
}

/** Callback that decrypts encrypted encryptable properties in documents pulled by the replicator.
\note   The decryption will be skipped (the encrypted data will be kept) when a null result
        without an error is returned. If an error is returned, the document will be failed to replicate
        with the kCBLErrorCrypto error. */
#[cfg(feature = "enterprise")]
pub type CollectionPropertyDecryptor = fn(
    scope: Option<String>,
    collection: Option<String>,
    document_id: Option<String>,
    properties: Dict,
    key_path: Option<String>,
    input: Vec<u8>,
    algorithm: Option<String>,
    kid: Option<String>,
    error: &Error,
) -> std::result::Result<Vec<u8>, EncryptionError>;
#[cfg(feature = "enterprise")]
extern "C" fn c_collection_property_decryptor(
    context: *mut ::std::os::raw::c_void,
    scope: FLString,
    collection: FLString,
    document_id: FLString,
    properties: FLDict,
    key_path: FLString,
    input: FLSlice,
    algorithm: FLString,
    kid: FLString,
    cbl_error: *mut CBLError,
) -> FLSliceResult {
    unsafe {
        let ctx = context as *const ReplicationConfigurationContext;

        // No decryptor registered: keep the encrypted data as-is (per header doc, an
        // empty result with no error means "skip decryption").
        let Some(callback) = (*ctx).collection_property_decryptor else {
            return FLSliceResult_New(0);
        };

        let Some(ciphertext) = input.to_vec() else {
            *cbl_error = Error::cbl_error(CouchbaseLiteError::Crypto).as_cbl_error();
            return FLSliceResult::null();
        };

        let result = callback(
            scope.to_string(),
            collection.to_string(),
            document_id.to_string(),
            Dict::wrap(properties, &properties),
            key_path.to_string(),
            ciphertext,
            algorithm.to_string(),
            kid.to_string(),
            &Error::default(),
        );

        match result {
            Ok(plaintext) => FLSlice_Copy(from_bytes(&plaintext[..]).get_ref()),
            Err(err) => {
                *cbl_error = encryption_error_to_cbl(err).as_cbl_error();
                FLSliceResult::null()
            }
        }
    }
}

/// Internal storage handed to the C replicator as its `void* context`. The C callbacks cast
/// this back to `&ReplicationConfigurationContext` to look up per-collection closures.
///
/// Built by [`Replicator::new`] from the public [`ReplicatorConfiguration`] — users do not
/// construct this directly. Filter/resolver maps are populated by draining the per-collection
/// callbacks out of each [`ReplicationCollection`]; encryptor/decryptor are moved off the
/// configuration. Filters and the conflict resolver are keyed by `(scope_name,
/// collection_name)` because the C callback signatures only receive the document, not the
/// collection — see [`document_collection_key`].
#[derive(Default)]
struct ReplicationConfigurationContext {
    push_filters: HashMap<CollectionKey, ReplicationFilter>,
    pull_filters: HashMap<CollectionKey, ReplicationFilter>,
    conflict_resolvers: HashMap<CollectionKey, ConflictResolver>,
    #[cfg(feature = "enterprise")]
    collection_property_encryptor: Option<CollectionPropertyEncryptor>,
    #[cfg(feature = "enterprise")]
    collection_property_decryptor: Option<CollectionPropertyDecryptor>,
}

pub struct ReplicationCollection {
    /// The collection (Required)
    pub collection: Collection,
    /// Conflict-resolver callback.
    pub conflict_resolver: Option<ConflictResolver>,
    /// Callback to filter which docs are pushed.
    pub push_filter: Option<ReplicationFilter>,
    /// Callback to filter which docs are pulled.
    pub pull_filter: Option<ReplicationFilter>,
    /// Set of channels to pull from. Only applicable when replicating with Sync Gateway.
    pub channels: MutableArray,
    /// Set of document IDs to replicate.
    pub document_ids: MutableArray,
}

impl ReplicationCollection {
    /// Creates a `ReplicationCollection` with no per-collection callbacks, no channel
    /// filter, and no document-id filter. Override fields via struct-update syntax:
    /// `ReplicationCollection { push_filter: Some(..), ..ReplicationCollection::new(c) }`.
    pub fn new(collection: Collection) -> Self {
        Self {
            collection,
            conflict_resolver: None,
            push_filter: None,
            pull_filter: None,
            channels: MutableArray::default(),
            document_ids: MutableArray::default(),
        }
    }

    /// Returns the `(scope_name, collection_name)` key used to look up this collection's
    /// callbacks in [`ReplicationConfigurationContext`].
    fn key(&self) -> CollectionKey {
        (self.collection.scope().name(), self.collection.name())
    }

    /// Builds the C-side `CBLReplicationCollection`. Function pointers are set based on
    /// what the context contains for this collection's key, so the C callback is only
    /// installed for collections that actually have a closure registered.
    fn to_cbl_replication_collection(
        &self,
        context: &ReplicationConfigurationContext,
    ) -> CBLReplicationCollection {
        let key = self.key();
        CBLReplicationCollection {
            collection: self.collection.get_ref(),
            conflictResolver: context
                .conflict_resolvers
                .contains_key(&key)
                .then_some(c_replication_conflict_resolver as _),
            pushFilter: context
                .push_filters
                .contains_key(&key)
                .then_some(c_replication_push_filter as _),
            pullFilter: context
                .pull_filters
                .contains_key(&key)
                .then_some(c_replication_pull_filter as _),
            channels: self.channels.get_ref(),
            documentIDs: self.document_ids.get_ref(),
        }
    }
}

/** The configuration of a replicator. */
pub struct ReplicatorConfiguration {
    /** Required fields: */

    /// The collections to replicate with the target's endpoint (Required)
    pub collections: Vec<ReplicationCollection>,
    /// The replication endpoint to replicate with (Required)
    pub endpoint: Endpoint,

    /** Core options and context: */

    /// Push, pull or both
    pub replicator_type: ReplicatorType,
    /// Continuous replication?
    pub continuous: bool,
    /// Authentication credentials, if needed
    pub authenticator: Option<Authenticator>,

    /** TLS settings */

    /// X.509 certificate (PEM or DER) to pin for TLS connections. The cert chain is valid only if it contains this cert.
    pub pinned_server_certificate: Option<Vec<u8>>,

    //** Auto Purge: */
    /** If auto purge is active, documents that the replicating user loses access to will be purged automatically.
    If disableAutoPurge is true, this behavior is disabled and an access removed event will be sent to
    document replication listeners if specified. Default is \ref kCBLDefaultReplicatorDisableAutoPurge.

    \note Auto Purge is only applicable when replicating with Sync Gateway,
          and will not be performed when a documentIDs filter is specified. */
    pub disable_auto_purge: bool,

    //** Retry Logic: */
    /// Max retry attempts where the initial connect to replicate counts toward the given value.
    pub max_attempts: u32,
    /// Max wait time between retry attempts in seconds.
    pub max_attempt_wait_time: u32,

    //** WebSocket: */
    /// The heartbeat interval in seconds.
    pub heartbeat: u32,

    //** */ HTTP settings: */
    /// Extra HTTP headers to add to the WebSocket request
    pub headers: HashMap<String, String>,
    /// HTTP client proxy settings
    pub proxy: Option<ProxySettings>,
    /** The option to remove the restriction that does not allow the replicator to save the parent-domain
    cookies, the cookies whose domains are the parent domain of the remote host, from the HTTP
    response. For example, when the option is set to true, the cookies whose domain are “.foo.com”
    returned by “bar.foo.com” host will be permitted to save. This is only recommended if the host
    issuing the cookie is well trusted.

    This option is disabled by default (see \ref kCBLDefaultReplicatorAcceptParentCookies) which means
    that the parent-domain cookies are not permitted to save by default. */
    pub accept_parent_domain_cookies: bool,

    //** Advance TLS Settings: */
    /// Set of anchor certs (PEM format)
    pub trusted_root_certificates: Option<Vec<u8>>,

    /** Accept only self-signed certificates; any other certificates are rejected. */
    #[cfg(feature = "enterprise")]
    pub accept_only_self_signed_server_certificate: bool,

    //** Property Encryption (Enterprise only): */
    /// Callback invoked for every \ref Encryptable property in documents being pushed.
    /// The callback receives the scope and collection so a single function can dispatch
    /// per-collection if needed.
    #[cfg(feature = "enterprise")]
    pub collection_property_encryptor: Option<CollectionPropertyEncryptor>,

    /// Callback invoked for every encrypted property in documents being pulled.
    #[cfg(feature = "enterprise")]
    pub collection_property_decryptor: Option<CollectionPropertyDecryptor>,
}

impl ReplicatorConfiguration {
    /// Creates a configuration with the required `endpoint` and `collections`, and every
    /// other field set to the CBL-defined default (the `kCBLDefaultReplicator*` constants
    /// from `CBLReplicator.h`). Override any field via struct-update syntax:
    ///
    /// ```ignore
    /// ReplicatorConfiguration {
    ///     continuous: true,
    ///     heartbeat: 60,
    ///     ..ReplicatorConfiguration::new(endpoint, collections)
    /// }
    /// ```
    pub fn new(endpoint: Endpoint, collections: Vec<ReplicationCollection>) -> Self {
        // Reading these `pub static` items requires `unsafe` because they are
        // declared inside `unsafe extern "C"` in the bindgen output. They are
        // link-time constants and reading them is sound.
        unsafe {
            Self {
                collections,
                endpoint,
                replicator_type: ReplicatorType::from(kCBLDefaultReplicatorType),
                continuous: kCBLDefaultReplicatorContinuous,
                authenticator: None,
                pinned_server_certificate: None,
                disable_auto_purge: kCBLDefaultReplicatorDisableAutoPurge,
                max_attempts: kCBLDefaultReplicatorMaxAttemptsSingleShot,
                max_attempt_wait_time: kCBLDefaultReplicatorMaxAttemptsWaitTime,
                heartbeat: kCBLDefaultReplicatorHeartbeat,
                headers: HashMap::new(),
                proxy: None,
                accept_parent_domain_cookies: kCBLDefaultReplicatorAcceptParentCookies,
                trusted_root_certificates: None,
                #[cfg(feature = "enterprise")]
                accept_only_self_signed_server_certificate: false,
                #[cfg(feature = "enterprise")]
                collection_property_encryptor: None,
                #[cfg(feature = "enterprise")]
                collection_property_decryptor: None,
            }
        }
    }
}

//======== LIFECYCLE

type ReplicatorsListeners<T> = Vec<Listener<Box<T>>>;

/** A background task that syncs a \ref Database with a remote server or peer. */
pub struct Replicator {
    cbl_ref: *mut CBLReplicator,
    pub config: Option<ReplicatorConfiguration>,
    pub headers: Option<MutableDict>,
    /// Outlives the C replicator: CBL keeps the raw pointer to this box and dereferences
    /// it from the C callbacks, so it must not move or drop until the replicator does.
    #[allow(dead_code)]
    context: Option<Box<ReplicationConfigurationContext>>,
    change_listeners: ReplicatorsListeners<ReplicatorChangeListener>,
    pub document_listeners: ReplicatorsListeners<ReplicatedDocumentListener>,
}

impl CblRef for Replicator {
    type Output = *mut CBLReplicator;
    fn get_ref(&self) -> Self::Output {
        self.cbl_ref
    }
}

impl Replicator {
    /** Creates a replicator with the given configuration.

    Per-collection `push_filter`, `pull_filter` and `conflict_resolver` closures set on each
    [`ReplicationCollection`] are drained into an internal context before the C replicator
    is created. CBL's filter and conflict-resolver C callbacks only receive the document
    (not the collection), so they look up the right closure by reading `(scope_name,
    collection_name)` off the document via [`document_collection_key`]. */
    pub fn new(mut config: ReplicatorConfiguration) -> Result<Self> {
        unsafe {
            let mut context = Box::<ReplicationConfigurationContext>::default();

            for c in &mut config.collections {
                let key = c.key();
                if let Some(f) = c.push_filter.take() {
                    context.push_filters.insert(key.clone(), f);
                }
                if let Some(f) = c.pull_filter.take() {
                    context.pull_filters.insert(key.clone(), f);
                }
                if let Some(r) = c.conflict_resolver.take() {
                    context.conflict_resolvers.insert(key, r);
                }
            }

            #[cfg(feature = "enterprise")]
            {
                context.collection_property_encryptor = config.collection_property_encryptor;
                context.collection_property_decryptor = config.collection_property_decryptor;
            }

            let headers = MutableDict::from_hashmap(&config.headers);
            let mut collections: Vec<CBLReplicationCollection> = config
                .collections
                .iter()
                .map(|c| c.to_cbl_replication_collection(&context))
                .collect();

            let cbl_config = CBLReplicatorConfiguration {
                //-- Required fields:
                collections: collections.as_mut_ptr(),
                collectionCount: collections.len(),
                endpoint: config.endpoint.get_ref(),
                //-- Core options and context:
                replicatorType: config.replicator_type.clone().into(),
                continuous: config.continuous,
                authenticator: config
                    .authenticator
                    .as_ref()
                    .map_or(ptr::null_mut(), CblRef::get_ref),
                context: std::ptr::addr_of!(*context) as *mut _,
                //-- TLS settings
                pinnedServerCertificate: config
                    .pinned_server_certificate
                    .as_ref()
                    .map_or(slice::NULL_SLICE, |c| slice::from_bytes(c).get_ref()),
                //-- Auto Purge:
                disableAutoPurge: config.disable_auto_purge,
                //-- Retry Logic:
                maxAttempts: config.max_attempts,
                maxAttemptWaitTime: config.max_attempt_wait_time,
                //-- WebSocket:
                heartbeat: config.heartbeat,
                //-- HTTP settings:
                headers: headers.as_dict().get_ref(),
                proxy: config
                    .proxy
                    .as_ref()
                    .map_or(ptr::null_mut(), CblRef::get_ref),
                acceptParentDomainCookies: config.accept_parent_domain_cookies,
                //-- Advance TLS Settings:
                trustedRootCertificates: config
                    .trusted_root_certificates
                    .as_ref()
                    .map_or(slice::NULL_SLICE, |c| slice::from_bytes(c).get_ref()),
                #[cfg(feature = "enterprise")]
                acceptOnlySelfSignedServerCertificate: config
                    .accept_only_self_signed_server_certificate,
                //-- Property Encryption
                #[cfg(feature = "enterprise")]
                documentPropertyEncryptor: context
                    .collection_property_encryptor
                    .as_ref()
                    .and(Some(c_collection_property_encryptor)),
                #[cfg(feature = "enterprise")]
                documentPropertyDecryptor: context
                    .collection_property_decryptor
                    .as_ref()
                    .and(Some(c_collection_property_decryptor)),
            };

            let mut error = CBLError::default();
            let replicator = CBLReplicator_Create(&cbl_config, std::ptr::addr_of_mut!(error));

            check_error(&error).map(move |_| Self {
                cbl_ref: replicator,
                config: Some(config),
                headers: Some(headers),
                context: Some(context),
                change_listeners: vec![],
                document_listeners: vec![],
            })
        }
    }

    /** Starts a replicator, asynchronously. Does nothing if it's already started. */
    pub fn start(&mut self, reset_checkpoint: bool) {
        unsafe {
            CBLReplicator_Start(self.get_ref(), reset_checkpoint);
        }
    }

    /** Stops a running replicator, asynchronously. Does nothing if it's not already started.
    The replicator will call your \ref CBLReplicatorChangeListener with an activity level of
    \ref kCBLReplicatorStopped after it stops. Until then, consider it still active.
    The parameter timout_seconds has a default value of 10. */
    pub fn stop(&mut self, timeout_seconds: Option<u64>) -> bool {
        unsafe {
            let timeout_seconds = timeout_seconds.unwrap_or(10);
            let (sender, receiver) = channel();
            let callback: ReplicatorChangeListener = Box::new(move |status| {
                if status.activity == ReplicatorActivityLevel::Stopped {
                    let _ = sender.send(true);
                }
            });

            let token = CBLReplicator_AddChangeListener(
                self.get_ref(),
                Some(c_replicator_change_listener),
                std::mem::transmute(&callback),
            );

            let mut success = true;
            if self.status().activity != ReplicatorActivityLevel::Stopped {
                CBLReplicator_Stop(self.get_ref());
                success = receiver
                    .recv_timeout(Duration::from_secs(timeout_seconds))
                    .is_ok();
            }
            CBLListener_Remove(token);
            success
        }
    }

    /** Informs the replicator whether it's considered possible to reach the remote host with
    the current network configuration. The default value is true. This only affects the
    replicator's behavior while it's in the Offline state:
    * Setting it to false will cancel any pending retry and prevent future automatic retries.
    * Setting it back to true will initiate an immediate retry.*/
    pub fn set_host_reachable(&mut self, reachable: bool) {
        unsafe {
            CBLReplicator_SetHostReachable(self.get_ref(), reachable);
        }
    }

    /** Puts the replicator in or out of "suspended" state. The default is false.
    * Setting suspended=true causes the replicator to disconnect and enter Offline state;
      it will not attempt to reconnect while it's suspended.
    * Setting suspended=false causes the replicator to attempt to reconnect, _if_ it was
      connected when suspended, and is still in Offline state. */
    pub fn set_suspended(&mut self, suspended: bool) {
        unsafe {
            CBLReplicator_SetSuspended(self.get_ref(), suspended);
        }
    }

    /** Returns the replicator's current status. */
    pub fn status(&self) -> ReplicatorStatus {
        unsafe { CBLReplicator_Status(self.get_ref()).into() }
    }

    /** Indicates which documents in the given collection have local changes that have not yet been
    pushed to the server by this replicator. This is of course a snapshot, that will go out of date
    as the replicator makes progress and/or documents are saved locally.

    The result is, effectively, a set of document IDs: a dictionary whose keys are the IDs and
    values are `true`.
    If there are no pending documents, the dictionary is empty.
    @warning If the given collection is not part of the replication, an error will be returned. */
    pub fn pending_document_ids(&self, collection: Collection) -> Result<HashSet<String>> {
        unsafe {
            let mut error = CBLError::default();
            let docs: FLDict = CBLReplicator_PendingDocumentIDs(
                self.get_ref(),
                collection.get_ref(),
                std::ptr::addr_of_mut!(error),
            );

            check_error(&error).and_then(|()| {
                if docs.is_null() {
                    return Err(Error::default());
                }

                let dict = Dict::wrap(docs, self);
                Ok(dict.to_keys_hash_set())
            })
        }
    }

    /** Indicates whether the document with the given ID in the given collection has local changes
    that have not yet been pushed to the server by this replicator.

    This is equivalent to, but faster than, calling \ref pending_document_ids and
    checking whether the result contains \p doc_id. See that function's documentation for details.
    @warning  If the given collection is not part of the replication, a NULL with an error will be returned. */
    pub fn is_document_pending(&self, collection: Collection, doc_id: &str) -> Result<bool> {
        unsafe {
            let mut error = CBLError::default();
            let result = CBLReplicator_IsDocumentPending(
                self.get_ref(),
                from_str(doc_id).get_ref(),
                collection.get_ref(),
                std::ptr::addr_of_mut!(error),
            );
            check_error(&error).map(|_| result)
        }
    }

    /**
     Adds a listener that will be called when the replicator's status changes.
    */
    #[must_use]
    pub fn add_change_listener(mut self, listener: ReplicatorChangeListener) -> Self {
        let listener = unsafe {
            let listener = Box::new(listener);
            let ptr = Box::into_raw(listener);
            Listener::new(
                ListenerToken::new(CBLReplicator_AddChangeListener(
                    self.get_ref(),
                    Some(c_replicator_change_listener),
                    ptr.cast(),
                )),
                Box::from_raw(ptr),
            )
        };
        self.change_listeners.push(listener);
        self
    }

    /** Adds a listener that will be called when documents are replicated. */
    #[must_use]
    pub fn add_document_listener(mut self, listener: ReplicatedDocumentListener) -> Self {
        let listener = unsafe {
            let listener = Box::new(listener);
            let ptr = Box::into_raw(listener);

            Listener::new(
                ListenerToken::new(CBLReplicator_AddDocumentReplicationListener(
                    self.get_ref(),
                    Some(c_replicator_document_change_listener),
                    ptr.cast(),
                )),
                Box::from_raw(ptr),
            )
        };
        self.document_listeners.push(listener);
        self
    }
}

impl Drop for Replicator {
    fn drop(&mut self) {
        unsafe { release(self.get_ref()) }
    }
}

//======== STATUS AND PROGRESS

/** The possible states a replicator can be in during its lifecycle. */
#[derive(Debug, PartialEq, Eq)]
pub enum ReplicatorActivityLevel {
    Stopped,    // The replicator is unstarted, finished, or hit a fatal error.
    Offline,    // The replicator is offline, as the remote host is unreachable.
    Connecting, // The replicator is connecting to the remote host.
    Idle,       // The replicator is inactive, waiting for changes to sync.
    Busy,       // The replicator is actively transferring data.
}

impl From<u8> for ReplicatorActivityLevel {
    fn from(level: u8) -> Self {
        match u32::from(level) {
            kCBLReplicatorStopped => Self::Stopped,
            kCBLReplicatorOffline => Self::Offline,
            kCBLReplicatorConnecting => Self::Connecting,
            kCBLReplicatorIdle => Self::Idle,
            kCBLReplicatorBusy => Self::Busy,
            _ => unreachable!(),
        }
    }
}

/** The current progress status of a Replicator. The `fraction_complete` ranges from 0.0 to 1.0 as
replication progresses. The value is very approximate and may bounce around during replication;
making it more accurate would require slowing down the replicator and incurring more load on the
server. It's fine to use in a progress bar, though. */
#[derive(Debug)]
pub struct ReplicatorProgress {
    pub fraction_complete: f32, // Very-approximate completion, from 0.0 to 1.0
    pub document_count: u64,    // Number of documents transferred so far
}

/** A replicator's current status. */
#[derive(Debug)]
pub struct ReplicatorStatus {
    pub activity: ReplicatorActivityLevel, // Current state
    pub progress: ReplicatorProgress,      // Approximate fraction complete
    pub error: Result<()>,                 // Error, if any
}

impl From<CBLReplicatorStatus> for ReplicatorStatus {
    fn from(status: CBLReplicatorStatus) -> Self {
        Self {
            activity: status.activity.into(),
            progress: ReplicatorProgress {
                fraction_complete: status.progress.complete,
                document_count: status.progress.documentCount,
            },
            error: check_error(&status.error),
        }
    }
}

/** A callback that notifies you when the replicator's status changes. */
pub type ReplicatorChangeListener = Box<dyn Fn(ReplicatorStatus)>;
unsafe extern "C" fn c_replicator_change_listener(
    context: *mut ::std::os::raw::c_void,
    _replicator: *mut CBLReplicator,
    status: *const CBLReplicatorStatus,
) {
    let callback = context as *const ReplicatorChangeListener;
    unsafe {
        let status: ReplicatorStatus = (*status).into();
        (*callback)(status);
    }
}

/** A callback that notifies you when documents are replicated. */
pub type ReplicatedDocumentListener = Box<dyn Fn(Direction, Vec<ReplicatedDocument>) + Send + Sync>;
unsafe extern "C" fn c_replicator_document_change_listener(
    context: *mut ::std::os::raw::c_void,
    _replicator: *mut CBLReplicator,
    is_push: bool,
    num_documents: u32,
    documents: *const CBLReplicatedDocument,
) {
    let callback = context as *const ReplicatedDocumentListener;

    let direction = if is_push {
        Direction::Pushed
    } else {
        Direction::Pulled
    };

    unsafe {
        let repl_documents = std::slice::from_raw_parts(documents, num_documents as usize)
            .iter()
            .filter_map(|document| {
                document.ID.to_string().map(|doc_id| ReplicatedDocument {
                    id: doc_id,
                    flags: document.flags,
                    error: check_error(&document.error),
                    scope: document.scope.to_string(),
                    collection: document.collection.to_string(),
                })
            })
            .collect();

        (*callback)(direction, repl_documents);
    }
}

/** Flags describing a replicated document. */
pub static DELETED: u32 = kCBLDocumentFlagsDeleted;
pub static ACCESS_REMOVED: u32 = kCBLDocumentFlagsAccessRemoved;

/** Information about a document that's been pushed or pulled. */
#[derive(Debug)]
pub struct ReplicatedDocument {
    pub id: String,        // The document ID
    pub flags: u32,        // Indicates whether the document was deleted or removed
    pub error: Result<()>, // Error, if document failed to replicate
    pub scope: Option<String>,
    pub collection: Option<String>,
}

/** Direction of document transfer. */
#[derive(Debug)]
pub enum Direction {
    Pulled,
    Pushed,
}
