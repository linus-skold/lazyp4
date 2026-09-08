//! The Perforce worker thread.
//!
//! `p4::Client` wraps a C++ `ClientApi`, which is not `Send` and whose `Run`
//! blocks until the server answers. So the client is built on this thread and
//! never leaves it; the UI talks to it over channels and stays responsive while
//! a command is in flight.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use p4::{ChangeFilter, ChangeId, ChangeStatus, Changelist, Client, Connection, FileAction, ServerInfo};

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
    Shutdown,
}

/// What the worker sends back. Terminal input arrives on the same channel so
/// the UI has one place to wait.
pub enum Event {
    Input(crossterm::event::Event),
    Info(ServerInfo),
    Changes(Vec<Changelist>),
    Files {
        change: ChangeId,
        files: Vec<FileEntry>,
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

fn run(requests: Receiver<Request>, events: Sender<Event>) {
    let mut client: Option<Client> = None;

    while let Ok(req) = requests.recv() {
        if matches!(req, Request::Shutdown) {
            return;
        }

        // Connect lazily, and again after the server drops us.
        if client.as_mut().is_none_or(Client::dropped) {
            match Client::connect(&Connection::default()) {
                Ok(c) => {
                    let _ = events.send(Event::Log("connect".into()));
                    client = Some(c);
                }
                Err(e) => {
                    let _ = events.send(Event::Error(format!("connect: {e}")));
                    let _ = events.send(Event::Idle);
                    continue;
                }
            }
        }
        let p4 = client.as_mut().expect("connected above");

        match req {
            Request::Shutdown => return,
            Request::Refresh => {
                let _ = events.send(Event::Log("info".into()));
                match p4.info() {
                    Ok(info) => {
                        let _ = events.send(Event::Info(info));
                    }
                    Err(e) => {
                        let _ = events.send(Event::Error(format!("info: {e}")));
                    }
                }

                let _ = events.send(Event::Log("changes -l -s pending".into()));
                let mut all = match p4.changes(&ChangeFilter::pending()) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = events.send(Event::Error(format!("changes: {e}")));
                        Vec::new()
                    }
                };

                let _ = events.send(Event::Log("changes -l -s submitted -m 25".into()));
                match p4.changes(&ChangeFilter::submitted(25)) {
                    Ok(v) => all.extend(v),
                    Err(e) => {
                        let _ = events.send(Event::Error(format!("changes: {e}")));
                    }
                }

                let _ = events.send(Event::Changes(all));
            }
            Request::LoadFiles {
                change,
                status,
                shelved,
            } => {
                let files = load_files(p4, &events, change, status, shelved);
                let _ = events.send(Event::Files { change, files });
            }
        }

        let _ = events.send(Event::Idle);
    }
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
    match p4.opened(Some(change)) {
        Ok(v) if !v.is_empty() => v
            .into_iter()
            .map(|f| FileEntry {
                depot_path: f.depot_path,
                rev: f.rev,
                action: f.action,
                unresolved: f.unresolved,
            })
            .collect(),
        Ok(_) => describe(p4, false),
        Err(e) => {
            let _ = events.send(Event::Error(format!("opened: {e}")));
            describe(p4, false)
        }
    }
}
