//! Rendering. Layout only — every decision lives in [`crate::app`].

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use p4::{Changelist, FileAction};

use crate::app::{change_marker, App, ChangeTab, Confirm, Destination, FileRow, Modal, Panel, Picker};
use crate::diffview::{self, Row, RowKind};
use crate::editor::Editor;
use crate::worker::FileEntry;

const FOCUS: Color = Color::Yellow;
const IDLE: Color = Color::DarkGray;

pub fn draw(frame: &mut Frame, app: &App) {
    let [body, status_bar] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    // A wide diff is worth the whole window; the panels are one key away.
    if app.diff_fullscreen {
        draw_diff(frame, app, body);
        draw_status_bar(frame, app, status_bar);
        if let Some(editor) = &app.editor {
            draw_editor(frame, editor);
        }
        return;
    }

    let [left, right] =
        Layout::horizontal([Constraint::Percentage(42), Constraint::Min(0)]).areas(body);
    // Status is fixed; the three lists share what is left.
    let [status, files, changes, history] = Layout::vertical([
        Constraint::Length(6),
        Constraint::Fill(1),
        Constraint::Fill(1),
        Constraint::Fill(1),
    ])
    .areas(left);

    draw_status(frame, app, status);
    draw_files(frame, app, files);
    draw_changes(frame, app, changes);
    draw_history(frame, app, history);
    draw_diff(frame, app, right);
    draw_status_bar(frame, app, status_bar);

    match app.modal {
        Modal::None => {}
        Modal::Help => draw_help(frame),
        Modal::Log => draw_log(frame, app),
        Modal::History => draw_file_history(frame, app),
    }

    // Drawn last so they sit above any overlay.
    if let Some(picker) = &app.picker {
        draw_picker(frame, picker);
    }
    if let Some(editor) = &app.editor {
        draw_editor(frame, editor);
    }
    if let Some(confirm) = &app.confirm {
        draw_confirm(frame, confirm);
    }
}

fn draw_file_history(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let height = area.height.saturating_sub(6).max(6);
    let width = area.width.saturating_sub(8);

    let lines: Vec<Line> = if app.history.is_empty() {
        vec![Line::from(Span::styled(
            "  loading…",
            Style::default().fg(IDLE),
        ))]
    } else {
        app.history
            .iter()
            .skip(app.history_scroll)
            .map(|r| {
                Line::from(vec![
                    Span::styled(
                        format!(" #{:<4}", r.rev),
                        Style::default().fg(Color::Green),
                    ),
                    Span::styled(
                        format!("{:>8} ", r.change),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled(
                        format!("{:<10} ", r.time.map(p4::civil_date).unwrap_or_default()),
                        Style::default().fg(IDLE),
                    ),
                    Span::styled(
                        format!("{:<10.10} ", r.user),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("{:<8.8} ", r.action.to_string()),
                        Style::default().fg(action_color(&r.action)),
                    ),
                    Span::raw(r.description.lines().next().unwrap_or_default().to_owned()),
                ])
            })
            .collect()
    };

    overlay(
        frame,
        &format!(" History of {} ", short_path(&app.history_path)),
        lines,
        width,
        height,
    );
}

fn draw_confirm(frame: &mut Frame, confirm: &Confirm) {
    // Long enough to read, but capped: a confirmation nobody can take in is
    // not a confirmation.
    const MOST: usize = 12;
    let shown = confirm.lines.len().min(MOST);
    let mut lines: Vec<Line> = confirm.lines[..shown]
        .iter()
        .map(|text| Line::from(Span::raw(format!("  {text}"))))
        .collect();
    if confirm.lines.len() > shown {
        lines.push(Line::from(Span::styled(
            format!("  … and {} more", confirm.lines.len() - shown),
            Style::default().fg(IDLE),
        )));
    }

    let width = frame.area().width.saturating_sub(10).min(76).max(30);
    let height = (lines.len() as u16 + 3).min(frame.area().height);
    let area = centered(frame.area(), width, height);

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_style(Style::default().fg(Color::Red))
                .title(Span::styled(
                    format!(" {} ", confirm.title),
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ))
                // Naming the one key that proceeds, rather than offering a
                // default that could be taken by a stray Enter.
                .title_bottom(Span::styled(
                    " y to confirm   any other key cancels ",
                    Style::default().fg(IDLE),
                )),
        ),
        area,
    );
}

fn draw_picker(frame: &mut Frame, picker: &Picker) {
    let width = frame.area().width.saturating_sub(10).min(64).max(30);
    let height = (picker.options.len() as u16 + 3).min(frame.area().height);
    let area = centered(frame.area(), width, height);

    frame.render_widget(Clear, area);
    let files = picker.files.len();
    let block = Block::bordered()
        .border_style(Style::default().fg(FOCUS))
        .title(Span::styled(
            format!(
                " Move {files} file{} to ",
                if files == 1 { "" } else { "s" }
            ),
            Style::default().fg(FOCUS).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            " Enter choose   Esc cancel ",
            Style::default().fg(IDLE),
        ));

    let items: Vec<ListItem> = picker
        .options
        .iter()
        .map(|option| match option {
            Destination::Existing(id, summary) => ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:>8} ", id.to_string()),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(summary.clone()),
            ])),
            Destination::New => ListItem::new(Line::from(Span::styled(
                "     new  create a changelist…",
                Style::default().fg(Color::Green),
            ))),
        })
        .collect();

    let mut state = ListState::default().with_selected(Some(picker.sel));
    frame.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_style(selection_style(true)),
        area,
        &mut state,
    );
}

fn draw_editor(frame: &mut Frame, editor: &Editor) {
    let width = frame.area().width.saturating_sub(10).min(80).max(20);
    // Room for the text, the border, and the key hint.
    let height = (editor.lines().len() as u16 + 3).min(frame.area().height);
    let area = centered(frame.area(), width, height);

    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_style(Style::default().fg(FOCUS))
        .title(Span::styled(
            format!(" {} ", editor.title),
            Style::default().fg(FOCUS).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            " Enter save   Shift-Enter newline   Esc cancel ",
            Style::default().fg(IDLE),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines: Vec<Line> = editor.lines().iter().map(|l| Line::raw(l.clone())).collect();
    frame.render_widget(Paragraph::new(lines), inner);

    let (row, col) = editor.cursor();
    // Only place the cursor where there is room to draw it.
    if (row as u16) < inner.height && (col as u16) <= inner.width {
        frame.set_cursor_position((inner.x + col as u16, inner.y + row as u16));
    }
}

fn panel_block(app: &App, panel: Panel, extra: Option<String>) -> Block<'static> {
    let focused = app.focus == panel;
    let name = match extra {
        Some(e) => format!("{} {e} ", panel.title()),
        None => format!("{} ", panel.title()),
    };
    Block::bordered()
        .border_style(Style::default().fg(if focused { FOCUS } else { IDLE }))
        // The number is the key that focuses this panel; the title is the only
        // place a reader can discover that.
        .title(Line::from(vec![
            Span::styled(
                format!(" {} ", panel.number()),
                Style::default().fg(Color::Black).bg(if focused {
                    FOCUS
                } else {
                    Color::Gray
                }),
            ),
            Span::styled(
                format!(" {name}"),
                Style::default()
                    .fg(if focused { FOCUS } else { Color::Gray })
                    .add_modifier(Modifier::BOLD),
            ),
        ]))
}

fn change_item(cl: &Changelist, my_client: &str) -> ListItem<'static> {
    let marker = change_marker(cl);
    let mut spans = vec![
        Span::styled(
            marker.to_string(),
            Style::default().fg(match marker {
                '✓' => Color::Green,
                '⌸' => Color::Magenta,
                _ => Color::Blue,
            }),
        ),
        Span::raw(" "),
        Span::styled(
            format!("{:>7}", cl.id.to_string()),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw(" "),
        Span::styled(
            format!("{:<10.10}", cl.user),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(" "),
        Span::raw(cl.summary().to_owned()),
    ];

    // Ownership is by user, so one of our own changelists can be sitting on a
    // different workspace. Say so, rather than implying it is checked out here.
    if !my_client.is_empty() && !cl.client.is_empty() && cl.client != my_client {
        spans.push(Span::styled(
            format!(" @{}", cl.client),
            Style::default().fg(Color::Yellow),
        ));
    }
    ListItem::new(Line::from(spans))
}

fn draw_changes(frame: &mut Frame, app: &App, area: Rect) {
    let block = panel_block(app, Panel::Changelists, None);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // One row of tabs, then the list below it.
    let [tabs, list] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
    frame.render_widget(Paragraph::new(tab_bar(app)), tabs);

    let changes = app.tab_changes();
    if changes.is_empty() {
        frame.render_widget(
            Paragraph::new("  nothing here").style(Style::default().fg(IDLE)),
            list,
        );
        return;
    }

    let items: Vec<ListItem> = changes
        .iter()
        .map(|cl| change_item(cl, app.my_client()))
        .collect();
    let mut state = ListState::default().with_selected(Some(app.change_sel));
    frame.render_stateful_widget(
        List::new(items).highlight_style(selection_style(app.focus == Panel::Changelists)),
        list,
        &mut state,
    );
}

/// `Local 2 │ Shelved 2 │ Others 1`, with the open tab highlighted.
fn tab_bar(app: &App) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, tab) in ChangeTab::ORDER.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", Style::default().fg(IDLE)));
        } else {
            spans.push(Span::raw(" "));
        }
        let open = *tab == app.tab;
        spans.push(Span::styled(
            format!("{} {}", tab.title(), app.tab_count(*tab)),
            if open {
                Style::default().fg(FOCUS).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(IDLE)
            },
        ));
    }
    Line::from(spans)
}

fn draw_history(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .submitted
        .iter()
        .map(|cl| change_item(cl, app.my_client()))
        .collect();
    let count = format!("({})", app.submitted.len());
    let mut state = ListState::default().with_selected(Some(app.history_sel));
    frame.render_stateful_widget(
        List::new(items)
            .block(panel_block(app, Panel::History, Some(count)))
            .highlight_style(selection_style(app.focus == Panel::History)),
        area,
        &mut state,
    );
}

fn draw_files(frame: &mut Frame, app: &App, area: Rect) {
    let block = panel_block(
        app,
        Panel::Files,
        app.selected_change().map(|cl| format!("of {}", cl.id)),
    );

    let rows = app.file_rows();
    if rows.is_empty() {
        let msg = if app.files_for.is_none() {
            "loading…".to_owned()
        } else if app.scanning {
            "scanning the workspace…".to_owned()
        } else if app.scanned.is_empty() {
            "no open files — press u to scan for untracked ones".to_owned()
        } else {
            "no files visible from this workspace".to_owned()
        };
        frame.render_widget(
            Paragraph::new(msg)
                .style(Style::default().fg(IDLE))
                .block(block),
            area,
        );
        return;
    }

    // The cursor counts only selectable rows; the list also holds group
    // headers, so the two indexes have to be reconciled here.
    let mut selected_row = None;
    let mut selectable = 0usize;
    let items: Vec<ListItem> = rows
        .iter()
        .enumerate()
        .map(|(row, entry)| match entry {
            FileRow::Header(text) => ListItem::new(Line::from(Span::styled(
                format!(" {text}"),
                Style::default().fg(IDLE).add_modifier(Modifier::BOLD),
            ))),
            FileRow::Dir {
                label,
                depth,
                collapsed,
                files,
                ..
            } => {
                if selectable == app.file_sel {
                    selected_row = Some(row);
                }
                selectable += 1;
                dir_item(label, *depth, *collapsed, *files)
            }
            FileRow::File {
                entry, label, depth, ..
            } => {
                if selectable == app.file_sel {
                    selected_row = Some(row);
                }
                selectable += 1;
                file_item(entry, label, *depth)
            }
        })
        .collect();

    let mut state = ListState::default().with_selected(selected_row);
    frame.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_style(selection_style(app.focus == Panel::Files)),
        area,
        &mut state,
    );
}

/// Every row is laid out the same way: a three-column gutter holding the file's
/// action mark, the depth indent, then a two-column slot for the fold arrow —
/// blank on a file. That slot is what puts a directory's contents one level to
/// the right of its name.
fn indent(depth: usize) -> String {
    "  ".repeat(depth)
}

fn dir_item(label: &str, depth: usize, collapsed: bool, files: usize) -> ListItem<'static> {
    ListItem::new(Line::from(vec![
        Span::raw("   "),
        Span::raw(indent(depth)),
        Span::styled(
            if collapsed { "▸ " } else { "▾ " }.to_owned(),
            Style::default().fg(IDLE),
        ),
        Span::styled(
            label.to_owned(),
            Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {files}"), Style::default().fg(IDLE)),
    ]))
}

fn file_item(f: &FileEntry, label: &str, depth: usize) -> ListItem<'static> {
    // `??` for a file Perforce has never seen, mirroring git's untracked mark.
    let (code, color) = if f.untracked() {
        ("??".to_owned(), Color::Magenta)
    } else {
        (f.action.code().to_string(), action_color(&f.action))
    };

    ListItem::new(Line::from(vec![
        Span::styled(
            format!("{code:<2}"),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::raw(indent(depth)),
        // The slot a directory row uses for its arrow.
        Span::raw("  "),
        Span::styled(
            label.to_owned(),
            // A file that is not open is not part of any changelist yet.
            if f.opened {
                Style::default()
            } else {
                Style::default().fg(Color::Gray)
            },
        ),
        if f.unresolved {
            Span::styled(" (unresolved)", Style::default().fg(Color::Red))
        } else {
            Span::raw("")
        },
    ]))
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    let mut lines = Vec::new();
    match &app.info {
        None => lines.push(Line::from(Span::styled(
            "connecting…",
            Style::default().fg(IDLE),
        ))),
        Some(info) => {
            lines.push(field("user", &info.user));
            if info.client_known {
                lines.push(field("client", &info.client));
                if let Some(stream) = &info.stream {
                    lines.push(field("stream", stream));
                }
                lines.push(field("server", &info.server_address));
            } else {
                // Without a client there are no open files and no workspace
                // diffs, so this needs to explain itself rather than sit there.
                lines.push(Line::from(vec![
                    Span::styled("client  ", Style::default().fg(IDLE)),
                    Span::styled("not set", Style::default().fg(Color::Red)),
                ]));
                lines.push(Line::from(Span::styled(
                    "start lazyp4 in a workspace, or set P4CLIENT",
                    Style::default().fg(Color::Red),
                )));
                lines.push(field("server", &info.server_address));
            }
        }
    }

    frame.render_widget(
        Paragraph::new(lines).block(panel_block(app, Panel::Status, None)),
        area,
    );
}

fn draw_diff(frame: &mut Frame, app: &App, area: Rect) {
    let Some(file) = app.selected_file() else {
        frame.render_widget(
            Paragraph::new("select a file")
                .style(Style::default().fg(IDLE))
                .block(panel_block(app, Panel::Diff, None)),
            area,
        );
        return;
    };

    let title = match file.rev {
        Some(r) => format!("{}#{r}", short_path(&file.depot_path)),
        None => short_path(&file.depot_path).to_owned(),
    };
    let block = panel_block(app, Panel::Diff, Some(title));

    let Some(diff) = app.selected_diff() else {
        let msg = if app.diffs_for.is_none() {
            "loading…"
        } else {
            "no diff for this file"
        };
        frame.render_widget(
            Paragraph::new(msg)
                .style(Style::default().fg(IDLE))
                .block(block),
            area,
        );
        return;
    };

    let rows = diffview::rows(&diff.hunks);
    let width = gutter_width(&rows);
    let lines: Vec<Line> = rows
        .iter()
        .map(|r| diff_line(r, width, app.diff_hscroll))
        .collect();

    // Keep the last screenful reachable but never scroll past it.
    let visible = area.height.saturating_sub(2) as usize;
    let max = lines.len().saturating_sub(visible);
    let offset = app.diff_scroll.min(max);

    frame.render_widget(
        Paragraph::new(lines).block(block).scroll((offset as u16, 0)),
        area,
    );
}

/// Digits needed for the line numbers, so both columns line up.
fn gutter_width(rows: &[Row]) -> usize {
    let widest = rows
        .iter()
        .filter_map(|r| r.old_no.max(r.new_no))
        .max()
        .unwrap_or(0);
    widest.to_string().len().max(2)
}

/// One diff row: line numbers, marker, then the text with the words that
/// changed picked out.
fn diff_line(row: &Row, width: usize, hscroll: usize) -> Line<'static> {
    if row.kind == RowKind::Header {
        return Line::from(Span::styled(
            row.text(),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));
    }

    let number = |n: Option<u32>| match n {
        Some(n) => format!("{n:>width$}"),
        None => " ".repeat(width),
    };
    let mut spans = vec![Span::styled(
        format!("{} {} ", number(row.old_no), number(row.new_no)),
        Style::default().fg(IDLE),
    )];

    let (fg, changed_bg) = match row.kind {
        RowKind::Add => (Color::Green, Color::Green),
        RowKind::Delete => (Color::Red, Color::Red),
        _ => (Color::Gray, Color::Gray),
    };
    spans.push(Span::styled(
        row.marker().to_string(),
        Style::default().fg(fg).add_modifier(Modifier::BOLD),
    ));

    let base = match row.kind {
        RowKind::Context => Style::default(),
        RowKind::Note => Style::default().fg(IDLE),
        _ => Style::default().fg(fg),
    };

    // Horizontal scroll applies to the text, never to the gutter.
    let mut skip = hscroll;
    for segment in &row.segments {
        let chars = segment.text.chars().count();
        if skip >= chars {
            skip -= chars;
            continue;
        }
        let text: String = segment.text.chars().skip(skip).collect();
        skip = 0;
        spans.push(Span::styled(
            text,
            if segment.changed {
                // Reversed rather than merely brighter, so the changed words
                // stand out even where the whole line is already coloured.
                Style::default().bg(changed_bg).fg(Color::Black)
            } else {
                base
            },
        ));
    }

    Line::from(spans)
}

/// Depot paths are long and share a prefix; the tail is what identifies them.
fn short_path(depot_path: &str) -> &str {
    depot_path.trim_start_matches('/')
}

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let line = match (&app.error, app.busy) {
        (Some(err), _) => Line::from(Span::styled(
            format!(" {} ", err.replace('\n', " ")),
            Style::default().fg(Color::White).bg(Color::Red),
        )),
        (None, true) => Line::from(Span::styled(
            " working… ",
            Style::default().fg(Color::Black).bg(FOCUS),
        )),
        (None, false) => Line::from(vec![
            Span::styled(" ? ", Style::default().fg(Color::Black).bg(Color::Gray)),
            Span::styled(" help   ", Style::default().fg(IDLE)),
            Span::styled(" x ", Style::default().fg(Color::Black).bg(Color::Gray)),
            Span::styled(" log   ", Style::default().fg(IDLE)),
            Span::styled(" r ", Style::default().fg(Color::Black).bg(Color::Gray)),
            Span::styled(" refresh   ", Style::default().fg(IDLE)),
            Span::styled(" q ", Style::default().fg(Color::Black).bg(Color::Gray)),
            Span::styled(" quit", Style::default().fg(IDLE)),
        ]),
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_help(frame: &mut Frame) {
    let row = |keys: String, what: String| {
        Line::from(vec![
            Span::styled(format!("  {keys:<14}"), Style::default().fg(FOCUS)),
            Span::raw(what),
        ])
    };

    let mut lines = vec![
        row("j / k, ↓ / ↑".into(), "move".into()),
        row("g / G".into(), "first / last".into()),
        row("Tab / Shift-Tab".into(), "cycle panels".into()),
        row("[ ]".into(), "switch tab within a panel".into()),
        row("Enter".into(), "diff fullscreen (Esc to leave)".into()),
        row(
            "h / l, ← / →".into(),
            "fold a directory, or scroll the diff".into(),
        ),
        row("Space".into(), "move a file in or out of the changelist".into()),
        row("u".into(), "scan for untracked files (slow)".into()),
        row("e".into(), "edit the changelist description".into()),
        row("n".into(), "new changelist".into()),
        row("c".into(), "submit the changelist".into()),
        row("d".into(), "revert files, or delete an empty changelist".into()),
        Line::raw(""),
    ];
    lines.extend(
        Panel::ORDER
            .iter()
            .map(|p| row(p.number().to_string(), format!("focus {}", p.title()))),
    );
    lines.push(Line::raw(""));
    lines.extend([
        row("r".into(), "refresh".into()),
        row("x".into(), "command log".into()),
        row("H".into(), "history of the selected file".into()),
        row("?".into(), "this help".into()),
        row("q".into(), "quit".into()),
    ]);

    let height = lines.len() as u16 + 2;
    overlay(frame, " Keys ", lines, 44, height);
}

fn draw_log(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let height = (area.height as usize).saturating_sub(8).max(4);
    let lines: Vec<Line> = app
        .log
        .iter()
        .rev()
        .take(height)
        .rev()
        .map(|cmd| {
            Line::from(vec![
                Span::styled("  p4 ", Style::default().fg(IDLE)),
                Span::raw(cmd.clone()),
            ])
        })
        .collect();

    overlay(
        frame,
        " Commands ",
        lines,
        area.width.saturating_sub(8),
        height as u16 + 2,
    );
}

fn overlay(frame: &mut Frame, title: &str, lines: Vec<Line>, width: u16, height: u16) {
    let area = centered(frame.area(), width, height);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_style(Style::default().fg(FOCUS))
                .title(Span::styled(
                    title.to_owned(),
                    Style::default().fg(FOCUS).add_modifier(Modifier::BOLD),
                )),
        ),
        area,
    );
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

fn field<'a>(name: &'a str, value: &str) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("{name:<8}"), Style::default().fg(IDLE)),
        Span::raw(value.to_owned()),
    ])
}

fn selection_style(focused: bool) -> Style {
    let base = Style::default().add_modifier(Modifier::BOLD);
    if focused {
        base.bg(Color::DarkGray).fg(Color::White)
    } else {
        base
    }
}

fn action_color(action: &FileAction) -> Color {
    match action.code() {
        'A' => Color::Green,
        'M' => Color::Yellow,
        'D' => Color::Red,
        'B' | 'I' => Color::Cyan,
        _ => Color::Gray,
    }
}
