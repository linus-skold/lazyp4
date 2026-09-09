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
| `c` submit, `d` revert | Both behind a confirmation with no default answer |
| `s` shelve, `S` unshelve, `D` delete a shelf | Replacing a shelf is confirmed |
| `e` edits a changelist description | Rewrites one spec field |
| `H` a file's revisions, `U` undo a submitted change | Undo opens into its own changelist |
| `u` scans the workspace for unopened changes | `p4 status`, ~40 s on a large tree |
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

What is left here:

- A submit that fails because files need resolving currently just reports the
  server's message. It should route into the resolve flow (D).
- Submitting is refused for the default changelist rather than supported.
  `p4 submit` with no `-c` would do it, but the default changelist has no
  description and would sweep in whatever else happens to be open.
- Reverting uses the file list lazyp4 already holds. `p4 revert -n` previews
  what the server would actually do and would be a stronger confirmation.

## B. The file flow

Five of the seven are done. What is left:

- **B1. Multi-select.** lazygit stages a range with `v` then movement. Without
  it, moving thirty scattered files is thirty keystrokes. Every verb — `Space`,
  `d`, `s` — should take a selection, not one row.
- **B2. Move all.** lazygit's `a` stages everything. `p4 reopen -c <cl> //...`
  does it in one command. `Space` on a directory covers most of this already,
  so it is only worth doing for the whole-workspace case.
- **B6. A cheaper scan.** `u` walks the whole workspace. `p4 status -f <dir>`
  scoped to the selected file's directory would make it usable mid-task.

Done: **B3** a target that does not exist yet, via the move picker; **B4**
shorter paths, via the folding tree; **B5** discard, via `d` in Files; **B7**
filtering, via `/` on any list panel.

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

lazygit has a dedicated merge-conflict view. Perforce needs the same, and
`p4 submit` will keep failing until it exists.

- List what needs resolving: `p4 resolve -n`.
- Accept a whole file: `p4 resolve -ay` (yours), `-at` (theirs), `-am` (merge).
- Anything genuinely conflicted should hand off to `P4MERGE` rather than trying
  to build a three-way merge editor in the TUI.
- This is stateful and interactive; it needs its own sub-mode, not a keystroke.

## E. Streams and branches

lazygit's panel 3 is branches. Ours is changelists, which is the right call for
day-to-day work — but there is no way to see or change stream.

- List streams: `p4 streams`.
- Switch: `p4 switch <stream>`. Slow and destructive of workspace state, so it
  needs a confirmation that says what will be resynced.
- Show the current stream and any pending integrations in Status.
- Sync: `p4 sync`, with progress. lazygit's `p` (pull) is the closest analogue
  and is missing entirely.

## F. History

Mostly done. Selecting a submitted changelist already points Files and Diff at
it, `H` shows a file's revisions, and `U` undoes a submitted change into a new
pending changelist.

What is left:

- **Annotate.** `p4 annotate` is the blame equivalent and has no UI.
- Undo covers a whole changelist. Undoing a single file revision from the `H`
  panel would be the finer-grained version.

## G. Polish

- `+`/`_` to zoom a panel, as lazygit does. `Enter` already fullscreens the diff.
- A config file for theme, keybindings and the diff tab width, which is fixed
  at four.
- Progress for slow commands. `u` takes ~40 s and the UI only says "working".
- Auto-refresh after external `p4` use.
- `.p4ignore` editing, matching lazygit's `i`.

Done: `/` filtering, and a help sheet grouped by panel with the command log
behind it.

## What is left, in order

1. ~~**A**~~, ~~**C**~~, most of ~~**F**~~, and ~~**B3**–**B5**~~, ~~**B7**~~ —
   done. lazyp4 can arrange, shelve and finish a task.
2. Nothing here is blocking day to day work except a conflict, which is **D**.
3. **D** — resolve, which unblocks submit in the conflict case and is the
   largest thing still missing.
4. **B1** — multi-select, so every verb takes a range rather than one row.
5. **B6** — a scan scoped to a directory, rather than the whole workspace.
6. **A leftovers** — routing a failed submit into D, submitting the default
   changelist, `p4 revert -n` as a stronger confirmation.
7. **E** — streams and `p4 sync`, which has no equivalent at all.
8. **G**, and the **F** leftovers.

## Non-goals

Copying these from lazygit would be wrong rather than merely hard:

- Interactive rebase, squash, fixup, reword of submitted history.
- Per-hunk and per-line staging.
- A reflog-backed undo stack.
- Cherry-pick as a first-class verb — `p4 integrate` is a different operation
  with different consequences and should not be dressed up as one.
