//! The Settings screen: volumes, text size, map zoom and the display
//! switches, from the title screen or the game menu. The single-key
//! shortcuts in the game (mute, palette, labels...) go through `adjust` too,
//! so a setting changes the same way however it is reached, and is saved.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
};

use super::{
    App, Hit,
    audio::{self, Levels},
    keys::Act,
    palette::{Palette, PaletteKind},
    prefs,
    view::MapZoom,
    widgets::{self, FAINT, GOLD, SOFT, TEXT, chip, heading, keycap},
};
use crate::world::AlertLevel;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Item {
    Music,
    Effects,
    Mute,
    TextSize,
    Fullscreen,
    MapZoom,
    KeyPanel,
    Labels,
    Terrain,
    Palette,
    Mouse,
    Controls,
    Back,
}

pub struct SettingsState {
    pub sel: usize,
}

/// The rows, top to bottom (text size and fullscreen only in the game's window).
pub fn items(app: &App) -> Vec<Item> {
    use Item::*;
    [
        Music, Effects, Mute, TextSize, Fullscreen, MapZoom, KeyPanel, Labels, Terrain, Palette,
        Mouse, Controls, Back,
    ]
    .into_iter()
    .filter(|i| app.windowed || !matches!(i, TextSize | Fullscreen))
    .collect()
}

impl Item {
    fn name(self) -> &'static str {
        match self {
            Item::Music => "Music volume",
            Item::Effects => "Effects volume",
            Item::Mute => "Mute all sound",
            Item::TextSize => "Text size",
            Item::Fullscreen => "Fullscreen",
            Item::MapZoom => "Map zoom",
            Item::KeyPanel => "Keys panel",
            Item::Labels => "Town labels",
            Item::Terrain => "Terrain marks",
            Item::Palette => "Color-blind palette",
            Item::Mouse => "Mouse",
            Item::Controls => "Controls…",
            Item::Back => "Back",
        }
    }

    /// One line on what the row does, shown under the list.
    fn help(self) -> &'static str {
        match self {
            Item::Music => "How loud the soundtrack plays. 0% switches the music off.",
            Item::Effects => {
                "How loud whistles, bells and clicks are. You'll hear one as you change it."
            }
            Item::Mute => {
                "Silence everything at once; the volumes are kept for when you switch it back."
            }
            Item::TextSize => {
                "The size of all the text in the window. The map has its own zoom below."
            }
            Item::Fullscreen => "Fill the whole screen with the game.",
            Item::MapZoom => {
                "How big the map is drawn, without changing the text. The mouse wheel zooms too."
            }
            Item::KeyPanel => {
                "The column left of the map that lists the keys for what you're doing right now."
            }
            Item::Labels => "Town names and populations on the map.",
            Item::Terrain => "Faint marks on forests, hills, mountains and water.",
            Item::Palette => "Colors that stay apart for red-green color blindness.",
            Item::Mouse => "Point, click and scroll. Off hands the mouse back to the terminal.",
            Item::Controls => "See every key, and change any of them.",
            Item::Back => "Close Settings.",
        }
    }

    /// The game key that does the same, if there is one.
    fn act(self) -> Option<Act> {
        match self {
            Item::Mute => Some(Act::Sound),
            Item::TextSize => Some(Act::ZoomIn),
            Item::MapZoom => Some(Act::MapZoomIn),
            Item::KeyPanel => Some(Act::KeyPanel),
            Item::Labels => Some(Act::Labels),
            Item::Terrain => Some(Act::Terrain),
            Item::Palette => Some(Act::Palette),
            _ => None,
        }
    }
}

pub fn open(app: &mut App) {
    app.settings = Some(SettingsState { sel: 0 });
}

/// Change a setting: `dir` -1 / +1 moves a slider or choice, 0 means "the
/// row was picked" (switches flip, sliders step up). Saved at once.
pub fn adjust(app: &mut App, item: Item, dir: i32) {
    let flip = |on: bool| if dir == 0 { !on } else { dir > 0 };
    match item {
        Item::Music | Item::Effects => {
            let l = app.audio.levels;
            let up = dir >= 0;
            let l = match item {
                Item::Music => Levels {
                    music: audio::step_percent(l.music, up),
                    ..l
                },
                _ => Levels {
                    sfx: audio::step_percent(l.sfx, up),
                    ..l
                },
            };
            set_levels(app, Levels { muted: false, ..l });
        }
        Item::Mute => {
            if flip(app.audio.levels.muted) != app.audio.levels.muted {
                app.audio.toggle_mute();
            }
            app.flash(app.audio.levels.label(), AlertLevel::Info);
        }
        Item::TextSize => super::request_zoom(app, if dir == 0 { 1 } else { dir }),
        Item::Fullscreen => app.toggle_fullscreen = true,
        Item::MapZoom => {
            let z = app.map_zoom;
            let next = if dir < 0 { z.farther() } else { z.closer() };
            match (next, dir) {
                (Some(z), _) => set_map_zoom(app, z),
                (None, 0) => set_map_zoom(app, MapZoom::ALL[0]), // picked past the end: round again
                (None, _) => {}
            }
        }
        Item::KeyPanel => app.key_panel = flip(app.key_panel),
        Item::Labels => app.labels = flip(app.labels),
        Item::Terrain => app.terrain_marks = flip(app.terrain_marks),
        Item::Palette => {
            let cb = flip(app.pal.kind == PaletteKind::ColorBlind);
            app.pal = Palette::new(if cb {
                PaletteKind::ColorBlind
            } else {
                PaletteKind::Classic
            });
        }
        Item::Mouse => {
            app.mouse = flip(app.mouse);
            super::apply_mouse(app);
        }
        Item::Controls => super::open_controls(app),
        Item::Back => app.settings = None,
    }
    prefs::save(app);
}

/// "Town labels: off", for the message line after a shortcut key.
pub fn state_label(app: &App, item: Item) -> String {
    let on = match item {
        Item::KeyPanel => app.key_panel,
        Item::Labels => app.labels,
        Item::Terrain => app.terrain_marks,
        Item::Palette => app.pal.kind == PaletteKind::ColorBlind,
        Item::Mouse => app.mouse,
        Item::Mute => app.audio.levels.muted,
        Item::Fullscreen => app.fullscreen,
        _ => return item.name().to_string(),
    };
    format!(
        "{}: {}. (Esc → Settings has every option.)",
        item.name(),
        if on { "on" } else { "off" }
    )
}

fn set_levels(app: &mut App, l: Levels) {
    let louder_fx = l.sfx != app.audio.levels.sfx;
    app.audio.set_levels(l);
    if louder_fx {
        app.audio.play(audio::Sfx::Coin); // so you hear the new level
    }
}

/// Zoom the map, keeping the cursor where it is on screen as far as it can.
pub fn set_map_zoom(app: &mut App, z: MapZoom) {
    if z == app.map_zoom {
        return;
    }
    let before = app.view();
    let (ox, oy) = before.offset(app.cursor);
    app.map_zoom = z;
    // put the camera so the cursor's tile lands on the same screen cell
    let after = app.view();
    let back = after.tile_at_offset(ox, oy);
    app.cam = crate::types::Pos::new(
        app.cam.x + app.cursor.x - back.x,
        app.cam.y + app.cursor.y - back.y,
    );
    app.flash(
        app.keys.fill(&format!(
            "Map zoom: {}. {{map_zoom_in}} / {{map_zoom_out}} or the mouse wheel zoom the map.",
            z.name()
        )),
        AlertLevel::Info,
    );
    prefs::save(app);
}

/// Keys on the Settings screen (the game menu's keys, plus left and right).
pub fn key(app: &mut App, k: &ratatui::crossterm::event::KeyEvent) {
    let km = app.keys.clone();
    let list = items(app);
    let Some(st) = app.settings.as_mut() else {
        return;
    };
    st.sel = st.sel.min(list.len() - 1);
    let item = list[st.sel];
    if km.is(Act::MenuUp, k) {
        st.sel = st.sel.saturating_sub(1);
    } else if km.is(Act::MenuDown, k) {
        st.sel = (st.sel + 1).min(list.len() - 1);
    } else if km.is(Act::MenuLeft, k) {
        adjust(app, item, -1);
    } else if km.is(Act::MenuRight, k) {
        adjust(app, item, 1);
    } else if km.is(Act::MenuPick, k) {
        adjust(app, item, 0);
    } else if km.is(Act::MenuClose, k) {
        app.settings = None;
    } else if let Some(i) = list
        .iter()
        .position(|i| i.act().is_some_and(|a| km.is(a, k)) && *i != Item::TextSize)
    {
        // a setting's own key works here too (M mutes, P palette...)
        adjust(app, list[i], 0);
    }
}

/// A click on the Settings screen. `x` is the pointer's column, for the sliders.
pub fn click(app: &mut App, hit: Hit, x: u16) {
    let list = items(app);
    match hit {
        Hit::Setting(i) => {
            if let Some(st) = app.settings.as_mut() {
                st.sel = i;
            }
            // switches flip on a click; sliders and choices have their own parts
            if let Some(&item) = list.get(i)
                && !matches!(
                    item,
                    Item::Music | Item::Effects | Item::MapZoom | Item::TextSize
                )
            {
                adjust(app, item, 0);
            }
        }
        Hit::SettingStep(i, up) => {
            if let Some(&item) = list.get(i) {
                adjust(app, item, if up { 1 } else { -1 });
            }
        }
        Hit::SettingBar(i, r) => {
            // each cell of the bar is a tenth: clicking the third sets 30%
            let v = ((x.saturating_sub(r.x) + 1) * 100 / r.width.max(1)).min(100) as u8;
            let l = app.audio.levels;
            match list.get(i) {
                Some(Item::Music) => set_levels(
                    app,
                    Levels {
                        music: v,
                        muted: false,
                        ..l
                    },
                ),
                Some(Item::Effects) => set_levels(
                    app,
                    Levels {
                        sfx: v,
                        muted: false,
                        ..l
                    },
                ),
                _ => {}
            }
            prefs::save(app);
        }
        Hit::ZoomChoice(z) => set_map_zoom(app, z),
        _ => {}
    }
}

/// Width of a volume bar, in cells (one per 10%).
const BAR: u16 = 10;
/// Where the value column starts (after the 2-cell margin and the names).
const VALUE_X: u16 = 24;

pub fn draw(f: &mut Frame, app: &App) {
    let Some(st) = app.settings.as_ref() else {
        return;
    };
    let list = items(app);
    let sel = st.sel.min(list.len() - 1);
    let rows = list.len() as u16 + 4 + 5; // items, headings and gaps, help
    let area = widgets::centered(f.area(), 72, rows + 2);
    f.render_widget(Clear, area);
    let foot = app.keys.fill("{menu_up} {menu_down} choose · {menu_left} {menu_right} change · {menu_pick} switch · {menu_close} back");
    let block = widgets::popup("SETTINGS", foot);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let mut lines: Vec<Line> = vec![];
    let mut hits = app.hits.borrow_mut();
    let l = app.audio.levels;
    let first_display = list
        .iter()
        .position(|i| matches!(i, Item::TextSize | Item::Fullscreen | Item::MapZoom));
    for (i, &item) in list.iter().enumerate() {
        if item == Item::Music || Some(i) == first_display {
            lines.push(Line::from(""));
            lines.push(heading(if item == Item::Music {
                "SOUND"
            } else {
                "DISPLAY"
            }));
        } else if item == Item::Controls {
            lines.push(Line::from(""));
        }
        let y = inner.y + lines.len() as u16;
        if y >= inner.bottom() {
            break;
        }
        let focus = i == sel;
        hits.push((Rect::new(inner.x, y, inner.width, 1), Hit::Setting(i)));
        let mark = if focus {
            Span::styled("▶ ", Style::default().fg(GOLD).add_modifier(Modifier::BOLD))
        } else {
            Span::raw("  ")
        };
        let name_style = if focus {
            Style::default().fg(GOLD).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT)
        };
        let mut spans = vec![
            mark,
            Span::styled(format!("{:<22}", item.name()), name_style),
        ];
        let on_off = |on: bool| {
            Span::styled(
                if on { " ● on " } else { " ○ off" },
                Style::default()
                    .fg(if on { Color::Rgb(140, 230, 160) } else { SOFT })
                    .add_modifier(Modifier::BOLD),
            )
        };
        let arrow = |s: &'static str| {
            Span::styled(
                s,
                Style::default()
                    .fg(if focus { GOLD } else { FAINT })
                    .add_modifier(Modifier::BOLD),
            )
        };
        let x0 = inner.x + VALUE_X;
        match item {
            Item::Music | Item::Effects => {
                let v = if item == Item::Music { l.music } else { l.sfx };
                let filled = (v as u16 * BAR).div_ceil(100);
                let col = if l.muted {
                    FAINT
                } else {
                    Color::Rgb(140, 230, 160)
                };
                hits.push((Rect::new(x0, y, 2, 1), Hit::SettingStep(i, false)));
                hits.push((
                    Rect::new(x0 + 2, y, BAR, 1),
                    Hit::SettingBar(i, Rect::new(x0 + 2, y, BAR, 1)),
                ));
                hits.push((Rect::new(x0 + 2 + BAR, y, 2, 1), Hit::SettingStep(i, true)));
                spans.push(arrow("◀ "));
                spans.push(Span::styled(
                    "█".repeat(filled as usize),
                    Style::default().fg(col),
                ));
                spans.push(Span::styled(
                    "░".repeat((BAR - filled) as usize),
                    Style::default().fg(Color::Rgb(60, 66, 84)),
                ));
                spans.push(arrow(" ▶"));
                spans.push(Span::styled(
                    format!(" {v:>3}%"),
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                ));
                if l.muted {
                    spans.push(Span::styled("  (muted)", Style::default().fg(FAINT)));
                }
            }
            Item::Mute => spans.push(on_off(l.muted)),
            Item::TextSize => {
                let px = if app.font_px > 0.0 {
                    app.font_px
                } else {
                    super::TEXT_DEFAULT
                };
                hits.push((Rect::new(x0, y, 2, 1), Hit::SettingStep(i, false)));
                hits.push((Rect::new(x0 + 9, y, 2, 1), Hit::SettingStep(i, true)));
                spans.extend([
                    arrow("◀ "),
                    Span::styled(
                        format!("{px:>2.0} px"),
                        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                    ),
                    arrow("  ▶"),
                ]);
            }
            Item::Fullscreen => spans.push(on_off(app.fullscreen)),
            Item::MapZoom => {
                let mut x = x0;
                for z in MapZoom::ALL {
                    let c = chip(z.name(), z == app.map_zoom, focus);
                    let w = c.width() as u16;
                    hits.push((Rect::new(x, y, w, 1), Hit::ZoomChoice(z)));
                    spans.push(c);
                    x += w;
                }
            }
            Item::KeyPanel => spans.push(on_off(app.key_panel)),
            Item::Labels => spans.push(on_off(app.labels)),
            Item::Terrain => spans.push(on_off(app.terrain_marks)),
            Item::Palette => spans.push(on_off(app.pal.kind == PaletteKind::ColorBlind)),
            Item::Mouse => spans.push(on_off(app.mouse)),
            Item::Controls | Item::Back => {}
        }
        // the game key that does the same thing, so players learn it
        if let Some(a) = item.act() {
            let label = match item {
                Item::TextSize => app.keys.fill("{zoom_in} {zoom_out}"),
                Item::MapZoom => app.keys.fill("{map_zoom_in} {map_zoom_out}"),
                _ => app.keys.label(a),
            };
            let pad = (inner.width as usize)
                .saturating_sub(widgets::width(&spans) + label.chars().count() + 4);
            spans.push(Span::raw(" ".repeat(pad)));
            for (j, part) in label.split(' ').enumerate() {
                if j > 0 {
                    spans.push(Span::raw(" "));
                }
                spans.push(keycap(part));
            }
        }
        lines.push(Line::from(spans));
    }
    drop(hits);
    lines.push(Line::from(""));
    let help = Paragraph::new(Line::from(Span::styled(
        list[sel].help(),
        Style::default().fg(SOFT).add_modifier(Modifier::ITALIC),
    )))
    .wrap(Wrap { trim: true });
    let body_h = (lines.len() as u16).min(inner.height);
    f.render_widget(
        Paragraph::new(lines),
        Rect::new(inner.x, inner.y, inner.width, body_h),
    );
    let help_y = inner.y + body_h;
    if help_y < inner.bottom() {
        f.render_widget(
            help,
            Rect::new(
                inner.x + 2,
                help_y,
                inner.width.saturating_sub(4),
                inner.bottom() - help_y,
            ),
        );
    }
}
