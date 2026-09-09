# lazyp4

A terminal UI for Perforce (Helix Core), in the style of lazygit: browse
changelists and their files in panels, and read diffs in the shell.

Status: early. You can browse pending, shelved and submitted changelists, read
each file's diff in the pane, move files between changelists, and edit
changelist descriptions. Submitting is not wired up yet — see
[ROADMAP.md](ROADMAP.md) for what is missing and in what order.

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

## Reading a diff

The diff renders in the pane, not in another program. Each row carries the line
number on both sides, and where a line was rewritten rather than replaced
outright, only the words that actually changed are highlighted:

```text
@@ -38,7 +38,8 @@
38 38  Intermediate/
39 39  Saved/
41    -let x = compute(alpha, beta);
   41 +let x = compute(alpha, gamma);
   42 +Binaries/
```

A line replaced end to end is left unhighlighted — lighting up every word of it
says nothing. Long lines are truncated rather than wrapped, so `h` and `l`
scroll sideways, and `Enter` gives the diff the whole window.

Tabs are expanded to four-column stops. A terminal draws a tab as a single cell
or not at all, so tab-indented source — most C++ under Perforce — would
otherwise lose its nesting entirely.

## Moving files between changelists

With a numbered changelist selected, Files shows two groups: what is in that
changelist, and the default changelist below it. `Space` moves the file under
the cursor across the divider — into the changelist, or back out to default.

Each group is a tree rooted at the depot root — the stream when the workspace
has one, otherwise the deepest directory the listed files share. Every
directory gets its own row, with its contents one level to the right, and the
number of files beneath it:

```text
 In changelist 395
   ▾ Source/ 3
     ▾ Darksim/ 2
       ▾ Actors/ 2
M          Door.cpp
M          Door.h
     ▾ Editor/ 1
A        Tool.cpp
M    README.md
```

`h` and `l` fold and unfold a directory, as does `Enter`. `Space` on a
directory moves every file beneath it, which is the quickest way to move a
whole feature's worth of files at once.

With the **default** changelist selected there is no second group, and so no
implied destination. `Space` there asks where the files should go:

```text
┌ Move 1 file to ──────────────────────────┐
│     395 # Do not submit                  │
│     308 Interaction                      │
│     new  create a changelist…            │
└ Enter choose   Esc cancel ───────────────┘
```

Only your own changelists are offered. Choosing `new` asks for a description
first — Perforce will not create a changelist without one — and then creates it
and moves the files in one step.

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

## Finishing a changelist

`c` submits the selected changelist, after a confirmation that lists every file
going in:

```text
┌ Submit changelist 395 to the depot? ──────────┐
│  A AGENTS.md                                  │
│  M Foo.cpp                                    │
│                                               │
│  # Do not submit                              │
└ y to confirm   any other key cancels ─────────┘
```

Confirmations have no default answer: only `y` proceeds, so a stray `Enter`
cannot submit or discard anything.

Submit is refused before it reaches the server when the changelist is the
default one, belongs to somebody else, is already submitted, or has no real
description — Perforce writes `<saved by Perforce>` itself when it shelves work
you never described, and that counts as no description.

`d` in the Changelists panel deletes an empty changelist; `d` in Files reverts
the file or directory under the cursor, throwing away its local changes. `n`
creates a new empty changelist.

## Editing a description

`e` on a changelist opens its description in a popup. `Enter` saves, `Esc`
discards, and `Shift-Enter` adds a newline. `Ctrl-J` also adds one, for
terminals that report a modified `Enter` as a plain one.

Only the `Description` field of the spec is rewritten, so everything else about
the changelist is left exactly as the server sent it.

| Key | Action |
| --- | --- |
| `j` `k`, `↓` `↑` | move, or scroll the diff |
| `g` `G` | first, last |
| `Tab`, `Shift-Tab` | cycle panels |
| `[` `]` | switch tab within a panel |
| `e` | edit the changelist description |
| `n` | new changelist |
| `c` | submit the changelist |
| `d` | revert files (Files), delete an empty changelist (Changelists) |
| `Space` | move a file, or a whole directory, in or out of the changelist |
| `h` `l`, `←` `→` | fold and unfold a directory (Files), scroll the diff (Diff) |
| `u` | scan for files that are changed but not open (slow) |
| `Enter` | fold a directory, else give the diff the whole window (`Esc` to leave) |
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
