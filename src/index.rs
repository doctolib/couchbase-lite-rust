use crate::{
    CblRef, Database,
    c_api::{
        CBLValueIndexConfiguration, CBLDatabase_GetIndexNames, CBLDatabase_DeleteIndex, CBLError,
        CBLDatabase_CreateValueIndex, CBLCollection_CreateValueIndex, CBLCollection_DeleteIndex,
        CBLCollection_GetIndexNames, CBLCollection_CreateArrayIndex, CBLArrayIndexConfiguration,
        CBLQueryIndex, CBLQueryIndex_Name, CBLQueryIndex_Collection, CBLCollection_GetIndex,
    },
    error::{Error, Result, failure},
    slice::{from_str, from_c_str, Slice},
    QueryLanguage, Array,
    collection::Collection,
    check_error, release, retain, CouchbaseLiteError,
};
use std::ffi::CString;

pub struct ValueIndexConfiguration {
    cbl_ref: CBLValueIndexConfiguration,
}

impl CblRef for ValueIndexConfiguration {
    type Output = CBLValueIndexConfiguration;
    fn get_ref(&self) -> Self::Output {
        self.cbl_ref
    }
}

impl ValueIndexConfiguration {
    /// Create a Value Index Configuration.
    /// You must indicate the query language used in the expressions.
    /// The expressions describe each coloumn of the index. The expressions could be specified
    /// in a JSON Array or in N1QL syntax using comma delimiter.
    pub fn new(query_language: QueryLanguage, expressions: &str) -> Self {
        let slice = from_str(expressions);
        Self {
            cbl_ref: CBLValueIndexConfiguration {
                expressionLanguage: query_language as u32,
                expressions: slice.get_ref(),
            },
        }
    }
}

/// Array Index Configuration for indexing property values within arrays
/// in documents, intended for use with the UNNEST query.
#[derive(Debug)]
pub struct ArrayIndexConfiguration {
    cbl_ref: CBLArrayIndexConfiguration,
    _path: Slice<CString>,
    _expressions: Slice<CString>,
}

impl CblRef for ArrayIndexConfiguration {
    type Output = CBLArrayIndexConfiguration;
    fn get_ref(&self) -> Self::Output {
        self.cbl_ref
    }
}

impl ArrayIndexConfiguration {
    /// Create an Array Index Configuration for indexing property values within arrays
    /// in documents, intended for use with the UNNEST query.
    ///   - query_langage:  The language used in the expressions (Required).
    ///   - path:  Path to the array, which can be nested to be indexed (Required).
    ///     Use "[]" to represent a property that is an array of each nested array level.
    ///     For a single array or the last level array, the "[]" is optional. For instance,
    ///     use "contacts[].phones" to specify an array of phones within each contact.
    ///   - expressions:  Optional expressions representing the values within the array to be
    ///     indexed. The expressions could be specified in a JSON Array or in N1QL syntax
    ///     using comma delimiter. If the array specified by the path contains scalar values,
    ///     the expressions should be left unset or set to null.
    ///
    /// # Example 1
    ///
    /// To index the values of array at path `likes` in documents:
    ///
    /// ```
    ///     ArrayIndexConfiguration::new(
    ///         QueryLanguage::N1QL,
    ///         "likes",
    ///         ""
    ///     )
    /// ```
    ///
    /// It would allow you to index the values "travel" and "skiing" in the following document:
    ///     {
    ///         likes: ["travel", "skiing"]
    ///     }
    ///
    /// # Example 2
    ///
    /// ```
    ///     ArrayIndexConfiguration::new(
    ///         QueryLanguage::N1QL,
    ///         "contacts[].phones",
    ///         "type"
    ///     )
    /// ```
    ///
    /// It would allow you to index the values "mobile" and "home" in the following document:
    ///     {
    ///         contacts: {
    ///             phones: [
    ///                 {
    ///                     type: "mobile"
    ///                 },
    ///                 {
    ///                     type: "home"
    ///                 }
    ///             ]
    ///         }
    ///     }
    pub fn new(query_language: QueryLanguage, path: &str, expressions: &str) -> Result<Self> {
        let path_c = CString::new(path)
            .map_err(|_| Error::cbl_error(CouchbaseLiteError::InvalidParameter))?;
        let expressions_c = CString::new(expressions)
            .map_err(|_| Error::cbl_error(CouchbaseLiteError::InvalidParameter))?;

        let path_s = from_c_str(path_c, path.len());
        let expressions_s = from_c_str(expressions_c, expressions.len());

        Ok(Self {
            cbl_ref: CBLArrayIndexConfiguration {
                expressionLanguage: query_language as u32,
                path: path_s.get_ref(),
                expressions: expressions_s.get_ref(),
            },
            _path: path_s,
            _expressions: expressions_s,
        })
    }
}

/// QueryIndex represents an existing index in a collection.
/// The QueryIndex can be used to obtain
/// a IndexUpdater object for updating the vector index in lazy mode.
pub struct QueryIndex {
    cbl_ref: *mut CBLQueryIndex,
}

impl CblRef for QueryIndex {
    type Output = *mut CBLQueryIndex;
    fn get_ref(&self) -> Self::Output {
        self.cbl_ref
    }
}

impl QueryIndex {
    //////// CONSTRUCTORS:

    /// Takes ownership of the object and increase it's reference counter.
    #[allow(dead_code)]
    pub(crate) fn retain(cbl_ref: *mut CBLQueryIndex) -> Self {
        Self {
            cbl_ref: unsafe { retain(cbl_ref) },
        }
    }

    /// References the object without taking ownership and increasing it's reference counter
    pub(crate) const fn wrap(cbl_ref: *mut CBLQueryIndex) -> Self {
        Self { cbl_ref }
    }

    ////////

    /// Returns the index's name.
    pub fn name(&self) -> String {
        unsafe {
            CBLQueryIndex_Name(self.get_ref())
                .to_string()
                .unwrap_or_default()
        }
    }

    /// Returns the collection that the index belongs to.
    pub fn collection(&self) -> Collection {
        unsafe { Collection::retain(CBLQueryIndex_Collection(self.get_ref())) }
    }
}

impl Drop for QueryIndex {
    fn drop(&mut self) {
        unsafe { release(self.get_ref()) }
    }
}

impl Database {
    /// Creates a value index.
    /// Indexes are persistent.
    /// If an identical index with that name already exists, nothing happens (and no error is returned.)
    /// If a non-identical index with that name already exists, it is deleted and re-created.
    #[deprecated(note = "please use `create_index` on default collection instead")]
    pub fn create_index(&self, name: &str, config: &ValueIndexConfiguration) -> Result<bool> {
        let mut err = CBLError::default();
        let slice = from_str(name);
        let r = unsafe {
            CBLDatabase_CreateValueIndex(
                self.get_ref(),
                slice.get_ref(),
                config.get_ref(),
                &mut err,
            )
        };
        if !err {
            return Ok(r);
        }
        failure(err)
    }

    /// Deletes an index given its name.
    #[deprecated(note = "please use `delete_index` on default collection instead")]
    pub fn delete_index(&self, name: &str) -> Result<bool> {
        let mut err = CBLError::default();
        let slice = from_str(name);
        let r = unsafe { CBLDatabase_DeleteIndex(self.get_ref(), slice.get_ref(), &mut err) };
        if !err {
            return Ok(r);
        }
        failure(err)
    }

    /// Returns the names of the indexes on this database, as an Array of strings.
    #[deprecated(note = "please use `get_index_names` on default collection instead")]
    pub fn get_index_names(&self) -> Array {
        let arr = unsafe { CBLDatabase_GetIndexNames(self.get_ref()) };
        Array::wrap(arr)
    }
}

impl Collection {
    /// Creates a value index in the collection.
    /// If an identical index with that name already exists, nothing happens (and no error is returned.)
    /// If a non-identical index with that name already exists, it is deleted and re-created.
    pub fn create_index(&self, name: &str, config: &ValueIndexConfiguration) -> Result<bool> {
        let mut err = CBLError::default();
        let slice = from_str(name);
        let r = unsafe {
            CBLCollection_CreateValueIndex(
                self.get_ref(),
                slice.get_ref(),
                config.get_ref(),
                &mut err,
            )
        };
        if !err {
            return Ok(r);
        }
        failure(err)
    }

    /// Creates an array index for use with UNNEST queries in the collection.
    /// If an identical index with that name already exists, nothing happens (and no error is returned.)
    /// If a non-identical index with that name already exists, it is deleted and re-created.
    pub fn create_array_index(&self, name: &str, config: &ArrayIndexConfiguration) -> Result<bool> {
        let mut err = CBLError::default();
        let slice = from_str(name);
        let r = unsafe {
            CBLCollection_CreateArrayIndex(
                self.get_ref(),
                slice.get_ref(),
                config.get_ref(),
                &mut err,
            )
        };
        if !err {
            return Ok(r);
        }
        failure(err)
    }

    /// Deletes an index in the collection by name.
    pub fn delete_index(&self, name: &str) -> Result<bool> {
        let mut err = CBLError::default();
        let slice = from_str(name);
        let r = unsafe { CBLCollection_DeleteIndex(self.get_ref(), slice.get_ref(), &mut err) };
        if !err {
            return Ok(r);
        }
        failure(err)
    }

    /// Returns the names of the indexes in the collection, as a Fleece array of strings.
    pub fn get_index_names(&self) -> Result<Array> {
        let mut err = CBLError::default();
        let arr = unsafe { CBLCollection_GetIndexNames(self.get_ref(), &mut err) };
        check_error(&err).map(|()| Array::wrap(arr))
    }

    /// Returns the names of the indexes in the collection, as a Fleece array of strings.
    pub fn get_index(&self, name: &str) -> Result<QueryIndex> {
        let mut err = CBLError::default();
        let slice = from_str(name);
        let index = unsafe { CBLCollection_GetIndex(self.get_ref(), slice.get_ref(), &mut err) };
        if !err {
            return Ok(QueryIndex::wrap(index));
        }
        failure(err)
    }
}
