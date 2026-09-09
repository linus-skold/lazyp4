//! The config file: which colours, which keys, and how wide a tab is.
//!
//! A tiny `[section]` / `key = value` reader rather than a TOML crate, the way
//! [`crate::app`]'s sibling `p4::spec` reads a Perforce form: the file shape is
//! flat and a dependency would be the larger cost.
//!
//! Everything is optional. A file that is not there, or a key that is not
//! understood, leaves the default in place — and an unknown key is reported
//! rather than swallowed, since a silent typo is worse than a loud one.

use std::collections::HashMap;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;

use crate::app::Panel;

/// Everything the config file can say.
#[derive(Debug, Clone)]
pub struct Config {
    pub theme: Theme,
    /// Columns a tab advances to in the diff and blame views.
    pub tab_width: usize,
    pub keys: Keymap,
    /// Lines that meant nothing, so a typo is not silent.
    pub warnings: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            theme: Theme::default(),
            tab_width: 4,
            keys: Keymap::default(),
            warnings: Vec::new(),
        }
    }
}

impl Config {
    /// Read the config file, or fall back to the defaults if there is none.
    pub fn load() -> Config {
        match Self::path().and_then(|p| std::fs::read_to_string(p).ok()) {
            Some(text) => Config::parse(&text),
            None => Config::default(),
        }
    }

    /// Where the config file lives. `LAZYP4_CONFIG` names one outright.
    pub fn path() -> Option<PathBuf> {
        if let Ok(explicit) = std::env::var("LAZYP4_CONFIG") {
            return Some(PathBuf::from(explicit));
        }
        let dir = std::env::var("APPDATA")
            .ok()
            .or_else(|| std::env::var("XDG_CONFIG_HOME").ok())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| PathBuf::from(h).join(".config"))
            })?;
        Some(dir.join("lazyp4").join("config.toml"))
    }

    pub fn parse(text: &str) -> Config {
        let mut config = Config::default();
        let mut section = String::new();
        // Keys are replaced wholesale, so the first mention of an action drops
        // the defaults rather than adding to them.
        let mut rebound: Vec<Action> = Vec::new();

        for (n, raw) in text.lines().enumerate() {
            let line = strip_comment(raw);
            if line.is_empty() {
                continue;
            }
            let where_ = |what: &str| format!("config line {}: {what}", n + 1);

            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = name.trim().to_lowercase();
                if !matches!(section.as_str(), "theme" | "keys" | "diff") {
                    config.warnings.push(where_(&format!("unknown section [{section}]")));
                }
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                config.warnings.push(where_("expected key = value"));
                continue;
            };
            let key = key.trim().to_lowercase();
            let value = value.trim().trim_matches('"');

            match section.as_str() {
                "theme" => match (Theme::field(&key), parse_color(value)) {
                    (Some(field), Some(color)) => config.theme.set(field, color),
                    (None, _) => config.warnings.push(where_(&format!("no colour named {key}"))),
                    (_, None) => config
                        .warnings
                        .push(where_(&format!("{value} is not a colour"))),
                },
                "diff" if key == "tab_width" => match value.parse::<usize>() {
                    Ok(w) if (1..=16).contains(&w) => config.tab_width = w,
                    _ => config
                        .warnings
                        .push(where_("tab_width must be a number from 1 to 16")),
                },
                "keys" => match (Action::named(&key), Key::parse(value)) {
                    (Some(action), Some(k)) => {
                        if !rebound.contains(&action) {
                            config.keys.clear(action);
                            rebound.push(action);
                        }
                        config.keys.bind(k, action);
                    }
                    (None, _) => config.warnings.push(where_(&format!("no action named {key}"))),
                    (_, None) => config.warnings.push(where_(&format!("{value} is not a key"))),
                },
                "" => config.warnings.push(where_("a key outside any section")),
                _ => config.warnings.push(where_(&format!("{key} means nothing here"))),
            }
        }
        config
    }
}

/// Drop a trailing comment. A `#` only starts one at the beginning of the line
/// or after a space, so `#ff8800` survives as a colour.
fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'#' && (i == 0 || bytes[i - 1].is_ascii_whitespace()) {
            return line[..i].trim();
        }
    }
    line.trim()
}

/// Every colour the UI draws with.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// Borders, keys and titles of whatever has focus.
    pub focus: Color,
    /// Chrome that does not.
    pub idle: Color,
    /// Secondary text that still has to be read.
    pub muted: Color,
    pub added: Color,
    pub modified: Color,
    pub deleted: Color,
    /// Branch and integrate marks.
    pub integrated: Color,
    /// A file Perforce has never seen.
    pub untracked: Color,
    pub directory: Color,
    /// Changelist numbers, and headers inside the diff.
    pub changelist: Color,
    pub shelved: Color,
    /// The ground behind an open range selection.
    pub selection: Color,
    /// Errors, and anything that cannot be undone.
    pub danger: Color,
    /// Notices.
    pub ok: Color,
    /// Text drawn on a dark ground.
    pub text: Color,
    /// Text drawn on a light one.
    pub inverse: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            focus: Color::Yellow,
            idle: Color::DarkGray,
            muted: Color::Gray,
            added: Color::Green,
            modified: Color::Yellow,
            deleted: Color::Red,
            integrated: Color::Cyan,
            untracked: Color::Magenta,
            directory: Color::Blue,
            changelist: Color::Cyan,
            shelved: Color::Magenta,
            selection: Color::Blue,
            danger: Color::Red,
            ok: Color::Green,
            text: Color::White,
            inverse: Color::Black,
        }
    }
}

/// Which field of [`Theme`] a config key names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Focus,
    Idle,
    Muted,
    Added,
    Modified,
    Deleted,
    Integrated,
    Untracked,
    Directory,
    Changelist,
    Shelved,
    Selection,
    Danger,
    Ok,
    Text,
    Inverse,
}

impl Theme {
    fn field(key: &str) -> Option<Field> {
        Some(match key {
            "focus" => Field::Focus,
            "idle" => Field::Idle,
            "muted" => Field::Muted,
            "added" => Field::Added,
            "modified" => Field::Modified,
            "deleted" => Field::Deleted,
            "integrated" => Field::Integrated,
            "untracked" => Field::Untracked,
            "directory" => Field::Directory,
            "changelist" => Field::Changelist,
            "shelved" => Field::Shelved,
            "selection" => Field::Selection,
            "danger" => Field::Danger,
            "ok" => Field::Ok,
            "text" => Field::Text,
            "inverse" => Field::Inverse,
            _ => return None,
        })
    }

    fn set(&mut self, field: Field, color: Color) {
        let slot = match field {
            Field::Focus => &mut self.focus,
            Field::Idle => &mut self.idle,
            Field::Muted => &mut self.muted,
            Field::Added => &mut self.added,
            Field::Modified => &mut self.modified,
            Field::Deleted => &mut self.deleted,
            Field::Integrated => &mut self.integrated,
            Field::Untracked => &mut self.untracked,
            Field::Directory => &mut self.directory,
            Field::Changelist => &mut self.changelist,
            Field::Shelved => &mut self.shelved,
            Field::Selection => &mut self.selection,
            Field::Danger => &mut self.danger,
            Field::Ok => &mut self.ok,
            Field::Text => &mut self.text,
            Field::Inverse => &mut self.inverse,
        };
        *slot = color;
    }
}

/// A colour name, `#rrggbb`, or a number in the 256-colour palette.
pub fn parse_color(text: &str) -> Option<Color> {
    let name = text.trim().to_lowercase();
    if let Some(hex) = name.strip_prefix('#') {
        if hex.len() != 6 {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        return Some(Color::Rgb(byte(0)?, byte(2)?, byte(4)?));
    }
    if let Ok(n) = name.parse::<u8>() {
        return Some(Color::Indexed(n));
    }
    Some(match name.replace(['_', '-'], "").as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" | "grey" | "white" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "lightred" => Color::LightRed,
        "lightgreen" => Color::LightGreen,
        "lightyellow" => Color::LightYellow,
        "lightblue" => Color::LightBlue,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        "brightwhite" => Color::White,
        "reset" | "default" => Color::Reset,
        _ => return None,
    })
}

/// One keystroke. Shift lives in the character itself — `S` rather than
/// `shift-s` — which is how a terminal reports it anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    pub code: KeyCode,
    pub ctrl: bool,
}

impl Key {
    pub const fn plain(code: KeyCode) -> Key {
        Key { code, ctrl: false }
    }

    pub const fn ch(c: char) -> Key {
        Key::plain(KeyCode::Char(c))
    }

    pub fn from_event(event: KeyEvent) -> Key {
        Key {
            code: event.code,
            ctrl: event.modifiers.contains(KeyModifiers::CONTROL),
        }
    }

    pub fn parse(text: &str) -> Option<Key> {
        let text = text.trim();
        if let Some(rest) = text
            .strip_prefix("ctrl-")
            .or_else(|| text.strip_prefix("Ctrl-"))
        {
            let mut key = Key::parse(rest)?;
            key.ctrl = true;
            return Some(key);
        }
        let mut chars = text.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            return Some(Key::ch(c));
        }
        Some(Key::plain(match text.to_lowercase().as_str() {
            "space" => KeyCode::Char(' '),
            "enter" | "return" => KeyCode::Enter,
            "tab" => KeyCode::Tab,
            "shift-tab" | "backtab" => KeyCode::BackTab,
            "esc" | "escape" => KeyCode::Esc,
            "backspace" => KeyCode::Backspace,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pageup" => KeyCode::PageUp,
            "pagedown" => KeyCode::PageDown,
            _ => return None,
        }))
    }

    /// How the help sheet writes it.
    pub fn show(&self) -> String {
        let name = match self.code {
            KeyCode::Char(' ') => "space".to_owned(),
            KeyCode::Char(c) => c.to_string(),
            KeyCode::Enter => "enter".to_owned(),
            KeyCode::Tab => "tab".to_owned(),
            KeyCode::BackTab => "shift-tab".to_owned(),
            KeyCode::Esc => "esc".to_owned(),
            KeyCode::Backspace => "backspace".to_owned(),
            KeyCode::Up => "↑".to_owned(),
            KeyCode::Down => "↓".to_owned(),
            KeyCode::Left => "←".to_owned(),
            KeyCode::Right => "→".to_owned(),
            KeyCode::Home => "home".to_owned(),
            KeyCode::End => "end".to_owned(),
            KeyCode::PageUp => "pgup".to_owned(),
            KeyCode::PageDown => "pgdn".to_owned(),
            other => format!("{other:?}").to_lowercase(),
        };
        if self.ctrl {
            format!("ctrl-{name}")
        } else {
            name
        }
    }
}

/// Where an action appears on the help sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Navigation,
    Files,
    Changelists,
    App,
}

/// A help-sheet row: what the action does, and a second action drawn on the
/// same row because the two are a pair.
pub struct Help {
    pub label: &'static str,
    pub with: Option<Action>,
}

/// Everything lazyp4 can be asked to do with one keystroke.
///
/// The order matters: a key can carry more than one action, and the first
/// whose panel matches wins. So `d` reaches [`Action::RevertFiles`] in Files
/// and [`Action::DeleteChange`] in Changelists, and `s` reaches
/// [`Action::ShelveFiles`] before [`Action::ShelveChange`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Down,
    Up,
    First,
    Last,
    PageDown,
    PageUp,
    Left,
    Right,
    NextPanel,
    PrevPanel,
    NextTab,
    PrevTab,
    Filter,
    ZoomIn,
    ZoomOut,
    Cancel,

    Move,
    RevertFiles,
    ShelveFiles,
    SelectRange,
    History,
    Blame,
    Ignore,
    Scan,

    NewChange,
    Describe,
    Submit,
    DeleteChange,
    ShelveChange,
    Unshelve,
    DeleteShelf,
    Undo,

    Fullscreen,
    Sync,
    Streams,
    Resolve,
    Refresh,
    Log,
    Help,
    Quit,
}

/// What each action answers to before the config file has its say. A `const`
/// so the slices live for the whole program rather than being rebuilt.
const DEFAULT_KEYS: [(Action, &[Key]); 40] = [
    (Action::Down, &[Key::ch('j'), Key::plain(KeyCode::Down)]),
    (Action::Up, &[Key::ch('k'), Key::plain(KeyCode::Up)]),
    (Action::First, &[Key::ch('g'), Key::plain(KeyCode::Home)]),
    (Action::Last, &[Key::ch('G'), Key::plain(KeyCode::End)]),
    (Action::PageDown, &[Key::plain(KeyCode::PageDown)]),
    (Action::PageUp, &[Key::plain(KeyCode::PageUp)]),
    (Action::Left, &[Key::ch('h'), Key::plain(KeyCode::Left)]),
    (Action::Right, &[Key::ch('l'), Key::plain(KeyCode::Right)]),
    (Action::NextPanel, &[Key::plain(KeyCode::Tab)]),
    (Action::PrevPanel, &[Key::plain(KeyCode::BackTab)]),
    (Action::NextTab, &[Key::ch(']')]),
    (Action::PrevTab, &[Key::ch('[')]),
    (Action::Filter, &[Key::ch('/')]),
    (Action::ZoomIn, &[Key::ch('+')]),
    (Action::ZoomOut, &[Key::ch('_'), Key::ch('-')]),
    (Action::Cancel, &[Key::plain(KeyCode::Esc)]),
    (Action::Move, &[Key::ch(' ')]),
    (Action::RevertFiles, &[Key::ch('d')]),
    (Action::ShelveFiles, &[Key::ch('s')]),
    (Action::SelectRange, &[Key::ch('v')]),
    (Action::History, &[Key::ch('H')]),
    (Action::Blame, &[Key::ch('a')]),
    (Action::Ignore, &[Key::ch('i')]),
    (Action::Scan, &[Key::ch('u')]),
    (Action::NewChange, &[Key::ch('n')]),
    (Action::Describe, &[Key::ch('e')]),
    (Action::Submit, &[Key::ch('c')]),
    (Action::DeleteChange, &[Key::ch('d')]),
    (Action::ShelveChange, &[Key::ch('s')]),
    (Action::Unshelve, &[Key::ch('S')]),
    (Action::DeleteShelf, &[Key::ch('D')]),
    (Action::Undo, &[Key::ch('U')]),
    (Action::Fullscreen, &[Key::plain(KeyCode::Enter)]),
    (Action::Sync, &[Key::ch('p')]),
    (Action::Streams, &[Key::ch('b')]),
    (Action::Resolve, &[Key::ch('R')]),
    (Action::Refresh, &[Key::ch('r')]),
    (Action::Log, &[Key::ch('x')]),
    (Action::Help, &[Key::ch('?')]),
    (Action::Quit, &[Key::ch('q')]),
];

impl Action {
    pub const ALL: [Action; 40] = [
        Action::Down,
        Action::Up,
        Action::First,
        Action::Last,
        Action::PageDown,
        Action::PageUp,
        Action::Left,
        Action::Right,
        Action::NextPanel,
        Action::PrevPanel,
        Action::NextTab,
        Action::PrevTab,
        Action::Filter,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::Cancel,
        Action::Move,
        Action::RevertFiles,
        Action::ShelveFiles,
        Action::SelectRange,
        Action::History,
        Action::Blame,
        Action::Ignore,
        Action::Scan,
        Action::NewChange,
        Action::Describe,
        Action::Submit,
        Action::DeleteChange,
        Action::ShelveChange,
        Action::Unshelve,
        Action::DeleteShelf,
        Action::Undo,
        Action::Fullscreen,
        Action::Sync,
        Action::Streams,
        Action::Resolve,
        Action::Refresh,
        Action::Log,
        Action::Help,
        Action::Quit,
    ];

    /// The name the config file uses.
    pub fn name(self) -> &'static str {
        match self {
            Action::Down => "down",
            Action::Up => "up",
            Action::First => "first",
            Action::Last => "last",
            Action::PageDown => "page_down",
            Action::PageUp => "page_up",
            Action::Left => "left",
            Action::Right => "right",
            Action::NextPanel => "next_panel",
            Action::PrevPanel => "prev_panel",
            Action::NextTab => "next_tab",
            Action::PrevTab => "prev_tab",
            Action::Filter => "filter",
            Action::ZoomIn => "zoom_in",
            Action::ZoomOut => "zoom_out",
            Action::Cancel => "cancel",
            Action::Move => "move",
            Action::RevertFiles => "revert",
            Action::ShelveFiles => "shelve_files",
            Action::SelectRange => "select_range",
            Action::History => "history",
            Action::Blame => "blame",
            Action::Ignore => "ignore",
            Action::Scan => "scan",
            Action::NewChange => "new_change",
            Action::Describe => "describe",
            Action::Submit => "submit",
            Action::DeleteChange => "delete_change",
            Action::ShelveChange => "shelve",
            Action::Unshelve => "unshelve",
            Action::DeleteShelf => "delete_shelf",
            Action::Undo => "undo",
            Action::Fullscreen => "fullscreen",
            Action::Sync => "sync",
            Action::Streams => "streams",
            Action::Resolve => "resolve",
            Action::Refresh => "refresh",
            Action::Log => "log",
            Action::Help => "help",
            Action::Quit => "quit",
        }
    }

    pub fn named(name: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.name() == name)
    }

    /// The keys it answers to out of the box. Several, where an arrow does the
    /// same job as a letter.
    pub fn default_keys(self) -> &'static [Key] {
        DEFAULT_KEYS
            .iter()
            .find(|(action, _)| *action == self)
            .map(|(_, keys)| *keys)
            .unwrap_or(&[])
    }

    /// Panels this action belongs to. Empty means anywhere, which is most of
    /// them: only the keys that mean two things need narrowing.
    pub fn panels(self) -> &'static [Panel] {
        match self {
            Action::RevertFiles | Action::ShelveFiles => &[Panel::Files],
            Action::DeleteChange => &[Panel::Changelists],
            _ => &[],
        }
    }

    pub fn applies_to(self, focus: Panel) -> bool {
        let panels = self.panels();
        panels.is_empty() || panels.contains(&focus)
    }

    pub fn group(self) -> Group {
        match self {
            Action::Move
            | Action::RevertFiles
            | Action::ShelveFiles
            | Action::SelectRange
            | Action::History
            | Action::Blame
            | Action::Ignore
            | Action::Scan => Group::Files,
            Action::NewChange
            | Action::Describe
            | Action::Submit
            | Action::DeleteChange
            | Action::ShelveChange
            | Action::Unshelve
            | Action::DeleteShelf
            | Action::Undo => Group::Changelists,
            Action::Fullscreen
            | Action::Sync
            | Action::Streams
            | Action::Resolve
            | Action::Refresh
            | Action::Log
            | Action::Help
            | Action::Quit => Group::App,
            _ => Group::Navigation,
        }
    }

    /// The help-sheet row, if it has one. An action named as another's `with`
    /// has none of its own, so it appears exactly once.
    pub fn help(self) -> Option<Help> {
        let row = |label, with| Some(Help { label, with });
        match self {
            Action::Down => row("move", Some(Action::Up)),
            Action::First => row("first / last", Some(Action::Last)),
            Action::Left => row("fold, or scroll the diff", Some(Action::Right)),
            Action::NextPanel => row("cycle panels", Some(Action::PrevPanel)),
            Action::NextTab => row("switch tab", Some(Action::PrevTab)),
            Action::Filter => row("narrow the list", None),
            Action::ZoomIn => row("zoom a panel", Some(Action::ZoomOut)),
            Action::Move => row("move to / from the changelist", None),
            Action::RevertFiles => row("revert, discarding local changes", None),
            Action::ShelveFiles => row("shelve just these", None),
            Action::SelectRange => row("select a range", None),
            Action::History => row("revision history, U to undo one", None),
            Action::Blame => row("blame, line by line", None),
            Action::Ignore => row("ignore an untracked file", None),
            Action::Scan => row("scan for unopened changes (slow)", None),
            Action::NewChange => row("new", None),
            Action::Describe => row("edit the description", None),
            Action::Submit => row("submit", None),
            Action::DeleteChange => row("delete an empty one", None),
            Action::ShelveChange => row("shelve to the server", None),
            Action::Unshelve => row("unshelve into another", None),
            Action::DeleteShelf => row("delete the shelf", None),
            Action::Undo => row("undo a submitted change", None),
            Action::Fullscreen => row("fullscreen the diff", None),
            Action::Sync => row("sync the workspace", None),
            Action::Streams => row("streams", None),
            Action::Resolve => row("resolve what is conflicting", None),
            Action::Refresh => row("refresh", None),
            Action::Log => row("the p4 command log", None),
            Action::Help => row("this sheet", None),
            Action::Quit => row("quit", None),
            _ => None,
        }
    }

    /// The word the status bar uses, which has less room than the help sheet.
    pub fn short(self) -> &'static str {
        match self {
            Action::Move => "move",
            Action::RevertFiles => "revert",
            Action::History => "history",
            Action::Blame => "blame",
            Action::Scan => "scan",
            Action::Submit => "submit",
            Action::ShelveChange => "shelve",
            Action::Unshelve => "unshelve",
            Action::NewChange => "new",
            Action::Describe => "describe",
            Action::DeleteChange => "delete",
            Action::Undo => "undo",
            Action::Fullscreen => "fullscreen",
            Action::Left => "scroll",
            Action::Refresh => "refresh",
            Action::Help => "help",
            Action::Quit => "quit",
            other => other.name(),
        }
    }
}

/// Which keys reach which actions.
#[derive(Debug, Clone)]
pub struct Keymap {
    /// Kept in [`Action::ALL`] order, so a key carrying two actions offers the
    /// narrower one first.
    bindings: HashMap<Key, Vec<Action>>,
}

impl Default for Keymap {
    fn default() -> Self {
        let mut map = Keymap {
            bindings: HashMap::new(),
        };
        for action in Action::ALL {
            for key in action.default_keys() {
                map.bind(*key, action);
            }
        }
        map
    }
}

impl Keymap {
    pub fn bind(&mut self, key: Key, action: Action) {
        let bound = self.bindings.entry(key).or_default();
        if !bound.contains(&action) {
            bound.push(action);
            // ALL order is what disambiguates a shared key, so keep it.
            bound.sort_by_key(|a| Action::ALL.iter().position(|x| x == a).unwrap_or(usize::MAX));
        }
    }

    /// Forget every key an action answers to, so the config replaces the
    /// defaults rather than piling onto them.
    pub fn clear(&mut self, action: Action) {
        self.bindings.retain(|_, actions| {
            actions.retain(|a| *a != action);
            !actions.is_empty()
        });
    }

    /// What this key can do, narrowest first.
    pub fn actions(&self, key: Key) -> &[Action] {
        self.bindings.get(&key).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Whether the key reaches this action at all, whatever else it also does.
    pub fn is(&self, key: Key, action: Action) -> bool {
        self.actions(key).contains(&action)
    }

    /// The first key bound to an action, for the help sheet and the status bar.
    pub fn key_for(&self, action: Action) -> Option<Key> {
        action
            .default_keys()
            .iter()
            .find(|k| self.is(**k, action))
            .copied()
            .or_else(|| {
                let mut keys: Vec<&Key> = self
                    .bindings
                    .iter()
                    .filter(|(_, actions)| actions.contains(&action))
                    .map(|(k, _)| k)
                    .collect();
                // A map has no order of its own, so pin one.
                keys.sort_by_key(|k| k.show());
                keys.first().copied().copied()
            })
    }

    /// How the help sheet names the key or key pair for a row.
    pub fn label_for(&self, action: Action, with: Option<Action>) -> String {
        let one = |a: Action| self.key_for(a).map(|k| k.show()).unwrap_or_default();
        match with.map(one).filter(|s| !s.is_empty()) {
            Some(second) => format!("{} / {second}", one(action)),
            None => one(action),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_file_leaves_every_default_in_place() {
        let config = Config::parse("");
        assert!(config.warnings.is_empty());
        assert_eq!(config.tab_width, 4);
        assert_eq!(config.theme.focus, Color::Yellow);
        assert!(config.keys.is(Key::ch('c'), Action::Submit));
    }

    #[test]
    fn reads_a_theme_a_tab_width_and_a_key() {
        let config = Config::parse(
            "# a comment\n\
             [theme]\n\
             focus = \"#ff8800\"\n\
             added = lightgreen\n\
             \n\
             [diff]\n\
             tab_width = 8\n\
             \n\
             [keys]\n\
             submit = C\n",
        );
        assert!(config.warnings.is_empty(), "{:?}", config.warnings);
        assert_eq!(config.theme.focus, Color::Rgb(0xff, 0x88, 0x00));
        assert_eq!(config.theme.added, Color::LightGreen);
        assert_eq!(config.tab_width, 8);
        assert_eq!(
            Config::parse("[theme]\nfocus = red # after a space").theme.focus,
            Color::Red,
            "a real trailing comment still goes"
        );
        assert!(config.keys.is(Key::ch('C'), Action::Submit));
        assert!(
            !config.keys.is(Key::ch('c'), Action::Submit),
            "rebinding replaces the default rather than adding to it"
        );
    }

    #[test]
    fn an_action_can_be_given_several_keys() {
        let config = Config::parse("[keys]\nsubmit = C\nsubmit = ctrl-s\n");
        assert!(config.warnings.is_empty(), "{:?}", config.warnings);
        assert!(config.keys.is(Key::ch('C'), Action::Submit));
        assert!(config.keys.is(
            Key {
                code: KeyCode::Char('s'),
                ctrl: true
            },
            Action::Submit
        ));
    }

    #[test]
    fn a_typo_is_reported_rather_than_swallowed() {
        let config = Config::parse(
            "[theme]\nfokus = red\nfocus = nonsense\n\
             [diff]\ntab_width = wide\n\
             [keys]\nsubmitt = c\nsubmit = notakey\n\
             [nonsense]\nx = y\n",
        );
        assert_eq!(config.warnings.len(), 7, "{:?}", config.warnings);
        assert!(config.warnings[0].contains("no colour named fokus"));
        assert!(config.warnings[1].contains("not a colour"));
        assert!(config.warnings[2].contains("tab_width"));
        assert!(config.warnings[3].contains("no action named submitt"));
        assert!(config.warnings[4].contains("not a key"));
        assert!(config.warnings[5].contains("unknown section"));
        assert!(config.warnings[6].contains("means nothing here"));
        // Nothing was taken from a file it could not read.
        assert_eq!(config.theme.focus, Color::Yellow);
        assert!(config.keys.is(Key::ch('c'), Action::Submit));
    }

    #[test]
    fn a_shared_key_offers_the_narrower_action_first() {
        let keys = Keymap::default();
        assert_eq!(
            keys.actions(Key::ch('s')),
            [Action::ShelveFiles, Action::ShelveChange]
        );
        assert_eq!(
            keys.actions(Key::ch('d')),
            [Action::RevertFiles, Action::DeleteChange]
        );
    }

    #[test]
    fn every_action_has_a_name_a_key_and_no_duplicates() {
        let mut names: Vec<&str> = Action::ALL.iter().map(|a| a.name()).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "two actions share a name");

        for action in Action::ALL {
            assert!(!action.default_keys().is_empty(), "{action:?} has no key");
            assert_eq!(Action::named(action.name()), Some(action));
        }
    }

    #[test]
    fn a_help_row_names_both_keys_of_a_pair() {
        let keys = Keymap::default();
        assert_eq!(keys.label_for(Action::Down, Some(Action::Up)), "j / k");
        assert_eq!(keys.label_for(Action::Submit, None), "c");
    }

    #[test]
    fn reads_the_keys_a_terminal_cannot_spell_as_characters() {
        assert_eq!(Key::parse("space"), Some(Key::ch(' ')));
        assert_eq!(Key::parse("enter"), Some(Key::plain(KeyCode::Enter)));
        assert_eq!(Key::parse("shift-tab"), Some(Key::plain(KeyCode::BackTab)));
        assert_eq!(Key::parse("["), Some(Key::ch('[')));
        assert_eq!(Key::parse("nonsense"), None);
        assert_eq!(Key::ch(' ').show(), "space");
        assert_eq!(Key::plain(KeyCode::BackTab).show(), "shift-tab");
    }

    #[test]
    fn reads_a_colour_by_name_by_hex_and_by_number() {
        assert_eq!(parse_color("Red"), Some(Color::Red));
        assert_eq!(parse_color("dark_gray"), Some(Color::DarkGray));
        assert_eq!(parse_color("#0a0B0c"), Some(Color::Rgb(10, 11, 12)));
        assert_eq!(parse_color("39"), Some(Color::Indexed(39)));
        assert_eq!(parse_color("#fff"), None);
        assert_eq!(parse_color("puce"), None);
    }
}
