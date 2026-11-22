//! Everything that talks to Music.app lives in this module.
//!
//! The rest of the program never sees AppleScript. It only sees the plain
//! Rust types defined here (`PlayerStatus`, `Track`, ...) and the methods on
//! [`Music`]. That boundary means we could later swap `osascript` for a faster
//! in-process approach (DESIGN.md §4, approach B) without touching the UI.
//!
//! Rust note: `pub mod x;` tells the compiler "there is a sub-module in
//! `music/x.rs`, and other code may use it". Without `pub` it would be private
//! to this module.

pub mod osascript;
pub mod parse;
pub mod queue;
pub mod scripts;

use thiserror::Error;

/// What the player is doing right now.
///
/// Rust note: `#[derive(...)]` asks the compiler to write some standard code
/// for us. `Debug` lets us print the value with `{:?}`, `Clone` lets us copy
/// it, and `PartialEq` lets tests compare two values with `==`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    Playing,
    Paused,
    Stopped,
}

/// Music.app's repeat setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
    Off,
    One,
    All,
}

/// The song that's currently loaded in the player.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    /// Music.app's "persistent ID". It stays the same for a song forever, so
    /// later milestones use it to notice "the track changed" (e.g. to fetch
    /// new album art).
    pub id: String,
    pub name: String,
    pub artist: String,
    pub album: String,
    /// Length in seconds. `f64` is a 64-bit decimal number.
    pub duration: f64,
}

/// One snapshot of the player, fetched by a single `osascript` call.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerStatus {
    pub state: PlayState,
    /// `Option<Track>` means "maybe a Track". It's `None` when nothing is
    /// loaded (player stopped). Rust has no `null`; `Option` is how you say
    /// "this might be missing", and the compiler makes you handle both cases.
    pub track: Option<Track>,
    /// Seconds into the current track at the moment of the fetch.
    pub position: f64,
    /// 0–100.
    pub volume: u8,
    pub shuffle: bool,
    pub repeat: Repeat,
}

/// One song in your library (or in a playlist / the queue).
///
/// This has more detail than `Track` (which describes only what's playing):
/// disc and track numbers let us sort an album into the right order.
#[derive(Debug, Clone, PartialEq)]
pub struct Song {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub album: String,
    /// The album's artist (e.g. "Various Artists"); often empty.
    pub album_artist: String,
    pub duration: f64,
    pub disc: u32,
    pub number: u32,
}

/// A saved playlist: just enough to list it and open it.
#[derive(Debug, Clone, PartialEq)]
pub struct Playlist {
    pub id: String,
    pub name: String,
}

/// An AirPlay output (your Mac's speakers count as one too).
#[derive(Debug, Clone, PartialEq)]
pub struct Speaker {
    pub name: String,
    /// e.g. "computer", "AirPlay TV", "HomePod".
    pub kind: String,
    /// Is Music currently playing through it?
    pub selected: bool,
    /// Can it be used right now?
    pub available: bool,
}

/// What's in `am`'s queue playlist and where we are in it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct QueueInfo {
    /// Is Music playing *from the queue playlist* right now?
    pub active: bool,
    /// The persistent ID of whatever is playing (anywhere), if anything.
    pub current_id: Option<String>,
    pub songs: Vec<Song>,
}

impl QueueInfo {
    /// Position (0-based) of the current song inside the queue, if we're
    /// playing from the queue.
    ///
    /// Rust note: `?` works on `Option` too: if `current_id` is `None`, the
    /// whole function returns `None` right there.
    pub fn current_index(&self) -> Option<usize> {
        if !self.active {
            return None;
        }
        let id = self.current_id.as_ref()?;
        self.songs.iter().position(|s| &s.id == id)
    }
}

/// The answer to "what's going on?". Music might not be running at all,
/// which is a normal situation, not an error, so it gets its own variant.
#[derive(Debug, Clone, PartialEq)]
pub enum StatusReply {
    NotRunning,
    Status(PlayerStatus),
}

/// Everything that can go wrong when talking to Music.app.
///
/// Rust note: functions that can fail return `Result<T, E>`: either
/// `Ok(value)` or `Err(error)`. This enum is our `E`. The `thiserror` crate's
/// `#[error("...")]` attribute writes the human-readable message for each
/// case so we can show it on screen.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum MusicError {
    #[error("Music isn't running")]
    NotRunning,
    #[error("Terminal isn't allowed to control Music (Automation permission)")]
    PermissionDenied,
    #[error("AppleScript error: {0}")]
    Script(String),
    #[error("unexpected reply from Music: {0}")]
    BadOutput(String),
    #[error("couldn't run osascript: {0}")]
    Io(String),
}

/// A handle for controlling Music.app.
///
/// It holds no data yet; it exists so the rest of the program has one obvious
/// thing to call (`music.next()`), and so later milestones have somewhere to
/// keep state (like a cache).
#[derive(Debug, Default, Clone)]
pub struct Music;

impl Music {
    pub fn new() -> Self {
        Music
    }

    /// Fetch the full player status in one round trip.
    ///
    /// Rust note: the `?` after a call means "if this returned `Err`, return
    /// that error from *this* function right now; otherwise unwrap the `Ok`
    /// value and keep going". It replaces a lot of `if err != nil` boilerplate.
    pub fn status(&self) -> Result<StatusReply, MusicError> {
        let raw = osascript::run(scripts::STATUS)?;
        parse::parse_status(&raw)
    }

    pub fn play_pause(&self) -> Result<(), MusicError> {
        osascript::run(scripts::PLAY_PAUSE).map(|_| ())
    }

    pub fn next(&self) -> Result<(), MusicError> {
        osascript::run(scripts::NEXT).map(|_| ())
    }

    pub fn previous(&self) -> Result<(), MusicError> {
        osascript::run(scripts::PREVIOUS).map(|_| ())
    }

    /// Jump forwards (positive) or backwards (negative) by whole seconds.
    pub fn seek_by(&self, seconds: i32) -> Result<(), MusicError> {
        osascript::run(&scripts::seek_by(seconds)).map(|_| ())
    }

    /// 0–100.
    pub fn set_volume(&self, volume: u8) -> Result<(), MusicError> {
        osascript::run(&scripts::set_volume(volume.min(100))).map(|_| ())
    }

    pub fn toggle_shuffle(&self) -> Result<(), MusicError> {
        osascript::run(scripts::TOGGLE_SHUFFLE).map(|_| ())
    }

    pub fn cycle_repeat(&self) -> Result<(), MusicError> {
        osascript::run(scripts::CYCLE_REPEAT).map(|_| ())
    }

    /// Start Music.app without bringing it to the front or showing its window.
    ///
    /// `open -g` = don't bring to foreground, `-j` = launch hidden,
    /// `-a Music` = the application to open. This uses the macOS `open`
    /// command rather than AppleScript, because any AppleScript
    /// `tell application "Music"` would launch it *visibly*.
    pub fn launch_hidden(&self) -> Result<(), MusicError> {
        let status = std::process::Command::new("/usr/bin/open")
            .args(["-g", "-j", "-a", "Music"])
            .status()
            .map_err(|e| MusicError::Io(e.to_string()))?;
        if status.success() {
            Ok(())
        } else {
            Err(MusicError::Script("`open -a Music` failed".into()))
        }
    }
}
