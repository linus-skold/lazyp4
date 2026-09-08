//! Headless rendering and key-routing checks.
//!
//! A TUI cannot be driven interactively from CI, so these render the real
//! widget tree onto a [`TestBackend`] and assert on the resulting cells.

use crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use p4::{ChangeId, ChangeStatus, Changelist, FileAction, ServerInfo};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use crate::app::{App, Modal, Panel};
use crate::ui;
use crate::worker::{Event, FileEntry, Worker};

fn app() -> App {
    let mut app = App::new(Worker::detached());
    app.handle(Event::Info(ServerInfo {
        user: "linsko".into(),
        client: "linus-desktop".into(),
        client_known: true,
        host: "linus-desktop".into(),
        server_address: "ssl:example:1666".into(),
        server_version: "P4D/LINUX/2024.2".into(),
        ..Default::default()
    }));
    app.handle(Event::Changes(vec![
        change(395, ChangeStatus::Pending, false, "# Do not submit"),
        change(166, ChangeStatus::Pending, true, "<saved by Perforce>"),
        change(396, ChangeStatus::Submitted, false, "# Updated .p4ignore"),
    ]));
    app.handle(Event::Files {
        change: ChangeId::Number(395),
        files: vec![
            file("//darksim/main/AGENTS.md", FileAction::Add, false),
            file("//darksim/main/Foo.cpp", FileAction::Edit, true),
        ],
    });
    app.handle(Event::Idle);
    app
}

fn change(n: u32, status: ChangeStatus, shelved: bool, desc: &str) -> Changelist {
    Changelist {
        id: ChangeId::Number(n),
        status,
        user: "linsko".into(),
        client: "linus-desktop".into(),
        time: None,
        description: desc.into(),
        shelved,
    }
}

fn file(path: &str, action: FileAction, unresolved: bool) -> FileEntry {
    FileEntry {
        depot_path: path.into(),
        rev: Some(3),
        action,
        unresolved,
    }
}

fn render(app: &App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test backend");
    terminal
        .draw(|frame| ui::draw(frame, app))
        .expect("draw must not panic");
    let buffer = terminal.backend().buffer().clone();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol().to_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn press(app: &mut App, code: KeyCode) {
    app.handle(Event::Input(TermEvent::Key(KeyEvent::new_with_kind(
        code,
        KeyModifiers::NONE,
        KeyEventKind::Press,
    ))));
}

#[test]
fn shows_changelists_files_and_status() {
    let out = render(&app(), 120, 30);
    assert!(out.contains("Changelists (3)"), "{out}");
    assert!(out.contains("395"), "{out}");
    assert!(out.contains("# Do not submit"), "{out}");
    assert!(out.contains("//darksim/main/AGENTS.md"), "{out}");
    assert!(out.contains("(unresolved)"), "{out}");
    assert!(out.contains("linus-desktop"), "{out}");
}

#[test]
fn warns_when_no_client_is_set() {
    let mut app = app();
    app.handle(Event::Info(ServerInfo {
        client: "*unknown*".into(),
        client_known: false,
        ..Default::default()
    }));
    assert!(render(&app, 120, 30).contains("not set"));
}

#[test]
fn selecting_another_changelist_drops_the_old_file_list() {
    let mut app = app();
    assert_eq!(app.files.len(), 2);

    press(&mut app, KeyCode::Char('j'));

    assert_eq!(app.change_sel, 1);
    assert!(app.files.is_empty(), "files must not outlive their changelist");
    assert!(render(&app, 120, 30).contains("loading…"));
}

#[test]
fn a_stale_file_answer_is_ignored() {
    let mut app = app();
    press(&mut app, KeyCode::Char('j'));

    // An answer for the changelist we just moved off.
    app.handle(Event::Files {
        change: ChangeId::Number(395),
        files: vec![file("//stale", FileAction::Edit, false)],
    });

    assert!(app.files.is_empty());
}

#[test]
fn tab_cycles_panels_and_wraps() {
    let mut app = app();
    assert_eq!(app.focus, Panel::Changelists);
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.focus, Panel::Files);
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.focus, Panel::Status);
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.focus, Panel::Changelists);
}

#[test]
fn modals_open_and_close_without_quitting() {
    let mut app = app();
    press(&mut app, KeyCode::Char('?'));
    assert_eq!(app.modal, Modal::Help);
    assert!(render(&app, 120, 30).contains("cycle panel"));

    // `q` closes the overlay rather than the application.
    press(&mut app, KeyCode::Char('q'));
    assert_eq!(app.modal, Modal::None);
    assert!(!app.quit);

    press(&mut app, KeyCode::Char('q'));
    assert!(app.quit);
}

#[test]
fn key_release_events_are_ignored() {
    // Windows terminals report both press and release for every keystroke.
    let mut app = app();
    app.handle(Event::Input(TermEvent::Key(KeyEvent::new_with_kind(
        KeyCode::Char('j'),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    ))));
    assert_eq!(app.change_sel, 0);
}

/// Dev aid: `cargo test -p lazyp4 -- --ignored --nocapture preview` prints the
/// layout without needing a real terminal or a server.
#[test]
#[ignore = "prints the layout for inspection"]
fn preview() {
    println!("{}", render(&app(), 110, 26));
}

#[test]
fn renders_in_a_cramped_terminal() {
    // Must not panic when the constraints cannot all be satisfied.
    for (w, h) in [(40, 12), (20, 8), (80, 24), (200, 60)] {
        let mut app = app();
        app.modal = Modal::Log;
        app.handle(Event::Log("describe -s 395".into()));
        render(&app, w, h);
    }
}
