# lazyp4

A terminal UI for Perforce (Helix Core), in the style of lazygit: browse
changelists and their files in panels, and read diffs in the shell.

Status: early. You can browse pending, shelved and submitted changelists, read
each file's diff in the pane, and press `Enter` to open the whole changelist in
[hunk](https://hunk.dev). Nothing writes to the depot yet.

```powershell
cargo run -p lazyp4
```

Panels run down the left, with the diff filling the right:

| Key | Panel | Shows |
| --- | --- | --- |
| `1` | Status | user, client, stream, server |
| `2` | Files | files of the selected changelist |
| `3` | Changelists | pending changelists, in three tabs |
| `4` | History | submitted changelists |
| `0` | Diff | the selected file's diff |

`3` holds the pending work, split by where its content lives:

| Tab | Contents |
| --- | --- |
| Local | yours, with nothing shelved |
| Shelved | yours, with content shelved on the server |
| Others | somebody else's |

A changelist is yours when your user owns it, not when it is open on this
workspace — you may have several. One open elsewhere is tagged with its client
name. `Local` also carries the **default** changelist, which `p4 changes` never
reports, so anything checked out without a numbered changelist still appears.

Selecting a changelist — in either `3` or `4` — repoints Files and Diff at it.

## Moving files between changelists

With a numbered changelist selected, Files shows two groups: what is in that
changelist, and the default changelist below it. `Space` moves the file under
the cursor across the divider — into the changelist, or back out to default.

`u` scans the workspace for files that differ from the depot without being
open, and adds them to the lower group. It walks the whole workspace and takes
tens of seconds on a large tree, so it only runs when you ask.

| Mark | Meaning |
| --- | --- |
| `A` | open for add |
| `M` | open for edit, or changed on disk without being open |
| `D` | open for delete, or missing from disk |
| `??` | Perforce has never seen this file |

`Space` on a file that is not open yet opens it first — `add`, `edit` or
`delete`, whichever reconciles it — and puts it straight into the changelist.

## Editing a description

`E` on a changelist opens its description in a popup. `Ctrl-S` saves, `Esc`
discards. Only the `Description` field of the spec is rewritten, so everything
else about the changelist is left exactly as the server sent it.

| Key | Action |
| --- | --- |
| `j` `k`, `↓` `↑` | move, or scroll the diff |
| `g` `G` | first, last |
| `Tab`, `Shift-Tab` | cycle panels |
| `[` `]` | switch tab within a panel |
| `E` | edit the changelist description |
| `Space` | move a file in or out of the changelist |
| `u` | scan for files that are changed but not open (slow) |
| `Enter` | open the changelist's patch in `hunk` |
| `r` | refresh |
| `x` | command log — every P4API call made |
| `?` | help |
| `q` | quit |

lazyp4 uses the ambient `P4PORT`, `P4USER` and `P4CLIENT`, so run it from a
workspace directory with a `p4config.txt` the same way you would run `p4`.

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
`p4api_vs2022_static_openssl3.zip`. The directory must contain
`include/p4/clientapi.h` and `lib/`.

### 3. OpenSSL 3

The P4API archive references OpenSSL but does not ship it, so `librpc` leaves
the `EVP_*` and `OPENSSL_*` symbols unresolved on its own. Supply a static
OpenSSL 3 build in the same CRT mode as the P4API — on Windows, `/MT`.

`scoop install openssl` puts one in `lib`, alongside the import libraries;
`vcpkg install openssl:x64-windows-static` works too.

### 4. Point the build at both

`.cargo/config.toml` carries the two paths:

```toml
[env]
P4API_DIR = "C:\\path\\to\\p4api-2025.1.xxxxxxx-vs2022_static"
OPENSSL_LIB_DIR = "C:\\Users\\you\\scoop\\apps\\openssl\\current\\lib"
```

Edit them to suit your machine. They are defaults rather than overrides: cargo
skips an entry that is already in the environment, so exporting `P4API_DIR` or
`OPENSSL_LIB_DIR` takes precedence without touching the file.

### 5. Build

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
| `crates/lazyp4` | The `ratatui` binary: panels, key routing, Perforce worker thread |

```powershell
cargo run -p p4 --example changes
```

lists the pending and recent submitted changelists of the current workspace.
