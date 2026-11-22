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
    /// Jump forwards/backwards by this many seconds.
    Seek(i32),
    /// Set the volume to exactly this value (0–100).
    SetVolume(u8),
    ToggleShuffle,
    CycleRepeat,
    /// Start Music.app hidden (the `o` key).
    LaunchMusic,
}

/// News from the worker to the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// A fresh status, plus how many commands the worker had finished when it
    /// fetched it. The app uses `acked` to ignore statuses that are older than
    /// a key you just pressed (see `App::apply`).
    ///
    /// Rust note: this is a *struct variant*: an enum case with named
    /// fields, like a small struct.
    Status {
        status: PlayerStatus,
        acked: u64,
    },
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
    // How many commands we've finished. `u64` is a 64-bit unsigned number;
    // it will never overflow in practice.
    let mut acked: u64 = 0;

    loop {
        // 1. Ask Music what's going on and tell the UI.
        let update = match music.status() {
            Ok(StatusReply::Status(s)) => {
                launched_at = None;
                Update::Status { status: s, acked }
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

        // 2. Wait up to POLL_INTERVAL for a command. If none arrives, loop
        //    round and poll again.
        let first = match commands.recv_timeout(POLL_INTERVAL) {
            Ok(cmd) => cmd,
            Err(RecvTimeoutError::Timeout) => continue, // nothing to do: poll again
            Err(RecvTimeoutError::Disconnected) => return,
        };

        // 3. Run that command, then any others that piled up meanwhile (e.g.
        //    you're holding `+`), *before* polling. Polling between each one
        //    would make held keys lag behind.
        //
        //    `std::iter::once(first).chain(commands.try_iter())` means "the
        //    first command, followed by every command already waiting".
        for cmd in std::iter::once(first).chain(commands.try_iter()) {
            if cmd == Command::LaunchMusic {
                launched_at = Some(launch(&music, &updates));
            } else if let Err(e) = execute(&music, &cmd) {
                let _ = updates.send(Update::Error(e));
            }
            acked += 1;
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
        Command::Seek(seconds) => music.seek_by(*seconds),
        Command::SetVolume(v) => music.set_volume(*v),
        Command::ToggleShuffle => music.toggle_shuffle(),
        Command::CycleRepeat => music.cycle_repeat(),
        Command::LaunchMusic => music.launch_hidden(),
    }
}
