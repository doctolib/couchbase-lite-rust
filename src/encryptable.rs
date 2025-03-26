use crate::{
    c_api::{
        CBLEncryptable, CBLEncryptable_CreateWithArray, CBLEncryptable_CreateWithBool,
        CBLEncryptable_CreateWithDict, CBLEncryptable_CreateWithDouble,
        CBLEncryptable_CreateWithFloat, CBLEncryptable_CreateWithInt,
        CBLEncryptable_CreateWithNull, CBLEncryptable_CreateWithString,
        CBLEncryptable_CreateWithUInt, CBLEncryptable_CreateWithValue, CBLEncryptable_Properties,
        CBLEncryptable_Value, FLSlice_Copy,
    },
    slice::from_str,
    Array, CblRef, Dict, Value, release, retain,
};

/// An Encryptable is a value to be encrypted by the replicator when a document is
/// pushed to the remote server. When a document is pulled from the remote server, the
/// encrypted value will be decrypted by the replicator.
///
/// Similar to Blob, an Encryptable acts as a proxy for a dictionary structure
/// with the special marker property `"@type":"encryptable"`, and another property `value`
/// whose value is the actual value to be encrypted by the push replicator.
///
/// The push replicator will automatically detect Encryptable dictionaries inside
/// the document and calls the specified PropertyEncryptor callback to encrypt the
/// actual value. When the value is successfully encrypted, the replicator will transform
/// the property key and the encrypted dictionary value into
/// Couchbase Server SDK's encrypted field format :
///
/// * The original key will be prefixed with 'encrypted$'.
///
/// * The transformed Encryptable dictionary will contain `alg` property indicating
///     the encryption algorithm, `ciphertext` property whose value is a base-64 string of the
///     encrypted value, and optionally `kid` property indicating the encryption key identifier
///     if specified when returning the result of PropertyEncryptor callback call.
///
/// For security reason, a document that contains Encryptable dictionaries will fail
/// to push if their value cannot be encrypted including
/// when a PropertyEncryptor callback is not specified or when there is an error
/// or a null result returned from the callback call.
///
/// The pull replicator will automatically detect the encrypted properties that are in the
/// Couchbase Server SDK's encrypted field format and call the specified PropertyDecryptor
/// callback to decrypt the encrypted value. When the value is successfully decrypted,
/// the replicator will transform the property format back to the Encryptable format
/// including removing the 'encrypted$' prefix.
///
/// The PropertyDecryptor callback can intentionally skip the decryption by returnning a
/// null result. When a decryption is skipped, the encrypted property in the form of
/// Couchbase Server SDK's encrypted field format will be kept as it was received from the remote
/// server. If an error is returned from the callback call, the document will be failed to pull with
/// the ErrorCrypto error.
///
/// If a PropertyDecryptor callback is not specified, the replicator will not attempt to
/// detect any encrypted properties. As a result, all encrypted properties in the form of
/// Couchbase Server SDK's encrypted field format will be kept as they was received from the remote
/// server.
pub struct Encryptable {
    cbl_ref: *mut CBLEncryptable,
}

impl CblRef for Encryptable {
    type Output = *mut CBLEncryptable;
    fn get_ref(&self) -> Self::Output {
        self.cbl_ref
    }
}

impl From<*mut CBLEncryptable> for Encryptable {
    fn from(cbl_ref: *mut CBLEncryptable) -> Self {
        Self::take_ownership(cbl_ref)
    }
}

impl Encryptable {
    //////// CONSTRUCTORS:

    /// Increase the reference counter of the CBL ref, so dropping the instance will NOT free the ref.
    pub(crate) fn reference(cbl_ref: *mut CBLEncryptable) -> Self {
        Self {
            cbl_ref: unsafe { retain(cbl_ref) },
        }
    }

    /// Takes ownership of the CBL ref, the reference counter is not increased so dropping the instance will free the ref.
    pub(crate) const fn take_ownership(cbl_ref: *mut CBLEncryptable) -> Self {
        Self { cbl_ref }
    }

    ////////

    /// Creates Encryptable object with null value.
    pub fn create_with_null() -> Self {
        unsafe { CBLEncryptable_CreateWithNull().into() }
    }

    /// Creates Encryptable object with a boolean value.
    pub fn create_with_bool(value: bool) -> Self {
        unsafe { CBLEncryptable_CreateWithBool(value).into() }
    }

    /// Creates Encryptable object with i64 value.
    pub fn create_with_int(value: i64) -> Self {
        unsafe { CBLEncryptable_CreateWithInt(value).into() }
    }

    /// Creates Encryptable object with an u64 value.
    pub fn create_with_uint(value: u64) -> Self {
        unsafe { CBLEncryptable_CreateWithUInt(value).into() }
    }

    /// Creates Encryptable object with a f32 value.
    pub fn create_with_float(value: f32) -> Self {
        unsafe { CBLEncryptable_CreateWithFloat(value).into() }
    }

    /// Creates Encryptable object with a f64 value.
    pub fn create_with_double(value: f64) -> Self {
        unsafe { CBLEncryptable_CreateWithDouble(value).into() }
    }

    /// Creates Encryptable object with a string value.
    pub fn create_with_string(value: &str) -> Self {
        unsafe {
            let slice = from_str(value);
            let copy_slice = FLSlice_Copy(slice.get_ref());
            let final_slice = copy_slice.as_slice();
            CBLEncryptable_CreateWithString(final_slice).into()
        }
    }

    /// Creates Encryptable object with an Value value.
    pub fn create_with_value(value: Value) -> Self {
        unsafe { CBLEncryptable_CreateWithValue(value.get_ref()).into() }
    }

    /// Creates Encryptable object with an Array value.
    pub fn create_with_array(value: Array) -> Self {
        unsafe { CBLEncryptable_CreateWithArray(value.get_ref()).into() }
    }

    /// Creates Encryptable object with a Dict value.
    pub fn create_with_dict(value: Dict) -> Self {
        unsafe { CBLEncryptable_CreateWithDict(value.get_ref()).into() }
    }

    /// Returns the value to be encrypted by the push replicator.
    pub fn get_value(&self) -> Value {
        unsafe { Value::wrap(CBLEncryptable_Value(self.get_ref()), self) }
    }

    /// Returns the dictionary format of the Encryptable object.
    pub fn get_properties(&self) -> Dict {
        unsafe { Dict::wrap(CBLEncryptable_Properties(self.get_ref()), self) }
    }
}

impl Drop for Encryptable {
    fn drop(&mut self) {
        unsafe {
            release(self.get_ref().cast::<CBLEncryptable>());
        }
    }
}

impl Clone for Encryptable {
    fn clone(&self) -> Self {
        unsafe {
            Self {
                cbl_ref: retain(self.get_ref().cast::<CBLEncryptable>()),
            }
        }
    }
}
