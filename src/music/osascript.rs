//! Runs AppleScript by starting macOS's `/usr/bin/osascript` program.
//!
//! Each call starts a new process, which costs roughly 50–100 ms. That's why
//! this is only ever called from the background worker thread, never from the
//! thread that draws the screen (see `worker.rs`).

use super::MusicError;
use std::process::Command;

/// Run an AppleScript and return what it printed (its `return` value).
///
/// Rust note: `&str` is a *borrowed* string. We only need to read the script,
/// not own or change it, so borrowing is enough. `String` (the return type) is
/// an *owned* string: the caller gets to keep it.
pub fn run(script: &str) -> Result<String, MusicError> {
    let output = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .output()
        // `map_err` converts the std::io::Error into our own error type.
        .map_err(|e| MusicError::Io(e.to_string()))?;

    if output.status.success() {
        // Program output is raw bytes. `from_utf8_lossy` turns it into text,
        // replacing any invalid bytes instead of failing.
        let text = String::from_utf8_lossy(&output.stdout);
        // osascript adds a final newline; remove only that, so a song title
        // that really ends in a space keeps it.
        Ok(text.trim_end_matches('\n').to_string())
    } else {
        Err(classify_error(&String::from_utf8_lossy(&output.stderr)))
    }
}

/// Turn osascript's error text into a `MusicError`.
///
/// osascript errors look like:
/// `execution error: Not authorized to send Apple events to Music. (-1743)`
/// The number in brackets is the useful part.
pub fn classify_error(stderr: &str) -> MusicError {
    if stderr.contains("(-1743)") {
        MusicError::PermissionDenied
    } else if stderr.contains("(-600)") || stderr.contains("(-609)") {
        // -600: "application isn't running"; -609: "connection is invalid"
        // (Music quit while we were talking to it).
        MusicError::NotRunning
    } else {
        MusicError::Script(stderr.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_denied_is_recognised() {
        let err = classify_error(
            "execution error: Not authorized to send Apple events to Music. (-1743)",
        );
        assert_eq!(err, MusicError::PermissionDenied);
    }

    #[test]
    fn not_running_codes_are_recognised() {
        assert_eq!(
            classify_error("... isn't running. (-600)"),
            MusicError::NotRunning
        );
        assert_eq!(
            classify_error("Connection is invalid. (-609)"),
            MusicError::NotRunning
        );
    }

    #[test]
    fn other_errors_keep_their_message() {
        assert_eq!(
            classify_error("  syntax error: Expected end of line. (-2741)\n"),
            MusicError::Script("syntax error: Expected end of line. (-2741)".into())
        );
    }
}
