//! lazyp4 — a terminal UI for Perforce.

mod app;
mod diffview;
mod editor;
#[cfg(test)]
mod tests;
mod tree;
mod ui;
mod worker;

use std::io;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use app::App;
use worker::Worker;

/// How often the spinner advances while a command is in flight.
const TICK: Duration = Duration::from_millis(120);

/// How often an idle lazyp4 looks for `p4` having been used elsewhere. Long
/// enough that a shell command lands before the next look, short enough that
/// the two windows do not disagree for long.
const POLL: Duration = Duration::from_secs(5);

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
                app.handle(event);
                // Drain whatever else arrived so a burst redraws once.
                while let Ok(event) = events.try_recv() {
                    app.handle(event);
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
