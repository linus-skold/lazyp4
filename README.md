# lazyp4

[![CI](https://github.com/linus-skold/lazyp4/actions/workflows/ci.yml/badge.svg)](https://github.com/linus-skold/lazyp4/actions/workflows/ci.yml)

A terminal UI for Perforce (Helix Core), in the style of lazygit: browse
changelists and their files in panels, and read diffs in the shell.

You can sync, arrange, shelve, resolve and submit a task without dropping to the
shell. What is still missing — chiefly a merge tool for conflicts `p4 resolve
-am` refuses — is tracked in
[issues](https://github.com/linus-skold/lazyp4/issues).

## Install

Take the archive for your platform from
[Releases](https://github.com/linus-skold/lazyp4/releases) and put the binary on
your `PATH`. The P4API, OpenSSL and — on Windows — the C runtime are all linked
statically, so it is one file: no DLLs, no redistributable, and no `p4` on the
machine.

It does still expect a workspace you have already logged in to. See
[Connecting](#connecting).

To build it yourself, `cargo build` — see [Build](#build).

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

For files that are not neighbours, `v` starts a range: move the cursor to
extend it, then `Space`, `d` or `s` acts on the lot. A directory inside the
range brings its contents with it, and a file counted twice that way is only
acted on once. `v` again or `Esc` abandons the range.

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

## Shelving

A shelf is a copy of a changelist's open files kept on the server. It is how
work moves between machines, and the closest Perforce comes to `git stash` —
though the files stay open in your workspace.

`s` shelves the selected changelist, or in the Files panel just the file or
directory under the cursor. Shelving a changelist that already has a shelf
replaces it, and asks first, because anything shelved but no longer open is
dropped.

`S` unshelves into another changelist — existing or created on the spot — and
leaves the shelf alone, so the same shelf can be unshelved on several machines.
`D` deletes the shelf, leaving the open files untouched.

## Syncing and streams

`p` syncs the workspace and says how many files changed. Files open for edit
are left alone — Perforce refuses to overwrite them rather than discarding
work.

`b` lists the depot's streams, with the one this workspace is on marked. `Enter`
switches, after a confirmation: the workspace is resynced to match, which can
move a lot of data, and Perforce refuses outright while any file is open.

## Resolving

A file that changed in the depot while you had it open cannot be submitted
until it is resolved. `R` lists what is outstanding:

```text
┌ 1 file(s) to resolve ──────────────────────────┐
│ 3waytext  #10,#12   darksim/main/Config/…ini   │
└ y yours   t theirs   m merge   a safe   R close┘
```

| Key | Meaning |
| --- | --- |
| `y` | keep your copy, discarding what arrived |
| `t` | take the depot copy, discarding your changes |
| `m` | merge, which fails rather than guessing at a conflict |
| `a` | safe — only where a single side changed |

`y` and `t` throw one side away, so both ask first. `m` and `a` do not: they
refuse rather than guess. A submit that fails because of an unresolved file
says so and points at `R`.

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
| `/` | narrow the focused list — `Enter` keeps it, `Esc` clears it |
| `+` `_` | give the focused panel most of the column, and back |
| `e` | edit the changelist description |
| `n` | new changelist |
| `c` | submit the changelist |
| `d` | revert files (Files), delete an empty changelist (Changelists) |
| `v` | start a range selection in Files |
| `Space` | move a file, directory or range in or out of the changelist |
| `h` `l`, `←` `→` | fold and unfold a directory (Files), scroll the diff (Diff) |
| `u` | scan for files that are changed but not open (slow) |
| `Enter` | fold a directory, else give the diff the whole window (`Esc` to leave) |
| `r` | refresh |
| `s` | shelve — the changelist, or just the selected files |
| `S` | unshelve into another changelist |
| `D` | delete the shelf |
| `p` | sync the workspace |
| `b` | list streams, and switch |
| `R` | resolve files that changed in the depot while open |
| `H` | revision history of the selected file — `U` there undoes one revision |
| `a` | blame the selected file, line by line |
| `i` | add an untracked file to the ignore file |
| `U` | undo a submitted change into a new changelist |
| `?` | help — every key, grouped; `x` from there opens the p4 command log |
| `q` | quit |

## Connecting

lazyp4 uses the ambient `P4PORT`, `P4USER` and `P4CLIENT`, so run it from a
workspace directory with a `p4config.txt` the same way you would run `p4`.

It speaks to the server through the native API rather than through `p4`, but it
has no `trust` or `login` of its own yet, so on a machine that has never talked
to your server you still need the `p4` CLI once:

```powershell
p4 set P4CONFIG=p4config.txt
p4 trust -y      # only for an ssl: port
p4 login
```

After that lazyp4 stands on its own. Closing that gap is
[issue #1](https://github.com/linus-skold/lazyp4/issues/1).

While it sits still, lazyp4 checks every five seconds whether `p4` has been used
in another window, and reloads if it has.

## Configuration

Colours, keys and the diff tab width come from a config file. Everything in it
is optional, and anything lazyp4 cannot read is reported in the status bar
rather than ignored.

| Platform | Path |
| --- | --- |
| Windows | `%APPDATA%\lazyp4\config.toml` |
| Linux, macOS | `$XDG_CONFIG_HOME/lazyp4/config.toml`, else `~/.config/lazyp4/config.toml` |

`LAZYP4_CONFIG` names a file outright and overrides both.

```toml
[diff]
tab_width = 4          # 1 to 16

[theme]
# A name, #rrggbb, or a number in the 256-colour palette.
focus      = "#ffb000" # borders and keys of whatever has focus
idle       = "darkgray"
muted      = "gray"
added      = "green"
modified   = "yellow"
deleted    = "red"
integrated = "cyan"
untracked  = "magenta"
directory  = "blue"
changelist = "cyan"
shelved    = "magenta"
selection  = "blue"    # the ground behind a range selection
danger     = "red"     # errors, and anything that cannot be undone
ok         = "green"   # notices
text       = "white"   # text on a dark ground
inverse    = "black"   # text on a light one

[keys]
# Naming an action replaces the keys it came with, rather than adding to them.
# Name it twice to give it two keys.
submit = "C"
blame  = "ctrl-b"
```

Actions are named `down` `up` `first` `last` `page_down` `page_up` `left`
`right` `next_panel` `prev_panel` `next_tab` `prev_tab` `filter` `zoom_in`
`zoom_out` `cancel` `move` `revert` `shelve_files` `select_range` `history`
`blame` `ignore` `scan` `new_change` `describe` `submit` `delete_change`
`shelve` `unshelve` `delete_shelf` `undo` `fullscreen` `sync` `streams`
`resolve` `refresh` `log` `help` `quit`.

A key is a single character, or one of `space` `enter` `tab` `shift-tab` `esc`
`backspace` `up` `down` `left` `right` `home` `end` `pageup` `pagedown`, with an
optional `ctrl-` in front. Shift lives in the character itself, so `S` rather
than `shift-s`.

Four things stay put: `1`–`4` and `0` focus the panels whose titles carry those
numbers, `Ctrl-C` quits, `Enter` and `Esc` answer a dialog, and `y` `t` `m` `a`
answer the resolve view. They answer a question rather than name a command.

The help sheet and the status bar are built from the keymap, so they show
whatever is actually bound.

## Build

```powershell
cargo build
```

That is the whole thing. You need Rust, a C++ toolchain and Perl; the build
script fetches everything else.

### What it needs installed

| | |
| --- | --- |
| Rust | stable |
| A C++ toolchain | MSVC on Windows — Rust's `x86_64-pc-windows-msvc` target links the same object format the P4API ships, and MinGW cannot |
| Perl | OpenSSL's `Configure` is a Perl script. Windows: `scoop install perl` |
| `curl` and `tar` | already on Windows 10 and later, macOS and Linux |

NASM is optional. `openssl-src` finds it if it is there and turns on OpenSSL's
assembly routines; without it the build is slower but identical.

### What it fetches

**The P4API**, from `https://ftp.perforce.com/perforce/r25.1/`, picking the
archive for your target — the `static` (static CRT) `openssl3.5` build on
Windows, the `glibc2.12` build on Linux x86_64. It is not vendored: its licence
does not permit redistribution, and the Windows distribution alone is 410 MB of
static libraries, two of whose archives are past GitHub's per-file limit.

**OpenSSL 3.5**, built from source by `openssl-src`. The P4API references
OpenSSL but does not ship it, so `librpc` leaves the `EVP_*` and `OPENSSL_*`
symbols unresolved on its own. Building it here also settles the CRT question:
`openssl-src` follows the target's `crt-static`, so it cannot disagree with the
`/MT` the P4API was compiled with.

The first build takes a few minutes for the download and a few more for
OpenSSL. Both are cached — the P4API under `%LOCALAPPDATA%\lazyp4\p4api` or
`~/.cache/lazyp4/p4api`, which survives `cargo clean`.

### If you would rather supply them yourself

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

### The one thing the repo does pin

`.cargo/config.toml` sets `+crt-static` on the MSVC target. That is not a
machine setting or a preference — every object in the binary has to agree about
the CRT, and the P4API picks `/MT` — so it has to hold for everyone, and it
cannot live in `build.rs` because a build script cannot set a target feature
for the crate graph.

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

## Where Perforce and git part company

lazyp4 follows lazygit's shape, but copying it exactly would produce something
wrong in four places. These are deliberate absences, not gaps.

- **No partial staging.** lazygit's best feature is `Enter` on a file to stage
  individual hunks or lines. Perforce opens a *whole file* or nothing. There is
  no per-hunk equivalent and inventing one would misrepresent what the server
  is about to receive.
- **No local commits.** A pending changelist is not a commit; it becomes one
  only on submit, and it goes straight to the server. So there is no
  `push`/`pull` pair, no rebase, no squash, no amend of local history, and no
  reflog to undo from.
- **No cheap branch switching.** A stream switch resyncs the workspace
  (`p4 switch`), which can move gigabytes. It cannot be a casual keystroke the
  way `space` on a branch is in lazygit, so it sits behind a confirmation that
  says so.
- **Shelving is not stashing.** A shelf belongs to a changelist and lives on the
  server. It is closer to a draft pull request than to `git stash`.

For the same reason, these are not planned: interactive rebase, squash, fixup or
reword of submitted history; per-hunk and per-line staging; a reflog-backed undo
stack; and cherry-pick as a first-class verb — `p4 integrate` is a different
operation with different consequences and should not be dressed up as one.

## Contributing

What is left to build is in
[issues](https://github.com/linus-skold/lazyp4/issues), labelled by area:
`ui` for the terminal interface, `p4-api` for the Perforce layer, and `build`
for packaging and CI.
