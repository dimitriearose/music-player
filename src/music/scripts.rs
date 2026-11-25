//! All AppleScript source text, kept in one place.
//!
//! Tip: you can copy any of these into Script Editor (in /Applications/Utilities)
//! and press ▶ to see exactly what our program sees. Replace `character id 31`
//! with "|" first if you want readable output.
//!
//! Rust note: `pub const NAME: &str = r#"..."#;` is a constant string baked
//! into the program. The `r#"` ... `"#` form is a *raw string*: backslashes and
//! double quotes inside it are taken literally, which is perfect for
//! embedding another language's source code.

/// Returns one line of fields separated by ASCII 31 ("unit separator"):
///
/// `state, volume, shuffle, repeat[, id, name, artist, album, duration, position]`
///
/// The track fields are left off when nothing is loaded.
/// If Music isn't running it returns the word `NOT_RUNNING`, and crucially it
/// does *not* launch Music: the `is running` check happens before any `tell`.
pub const STATUS: &str = r#"
if application "Music" is not running then return "NOT_RUNNING"
set US to character id 31
tell application "Music"
    -- (Variable names avoid `st`, which AppleScript reserves for "1st".)
    set pstate to player state as text
    -- Is the song coming from am's queue, shuffled by am (the `S` key)?
    -- `replace_queue` writes the mark into the queue's description.
    set qshuf to false
    try
        if (name of current playlist) is "am · Queue" and (description of current playlist) is "am:shuffled" then set qshuf to true
    end try
    set base to pstate & US & (sound volume) & US & (shuffle enabled) & US & (song repeat as text) & US & qshuf
    if pstate is "stopped" then return base
    try
        set t to current track
        set pos to player position
        if pos is missing value then set pos to 0
        -- ♥ ("favorited"; older versions of Music called it "loved").
        set fav to false
        try
            set fav to favorited of t
        end try
        return base & US & (persistent ID of t) & US & (name of t) & US & (artist of t) & US & (album of t) & US & (duration of t) & US & pos & US & fav
    on error
        -- Some streams (e.g. radio) don't expose every field. Report what we have.
        return base
    end try
end tell
"#;

pub const PLAY_PAUSE: &str = r#"tell application "Music" to playpause"#;
/// Pause only (never starts playback), for the sleep timer.
pub const PAUSE: &str = r#"tell application "Music" to pause"#;
pub const NEXT: &str = r#"tell application "Music" to next track"#;
pub const PREVIOUS: &str = r#"tell application "Music" to previous track"#;

/// ♥ the current song, or take the ♥ away. In Music a ♥'d song also goes
/// into your Apple Music "Favorite Songs".
pub const TOGGLE_FAVORITE: &str = r#"tell application "Music" to set favorited of current track to not (favorited of current track)"#;

pub const TOGGLE_SHUFFLE: &str =
    r#"tell application "Music" to set shuffle enabled to not shuffle enabled"#;

/// Off → all → one → off. (The same order as `app::next_repeat`.)
pub const CYCLE_REPEAT: &str = r#"
tell application "Music"
    set r to song repeat
    if r is off then
        set song repeat to all
    else if r is all then
        set song repeat to one
    else
        set song repeat to off
    end if
end tell
"#;

/// Move the playhead by `seconds` (negative = backwards), never before 0.
///
/// Rust note: this one is a *function* returning `String`, not a constant,
/// because the script needs a number filled in. `format!` works like
/// `println!` but returns the text instead of printing it; `{seconds}` is
/// replaced by the value of the `seconds` variable.
pub fn seek_by(seconds: i32) -> String {
    format!(
        r#"
tell application "Music"
    set p to (player position) + ({seconds})
    if p < 0 then set p to 0
    set player position to p
end tell
"#
    )
}

/// Set the volume to an exact value. `u8` can't be negative, and the caller
/// clamps it to 100, so the number is always valid.
pub fn set_volume(volume: u8) -> String {
    format!(r#"tell application "Music" to set sound volume to {volume}"#)
}

// ───────────────────────── Library (M3) ─────────────────────────
//
// The scripts below need values filled in (IDs, names, numbers). Instead of
// `format!`, they use a template with `{PLACEHOLDERS}` and `.replace(...)`.
// That's because AppleScript itself uses `{ }` for lists and records, and
// `format!` would treat every `{` as a placeholder.
//
// Every value put into a script goes through `quote()` (for text) or is a
// number, so a song or speaker name containing `"` can't break the script.

/// The name of the hidden playlist `am` uses as its queue (DESIGN.md §6.3).
pub const QUEUE_NAME: &str = "am · Queue";

/// Wrap text in AppleScript double quotes, escaping `\` and `"` inside it.
///
/// `say "hi"` would end the string early; `quote("say \"hi\"")` makes it safe.
pub fn quote(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// An AppleScript list of quoted strings: `{"A", "B"}`.
pub fn quote_list(items: &[String]) -> String {
    // `.iter().map(...)` transforms each item; `.collect::<Vec<_>>()` gathers
    // the results; `.join(", ")` glues them together with commas.
    let quoted: Vec<String> = items.iter().map(|s| quote(s)).collect();
    format!("{{{}}}", quoted.join(", ")) // `{{` in format! means a literal `{`
}

/// Every track in `container`, as 8 columns separated by ASCII 30, each
/// column a list of values separated by ASCII 31:
///
/// `ids ␞ names ␞ artists ␞ albums ␞ album artists ␞ durations ␞ discs ␞ track numbers`
///
/// Asking for "the name of every track" is one Apple event for the whole
/// list, so this is ~8 round trips in total instead of 8 per song: the
/// difference between under a second and minutes for a big library.
///
/// `container` is AppleScript like `library playlist 1`. Returns "" if the
/// container doesn't exist or is empty.
pub fn tracks_of(container: &str) -> String {
    r#"
set US to character id 31
set RS to character id 30
tell application "Music"
    try
        set c to {CONTAINER}
        if (count of tracks of c) is 0 then return ""
    on error
        return ""
    end try
    set cols to {persistent ID of every track of c, name of every track of c, artist of every track of c, album of every track of c, album artist of every track of c, duration of every track of c, disc number of every track of c, track number of every track of c}
end tell
set out to {}
set AppleScript's text item delimiters to US
repeat with i from 1 to count of cols
    set end of out to ((item i of cols) as text)
end repeat
set AppleScript's text item delimiters to RS
return out as text
"#
    .replace("{CONTAINER}", container)
}

/// Every playlist in your sidebar (no folders, no built-in "Music" or
/// "Library" lists): `ids ␞ names ␞ kinds`, each a list separated by ASCII 31.
///
/// It asks for every `playlist`, not just every `user playlist`: Apple Music
/// playlists you've added (and your liked songs, "Favorite Songs") are a
/// different class, `subscription playlist`. The kind column tells them apart.
pub const PLAYLISTS: &str = r#"
set US to character id 31
set RS to character id 30
tell application "Music"
    -- (Not `names`: Music's dictionary reserves that word.)
    set plIds to persistent ID of every playlist whose special kind is none
    set plNames to name of every playlist whose special kind is none
    set plKinds to class of every playlist whose special kind is none
    -- Playlists you can add songs to (a separate, shorter list of IDs).
    set plEditable to persistent ID of every user playlist whose smart is false
end tell
set AppleScript's text item delimiters to US
set a to plIds as text
set b to plNames as text
set c to plKinds as text
set d to plEditable as text
set AppleScript's text item delimiters to RS
return {a, b, c, d} as text
"#;

/// AppleScript for "the playlist with this persistent ID".
pub fn playlist_ref(id: &str) -> String {
    // `playlist`, not `user playlist`, so Apple Music playlists work too.
    format!("(first playlist whose persistent ID is {})", quote(id))
}

/// Wait (up to 5 s) until Music is really playing *from `target`* (an
/// AppleScript variable holding a playlist), then a moment more.
///
/// Found by testing: commands sent before playback is under way are silently
/// ignored, and an Apple Music playlist (streamed) takes much longer to start
/// than songs on your Mac, so a fixed pause isn't enough. Found later: after
/// `play`, Music keeps playing the *old* song for up to a second (longer
/// with Bluetooth headphones), so "is it playing?" alone answers yes too
/// early. That's why we `stop` first and also check *which* playlist plays.
fn wait_for_playback(target: &str) -> String {
    format!(
        "    repeat 50 times
        try
            if player state is playing and (persistent ID of current playlist) is (persistent ID of {target}) then exit repeat
        end try
        delay 0.1
    end repeat
    delay 0.3
"
    )
}

/// Play a saved playlist starting at song `index` (1 = first).
///
/// Found by testing (M3): `play track 3 of playlist P` plays that one song
/// *outside* the playlist, so "next" then wanders off elsewhere. Playing the
/// playlist itself keeps "next" inside it. So we start the playlist and
/// skip forward once playback has really started (`wait_for_playback`).
/// Copy the current song into one of your playlists, unless it's already
/// there. Prints `ADDED` or `ALREADY`. (Copying works even for Apple Music
/// songs that aren't in your library; see `QUEUE_APPEND`.)
pub fn add_current_to(id: &str) -> String {
    format!(
        r#"tell application "Music"
    set p to {}
    set t to current track
    set tid to persistent ID of t
    if (count of (every track of p whose persistent ID is tid)) > 0 then return "ALREADY"
    duplicate t to p
    return "ADDED"
end tell
"#,
        playlist_ref(id)
    )
}

pub fn play_playlist_at(id: &str, index: usize) -> String {
    let mut s = format!(
        "tell application \"Music\"\n    set playTarget to {}\n    stop\n    play playTarget\n",
        playlist_ref(id)
    );
    if index > 1 {
        s.push_str(&wait_for_playback("playTarget"));
        s.push_str(&format!(
            "    repeat {} times\n        next track\n    end repeat\n",
            index - 1
        ));
    }
    s.push_str("end tell\n");
    s
}

/// Written into the queue playlist's description when `am` shuffled it, so
/// the status line can say so (`STATUS` reads it back). Stored in Music
/// rather than in `am`, so it's still known after you restart `am`.
pub const SHUFFLE_MARK: &str = "am:shuffled";

/// While the queue is rebuilt, the previous one is kept under this name so
/// its songs can be copied across, then deleted.
pub const OLD_QUEUE_NAME: &str = "am · Queue (old)";

/// Shared opening for every queue script: set the current queue aside as
/// `old` (if there is one) and start a fresh, empty one called `q`.
const QUEUE_PRELUDE: &str = r#"
tell application "Music"
    if exists user playlist {OLDNAME} then delete user playlist {OLDNAME}
    set old to missing value
    set oldMark to missing value
    if exists user playlist {QNAME} then
        set old to user playlist {QNAME}
        try
            set oldMark to description of old
        end try
        set name of old to {OLDNAME}
    end if
    set q to make new user playlist with properties {name:{QNAME}}
{MARK}"#;

/// Copy the songs with these IDs, in order, into `q`, then delete `old`.
///
/// Each song is looked for in your library first, then in the previous
/// queue, then in `{SOURCE}` (the playlist you queued from, if any). Found by
/// testing: most liked songs in an Apple Music "Favorite Songs" playlist
/// aren't in the library, but they can still be copied from the playlist
/// itself (without being added to your library). A song found nowhere is
/// skipped instead of stopping the script.
const QUEUE_APPEND: &str = r#"
    repeat with pid in {IDS}
        set found to false
        try
            duplicate (first track of library playlist 1 whose persistent ID is (contents of pid)) to q
            set found to true
        end try
        if not found and old is not missing value then
            try
                duplicate (first track of old whose persistent ID is (contents of pid)) to q
                set found to true
            end try
        end if
{SOURCE}    end repeat
    if old is not missing value then delete old
"#;

/// The extra lookup in `QUEUE_APPEND` for songs from a particular playlist.
fn source_lookup(source: Option<&str>) -> String {
    match source {
        None => String::new(),
        Some(id) => format!(
            "        if not found then
            try
                duplicate (first track of {} whose persistent ID is (contents of pid)) to q
            end try
        end if
",
            playlist_ref(id)
        ),
    }
}

/// Build a fresh queue from `ids` and (if `play`) start playing it from the
/// first song. `source` is the playlist the songs came from, if they came
/// from one. With `resume_at`, jump that many seconds into the first song,
/// and pause again if `paused`: that's how "play next" carries on with the
/// song you're already hearing.
///
/// Why always rebuild instead of inserting? Found by testing (M4): Music
/// fixes the playing order when a playlist *starts*, and ignores songs added
/// to it afterwards. Restarting the rebuilt queue is the only reliable way.
/// (And `play q`, not `play track 1 of q`, for the reason given at
/// `play_playlist_at`.)
///
/// `shuffled`: `Some(true)` marks the new queue as shuffled by `am`,
/// `Some(false)` clears the mark, `None` keeps whatever the old queue had
/// (adding a song to a shuffled queue leaves it shuffled).
pub fn replace_queue(
    ids: &[String],
    source: Option<&str>,
    play: bool,
    resume_at: Option<(f64, bool)>,
    shuffled: Option<bool>,
) -> String {
    let mark = match shuffled {
        Some(true) => format!("    set description of q to {}\n", quote(SHUFFLE_MARK)),
        Some(false) => String::new(), // a new playlist has no description
        // Found by testing: `set description of q to (description of old)`
        // fails in one step; reading it into `oldMark` first works.
        None => {
            "    if oldMark is not missing value then set description of q to oldMark\n".to_string()
        }
    };
    let mut s = QUEUE_PRELUDE.replace("{MARK}", &mark);
    s.push_str(&QUEUE_APPEND.replace("{SOURCE}", &source_lookup(source)));
    if play {
        // Wait even when there's nothing more to do, so the caller (and the
        // next status poll) sees the new song, not the old one.
        s.push_str("    stop\n    play q\n");
        s.push_str(&wait_for_playback("q"));
        if let Some((pos, paused)) = resume_at {
            // `{:.1}` = one decimal place, always written with a "." which is
            // what AppleScript *source code* expects (whatever your region).
            s.push_str(&format!("    set player position to {pos:.1}\n"));
            if paused {
                s.push_str("    pause\n");
            }
        }
    }
    s.push_str("end tell\n");
    script_with(s, ids)
}

fn script_with(script: String, ids: &[String]) -> String {
    script
        .replace("{OLDNAME}", &quote(OLD_QUEUE_NAME))
        .replace("{QNAME}", &quote(QUEUE_NAME))
        .replace("{IDS}", &quote_list(ids))
}

/// `true␟<current track id>` if Music is playing from our queue playlist,
/// otherwise `false␟<id>` (the id may be empty if nothing is loaded).
pub const QUEUE_STATE: &str = r#"
set US to character id 31
tell application "Music"
    -- (Not `active`: Music reserves that word for AirPlay devices.)
    set inQueue to false
    set tid to ""
    try
        set inQueue to ((name of current playlist) is {QNAME})
    end try
    try
        set tid to persistent ID of current track
    end try
end tell
return (inQueue as text) & US & tid
"#;

pub fn queue_state() -> String {
    QUEUE_STATE.replace("{QNAME}", &quote(QUEUE_NAME))
}

pub fn queue_tracks() -> String {
    tracks_of(&format!("user playlist {}", quote(QUEUE_NAME)))
}

// ───────────────────────── AirPlay (M5) ─────────────────────────

/// One record per AirPlay device, separated by ASCII 30:
/// `name ␟ kind ␟ selected ␟ available`.
pub const SPEAKERS: &str = r#"
set US to character id 31
set RS to character id 30
set out to {}
tell application "Music"
    repeat with d in (every AirPlay device)
        set end of out to (name of d) & US & (kind of d as text) & US & (selected of d as text) & US & (available of d as text)
    end repeat
end tell
set AppleScript's text item delimiters to RS
return out as text
"#;

/// Turn one AirPlay device on or off (Music can play to several at once).
pub fn set_speaker(name: &str, on: bool) -> String {
    format!(
        "tell application \"Music\" to set selected of (first AirPlay device whose name is {}) to {on}",
        quote(name)
    )
}

// ───────────────────────── Album art (M6) ─────────────────────────

/// Write the current track's artwork to the file at `path` and return `OK`,
/// or return `NONE` if the track has no artwork.
///
/// The file writing happens *outside* the `tell` block: inside it, AppleScript
/// would ask Music to write the file, and Music doesn't know how.
pub fn save_artwork(path: &str) -> String {
    r#"
tell application "Music"
    try
        set d to raw data of artwork 1 of current track
    on error
        return "NONE"
    end try
end tell
set f to open for access (POSIX file {PATH}) with write permission
try
    set eof f to 0
    write d to f
    close access f
on error e
    close access f
    error e
end try
return "OK"
"#
    .replace("{PATH}", &quote(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_escapes_quotes_and_backslashes() {
        assert_eq!(quote("plain"), "\"plain\"");
        assert_eq!(quote(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote(r"back\slash"), r#""back\\slash""#);
    }

    #[test]
    fn quote_list_builds_an_applescript_list() {
        assert_eq!(quote_list(&["A1".into(), "B2".into()]), r#"{"A1", "B2"}"#);
        assert_eq!(quote_list(&[]), "{}");
    }

    #[test]
    fn templates_leave_no_placeholders_behind() {
        let scripts = [
            tracks_of("library playlist 1"),
            play_playlist_at("ABC", 3),
            replace_queue(&["A".into()], Some("P1"), true, Some((12.5, true)), None),
            queue_state(),
            save_artwork("/tmp/x"),
        ];
        for s in scripts {
            for placeholder in [
                "{OLDNAME}",
                "{SOURCE}",
                "{CONTAINER}",
                "{INDEX}",
                "{PLAYLIST}",
                "{QNAME}",
                "{IDS}",
                "{PATH}",
            ] {
                assert!(!s.contains(placeholder), "{placeholder} left in:\n{s}");
            }
        }
    }

    #[test]
    fn replace_queue_resumes_with_a_dot_decimal() {
        let s = replace_queue(&["A".into()], None, true, Some((12.25, true)), None);
        assert!(s.contains("play q"));
        assert!(
            s.contains("set player position to 12.2") || s.contains("set player position to 12.3")
        );
        assert!(s.contains("    pause\n"));
        let s = replace_queue(&["A".into()], None, false, None, None);
        assert!(!s.contains("play q"));
    }

    #[test]
    fn queue_songs_can_come_from_the_source_playlist() {
        let with = replace_queue(&["A".into()], Some("ABC"), false, None, None);
        assert!(
            with.contains(
                "duplicate (first track of (first playlist whose persistent ID is \"ABC\")"
            )
        );
        let without = replace_queue(&["A".into()], None, false, None, None);
        assert!(!without.contains("first playlist whose"));
        // Both look in the library and the previous queue, then tidy up.
        for s in [with, without] {
            assert!(s.contains("first track of library playlist 1"));
            assert!(s.contains("first track of old"));
            assert!(s.contains("if old is not missing value then delete old"));
        }
    }

    #[test]
    fn the_shuffle_mark_is_set_kept_or_cleared() {
        let ids = ["A".to_string()];
        let set = replace_queue(&ids, None, true, None, Some(true));
        assert!(set.contains("set description of q to \"am:shuffled\""));
        let kept = replace_queue(&ids, None, true, None, None);
        assert!(kept.contains("set description of q to oldMark"));
        let cleared = replace_queue(&ids, None, true, None, Some(false));
        assert!(!cleared.contains("set description of q"));
        // STATUS reads back the same name and mark (it can't use `quote` as
        // it's a constant, so check they agree).
        assert!(STATUS.contains(&quote(QUEUE_NAME)));
        assert!(STATUS.contains(&quote(SHUFFLE_MARK)));
    }

    #[test]
    fn playing_a_playlist_from_song_3_skips_twice_after_a_pause() {
        let s = play_playlist_at("ABC", 3);
        assert!(s.contains("set playTarget to (first playlist whose persistent ID is \"ABC\")"));
        // Stop the old song first, then wait for *this* playlist to play.
        assert!(s.contains("stop\n    play playTarget"));
        assert!(s.contains("(persistent ID of current playlist) is (persistent ID of playTarget)"));
        assert!(s.contains("repeat 2 times"));
        assert!(
            !play_playlist_at("ABC", 1).contains("repeat"),
            "song 1 needs no skipping"
        );
    }
}
