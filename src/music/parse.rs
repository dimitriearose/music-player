//! Turns the text our AppleScripts print into Rust structs.
//!
//! This is kept separate from `osascript.rs` on purpose: parsing is pure
//! (text in, struct out, no Music.app needed), so it's easy to test.

use super::{
    MusicError, PlayState, PlayerStatus, Playlist, Repeat, Song, Speaker, StatusReply, Track,
};

/// ASCII 31, the "unit separator". `char::from(31)` would also work; `\u{1f}`
/// is the same character written as a Unicode escape.
pub const FIELD_SEP: char = '\u{1f}';
/// ASCII 30, the "record separator": splits columns or records.
pub const RECORD_SEP: char = '\u{1e}';

/// Parse the output of `scripts::STATUS`.
pub fn parse_status(raw: &str) -> Result<StatusReply, MusicError> {
    if raw == "NOT_RUNNING" {
        return Ok(StatusReply::NotRunning);
    }

    // Split into fields. `collect()` gathers them into a `Vec` (a growable list).
    let f: Vec<&str> = raw.split(FIELD_SEP).collect();

    // A small helper (a *closure*) that builds our error with the raw text, so
    // a bad reply is easy to debug.
    let bad = || MusicError::BadOutput(raw.to_string());

    if f.len() < 5 {
        return Err(bad());
    }

    // `match` is like a switch statement, but the compiler checks that every
    // possibility is covered. `_` means "anything else".
    let state = match f[0] {
        "playing" | "fast forwarding" | "rewinding" => PlayState::Playing,
        "paused" => PlayState::Paused,
        "stopped" => PlayState::Stopped,
        _ => return Err(bad()),
    };

    let volume: u8 = f[1].trim().parse().map_err(|_| bad())?;
    let shuffle = f[2] == "true";
    let repeat = match f[3] {
        "one" => Repeat::One,
        "all" => Repeat::All,
        _ => Repeat::Off,
    };
    let queue_shuffled = f[4] == "true";

    // Track fields are optional: either all 6 are there, or none are.
    let (track, position) = if f.len() >= 11 {
        let track = Track {
            id: f[5].to_string(),
            name: f[6].to_string(),
            artist: f[7].to_string(),
            album: f[8].to_string(),
            duration: parse_number(f[9]).ok_or_else(bad)?,
            // `get` returns None instead of crashing if the field is missing.
            favorited: f.get(11) == Some(&"true"),
        };
        (Some(track), parse_number(f[10]).unwrap_or(0.0))
    } else {
        (None, 0.0)
    };

    Ok(StatusReply::Status(PlayerStatus {
        state,
        track,
        position,
        volume,
        shuffle,
        repeat,
        queue_shuffled,
    }))
}

/// Parse the output of `scripts::tracks_of`: 8 columns, each a list.
pub fn parse_songs(raw: &str) -> Result<Vec<Song>, MusicError> {
    if raw.is_empty() {
        return Ok(Vec::new()); // empty playlist / no queue yet
    }
    let bad = || MusicError::BadOutput(preview(raw));

    // A `Vec<Vec<&str>>` is a list of lists: one inner list per column.
    let cols: Vec<Vec<&str>> = raw
        .split(RECORD_SEP)
        .map(|col| col.split(FIELD_SEP).collect())
        .collect();
    if cols.len() != 8 {
        return Err(bad());
    }
    let n = cols[0].len();
    // Every column must have one value per song, or the columns don't line
    // up and we'd pair the wrong artist with the wrong song.
    if cols.iter().any(|c| c.len() != n) {
        return Err(bad());
    }

    // `(0..n)` counts 0, 1, ..., n-1; `map` builds a Song for each.
    let songs = (0..n)
        .map(|i| Song {
            id: cols[0][i].to_string(),
            name: text(cols[1][i]),
            artist: text(cols[2][i]),
            album: text(cols[3][i]),
            album_artist: text(cols[4][i]),
            duration: parse_number(cols[5][i]).unwrap_or(0.0),
            disc: cols[6][i].trim().parse().unwrap_or(0),
            number: cols[7][i].trim().parse().unwrap_or(0),
        })
        .collect();
    Ok(songs)
}

/// Parse `scripts::PLAYLISTS`: three columns, ids, names and kinds.
pub fn parse_playlists(raw: &str) -> Result<Vec<Playlist>, MusicError> {
    let cols: Vec<Vec<&str>> = raw
        .split(RECORD_SEP)
        .map(|col| col.split(FIELD_SEP).collect())
        .collect();
    if raw.is_empty() || cols[0] == [""] {
        return Ok(Vec::new()); // no playlists at all
    }
    // Three lined-up columns (ID, name, kind), then a shorter list: the IDs
    // of playlists you can add songs to.
    if cols.len() != 4 || cols[..3].iter().any(|c| c.len() != cols[0].len()) {
        return Err(MusicError::BadOutput(preview(raw)));
    }
    let editable = &cols[3];
    Ok((0..cols[0].len())
        .map(|i| Playlist {
            id: cols[0][i].to_string(),
            name: cols[1][i].to_string(),
            apple_music: cols[2][i] == "subscription playlist",
            editable: editable.contains(&cols[0][i]),
        })
        .collect())
}

/// Parse `scripts::SPEAKERS`: one record per device.
pub fn parse_speakers(raw: &str) -> Result<Vec<Speaker>, MusicError> {
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    raw.split(RECORD_SEP)
        .map(|record| {
            let f: Vec<&str> = record.split(FIELD_SEP).collect();
            if f.len() != 4 {
                return Err(MusicError::BadOutput(preview(raw)));
            }
            Ok(Speaker {
                name: f[0].to_string(),
                kind: f[1].to_string(),
                selected: f[2] == "true",
                available: f[3] == "true",
            })
        })
        // Collecting an iterator of `Result`s into `Result<Vec<_>, _>` stops at
        // the first error. Handy!
        .collect()
}

/// Parse `scripts::QUEUE_STATE`: `active ␟ current-id`.
pub fn parse_queue_state(raw: &str) -> (bool, Option<String>) {
    let (active, id) = raw.split_once(FIELD_SEP).unwrap_or((raw, ""));
    let id = (!id.is_empty()).then(|| id.to_string());
    (active == "true", id)
}

/// AppleScript prints a missing value as the words "missing value".
fn text(s: &str) -> String {
    if s == "missing value" {
        String::new()
    } else {
        s.to_string()
    }
}

/// First 200 characters of a reply, for error messages. (Large replies,
/// like a whole library, would otherwise flood the screen.)
fn preview(raw: &str) -> String {
    raw.chars().take(200).collect()
}

/// Parse a decimal number the way AppleScript prints it.
///
/// AppleScript follows your Mac's region settings, so in many countries it
/// prints `276,5` instead of `276.5`. We accept both.
///
/// Returns `Option<f64>`: `Some(number)` or `None` if it isn't a number.
fn parse_number(s: &str) -> Option<f64> {
    s.trim().replace(',', ".").parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a fake reply, joining fields with the separator, so the tests
    /// read naturally. Made-up songs only; never real library data.
    fn reply(fields: &[&str]) -> String {
        fields.join(&FIELD_SEP.to_string())
    }

    #[test]
    fn parses_a_playing_track() {
        let raw = reply(&[
            "playing",
            "60",
            "true",
            "all",
            "true",
            "ABC123",
            "Test Song",
            "Some Band",
            "Some Album",
            "276.4",
            "102.5",
            "true",
        ]);
        let StatusReply::Status(s) = parse_status(&raw).unwrap() else {
            panic!("expected a status");
        };
        assert_eq!(s.state, PlayState::Playing);
        assert_eq!(s.volume, 60);
        assert!(s.shuffle);
        assert_eq!(s.repeat, Repeat::All);
        assert!(s.queue_shuffled);
        assert_eq!(s.position, 102.5);
        let t = s.track.unwrap();
        assert_eq!(t.name, "Test Song");
        assert_eq!(t.artist, "Some Band");
        assert_eq!(t.duration, 276.4);
        assert!(t.favorited);
    }

    #[test]
    fn parses_paused_with_comma_decimals() {
        let raw = reply(&[
            "paused",
            "35",
            "false",
            "off",
            "false",
            "ID",
            "Café “Ünïcode” 🎵",
            "A",
            "B",
            "200,25",
            "10,5",
        ]);
        let StatusReply::Status(s) = parse_status(&raw).unwrap() else {
            panic!("expected a status");
        };
        assert_eq!(s.state, PlayState::Paused);
        assert_eq!(s.repeat, Repeat::Off);
        assert_eq!(s.position, 10.5);
        let t = s.track.unwrap();
        assert_eq!(t.duration, 200.25);
        assert_eq!(t.name, "Café “Ünïcode” 🎵");
    }

    #[test]
    fn stopped_has_no_track() {
        let raw = reply(&["stopped", "50", "false", "one", "false"]);
        let StatusReply::Status(s) = parse_status(&raw).unwrap() else {
            panic!("expected a status");
        };
        assert_eq!(s.state, PlayState::Stopped);
        assert_eq!(s.repeat, Repeat::One);
        assert_eq!(s.track, None);
    }

    #[test]
    fn stream_without_track_fields_still_parses() {
        let raw = reply(&["playing", "50", "false", "off", "false"]);
        let StatusReply::Status(s) = parse_status(&raw).unwrap() else {
            panic!("expected a status");
        };
        assert_eq!(s.state, PlayState::Playing);
        assert_eq!(s.track, None);
    }

    #[test]
    fn not_running_sentinel() {
        assert_eq!(
            parse_status("NOT_RUNNING").unwrap(),
            StatusReply::NotRunning
        );
    }

    /// Join columns with RECORD_SEP, values with FIELD_SEP.
    fn columns(cols: &[&[&str]]) -> String {
        cols.iter()
            .map(|c| c.join(&FIELD_SEP.to_string()))
            .collect::<Vec<_>>()
            .join(&RECORD_SEP.to_string())
    }

    #[test]
    fn parses_songs_column_by_column() {
        let raw = columns(&[
            &["ID1", "ID2"],
            &["First", "Second"],
            &["Band", "Band"],
            &["Album", "Album"],
            &["", "missing value"],
            &["200.5", "180,25"],
            &["1", "1"],
            &["1", "2"],
        ]);
        let songs = parse_songs(&raw).unwrap();
        assert_eq!(songs.len(), 2);
        assert_eq!(songs[1].name, "Second");
        assert_eq!(songs[1].album_artist, "", "missing value becomes empty");
        assert_eq!(songs[1].duration, 180.25);
        assert_eq!(songs[1].number, 2);
    }

    #[test]
    fn empty_song_list() {
        assert_eq!(parse_songs("").unwrap(), vec![]);
    }

    #[test]
    fn misaligned_song_columns_are_an_error() {
        let raw = columns(&[
            &["ID1", "ID2"],
            &["only one"],
            &[""],
            &[""],
            &[""],
            &[""],
            &[""],
            &[""],
        ]);
        assert!(matches!(parse_songs(&raw), Err(MusicError::BadOutput(_))));
    }

    #[test]
    fn parses_playlists() {
        let raw = columns(&[
            &["P1", "P2"],
            &["Running", "Favorite Songs"],
            &["user playlist", "subscription playlist"],
            &["P1"], // only P1 can take songs
        ]);
        let lists = parse_playlists(&raw).unwrap();
        assert!(!lists[0].apple_music);
        assert!(lists[0].editable);
        assert_eq!(
            lists[1],
            Playlist {
                id: "P2".into(),
                name: "Favorite Songs".into(),
                apple_music: true,
                editable: false,
            }
        );
        assert_eq!(
            parse_playlists(&columns(&[&[""], &[""], &[""], &[""]])).unwrap(),
            vec![]
        );
        assert!(parse_playlists(&columns(&[&["P1"], &["only two columns"]])).is_err());
    }

    #[test]
    fn parses_speakers() {
        let raw = [
            ["Computer", "computer", "true", "true"].join(&FIELD_SEP.to_string()),
            ["Kitchen", "HomePod", "false", "true"].join(&FIELD_SEP.to_string()),
        ]
        .join(&RECORD_SEP.to_string());
        let s = parse_speakers(&raw).unwrap();
        assert_eq!(s.len(), 2);
        assert!(s[0].selected);
        assert!(!s[1].selected);
        assert_eq!(s[1].kind, "HomePod");
    }

    #[test]
    fn parses_queue_state() {
        let sep = FIELD_SEP.to_string();
        assert_eq!(
            parse_queue_state(&format!("true{sep}AB12")),
            (true, Some("AB12".into()))
        );
        assert_eq!(parse_queue_state(&format!("false{sep}")), (false, None));
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(matches!(
            parse_status("hello"),
            Err(MusicError::BadOutput(_))
        ));
        let raw = reply(&["dancing", "50", "false", "off", "false"]);
        assert!(matches!(parse_status(&raw), Err(MusicError::BadOutput(_))));
    }
}
