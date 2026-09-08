//! Application state and key routing.

use crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use p4::{ChangeId, Changelist, ServerInfo};

use crate::worker::{Event, FileEntry, Request, Worker};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Changelists,
    Files,
    Status,
    Diff,
}

impl Panel {
    /// Tab order, and the source of the number key each panel answers to: the
    /// panel at index 0 is `1`. Keep them in step — the number is drawn in the
    /// panel's own title.
    pub const ORDER: [Panel; 4] = [
        Panel::Changelists,
        Panel::Files,
        Panel::Status,
        Panel::Diff,
    ];

    fn index(self) -> usize {
        Self::ORDER.iter().position(|p| *p == self).unwrap_or(0)
    }

    fn step(self, by: isize) -> Panel {
        let n = Self::ORDER.len() as isize;
        Self::ORDER[((self.index() as isize + by).rem_euclid(n)) as usize]
    }

    /// The key that focuses this panel, as a digit.
    pub fn number(self) -> usize {
        self.index() + 1
    }

    pub fn from_number(n: usize) -> Option<Panel> {
        Self::ORDER.get(n.checked_sub(1)?).copied()
    }

    pub fn title(self) -> &'static str {
        match self {
            Panel::Changelists => "Changelists",
            Panel::Files => "Files",
            Panel::Status => "Status",
            Panel::Diff => "Diff",
        }
    }
}

/// Which full-screen overlay is open, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modal {
    None,
    Help,
    Log,
}

pub struct App {
    pub focus: Panel,
    pub modal: Modal,
    pub quit: bool,
    pub busy: bool,

    pub info: Option<ServerInfo>,
    pub changes: Vec<Changelist>,
    pub change_sel: usize,

    pub files: Vec<FileEntry>,
    /// Which changelist `files` belongs to; `None` while a load is in flight.
    pub files_for: Option<ChangeId>,
    pub file_sel: usize,

    /// Every command the worker ran, newest last.
    pub log: Vec<String>,
    /// The last error, shown in the status bar until something replaces it.
    pub error: Option<String>,

    worker: Worker,
    /// Set while a file load has been asked for but not answered.
    pending_files: Option<ChangeId>,
}

impl App {
    pub fn new(worker: Worker) -> Self {
        worker.send(Request::Refresh);
        App {
            focus: Panel::Changelists,
            modal: Modal::None,
            quit: false,
            busy: true,
            info: None,
            changes: Vec::new(),
            change_sel: 0,
            files: Vec::new(),
            files_for: None,
            file_sel: 0,
            log: Vec::new(),
            error: None,
            worker,
            pending_files: None,
        }
    }

    pub fn selected_change(&self) -> Option<&Changelist> {
        self.changes.get(self.change_sel)
    }

    pub fn selected_file(&self) -> Option<&FileEntry> {
        self.files.get(self.file_sel)
    }

    pub fn handle(&mut self, event: Event) {
        match event {
            Event::Input(TermEvent::Key(key)) => self.on_key(key),
            Event::Input(_) => {}
            Event::Info(info) => self.info = Some(info),
            Event::Changes(changes) => {
                self.changes = changes;
                self.change_sel = self.change_sel.min(self.changes.len().saturating_sub(1));
                self.request_files();
            }
            Event::Files { change, files } => {
                // A stale answer for a changelist we have moved off.
                if self.pending_files == Some(change) {
                    self.pending_files = None;
                    self.files = files;
                    self.files_for = Some(change);
                    self.file_sel = 0;
                }
            }
            Event::Log(cmd) => {
                self.log.push(cmd);
                // The log is a debugging aid, not a transcript to keep forever.
                if self.log.len() > 500 {
                    self.log.drain(..self.log.len() - 500);
                }
            }
            Event::Error(msg) => self.error = Some(msg),
            Event::Idle => self.busy = false,
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        // Windows sends both press and release; act on press only.
        if key.kind != KeyEventKind::Press {
            return;
        }

        if self.modal != Modal::None {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.modal = Modal::None,
                KeyCode::Char('?') if self.modal == Modal::Help => self.modal = Modal::None,
                KeyCode::Char('x') if self.modal == Modal::Log => self.modal = Modal::None,
                _ => {}
            }
            return;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.modal = Modal::Help,
            KeyCode::Char('x') => self.modal = Modal::Log,
            KeyCode::Char('r') => {
                self.busy = true;
                self.error = None;
                self.worker.send(Request::Refresh);
            }

            KeyCode::Tab | KeyCode::Char(']') => self.focus = self.focus.step(1),
            KeyCode::BackTab | KeyCode::Char('[') => self.focus = self.focus.step(-1),
            KeyCode::Char(c @ '1'..='9') => {
                if let Some(panel) = Panel::from_number(c as usize - '0' as usize) {
                    self.focus = panel;
                }
            }

            KeyCode::Char('j') | KeyCode::Down => self.move_by(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_by(-1),
            KeyCode::Char('g') | KeyCode::Home => self.move_to(0),
            KeyCode::Char('G') | KeyCode::End => self.move_to(usize::MAX),
            KeyCode::PageDown => self.move_by(10),
            KeyCode::PageUp => self.move_by(-10),

            _ => {}
        }
    }

    fn move_by(&mut self, delta: isize) {
        let (sel, len) = match self.focus {
            Panel::Changelists => (self.change_sel, self.changes.len()),
            Panel::Files => (self.file_sel, self.files.len()),
            Panel::Status | Panel::Diff => return,
        };
        if len == 0 {
            return;
        }
        let next = (sel as isize + delta).clamp(0, len as isize - 1) as usize;
        self.set_selection(next);
    }

    fn move_to(&mut self, index: usize) {
        let len = match self.focus {
            Panel::Changelists => self.changes.len(),
            Panel::Files => self.files.len(),
            Panel::Status | Panel::Diff => return,
        };
        if len == 0 {
            return;
        }
        self.set_selection(index.min(len - 1));
    }

    fn set_selection(&mut self, index: usize) {
        match self.focus {
            Panel::Changelists => {
                if self.change_sel != index {
                    self.change_sel = index;
                    self.request_files();
                }
            }
            Panel::Files => self.file_sel = index,
            Panel::Status | Panel::Diff => {}
        }
    }

    /// Ask the worker for the selected changelist's files, unless we already
    /// have them.
    fn request_files(&mut self) {
        let Some((change, status, shelved)) = self
            .selected_change()
            .map(|cl| (cl.id, cl.status, cl.shelved))
        else {
            self.files.clear();
            self.files_for = None;
            return;
        };
        if self.files_for == Some(change) || self.pending_files == Some(change) {
            return;
        }

        self.files.clear();
        self.files_for = None;
        self.file_sel = 0;
        self.pending_files = Some(change);
        self.busy = true;
        self.worker.send(Request::LoadFiles {
            change,
            status,
            shelved,
        });
    }

    pub fn shutdown(&self) {
        self.worker.send(Request::Shutdown);
    }
}
