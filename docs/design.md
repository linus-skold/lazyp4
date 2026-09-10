# Design

## Crates

| Crate | Contents |
| --- | --- |
| `crates/p4-sys` | `cxx` bridge to `ClientApi`/`ClientUser`; the only crate that sees C++ |
| `crates/p4` | Typed commands and models — changelists, opened files, revisions |
| `crates/lazyp4` | The `ratatui` binary: panels, key routing, Perforce worker thread |

The split follows the usual Rust `-sys` convention, and earns its keep three
ways. Cargo declares native-library link metadata once per `-sys` crate, which
is why two crates cannot both link the P4API directly. The C++ does not rebuild
when the UI changes. And `crates/p4`'s parsers — the spec form, the diff
normaliser, the date arithmetic — are testable against captured strings with no
server and no P4API in the way.

## The worker thread

`p4::Client` wraps a C++ `ClientApi`, which is not `Send` and whose `Run` blocks
until the server answers. So the client is built on a worker thread and never
leaves it; the UI talks to it over channels and stays responsive while a command
is in flight. Terminal input arrives on the same channel, so the UI has one
place to wait.

Two connections are held, not one: the server strips diff content out of a
tagged reply, and tagging is fixed at handshake time, so structured records and
diff text cannot share a connection.

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

## Things that had to be built rather than borrowed

Each of these is a dependency the project chose not to take:

- **The date formatter.** `p4::civil_date` is Hinnant's civil-from-days, for the
  one thing lazyp4 shows: which day a revision landed.
- **The spec form reader.** `p4::spec` edits one field of a Perforce form and
  leaves the rest exactly as the server sent it.
- **The config reader.** A flat `[section]` / `key = value` parser rather than
  TOML and serde.

## Testing

A TUI cannot be driven interactively from CI, so `crates/lazyp4/src/tests.rs`
renders the real widget tree onto a `TestBackend` and asserts on the resulting
cells. `Worker::detached` gives an `App` with nothing behind it, so key routing
can be driven with canned data and the request it emits inspected.
