//! Draws the library pop-up: tabs along the top, the search box or a
//! heading, the rows, and a one-line hint at the bottom.

use super::{BOLD, DIM};
use crate::app::App;
use crate::popup::{self, Tab};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};

pub fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let Some(p) = &app.popup else { return };
    let rows = popup::rows(p, &app.data());

    // Most of the screen, but not all of it: a little of the quiet focus
    // screen around the edge keeps it feeling like a pop-up.
    let width = area.width.saturating_sub(4).min(90);
    let height = area.height.saturating_sub(2).min(30);
    let rect = area.centered(Constraint::Length(width), Constraint::Length(height));
    frame.render_widget(Clear, rect);
    let block = Block::bordered().border_style(DIM);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let [tabs_area, header_area, list_area, hint_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    // Tabs: the open one bold and underlined, the rest dim.
    let mut tabs: Vec<Span> = Vec::new();
    for tab in Tab::ALL {
        let style = if tab == p.tab {
            BOLD.add_modifier(Modifier::UNDERLINED)
        } else {
            DIM
        };
        tabs.push(Span::styled(tab.title(), style));
        tabs.push(Span::raw("   "));
    }
    frame.render_widget(Paragraph::new(Line::from(tabs)), tabs_area);

    // Search box, or the name of what you opened.
    let header = if p.typing() {
        Line::from(vec![
            Span::styled("› ", DIM),
            Span::raw(p.query.clone()),
            Span::styled("▏", DIM), // a fake cursor
        ])
    } else if let Some(heading) = p.heading() {
        Line::from(vec![Span::styled("‹ ", DIM), Span::styled(heading, BOLD)])
    } else {
        Line::raw("")
    };
    frame.render_widget(Paragraph::new(header), header_area);

    // The rows. If the selection is below the visible area, scroll so it's
    // on the last visible line.
    let visible = list_area.height as usize;
    let selected = p.selected.min(rows.len().saturating_sub(1));
    let offset = (selected + 1).saturating_sub(visible);
    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible)
        .map(|(i, row)| {
            let mut style = Style::new();
            if row.is_message() {
                style = DIM;
            } else if row.current {
                style = BOLD;
            }
            let mut line = Line::from(vec![
                Span::styled(format!("{} ", row.icon), DIM),
                Span::styled(row.label.clone(), style),
                Span::raw("  "),
                Span::styled(row.detail.clone(), DIM),
            ]);
            if i == selected && !row.is_message() {
                // Reversed colours = the highlight bar.
                line = line.style(Style::new().add_modifier(Modifier::REVERSED));
            }
            line
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), list_area);

    frame.render_widget(Paragraph::new(Line::styled(hint(p), DIM)), hint_area);
}

/// The key reminder at the bottom of the pop-up.
fn hint(p: &popup::Popup) -> &'static str {
    match p.tab {
        Tab::Speakers => "enter turn on/off · tab next tab · esc close",
        Tab::Queue => "enter jump to song · tab next tab · esc close",
        _ if p.typing() => {
            "enter play · ctrl-n play next · ctrl-a add to queue · tab next tab · esc close"
        }
        _ => "enter open/play · . play next · Q add to queue · h back · esc close",
    }
}
