# lazyp4

A terminal UI for Perforce (Helix Core), in the style of lazygit: browse
changelists and their files in panels, and read diffs in the shell.

Status: early. The native P4API binding layer works; the UI is not built yet.

## Build

lazyp4 links the Helix Core C++ API directly. That library is not vendored,
because its licence does not permit redistribution, so you must supply it.

### 1. A C++ toolchain

On Windows this must be MSVC. Rust's default Windows target is
`x86_64-pc-windows-msvc`, which links the same object format as the P4API
distribution. MinGW cannot.

### 2. The P4API

Download a distribution from <https://ftp.perforce.com/perforce/> under
`<release>/bin.<platform>/` and unpack it.

On Windows choose a `static` build — "static" here means the static CRT (`/MT`),
which the P4API is compiled with — and an `openssl3` suffix, for example
`p4api_vs2022_static_openssl3.zip`.

```powershell
$env:P4API_DIR = "C:\path\to\p4api-2025.1.xxxxxxx-vs2022_static"
```

The directory must contain `include/p4/clientapi.h` and `lib/`.

### 3. OpenSSL 3

The P4API archive references OpenSSL but does not ship it. Point at a static
OpenSSL 3 build that matches the P4API's CRT mode — on Windows, the `/MT` one:

```powershell
$env:OPENSSL_LIB_DIR = "C:\Users\you\scoop\apps\openssl\current\lib\VC\x64\MT"
```

`scoop install openssl` provides this layout. So does
`vcpkg install openssl:x64-windows-static`.

### 4. Build

```powershell
cargo build
```

`.cargo/config.toml` sets `+crt-static` on the MSVC target. Every object in the
binary must agree on the CRT, and the P4API distribution picks `/MT`.

## Verify the connection

```powershell
cargo run -p p4-sys --example info
```

This runs `p4 info` through the native API using the ambient `P4PORT`,
`P4USER` and `P4CLIENT`, exactly as the `p4` binary does. It proves the link,
the static CRT and — on an `ssl:` port — the TLS handshake.

## Layout

| Crate | Contents |
| --- | --- |
| `crates/p4-sys` | `cxx` bridge to `ClientApi`/`ClientUser`; the only crate that sees C++ |
| `crates/p4` | Typed commands and models — changelists, opened files, revisions |

```powershell
cargo run -p p4 --example changes
```

lists the pending and recent submitted changelists of the current workspace.
