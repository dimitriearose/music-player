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
