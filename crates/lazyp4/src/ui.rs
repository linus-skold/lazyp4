//! Rendering. Layout only — every decision lives in [`crate::app`].

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use p4::{ChangeStatus, FileAction};

use crate::app::{App, Modal, Panel};

const FOCUS: Color = Color::Yellow;
const IDLE: Color = Color::DarkGray;

pub fn draw(frame: &mut Frame, app: &App) {
    let [body, status_bar] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(42), Constraint::Min(0)]).areas(body);
    let [changes, files, status] = Layout::vertical([
        Constraint::Percentage(55),
        Constraint::Min(5),
        Constraint::Length(7),
    ])
    .areas(left);

    draw_changes(frame, app, changes);
    draw_files(frame, app, files);
    draw_status(frame, app, status);
    draw_diff(frame, app, right);
    draw_status_bar(frame, app, status_bar);

    match app.modal {
        Modal::None => {}
        Modal::Help => draw_help(frame),
        Modal::Log => draw_log(frame, app),
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

fn draw_changes(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .changes
        .iter()
        .map(|cl| {
            let marker = match cl.status {
                ChangeStatus::Submitted => Span::styled("✓", Style::default().fg(Color::Green)),
                _ if cl.shelved => Span::styled("⌸", Style::default().fg(Color::Magenta)),
                _ => Span::styled("▸", Style::default().fg(Color::Blue)),
            };
            ListItem::new(Line::from(vec![
                marker,
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
            ]))
        })
        .collect();

    let count = format!("({})", app.changes.len());
    let mut state = ListState::default().with_selected(Some(app.change_sel));
    frame.render_stateful_widget(
        List::new(items)
            .block(panel_block(app, Panel::Changelists, Some(count)))
            .highlight_style(selection_style(app.focus == Panel::Changelists)),
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

    if app.files.is_empty() {
        let msg = if app.files_for.is_none() {
            "loading…"
        } else {
            "no files visible from this workspace"
        };
        frame.render_widget(
            Paragraph::new(msg).style(Style::default().fg(IDLE)).block(block),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = app
        .files
        .iter()
        .map(|f| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    f.action.code().to_string(),
                    Style::default()
                        .fg(action_color(&f.action))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::raw(f.depot_path.clone()),
                if f.unresolved {
                    Span::styled(" (unresolved)", Style::default().fg(Color::Red))
                } else {
                    Span::raw("")
                },
            ]))
        })
        .collect();

    let mut state = ListState::default().with_selected(Some(app.file_sel));
    frame.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_style(selection_style(app.focus == Panel::Files)),
        area,
        &mut state,
    );
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
            lines.push(if info.client_known {
                field("client", &info.client)
            } else {
                Line::from(vec![
                    Span::styled("client  ", Style::default().fg(IDLE)),
                    Span::styled("not set", Style::default().fg(Color::Red)),
                ])
            });
            if let Some(stream) = &info.stream {
                lines.push(field("stream", stream));
            }
            lines.push(field("server", &info.server_address));
        }
    }

    frame.render_widget(
        Paragraph::new(lines).block(panel_block(app, Panel::Status, None)),
        area,
    );
}

fn draw_diff(frame: &mut Frame, app: &App, area: Rect) {
    let block = panel_block(app, Panel::Diff, None);

    let body = match app.selected_file() {
        Some(f) => vec![
            Line::from(Span::styled(
                f.depot_path.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                match f.rev {
                    Some(r) => format!("#{r}  {}", f.action),
                    None => f.action.to_string(),
                },
                Style::default().fg(IDLE),
            )),
            Line::raw(""),
            Line::from(Span::styled(
                "diffs arrive with the hunk integration",
                Style::default().fg(IDLE),
            )),
        ],
        None => vec![Line::from(Span::styled(
            "select a file",
            Style::default().fg(IDLE),
        ))],
    };

    frame.render_widget(Paragraph::new(body).block(block).wrap(Wrap { trim: false }), area);
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
        row("Tab, [ ]".into(), "cycle panel".into()),
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
