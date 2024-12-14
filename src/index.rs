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
    check_error, retain, CouchbaseLiteError,
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
    /** Create a Value Index Configuration.
    @param query_langage  The language used in the expressions.
    @param expressions  The expressions describing each coloumn of the index. The expressions could be specified
        in a JSON Array or in N1QL syntax using comma delimiter. */
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
    /** Create an Array Index Configuration for indexing property values within arrays
    in documents, intended for use with the UNNEST query.
    @param query_langage  The language used in the expressions (Required).
    @param path  Path to the array, which can be nested to be indexed (Required).
        Use "[]" to represent a property that is an array of each nested array level.
        For a single array or the last level array, the "[]" is optional. For instance,
        use "contacts[].phones" to specify an array of phones within each contact.
    @param expressions  Optional expressions representing the values within the array to be
        indexed. The expressions could be specified in a JSON Array or in N1QL syntax
        using comma delimiter. If the array specified by the path contains scalar values,
        the expressions should be left unset or set to null. */
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
    pub(crate) fn retain(cbl_ref: *mut CBLQueryIndex) -> Self {
        Self {
            cbl_ref: unsafe { retain(cbl_ref) },
        }
    }

    pub fn name(&self) -> String {
        unsafe {
            CBLQueryIndex_Name(self.get_ref())
                .to_string()
                .unwrap_or_default()
        }
    }

    pub fn collection(&self) -> Collection {
        unsafe { Collection::retain(CBLQueryIndex_Collection(self.get_ref())) }
    }
}

impl Database {
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

    #[deprecated(note = "please use `get_index_names` on default collection instead")]
    pub fn get_index_names(&self) -> Array {
        let arr = unsafe { CBLDatabase_GetIndexNames(self.get_ref()) };
        Array::wrap(arr)
    }
}

impl Collection {
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

    pub fn delete_index(&self, name: &str) -> Result<bool> {
        let mut err = CBLError::default();
        let slice = from_str(name);
        let r = unsafe { CBLCollection_DeleteIndex(self.get_ref(), slice.get_ref(), &mut err) };
        if !err {
            return Ok(r);
        }
        failure(err)
    }

    pub fn get_index_names(&self) -> Result<Array> {
        let mut err = CBLError::default();
        let arr = unsafe { CBLCollection_GetIndexNames(self.get_ref(), &mut err) };
        check_error(&err).map(|()| Array::wrap(arr))
    }

    pub fn get_index(&self, name: &str) -> Result<QueryIndex> {
        let mut err = CBLError::default();
        let slice = from_str(name);
        let index = unsafe { CBLCollection_GetIndex(self.get_ref(), slice.get_ref(), &mut err) };
        if !err {
            return Ok(QueryIndex::retain(index));
        }
        failure(err)
    }
}
