# Rust Bindings For Couchbase Lite

This is a Rust API of [Couchbase Lite][CBL], an embedded NoSQL document database engine with sync.

The crate wraps the [couchbase-lite-C][CBL_C] releases with an idiomatic Rust API.

## Disclaimer

This library is **NOT SUPPORTED BY COUCHBASE**, it was forked from Couchbase Labs' repo [couchbase-lite-rust][CBL_RUST] and finalized.
It is currently used and maintained by [Doctolib][DOCTOLIB] ([GitHub][DOCTOLIB_GH]).

The supported platforms are Windows, macOS, Linux, Android and iOS.

## Building

### 1. Install LLVM/Clang

In addition to [Rust][RUST], you'll need to install LLVM and Clang, which are required by the [bindgen][BINDGEN] tool that generates Rust FFI APIs from C headers.
Installation instructions are [here][BINDGEN_INSTALL].

### 2. Build!

```shell
$ cargo build
```

## Maintaining

### Couchbase Lite For C

The Couchbase Lite For C shared library and headers ([Git repo][CBL_C]) are already embedded in this repo.
They are present in the directory `libcblite`.

### Upgrade Couchbase Lite C

The different releases can be found in [this page][CBL_DOWNLOAD_PAGE].

When a new C release is available, a new Rust release must be created. Running the following script will download and setup the libraries locally:

```shell
$ ./update_cblite_c.sh -v 3.2.1
```

If the script fails on MacOS, you might need to install wget or a recent bash version:

```shell
$ brew install wget
$ brew install bash
```

After that, fix the compilation & tests and you can create a pull request.

New C features should also be added to the Rust API at some point.

### Test

**The unit tests must be run single-threaded.** This is because each test case checks for leaks by
counting the number of extant Couchbase Lite objects before and after it runs, and failing if the
number increases. That works only if a single test runs at a time.

```shell
$ LEAK_CHECK=y cargo test -- --test-threads 1
```

### Sanitizer

```shell
$ LSAN_OPTIONS=suppressions=san.supp RUSTFLAGS="-Zsanitizer=address" cargo +nightly test 
```

**To diag flaky test**

```shell
$ LSAN_OPTIONS=suppressions=san.supp RUSTFLAGS="-Zsanitizer=address" cargo +nightly test --verbose --features=flaky-test flaky
```

## Learning

[Official Couchbase Lite documentation][CBL_DOCS]

[C API reference][CBL_API_REFERENCE]

[Using Fleece][FLEECE]

[RUST]: https://www.rust-lang.org

[CBL]: https://www.couchbase.com/products/lite

[CBL_DOWNLOAD_PAGE]: https://www.couchbase.com/downloads/?family=couchbase-lite

[CBL_C]: https://github.com/couchbase/couchbase-lite-C

[CBL_RUST]: https://github.com/couchbaselabs/couchbase-lite-rust

[CBL_DOCS]: https://docs.couchbase.com/couchbase-lite/current/introduction.html

[CBL_API_REFERENCE]: https://docs.couchbase.com/mobile/3.2.1/couchbase-lite-c/C/html/modules.html

[FLEECE]: https://github.com/couchbaselabs/fleece/wiki/Using-Fleece

[BINDGEN]: https://rust-lang.github.io/rust-bindgen/

[BINDGEN_INSTALL]: https://rust-lang.github.io/rust-bindgen/requirements.html

[DOCTOLIB]: https://www.doctolib.fr/

[DOCTOLIB_GH]: https://github.com/doctolib
