// Couchbase Lite document API
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
    c_api::{
        CBLDatabase, CBLDatabase_AddDocumentChangeListener, CBLDatabase_DeleteDocument,
        CBLDatabase_DeleteDocumentWithConcurrencyControl, CBLDatabase_GetDocumentExpiration,
        CBLDatabase_GetMutableDocument, CBLDatabase_PurgeDocument, CBLDatabase_PurgeDocumentByID,
        CBLDatabase_SaveDocument, CBLDatabase_SaveDocumentWithConcurrencyControl,
        CBLDatabase_SaveDocumentWithConflictHandler, CBLDatabase_SetDocumentExpiration,
        CBLDocument, CBLDocument_Create, CBLDocument_CreateJSON, CBLDocument_CreateWithID,
        CBLDocument_ID, CBLDocument_MutableProperties, CBLDocument_Properties,
        CBLDocument_RevisionID, CBLDocument_Sequence, CBLDocument_SetJSON,
        CBLDocument_SetProperties, CBLError, FLString, kCBLConcurrencyControlFailOnConflict,
        kCBLConcurrencyControlLastWriteWins, CBLDocumentChange, CBLCollection,
        CBLCollection_GetMutableDocument, CBLCollection_SaveDocument,
        CBLCollection_SaveDocumentWithConcurrencyControl,
        CBLCollection_SaveDocumentWithConflictHandler, CBLCollection_DeleteDocument,
        CBLCollection_DeleteDocumentWithConcurrencyControl, CBLCollection_PurgeDocument,
        CBLCollection_PurgeDocumentByID, CBLCollection_GetDocumentExpiration,
        CBLCollection_SetDocumentExpiration, CBLCollection_AddDocumentChangeListener,
    },
    slice::from_str,
    CblRef, CouchbaseLiteError, Database, Dict, Error, ListenerToken, MutableDict, Result,
    Timestamp, check_bool, check_failure, failure, release, retain, Listener,
    collection::Collection,
};

/// An in-memory copy of a document.
#[derive(Debug)]
pub struct Document {
    cbl_ref: *mut CBLDocument,
}

impl CblRef for Document {
    type Output = *mut CBLDocument;
    fn get_ref(&self) -> Self::Output {
        self.cbl_ref
    }
}

/// Conflict-handling options when saving or deleting a document.
pub enum ConcurrencyControl {
    /// The current save/delete will overwrite a conflicting revision if there is a conflict.
    LastWriteWins = kCBLConcurrencyControlLastWriteWins as isize,
    /// The current save/delete will fail if there is a conflict.
    FailOnConflict = kCBLConcurrencyControlFailOnConflict as isize,
}

/// Custom conflict handler for use when saving or deleting a document. This handler is called
/// if the save would cause a conflict, i.e. if the document in the database has been updated
/// (probably by a pull replicator, or by application code on another thread)
/// since it was loaded into the CBLDocument being saved.
type ConflictHandler = fn(&mut Document, &Document) -> bool;
#[unsafe(no_mangle)]
unsafe extern "C" fn c_conflict_handler(
    context: *mut ::std::os::raw::c_void,
    document_being_saved: *mut CBLDocument,
    conflicting_document: *const CBLDocument,
) -> bool {
    unsafe {
        let callback: ConflictHandler = std::mem::transmute(context);

        callback(
            &mut Document::reference(document_being_saved),
            &Document::reference(conflicting_document as *mut CBLDocument),
        )
    }
}

///  A document change listener lets you detect changes made to a specific document after they
/// are persisted to the database.
#[deprecated(note = "please use `CollectionDocumentChangeListener` instead")]
type DatabaseDocumentChangeListener = Box<dyn Fn(&Database, Option<String>)>;

#[unsafe(no_mangle)]
unsafe extern "C" fn c_database_document_change_listener(
    context: *mut ::std::os::raw::c_void,
    db: *const CBLDatabase,
    c_doc_id: FLString,
) {
    unsafe {
        let callback = context as *const DatabaseDocumentChangeListener;
        let database = Database::reference(db as *mut CBLDatabase);
        (*callback)(&database, c_doc_id.to_string());
    }
}

//////// DATABASE'S DOCUMENT API:

impl Database {
    /// Reads a document from the database. Each call to this function returns a new object
    /// containing the document's current state.
    #[deprecated(note = "please use `get_document` on default collection instead")]
    pub fn get_document(&self, id: &str) -> Result<Document> {
        unsafe {
            // we always get a mutable CBLDocument,
            // since Rust doesn't let us have MutableDocument subclass.
            let mut error = CBLError::default();
            let doc =
                CBLDatabase_GetMutableDocument(self.get_ref(), from_str(id).get_ref(), &mut error);
            if doc.is_null() {
                return if error.code == 0 {
                    Err(Error::cbl_error(CouchbaseLiteError::NotFound))
                } else {
                    failure(error)
                };
            }
            Ok(Document::take_ownership(doc))
        }
    }

    /// Saves a new or modified document to the database.
    /// If a newer revision has been saved since \p doc was loaded, it will be overwritten by
    /// this one. This can lead to data loss! To avoid this, call
    /// `save_document_with_concurency_control` or `save_document_resolving` instead.
    #[deprecated(note = "please use `save_document` on default collection instead")]
    pub fn save_document(&mut self, doc: &mut Document) -> Result<()> {
        unsafe {
            check_bool(|error| CBLDatabase_SaveDocument(self.get_ref(), doc.get_ref(), error))
        }
    }

    /// Saves a new or modified document to the database.
    /// If a conflicting revision has been saved since `doc` was loaded, the `concurrency`
    /// parameter specifies whether the save should fail, or the conflicting revision should
    /// be overwritten with the revision being saved.
    /// If you need finer-grained control, call `save_document_resolving` instead.
    #[deprecated(
        note = "please use `save_document_with_concurrency_control` on default collection instead"
    )]
    pub fn save_document_with_concurency_control(
        &mut self,
        doc: &mut Document,
        concurrency: ConcurrencyControl,
    ) -> Result<()> {
        let c_concurrency = concurrency as u8;
        unsafe {
            check_bool(|error| {
                CBLDatabase_SaveDocumentWithConcurrencyControl(
                    self.get_ref(),
                    doc.get_ref(),
                    c_concurrency,
                    error,
                )
            })
        }
    }

    /// Saves a new or modified document to the database. This function is the same as
    /// `save_document`, except that it allows for custom conflict handling in the event
    /// that the document has been updated since `doc` was loaded.
    #[deprecated(note = "please use `save_document_resolving` on default collection instead")]
    pub fn save_document_resolving(
        &mut self,
        doc: &mut Document,
        conflict_handler: ConflictHandler,
    ) -> Result<Document> {
        unsafe {
            let callback = conflict_handler as *mut std::ffi::c_void;
            match check_bool(|error| {
                CBLDatabase_SaveDocumentWithConflictHandler(
                    self.get_ref(),
                    doc.get_ref(),
                    Some(c_conflict_handler),
                    callback,
                    error,
                )
            }) {
                Ok(_) => Ok(doc.clone()),
                Err(err) => Err(err),
            }
        }
    }

    /// Deletes a document from the database. Deletions are replicated.
    #[deprecated(note = "please use `delete_document` on default collection instead")]
    pub fn delete_document(&mut self, doc: &Document) -> Result<()> {
        unsafe {
            check_bool(|error| CBLDatabase_DeleteDocument(self.get_ref(), doc.get_ref(), error))
        }
    }

    /// Deletes a document from the database. Deletions are replicated.
    #[deprecated(
        note = "please use `delete_document_with_concurrency_control` on default collection instead"
    )]
    pub fn delete_document_with_concurency_control(
        &mut self,
        doc: &Document,
        concurrency: ConcurrencyControl,
    ) -> Result<()> {
        let c_concurrency = concurrency as u8;
        unsafe {
            check_bool(|error| {
                CBLDatabase_DeleteDocumentWithConcurrencyControl(
                    self.get_ref(),
                    doc.get_ref(),
                    c_concurrency,
                    error,
                )
            })
        }
    }

    /// Purges a document. This removes all traces of the document from the database.
    /// Purges are _not_ replicated. If the document is changed on a server, it will be re-created.
    #[deprecated(note = "please use `purge_document` on default collection instead")]
    pub fn purge_document(&mut self, doc: &Document) -> Result<()> {
        unsafe {
            check_bool(|error| CBLDatabase_PurgeDocument(self.get_ref(), doc.get_ref(), error))
        }
    }

    /// Purges a document, given only its ID.
    #[deprecated(note = "please use `purge_document_by_id` on default collection instead")]
    pub fn purge_document_by_id(&mut self, id: &str) -> Result<()> {
        unsafe {
            check_bool(|error| {
                CBLDatabase_PurgeDocumentByID(self.get_ref(), from_str(id).get_ref(), error)
            })
        }
    }

    /// Returns the time, if any, at which a given document will expire and be purged in milliseconds since the Unix epoch (1/1/1970.).
    /// Documents don't normally expire; you have to call `set_document_expiration`
    /// to set a document's expiration time.
    #[deprecated(note = "please use `document_expiration` on default collection instead")]
    pub fn document_expiration(&self, doc_id: &str) -> Result<Option<Timestamp>> {
        unsafe {
            let mut error = CBLError::default();
            let exp = CBLDatabase_GetDocumentExpiration(
                self.get_ref(),
                from_str(doc_id).get_ref(),
                &mut error,
            );
            match exp {
                0 => Ok(None),
                _ if exp > 0 => Ok(Some(Timestamp::new(exp))),
                _ => failure(error),
            }
        }
    }

    /// Sets or clears the expiration time of a document.
    #[deprecated(note = "please use `set_document_expiration` on default collection instead")]
    pub fn set_document_expiration(&mut self, doc_id: &str, when: Option<Timestamp>) -> Result<()> {
        let exp: i64 = match when {
            Some(Timestamp { timestamp }) => timestamp,
            _ => 0,
        };
        unsafe {
            check_bool(|error| {
                CBLDatabase_SetDocumentExpiration(
                    self.get_ref(),
                    from_str(doc_id).get_ref(),
                    exp,
                    error,
                )
            })
        }
    }

    /// Registers a document change listener callback. It will be called after a specific document
    /// is changed on disk.
    ///
    ///
    /// # Lifetime
    ///
    /// The listener is deleted at the end of life of the Listener object.
    /// You must keep the Listener object alive as long as you need it
    #[must_use]
    #[deprecated(note = "please use `add_document_change_listener` on default collection instead")]
    pub fn add_document_change_listener(
        &self,
        document: &Document,
        listener: DatabaseDocumentChangeListener,
    ) -> Listener<DatabaseDocumentChangeListener> {
        unsafe {
            let listener = Box::new(listener);
            let ptr = Box::into_raw(listener);
            Listener::new(
                ListenerToken::new(CBLDatabase_AddDocumentChangeListener(
                    self.get_ref(),
                    CBLDocument_ID(document.get_ref()),
                    Some(c_database_document_change_listener),
                    ptr.cast(),
                )),
                Box::from_raw(ptr),
            )
        }
    }
}

//////// COLLECTION'S DOCUMENT API:

/// A document change listener lets you detect changes made to a specific document after they
/// are persisted to the collection.
type CollectionDocumentChangeListener = Box<dyn Fn(Collection, Option<String>)>;

#[unsafe(no_mangle)]
unsafe extern "C" fn c_collection_document_change_listener(
    context: *mut ::std::os::raw::c_void,
    change: *const CBLDocumentChange,
) {
    let callback = context as *const CollectionDocumentChangeListener;
    unsafe {
        if let Some(change) = change.as_ref() {
            let collection = Collection::reference(change.collection as *mut CBLCollection);
            (*callback)(collection, change.docID.to_string());
        }
    }
}

impl Collection {
    /// Reads a document from the collection, returning a new Document object.
    /// Each call to this function creates a new object (which must later be released.).
    pub fn get_document(&self, id: &str) -> Result<Document> {
        unsafe {
            // we always get a mutable CBLDocument,
            // since Rust doesn't let us have MutableDocument subclass.
            let mut error = CBLError::default();
            let doc = CBLCollection_GetMutableDocument(
                self.get_ref(),
                from_str(id).get_ref(),
                &mut error,
            );
            if doc.is_null() {
                return if error.code == 0 {
                    Err(Error::cbl_error(CouchbaseLiteError::NotFound))
                } else {
                    failure(error)
                };
            }
            Ok(Document::take_ownership(doc))
        }
    }

    /// Saves a document to the collection.
    pub fn save_document(&mut self, doc: &mut Document) -> Result<()> {
        unsafe {
            check_bool(|error| CBLCollection_SaveDocument(self.get_ref(), doc.get_ref(), error))
        }
    }

    /// Saves a document to the collection.
    /// If a conflicting revision has been saved since the document was loaded, the concurrency
    /// parameter specifies whether the save should fail, or the conflicting revision should
    /// be overwritten with the revision being saved.
    /// If you need finer-grained control, call save_document_resolving instead.
    pub fn save_document_with_concurency_control(
        &mut self,
        doc: &mut Document,
        concurrency: ConcurrencyControl,
    ) -> Result<()> {
        let c_concurrency = concurrency as u8;
        unsafe {
            check_bool(|error| {
                CBLCollection_SaveDocumentWithConcurrencyControl(
                    self.get_ref(),
                    doc.get_ref(),
                    c_concurrency,
                    error,
                )
            })
        }
    }

    /// Saves a document to the collection, allowing for custom conflict handling in the event
    /// that the document has been updated since \p doc was loaded.
    pub fn save_document_resolving(
        &mut self,
        doc: &mut Document,
        conflict_handler: ConflictHandler,
    ) -> Result<Document> {
        unsafe {
            let callback = conflict_handler as *mut std::ffi::c_void;
            match check_bool(|error| {
                CBLCollection_SaveDocumentWithConflictHandler(
                    self.get_ref(),
                    doc.get_ref(),
                    Some(c_conflict_handler),
                    callback,
                    error,
                )
            }) {
                Ok(_) => Ok(doc.clone()),
                Err(err) => Err(err),
            }
        }
    }

    /// Deletes a document from the collection. Deletions are replicated.
    pub fn delete_document(&mut self, doc: &Document) -> Result<()> {
        unsafe {
            check_bool(|error| CBLCollection_DeleteDocument(self.get_ref(), doc.get_ref(), error))
        }
    }

    /// Deletes a document from the collection. Deletions are replicated.
    pub fn delete_document_with_concurency_control(
        &mut self,
        doc: &Document,
        concurrency: ConcurrencyControl,
    ) -> Result<()> {
        let c_concurrency = concurrency as u8;
        unsafe {
            check_bool(|error| {
                CBLCollection_DeleteDocumentWithConcurrencyControl(
                    self.get_ref(),
                    doc.get_ref(),
                    c_concurrency,
                    error,
                )
            })
        }
    }

    /// Purges a document. This removes all traces of the document from the collection.
    /// Purges are _not_ replicated. If the document is changed on a server, it will be re-created
    /// when pulled.
    pub fn purge_document(&mut self, doc: &Document) -> Result<()> {
        unsafe {
            check_bool(|error| CBLCollection_PurgeDocument(self.get_ref(), doc.get_ref(), error))
        }
    }

    /// Purges a document, given only its ID.
    pub fn purge_document_by_id(&mut self, id: &str) -> Result<()> {
        unsafe {
            check_bool(|error| {
                CBLCollection_PurgeDocumentByID(self.get_ref(), from_str(id).get_ref(), error)
            })
        }
    }

    /// Returns the time, if any, at which a given document will expire and be purged in milliseconds since the Unix epoch (1/1/1970.).
    /// Documents don't normally expire; you have to call set_document_expiration
    /// to set a document's expiration time.
    pub fn document_expiration(&self, doc_id: &str) -> Result<Option<Timestamp>> {
        unsafe {
            let mut error = CBLError::default();
            let exp = CBLCollection_GetDocumentExpiration(
                self.get_ref(),
                from_str(doc_id).get_ref(),
                &mut error,
            );
            match exp {
                0 => Ok(None),
                _ if exp > 0 => Ok(Some(Timestamp::new(exp))),
                _ => failure(error),
            }
        }
    }

    /// Sets or clears the expiration time of a document in milliseconds since the Unix epoch (1/1/1970.).
    pub fn set_document_expiration(&mut self, doc_id: &str, when: Option<Timestamp>) -> Result<()> {
        let exp: i64 = match when {
            Some(Timestamp { timestamp }) => timestamp,
            _ => 0,
        };
        unsafe {
            check_bool(|error| {
                CBLCollection_SetDocumentExpiration(
                    self.get_ref(),
                    from_str(doc_id).get_ref(),
                    exp,
                    error,
                )
            })
        }
    }

    /// Registers a document change listener callback. It will be called after a specific document is changed on disk.
    pub fn add_document_change_listener(
        &self,
        document: &Document,
        listener: CollectionDocumentChangeListener,
    ) -> Listener<CollectionDocumentChangeListener> {
        unsafe {
            let listener = Box::new(listener);
            let ptr = Box::into_raw(listener);
            Listener::new(
                ListenerToken::new(CBLCollection_AddDocumentChangeListener(
                    self.get_ref(),
                    CBLDocument_ID(document.get_ref()),
                    Some(c_collection_document_change_listener),
                    ptr.cast(),
                )),
                Box::from_raw(ptr),
            )
        }
    }
}

//////// DOCUMENT API:

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    //////// CONSTRUCTORS:

    /// Creates a new, empty document in memory, with an automatically generated unique ID.
    /// It will not be added to a database until saved.
    pub fn new() -> Self {
        unsafe { Self::take_ownership(CBLDocument_Create()) }
    }

    /// Creates a new, empty document in memory, with the given ID.
    /// It will not be added to a database until saved.
    pub fn new_with_id(id: &str) -> Self {
        unsafe { Self::take_ownership(CBLDocument_CreateWithID(from_str(id).get_ref())) }
    }

    /// Increase the reference counter of the CBL ref, so dropping the instance will NOT free the ref.
    pub(crate) fn reference(cbl_ref: *mut CBLDocument) -> Self {
        unsafe {
            Self {
                cbl_ref: retain(cbl_ref),
            }
        }
    }

    /// Takes ownership of the CBL ref, the reference counter is not increased so dropping the instance will free the ref.
    pub(crate) const fn take_ownership(cbl_ref: *mut CBLDocument) -> Self {
        Self { cbl_ref }
    }

    ////////

    /// Returns the document's ID.
    pub fn id(&self) -> &str {
        unsafe { CBLDocument_ID(self.get_ref()).as_str().unwrap() }
    }

    /// Returns a document's revision ID, which is a short opaque string that's guaranteed to be
    /// unique to every change made to the document.
    /// If the document has not been saved yet, this method returns None.
    pub fn revision_id(&self) -> Option<&str> {
        unsafe { CBLDocument_RevisionID(self.get_ref()).as_str() }
    }

    /// Returns a document's current sequence in the local database.
    /// This number increases every time the document is saved, and a more recently saved document
    /// will have a greater sequence number than one saved earlier, so sequences may be used as an
    /// abstract 'clock' to tell relative modification times. */
    pub fn sequence(&self) -> u64 {
        unsafe { CBLDocument_Sequence(self.get_ref()) }
    }

    /// Returns true if a document is deleted.
    pub fn is_deleted(&self) -> bool {
        self.properties().empty()
    }

    /// Returns a document's properties as a dictionary.
    /// They cannot be mutated; call `mutable_properties` if you want to make
    /// changes to the document.
    pub fn properties(&self) -> Dict {
        unsafe { Dict::wrap(CBLDocument_Properties(self.get_ref()), self) }
    }

    /// Returns a document's properties as an mutable dictionary. Any changes made to this
    /// dictionary will be saved to the database when this Document instance is saved.
    pub fn mutable_properties(&mut self) -> MutableDict {
        unsafe { MutableDict::adopt(CBLDocument_MutableProperties(self.get_ref())) }
    }

    /// Replaces a document's properties with the contents of the dictionary.
    /// The dictionary is retained, not copied, so further changes _will_ affect the document.
    pub fn set_properties(&mut self, properties: &MutableDict) {
        unsafe { CBLDocument_SetProperties(self.get_ref(), properties.get_ref()) }
    }

    /// Returns a document's properties as a JSON string.
    pub fn properties_as_json(&self) -> String {
        unsafe { CBLDocument_CreateJSON(self.get_ref()).to_string().unwrap() }
    }

    /// Sets a mutable document's properties from a JSON string.
    pub fn set_properties_as_json(&mut self, json: &str) -> Result<()> {
        unsafe {
            let mut err = CBLError::default();
            let ok = CBLDocument_SetJSON(self.get_ref(), from_str(json).get_ref(), &mut err);
            check_failure(ok, &err)
        }
    }
}

impl Drop for Document {
    fn drop(&mut self) {
        unsafe { release(self.get_ref()) }
    }
}

impl Clone for Document {
    fn clone(&self) -> Self {
        Self::reference(self.get_ref())
    }
}
