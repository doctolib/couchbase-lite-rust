use crate::{
    CblRef, Listener, ListenerToken, release, retain,
    c_api::{
        CBLCollection, CBLCollectionChange, CBLCollection_AddChangeListener, CBLCollection_Scope,
        CBLCollection_Name, CBLCollection_Count, CBLCollection_FullName, CBLCollection_Database,
    },
    scope::Scope,
    Database,
};

pub static DEFAULT_NAME: &str = "_default";

/// A Collection represents a collection which is a container for documents.
///
/// A collection can be thought as a table in a relational database. Each collection belongs to
/// a scope which is simply a namespce, and has a name which is unique within its scope.
///
/// When a new database is created, a default collection named "_default" will be automatically
/// created. The default collection is created under the default scope named "_default".
/// The name of the default collection and scope can be referenced by using
/// kCBLDefaultCollectionName and \ref kCBLDefaultScopeName constant.
///
///    @note The default collection cannot be deleted.
///
///    When creating a new collection, the collection name, and the scope name are required.
///    The naming rules of the collections and scopes are as follows:
///    - Must be between 1 and 251 characters in length.
///    - Can only contain the characters A-Z, a-z, 0-9, and the symbols _, -, and %.
///    - Cannot start with _ or %.
///    - Both scope and collection names are case sensitive.
///
///    ## `CBLCollection` Lifespan
///    `CBLCollection` is ref-counted. Same as the CBLDocument, the CBLCollection objects
///    created or retrieved from the database must be released after you are done using them.
///    When the database is closed or released, the collection objects will become invalid,
///    most operations on the invalid \ref CBLCollection object will fail with either the
///    \ref kCBLErrorNotOpen error or null/zero/empty result.
///
///    ##Legacy Database and API
///    When using the legacy database, the existing documents and indexes in the database will be
///    automatically migrated to the default collection.
///
///    Any pre-existing database functions that refer to documents, listeners, and indexes without
///    specifying a collection such as \ref CBLDatabase_GetDocument will implicitly operate on
///    the default collection. In other words, they behave exactly the way they used to, but
///    collection-aware code should avoid them and use the new Collection API instead.
///    These legacy functions are deprecated and will be removed eventually.
#[derive(Debug, PartialEq, Eq)]
pub struct Collection {
    cbl_ref: *mut CBLCollection,
}

impl Collection {
    pub const DEFAULT_NAME: &str = "_default";
    pub const DEFAULT_FULLE_NAME: &str = "_default._default";

    //////// CONSTRUCTORS:

    /// Increase the reference counter of the CBL ref, so dropping the instance will NOT free the ref.
    pub(crate) fn reference(cbl_ref: *mut CBLCollection) -> Self {
        Self {
            cbl_ref: unsafe { retain(cbl_ref) },
        }
    }

    /// Takes ownership of the CBL ref, the reference counter is not increased so dropping the instance will free the ref.
    pub(crate) const fn take_ownership(cbl_ref: *mut CBLCollection) -> Self {
        Self { cbl_ref }
    }

    ////////

    /// Returns the scope of the collection.
    pub fn scope(&self) -> Scope {
        unsafe { Scope::take_ownership(CBLCollection_Scope(self.get_ref())) }
    }

    /// Returns the collection name.
    pub fn name(&self) -> String {
        unsafe {
            CBLCollection_Name(self.get_ref())
                .to_string()
                .unwrap_or_default()
        }
    }

    /// Returns the collection full name.
    pub fn full_name(&self) -> String {
        unsafe {
            CBLCollection_FullName(self.get_ref())
                .to_string()
                .unwrap_or_default()
        }
    }

    /// Returns the collection's database.
    pub fn database(&self) -> Database {
        unsafe { Database::reference(CBLCollection_Database(self.get_ref())) }
    }

    /// Returns the number of documents in the collection.
    pub fn count(&self) -> u64 {
        unsafe { CBLCollection_Count(self.get_ref()) }
    }

    /// Registers a collection change listener callback. It will be called after one or more documents are changed on disk.
    pub fn add_listener(
        &mut self,
        listener: CollectionChangeListener,
    ) -> Listener<CollectionChangeListener> {
        unsafe {
            let listener = Box::new(listener);
            let ptr = Box::into_raw(listener);

            Listener::new(
                ListenerToken {
                    cbl_ref: CBLCollection_AddChangeListener(
                        self.get_ref(),
                        Some(c_collection_change_listener),
                        ptr.cast(),
                    ),
                },
                Box::from_raw(ptr),
            )
        }
    }
}

impl CblRef for Collection {
    type Output = *mut CBLCollection;
    fn get_ref(&self) -> Self::Output {
        self.cbl_ref
    }
}

impl Drop for Collection {
    fn drop(&mut self) {
        unsafe { release(self.get_ref()) }
    }
}

impl Clone for Collection {
    fn clone(&self) -> Self {
        Self::reference(self.get_ref())
    }
}

/// A collection change listener callback, invoked after one or more documents are changed on disk.
pub type CollectionChangeListener = Box<dyn Fn(Collection, Vec<String>)>;

#[unsafe(no_mangle)]
unsafe extern "C" fn c_collection_change_listener(
    context: *mut ::std::os::raw::c_void,
    change: *const CBLCollectionChange,
) {
    let callback = context as *const CollectionChangeListener;
    unsafe {
        if let Some(change) = change.as_ref() {
            let collection = Collection::reference(change.collection as *mut CBLCollection);
            let doc_ids = std::slice::from_raw_parts(change.docIDs, change.numDocs as usize)
                .iter()
                .filter_map(|doc_id| doc_id.to_string())
                .collect();

            (*callback)(collection, doc_ids);
        }
    }
}
