//! The library pop-up (`/`): which tab is open, what's typed, which row is
//! selected, and the rows each tab shows.
//!
//! Like `app.rs`, this is plain logic with no drawing, so it can be tested.
//! `ui/popup.rs` draws whatever `rows()` returns.

use crate::app::format_time;
use crate::library::{AlbumKey, Hit, Library};
use crate::music::{QueueInfo, Song, Speaker};
use std::collections::HashMap;

/// Most search results shown at once. More than a screenful is never useful.
pub const SEARCH_LIMIT: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Search,
    Playlists,
    Albums,
    Artists,
    Queue,
    Speakers,
}

impl Tab {
    /// In the order they appear along the top.
    pub const ALL: [Tab; 6] = [
        Tab::Search,
        Tab::Playlists,
        Tab::Albums,
        Tab::Artists,
        Tab::Queue,
        Tab::Speakers,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Search => "Search",
            Tab::Playlists => "Playlists",
            Tab::Albums => "Albums",
            Tab::Artists => "Artists",
            Tab::Queue => "Queue",
            Tab::Speakers => "Speakers",
        }
    }

    fn index(self) -> usize {
        // `position` finds where `self` is in the list. It's always there,
        // so `unwrap_or(0)` never actually uses the 0.
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    pub fn next(self) -> Tab {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Tab {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// What's shown inside a tab: its main list, or something you opened.
#[derive(Debug, Clone, PartialEq)]
pub enum View {
    List,
    Album(AlbumKey),
    Artist(String),
    Playlist { id: String, name: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Popup {
    pub tab: Tab,
    pub view: View,
    /// What's typed in the Search tab.
    pub query: String,
    /// Index of the highlighted row (clamped to the row count when used).
    pub selected: usize,
}

impl Popup {
    pub fn new() -> Self {
        Popup {
            tab: Tab::Search,
            view: View::List,
            query: String::new(),
            selected: 0,
        }
    }

    /// When true, letter keys type into the search box instead of acting
    /// as shortcuts.
    pub fn typing(&self) -> bool {
        self.tab == Tab::Search && self.view == View::List
    }

    /// The heading for an opened album/artist/playlist.
    pub fn heading(&self) -> Option<String> {
        match &self.view {
            View::List => None,
            View::Album(a) => Some(format!("{} — {}", a.title, a.artist)),
            View::Artist(name) => Some(name.clone()),
            View::Playlist { name, .. } => Some(name.clone()),
        }
    }
}

/// What pressing Enter on a row does.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Open(View),
    /// Play these songs in order from `start` (1 = first), via the queue.
    PlaySongs {
        ids: Vec<String>,
        start: usize,
    },
    PlayPlaylist {
        id: String,
        index: usize,
    },
    PlayQueueAt(usize),
    ToggleSpeaker {
        name: String,
        on: bool,
    },
    Nothing,
}

/// Which songs `ctrl-n` / `ctrl-a` would queue from a row. It's worked out
/// only when you press the key, so we don't build song lists for every
/// album on every frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Queueable {
    Songs(Vec<String>),
    Album(AlbumKey),
    Artist(String),
    Playlist(String),
    Nothing,
}

/// One line in the pop-up.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub icon: &'static str,
    pub label: String,
    /// Secondary text shown dimmed after the label.
    pub detail: String,
    pub action: Action,
    pub queue: Queueable,
    /// Drawn in bold (the song playing now, in the Queue tab).
    pub current: bool,
}

impl Row {
    /// A grey, do-nothing row like "Loading…".
    fn message(text: &str) -> Row {
        Row {
            icon: " ",
            label: text.to_string(),
            detail: String::new(),
            action: Action::Nothing,
            queue: Queueable::Nothing,
            current: false,
        }
    }

    pub fn is_message(&self) -> bool {
        self.action == Action::Nothing && self.queue == Queueable::Nothing
    }
}

/// Everything `rows()` needs to look at. Rust note: the `'a` is a
/// *lifetime*: it says these references all borrow from something that
/// lives at least as long as this struct (here, the `App`).
pub struct Data<'a> {
    pub library: Option<&'a Library>,
    pub playlist_songs: &'a HashMap<String, Vec<Song>>,
    pub queue: Option<&'a QueueInfo>,
    pub speakers: Option<&'a [Speaker]>,
}

const ICON_SONG: &str = "♪";
const ICON_ALBUM: &str = "◉";
const ICON_ARTIST: &str = "◆";
const ICON_PLAYLIST: &str = "≡";

/// The rows to show for the pop-up's current tab and view.
pub fn rows(popup: &Popup, data: &Data) -> Vec<Row> {
    // Queue and Speakers don't need the library.
    match popup.tab {
        Tab::Queue => return queue_rows(data.queue),
        Tab::Speakers => return speaker_rows(data.speakers),
        _ => {}
    }
    let Some(lib) = data.library else {
        return vec![Row::message("Loading your library…")];
    };

    let rows = match &popup.view {
        View::Album(key) => song_rows(&lib.album_songs(key), |s| format_time(s.duration)),
        View::Artist(name) => song_rows(&lib.artist_songs(name), |s| s.album.clone()),
        View::Playlist { id, .. } => match data.playlist_songs.get(id) {
            Some(songs) => playlist_rows(id, songs),
            None => vec![Row::message("Loading…")],
        },
        View::List => match popup.tab {
            Tab::Search => search_rows(lib, &popup.query),
            Tab::Playlists => lib.playlists.iter().map(playlist_row).collect(),
            Tab::Albums => lib.albums.iter().map(album_row).collect(),
            Tab::Artists => lib.artists.iter().map(|a| artist_row(a)).collect(),
            Tab::Queue | Tab::Speakers => unreachable!("handled above"),
        },
    };
    if rows.is_empty() {
        vec![Row::message("Nothing here")]
    } else {
        rows
    }
}

/// Rows for a list of songs. Enter plays the whole list from that song on.
/// `detail` picks the grey text for each song (Rust note: `impl Fn(&Song) ->
/// String` means "any function or closure that takes a song and returns a
/// String").
fn song_rows(songs: &[Song], detail: impl Fn(&Song) -> String) -> Vec<Row> {
    let ids: Vec<String> = songs.iter().map(|s| s.id.clone()).collect();
    songs
        .iter()
        .enumerate() // gives (index, song) pairs
        .map(|(i, s)| Row {
            icon: ICON_SONG,
            label: s.name.clone(),
            detail: detail(s),
            action: Action::PlaySongs {
                ids: ids.clone(),
                start: i + 1,
            },
            queue: Queueable::Songs(vec![s.id.clone()]),
            current: false,
        })
        .collect()
}

/// Songs inside a saved playlist: Enter plays the real playlist from there.
fn playlist_rows(id: &str, songs: &[Song]) -> Vec<Row> {
    songs
        .iter()
        .enumerate()
        .map(|(i, s)| Row {
            icon: ICON_SONG,
            label: s.name.clone(),
            detail: s.artist.clone(),
            action: Action::PlayPlaylist {
                id: id.to_string(),
                index: i + 1,
            },
            queue: Queueable::Songs(vec![s.id.clone()]),
            current: false,
        })
        .collect()
}

fn playlist_row(p: &crate::music::Playlist) -> Row {
    Row {
        icon: ICON_PLAYLIST,
        label: p.name.clone(),
        detail: String::new(),
        action: Action::Open(View::Playlist {
            id: p.id.clone(),
            name: p.name.clone(),
        }),
        queue: Queueable::Playlist(p.id.clone()),
        current: false,
    }
}

fn album_row(a: &AlbumKey) -> Row {
    Row {
        icon: ICON_ALBUM,
        label: a.title.clone(),
        detail: a.artist.clone(),
        action: Action::Open(View::Album(a.clone())),
        queue: Queueable::Album(a.clone()),
        current: false,
    }
}

fn artist_row(name: &str) -> Row {
    Row {
        icon: ICON_ARTIST,
        label: name.to_string(),
        detail: String::new(),
        action: Action::Open(View::Artist(name.to_string())),
        queue: Queueable::Artist(name.to_string()),
        current: false,
    }
}

fn search_rows(lib: &Library, query: &str) -> Vec<Row> {
    if query.trim().is_empty() {
        return vec![Row::message(
            "Type to search songs, albums, artists and playlists",
        )];
    }
    let hits = lib.search(query, SEARCH_LIMIT);
    if hits.is_empty() {
        return vec![Row::message("No matches in your library")];
    }

    // Enter on a song plays the matching songs from that one onward, like
    // double-clicking in a song list in Music.app.
    let song_ids: Vec<String> = hits
        .iter()
        .filter_map(|h| match h {
            Hit::Song(s) => Some(s.id.clone()),
            _ => None,
        })
        .collect();
    let mut song_index = 0;

    hits.iter()
        .map(|hit| match hit {
            Hit::Artist(name) => artist_row(name),
            Hit::Album(a) => album_row(a),
            Hit::Playlist(p) => playlist_row(p),
            Hit::Song(s) => {
                song_index += 1;
                Row {
                    icon: ICON_SONG,
                    label: s.name.clone(),
                    detail: format!("{} — {}", s.artist, s.album),
                    action: Action::PlaySongs {
                        ids: song_ids.clone(),
                        start: song_index,
                    },
                    queue: Queueable::Songs(vec![s.id.clone()]),
                    current: false,
                }
            }
        })
        .collect()
}

/// The queue from the current song onward (earlier songs are history).
fn queue_rows(queue: Option<&QueueInfo>) -> Vec<Row> {
    let Some(q) = queue else {
        return vec![Row::message("Loading…")];
    };
    if q.songs.is_empty() {
        return vec![Row::message(
            "The queue is empty. Add songs with ctrl-a (end) or ctrl-n (next).",
        )];
    }
    let current = q.current_index();
    let start = current.unwrap_or(0);
    q.songs
        .iter()
        .enumerate()
        .skip(start)
        .map(|(i, s)| Row {
            icon: if Some(i) == current { "▶" } else { ICON_SONG },
            label: s.name.clone(),
            detail: s.artist.clone(),
            action: Action::PlayQueueAt(i + 1),
            queue: Queueable::Songs(vec![s.id.clone()]),
            current: Some(i) == current,
        })
        .collect()
}

fn speaker_rows(speakers: Option<&[Speaker]>) -> Vec<Row> {
    let Some(speakers) = speakers else {
        return vec![Row::message("Looking for speakers…")];
    };
    if speakers.is_empty() {
        return vec![Row::message("No AirPlay outputs found")];
    }
    speakers
        .iter()
        .map(|s| Row {
            icon: if s.selected { "●" } else { "○" },
            label: s.name.clone(),
            detail: if s.available {
                s.kind.clone()
            } else {
                format!("{} · unavailable", s.kind)
            },
            action: Action::ToggleSpeaker {
                name: s.name.clone(),
                on: !s.selected,
            },
            queue: Queueable::Nothing,
            current: s.selected,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::Playlist;

    fn song(id: &str, name: &str, album: &str, number: u32) -> Song {
        Song {
            id: id.into(),
            name: name.into(),
            artist: "Band".into(),
            album: album.into(),
            album_artist: String::new(),
            duration: 61.0,
            disc: 1,
            number,
        }
    }

    fn library() -> Library {
        Library::new(
            vec![
                song("A1", "Alpha", "Record", 1),
                song("A2", "Beta", "Record", 2),
            ],
            vec![Playlist {
                id: "P1".into(),
                name: "Mix".into(),
            }],
        )
    }

    fn data<'a>(
        lib: Option<&'a Library>,
        cache: &'a HashMap<String, Vec<Song>>,
        queue: Option<&'a QueueInfo>,
        speakers: Option<&'a [Speaker]>,
    ) -> Data<'a> {
        Data {
            library: lib,
            playlist_songs: cache,
            queue,
            speakers,
        }
    }

    #[test]
    fn tabs_cycle_both_ways() {
        assert_eq!(Tab::Search.next(), Tab::Playlists);
        assert_eq!(Tab::Speakers.next(), Tab::Search);
        assert_eq!(Tab::Search.prev(), Tab::Speakers);
    }

    #[test]
    fn shows_loading_until_the_library_arrives() {
        let cache = HashMap::new();
        let rows = rows(&Popup::new(), &data(None, &cache, None, None));
        assert_eq!(rows[0].label, "Loading your library…");
        assert!(rows[0].is_message());
    }

    #[test]
    fn search_songs_play_the_results_from_the_chosen_one() {
        let lib = library();
        let cache = HashMap::new();
        let mut p = Popup::new();
        p.query = "a".into(); // matches both songs, the album and the artist
        let rows = rows(&p, &data(Some(&lib), &cache, None, None));
        let songs: Vec<&Row> = rows.iter().filter(|r| r.icon == ICON_SONG).collect();
        assert_eq!(songs.len(), 2);
        // The second song's Enter plays the song list starting at position 2.
        match &songs[1].action {
            Action::PlaySongs { ids, start } => {
                assert_eq!(ids.len(), 2);
                assert_eq!(*start, 2);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn album_view_lists_songs_in_order_with_durations() {
        let lib = library();
        let cache = HashMap::new();
        let mut p = Popup::new();
        p.tab = Tab::Albums;
        p.view = View::Album(lib.albums[0].clone());
        let rows = rows(&p, &data(Some(&lib), &cache, None, None));
        assert_eq!(rows[0].label, "Alpha");
        assert_eq!(rows[0].detail, "1:01");
        assert_eq!(rows[1].queue, Queueable::Songs(vec!["A2".into()]));
    }

    #[test]
    fn playlist_view_waits_for_songs_then_plays_the_real_playlist() {
        let lib = library();
        let mut cache = HashMap::new();
        let mut p = Popup::new();
        p.tab = Tab::Playlists;
        p.view = View::Playlist {
            id: "P1".into(),
            name: "Mix".into(),
        };
        assert_eq!(
            rows(&p, &data(Some(&lib), &cache, None, None))[0].label,
            "Loading…"
        );

        cache.insert("P1".to_string(), vec![song("A2", "Beta", "Record", 2)]);
        let rows = rows(&p, &data(Some(&lib), &cache, None, None));
        assert_eq!(
            rows[0].action,
            Action::PlayPlaylist {
                id: "P1".into(),
                index: 1
            }
        );
    }

    #[test]
    fn queue_starts_at_the_current_song() {
        let cache = HashMap::new();
        let q = QueueInfo {
            active: true,
            current_id: Some("2".into()),
            songs: vec![
                song("1", "Old", "", 1),
                song("2", "Now", "", 2),
                song("3", "Later", "", 3),
            ],
        };
        let rows = rows(
            &Popup {
                tab: Tab::Queue,
                ..Popup::new()
            },
            &data(None, &cache, Some(&q), None),
        );
        assert_eq!(rows.len(), 2);
        assert!(rows[0].current);
        assert_eq!(rows[0].icon, "▶");
        assert_eq!(rows[1].action, Action::PlayQueueAt(3));
    }

    #[test]
    fn speakers_toggle_to_the_opposite_state() {
        let cache = HashMap::new();
        let speakers = vec![Speaker {
            name: "Kitchen".into(),
            kind: "HomePod".into(),
            selected: false,
            available: false,
        }];
        let rows = rows(
            &Popup {
                tab: Tab::Speakers,
                ..Popup::new()
            },
            &data(None, &cache, None, Some(&speakers)),
        );
        assert_eq!(
            rows[0].action,
            Action::ToggleSpeaker {
                name: "Kitchen".into(),
                on: true
            }
        );
        assert_eq!(rows[0].detail, "HomePod · unavailable");
    }
}
