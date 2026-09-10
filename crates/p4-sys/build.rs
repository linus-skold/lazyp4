//! Compiles the shim and links the P4API.
//!
//! Left alone this fetches the P4API for the target and builds OpenSSL from
//! source; `P4API_DIR` and `OPENSSL_LIB_DIR` short-circuit either step. Why it
//! fetches rather than vendors, and every variable it reads, are in
//! `docs/building.md`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

/// The P4API release the download URLs point at.
const RELEASE: &str = "r25.1";

const WHERE: &str = "\
Export it, or — so it applies to every shell and IDE — put it in the [env]
table of your own ~/.cargo/config.toml. It is deliberately not in the repo:
where these libraries sit is a property of the machine, not of the project.";

const HELP: &str = "\
P4API_DIR must name an unpacked Helix Core C++ API distribution.

It must contain include/p4/clientapi.h and lib/. Leave the variable unset to
have this build script download the right one for the target instead.";

fn main() {
    for var in [
        "P4API_DIR",
        "P4API_URL",
        "P4API_SHA256",
        "P4API_CACHE_DIR",
        "OPENSSL_LIB_DIR",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    println!("cargo:rerun-if-changed=src/shim.cc");
    println!("cargo:rerun-if-changed=include/shim.h");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let windows = target_os == "windows";

    let root = match env::var("P4API_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => fetch_p4api(),
    };
    let include = root.join("include").join("p4");
    let lib = root.join("lib");
    if !include.join("clientapi.h").is_file() {
        panic!("no clientapi.h under {}\n\n{HELP}\n\n{WHERE}", include.display());
    }

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
    for name in p4_libraries(&lib, windows) {
        println!("cargo:rustc-link-lib=static={name}");
    }

    link_openssl(&target_os);

    if windows {
        for name in [
            "ws2_32", "advapi32", "iphlpapi", "bcrypt", "crypt32", "user32",
            // P4API's runshell.cc opens URLs for SSO/HandleUrl.
            "shell32", "ole32",
        ] {
            println!("cargo:rustc-link-lib=dylib={name}");
        }
    } else if target_os == "linux" {
        for name in ["rt", "dl", "pthread", "m", "stdc++"] {
            println!("cargo:rustc-link-lib=dylib={name}");
        }
    } else if target_os == "macos" {
        println!("cargo:rustc-link-lib=dylib=c++");
        // OpenSSL on macOS reaches for the keychain through these.
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
        println!("cargo:rustc-link-lib=framework=Security");
    }
}

/// The P4API static libraries actually present, in an order the linker accepts.
///
/// Read from the directory rather than hardcoded: the distributions differ in
/// which `libp4script*` pieces they carry, and naming one that is absent fails
/// the link.
fn p4_libraries(lib: &Path, windows: bool) -> Vec<String> {
    // Dependants first. `client` needs `rpc`, which needs `supp`; the script
    // libraries lean on all three.
    const ORDER: [&str; 7] = [
        "libclient",
        "librpc",
        "libsupp",
        "libp4script",
        "libp4script_c",
        "libp4script_curl",
        "libp4script_sqlite",
    ];

    let extension = if windows { "lib" } else { "a" };
    let present: Vec<String> = fs::read_dir(lib)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", lib.display()))
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|e| e == extension))
        .filter_map(|entry| entry.path().file_stem()?.to_str().map(str::to_owned))
        .collect();

    let mut chosen: Vec<String> = ORDER
        .iter()
        .filter(|name| present.iter().any(|p| p == *name))
        .map(|name| (*name).to_owned())
        .collect();
    if chosen.is_empty() {
        panic!("no P4API static libraries in {}", lib.display());
    }
    // `libp4api.lib` is the same objects rolled together; linking both would
    // duplicate every symbol.
    if !windows {
        // Non-Windows distributions use libfoo.a, which the linker spells `foo`.
        chosen = chosen
            .into_iter()
            .map(|n| n.trim_start_matches("lib").to_owned())
            .collect();
    }
    chosen
}

/// Point the linker at OpenSSL, building it from source if need be.
fn link_openssl(target_os: &str) {
    if let Ok(dir) = env::var("OPENSSL_LIB_DIR") {
        let dir = PathBuf::from(dir);
        if !dir.is_dir() {
            panic!("OPENSSL_LIB_DIR {} is not a directory\n\n{WHERE}", dir.display());
        }
        println!("cargo:rustc-link-search=native={}", dir.display());
        // Windows OpenSSL names its static archives *_static to distinguish
        // them from the import libraries sitting beside them.
        let (ssl, crypto) = if target_os == "windows" {
            ("libssl_static", "libcrypto_static")
        } else {
            ("ssl", "crypto")
        };
        println!("cargo:rustc-link-lib=static={ssl}");
        println!("cargo:rustc-link-lib=static={crypto}");
        return;
    }

    // Nothing to point at, so build one. The P4API archive references OpenSSL
    // but does not ship it, and the build has to agree with it about the CRT —
    // which building it here guarantees, since openssl-src follows the target's
    // crt-static setting.
    println!("cargo:warning=OPENSSL_LIB_DIR unset; building OpenSSL from source (first build only)");
    let artifacts = openssl_src::Build::new().build();
    println!(
        "cargo:rustc-link-search=native={}",
        artifacts.lib_dir().display()
    );
    for lib in artifacts.libs() {
        println!("cargo:rustc-link-lib=static={lib}");
    }
}

/// Download and unpack the P4API for the target, caching it between builds.
fn fetch_p4api() -> PathBuf {
    let (dir, file) = distribution();
    let url = env::var("P4API_URL")
        .unwrap_or_else(|_| format!("https://ftp.perforce.com/perforce/{RELEASE}/{dir}/{file}"));

    let cache = cache_dir().join(format!("{RELEASE}-{dir}"));
    let unpacked = cache.join("unpacked");
    // The marker records that an extraction finished, so a run killed halfway
    // is retried rather than half-used.
    let marker = cache.join("complete");
    if marker.is_file() {
        if let Some(root) = api_root(&unpacked) {
            return root;
        }
    }

    fs::create_dir_all(&cache).unwrap_or_else(|e| panic!("cannot make {}: {e}", cache.display()));
    let archive = cache.join(&file);
    if !archive.is_file() {
        println!("cargo:warning=downloading the P4API from {url}");
        download(&url, &archive);
    }
    verify(&archive);

    let _ = fs::remove_dir_all(&unpacked);
    fs::create_dir_all(&unpacked).unwrap();
    // bsdtar ships with Windows 10 and later and reads zip as well as tar, so
    // one tool covers every platform and nothing has to be installed.
    let status = Command::new("tar")
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(&unpacked)
        .status()
        .unwrap_or_else(|e| panic!("cannot run tar: {e}\n\nInstall tar, or set P4API_DIR."));
    if !status.success() {
        panic!("tar failed to unpack {}", archive.display());
    }

    let root = api_root(&unpacked).unwrap_or_else(|| {
        panic!(
            "no include/p4/clientapi.h anywhere under {}",
            unpacked.display()
        )
    });
    fs::write(&marker, url.as_bytes()).ok();
    root
}

/// Which archive this target needs, as `(platform directory, file name)`.
fn distribution() -> (&'static str, &'static str) {
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    match (os.as_str(), arch.as_str()) {
        // "static" is the static CRT; the openssl3.5 suffix matches the
        // openssl-src series pinned in Cargo.toml.
        ("windows", "x86_64") => ("bin.ntx64", "p4api_vs2022_static_openssl3.5.zip"),
        // The glibc2.12 build is the one carrying the p4script libraries.
        ("linux", "x86_64") => ("bin.linux26x86_64", "p4api-glibc2.12-openssl3.5.tgz"),
        ("linux", "aarch64") => ("bin.linux26aarch64", "p4api-openssl3.5.tgz"),
        ("macos", "x86_64") => ("bin.macosx12x86_64", "p4api-openssl3.5.tgz"),
        ("macos", "aarch64") => ("bin.macosx12arm64", "p4api-openssl3.5.tgz"),
        _ => panic!(
            "no P4API download is wired up for {os}/{arch}.\n\n\
             Unpack one from https://ftp.perforce.com/perforce/{RELEASE}/ and set P4API_DIR,\n\
             or set P4API_URL to the archive for this target.\n\n{WHERE}"
        ),
    }
}

/// Where downloads are kept, so `cargo clean` does not cost another 74 MB.
fn cache_dir() -> PathBuf {
    if let Ok(dir) = env::var("P4API_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    let base = env::var("LOCALAPPDATA")
        .or_else(|_| env::var("XDG_CACHE_HOME"))
        .map(PathBuf::from)
        .or_else(|_| env::var("HOME").map(|h| PathBuf::from(h).join(".cache")))
        // Nowhere better to put it; OUT_DIR at least always exists.
        .unwrap_or_else(|_| PathBuf::from(env::var("OUT_DIR").unwrap()));
    base.join("lazyp4").join("p4api")
}

fn download(url: &str, to: &Path) {
    // curl ships with Windows 10 and later, macOS and every CI image, so this
    // needs no HTTP crate in the build graph. HTTPS to Perforce's own host is
    // what makes the download trustworthy; P4API_SHA256 pins it further.
    let partial = to.with_extension("partial");
    let status = Command::new("curl")
        .args(["-fSL", "--retry", "3", "--retry-delay", "2", "-o"])
        .arg(&partial)
        .arg(url)
        .status()
        .unwrap_or_else(|e| panic!("cannot run curl: {e}\n\nInstall curl, or set P4API_DIR."));
    if !status.success() {
        let _ = fs::remove_file(&partial);
        panic!("could not download {url}\n\nSet P4API_DIR to an unpacked distribution instead.");
    }
    // Renamed only once it is whole, so an interrupted download is never
    // mistaken for a cached one.
    fs::rename(&partial, to).unwrap_or_else(|e| panic!("cannot finish {}: {e}", to.display()));
}

/// Check the archive against `P4API_SHA256` when one is given.
///
/// Perforce refreshes these files in place within a release line, so a pinned
/// hash would eventually fail on its own. It is offered rather than required,
/// and the hash is printed so pinning one is a copy and paste.
fn verify(archive: &Path) {
    use sha2::{Digest, Sha256};

    let expected = env::var("P4API_SHA256").ok();
    if expected.is_none() && env::var("CI").is_err() {
        return;
    }
    let bytes = fs::read(archive).unwrap_or_else(|e| panic!("cannot read {}: {e}", archive.display()));
    let actual = format!("{:x}", Sha256::digest(&bytes));

    match expected {
        Some(want) if want.trim().eq_ignore_ascii_case(&actual) => {}
        Some(want) => {
            let _ = fs::remove_file(archive);
            panic!(
                "P4API_SHA256 does not match the download.\n  expected {}\n  got      {actual}\n\n\
                 Perforce refreshes these archives in place, so this may simply be a newer\n\
                 build. Check it, then update P4API_SHA256.",
                want.trim()
            );
        }
        None => println!("cargo:warning=P4API sha256 {actual} (set P4API_SHA256 to pin it)"),
    }
}

/// The directory holding `include/p4/clientapi.h`, at or one level below `dir`.
fn api_root(dir: &Path) -> Option<PathBuf> {
    let has_header = |d: &Path| d.join("include").join("p4").join("clientapi.h").is_file();
    if has_header(dir) {
        return Some(dir.to_path_buf());
    }
    fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.is_dir() && has_header(p))
}
