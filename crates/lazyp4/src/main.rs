//! lazyp4 — a terminal UI for Perforce.

mod app;
mod editor;
mod hunk;
#[cfg(test)]
mod tests;
mod ui;
mod worker;

use std::io;

use app::{Action, App};
use worker::Worker;

fn main() -> io::Result<()> {
    let (worker, events) = Worker::spawn();
    let mut app = App::new(worker);

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app, &events);
    ratatui::restore();

    app.shutdown();
    result
}

fn run(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    events: &std::sync::mpsc::Receiver<worker::Event>,
) -> io::Result<()> {
    terminal.draw(|frame| ui::draw(frame, app))?;

    // Both the worker and the input reader feed this channel, so a blocking
    // receive costs nothing and redraws happen only when something changed.
    while let Ok(event) = events.recv() {
        app.handle(event);

        // Drain whatever else arrived so a burst redraws once.
        while let Ok(event) = events.try_recv() {
            app.handle(event);
        }

        if app.quit {
            return Ok(());
        }
        if let Some(action) = app.action.take() {
            run_action(terminal, app, action)?;
        }
        terminal.draw(|frame| ui::draw(frame, app))?;
    }
    Ok(())
}

/// Give the terminal to another program, then take it back.
fn run_action(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    action: Action,
) -> io::Result<()> {
    match action {
        Action::OpenInHunk(patch) => {
            if !hunk::available() {
                app.error = Some("hunk is not on PATH — see https://hunk.dev".into());
                return Ok(());
            }
            ratatui::restore();
            let result = hunk::show(&patch);
            *terminal = ratatui::init();
            terminal.clear()?;
            if let Err(e) = result {
                app.error = Some(format!("hunk: {e}"));
            }
        }
    }
    Ok(())
}

// The lib.rs cargo generated for this crate is unused; the binary is the crate.
