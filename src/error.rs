// Couchbase Lite error classs
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

use crate::c_api::{
    CBLError, CBLErrorDomain, CBLError_Message, FLError, kCBLDomain, kCBLFleeceDomain,
    kCBLNetworkDomain, kCBLPOSIXDomain, kCBLSQLiteDomain, kCBLWebSocketDomain,
};
use enum_primitive::FromPrimitive;
use std::fmt;

//////// ERROR STRUCT:

/// Error type. Wraps multiple types of errors in an enum.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Error {
    pub code: ErrorCode,
    pub(crate) internal_info: Option<u32>,
}

/// The enum that stores the error domain and code for an Error.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum ErrorCode {
    CouchbaseLite(CouchbaseLiteError),
    POSIX(i32),
    SQLite(i32),
    Fleece(FleeceError),
    Network(NetworkError),
    WebSocket(i32),
}

/// Redefine `Result` to assume our `Error` type
pub type Result<T> = std::result::Result<T, Error>;

enum_from_primitive! {
    /// Couchbase Lite error codes.
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub enum CouchbaseLiteError {
        /// Internal assertion failure
        AssertionFailed = 1,
        /// Oops, an unimplemented API call
        Unimplemented,
        /// Unsupported encryption algorithm
        UnsupportedEncryption,
        /// Invalid revision ID syntax
        BadRevisionID,
        /// Revision contains corrupted/unreadable data
        CorruptRevisionData,
        /// Database/KeyStore/index is not open
        NotOpen,
        /// Document not found
        NotFound,
        /// Document update conflict
        Conflict,
        /// Invalid function parameter or struct value
        InvalidParameter,
        /// Internal unexpected C++ exception
        UnexpectedError, /*10*/
        /// Database file can't be opened; may not exist
        CantOpenFile,
        /// File I/O error
        IOError,
        /// Memory allocation failed (out of memory?)
        MemoryError,
        /// File is not writeable
        NotWriteable,
        /// Data is corrupted
        CorruptData,
        /// Database is busy/locked
        Busy,
        /// Function must be called while in a transaction
        NotInTransaction,
        /// Database can't be closed while a transaction is open
        TransactionNotClosed,
        /// Operation not supported in this database
        Unsupported,
        /// File is not a database, or encryption key is wrong
        NotADatabaseFile,/*20*/
        /// Database exists but not in the format/storage requested
        WrongFormat,
        /// Encryption/decryption error
        Crypto,
        /// Invalid query
        InvalidQuery,
        /// No such index, or query requires a nonexistent index
        MissingIndex,
        /// Unknown query param name, or param number out of range
        InvalidQueryParam,
        /// Unknown error from remote server
        RemoteError,
        /// Database file format is older than what I can open
        DatabaseTooOld,
        /// Database file format is newer than what I can open
        DatabaseTooNew,
        /// Invalid document ID
        BadDocID,
        /// DB can't be upgraded (might be unsupported dev version)
        CantUpgradeDatabase,/*30*/

        /// Can't translate native error (unknown domain or code)
        UntranslatableError = 1000,
    }
}

enum_from_primitive! {
    /// Fleece error codes
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub enum FleeceError {
        /// Out of memory, or allocation failed
        MemoryError = 1,
        /// Array index or iterator out of range
        OutOfRange,
        /// Bad input data (NaN, non-string key, etc.)
        InvalidData,
        /// Structural error encoding (missing value, too many ends, etc.)
        EncodeError,
        /// Error parsing JSON
        JSONError,
        /// Unparseable data in a Value (corrupt? Or from some distant future?)
        UnknownValue,
        /// Something that shouldn't happen
        InternalError,
        /// Key not found
        NotFound,
        /// Misuse of shared keys (not in transaction, etc.)
        SharedKeysStateError,
        /// Something went wrong at the OS level (file I/O, etc.)
        POSIXError,
        /// Operation is unsupported
        Unsupported,
    }
}

enum_from_primitive! {
    /// Network error codes defined by Couchbase Lite.
    #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    pub enum NetworkError {
        /// DNS lookup failed
        DNSFailure = 1,
        /// DNS server doesn't know the hostname
        UnknownHost,
        /// No response received before timeout
        Timeout,
        /// Invalid URL
        InvalidURL,
        /// HTTP redirect loop
        TooManyRedirects,
        /// Low-level error establishing TLS
        TLSHandshakeFailed,
        /// Server's TLS certificate has expired
        TLSCertExpired,
        /// Cert isn't trusted for other reason
        TLSCertUntrusted,
        /// Server requires client to have a TLS certificate
        TLSClientCertRequired,
        /// Server rejected my TLS client certificate
        TLSClientCertRejected,
        /// Self-signed cert, or unknown anchor cert
        TLSCertUnknownRoot,
        /// Attempted redirect to invalid URL
        InvalidRedirect,
        /// Unknown networking error
        Unknown,
        /// Server's cert has been revoked
        TLSCertRevoked,
        /// Server cert's name does not match DNS name
        TLSCertNameMismatch,
    }
}

impl Default for Error {
    fn default() -> Self {
        Self::new(&CBLError::default())
    }
}

impl Error {
    pub(crate) fn new(err: &CBLError) -> Self {
        Self {
            code: ErrorCode::new(err),
            internal_info: Some(err.internal_info),
        }
    }

    pub(crate) const fn cbl_error(e: CouchbaseLiteError) -> Self {
        Self {
            code: ErrorCode::CouchbaseLite(e),
            internal_info: None,
        }
    }

    pub(crate) fn fleece_error(e: FLError) -> Self {
        Self {
            code: ErrorCode::from_fleece(e as i32),
            internal_info: None,
        }
    }

    pub(crate) fn as_cbl_error(&self) -> CBLError {
        let domain: i32;
        let code: i32;
        match &self.code {
            ErrorCode::CouchbaseLite(e) => {
                domain = kCBLDomain;
                code = *e as i32;
            }
            ErrorCode::Fleece(e) => {
                domain = kCBLFleeceDomain;
                code = *e as i32;
            }
            ErrorCode::Network(e) => {
                domain = kCBLNetworkDomain;
                code = *e as i32;
            }
            ErrorCode::POSIX(e) => {
                domain = kCBLPOSIXDomain;
                code = *e;
            }
            ErrorCode::SQLite(e) => {
                domain = kCBLSQLiteDomain;
                code = *e;
            }
            ErrorCode::WebSocket(e) => {
                domain = kCBLWebSocketDomain;
                code = *e;
            }
        }
        CBLError {
            domain: domain as CBLErrorDomain,
            code,
            internal_info: self.internal_info.unwrap_or(0),
        }
    }

    /// Returns a message describing an error.
    pub fn message(&self) -> String {
        if let ErrorCode::CouchbaseLite(e) = self.code {
            if e == CouchbaseLiteError::UntranslatableError {
                return "Unknown error".to_string();
            }
        }
        unsafe {
            CBLError_Message(&self.as_cbl_error())
                .to_string()
                .unwrap_or_default()
        }
    }
}

impl std::error::Error for Error {}

impl fmt::Debug for Error {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> std::result::Result<(), fmt::Error> {
        fmt.write_fmt(format_args!("{:?}: {})", self.code, self.message()))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> std::result::Result<(), fmt::Error> {
        fmt.write_str(&self.message())
    }
}

impl ErrorCode {
    fn new(err: &CBLError) -> Self {
        match i32::from(err.domain) {
            kCBLDomain => CouchbaseLiteError::from_i32(err.code)
                .map_or(Self::untranslatable(), Self::CouchbaseLite),
            kCBLNetworkDomain => {
                NetworkError::from_i32(err.code).map_or(Self::untranslatable(), Self::Network)
            }
            kCBLPOSIXDomain => Self::POSIX(err.code),
            kCBLSQLiteDomain => Self::SQLite(err.code),
            kCBLFleeceDomain => Self::from_fleece(err.code),
            kCBLWebSocketDomain => Self::WebSocket(err.code),
            _ => Self::untranslatable(),
        }
    }

    fn from_fleece(fleece_error: i32) -> Self {
        FleeceError::from_i32(fleece_error).map_or(Self::untranslatable(), Self::Fleece)
    }

    const fn untranslatable() -> Self {
        Self::CouchbaseLite(CouchbaseLiteError::UntranslatableError)
    }
}

//////// CBLERROR UTILITIES:
#[allow(clippy::derivable_impls)]
impl Default for CBLError {
    fn default() -> Self {
        Self {
            domain: 0,
            code: 0,
            internal_info: 0,
        }
    }
}

impl std::ops::Not for CBLError {
    type Output = bool;
    fn not(self) -> bool {
        self.code == 0
    }
}

impl std::ops::Not for &CBLError {
    type Output = bool;
    fn not(self) -> bool {
        self.code == 0
    }
}

// Convenient way to return a Result from a failed CBLError
pub(crate) fn failure<T>(err: CBLError) -> Result<T> {
    assert!(err.code != 0);
    Err(Error::new(&err))
}

pub(crate) fn check_failure(status: bool, err: &CBLError) -> Result<()> {
    if status {
        return Ok(());
    }
    assert!(err.code != 0);
    Err(Error::new(err))
}

pub(crate) fn check_error(err: &CBLError) -> Result<()> {
    if err.domain == 0 || err.code == 0 {
        Ok(())
    } else {
        Err(Error::new(err))
    }
}

pub(crate) fn check_bool<F>(func: F) -> Result<()>
where
    F: Fn(*mut CBLError) -> bool,
{
    let mut error = CBLError::default();
    let ok = func(&mut error);
    check_failure(ok, &error)
}

/// The first parameter is a function that returns a non-null pointer or sets the error.
/// The second parameter is a function that takes the returned pointer and returns the final result.
pub(crate) fn check_ptr<PTR, F, MAPF, RESULT>(func: F, map: MAPF) -> Result<RESULT>
where
    F: Fn(*mut CBLError) -> *mut PTR,
    MAPF: FnOnce(*mut PTR) -> RESULT,
{
    let mut error = CBLError::default();
    let ptr = func(&mut error);
    if ptr.is_null() {
        failure(error)
    } else {
        Ok(map(ptr))
    }
}

/// The first parameter is a function that returns a non-null pointer or sets the error.
/// The second parameter is a function that takes the returned pointer and returns the final result.
pub(crate) fn check_io<F>(mut func: F) -> std::io::Result<usize>
where
    F: FnMut(*mut CBLError) -> i32,
{
    let mut error = CBLError::default();
    let n = func(&mut error);
    if n < 0 {
        // TODO: Better error mapping!
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            Error::new(&error),
        ));
    }
    #[allow(clippy::cast_sign_loss)]
    Ok(n as usize)
}
