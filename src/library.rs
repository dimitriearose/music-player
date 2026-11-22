//! Your library, held in memory after one load, plus search.
//!
//! Music.app is slow to ask questions of (~100 ms each), but Rust can scan a
//! few thousand songs in well under a millisecond. So we fetch everything
//! once (see `worker.rs`) and do all browsing and searching here.

use crate::music::{Playlist, Song};

/// An album is identified by its title *and* artist: two different albums
/// can both be called "Greatest Hits".
///
/// Rust note: deriving `PartialOrd, Ord` lets us `sort()` a list of these.
/// Fields are compared in order, so albums sort by title, then by artist.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AlbumKey {
    pub title: String,
    pub artist: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Library {
    pub songs: Vec<Song>,
    pub playlists: Vec<Playlist>,
    /// Worked out from `songs` once, when the library is built.
    pub albums: Vec<AlbumKey>,
    pub artists: Vec<String>,
}

/// One search result.
#[derive(Debug, Clone, PartialEq)]
pub enum Hit {
    Artist(String),
    Album(AlbumKey),
    Playlist(Playlist),
    Song(Song),
}

/// The artist an album is filed under: the album artist if there is one
/// (so compilations stay together), otherwise the song's artist.
pub fn album_artist(song: &Song) -> &str {
    if song.album_artist.is_empty() {
        &song.artist
    } else {
        &song.album_artist
    }
}

/// Lower-case version of `s`, for comparing without caring about capitals.
fn fold(s: &str) -> String {
    s.to_lowercase()
}

impl Library {
    pub fn new(songs: Vec<Song>, playlists: Vec<Playlist>) -> Self {
        let mut albums: Vec<AlbumKey> = songs
            .iter()
            .filter(|s| !s.album.is_empty())
            .map(|s| AlbumKey {
                title: s.album.clone(),
                artist: album_artist(s).to_string(),
            })
            .collect();
        // `sort_by_key` + `dedup` = sorted list without repeats. We sort by
        // the lower-cased title so "abba" and "ABBA" sit together.
        albums.sort_by_key(|a| (fold(&a.title), fold(&a.artist)));
        albums.dedup();

        let mut artists: Vec<String> = songs
            .iter()
            .map(|s| s.artist.clone())
            .filter(|a| !a.is_empty())
            .collect();
        artists.sort_by_key(|a| fold(a));
        artists.dedup();

        // Your own playlists first, then Apple Music ones, each A–Z.
        // (`false` sorts before `true`.)
        let mut playlists = playlists;
        playlists.sort_by_key(|p| (p.apple_music, fold(&p.name)));

        Library {
            songs,
            playlists,
            albums,
            artists,
        }
    }

    /// An album's songs in disc/track order.
    pub fn album_songs(&self, album: &AlbumKey) -> Vec<Song> {
        let mut songs: Vec<Song> = self
            .songs
            .iter()
            .filter(|s| s.album == album.title && album_artist(s) == album.artist)
            .cloned() // copy each matching song out of the library
            .collect();
        songs.sort_by_key(|s| (s.disc, s.number, fold(&s.name)));
        songs
    }

    /// Everything by an artist (as performer or album artist), album by album.
    pub fn artist_songs(&self, artist: &str) -> Vec<Song> {
        let mut songs: Vec<Song> = self
            .songs
            .iter()
            .filter(|s| s.artist == artist || s.album_artist == artist)
            .cloned()
            .collect();
        songs.sort_by_key(|s| (fold(&s.album), s.disc, s.number));
        songs
    }

    /// Search everything. Every word you type must appear somewhere (in any
    /// order), so "nat blood" finds "Bloodbuzz Ohio" by The National.
    /// Best matches first; at most `limit` results.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        let words: Vec<String> = query.split_whitespace().map(fold).collect();
        if words.is_empty() {
            return Vec::new();
        }

        // (score, kind order, hit). Kind order breaks ties: artists, then
        // albums, playlists, songs, because a broad match is usually what you
        // meant when the scores are equal.
        let mut scored: Vec<(u32, u8, Hit)> = Vec::new();

        for artist in &self.artists {
            if let Some(s) = score(artist, &words) {
                scored.push((s, 0, Hit::Artist(artist.clone())));
            }
        }
        for album in &self.albums {
            if let Some(s) = score(&format!("{} {}", album.title, album.artist), &words) {
                scored.push((s, 1, Hit::Album(album.clone())));
            }
        }
        for playlist in &self.playlists {
            if let Some(s) = score(&playlist.name, &words) {
                scored.push((s, 2, Hit::Playlist(playlist.clone())));
            }
        }
        for song in &self.songs {
            let haystack = format!("{} {} {}", song.name, song.artist, song.album);
            if let Some(s) = score(&haystack, &words) {
                scored.push((s, 3, Hit::Song(song.clone())));
            }
        }

        // Highest score first (`Reverse` flips the order), then kind order.
        scored.sort_by_key(|(score, kind, _)| (std::cmp::Reverse(*score), *kind));
        scored
            .into_iter()
            .take(limit)
            .map(|(_, _, hit)| hit)
            .collect()
    }
}

/// How well `haystack` matches all `words` (already lower-cased), or `None`
/// if any word is missing. Per word: 3 points if the text *starts* with it,
/// 2 if a word inside starts with it, 1 if it's just somewhere in the middle.
pub fn score(haystack: &str, words: &[String]) -> Option<u32> {
    let hay = fold(haystack);
    let mut total = 0;
    for word in words {
        let pos = hay.find(word.as_str())?; // `?`: missing word = no match
        total += if pos == 0 {
            3
        } else if hay.split_whitespace().any(|w| w.starts_with(word.as_str())) {
            2
        } else {
            1
        };
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up song. Never real library data in tests.
    fn song(id: &str, name: &str, artist: &str, album: &str, number: u32) -> Song {
        Song {
            id: id.into(),
            name: name.into(),
            artist: artist.into(),
            album: album.into(),
            album_artist: String::new(),
            duration: 200.0,
            disc: 1,
            number,
        }
    }

    fn sample() -> Library {
        Library::new(
            vec![
                song("1", "Second Track", "The Testers", "First Album", 2),
                song("2", "Opening", "The Testers", "First Album", 1),
                song("3", "Blue Light", "Other Band", "Colours", 1),
                song("4", "Testing Ground", "Other Band", "Colours", 2),
            ],
            vec![Playlist {
                id: "P1".into(),
                name: "Morning Run".into(),
                apple_music: false,
            }],
        )
    }

    #[test]
    fn albums_and_artists_are_listed_once_and_sorted() {
        let lib = sample();
        let titles: Vec<&str> = lib.albums.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(titles, ["Colours", "First Album"]);
        assert_eq!(lib.artists, ["Other Band", "The Testers"]);
    }

    #[test]
    fn own_playlists_come_before_apple_music_ones() {
        let p = |name: &str, apple_music| Playlist {
            id: "1".into(),
            name: name.into(),
            apple_music,
        };
        let lib = Library::new(
            vec![],
            vec![
                p("Beats", true),
                p("ital", false),
                p("Ease", false),
                p("Abc", true),
            ],
        );
        let names: Vec<&str> = lib.playlists.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Ease", "ital", "Abc", "Beats"]);
    }

    #[test]
    fn album_songs_are_in_track_order() {
        let lib = sample();
        let key = AlbumKey {
            title: "First Album".into(),
            artist: "The Testers".into(),
        };
        let names: Vec<String> = lib.album_songs(&key).into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["Opening", "Second Track"]);
    }

    #[test]
    fn compilations_are_grouped_by_album_artist() {
        let mut a = song("1", "One", "Singer A", "Hits", 1);
        let mut b = song("2", "Two", "Singer B", "Hits", 2);
        a.album_artist = "Various Artists".into();
        b.album_artist = "Various Artists".into();
        let lib = Library::new(vec![a, b], vec![]);
        assert_eq!(lib.albums.len(), 1);
        assert_eq!(lib.albums[0].artist, "Various Artists");
        assert_eq!(lib.album_songs(&lib.albums[0]).len(), 2);
        assert_eq!(lib.artist_songs("Various Artists").len(), 2);
    }

    #[test]
    fn search_needs_every_word_in_any_order() {
        let lib = sample();
        let hits = lib.search("band blue", 10);
        assert_eq!(hits.len(), 1);
        assert!(matches!(&hits[0], Hit::Song(s) if s.name == "Blue Light"));
        assert!(lib.search("blue nothing", 10).is_empty());
        assert!(lib.search("   ", 10).is_empty());
    }

    #[test]
    fn search_ranks_prefix_matches_first_and_is_case_insensitive() {
        let lib = sample();
        let hits = lib.search("TEST", 10);
        // "The Testers" (artist) and "Testing Ground" both match; the song
        // that *starts* with "test" scores 3, the artist's word match 2.
        assert!(matches!(&hits[0], Hit::Song(s) if s.name == "Testing Ground"));
        assert!(
            hits.iter()
                .any(|h| matches!(h, Hit::Artist(a) if a == "The Testers"))
        );
    }

    #[test]
    fn search_finds_playlists_and_respects_the_limit() {
        let lib = sample();
        assert!(matches!(&lib.search("run", 10)[0], Hit::Playlist(p) if p.name == "Morning Run"));
        assert_eq!(lib.search("e", 2).len(), 2);
    }

    #[test]
    fn scoring() {
        let w = |s: &str| vec![s.to_string()];
        assert_eq!(score("Blue Light", &w("blue")), Some(3));
        assert_eq!(score("Blue Light", &w("lig")), Some(2));
        assert_eq!(score("Blue Light", &w("ght")), Some(1));
        assert_eq!(score("Blue Light", &w("red")), None);
    }
}
