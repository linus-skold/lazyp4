# Configuration

Colours, keys and the diff tab width come from one file. Everything in it is
optional, and anything lazyp4 cannot read is reported in the status bar rather
than ignored — a typo never fails silently.

| Platform | Path |
| --- | --- |
| Windows | `%APPDATA%\lazyp4\config.toml` |
| Linux, macOS | `$XDG_CONFIG_HOME/lazyp4/config.toml`, else `~/.config/lazyp4/config.toml` |

`LAZYP4_CONFIG` names a file outright and overrides both.

## A whole file

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

## Keys

Every command routes through a named action, so any of them can move.

| Group | Actions |
| --- | --- |
| Moving around | `down` `up` `first` `last` `page_down` `page_up` `left` `right` `next_panel` `prev_panel` `next_tab` `prev_tab` `filter` `zoom_in` `zoom_out` `cancel` |
| Files | `move` `revert` `shelve_files` `select_range` `history` `blame` `ignore` `scan` |
| Changelists | `new_change` `describe` `submit` `delete_change` `shelve` `unshelve` `delete_shelf` `undo` |
| Everywhere | `fullscreen` `sync` `streams` `resolve` `refresh` `log` `help` `quit` |

A key is a single character, or one of `space` `enter` `tab` `shift-tab` `esc`
`backspace` `up` `down` `left` `right` `home` `end` `pageup` `pagedown`, with an
optional `ctrl-` in front. Shift lives in the character itself, so `S` rather
than `shift-s`.

The help sheet and the status bar are generated from the keymap, so a rebound
key is right everywhere without a second list to keep in step. A handful of keys
[cannot be rebound](keys.md#what-cannot-be-rebound).

## Colours

A colour is a name (`red`, `dark_gray`, `light_green`, …), a `#rrggbb` triple,
or a number from 0 to 255 in the terminal's 256-colour palette.

The names above are roles rather than places, so setting `danger` recolours
errors, destructive confirmations and the unresolved marker together.

## The file format

It is read by a small `[section]` / `key = value` reader of lazyp4's own rather
than a TOML crate — the shape is flat, and a dependency would be the larger
cost. It understands comments (`#`, at the start of a line or after a space, so
`#ffb000` survives), optional quotes around values, and nothing else. Anything
it does not understand is reported rather than skipped.
