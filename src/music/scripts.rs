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
    set base to pstate & US & (sound volume) & US & (shuffle enabled) & US & (song repeat as text)
    if pstate is "stopped" then return base
    try
        set t to current track
        set pos to player position
        if pos is missing value then set pos to 0
        return base & US & (persistent ID of t) & US & (name of t) & US & (artist of t) & US & (album of t) & US & (duration of t) & US & pos
    on error
        -- Some streams (e.g. radio) don't expose every field. Report what we have.
        return base
    end try
end tell
"#;

pub const PLAY_PAUSE: &str = r#"tell application "Music" to playpause"#;
pub const NEXT: &str = r#"tell application "Music" to next track"#;
pub const PREVIOUS: &str = r#"tell application "Music" to previous track"#;

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
