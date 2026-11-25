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
mod art;
mod check;
mod config;
mod library;
mod music;
mod popup;
mod shuffle;
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
    // `am --check` runs a read-only self-test instead of the full-screen app.
    // `std::env::args()` is the list of words typed after the program name
    // (`nth(1)` = the first one).
    match std::env::args().nth(1).as_deref() {
        Some("--check") => return check::run(),
        Some("--config") => {
            config::print();
            return Ok(());
        }
        Some("--init-config") => return config::init(),
        // `env!` reads the version from Cargo.toml when compiling.
        Some("--version" | "-V") => {
            println!("am {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--help" | "-h") => {
            println!(
                "am {}: a quiet, keyboard-driven player for Apple Music",
                env!("CARGO_PKG_VERSION")
            );
            println!();
            println!("  am                 start (press ? inside for keys)");
            println!("  am --check         test the connection to Music, read-only");
            println!("  am --config        show settings and where they come from");
            println!("  am --init-config   create ~/.config/am/config.toml");
            println!("  am --version       print the version");
            return Ok(());
        }
        _ => {}
    }

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
    app.truecolor = art::truecolor_supported();
    app.use_config(&config::load(), Instant::now());

    while !app.should_quit {
        // 1. Apply everything the worker has sent since last time.
        //    `try_recv` never waits: it returns `Err` when the channel is empty.
        while let Ok(update) = updates.try_recv() {
            app.apply(update, Instant::now());
        }

        // 2. The sleep timer may want to pause the music.
        if let Some(cmd) = app.tick(Instant::now()) {
            let _ = commands.send(cmd);
        }

        // 3. Draw the screen.
        let now = Instant::now();
        terminal.draw(|frame| ui::draw(frame, &app, now))?;

        // 4. Wait up to one TICK for input.
        if event::poll(TICK)? {
            // Only react to key *presses* (not releases or repeats-as-release).
            if let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
                && let Some(cmd) = app.handle_key(key, Instant::now())
            {
                // If the worker has died, sending fails; there's nothing
                // useful to do about that here, so we ignore it.
                let _ = commands.send(cmd);
            }
        }
    }
    Ok(())
}
