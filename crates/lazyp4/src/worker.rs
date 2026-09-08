//! The Perforce worker thread.
//!
//! `p4::Client` wraps a C++ `ClientApi`, which is not `Send` and whose `Run`
//! blocks until the server answers. So the client is built on this thread and
//! never leaves it; the UI talks to it over channels and stays responsive while
//! a command is in flight.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use p4::{
    diff, ChangeFilter, ChangeId, ChangeStatus, Changelist, Client, Connection, FileAction,
    FileDiff, ServerInfo,
};

/// A file in a changelist, from whichever command could see it.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub depot_path: String,
    pub rev: Option<u32>,
    pub action: FileAction,
    pub unresolved: bool,
}

pub enum Request {
    /// Reconnect and reload everything.
    Refresh,
    LoadFiles {
        change: ChangeId,
        status: ChangeStatus,
        shelved: bool,
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
        files: Vec<FileEntry>,
    },
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
}

impl Worker {
    /// Start the worker and return it together with the shared event channel.
    pub fn spawn() -> (Worker, Receiver<Event>) {
        let (req_tx, req_rx) = mpsc::channel::<Request>();
        let (ev_tx, ev_rx) = mpsc::channel::<Event>();

        let events = ev_tx.clone();
        thread::spawn(move || run(req_rx, events));
        spawn_input_reader(ev_tx);

        (Worker { requests: req_tx }, ev_rx)
    }

    /// A worker with nothing behind it. Requests are dropped, so an [`App`]
    /// built on one can be driven with canned data — used by the render tests.
    ///
    /// [`App`]: crate::app::App
    #[cfg(test)]
    pub fn detached() -> Worker {
        let (tx, _) = mpsc::channel();
        Worker { requests: tx }
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
                let _ = events.send(Event::Files { change, files });
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
                    rev: f.rev,
                    action: f.action,
                    unresolved: false,
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

    opened
        .into_iter()
        .map(|f| FileEntry {
            depot_path: f.depot_path,
            rev: f.rev,
            action: f.action,
            unresolved: f.unresolved,
        })
        .collect()
}
