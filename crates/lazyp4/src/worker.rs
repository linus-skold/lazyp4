//! The Perforce worker thread.
//!
//! `p4::Client` wraps a C++ `ClientApi`, which is not `Send` and whose `Run`
//! blocks until the server answers. So the client is built on this thread and
//! never leaves it; the UI talks to it over channels and stays responsive while
//! a command is in flight.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use p4::{
    diff, AnnotatedLine, ChangeFilter, ChangeId, ChangeStatus, Changelist, Client, Connection,
    FileAction,
    FileDiff, Resolution, RevertPreview, Revision, ServerInfo, Stream, Unresolved,
};

/// A file in a changelist, from whichever command could see it.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub depot_path: String,
    /// Where the file sits on disk. Only known for files found by scanning,
    /// which is also the only case that needs it.
    pub local_path: Option<String>,
    pub rev: Option<u32>,
    pub action: FileAction,
    pub unresolved: bool,
    /// False for a file the workspace scan found but Perforce has not opened.
    /// Those need `add`/`edit`/`delete` rather than `reopen`.
    pub opened: bool,
}

impl FileEntry {
    fn opened(f: p4::OpenedFile) -> Self {
        FileEntry {
            depot_path: f.depot_path,
            local_path: None,
            rev: f.rev,
            action: f.action,
            unresolved: f.unresolved,
            opened: true,
        }
    }

    /// Perforce does not track this file at all, so it shows as `??`.
    pub fn untracked(&self) -> bool {
        !self.opened && self.action == FileAction::Add
    }

    /// The path to name on a command line. A file Perforce has never seen has
    /// no usable depot path.
    pub fn command_path(&self) -> &str {
        match (&self.local_path, self.opened) {
            (Some(local), false) => local,
            _ => &self.depot_path,
        }
    }
}

/// What a freshly created changelist is for.
#[derive(Debug, Clone)]
pub enum PostCreate {
    /// Nothing; an empty changelist.
    Nothing,
    Move(Vec<FileEntry>),
    Unshelve(ChangeId),
}

pub enum Request {
    /// Reconnect and reload everything.
    Refresh,
    LoadFiles {
        change: ChangeId,
        status: ChangeStatus,
        shelved: bool,
    },
    /// Walk the workspace for files that differ but are not open. Slow, so it
    /// only runs when the user asks.
    ScanWorkspace,
    /// Cheap check for `p4` having been used outside lazyp4.
    CheckExternal,
    /// Append `pattern` to the workspace ignore file at `file`.
    Ignore {
        file: String,
        pattern: String,
        /// The file the rule is about, so the UI can drop its row.
        depot_path: String,
    },
    SetDescription {
        change: ChangeId,
        description: String,
    },
    /// Create a changelist and fill it, in one step so the UI never has to
    /// hold a half-made changelist.
    CreateChange {
        description: String,
        then: PostCreate,
    },
    Shelve {
        change: ChangeId,
        /// Empty shelves the whole changelist.
        files: Vec<FileEntry>,
    },
    ReplaceShelf {
        change: ChangeId,
    },
    DeleteShelf {
        change: ChangeId,
    },
    Unshelve {
        from: ChangeId,
        into: ChangeId,
    },
    DeleteChange {
        change: ChangeId,
    },
    /// Ask the server what a revert would do, without doing it.
    PreviewRevert {
        files: Vec<FileEntry>,
    },
    RevertFiles {
        files: Vec<FileEntry>,
    },
    Submit {
        change: ChangeId,
    },
    /// Submit everything open in the default changelist, under `description`.
    SubmitDefault {
        description: String,
    },
    LoadHistory {
        depot_path: String,
    },
    /// Who last wrote each line of a file.
    LoadBlame {
        depot_path: String,
    },
    Sync,
    LoadStreams,
    SwitchStream {
        stream: String,
    },
    LoadUnresolved,
    Resolve {
        how: Resolution,
        paths: Vec<String>,
    },
    /// Open a reversal of submitted work in a changelist of its own.
    ///
    /// `spec` is a path with a revision range: `//depot/main/...@=412` for a
    /// whole changelist, `//depot/main/f.cpp#3` for one revision of one file.
    Undo {
        spec: String,
        description: String,
    },
    /// Move files into `change`, opening them first if Perforce has not seen
    /// them. `ChangeId::Default` moves them out of a numbered changelist.
    MoveFiles {
        change: ChangeId,
        files: Vec<FileEntry>,
    },
    /// Diff a whole changelist at once. One command covers every file, and the
    /// pane picks out the one under the cursor.
    LoadDiff {
        change: ChangeId,
        status: ChangeStatus,
        shelved: bool,
        files: Vec<FileEntry>,
    },
    Shutdown,
}

/// What the worker sends back. Terminal input arrives on the same channel so
/// the UI has one place to wait.
pub enum Event {
    Input(crossterm::event::Event),
    Info(ServerInfo),
    Changes {
        pending: Vec<Changelist>,
        submitted: Vec<Changelist>,
    },
    Files {
        change: ChangeId,
        /// Files open in `change`.
        files: Vec<FileEntry>,
        /// Files open in the default changelist, shown beneath them so a file
        /// can be moved across. Empty when `change` is itself the default, or
        /// is not a pending changelist of ours.
        default_files: Vec<FileEntry>,
    },
    /// Files the workspace scan turned up.
    Scanned(Vec<FileEntry>),
    /// A fingerprint of what is open in the workspace. Only its changing
    /// matters, so the app keeps the last one and compares.
    External(String),
    /// A pattern reached the ignore file.
    Ignored {
        depot_path: String,
        pattern: String,
        file: String,
    },
    History {
        depot_path: String,
        revisions: Vec<Revision>,
    },
    Blame {
        depot_path: String,
        lines: Vec<AnnotatedLine>,
    },
    Unresolved(Vec<Unresolved>),
    /// What a revert would do, with the files it was asked about carried back
    /// so the answer can be acted on unchanged.
    RevertPreview {
        files: Vec<FileEntry>,
        preview: Vec<RevertPreview>,
    },
    Streams(Vec<Stream>),
    /// Something worth saying that is not an error.
    Notice(String),
    /// A write finished. Descriptions and the default changelist's very
    /// existence both come from the change lists, so everything is now stale.
    Changed,
    Diff {
        change: ChangeId,
        files: Vec<FileDiff>,
    },
    /// A command the worker ran, for the command log.
    Log(String),
    Error(String),
    /// The worker finished a batch of work; used to clear the busy indicator.
    Idle,
}

pub struct Worker {
    requests: Sender<Request>,
    /// Kept by [`Worker::detached`] so tests can see what the app asked for.
    #[cfg(test)]
    outbox: Option<Receiver<Request>>,
}

impl Worker {
    /// Start the worker and return it together with the shared event channel.
    pub fn spawn() -> (Worker, Receiver<Event>) {
        let (req_tx, req_rx) = mpsc::channel::<Request>();
        let (ev_tx, ev_rx) = mpsc::channel::<Event>();

        let events = ev_tx.clone();
        thread::spawn(move || run(req_rx, events));
        spawn_input_reader(ev_tx);

        (
            Worker {
                requests: req_tx,
                #[cfg(test)]
                outbox: None,
            },
            ev_rx,
        )
    }

    /// A worker with nothing behind it. Requests are dropped, so an [`App`]
    /// built on one can be driven with canned data — used by the render tests.
    ///
    /// [`App`]: crate::app::App
    #[cfg(test)]
    pub fn detached() -> Worker {
        let (tx, rx) = mpsc::channel();
        Worker {
            requests: tx,
            outbox: Some(rx),
        }
    }

    /// The most recent request, discarding any before it. `None` when nothing
    /// has been sent since the last call.
    #[cfg(test)]
    pub fn last_request(&self) -> Option<Request> {
        let rx = self.outbox.as_ref()?;
        let mut last = None;
        while let Ok(req) = rx.try_recv() {
            last = Some(req);
        }
        last
    }

    pub fn send(&self, req: Request) {
        // A dead worker means the app is shutting down; nothing to report.
        let _ = self.requests.send(req);
    }
}

/// Blocks on terminal input so the UI thread never has to poll.
fn spawn_input_reader(events: Sender<Event>) {
    thread::spawn(move || loop {
        match crossterm::event::read() {
            Ok(ev) => {
                if events.send(Event::Input(ev)).is_err() {
                    return;
                }
            }
            Err(e) => {
                let _ = events.send(Event::Error(format!("input: {e}")));
                return;
            }
        }
    });
}

/// Two connections to the same server.
///
/// The server strips diff content out of a tagged reply, and tagging is fixed
/// at handshake time, so structured data and diff text cannot share one
/// connection.
struct Clients {
    tagged: Client,
    untagged: Client,
}

fn run(requests: Receiver<Request>, events: Sender<Event>) {
    let mut clients: Option<Clients> = None;

    while let Ok(req) = requests.recv() {
        if matches!(req, Request::Shutdown) {
            return;
        }

        // Connect lazily, and again after the server drops us.
        if clients
            .as_mut()
            .is_none_or(|c| c.tagged.dropped() || c.untagged.dropped())
        {
            let _ = events.send(Event::Log("connect".into()));
            match (
                Client::connect(&Connection::default()),
                Client::connect(&Connection::untagged()),
            ) {
                (Ok(tagged), Ok(untagged)) => clients = Some(Clients { tagged, untagged }),
                (Err(e), _) | (_, Err(e)) => {
                    let _ = events.send(Event::Error(format!("connect: {e}")));
                    let _ = events.send(Event::Idle);
                    continue;
                }
            }
        }
        let both = clients.as_mut().expect("connected above");
        let p4 = &mut both.tagged;

        match req {
            Request::Shutdown => return,
            Request::Refresh => {
                let _ = events.send(Event::Log("info".into()));
                let info = match p4.info() {
                    Ok(info) => {
                        let _ = events.send(Event::Info(info.clone()));
                        Some(info)
                    }
                    Err(e) => {
                        let _ = events.send(Event::Error(format!("info: {e}")));
                        None
                    }
                };

                let _ = events.send(Event::Log("changes -l -s pending".into()));
                let mut pending = match p4.changes(&ChangeFilter::pending()) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = events.send(Event::Error(format!("changes: {e}")));
                        Vec::new()
                    }
                };

                // `p4 changes` never reports the default changelist, so it has
                // to be built from the files opened in it.
                if let Some(info) = &info {
                    let _ = events.send(Event::Log("opened -c default".into()));
                    match p4.opened(Some(ChangeId::Default)) {
                        Ok(files) if !files.is_empty() => pending.insert(0, default_change(info)),
                        Ok(_) => {}
                        Err(e) => {
                            let _ = events.send(Event::Error(format!("opened: {e}")));
                        }
                    }
                }

                let _ = events.send(Event::Log("changes -l -s submitted -m 50".into()));
                let submitted = match p4.changes(&ChangeFilter::submitted(50)) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = events.send(Event::Error(format!("changes: {e}")));
                        Vec::new()
                    }
                };

                let _ = events.send(Event::Changes { pending, submitted });
            }
            Request::LoadFiles {
                change,
                status,
                shelved,
            } => {
                let files = load_files(p4, &events, change, status, shelved);

                // Only a numbered pending changelist of ours has somewhere to
                // move files to and from.
                let default_files = if change == ChangeId::Default
                    || status == ChangeStatus::Submitted
                {
                    Vec::new()
                } else {
                    let _ = events.send(Event::Log("opened -c default".into()));
                    p4.opened(Some(ChangeId::Default))
                        .unwrap_or_default()
                        .into_iter()
                        .map(FileEntry::opened)
                        .collect()
                };

                let _ = events.send(Event::Files {
                    change,
                    files,
                    default_files,
                });
            }
            Request::ScanWorkspace => {
                let _ = events.send(Event::Log("status".into()));
                match p4.status() {
                    Ok(entries) => {
                        let files = entries
                            .into_iter()
                            .map(|e| FileEntry {
                                depot_path: e.depot_path,
                                local_path: Some(e.local_path),
                                rev: None,
                                action: e.action,
                                unresolved: false,
                                opened: false,
                            })
                            .collect();
                        let _ = events.send(Event::Scanned(files));
                    }
                    Err(e) => {
                        let _ = events.send(Event::Error(format!("status: {e}")));
                    }
                }
            }
            Request::CheckExternal => {
                // Deliberately not logged: this runs on a timer, and a poll in
                // the command log every few seconds would bury what the user
                // actually did. `opened` is workspace-scoped and cheap, unlike
                // the `status` walk behind `u`.
                if let Ok(files) = p4.opened(None) {
                    let mut marks: Vec<String> = files
                        .iter()
                        .map(|f| format!("{}:{}:{}", f.change, f.action, f.depot_path))
                        .collect();
                    marks.sort();
                    let _ = events.send(Event::External(marks.join("\n")));
                }
                // No `Idle`: a poll must not clear a spinner it never raised.
                continue;
            }
            Request::Ignore {
                file,
                pattern,
                depot_path,
            } => {
                match append_line(&file, &pattern) {
                    Ok(()) => {
                        let _ = events.send(Event::Ignored {
                            depot_path,
                            pattern,
                            file,
                        });
                    }
                    Err(e) => {
                        // Perforce keeps unopened files read-only, and a shared
                        // ignore file is usually one of them.
                        let _ = events.send(Event::Error(format!(
                            "{file}: {e} — open it for edit first"
                        )));
                    }
                }
            }
            Request::MoveFiles { change, files } => {
                move_files(p4, &events, change, &files);
                let _ = events.send(Event::Changed);
            }
            Request::SetDescription {
                change,
                description,
            } => {
                // The spec form only comes back whole on an untagged
                // connection; a tagged reply is a record, not a form.
                let _ = events.send(Event::Log(format!("change -o {change} | change -i")));
                if let Err(e) = both.untagged.set_description(change, &description) {
                    let _ = events.send(Event::Error(format!("change: {e}")));
                }
                let _ = events.send(Event::Changed);
            }
            Request::Undo { spec, description } => {
                // The reversal needs somewhere to live, and putting it in its
                // own changelist keeps it reviewable before it is submitted.
                let _ = events.send(Event::Log(format!("change -i ({description})")));
                match both.untagged.create_change(&description) {
                    Ok(into) => {
                        let _ = events.send(Event::Log(format!("undo -c {into} {spec}")));
                        if let Err(e) = both.tagged.undo(into, &spec) {
                            let _ = events.send(Event::Error(format!("undo: {e}")));
                        }
                    }
                    Err(e) => {
                        let _ = events.send(Event::Error(format!("change: {e}")));
                    }
                }
                let _ = events.send(Event::Changed);
            }
            Request::Sync => {
                let _ = events.send(Event::Log("sync".into()));
                match p4.sync() {
                    Ok(0) => {
                        let _ = events.send(Event::Notice("already up to date".into()));
                    }
                    Ok(n) => {
                        let _ = events.send(Event::Notice(format!("{n} file(s) updated")));
                    }
                    Err(e) => {
                        let _ = events.send(Event::Error(format!("sync: {e}")));
                    }
                }
                let _ = events.send(Event::Changed);
            }
            Request::LoadStreams => {
                let _ = events.send(Event::Log("streams".into()));
                match p4.streams() {
                    Ok(streams) => {
                        let _ = events.send(Event::Streams(streams));
                    }
                    Err(e) => {
                        let _ = events.send(Event::Error(format!("streams: {e}")));
                    }
                }
            }
            Request::SwitchStream { stream } => {
                let _ = events.send(Event::Log(format!("switch {stream}")));
                match p4.switch_stream(&stream) {
                    Ok(()) => {
                        let _ = events.send(Event::Notice(format!("switched to {stream}")));
                    }
                    Err(e) => {
                        let _ = events.send(Event::Error(format!("switch: {e}")));
                    }
                }
                let _ = events.send(Event::Changed);
            }
            Request::LoadUnresolved => {
                let _ = events.send(Event::Log("resolve -n".into()));
                match p4.unresolved() {
                    Ok(files) => {
                        let _ = events.send(Event::Unresolved(files));
                    }
                    Err(e) => {
                        let _ = events.send(Event::Error(format!("resolve -n: {e}")));
                    }
                }
            }
            Request::Resolve { how, paths } => {
                let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
                let _ = events.send(Event::Log(format!(
                    "resolve {} ({} files)",
                    how.flag(),
                    paths.len()
                )));
                if let Err(e) = p4.resolve(how, &paths) {
                    let _ = events.send(Event::Error(format!("resolve: {e}")));
                }
                // The list of what is left, and the diffs, both move.
                let _ = events.send(Event::Changed);
            }
            Request::LoadHistory { depot_path } => {
                let _ = events.send(Event::Log(format!("filelog -l -m 50 {depot_path}")));
                match p4.filelog(&depot_path, Some(50)) {
                    Ok(revisions) => {
                        let _ = events.send(Event::History {
                            depot_path,
                            revisions,
                        });
                    }
                    Err(e) => {
                        let _ = events.send(Event::Error(format!("filelog: {e}")));
                    }
                }
            }
            Request::LoadBlame { depot_path } => {
                let _ = events.send(Event::Log(format!("annotate -c -u -q {depot_path}")));
                match p4.annotate(&depot_path) {
                    Ok(lines) => {
                        let _ = events.send(Event::Blame { depot_path, lines });
                    }
                    Err(e) => {
                        let _ = events.send(Event::Error(format!("annotate: {e}")));
                    }
                }
            }
            Request::Submit { change } => {
                let _ = events.send(Event::Log(format!("submit -c {change}")));
                match p4.submit(change) {
                    Ok(()) => {
                        let _ = events.send(Event::Log(format!("{change} submitted")));
                    }
                    Err(e) => submit_failed(&events, e),
                }
                let _ = events.send(Event::Changed);
            }
            Request::SubmitDefault { description } => {
                let _ = events.send(Event::Log("submit -d (default)".into()));
                match p4.submit_default(&description) {
                    Ok(()) => {
                        let _ = events
                            .send(Event::Notice("default changelist submitted".into()));
                    }
                    Err(e) => submit_failed(&events, e),
                }
                let _ = events.send(Event::Changed);
            }
            Request::PreviewRevert { files } => {
                let paths: Vec<&str> = files.iter().map(|f| f.command_path()).collect();
                let _ = events.send(Event::Log(format!("revert -n ({} files)", paths.len())));
                match p4.revert_preview(&paths) {
                    Ok(preview) => {
                        let _ = events.send(Event::RevertPreview { files, preview });
                    }
                    Err(e) => {
                        let _ = events.send(Event::Error(format!("revert -n: {e}")));
                    }
                }
            }
            Request::RevertFiles { files } => {
                let paths: Vec<&str> = files.iter().map(|f| f.command_path()).collect();
                let _ = events.send(Event::Log(format!("revert ({} files)", paths.len())));
                if let Err(e) = p4.revert(&paths) {
                    let _ = events.send(Event::Error(format!("revert: {e}")));
                }
                let _ = events.send(Event::Changed);
            }
            Request::DeleteChange { change } => {
                let _ = events.send(Event::Log(format!("change -d {change}")));
                if let Err(e) = p4.delete_change(change) {
                    let _ = events.send(Event::Error(format!("change -d: {e}")));
                }
                let _ = events.send(Event::Changed);
            }
            Request::CreateChange { description, then } => {
                let _ = events.send(Event::Log("change -i (new)".into()));
                match both.untagged.create_change(&description) {
                    Ok(change) => match then {
                        PostCreate::Nothing => {}
                        PostCreate::Move(files) => {
                            move_files(&mut both.tagged, &events, change, &files);
                        }
                        PostCreate::Unshelve(from) => {
                            let _ = events
                                .send(Event::Log(format!("unshelve -s {from} -c {change}")));
                            if let Err(e) = both.tagged.unshelve(from, change) {
                                let _ = events.send(Event::Error(format!("unshelve: {e}")));
                            }
                        }
                    },
                    Err(e) => {
                        let _ = events.send(Event::Error(format!("change: {e}")));
                    }
                }
                let _ = events.send(Event::Changed);
            }
            Request::Shelve { change, files } => {
                let paths: Vec<&str> = files.iter().map(|f| f.command_path()).collect();
                let _ = events.send(Event::Log(format!(
                    "shelve -c {change} -f ({} files)",
                    paths.len()
                )));
                if let Err(e) = p4.shelve(change, &paths) {
                    let _ = events.send(Event::Error(format!("shelve: {e}")));
                }
                let _ = events.send(Event::Changed);
            }
            Request::ReplaceShelf { change } => {
                let _ = events.send(Event::Log(format!("shelve -r -c {change}")));
                if let Err(e) = p4.replace_shelf(change) {
                    let _ = events.send(Event::Error(format!("shelve -r: {e}")));
                }
                let _ = events.send(Event::Changed);
            }
            Request::DeleteShelf { change } => {
                let _ = events.send(Event::Log(format!("shelve -d -c {change}")));
                if let Err(e) = p4.delete_shelf(change) {
                    let _ = events.send(Event::Error(format!("shelve -d: {e}")));
                }
                let _ = events.send(Event::Changed);
            }
            Request::Unshelve { from, into } => {
                let _ = events.send(Event::Log(format!("unshelve -s {from} -c {into}")));
                if let Err(e) = p4.unshelve(from, into) {
                    let _ = events.send(Event::Error(format!("unshelve: {e}")));
                }
                let _ = events.send(Event::Changed);
            }
            Request::LoadDiff {
                change,
                status,
                shelved,
                files,
            } => {
                let diffs = load_diff(both, &events, change, status, shelved, &files);
                let _ = events.send(Event::Diff {
                    change,
                    files: diffs,
                });
            }
        }

        let _ = events.send(Event::Idle);
    }
}

/// Move files into `change`.
///
/// Files Perforce already has open are reopened. Files the scan found are not
/// open at all, so they must be opened for the action that reconciles them —
/// `add` for one Perforce has never seen, `edit` for one changed behind its
/// back, `delete` for one removed from disk. Those are grouped so each action
/// costs a single command.
fn move_files(p4: &mut Client, events: &Sender<Event>, change: ChangeId, files: &[FileEntry]) {
    let (open, unopened): (Vec<&FileEntry>, Vec<&FileEntry>) =
        files.iter().partition(|f| f.opened);

    if !open.is_empty() {
        let paths: Vec<&str> = open.iter().map(|f| f.command_path()).collect();
        let _ = events.send(Event::Log(format!("reopen -c {change} ({} files)", paths.len())));
        if let Err(e) = p4.reopen(change, &paths) {
            let _ = events.send(Event::Error(format!("reopen: {e}")));
        }
    }

    for action in [FileAction::Add, FileAction::Edit, FileAction::Delete] {
        let paths: Vec<&str> = unopened
            .iter()
            .filter(|f| f.action == action)
            .map(|f| f.command_path())
            .collect();
        if paths.is_empty() {
            continue;
        }
        let _ = events.send(Event::Log(format!(
            "{action} -c {change} ({} files)",
            paths.len()
        )));
        if let Err(e) = p4.open_files(&action, change, &paths) {
            let _ = events.send(Event::Error(format!("{action}: {e}")));
        }
    }
}

/// Append one line to a text file, creating it if it is not there.
fn append_line(path: &str, line: &str) -> std::io::Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(path)?;

    // A file that does not end in a newline would otherwise swallow the new
    // pattern onto its last line.
    let len = file.metadata()?.len();
    if len > 0 {
        let mut last = [0u8; 1];
        file.seek(SeekFrom::End(-1))?;
        file.read_exact(&mut last)?;
        if last[0] != b'\n' {
            file.write_all(b"\n")?;
        }
    }
    writeln!(file, "{line}")
}

/// Report a failed submit, pointing at the resolve view when that is what went
/// wrong — much the most common cause.
fn submit_failed(events: &Sender<Event>, e: p4::Error) {
    let message = e.to_string();
    let hint = if message.to_lowercase().contains("resolve") {
        " — press R to resolve"
    } else {
        ""
    };
    let _ = events.send(Event::Error(format!("submit: {message}{hint}")));
}

/// A stand-in for the default changelist, which the server never lists.
fn default_change(info: &ServerInfo) -> Changelist {
    Changelist {
        id: ChangeId::Default,
        status: ChangeStatus::Pending,
        user: info.user.clone(),
        client: info.client.clone(),
        time: None,
        description: "files not in a numbered changelist".into(),
        shelved: false,
    }
}

/// Diff every file in a changelist.
///
/// One command covers the whole changelist. Adds and deletes are then filled in
/// separately: Perforce reports no diff for either, because there is nothing on
/// one of the two sides to compare against.
fn load_diff(
    c: &mut Clients,
    events: &Sender<Event>,
    change: ChangeId,
    status: ChangeStatus,
    shelved: bool,
    entries: &[FileEntry],
) -> Vec<FileDiff> {
    let submitted = status == ChangeStatus::Submitted;

    let raw = if submitted || shelved {
        let _ = events.send(Event::Log(format!(
            "describe -du{} {change}",
            if shelved { " -S" } else { "" }
        )));
        c.untagged.describe_diff_text(change, shelved)
    } else {
        // Pending files live in the workspace, so the diff is against local
        // content. Naming the files keeps other open changelists out of it.
        let paths: Vec<&str> = entries.iter().map(|f| f.depot_path.as_str()).collect();
        let _ = events.send(Event::Log(format!("diff -du ({} files)", paths.len())));
        c.untagged.diff_text(&paths)
    };

    let mut diffs = match raw {
        Ok(text) => diff::normalize(&text),
        Err(e) => {
            let _ = events.send(Event::Error(format!("diff: {e}")));
            Vec::new()
        }
    };

    // Anything the diff did not cover: adds, deletes, and files whose content
    // has to be fetched whole.
    for entry in entries {
        if diffs
            .iter()
            .any(|d| d.depot_path == entry.depot_path && !d.hunks.is_empty())
        {
            continue;
        }
        let added = matches!(
            entry.action,
            FileAction::Add | FileAction::MoveAdd | FileAction::Branch | FileAction::Import
        );
        let deleted = matches!(entry.action, FileAction::Delete | FileAction::MoveDelete);
        if !added && !deleted {
            continue;
        }

        let Some(content) = whole_content(c, events, change, submitted, shelved, entry, added)
        else {
            continue;
        };

        let patch = diff::whole_file(&entry.depot_path, &content, added);
        // whole_file emits a complete patch; re-read it so every entry is
        // shaped the same way.
        if let Some(parsed) = diff::normalize(&patch).into_iter().next() {
            match diffs.iter_mut().find(|d| d.depot_path == entry.depot_path) {
                Some(existing) => existing.hunks = parsed.hunks,
                None => diffs.push(parsed),
            }
        }
    }

    diffs
}

/// Whole content of a file that has no diff, from wherever it can be reached.
fn whole_content(
    c: &mut Clients,
    events: &Sender<Event>,
    change: ChangeId,
    submitted: bool,
    shelved: bool,
    entry: &FileEntry,
    added: bool,
) -> Option<String> {
    let path = &entry.depot_path;

    let spec = if shelved {
        // A shelf is addressed by its changelist, not by a revision.
        Some(format!("{path}@={change}"))
    } else if submitted {
        match (added, entry.rev) {
            (true, Some(rev)) => Some(format!("{path}#{rev}")),
            // The content of a delete is whatever the revision before it held.
            (false, Some(rev)) if rev > 1 => Some(format!("{path}#{}", rev - 1)),
            _ => None,
        }
    } else if !added {
        // A pending delete still has its depot revision on the server.
        entry.rev.map(|rev| format!("{path}#{rev}"))
    } else {
        None
    };

    if let Some(spec) = spec {
        let _ = events.send(Event::Log(format!("print -q {spec}")));
        return c.untagged.print_text(&spec).ok();
    }

    // A pending add exists only in the workspace.
    let _ = events.send(Event::Log(format!("where {path}")));
    let local = c.tagged.local_path(path).ok().flatten()?;
    std::fs::read_to_string(local).ok()
}

/// Picks the command that can actually see this changelist's files.
///
/// `opened` only reports files open in *our* workspace, so a pending
/// changelist on another client comes back empty; `describe` is the fallback
/// and the only way to see a shelf.
fn load_files(
    p4: &mut Client,
    events: &Sender<Event>,
    change: ChangeId,
    status: ChangeStatus,
    shelved: bool,
) -> Vec<FileEntry> {
    let describe = |p4: &mut Client, shelf: bool| {
        let _ = events.send(Event::Log(format!(
            "describe -s{} {change}",
            if shelf { " -S" } else { "" }
        )));
        match p4.describe(change, shelf) {
            Ok(d) => d
                .files
                .into_iter()
                .map(|f| FileEntry {
                    depot_path: f.depot_path,
                    local_path: None,
                    rev: f.rev,
                    action: f.action,
                    unresolved: false,
                    opened: true,
                })
                .collect(),
            Err(e) => {
                let _ = events.send(Event::Error(format!("describe: {e}")));
                Vec::new()
            }
        }
    };

    if status == ChangeStatus::Submitted {
        return describe(p4, false);
    }

    if shelved {
        let files = describe(p4, true);
        if !files.is_empty() {
            return files;
        }
    }

    let _ = events.send(Event::Log(format!("opened -c {change}")));
    let opened = match p4.opened(Some(change)) {
        Ok(v) => v,
        Err(e) => {
            let _ = events.send(Event::Error(format!("opened: {e}")));
            Vec::new()
        }
    };

    // The default changelist exists only as its open files; `describe` cannot
    // be asked about it.
    if opened.is_empty() && change != ChangeId::Default {
        return describe(p4, false);
    }

    opened.into_iter().map(FileEntry::opened).collect()
}
