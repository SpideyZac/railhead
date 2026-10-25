//! The keys panel left of the map: the handful of keys that matter for what
//! the player is doing right now (a tool out, a train selected, the cursor on
//! a station...), so nobody has to remember all of them. Rows are buttons:
//! clicking one presses its key. In the Tutorial, the keys the current step
//! wants come first and glow.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use super::{
    App, Hit, Mode, Place,
    keys::Act,
    tutorial,
    widgets::{FAINT, SKY, SOFT, TEXT, keycap_lit},
};
use crate::world::{SiteKind, build::LayerKind, vehicles::VKind};

/// One row: the key (by action, so rebinding shows) and what it does here.
pub struct Hint {
    pub act: Act,
    /// What the key cap says, when it isn't just the action's first key
    /// ("↑↓←→" for moving, "1-4" for the speeds).
    pub keys: Option<String>,
    pub what: String,
}

pub struct Section {
    pub title: String,
    pub hints: Vec<Hint>,
    /// The tutorial's own section: drawn lit.
    pub lit: bool,
}

fn h(act: Act, what: impl Into<String>) -> Hint {
    Hint {
        act,
        keys: None,
        what: what.into(),
    }
}

fn h_keys(act: Act, keys: String, what: impl Into<String>) -> Hint {
    Hint {
        act,
        keys: Some(keys),
        what: what.into(),
    }
}

fn section(title: impl Into<String>, hints: Vec<Hint>) -> Section {
    Section {
        title: title.into(),
        hints,
        lit: false,
    }
}

/// The panel's sections for the moment, most relevant first.
pub fn sections(app: &App) -> Vec<Section> {
    let Some(w) = app.world.as_ref() else {
        return vec![];
    };
    let km = &app.keys;
    let arrows = km.fill("{move_up}{move_down}{move_left}{move_right}");
    let mut out = vec![];
    if let (Some(tut), Some(step)) = (
        app.tutorial.as_ref(),
        app.tutorial.as_ref().and_then(|t| t.current()),
    ) {
        let want = tutorial::keys_now(app, w, tut);
        if !want.is_empty() && !tut.reviewing() {
            let hints = want
                .into_iter()
                .map(|(a, what)| {
                    if a == Act::MoveUp {
                        h_keys(a, arrows.clone(), what)
                    } else {
                        h(a, what)
                    }
                })
                .collect();
            out.push(Section {
                title: format!("TUTORIAL · {}", step.title.to_uppercase()),
                hints,
                lit: true,
            });
        }
    }
    if app.show_overview {
        out.push(section(
            "OVERVIEW",
            vec![
                h_keys(Act::MoveUp, arrows, "look around"),
                h(Act::Overview, "back to the map"),
            ],
        ));
        return out;
    }
    match &app.mode {
        Mode::Confirm(_) => {
            out.push(section(
                "QUESTION",
                vec![h(Act::Yes, "yes"), h(Act::Cancel, "no (any other key)")],
            ));
            return out;
        }
        Mode::Schedule { picking: true, .. } => {
            out.push(section(
                "PICK A STOP",
                vec![
                    h_keys(Act::MoveUp, arrows, "move to a stop"),
                    h(Act::Confirm, "add the stop here"),
                    h(Act::Cancel, "back to the list"),
                ],
            ));
            out.push(game_section(app));
            return out;
        }
        Mode::Schedule { .. } => {
            out.push(section(
                "SCHEDULE EDITOR",
                vec![
                    h_keys(
                        Act::SchedUp,
                        km.fill("{sched_up}{sched_down}"),
                        "choose a stop",
                    ),
                    h(Act::SchedAdd, "add a stop"),
                    h(Act::SchedDelete, "delete the stop"),
                    h(Act::SchedAction, "load / unload"),
                    h(Act::SchedCargo, "which cargo"),
                    h(Act::SchedCond, "when to leave"),
                    h(Act::SchedTimeout, "give up waiting"),
                    h(Act::SchedGate, "only go if…"),
                    h_keys(
                        Act::SchedMoveUp,
                        km.fill("{sched_move_up}{sched_move_down}"),
                        "move the stop",
                    ),
                    h(Act::SchedGo, "go there now"),
                    h(Act::SchedClose, "close the editor"),
                ],
            ));
            return out;
        }
        Mode::Line { kind, start } => {
            let what = if *kind == LayerKind::Rail {
                "LAYING TRACK"
            } else {
                "BUILDING ROAD"
            };
            let own = if *kind == LayerKind::Rail {
                Act::Rail
            } else {
                Act::Road
            };
            let hints = match start {
                None => vec![
                    h_keys(Act::MoveUp, arrows, "move to the start"),
                    h(Act::Confirm, "start here"),
                    h(Act::Cancel, "put the tool away"),
                ],
                Some(_) => vec![
                    h_keys(Act::MoveUp, arrows, "move to the end"),
                    h(Act::Confirm, "build to here"),
                    h(own, "build to here too"),
                    h(Act::Cancel, "drop the start"),
                ],
            };
            out.push(section(what, hints));
            out.push(game_section(app));
            return out;
        }
        Mode::Double { start } => {
            let hints = match start {
                None => vec![
                    h(Act::Confirm, "start here"),
                    h(Act::Cancel, "put the tool away"),
                ],
                Some(_) => vec![
                    h(Act::Confirm, "build to here"),
                    h(Act::Cancel, "drop the start"),
                ],
            };
            out.push(section("DOUBLE TRACK", hints));
            out.push(game_section(app));
            return out;
        }
        Mode::Place(p) => {
            let hints = match p {
                Place::Station => vec![
                    h(Act::Confirm, "station here"),
                    h(Act::Cancel, "stop placing"),
                ],
                Place::Signal => vec![
                    h(Act::Confirm, "signal / change it"),
                    h(Act::Chain, "block ↔ chain signal"),
                    h(Act::Cancel, "stop placing"),
                ],
                Place::Hub => vec![h(Act::Confirm, "hub here"), h(Act::Cancel, "stop placing")],
                Place::Demolish => vec![
                    h(Act::Confirm, "demolish here"),
                    h(Act::Demolish, "hold + move: clear"),
                    h(Act::Cancel, "stop demolishing"),
                ],
            };
            out.push(section(
                format!("PLACING {}", p.name().to_uppercase()),
                hints,
            ));
            out.push(game_section(app));
            return out;
        }
        Mode::Normal => {}
    }
    // a vehicle selected
    if let Some((c, kind)) = app.selected.and_then(|id| w.carrier(id)) {
        let mut v = vec![
            h(Act::Edit, "edit its schedule"),
            h(Act::Duplicate, "buy a copy"),
        ];
        if kind == VKind::Train {
            v.push(h_keys(
                Act::More,
                km.fill("{more} {less}"),
                "wagons (at station)",
            ));
            v.push(h(Act::Reverse, "turn it round"));
        }
        v.push(h(Act::Demolish, "scrap it"));
        v.push(h_keys(
            Act::NextVehicle,
            km.fill("{next_vehicle} {prev_vehicle}"),
            "next / previous",
        ));
        v.push(h(Act::Deselect, "deselect"));
        out.push(section(format!("SELECTED · {}", c.name.to_uppercase()), v));
    }
    // what's under the cursor
    let t = w.tile(app.cursor);
    let here: Option<(String, Vec<Hint>)> = if let Some(sid) = t.station {
        let s = &w.stations[sid as usize];
        Some((
            "THIS STATION".into(),
            vec![
                h(Act::BuyFreight, "buy a freight train"),
                h(Act::BuyExpress, "buy an express"),
                h_keys(
                    Act::More,
                    km.fill("{more} {less}"),
                    format!("train limit ({})", s.limit),
                ),
                h(Act::Demolish, "demolish tool"),
            ],
        ))
    } else if let Some(id) = t.site {
        let what = match w.sites[id as usize].kind {
            SiteKind::Town(_) => "THIS TOWN",
            SiteKind::Industry(_) => "THIS INDUSTRY",
            SiteKind::Hub => "THIS HUB",
        };
        Some((
            what.into(),
            vec![
                h(Act::BuyFreight, "buy a truck here"),
                h(Act::Rail, "lay track"),
                h(Act::Station, "station nearby"),
            ],
        ))
    } else if t.signal.is_some() {
        Some((
            "THIS SIGNAL".into(),
            vec![
                h(Act::Signal, "change / remove"),
                h(Act::Chain, "block ↔ chain"),
                h(Act::AutoSignal, "signal whole line"),
            ],
        ))
    } else if t.rail.present() {
        Some((
            "THIS TRACK".into(),
            vec![
                h(Act::Rail, "lay track from here"),
                h(Act::Station, "station here"),
                h(Act::Signal, "signal here"),
                h(Act::AutoSignal, "signal whole line"),
            ],
        ))
    } else if t.road.present() {
        Some((
            "THIS ROAD".into(),
            vec![
                h(Act::Road, "road from here"),
                h(Act::Demolish, "demolish tool"),
            ],
        ))
    } else if !w.vehicles_at(app.cursor).is_empty() && app.selected.is_none() {
        Some(("HERE".into(), vec![h(Act::Select, "select the vehicle")]))
    } else {
        None
    };
    if let Some((title, hints)) = here {
        out.push(section(title, hints));
    } else {
        out.push(section(
            "BUILD",
            vec![
                h(Act::Rail, "lay rail track"),
                h(Act::Road, "build road"),
                h(Act::Station, "place a station"),
                h(Act::Signal, "place a signal"),
                h(Act::Demolish, "demolish"),
            ],
        ));
    }
    out.push(game_section(app));
    out
}

/// Keys that work all the time.
fn game_section(app: &App) -> Section {
    let km = &app.keys;
    let pause = if app.speed == 0 {
        "start the clock"
    } else {
        "pause"
    };
    let mut v = vec![
        h(Act::Pause, pause),
        h_keys(Act::Speed1, km.fill("{speed_1}-{speed_4}"), "speed"),
        h_keys(
            Act::MapZoomIn,
            km.fill("{map_zoom_in} {map_zoom_out}"),
            "zoom the map",
        ),
        h(Act::Overview, "whole map"),
        h(Act::NextTab, "bottom panel tabs"),
        h(Act::Help, "handbook"),
    ];
    if app.mode == Mode::Normal && app.selected.is_none() {
        v.push(h(Act::Deselect, "menu & settings"));
    }
    section("ANY TIME", v)
}

/// The panel's width for a screen this wide (0: no room for it).
pub fn panel_width(app: &App, screen_width: u16) -> u16 {
    if !app.key_panel || screen_width < 118 {
        0
    } else if screen_width >= 150 {
        28
    } else {
        25
    }
}

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(Color::Rgb(60, 66, 90)))
        .style(Style::default().bg(Color::Rgb(18, 20, 28)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let mut lines: Vec<Line> = vec![];
    let mut hits = app.hits.borrow_mut();
    let width = inner.width as usize;
    for s in sections(app) {
        if lines.len() + 2 > inner.height as usize {
            break;
        }
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        let title_col = if s.lit { Color::Rgb(255, 214, 90) } else { SKY };
        let title: String = s.title.chars().take(width.saturating_sub(1)).collect();
        lines.push(Line::from(Span::styled(
            format!(" {title}"),
            Style::default().fg(title_col).add_modifier(Modifier::BOLD),
        )));
        for hint in s.hints {
            if lines.len() >= inner.height as usize {
                break;
            }
            let label = hint
                .keys
                .clone()
                .unwrap_or_else(|| app.keys.label(hint.act));
            let cap = keycap_lit(&label, s.lit, app.frame);
            let used = cap.width() + 2;
            let what: String = hint.what.chars().take(width.saturating_sub(used)).collect();
            let y = inner.y + lines.len() as u16;
            hits.push((Rect::new(inner.x, y, inner.width, 1), Hit::Key(hint.act)));
            lines.push(Line::from(vec![
                Span::raw(" "),
                cap,
                Span::raw(" "),
                Span::styled(what, Style::default().fg(if s.lit { TEXT } else { SOFT })),
            ]));
        }
    }
    drop(hits);
    // a reminder of where every key lives, at the bottom if there's room
    if (lines.len() as u16) + 3 <= inner.height {
        let foot = app.keys.fill("{controls} all keys · {key_panel} hide");
        let y = inner.bottom() - 1;
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {foot}"),
                Style::default().fg(FAINT),
            ))),
            Rect::new(inner.x, y, inner.width, 1),
        );
    }
    f.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Settings, World};

    fn acts(app: &App) -> Vec<Act> {
        sections(app)
            .into_iter()
            .flat_map(|s| s.hints.into_iter().map(|h| h.act))
            .collect()
    }

    /// The panel follows what the player is doing.
    #[test]
    fn hints_fit_the_moment() {
        let mut app = App::new(
            Some("42".into()),
            Settings::default(),
            std::env::temp_dir().join("rh_hints.json"),
        );
        app.start_game(World::new(42, Settings::default()));
        let w = app.world.as_ref().unwrap();
        // on the starter station: buy trains
        app.cursor = w.stations[0].pos;
        assert!(acts(&app).contains(&Act::BuyFreight));
        // the rail tool out: build here / cancel, no shop
        app.mode = Mode::Line {
            kind: LayerKind::Rail,
            start: None,
        };
        let a = acts(&app);
        assert!(
            a.contains(&Act::Confirm) && a.contains(&Act::Cancel) && !a.contains(&Act::BuyFreight)
        );
        // a truck selected: its schedule and a copy
        app.mode = Mode::Normal;
        app.selected = app
            .world
            .as_ref()
            .unwrap()
            .trucks
            .first()
            .map(|t| t.carrier.id);
        let a = acts(&app);
        assert!(
            a.contains(&Act::Edit) && a.contains(&Act::Duplicate) && !a.contains(&Act::Reverse),
            "trucks don't reverse"
        );
        // schedule editor
        app.mode = Mode::Schedule {
            vehicle: app.selected.unwrap(),
            sel: 0,
            picking: false,
        };
        assert!(acts(&app).contains(&Act::SchedAdd));
        assert_eq!(panel_width(&app, 100), 0, "no room on a small screen");
        app.key_panel = false;
        assert_eq!(panel_width(&app, 200), 0, "switched off");
    }
}
