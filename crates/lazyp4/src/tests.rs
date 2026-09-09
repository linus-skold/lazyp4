//! Headless rendering and key-routing checks.
//!
//! A TUI cannot be driven interactively from CI, so these render the real
//! widget tree onto a [`TestBackend`] and assert on the resulting cells.

use crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use p4::{ChangeId, ChangeStatus, Changelist, FileAction, FileDiff, ServerInfo};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use crate::app::{
    has_description, ignore_entry, App, ChangeTab, Destination, FileRow, Modal, Panel,
};
use crate::config::Config;
use crate::ui;
use crate::worker::{Event, FileEntry, PostCreate, Request, Worker};

const MINE: &str = "linus-desktop";

fn app() -> App {
    app_with(Config::default())
}

fn app_with(config: Config) -> App {
    let mut app = App::new(Worker::detached(), config);
    app.handle(Event::Info(ServerInfo {
        user: "linsko".into(),
        client: MINE.into(),
        client_known: true,
        // Matches the local paths `unopened` hands out.
        client_root: Some("E:\\ws".into()),
        host: MINE.into(),
        server_address: "ssl:example:1666".into(),
        server_version: "P4D/LINUX/2024.2".into(),
        // Pinned so the tree root does not shift with the files under test:
        // without a stream it is whatever directory they happen to share.
        stream: Some("//darksim/main".into()),
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

/// Answer a revert preview the way a server that agrees would: every file the
/// app asked about comes back.
fn allow_revert(app: &mut App) -> Vec<FileEntry> {
    let Some(Request::PreviewRevert { files }) = app.last_request() else {
        panic!("expected a revert preview");
    };
    let preview = files
        .iter()
        .map(|f| p4::RevertPreview {
            depot_path: f.depot_path.clone(),
            action: f.action.clone(),
        })
        .collect();
    app.handle(Event::RevertPreview {
        files: files.clone(),
        preview,
    });
    files
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

/// Every foreground colour the drawn screen used.
fn colors(app: &App, width: u16, height: u16) -> Vec<ratatui::style::Color> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test backend");
    terminal
        .draw(|frame| ui::draw(frame, app))
        .expect("draw must not panic");
    let buffer = terminal.backend().buffer().clone();
    let mut seen: Vec<ratatui::style::Color> = buffer.content().iter().map(|c| c.fg).collect();
    seen.sort_by_key(|c| format!("{c:?}"));
    seen.dedup();
    seen
}

fn press(app: &mut App, code: KeyCode) {
    chord(app, code, KeyModifiers::NONE);
}

fn chord(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    app.handle(Event::Input(TermEvent::Key(KeyEvent::new_with_kind(
        code,
        modifiers,
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

    // The default group's file sits under Config/, so the directory row comes
    // first.
    assert!(
        matches!(app.selected_row(), Some(FileRow::Dir { label, .. }) if label == "Config/"),
        "j past the last file of the changelist lands on the next group's tree"
    );

    press(&mut app, KeyCode::Char('j'));
    assert_eq!(
        app.selected_file().unwrap().depot_path,
        "//darksim/main/Config/DefaultEngine.ini"
    );
}

/// Deeper paths than the shared fixture, to exercise the tree.
fn nested() -> App {
    let mut app = app();
    app.files = vec![
        file("//darksim/main/Source/Darksim/Actors/Door.cpp", FileAction::Edit, false),
        file("//darksim/main/Source/Darksim/Actors/Door.h", FileAction::Edit, false),
        file("//darksim/main/Source/Editor/Tool.cpp", FileAction::Add, false),
        file("//darksim/main/README.md", FileAction::Edit, false),
    ];
    app.loose_files.clear();
    app.file_sel = 0;
    app
}

#[test]
fn files_are_shown_as_a_tree_below_the_depot_root() {
    let out = render(&nested(), 120, 40);
    // Each directory is its own row and the shared depot prefix is gone.
    assert!(out.contains("Source/"), "{out}");
    assert!(out.contains("Darksim/"), "{out}");
    assert!(out.contains("Actors/"), "{out}");
    assert!(out.contains("Door.cpp"), "{out}");
    assert!(
        !out.contains("Darksim/Actors/"),
        "directories are not stacked onto one row\n{out}"
    );
    assert!(
        !out.contains("//darksim/main/Source"),
        "the root prefix should not be repeated on every row\n{out}"
    );
}

#[test]
fn collapsing_a_directory_hides_its_files() {
    let mut app = nested();
    app.focus = Panel::Files;
    // Row 0 is Source/, the first directory.
    assert!(matches!(app.selected_row(), Some(FileRow::Dir { .. })));

    press(&mut app, KeyCode::Char('h'));
    let out = render(&app, 120, 40);
    assert!(out.contains("Source/"), "{out}");
    assert!(!out.contains("Door.cpp"), "collapsed contents are hidden\n{out}");

    press(&mut app, KeyCode::Char('l'));
    assert!(render(&app, 120, 40).contains("Door.cpp"), "and come back");
}

#[test]
fn folding_a_directory_leaves_the_same_name_in_the_other_group_alone() {
    // The two groups are separate trees; a directory can hold different files
    // in each, so their fold state is separate too.
    let mut app = app();
    app.files = vec![file("//darksim/main/Config/A.ini", FileAction::Edit, false)];
    app.loose_files = vec![file("//darksim/main/Config/B.ini", FileAction::Edit, false)];
    app.file_sel = 0;
    app.focus = Panel::Files;

    // Row 0 is the changelist group's Config/.
    press(&mut app, KeyCode::Enter);

    let out = render(&app, 120, 40);
    assert!(!out.contains("A.ini"), "the folded group is hidden\n{out}");
    assert!(
        out.contains("B.ini"),
        "the other group's Config/ stays open\n{out}"
    );
}

#[test]
fn each_group_remembers_its_own_folds() {
    let mut app = app();
    app.files = vec![file("//darksim/main/Config/A.ini", FileAction::Edit, false)];
    app.loose_files = vec![file("//darksim/main/Config/B.ini", FileAction::Edit, false)];
    app.file_sel = 0;
    app.focus = Panel::Files;

    press(&mut app, KeyCode::Enter); // fold the changelist group
    press(&mut app, KeyCode::Char('j')); // onto the default group's Config/
    press(&mut app, KeyCode::Enter); // fold that one too

    let out = render(&app, 120, 40);
    assert!(!out.contains("A.ini"), "{out}");
    assert!(!out.contains("B.ini"), "{out}");

    press(&mut app, KeyCode::Enter); // unfold only the default group
    let out = render(&app, 120, 40);
    assert!(!out.contains("A.ini"), "the first stays folded\n{out}");
    assert!(out.contains("B.ini"), "{out}");
}

#[test]
fn enter_on_a_directory_folds_it_rather_than_going_fullscreen() {
    let mut app = nested();
    app.focus = Panel::Files;
    press(&mut app, KeyCode::Enter);

    assert!(!app.diff_fullscreen, "Enter on a folder is not the diff key");
    assert!(!render(&app, 120, 40).contains("Door.cpp"));
}

#[test]
fn a_directory_row_counts_the_files_beneath_it() {
    let out = render(&nested(), 120, 40);
    // Source/ holds three of the four files.
    assert!(out.contains("Source/ 3"), "{out}");
}

#[test]
fn space_on_a_directory_moves_everything_under_it() {
    let mut app = nested();
    app.focus = Panel::Files;
    app.last_request();

    // Source/ then Darksim/ then Actors/, each on its own row.
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('j'));
    let Some(FileRow::Dir { label, .. }) = app.selected_row() else {
        panic!("expected a directory row");
    };
    assert_eq!(label, "Actors/");

    press(&mut app, KeyCode::Char(' '));
    let Some(Request::MoveFiles { change, files }) = app.last_request() else {
        panic!("expected a move");
    };
    assert_eq!(change, ChangeId::Default, "out of the changelist");
    let moved: Vec<&str> = files.iter().map(|f| f.depot_path.as_str()).collect();
    assert_eq!(
        moved,
        [
            "//darksim/main/Source/Darksim/Actors/Door.cpp",
            "//darksim/main/Source/Darksim/Actors/Door.h"
        ],
        "only the files under that directory, not its siblings"
    );
}

#[test]
fn the_stream_is_the_tree_root_when_the_server_reports_one() {
    let mut app = nested();
    app.handle(Event::Info(ServerInfo {
        user: "linsko".into(),
        client: MINE.into(),
        client_known: true,
        stream: Some("//darksim/main".into()),
        ..Default::default()
    }));
    assert_eq!(app.depot_root(), "//darksim/main");
    assert!(render(&app, 120, 40).contains("README.md"));
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

/// Sitting on the default changelist, where a move has no implied destination.
fn on_default() -> App {
    let mut app = app();
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char('g'));
    app.handle(Event::Files {
        change: ChangeId::Default,
        files: vec![file("//darksim/main/Loose.cpp", FileAction::Edit, false)],
        default_files: Vec::new(),
    });
    app.focus = Panel::Files;
    app.last_request(); // discard the setup traffic
    app
}

#[test]
fn space_on_the_default_changelist_asks_where_to_move() {
    let mut app = on_default();
    press(&mut app, KeyCode::Char(' '));

    let picker = app.picker.as_ref().expect("a picker should open");
    assert!(matches!(&picker.what, PostCreate::Move(files) if files.len() == 1));
    // Our numbered changelists, then the option of a new one.
    assert!(matches!(picker.options[0], Destination::Existing(id, _) if id == ChangeId::Number(395)));
    assert_eq!(picker.options.last(), Some(&Destination::New));
    assert!(
        !picker.options.iter().any(
            |o| matches!(o, Destination::Existing(id, _) if *id == ChangeId::Default)
        ),
        "the default changelist is where the files already are"
    );

    let out = render(&app, 120, 40);
    assert!(out.contains("Move 1 file to"), "{out}");
    assert!(out.contains("create a changelist"), "{out}");
}

#[test]
fn the_picker_offers_only_our_own_changelists() {
    let mut app = on_default();
    press(&mut app, KeyCode::Char(' '));

    let picker = app.picker.as_ref().unwrap();
    let ids: Vec<String> = picker
        .options
        .iter()
        .filter_map(|o| match o {
            Destination::Existing(id, _) => Some(id.to_string()),
            Destination::New => None,
        })
        .collect();
    // 106 belongs to another user; 166 is ours even though it is shelved.
    assert_eq!(ids, ["395", "308", "166", "117"]);
}

#[test]
fn choosing_an_existing_changelist_moves_the_files() {
    let mut app = on_default();
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char('j')); // 308
    press(&mut app, KeyCode::Enter);

    assert!(app.picker.is_none());
    let Some(Request::MoveFiles { change, files }) = app.last_request() else {
        panic!("expected a move");
    };
    assert_eq!(change, ChangeId::Number(308));
    assert_eq!(files[0].depot_path, "//darksim/main/Loose.cpp");
}

#[test]
fn choosing_new_asks_for_a_description_before_creating_anything() {
    let mut app = on_default();
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char('G')); // the "new" row
    press(&mut app, KeyCode::Enter);

    assert!(app.picker.is_none());
    let editor = app.editor.as_ref().expect("a description is required first");
    assert_eq!(editor.title, "Description of the new changelist");
    assert_eq!(editor.text(), "", "a new changelist starts empty");
    assert!(
        app.last_request().is_none(),
        "nothing is created until the description is given"
    );

    press(&mut app, KeyCode::Char('x'));
    press(&mut app, KeyCode::Enter);

    let Some(Request::CreateChange { description, then }) = app.last_request() else {
        panic!("expected a create");
    };
    assert_eq!(description, "x");
    let PostCreate::Move(files) = then else {
        panic!("the new changelist should take the files");
    };
    assert_eq!(files[0].depot_path, "//darksim/main/Loose.cpp");
}

fn stream(path: &str, kind: &str) -> p4::Stream {
    p4::Stream {
        path: path.into(),
        name: path.rsplit('/').next().unwrap_or_default().into(),
        parent: "//darksim/main".into(),
        kind: kind.into(),
        owner: "linsko".into(),
    }
}

#[test]
fn plus_gives_the_focused_panel_most_of_the_column() {
    let mut app = nested();
    app.focus = Panel::Files;
    let before = render(&app, 100, 24);

    press(&mut app, KeyCode::Char('+'));
    let zoomed = render(&app, 100, 24);
    assert_ne!(before, zoomed);
    // Everything in the tree fits once the other panels give up their rows.
    assert!(zoomed.contains("Tool.cpp"), "{zoomed}");
    assert!(zoomed.contains("README.md"), "{zoomed}");
    // The other panels are still there, just narrow.
    assert!(zoomed.contains("Changelists"), "{zoomed}");

    press(&mut app, KeyCode::Char('_'));
    assert_eq!(render(&app, 100, 24), before, "and back again");
}

#[test]
fn zoom_follows_the_focused_panel() {
    let mut app = app();
    app.focus = Panel::Files;
    press(&mut app, KeyCode::Char('+'));
    let on_files = render(&app, 100, 24);

    app.focus = Panel::History;
    assert_ne!(render(&app, 100, 24), on_files);
}

#[test]
fn a_running_command_is_named_rather_than_just_working() {
    let mut app = app();
    app.handle(Event::Log("status".into()));
    app.busy = true;

    let out = render(&app, 120, 40);
    assert!(out.contains("p4 status"), "{out}");
}

#[test]
fn the_spinner_turns_on_a_tick() {
    let mut app = app();
    app.busy = true;
    app.handle(Event::Log("sync".into()));

    let first = render(&app, 120, 40);
    app.tick();
    let second = render(&app, 120, 40);
    assert_ne!(first, second, "a long command should not look stuck");
}

#[test]
fn p_syncs_the_workspace_and_says_what_it_did() {
    let mut app = app();
    app.last_request();
    press(&mut app, KeyCode::Char('p'));

    assert!(matches!(app.last_request(), Some(Request::Sync)));

    app.handle(Event::Notice("14 file(s) updated".into()));
    app.handle(Event::Idle);
    let out = render(&app, 120, 40);
    assert!(out.contains("14 file(s) updated"), "{out}");
}

#[test]
fn a_notice_clears_a_previous_error() {
    let mut app = app();
    app.handle(Event::Error("something went wrong".into()));
    app.handle(Event::Notice("already up to date".into()));

    assert!(app.error.is_none());
    assert!(render(&app, 120, 40).contains("already up to date"));
}

#[test]
fn b_lists_the_streams_and_marks_the_current_one() {
    let mut app = app();
    app.last_request();
    press(&mut app, KeyCode::Char('b'));

    assert_eq!(app.modal, Modal::Streams);
    assert!(matches!(app.last_request(), Some(Request::LoadStreams)));

    app.handle(Event::Streams(vec![
        stream("//darksim/main", "mainline"),
        stream("//darksim/dev", "virtual"),
    ]));
    app.handle(Event::Idle);

    let out = render(&app, 120, 40);
    assert!(out.contains("//darksim/dev"), "{out}");
    assert!(out.contains("mainline"), "{out}");
    // The fixture's workspace is on main, so that row is marked.
    assert!(out.contains("▸ //darksim/main"), "{out}");
}

#[test]
fn switching_stream_is_confirmed_and_warns_about_the_resync() {
    let mut app = app();
    press(&mut app, KeyCode::Char('b'));
    app.handle(Event::Streams(vec![
        stream("//darksim/main", "mainline"),
        stream("//darksim/dev", "virtual"),
    ]));
    app.last_request();

    press(&mut app, KeyCode::Char('j')); // onto dev
    press(&mut app, KeyCode::Enter);

    let confirm = app.confirm.as_ref().expect("switching needs confirming");
    assert_eq!(confirm.title, "Switch to //darksim/dev?");
    assert!(confirm.lines.iter().any(|l| l.contains("resynced")));

    press(&mut app, KeyCode::Char('y'));
    let Some(Request::SwitchStream { stream }) = app.last_request() else {
        panic!("expected a switch");
    };
    assert_eq!(stream, "//darksim/dev");
}

#[test]
fn switching_to_the_stream_already_on_is_refused() {
    let mut app = app();
    press(&mut app, KeyCode::Char('b'));
    app.handle(Event::Streams(vec![stream("//darksim/main", "mainline")]));
    app.last_request();

    press(&mut app, KeyCode::Enter);
    assert!(app.confirm.is_none());
    assert!(app.last_request().is_none());
    assert!(app.error.as_deref().is_some_and(|e| e.contains("already on")));
}

#[test]
fn v_selects_a_range_that_every_verb_then_acts_on() {
    let mut app = nested();
    app.focus = Panel::Files;
    // Onto Door.cpp: Source/, Darksim/, Actors/, then the files.
    for _ in 0..3 {
        press(&mut app, KeyCode::Char('j'));
    }
    app.last_request();

    press(&mut app, KeyCode::Char('v'));
    press(&mut app, KeyCode::Char('j')); // extend over Door.h
    assert_eq!(app.selection_range(), (3, 4));

    press(&mut app, KeyCode::Char('s'));
    let Some(Request::Shelve { files, .. }) = app.last_request() else {
        panic!("expected a shelve");
    };
    assert_eq!(files.len(), 2, "both rows, not just the cursor");
}

#[test]
fn a_range_extends_upwards_too() {
    let mut app = nested();
    app.focus = Panel::Files;
    press(&mut app, KeyCode::Char('G')); // the last row
    let last = app.file_sel;

    press(&mut app, KeyCode::Char('v'));
    press(&mut app, KeyCode::Char('k'));
    assert_eq!(app.selection_range(), (last - 1, last));
}

#[test]
fn a_range_covering_a_directory_takes_its_contents_once() {
    let mut app = nested();
    app.focus = Panel::Files;
    app.last_request();

    // From Source/ down through Actors/ and both its files.
    press(&mut app, KeyCode::Char('v'));
    for _ in 0..4 {
        press(&mut app, KeyCode::Char('j'));
    }
    press(&mut app, KeyCode::Char('d'));
    allow_revert(&mut app);
    press(&mut app, KeyCode::Char('y'));

    let Some(Request::RevertFiles { files }) = app.last_request() else {
        panic!("expected a revert");
    };
    let paths: Vec<&str> = files.iter().map(|f| f.depot_path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "//darksim/main/Source/Darksim/Actors/Door.cpp",
            "//darksim/main/Source/Darksim/Actors/Door.h",
            "//darksim/main/Source/Editor/Tool.cpp",
        ],
        "a directory and its own files in one range must not double up"
    );
}

#[test]
fn the_range_is_dropped_once_it_has_been_acted_on() {
    let mut app = nested();
    app.focus = Panel::Files;
    press(&mut app, KeyCode::Char('v'));
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('d'));

    assert!(
        app.select_anchor.is_none(),
        "a highlighted range would suggest it is still pending"
    );
}

#[test]
fn v_again_or_esc_abandons_the_range() {
    for key in [KeyCode::Char('v'), KeyCode::Esc] {
        let mut app = nested();
        app.focus = Panel::Files;
        press(&mut app, KeyCode::Char('v'));
        press(&mut app, KeyCode::Char('j'));
        assert!(app.select_anchor.is_some());

        press(&mut app, key);
        assert!(app.select_anchor.is_none(), "{key:?}");
        assert_eq!(app.selection_range().0, app.selection_range().1);
    }
}

#[test]
fn the_selected_range_is_shown() {
    let mut app = nested();
    app.focus = Panel::Files;
    press(&mut app, KeyCode::Char('v'));
    press(&mut app, KeyCode::Char('j'));

    let out = render(&app, 120, 40);
    assert!(out.contains("2 row(s) selected"), "{out}");
    assert!(out.contains("v or Esc cancel"), "{out}");
}

#[test]
fn a_range_only_makes_sense_in_the_files_panel() {
    let mut app = app();
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char('v'));

    assert!(app.select_anchor.is_none());
    assert!(app.error.as_deref().is_some_and(|e| e.contains("in Files")));
}

fn unresolved(path: &str) -> p4::Unresolved {
    p4::Unresolved {
        local_path: format!("E:\\ws{}", path.trim_start_matches("//darksim/main")),
        from_path: path.into(),
        start_rev: Some(10),
        end_rev: Some(12),
        resolve_type: "content".into(),
        content_type: "3waytext".into(),
    }
}

/// The resolve view, open with one file waiting.
fn resolving() -> App {
    let mut app = app();
    press(&mut app, KeyCode::Char('R'));
    app.handle(Event::Unresolved(vec![unresolved(
        "//darksim/main/Config/DefaultEngine.ini",
    )]));
    app.last_request();
    app
}

#[test]
fn shift_r_lists_what_needs_resolving() {
    let mut app = app();
    app.last_request();
    press(&mut app, KeyCode::Char('R'));

    assert_eq!(app.modal, Modal::Resolve);
    assert!(matches!(app.last_request(), Some(Request::LoadUnresolved)));

    app.handle(Event::Unresolved(vec![unresolved(
        "//darksim/main/Config/DefaultEngine.ini",
    )]));
    app.handle(Event::Idle);

    let out = render(&app, 120, 40);
    assert!(out.contains("1 file(s) to resolve"), "{out}");
    assert!(out.contains("DefaultEngine.ini"), "{out}");
    assert!(out.contains("3waytext"), "{out}");
    assert!(out.contains("#10,#12"), "the range that arrived\n{out}");
}

#[test]
fn an_empty_resolve_list_says_so_rather_than_looking_broken() {
    let mut app = app();
    press(&mut app, KeyCode::Char('R'));
    app.handle(Event::Unresolved(Vec::new()));
    app.handle(Event::Idle);

    assert!(render(&app, 120, 40).contains("nothing to resolve"));
}

#[test]
fn merging_needs_no_confirmation() {
    // -am only succeeds where there is nothing to argue about.
    let mut app = resolving();
    press(&mut app, KeyCode::Char('m'));

    assert!(app.confirm.is_none());
    let Some(Request::Resolve { how, paths }) = app.last_request() else {
        panic!("expected a resolve");
    };
    assert_eq!(how, p4::Resolution::Merge);
    assert_eq!(paths.len(), 1);
}

#[test]
fn taking_one_side_outright_is_confirmed_and_says_what_is_lost() {
    let mut app = resolving();
    press(&mut app, KeyCode::Char('y'));

    let confirm = app.confirm.as_ref().expect("keeping yours discards theirs");
    assert!(confirm.title.contains("Keep your copy"));
    assert!(confirm.lines.iter().any(|l| l.contains("depot is discarded")));

    press(&mut app, KeyCode::Char('y'));
    assert!(matches!(
        app.last_request(),
        Some(Request::Resolve { how, .. }) if how == p4::Resolution::Yours
    ));

    let mut app = resolving();
    press(&mut app, KeyCode::Char('t'));
    let confirm = app.confirm.as_ref().expect("taking theirs discards yours");
    assert!(confirm.lines.iter().any(|l| l.contains("local changes are discarded")));
}

#[test]
fn the_resolve_view_uses_the_local_path_perforce_expects() {
    let mut app = resolving();
    press(&mut app, KeyCode::Char('a'));

    let Some(Request::Resolve { paths, .. }) = app.last_request() else {
        panic!("expected a resolve");
    };
    assert!(paths[0].starts_with("E:\\ws"), "{:?}", paths);
}

#[test]
fn the_resolve_view_closes_without_quitting() {
    let mut app = resolving();
    press(&mut app, KeyCode::Char('R'));
    assert_eq!(app.modal, Modal::None);
    assert!(!app.quit);
}

fn type_filter(app: &mut App, text: &str) {
    press(app, KeyCode::Char('/'));
    for c in text.chars() {
        press(app, KeyCode::Char(c));
    }
}

#[test]
fn slash_narrows_the_files_panel() {
    let mut app = nested();
    app.focus = Panel::Files;

    type_filter(&mut app, "door");
    let out = render(&app, 120, 40);
    assert!(out.contains("Door.cpp"), "{out}");
    assert!(out.contains("Door.h"), "{out}");
    assert!(!out.contains("Tool.cpp"), "the rest is hidden\n{out}");
    assert!(!out.contains("README.md"), "{out}");
}

#[test]
fn filtering_is_case_insensitive() {
    let mut app = nested();
    app.focus = Panel::Files;
    type_filter(&mut app, "DOOR");
    assert!(render(&app, 120, 40).contains("Door.cpp"));
}

#[test]
fn the_filter_shows_in_the_panel_title() {
    // A narrowed list looks like a short one otherwise.
    let mut app = nested();
    app.focus = Panel::Files;
    type_filter(&mut app, "door");
    assert!(render(&app, 120, 40).contains("/door"));
}

#[test]
fn enter_keeps_the_filter_and_esc_clears_it() {
    let mut app = nested();
    app.focus = Panel::Files;

    type_filter(&mut app, "door");
    press(&mut app, KeyCode::Enter);
    assert!(app.filtering.is_none(), "typing has stopped");
    assert_eq!(app.filter(Panel::Files), "door", "but the list stays narrowed");

    press(&mut app, KeyCode::Char('/'));
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.filter(Panel::Files), "");
    assert!(render(&app, 120, 40).contains("Tool.cpp"), "everything is back");
}

#[test]
fn backspace_widens_the_filter_again() {
    let mut app = nested();
    app.focus = Panel::Files;
    type_filter(&mut app, "doorx");
    assert!(!render(&app, 120, 40).contains("Door.cpp"), "nothing matches");

    press(&mut app, KeyCode::Backspace);
    assert!(render(&app, 120, 40).contains("Door.cpp"));
}

#[test]
fn a_filter_swallows_keys_that_are_commands_elsewhere() {
    let mut app = nested();
    app.focus = Panel::Files;
    press(&mut app, KeyCode::Char('/'));
    app.last_request();

    for c in ['d', 'c', 'q', 's'] {
        press(&mut app, KeyCode::Char(c));
    }

    assert!(!app.quit, "q is filter text, not quit");
    assert!(app.confirm.is_none(), "d did not start a revert");
    assert_eq!(app.filter(Panel::Files), "dcqs");
}

#[test]
fn each_panel_keeps_its_own_filter() {
    let mut app = app();

    app.focus = Panel::Changelists;
    type_filter(&mut app, "interaction");
    press(&mut app, KeyCode::Enter);

    app.focus = Panel::History;
    type_filter(&mut app, "p4ignore");
    press(&mut app, KeyCode::Enter);

    assert_eq!(app.filter(Panel::Changelists), "interaction");
    assert_eq!(app.filter(Panel::History), "p4ignore");

    let ids: Vec<String> = app.tab_changes().iter().map(|c| c.id.to_string()).collect();
    assert_eq!(ids, ["308"], "matched on the description");
}

#[test]
fn a_changelist_filter_matches_number_user_or_description() {
    let mut app = app();
    app.focus = Panel::Changelists;

    type_filter(&mut app, "395");
    assert_eq!(app.tab_changes().len(), 1, "by number");

    press(&mut app, KeyCode::Esc);
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char(']'));
    press(&mut app, KeyCode::Char(']')); // Others
    type_filter(&mut app, "sarwag");
    assert_eq!(app.tab_changes().len(), 1, "by user");
}

#[test]
fn narrowing_past_the_selection_does_not_leave_it_stranded() {
    let mut app = app();
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char('G')); // the last changelist
    let before = app.change_sel;
    assert!(before > 0);

    type_filter(&mut app, "395");
    assert_eq!(app.tab_changes().len(), 1);
    assert_eq!(app.change_sel, 0, "the cursor moves into the shorter list");
    assert_eq!(app.selected_change().unwrap().id, ChangeId::Number(395));
}

#[test]
fn the_diff_panel_has_no_list_to_filter() {
    let mut app = app();
    app.focus = Panel::Diff;
    press(&mut app, KeyCode::Char('/'));

    assert!(app.filtering.is_none());
    assert!(app.error.as_deref().is_some_and(|e| e.contains("not a list")));
}

/// Sitting on 166, the fixture's shelved changelist.
fn on_shelved() -> App {
    let mut app = app();
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char(']')); // the Shelved tab
    app.last_request();
    app
}

#[test]
fn s_shelves_a_changelist_that_has_no_shelf_yet() {
    let mut app = app();
    app.focus = Panel::Changelists;
    app.last_request();

    press(&mut app, KeyCode::Char('s'));

    // Nothing is lost by shelving for the first time, so no confirmation.
    assert!(app.confirm.is_none());
    let Some(Request::Shelve { change, files }) = app.last_request() else {
        panic!("expected a shelve");
    };
    assert_eq!(change, ChangeId::Number(395));
    assert!(files.is_empty(), "the whole changelist");
}

#[test]
fn s_on_an_existing_shelf_confirms_before_replacing_it() {
    let mut app = on_shelved();
    press(&mut app, KeyCode::Char('s'));

    let confirm = app.confirm.as_ref().expect("replacing needs confirming");
    assert_eq!(confirm.title, "Replace the shelf on 166?");
    assert!(confirm.lines.iter().any(|l| l.contains("no longer open")));

    press(&mut app, KeyCode::Char('y'));
    assert!(matches!(
        app.last_request(),
        Some(Request::ReplaceShelf { change }) if change == ChangeId::Number(166)
    ));
}

#[test]
fn s_in_files_shelves_only_the_selection() {
    let mut app = app();
    app.focus = Panel::Files;
    app.last_request();

    press(&mut app, KeyCode::Char('s'));

    let Some(Request::Shelve { change, files }) = app.last_request() else {
        panic!("expected a shelve");
    };
    assert_eq!(change, ChangeId::Number(395));
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].depot_path, "//darksim/main/AGENTS.md");
}

#[test]
fn shift_s_unshelves_through_the_picker() {
    let mut app = on_shelved();
    press(&mut app, KeyCode::Char('S'));

    let picker = app.picker.as_ref().expect("a destination is needed");
    assert_eq!(picker.title, "Unshelve 166 into");
    assert!(matches!(picker.what, PostCreate::Unshelve(id) if id == ChangeId::Number(166)));
    assert!(
        !picker.options.iter().any(
            |o| matches!(o, Destination::Existing(id, _) if *id == ChangeId::Number(166))
        ),
        "unshelving into itself is not offered"
    );

    press(&mut app, KeyCode::Enter);
    let Some(Request::Unshelve { from, into }) = app.last_request() else {
        panic!("expected an unshelve");
    };
    assert_eq!(from, ChangeId::Number(166));
    assert_eq!(into, ChangeId::Number(395));
}

#[test]
fn unshelving_into_a_new_changelist_asks_for_a_description() {
    let mut app = on_shelved();
    press(&mut app, KeyCode::Char('S'));
    press(&mut app, KeyCode::Char('G')); // the "new" row
    press(&mut app, KeyCode::Enter);
    app.last_request();

    press(&mut app, KeyCode::Char('x'));
    press(&mut app, KeyCode::Enter);

    let Some(Request::CreateChange { then, .. }) = app.last_request() else {
        panic!("expected a create");
    };
    assert!(matches!(then, PostCreate::Unshelve(id) if id == ChangeId::Number(166)));
}

#[test]
fn shift_d_deletes_a_shelf_after_confirming() {
    let mut app = on_shelved();
    press(&mut app, KeyCode::Char('D'));

    let confirm = app.confirm.as_ref().expect("a confirmation is required");
    assert_eq!(confirm.title, "Delete the shelf on 166?");
    assert!(confirm.lines.iter().any(|l| l.contains("Open files stay")));

    press(&mut app, KeyCode::Char('y'));
    assert!(matches!(
        app.last_request(),
        Some(Request::DeleteShelf { change }) if change == ChangeId::Number(166)
    ));
}

#[test]
fn unshelving_and_deleting_need_something_shelved() {
    let mut app = app();
    app.focus = Panel::Changelists;
    app.last_request();

    for key in ['S', 'D'] {
        press(&mut app, KeyCode::Char(key));
        assert!(app.picker.is_none() && app.confirm.is_none(), "{key}");
        assert!(app.last_request().is_none(), "{key}");
        assert!(app
            .error
            .as_deref()
            .is_some_and(|e| e.contains("nothing shelved")));
    }
}

#[test]
fn the_default_changelist_cannot_be_shelved() {
    let mut app = on_default();
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char('s'));

    assert!(app.last_request().is_none());
    assert!(app
        .error
        .as_deref()
        .is_some_and(|e| e.contains("numbered pending changelist")));
}

fn revision(rev: u32, change: u32, desc: &str) -> p4::Revision {
    p4::Revision {
        rev,
        change,
        action: FileAction::Edit,
        user: "linsko".into(),
        time: Some(1_788_895_733),
        file_type: "text".into(),
        description: desc.into(),
    }
}

#[test]
fn shift_u_undoes_a_submitted_change_into_a_new_changelist() {
    let mut app = app();
    app.focus = Panel::History;
    press(&mut app, KeyCode::Char('g'));
    app.last_request();

    press(&mut app, KeyCode::Char('U'));
    let confirm = app.confirm.as_ref().expect("a confirmation is required");
    assert_eq!(confirm.title, "Undo change 396?");
    assert!(
        confirm.lines.iter().any(|l| l.contains("Nothing is submitted")),
        "the confirmation says the depot is untouched: {:?}",
        confirm.lines
    );

    press(&mut app, KeyCode::Char('y'));
    let Some(Request::Undo { spec, description }) = app.last_request() else {
        panic!("expected an undo");
    };
    assert_eq!(spec, "//darksim/main/...@=396");
    assert_eq!(description, "Undo of change 396");
}

#[test]
fn shift_u_in_the_history_view_undoes_one_revision() {
    let mut app = app();
    app.focus = Panel::Files;
    press(&mut app, KeyCode::Char('H'));
    app.handle(Event::History {
        depot_path: "//darksim/main/AGENTS.md".into(),
        revisions: vec![revision(7, 396, "# Updated .p4ignore"), revision(6, 390, "earlier")],
    });
    app.last_request();

    press(&mut app, KeyCode::Char('U'));
    let confirm = app.confirm.as_ref().expect("a confirmation is required");
    assert_eq!(confirm.title, "Undo revision #7 of AGENTS.md?");
    assert!(confirm.lines.iter().any(|l| l.contains("change 396")));

    press(&mut app, KeyCode::Char('y'));
    let Some(Request::Undo { spec, description }) = app.last_request() else {
        panic!("expected an undo");
    };
    assert_eq!(spec, "//darksim/main/AGENTS.md#7");
    assert_eq!(description, "Undo of //darksim/main/AGENTS.md#7");
}

#[test]
fn the_history_view_undoes_the_revision_under_the_cursor() {
    let mut app = app();
    app.focus = Panel::Files;
    press(&mut app, KeyCode::Char('H'));
    app.handle(Event::History {
        depot_path: "//darksim/main/AGENTS.md".into(),
        revisions: vec![revision(7, 396, "newest"), revision(6, 390, "earlier")],
    });

    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('U'));
    press(&mut app, KeyCode::Char('y'));

    let Some(Request::Undo { spec, .. }) = app.last_request() else {
        panic!("expected an undo");
    };
    assert_eq!(spec, "//darksim/main/AGENTS.md#6");
}

#[test]
fn the_first_revision_of_a_file_cannot_be_undone() {
    let mut app = app();
    app.focus = Panel::Files;
    press(&mut app, KeyCode::Char('H'));
    app.handle(Event::History {
        depot_path: "//darksim/main/AGENTS.md".into(),
        revisions: vec![revision(1, 300, "added")],
    });
    app.last_request();

    press(&mut app, KeyCode::Char('U'));
    assert!(app.confirm.is_none());
    assert!(app.last_request().is_none());
    assert!(app
        .error
        .as_deref()
        .is_some_and(|e| e.contains("first revision")));
}

#[test]
fn a_pending_change_cannot_be_undone() {
    let mut app = app();
    app.focus = Panel::Changelists;
    app.last_request();

    press(&mut app, KeyCode::Char('U'));
    assert!(app.confirm.is_none());
    assert!(app.last_request().is_none());
    assert!(app
        .error
        .as_deref()
        .is_some_and(|e| e.contains("submitted change")));
}

#[test]
fn shift_h_shows_the_history_of_the_selected_file() {
    let mut app = app();
    app.focus = Panel::Files;
    app.last_request();

    press(&mut app, KeyCode::Char('H'));

    assert_eq!(app.modal, Modal::History);
    let Some(Request::LoadHistory { depot_path }) = app.last_request() else {
        panic!("expected a filelog");
    };
    assert_eq!(depot_path, "//darksim/main/AGENTS.md");

    app.handle(Event::History {
        depot_path,
        revisions: vec![revision(7, 396, "# Updated .p4ignore")],
    });

    let out = render(&app, 120, 40);
    assert!(out.contains("History of darksim/main/AGENTS.md"), "{out}");
    assert!(out.contains("#7"), "{out}");
    assert!(out.contains("396"), "{out}");
    assert!(out.contains("2026-09-08"), "the date is shown\n{out}");
    assert!(out.contains("# Updated .p4ignore"), "{out}");
}

#[test]
fn history_for_a_file_the_cursor_has_left_is_ignored() {
    let mut app = app();
    app.focus = Panel::Files;
    press(&mut app, KeyCode::Char('H'));

    app.handle(Event::History {
        depot_path: "//darksim/main/Somewhere/Else.cpp".into(),
        revisions: vec![revision(1, 1, "stale")],
    });

    assert!(app.history.is_empty());
}

fn blamed(change: u32, user: &str, text: &str) -> p4::AnnotatedLine {
    p4::AnnotatedLine {
        change,
        user: user.into(),
        time: Some(1_788_895_733),
        text: text.into(),
    }
}

#[test]
fn a_blames_the_selected_file_line_by_line() {
    let mut app = app();
    app.focus = Panel::Files;
    app.last_request();

    press(&mut app, KeyCode::Char('a'));

    assert_eq!(app.modal, Modal::Blame);
    let Some(Request::LoadBlame { depot_path }) = app.last_request() else {
        panic!("expected an annotate");
    };
    assert_eq!(depot_path, "//darksim/main/AGENTS.md");

    app.handle(Event::Blame {
        depot_path,
        lines: vec![
            blamed(390, "sarwag", "# Agents"),
            blamed(396, "linsko", "\tone rule"),
        ],
    });
    app.handle(Event::Idle);

    let out = render(&app, 120, 40);
    assert!(out.contains("Blame of darksim/main/AGENTS.md"), "{out}");
    assert!(out.contains("390"), "{out}");
    assert!(out.contains("sarwag"), "{out}");
    assert!(out.contains("2026-09-08"), "the date is shown\n{out}");
    assert!(out.contains("# Agents"), "{out}");
    assert!(
        !out.contains('\t'),
        "a terminal draws a tab as one cell or none\n{out}"
    );
}

#[test]
fn a_run_of_lines_from_one_change_is_named_once() {
    let mut app = app();
    app.focus = Panel::Files;
    press(&mut app, KeyCode::Char('a'));
    app.handle(Event::Blame {
        depot_path: "//darksim/main/AGENTS.md".into(),
        lines: vec![
            blamed(396, "linsko", "first"),
            blamed(396, "linsko", "second"),
            blamed(390, "sarwag", "third"),
        ],
    });

    let out = render(&app, 120, 40);
    assert_eq!(out.matches("396").count(), 1, "one heading per run\n{out}");
    assert_eq!(out.matches("390").count(), 1, "{out}");
}

#[test]
fn blame_for_a_file_the_cursor_has_left_is_ignored() {
    let mut app = app();
    app.focus = Panel::Files;
    press(&mut app, KeyCode::Char('a'));

    app.handle(Event::Blame {
        depot_path: "//darksim/main/Somewhere/Else.cpp".into(),
        lines: vec![blamed(1, "nobody", "stale")],
    });

    assert!(app.blame.is_empty());
}

#[test]
fn a_file_the_depot_has_never_seen_cannot_be_blamed() {
    let mut app = app();
    app.files.clear();
    app.loose_files = vec![unopened("//darksim/main/New.cpp", FileAction::Add)];
    app.file_sel = 0;
    app.focus = Panel::Files;
    app.last_request();

    press(&mut app, KeyCode::Char('a'));

    assert_eq!(app.modal, Modal::None);
    assert!(app.last_request().is_none());
    assert!(app
        .error
        .as_deref()
        .is_some_and(|e| e.contains("not in the depot")));
}

#[test]
fn blame_closes_without_quitting() {
    for key in [KeyCode::Esc, KeyCode::Char('a')] {
        let mut app = app();
        app.focus = Panel::Files;
        press(&mut app, KeyCode::Char('a'));
        press(&mut app, key);

        assert_eq!(app.modal, Modal::None);
        assert!(!app.quit);
    }
}

#[test]
fn history_closes_without_quitting() {
    let mut app = app();
    app.focus = Panel::Files;
    press(&mut app, KeyCode::Char('H'));
    press(&mut app, KeyCode::Esc);

    assert_eq!(app.modal, Modal::None);
    assert!(!app.quit);
}

#[test]
fn history_needs_a_file_not_a_directory() {
    let mut app = nested();
    app.focus = Panel::Files;
    app.last_request();
    // Row 0 is Source/.
    press(&mut app, KeyCode::Char('H'));

    assert_eq!(app.modal, Modal::None);
    assert!(app.last_request().is_none());
    assert!(app.error.as_deref().is_some_and(|e| e.contains("select a file")));
}

#[test]
fn c_submits_after_confirming_and_listing_the_files() {
    let mut app = app();
    app.last_request();
    press(&mut app, KeyCode::Char('c'));

    let confirm = app.confirm.as_ref().expect("a confirmation is required");
    assert_eq!(confirm.title, "Submit changelist 395 to the depot?");
    assert!(confirm.lines.iter().any(|l| l.contains("AGENTS.md")));
    assert!(confirm.lines.iter().any(|l| l.contains("Foo.cpp")));
    assert!(app.last_request().is_none(), "nothing happens until y");

    press(&mut app, KeyCode::Char('y'));
    assert!(matches!(
        app.last_request(),
        Some(Request::Submit { change }) if change == ChangeId::Number(395)
    ));
}

#[test]
fn a_changelist_without_a_real_description_is_not_submitted() {
    // Perforce writes `<saved by Perforce>` itself; it is not a message.
    assert!(!has_description("<saved by Perforce>"));
    assert!(!has_description("   "));
    assert!(has_description("# Do not submit"));

    let mut app = app();
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char(']')); // the Shelved tab holds 166
    app.last_request();

    press(&mut app, KeyCode::Char('c'));
    assert!(app.confirm.is_none());
    assert!(app
        .error
        .as_deref()
        .is_some_and(|e| e.contains("description")));
}

#[test]
fn the_default_changelist_is_submitted_under_a_description_typed_first() {
    let mut app = on_default();
    press(&mut app, KeyCode::Char('c'));

    // It is not a spec and has no description of its own, so one is asked for.
    let editor = app.editor.as_ref().expect("a description is required");
    assert!(editor.title.contains("default changelist"), "{}", editor.title);
    assert!(app.confirm.is_none());
    assert!(app.last_request().is_none());

    for c in "Loose work".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    press(&mut app, KeyCode::Enter);

    assert!(app.editor.is_none());
    let confirm = app.confirm.as_ref().expect("a confirmation is required");
    assert_eq!(confirm.title, "Submit the default changelist to the depot?");
    assert!(confirm.lines.iter().any(|l| l.contains("Loose.cpp")));
    assert!(confirm.lines.iter().any(|l| l.contains("Loose work")));
    assert!(
        confirm.lines.iter().any(|l| l.contains("Everything open")),
        "no -c means the whole default changelist goes: {:?}",
        confirm.lines
    );
    assert!(app.last_request().is_none(), "nothing happens until y");

    press(&mut app, KeyCode::Char('y'));
    let Some(Request::SubmitDefault { description }) = app.last_request() else {
        panic!("expected a default submit");
    };
    assert_eq!(description, "Loose work");
}

#[test]
fn an_empty_default_changelist_has_nothing_to_submit() {
    let mut app = on_default();
    app.files.clear();

    press(&mut app, KeyCode::Char('c'));
    assert!(app.editor.is_none());
    assert!(app.confirm.is_none());
    assert!(app.error.as_deref().is_some_and(|e| e.contains("no files")));
}

#[test]
fn somebody_elses_changelist_is_not_submitted() {
    let mut app = app();
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char(']'));
    press(&mut app, KeyCode::Char(']')); // Others
    app.last_request();

    press(&mut app, KeyCode::Char('c'));
    assert!(app.confirm.is_none());
    assert!(app
        .error
        .as_deref()
        .is_some_and(|e| e.contains("somebody else")));
}

#[test]
fn an_already_submitted_changelist_is_not_submitted_again() {
    let mut app = app();
    app.focus = Panel::History;
    press(&mut app, KeyCode::Char('g'));
    app.last_request();

    press(&mut app, KeyCode::Char('c'));
    assert!(app.confirm.is_none());
    assert!(app
        .error
        .as_deref()
        .is_some_and(|e| e.contains("already submitted")));
}

#[test]
fn d_in_files_reverts_after_confirming_and_naming_the_files() {
    let mut app = app();
    app.focus = Panel::Files;
    app.last_request();

    press(&mut app, KeyCode::Char('d'));
    assert!(
        app.confirm.is_none(),
        "the question waits on the server's own account of the damage"
    );
    allow_revert(&mut app);

    let confirm = app.confirm.as_ref().expect("a confirmation is required");
    assert_eq!(confirm.title, "Revert 1 file(s)?");
    assert!(
        confirm.lines.iter().any(|l| l.contains("AGENTS.md")),
        "the confirmation names what is at stake: {:?}",
        confirm.lines
    );
    assert!(confirm.lines.iter().any(|l| l.contains("will be lost")));
    assert!(app.last_request().is_none(), "nothing happens until y");

    press(&mut app, KeyCode::Char('y'));
    let Some(Request::RevertFiles { files }) = app.last_request() else {
        panic!("expected a revert");
    };
    assert_eq!(files[0].depot_path, "//darksim/main/AGENTS.md");
}

#[test]
fn reverting_a_directory_takes_everything_under_it() {
    let mut app = nested();
    app.focus = Panel::Files;
    app.last_request();
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('j')); // Actors/

    press(&mut app, KeyCode::Char('d'));
    allow_revert(&mut app);
    press(&mut app, KeyCode::Char('y'));

    let Some(Request::RevertFiles { files }) = app.last_request() else {
        panic!("expected a revert");
    };
    assert_eq!(files.len(), 2);
}

#[test]
fn the_revert_confirmation_lists_what_the_server_would_do() {
    let mut app = app();
    app.focus = Panel::Files;
    press(&mut app, KeyCode::Char('v'));
    press(&mut app, KeyCode::Char('j')); // both files in the changelist
    press(&mut app, KeyCode::Char('d'));

    let Some(Request::PreviewRevert { files }) = app.last_request() else {
        panic!("expected a revert preview");
    };
    assert_eq!(files.len(), 2);

    // The server has since seen one of them closed, so it would revert one.
    app.handle(Event::RevertPreview {
        files: files.clone(),
        preview: vec![p4::RevertPreview {
            depot_path: "//darksim/main/Foo.cpp".into(),
            action: FileAction::Edit,
        }],
    });

    let confirm = app.confirm.as_ref().expect("a confirmation is required");
    assert_eq!(confirm.title, "Revert 1 file(s)?");
    assert!(
        confirm.lines.iter().any(|l| l.contains("Foo.cpp")),
        "{:?}",
        confirm.lines
    );
    assert!(
        !confirm.lines.iter().any(|l| l.contains("AGENTS.md")),
        "what the server would not touch must not be listed: {:?}",
        confirm.lines
    );
}

#[test]
fn a_revert_the_server_would_do_nothing_about_says_so() {
    let mut app = app();
    app.focus = Panel::Files;
    press(&mut app, KeyCode::Char('d'));

    let Some(Request::PreviewRevert { files }) = app.last_request() else {
        panic!("expected a revert preview");
    };
    app.handle(Event::RevertPreview {
        files,
        preview: Vec::new(),
    });

    assert!(app.confirm.is_none());
    assert!(app
        .error
        .as_deref()
        .is_some_and(|e| e.contains("would revert nothing")));
}

#[test]
fn a_file_that_is_not_open_has_nothing_to_revert() {
    let mut app = app();
    app.files.clear();
    app.loose_files = vec![unopened("//darksim/main/New.cpp", FileAction::Add)];
    app.file_sel = 0;
    app.focus = Panel::Files;
    app.last_request();

    press(&mut app, KeyCode::Char('d'));

    assert!(app.confirm.is_none());
    assert!(app.last_request().is_none());
    assert!(app
        .error
        .as_deref()
        .is_some_and(|e| e.contains("not open")));
}

#[test]
fn d_deletes_an_empty_changelist_after_confirming() {
    let mut app = app();
    app.focus = Panel::Changelists;
    // 308 has no files loaded, so nothing is known to be in the way.
    press(&mut app, KeyCode::Char('j'));
    app.files.clear();
    app.files_for = None;
    app.last_request();

    press(&mut app, KeyCode::Char('d'));
    let confirm = app.confirm.as_ref().expect("a confirmation is required");
    assert_eq!(confirm.title, "Delete changelist 308?");
    assert!(app.last_request().is_none(), "nothing happens until y");

    let out = render(&app, 120, 40);
    assert!(out.contains("y to confirm"), "{out}");

    press(&mut app, KeyCode::Char('y'));
    assert!(matches!(
        app.last_request(),
        Some(Request::DeleteChange { change }) if change == ChangeId::Number(308)
    ));
}

#[test]
fn any_key_but_y_cancels_a_confirmation() {
    for key in [KeyCode::Char('n'), KeyCode::Esc, KeyCode::Enter] {
        let mut app = app();
        app.focus = Panel::Changelists;
        press(&mut app, KeyCode::Char('j'));
        app.files.clear();
        app.files_for = None;
        press(&mut app, KeyCode::Char('d'));
        app.last_request();

        press(&mut app, key);
        assert!(app.confirm.is_none(), "{key:?} should close it");
        assert!(app.last_request().is_none(), "{key:?} must not confirm");
    }
}

#[test]
fn a_changelist_holding_files_is_not_offered_for_deletion() {
    let mut app = app();
    app.focus = Panel::Changelists;
    app.last_request();
    // 395's files are loaded by the fixture.
    press(&mut app, KeyCode::Char('d'));

    assert!(app.confirm.is_none());
    assert!(app.last_request().is_none());
    assert!(app
        .error
        .as_deref()
        .is_some_and(|e| e.contains("move or revert")));
}

#[test]
fn the_default_and_submitted_changelists_cannot_be_deleted() {
    let mut app = app();
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Char('d'));
    assert!(app.confirm.is_none());
    assert!(app.error.as_deref().is_some_and(|e| e.contains("default")));

    app.focus = Panel::History;
    press(&mut app, KeyCode::Char('g'));
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char('d'));
    assert!(app.confirm.is_none());
    assert!(app.error.as_deref().is_some_and(|e| e.contains("submitted")));
}

#[test]
fn n_creates_an_empty_changelist() {
    let mut app = app();
    press(&mut app, KeyCode::Char('n'));

    let editor = app.editor.as_ref().expect("a description is required");
    assert_eq!(editor.title, "Description of the new changelist");
    app.last_request();

    press(&mut app, KeyCode::Char('x'));
    press(&mut app, KeyCode::Enter);

    let Some(Request::CreateChange { description, then }) = app.last_request() else {
        panic!("expected a create");
    };
    assert_eq!(description, "x");
    assert!(
        matches!(then, PostCreate::Nothing),
        "nothing is moved into it"
    );
}

#[test]
fn esc_closes_the_picker_without_moving_anything() {
    let mut app = on_default();
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Esc);

    assert!(app.picker.is_none());
    assert!(app.last_request().is_none());
}

#[test]
fn abandoning_the_new_changelist_description_creates_nothing() {
    let mut app = on_default();
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char('G'));
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Esc);

    assert!(app.editor.is_none());
    assert!(app.last_request().is_none());
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
fn a_rebound_key_moves_the_command_off_the_old_one() {
    let mut app = app_with(Config::parse("[keys]\nsubmit = C\n"));
    app.handle(Event::Files {
        change: ChangeId::Number(395),
        files: vec![file("//darksim/main/AGENTS.md", FileAction::Add, false)],
        default_files: Vec::new(),
    });
    app.last_request();

    press(&mut app, KeyCode::Char('c'));
    assert!(app.confirm.is_none(), "c no longer submits");

    press(&mut app, KeyCode::Char('C'));
    assert!(app.confirm.is_some(), "C does");
}

#[test]
fn the_help_sheet_and_the_status_bar_name_the_keys_actually_bound() {
    let mut app = app_with(Config::parse("[keys]\nsubmit = C\nblame = ctrl-b\n"));
    app.focus = Panel::Files;

    let bar = render(&app, 120, 40);
    assert!(bar.contains("ctrl-b"), "the bar hints the real key\n{bar}");

    press(&mut app, KeyCode::Char('?'));
    let out = render(&app, 120, 40);
    assert!(out.contains("C        submit"), "{out}");
    assert!(out.contains("ctrl-b   blame"), "{out}");
}

#[test]
fn the_theme_changes_what_is_drawn() {
    use ratatui::style::Color;
    let plain = colors(&app(), 120, 40);
    assert!(plain.contains(&Color::Yellow), "the default focus colour");

    let themed = app_with(Config::parse("[theme]\nfocus = \"#ff00ff\"\n"));
    let seen = colors(&themed, 120, 40);
    assert!(seen.contains(&Color::Rgb(255, 0, 255)), "{seen:?}");
}

#[test]
fn the_diff_uses_the_configured_tab_width() {
    let drawn = |width: &str| {
        let mut app = app_with(Config::parse(&format!("[diff]\ntab_width = {width}\n")));
        app.diffs = vec![FileDiff {
            depot_path: "//darksim/main/AGENTS.md".into(),
            rev: Some(3),
            hunks: "@@ -1,1 +1,1 @@\n \tindented\n".into(),
        }];
        app.diffs_for = Some(ChangeId::Number(395));
        app.focus = Panel::Files;
        render(&app, 120, 40)
    };

    // The gutter is the same either way, so the column the word lands in is
    // the tab and nothing else.
    let column = |out: &str| {
        out.lines()
            .find(|l| l.contains("indented"))
            .and_then(|l| l.find("indented"))
            .expect("the diff line is drawn")
    };
    assert_eq!(column(&drawn("8")) - column(&drawn("2")), 6);
}

#[test]
fn a_config_lazyp4_could_not_read_says_so_rather_than_going_quiet() {
    let app = app_with(Config::parse("[keys]\nsubmitt = c\n"));
    assert!(app
        .error
        .as_deref()
        .is_some_and(|e| e.contains("no action named submitt")));
}

#[test]
fn the_first_look_at_the_workspace_is_not_a_change() {
    let mut app = app();
    app.last_request();

    app.poll_external();
    assert!(matches!(app.last_request(), Some(Request::CheckExternal)));

    app.handle(Event::External("edit //darksim/main/Foo.cpp".into()));
    assert!(app.notice.is_none());
    assert!(app.last_request().is_none(), "nothing to reload yet");
}

#[test]
fn p4_used_in_another_window_reloads_and_says_so() {
    let mut app = app();
    app.handle(Event::External("edit //darksim/main/Foo.cpp".into()));
    app.last_request();

    app.handle(Event::External("edit //darksim/main/Foo.cpp\nedit //darksim/main/Bar.cpp".into()));

    assert!(matches!(app.last_request(), Some(Request::Refresh)));
    assert!(app
        .notice
        .as_deref()
        .is_some_and(|n| n.contains("outside lazyp4")));
}

#[test]
fn an_unchanged_workspace_is_left_alone() {
    let mut app = app();
    app.handle(Event::External("edit //darksim/main/Foo.cpp".into()));
    app.last_request();

    app.handle(Event::External("edit //darksim/main/Foo.cpp".into()));

    assert!(app.last_request().is_none());
    assert!(app.notice.is_none());
}

#[test]
fn our_own_write_is_not_reported_back_as_an_external_change() {
    let mut app = app();
    app.handle(Event::External("edit //darksim/main/Foo.cpp".into()));

    // A move of our own; the next poll sees a workspace that has moved.
    app.handle(Event::Changed);
    app.last_request();
    app.handle(Event::External("edit //darksim/main/Foo.cpp\nedit //darksim/main/Bar.cpp".into()));

    assert!(app.notice.is_none(), "we did that ourselves");
    assert!(app.last_request().is_none());
}

#[test]
fn nothing_is_polled_while_a_question_is_open() {
    let mut app = app();
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char('c')); // raises a submit confirmation
    app.last_request();

    app.poll_external();
    assert!(
        app.last_request().is_none(),
        "the ground must not move under a decision"
    );
}

#[test]
fn an_ignore_pattern_is_the_path_below_the_workspace_root() {
    let (file, pattern) =
        ignore_entry("E:\\ws", ".p4ignore", "E:\\ws\\Content\\Big.uasset").unwrap();
    assert_eq!(file, "E:/ws/.p4ignore");
    assert_eq!(pattern, "Content/Big.uasset");

    // Windows spells the same path in several cases.
    assert_eq!(
        ignore_entry("E:\\WS", ".p4ignore", "e:\\ws\\A.txt").unwrap().1,
        "A.txt"
    );
    // A trailing separator on the root must not double up.
    assert_eq!(
        ignore_entry("E:/ws/", ".p4ignore", "E:/ws/A.txt").unwrap().0,
        "E:/ws/.p4ignore"
    );
    // P4IGNORE may name a path rather than a file.
    assert_eq!(
        ignore_entry("E:/ws", "D:/shared/ignore.txt", "E:/ws/A.txt").unwrap().0,
        "D:/shared/ignore.txt"
    );
    // Outside the workspace there is no pattern to write.
    assert!(ignore_entry("E:/ws", ".p4ignore", "C:/elsewhere/A.txt").is_none());
    assert!(ignore_entry("E:/ws", ".p4ignore", "E:/wsx/A.txt").is_none());
}

#[test]
fn i_adds_an_untracked_file_to_the_ignore_file() {
    let mut app = app();
    app.files.clear();
    app.loose_files = vec![unopened("//darksim/main/Big.uasset", FileAction::Add)];
    app.file_sel = 0;
    app.focus = Panel::Files;
    app.last_request();

    press(&mut app, KeyCode::Char('i'));

    let Some(Request::Ignore {
        file,
        pattern,
        depot_path,
    }) = app.last_request()
    else {
        panic!("expected an ignore");
    };
    // Whatever P4IGNORE names on the machine running this; the path arithmetic
    // itself is pinned by `an_ignore_pattern_is_the_path_below_the_workspace_root`.
    let name = std::env::var("P4IGNORE").unwrap_or_else(|_| ".p4ignore".to_owned());
    assert!(file.ends_with(&name), "{file}");
    assert_eq!(pattern, "Big.uasset");
    assert_eq!(depot_path, "//darksim/main/Big.uasset");
}

#[test]
fn an_ignored_file_leaves_the_list_and_the_bar_says_where_it_went() {
    let mut app = app();
    app.handle(Event::Scanned(vec![unopened(
        "//darksim/main/Big.uasset",
        FileAction::Add,
    )]));
    assert!(app
        .all_files()
        .iter()
        .any(|f| f.depot_path.ends_with("Big.uasset")));

    app.handle(Event::Ignored {
        depot_path: "//darksim/main/Big.uasset".into(),
        pattern: "Big.uasset".into(),
        file: "E:/ws/.p4ignore".into(),
    });

    assert!(
        !app.all_files()
            .iter()
            .any(|f| f.depot_path.ends_with("Big.uasset")),
        "the rule only bites on the next scan, so the row has to go now"
    );
    assert!(app
        .notice
        .as_deref()
        .is_some_and(|n| n.contains("Big.uasset") && n.contains(".p4ignore")));
}

#[test]
fn a_file_perforce_already_tracks_cannot_be_ignored() {
    let mut app = app();
    app.focus = Panel::Files;
    app.last_request();

    press(&mut app, KeyCode::Char('i'));

    assert!(app.last_request().is_none());
    assert!(app
        .error
        .as_deref()
        .is_some_and(|e| e.contains("under Perforce control")));
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
fn e_opens_the_description_of_the_selected_changelist() {
    let mut app = app();
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char('e'));

    let editor = app.editor.as_ref().expect("editor should be open");
    assert_eq!(editor.title, "Description of 395");
    assert_eq!(editor.text(), "# Do not submit");

    let out = render(&app, 120, 40);
    assert!(out.contains("Description of 395"), "{out}");
    assert!(out.contains("Enter save"), "{out}");
}

#[test]
fn the_editor_takes_keys_that_are_commands_elsewhere() {
    let mut app = app();
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char('e'));

    for c in ['q', 'x', 'r', 'j'] {
        press(&mut app, KeyCode::Char(c));
    }

    assert!(!app.quit, "q must be typeable in a description");
    assert_eq!(app.modal, Modal::None);
    assert_eq!(app.editor.as_ref().unwrap().text(), "# Do not submitqxrj");
}

#[test]
fn enter_saves_the_description() {
    let mut app = app();
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char('e'));
    press(&mut app, KeyCode::Char('!'));
    app.last_request(); // discard the setup traffic

    press(&mut app, KeyCode::Enter);

    assert!(app.editor.is_none(), "the popup closes on save");
    let Some(Request::SetDescription {
        change,
        description,
    }) = app.last_request()
    else {
        panic!("expected a description write");
    };
    assert_eq!(change, ChangeId::Number(395));
    assert_eq!(description, "# Do not submit!");
}

#[test]
fn shift_enter_writes_a_newline_since_enter_now_saves() {
    let mut app = app();
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char('e'));
    app.last_request();

    chord(&mut app, KeyCode::Enter, KeyModifiers::SHIFT);
    press(&mut app, KeyCode::Char('x'));

    let editor = app.editor.as_ref().expect("still editing, not saved");
    assert_eq!(editor.text(), "# Do not submit\nx");
    assert!(app.last_request().is_none(), "Shift-Enter must not save");
}

#[test]
fn ctrl_j_still_writes_a_newline() {
    // Fallback for terminals that report a modified Enter as a plain one.
    let mut app = app();
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char('e'));
    app.last_request();

    chord(&mut app, KeyCode::Char('j'), KeyModifiers::CONTROL);

    assert!(app.editor.is_some());
    assert_eq!(app.editor.as_ref().unwrap().text(), "# Do not submit\n");
    assert!(app.last_request().is_none());
}

#[test]
fn esc_discards_the_edit() {
    let mut app = app();
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char('e'));
    press(&mut app, KeyCode::Char('x'));
    app.last_request();

    press(&mut app, KeyCode::Esc);

    assert!(app.editor.is_none());
    assert!(app.last_request().is_none(), "nothing is written on cancel");
}

#[test]
fn an_empty_description_is_refused_before_the_round_trip() {
    let mut app = app();
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char('e'));
    // Clear the pre-filled text.
    for _ in 0.."# Do not submit".len() {
        press(&mut app, KeyCode::Backspace);
    }
    app.last_request();

    press(&mut app, KeyCode::Enter);

    assert!(app.editor.is_some(), "the popup stays open so the text is not lost");
    assert!(app.last_request().is_none());
    assert!(app.error.as_deref().is_some_and(|e| e.contains("empty")));
}

#[test]
fn the_default_and_submitted_changelists_have_no_description_to_edit() {
    let mut app = app();
    app.focus = Panel::Changelists;
    press(&mut app, KeyCode::Char('g')); // the default changelist
    press(&mut app, KeyCode::Char('e'));
    assert!(app.editor.is_none());
    assert!(app.error.as_deref().is_some_and(|e| e.contains("default")));

    app.focus = Panel::History;
    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Char('e'));
    assert!(app.editor.is_none());
    assert!(app.error.as_deref().is_some_and(|e| e.contains("submitted")));
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
fn enter_gives_the_diff_the_whole_window() {
    let mut app = app();
    app.focus = Panel::Files;
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Enter);

    assert!(app.diff_fullscreen);
    assert_eq!(app.focus, Panel::Diff, "focus follows, so j/k scroll it");

    let out = render(&app, 120, 40);
    assert!(out.contains("@@ -1,3 +1,3 @@"), "{out}");
    assert!(!out.contains("Changelists"), "the panels give up the window\n{out}");
}

#[test]
fn enter_and_esc_both_leave_fullscreen() {
    let mut app = app();
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Enter);
    assert!(!app.diff_fullscreen);

    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Esc);
    assert!(!app.diff_fullscreen);
}

#[test]
fn the_diff_shows_line_numbers_and_marks_the_changed_words() {
    let mut app = app();
    // Set directly: an Event::Diff here would be discarded as a stale answer.
    app.diffs = vec![FileDiff {
        depot_path: "//darksim/main/AGENTS.md".into(),
        rev: Some(3),
        hunks: "@@ -10,2 +10,2 @@\n keep\n-let x = f(a, b);\n+let x = f(a, c);\n".into(),
    }];

    let out = render(&app, 120, 40);
    // Both sides are numbered from the hunk header.
    assert!(out.contains("10 10"), "line numbers in the gutter\n{out}");
    assert!(out.contains("let x = f(a, b);"), "{out}");
}

#[test]
fn the_diff_scrolls_sideways_for_long_lines() {
    let mut app = app();
    app.focus = Panel::Diff;
    assert_eq!(app.diff_hscroll, 0);

    press(&mut app, KeyCode::Char('l'));
    assert_eq!(app.diff_hscroll, 8);
    press(&mut app, KeyCode::Char('h'));
    assert_eq!(app.diff_hscroll, 0);
    press(&mut app, KeyCode::Char('h'));
    assert_eq!(app.diff_hscroll, 0, "and stops at the left edge");
}

#[test]
fn changing_file_resets_the_sideways_scroll_too() {
    let mut app = app();
    app.focus = Panel::Diff;
    press(&mut app, KeyCode::Char('l'));

    app.focus = Panel::Files;
    press(&mut app, KeyCode::Char('j'));
    assert_eq!(app.diff_hscroll, 0);
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
    assert!(render(&app, 120, 40).contains("switch tab"));

    // `q` closes the overlay rather than the application.
    press(&mut app, KeyCode::Char('q'));
    assert_eq!(app.modal, Modal::None);
    assert!(!app.quit);

    press(&mut app, KeyCode::Char('q'));
    assert!(app.quit);
}

#[test]
fn the_command_log_lives_behind_the_help_sheet() {
    let mut app = app();
    app.handle(Event::Log("changes -l -s pending".into()));

    // No longer a key of its own.
    press(&mut app, KeyCode::Char('x'));
    assert_eq!(app.modal, Modal::None);

    press(&mut app, KeyCode::Char('?'));
    assert!(render(&app, 120, 40).contains("the p4 command log"), "help points at it");

    press(&mut app, KeyCode::Char('x'));
    assert_eq!(app.modal, Modal::Log);
    assert!(render(&app, 120, 40).contains("changes -l -s pending"));

    // And back, rather than dumping you out of help entirely.
    press(&mut app, KeyCode::Char('x'));
    assert_eq!(app.modal, Modal::Help);
}

#[test]
fn the_help_sheet_groups_keys_by_where_they_apply() {
    let mut app = app();
    press(&mut app, KeyCode::Char('?'));
    let out = render(&app, 120, 40);

    for heading in ["Navigation", "Files", "Changelists", "Diff and app"] {
        assert!(out.contains(heading), "missing {heading}\n{out}");
    }
    assert!(out.contains("revert, discarding"), "{out}");
}

#[test]
fn the_status_bar_shows_the_focused_panel_s_keys() {
    let mut app = app();

    app.focus = Panel::Files;
    let out = render(&app, 120, 40);
    assert!(out.contains("revert"), "{out}");
    assert!(out.contains("scan"), "{out}");

    app.focus = Panel::Changelists;
    let out = render(&app, 120, 40);
    assert!(out.contains("submit"), "{out}");
    assert!(out.contains("describe"), "{out}");
    assert!(!out.contains("scan"), "Files keys are gone\n{out}");

    app.focus = Panel::History;
    assert!(render(&app, 120, 40).contains("undo"));

    // The everywhere keys stay put.
    assert!(render(&app, 120, 40).contains("quit"));
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
    let mut zoomed = nested();
    zoomed.focus = Panel::Files;
    press(&mut zoomed, KeyCode::Char('+'));
    println!("{}\n", render(&zoomed, 92, 22));

    let mut filtering = nested();
    filtering.focus = Panel::Files;
    type_filter(&mut filtering, "door");
    println!("{}\n", render(&filtering, 92, 16));

    let mut helping = app();
    helping.focus = Panel::Files;
    press(&mut helping, KeyCode::Char('?'));
    println!("{}\n", render(&helping, 100, 24));

    let mut submitting = app();
    press(&mut submitting, KeyCode::Char('c'));
    println!("{}\n", render(&submitting, 92, 12));

    // Real tab-indented content, as captured from Darksim.Build.cs.
    let mut tabs = app();
    tabs.diffs = vec![FileDiff {
        depot_path: "//darksim/main/AGENTS.md".into(),
        rev: Some(3),
        hunks: concat!(
            "@@ -7,8 +8,10 @@\n",
            " \tpublic Darksim(ReadOnlyTargetRules Target) : base(Target)\n",
            " \t{\n",
            " \t\tPCHUsage = PCHUsageMode.UseExplicitOrSharedPCHs;\n",
            "-\t\n",
            "+\n",
            " \t\tPublicDependencyModuleNames.Add(\"ImGui\");\n",
        )
        .to_owned(),
    }];
    tabs.diff_fullscreen = true;
    println!("{}\n", render(&tabs, 92, 12));

    let mut app = nested();
    app.loose_files = vec![
        file("//darksim/main/Config/DefaultEngine.ini", FileAction::Edit, false),
        unopened("//darksim/main/Source/Darksim/New.cpp", FileAction::Add),
    ];
    app.focus = Panel::Files;
    app.handle(Event::Scanned(vec![
        unopened("//darksim/main/NewThing.cpp", FileAction::Add),
        unopened("//darksim/main/Changed.cpp", FileAction::Edit),
    ]));
    app.diffs = vec![FileDiff {
        depot_path: "//darksim/main/AGENTS.md".into(),
        rev: Some(3),
        hunks: "@@ -38,7 +38,8 @@\n Intermediate/\n Saved/\n \n-let x = compute(alpha, beta);\n+let x = compute(alpha, gamma);\n+Binaries/\n \n # Ignore UBT\n"
            .into(),
    }];
    println!("{}", render(&app, 100, 40));
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
