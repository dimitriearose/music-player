//! The `?` overlay: a small box listing every key.

use super::DIM;
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};

/// (key, what it does). Kept as data so adding a key is a one-line change.
/// Rust note: `&[(&str, &str)]` is a borrowed list of pairs of strings.
const KEYS: &[(&str, &str)] = &[
    ("space", "play / pause"),
    ("n  p", "next / previous track"),
    ("← →  h l", "seek −5 s / +5 s"),
    ("+  -", "volume up / down"),
    ("S", "shuffle your whole library"),
    ("s", "Music's shuffle on / off"),
    ("r", "repeat: off → all → one"),
    ("L", "♥ love / unlove this song"),
    ("P", "add this song to one of your playlists"),
    ("z", "sleep: end of song → 15 → 30 → 60 min → off"),
    ("1–9", "play a favorite playlist"),
    ("/", "library: search, browse, queue, speakers"),
    ("a", "album art on / off"),
    ("o", "open Music (if closed)"),
    ("?  esc", "close this list"),
    ("q", "quit (Music keeps playing)"),
];

pub fn draw(frame: &mut Frame, area: Rect) {
    let lines: Vec<Line> = KEYS
        .iter()
        // `map` turns each (key, action) pair into a line of text.
        // `{key:>10}` right-aligns the key in a 10-character column.
        .map(|(key, action)| {
            Line::from(vec![
                Span::raw(format!("{key:>10}")),
                Span::raw("   "),
                Span::styled(*action, DIM),
            ])
        })
        .collect();

    // Box size: content plus borders and a little padding, but never larger
    // than the screen (`min`), so a tiny window doesn't crash.
    let width = 46.min(area.width);
    let height = (lines.len() as u16 + 4).min(area.height);
    let popup = area.centered(Constraint::Length(width), Constraint::Length(height));

    // `Clear` blanks the cells under the box so the focus screen doesn't
    // show through.
    frame.render_widget(Clear, popup);
    let block = Block::bordered()
        .title(" keys ")
        .border_style(DIM)
        .padding(ratatui::widgets::Padding::uniform(1));
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}
