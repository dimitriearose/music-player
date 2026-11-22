//! `am`: a quiet terminal front end for Apple Music.
//!
//! Start reading here. This file:
//! 1. starts the background worker that talks to Music.app (`worker.rs`),
//! 2. switches the terminal into full-screen "raw" mode,
//! 3. runs the main loop: apply updates → draw → handle a key → repeat,
//! 4. puts the terminal back to normal on the way out.
//!
//! Rust note: each `mod x;` line pulls in the file `src/x.rs` (or
//! `src/x/mod.rs`) as a module. Rust only compiles files that are declared
//! this way.

mod app;
mod music;
mod ui;
mod worker;

use app::App;
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyEventKind},
};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};
use worker::{Command, Update};

/// How long we wait for a key before redrawing anyway. 100 ms = 10 frames a
/// second, smooth enough for a progress bar and very light on the CPU.
const TICK: Duration = Duration::from_millis(100);

/// Rust note: `anyhow::Result<()>` means "returns nothing on success, or any
/// kind of error". If `main` returns an error, Rust prints it and exits with a
/// non-zero status.
fn main() -> anyhow::Result<()> {
    let (commands, updates) = worker::spawn(music::Music::new());

    // `ratatui::init()` enters the "alternate screen" (so your shell history
    // reappears untouched when we quit), enables raw mode (keys arrive one at a
    // time, not line by line), and installs a panic hook that restores the
    // terminal even if the program crashes.
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &commands, &updates);
    ratatui::restore();

    // Returning the loop's result *after* restoring means any error message is
    // printed to a normal, readable terminal.
    result
}

/// The main loop. Returns when the user quits.
fn run(
    terminal: &mut DefaultTerminal,
    commands: &Sender<Command>,
    updates: &Receiver<Update>,
) -> anyhow::Result<()> {
    let mut app = App::new(Instant::now());

    while !app.should_quit {
        // 1. Apply everything the worker has sent since last time.
        //    `try_recv` never waits: it returns `Err` when the channel is empty.
        while let Ok(update) = updates.try_recv() {
            app.apply(update, Instant::now());
        }

        // 2. Draw the screen.
        let now = Instant::now();
        terminal.draw(|frame| ui::draw(frame, &app, now))?;

        // 3. Wait up to one TICK for input.
        if event::poll(TICK)? {
            // Only react to key *presses* (not releases or repeats-as-release).
            if let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
                && let Some(cmd) = app.handle_key(key)
            {
                // If the worker has died, sending fails; there's nothing
                // useful to do about that here, so we ignore it.
                let _ = commands.send(cmd);
            }
        }
    }
    Ok(())
}
