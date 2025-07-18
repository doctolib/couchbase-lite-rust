// Couchbase Lite database API
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

use crate::{
    CblRef, ListenerToken, release, retain,
    slice::from_str,
    error::{Result, check_bool, failure},
    c_api::{
        CBLDatabase, CBLDatabaseConfiguration, CBLDatabaseConfiguration_Default,
        CBLDatabase_AddChangeListener, CBLDatabase_BeginTransaction,
        CBLDatabase_BufferNotifications, CBLDatabase_Close, CBLDatabase_Count, CBLDatabase_Delete,
        CBLDatabase_EndTransaction, CBLDatabase_Name, CBLDatabase_Open, CBLDatabase_Path,
        CBLDatabase_PerformMaintenance, CBLDatabase_SendNotifications, CBLError,
        CBL_DatabaseExists, CBL_DeleteDatabase, FLString, kCBLMaintenanceTypeCompact,
        kCBLMaintenanceTypeFullOptimize, kCBLMaintenanceTypeIntegrityCheck,
        kCBLMaintenanceTypeOptimize, kCBLMaintenanceTypeReindex, CBL_CopyDatabase,
        CBLDatabase_ScopeNames, CBLDatabase_CollectionNames, CBLDatabase_Scope,
        CBLDatabase_Collection, CBLDatabase_CreateCollection, CBLDatabase_DeleteCollection,
        CBLDatabase_DefaultScope, CBLDatabase_DefaultCollection,
    },
    Listener, check_error, Error, CouchbaseLiteError,
    collection::Collection,
    scope::Scope,
    MutableArray,
};
#[cfg(feature = "enterprise")]
use crate::c_api::{
    CBLDatabase_ChangeEncryptionKey, CBLEncryptionKey, CBLEncryptionKey_FromPassword,
    kCBLEncryptionAES256, kCBLEncryptionNone,
};
use std::path::{Path, PathBuf};
use std::ptr;

#[cfg(feature = "enterprise")]
enum_from_primitive! {
    /// Database encryption algorithms
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum EncryptionAlgorithm {
        None = kCBLEncryptionNone as isize,
        AES256 = kCBLEncryptionAES256 as isize,
    }
}

/// Database encryption key size
pub const ENCRYPTION_KEY_SIZE_AES256: i64 = 32;

/// Encryption key specified in a DatabaseConfiguration
#[derive(Debug, Clone)]
#[cfg(feature = "enterprise")]
pub struct EncryptionKey {
    cbl_ref: Box<CBLEncryptionKey>,
}

#[cfg(feature = "enterprise")]
impl EncryptionKey {
    /// Derives an encryption key from a password. If your UI uses passwords, call this function to
    /// create the key used to encrypt the database. It is designed for security, and deliberately
    /// runs slowly to make brute-force attacks impractical.
    pub fn new_from_password(algorithm: EncryptionAlgorithm, password: &str) -> Option<Self> {
        unsafe {
            let key = CBLEncryptionKey {
                algorithm: algorithm as u32,
                bytes: [0; 32],
            };
            let encryption_key = Self {
                cbl_ref: Box::new(key),
            };

            if CBLEncryptionKey_FromPassword(
                encryption_key.get_ref() as *mut CBLEncryptionKey,
                from_str(password).get_ref(),
            ) {
                Some(encryption_key)
            } else {
                None
            }
        }
    }
}

#[cfg(feature = "enterprise")]
impl CblRef for EncryptionKey {
    type Output = *const CBLEncryptionKey;
    fn get_ref(&self) -> Self::Output {
        std::ptr::addr_of!(*self.cbl_ref)
    }
}

/// Database configuration options
#[derive(Debug, Clone)]
pub struct DatabaseConfiguration<'a> {
    pub directory: &'a std::path::Path,
    #[cfg(feature = "enterprise")]
    pub encryption_key: Option<EncryptionKey>,
}

enum_from_primitive! {
    /// Maintenance Type used when performing database maintenance
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum MaintenanceType {
        /// Compact the database file and delete unused attachments.
        Compact         = kCBLMaintenanceTypeCompact as isize,
        /// Rebuild the entire database's indexes.
        Reindex         = kCBLMaintenanceTypeReindex as isize,
        /// Check for the database’s corruption. If found, an error will be returned
        IntegrityCheck  = kCBLMaintenanceTypeIntegrityCheck as isize,
        /// Partially scan indexes to gather database statistics that help optimize queries.
        /// This operation is also performed automatically when closing the database.
        Optimize        = kCBLMaintenanceTypeOptimize as isize,
        /// Fully scan all indexes to gather database statistics that help optimize queries.
        /// This may take some time, depending on the size of the indexes, but it doesn't have to
        /// be redone unless the database changes drastically, or new indexes are created.
        FullOptimize    = kCBLMaintenanceTypeFullOptimize as isize,
    }
}

/// A database change listener callback, invoked after one or more documents are changed on disk
#[deprecated(note = "please use `CollectionChangeListener` on default collection instead")]
type DatabaseChangeListener = Box<dyn Fn(&Database, Vec<String>)>;

#[unsafe(no_mangle)]
unsafe extern "C" fn c_database_change_listener(
    context: *mut ::std::os::raw::c_void,
    db: *const CBLDatabase,
    num_docs: ::std::os::raw::c_uint,
    c_doc_ids: *mut FLString,
) {
    let callback = context as *const DatabaseChangeListener;
    let database = Database::reference(db as *mut CBLDatabase);

    unsafe {
        let doc_ids = std::slice::from_raw_parts(c_doc_ids, num_docs as usize)
            .iter()
            .filter_map(|doc_id| doc_id.to_string())
            .collect();

        (*callback)(&database, doc_ids);
    }
}

/// Callback indicating that the database (or an object belonging to it) is ready to call one or more listeners.
type BufferNotifications = fn(db: &Database);
#[unsafe(no_mangle)]
unsafe extern "C" fn c_database_buffer_notifications(
    context: *mut ::std::os::raw::c_void,
    db: *mut CBLDatabase,
) {
    unsafe {
        let callback: BufferNotifications = std::mem::transmute(context);

        let database = Database::reference(db.cast::<CBLDatabase>());

        callback(&database);
    }
}

/// A connection to an open database
#[derive(Debug, PartialEq, Eq)]
pub struct Database {
    cbl_ref: *mut CBLDatabase,
}

impl CblRef for Database {
    type Output = *mut CBLDatabase;
    fn get_ref(&self) -> Self::Output {
        self.cbl_ref
    }
}

impl Database {
    //////// CONSTRUCTORS:

    /// Increase the reference counter of the CBL ref, so dropping the instance will NOT free the ref.
    pub(crate) fn reference(cbl_ref: *mut CBLDatabase) -> Self {
        Self {
            cbl_ref: unsafe { retain(cbl_ref) },
        }
    }

    /// Takes ownership of the CBL ref, the reference counter is not increased so dropping the instance will free the ref.
    pub(crate) const fn take_ownership(cbl_ref: *mut CBLDatabase) -> Self {
        Self { cbl_ref }
    }

    /// Opens a database, or creates it if it doesn't exist yet, returning a new `Database`
    /// instance.
    /// It's OK to open the same database file multiple times. Each `Database` instance is
    /// independent of the others (and must be separately closed and released.)
    pub fn open(name: &str, config: Option<DatabaseConfiguration>) -> Result<Self> {
        unsafe {
            if let Some(cfg) = config {
                let mut c_config: CBLDatabaseConfiguration = CBLDatabaseConfiguration_Default();
                c_config.directory = from_str(cfg.directory.to_str().unwrap()).get_ref();
                #[cfg(feature = "enterprise")]
                if let Some(encryption_key) = cfg.encryption_key {
                    c_config.encryptionKey = *encryption_key.get_ref();
                }
                return Self::_open(name, &c_config);
            }
            Self::_open(name, ptr::null())
        }
    }

    unsafe fn _open(name: &str, config_ptr: *const CBLDatabaseConfiguration) -> Result<Self> {
        let mut err = CBLError::default();
        unsafe {
            let db_ref = CBLDatabase_Open(from_str(name).get_ref(), config_ptr, &mut err);
            if db_ref.is_null() {
                return failure(err);
            }
            Ok(Self::take_ownership(db_ref))
        }
    }

    //////// OTHER STATIC METHODS:

    /// Returns true if a database with the given name exists in the given directory.
    pub fn exists<P: AsRef<Path>>(name: &str, in_directory: P) -> bool {
        unsafe {
            CBL_DatabaseExists(
                from_str(name).get_ref(),
                from_str(in_directory.as_ref().to_str().unwrap()).get_ref(),
            )
        }
    }

    /// Copies a database file to a new location, and assigns it a new internal UUID to distinguish
    /// it from the original database when replicating.
    pub fn copy_file<P: AsRef<Path>>(
        current_db_full_path: P,
        new_db_name: &str,
        config: Option<DatabaseConfiguration>,
    ) -> Result<()> {
        let config = config
            .map(|cfg| {
                let mut c_config: CBLDatabaseConfiguration =
                    unsafe { CBLDatabaseConfiguration_Default() };

                c_config.directory = from_str(
                    cfg.directory
                        .to_str()
                        .ok_or(Error::cbl_error(CouchbaseLiteError::InvalidParameter))?,
                )
                .get_ref();

                #[cfg(feature = "enterprise")]
                if let Some(encryption_key) = cfg.encryption_key {
                    c_config.encryptionKey = unsafe { *encryption_key.get_ref() };
                }
                Ok(c_config)
            })
            .transpose()?;

        let config_param = config
            .as_ref()
            .map(|cfg| cfg as *const CBLDatabaseConfiguration)
            .unwrap_or(ptr::null());

        let mut error = CBLError::default();
        unsafe {
            CBL_CopyDatabase(
                from_str(
                    current_db_full_path
                        .as_ref()
                        .to_str()
                        .ok_or(Error::cbl_error(CouchbaseLiteError::InvalidParameter))?,
                )
                .get_ref(),
                from_str(new_db_name).get_ref(),
                config_param,
                &mut error,
            );
        }

        check_error(&error)
    }

    /// Deletes a database file. If the database file is open, an error is returned.
    pub fn delete_file<P: AsRef<Path>>(name: &str, in_directory: P) -> Result<bool> {
        unsafe {
            let mut error = CBLError::default();
            if CBL_DeleteDatabase(
                from_str(name).get_ref(),
                from_str(in_directory.as_ref().to_str().unwrap()).get_ref(),
                &mut error,
            ) {
                Ok(true)
            } else if !error {
                Ok(false)
            } else {
                failure(error)
            }
        }
    }

    //////// OPERATIONS:

    /// Closes an open database.
    pub fn close(self) -> Result<()> {
        unsafe { check_bool(|error| CBLDatabase_Close(self.get_ref(), error)) }
    }

    /// Closes and deletes a database. If there are any other connections to the database,
    /// error is returned.
    pub fn delete(self) -> Result<()> {
        unsafe { check_bool(|error| CBLDatabase_Delete(self.get_ref(), error)) }
    }

    /// Runs a maintenance operation on the database.
    pub fn perform_maintenance(&mut self, of_type: MaintenanceType) -> Result<()> {
        unsafe {
            check_bool(|error| {
                CBLDatabase_PerformMaintenance(self.get_ref(), of_type as u32, error)
            })
        }
    }

    /// Invokes the callback within a database transaction
    ///   - Multiple writes are _much_ faster when grouped in a transaction.
    ///   - Changes will not be visible to other Database instances on the same database until
    ///     the transaction ends.
    ///   - Transactions can nest. Changes are not committed until the outer one ends.
    pub fn in_transaction<T, F>(&mut self, mut callback: F) -> Result<T>
    where
        F: FnMut(&mut Self) -> Result<T>,
    {
        let mut err = CBLError::default();
        unsafe {
            if !CBLDatabase_BeginTransaction(self.get_ref(), &mut err) {
                return failure(err);
            }
        }
        let result = callback(self);
        unsafe {
            if !CBLDatabase_EndTransaction(self.get_ref(), result.is_ok(), &mut err) {
                return failure(err);
            }
        }
        result
    }

    /// Creates a new transaction that can be committed or rolled back manually.
    /// Returns a Transaction object that must be committed explicitly, otherwise
    /// it will be rolled back when dropped.
    ///
    /// For simpler cases, consider using `in_transaction()` instead.
    pub fn begin_transaction(&mut self) -> Result<Transaction> {
        Transaction::new(self.get_ref())
    }

    /// Encrypts or decrypts a database, or changes its encryption key.
    #[cfg(feature = "enterprise")]
    pub fn change_encryption_key(&mut self, encryption_key: &EncryptionKey) -> Result<()> {
        unsafe {
            check_bool(|error| {
                CBLDatabase_ChangeEncryptionKey(self.get_ref(), encryption_key.get_ref(), error)
            })
        }
    }

    //////// ACCESSORS:

    /// Returns the database's name.
    pub fn name(&self) -> &str {
        unsafe { CBLDatabase_Name(self.get_ref()).as_str().unwrap() }
    }

    /// Returns the database's full filesystem path.
    pub fn path(&self) -> PathBuf {
        unsafe { PathBuf::from(CBLDatabase_Path(self.get_ref()).to_string().unwrap()) }
    }

    /// Returns the number of documents in the database.
    #[deprecated(note = "please use `count` on the default collection instead")]
    pub fn count(&self) -> u64 {
        unsafe { CBLDatabase_Count(self.get_ref()) }
    }

    /// Returns the names of all existing scopes in the database.
    ///   - the default scope (_default) always exists.
    ///   - other scopes exist when it contains at least one collection
    pub fn scope_names(&self) -> Result<Vec<String>> {
        let mut error = CBLError::default();
        let array = unsafe { CBLDatabase_ScopeNames(self.get_ref(), &mut error) };

        check_error(&error).map(|()| unsafe {
            MutableArray::adopt(array)
                .iter()
                .map(|v| v.as_string().unwrap_or("").to_string())
                .collect()
        })
    }

    /// Returns the names of all collections in the scope.
    pub fn collection_names(&self, scope_name: String) -> Result<Vec<String>> {
        let scope_name = from_str(&scope_name);
        let mut error = CBLError::default();
        let array = unsafe {
            CBLDatabase_CollectionNames(self.get_ref(), scope_name.get_ref(), &mut error)
        };

        check_error(&error).map(|()| unsafe {
            MutableArray::adopt(array)
                .iter()
                .map(|v| v.as_string().unwrap_or("").to_string())
                .collect()
        })
    }

    /// Returns an existing scope with the given name.
    ///   - the default scope (_default) always exists.
    ///   - other scopes exist when it contains at least one collection
    pub fn scope(&self, scope_name: String) -> Result<Option<Scope>> {
        let scope_name = from_str(&scope_name);
        let mut error = CBLError::default();
        let scope = unsafe { CBLDatabase_Scope(self.get_ref(), scope_name.get_ref(), &mut error) };

        check_error(&error).map(|()| {
            if scope.is_null() {
                None
            } else {
                Some(Scope::take_ownership(scope))
            }
        })
    }

    /// Returns the existing collection with the given name and scope.
    pub fn collection(
        &self,
        collection_name: String,
        scope_name: String,
    ) -> Result<Option<Collection>> {
        let collection_name = from_str(&collection_name);
        let scope_name = from_str(&scope_name);
        let mut error = CBLError::default();
        let collection = unsafe {
            CBLDatabase_Collection(
                self.get_ref(),
                collection_name.get_ref(),
                scope_name.get_ref(),
                &mut error,
            )
        };

        check_error(&error).map(|()| {
            if collection.is_null() {
                None
            } else {
                Some(Collection::take_ownership(collection))
            }
        })
    }

    /// Create a new collection.
    /// The naming rules of the collections and scopes are as follows:
    ///     - Must be between 1 and 251 characters in length.
    ///     - Can only contain the characters A-Z, a-z, 0-9, and the symbols _, -, and %.
    ///     - Cannot start with _ or %.
    ///     - Both scope and collection names are case sensitive.
    /// If the collection already exists, the existing collection will be returned.
    pub fn create_collection(
        &self,
        collection_name: String,
        scope_name: String,
    ) -> Result<Collection> {
        let collection_name = from_str(&collection_name);
        let scope_name = from_str(&scope_name);
        let mut error = CBLError::default();
        let collection = unsafe {
            CBLDatabase_CreateCollection(
                self.get_ref(),
                collection_name.get_ref(),
                scope_name.get_ref(),
                &mut error,
            )
        };

        check_error(&error).map(|()| Collection::take_ownership(collection))
    }

    /// Delete an existing collection.
    /// The default collection cannot be deleted.
    pub fn delete_collection(&self, collection_name: String, scope_name: String) -> Result<()> {
        let collection_name = from_str(&collection_name);
        let scope_name = from_str(&scope_name);
        unsafe {
            check_bool(|error| {
                CBLDatabase_DeleteCollection(
                    self.get_ref(),
                    collection_name.get_ref(),
                    scope_name.get_ref(),
                    error,
                )
            })
        }
    }

    /// Returns the default scope.
    pub fn default_scope(&self) -> Result<Scope> {
        let mut error = CBLError::default();
        let scope = unsafe { CBLDatabase_DefaultScope(self.get_ref(), &mut error) };

        check_error(&error).map(|()| Scope::take_ownership(scope))
    }

    /// Returns the default collection.
    pub fn default_collection(&self) -> Result<Option<Collection>> {
        let mut error = CBLError::default();
        let collection = unsafe { CBLDatabase_DefaultCollection(self.get_ref(), &mut error) };

        check_error(&error).map(|()| {
            if collection.is_null() {
                None
            } else {
                Some(Collection::take_ownership(collection))
            }
        })
    }

    /// Returns the default collection.
    pub fn default_collection_or_error(&self) -> Result<Collection> {
        let mut error = CBLError::default();
        let collection = unsafe { CBLDatabase_DefaultCollection(self.get_ref(), &mut error) };

        check_error(&error)?;

        if collection.is_null() {
            Err(Error::cbl_error(CouchbaseLiteError::NotFound))
        } else {
            Ok(Collection::take_ownership(collection))
        }
    }

    //////// NOTIFICATIONS:

    /// Registers a database change listener function. It will be called after one or more
    /// documents are changed on disk. Remember to keep the reference to the ChangeListener
    /// if you want the callback to keep working.
    ///
    /// # Lifetime
    ///
    /// The listener is deleted at the end of life of the `Listener` object.
    /// You must keep the `Listener` object alive as long as you need it.
    #[must_use]
    #[deprecated(note = "please use `add_listener` on default collection instead")]
    pub fn add_listener(
        &mut self,
        listener: DatabaseChangeListener,
    ) -> Listener<DatabaseChangeListener> {
        unsafe {
            let listener = Box::new(listener);
            let ptr = Box::into_raw(listener);

            Listener::new(
                ListenerToken {
                    cbl_ref: CBLDatabase_AddChangeListener(
                        self.cbl_ref,
                        Some(c_database_change_listener),
                        ptr.cast(),
                    ),
                },
                Box::from_raw(ptr),
            )
        }
    }

    /// Switches the database to buffered-notification mode. Notifications for objects belonging
    /// to this database (documents, queries, replicators, and of course the database) will not be
    /// called immediately; your callback function will be called instead. You can then call
    /// `send_notifications` when you're ready.
    pub fn buffer_notifications(&self, callback: BufferNotifications) {
        unsafe {
            let callback = callback as *mut std::ffi::c_void;

            CBLDatabase_BufferNotifications(
                self.get_ref(),
                Some(c_database_buffer_notifications),
                callback,
            );
        }
    }

    /// Immediately issues all pending notifications for this database, by calling their listener
    /// callbacks. (Only useful after `buffer_notifications` has been called.) */
    pub fn send_notifications(&self) {
        unsafe {
            CBLDatabase_SendNotifications(self.get_ref());
        }
    }
}

/// A database transaction that can be committed or rolled back.
/// When dropped without being committed, the transaction is automatically rolled back.
pub struct Transaction {
    db_ref: *mut CBLDatabase,
    committed: bool,
}

impl Transaction {
    fn new(db_ref: *mut CBLDatabase) -> Result<Self> {
        unsafe {
            let mut err = CBLError::default();
            if !CBLDatabase_BeginTransaction(db_ref, &mut err) {
                return failure(err);
            }
        }
        Ok(Transaction {
            db_ref,
            committed: false,
        })
    }

    /// Commits the transaction, making all changes permanent.
    pub fn commit(mut self) -> Result<()> {
        unsafe {
            let mut err = CBLError::default();
            if !CBLDatabase_EndTransaction(self.db_ref, true, &mut err) {
                return failure(err);
            }
        }
        self.committed = true;
        Ok(())
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        if !self.committed {
            unsafe {
                let mut err = CBLError::default();
                let _ = CBLDatabase_EndTransaction(self.db_ref, false, &mut err);
            }
        }
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        unsafe { release(self.get_ref()) }
    }
}

impl Clone for Database {
    fn clone(&self) -> Self {
        Self::reference(self.get_ref())
    }
}
