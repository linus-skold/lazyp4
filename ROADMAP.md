# Roadmap to lazygit parity

What lazyp4 still needs, in the order it is worth building. Written against
lazygit's panel-by-panel feature set, with each item mapped to the Perforce
command that actually does the job.

## Where we are

| Working | Notes |
| --- | --- |
| Five panels, numbered `1`–`4` and `0` | Status, Files, Changelists, History, Diff |
| Changelist tabs — Local, Shelved, Others | Ownership by user, `[`/`]` |
| The default changelist, synthesised | `p4 changes` never reports it |
| Files as a folding tree rooted at the stream | Per-group fold state |
| `/` narrows any list panel | Per panel, shown in its title |
| Inline diff, line numbers, word-level highlighting, tabs expanded | No external viewer |
| `Space` moves a file or a whole directory | `reopen`, or `add`/`edit`/`delete` first |
| A picker when there is no implied destination | Including a changelist created on the spot |
| `n` new changelist, `d` delete an empty one | |
| `c` submit, `d` revert | Both confirmed, with no default answer; the default changelist too |
| `s` shelve, `S` unshelve, `D` delete a shelf | Replacing a shelf is confirmed |
| `e` edits a changelist description | Rewrites one spec field |
| `H` a file's revisions, `U` undo a change or one revision | Undo opens into its own changelist |
| `a` blames a file line by line | `p4 annotate -c -u -q` |
| `i` ignores an untracked file | Appends to what `P4IGNORE` names |
| `u` scans the workspace for unopened changes | `p4 status`, ~40 s on a large tree |
| Reloads when `p4` is used elsewhere | Polls `p4 opened` every five seconds while idle |
| Theme, keys and tab width from a config file | See the README |
| Help (`?`), and the command log behind it (`x`) | Every P4API call is logged |

The `p4` crate also has `print_text` wired up but nothing in the UI uses it.

## What does not map

Worth stating plainly, because these are the places where copying lazygit
exactly would produce something wrong.

- **No partial staging.** lazygit's best feature is `Enter` on a file to stage
  individual hunks or lines. Perforce opens a *whole file* or nothing. There is
  no per-hunk equivalent and we should not invent one.
- **No local commits.** A pending changelist is not a commit; it becomes one
  only on submit, and it goes straight to the server. So there is no
  `push`/`pull` pair, no rebase, no squash, no amend of local history, and no
  reflog to undo from.
- **No cheap branch switching.** A stream switch resyncs the workspace
  (`p4 switch`), which can move gigabytes. It cannot be a casual keystroke the
  way `space` on a branch is in lazygit.
- **Shelving is not stashing.** A shelf belongs to a changelist and stays on the
  server. It is closer to a draft PR than to `git stash`.

---

## A. The submit loop

The core gap: you can arrange work but not finish it. Highest priority.

Done. `n` creates, `c` submits, `d` deletes an empty changelist or reverts
files, all behind a confirmation with no default answer.

| Need | Command | State |
| --- | --- | --- |
| ~~Create a changelist~~ | `p4 change -i` with `Change: new` | `n`, and from the move picker |
| ~~Submit~~ | `p4 submit -c <cl>` | `c`, confirmed |
| ~~Delete an empty changelist~~ | `p4 change -d <cl>` | `d` in Changelists |
| ~~Revert files~~ | `p4 revert <files>` | `d` in Files |
| ~~Move to a *new* changelist~~ | create, then `reopen` | The move picker |
| ~~Submit the default changelist~~ | `p4 submit -d <desc>` | `c`, after asking for a description |
| ~~Preview a revert~~ | `p4 revert -n <files>` | What the confirmation lists |

Done. `c` on the default changelist asks for a description and then submits it
with `p4 submit -d`, saying plainly that the whole changelist goes. `d` in Files
asks the server with `p4 revert -n` what it would really do and confirms against
that answer rather than against the list lazyp4 happens to hold. A submit that
fails on an unresolved file points at `R`.

## B. The file flow

Six of the seven are done. What is left:

- **B2. Move all.** lazygit's `a` stages everything. `p4 reopen -c <cl> //...`
  does it in one command. `Space` on a directory, and now `v` over a range,
  cover most of this, so it is only worth doing for the whole-workspace case.
- **B6. A cheaper scan.** `u` walks the whole workspace. `p4 status -f <dir>`
  scoped to the selected file's directory would make it usable mid-task.

Done: **B1** multi-select, via `v` in Files; **B3** a target that does not exist
yet, via the move picker; **B4** shorter paths, via the folding tree; **B5**
discard, via `d` in Files; **B7** filtering, via `/` on any list panel.

Multi-select is Files only. The resolve view still settles one file at a time,
though `p4 resolve` would take several.

## C. Shelving

Done. `s` shelves, `S` unshelves through the destination picker, `D` deletes a
shelf.

| Need | Command | State |
| --- | --- | --- |
| ~~Shelve a changelist~~ | `p4 shelve -c <cl> -f` | `s` in Changelists |
| ~~Shelve selected files~~ | `p4 shelve -c <cl> -f <files>` | `s` in Files |
| ~~Unshelve into a changelist~~ | `p4 unshelve -s <cl> -c <target>` | `S`, into an existing or new changelist |
| ~~Replace an existing shelf~~ | `p4 shelve -r -c <cl>` | `s` on an already shelved changelist, confirmed |
| ~~Delete a shelf~~ | `p4 shelve -d -c <cl>` | `D`, confirmed |

What is left:

- Unshelving from the Others tab works, but a shelf on another user's
  changelist often needs `-f`, which is not offered.
- Deleting individual files from a shelf, rather than the whole shelf.

## D. Resolve

Mostly done. `R` lists what `p4 resolve -n` reports, in its own view, and
settles a file four ways: `y` yours, `t` theirs, `m` merge, `a` safe. Taking
one side outright is confirmed and says which side is lost; merging is not,
since `-am` fails rather than guessing. A submit that fails on an unresolved
file now points at `R`.

What is left:

- **A merge tool.** Anything `-am` refuses still has nowhere to go. Handing off
  to `P4MERGE` means leaving the alternate screen and coming back, which is the
  machinery that went with the external diff viewer.
- Resolving one file at a time. `p4 resolve` accepts several paths, so this
  falls out of multi-select (B1).

## E. Streams and branches

Done. `p` syncs, `b` lists the streams and switches between them, and Status
already shows the current one.

| Need | Command | State |
| --- | --- | --- |
| ~~Sync~~ | `p4 sync` | `p`, reporting how many files changed |
| ~~List streams~~ | `p4 streams` | `b`, with the current one marked |
| ~~Switch~~ | `p4 switch <stream>` | `Enter` in that view, confirmed |
| ~~Show the current stream~~ | `p4 info` | The Status panel |

What is left:

- Progress while syncing. A large sync moves gigabytes and the UI only says
  "working"; see **G**.
- Pending integrations are not shown anywhere.

## F. History

Done. Selecting a submitted changelist already points Files and Diff at
it, `H` shows a file's revisions, and `U` undoes a submitted change into a new
pending changelist. `U` inside the `H` view undoes one revision of one file
instead, into a changelist of its own. `a` blames a file line by line with
`p4 annotate`, naming a change once per run of lines rather than on every line.

## G. Polish

Done: `/` filtering; a help sheet grouped by panel with the command log behind
it; `+`/`_` to zoom a panel; the running command named beside a spinner rather
than a bare "working"; auto-refresh, which polls `p4 opened` every five seconds
while the app sits still and reloads when the answer moves; `i`, which adds an
untracked file to the workspace ignore file; and a config file for the theme,
the keys and the diff tab width, read by a reader of lazyp4's own rather than a
TOML crate. Every key routes through a named action, and the help sheet and
status bar are generated from the keymap, so a rebound key is right everywhere
without anyone having to remember to say so. See the README for the file.

`i` writes to the file `P4IGNORE` names in the workspace root. Perforce keeps
unopened files read-only, so ignoring fails on an ignore file that is in the
depot and not open for edit; the message says so rather than opening it.

## What is left, in order

Done: **A**, **C**, **D**, **E**, **F**, **G**, and all of **B** but **B2** and
**B6**. lazyp4 can sync, arrange, shelve, resolve and finish a task without
dropping to the shell.

What remains, in the order it is worth doing:

1. **`trust` and `login`.** lazyp4 drives the native API, so it needs no `p4`
   binary — except that it cannot accept an SSL fingerprint or get a ticket, so
   a machine that has never reached the server still needs `p4 trust` and
   `p4 login` once. `p4-sys` already exposes `set_password`; nothing calls it.
   This is what stands between a release build and a tool you can hand someone.
2. **A merge tool** for what `resolve -am` refuses. Needs the machinery for
   leaving the alternate screen and coming back, which went with the external
   diff viewer. Until then a real conflict has nowhere to go.
3. **B6** — a scan scoped to a directory rather than the whole workspace.
4. **B2** — `p4 reopen -c <cl> //...` for the whole workspace at once.
5. Multi-select in the resolve view, and the **C** leftovers: `-f` when
   unshelving somebody else's shelf, and deleting single files from a shelf.

## Non-goals

Copying these from lazygit would be wrong rather than merely hard:

- Interactive rebase, squash, fixup, reword of submitted history.
- Per-hunk and per-line staging.
- A reflog-backed undo stack.
- Cherry-pick as a first-class verb — `p4 integrate` is a different operation
  with different consequences and should not be dressed up as one.
