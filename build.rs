// Couchbase Lite C API bindings generator
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

// This script runs during a Cargo build and generates the raw/unsafe Rust bindings, "bindings.rs",
// in an internal build directory, where they are included by `src/c_api.rs`.
//
// References:
// - https://rust-lang.github.io/rust-bindgen/tutorial-3.html
// - https://doc.rust-lang.org/cargo/reference/build-scripts.html

#[cfg(all(not(feature = "community"), not(feature = "enterprise")))]
compile_error!(
    "You need to have at least one the following features activated: community, enterprise"
);
#[cfg(all(feature = "community", feature = "enterprise"))]
compile_error!(
    "You need to have at most one the following features activated: community, enterprise"
);

extern crate bindgen;
extern crate fs_extra;

use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use fs_extra::dir;

// Custom callback to handle MSVC type mapping issues
#[derive(Debug)]
struct MSVCTypeCallback;

impl bindgen::callbacks::ParseCallbacks for MSVCTypeCallback {
    fn int_macro(&self, name: &str, value: i64) -> Option<bindgen::callbacks::IntKind> {
        // Force unsigned macros and values to use u32 on MSVC
        if name.contains("UINT")
            || name.ends_with("_U")
            || (value >= 0 && name.contains("UNSIGNED"))
        {
            Some(bindgen::callbacks::IntKind::U32)
        } else {
            None
        }
    }

    fn item_name(&self, item_info: bindgen::callbacks::ItemInfo<'_>) -> Option<String> {
        // Handle specific type mappings if needed
        match item_info.name {
            "c_uint" => Some("u32".to_string()),
            "c_ulong" => Some("u32".to_string()),
            _ => None,
        }
    }

    // Chain with CargoCallbacks
    fn include_file(&self, filename: &str) {
        bindgen::CargoCallbacks::new().include_file(filename);
    }
}

#[cfg(feature = "community")]
static CBL_INCLUDE_DIR: &str = "libcblite_community/include";
#[cfg(feature = "enterprise")]
static CBL_INCLUDE_DIR: &str = "libcblite_enterprise/include";

#[cfg(feature = "community")]
static CBL_LIB_DIR: &str = "libcblite_community/lib";
#[cfg(feature = "enterprise")]
static CBL_LIB_DIR: &str = "libcblite_enterprise/lib";

fn main() -> Result<(), Box<dyn Error>> {
    generate_bindings()?;
    configure_rustc()?;

    // Bypass copying libraries when the build script is called in a cargo check context.
    if env::var("ONLY_CARGO_CHECK").unwrap_or_default() != *"true" {
        copy_lib().unwrap_or_else(|_| {
            panic!(
                "can't copy cblite libs, is '{}' a supported target?",
                env::var("TARGET").unwrap_or_default()
            )
        });
    }

    Ok(())
}

fn bindgen_for_mac(builder: bindgen::Builder) -> Result<bindgen::Builder, Box<dyn Error>> {
    let target = env::var("TARGET")?;
    if !target.contains("apple") {
        return Ok(builder);
    }

    let sdk = if target.contains("ios") {
        if target.contains("x86") || target.contains("sim") {
            "iphonesimulator"
        } else {
            "iphoneos"
        }
    } else {
        "macosx"
    };

    let sdk = String::from_utf8(
        Command::new("xcrun")
            .args(["--sdk", sdk, "--show-sdk-path"])
            .output()
            .expect("failed to execute process")
            .stdout,
    )?;

    Ok(builder.clang_arg(format!("-isysroot{}", sdk.trim())))
}

#[allow(dead_code)]
enum OperatingSystem {
    MacOs,
    Windows,
    Android,
    IOs,
}

fn find_msvc_paths() -> Result<(Option<String>, Option<String>), Box<dyn Error>> {
    // Try to find MSVC installation paths automatically
    let mut msvc_include = None;
    let mut ucrt_include = None;

    // Method 1: Use environment variables from Rust/Cargo build (preferred method)
    if let Ok(include_path) = env::var("INCLUDE") {
        // Parse the INCLUDE environment variable that MSVC sets
        for path in include_path.split(';') {
            let path = path.trim();
            if !path.is_empty() {
                if path.contains("VC\\Tools\\MSVC") && path.ends_with("include") {
                    msvc_include = Some(path.to_string());
                } else if path.contains("Windows Kits") && path.ends_with("ucrt") {
                    ucrt_include = Some(path.to_string());
                }
            }
        }
    }

    // Method 2: Try using vswhere.exe if available and no env vars found
    if msvc_include.is_none()
        && let Ok(output) = Command::new("vswhere.exe")
            .args([
                "-latest",
                "-products",
                "*",
                "-requires",
                "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
                "-property",
                "installationPath",
            ])
            .output()
        && output.status.success()
    {
        let vs_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !vs_path.is_empty() {
            // Try to find the MSVC version
            let vc_tools_path = format!("{}\\VC\\Tools\\MSVC", vs_path);
            if let Ok(entries) = fs::read_dir(&vc_tools_path) {
                let mut versions = Vec::new();
                for entry in entries.flatten() {
                    if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        let version_path = entry.path();
                        let include_path = version_path.join("include");
                        if include_path.exists() {
                            let version = entry.file_name().to_string_lossy().to_string();
                            versions.push((version, include_path.to_string_lossy().to_string()));
                        }
                    }
                }
                // Sort versions and take the latest
                versions.sort_by(|a, b| b.0.cmp(&a.0));
                if let Some((_, path)) = versions.first() {
                    msvc_include = Some(path.clone());
                }
            }
        }
    }

    // Method 3: Check common Visual Studio locations
    if msvc_include.is_none() {
        let common_vs_paths = [
            "C:\\Program Files\\Microsoft Visual Studio\\2022\\Enterprise\\VC\\Tools\\MSVC",
            "C:\\Program Files\\Microsoft Visual Studio\\2022\\Professional\\VC\\Tools\\MSVC",
            "C:\\Program Files\\Microsoft Visual Studio\\2022\\Community\\VC\\Tools\\MSVC",
            "C:\\Program Files\\Microsoft Visual Studio\\2019\\Enterprise\\VC\\Tools\\MSVC",
            "C:\\Program Files\\Microsoft Visual Studio\\2019\\Professional\\VC\\Tools\\MSVC",
            "C:\\Program Files\\Microsoft Visual Studio\\2019\\Community\\VC\\Tools\\MSVC",
        ];

        for vs_path in &common_vs_paths {
            if let Ok(entries) = fs::read_dir(vs_path) {
                let mut versions = Vec::new();
                for entry in entries.flatten() {
                    if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        let version_path = entry.path();
                        let include_path = version_path.join("include");
                        if include_path.exists() {
                            let version = entry.file_name().to_string_lossy().to_string();
                            versions.push((version, include_path.to_string_lossy().to_string()));
                        }
                    }
                }
                // Sort versions and take the latest
                versions.sort_by(|a, b| b.0.cmp(&a.0));
                if let Some((_, path)) = versions.first() {
                    msvc_include = Some(path.clone());
                    break;
                }
            }
        }
    }

    // Method 4: Try to find Windows SDK UCRT if not found via env vars
    if ucrt_include.is_none() {
        let sdk_paths = [
            "C:\\Program Files (x86)\\Windows Kits\\10\\Include",
            "C:\\Program Files\\Windows Kits\\10\\Include",
        ];

        for sdk_path in &sdk_paths {
            if let Ok(entries) = fs::read_dir(sdk_path) {
                // Find the latest version
                let mut versions = Vec::new();
                for entry in entries.flatten() {
                    if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        let name = entry.file_name();
                        let name_str = name.to_string_lossy();
                        if name_str.starts_with("10.") {
                            let ucrt_path = entry.path().join("ucrt");
                            if ucrt_path.exists() {
                                versions.push((
                                    name_str.to_string(),
                                    ucrt_path.to_string_lossy().to_string(),
                                ));
                            }
                        }
                    }
                }
                // Sort versions and take the latest
                versions.sort_by(|a, b| b.0.cmp(&a.0));
                if let Some((_, path)) = versions.first() {
                    ucrt_include = Some(path.clone());
                    break;
                }
            }
        }
    }

    Ok((msvc_include, ucrt_include))
}

fn is_target(target: OperatingSystem) -> Result<bool, Box<dyn Error>> {
    let target_os = env::var("CARGO_CFG_TARGET_OS")?;
    Ok(match target {
        OperatingSystem::Android => target_os.contains("android"),
        OperatingSystem::Windows => target_os.contains("windows"),
        OperatingSystem::IOs => target_os.contains("apple-ios"),
        OperatingSystem::MacOs => target_os.contains("apple-darwin"),
    })
}

fn is_host(host: OperatingSystem) -> Result<bool, Box<dyn Error>> {
    let host_os = env::var("HOST")?;
    Ok(match host {
        OperatingSystem::MacOs => host_os.contains("apple-darwin"),
        OperatingSystem::Windows => host_os.contains("windows"),
        OperatingSystem::Android => host_os.contains("android"),
        OperatingSystem::IOs => host_os.contains("apple-ios"),
    })
}

fn generate_bindings() -> Result<(), Box<dyn Error>> {
    let mut bindings = bindgen_for_mac(bindgen::Builder::default())?
        .header("src/wrapper.h")
        .clang_arg(format!("-I{}", CBL_INCLUDE_DIR));

    // Fix cross-compilation from MacOS to Android targets.
    // The following clang_arg calls prevent bindgen from trying to include
    // MacOS standards headers and returning an error when trying to generate bindings.
    // Basically, we specifiy NDK sysroot and usr/include dirs depending on the target arch.
    //
    // Sample of errors:
    //
    // /Applications/Xcode.app/.../Developer/SDKs/MacOSX10.15.sdk/usr/include/sys/cdefs.h:807:2: error: Unsupported architecture
    // /Applications/Xcode.app/.../Developer/SDKs/MacOSX10.15.sdk/usr/include/machine/_types.h:34:2: error: architecture not supported
    // FTR: https://github.com/rust-lang/rust-bindgen/issues/1780
    if is_host(OperatingSystem::MacOs)? && is_target(OperatingSystem::Android)? {
        let ndk_sysroot = format!(
            "{}/toolchains/llvm/prebuilt/darwin-x86_64/sysroot",
            env::var("NDK_HOME")?,
        );
        let target_triplet =
            if env::var("CARGO_CFG_TARGET_ARCH").expect("Can't read target arch value!") == "arm" {
                "arm-linux-androideabi"
            } else {
                "aarch64-linux-android"
            };
        bindings = bindings
            .clang_arg(format!("--sysroot={}", ndk_sysroot))
            .clang_arg(format!("-I{}/usr/include", ndk_sysroot))
            .clang_arg(format!("-I{}/usr/include/{}", ndk_sysroot, target_triplet))
            .clang_arg(format!("--target={}", target_triplet));
    }

    // Cross compiling from Mac to Windows
    if is_host(OperatingSystem::MacOs)? && is_target(OperatingSystem::Windows)? {
        let homebrew_prefix = Command::new("brew")
            .arg("--prefix")
            .arg("mingw-w64")
            .output()?
            .stdout;
        let homebrew_prefix =
            String::from_utf8_lossy(&homebrew_prefix[..homebrew_prefix.len() - 1]);
        let mingw_path = format!("{homebrew_prefix}/toolchain-x86_64/x86_64-w64-mingw32");
        let mingw_include_path = format!("{mingw_path}/include");
        bindings = bindings
            .clang_arg(format!("-I{}", mingw_include_path))
            .clang_arg(format!("--target={}", "x86_64-pc-windows-gnu"));
    }

    // Special handling for MSVC targets to fix unsigned type generation
    if is_target(OperatingSystem::Windows)? {
        // Try to auto-detect MSVC paths
        let (msvc_include, ucrt_include) = find_msvc_paths()?;

        if let Some(msvc_path) = msvc_include {
            bindings = bindings.clang_arg(format!("-I{}", msvc_path));
        } else {
            eprintln!("Warning: Could not auto-detect MSVC include path, using fallback");
            bindings = bindings.clang_arg("-IC:\\Program Files\\Microsoft Visual Studio\\2022\\Enterprise\\VC\\Tools\\MSVC\\14.40.33807\\include");
        }

        if let Some(ucrt_path) = ucrt_include {
            bindings = bindings.clang_arg(format!("-I{}", ucrt_path));
        } else {
            eprintln!("Warning: Could not auto-detect Windows SDK UCRT path, using fallback");
            bindings = bindings.clang_arg(
                "-IC:\\Program Files (x86)\\Windows Kits\\10\\Include\\10.0.17763.0\\ucrt",
            );
        }

        bindings = bindings
            // Force unsigned types on MSVC
            .clang_arg("-DUINT32=unsigned int")
            .clang_arg("-DULONG=unsigned long")
            // Ensure proper type detection
            .clang_arg("-D_WIN32_WINNT=0x0601")
            .blocklist_type("c_uint")
            .blocklist_type("c_ulong");
    }

    let out_dir = env::var("OUT_DIR")?;
    let mut final_bindings = bindings
        .allowlist_type("CBL.*")
        .allowlist_type("FL.*")
        .allowlist_var("k?CBL.*")
        .allowlist_var("k?FL.*")
        .allowlist_function("CBL.*")
        .allowlist_function("_?FL.*")
        .no_copy("FLSliceResult")
        .size_t_is_usize(true)
        // Fix for MSVC: Force unsigned types to be generated as u32 instead of i32
        .default_enum_style(bindgen::EnumVariation::Consts)
        // Force constants to be generated as const u32 instead of complex enums
        .translate_enum_integer_types(true);

    // Add MSVC-specific type fixes
    if is_target(OperatingSystem::Windows)? {
        final_bindings = final_bindings
            .raw_line("#[allow(non_camel_case_types)]")
            .raw_line("pub type c_uint = u32;")
            .raw_line("pub type c_ulong = u32;")
            .raw_line("pub type DWORD = u32;")
            .raw_line("pub type UINT = u32;")
            .raw_line("pub type ULONG = u32;");
    }

    final_bindings
        .parse_callbacks(Box::new(MSVCTypeCallback))
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file(PathBuf::from(out_dir).join("bindings.rs"))
        .expect("Couldn't write bindings!");

    Ok(())
}

fn configure_rustc() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=src/wrapper.h");
    println!("cargo:rerun-if-changed={}", CBL_INCLUDE_DIR);
    println!("cargo:rerun-if-changed={}", CBL_LIB_DIR);
    println!("cargo:rustc-link-search={}", env::var("OUT_DIR")?);

    let target_dir = env::var("TARGET")?;
    let target_os = env::var("CARGO_CFG_TARGET_OS")?;
    if target_os == "ios" {
        let target_arch = env::var("CARGO_CFG_TARGET_ARCH").expect("Can't read target_arch");
        let ios_framework = match target_arch.as_str() {
            "aarch64" => "ios-arm64",
            "x86_64" => "ios-arm64_x86_64-simulator",
            _ => panic!("Unsupported ios target"),
        };

        println!(
            "cargo:rustc-link-search=framework={}/{}/ios/CouchbaseLite.xcframework/{}",
            env!("CARGO_MANIFEST_DIR"),
            CBL_LIB_DIR,
            ios_framework,
        );
        println!("cargo:rustc-link-lib=framework=CouchbaseLite");
    } else if target_os == "macos" {
        println!("cargo:rustc-link-lib=dylib=cblite");
        println!(
            "cargo:rustc-link-search={}/{}/macos",
            env!("CARGO_MANIFEST_DIR"),
            CBL_LIB_DIR
        );
    } else if target_os == "windows" {
        println!("cargo:rustc-link-lib=dylib=cblite");
        println!(
            "cargo:rustc-link-search={}/{}/windows",
            env!("CARGO_MANIFEST_DIR"),
            CBL_LIB_DIR
        );
    } else {
        println!("cargo:rustc-link-lib=dylib=cblite");
        println!(
            "cargo:rustc-link-search={}/{}/{}",
            env!("CARGO_MANIFEST_DIR"),
            CBL_LIB_DIR,
            target_dir
        );
    }
    Ok(())
}

pub fn copy_lib() -> Result<(), Box<dyn Error>> {
    let target_os = env::var("CARGO_CFG_TARGET_OS")?;
    let lib_path = PathBuf::from(format!(
        "{}/{}/{}/",
        env!("CARGO_MANIFEST_DIR"),
        CBL_LIB_DIR,
        if target_os == "ios" || target_os == "macos" || target_os == "windows" {
            target_os.clone()
        } else {
            env::var("TARGET").unwrap()
        }
    ));
    let dest_path = PathBuf::from(format!("{}/", env::var("OUT_DIR")?));

    match target_os.as_str() {
        "android" => {
            fs::copy(
                lib_path.join("libcblite.stripped.so"),
                dest_path.join("libcblite.so"),
            )?;
        }
        "ios" => {
            let mut copy_options = dir::CopyOptions::new();
            copy_options.overwrite = true;

            dir::copy(
                lib_path.join("CouchbaseLite.xcframework"),
                dest_path,
                &copy_options,
            )?;
        }
        "linux" => {
            fs::copy(
                lib_path.join("libcblite.so.3"),
                dest_path.join("libcblite.so.3"),
            )?;
            fs::copy(
                lib_path.join("libicudata.so.66"),
                dest_path.join("libicudata.so.66"),
            )?;
            fs::copy(
                lib_path.join("libicui18n.so.66"),
                dest_path.join("libicui18n.so.66"),
            )?;
            fs::copy(
                lib_path.join("libicuio.so.66"),
                dest_path.join("libicuio.so.66"),
            )?;
            fs::copy(
                lib_path.join("libicutu.so.66"),
                dest_path.join("libicutu.so.66"),
            )?;
            fs::copy(
                lib_path.join("libicuuc.so.66"),
                dest_path.join("libicuuc.so.66"),
            )?;
            // Needed only for build, not required for run
            fs::copy(
                lib_path.join("libcblite.so.3"),
                dest_path.join("libcblite.so"),
            )?;
        }
        "macos" => {
            fs::copy(
                lib_path.join("libcblite.3.dylib"),
                dest_path.join("libcblite.3.dylib"),
            )?;
            // Needed only for build, not required for run
            fs::copy(
                lib_path.join("libcblite.3.dylib"),
                dest_path.join("libcblite.dylib"),
            )?;
        }
        "windows" => {
            fs::copy(lib_path.join("cblite.dll"), dest_path.join("cblite.dll"))?;
            // Needed only for build, not required for run
            fs::copy(lib_path.join("cblite.lib"), dest_path.join("cblite.lib"))?;
        }
        _ => {
            panic!("Unsupported target: {}", env::var("CARGO_CFG_TARGET_OS")?);
        }
    }

    Ok(())
}
