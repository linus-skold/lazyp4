# Using lazyp4

Panels run down the left, with the diff filling the right.

| Key | Panel | Shows |
| --- | --- | --- |
| `1` | Status | user, client, stream, server |
| `2` | Files | files of the selected changelist |
| `3` | Changelists | pending changelists, in three tabs |
| `4` | History | submits against this workspace |
| `0` | Diff | the selected file's diff |

`3` holds the pending work, split by where its content lives:

| Tab | Contents |
| --- | --- |
| Local | on this workspace, with nothing shelved |
| Shelved | on this workspace, with content shelved on the server |
| Others | on another workspace |

A changelist is local when it is open on the client you are connected to.
Perforce ties a pending changelist to one workspace, so your own changelists on
your other workspaces sit under `Others`, tagged with their client name, and
lazyp4 does not shelve, submit, or move files into them. Started outside a
workspace the server resolves no client, so the tabs fall back to your user
name. `Local` also carries the **default** changelist, which `p4 changes` never
reports, so anything checked out without a numbered changelist still appears.

`Others` is not the whole server. lazyp4 asks for the pending changelists that
touch a file your client maps, so what you see there is work that can collide
with yours, not every changelist in the depot.

`4` is narrowed the same way: the 50 most recent changes submitted against the
files your workspace maps, whoever submitted them. A submit to another stream
or another depot does not appear.

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
otherwise lose its nesting entirely. The width is
[configurable](configuration.md).

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
     ▾ Core/ 2
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
`i` adds an untracked file to the workspace ignore file instead.

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

Submit is refused before it reaches the server when the changelist belongs to
somebody else, is already submitted, or has no real description — Perforce
writes `<saved by Perforce>` itself when it shelves work you never described,
and that counts as no description. On the **default** changelist `c` asks for a
description first and then submits it with `p4 submit -d`, saying plainly that
everything open in it goes.

`d` in the Changelists panel deletes an empty changelist; `d` in Files reverts
the file or directory under the cursor. The revert confirmation lists what
`p4 revert -n` says the server would actually do, rather than what lazyp4
happens to be holding. `n` creates a new empty changelist.

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

`b` lists the streams of the depot you are in, with the one this workspace is
on marked. A switch cannot leave the depot, so the rest of the server is left
out; a client with no stream gets the full list. `Enter`
switches, after a confirmation: the workspace is resynced to match, which can
move a lot of data, and Perforce refuses outright while any file is open.

## Resolving

A file that changed in the depot while you had it open cannot be submitted
until it is resolved. `R` lists what is outstanding:

```text
┌ 1 file(s) to resolve ──────────────────────────┐
│ 3waytext  #10,#12   depot/main/Config/…ini     │
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

Anything `m` refuses currently has nowhere to go — a merge tool is
[issue #2](https://github.com/linus-skold/lazyp4/issues/2).

## History and blame

`H` shows every revision of the file under the cursor. `U` there undoes a
single revision into a changelist of its own; `U` in the History panel undoes a
whole submitted change the same way. Nothing reaches the depot until you submit
the changelist it opens.

`a` blames the file line by line with `p4 annotate`, heading a run of lines from
one change once rather than on every line.

## Editing a description

`e` on a changelist opens its description in a popup. `Enter` saves, `Esc`
discards, and `Shift-Enter` adds a newline. `Ctrl-J` also adds one, for
terminals that report a modified `Enter` as a plain one.

Only the `Description` field of the spec is rewritten, so everything else about
the changelist is left exactly as the server sent it.

## Keeping up to date

While it sits still, lazyp4 checks every five seconds whether `p4` has been used
in another window, and reloads if it has. `r` forces it.
