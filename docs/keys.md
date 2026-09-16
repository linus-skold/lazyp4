# Keys

`?` inside lazyp4 shows this grouped by panel, built from the live keymap — so
if you have [rebound anything](configuration.md), the help sheet and the status
bar show what is actually bound rather than what shipped.

`x` opens the log of every command lazyp4 has run. The Log pane under the diff
shows the newest lines; the overlay holds the last 500. Opened from the help
sheet, `x` steps back to it.

## Moving around

| Key | Action |
| --- | --- |
| `j` `k`, `↓` `↑` | move, or scroll the diff |
| `g` `G` | first, last |
| `Tab`, `Shift-Tab` | cycle panels |
| `1`–`4`, `0` | focus a panel by its number |
| `[` `]` | switch tab within a panel |
| `/` | narrow the focused list — `Enter` keeps it, `Esc` clears it |
| `+` `_` | give the focused panel most of the column, and back |
| `h` `l`, `←` `→` | fold a directory (Files), scroll sideways (Diff) |
| `Enter` | open a change, fold a directory, else give the diff the whole window |
| `Esc` | back out — a range, then fullscreen, then an opened change |

## Files

| Key | Action |
| --- | --- |
| `Space` | move a file, directory or range in or out of the changelist |
| `d` | revert, discarding local changes |
| `s` | shelve just the selection |
| `v` | start a range selection |
| `H` | revision history — `U` there undoes one revision |
| `a` | blame, line by line |
| `i` | add an untracked file to the ignore file |
| `u` | find offline work: files changed on disk without telling Perforce, which is what `p4 reconcile` acts on (slow) |

## Changelists

| Key | Action |
| --- | --- |
| `n` | new changelist |
| `e` | edit the description |
| `c` | submit |
| `d` | delete an empty one |
| `s` | shelve to the server |
| `S` | unshelve into another |
| `D` | delete the shelf |
| `U` | undo a submitted change |

## Everywhere

| Key | Action |
| --- | --- |
| `p` | sync the workspace |
| `b` | list streams, and switch |
| `w` | list your workspaces, and switch |
| `R` | resolve files that changed in the depot while open |
| `r` | refresh |
| `?` | help |
| `q` | quit |
| `Ctrl-C` | quit |

## What cannot be rebound

Four things stay put, because they answer a question rather than name a command:

- `1`–`4` and `0`, which focus the panels whose titles carry those numbers
- `Ctrl-C`
- `Enter` and `Esc` inside a dialog
- `y` `t` `m` `a` inside the resolve view, and the `Enter` and `y` of any
  confirmation

Everything else is [configurable](configuration.md).
