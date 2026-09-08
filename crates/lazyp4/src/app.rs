//! Application state and key routing.

use crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use p4::{diff, ChangeId, ChangeStatus, Changelist, FileDiff, ServerInfo};

use crate::worker::{Event, FileEntry, Request, Worker};

/// Something the main loop must do outside the alternate screen.
pub enum Action {
    /// Hand this patch to the external viewer.
    OpenInHunk(String),
}

/// The panels, in tab order. The layout runs top to bottom down the left
/// column, with the diff filling the right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Status,
    Files,
    Changelists,
    History,
    Diff,
}

impl Panel {
    pub const ORDER: [Panel; 5] = [
        Panel::Status,
        Panel::Files,
        Panel::Changelists,
        Panel::History,
        Panel::Diff,
    ];

    fn step(self, by: isize) -> Panel {
        let i = Self::ORDER.iter().position(|p| *p == self).unwrap_or(0) as isize;
        let n = Self::ORDER.len() as isize;
        Self::ORDER[((i + by).rem_euclid(n)) as usize]
    }

    /// The key that focuses this panel. The diff takes `0` because it sits
    /// apart from the numbered column.
    pub fn number(self) -> u8 {
        match self {
            Panel::Status => 1,
            Panel::Files => 2,
            Panel::Changelists => 3,
            Panel::History => 4,
            Panel::Diff => 0,
        }
    }

    pub fn from_number(n: u8) -> Option<Panel> {
        Self::ORDER.into_iter().find(|p| p.number() == n)
    }

    pub fn title(self) -> &'static str {
        match self {
            Panel::Status => "Status",
            Panel::Files => "Files",
            Panel::Changelists => "Changelists",
            Panel::History => "History",
            Panel::Diff => "Diff",
        }
    }
}

/// Tabs of the Changelists panel, cycled with `[` and `]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeTab {
    /// Pending on this workspace, with nothing shelved.
    Local,
    /// Pending on this workspace, with content shelved on the server.
    Shelved,
    /// Pending on somebody else's workspace.
    Others,
}

impl ChangeTab {
    pub const ORDER: [ChangeTab; 3] = [ChangeTab::Local, ChangeTab::Shelved, ChangeTab::Others];

    fn step(self, by: isize) -> ChangeTab {
        let i = Self::ORDER.iter().position(|t| *t == self).unwrap_or(0) as isize;
        let n = Self::ORDER.len() as isize;
        Self::ORDER[((i + by).rem_euclid(n)) as usize]
    }

    pub fn title(self) -> &'static str {
        match self {
            ChangeTab::Local => "Local",
            ChangeTab::Shelved => "Shelved",
            ChangeTab::Others => "Others",
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

    /// Every pending changelist; the tabs are views onto this.
    pub pending: Vec<Changelist>,
    pub tab: ChangeTab,
    pub change_sel: usize,

    /// Submitted changelists, newest first.
    pub submitted: Vec<Changelist>,
    pub history_sel: usize,

    /// Whether Changelists or History last moved, and so which one the Files
    /// and Diff panels are following.
    pub change_source: Panel,

    pub files: Vec<FileEntry>,
    /// Which changelist `files` belongs to; `None` while a load is in flight.
    pub files_for: Option<ChangeId>,
    pub file_sel: usize,

    /// Diffs for the whole selected changelist, keyed by `diffs_for`.
    pub diffs: Vec<FileDiff>,
    pub diffs_for: Option<ChangeId>,
    pub diff_scroll: usize,

    /// Work for the main loop to do once the terminal is released.
    pub action: Option<Action>,

    /// Every command the worker ran, newest last.
    pub log: Vec<String>,
    /// The last error, shown in the status bar until something replaces it.
    pub error: Option<String>,

    worker: Worker,
    pending_files: Option<ChangeId>,
    pending_diff: Option<ChangeId>,
}

impl App {
    pub fn new(worker: Worker) -> Self {
        worker.send(Request::Refresh);
        App {
            focus: Panel::Files,
            modal: Modal::None,
            quit: false,
            busy: true,
            info: None,
            pending: Vec::new(),
            tab: ChangeTab::Local,
            change_sel: 0,
            submitted: Vec::new(),
            history_sel: 0,
            change_source: Panel::Changelists,
            files: Vec::new(),
            files_for: None,
            file_sel: 0,
            diffs: Vec::new(),
            diffs_for: None,
            diff_scroll: 0,
            action: None,
            log: Vec::new(),
            error: None,
            worker,
            pending_files: None,
            pending_diff: None,
        }
    }

    /// Our own user name.
    fn my_user(&self) -> &str {
        self.info.as_ref().map(|i| i.user.as_str()).unwrap_or_default()
    }

    /// Our workspace, when the server could resolve one.
    ///
    /// Empty when P4CLIENT does not name a real client — which happens simply
    /// by running lazyp4 outside a workspace.
    pub fn my_client(&self) -> &str {
        self.info
            .as_ref()
            .filter(|i| i.client_known)
            .map(|i| i.client.as_str())
            .unwrap_or_default()
    }

    /// A changelist is ours if our user owns it.
    ///
    /// Deliberately not keyed on the client: a user commonly has several
    /// workspaces, and lazyp4 started outside one has no client name at all,
    /// which would otherwise put every changelist under Others.
    fn is_mine(&self, cl: &Changelist) -> bool {
        !self.my_user().is_empty() && cl.user == self.my_user()
    }

    fn belongs_in(&self, cl: &Changelist, tab: ChangeTab) -> bool {
        let mine = self.is_mine(cl);
        match tab {
            ChangeTab::Local => mine && !cl.shelved,
            ChangeTab::Shelved => mine && cl.shelved,
            ChangeTab::Others => !mine,
        }
    }

    /// Pending changelists shown by the current tab.
    pub fn tab_changes(&self) -> Vec<&Changelist> {
        self.pending
            .iter()
            .filter(|cl| self.belongs_in(cl, self.tab))
            .collect()
    }

    /// How many changelists each tab holds, for the tab bar.
    pub fn tab_count(&self, tab: ChangeTab) -> usize {
        self.pending
            .iter()
            .filter(|cl| self.belongs_in(cl, tab))
            .count()
    }

    /// The changelist the Files and Diff panels are following.
    pub fn selected_change(&self) -> Option<Changelist> {
        match self.change_source {
            Panel::History => self.submitted.get(self.history_sel).cloned(),
            _ => self.tab_changes().get(self.change_sel).map(|cl| (*cl).clone()),
        }
    }

    pub fn selected_file(&self) -> Option<&FileEntry> {
        self.files.get(self.file_sel)
    }

    /// The diff of the file under the cursor, if it has been fetched.
    pub fn selected_diff(&self) -> Option<&FileDiff> {
        let file = self.selected_file()?;
        self.diffs.iter().find(|d| d.depot_path == file.depot_path)
    }

    pub fn handle(&mut self, event: Event) {
        match event {
            Event::Input(TermEvent::Key(key)) => self.on_key(key),
            Event::Input(_) => {}
            Event::Info(info) => {
                self.info = Some(info);
                // Tab membership depends on knowing our own client name.
                self.follow_selection();
            }
            Event::Changes { pending, submitted } => {
                self.pending = pending;
                self.submitted = submitted;
                self.clamp_selections();
                self.follow_selection();
            }
            Event::Files { change, files } => {
                // A stale answer for a changelist we have moved off.
                if self.pending_files == Some(change) {
                    self.pending_files = None;
                    self.files = files;
                    self.files_for = Some(change);
                    self.file_sel = 0;
                    self.request_diff();
                }
            }
            Event::Diff { change, files } => {
                if self.pending_diff == Some(change) {
                    self.pending_diff = None;
                    self.diffs = files;
                    self.diffs_for = Some(change);
                    self.diff_scroll = 0;
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
                self.files_for = None;
                self.diffs_for = None;
                self.worker.send(Request::Refresh);
            }
            KeyCode::Enter => {
                let patch = diff::to_unified(&self.diffs);
                if patch.is_empty() {
                    self.error = Some("nothing to diff in this changelist".into());
                } else {
                    self.action = Some(Action::OpenInHunk(patch));
                }
            }

            KeyCode::Tab => self.focus = self.focus.step(1),
            KeyCode::BackTab => self.focus = self.focus.step(-1),
            // Within a panel rather than between panels: only Changelists has
            // tabs today, so elsewhere these do nothing.
            KeyCode::Char(']') => self.switch_tab(1),
            KeyCode::Char('[') => self.switch_tab(-1),
            KeyCode::Char(c @ '0'..='9') => {
                if let Some(panel) = Panel::from_number(c as u8 - b'0') {
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

    fn switch_tab(&mut self, by: isize) {
        if self.focus != Panel::Changelists {
            return;
        }
        self.tab = self.tab.step(by);
        self.change_sel = 0;
        self.change_source = Panel::Changelists;
        self.follow_selection();
    }

    fn move_by(&mut self, delta: isize) {
        if self.focus == Panel::Diff {
            self.diff_scroll = self.diff_scroll.saturating_add_signed(delta);
            return;
        }
        let (sel, len) = match self.focus {
            Panel::Files => (self.file_sel, self.files.len()),
            Panel::Changelists => (self.change_sel, self.tab_changes().len()),
            Panel::History => (self.history_sel, self.submitted.len()),
            Panel::Status | Panel::Diff => return,
        };
        if len == 0 {
            return;
        }
        let next = (sel as isize + delta).clamp(0, len as isize - 1) as usize;
        self.set_selection(next);
    }

    fn move_to(&mut self, index: usize) {
        if self.focus == Panel::Diff {
            // `G` on a diff means "as far down as it goes"; the renderer clamps.
            self.diff_scroll = if index == usize::MAX { usize::MAX } else { 0 };
            return;
        }
        let len = match self.focus {
            Panel::Files => self.files.len(),
            Panel::Changelists => self.tab_changes().len(),
            Panel::History => self.submitted.len(),
            Panel::Status | Panel::Diff => return,
        };
        if len == 0 {
            return;
        }
        self.set_selection(index.min(len - 1));
    }

    fn set_selection(&mut self, index: usize) {
        match self.focus {
            Panel::Files => {
                if self.file_sel != index {
                    self.file_sel = index;
                    // The pane shows one file at a time, so its scroll is per file.
                    self.diff_scroll = 0;
                }
            }
            Panel::Changelists => {
                if self.change_sel != index || self.change_source != Panel::Changelists {
                    self.change_sel = index;
                    self.change_source = Panel::Changelists;
                    self.follow_selection();
                }
            }
            Panel::History => {
                if self.history_sel != index || self.change_source != Panel::History {
                    self.history_sel = index;
                    self.change_source = Panel::History;
                    self.follow_selection();
                }
            }
            Panel::Status | Panel::Diff => {}
        }
    }

    fn clamp_selections(&mut self) {
        self.change_sel = self
            .change_sel
            .min(self.tab_changes().len().saturating_sub(1));
        self.history_sel = self.history_sel.min(self.submitted.len().saturating_sub(1));
    }

    /// Point the Files panel at whichever changelist is now selected.
    fn follow_selection(&mut self) {
        let Some(cl) = self.selected_change() else {
            self.files.clear();
            self.files_for = None;
            self.diffs.clear();
            self.diffs_for = None;
            return;
        };
        if self.files_for == Some(cl.id) || self.pending_files == Some(cl.id) {
            return;
        }

        self.files.clear();
        self.files_for = None;
        self.file_sel = 0;
        self.diffs.clear();
        self.diffs_for = None;
        self.diff_scroll = 0;
        self.pending_files = Some(cl.id);
        self.busy = true;
        self.worker.send(Request::LoadFiles {
            change: cl.id,
            status: cl.status,
            shelved: cl.shelved,
        });
    }

    /// Ask for the whole changelist's diff once its file list is known.
    fn request_diff(&mut self) {
        let Some(cl) = self.selected_change() else {
            return;
        };
        if self.diffs_for == Some(cl.id) || self.pending_diff == Some(cl.id) {
            return;
        }

        self.diffs.clear();
        self.diffs_for = None;
        self.diff_scroll = 0;
        self.pending_diff = Some(cl.id);
        self.busy = true;
        self.worker.send(Request::LoadDiff {
            change: cl.id,
            status: cl.status,
            shelved: cl.shelved,
            files: self.files.clone(),
        });
    }

    pub fn shutdown(&self) {
        self.worker.send(Request::Shutdown);
    }
}

/// Marker shown beside a changelist in a list.
pub fn change_marker(cl: &Changelist) -> char {
    match cl.status {
        ChangeStatus::Submitted => '✓',
        _ if cl.shelved => '⌸',
        _ => '▸',
    }
}
