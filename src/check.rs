//! `am --check`: a quick self-test against the real Music.app.
//!
//! It runs every *read-only* script once, times it, and prints a summary
//! (counts, not song names). Nothing is played, changed or created. Use it
//! when something seems wrong: it shows exactly which step fails and why.

use crate::art::{self, ART_SIZE};
use crate::library::Library;
use crate::music::{Music, MusicError, StatusReply};
use std::time::Instant;

pub fn run() -> anyhow::Result<()> {
    let music = Music::new();
    println!("am --check: talking to Music.app (read-only)\n");

    // Rust note: `step` is a closure that runs another closure, times it and
    // prints the result. `&dyn Fn() -> Result<String, MusicError>` means "a
    // reference to any function that returns either a summary or an error".
    let step = |name: &str, f: &dyn Fn() -> Result<String, MusicError>| {
        let start = Instant::now();
        let result = f();
        let ms = start.elapsed().as_millis();
        match result {
            Ok(summary) => println!("  ok    {name:<10} {ms:>5} ms   {summary}"),
            Err(e) => println!("  FAIL  {name:<10} {ms:>5} ms   {e}"),
        }
    };

    // Status first: it's the only script that's safe when Music is closed.
    match music.status() {
        Ok(StatusReply::NotRunning) => {
            println!(
                "  Music isn't running. Open it (or run `am`, which opens it hidden) and try again."
            );
            return Ok(());
        }
        Err(MusicError::PermissionDenied) => {
            println!("  FAIL  permission: this terminal isn't allowed to control Music.");
            println!(
                "        System Settings → Privacy & Security → Automation → (your terminal) → Music"
            );
            return Ok(());
        }
        _ => {}
    }

    step("status", &|| {
        Ok(match music.status()? {
            StatusReply::Status(s) => format!(
                "{:?}, volume {}, {}",
                s.state,
                s.volume,
                if s.track.is_some() {
                    "a song is loaded"
                } else {
                    "no song loaded"
                }
            ),
            StatusReply::NotRunning => "not running".into(),
        })
    });
    step("library", &|| {
        let (songs, playlists) = music.library()?;
        let lib = Library::new(songs, playlists);
        Ok(format!(
            "{} songs, {} albums, {} artists, {} playlists",
            lib.songs.len(),
            lib.albums.len(),
            lib.artists.len(),
            lib.playlists.len()
        ))
    });
    step("queue", &|| {
        let q = music.queue()?;
        Ok(format!(
            "{} songs in \"{}\", playing from it: {}",
            q.songs.len(),
            crate::music::scripts::QUEUE_NAME,
            q.active
        ))
    });
    step("speakers", &|| {
        let s = music.speakers()?;
        let on = s.iter().filter(|s| s.selected).count();
        Ok(format!("{} found, {} in use", s.len(), on))
    });
    step("artwork", &|| {
        let path = std::env::temp_dir().join("am-check-artwork");
        let saved = music.save_artwork(&path.to_string_lossy())?;
        let result = if !saved {
            "current song has no artwork".to_string()
        } else {
            let bytes = std::fs::read(&path).map_err(|e| MusicError::Io(e.to_string()))?;
            match art::decode(&bytes, ART_SIZE) {
                Some(_) => format!("{} KB image, decoded fine", bytes.len() / 1024),
                None => format!("{} KB, but not a JPEG/PNG we can read", bytes.len() / 1024),
            }
        };
        let _ = std::fs::remove_file(&path);
        Ok(result)
    });

    println!(
        "\ncolour: {}",
        if art::truecolor_supported() {
            "24-bit (exact album art colours)"
        } else {
            "256 colours (album art is approximated)"
        }
    );
    Ok(())
}
