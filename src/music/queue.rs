//! Library browsing, the queue, AirPlay and artwork: the `Music` methods
//! added in milestones 3–6.
//!
//! Rust note: you can write several `impl Music { ... }` blocks in different
//! files. They all add methods to the same type. This keeps `mod.rs` short.

use super::scripts::{self, OLD_QUEUE_NAME, QUEUE_NAME};
use super::{
    Music, MusicError, PlayState, Playlist, QueueInfo, Song, Speaker, StatusReply, osascript, parse,
};

/// What happened when songs were queued, for the notice on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Queued {
    /// Added to the queue we were already playing from.
    Added,
    /// We weren't playing from the queue, so it was started with the current
    /// song first (picking up where it was).
    Started,
    /// Nothing was playing; the queue is ready but not started.
    Ready,
}

/// The queue never holds more than this many songs: filling it takes about
/// 10 ms per song (measured), so 200 songs ≈ 2 seconds.
pub const MAX_QUEUE: usize = 200;

/// The first `MAX_QUEUE` items of a list.
fn capped(ids: &[String]) -> &[String] {
    &ids[..ids.len().min(MAX_QUEUE)]
}

/// The new queue: the current song (if any), then either the new songs
/// followed by what was coming up (`next`), or the other way round.
pub fn queue_order(
    current: Option<String>,
    upcoming: &[String],
    new: &[String],
    next: bool,
) -> Vec<String> {
    // `into_iter()` on an Option yields 0 or 1 items: a neat way to say
    // "the current song, if there is one".
    let mut order: Vec<String> = current.into_iter().collect();
    let (first, second) = if next {
        (new, upcoming)
    } else {
        (upcoming, new)
    };
    order.extend_from_slice(first);
    order.extend_from_slice(second);
    order.truncate(MAX_QUEUE);
    order
}

/// Persistent IDs are hex strings like `8A2F3C...`. We check before pasting
/// them into a script, so a weird value can never become AppleScript code.
fn check_ids(ids: &[String]) -> Result<(), MusicError> {
    let ok = |id: &String| !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit());
    if ids.iter().all(ok) {
        Ok(())
    } else {
        Err(MusicError::Script("invalid song or playlist ID".into()))
    }
}

impl Music {
    /// Every song in your library plus your playlists. Takes about a second.
    pub fn library(&self) -> Result<(Vec<Song>, Vec<Playlist>), MusicError> {
        let songs =
            parse::parse_songs(&osascript::run(&scripts::tracks_of("library playlist 1"))?)?;
        let mut playlists = parse::parse_playlists(&osascript::run(scripts::PLAYLISTS)?)?;
        // Our queue playlist is an implementation detail: hide it.
        playlists.retain(|p| p.name != QUEUE_NAME && p.name != OLD_QUEUE_NAME);
        Ok((songs, playlists))
    }

    /// The songs in one playlist, in playlist order.
    pub fn playlist_songs(&self, id: &str) -> Result<Vec<Song>, MusicError> {
        check_ids(&[id.to_string()])?;
        parse::parse_songs(&osascript::run(&scripts::tracks_of(
            &scripts::playlist_ref(id),
        ))?)
    }

    /// Play a saved playlist starting at `index` (1 = first song).
    pub fn play_playlist(&self, id: &str, index: usize) -> Result<(), MusicError> {
        check_ids(&[id.to_string()])?;
        osascript::run(&scripts::play_playlist_at(id, index)).map(|_| ())
    }

    /// Play `ids` in order starting at `start` (1 = first), via the queue.
    /// Used to play an album, an artist, or search results. Songs before
    /// `start` are left out: the queue only holds what's still to come.
    pub fn play_songs(&self, ids: &[String], start: usize) -> Result<(), MusicError> {
        check_ids(ids)?;
        let from = start.saturating_sub(1).min(ids.len());
        let songs = capped(&ids[from..]);
        osascript::run(&scripts::replace_queue(songs, None, true, None)).map(|_| ())
    }

    /// What's in the queue, and whether we're playing from it.
    pub fn queue(&self) -> Result<QueueInfo, MusicError> {
        let (active, current_id) =
            parse::parse_queue_state(&osascript::run(&scripts::queue_state())?);
        let songs = parse::parse_songs(&osascript::run(&scripts::queue_tracks())?)?;
        Ok(QueueInfo {
            active,
            current_id,
            songs,
        })
    }

    /// Jump to song `index` (1 = first) in the queue, dropping the songs
    /// before it.
    pub fn play_queue_at(&self, index: usize) -> Result<(), MusicError> {
        let ids: Vec<String> = self.queue()?.songs.into_iter().map(|s| s.id).collect();
        self.play_songs(&ids, index)
    }

    /// Put songs in the queue: right after the current song (`next = true`)
    /// or at the end (`next = false`).
    ///
    /// Music ignores changes to a playlist that's already playing (see
    /// `scripts::replace_queue`), so whenever something is playing we
    /// rebuild the queue as [current song, …new order…] and restart it at
    /// the same spot. You may hear a tiny skip; the song carries on.
    ///
    /// `source` is the playlist the songs were picked from, if any: songs
    /// that aren't in your library (e.g. liked Apple Music songs) are copied
    /// from there.
    pub fn enqueue(
        &self,
        ids: &[String],
        next: bool,
        source: Option<&str>,
    ) -> Result<Queued, MusicError> {
        check_ids(ids)?;
        if let Some(id) = source {
            check_ids(&[id.to_string()])?;
        }
        let queue = self.queue()?;

        // What's playing right now (only songs from your library can be
        // copied into the queue, hence the ID check).
        let now_playing = match self.status()? {
            StatusReply::Status(s) if s.state != PlayState::Stopped => s
                .track
                .filter(|t| check_ids(std::slice::from_ref(&t.id)).is_ok())
                .map(|t| (t.id, s.position, s.state == PlayState::Paused)),
            _ => None,
        };

        match now_playing {
            Some((current, position, paused)) => {
                // If we were already playing from the queue, keep what was
                // coming up after the current song.
                let upcoming: Vec<String> = match queue.current_index() {
                    Some(i) => queue.songs[i + 1..].iter().map(|s| s.id.clone()).collect(),
                    None => Vec::new(),
                };
                let order = queue_order(Some(current), &upcoming, ids, next);
                let script = scripts::replace_queue(&order, source, true, Some((position, paused)));
                osascript::run(&script)?;
                Ok(if queue.active {
                    Queued::Added
                } else {
                    Queued::Started
                })
            }
            None => {
                // Nothing playing: add to whatever is already queued, but
                // don't start it.
                let existing: Vec<String> = queue.songs.into_iter().map(|s| s.id).collect();
                let order = queue_order(None, &existing, ids, next);
                osascript::run(&scripts::replace_queue(&order, source, false, None))?;
                Ok(Queued::Ready)
            }
        }
    }

    /// All AirPlay outputs Music knows about.
    pub fn speakers(&self) -> Result<Vec<Speaker>, MusicError> {
        parse::parse_speakers(&osascript::run(scripts::SPEAKERS)?)
    }

    /// Turn one output on or off.
    pub fn set_speaker(&self, name: &str, on: bool) -> Result<(), MusicError> {
        osascript::run(&scripts::set_speaker(name, on)).map(|_| ())
    }

    /// Save the current song's artwork to `path`. `Ok(false)` = it has none.
    pub fn save_artwork(&self, path: &str) -> Result<bool, MusicError> {
        Ok(osascript::run(&scripts::save_artwork(path))? == "OK")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_must_be_hex() {
        assert!(check_ids(&["8A2F3C00DEADBEEF".into()]).is_ok());
        assert!(check_ids(&[]).is_ok());
        assert!(check_ids(&["".into()]).is_err());
        assert!(check_ids(&[r#"X" & do shell script "rm"#.into()]).is_err());
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn play_next_goes_straight_after_the_current_song() {
        let order = queue_order(Some("NOW".into()), &ids(&["U1", "U2"]), &ids(&["N"]), true);
        assert_eq!(order, ids(&["NOW", "N", "U1", "U2"]));
    }

    #[test]
    fn add_to_queue_goes_at_the_end() {
        let order = queue_order(
            Some("NOW".into()),
            &ids(&["U1"]),
            &ids(&["N1", "N2"]),
            false,
        );
        assert_eq!(order, ids(&["NOW", "U1", "N1", "N2"]));
        let order = queue_order(None, &ids(&["OLD"]), &ids(&["N"]), false);
        assert_eq!(order, ids(&["OLD", "N"]), "nothing playing: just append");
    }

    #[test]
    fn the_queue_is_capped() {
        let many: Vec<String> = (0..500).map(|i| format!("{i:X}")).collect();
        assert_eq!(queue_order(None, &[], &many, false).len(), MAX_QUEUE);
        assert_eq!(capped(&many).len(), MAX_QUEUE);
    }

    #[test]
    fn current_index_only_counts_when_playing_from_the_queue() {
        let song = |id: &str| Song {
            id: id.into(),
            name: String::new(),
            artist: String::new(),
            album: String::new(),
            album_artist: String::new(),
            duration: 0.0,
            disc: 1,
            number: 1,
        };
        let mut q = QueueInfo {
            active: true,
            current_id: Some("B".into()),
            songs: vec![song("A"), song("B"), song("C")],
        };
        assert_eq!(q.current_index(), Some(1));
        q.active = false;
        assert_eq!(q.current_index(), None);
    }
}
