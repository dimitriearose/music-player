//! The application's state and what each key does.
//!
//! This file deliberately knows nothing about drawing or AppleScript. It's
//! plain logic, which makes it easy to unit-test: feed in keys and updates,
//! check the state that comes out.

use crate::art::Art;
use crate::library::Library;
use crate::music::{MusicError, PlayState, PlayerStatus, QueueInfo, Repeat, Song, Speaker};
use crate::popup::{self, Action, Data, Popup, Queueable, Row, Tab, View};
use crate::worker::{Command, Update};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
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

    // ── Milestones 3–6 ──
    /// The library pop-up, when it's open.
    pub popup: Option<Popup>,
    /// Your library, once the worker has loaded it.
    pub library: Option<Library>,
    /// Songs of playlists you've opened, by playlist ID. (A `HashMap` is a
    /// lookup table: give it a key, get the value back quickly.)
    pub playlist_songs: HashMap<String, Vec<Song>>,
    pub queue: Option<QueueInfo>,
    pub speakers: Option<Vec<Speaker>>,
    /// Artwork and the ID of the song it belongs to.
    pub art: Option<(String, Art)>,
    /// The `a` key toggles this.
    pub show_art: bool,
    /// Can the terminal show exact colours? (See `art::truecolor_supported`.)
    pub truecolor: bool,
    /// Seconds moved by one press of ← / → (settings: `seek_seconds`).
    pub seek_step: i32,
    /// Volume change for one press of + / - (settings: `volume_step`).
    pub volume_step: i32,
    /// Playlist names to pin to the top (settings: `favorites`).
    pub favorites: Vec<String>,
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
            popup: None,
            library: None,
            playlist_songs: HashMap::new(),
            queue: None,
            speakers: None,
            art: None,
            show_art: true,
            truecolor: false,
            seek_step: 5,
            volume_step: 5,
            favorites: Vec::new(),
        }
    }

    /// The artwork for the song playing now, if we have it and it's wanted.
    pub fn current_art(&self) -> Option<&Art> {
        if !self.show_art {
            return None;
        }
        let playing = self.status.as_ref()?.track.as_ref()?;
        match &self.art {
            Some((id, art)) if *id == playing.id => Some(art),
            _ => None,
        }
    }

    /// Bundle up what the pop-up needs to build its rows.
    pub fn data(&self) -> Data<'_> {
        Data {
            library: self.library.as_ref(),
            playlist_songs: &self.playlist_songs,
            queue: self.queue.as_ref(),
            speakers: self.speakers.as_deref(),
        }
    }

    /// Put a short notice on the bottom line.
    /// Take on your settings (see `config.rs`), and show the first problem
    /// with them, if any, on the bottom line.
    pub fn use_config(&mut self, loaded: &crate::config::Loaded, now: Instant) {
        let c = &loaded.config;
        self.show_art = c.show_art;
        self.seek_step = c.seek_seconds as i32;
        self.volume_step = c.volume_step as i32;
        self.favorites = c.favorites.clone();
        if let Some(problem) = loaded.problems.first() {
            self.notice(&format!("{problem} (see am --config)"), now);
        }
    }

    fn notice(&mut self, text: &str, now: Instant) {
        self.message = Some((text.to_string(), now));
    }

    /// Decide what a key press means. Returns a command for the worker, if any.
    ///
    /// When a command is returned, the screen is *also* updated straight away
    /// with what we expect to happen (e.g. the volume number goes up), so the
    /// app feels instant even though Music takes ~100 ms to respond.
    pub fn handle_key(&mut self, key: KeyEvent, now: Instant) -> Option<Command> {
        let cmd = self.command_for(key, now)?; // `?` on an Option: return None if None
        self.predict(&cmd, now);
        self.sent += 1;
        Some(cmd)
    }

    /// The key → command table. Keys that only change the screen (like `?`)
    /// are handled here too and return `None`.
    fn command_for(&mut self, key: KeyEvent, now: Instant) -> Option<Command> {
        // In "raw mode" the terminal no longer turns Ctrl-C into "stop the
        // program", so we handle it ourselves.
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return None;
        }

        // While the pop-up is open it gets every key.
        if self.popup.is_some() {
            return self.popup_key(key, now);
        }

        match key.code {
            KeyCode::Char('/') => {
                self.show_help = false;
                self.popup = Some(Popup::new());
                return None;
            }
            KeyCode::Char('a') => {
                self.show_art = !self.show_art;
                return None;
            }
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
            // Shift-S: shuffle your whole library.
            KeyCode::Char('S') if self.connection == Connection::Connected => {
                return match &self.library {
                    Some(lib) if !lib.songs.is_empty() => Some(Command::PlayShuffled {
                        ids: lib.songs.iter().map(|s| s.id.clone()).collect(),
                        source: None,
                    }),
                    _ => {
                        self.notice("Your library is still loading", now);
                        None
                    }
                };
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
            KeyCode::Left | KeyCode::Char('h') => Some(Command::Seek(-self.seek_step)),
            KeyCode::Right | KeyCode::Char('l') => Some(Command::Seek(self.seek_step)),
            // `=` is accepted too, so you don't need Shift for "+".
            KeyCode::Char('+') | KeyCode::Char('=') => Some(Command::SetVolume(
                (volume + self.volume_step).clamp(0, 100) as u8,
            )),
            KeyCode::Char('-') | KeyCode::Char('_') => Some(Command::SetVolume(
                (volume - self.volume_step).clamp(0, 100) as u8,
            )),
            KeyCode::Char('s') => Some(Command::ToggleShuffle),
            KeyCode::Char('r') => Some(Command::CycleRepeat),
            // 1–9: play that favorite playlist from the top.
            // `c @ '1'..='9'` matches any digit 1–9 and names it `c`.
            KeyCode::Char(c @ '1'..='9') => {
                let n = c as usize - '0' as usize;
                let id = self
                    .library
                    .as_ref()
                    .filter(|lib| n <= lib.favorites)
                    .map(|lib| lib.playlists[n - 1].id.clone());
                if id.is_none() {
                    self.notice(&format!("No favorite {n} (see am --config)"), now);
                }
                id.map(|id| Command::PlayPlaylist { id, index: 1 })
            }
            _ => None,
        }
    }

    /// Update our copy of the status to what `cmd` will do, before Music
    /// confirms it.
    fn predict(&mut self, cmd: &Command, now: Instant) {
        // Flip a speaker's dot straight away; the worker re-reads them after.
        if let Command::SetSpeaker { name, on } = cmd
            && let Some(speakers) = &mut self.speakers
        {
            for sp in speakers.iter_mut().filter(|sp| &sp.name == name) {
                sp.selected = *on;
            }
        }

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
            // Everything else: we can't know the result in advance (e.g. the
            // next song), so we just wait for the worker to tell us.
            // Rust note: `_` matches all remaining cases.
            _ => {}
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
            Update::Notice(text) => self.notice(&text, now),
            Update::Library(mut lib) => {
                let missing = lib.pin(&self.favorites);
                if !missing.is_empty() {
                    self.notice(&format!("No playlist called: {}", missing.join(", ")), now);
                }
                self.library = Some(lib);
            }
            Update::PlaylistSongs { id, songs } => {
                self.playlist_songs.insert(id, songs);
            }
            Update::Queue(q) => self.queue = Some(q),
            Update::Speakers(s) => self.speakers = Some(s),
            Update::Art { track_id, art } => {
                // `map` only runs if there *is* art: Some(art) → Some((id, art)).
                self.art = art.map(|a| (track_id, a));
            }
        }
    }

    /// Keys while the library pop-up is open.
    ///
    /// In the Search tab's main list, letters type into the search box. Everywhere
    /// else they're shortcuts (j/k to move, . to play next, …). Ctrl
    /// combinations work everywhere.
    fn popup_key(&mut self, key: KeyEvent, now: Instant) -> Option<Command> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        // Build the rows first, then take what we need from them, so we're not
        // still borrowing `self` when we change it below.
        let rows = popup::rows(self.popup.as_ref()?, &self.data());
        let last = rows.len().saturating_sub(1);
        let (typing, selected) = {
            let p = self.popup.as_ref()?;
            (p.typing(), p.selected.min(last))
        };
        let row: Option<Row> = rows.get(selected).cloned();

        // Queue keys: ctrl-n / . = play next, ctrl-a / Q = add to the end.
        let queue_next = match key.code {
            KeyCode::Char('n') if ctrl => Some(true),
            KeyCode::Char('a') if ctrl => Some(false),
            KeyCode::Char('.') if !typing => Some(true),
            KeyCode::Char('Q') if !typing => Some(false),
            _ => None,
        };
        if let Some(next) = queue_next {
            let row_queue = row.map(|r| r.queue);
            let ids = row_queue
                .as_ref()
                .map(|q| self.resolve_queue(q))
                .unwrap_or_default();
            if ids.is_empty() {
                self.notice("Nothing to queue here (open a playlist first)", now);
                return None;
            }
            // Songs picked inside a playlist (or a whole playlist) can be
            // copied from that playlist if they aren't in your library.
            let source = match (&row_queue, &self.popup.as_ref()?.view) {
                (Some(Queueable::Playlist(id)), _) => Some(id.clone()),
                (_, View::Playlist { id, .. }) => Some(id.clone()),
                _ => None,
            };
            return Some(Command::Enqueue { ids, next, source });
        }

        // Shuffle: Shift-S (or ctrl-s while typing). Inside an album, artist
        // or playlist it shuffles all of it; on a list it shuffles the row.
        let shuffle_key = match key.code {
            KeyCode::Char('s') if ctrl => true,
            KeyCode::Char('S') if !typing => true,
            _ => false,
        };
        if shuffle_key {
            let view = self.popup.as_ref()?.view.clone();
            let target = match &view {
                View::Album(k) => Queueable::Album(k.clone()),
                View::Artist(name) => Queueable::Artist(name.clone()),
                View::Playlist { id, .. } => Queueable::Playlist(id.clone()),
                View::List => row.map(|r| r.queue).unwrap_or(Queueable::Nothing),
            };
            let source = match &target {
                Queueable::Playlist(id) => Some(id.clone()),
                _ => None,
            };
            // A playlist you haven't opened yet has no songs here; the
            // worker fetches them (empty `ids` + a `source`).
            let ids = self.resolve_queue(&target);
            if ids.is_empty() && source.is_none() {
                self.notice("Nothing to shuffle here", now);
                return None;
            }
            self.popup = None; // back to the quiet screen, like Enter
            return Some(Command::PlayShuffled { ids, source });
        }

        let enter = match key.code {
            KeyCode::Enter => true,
            KeyCode::Right | KeyCode::Char('l') if !typing => true,
            _ => false,
        };
        if enter {
            return row.and_then(|r| self.activate(r.action));
        }

        let p = self.popup.as_mut()?;
        match key.code {
            KeyCode::Char('r') if ctrl => return Some(Command::LoadLibrary),
            KeyCode::Esc => {
                if p.view == View::List {
                    self.popup = None;
                } else {
                    p.view = View::List;
                    p.selected = 0;
                }
            }
            KeyCode::Tab | KeyCode::BackTab => {
                p.tab = if key.code == KeyCode::Tab {
                    p.tab.next()
                } else {
                    p.tab.prev()
                };
                p.view = View::List;
                p.selected = 0;
                // Queue and speakers can change at any time, so re-read them
                // every time you visit the tab.
                return match p.tab {
                    Tab::Queue => Some(Command::LoadQueue),
                    Tab::Speakers => Some(Command::LoadSpeakers),
                    _ => None,
                };
            }
            KeyCode::Up => p.selected = selected.saturating_sub(1),
            KeyCode::Down => p.selected = (selected + 1).min(last),
            KeyCode::PageUp => p.selected = selected.saturating_sub(10),
            KeyCode::PageDown => p.selected = (selected + 10).min(last),
            KeyCode::Backspace if typing => {
                p.query.pop();
                p.selected = 0;
            }
            KeyCode::Char(c) if typing && !ctrl => {
                p.query.push(c);
                p.selected = 0;
            }
            // From here on: browsing (not typing).
            KeyCode::Char('k') => p.selected = selected.saturating_sub(1),
            KeyCode::Char('j') => p.selected = (selected + 1).min(last),
            KeyCode::Char('g') => p.selected = 0,
            KeyCode::Char('G') => p.selected = last,
            KeyCode::Backspace | KeyCode::Left | KeyCode::Char('h') => {
                if p.view != View::List {
                    p.view = View::List;
                    p.selected = 0;
                }
            }
            KeyCode::Char('q') => self.popup = None,
            _ => {}
        }
        None
    }

    /// Do what Enter means for a row.
    fn activate(&mut self, action: Action) -> Option<Command> {
        match action {
            Action::Open(view) => {
                // A playlist's songs are fetched the first time you open it.
                let load = match &view {
                    View::Playlist { id, .. } if !self.playlist_songs.contains_key(id) => {
                        Some(Command::LoadPlaylist(id.clone()))
                    }
                    _ => None,
                };
                if let Some(p) = &mut self.popup {
                    p.view = view;
                    p.selected = 0;
                }
                load
            }
            // Playing something closes the pop-up: back to the quiet screen.
            Action::PlaySongs { ids, start } => {
                self.popup = None;
                Some(Command::PlaySongs { ids, start })
            }
            Action::PlayPlaylist { id, index } => {
                self.popup = None;
                Some(Command::PlayPlaylist { id, index })
            }
            Action::PlayQueueAt(index) => {
                self.popup = None;
                Some(Command::PlayQueueAt(index))
            }
            // Speakers stay open, so you can switch several on or off.
            Action::ToggleSpeaker { name, on } => Some(Command::SetSpeaker { name, on }),
            Action::Nothing => None,
        }
    }

    /// The song IDs a row stands for when queueing.
    fn resolve_queue(&self, q: &Queueable) -> Vec<String> {
        let ids = |songs: Vec<Song>| -> Vec<String> { songs.into_iter().map(|s| s.id).collect() };
        match (q, &self.library) {
            (Queueable::Songs(ids), _) => ids.clone(),
            (Queueable::Album(key), Some(lib)) => ids(lib.album_songs(key)),
            (Queueable::Artist(name), Some(lib)) => ids(lib.artist_songs(name)),
            (Queueable::Playlist(id), _) => self
                .playlist_songs
                .get(id)
                .cloned()
                .map(ids)
                .unwrap_or_default(),
            _ => Vec::new(),
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

    /// Simulate typing a string, one key at a time.
    fn type_text(app: &mut App, text: &str, now: Instant) {
        for c in text.chars() {
            app.handle_key(key(c), now);
        }
    }

    fn app_with_library(now: Instant) -> App {
        let mut app = connected_app(now);
        let songs = vec![
            Song {
                id: "A1".into(),
                name: "Alpha".into(),
                artist: "Band".into(),
                album: "Record".into(),
                album_artist: String::new(),
                duration: 100.0,
                disc: 1,
                number: 1,
            },
            Song {
                id: "B2".into(),
                name: "Beta".into(),
                artist: "Band".into(),
                album: "Record".into(),
                album_artist: String::new(),
                duration: 100.0,
                disc: 1,
                number: 2,
            },
        ];
        app.apply(Update::Library(Library::new(songs, vec![])), now);
        app
    }

    #[test]
    fn slash_opens_search_and_letters_type() {
        let now = Instant::now();
        let mut app = app_with_library(now);
        app.handle_key(key('/'), now);
        // While typing, `n` and space are letters, not next/play-pause.
        type_text(&mut app, "n b", now);
        assert_eq!(app.popup.as_ref().unwrap().query, "n b");
        app.handle_key(KeyEvent::from(KeyCode::Backspace), now);
        assert_eq!(app.popup.as_ref().unwrap().query, "n ");
    }

    #[test]
    fn enter_on_a_search_result_plays_and_closes() {
        let now = Instant::now();
        let mut app = app_with_library(now);
        app.handle_key(key('/'), now);
        type_text(&mut app, "beta", now);
        let cmd = app.handle_key(KeyEvent::from(KeyCode::Enter), now);
        assert_eq!(
            cmd,
            Some(Command::PlaySongs {
                ids: vec!["B2".into()],
                start: 1
            })
        );
        assert!(app.popup.is_none(), "pop-up closes after playing");
    }

    #[test]
    fn opening_an_album_then_queueing_a_song() {
        let now = Instant::now();
        let mut app = app_with_library(now);
        app.handle_key(key('/'), now);
        app.handle_key(KeyEvent::from(KeyCode::Tab), now); // Playlists
        app.handle_key(KeyEvent::from(KeyCode::Tab), now); // Albums
        app.handle_key(KeyEvent::from(KeyCode::Enter), now); // open "Record"
        assert!(matches!(app.popup.as_ref().unwrap().view, View::Album(_)));

        app.handle_key(key('j'), now); // second song
        assert_eq!(
            app.handle_key(key('.'), now),
            Some(Command::Enqueue {
                ids: vec!["B2".into()],
                next: true,
                source: None,
            })
        );
        // Esc goes back to the album list, then closes.
        app.handle_key(KeyEvent::from(KeyCode::Esc), now);
        assert_eq!(app.popup.as_ref().unwrap().view, View::List);
        app.handle_key(KeyEvent::from(KeyCode::Esc), now);
        assert!(app.popup.is_none());
    }

    #[test]
    fn ctrl_a_on_an_album_queues_the_whole_album() {
        let now = Instant::now();
        let mut app = app_with_library(now);
        app.handle_key(key('/'), now);
        app.handle_key(KeyEvent::from(KeyCode::BackTab), now); // Speakers
        app.handle_key(KeyEvent::from(KeyCode::BackTab), now); // Queue
        app.handle_key(KeyEvent::from(KeyCode::BackTab), now); // Artists
        app.handle_key(KeyEvent::from(KeyCode::BackTab), now); // Albums
        let ctrl_a = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL);
        assert_eq!(
            app.handle_key(ctrl_a, now),
            Some(Command::Enqueue {
                ids: vec!["A1".into(), "B2".into()],
                next: false,
                source: None,
            })
        );
    }

    #[test]
    fn songs_queued_from_inside_a_playlist_name_it_as_the_source() {
        let now = Instant::now();
        let mut app = app_with_library(now);
        app.apply(
            Update::PlaylistSongs {
                id: "P9".into(),
                songs: app.library.as_ref().unwrap().songs.clone(),
            },
            now,
        );
        app.popup = Some(Popup {
            tab: Tab::Playlists,
            view: View::Playlist {
                id: "P9".into(),
                name: "Favorite Songs".into(),
            },
            ..Popup::new()
        });
        assert_eq!(
            app.handle_key(key('.'), now),
            Some(Command::Enqueue {
                ids: vec!["A1".into()],
                next: true,
                source: Some("P9".into()),
            })
        );
    }

    #[test]
    fn shift_s_shuffles_the_whole_library() {
        let now = Instant::now();
        let mut app = app_with_library(now);
        assert_eq!(
            app.handle_key(key('S'), now),
            Some(Command::PlayShuffled {
                ids: vec!["A1".into(), "B2".into()],
                source: None,
            })
        );
    }

    #[test]
    fn shift_s_inside_a_playlist_shuffles_all_of_it_and_closes() {
        let now = Instant::now();
        let mut app = app_with_library(now);
        app.apply(
            Update::PlaylistSongs {
                id: "P9".into(),
                songs: app.library.as_ref().unwrap().songs.clone(),
            },
            now,
        );
        app.popup = Some(Popup {
            tab: Tab::Playlists,
            view: View::Playlist {
                id: "P9".into(),
                name: "Favorite Songs".into(),
            },
            selected: 1,
            ..Popup::new()
        });
        assert_eq!(
            app.handle_key(key('S'), now),
            Some(Command::PlayShuffled {
                ids: vec!["A1".into(), "B2".into()],
                source: Some("P9".into()),
            })
        );
        assert!(app.popup.is_none());
    }

    #[test]
    fn shuffling_an_unopened_playlist_lets_the_worker_fetch_it() {
        let now = Instant::now();
        let mut app = connected_app(now);
        let playlist = crate::music::Playlist {
            id: "P9".into(),
            name: "Mix".into(),
            apple_music: true,
        };
        app.apply(Update::Library(Library::new(vec![], vec![playlist])), now);
        app.popup = Some(Popup {
            tab: Tab::Playlists,
            ..Popup::new()
        });
        assert_eq!(
            app.handle_key(key('S'), now),
            Some(Command::PlayShuffled {
                ids: vec![],
                source: Some("P9".into()),
            })
        );
    }

    #[test]
    fn capital_s_types_in_the_search_box_but_ctrl_s_shuffles() {
        let now = Instant::now();
        let mut app = app_with_library(now);
        app.handle_key(key('/'), now);
        type_text(&mut app, "AlphS", now);
        assert_eq!(app.popup.as_ref().unwrap().query, "AlphS");
        app.handle_key(KeyEvent::from(KeyCode::Backspace), now);
        let ctrl_s = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
        assert_eq!(
            app.handle_key(ctrl_s, now),
            Some(Command::PlayShuffled {
                ids: vec!["A1".into()],
                source: None,
            })
        );
    }

    fn app_with_favorites(now: Instant) -> App {
        let mut app = connected_app(now);
        app.favorites = vec!["Lo-Fi Japan".into(), "ease".into(), "gone".into()];
        let p = |id: &str, name: &str| crate::music::Playlist {
            id: id.into(),
            name: name.into(),
            apple_music: false,
        };
        app.apply(
            Update::Library(Library::new(
                vec![],
                vec![p("E1", "ease"), p("L2", "Lo-Fi Japan"), p("X3", "Other")],
            )),
            now,
        );
        app
    }

    #[test]
    fn favorites_are_pinned_and_typos_reported() {
        let now = Instant::now();
        let app = app_with_favorites(now);
        let lib = app.library.as_ref().unwrap();
        let names: Vec<&str> = lib.playlists.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Lo-Fi Japan", "ease", "Other"]);
        assert_eq!(app.message(now), Some("No playlist called: gone"));
    }

    #[test]
    fn number_keys_play_favorites() {
        let now = Instant::now();
        let mut app = app_with_favorites(now);
        assert_eq!(
            app.handle_key(key('2'), now),
            Some(Command::PlayPlaylist {
                id: "E1".into(),
                index: 1
            })
        );
        assert_eq!(app.handle_key(key('3'), now), None, "only 2 favorites");
        assert_eq!(app.message(now), Some("No favorite 3 (see am --config)"));
    }

    #[test]
    fn settings_change_steps_and_art() {
        let now = Instant::now();
        let mut app = connected_app(now);
        let loaded = crate::config::Loaded {
            config: crate::config::Config {
                favorites: vec![],
                show_art: false,
                seek_seconds: 15,
                volume_step: 10,
            },
            path: None,
            file_found: false,
            problems: vec![],
        };
        app.use_config(&loaded, now);
        assert!(!app.show_art);
        assert_eq!(app.handle_key(key('l'), now), Some(Command::Seek(15)));
        let vol = app.status.as_ref().unwrap().volume;
        assert_eq!(
            app.handle_key(key('-'), now),
            Some(Command::SetVolume(vol - 10))
        );
    }

    #[test]
    fn visiting_queue_and_speaker_tabs_refreshes_them() {
        let now = Instant::now();
        let mut app = app_with_library(now);
        app.handle_key(key('/'), now);
        let tab = || KeyEvent::from(KeyCode::Tab);
        for _ in 0..3 {
            app.handle_key(tab(), now); // Playlists, Albums, Artists
        }
        assert_eq!(app.handle_key(tab(), now), Some(Command::LoadQueue));
        assert_eq!(app.handle_key(tab(), now), Some(Command::LoadSpeakers));
    }

    #[test]
    fn toggling_a_speaker_flips_its_dot_immediately() {
        let now = Instant::now();
        let mut app = connected_app(now);
        app.apply(
            Update::Speakers(vec![Speaker {
                name: "Kitchen".into(),
                kind: "HomePod".into(),
                selected: false,
                available: true,
            }]),
            now,
        );
        app.popup = Some(Popup {
            tab: Tab::Speakers,
            ..Popup::new()
        });
        let cmd = app.handle_key(KeyEvent::from(KeyCode::Enter), now);
        assert_eq!(
            cmd,
            Some(Command::SetSpeaker {
                name: "Kitchen".into(),
                on: true
            })
        );
        assert!(app.speakers.as_ref().unwrap()[0].selected);
        assert!(app.popup.is_some(), "speakers tab stays open");
    }

    #[test]
    fn art_only_shows_for_the_current_song_and_can_be_hidden() {
        let now = Instant::now();
        let mut app = connected_app(now); // playing track id "X"
        let art = Art {
            size: 1,
            pixels: vec![[0, 0, 0]],
        };
        app.apply(
            Update::Art {
                track_id: "OTHER".into(),
                art: Some(art.clone()),
            },
            now,
        );
        assert!(app.current_art().is_none(), "art for a different song");
        app.apply(
            Update::Art {
                track_id: "X".into(),
                art: Some(art),
            },
            now,
        );
        assert!(app.current_art().is_some());
        app.handle_key(key('a'), now);
        assert!(app.current_art().is_none(), "hidden with `a`");
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
