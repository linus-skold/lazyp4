//! Headless rendering and key-routing checks.
//!
//! A TUI cannot be driven interactively from CI, so these render the real
//! widget tree onto a [`TestBackend`] and assert on the resulting cells.

use crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use p4::{ChangeId, ChangeStatus, Changelist, FileAction, FileDiff, ServerInfo};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use crate::app::{Action, App, ChangeTab, Modal, Panel};
use crate::ui;
use crate::worker::{Event, FileEntry, Request, Worker};

const MINE: &str = "linus-desktop";

fn app() -> App {
    let mut app = App::new(Worker::detached());
    app.handle(Event::Info(ServerInfo {
        user: "linsko".into(),
        client: MINE.into(),
        client_known: true,
        host: MINE.into(),
        server_address: "ssl:example:1666".into(),
        server_version: "P4D/LINUX/2024.2".into(),
        ..Default::default()
    }));
    app.handle(Event::Changes {
        pending: vec![
            default_change(),
            change(395, MINE, ChangeStatus::Pending, false, "# Do not submit"),
            change(308, MINE, ChangeStatus::Pending, false, "Interaction"),
            change(166, MINE, ChangeStatus::Pending, true, "<saved by Perforce>"),
            elsewhere(117, "jenkins-builder-2", "CI work"),
            theirs(106, "sarwag", "REVERT"),
        ],
        submitted: vec![change(
            396,
            MINE,
            ChangeStatus::Submitted,
            false,
            "# Updated .p4ignore",
        )],
    });
    // The default changelist sorts first and is therefore selected; step onto
    // 395 so the fixture has a numbered changelist to work with.
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char('j'));
    app.focus = Panel::Files;

    app.handle(Event::Files {
        change: ChangeId::Number(395),
        files: vec![
            file("//darksim/main/AGENTS.md", FileAction::Add, false),
            file("//darksim/main/Foo.cpp", FileAction::Edit, true),
        ],
        default_files: vec![file(
            "//darksim/main/Config/DefaultEngine.ini",
            FileAction::Edit,
            false,
        )],
    });
    app.handle(Event::Diff {
        change: ChangeId::Number(395),
        files: vec![FileDiff {
            depot_path: "//darksim/main/Foo.cpp".into(),
            rev: Some(3),
            hunks: "@@ -1,3 +1,3 @@\n context\n-gone\n+added\n".into(),
        }],
    });
    app.handle(Event::Idle);
    app
}

fn change(n: u32, client: &str, status: ChangeStatus, shelved: bool, desc: &str) -> Changelist {
    Changelist {
        id: ChangeId::Number(n),
        status,
        user: "linsko".into(),
        client: client.into(),
        time: None,
        description: desc.into(),
        shelved,
    }
}

/// What the worker synthesizes for the default changelist.
fn default_change() -> Changelist {
    Changelist {
        id: ChangeId::Default,
        status: ChangeStatus::Pending,
        user: "linsko".into(),
        client: MINE.into(),
        time: None,
        description: "files not in a numbered changelist".into(),
        shelved: false,
    }
}

/// Ours, but open on a different workspace.
fn elsewhere(n: u32, client: &str, desc: &str) -> Changelist {
    change(n, client, ChangeStatus::Pending, false, desc)
}

/// Somebody else's.
fn theirs(n: u32, user: &str, desc: &str) -> Changelist {
    Changelist {
        user: user.into(),
        client: format!("{user}_PC"),
        ..change(n, "", ChangeStatus::Pending, false, desc)
    }
}

fn file(path: &str, action: FileAction, unresolved: bool) -> FileEntry {
    FileEntry {
        depot_path: path.into(),
        local_path: None,
        rev: Some(3),
        action,
        unresolved,
        opened: true,
    }
}

/// A file the workspace scan found but Perforce has not opened.
fn unopened(path: &str, action: FileAction) -> FileEntry {
    FileEntry {
        depot_path: path.into(),
        local_path: Some(format!("E:\\ws{}", path.trim_start_matches("//darksim/main"))),
        rev: None,
        action,
        unresolved: false,
        opened: false,
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
fn panels_are_numbered_status_first_and_diff_zero() {
    assert_eq!(Panel::Status.number(), 1);
    assert_eq!(Panel::Files.number(), 2);
    assert_eq!(Panel::Changelists.number(), 3);
    assert_eq!(Panel::History.number(), 4);
    assert_eq!(Panel::Diff.number(), 0);
}

#[test]
fn every_panel_shows_its_number() {
    let out = render(&app(), 120, 40);
    for panel in Panel::ORDER {
        let title = format!(" {}  {}", panel.number(), panel.title());
        assert!(out.contains(&title), "missing {title:?} in\n{out}");
    }
}

#[test]
fn number_keys_focus_their_panel() {
    let mut app = app();
    for panel in Panel::ORDER {
        app.focus = Panel::Status;
        press(&mut app, KeyCode::Char((b'0' + panel.number()) as char));
        assert_eq!(app.focus, panel, "key {} should focus", panel.number());
    }
}

#[test]
fn tab_cycles_panels_top_to_bottom_then_the_diff() {
    let mut app = app();
    app.focus = Panel::Status;
    for expected in [
        Panel::Files,
        Panel::Changelists,
        Panel::History,
        Panel::Diff,
        Panel::Status,
    ] {
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.focus, expected);
    }
}

#[test]
fn brackets_switch_tabs_within_the_changelist_panel() {
    let mut app = app();
    app.focus = Panel::Changelists;
    assert_eq!(app.tab, ChangeTab::Local);

    press(&mut app, KeyCode::Char(']'));
    assert_eq!(app.tab, ChangeTab::Shelved);
    press(&mut app, KeyCode::Char(']'));
    assert_eq!(app.tab, ChangeTab::Others);
    press(&mut app, KeyCode::Char(']'));
    assert_eq!(app.tab, ChangeTab::Local, "tabs wrap");
    press(&mut app, KeyCode::Char('['));
    assert_eq!(app.tab, ChangeTab::Others, "and wrap backwards");
}

#[test]
fn brackets_do_not_move_focus_between_panels() {
    let mut app = app();
    app.focus = Panel::Files;
    press(&mut app, KeyCode::Char(']'));
    assert_eq!(app.focus, Panel::Files);
    assert_eq!(app.tab, ChangeTab::Local, "another panel has no tabs to turn");
}

#[test]
fn tabs_partition_pending_changelists() {
    let app = app();
    // default, 395, 308 and 117 are ours and unshelved; 166 is shelved;
    // 106 belongs to another user.
    assert_eq!(app.tab_count(ChangeTab::Local), 4);
    assert_eq!(app.tab_count(ChangeTab::Shelved), 1);
    assert_eq!(app.tab_count(ChangeTab::Others), 1);

    let ids: Vec<String> = app.tab_changes().iter().map(|c| c.id.to_string()).collect();
    assert_eq!(ids, ["default", "395", "308", "117"]);
}

#[test]
fn the_default_changelist_is_listed_and_explains_itself() {
    // `p4 changes` never reports the default changelist; the worker builds one,
    // and it must reach the list.
    let app = app();
    // Wide enough that the description is not truncated.
    let out = render(&app, 180, 40);
    assert!(out.contains("default"), "{out}");
    assert!(out.contains("files not in a numbered changelist"), "{out}");
}

#[test]
fn ownership_is_by_user_so_it_survives_an_unresolved_client() {
    // Running outside a workspace leaves the client unknown. Our changelists
    // must still be ours.
    let mut app = app();
    app.handle(Event::Info(ServerInfo {
        user: "linsko".into(),
        client: "*unknown*".into(),
        client_known: false,
        ..Default::default()
    }));

    assert_eq!(app.tab_count(ChangeTab::Local), 4);
    assert_eq!(app.tab_count(ChangeTab::Shelved), 1);
    assert_eq!(app.tab_count(ChangeTab::Others), 1);
}

#[test]
fn a_changelist_on_another_workspace_is_marked() {
    let out = render(&app(), 120, 40);
    assert!(
        out.contains("@jenkins-builder-2"),
        "one of our changelists is open elsewhere and should say so\n{out}"
    );
    assert!(
        !out.contains(&format!("@{MINE}")),
        "our own workspace is the default and needs no label"
    );
}

#[test]
fn submitted_changelists_live_in_history_not_the_tabs() {
    let app = app();
    assert_eq!(app.submitted.len(), 1);
    assert!(
        !app.pending.iter().any(|c| c.status == ChangeStatus::Submitted),
        "history must not leak into the pending tabs"
    );
    let out = render(&app, 120, 40);
    assert!(out.contains("# Updated .p4ignore"), "{out}");
}

#[test]
fn the_tab_bar_shows_counts_and_marks_the_open_tab() {
    let out = render(&app(), 120, 40);
    assert!(out.contains("Local 4"), "{out}");
    assert!(out.contains("Shelved 1"), "{out}");
    assert!(out.contains("Others 1"), "{out}");
}

#[test]
fn selecting_in_history_repoints_the_files_panel() {
    let mut app = app();
    assert_eq!(app.selected_change().unwrap().id, ChangeId::Number(395));

    app.focus = Panel::History;
    press(&mut app, KeyCode::Char('g'));

    assert_eq!(app.selected_change().unwrap().id, ChangeId::Number(396));
    // The old changelist's files must not linger under the new heading.
    assert!(app.files.is_empty());
}

#[test]
fn switching_tab_repoints_the_files_panel() {
    let mut app = app();
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char(']'));

    assert_eq!(app.selected_change().unwrap().id, ChangeId::Number(166));
    assert!(app.files.is_empty());
}

#[test]
fn a_stale_file_answer_is_ignored() {
    let mut app = app();
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char('j'));

    app.handle(Event::Files {
        change: ChangeId::Number(395),
        files: vec![file("//stale", FileAction::Edit, false)],
        default_files: Vec::new(),
    });

    assert!(app.files.is_empty());
}

#[test]
fn the_diff_pane_follows_the_selected_file() {
    let mut app = app();
    assert!(app.selected_diff().is_none());

    app.focus = Panel::Files;
    press(&mut app, KeyCode::Char('j'));

    assert_eq!(
        app.selected_diff().unwrap().depot_path,
        "//darksim/main/Foo.cpp"
    );
    let out = render(&app, 120, 40);
    assert!(out.contains("@@ -1,3 +1,3 @@"), "{out}");
    assert!(out.contains("+added"), "{out}");
}

#[test]
fn files_are_split_into_the_changelist_and_the_default_group() {
    let app = app();
    let out = render(&app, 120, 40);
    assert!(out.contains("In changelist 395"), "{out}");
    assert!(out.contains("Default"), "{out}");
    // Selection spans both groups as one sequence.
    assert_eq!(app.all_files().len(), 3);
}

#[test]
fn the_cursor_walks_from_one_group_into_the_next() {
    let mut app = app();
    app.focus = Panel::Files;
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('j'));

    assert_eq!(
        app.selected_file().unwrap().depot_path,
        "//darksim/main/Config/DefaultEngine.ini",
        "j past the last file of the changelist lands in the default group"
    );
}

#[test]
fn space_sends_a_changelist_file_back_to_default() {
    let mut app = app();
    app.focus = Panel::Files;
    press(&mut app, KeyCode::Char(' '));

    let Some(Request::MoveFiles { change, files }) = app.last_request() else {
        panic!("expected a move");
    };
    assert_eq!(change, ChangeId::Default);
    assert_eq!(files[0].depot_path, "//darksim/main/AGENTS.md");
}

#[test]
fn space_pulls_a_default_file_into_the_selected_changelist() {
    let mut app = app();
    app.focus = Panel::Files;
    // Step down into the Default group.
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char(' '));

    let Some(Request::MoveFiles { change, files }) = app.last_request() else {
        panic!("expected a move");
    };
    assert_eq!(change, ChangeId::Number(395));
    assert_eq!(files[0].depot_path, "//darksim/main/Config/DefaultEngine.ini");
}

#[test]
fn space_refuses_when_there_is_no_numbered_changelist_to_move_into() {
    let mut app = app();
    // Back to the default changelist, where both ends of the move are the same.
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char('g'));
    app.handle(Event::Files {
        change: ChangeId::Default,
        files: vec![file("//darksim/main/Loose.cpp", FileAction::Edit, false)],
        default_files: Vec::new(),
    });

    app.focus = Panel::Files;
    app.last_request(); // discard the setup traffic
    press(&mut app, KeyCode::Char(' '));

    assert!(app.last_request().is_none());
    assert!(app
        .error
        .as_deref()
        .is_some_and(|e| e.contains("numbered changelist")));
}

#[test]
fn space_refuses_to_edit_a_submitted_changelist() {
    let mut app = app();
    app.focus = Panel::History;
    press(&mut app, KeyCode::Char('g'));
    app.handle(Event::Files {
        change: ChangeId::Number(396),
        files: vec![file("//darksim/main/.p4ignore", FileAction::Edit, false)],
        default_files: Vec::new(),
    });

    app.focus = Panel::Files;
    app.last_request(); // discard the setup traffic
    press(&mut app, KeyCode::Char(' '));

    assert!(app.last_request().is_none());
    assert!(app
        .error
        .as_deref()
        .is_some_and(|e| e.contains("submitted")));
}

#[test]
fn scanned_files_join_the_default_group_and_untracked_ones_show_two_question_marks() {
    let mut app = app();
    app.handle(Event::Scanned(vec![
        unopened("//darksim/main/NewThing.cpp", FileAction::Add),
        unopened("//darksim/main/Changed.cpp", FileAction::Edit),
    ]));

    assert_eq!(app.all_files().len(), 5);
    let out = render(&app, 120, 40);
    assert!(out.contains("??"), "an untracked file is marked ??\n{out}");
    assert!(out.contains("NewThing.cpp"), "{out}");
    assert!(out.contains("Changed.cpp"), "{out}");
}

#[test]
fn a_scanned_file_that_is_already_open_is_not_listed_twice() {
    // `p4 status` reports open files too; they are already in a group.
    let mut app = app();
    app.handle(Event::Scanned(vec![unopened(
        "//darksim/main/AGENTS.md",
        FileAction::Add,
    )]));

    assert_eq!(app.all_files().len(), 3, "no duplicate of the open file");
}

#[test]
fn space_on_an_untracked_file_opens_it_for_add_by_local_path() {
    let mut app = app();
    app.handle(Event::Scanned(vec![unopened(
        "//darksim/main/NewThing.cpp",
        FileAction::Add,
    )]));

    app.focus = Panel::Files;
    press(&mut app, KeyCode::Char('G'));
    press(&mut app, KeyCode::Char(' '));

    let Some(Request::MoveFiles { change, files }) = app.last_request() else {
        panic!("expected a move");
    };
    assert_eq!(change, ChangeId::Number(395));
    assert!(files[0].untracked());
    // A file Perforce has never seen has no usable depot path.
    assert!(files[0].command_path().starts_with("E:\\ws"));
}

#[test]
fn u_starts_one_scan_at_a_time() {
    let mut app = app();
    press(&mut app, KeyCode::Char('u'));
    assert!(app.scanning);
    assert!(matches!(app.last_request(), Some(Request::ScanWorkspace)));

    // A second press while the first is still running must not queue another.
    press(&mut app, KeyCode::Char('u'));
    assert!(app.last_request().is_none());
}

#[test]
fn moving_between_files_resets_the_diff_scroll() {
    let mut app = app();
    app.focus = Panel::Diff;
    press(&mut app, KeyCode::Char('j'));
    assert_eq!(app.diff_scroll, 1);

    app.focus = Panel::Files;
    press(&mut app, KeyCode::Char('j'));
    assert_eq!(app.diff_scroll, 0, "scroll belongs to the file, not the pane");
}

#[test]
fn enter_asks_to_open_the_changelist_patch() {
    let mut app = app();
    press(&mut app, KeyCode::Enter);

    let Some(Action::OpenInHunk(patch)) = app.action.take() else {
        panic!("expected a patch to open");
    };
    assert!(patch.contains("diff --git a/darksim/main/Foo.cpp"));
    assert!(patch.contains("+added"));
}

#[test]
fn enter_with_nothing_to_diff_reports_it() {
    let mut app = app();
    app.diffs.clear();
    press(&mut app, KeyCode::Enter);

    assert!(app.action.is_none());
    assert!(app
        .error
        .as_deref()
        .is_some_and(|e| e.contains("nothing to diff")));
}

#[test]
fn warns_when_no_client_is_set_and_says_what_to_do() {
    let mut app = app();
    app.handle(Event::Info(ServerInfo {
        user: "linsko".into(),
        client: "*unknown*".into(),
        client_known: false,
        ..Default::default()
    }));
    let out = render(&app, 120, 40);
    assert!(out.contains("not set"), "{out}");
    assert!(out.contains("start lazyp4 in a workspace"), "{out}");
}

#[test]
fn modals_open_and_close_without_quitting() {
    let mut app = app();
    press(&mut app, KeyCode::Char('?'));
    assert_eq!(app.modal, Modal::Help);
    assert!(render(&app, 120, 40).contains("switch tab within a panel"));

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
    app.focus = Panel::Changelists;
    let before = app.change_sel;
    app.handle(Event::Input(TermEvent::Key(KeyEvent::new_with_kind(
        KeyCode::Char('j'),
        KeyModifiers::NONE,
        KeyEventKind::Release,
    ))));
    assert_eq!(app.change_sel, before);
}

/// Dev aid: `cargo test -p lazyp4 -- --ignored --nocapture preview` prints the
/// layout without needing a real terminal or a server.
#[test]
#[ignore = "prints the layout for inspection"]
fn preview() {
    let mut app = app();
    app.handle(Event::Scanned(vec![
        unopened("//darksim/main/NewThing.cpp", FileAction::Add),
        unopened("//darksim/main/Changed.cpp", FileAction::Edit),
    ]));
    app.focus = Panel::Files;
    println!("{}", render(&app, 110, 34));
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
