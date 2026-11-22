//! Turns the text our AppleScripts print into Rust structs.
//!
//! This is kept separate from `osascript.rs` on purpose: parsing is pure
//! (text in, struct out, no Music.app needed), so it's easy to test.

use super::{MusicError, PlayState, PlayerStatus, Repeat, StatusReply, Track};

/// ASCII 31, the "unit separator". `char::from(31)` would also work; `\u{1f}`
/// is the same character written as a Unicode escape.
pub const FIELD_SEP: char = '\u{1f}';

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

    if f.len() < 4 {
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

    // Track fields are optional: either all 6 are there, or none are.
    let (track, position) = if f.len() >= 10 {
        let track = Track {
            id: f[4].to_string(),
            name: f[5].to_string(),
            artist: f[6].to_string(),
            album: f[7].to_string(),
            duration: parse_number(f[8]).ok_or_else(bad)?,
        };
        (Some(track), parse_number(f[9]).unwrap_or(0.0))
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
    }))
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
            "ABC123",
            "Test Song",
            "Some Band",
            "Some Album",
            "276.4",
            "102.5",
        ]);
        let StatusReply::Status(s) = parse_status(&raw).unwrap() else {
            panic!("expected a status");
        };
        assert_eq!(s.state, PlayState::Playing);
        assert_eq!(s.volume, 60);
        assert!(s.shuffle);
        assert_eq!(s.repeat, Repeat::All);
        assert_eq!(s.position, 102.5);
        let t = s.track.unwrap();
        assert_eq!(t.name, "Test Song");
        assert_eq!(t.artist, "Some Band");
        assert_eq!(t.duration, 276.4);
    }

    #[test]
    fn parses_paused_with_comma_decimals() {
        let raw = reply(&[
            "paused",
            "35",
            "false",
            "off",
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
        let raw = reply(&["stopped", "50", "false", "one"]);
        let StatusReply::Status(s) = parse_status(&raw).unwrap() else {
            panic!("expected a status");
        };
        assert_eq!(s.state, PlayState::Stopped);
        assert_eq!(s.repeat, Repeat::One);
        assert_eq!(s.track, None);
    }

    #[test]
    fn stream_without_track_fields_still_parses() {
        let raw = reply(&["playing", "50", "false", "off"]);
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

    #[test]
    fn garbage_is_an_error() {
        assert!(matches!(
            parse_status("hello"),
            Err(MusicError::BadOutput(_))
        ));
        let raw = reply(&["dancing", "50", "false", "off"]);
        assert!(matches!(parse_status(&raw), Err(MusicError::BadOutput(_))));
    }
}
