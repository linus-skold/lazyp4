//! Self-update: the `--update` command and the "update available" banner.
//!
//! The decisions live in plain functions that take their input as arguments,
//! so the tests need no network and no real install. Only `run`,
//! `fetch_latest` and `download` touch the network. The banner never changes
//! the binary: only `--update` does. See `docs/usage.md`.

use std::fmt;
use std::fs;
use std::io::{self, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use crate::worker::Event;

const REPO: &str = "linus-skold/lazyp4";
const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// One network check a day is enough for a banner.
const CHECK_EVERY: u64 = 24 * 60 * 60;

const MAX_JSON: u64 = 1024 * 1024;
const MAX_SUMS: u64 = 1024 * 1024;
const MAX_ARCHIVE: u64 = 100 * 1024 * 1024;
const MAX_BINARY: u64 = 200 * 1024 * 1024;

// -- versions ---------------------------------------------------------------

/// A release version. Field order makes the derived order numeric, so 0.10.0
/// is above 0.9.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u64, pub u64, pub u64);

impl Version {
    /// `1.2.3` or `v1.2.3`, and nothing else. A prerelease or build suffix
    /// does not parse, so those tags never reach a user.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.');
        let mut next = || {
            let part = parts.next()?;
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            part.parse::<u64>().ok()
        };
        let version = Version(next()?, next()?, next()?);
        parts.next().is_none().then_some(version)
    }

    pub fn current() -> Version {
        Version::parse(CURRENT).unwrap_or(Version(0, 0, 0))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// The version in a `releases/latest` answer. A draft, a prerelease or a tag
/// that does not parse is an error.
pub fn parse_latest(json: &str) -> Result<Version, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|_| "the release data is not valid".to_owned())?;
    if value["draft"].as_bool() != Some(false) || value["prerelease"].as_bool() != Some(false) {
        return Err("the latest release is a draft or a prerelease".to_owned());
    }
    value["tag_name"]
        .as_str()
        .and_then(Version::parse)
        .ok_or_else(|| "the release tag is not a version".to_owned())
}

// -- checksums and assets ---------------------------------------------------

/// The hash that `SHA256SUMS` lists for `name`. A line is `hash  name`, or
/// `hash *name` for a binary-mode entry.
pub fn parse_sums(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        let file = file.trim_start().trim_start_matches('*');
        let valid = hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit());
        (file == name && valid).then(|| hash.to_ascii_lowercase())
    })
}

fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// Make sure `data` is the file that `SHA256SUMS` lists under `name`.
pub fn check_sum(data: &[u8], sums: &str, name: &str) -> Result<(), String> {
    let expected =
        parse_sums(sums, name).ok_or_else(|| format!("SHA256SUMS has no entry for {name}"))?;
    if sha256_hex(data) != expected {
        return Err(format!("the checksum of {name} is wrong. Nothing was changed."));
    }
    Ok(())
}

/// Which release archive suits this platform, and the binary inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asset {
    pub archive: &'static str,
    pub binary: &'static str,
    pub zip: bool,
}

/// Same choice as `scripts/install.sh` and `scripts/install.ps1`. ARM64
/// Windows runs the x86_64 binary under emulation.
pub fn asset_for(os: &str, arch: &str) -> Option<Asset> {
    match (os, arch) {
        ("windows", "x86_64" | "aarch64") => Some(Asset {
            archive: "lazyp4-windows-x86_64.zip",
            binary: "lazyp4.exe",
            zip: true,
        }),
        ("linux", "x86_64") => Some(Asset {
            archive: "lazyp4-linux-x86_64.tar.gz",
            binary: "lazyp4",
            zip: false,
        }),
        ("macos", "aarch64") => Some(Asset {
            archive: "lazyp4-macos-arm64.tar.gz",
            binary: "lazyp4",
            zip: false,
        }),
        _ => None,
    }
}

// -- archives ---------------------------------------------------------------

/// The release tarballs are made from `./*`, so the entry may carry a `./`.
/// Nothing else is allowed to differ from the wanted name.
fn is_entry(path: &str, binary: &str) -> bool {
    let mut path = path;
    while let Some(rest) = path.strip_prefix("./") {
        path = rest;
    }
    path == binary
}

fn read_limited(reader: impl Read, limit: u64) -> Result<Vec<u8>, String> {
    let mut data = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut data)
        .map_err(|e| format!("could not unpack the archive: {e}"))?;
    if data.len() as u64 > limit {
        return Err("the binary in the archive is too large".to_owned());
    }
    Ok(data)
}

/// The bytes of the one wanted file. The archive never chooses a path to
/// write: the content goes to memory, and the caller picks the place.
pub fn extract(archive: &[u8], asset: &Asset) -> Result<Vec<u8>, String> {
    let missing = || format!("the archive has no {}", asset.binary);
    if asset.zip {
        let mut zip = zip::ZipArchive::new(Cursor::new(archive))
            .map_err(|e| format!("could not open the archive: {e}"))?;
        for i in 0..zip.len() {
            let file = zip
                .by_index(i)
                .map_err(|e| format!("could not read the archive: {e}"))?;
            if file.is_file() && file.name().is_ok_and(|n| is_entry(&n, asset.binary)) {
                return read_limited(file, MAX_BINARY);
            }
        }
    } else {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
        let entries = tar
            .entries()
            .map_err(|e| format!("could not open the archive: {e}"))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("could not read the archive: {e}"))?;
            let path = entry.path().map_err(|e| format!("bad archive entry: {e}"))?;
            let wanted = entry.header().entry_type().is_file()
                && path.to_str().is_some_and(|p| is_entry(p, asset.binary));
            drop(path);
            if wanted {
                return read_limited(entry, MAX_BINARY);
            }
        }
    }
    Err(missing())
}

// -- install kind -----------------------------------------------------------

/// Who put this binary here, and so who may replace it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Install {
    /// The release archive or the install script. `--update` may replace it.
    Direct,
    /// A `.deb` or `.rpm`, which installs `/usr/bin/lazyp4`.
    Package,
    Nix,
    Cargo,
    /// A binary in a cargo `target` directory.
    DevBuild,
    /// A direct install on a platform that has no release archive.
    NoAsset,
}

/// The directories `cargo install` writes to: the install root, then
/// `CARGO_HOME`, then the home directory's `.cargo`. All of them count, so a
/// binary from any earlier setting is still seen as cargo's. Cargo takes the
/// home directory from `USERPROFILE` on Windows and from `HOME` elsewhere
/// (from the `home` crate's documented rule; not checked against cargo itself).
pub fn cargo_bin_dirs(var: impl Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(root) = var("CARGO_INSTALL_ROOT") {
        dirs.push(PathBuf::from(root).join("bin"));
    }
    if let Some(home) = var("CARGO_HOME") {
        dirs.push(PathBuf::from(home).join("bin"));
    }
    let user = if cfg!(windows) {
        var("USERPROFILE").or_else(|| var("HOME"))
    } else {
        var("HOME")
    };
    if let Some(user) = user {
        dirs.push(PathBuf::from(user).join(".cargo").join("bin"));
    }
    dirs
}

/// What `classify` needs to know about the machine.
pub struct Probe<'a> {
    pub cargo_bins: &'a [PathBuf],
    /// `CARGO_TARGET_DIR`, for a build output outside a `target` directory.
    pub target_dir: Option<&'a Path>,
    pub exists: &'a dyn Fn(&Path) -> bool,
}

/// Classify a resolved executable path (symlinks already followed).
///
/// Limit: a developer build in a target directory with another name, and not
/// set in `CARGO_TARGET_DIR` (for example in `.cargo/config.toml`), looks like
/// a direct install.
pub fn classify(exe: &Path, probe: &Probe) -> Install {
    if exe.starts_with("/nix/store") {
        return Install::Nix;
    }
    let names: Vec<_> = exe.components().map(|c| c.as_os_str()).collect();
    if let Some(at) = names.iter().position(|n| *n == "target") {
        if names[at + 1..].iter().any(|n| *n == "debug" || *n == "release") {
            return Install::DevBuild;
        }
    }
    if probe.target_dir.is_some_and(|t| exe.starts_with(t)) {
        return Install::DevBuild;
    }
    let dir = exe.parent();
    // `cargo install` records what it installed next to the bin directory.
    let recorded = dir.and_then(Path::parent).is_some_and(|root| {
        [".crates.toml", ".crates2.json"].iter().any(|f| (probe.exists)(&root.join(f)))
    });
    if recorded || dir.is_some_and(|d| probe.cargo_bins.iter().any(|c| d == c.as_path())) {
        return Install::Cargo;
    }
    if dir == Some(Path::new("/usr/bin")) {
        return Install::Package;
    }
    Install::Direct
}

impl Install {
    /// Short text for the banner.
    pub fn hint(self) -> &'static str {
        match self {
            Install::Direct | Install::DevBuild => "run lazyp4 --update",
            Install::Package => "use the package manager",
            Install::Nix => "update with Nix",
            Install::Cargo => "run cargo install again",
            Install::NoAsset => "build from source",
        }
    }

    /// What `--update` says for an install it must not change.
    fn guidance(self) -> &'static str {
        match self {
            Install::Direct => "",
            Install::Package => {
                "This copy came from a .deb or .rpm package. Use your package manager to install \
                 the newer package from the GitHub releases page."
            }
            Install::Nix => {
                "This copy is in the Nix store. Update it with Nix, the same way you installed it."
            }
            Install::Cargo => {
                "This copy came from cargo install. Update the source, then run: \
                 cargo install --path crates/lazyp4"
            }
            Install::DevBuild => {
                "This is a developer build in a cargo target directory. Build the source again \
                 with cargo build."
            }
            Install::NoAsset => {
                "There is no release for this platform. To build from source, see docs/building.md."
            }
        }
    }
}

/// `--update` changes only a direct install. Any other kind is a refusal, and
/// the error carries what to do instead.
pub fn require_direct(kind: Install) -> Result<(), String> {
    match kind {
        Install::Direct => Ok(()),
        other => Err(format!("--update changed nothing. {}", other.guidance())),
    }
}

/// True if `--update` is among the arguments. Compared as `OsString`, so a
/// non-Unicode argument cannot panic.
pub fn wants_update(args: impl IntoIterator<Item = std::ffi::OsString>) -> bool {
    args.into_iter().any(|a| a == "--update")
}

/// The running binary with symlinks resolved, and what kind of install it is.
fn current_install() -> Result<(PathBuf, Install), String> {
    let exe = std::env::current_exe()
        .and_then(fs::canonicalize)
        .map_err(|e| format!("could not find the running binary: {e}"))?;
    let cargo: Vec<PathBuf> = cargo_bin_dirs(|k| std::env::var(k).ok())
        .into_iter()
        .map(|d| fs::canonicalize(&d).unwrap_or(d))
        .collect();
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .and_then(|d| fs::canonicalize(d).ok());
    let exists = |p: &Path| p.exists();
    let probe = Probe { cargo_bins: &cargo, target_dir: target.as_deref(), exists: &exists };
    let mut kind = classify(&exe, &probe);
    if kind == Install::Direct && asset_for(std::env::consts::OS, std::env::consts::ARCH).is_none() {
        kind = Install::NoAsset;
    }
    Ok((exe, kind))
}

// -- replacing the binary ---------------------------------------------------

fn write_new(path: &Path, data: &[u8]) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o755);
    let mut file = options.open(path)?;
    file.write_all(data)?;
    file.sync_all()
}

/// The rename step is a parameter so that tests can make it fail.
type Rename<'a> = &'a dyn Fn(&Path, &Path) -> io::Result<()>;

enum SwapError {
    /// The old binary is still in place.
    Failed(io::Error),
    /// The old binary is at `old` and could not be moved back.
    Stranded { old: PathBuf, error: io::Error },
}

fn swap(exe: &Path, new: &Path, windows: bool, rename: Rename) -> Result<(), SwapError> {
    if !windows {
        // A rename replaces a running binary; a copy onto it can fail.
        return rename(new, exe).map_err(SwapError::Failed);
    }
    // Windows locks a running exe against writes but not against a rename.
    // A `.old` that cannot be removed (an older lazyp4 may still run from it)
    // must not stop the update, so the next name carries the process id.
    let mut old = PathBuf::from(format!("{}.old", exe.display()));
    if fs::remove_file(&old).is_err() && old.exists() {
        old = PathBuf::from(format!("{}.old.{}", exe.display(), std::process::id()));
        let _ = fs::remove_file(&old);
    }
    rename(exe, &old).map_err(SwapError::Failed)?;
    if let Err(error) = rename(new, exe) {
        return Err(match rename(&old, exe) {
            Ok(()) => SwapError::Failed(error),
            Err(_) => SwapError::Stranded { old, error },
        });
    }
    // Fails while this process runs from it; the next update removes it.
    let _ = fs::remove_file(&old);
    Ok(())
}

/// Remove what an earlier update left in the directory: `.old` files and
/// temp files. Errors are ignored: a file that a running lazyp4 holds goes
/// on the next run.
pub fn clean_leftovers(exe: &Path) {
    let (Some(dir), Some(name)) = (exe.parent(), exe.file_name().and_then(|n| n.to_str())) else {
        return;
    };
    let old = format!("{name}.old");
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let file = entry.file_name();
        let Some(file) = file.to_str() else { continue };
        let stale = file == old
            || file.strip_prefix(&old).is_some_and(|rest| rest.starts_with('.'))
            || file.starts_with(".lazyp4.new.");
        if stale && entry.file_type().is_ok_and(|t| t.is_file()) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Put `data` in place of `exe`.
pub fn replace_binary(exe: &Path, data: &[u8]) -> Result<(), String> {
    replace_with(exe, data, cfg!(windows), &|from, to| fs::rename(from, to))
}

/// The temp file is in the same directory so that the rename cannot cross a
/// file system. On failure the old binary is in place and the temp file is
/// gone. The one exception is a Windows swap that cannot undo itself: then
/// the message names the `.old` file to move back.
pub fn replace_with(exe: &Path, data: &[u8], windows: bool, rename: Rename) -> Result<(), String> {
    let dir = exe.parent().ok_or("the binary has no directory")?;
    let temp = dir.join(format!(".lazyp4.new.{}", std::process::id()));
    let denied = |e: &io::Error| {
        (e.kind() == io::ErrorKind::PermissionDenied).then(|| {
            format!(
                "cannot write to {}. Run the update as a user who can write there, or install \
                 lazyp4 in a directory you own.",
                dir.display()
            )
        })
    };
    if let Err(e) = write_new(&temp, data) {
        // Never delete a file that this call did not create.
        if e.kind() != io::ErrorKind::AlreadyExists {
            let _ = fs::remove_file(&temp);
        }
        return Err(denied(&e).unwrap_or_else(|| format!("could not write {}: {e}", temp.display())));
    }
    match swap(exe, &temp, windows, rename) {
        Ok(()) => Ok(()),
        Err(SwapError::Failed(e)) => {
            let _ = fs::remove_file(&temp);
            Err(denied(&e).unwrap_or_else(|| format!("could not replace {}: {e}", exe.display())))
        }
        Err(SwapError::Stranded { old, error }) => Err(format!(
            "could not replace {}: {error}. The old binary is now {}. To recover, rename it back to {}. \
             The new binary is {}.",
            exe.display(),
            old.display(),
            exe.display(),
            temp.display()
        )),
    }
}
// -- network ----------------------------------------------------------------

enum Failure {
    NoRelease,
    Other(String),
}

/// `body_secs` limits the whole body, not the gap between reads, so a large
/// archive needs a long one and the banner check a short one.
fn agent(body_secs: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        // Redirects count: a redirect to plain HTTP is refused too.
        .https_only(true)
        .max_redirects(5)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(20)))
        .timeout_recv_body(Some(Duration::from_secs(body_secs)))
        .timeout_global(Some(Duration::from_secs(body_secs + 30)))
        // Name and version only. Nothing about the user or the server.
        .user_agent(format!("lazyp4/{CURRENT}"))
        .build()
        .into()
}

fn get(url: &str, limit: u64, body_secs: u64) -> Result<Vec<u8>, Failure> {
    let mut response = agent(body_secs).get(url).call().map_err(|e| match e {
        ureq::Error::StatusCode(404) => Failure::NoRelease,
        e => Failure::Other(format!("request failed: {e}")),
    })?;
    response
        .body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()
        .map_err(|e| Failure::Other(format!("download failed: {e}")))
}

fn fetch_latest(body_secs: u64) -> Result<Version, Failure> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let body = get(&url, MAX_JSON, body_secs)?;
    let text = String::from_utf8(body).map_err(|_| Failure::Other("bad release data".into()))?;
    parse_latest(&text).map_err(Failure::Other)
}

// -- --update ---------------------------------------------------------------

/// The `--update` command. Returns the exit code.
pub fn run() -> i32 {
    match update() {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("lazyp4: {message}");
            1
        }
    }
}

fn update() -> Result<(), String> {
    println!("current version: {CURRENT}");
    let (exe, kind) = current_install()?;
    require_direct(kind)?;
    let asset = asset_for(std::env::consts::OS, std::env::consts::ARCH)
        .ok_or_else(|| Install::NoAsset.guidance().to_owned())?;
    // Before the version check, so that a stale file never blocks a later update.
    clean_leftovers(&exe);

    let latest = fetch_latest(30).map_err(|f| match f {
        Failure::NoRelease => "there is no published release yet.".to_owned(),
        Failure::Other(e) => e,
    })?;
    println!("latest version: {latest}");
    if latest <= Version::current() {
        println!("lazyp4 is up to date. Nothing changed.");
        return Ok(());
    }

    let base = format!("https://github.com/{REPO}/releases/download/v{latest}");
    let fetch = |name: &str, limit| {
        get(&format!("{base}/{name}"), limit, 300).map_err(|f| match f {
            Failure::NoRelease => format!("the release has no {name}"),
            Failure::Other(e) => e,
        })
    };
    println!("downloading {}", asset.archive);
    let archive = fetch(asset.archive, MAX_ARCHIVE)?;
    let sums = fetch("SHA256SUMS", MAX_SUMS)?;

    check_sum(&archive, &String::from_utf8_lossy(&sums), asset.archive)?;

    let binary = extract(&archive, &asset)?;
    replace_binary(&exe, &binary)?;
    println!("updated {} to {latest}", exe.display());
    Ok(())
}

// -- banner -----------------------------------------------------------------

/// What the banner says: a newer version, and how to get it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub version: Version,
    pub hint: &'static str,
}

/// The cache file: when the last check ran, and the newest version it saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cache {
    pub checked: u64,
    pub latest: Option<Version>,
}

impl Cache {
    /// `None` for anything unreadable, which then counts as no cache.
    pub fn parse(text: &str) -> Option<Cache> {
        let (mut checked, mut latest) = (None, None);
        for line in text.lines() {
            match line.split_once('=')? {
                ("checked", v) => checked = Some(v.parse().ok()?),
                ("latest", v) => latest = Some(Version::parse(v)?),
                _ => return None,
            }
        }
        Some(Cache { checked: checked?, latest })
    }

    pub fn render(&self) -> String {
        let mut text = format!("checked={}\n", self.checked);
        if let Some(v) = self.latest {
            text.push_str(&format!("latest={v}\n"));
        }
        text
    }
}

/// A clock that went backwards must not stop the check for good.
pub fn check_due(cache: Option<&Cache>, now: u64) -> bool {
    cache.is_none_or(|c| now < c.checked || now - c.checked >= CHECK_EVERY)
}

/// Only a strictly newer version gives a notice. A developer build gets none.
pub fn notice(latest: Option<Version>, current: Version, kind: Install) -> Option<Notice> {
    let version = latest.filter(|v| *v > current)?;
    (kind != Install::DevBuild).then_some(Notice { version, hint: kind.hint() })
}

/// Where the cache lives: the platform's cache directory.
pub fn cache_path(var: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    // A relative path would put the file in whatever directory lazyp4 starts
    // in, which can be a Perforce workspace.
    let absolute = |name: &str| var(name).map(PathBuf::from).filter(|p| p.is_absolute());
    let dir = absolute("LOCALAPPDATA")
        .or_else(|| absolute("XDG_CACHE_HOME"))
        .or_else(|| absolute("HOME").map(|h| h.join(".cache")))?;
    Some(dir.join("lazyp4").join("update-check"))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The whole banner decision. `fetch` is the only network call, and it runs
/// only when the check is enabled and the cache is a day old or missing. The
/// stamp is written before the fetch. If it cannot be written, there is no
/// fetch. So there is at most one request a day, also when the user quits
/// during the fetch or the cache directory is read-only.
pub fn check(
    enabled: bool,
    cache_file: Option<&Path>,
    now: u64,
    kind: Install,
    fetch: impl FnOnce() -> Option<Version>,
) -> Option<Notice> {
    if !enabled || kind == Install::DevBuild {
        return None;
    }
    let cache = cache_file
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|t| Cache::parse(&t));
    let mut latest = cache.and_then(|c| c.latest);
    if check_due(cache.as_ref(), now) {
        let write = |path: &Path, latest| {
            let _ = path.parent().map(fs::create_dir_all);
            fs::write(path, Cache { checked: now, latest }.render())
        };
        // The cached answer still counts when the stamp cannot be written.
        if let Some(path) = cache_file.filter(|p| write(p, latest).is_ok()) {
            if let Some(found) = fetch() {
                latest = Some(found);
                let _ = write(path, latest);
            }
        }
    }
    notice(latest, Version::current(), kind)
}

/// Check in a thread so that start and drawing never wait for it. Every
/// failure ends the thread without a trace.
pub fn spawn_check(enabled: bool, events: Sender<Event>) {
    if !enabled {
        return;
    }
    std::thread::spawn(move || {
        let Ok((_, kind)) = current_install() else { return };
        let path = cache_path(|k| std::env::var(k).ok());
        let found = check(true, path.as_deref(), now(), kind, || fetch_latest(15).ok());
        if let Some(notice) = found {
            let _ = events.send(Event::Update(notice));
        }
    });
}
