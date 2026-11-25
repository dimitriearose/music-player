//! The focus screen: title, artist/album, progress bar, play state.

use super::{BOLD, DIM, centered};
use crate::app::{App, format_time};
use crate::art::{Art, color};
use crate::music::{PlayState, Repeat};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
};
use std::time::Instant;

/// The progress bar never gets wider than this, even on a huge window.
/// A long bar is harder to read, not easier.
const MAX_BAR_WIDTH: usize = 50;

pub fn draw(frame: &mut Frame, area: Rect, app: &App, now: Instant) {
    // `let Some(x) = ... else { ... }` means: if it's `None`, run the
    // `else` block (which must leave the function) instead.
    let Some(status) = &app.status else { return };

    let mut lines: Vec<Line> = Vec::new();

    match &status.track {
        Some(track) => {
            // The title, with a red ♥ if you've favorited it.
            let mut title = vec![Span::styled(track.name.clone(), BOLD)];
            if track.favorited {
                title.push(Span::styled(
                    " ♥",
                    Style::new().fg(ratatui::style::Color::Red),
                ));
            }
            lines.push(Line::from(title));
            lines.push(Line::styled(artist_album(&track.artist, &track.album), DIM));
            lines.push(Line::raw(""));

            // Width available for the bar: the screen minus the two time
            // labels and some margin. `saturating_sub` stops at 0 instead
            // of crashing on a tiny window.
            let pos = app.position(now);
            let left = format_time(pos);
            let right = format_time(track.duration);
            let room = (area.width as usize).saturating_sub(left.len() + right.len() + 8);
            let width = room.min(MAX_BAR_WIDTH);
            let (filled, empty) = bar_parts(pos, track.duration, width);

            lines.push(Line::from(vec![
                Span::styled(left, DIM),
                Span::raw(" "),
                Span::raw("━".repeat(filled)),
                Span::styled("─".repeat(empty), DIM),
                Span::raw(" "),
                Span::styled(right, DIM),
            ]));
        }
        None => lines.push(Line::styled("Nothing playing", DIM)),
    }

    lines.push(Line::raw(""));
    lines.push(state_line(
        status.state,
        status.volume,
        status.shuffle,
        status.queue_shuffled,
        status.repeat,
        app.sleep_label(now),
    ));
    if let Some(next) = app.next_up() {
        lines.push(Line::styled(format!("next  {}", artist_title(next)), DIM));
    }

    // Album art goes on top, but only if the window is tall enough for it
    // *and* the text; otherwise the text wins and the art is skipped.
    if let Some(art) = app.current_art() {
        let art_lines = art_lines(art, app.truecolor);
        if (area.height as usize) >= lines.len() + art_lines.len() + 2 {
            let mut all = art_lines;
            all.push(Line::raw(""));
            all.append(&mut lines); // `append` moves every line from `lines` onto the end
            lines = all;
        }
    }

    centered(frame, area, lines);
}

/// Turn the artwork into lines of `▀` characters: each character's
/// foreground is one pixel and its background the pixel below it.
fn art_lines(art: &Art, truecolor: bool) -> Vec<Line<'static>> {
    (0..art.size / 2)
        .map(|row| {
            let spans: Vec<Span> = (0..art.size)
                .map(|x| {
                    let top = color(art.pixel(x, row * 2), truecolor);
                    let bottom = color(art.pixel(x, row * 2 + 1), truecolor);
                    Span::styled("▀", Style::new().fg(top).bg(bottom))
                })
                .collect();
            Line::from(spans)
        })
        .collect()
}

/// "Title · Artist" for the next-up line.
fn artist_title(song: &crate::music::Song) -> String {
    if song.artist.is_empty() {
        song.name.clone()
    } else {
        format!("{} · {}", song.name, song.artist)
    }
}

/// "Artist — Album", leaving out whichever part is empty.
fn artist_album(artist: &str, album: &str) -> String {
    match (artist.is_empty(), album.is_empty()) {
        (false, false) => format!("{artist} — {album}"),
        (false, true) => artist.to_string(),
        (true, false) => album.to_string(),
        (true, true) => String::new(),
    }
}

/// How many bar cells are filled vs. empty for a given position.
fn bar_parts(pos: f64, duration: f64, width: usize) -> (usize, usize) {
    if duration <= 0.0 {
        return (0, width);
    }
    let fraction = (pos / duration).clamp(0.0, 1.0);
    let filled = (fraction * width as f64).round() as usize;
    (filled, width - filled)
}

/// "▶ playing   vol 60   ⤮ shuffle   ↻ all". Shuffle and repeat are
/// only shown when they're on, to keep the line quiet. "⤮ shuffled queue"
/// means `am` shuffled the songs (the `S` key); "⤮ shuffle" is Music's own
/// setting (the `s` key). They're different things, so they're named apart.
fn state_line(
    state: PlayState,
    volume: u8,
    shuffle: bool,
    queue_shuffled: bool,
    repeat: Repeat,
    sleep: Option<String>,
) -> Line<'static> {
    let state = match state {
        PlayState::Playing => "▶ playing",
        PlayState::Paused => "⏸ paused",
        PlayState::Stopped => "■ stopped",
    };
    let mut parts = vec![state.to_string(), format!("vol {volume}")];
    if queue_shuffled {
        parts.push("⤮ shuffled queue".into());
    }
    if shuffle {
        parts.push("⤮ shuffle".into());
    }
    match repeat {
        Repeat::Off => {}
        Repeat::One => parts.push("↻ one".into()),
        Repeat::All => parts.push("↻ all".into()),
    }
    parts.extend(sleep); // adds the sleep label if there is one
    Line::styled(parts.join("   "), DIM)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn art_is_half_as_tall_as_it_is_wide() {
        let art = Art {
            size: 4,
            pixels: vec![[10, 20, 30]; 16],
        };
        let lines = art_lines(&art, true);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].spans.len(), 4);
        assert_eq!(lines[0].spans[0].content, "▀");
    }

    #[test]
    fn bar_fills_proportionally() {
        assert_eq!(bar_parts(0.0, 100.0, 10), (0, 10));
        assert_eq!(bar_parts(50.0, 100.0, 10), (5, 5));
        assert_eq!(bar_parts(100.0, 100.0, 10), (10, 0));
        assert_eq!(bar_parts(500.0, 100.0, 10), (10, 0), "clamped");
        assert_eq!(bar_parts(5.0, 0.0, 10), (0, 10), "unknown length");
        assert_eq!(bar_parts(5.0, 10.0, 0), (0, 0), "tiny window");
    }

    #[test]
    fn artist_album_skips_missing_parts() {
        assert_eq!(artist_album("A", "B"), "A — B");
        assert_eq!(artist_album("A", ""), "A");
        assert_eq!(artist_album("", "B"), "B");
        assert_eq!(artist_album("", ""), "");
    }

    #[test]
    fn state_line_hides_settings_that_are_off() {
        let quiet = state_line(PlayState::Paused, 40, false, false, Repeat::Off, None).to_string();
        assert_eq!(quiet, "⏸ paused   vol 40");
        let busy = state_line(
            PlayState::Playing,
            60,
            true,
            false,
            Repeat::All,
            Some("☾ 5m".into()),
        )
        .to_string();
        assert_eq!(busy, "▶ playing   vol 60   ⤮ shuffle   ↻ all   ☾ 5m");
        let ours = state_line(PlayState::Playing, 60, false, true, Repeat::Off, None).to_string();
        assert_eq!(ours, "▶ playing   vol 60   ⤮ shuffled queue");
    }
}
