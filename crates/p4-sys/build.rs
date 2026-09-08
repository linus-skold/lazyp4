use std::env;
use std::path::PathBuf;

const HELP: &str = "\
set P4API_DIR to an unpacked Helix Core C++ API distribution.

It must contain include/p4/clientapi.h and lib/. Download one from
https://ftp.perforce.com/perforce/ under <release>/bin.<platform>/ — on Windows
pick a `static` build (static CRT) with an openssl3 suffix, e.g.
p4api_vs2022_static_openssl3.zip. The library is not vendored because its
licence does not allow redistribution.";

const SSL_HELP: &str = "\
set OPENSSL_LIB_DIR to a directory holding OpenSSL 3 static libraries.

The P4API archive references OpenSSL but does not ship it, so librpc leaves
EVP_*/OPENSSL_* symbols unresolved without it. The build must match the P4API
one: on Windows that is the /MT (static CRT) variant, e.g. the
lib\\VC\\x64\\MT directory of a scoop or vcpkg OpenSSL 3 install.";

fn main() {
    println!("cargo:rerun-if-env-changed=P4API_DIR");
    println!("cargo:rerun-if-env-changed=OPENSSL_LIB_DIR");
    println!("cargo:rerun-if-changed=src/shim.cc");
    println!("cargo:rerun-if-changed=include/shim.h");

    let root = PathBuf::from(env::var("P4API_DIR").unwrap_or_else(|_| panic!("{HELP}")));
    let include = root.join("include").join("p4");
    let lib = root.join("lib");
    if !include.join("clientapi.h").is_file() {
        panic!("no clientapi.h under {}\n\n{HELP}", include.display());
    }

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let windows = target_os == "windows";

    let mut build = cxx_build::bridge("src/lib.rs");
    build
        .file("src/shim.cc")
        .include(&include)
        .include(".")
        .std("c++17");

    if windows {
        build.define("OS_NT", None).define("CASE_INSENSITIVE", None);
        // Must match the /MT of the P4API static distribution and the
        // +crt-static in .cargo/config.toml.
        build.static_crt(true);
    } else if target_os == "linux" {
        build.define("OS_LINUX", None);
    } else if target_os == "macos" {
        build.define("OS_MACOSX", None);
    }

    build.compile("p4shim");

    println!("cargo:rustc-link-search=native={}", lib.display());
    // Link order matters: client before rpc before supp.
    for name in [
        "libclient",
        "librpc",
        "libsupp",
        "libp4script",
        "libp4script_c",
        "libp4script_curl",
        "libp4script_sqlite",
    ] {
        let name = if windows {
            name.to_string()
        } else {
            // Non-Windows distributions use libfoo.a, which the linker spells `foo`.
            name.trim_start_matches("lib").to_string()
        };
        println!("cargo:rustc-link-lib=static={name}");
    }

    let ssl_dir = PathBuf::from(env::var("OPENSSL_LIB_DIR").unwrap_or_else(|_| panic!("{SSL_HELP}")));
    if !ssl_dir.is_dir() {
        panic!("{} is not a directory\n\n{SSL_HELP}", ssl_dir.display());
    }
    println!("cargo:rustc-link-search=native={}", ssl_dir.display());
    // Windows OpenSSL names its static archives *_static to distinguish them
    // from the import libraries sitting beside them.
    let (ssl, crypto) = if windows {
        ("libssl_static", "libcrypto_static")
    } else {
        ("ssl", "crypto")
    };
    println!("cargo:rustc-link-lib=static={ssl}");
    println!("cargo:rustc-link-lib=static={crypto}");

    if windows {
        for name in [
            "ws2_32", "advapi32", "iphlpapi", "bcrypt", "crypt32", "user32",
            // P4API's runshell.cc opens URLs for SSO/HandleUrl.
            "shell32", "ole32",
        ] {
            println!("cargo:rustc-link-lib=dylib={name}");
        }
    } else if target_os == "linux" {
        println!("cargo:rustc-link-lib=dylib=rt");
        println!("cargo:rustc-link-lib=dylib=dl");
    }
}
