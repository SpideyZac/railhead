//! Small pieces of look shared by the screens: colors, key caps, the boxes
//! menus sit in. One place, so the title screen, the game menu, Settings and
//! the keys panel all look like the same game.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders},
};

pub const GOLD: Color = Color::Rgb(255, 200, 90);
pub const SKY: Color = Color::Rgb(120, 200, 255);
/// Text on the dark panels: normal, quieter, and quietest.
pub const TEXT: Color = Color::Rgb(225, 228, 235);
pub const SOFT: Color = Color::Rgb(160, 168, 186);
pub const FAINT: Color = Color::Rgb(105, 112, 132);
/// The background of popups and menus.
pub const PANEL_BG: Color = Color::Rgb(14, 16, 24);

/// A key drawn like a key cap: " T " in dark on light.
pub fn keycap(label: &str) -> Span<'static> {
    Span::styled(
        format!(" {label} "),
        Style::default()
            .fg(Color::Rgb(20, 22, 30))
            .bg(Color::Rgb(205, 212, 226))
            .add_modifier(Modifier::BOLD),
    )
}

/// A key cap that pulses (the tutorial points at it), else a plain one.
pub fn keycap_lit(label: &str, lit: bool, frame: u64) -> Span<'static> {
    if !lit {
        return keycap(label);
    }
    let bg = if (frame / 8).is_multiple_of(2) {
        Color::Rgb(255, 214, 90)
    } else {
        Color::Rgb(255, 160, 60)
    };
    Span::styled(
        format!(" {label} "),
        Style::default()
            .fg(Color::Black)
            .bg(bg)
            .add_modifier(Modifier::BOLD),
    )
}

/// The style of a highlighted menu row (and an ordinary one).
pub fn row_style(selected: bool) -> Style {
    if selected {
        Style::default()
            .fg(Color::Black)
            .bg(GOLD)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(TEXT)
    }
}

/// A clickable choice among a few ("Far  Normal  Close"): the picked one
/// stands out, brighter still when its row is the one being changed.
pub fn chip(text: &str, picked: bool, row_focus: bool) -> Span<'static> {
    let st = match (picked, row_focus) {
        (true, true) => Style::default()
            .fg(Color::Black)
            .bg(GOLD)
            .add_modifier(Modifier::BOLD),
        (true, false) => Style::default()
            .fg(GOLD)
            .bg(Color::Rgb(46, 42, 30))
            .add_modifier(Modifier::BOLD),
        _ => Style::default().fg(SOFT),
    };
    Span::styled(format!(" {text} "), st)
}

/// A popup box with a gold border and a title, and a hint line along the bottom.
pub fn popup(title: &str, foot: String) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(GOLD))
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(GOLD).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(
            Line::from(Span::styled(format!(" {foot} "), Style::default().fg(SOFT))).centered(),
        )
        .style(Style::default().bg(PANEL_BG))
}

/// A section heading inside a popup ("SOUND").
pub fn heading(s: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!("  {s}"),
        Style::default().fg(SKY).add_modifier(Modifier::BOLD),
    ))
}

/// A `w`x`h` box in the middle of `area` (smaller if the area is).
pub fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

/// Word-wrap `text` to `width` cells, drawing every `{action}` placeholder
/// as a key cap (lit ones pulse), so the keys to press stand out.
pub fn rich_wrap(
    km: &super::keys::Keymap,
    text: &str,
    width: usize,
    style: Style,
    lit: Option<u64>,
) -> Vec<Line<'static>> {
    let mut lines = vec![];
    let mut cur: Vec<Span<'static>> = vec![];
    let mut used = 0;
    for word in text.split_whitespace() {
        // a word is plain text and key caps, e.g. "({speed_3}"
        let mut parts: Vec<Span<'static>> = vec![];
        let mut rest = word;
        while let Some(i) = rest.find('{') {
            let Some(j) = rest[i..].find('}') else { break };
            if i > 0 {
                parts.push(Span::styled(rest[..i].to_string(), style));
            }
            let key = km.fill(&rest[i..i + j + 1]);
            parts.push(match lit {
                Some(frame) => keycap_lit(&key, true, frame),
                None => keycap(&key),
            });
            rest = &rest[i + j + 1..];
        }
        if !rest.is_empty() {
            parts.push(Span::styled(rest.to_string(), style));
        }
        let w = width_of(&parts);
        if used > 0 && used + 1 + w > width {
            lines.push(Line::from(std::mem::take(&mut cur)));
            used = 0;
        }
        if used > 0 {
            cur.push(Span::styled(" ", style));
            used += 1;
        }
        used += w;
        cur.extend(parts);
    }
    if !cur.is_empty() {
        lines.push(Line::from(cur));
    }
    lines
}

fn width_of(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.width()).sum()
}

/// How many cells wide a line is.
pub fn width(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.width()).sum()
}
