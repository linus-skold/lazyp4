# Building from source

```powershell
cargo build
```

That is the whole thing. You need Rust, a C++ toolchain and Perl; the build
script fetches everything else.

To install it onto your `PATH`, from the repository root:

```powershell
cargo install --path crates/lazyp4
```

Run that **from the repository root**. Cargo discovers `.cargo/config.toml`
from the working directory rather than from the manifest, and the build needs
the `+crt-static` it sets — see [The one thing the repo
pins](#the-one-thing-the-repo-pins).

## What you need installed

| | |
| --- | --- |
| Rust | stable |
| A C++ toolchain | MSVC on Windows — Rust's `x86_64-pc-windows-msvc` target links the same object format the P4API ships, and MinGW cannot |
| Perl | OpenSSL's `Configure` is a Perl script. Windows: `scoop install perl` |
| `curl` and `tar` | already on Windows 10 and later, macOS and Linux |

NASM is optional. `openssl-src` finds it if it is there and turns on OpenSSL's
assembly routines; without it the build is slower but identical.

## What the build fetches

**The P4API**, from `https://ftp.perforce.com/perforce/r25.1/`, picking the
archive for your target — the `static` (static CRT) `openssl3.5` build on
Windows, the `glibc2.12` build on Linux x86_64.

It is not vendored, for two independent reasons. Its licence does not appear to
permit redistribution — though there is no licence file in the distribution at
all, which is [issue #10](https://github.com/linus-skold/lazyp4/issues/10). And
the Windows distribution alone is 410 MB of static libraries, two of whose
archives are past GitHub's 100 MB per-file limit.

**OpenSSL 3.5**, built from source by `openssl-src`. The P4API references
OpenSSL but does not ship it, so `librpc` leaves the `EVP_*` and `OPENSSL_*`
symbols unresolved on its own. Building it here also settles the CRT question:
`openssl-src` follows the target's `crt-static`, so it cannot disagree with the
`/MT` the P4API was compiled with.

The first build takes a few minutes for the download and a few more for
OpenSSL. Both are cached — the P4API under `%LOCALAPPDATA%\lazyp4\p4api` or
`~/.cache/lazyp4/p4api`, which survives `cargo clean`.

## If you would rather supply them yourself

Any of these turns the matching step off. Put them in the `[env]` table of your
**own** `~/.cargo/config.toml` — `%USERPROFILE%\.cargo\config.toml` on Windows
— so they reach every shell and IDE, or export them.

| Variable | Effect |
| --- | --- |
| `P4API_DIR` | Use this unpacked distribution; download nothing. Must hold `include/p4/clientapi.h` and `lib/` |
| `OPENSSL_LIB_DIR` | Link the static OpenSSL in this directory instead of building one |
| `P4API_URL` | Fetch this archive rather than the one picked for the target |
| `P4API_SHA256` | Refuse the download unless it hashes to this. The build prints the hash it saw, so pinning one is a copy and paste |
| `P4API_CACHE_DIR` | Keep downloads here |

Perforce refreshes these archives in place within a release line, so a pinned
hash will eventually fail on a legitimately newer build. That is why it is
offered rather than required: HTTPS to Perforce's own host is the default trust
anchor.

## The one thing the repo pins

`.cargo/config.toml` sets `+crt-static` on the MSVC target. That is not a
machine setting or a preference — every object in the binary has to agree about
the CRT, and the P4API picks `/MT` — so it has to hold for everyone, and it
cannot live in `build.rs` because a build script cannot set a target feature
for the crate graph.

## Checking the result

```powershell
cargo run -p p4-sys --example info
```

Runs `p4 info` through the native API using the ambient `P4PORT`, `P4USER` and
`P4CLIENT`, exactly as the `p4` binary does. It proves the link, the static CRT
and — on an `ssl:` port — the TLS handshake.

```powershell
cargo run -p p4 --example changes
```

Lists the pending and recently submitted changelists of the current workspace
through the typed command layer.

## Continuous integration

`.github/workflows/ci.yml` builds and tests on Windows, Linux and macOS on every
push. `.github/workflows/release.yml` builds four targets on a `v*` tag and
attaches the archives to a draft release.

Both cache the P4API download. CI also caches `target` whole rather than using
`Swatinem/rust-cache`, because OpenSSL is compiled into `p4-sys`'s `OUT_DIR` and
that action prunes workspace-member build output — losing it would mean
rebuilding OpenSSL on every run.
