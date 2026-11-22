//! The application's state and what each key does.
//!
//! This file deliberately knows nothing about drawing or AppleScript. It's
//! plain logic, which makes it easy to unit-test: feed in keys and updates,
//! check the state that comes out.

use crate::music::{MusicError, PlayState, PlayerStatus, Repeat};
use crate::worker::{Command, Update};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::{Duration, Instant};

/// How long an error/notice stays on the bottom line.
const MESSAGE_TTL: Duration = Duration::from_secs(4);
/// After this long with no reply, hint that a permission prompt may be waiting.
const SLOW_START: Duration = Duration::from_secs(2);
/// Seconds moved by one press of ← / →.
pub const SEEK_STEP: i32 = 5;
/// Volume change for one press of + / -.
pub const VOLUME_STEP: i32 = 5;

/// Are we talking to Music.app, and if not, why not?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connection {
    /// Started, no reply yet.
    Waiting,
    Connected,
    Launching,
    NotRunning,
    PermissionDenied,
}

/// Everything the screen needs to know.
pub struct App {
    pub connection: Connection,
    /// The latest status from Music (adjusted by our own predictions).
    pub status: Option<PlayerStatus>,
    /// When `status.position` was last set, used to move the progress bar.
    status_at: Instant,
    started: Instant,
    /// A short notice for the bottom line, and when it was posted.
    message: Option<(String, Instant)>,
    /// Is the `?` key list open?
    pub show_help: bool,
    /// How many commands we've sent to the worker (see `apply`).
    sent: u64,
    pub should_quit: bool,
}

impl App {
    /// Rust note: `now: Instant` is passed in (rather than calling
    /// `Instant::now()` inside) so tests can control time.
    pub fn new(now: Instant) -> Self {
        App {
            connection: Connection::Waiting,
            status: None,
            status_at: now,
            started: now,
            message: None,
            show_help: false,
            sent: 0,
            should_quit: false,
        }
    }

    /// Decide what a key press means. Returns a command for the worker, if any.
    ///
    /// When a command is returned, the screen is *also* updated straight away
    /// with what we expect to happen (e.g. the volume number goes up), so the
    /// app feels instant even though Music takes ~100 ms to respond.
    pub fn handle_key(&mut self, key: KeyEvent, now: Instant) -> Option<Command> {
        let cmd = self.command_for(key)?; // `?` on an Option: return None if None
        self.predict(&cmd, now);
        self.sent += 1;
        Some(cmd)
    }

    /// The key → command table. Keys that only change the screen (like `?`)
    /// are handled here too and return `None`.
    fn command_for(&mut self, key: KeyEvent) -> Option<Command> {
        // In "raw mode" the terminal no longer turns Ctrl-C into "stop the
        // program", so we handle it ourselves.
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return None;
        }

        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                return None;
            }
            KeyCode::Char('?') => {
                self.show_help = !self.show_help;
                return None;
            }
            KeyCode::Esc => {
                self.show_help = false;
                return None;
            }
            KeyCode::Char('o') if self.connection == Connection::NotRunning => {
                return Some(Command::LaunchMusic);
            }
            _ => {}
        }

        // Playback keys only work once we're connected. Sending
        // `tell application "Music"` while Music is closed would launch it
        // *visibly*, which is exactly what we promised not to do.
        if self.connection != Connection::Connected {
            return None;
        }
        let volume = self.status.as_ref().map_or(50, |s| s.volume) as i32;

        match key.code {
            KeyCode::Char(' ') => Some(Command::PlayPause),
            KeyCode::Char('n') => Some(Command::Next),
            KeyCode::Char('p') => Some(Command::Previous),
            KeyCode::Left | KeyCode::Char('h') => Some(Command::Seek(-SEEK_STEP)),
            KeyCode::Right | KeyCode::Char('l') => Some(Command::Seek(SEEK_STEP)),
            // `=` is accepted too, so you don't need Shift for "+".
            KeyCode::Char('+') | KeyCode::Char('=') => Some(Command::SetVolume(
                (volume + VOLUME_STEP).clamp(0, 100) as u8,
            )),
            KeyCode::Char('-') | KeyCode::Char('_') => Some(Command::SetVolume(
                (volume - VOLUME_STEP).clamp(0, 100) as u8,
            )),
            KeyCode::Char('s') => Some(Command::ToggleShuffle),
            KeyCode::Char('r') => Some(Command::CycleRepeat),
            _ => None,
        }
    }

    /// Update our copy of the status to what `cmd` will do, before Music
    /// confirms it.
    fn predict(&mut self, cmd: &Command, now: Instant) {
        // Work out the current position *before* borrowing `status` mutably.
        // (Rust won't let us read `self` while we hold a mutable borrow of
        // part of it, so the order matters.)
        let pos = self.position(now);
        let Some(s) = &mut self.status else { return };
        let duration = s.track.as_ref().map_or(f64::MAX, |t| t.duration);

        match cmd {
            Command::PlayPause => {
                s.position = pos; // freeze the bar exactly where it is
                self.status_at = now;
                s.state = match s.state {
                    PlayState::Playing => PlayState::Paused,
                    _ => PlayState::Playing,
                };
            }
            Command::Seek(delta) => {
                s.position = (pos + *delta as f64).clamp(0.0, duration);
                self.status_at = now;
            }
            Command::SetVolume(v) => s.volume = *v,
            Command::ToggleShuffle => s.shuffle = !s.shuffle,
            Command::CycleRepeat => s.repeat = next_repeat(s.repeat),
            // Next/previous: we can't know the next song, so just wait for
            // the poll. LaunchMusic: nothing to predict.
            Command::Next | Command::Previous | Command::LaunchMusic => {}
        }
    }

    /// Apply news from the worker.
    pub fn apply(&mut self, update: Update, now: Instant) {
        match update {
            Update::Status { status, acked } => {
                // A status fetched before the worker finished our latest
                // command would undo our prediction (the volume would flick
                // back for a moment), so skip it; a fresher one is coming.
                if acked < self.sent {
                    return;
                }
                self.connection = Connection::Connected;
                self.status = Some(status);
                self.status_at = now;
            }
            Update::NotRunning => {
                self.connection = Connection::NotRunning;
                self.status = None;
            }
            Update::Launching => self.connection = Connection::Launching,
            Update::Error(MusicError::PermissionDenied) => {
                self.connection = Connection::PermissionDenied;
            }
            Update::Error(e) => self.message = Some((e.to_string(), now)),
        }
    }

    /// Where the song is *now*, in seconds.
    ///
    /// We only hear from Music once a second, so between replies we add the
    /// time that has passed (if playing). That keeps the progress bar smooth.
    pub fn position(&self, now: Instant) -> f64 {
        let Some(s) = &self.status else { return 0.0 };
        let mut pos = s.position;
        if s.state == PlayState::Playing {
            pos += now.saturating_duration_since(self.status_at).as_secs_f64();
        }
        match &s.track {
            // Never run past the end, even if the next poll is late.
            Some(t) if t.duration > 0.0 => pos.min(t.duration),
            _ => pos,
        }
    }

    /// True if we've been waiting a while with no reply at all.
    pub fn slow_start(&self, now: Instant) -> bool {
        self.connection == Connection::Waiting && now.duration_since(self.started) > SLOW_START
    }

    /// The bottom-line notice, if it hasn't expired yet.
    ///
    /// Rust note: the return type `Option<&str>` borrows the text from `self`
    /// instead of copying it. The compiler checks the borrow can't outlive `App`.
    pub fn message(&self, now: Instant) -> Option<&str> {
        match &self.message {
            Some((text, at)) if now.duration_since(*at) < MESSAGE_TTL => Some(text),
            _ => None,
        }
    }
}

/// Repeat cycles off → all → one → off (matches `scripts::CYCLE_REPEAT`).
pub fn next_repeat(r: Repeat) -> Repeat {
    match r {
        Repeat::Off => Repeat::All,
        Repeat::All => Repeat::One,
        Repeat::One => Repeat::Off,
    }
}

/// Seconds to `m:ss`, e.g. `276.4` becomes `"4:36"`.
pub fn format_time(secs: f64) -> String {
    let total = secs.max(0.0) as u64; // `as` converts types; drops the decimals
    format!("{}:{:02}", total / 60, total % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::Track;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn status(state: PlayState, position: f64, duration: f64) -> PlayerStatus {
        PlayerStatus {
            state,
            track: Some(Track {
                id: "X".into(),
                name: "Test Song".into(),
                artist: "Band".into(),
                album: "Album".into(),
                duration,
            }),
            position,
            volume: 50,
            shuffle: false,
            repeat: Repeat::Off,
        }
    }

    /// Shorthand for a status update the worker would send before any command.
    fn fresh(s: PlayerStatus) -> Update {
        Update::Status {
            status: s,
            acked: 0,
        }
    }

    fn connected_app(now: Instant) -> App {
        let mut app = App::new(now);
        app.apply(fresh(status(PlayState::Playing, 0.0, 100.0)), now);
        app
    }

    #[test]
    fn playback_keys_map_to_commands_when_connected() {
        let now = Instant::now();
        let mut app = connected_app(now);
        assert_eq!(app.handle_key(key(' '), now), Some(Command::PlayPause));
        assert_eq!(app.handle_key(key('n'), now), Some(Command::Next));
        assert_eq!(app.handle_key(key('p'), now), Some(Command::Previous));
        assert_eq!(app.handle_key(key('h'), now), Some(Command::Seek(-5)));
        assert_eq!(
            app.handle_key(KeyEvent::from(KeyCode::Right), now),
            Some(Command::Seek(5))
        );
        assert_eq!(app.handle_key(key('s'), now), Some(Command::ToggleShuffle));
        assert_eq!(app.handle_key(key('r'), now), Some(Command::CycleRepeat));
        assert_eq!(app.handle_key(key('x'), now), None);
        assert!(!app.should_quit);
    }

    #[test]
    fn playback_keys_do_nothing_when_music_is_closed() {
        let now = Instant::now();
        let mut app = App::new(now);
        app.apply(Update::NotRunning, now);
        assert_eq!(app.handle_key(key(' '), now), None);
        assert_eq!(app.handle_key(key('+'), now), None);
        assert_eq!(app.handle_key(key('o'), now), Some(Command::LaunchMusic));
    }

    #[test]
    fn q_and_ctrl_c_quit() {
        let now = Instant::now();
        let mut app = App::new(now);
        app.handle_key(key('q'), now);
        assert!(app.should_quit);

        let mut app = App::new(now);
        app.handle_key(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            now,
        );
        assert!(app.should_quit);
    }

    #[test]
    fn volume_goes_up_and_down_and_clamps() {
        let now = Instant::now();
        let mut app = connected_app(now); // volume 50
        assert_eq!(app.handle_key(key('+'), now), Some(Command::SetVolume(55)));
        assert_eq!(app.handle_key(key('='), now), Some(Command::SetVolume(60)));
        assert_eq!(app.status.as_ref().unwrap().volume, 60, "predicted");
        assert_eq!(app.handle_key(key('-'), now), Some(Command::SetVolume(55)));

        app.status.as_mut().unwrap().volume = 98;
        assert_eq!(app.handle_key(key('+'), now), Some(Command::SetVolume(100)));
        app.status.as_mut().unwrap().volume = 3;
        assert_eq!(app.handle_key(key('-'), now), Some(Command::SetVolume(0)));
    }

    #[test]
    fn seek_prediction_clamps_to_the_track() {
        let now = Instant::now();
        let mut app = App::new(now);
        app.apply(fresh(status(PlayState::Paused, 3.0, 100.0)), now);
        app.handle_key(key('h'), now);
        assert_eq!(app.position(now), 0.0, "not before the start");

        app.status.as_mut().unwrap().position = 98.0;
        app.handle_key(key('l'), now);
        assert_eq!(app.position(now), 100.0, "not past the end");
    }

    #[test]
    fn shuffle_and_repeat_predictions() {
        let now = Instant::now();
        let mut app = connected_app(now);
        app.handle_key(key('s'), now);
        assert!(app.status.as_ref().unwrap().shuffle);

        let repeat = |app: &App| app.status.as_ref().unwrap().repeat;
        app.handle_key(key('r'), now);
        assert_eq!(repeat(&app), Repeat::All);
        app.handle_key(key('r'), now);
        assert_eq!(repeat(&app), Repeat::One);
        app.handle_key(key('r'), now);
        assert_eq!(repeat(&app), Repeat::Off);
    }

    #[test]
    fn play_pause_freezes_and_resumes_the_bar() {
        let now = Instant::now();
        let mut app = App::new(now);
        app.apply(fresh(status(PlayState::Playing, 10.0, 100.0)), now);

        let t1 = now + Duration::from_secs(2);
        app.handle_key(key(' '), t1); // pause at 12 s
        assert_eq!(app.position(t1 + Duration::from_secs(5)), 12.0);

        let t2 = t1 + Duration::from_secs(5);
        app.handle_key(key(' '), t2); // resume
        assert_eq!(app.position(t2 + Duration::from_secs(1)), 13.0);
    }

    #[test]
    fn stale_status_is_ignored_until_the_worker_catches_up() {
        let now = Instant::now();
        let mut app = connected_app(now); // volume 50
        app.handle_key(key('+'), now); // sent = 1, predicted 55

        // A poll that started before our command: still says 50. Ignore it.
        app.apply(fresh(status(PlayState::Playing, 0.0, 100.0)), now);
        assert_eq!(app.status.as_ref().unwrap().volume, 55);

        // The worker has now done our command: trust it.
        let mut confirmed = status(PlayState::Playing, 0.0, 100.0);
        confirmed.volume = 54; // Music sometimes rounds; we show what it says
        app.apply(
            Update::Status {
                status: confirmed,
                acked: 1,
            },
            now,
        );
        assert_eq!(app.status.as_ref().unwrap().volume, 54);
    }

    #[test]
    fn help_toggles_with_question_mark_and_closes_with_esc() {
        let now = Instant::now();
        let mut app = App::new(now);
        app.handle_key(key('?'), now);
        assert!(app.show_help);
        app.handle_key(key('?'), now);
        assert!(!app.show_help);
        app.handle_key(key('?'), now);
        app.handle_key(KeyEvent::from(KeyCode::Esc), now);
        assert!(!app.show_help);
    }

    #[test]
    fn permission_error_switches_screen_other_errors_show_a_message() {
        let now = Instant::now();
        let mut app = App::new(now);
        app.apply(Update::Error(MusicError::PermissionDenied), now);
        assert_eq!(app.connection, Connection::PermissionDenied);

        app.apply(Update::Error(MusicError::Script("boom".into())), now);
        assert_eq!(app.message(now), Some("AppleScript error: boom"));
        assert_eq!(
            app.message(now + Duration::from_secs(5)),
            None,
            "message fades"
        );
    }

    #[test]
    fn position_advances_while_playing() {
        let now = Instant::now();
        let mut app = App::new(now);
        app.apply(fresh(status(PlayState::Playing, 10.0, 100.0)), now);
        let later = now + Duration::from_millis(2500);
        assert!((app.position(later) - 12.5).abs() < 1e-9);
    }

    #[test]
    fn position_is_frozen_while_paused() {
        let now = Instant::now();
        let mut app = App::new(now);
        app.apply(fresh(status(PlayState::Paused, 10.0, 100.0)), now);
        assert_eq!(app.position(now + Duration::from_secs(5)), 10.0);
    }

    #[test]
    fn position_never_passes_the_end() {
        let now = Instant::now();
        let mut app = App::new(now);
        app.apply(fresh(status(PlayState::Playing, 99.0, 100.0)), now);
        assert_eq!(app.position(now + Duration::from_secs(10)), 100.0);
    }

    #[test]
    fn slow_start_hint_after_two_seconds() {
        let now = Instant::now();
        let app = App::new(now);
        assert!(!app.slow_start(now + Duration::from_secs(1)));
        assert!(app.slow_start(now + Duration::from_secs(3)));
    }

    #[test]
    fn formats_times() {
        assert_eq!(format_time(276.4), "4:36");
        assert_eq!(format_time(5.0), "0:05");
        assert_eq!(format_time(0.0), "0:00");
        assert_eq!(format_time(-3.0), "0:00");
        assert_eq!(format_time(3725.0), "62:05");
    }
}
