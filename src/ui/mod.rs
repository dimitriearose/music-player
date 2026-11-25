//! Drawing. Every frame, `draw` looks at the `App` state and paints the whole
//! screen from scratch. ratatui then works out which characters actually
//! changed and only sends those to the terminal, so this is cheap.
//!
//! Rust note: this way of building UIs (state in, picture out, no hidden
//! widget state) is called "immediate mode".

mod help;
mod now_playing;
mod popup;

use crate::app::{App, Connection};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::Paragraph,
};
use std::time::Instant;

/// Secondary text colour. "Dark grey" is ANSI colour 8, which every terminal
/// (including Terminal.app) supports.
pub const DIM: Style = Style::new().fg(Color::DarkGray);
pub const BOLD: Style = Style::new().add_modifier(Modifier::BOLD);
/// Shuffle is the one setting that changes what you hear next without you
/// seeing it, so when it's on it gets a colour, not the quiet grey.
pub const SHUFFLE: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);

/// The shuffle badge, if either kind of shuffle is on: "⤮ shuffled queue"
/// (am shuffled the songs, `S`) or "⤮ shuffle on" (Music's own, `s`).
pub fn shuffle_label(status: &crate::music::PlayerStatus) -> Option<&'static str> {
    if status.queue_shuffled {
        Some("⤮ shuffled queue")
    } else if status.shuffle {
        Some("⤮ shuffle on")
    } else {
        None
    }
}

/// Draw one frame.
pub fn draw(frame: &mut Frame, app: &App, now: Instant) {
    let area = frame.area();

    // Split the screen: everything above, one line at the bottom for notices.
    let [main, bottom] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);

    match app.connection {
        Connection::Connected => now_playing::draw(frame, main, app, now),
        Connection::Waiting => {
            let mut lines = vec![Line::styled("Connecting to Music…", DIM)];
            if app.slow_start(now) {
                lines.push(Line::raw(""));
                lines.push(Line::styled(
                    "If macOS is asking whether Terminal may control Music, click OK.",
                    DIM,
                ));
            }
            centered(frame, main, lines);
        }
        Connection::Launching => {
            centered(
                frame,
                main,
                vec![Line::styled("Opening Music in the background…", DIM)],
            );
        }
        Connection::NotRunning => centered(
            frame,
            main,
            vec![
                Line::raw("Music isn't running"),
                Line::raw(""),
                Line::styled("press o to open it  ·  q to quit", DIM),
            ],
        ),
        Connection::PermissionDenied => centered(frame, main, permission_help()),
    }

    // Bottom line: a fading notice if there is one, otherwise the key hints.
    let bottom_line = match app.message(now) {
        Some(msg) => Line::styled(msg.to_string(), Style::new().fg(Color::Yellow)),
        // Just a quiet pointer to the key list; the list itself is behind `?`.
        None => Line::styled("? keys", DIM),
    };
    frame.render_widget(
        Paragraph::new(bottom_line).alignment(Alignment::Center),
        bottom,
    );

    // Drawn last so it sits on top of everything else.
    if app.popup.is_some() {
        popup::draw(frame, area, app);
    }
    if app.show_help {
        help::draw(frame, area);
    }
}

/// Draw some lines in the middle of `area`, both horizontally and vertically.
pub fn centered(frame: &mut Frame, area: Rect, lines: Vec<Line>) {
    // `as u16` converts the length (a `usize`) to the terminal's number type.
    let height = lines.len() as u16;
    let [_, middle, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .areas(area);
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), middle);
}

/// The screen shown when macOS blocked us from controlling Music.
fn permission_help() -> Vec<Line<'static>> {
    vec![
        Line::styled("am needs permission to control Music", BOLD),
        Line::raw(""),
        Line::raw("1. Open System Settings → Privacy & Security → Automation"),
        Line::raw("2. Find your terminal app (e.g. Terminal) in the list"),
        Line::raw("3. Turn on the switch for Music"),
        Line::raw(""),
        Line::styled(
            "am will notice within a second once it's allowed.  q to quit",
            DIM,
        ),
    ]
}
