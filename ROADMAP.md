# Roadmap to lazygit parity

What lazyp4 still needs, in the order it is worth building. Written against
lazygit's panel-by-panel feature set, with each item mapped to the Perforce
command that actually does the job.

## Where we are

| Working | Notes |
| --- | --- |
| Five panels, numbered, `Tab` and `[`/`]` navigation | Status, Files, Changelists, History, Diff |
| Changelist tabs — Local, Shelved, Others | Ownership by user |
| The default changelist, synthesised | `p4 changes` never reports it |
| Inline diff with line numbers and word-level highlighting | No external viewer |
| `Space` moves one file between default and a changelist | `reopen`, or `add`/`edit`/`delete` first |
| `u` scans the workspace for unopened changes | `p4 status`, ~40 s on a large tree |
| `e` edits a changelist description | Rewrites one spec field |
| Command log (`x`), help (`?`) | Every P4API call is logged |

The `p4` crate also has `filelog` and `print_text` wired up but nothing in the
UI uses them yet.

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

| Need | Command | UI |
| --- | --- | --- |
| ~~Create a changelist~~ | `p4 change -i` with `Change: new` | Done, via the move picker. Still wants an `n` key in the Changelists panel for an empty one |
| Submit | `p4 submit -c <cl>` | `S` (or `c`, matching lazygit's commit) with a confirmation showing every file |
| Delete an empty changelist | `p4 change -d <cl>` | `d`, refusing while files remain |
| Revert files | `p4 revert -c <cl> <files>` | `d` in Files, always confirmed — this is unrecoverable |
| Move to a *new* changelist | create, then `reopen` | `Space` should be able to target a changelist that does not exist yet |

Submit needs care:

- The description must be non-empty, and `<saved by Perforce>` should be
  treated as empty — Perforce writes it for shelves and it is not a message.
- Submit can fail because files need resolving. That error should route into the
  resolve flow (D), not just print.
- Submit is the one genuinely irreversible action here. A confirmation listing
  the exact files, and no default-to-yes.

Reverting is equally irreversible and deserves the same treatment. `p4 revert -k`
and `-n` (preview) exist and are worth using to show what would be lost.

## B. The file flow

The current `Space`-moves-one-file loop is the weakest part of the app.

1. **Multi-select.** lazygit stages a range with `v` then movement. Without it,
   moving thirty files is thirty keystrokes. Everything in A and C should accept
   a selection, not just the cursor line.
2. **Move all.** lazygit's `a` stages everything. `p4 reopen -c <cl> //...` does
   it in one command. `Space` on a directory already covers the common case.
3. ~~**A target that does not exist yet.**~~ Done — `Space` on the default
   changelist offers a picker, including a new changelist created on the spot.
4. ~~**Shorter paths.**~~ Done — the panel is a folding tree rooted at the
   stream.
5. **Discard.** There is no way to undo an `add` or throw away a local edit —
   see `revert` in A.
6. **A cheaper scan.** `u` walks the whole workspace. `p4 status -f <dir>` scoped
   to the selected file's directory would make it usable mid-task.
7. **Filtering.** lazygit's `/` filters the panel. With hundreds of open files in
   an Unreal workspace this matters more here than it does in git.

## C. Shelving

Perforce's answer to stash, and half-built already — the Shelved tab lists them
and diffs them, but nothing can create or apply one.

| Need | Command |
| --- | --- |
| Shelve a changelist | `p4 shelve -c <cl>` |
| Shelve selected files | `p4 shelve -c <cl> <files>` |
| Unshelve into a changelist | `p4 unshelve -s <cl> -c <target>` |
| Replace an existing shelf | `p4 shelve -r -c <cl>` |
| Delete a shelf | `p4 shelve -d -c <cl>` |

Unshelving is how you move work between machines, so it should also accept a
changelist from the Others tab.

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

The History panel lists submitted changelists but does nothing with them.

- `Enter` on a submitted changelist already shows its diff; it should also offer
  its files.
- File history: `p4 filelog` — already implemented in the `p4` crate, unused.
- Undo a submitted change: `p4 undo -c <new-cl> <file>#<rev>` (server 2019.1+;
  yours is 2024.2). This is the nearest thing to lazygit's revert.
- Annotate: `p4 annotate` is the blame equivalent.

## G. Polish

- `/` to filter any list.
- `+`/`_` to zoom a panel, as lazygit does. `Enter` already fullscreens the diff.
- A config file for theme and keybindings.
- `p4 sync` progress and a spinner — several commands take tens of seconds and
  the UI only says "working".
- Auto-refresh after external `p4` use.
- `.p4ignore` editing, matching lazygit's `i`.

## Suggested order

1. **A** — create, submit, revert. Without these lazyp4 cannot finish a task.
2. **B1–B3** — multi-select, move all, create-on-the-spot. These make A pleasant
   rather than tedious.
3. **C** — shelving, which completes the tab that already exists.
4. **B4–B7** — path display, discard, cheaper scan, filtering.
5. **D** — resolve, which unblocks submit in the conflict case.
6. **E**, **F**, **G**.

## Non-goals

Copying these from lazygit would be wrong rather than merely hard:

- Interactive rebase, squash, fixup, reword of submitted history.
- Per-hunk and per-line staging.
- A reflog-backed undo stack.
- Cherry-pick as a first-class verb — `p4 integrate` is a different operation
  with different consequences and should not be dressed up as one.
