#![cfg(test)]

extern crate couchbase_lite;

use couchbase_lite::*;

#[test]
fn couchbase_lite_c_version_test() {
    assert_eq!(couchbase_lite_c_version(), "3.2.2".to_string());
}
