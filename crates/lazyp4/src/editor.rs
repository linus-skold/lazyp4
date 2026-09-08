//! A small multi-line text editor for the description popup.
//!
//! Deliberately minimal: enough to retype a changelist description without
//! leaving lazyp4. Lines are held separately and indexed by character, not
//! byte, so non-ASCII text behaves.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub struct Editor {
    pub title: String,
    lines: Vec<String>,
    /// Cursor line, and column in characters.
    row: usize,
    col: usize,
}

/// What a keystroke asked the editor to do.
pub enum Outcome {
    /// Still editing.
    Continue,
    Save,
    Cancel,
}

impl Editor {
    pub fn new(title: impl Into<String>, text: &str) -> Self {
        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        let row = lines.len() - 1;
        let col = lines[row].chars().count();
        Editor {
            title: title.into(),
            lines,
            row,
            col,
        }
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Cursor position as (line, column), both zero-based.
    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// True when there is nothing worth saving.
    pub fn is_blank(&self) -> bool {
        self.text().trim().is_empty()
    }

    pub fn handle(&mut self, key: KeyEvent) -> Outcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return Outcome::Cancel,
            // Ctrl-C leaves the popup rather than the application: quitting
            // mid-edit would throw the text away without saying so.
            KeyCode::Char('c') if ctrl => return Outcome::Cancel,

            // A bare Enter saves; Shift-Enter and friends add a newline.
            // Ctrl-J stays as a fallback for terminals that report a modified
            // Enter as a plain one.
            KeyCode::Enter if key.modifiers.is_empty() => return Outcome::Save,
            KeyCode::Enter => self.split_line(),
            KeyCode::Char('j') if ctrl => self.split_line(),

            KeyCode::Char(c) => self.insert(c),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),

            KeyCode::Left => self.move_left(),
            KeyCode::Right => self.move_right(),
            KeyCode::Up => self.move_vertically(-1),
            KeyCode::Down => self.move_vertically(1),
            KeyCode::Home => self.col = 0,
            KeyCode::End => self.col = self.line_len(self.row),
            _ => {}
        }
        Outcome::Continue
    }

    fn line_len(&self, row: usize) -> usize {
        self.lines[row].chars().count()
    }

    /// Byte offset of character `col` on the current line.
    fn byte_at(&self, row: usize, col: usize) -> usize {
        self.lines[row]
            .char_indices()
            .nth(col)
            .map(|(i, _)| i)
            .unwrap_or(self.lines[row].len())
    }

    fn insert(&mut self, c: char) {
        let at = self.byte_at(self.row, self.col);
        self.lines[self.row].insert(at, c);
        self.col += 1;
    }

    fn split_line(&mut self) {
        let at = self.byte_at(self.row, self.col);
        let tail = self.lines[self.row].split_off(at);
        self.lines.insert(self.row + 1, tail);
        self.row += 1;
        self.col = 0;
    }

    fn backspace(&mut self) {
        if self.col > 0 {
            let at = self.byte_at(self.row, self.col - 1);
            self.lines[self.row].remove(at);
            self.col -= 1;
        } else if self.row > 0 {
            // Join this line onto the end of the one above.
            let line = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.line_len(self.row);
            self.lines[self.row].push_str(&line);
        }
    }

    fn delete(&mut self) {
        if self.col < self.line_len(self.row) {
            let at = self.byte_at(self.row, self.col);
            self.lines[self.row].remove(at);
        } else if self.row + 1 < self.lines.len() {
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&next);
        }
    }

    fn move_left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.line_len(self.row);
        }
    }

    fn move_right(&mut self) {
        if self.col < self.line_len(self.row) {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    fn move_vertically(&mut self, delta: isize) {
        let next = (self.row as isize + delta).clamp(0, self.lines.len() as isize - 1) as usize;
        self.row = next;
        // Keep the column inside the new line.
        self.col = self.col.min(self.line_len(self.row));
    }
}
