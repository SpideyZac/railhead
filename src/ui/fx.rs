//! Eye candy: fireworks for big moments, steam from moving trains, the
//! Railhead Gazette's headlines and the Charter's sunset. All of it is UI
//! state that reads the world and never changes it, and all of it runs on
//! `App::frame` (real time), so tests can step it by hand.

use std::collections::VecDeque;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use super::{
    App,
    palette::{lerp, rgb, scale},
};
use crate::{
    types::Pos,
    world::{Fanfare, VState, World},
};

/// How long things last, in frames (30 a second).
pub const BURST_FRAMES: u64 = 42;
pub const PUFF_FRAMES: u64 = 36;
pub const HEADLINE_FRAMES: u64 = 170;
/// The headline banner slides up out of the way (or back) in this many frames.
const DRAWER_FRAMES: f32 = 6.0;
/// A moving train puffs every this many frames.
const PUFF_EVERY: u64 = 5;
/// The sun starts setting this many game seconds before a Charter ends.
pub const SUNSET_SECS: f64 = 600.0;

pub struct Burst {
    pub pos: Pos,
    /// Frame it goes off (bursts can be queued a little apart).
    pub born: u64,
    pub color: (u8, u8, u8),
}

pub struct Puff {
    pub x: f32,
    pub y: f32,
    pub born: u64,
}

pub struct Headline {
    pub kind: Fanfare,
    /// The banner's title line ("THE RAILHEAD GAZETTE", "MILESTONE"...).
    pub kicker: String,
    pub title: String,
    pub body: String,
    /// When it went up (None while waiting its turn).
    pub shown: Option<u64>,
}

#[derive(Default)]
pub struct Fx {
    pub bursts: Vec<Burst>,
    pub puffs: Vec<Puff>,
    pub headlines: VecDeque<Headline>,
    /// How far the banner is tucked up out of the way, 0 (down) to 1 (away):
    /// it slides up while the mouse is over where it hangs (`update`).
    pub drawer: f32,
    /// Where the banner hangs when down, from the last frame drawn.
    pub banner: std::cell::Cell<Option<Rect>>,
    pub(super) drawer_frame: u64,
    /// Alerts already looked at (`World::alert_count`).
    pub(super) seen: u64,
    pub(super) last_puff: u64,
}

impl Fx {
    /// Fresh effects for a world, ignoring anything that already happened in it.
    pub fn new(w: &World) -> Fx {
        Fx {
            seen: w.alert_count,
            ..Fx::default()
        }
    }

    pub fn headline(&self) -> Option<&Headline> {
        self.headlines.front().filter(|h| h.shown.is_some())
    }
}

const GOLD: (u8, u8, u8) = (255, 210, 90);
const SPARKS: [(u8, u8, u8); 5] = [
    (255, 210, 90),
    (255, 120, 120),
    (120, 220, 255),
    (170, 255, 140),
    (255, 160, 240),
];

/// Once a frame: react to new big moments, age everything, puff steam.
pub fn update(app: &mut App) {
    let frame = app.frame;
    let Some(w) = app.world.as_ref() else { return };
    let fx = &mut app.fx;
    fx.bursts.retain(|b| frame < b.born + BURST_FRAMES);
    fx.puffs.retain(|p| frame < p.born + PUFF_FRAMES);
    if fx
        .headlines
        .front()
        .is_some_and(|h| h.shown.is_some_and(|s| frame >= s + HEADLINE_FRAMES))
    {
        fx.headlines.pop_front();
    }
    if let Some(h) = fx.headlines.front_mut() {
        h.shown.get_or_insert(frame);
    }

    if w.alert_count > fx.seen {
        let new = ((w.alert_count - fx.seen) as usize).min(w.alerts.len());
        fx.seen = w.alert_count;
        for a in &w.alerts[w.alerts.len() - new..] {
            let Some(kind) = a.fanfare else { continue };
            let pos = a.pos.unwrap_or(w.towns[w.map.home].pos);
            let n = fx.bursts.len() as u64;
            match kind {
                Fanfare::Medal => {
                    for i in 0..4 {
                        let off = [(0, 0), (-5, -2), (5, -1), (0, -3)][i];
                        fx.bursts.push(Burst {
                            pos: Pos::new(pos.x + off.0, pos.y + off.1),
                            born: frame + i as u64 * 9,
                            color: SPARKS[i % SPARKS.len()],
                        });
                    }
                }
                Fanfare::News => {}
                _ => fx.bursts.push(Burst {
                    pos,
                    born: frame + n % 3 * 4,
                    color: SPARKS[(n as usize + a.text.len()) % SPARKS.len()],
                }),
            }
            let text = app.keys.fill(&a.text);
            let headline = match kind {
                Fanfare::News => {
                    // "▤ HEADLINE — body"
                    let t = text.trim_start_matches("▤ ");
                    let (title, body) = t.split_once(" — ").unwrap_or((t, ""));
                    Some((
                        "THE RAILHEAD GAZETTE · EXTRA!",
                        title.to_string(),
                        body.to_string(),
                    ))
                }
                Fanfare::Milestone => {
                    let t = text.trim_start_matches("✦ MILESTONE: ");
                    let (title, body) = t.split_once(" — ").unwrap_or((t, ""));
                    Some(("✦ MILESTONE ✦", title.to_string(), body.to_string()))
                }
                Fanfare::Medal => {
                    let t = text.trim_start_matches("● ");
                    let (title, body) = t.split_once("! ").unwrap_or((t, ""));
                    Some((
                        "● THE RAILWAY BOARD AWARDS ●",
                        format!("{title}!"),
                        body.to_string(),
                    ))
                }
                _ => None,
            };
            if let Some((kicker, title, body)) = headline {
                fx.headlines.push_back(Headline {
                    kind,
                    kicker: kicker.into(),
                    title,
                    body,
                    shown: None,
                });
            }
        }
        // a backlog of news (e.g. at 8x speed) shouldn't keep the banner up for a minute
        while fx.headlines.len() > 3 {
            fx.headlines.remove(1);
        }
    }
    if fx
        .headlines
        .front()
        .is_some_and(|h| h.shown.is_some_and(|s| frame >= s + HEADLINE_FRAMES))
    {
        fx.headlines.pop_front();
    }
    if let Some(h) = fx.headlines.front_mut() {
        h.shown.get_or_insert(frame);
    }

    // The banner tucks itself up like a drawer while the mouse is over where
    // it hangs (the player is working there), and comes back once it leaves.
    // Its clock waits while it's away, so the news isn't missed.
    let steps = frame.saturating_sub(fx.drawer_frame).min(30) as f32;
    fx.drawer_frame = frame;
    let over = match (fx.banner.get(), app.mouse_pos) {
        (Some(r), Some((x, y))) if fx.headline().is_some() => {
            r.contains(ratatui::layout::Position { x, y })
        }
        _ => false,
    };
    let goal = if over { 1.0 } else { 0.0 };
    let step = steps / DRAWER_FRAMES;
    fx.drawer = if goal > fx.drawer {
        (fx.drawer + step).min(goal)
    } else {
        (fx.drawer - step).max(goal)
    };
    if fx.drawer > 0.5
        && let Some(s) = fx.headlines.front_mut().and_then(|h| h.shown.as_mut())
    {
        *s += steps as u64;
    }
    if fx.headline().is_none() {
        fx.banner.set(None);
    }

    // Steam: every moving train puffs from its engine.
    if frame >= fx.last_puff + PUFF_EVERY && app.speed > 0 {
        fx.last_puff = frame;
        for t in w
            .trains
            .iter()
            .filter(|t| t.carrier.state == VState::Travel)
        {
            let h = t.head();
            fx.puffs.push(Puff {
                x: h.x as f32,
                y: h.y as f32 - 1.0,
                born: frame,
            });
        }
        if fx.puffs.len() > 400 {
            fx.puffs.drain(0..100);
        }
    }
}

/// The Railway Board's briefing at the start of a new Charter: the goal and the medals.
pub fn briefing(app: &mut App) {
    let Some(w) = app.world.as_ref() else { return };
    if w.sandbox() || w.time > 0.0 {
        return;
    }
    let n = |v: f64| crate::world::money(v).replacen('£', "", 1);
    if w.franchise() {
        use crate::balance as B;
        let (first, second) = (w.review_target(1), w.review_target(2));
        app.fx.headlines.push_back(Headline {
            kind: Fanfare::Medal,
            kicker: "⚖ THE RAILWAY BOARD · YOUR FRANCHISE ⚖".into(),
            title: "The railway is yours, for as long as you deserve it".into(),
            body: format!(
                "The Board reviews you every {:.0} minutes and wants more each time: Prosperity {} at the first, {} at the second. Fail {} reviews, or face one in the red, and the franchise goes to someone else.",
                B::REVIEW_EVERY_SECS as f64 / 60.0,
                n(first),
                n(second),
                B::REVIEW_STRIKES
            ),
            shown: None,
        });
        return;
    }
    let [b, s, g] = w.medal_targets();
    app.fx.headlines.push_back(Headline {
        kind: Fanfare::Medal,
        kicker: "● THE RAILWAY BOARD · YOUR CHARTER ●".into(),
        title: format!("Grow the region in {:.0} minutes", crate::balance::CHARTER_MINUTES),
        body: format!("Bronze at {} · Silver at {} · Gold at {}. Feed the towns, connect them all, and mind the junctions.", n(b), n(s), n(g)),
        shown: None,
    });
}

/// How far into the Charter's sunset we are, 0 (day) to 1 (dusk).
pub fn dusk(w: &World) -> f64 {
    match w.time_left() {
        Some(left) if !w.finished => (1.0 - left / SUNSET_SECS).clamp(0.0, 1.0),
        Some(_) => 1.0,
        None => 0.0,
    }
}

/// Evening light on a background color.
pub fn dusk_bg(c: (u8, u8, u8), d: f64) -> (u8, u8, u8) {
    if d <= 0.0 {
        return c;
    }
    let evening = (
        (c.0 as f64 * 0.8 + 28.0).min(255.0) as u8,
        (c.1 as f64 * 0.58 + 6.0) as u8,
        (c.2 as f64 * 0.7 + 22.0) as u8,
    );
    lerp(c, evening, d)
}

/// Is the map cell at screen (x, y) free for steam or sparks to show on?
fn open_cell(buf: &Buffer, x: u16, y: u16) -> bool {
    buf.cell((x, y))
        .is_some_and(|c| matches!(c.symbol(), " " | "♣" | "∩" | "▲" | "~" | "·"))
}

/// Draw steam and fireworks over the map.
pub fn draw_map_fx(buf: &mut Buffer, app: &App) {
    let frame = app.frame;
    let view = app.view();
    let to_screen = |x: f32, y: f32| -> Option<(u16, u16)> { view.cell_of(x, y) };
    for p in &app.fx.puffs {
        let age = frame.saturating_sub(p.born) as f32;
        let t = age / PUFF_FRAMES as f32;
        // rises, drifts downwind, thins out
        let Some((x, y)) = to_screen(p.x + age / 14.0, p.y - age / 12.0) else {
            continue;
        };
        if !open_cell(buf, x, y) {
            continue;
        }
        let ch = if t < 0.3 {
            'O'
        } else if t < 0.65 {
            'o'
        } else {
            '°'
        };
        if let Some(cell) = buf.cell_mut((x, y)) {
            let bg = color_rgb(cell.bg);
            let fg = lerp((235, 235, 230), bg, t as f64 * 0.8);
            cell.reset();
            cell.set_char(ch)
                .set_style(Style::default().fg(rgb(fg)).bg(rgb(bg)));
        }
    }
    for b in &app.fx.bursts {
        if frame < b.born {
            continue;
        }
        let age = (frame - b.born) as f32;
        let t = age / BURST_FRAMES as f32;
        let r = 1.0 + age * 0.28;
        for i in 0..14 {
            let a = i as f32 / 14.0 * std::f32::consts::TAU + b.born as f32;
            // cells are about twice as tall as wide; sparks fall a little as they fade
            let (dx, dy) = (a.cos() * r * 1.6, a.sin() * r * 0.8 + t * t * 2.5);
            let Some((x, y)) = to_screen(b.pos.x as f32 + dx, b.pos.y as f32 + dy) else {
                continue;
            };
            let ch = if t < 0.25 {
                '✶'
            } else if t < 0.6 {
                '+'
            } else {
                '·'
            };
            if let Some(cell) = buf.cell_mut((x, y)) {
                let bg = color_rgb(cell.bg);
                let fg = lerp(b.color, bg, (t as f64 - 0.4).max(0.0) * 1.4);
                cell.reset();
                cell.set_char(ch).set_style(
                    Style::default()
                        .fg(rgb(fg))
                        .bg(rgb(bg))
                        .add_modifier(Modifier::BOLD),
                );
            }
        }
        // a bright flash at the heart for the first moments
        if t < 0.15
            && let Some((x, y)) = to_screen(b.pos.x as f32, b.pos.y as f32 - 1.0)
            && let Some(cell) = buf.cell_mut((x, y))
        {
            let bg = cell.bg;
            cell.reset();
            cell.set_char('✺').set_style(
                Style::default()
                    .fg(rgb((255, 255, 230)))
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            );
        }
    }
}

/// The current headline, as a banner across the top of the map.
pub fn draw_headline(buf: &mut Buffer, app: &App, area: Rect) {
    let Some(h) = app.fx.headline() else { return };
    let age = app.frame.saturating_sub(h.shown.unwrap_or(app.frame));
    let width = area.width.clamp(30, 84);
    if area.width < 30 || area.height < 8 {
        return;
    }
    let inner = width as usize - 4;
    let body = super::draw::wrap_words(&h.body, inner);
    let height = (4 + body.len().min(2)) as u16;
    // slide down from the top edge, then sit
    let drop = (age as u16).min(height);
    let x0 = area.x + (area.width - width) / 2;
    let (paper, ink, accent) = match h.kind {
        Fanfare::News => ((236, 226, 200), (30, 26, 22), (150, 30, 30)),
        Fanfare::Medal => ((40, 32, 10), (255, 235, 170), GOLD),
        _ => ((18, 30, 46), (225, 235, 250), (120, 200, 255)),
    };
    let mut rows: Vec<(String, (u8, u8, u8), bool)> = vec![
        (center(&h.kicker, inner), accent, true),
        (center(&h.title.to_uppercase(), inner), ink, true),
    ];
    for l in body.iter().take(2) {
        rows.push((center(l, inner), scale(ink, 0.85), false));
    }
    let top = area.y + 1;
    let total = rows.len() as u16 + 2;
    // where it hangs when down: the mouse over this tucks it away
    app.fx.banner.set(Some(Rect::new(
        x0,
        top,
        width,
        total.min(area.y + area.height - top),
    )));
    // tucked up into the top of the map, like a drawer (by whole rows; the
    // window shows them going one at a time, which reads as a slide)
    let tucked = (app.fx.drawer * (total + 1) as f32).round() as i32;
    for (i, row) in std::iter::once(None)
        .chain(rows.iter().map(Some))
        .chain(std::iter::once(None))
        .enumerate()
    {
        // rows appear top-down as the banner slides in
        if i as u16 >= drop {
            break;
        }
        let y = top as i32 + i as i32 - tucked;
        if y < area.y as i32 {
            continue; // in the drawer
        }
        let y = y as u16;
        if y >= area.y + area.height {
            break;
        }
        for dx in 0..width {
            let x = x0 + dx;
            let edge = dx == 0 || dx == width - 1;
            let (ch, fg, bold) = match row {
                None => ('━', accent, false),
                Some(_) if edge => ('┃', accent, false),
                Some((text, fg, bold)) => {
                    let c = text
                        .chars()
                        .nth(dx as usize - 2)
                        .filter(|_| dx >= 2)
                        .unwrap_or(' ');
                    (c, *fg, *bold)
                }
            };
            let ch = match (row, dx) {
                (None, 0) if i == 0 => '┏',
                (None, d) if i == 0 && d == width - 1 => '┓',
                (None, 0) => '┗',
                (None, d) if d == width - 1 => '┛',
                _ => ch,
            };
            if let Some(cell) = buf.cell_mut((x, y)) {
                let mut st = Style::default().fg(rgb(fg)).bg(rgb(paper));
                if bold {
                    st = st.add_modifier(Modifier::BOLD);
                }
                cell.reset();
                cell.set_char(ch).set_style(st);
            }
        }
    }
    // put away: a little tab on the map's top edge says what's in the drawer
    if tucked > total as i32 {
        let tab = format!(" ▾ {} ▾ ", h.title.to_uppercase());
        let tab: String = tab.chars().take(width as usize).collect();
        let n = tab.chars().count() as u16;
        let x = area.x + (area.width - n) / 2;
        for (k, ch) in tab.chars().enumerate() {
            if let Some(cell) = buf.cell_mut((x + k as u16, area.y)) {
                cell.reset();
                cell.set_char(ch).set_style(
                    Style::default()
                        .fg(rgb(accent))
                        .bg(rgb(paper))
                        .add_modifier(Modifier::BOLD),
                );
            }
        }
    }
}

fn center(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n >= width {
        return s.chars().take(width).collect();
    }
    format!("{}{s}", " ".repeat((width - n) / 2))
}

fn color_rgb(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (20, 22, 30),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{AlertLevel, Settings};

    #[test]
    fn big_moments_get_fireworks_and_headlines() {
        let mut app = App::new(
            Some("4".into()),
            Settings::default(),
            std::env::temp_dir().join("rh_fx_save.json"),
        );
        app.start_game(World::new(4, Settings::default()));
        let w = app.world.as_mut().unwrap();
        w.celebrate(
            "✦ MILESTONE: All aboard! +£400 — A whistle.".into(),
            None,
            Fanfare::Milestone,
        );
        w.push_alert(
            AlertLevel::Good,
            "▤ NEWS AT TEN — Something happened.".into(),
            None,
            Some(Fanfare::News),
        );
        app.frame = 10;
        update(&mut app);
        assert_eq!(app.fx.bursts.len(), 1, "milestone fireworks, none for news");
        let h = app.fx.headline().expect("a headline is up");
        assert_eq!(h.title, "All aboard! +£400");
        app.frame += HEADLINE_FRAMES;
        update(&mut app);
        assert_eq!(
            app.fx.headline().map(|h| h.title.as_str()),
            Some("NEWS AT TEN"),
            "the next one follows"
        );
        app.frame += HEADLINE_FRAMES + BURST_FRAMES;
        update(&mut app);
        assert!(
            app.fx.headline().is_none() && app.fx.bursts.is_empty(),
            "all gone"
        );
    }

    /// The mouse over the banner slides it up into a drawer (a tab stays on
    /// the map's top edge); it comes back when the mouse leaves, and its
    /// clock waits meanwhile.
    #[test]
    fn the_banner_tucks_away_under_the_mouse() {
        use ratatui::{Terminal, backend::TestBackend};
        let mut app = App::new(
            Some("4".into()),
            Settings::default(),
            std::env::temp_dir().join("rh_fx_drawer.json"),
        );
        app.start_game(World::new(4, Settings::default()));
        app.fx.headlines.clear();
        app.world.as_mut().unwrap().push_alert(
            AlertLevel::Good,
            "▤ NEWS AT TEN — Something happened.".into(),
            None,
            Some(Fanfare::News),
        );
        let mut term = Terminal::new(TestBackend::new(160, 48)).unwrap();
        let mut frame = |app: &mut App, n: u64| {
            app.frame += n;
            update(app);
            term.draw(|f| super::super::draw::draw(f, app)).unwrap();
            term.backend()
                .buffer()
                .content()
                .iter()
                .map(|c| c.symbol())
                .collect::<String>()
        };
        frame(&mut app, 30); // it goes up
        let screen = frame(&mut app, 10); // and has slid down
        assert!(screen.contains("RAILHEAD GAZETTE"), "the banner is down");
        let r = app.fx.banner.get().expect("it says where it hangs");
        app.mouse_pos = Some((r.x + r.width / 2, r.y + r.height - 1));
        let shown = app.fx.headline().unwrap().shown;
        for _ in 0..10 {
            frame(&mut app, 1);
        }
        let screen = frame(&mut app, 1);
        assert_eq!(app.fx.drawer, 1.0);
        assert!(
            !screen.contains("RAILHEAD GAZETTE") && screen.contains("▾ NEWS AT TEN ▾"),
            "tucked away, a tab left"
        );
        assert!(app.fx.headline().unwrap().shown > shown, "its clock waited");
        app.mouse_pos = Some((0, 40));
        for _ in 0..10 {
            frame(&mut app, 1);
        }
        assert_eq!(app.fx.drawer, 0.0);
        assert!(
            frame(&mut app, 1).contains("RAILHEAD GAZETTE"),
            "and back down"
        );
    }

    #[test]
    fn the_sun_sets_on_the_charter() {
        let mut w = World::new(4, Settings::default());
        assert_eq!(dusk(&w), 0.0);
        w.time = crate::balance::CHARTER_MINUTES * 60.0 - SUNSET_SECS / 2.0;
        assert!((dusk(&w) - 0.5).abs() < 1e-9);
        assert_eq!(dusk_bg((100, 100, 100), 0.0), (100, 100, 100));
        let e = dusk_bg((100, 100, 100), 1.0);
        assert!(e.1 < 100 && e.0 > e.1, "warmer and darker");
    }
}
