//! The application's state and what each key does.
//!
//! This file deliberately knows nothing about drawing or AppleScript. It's
//! plain logic, which makes it easy to unit-test: feed in keys and updates,
//! check the state that comes out.

use crate::music::{MusicError, PlayState, PlayerStatus};
use crate::worker::{Command, Update};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::{Duration, Instant};

/// How long an error/notice stays on the bottom line.
const MESSAGE_TTL: Duration = Duration::from_secs(4);
/// After this long with no reply, hint that a permission prompt may be waiting.
const SLOW_START: Duration = Duration::from_secs(2);

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
    /// The latest status from Music, if we've had one.
    pub status: Option<PlayerStatus>,
    /// When `status` arrived, used to move the progress bar between polls.
    status_at: Instant,
    started: Instant,
    /// A short notice for the bottom line, and when it was posted.
    message: Option<(String, Instant)>,
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
            should_quit: false,
        }
    }

    /// Decide what a key press means. Returns a command for the worker, if any.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Command> {
        // In "raw mode" the terminal no longer turns Ctrl-C into "stop the
        // program", so we handle it ourselves.
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return None;
        }

        let connected = self.connection == Connection::Connected;
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                None
            }
            // Playback keys only work once we're connected. Sending
            // `tell application "Music"` while Music is closed would launch it
            // *visibly*, which is exactly what we promised not to do.
            KeyCode::Char(' ') if connected => Some(Command::PlayPause),
            KeyCode::Char('n') if connected => Some(Command::Next),
            KeyCode::Char('p') if connected => Some(Command::Previous),
            KeyCode::Char('o') if self.connection == Connection::NotRunning => {
                Some(Command::LaunchMusic)
            }
            _ => None,
        }
    }

    /// Apply news from the worker.
    pub fn apply(&mut self, update: Update, now: Instant) {
        match update {
            Update::Status(s) => {
                self.connection = Connection::Connected;
                self.status = Some(s);
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

/// Seconds to `m:ss`, e.g. `276.4` becomes `"4:36"`.
pub fn format_time(secs: f64) -> String {
    let total = secs.max(0.0) as u64; // `as` converts types; drops the decimals
    format!("{}:{:02}", total / 60, total % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::{Repeat, Track};

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

    fn connected_app(now: Instant) -> App {
        let mut app = App::new(now);
        app.apply(Update::Status(status(PlayState::Playing, 0.0, 100.0)), now);
        app
    }

    #[test]
    fn playback_keys_map_to_commands_when_connected() {
        let mut app = connected_app(Instant::now());
        assert_eq!(app.handle_key(key(' ')), Some(Command::PlayPause));
        assert_eq!(app.handle_key(key('n')), Some(Command::Next));
        assert_eq!(app.handle_key(key('p')), Some(Command::Previous));
        assert_eq!(app.handle_key(key('x')), None);
        assert!(!app.should_quit);
    }

    #[test]
    fn playback_keys_do_nothing_when_music_is_closed() {
        let mut app = App::new(Instant::now());
        app.apply(Update::NotRunning, Instant::now());
        assert_eq!(app.handle_key(key(' ')), None);
        assert_eq!(app.handle_key(key('o')), Some(Command::LaunchMusic));
    }

    #[test]
    fn q_and_ctrl_c_quit() {
        let mut app = App::new(Instant::now());
        app.handle_key(key('q'));
        assert!(app.should_quit);

        let mut app = App::new(Instant::now());
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.should_quit);
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
        app.apply(Update::Status(status(PlayState::Playing, 10.0, 100.0)), now);
        let later = now + Duration::from_millis(2500);
        assert!((app.position(later) - 12.5).abs() < 1e-9);
    }

    #[test]
    fn position_is_frozen_while_paused() {
        let now = Instant::now();
        let mut app = App::new(now);
        app.apply(Update::Status(status(PlayState::Paused, 10.0, 100.0)), now);
        assert_eq!(app.position(now + Duration::from_secs(5)), 10.0);
    }

    #[test]
    fn position_never_passes_the_end() {
        let now = Instant::now();
        let mut app = App::new(now);
        app.apply(Update::Status(status(PlayState::Playing, 99.0, 100.0)), now);
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
