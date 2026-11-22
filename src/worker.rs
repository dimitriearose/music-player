//! The background thread that does all the slow talking to Music.app.
//!
//! Why a separate thread? Each `osascript` call takes ~50–100 ms, and the very
//! first one can block for as long as you take to answer the macOS permission
//! prompt. If the drawing thread made those calls, the screen would freeze.
//!
//! The two threads communicate through *channels*. A channel is a one-way
//! pipe: one side `send`s values, the other side `recv`s them, in order.
//!
//! ```text
//!  main thread  ── Command ──▶  worker thread
//!  main thread  ◀── Update ───  worker thread
//! ```

use crate::music::{Music, MusicError, PlayerStatus, StatusReply};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

/// How often we ask Music.app for its status when nothing else is happening.
pub const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// How long we keep saying "opening Music…" after launching it, before
/// admitting it didn't start.
const LAUNCH_GRACE: Duration = Duration::from_secs(10);

/// Requests from the UI to the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    PlayPause,
    Next,
    Previous,
    /// Start Music.app hidden (the `o` key).
    LaunchMusic,
}

/// News from the worker to the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    Status(PlayerStatus),
    NotRunning,
    /// We've asked macOS to start Music and are waiting for it.
    Launching,
    Error(MusicError),
}

/// Start the worker thread. Returns the two channel ends the UI needs:
/// one to send commands, one to receive updates.
pub fn spawn(music: Music) -> (Sender<Command>, Receiver<Update>) {
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (upd_tx, upd_rx) = mpsc::channel();

    // Rust note: `move` hands ownership of `music`, `cmd_rx` and `upd_tx` to the
    // new thread. Afterwards the main thread can't touch them, and the compiler
    // enforces this. That's how Rust prevents two threads fighting over data.
    thread::spawn(move || run(music, cmd_rx, upd_tx));

    (cmd_tx, upd_rx)
}

/// The worker's main loop: poll, wait for a command or the next poll, repeat.
fn run(music: Music, commands: Receiver<Command>, updates: Sender<Update>) {
    // When we last launched Music (if we're still waiting for it to start).
    let mut launched_at: Option<Instant> = None;
    let mut first_poll = true;

    loop {
        // 1. Ask Music what's going on and tell the UI.
        let update = match music.status() {
            Ok(StatusReply::Status(s)) => {
                launched_at = None;
                Update::Status(s)
            }
            Ok(StatusReply::NotRunning) | Err(MusicError::NotRunning) => {
                if first_poll {
                    // DESIGN.md §6.2: at startup, open Music hidden for you.
                    launched_at = Some(launch(&music, &updates));
                }
                match launched_at {
                    Some(t) if t.elapsed() < LAUNCH_GRACE => Update::Launching,
                    _ => Update::NotRunning,
                }
            }
            Err(e) => Update::Error(e),
        };
        first_poll = false;

        // `send` fails only if the UI has gone away (the app quit), so stop.
        if updates.send(update).is_err() {
            return;
        }

        // 2. Wait up to POLL_INTERVAL for a command. If one arrives we run it
        //    and loop straight back to polling, so the screen updates quickly
        //    after you press a key.
        match commands.recv_timeout(POLL_INTERVAL) {
            Ok(Command::LaunchMusic) => launched_at = Some(launch(&music, &updates)),
            Ok(cmd) => {
                if let Err(e) = execute(&music, &cmd) {
                    let _ = updates.send(Update::Error(e));
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            // The UI dropped its end of the channel: time to stop.
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Launch Music hidden; report a failure to the UI. Returns the launch time.
fn launch(music: &Music, updates: &Sender<Update>) -> Instant {
    let _ = updates.send(Update::Launching);
    if let Err(e) = music.launch_hidden() {
        let _ = updates.send(Update::Error(e));
    }
    Instant::now()
}

/// Carry out one playback command.
fn execute(music: &Music, cmd: &Command) -> Result<(), MusicError> {
    match cmd {
        Command::PlayPause => music.play_pause(),
        Command::Next => music.next(),
        Command::Previous => music.previous(),
        Command::LaunchMusic => music.launch_hidden(),
    }
}
