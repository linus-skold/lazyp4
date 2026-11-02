//! lazyp4 — a terminal UI for Perforce.

mod app;
mod config;
mod diffview;
mod editor;
mod ignore;
#[cfg(test)]
mod tests;
mod text;
mod tree;
mod ui;
mod update;
mod worker;

use std::io;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use crossterm::event::{DisableMouseCapture, EnableMouseCapture, Event as TermEvent};
use crossterm::execute;
use ratatui::layout::Rect;

use app::App;
use config::Config;
use worker::Worker;

/// How often the spinner advances while a command is in flight.
const TICK: Duration = Duration::from_millis(120);

/// How often an idle lazyp4 looks for `p4` having been used elsewhere. Long
/// enough that a shell command lands before the next look, short enough that
/// the two windows do not disagree for long.
const POLL: Duration = Duration::from_secs(5);

fn main() -> io::Result<()> {
    // No worker and no terminal: this must work without a Perforce server.
    if update::wants_update(std::env::args_os().skip(1)) {
        std::process::exit(update::run());
    }

    let config = Config::load();
    let (worker, events) = Worker::spawn(config.update_check);
    let mut app = App::new(worker, config);

    let mut terminal = ratatui::init();
    let result = (|| {
        let _mouse = MouseCapture;
        execute!(io::stdout(), EnableMouseCapture)?;
        run(&mut terminal, &mut app, &events)
    })();
    ratatui::restore();

    app.shutdown();
    result
}

struct MouseCapture;

impl Drop for MouseCapture {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), DisableMouseCapture);
    }
}

fn handle_event(terminal: &ratatui::DefaultTerminal, app: &mut App, event: worker::Event) -> io::Result<()> {
    match event {
        worker::Event::Input(TermEvent::Mouse(mouse)) => {
            let size = terminal.size()?;
            app.viewport = Rect::new(0, 0, size.width, size.height);
            app.handle_mouse(mouse, app.viewport);
        }
        worker::Event::Input(TermEvent::Key(key)) => {
            let size = terminal.size()?;
            app.viewport = Rect::new(0, 0, size.width, size.height);
            app.handle(worker::Event::Input(TermEvent::Key(key)));
        }
        event => app.handle(event),
    }
    Ok(())
}

fn run(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    events: &std::sync::mpsc::Receiver<worker::Event>,
) -> io::Result<()> {
    terminal.draw(|frame| ui::draw(frame, app))?;

    loop {
        // Both the worker and the input reader feed this channel, so redraws
        // happen only when something changed. While a command is running the
        // wait is short so the spinner keeps turning — several commands take
        // tens of seconds; idle, it is only long enough to notice `p4` having
        // been used in another window.
        let wait = if app.busy { TICK } else { POLL };
        let event = match events.recv_timeout(wait) {
            Ok(event) => Some(event),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        };

        match event {
            Some(event) => {
                handle_event(terminal, app, event)?;
                // Drain whatever else arrived so a burst redraws once.
                while let Ok(event) = events.try_recv() {
                    handle_event(terminal, app, event)?;
                }
            }
            None if app.busy => app.tick(),
            None => app.poll_external(),
        }

        if app.quit {
            return Ok(());
        }
        terminal.draw(|frame| ui::draw(frame, app))?;
    }
}
