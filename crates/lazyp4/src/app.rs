//! Application state and key routing.

use std::collections::{HashMap, HashSet};

use crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyEventKind};
use p4::{
    AnnotatedLine, ChangeId, ChangeStatus, Changelist, FileDiff, Resolution, RevertPreview,
    Revision, ServerInfo, Stream, Unresolved,
};

use crate::config::{Action, Config, Key};
use crate::editor::{Editor, Outcome};
use crate::tree;
use crate::worker::{Event, FileEntry, PostCreate, Request, Worker};

/// The panels, in tab order. The layout runs top to bottom down the left
/// column, with the diff filling the right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// Pending on another workspace, ours or somebody else's.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// What is being placed into the chosen changelist.
    pub what: PostCreate,
    pub title: String,
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
    /// Describe a changelist that does not exist yet, then fill it.
    NewChange(PostCreate),
    /// Describe the default changelist, which has no description of its own,
    /// so that it can be submitted.
    SubmitDefault,
}

/// Which full-screen overlay is open, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modal {
    None,
    Help,
    Log,
    /// Revision history of one file.
    History,
    /// Who last wrote each line of one file.
    Blame,
    /// Files that must be resolved before they can be submitted.
    Resolve,
    /// The depot's streams, and which one this workspace is on.
    Streams,
}

pub struct App {
    pub config: Config,
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
    /// Directory paths whose contents are hidden, per group. The two groups
    /// are separate trees: the same directory can hold different files in
    /// each, so folding one must not fold the other.
    collapsed: HashSet<(Group, String)>,

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

    /// Revision history of the file the History modal is showing.
    pub history: Vec<Revision>,
    pub history_path: String,
    /// Cursor within those revisions. Distinct from `history_sel`, which is the
    /// History *panel*'s cursor over submitted changelists.
    pub history_rev_sel: usize,

    /// Line-by-line authorship of the file the Blame modal is showing.
    pub blame: Vec<AnnotatedLine>,
    pub blame_path: String,
    pub blame_sel: usize,

    /// Where a range selection was started, if one is open. The range runs
    /// from here to the cursor, in either direction.
    pub select_anchor: Option<usize>,

    /// Files waiting to be resolved, and the cursor within them.
    pub unresolved: Vec<Unresolved>,
    pub unresolved_sel: usize,

    /// Streams in the depot, and the cursor within them.
    pub streams: Vec<Stream>,
    pub streams_sel: usize,
    /// Something that went right, shown until something replaces it.
    pub notice: Option<String>,

    /// Text each list panel is filtered by. Kept per panel so moving between
    /// them does not lose what you narrowed to.
    filters: HashMap<Panel, String>,
    /// The panel whose filter is being typed, if any.
    pub filtering: Option<Panel>,

    /// Give the focused left-hand panel the whole column.
    pub zoom: bool,

    /// Advances while a command is in flight, so the spinner turns.
    pub spinner: usize,

    /// What was open in the workspace when it was last looked at. `None` until
    /// the first poll answers, so starting up is not read as a change.
    external: Option<String>,

    /// Every command the worker ran, newest last.
    pub log: Vec<String>,
    /// The last error, shown in the status bar until something replaces it.
    pub error: Option<String>,

    worker: Worker,
    pending_files: Option<ChangeId>,
    pending_diff: Option<ChangeId>,
    /// How many files a revert in flight left alone because they are not open.
    /// `Some` only while the server's preview is awaited.
    pending_revert: Option<usize>,
}

impl App {
    pub fn new(worker: Worker, config: Config) -> Self {
        worker.send(Request::Refresh);
        // A config the reader did not understand has to say so: the alternative
        // is a key that silently does nothing.
        let error = match config.warnings.len() {
            0 => None,
            1 => Some(config.warnings[0].clone()),
            n => Some(format!("{} (and {} more)", config.warnings[0], n - 1)),
        };
        App {
            config,
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
            history: Vec::new(),
            history_path: String::new(),
            history_rev_sel: 0,
            blame: Vec::new(),
            blame_path: String::new(),
            blame_sel: 0,
            select_anchor: None,
            unresolved: Vec::new(),
            unresolved_sel: 0,
            streams: Vec::new(),
            streams_sel: 0,
            notice: None,
            zoom: false,
            spinner: 0,
            external: None,
            filters: HashMap::new(),
            filtering: None,
            diffs: Vec::new(),
            diffs_for: None,
            diff_scroll: 0,
            diff_hscroll: 0,
            diff_fullscreen: false,
            log: Vec::new(),
            error,
            worker,
            pending_files: None,
            pending_diff: None,
            pending_revert: None,
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

    /// A changelist is on this workspace if it names our client.
    ///
    /// Perforce ties a pending changelist to one client, so this is also what
    /// decides whether we can shelve it, submit it, or move files into it.
    /// Started outside a workspace the server resolves no client, so fall back
    /// to the user rather than disowning everything.
    fn is_here(&self, cl: &Changelist) -> bool {
        match self.my_client() {
            "" => !self.my_user().is_empty() && cl.user == self.my_user(),
            client => cl.client == client,
        }
    }

    /// Why a changelist is out of reach, for the actions that need it here.
    fn not_here(&self, cl: &Changelist) -> &'static str {
        if cl.user == self.my_user() {
            "that changelist is on another workspace"
        } else {
            "that changelist belongs to somebody else"
        }
    }

    fn belongs_in(&self, cl: &Changelist, tab: ChangeTab) -> bool {
        let here = self.is_here(cl);
        match tab {
            ChangeTab::Local => here && !cl.shelved,
            ChangeTab::Shelved => here && cl.shelved,
            ChangeTab::Others => !here,
        }
    }

    /// What a panel is currently narrowed to.
    pub fn filter(&self, panel: Panel) -> &str {
        self.filters.get(&panel).map(String::as_str).unwrap_or("")
    }

    /// Case-insensitive substring match, which is what `/` is for: narrowing a
    /// long list quickly, not writing a pattern.
    fn matches(filter: &str, haystack: &str) -> bool {
        filter.is_empty() || haystack.to_lowercase().contains(&filter.to_lowercase())
    }

    fn changelist_matches(&self, panel: Panel, cl: &Changelist) -> bool {
        let filter = self.filter(panel);
        Self::matches(filter, &cl.id.to_string())
            || Self::matches(filter, &cl.user)
            || Self::matches(filter, &cl.description)
    }

    /// Pending changelists shown by the current tab, after any filter.
    pub fn tab_changes(&self) -> Vec<&Changelist> {
        self.pending
            .iter()
            .filter(|cl| self.belongs_in(cl, self.tab))
            .filter(|cl| self.changelist_matches(Panel::Changelists, cl))
            .collect()
    }

    /// Submitted changelists after any filter.
    pub fn visible_submitted(&self) -> Vec<&Changelist> {
        self.submitted
            .iter()
            .filter(|cl| self.changelist_matches(Panel::History, cl))
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
            Panel::History => self.visible_submitted().get(self.history_sel).map(|cl| (*cl).clone()),
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
            let filter = self.filter(Panel::Files);
            let entries: Vec<tree::Entry> = files
                .iter()
                .enumerate()
                .filter(|(_, f)| Self::matches(filter, &f.depot_path))
                .map(|(i, f)| tree::Entry {
                    index: offset + i,
                    path: tree::relative(&f.depot_path, &root),
                })
                .collect();

            tree::build(&entries, &|path: &str| {
                self.collapsed.contains(&(group, path.to_owned()))
            })
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

    /// Nothing arrived; only the spinner moves.
    pub fn tick(&mut self) {
        self.spinner = self.spinner.wrapping_add(1);
    }

    /// Look for `p4` having been used outside lazyp4.
    ///
    /// Only while the app is sitting still: an answer that arrives mid-edit or
    /// mid-question would move the ground under whatever is being decided.
    pub fn poll_external(&mut self) {
        if self.busy
            || self.modal != Modal::None
            || self.editor.is_some()
            || self.picker.is_some()
            || self.confirm.is_some()
            || self.filtering.is_some()
        {
            return;
        }
        self.worker.send(Request::CheckExternal);
    }

    /// Reload if the workspace has moved since the last look.
    fn saw_external(&mut self, fingerprint: String) {
        let first = self.external.is_none();
        if self.external.as_deref() == Some(fingerprint.as_str()) {
            return;
        }
        self.external = Some(fingerprint);
        if first {
            return;
        }

        self.notice = Some("the workspace changed outside lazyp4 — reloaded".into());
        self.files_for = None;
        self.diffs_for = None;
        self.busy = true;
        self.worker.send(Request::Refresh);
    }

    /// The command currently running, for the busy line.
    pub fn running(&self) -> Option<&str> {
        self.log.last().map(String::as_str)
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
            Event::History {
                depot_path,
                revisions,
            } => {
                // Ignore an answer for a file the cursor has since left.
                if self.history_path == depot_path {
                    self.history = revisions;
                    self.history_rev_sel = 0;
                }
            }
            Event::Blame { depot_path, lines } => {
                if self.blame_path == depot_path {
                    self.blame = lines;
                    self.blame_sel = 0;
                }
            }
            Event::Streams(streams) => {
                self.streams = streams;
                self.streams_sel = self.streams_sel.min(self.streams.len().saturating_sub(1));
            }
            Event::Notice(text) => {
                self.notice = Some(text);
                self.error = None;
            }
            Event::RevertPreview { files, preview } => self.confirm_revert(files, preview),
            Event::Unresolved(files) => {
                self.unresolved = files;
                self.unresolved_sel = self
                    .unresolved_sel
                    .min(self.unresolved.len().saturating_sub(1));
            }
            Event::Scanned(files) => {
                self.scanning = false;
                self.scanned = files;
                self.merge_scanned();
            }
            Event::External(fingerprint) => self.saw_external(fingerprint),
            Event::Ignored {
                depot_path,
                pattern,
                file,
            } => {
                // The rule only takes effect on the next scan, so drop the row
                // now rather than leaving it there looking unignored.
                self.scanned.retain(|f| f.depot_path != depot_path);
                self.merge_scanned();
                self.file_sel = self.file_sel.min(self.selectable_count().saturating_sub(1));
                self.notice = Some(format!("added {pattern} to {file}"));
                self.error = None;
            }
            Event::Changed => {
                // A write can add or empty the default changelist and can
                // rewrite a description, so reload the lists too, not just the
                // files.
                self.files_for = None;
                self.diffs_for = None;
                // Our own write moved the workspace; re-baseline silently
                // rather than announcing it back to the person who did it.
                self.external = None;
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

        if let Some(panel) = self.filtering {
            self.filter_key(panel, key.code);
            return;
        }

        let key = Key::from_event(key);

        if self.picker.is_some() {
            self.pick_key(key);
            return;
        }

        if self.modal != Modal::None {
            self.modal_key(key);
            return;
        }

        // Ctrl-C is the terminal's own way out, not something to rebind.
        if key.ctrl && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        // Each panel's number is drawn in its title, so these stay put too.
        if let KeyCode::Char(c @ '0'..='9') = key.code {
            if let Some(panel) = Panel::from_number(c as u8 - b'0') {
                self.focus = panel;
                return;
            }
        }

        // A key can carry more than one action; the first whose panel matches
        // is the one meant.
        let action = self
            .config
            .keys
            .actions(key)
            .iter()
            .copied()
            .find(|a| a.applies_to(self.focus));
        if let Some(action) = action {
            self.run(action);
        }
    }

    fn run(&mut self, action: Action) {
        match action {
            Action::Quit => self.quit = true,
            Action::Help => self.modal = Modal::Help,
            // The log is a debugging aid, so it stays one step in, reachable
            // from the help sheet rather than from a key of its own.
            Action::Log => {}
            Action::Refresh => {
                self.busy = true;
                self.error = None;
                self.files_for = None;
                self.diffs_for = None;
                self.worker.send(Request::Refresh);
            }
            Action::Describe => self.edit_description(),
            Action::NewChange => self.new_changelist(),
            Action::DeleteChange => self.delete_changelist(),
            Action::RevertFiles => self.revert_selected(),
            Action::Submit => self.submit_changelist(),
            Action::History => self.show_history(),
            Action::Blame => self.show_blame(),
            Action::Ignore => self.ignore_selected(),
            Action::Undo => self.undo_change(),
            Action::ShelveFiles => self.shelve_selected_files(),
            Action::ShelveChange => self.shelve_changelist(),
            Action::Unshelve => self.unshelve_changelist(),
            Action::DeleteShelf => self.delete_shelf(),
            Action::Filter => self.start_filter(),
            Action::Resolve => self.show_unresolved(),
            Action::SelectRange => self.toggle_range(),
            Action::Streams => self.show_streams(),
            Action::Sync => {
                self.busy = true;
                self.error = None;
                self.notice = None;
                self.worker.send(Request::Sync);
            }
            Action::Scan => {
                if !self.scanning {
                    self.scanning = true;
                    self.worker.send(Request::ScanWorkspace);
                }
            }
            // The left column stacks four panels, so a long list is cramped.
            Action::ZoomIn => self.zoom = true,
            Action::ZoomOut => self.zoom = false,
            Action::Move => self.move_selected_file(),
            Action::Fullscreen => self.toggle_fullscreen(),
            Action::Cancel => self.cancel(),
            Action::NextPanel => self.focus = self.focus.step(1),
            Action::PrevPanel => self.focus = self.focus.step(-1),
            // Within a panel rather than between panels: only Changelists has
            // tabs today, so elsewhere these do nothing.
            Action::NextTab => self.switch_tab(1),
            Action::PrevTab => self.switch_tab(-1),
            Action::Down => self.move_by(1),
            Action::Up => self.move_by(-1),
            Action::PageDown => self.move_by(10),
            Action::PageUp => self.move_by(-10),
            Action::First => self.move_to(0),
            Action::Last => self.move_to(usize::MAX),
            Action::Left => self.sideways(-1),
            Action::Right => self.sideways(1),
        }
    }

    /// On a directory this folds the tree; everywhere else it is the fullscreen
    /// diff, as lazygit does with Enter on a folder.
    fn toggle_fullscreen(&mut self) {
        if self.focus == Panel::Files && matches!(self.selected_row(), Some(FileRow::Dir { .. })) {
            self.toggle_collapse(None);
            return;
        }
        self.diff_fullscreen = !self.diff_fullscreen;
        if self.diff_fullscreen {
            self.focus = Panel::Diff;
        }
    }

    /// Back out of whatever is open, innermost first.
    fn cancel(&mut self) {
        if self.select_anchor.is_some() {
            self.select_anchor = None;
        } else if self.diff_fullscreen {
            self.diff_fullscreen = false;
        }
    }

    /// What moving left or right means, which is not the same in every panel.
    fn sideways(&mut self, by: isize) {
        match self.focus {
            // Long lines are truncated rather than wrapped, so the diff scrolls
            // sideways.
            Panel::Diff if by < 0 => self.diff_hscroll = self.diff_hscroll.saturating_sub(8),
            Panel::Diff => self.diff_hscroll += 8,
            // Tree navigation, which only the Files panel has.
            Panel::Files => self.toggle_collapse(Some(by < 0)),
            _ => {}
        }
    }

    /// Keys while an overlay is up. Each one closes on the key that opened it,
    /// as well as on Esc and the quit key.
    fn modal_key(&mut self, key: Key) {
        let closes = self
            .modal_toggle()
            .is_some_and(|action| self.config.keys.is(key, action));
        if closes || key.code == KeyCode::Esc || self.config.keys.is(key, Action::Quit) {
            self.modal = Modal::None;
            return;
        }

        match self.modal {
            // The log is a debugging aid, so it lives one step in, behind the
            // help sheet rather than on a key of its own.
            Modal::Help if self.config.keys.is(key, Action::Log) => self.modal = Modal::Log,
            Modal::Log if self.config.keys.is(key, Action::Log) => self.modal = Modal::Help,
            Modal::History => self.history_key(key),
            Modal::Blame => self.blame_key(key),
            Modal::Resolve => self.resolve_key(key),
            Modal::Streams => self.streams_key(key),
            _ => {}
        }
    }

    /// The action whose key opened the overlay, and so also closes it.
    fn modal_toggle(&self) -> Option<Action> {
        match self.modal {
            Modal::Help => Some(Action::Help),
            Modal::History => Some(Action::History),
            Modal::Blame => Some(Action::Blame),
            Modal::Resolve => Some(Action::Resolve),
            Modal::Streams => Some(Action::Streams),
            // The log closes on Esc, and steps back to the help sheet on `x`.
            Modal::Log | Modal::None => None,
        }
    }

    /// Which way a key moves a cursor, if it moves one at all.
    fn nav(&self, key: Key) -> Option<Action> {
        [
            Action::Down,
            Action::Up,
            Action::First,
            Action::Last,
            Action::PageDown,
            Action::PageUp,
        ]
        .into_iter()
        .find(|action| self.config.keys.is(key, *action))
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
        self.editing = Some(Editing::NewChange(PostCreate::Nothing));
    }

    /// The stream this workspace is on, if it is a stream client.
    pub fn current_stream(&self) -> &str {
        self.info
            .as_ref()
            .and_then(|i| i.stream.as_deref())
            .unwrap_or_default()
    }

    fn show_streams(&mut self) {
        self.modal = Modal::Streams;
        self.error = None;
        self.busy = true;
        // A switch stays inside one depot, so the rest of the server is noise.
        self.worker.send(Request::LoadStreams {
            depot: depot_of(self.current_stream()),
        });
    }

    fn streams_key(&mut self, key: Key) {
        if let Some(nav) = self.nav(key) {
            self.streams_sel = step(self.streams_sel, self.streams.len(), nav);
        } else if key.code == KeyCode::Enter {
            self.switch_stream();
        }
    }

    fn switch_stream(&mut self) {
        let Some(stream) = self.streams.get(self.streams_sel).cloned() else {
            return;
        };
        if stream.path == self.current_stream() {
            self.error = Some("already on that stream".into());
            return;
        }

        self.ask(
            format!("Switch to {}?", stream.path),
            vec![
                format!("{} ({})", stream.name, stream.kind),
                String::new(),
                "The workspace is resynced to match, which can move a lot of"
                    .to_owned(),
                "data. Perforce refuses while any file is open.".to_owned(),
            ],
            Request::SwitchStream { stream: stream.path },
        );
    }

    /// Show what is waiting to be resolved.
    fn show_unresolved(&mut self) {
        self.unresolved_sel = 0;
        self.modal = Modal::Resolve;
        self.error = None;
        self.busy = true;
        self.worker.send(Request::LoadUnresolved);
    }

    /// Keys inside the resolve view.
    fn resolve_key(&mut self, key: Key) {
        if let Some(nav) = self.nav(key) {
            self.unresolved_sel = step(self.unresolved_sel, self.unresolved.len(), nav);
            return;
        }
        // These answer the question the view asks rather than naming a command,
        // so they are fixed, like the `y` of a confirmation.
        match key.code {
            KeyCode::Char('y') => self.settle(Resolution::Yours),
            KeyCode::Char('t') => self.settle(Resolution::Theirs),
            KeyCode::Char('m') => self.settle(Resolution::Merge),
            KeyCode::Char('a') => self.settle(Resolution::Safe),
            _ => {}
        }
    }

    /// Resolve the file under the cursor.
    fn settle(&mut self, how: Resolution) {
        let Some(file) = self.unresolved.get(self.unresolved_sel) else {
            return;
        };
        let paths = vec![file.local_path.clone()];
        let name = tree::relative(&file.from_path, &self.depot_root()).to_owned();

        // Merging only succeeds where there is nothing to argue about, so it
        // needs no warning. Taking one side outright does.
        if !how.discards() {
            self.busy = true;
            self.error = None;
            self.worker.send(Request::Resolve { how, paths });
            return;
        }

        let (title, lost) = match how {
            Resolution::Yours => ("Keep your copy", "What arrived from the depot is discarded."),
            _ => ("Take the depot copy", "Your local changes are discarded."),
        };
        self.ask(
            format!("{title} of {name}?"),
            vec![lost.to_owned()],
            Request::Resolve { how, paths },
        );
    }

    /// Start narrowing the focused list.
    fn start_filter(&mut self) {
        if !matches!(
            self.focus,
            Panel::Files | Panel::Changelists | Panel::History
        ) {
            self.error = Some("that panel is not a list".into());
            return;
        }
        self.error = None;
        self.filtering = Some(self.focus);
    }

    /// Keys while a filter is being typed. The list narrows as you go, so
    /// there is nothing to submit — Enter just stops typing.
    fn filter_key(&mut self, panel: Panel, code: KeyCode) {
        match code {
            KeyCode::Esc => {
                // Esc undoes the narrowing rather than keeping it, so a filter
                // is never left on a panel you have stopped looking at.
                self.filters.remove(&panel);
                self.filtering = None;
            }
            KeyCode::Enter => self.filtering = None,
            KeyCode::Backspace => {
                let text = self.filters.entry(panel).or_default();
                text.pop();
            }
            KeyCode::Char(c) => {
                self.filters.entry(panel).or_default().push(c);
            }
            _ => return,
        }
        // Whatever was selected may no longer be in the list.
        self.clamp_selections();
        self.file_sel = self
            .file_sel
            .min(self.selectable_count().saturating_sub(1));
        self.follow_selection();
    }

    /// Copy the selected changelist's open files to the server as a shelf.
    fn shelve_changelist(&mut self) {
        let Some(cl) = self.selected_change() else {
            return;
        };
        if cl.status == ChangeStatus::Submitted || cl.id == ChangeId::Default {
            self.error = Some("only a numbered pending changelist can be shelved".into());
            return;
        }
        if !self.is_here(&cl) {
            self.error = Some(self.not_here(&cl).into());
            return;
        }

        // Replacing drops files that were shelved but are no longer open, so
        // it needs saying before it happens.
        if cl.shelved {
            self.ask(
                format!("Replace the shelf on {}?", cl.id),
                vec![
                    cl.summary().to_owned(),
                    String::new(),
                    "Anything shelved but no longer open will be dropped.".to_owned(),
                ],
                Request::ReplaceShelf { change: cl.id },
            );
        } else {
            self.busy = true;
            self.error = None;
            self.worker.send(Request::Shelve {
                change: cl.id,
                files: Vec::new(),
            });
        }
    }

    /// Shelve only the file or directory under the cursor.
    fn shelve_selected_files(&mut self) {
        let Some(cl) = self.selected_change() else {
            return;
        };
        if cl.status == ChangeStatus::Submitted || cl.id == ChangeId::Default {
            self.error = Some("only a numbered pending changelist can be shelved".into());
            return;
        }
        let files = self.selected_files();
        if files.is_empty() {
            return;
        }
        self.busy = true;
        self.error = None;
        self.select_anchor = None;
        self.worker.send(Request::Shelve {
            change: cl.id,
            files,
        });
    }

    /// Open a shelf's files in another changelist, leaving the shelf alone.
    fn unshelve_changelist(&mut self) {
        let Some(cl) = self.selected_change() else {
            return;
        };
        if !cl.shelved {
            self.error = Some("that changelist has nothing shelved".into());
            return;
        }
        // Unshelving into its own changelist would be a no-op at best.
        self.open_picker(
            format!("Unshelve {} into", cl.id),
            PostCreate::Unshelve(cl.id),
            Some(cl.id),
        );
    }

    /// Throw away a shelf.
    fn delete_shelf(&mut self) {
        let Some(cl) = self.selected_change() else {
            return;
        };
        if !cl.shelved {
            self.error = Some("that changelist has nothing shelved".into());
            return;
        }
        self.ask(
            format!("Delete the shelf on {}?", cl.id),
            vec![
                cl.summary().to_owned(),
                String::new(),
                "The shelved copy on the server is lost. Open files stay.".to_owned(),
            ],
            Request::DeleteShelf { change: cl.id },
        );
    }

    /// Open a reversal of the selected submitted change.
    ///
    /// Nothing reaches the depot: the reversal lands in a pending changelist
    /// to be reviewed and submitted like any other work.
    fn undo_change(&mut self) {
        let Some(cl) = self.selected_change() else {
            return;
        };
        if cl.status != ChangeStatus::Submitted {
            self.error = Some("only a submitted change can be undone".into());
            return;
        }
        let root = self.depot_root();
        if root.is_empty() {
            self.error = Some("no depot root to undo within".into());
            return;
        }

        self.ask(
            format!("Undo change {}?", cl.id),
            vec![
                cl.summary().to_owned(),
                String::new(),
                format!("Opens the reversal of {root}/... in a new changelist."),
                "Nothing is submitted until you submit it.".to_owned(),
            ],
            Request::Undo {
                spec: format!("{root}/...@={}", cl.id),
                description: format!("Undo of change {}", cl.id),
            },
        );
    }

    /// Show every revision of the file under the cursor.
    fn show_history(&mut self) {
        let Some(file) = self.selected_file() else {
            self.error = Some("select a file to see its history".into());
            return;
        };
        let depot_path = file.depot_path.clone();

        // Keep whatever is already loaded for this file, so reopening the
        // panel does not blank it while the server answers.
        if self.history_path != depot_path {
            self.history.clear();
            self.history_path = depot_path.clone();
        }
        self.history_rev_sel = 0;
        self.modal = Modal::History;
        self.error = None;
        self.busy = true;
        self.worker.send(Request::LoadHistory { depot_path });
    }

    /// Add the file under the cursor to the workspace ignore file.
    ///
    /// Only a file Perforce has never seen: an ignore rule does nothing about
    /// one that is already tracked.
    fn ignore_selected(&mut self) {
        if self.focus != Panel::Files {
            return;
        }
        let Some(file) = self.selected_file() else {
            self.error = Some("select a file to ignore".into());
            return;
        };
        if !file.untracked() {
            self.error = Some("that file is already under Perforce control".into());
            return;
        }
        let (Some(local), Some(root)) = (
            file.local_path.clone(),
            self.info.as_ref().and_then(|i| i.client_root.clone()),
        ) else {
            self.error = Some("no workspace root to write an ignore file in".into());
            return;
        };
        let depot_path = file.depot_path.clone();

        // P4IGNORE can also be set in a P4CONFIG file, which lazyp4 cannot
        // read; the default is what Perforce itself falls back to.
        let name = std::env::var("P4IGNORE").unwrap_or_else(|_| ".p4ignore".to_owned());
        let Some((file, pattern)) = ignore_entry(&root, &name, &local) else {
            self.error = Some("that file is outside the workspace".into());
            return;
        };

        self.error = None;
        self.busy = true;
        self.worker.send(Request::Ignore {
            file,
            pattern,
            depot_path,
        });
    }

    /// Show who last wrote each line of the file under the cursor.
    fn show_blame(&mut self) {
        let Some(file) = self.selected_file() else {
            self.error = Some("select a file to blame it".into());
            return;
        };
        if file.untracked() {
            // Nothing is on the server to annotate.
            self.error = Some("that file is not in the depot yet".into());
            return;
        }
        let depot_path = file.depot_path.clone();

        // Keep what is already loaded for this file, so reopening does not
        // blank the view while the server answers.
        if self.blame_path != depot_path {
            self.blame.clear();
            self.blame_path = depot_path.clone();
        }
        self.blame_sel = 0;
        self.modal = Modal::Blame;
        self.error = None;
        self.busy = true;
        self.worker.send(Request::LoadBlame { depot_path });
    }

    /// Keys inside the blame view.
    fn blame_key(&mut self, key: Key) {
        if let Some(nav) = self.nav(key) {
            self.blame_sel = step(self.blame_sel, self.blame.len(), nav);
        }
    }

    /// Keys inside the revision-history view.
    fn history_key(&mut self, key: Key) {
        if let Some(nav) = self.nav(key) {
            self.history_rev_sel = step(self.history_rev_sel, self.history.len(), nav);
        } else if self.config.keys.is(key, Action::Undo) {
            self.undo_revision();
        }
    }

    /// Open a reversal of the one revision under the cursor.
    ///
    /// The finer-grained sibling of `U` on a submitted changelist: one file at
    /// one revision, rather than everything that changelist touched.
    fn undo_revision(&mut self) {
        let Some(rev) = self.history.get(self.history_rev_sel) else {
            return;
        };
        if rev.rev <= 1 {
            // There is no earlier revision to put back.
            self.error = Some("the first revision cannot be undone".into());
            return;
        }
        let spec = format!("{}#{}", self.history_path, rev.rev);
        let name = tree::relative(&self.history_path, &self.depot_root()).to_owned();

        self.ask(
            format!("Undo revision #{} of {name}?", rev.rev),
            vec![
                format!("change {}: {}", rev.change, first_line(&rev.description)),
                String::new(),
                format!("Opens the reversal of {spec} in a new changelist."),
                "Nothing is submitted until you submit it.".to_owned(),
            ],
            Request::Undo {
                description: format!("Undo of {spec}"),
                spec,
            },
        );
    }

    /// Submit the selected changelist.
    fn submit_changelist(&mut self) {
        let Some(cl) = self.selected_change() else {
            return;
        };
        if cl.status == ChangeStatus::Submitted {
            self.error = Some("that changelist is already submitted".into());
            return;
        }
        if cl.id == ChangeId::Default {
            // The default changelist is not a spec and carries no description,
            // so ask for one. Everything open in it goes, which is what the
            // confirmation then has to show.
            if self.files.is_empty() {
                self.error = Some("the default changelist has no files".into());
                return;
            }
            self.error = None;
            self.editor = Some(Editor::new(
                "Description to submit the default changelist",
                "",
            ));
            self.editing = Some(Editing::SubmitDefault);
            return;
        }
        if !self.is_here(&cl) {
            self.error = Some(self.not_here(&cl).into());
            return;
        }
        if !has_description(&cl.description) {
            self.error = Some("give the changelist a description first (e)".into());
            return;
        }
        if self.files_for == Some(cl.id) && self.files.is_empty() {
            self.error = Some("that changelist has no files".into());
            return;
        }

        let mut lines = self.file_lines(&self.files);
        lines.push(String::new());
        lines.push(cl.summary().to_owned());

        self.ask(
            format!("Submit changelist {} to the depot?", cl.id),
            lines,
            Request::Submit { change: cl.id },
        );
    }

    /// Confirm submitting everything open in the default changelist.
    fn confirm_submit_default(&mut self, description: String) {
        let mut lines = self.file_lines(&self.files);
        lines.push(String::new());
        lines.push(description.lines().next().unwrap_or_default().to_owned());
        lines.push(String::new());
        // No `-c` means the server takes the whole default changelist, not a
        // selection, so say so before it happens.
        lines.push("Everything open in the default changelist goes.".to_owned());

        self.ask(
            "Submit the default changelist to the depot?".to_owned(),
            lines,
            Request::SubmitDefault { description },
        );
    }

    /// One line per file: its action mark and its path below the tree root.
    fn file_lines<'a>(&self, files: impl IntoIterator<Item = &'a FileEntry>) -> Vec<String> {
        let root = self.depot_root();
        files
            .into_iter()
            .map(|f| format!("{} {}", f.action.code(), tree::relative(&f.depot_path, &root)))
            .collect()
    }

    /// Throw away the local changes to the selected file, or to everything
    /// under the selected directory.
    fn revert_selected(&mut self) {
        let files = self.selected_files();
        if files.is_empty() {
            return;
        }
        if self.selected_change().is_some_and(|cl| cl.status == ChangeStatus::Submitted) {
            self.error = Some("a submitted changelist cannot be reverted".into());
            return;
        }

        // Reverting means "close the open file"; a file the scan found is not
        // open, so there is nothing for Perforce to close.
        let (open, unopened): (Vec<FileEntry>, Vec<FileEntry>) =
            files.into_iter().partition(|f| f.opened);
        if open.is_empty() {
            self.error = Some(format!(
                "{} file(s) are not open — nothing to revert",
                unopened.len()
            ));
            return;
        }

        // Ask the server what it would actually do before showing a question
        // about it: our own list can be stale, and it cannot see a file that is
        // open with nothing to throw away.
        self.error = None;
        self.busy = true;
        // The range has been read into the request; leaving it highlighted
        // would suggest it is still pending.
        self.select_anchor = None;
        self.pending_revert = Some(unopened.len());
        self.worker.send(Request::PreviewRevert { files: open });
    }

    /// Put the revert question up once the server has said what it would do.
    fn confirm_revert(&mut self, files: Vec<FileEntry>, preview: Vec<RevertPreview>) {
        let Some(skipped) = self.pending_revert.take() else {
            return;
        };
        if preview.is_empty() {
            self.error = Some("the server would revert nothing".into());
            return;
        }

        let root = self.depot_root();
        let mut lines: Vec<String> = preview
            .iter()
            .map(|p| format!("{} {}", p.action.code(), tree::relative(&p.depot_path, &root)))
            .collect();
        if skipped > 0 {
            lines.push(format!("({skipped} not open, left alone)"));
        }
        lines.push(String::new());
        lines.push("Local changes to these files will be lost.".to_owned());

        self.ask(
            format!("Revert {} file(s)?", preview.len()),
            lines,
            Request::RevertFiles { files },
        );
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
        // The range has been read into the request; leaving it highlighted
        // would suggest it is still pending.
        self.select_anchor = None;
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
            Some(Editing::NewChange(then)) => Request::CreateChange { description, then },
            // Submitting is irreversible, so the description is only the first
            // half: the confirmation still has to be answered.
            Some(Editing::SubmitDefault) => {
                self.editor = None;
                self.confirm_submit_default(description);
                return;
            }
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
        let (first, last) = self.selection_range();
        let all = self.all_files();
        let root = self.depot_root();

        // A range can hold a directory and its own contents at once, so
        // gather indexes and let the order of `all_files` settle duplicates.
        let mut wanted: Vec<usize> = Vec::new();
        for (i, row) in self
            .file_rows()
            .into_iter()
            .filter(FileRow::selectable)
            .enumerate()
        {
            if i < first || i > last {
                continue;
            }
            match row {
                FileRow::File { index, .. } => wanted.push(index),
                FileRow::Dir { group, path, .. } => {
                    let prefix = format!("{path}/");
                    let offset = match group {
                        Group::InChange => 0,
                        Group::Loose => self.files.len(),
                    };
                    let source = match group {
                        Group::InChange => &self.files,
                        Group::Loose => &self.loose_files,
                    };
                    wanted.extend(
                        source
                            .iter()
                            .enumerate()
                            .filter(|(_, f)| {
                                tree::relative(&f.depot_path, &root).starts_with(&prefix)
                            })
                            .map(|(i, _)| offset + i),
                    );
                }
                FileRow::Header(_) => {}
            }
        }

        wanted.sort_unstable();
        wanted.dedup();
        wanted
            .into_iter()
            .filter_map(|i| all.get(i).map(|f| (*f).clone()))
            .collect()
    }

    /// The rows the next action will act on, as inclusive selectable indexes.
    ///
    /// Without an anchor that is just the cursor.
    pub fn selection_range(&self) -> (usize, usize) {
        match self.select_anchor {
            Some(anchor) => (anchor.min(self.file_sel), anchor.max(self.file_sel)),
            None => (self.file_sel, self.file_sel),
        }
    }

    /// Whether a selectable row is inside the range.
    pub fn row_selected(&self, index: usize) -> bool {
        let (first, last) = self.selection_range();
        self.select_anchor.is_some() && index >= first && index <= last
    }

    /// Start or abandon a range selection.
    fn toggle_range(&mut self) {
        if self.focus != Panel::Files {
            self.error = Some("a range can only be selected in Files".into());
            return;
        }
        self.error = None;
        self.select_anchor = match self.select_anchor {
            Some(_) => None,
            None => Some(self.file_sel),
        };
    }

    /// Expand or collapse the selected directory. Does nothing on a file.
    fn toggle_collapse(&mut self, want_collapsed: Option<bool>) {
        let Some(FileRow::Dir {
            group,
            path,
            collapsed,
            ..
        }) = self.selected_row()
        else {
            return;
        };
        let collapse = want_collapsed.unwrap_or(!collapsed);
        let key = (group, path);
        if collapse {
            self.collapsed.insert(key);
        } else {
            self.collapsed.remove(&key);
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
            let title = format!(
                "Move {} file{} to",
                files.len(),
                if files.len() == 1 { "" } else { "s" }
            );
            self.open_picker(title, PostCreate::Move(files), None);
            return;
        }

        // With a range spanning both groups the direction follows the cursor,
        // which is where the eye is. Moving a file to where it already sits is
        // harmless.
        let target = if loose { cl.id } else { ChangeId::Default };
        self.busy = true;
        self.error = None;
        self.select_anchor = None;
        self.worker.send(Request::MoveFiles {
            change: target,
            files,
        });
    }

    /// Offer the numbered changelists this could go into, plus a new one.
    fn open_picker(&mut self, title: String, what: PostCreate, exclude: Option<ChangeId>) {
        let mut options: Vec<Destination> = self
            .pending
            .iter()
            .filter(|cl| {
                self.is_here(cl) && cl.id != ChangeId::Default && Some(cl.id) != exclude
            })
            .map(|cl| Destination::Existing(cl.id, cl.summary().to_owned()))
            .collect();
        options.push(Destination::New);

        self.error = None;
        self.picker = Some(Picker {
            what,
            title,
            options,
            sel: 0,
        });
    }

    fn pick_key(&mut self, key: Key) {
        if key.code == KeyCode::Esc || self.config.keys.is(key, Action::Quit) {
            self.picker = None;
            return;
        }
        if let (Some(nav), Some(picker)) = (self.nav(key), self.picker.as_mut()) {
            picker.sel = step(picker.sel, picker.options.len(), nav);
            return;
        }
        if matches!(key.code, KeyCode::Enter | KeyCode::Char(' ')) {
            self.confirm_pick();
        }
    }

    fn confirm_pick(&mut self) {
        let Some(picker) = self.picker.take() else {
            return;
        };
        match picker.options.get(picker.sel).cloned() {
            Some(Destination::Existing(change, _)) => {
                self.busy = true;
                let request = match picker.what {
                    PostCreate::Move(files) => Request::MoveFiles { change, files },
                    PostCreate::Unshelve(from) => Request::Unshelve { from, into: change },
                    PostCreate::Nothing => return,
                };
                self.worker.send(request);
            }
            Some(Destination::New) => {
                // A changelist cannot exist without a description, so ask for
                // one before creating anything.
                self.editor = Some(Editor::new("Description of the new changelist", ""));
                self.editing = Some(Editing::NewChange(picker.what));
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
            Panel::History => (self.history_sel, self.visible_submitted().len()),
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
            Panel::History => self.visible_submitted().len(),
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
        self.history_sel = self
            .history_sel
            .min(self.visible_submitted().len().saturating_sub(1));
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

/// Whether a description says anything.
///
/// Perforce writes `<saved by Perforce>` itself when it shelves work into a
/// changelist you never described, so that placeholder counts as empty.
/// The depot a stream lives in, as a filespec: `//darksim/main` gives
/// `//darksim/...`. Nothing for a client with no stream.
fn depot_of(stream: &str) -> Option<String> {
    let depot = stream.strip_prefix("//")?.split('/').next()?;
    (!depot.is_empty()).then(|| format!("//{depot}/..."))
}

pub fn has_description(description: &str) -> bool {
    let text = description.trim();
    !text.is_empty() && text != "<saved by Perforce>"
}

/// Where the ignore file lives, and the pattern that names `local` inside it.
///
/// `P4IGNORE` may be a bare file name, which Perforce looks for from each
/// file's directory upwards, or a path. lazyp4 writes to the one in the
/// workspace root, which is where a shared ignore file belongs. `None` when the
/// file is not inside the workspace at all.
pub fn ignore_entry(root: &str, name: &str, local: &str) -> Option<(String, String)> {
    let slashes = |s: &str| s.replace('\\', "/");
    let file = if std::path::Path::new(name).is_absolute() {
        slashes(name)
    } else {
        format!("{}/{name}", slashes(root).trim_end_matches('/'))
    };

    let root = slashes(root);
    let root = root.trim_end_matches('/');
    let local = slashes(local);
    // Windows spells the same path in several cases, so compare loosely.
    let head = local.get(..root.len())?;
    if !head.eq_ignore_ascii_case(root) || local.as_bytes().get(root.len()) != Some(&b'/') {
        return None;
    }
    Some((file, local[root.len() + 1..].to_owned()))
}

/// Move a cursor within `count` rows the way a navigation action says.
fn step(sel: usize, count: usize, action: Action) -> usize {
    let last = count.saturating_sub(1) as isize;
    let by = |d: isize| (sel as isize + d).clamp(0, last) as usize;
    match action {
        Action::Down => by(1),
        Action::Up => by(-1),
        Action::PageDown => by(20),
        Action::PageUp => by(-20),
        Action::First => 0,
        Action::Last => last.max(0) as usize,
        _ => sel,
    }
}

/// First line of a description, for a one-line entry.
fn first_line(description: &str) -> &str {
    description.lines().next().unwrap_or_default().trim_end()
}

/// Marker shown beside a changelist in a list.
pub fn change_marker(cl: &Changelist) -> char {
    match cl.status {
        ChangeStatus::Submitted => '✓',
        _ if cl.shelved => '⌸',
        _ => '▸',
    }
}
