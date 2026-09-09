//! Application state and key routing.

use std::collections::HashSet;

use crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use p4::{ChangeId, ChangeStatus, Changelist, FileDiff, ServerInfo};

use crate::editor::{Editor, Outcome};
use crate::tree;
use crate::worker::{Event, FileEntry, Request, Worker};

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

/// Which half of the Files panel a row belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// Open in the selected changelist.
    InChange,
    /// The default changelist, plus anything the workspace scan found.
    Loose,
}

/// A row of the Files panel.
///
/// Headers only appear when there are two groups. Directories and files are
/// both selectable; headers are not.
pub enum FileRow<'a> {
    Header(String),
    Dir {
        group: Group,
        /// Path below the tree root, which identifies the row for collapsing.
        path: String,
        label: String,
        depth: usize,
        collapsed: bool,
        files: usize,
    },
    File {
        /// Index into [`App::all_files`].
        index: usize,
        entry: &'a FileEntry,
        label: String,
        depth: usize,
    },
}

impl FileRow<'_> {
    fn selectable(&self) -> bool {
        !matches!(self, FileRow::Header(_))
    }
}

/// Where a picked set of files should go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    Existing(ChangeId, String),
    /// Make one first, asking for a description.
    New,
}

/// Choosing a changelist to move files into.
pub struct Picker {
    pub files: Vec<FileEntry>,
    pub options: Vec<Destination>,
    pub sel: usize,
}

/// A question that must be answered before something irreversible happens.
///
/// There is no default answer: only `y` goes ahead, and any other key backs
/// out.
pub struct Confirm {
    pub title: String,
    /// What is about to happen, and to what.
    pub lines: Vec<String>,
    request: Request,
}

/// What the open editor is for.
enum Editing {
    /// Rewrite an existing changelist's description.
    Description(ChangeId),
    /// Describe a changelist that does not exist yet, then move these into it.
    NewChange(Vec<FileEntry>),
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

    /// Files open in the selected changelist.
    pub files: Vec<FileEntry>,
    /// Files open in the default changelist, plus anything the workspace scan
    /// found. Shown below `files` so a file can be moved across.
    pub loose_files: Vec<FileEntry>,
    /// Which changelist `files` belongs to; `None` while a load is in flight.
    pub files_for: Option<ChangeId>,
    /// Index into [`App::all_files`].
    pub file_sel: usize,
    /// Result of the last workspace scan, merged into `loose_files`.
    pub scanned: Vec<FileEntry>,
    pub scanning: bool,
    /// Directory paths whose contents are hidden. Shared by both groups, so a
    /// directory reads the same way wherever it appears.
    collapsed: HashSet<String>,

    /// Diffs for the whole selected changelist, keyed by `diffs_for`.
    pub diffs: Vec<FileDiff>,
    pub diffs_for: Option<ChangeId>,
    pub diff_scroll: usize,
    /// Columns scrolled off the left of the diff, for lines wider than the pane.
    pub diff_hscroll: usize,
    /// Give the diff the whole window instead of the right-hand pane.
    pub diff_fullscreen: bool,

    /// Open description editor, if any. Takes every keystroke while it lives.
    pub editor: Option<Editor>,
    editing: Option<Editing>,
    /// Open changelist picker, if any.
    pub picker: Option<Picker>,
    /// Pending confirmation, if any.
    pub confirm: Option<Confirm>,

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
            loose_files: Vec::new(),
            files_for: None,
            file_sel: 0,
            scanned: Vec::new(),
            scanning: false,
            collapsed: HashSet::new(),
            editor: None,
            editing: None,
            picker: None,
            confirm: None,
            diffs: Vec::new(),
            diffs_for: None,
            diff_scroll: 0,
            diff_hscroll: 0,
            diff_fullscreen: false,
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

    /// Both groups end to end. `file_sel` indexes this.
    pub fn all_files(&self) -> Vec<&FileEntry> {
        self.files.iter().chain(self.loose_files.iter()).collect()
    }

    /// The depot path the tree hangs off. `/` on screen.
    ///
    /// The stream when the workspace has one; otherwise the deepest directory
    /// every listed file shares.
    pub fn depot_root(&self) -> String {
        if let Some(stream) = self.info.as_ref().and_then(|i| i.stream.clone()) {
            return stream;
        }
        tree::common_root(self.all_files().iter().map(|f| f.depot_path.as_str()))
    }

    /// The selected row, or `None` when nothing is selectable.
    pub fn selected_row(&self) -> Option<FileRow<'_>> {
        self.file_rows()
            .into_iter()
            .filter(FileRow::selectable)
            .nth(self.file_sel)
    }

    pub fn selected_file(&self) -> Option<&FileEntry> {
        match self.selected_row()? {
            FileRow::File { index, .. } => self.all_files().get(index).copied(),
            _ => None,
        }
    }

    /// Number of rows the cursor can land on.
    fn selectable_count(&self) -> usize {
        self.file_rows().iter().filter(|r| r.selectable()).count()
    }

    /// Which group a file index belongs to.
    fn group_of(&self, index: usize) -> Group {
        if index < self.files.len() {
            Group::InChange
        } else {
            Group::Loose
        }
    }

    /// The rows to draw. Each group is its own tree; headers separate them and
    /// only appear when there is a second group.
    pub fn file_rows(&self) -> Vec<FileRow<'_>> {
        let root = self.depot_root();
        let all = self.all_files();
        let split = self.files.len();

        let subtree = |group: Group, offset: usize, files: &[FileEntry]| -> Vec<FileRow<'_>> {
            let entries: Vec<tree::Entry> = files
                .iter()
                .enumerate()
                .map(|(i, f)| tree::Entry {
                    index: offset + i,
                    path: tree::relative(&f.depot_path, &root),
                })
                .collect();

            tree::build(&entries, &self.collapsed)
                .into_iter()
                .map(|row| match row.node {
                    tree::Node::Dir {
                        path,
                        files,
                        collapsed,
                    } => FileRow::Dir {
                        group,
                        path,
                        label: row.label,
                        depth: row.depth,
                        collapsed,
                        files,
                    },
                    tree::Node::File { index } => FileRow::File {
                        index,
                        entry: all[index],
                        label: row.label,
                        depth: row.depth,
                    },
                })
                .collect()
        };

        let in_change = subtree(Group::InChange, 0, &self.files);
        if self.loose_files.is_empty() {
            return in_change;
        }

        let heading = match self.selected_change() {
            Some(cl) => format!("In changelist {}", cl.id),
            None => "In changelist".to_owned(),
        };

        let mut rows = vec![FileRow::Header(heading)];
        rows.extend(in_change);
        rows.push(FileRow::Header("Default".to_owned()));
        rows.extend(subtree(Group::Loose, split, &self.loose_files));
        rows
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
            Event::Files {
                change,
                files,
                default_files,
            } => {
                // A stale answer for a changelist we have moved off.
                if self.pending_files == Some(change) {
                    self.pending_files = None;
                    self.files = files;
                    self.loose_files = default_files;
                    self.merge_scanned();
                    self.files_for = Some(change);
                    self.file_sel = 0;
                    self.request_diff();
                }
            }
            Event::Scanned(files) => {
                self.scanning = false;
                self.scanned = files;
                self.merge_scanned();
            }
            Event::Changed => {
                // A write can add or empty the default changelist and can
                // rewrite a description, so reload the lists too, not just the
                // files.
                self.files_for = None;
                self.diffs_for = None;
                self.busy = true;
                self.worker.send(Request::Refresh);
            }
            Event::Diff { change, files } => {
                if self.pending_diff == Some(change) {
                    self.pending_diff = None;
                    self.diffs = files;
                    self.diffs_for = Some(change);
                    self.diff_scroll = 0;
                    self.diff_hscroll = 0;
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

        // The editor swallows everything, including keys that are commands
        // elsewhere — `q` has to be typeable in a description.
        if let Some(editor) = &mut self.editor {
            match editor.handle(key) {
                Outcome::Continue => {}
                Outcome::Cancel => {
                    self.editor = None;
                    self.editing = None;
                }
                Outcome::Save => self.save_editor(),
            }
            return;
        }

        // Only `y` goes ahead. Anything else — including a stray keystroke that
        // means something elsewhere — backs out.
        if let Some(confirm) = self.confirm.take() {
            if key.code == KeyCode::Char('y') {
                self.busy = true;
                self.error = None;
                self.worker.send(confirm.request);
            }
            return;
        }

        if self.picker.is_some() {
            self.pick_key(key.code);
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
            KeyCode::Char('e') => self.edit_description(),
            KeyCode::Char('n') => self.new_changelist(),
            KeyCode::Char('d') if self.focus == Panel::Changelists => self.delete_changelist(),
            KeyCode::Char('u') => {
                if !self.scanning {
                    self.scanning = true;
                    self.worker.send(Request::ScanWorkspace);
                }
            }
            KeyCode::Char(' ') => self.move_selected_file(),
            // On a directory this folds the tree; everywhere else it is the
            // fullscreen diff, as lazygit does with Enter on a folder.
            KeyCode::Enter
                if self.focus == Panel::Files
                    && matches!(self.selected_row(), Some(FileRow::Dir { .. })) =>
            {
                self.toggle_collapse(None);
            }
            KeyCode::Enter => {
                self.diff_fullscreen = !self.diff_fullscreen;
                if self.diff_fullscreen {
                    self.focus = Panel::Diff;
                }
            }
            KeyCode::Esc if self.diff_fullscreen => self.diff_fullscreen = false,

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
            // Long lines are truncated rather than wrapped, so the diff scrolls
            // sideways.
            KeyCode::Char('h') | KeyCode::Left if self.focus == Panel::Diff => {
                self.diff_hscroll = self.diff_hscroll.saturating_sub(8);
            }
            KeyCode::Char('l') | KeyCode::Right if self.focus == Panel::Diff => {
                self.diff_hscroll += 8;
            }
            // Tree navigation, which only the Files panel has.
            KeyCode::Char('h') | KeyCode::Left if self.focus == Panel::Files => {
                self.toggle_collapse(Some(true));
            }
            KeyCode::Char('l') | KeyCode::Right if self.focus == Panel::Files => {
                self.toggle_collapse(Some(false));
            }

            _ => {}
        }
    }

    /// Fold the last workspace scan into the lower group, dropping anything
    /// Perforce has since opened — `p4 status` reports open files too, and they
    /// already appear in one of the groups.
    fn merge_scanned(&mut self) {
        self.loose_files.retain(|f| f.opened);
        let known: Vec<&str> = self
            .files
            .iter()
            .chain(self.loose_files.iter())
            .map(|f| f.depot_path.as_str())
            .collect();
        let fresh: Vec<FileEntry> = self
            .scanned
            .iter()
            .filter(|f| !known.contains(&f.depot_path.as_str()))
            .cloned()
            .collect();
        self.loose_files.extend(fresh);
    }

    /// Create an empty changelist. The same path as creating one to move files
    /// into, with nothing to move.
    fn new_changelist(&mut self) {
        self.error = None;
        self.editor = Some(Editor::new("Description of the new changelist", ""));
        self.editing = Some(Editing::NewChange(Vec::new()));
    }

    /// Delete the selected changelist, which Perforce allows only once it is
    /// empty.
    fn delete_changelist(&mut self) {
        let Some(cl) = self.selected_change() else {
            return;
        };
        if cl.id == ChangeId::Default {
            self.error = Some("the default changelist cannot be deleted".into());
            return;
        }
        if cl.status == ChangeStatus::Submitted {
            self.error = Some("a submitted changelist cannot be deleted".into());
            return;
        }
        // Only trustworthy for the changelist whose files we have actually
        // loaded; otherwise the server refuses and says so.
        if self.files_for == Some(cl.id) && !self.files.is_empty() {
            self.error = Some(format!(
                "changelist {} still holds {} file(s) — move or revert them first",
                cl.id,
                self.files.len()
            ));
            return;
        }

        self.ask(
            format!("Delete changelist {}?", cl.id),
            vec![cl.summary().to_owned()],
            Request::DeleteChange { change: cl.id },
        );
    }

    /// Put a question up before doing something that cannot be undone.
    fn ask(&mut self, title: String, lines: Vec<String>, request: Request) {
        self.error = None;
        self.confirm = Some(Confirm {
            title,
            lines,
            request,
        });
    }

    /// Open the description of the selected changelist for editing.
    fn edit_description(&mut self) {
        let Some(cl) = self.selected_change() else {
            return;
        };
        if cl.id == ChangeId::Default {
            // The default changelist is not a spec and has no description.
            self.error = Some("the default changelist has no description".into());
            return;
        }
        if cl.status == ChangeStatus::Submitted {
            self.error = Some("a submitted changelist cannot be edited".into());
            return;
        }

        self.error = None;
        self.editor = Some(Editor::new(
            format!("Description of {}", cl.id),
            &cl.description,
        ));
        self.editing = Some(Editing::Description(cl.id));
    }

    fn save_editor(&mut self) {
        let Some(editor) = &self.editor else {
            return;
        };
        if editor.is_blank() {
            // Perforce rejects an empty description; say so before the round trip.
            self.error = Some("a description cannot be empty".into());
            return;
        }
        let description = editor.text();

        let request = match self.editing.take() {
            Some(Editing::Description(change)) => Request::SetDescription {
                change,
                description,
            },
            Some(Editing::NewChange(files)) => Request::CreateChange { description, files },
            None => return,
        };

        self.editor = None;
        self.busy = true;
        self.error = None;
        self.worker.send(request);
    }

    /// Move the file under the cursor across the divider.
    ///
    /// Down from the changelist means back to the default one; up from the
    /// default group means into the selected changelist, opening the file first
    /// if the scan found it unopened.
    /// Files the selected row stands for: one for a file, everything beneath
    /// it for a directory.
    fn selected_files(&self) -> Vec<FileEntry> {
        match self.selected_row() {
            Some(FileRow::File { index, .. }) => {
                self.all_files().get(index).map(|f| (*f).clone()).into_iter().collect()
            }
            Some(FileRow::Dir { group, path, .. }) => {
                let root = self.depot_root();
                let prefix = format!("{path}/");
                let source = match group {
                    Group::InChange => &self.files,
                    Group::Loose => &self.loose_files,
                };
                source
                    .iter()
                    .filter(|f| tree::relative(&f.depot_path, &root).starts_with(&prefix))
                    .cloned()
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// Expand or collapse the selected directory. Does nothing on a file.
    fn toggle_collapse(&mut self, want_collapsed: Option<bool>) {
        let Some(FileRow::Dir { path, collapsed, .. }) = self.selected_row() else {
            return;
        };
        let collapse = want_collapsed.unwrap_or(!collapsed);
        if collapse {
            self.collapsed.insert(path);
        } else {
            self.collapsed.remove(&path);
        }
        // Rows above the cursor never move, so the selection stays put.
        self.file_sel = self.file_sel.min(self.selectable_count().saturating_sub(1));
    }

    fn move_selected_file(&mut self) {
        if self.focus != Panel::Files {
            return;
        }
        let files = self.selected_files();
        if files.is_empty() {
            return;
        }
        let loose = matches!(
            self.selected_row(),
            Some(FileRow::File { index, .. }) if self.group_of(index) == Group::Loose
        ) || matches!(
            self.selected_row(),
            Some(FileRow::Dir { group: Group::Loose, .. })
        );
        let Some(cl) = self.selected_change() else {
            return;
        };

        if cl.status == ChangeStatus::Submitted {
            self.error = Some("a submitted changelist cannot be changed".into());
            return;
        }
        // Looking at the default changelist, there is no second group and so no
        // implied destination: ask which changelist to move into.
        if cl.id == ChangeId::Default {
            self.open_picker(files);
            return;
        }

        let target = if loose { cl.id } else { ChangeId::Default };
        self.busy = true;
        self.error = None;
        self.worker.send(Request::MoveFiles {
            change: target,
            files,
        });
    }

    /// Offer the numbered changelists these files could move into, plus the
    /// option of a new one.
    fn open_picker(&mut self, files: Vec<FileEntry>) {
        let mut options: Vec<Destination> = self
            .pending
            .iter()
            .filter(|cl| self.is_mine(cl) && cl.id != ChangeId::Default)
            .map(|cl| Destination::Existing(cl.id, cl.summary().to_owned()))
            .collect();
        options.push(Destination::New);

        self.error = None;
        self.picker = Some(Picker {
            files,
            options,
            sel: 0,
        });
    }

    fn pick_key(&mut self, code: KeyCode) {
        let Some(picker) = &mut self.picker else {
            return;
        };
        match code {
            KeyCode::Esc | KeyCode::Char('q') => self.picker = None,
            KeyCode::Char('j') | KeyCode::Down => {
                picker.sel = (picker.sel + 1).min(picker.options.len() - 1);
            }
            KeyCode::Char('k') | KeyCode::Up => picker.sel = picker.sel.saturating_sub(1),
            KeyCode::Char('g') | KeyCode::Home => picker.sel = 0,
            KeyCode::Char('G') | KeyCode::End => picker.sel = picker.options.len() - 1,
            KeyCode::Enter | KeyCode::Char(' ') => self.confirm_pick(),
            _ => {}
        }
    }

    fn confirm_pick(&mut self) {
        let Some(picker) = self.picker.take() else {
            return;
        };
        match picker.options.get(picker.sel).cloned() {
            Some(Destination::Existing(change, _)) => {
                self.busy = true;
                self.worker.send(Request::MoveFiles {
                    change,
                    files: picker.files,
                });
            }
            Some(Destination::New) => {
                // A changelist cannot exist without a description, so ask for
                // one before creating anything.
                self.editor = Some(Editor::new("Description of the new changelist", ""));
                self.editing = Some(Editing::NewChange(picker.files));
            }
            None => {}
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
            Panel::Files => (self.file_sel, self.selectable_count()),
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
            Panel::Files => self.selectable_count(),
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
                    self.diff_hscroll = 0;
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
            self.loose_files.clear();
            self.files_for = None;
            self.diffs.clear();
            self.diffs_for = None;
            return;
        };
        if self.files_for == Some(cl.id) || self.pending_files == Some(cl.id) {
            return;
        }

        self.files.clear();
        self.loose_files.clear();
        self.files_for = None;
        self.file_sel = 0;
        self.diffs.clear();
        self.diffs_for = None;
        self.diff_scroll = 0;
        self.diff_hscroll = 0;
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
        self.diff_hscroll = 0;
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

    /// The most recent request sent to the worker, for the tests.
    #[cfg(test)]
    pub fn last_request(&self) -> Option<Request> {
        self.worker.last_request()
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
