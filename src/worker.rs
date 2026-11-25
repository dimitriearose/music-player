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

use crate::art::{self, ART_SIZE, Art};
use crate::library::Library;
use crate::music::queue::{MAX_QUEUE, Queued};
use crate::music::{Music, MusicError, PlayerStatus, QueueInfo, Song, Speaker, StatusReply};
use crate::shuffle;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

/// How often we ask Music.app for its status when nothing else is happening.
pub const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Pause after running commands before polling again (see `run`).
const SETTLE: Duration = Duration::from_millis(300);

/// How long we keep saying "opening Music…" after launching it, before
/// admitting it didn't start.
const LAUNCH_GRACE: Duration = Duration::from_secs(10);

/// Requests from the UI to the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    PlayPause,
    /// Pause (and only pause): what the sleep timer sends.
    Pause,
    Next,
    Previous,
    /// Jump forwards/backwards by this many seconds.
    Seek(i32),
    /// Set the volume to exactly this value (0–100).
    SetVolume(u8),
    ToggleShuffle,
    CycleRepeat,
    /// ♥ / un-♥ the current song.
    ToggleFavorite,
    /// Start Music.app hidden (the `o` key).
    LaunchMusic,
    /// (Re)load the whole library.
    LoadLibrary,
    /// Fetch one playlist's songs (by persistent ID).
    LoadPlaylist(String),
    /// Play a saved playlist from song `index` (1 = first).
    PlayPlaylist {
        id: String,
        index: usize,
    },
    /// Play these songs in order, starting at `start` (1 = first), via the
    /// queue playlist.
    PlaySongs {
        ids: Vec<String>,
        start: usize,
    },
    /// Add songs after the current one (`next: true`) or at the end.
    Enqueue {
        ids: Vec<String>,
        next: bool,
        /// The playlist the songs were picked from, if any (see
        /// `Music::enqueue`).
        source: Option<String>,
    },
    /// Shuffle these songs and play them (DESIGN.md §12). If `ids` is
    /// empty and `source` names a playlist, the worker fetches that
    /// playlist's songs first (you can shuffle one without opening it).
    PlayShuffled {
        ids: Vec<String>,
        source: Option<String>,
    },
    LoadQueue,
    /// Jump to song `index` (1 = first) of the queue.
    PlayQueueAt(usize),
    /// Add the song that's playing to one of your playlists.
    AddCurrentTo {
        id: String,
        name: String,
    },
    LoadSpeakers,
    SetSpeaker {
        name: String,
        on: bool,
    },
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
    /// A short message for the bottom line ("Added to queue").
    Notice(String),
    Library(Library),
    PlaylistSongs {
        id: String,
        songs: Vec<Song>,
    },
    Queue(QueueInfo),
    Speakers(Vec<Speaker>),
    /// The Mac's current sound output device, if found.
    MacOutput(Option<String>),
    /// Artwork for a song (`None` if it has none).
    Art {
        track_id: String,
        art: Option<Art>,
    },
}

/// Start the worker thread. Returns the two channel ends the UI needs:
/// one to send commands, one to receive updates.
pub fn spawn(music: Music) -> (Sender<Command>, Receiver<Update>) {
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (upd_tx, upd_rx) = mpsc::channel();

    // Rust note: `move` hands ownership of `music`, `cmd_rx` and `upd_tx` to the
    // new thread. Afterwards the main thread can't touch them, and the compiler
    // enforces this. That's how Rust prevents two threads fighting over data.
    thread::spawn(move || Worker::new(music, upd_tx).run(cmd_rx));

    (cmd_tx, upd_rx)
}

/// The worker's memory between loop rounds.
struct Worker {
    music: Music,
    updates: Sender<Update>,
    /// When we last launched Music (if we're still waiting for it to start).
    launched_at: Option<Instant>,
    first_poll: bool,
    /// How many commands we've finished. `u64` is a 64-bit unsigned number;
    /// it will never overflow in practice.
    acked: u64,
    library_loaded: bool,
    /// The song we last fetched artwork for, so we only fetch on a change.
    art_track: Option<String>,
}

impl Worker {
    fn new(music: Music, updates: Sender<Update>) -> Self {
        Worker {
            music,
            updates,
            launched_at: None,
            first_poll: true,
            acked: 0,
            library_loaded: false,
            art_track: None,
        }
    }

    /// Send an update to the UI. Returns `false` if the UI has gone away.
    fn send(&self, update: Update) -> bool {
        self.updates.send(update).is_ok()
    }

    /// The main loop: poll, wait for a command or the next poll, repeat.
    fn run(mut self, commands: Receiver<Command>) {
        loop {
            // 1. Ask Music what's going on and tell the UI.
            if !self.poll() {
                return; // UI gone: the app is quitting
            }

            // 2. Wait up to POLL_INTERVAL for a command. If none arrives,
            //    loop round and poll again.
            let first = match commands.recv_timeout(POLL_INTERVAL) {
                Ok(cmd) => cmd,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return,
            };

            // 3. Run that command, then any others that piled up meanwhile
            //    (e.g. you're holding `+`), *before* polling. Polling between
            //    each one would make held keys lag behind.
            //
            //    `std::iter::once(first).chain(commands.try_iter())` means "the
            //    first command, followed by every command already waiting".
            for cmd in std::iter::once(first).chain(commands.try_iter()) {
                if let Err(e) = self.execute(cmd) {
                    self.send(Update::Error(e));
                }
                self.acked += 1;
            }

            // 4. Give Music a moment before the next poll. Found by testing:
            //    right after a change (shuffle, repeat, next…) Music still
            //    reports the *old* value for a fraction of a second.
            thread::sleep(SETTLE);
        }
    }

    /// Fetch the status (plus the library and artwork when needed).
    fn poll(&mut self) -> bool {
        let update = match self.music.status() {
            Ok(StatusReply::Status(status)) => {
                self.launched_at = None;
                let track_id = status.track.as_ref().map(|t| t.id.clone());
                let sent = self.send(Update::Status {
                    status,
                    acked: self.acked,
                });
                // First successful contact: load the library in the
                // background so it's ready when you press `/`.
                if !self.library_loaded {
                    self.library_loaded = true;
                    self.load_library();
                }
                self.refresh_art(track_id);
                return sent;
            }
            Ok(StatusReply::NotRunning) | Err(MusicError::NotRunning) => {
                if self.first_poll {
                    // DESIGN.md §6.2: at startup, open Music hidden for you.
                    self.launch();
                }
                match self.launched_at {
                    Some(t) if t.elapsed() < LAUNCH_GRACE => Update::Launching,
                    _ => Update::NotRunning,
                }
            }
            Err(e) => Update::Error(e),
        };
        self.first_poll = false;
        self.send(update)
    }

    /// Launch Music hidden; report a failure to the UI.
    fn launch(&mut self) {
        self.send(Update::Launching);
        if let Err(e) = self.music.launch_hidden() {
            self.send(Update::Error(e));
        }
        self.launched_at = Some(Instant::now());
    }

    fn load_library(&self) {
        match self.music.library() {
            Ok((songs, playlists)) => self.send(Update::Library(Library::new(songs, playlists))),
            Err(e) => self.send(Update::Error(e)),
        };
    }

    /// If the song changed, fetch its artwork (DESIGN.md §6.4).
    fn refresh_art(&mut self, track_id: Option<String>) {
        if track_id == self.art_track {
            return;
        }
        self.art_track = track_id.clone();
        let Some(track_id) = track_id else { return };
        // A new song: re-read the queue too, for the "next" line.
        self.send_queue();
        let art = self.fetch_art();
        self.send(Update::Art { track_id, art });
    }

    /// Ask Music to write the artwork to a temp file, then decode it.
    /// Any failure just means "no art": it's decoration, not worth an error.
    fn fetch_art(&self) -> Option<Art> {
        // `temp_dir()` is your private per-user temp folder ($TMPDIR), never
        // inside the project, so artwork can't end up in git.
        let path = std::env::temp_dir().join(format!("am-artwork-{}", std::process::id()));
        let path_str = path.to_str()?;
        let saved = self.music.save_artwork(path_str).unwrap_or(false);
        let art = if saved {
            std::fs::read(&path)
                .ok()
                .and_then(|bytes| art::decode(&bytes, ART_SIZE))
        } else {
            None
        };
        let _ = std::fs::remove_file(&path); // tidy up; ignore "not found"
        art
    }

    /// Carry out one command.
    fn execute(&mut self, cmd: Command) -> Result<(), MusicError> {
        let m = &self.music;
        match cmd {
            Command::PlayPause => m.play_pause(),
            Command::Pause => m.pause(),
            Command::Next => m.next(),
            Command::Previous => m.previous(),
            Command::Seek(seconds) => m.seek_by(seconds),
            Command::SetVolume(v) => m.set_volume(v),
            Command::ToggleShuffle => m.toggle_shuffle(),
            Command::CycleRepeat => m.cycle_repeat(),
            Command::ToggleFavorite => m.toggle_favorite(),
            Command::LaunchMusic => {
                self.launch();
                Ok(())
            }
            Command::LoadLibrary => {
                self.load_library();
                self.send(Update::Notice("Library reloaded".into()));
                Ok(())
            }
            Command::LoadPlaylist(id) => {
                let songs = m.playlist_songs(&id)?;
                self.send(Update::PlaylistSongs { id, songs });
                Ok(())
            }
            Command::PlayPlaylist { id, index } => m.play_playlist(&id, index),
            Command::PlaySongs { ids, start } => {
                m.play_songs(&ids, start, None, Some(false))?;
                self.send_queue();
                Ok(())
            }
            Command::Enqueue { ids, next, source } => {
                let count = ids.len();
                let what = if count == 1 {
                    "1 song".to_string()
                } else {
                    format!("{count} songs")
                };
                let notice = match m.enqueue(&ids, next, source.as_deref())? {
                    Queued::Added if next => format!("Playing next: {what}"),
                    Queued::Added => format!("Added to queue: {what}"),
                    Queued::Started => format!("Queue started; {what} queued after this song"),
                    Queued::Ready => format!("Queued {what}; start it from the Queue tab"),
                };
                self.send(Update::Notice(notice));
                self.send_queue();
                Ok(())
            }
            Command::PlayShuffled { mut ids, source } => {
                if ids.is_empty()
                    && let Some(id) = &source
                {
                    ids = m.playlist_songs(id)?.into_iter().map(|s| s.id).collect();
                }
                if ids.is_empty() {
                    self.send(Update::Notice("Nothing to shuffle here".into()));
                    return Ok(());
                }
                shuffle::shuffle(&mut ids, shuffle::seed());
                m.play_songs(&ids, 1, source.as_deref(), Some(true))?;
                let notice = if ids.len() > MAX_QUEUE {
                    format!("Shuffling {MAX_QUEUE} random songs of {}", ids.len())
                } else {
                    format!("Shuffling {} songs", ids.len())
                };
                self.send(Update::Notice(notice));
                self.send_queue();
                Ok(())
            }
            Command::LoadQueue => {
                self.send_queue();
                Ok(())
            }
            Command::PlayQueueAt(index) => {
                m.play_queue_at(index)?;
                self.send_queue();
                Ok(())
            }
            Command::AddCurrentTo { id, name } => {
                let notice = if m.add_current_to(&id)? {
                    format!("Added to {name}")
                } else {
                    format!("Already in {name}")
                };
                self.send(Update::Notice(notice));
                Ok(())
            }
            Command::LoadSpeakers => {
                let speakers = m.speakers()?;
                self.send(Update::Speakers(speakers));
                // Slower (~1.5 s), so it follows once the list is showing.
                self.send(Update::MacOutput(crate::music::mac_output()));
                Ok(())
            }
            Command::SetSpeaker { name, on } => {
                m.set_speaker(&name, on)?;
                let speakers = m.speakers()?;
                self.send(Update::Speakers(speakers));
                Ok(())
            }
        }
    }

    /// Fetch the queue and send it to the UI (errors are reported, not fatal).
    fn send_queue(&self) {
        match self.music.queue() {
            Ok(q) => self.send(Update::Queue(q)),
            Err(e) => self.send(Update::Error(e)),
        };
    }
}
