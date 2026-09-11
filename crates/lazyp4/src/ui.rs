//! Rendering. Layout only — every decision lives in [`crate::app`].

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use p4::{Changelist, FileAction};

use crate::app::{change_marker, App, ChangeTab, Confirm, Destination, FileRow, Modal, Panel, Picker};
use crate::config::{Action, Group, Keymap, Theme};
use crate::diffview::{self, Row, RowKind};
use crate::editor::Editor;
use crate::worker::FileEntry;

/// Braille frames, which turn in place rather than shifting the text after them.
const SPINNER: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];

pub fn draw(frame: &mut Frame, app: &App) {
    let t = &app.config.theme;
    let [body, status_bar] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    // A wide diff is worth the whole window; the panels are one key away.
    if app.diff_fullscreen {
        draw_diff(frame, app, body);
        draw_status_bar(frame, app, status_bar);
        if let Some(editor) = &app.editor {
            draw_editor(frame, t, editor);
        }
        return;
    }

    let [left, right] =
        Layout::horizontal([Constraint::Percentage(42), Constraint::Min(0)]).areas(body);
    // Status is fixed; the three lists share what is left. Zooming gives the
    // focused one nearly all of it rather than hiding the others outright, so
    // the column still reads as a column.
    let weight = |panel: Panel| -> Constraint {
        match (app.zoom, app.focus == panel) {
            (true, true) => Constraint::Fill(12),
            (true, false) => Constraint::Length(3),
            _ => Constraint::Fill(1),
        }
    };
    let [status, files, changes, history] = Layout::vertical([
        Constraint::Length(6),
        weight(Panel::Files),
        weight(Panel::Changelists),
        weight(Panel::History),
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
        Modal::Help => draw_help(frame, t, &app.config.keys),
        Modal::Log => draw_log(frame, app),
        Modal::History => draw_file_history(frame, app),
        Modal::Blame => draw_blame(frame, app),
        Modal::Resolve => draw_resolve(frame, app),
        Modal::Streams => draw_streams(frame, app),
        Modal::Workspaces => draw_workspaces(frame, app),
    }

    // Drawn last so they sit above any overlay.
    if let Some(picker) = &app.picker {
        draw_picker(frame, t, picker);
    }
    if let Some(editor) = &app.editor {
        draw_editor(frame, t, editor);
    }
    if let Some(confirm) = &app.confirm {
        draw_confirm(frame, t, confirm);
    }
}

fn draw_workspaces(frame: &mut Frame, app: &App) {
    let t = &app.config.theme;
    let current = app.my_client();
    let items: Vec<ListItem> = app
        .workspaces
        .iter()
        .map(|w| {
            let here = w.name == current;
            // The stream says what the workspace is for; a classic client has
            // none, so its root is the only thing that tells it apart.
            let about = match &w.stream {
                Some(stream) => stream.clone(),
                None => w.root.clone(),
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    if here { " ▸ " } else { "   " }.to_owned(),
                    Style::default().fg(t.ok),
                ),
                Span::styled(
                    format!("{:<28.28} ", w.name),
                    if here {
                        Style::default().fg(t.ok).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    },
                ),
                Span::styled(about, Style::default().fg(t.idle)),
            ]))
        })
        .collect();

    let area = frame.area();
    let width = area.width.saturating_sub(8).min(80);
    let height = (items.len().max(1) as u16 + 3).min(area.height.saturating_sub(2));
    let area = centered(area, width, height);

    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_style(Style::default().fg(t.focus))
        .title(Span::styled(
            " Workspaces ",
            Style::default().fg(t.focus).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            format!(" Enter switch   {} close ", key_of(app, Action::Workspaces)),
            Style::default().fg(t.idle),
        ));

    if app.workspaces.is_empty() {
        frame.render_widget(
            Paragraph::new(if app.busy { "  loading…" } else { "  no workspaces" })
                .style(Style::default().fg(t.idle))
                .block(block),
            area,
        );
        return;
    }

    let mut state = ListState::default().with_selected(Some(app.workspaces_sel));
    frame.render_stateful_widget(
        List::new(items).block(block).highlight_style(selection_style(t, true)),
        area,
        &mut state,
    );
}

fn draw_streams(frame: &mut Frame, app: &App) {
    let t = &app.config.theme;
    let current = app.current_stream();
    let items: Vec<ListItem> = app
        .streams
        .iter()
        .map(|s| {
            let here = s.path == current;
            ListItem::new(Line::from(vec![
                Span::styled(
                    if here { " ▸ " } else { "   " }.to_owned(),
                    Style::default().fg(t.ok),
                ),
                Span::styled(
                    format!("{:<28.28} ", s.path),
                    if here {
                        Style::default().fg(t.ok).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    },
                ),
                Span::styled(format!("{:<12.12} ", s.kind), Style::default().fg(t.idle)),
                Span::styled(s.parent.clone(), Style::default().fg(t.idle)),
            ]))
        })
        .collect();

    let area = frame.area();
    let width = area.width.saturating_sub(8).min(80);
    let height = (items.len().max(1) as u16 + 3).min(area.height.saturating_sub(2));
    let area = centered(area, width, height);

    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_style(Style::default().fg(t.focus))
        .title(Span::styled(
            " Streams ",
            Style::default().fg(t.focus).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            format!(" Enter switch   {} close ", key_of(app, Action::Streams)),
            Style::default().fg(t.idle),
        ));

    if app.streams.is_empty() {
        frame.render_widget(
            Paragraph::new(if app.busy { "  loading…" } else { "  no streams" })
                .style(Style::default().fg(t.idle))
                .block(block),
            area,
        );
        return;
    }

    let mut state = ListState::default().with_selected(Some(app.streams_sel));
    frame.render_stateful_widget(
        List::new(items).block(block).highlight_style(selection_style(t, true)),
        area,
        &mut state,
    );
}

fn draw_resolve(frame: &mut Frame, app: &App) {
    let t = &app.config.theme;
    let root = app.depot_root();
    let items: Vec<ListItem> = app
        .unresolved
        .iter()
        .map(|u| {
            let revs = match (u.start_rev, u.end_rev) {
                (Some(a), Some(b)) => format!("#{a},#{b}"),
                _ => String::new(),
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {:<9.9} ", u.content_type),
                    Style::default().fg(t.untracked),
                ),
                Span::styled(format!("{revs:<9} "), Style::default().fg(t.idle)),
                Span::raw(short_path(&crate::tree::relative(&u.from_path, &root)).to_owned()),
            ]))
        })
        .collect();

    let area = frame.area();
    let width = area.width.saturating_sub(8);
    let height = (items.len().max(1) as u16 + 3).min(area.height.saturating_sub(2));
    let area = centered(area, width, height);

    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_style(Style::default().fg(t.danger))
        .title(Span::styled(
            format!(" {} file(s) to resolve ", app.unresolved.len()),
            Style::default().fg(t.danger).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            format!(" y yours   t theirs   m merge   a safe   {} close ", key_of(app, Action::Resolve)),
            Style::default().fg(t.idle),
        ));

    if app.unresolved.is_empty() {
        let message = if app.busy {
            "checking…"
        } else {
            "nothing to resolve"
        };
        frame.render_widget(
            Paragraph::new(format!("  {message}"))
                .style(Style::default().fg(t.idle))
                .block(block),
            area,
        );
        return;
    }

    let mut state = ListState::default().with_selected(Some(app.unresolved_sel));
    frame.render_stateful_widget(
        List::new(items).block(block).highlight_style(selection_style(t, true)),
        area,
        &mut state,
    );
}

fn draw_blame(frame: &mut Frame, app: &App) {
    let t = &app.config.theme;
    // Runs of lines from the same change read as one block, so the change is
    // named on its first line only and the rest of the block stays quiet.
    let mut previous: Option<u32> = None;
    let width = (app.blame.len().to_string().len()).max(3);
    let items: Vec<ListItem> = app
        .blame
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let same = previous == Some(line.change);
            previous = Some(line.change);
            let head = if same {
                " ".repeat(30)
            } else {
                format!(
                    "{:>8} {:<10.10} {:<10}",
                    line.change,
                    line.user,
                    line.time.map(p4::civil_date).unwrap_or_default()
                )
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    head,
                    Style::default().fg(if same { t.idle } else { t.changelist }),
                ),
                Span::styled(
                    format!(" {:>width$} ", i + 1),
                    Style::default().fg(t.idle),
                ),
                Span::raw(diffview::expand_tabs(&line.text, app.config.tab_width)),
            ]))
        })
        .collect();

    let area = frame.area();
    let area = centered(
        area,
        area.width.saturating_sub(4),
        area.height.saturating_sub(4).max(6),
    );

    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_style(Style::default().fg(t.focus))
        .title(Span::styled(
            format!(" Blame of {} ", short_path(&app.blame_path)),
            Style::default().fg(t.focus).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            format!(" {}   {}   {} close ", app.config.keys.label_for(Action::Down, Some(Action::Up)), app.config.keys.label_for(Action::First, Some(Action::Last)), key_of(app, Action::Blame)),
            Style::default().fg(t.idle),
        ));

    if app.blame.is_empty() {
        frame.render_widget(
            Paragraph::new(if app.busy { "  loading…" } else { "  nothing to blame" })
                .style(Style::default().fg(t.idle))
                .block(block),
            area,
        );
        return;
    }

    let mut state = ListState::default().with_selected(Some(app.blame_sel));
    frame.render_stateful_widget(
        List::new(items).block(block).highlight_style(selection_style(t, true)),
        area,
        &mut state,
    );
}

fn draw_file_history(frame: &mut Frame, app: &App) {
    let t = &app.config.theme;
    let items: Vec<ListItem> = app
        .history
        .iter()
        .map(|r| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" #{:<4}", r.rev),
                    Style::default().fg(t.added),
                ),
                Span::styled(
                    format!("{:>8} ", r.change),
                    Style::default().fg(t.changelist),
                ),
                Span::styled(
                    format!("{:<10} ", r.time.map(p4::civil_date).unwrap_or_default()),
                    Style::default().fg(t.idle),
                ),
                Span::styled(
                    format!("{:<10.10} ", r.user),
                    Style::default().fg(t.idle),
                ),
                Span::styled(
                    format!("{:<8.8} ", r.action.to_string()),
                    Style::default().fg(action_color(t, &r.action)),
                ),
                Span::raw(r.description.lines().next().unwrap_or_default().to_owned()),
            ]))
        })
        .collect();

    let area = frame.area();
    let width = area.width.saturating_sub(8);
    let height = area.height.saturating_sub(6).max(6);
    let area = centered(area, width, height);

    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_style(Style::default().fg(t.focus))
        .title(Span::styled(
            format!(" History of {} ", short_path(&app.history_path)),
            Style::default().fg(t.focus).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            format!(" {} undo this revision   {} close ", key_of(app, Action::Undo), key_of(app, Action::History)),
            Style::default().fg(t.idle),
        ));

    if app.history.is_empty() {
        frame.render_widget(
            Paragraph::new("  loading…")
                .style(Style::default().fg(t.idle))
                .block(block),
            area,
        );
        return;
    }

    let mut state = ListState::default().with_selected(Some(app.history_rev_sel));
    frame.render_stateful_widget(
        List::new(items).block(block).highlight_style(selection_style(t, true)),
        area,
        &mut state,
    );
}

fn draw_confirm(frame: &mut Frame, t: &Theme, confirm: &Confirm) {
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
            Style::default().fg(t.idle),
        )));
    }

    let width = frame.area().width.saturating_sub(10).min(76).max(30);
    let height = (lines.len() as u16 + 3).min(frame.area().height);
    let area = centered(frame.area(), width, height);

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_style(Style::default().fg(t.danger))
                .title(Span::styled(
                    format!(" {} ", confirm.title),
                    Style::default().fg(t.danger).add_modifier(Modifier::BOLD),
                ))
                // Naming the one key that proceeds, rather than offering a
                // default that could be taken by a stray Enter.
                .title_bottom(Span::styled(
                    " y to confirm   any other key cancels ",
                    Style::default().fg(t.idle),
                )),
        ),
        area,
    );
}

fn draw_picker(frame: &mut Frame, t: &Theme, picker: &Picker) {
    let width = frame.area().width.saturating_sub(10).min(64).max(30);
    let height = (picker.options.len() as u16 + 3).min(frame.area().height);
    let area = centered(frame.area(), width, height);

    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_style(Style::default().fg(t.focus))
        .title(Span::styled(
            format!(" {} ", picker.title),
            Style::default().fg(t.focus).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            " Enter choose   Esc cancel ",
            Style::default().fg(t.idle),
        ));

    let items: Vec<ListItem> = picker
        .options
        .iter()
        .map(|option| match option {
            Destination::Existing(id, summary) => ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:>8} ", id.to_string()),
                    Style::default().fg(t.changelist),
                ),
                Span::raw(summary.clone()),
            ])),
            Destination::New => ListItem::new(Line::from(Span::styled(
                "     new  create a changelist…",
                Style::default().fg(t.added),
            ))),
        })
        .collect();

    let mut state = ListState::default().with_selected(Some(picker.sel));
    frame.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_style(selection_style(t, true)),
        area,
        &mut state,
    );
}

/// Smallest text area the popup ever gets, so an empty description still looks
/// like something you can type into.
const EDITOR_MIN_ROWS: u16 = 5;

/// One visual row of soft-wrapped editor text.
struct WrapRow {
    /// Logical line this row came from.
    line: usize,
    /// Character offset of the row inside that line.
    start: usize,
    text: String,
}

fn draw_editor(frame: &mut Frame, t: &Theme, editor: &Editor) {
    let width = frame.area().width.saturating_sub(10).min(80).max(20);
    // The border eats a column on each side; the text has the rest.
    let text_width = width.saturating_sub(2).max(1);

    let rows = wrap(editor.lines(), text_width as usize);
    // Grow with the text, but never past a share of the window: the popup is a
    // popup, not a screen.
    let max_rows = (frame.area().height * 3 / 5).saturating_sub(2).max(1);
    // A very short window has no room for the floor, and the ceiling wins.
    let min_rows = EDITOR_MIN_ROWS.min(max_rows);
    let text_rows = (rows.len() as u16).clamp(min_rows, max_rows);
    let height = (text_rows + 2).min(frame.area().height);
    let area = centered(frame.area(), width, height);

    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_style(Style::default().fg(t.focus))
        .title(Span::styled(
            format!(" {} ", editor.title),
            Style::default().fg(t.focus).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            " Enter save   Shift-Enter newline   Esc cancel ",
            Style::default().fg(t.idle),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (vrow, vcol) = wrapped_cursor(&rows, editor.cursor());
    // Scroll only as far as it takes to keep the cursor on screen.
    let view = inner.height as usize;
    let offset = vrow.saturating_sub(view.saturating_sub(1));

    let lines: Vec<Line> = rows.iter().map(|r| Line::raw(r.text.clone())).collect();
    frame.render_widget(Paragraph::new(lines).scroll((offset as u16, 0)), inner);

    if inner.height > 0 && vcol < inner.width as usize {
        frame.set_cursor_position((
            inner.x + vcol as u16,
            inner.y + (vrow - offset).min(view - 1) as u16,
        ));
    }
}

/// Soft-wrap logical lines to `width` characters, breaking at a space when one
/// is in reach so words stay whole.
fn wrap(lines: &[String], width: usize) -> Vec<WrapRow> {
    let width = width.max(1);
    let mut out = Vec::new();
    for (line, text) in lines.iter().enumerate() {
        let chars: Vec<char> = text.chars().collect();
        let mut pos = 0;
        while pos < chars.len() {
            let hard = (pos + width).min(chars.len());
            let end = if hard == chars.len() {
                hard
            } else {
                // Break after the last space in the row; a word longer than the
                // row has none, so fall back to cutting at the edge.
                match chars[pos..hard].iter().rposition(|c| c.is_whitespace()) {
                    Some(i) if i > 0 => pos + i + 1,
                    _ => hard,
                }
            };
            out.push(WrapRow {
                line,
                start: pos,
                text: chars[pos..end].iter().collect(),
            });
            pos = end;
        }
        // An empty line, or one that filled its last row exactly, still needs a
        // row for the cursor to sit on.
        if chars.is_empty() || out.last().is_some_and(|r| r.text.chars().count() == width) {
            out.push(WrapRow {
                line,
                start: chars.len(),
                text: String::new(),
            });
        }
    }
    out
}

/// Where the logical cursor lands once the text is wrapped.
fn wrapped_cursor(rows: &[WrapRow], (line, col): (usize, usize)) -> (usize, usize) {
    let found = rows
        .iter()
        .enumerate()
        .rfind(|(_, r)| r.line == line && r.start <= col);
    match found {
        Some((i, r)) => (i, col - r.start),
        None => (0, 0),
    }
}

fn panel_block(app: &App, panel: Panel, extra: Option<String>) -> Block<'static> {
    let t = &app.config.theme;
    let focused = app.focus == panel;
    let mut name = match extra {
        Some(e) => format!("{} {e} ", panel.title()),
        None => format!("{} ", panel.title()),
    };
    // A narrowed list looks like a short one, so say what is hiding the rest.
    let filter = app.filter(panel);
    if !filter.is_empty() {
        name.push_str(&format!("/{filter} "));
    }
    Block::bordered()
        .border_style(Style::default().fg(if focused { t.focus } else { t.idle }))
        // The number is the key that focuses this panel; the title is the only
        // place a reader can discover that.
        .title(Line::from(vec![
            Span::styled(
                format!(" {} ", panel.number()),
                Style::default().fg(t.inverse).bg(if focused {
                    t.focus
                } else {
                    t.muted
                }),
            ),
            Span::styled(
                format!(" {name}"),
                Style::default()
                    .fg(if focused { t.focus } else { t.muted })
                    .add_modifier(Modifier::BOLD),
            ),
        ]))
}

fn change_item(t: &Theme, cl: &Changelist, my_client: &str) -> ListItem<'static> {
    let marker = change_marker(cl);
    let mut spans = vec![
        Span::styled(
            marker.to_string(),
            Style::default().fg(match marker {
                '✓' => t.ok,
                '⌸' => t.shelved,
                _ => t.directory,
            }),
        ),
        Span::raw(" "),
        Span::styled(
            format!("{:>7}", cl.id.to_string()),
            Style::default().fg(t.changelist),
        ),
        Span::raw(" "),
        Span::styled(
            format!("{:<10.10}", cl.user),
            Style::default().fg(t.idle),
        ),
        Span::raw(" "),
        Span::raw(cl.summary().to_owned()),
    ];

    // History and the Others tab both carry changelists from elsewhere. Name
    // the workspace, rather than implying the content is checked out here.
    if !my_client.is_empty() && !cl.client.is_empty() && cl.client != my_client {
        spans.push(Span::styled(
            format!(" @{}", cl.client),
            Style::default().fg(t.modified),
        ));
    }
    ListItem::new(Line::from(spans))
}

fn draw_changes(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.config.theme;
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
            Paragraph::new("  nothing here").style(Style::default().fg(t.idle)),
            list,
        );
        return;
    }

    let items: Vec<ListItem> = changes
        .iter()
        .map(|cl| change_item(t, cl, app.my_client()))
        .collect();
    let mut state = ListState::default().with_selected(Some(app.change_sel));
    frame.render_stateful_widget(
        List::new(items).highlight_style(selection_style(t, app.focus == Panel::Changelists)),
        list,
        &mut state,
    );
}

/// `Local 2 │ Shelved 2 │ Others 1`, with the open tab highlighted.
fn tab_bar(app: &App) -> Line<'static> {
    let t = &app.config.theme;
    let mut spans = Vec::new();
    for (i, tab) in ChangeTab::ORDER.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", Style::default().fg(t.idle)));
        } else {
            spans.push(Span::raw(" "));
        }
        let open = *tab == app.tab;
        spans.push(Span::styled(
            format!("{} {}", tab.title(), app.tab_count(*tab)),
            if open {
                Style::default().fg(t.focus).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(t.idle)
            },
        ));
    }
    Line::from(spans)
}

fn draw_history(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.config.theme;
    // Opened on a change, the panel is that change's file tree.
    if let Some(open) = &app.history_open {
        let block = panel_block(app, Panel::History, Some(format!("of {}", open.change)));
        let empty = if open.loaded {
            "nothing in this change"
        } else {
            "loading…"
        };
        draw_tree_rows(
            frame,
            app,
            area,
            block,
            Panel::History,
            app.opened_rows(),
            open.sel,
            empty,
        );
        return;
    }

    let visible = app.visible_submitted();
    let items: Vec<ListItem> = visible
        .iter()
        .map(|cl| change_item(t, cl, app.my_client()))
        .collect();
    let count = format!("({})", visible.len());
    let mut state = ListState::default().with_selected(Some(app.history_sel));
    frame.render_stateful_widget(
        List::new(items)
            .block(panel_block(app, Panel::History, Some(count)))
            .highlight_style(selection_style(t, app.focus == Panel::History)),
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
    let empty = if app.selected_change().is_none() {
        "no changelist selected"
    } else if app.files_for.is_none() {
        "loading…"
    } else if app.scanning {
        "scanning the workspace…"
    } else if app.scanned.is_empty() {
        "no open files — press u to scan for untracked ones"
    } else {
        "no files visible from this workspace"
    };
    draw_tree_rows(
        frame,
        app,
        area,
        block,
        Panel::Files,
        app.file_rows(),
        app.file_sel,
        empty,
    );
}

/// A file tree, as drawn by the Files panel and by the History panel opened on
/// a change.
#[allow(clippy::too_many_arguments)]
fn draw_tree_rows(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    block: Block<'static>,
    panel: Panel,
    rows: Vec<FileRow<'_>>,
    sel: usize,
    empty: &str,
) {
    let t = &app.config.theme;
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new(empty.to_owned())
                .style(Style::default().fg(t.idle))
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
                Style::default().fg(t.idle).add_modifier(Modifier::BOLD),
            ))),
            FileRow::Dir {
                label,
                depth,
                collapsed,
                files,
                ..
            } => {
                if selectable == sel {
                    selected_row = Some(row);
                }
                let item = dir_item(t, label, *depth, *collapsed, *files);
                // A range only ever exists in the Files panel.
                let ranged = panel == Panel::Files && app.row_selected(selectable);
                let item = mark_range(t, item, ranged);
                selectable += 1;
                item
            }
            FileRow::File {
                entry, label, depth, ..
            } => {
                if selectable == sel {
                    selected_row = Some(row);
                }
                let item = file_item(t, entry, label, *depth);
                // A range only ever exists in the Files panel.
                let ranged = panel == Panel::Files && app.row_selected(selectable);
                let item = mark_range(t, item, ranged);
                selectable += 1;
                item
            }
        })
        .collect();

    let mut state = ListState::default().with_selected(selected_row);
    frame.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_style(selection_style(t, app.focus == panel)),
        area,
        &mut state,
    );
}

/// Tint a row that is inside an open range selection. The cursor's own
/// highlight is drawn over the top by the list widget.
fn mark_range(t: &Theme, item: ListItem<'static>, selected: bool) -> ListItem<'static> {
    if selected {
        item.style(Style::default().bg(t.selection).fg(t.text))
    } else {
        item
    }
}

/// Every row is laid out the same way: a three-column gutter holding the file's
/// action mark, the depth indent, then a two-column slot for the fold arrow —
/// blank on a file. That slot is what puts a directory's contents one level to
/// the right of its name.
fn indent(depth: usize) -> String {
    "  ".repeat(depth)
}

fn dir_item(t: &Theme, label: &str, depth: usize, collapsed: bool, files: usize) -> ListItem<'static> {
    ListItem::new(Line::from(vec![
        Span::raw("   "),
        Span::raw(indent(depth)),
        Span::styled(
            if collapsed { "▸ " } else { "▾ " }.to_owned(),
            Style::default().fg(t.idle),
        ),
        Span::styled(
            label.to_owned(),
            Style::default().fg(t.directory).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {files}"), Style::default().fg(t.idle)),
    ]))
}

fn file_item(t: &Theme, f: &FileEntry, label: &str, depth: usize) -> ListItem<'static> {
    // `??` for a file Perforce has never seen, mirroring git's untracked mark.
    let (code, color) = if f.untracked() {
        ("??".to_owned(), t.untracked)
    } else {
        (f.action.code().to_string(), action_color(t, &f.action))
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
                Style::default().fg(t.muted)
            },
        ),
        if f.unresolved {
            Span::styled(" (unresolved)", Style::default().fg(t.danger))
        } else {
            Span::raw("")
        },
    ]))
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.config.theme;
    let mut lines = Vec::new();
    match &app.info {
        None => lines.push(Line::from(Span::styled(
            "connecting…",
            Style::default().fg(t.idle),
        ))),
        Some(info) => {
            lines.push(field(t, "user", &info.user));
            if info.client_known {
                lines.push(field(t, "client", &info.client));
                if let Some(stream) = &info.stream {
                    lines.push(field(t, "stream", stream));
                }
                lines.push(field(t, "server", &info.server_address));
            } else {
                // Without a client there are no open files and no workspace
                // diffs, so this needs to explain itself rather than sit there.
                lines.push(Line::from(vec![
                    Span::styled("client  ", Style::default().fg(t.idle)),
                    Span::styled("not set", Style::default().fg(t.danger)),
                ]));
                lines.push(Line::from(Span::styled(
                    "start lazyp4 in a workspace, or set P4CLIENT",
                    Style::default().fg(t.danger),
                )));
                lines.push(field(t, "server", &info.server_address));
            }
        }
    }

    frame.render_widget(
        Paragraph::new(lines).block(panel_block(app, Panel::Status, None)),
        area,
    );
}

fn draw_diff(frame: &mut Frame, app: &App, area: Rect) {
    let t = &app.config.theme;
    let Some(file) = app.browsed_file() else {
        frame.render_widget(
            Paragraph::new("select a file")
                .style(Style::default().fg(t.idle))
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
        let msg = if !app.diffs_ready() {
            "loading…"
        } else {
            "no diff for this file"
        };
        frame.render_widget(
            Paragraph::new(msg)
                .style(Style::default().fg(t.idle))
                .block(block),
            area,
        );
        return;
    };

    let rows = diffview::rows(&diff.hunks, app.config.tab_width);
    let width = gutter_width(&rows);
    let lines: Vec<Line> = rows
        .iter()
        .map(|r| diff_line(t, r, width, app.diff_hscroll))
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
fn diff_line(t: &Theme, row: &Row, width: usize, hscroll: usize) -> Line<'static> {
    if row.kind == RowKind::Header {
        return Line::from(Span::styled(
            row.text(),
            Style::default().fg(t.changelist).add_modifier(Modifier::BOLD),
        ));
    }

    let number = |n: Option<u32>| match n {
        Some(n) => format!("{n:>width$}"),
        None => " ".repeat(width),
    };
    let mut spans = vec![Span::styled(
        format!("{} {} ", number(row.old_no), number(row.new_no)),
        Style::default().fg(t.idle),
    )];

    let (fg, changed_bg) = match row.kind {
        RowKind::Add => (t.added, t.added),
        RowKind::Delete => (t.deleted, t.deleted),
        _ => (t.muted, t.muted),
    };
    spans.push(Span::styled(
        row.marker().to_string(),
        Style::default().fg(fg).add_modifier(Modifier::BOLD),
    ));

    let base = match row.kind {
        RowKind::Context => Style::default(),
        RowKind::Note => Style::default().fg(t.idle),
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
                Style::default().bg(changed_bg).fg(t.inverse)
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
    let t = &app.config.theme;
    // Typing a filter takes over the bar, the way `/` does in a pager.
    if let Some(panel) = app.filtering {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" /{}", app.filter(panel)),
                    Style::default().fg(t.focus).add_modifier(Modifier::BOLD),
                ),
                Span::styled("█", Style::default().fg(t.focus)),
                Span::styled(
                    "   Enter keep   Esc clear",
                    Style::default().fg(t.idle),
                ),
            ])),
            area,
        );
        return;
    }

    // An open range changes what every verb will act on, so it takes the bar.
    if app.select_anchor.is_some() {
        let (first, last) = app.selection_range();
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" {} row(s) selected ", last - first + 1),
                    Style::default().fg(t.inverse).bg(t.selection),
                ),
                Span::styled(
                    format!(
                        "  {} move   {} revert   {} shelve   {} or Esc cancel",
                        key_of(app, Action::Move),
                        key_of(app, Action::RevertFiles),
                        key_of(app, Action::ShelveFiles),
                        key_of(app, Action::SelectRange),
                    ),
                    Style::default().fg(t.idle),
                ),
            ])),
            area,
        );
        return;
    }

    let line = match (&app.error, app.busy) {
        (Some(err), _) => Line::from(Span::styled(
            format!(" {} ", err.replace('\n', " ")),
            Style::default().fg(t.text).bg(t.danger),
        )),
        (None, false) if app.notice.is_some() => Line::from(Span::styled(
            format!(" {} ", app.notice.as_deref().unwrap_or_default()),
            Style::default().fg(t.inverse).bg(t.ok),
        )),
        // Several commands take tens of seconds, so say which one is running
        // rather than leaving the UI looking stuck.
        (None, true) => Line::from(vec![
            Span::styled(
                format!(" {} ", SPINNER[app.spinner % SPINNER.len()]),
                Style::default().fg(t.inverse).bg(t.focus),
            ),
            Span::styled(
                format!(" p4 {}", app.running().unwrap_or("working")),
                Style::default().fg(t.focus),
            ),
        ]),
        (None, false) => {
            let mut spans = Vec::new();
            // What the focused panel can do comes first, then the keys that
            // work everywhere.
            let hint = |spans: &mut Vec<Span<'static>>, action: Action, ground, ink| {
                spans.push(Span::styled(
                    format!(" {} ", key_of(app, action)),
                    Style::default().fg(t.inverse).bg(ground),
                ));
                spans.push(Span::styled(
                    format!(" {}   ", action.short()),
                    Style::default().fg(ink),
                ));
            };
            for action in panel_actions(app) {
                hint(&mut spans, *action, t.focus, t.text);
            }
            for action in [Action::Refresh, Action::Help, Action::Quit] {
                hint(&mut spans, action, t.muted, t.idle);
            }
            Line::from(spans)
        }
    };
    frame.render_widget(Paragraph::new(line), area);
}

/// The key that reaches an action, for a hint the reader can act on.
fn key_of(app: &App, action: Action) -> String {
    app.config
        .keys
        .key_for(action)
        .map(|k| k.show())
        .unwrap_or_else(|| "—".to_owned())
}

/// The handful of actions worth showing for the focused panel. Which keys
/// reach them is the keymap's business, not this list's.
fn panel_actions(app: &App) -> &'static [Action] {
    // Opened on a change, History is a file tree with a way back out.
    if app.focus == Panel::History && app.history_open.is_some() {
        return &[Action::Blame, Action::History, Action::Cancel];
    }
    match app.focus {
        Panel::Files => &[
            Action::Move,
            Action::RevertFiles,
            Action::History,
            Action::Blame,
            Action::Scan,
        ],
        Panel::Changelists => &[
            Action::Submit,
            Action::ShelveChange,
            Action::Unshelve,
            Action::NewChange,
            Action::Describe,
            Action::DeleteChange,
        ],
        Panel::History => &[Action::Undo, Action::Fullscreen],
        Panel::Diff => &[Action::Fullscreen, Action::Left],
        Panel::Status => &[],
    }
}

/// Every key, grouped by where it applies, in two columns.
///
/// Built from the keymap rather than written out, so a rebound key is right
/// here without anyone having to remember to say so.
fn draw_help(frame: &mut Frame, t: &Theme, keys: &Keymap) {
    let rows = |group: Group| -> Vec<(String, &'static str)> {
        let mut out: Vec<(String, &'static str)> = Action::ALL
            .into_iter()
            .filter(|a| a.group() == group)
            .filter_map(|a| a.help().map(|h| (keys.label_for(a, h.with), h.label)))
            .filter(|(key, _)| !key.is_empty())
            .collect();
        // Each panel's number focuses it, which the panel titles show but
        // nothing else explains.
        if group == Group::Navigation {
            out.extend(
                Panel::ORDER
                    .iter()
                    .map(|p| (p.number().to_string(), p.title())),
            );
        }
        out
    };

    // Two columns, so the sheet stays one screenful.
    let column = |title: &str, group: Group| -> Vec<Line<'static>> {
        let mut out = vec![Line::from(Span::styled(
            title.to_owned(),
            Style::default()
                .fg(t.changelist)
                .add_modifier(Modifier::BOLD),
        ))];
        out.extend(rows(group).into_iter().map(|(key, what)| {
            Line::from(vec![
                Span::styled(format!("{key:<9}"), Style::default().fg(t.focus)),
                Span::raw(what.to_owned()),
            ])
        }));
        out
    };

    let left: Vec<Line> = column("Navigation", Group::Navigation)
        .into_iter()
        .chain([Line::raw("")])
        .chain(column("Files", Group::Files))
        .collect();
    let right: Vec<Line> = column("Changelists", Group::Changelists)
        .into_iter()
        .chain([Line::raw("")])
        .chain(column("Diff and app", Group::App))
        .collect();

    let height = (left.len().max(right.len()) as u16 + 2).min(frame.area().height);
    // Wide enough that the longest description is not clipped by a column.
    let width = 80.min(frame.area().width);
    let area = centered(frame.area(), width, height);

    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_style(Style::default().fg(t.focus))
        .title(Span::styled(
            " Keys ",
            Style::default().fg(t.focus).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            format!(" {} for the command log   Esc to close ", keys.key_for(Action::Log).map(|k| k.show()).unwrap_or_default()),
            Style::default().fg(t.idle),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [left_area, right_area] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(inner);
    frame.render_widget(Paragraph::new(left), left_area);
    frame.render_widget(Paragraph::new(right), right_area);
}

fn draw_log(frame: &mut Frame, app: &App) {
    let t = &app.config.theme;
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
                Span::styled("  p4 ", Style::default().fg(t.idle)),
                Span::raw(cmd.clone()),
            ])
        })
        .collect();

    overlay(
        frame,
        t,
        " Commands ",
        lines,
        area.width.saturating_sub(8),
        height as u16 + 2,
    );
}

fn overlay(frame: &mut Frame, t: &Theme, title: &str, lines: Vec<Line>, width: u16, height: u16) {
    let area = centered(frame.area(), width, height);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_style(Style::default().fg(t.focus))
                .title(Span::styled(
                    title.to_owned(),
                    Style::default().fg(t.focus).add_modifier(Modifier::BOLD),
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

fn field<'a>(t: &Theme, name: &'a str, value: &str) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("{name:<8}"), Style::default().fg(t.idle)),
        Span::raw(value.to_owned()),
    ])
}

fn selection_style(t: &Theme, focused: bool) -> Style {
    let base = Style::default().add_modifier(Modifier::BOLD);
    if focused {
        base.bg(t.selection).fg(t.text)
    } else {
        base
    }
}

fn action_color(t: &Theme, action: &FileAction) -> Color {
    match action.code() {
        'A' => t.added,
        'M' => t.modified,
        'D' => t.deleted,
        'B' | 'I' => t.integrated,
        _ => t.muted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.split('\n').map(str::to_owned).collect()
    }

    fn texts(rows: &[WrapRow]) -> Vec<&str> {
        rows.iter().map(|r| r.text.as_str()).collect()
    }

    #[test]
    fn a_long_line_wraps_at_a_space() {
        let rows = wrap(&lines("the quick brown fox"), 10);
        assert_eq!(texts(&rows), ["the quick ", "brown fox"]);
        assert!(rows.iter().all(|r| r.line == 0));
    }

    #[test]
    fn a_word_longer_than_the_row_is_cut() {
        let rows = wrap(&lines("supercalifragilistic"), 8);
        assert_eq!(texts(&rows), ["supercal", "ifragili", "stic"]);
    }

    #[test]
    fn every_logical_line_keeps_a_row_of_its_own() {
        let rows = wrap(&lines("first\n\nthird"), 10);
        assert_eq!(texts(&rows), ["first", "", "third"]);
        assert_eq!(rows.iter().map(|r| r.line).collect::<Vec<_>>(), [0, 1, 2]);
    }

    #[test]
    fn the_cursor_follows_the_text_onto_the_next_row() {
        let rows = wrap(&lines("the quick brown fox"), 10);
        // Column 12 is the "o" of "brown", the third character of row two.
        assert_eq!(wrapped_cursor(&rows, (0, 12)), (1, 2));
        // The end of the text sits after the last character.
        assert_eq!(wrapped_cursor(&rows, (0, 19)), (1, 9));
    }

    #[test]
    fn a_full_row_gives_the_cursor_somewhere_to_land() {
        let rows = wrap(&lines("0123456789"), 10);
        assert_eq!(texts(&rows), ["0123456789", ""]);
        assert_eq!(wrapped_cursor(&rows, (0, 10)), (1, 0));
    }

    #[test]
    fn wrapping_counts_characters_not_bytes() {
        let rows = wrap(&lines("ååååå ööööö"), 6);
        assert_eq!(texts(&rows), ["ååååå ", "ööööö"]);
        assert_eq!(wrapped_cursor(&rows, (0, 8)), (1, 2));
    }
}
