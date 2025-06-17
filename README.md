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

There two different editions of Couchbase Lite C: community & enterprise.
You can find the differences [here][CBL_EDITIONS_DIFF].

When building or declaring this repository as a dependency, you need to specify the edition through a cargo feature:

```shell
$ cargo build --features=community
```

```shell
$ cargo build --features=enterprise
```

## Maintaining

### Couchbase Lite For C

The Couchbase Lite For C shared library and headers ([Git repo][CBL_C]) are already embedded in this repo.
They are present in two directories, one for each edition: `libcblite_community` & `libcblite_enterprise`.

### Upgrade Couchbase Lite C

The different releases can be found in [this page][CBL_DOWNLOAD_PAGE].

When a new C release is available, a new Rust release must be created. Running the following script will download and setup the libraries locally:

```shell
$ ./update_cblite_c.sh -v 3.2.2
```

If the script fails (for example `declare: -A: invalid option`, you'll need bash >= version 4) on MacOS, you might need to install wget or a recent bash version:

```shell
$ brew install wget
$ brew install bash
```

If the script was successful:
- Change the link `CBL_API_REFERENCE` in this README
- Change the version in the test `couchbase_lite_c_version_test`
- Update the version in `Cargo.toml`
- Fix the compilation in both editions
- Fix the tests in both editions
- Create pull request

New C features should also be added to the Rust API at some point.

### Test

Tests can be found in the `tests` subdirectory.
Test are run in the GitHub wrokflow `Test`. You can find the commands used there.

There are three variations:

### Nominal run

```shell
$ cargo test --features=enterprise
```

### Run with Couchbase Lite C leak check

Couchbase Lite C allows checking if instances of their objects are still alive through the functions `CBL_InstanceCount` & `CBL_DumpInstances`.
If the `LEAK_CHECK` environment variable is set, we check that the number of instances at the end of each test is 0.

If this step fails in one of your pull requests, you should look into the `take_ownership`/`reference` logic on CBL pointers in the constructor of the Rust structs:
- `take_ownership` takes ownership of the pointer, it will not increase the ref count of the `ref` CBL pointer so releasing it (in a `drop` for example) will free the pointer
- `reference` just references the pointer, it will increase the ref count of CBL pointers so releasing it will not free the pointer

```shell
$ LEAK_CHECK=y cargo test --features=enterprise -- --test-threads 1
```

### Run with address sanitizer

```shell
$ LSAN_OPTIONS=suppressions=san.supp RUSTFLAGS="-Zsanitizer=address" cargo +nightly test  --features=enterprise
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

[CBL_API_REFERENCE]: https://docs.couchbase.com/mobile/3.2.3/couchbase-lite-c/C/html/modules.html

[CBL_EDITIONS_DIFF]: https://www.couchbase.com/products/editions/

[FLEECE]: https://github.com/couchbaselabs/fleece/wiki/Using-Fleece

[BINDGEN]: https://rust-lang.github.io/rust-bindgen/

[BINDGEN_INSTALL]: https://rust-lang.github.io/rust-bindgen/requirements.html

[DOCTOLIB]: https://www.doctolib.fr/

[DOCTOLIB_GH]: https://github.com/doctolib
