//! All rendering. Reads `App`/`World`, never mutates game state.

use std::collections::HashMap;

use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use super::{
    hints,
    keys::{self, Act},
    palette::{TRAIN_COLORS, lerp, rgb, scale},
    settings,
    view::{MapBlock, MapZoom},
    widgets, *,
};
use crate::{
    map::{IndustryKind, Terrain},
    rng::hash2,
    types::{Cargo, Dir},
    world::{
        AlertLevel, Layer, SignalKind, VState,
        contracts::ContractState,
        vehicles::{Gate, GateReading, Stop},
    },
};

pub fn draw(f: &mut Frame, app: &mut App) {
    app.hits.borrow_mut().clear();
    app.map_blocks.borrow_mut().clear();
    match app.screen {
        Screen::Title => draw_title(f, app),
        Screen::Game => draw_game(f, app),
    }
    if app.menu.is_some() {
        draw_menu(f, app);
    }
    if app.settings.is_some() {
        settings::draw(f, app);
    }
    if app.show_help {
        handbook::draw(f, app);
    }
    if app.controls.is_some() {
        draw_controls(f, app);
    }
}

/// The Controls screen: every action, grouped by where it works, with its keys.
fn draw_controls(f: &mut Frame, app: &App) {
    let Some(st) = app.controls.as_ref() else {
        return;
    };
    let area = f.area();
    let r = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height,
    );
    let accent = Color::Rgb(255, 200, 90);
    let mut rows: Vec<Line> = vec![];
    let mut sel_row = 0;
    let mut group = None;
    for (i, a) in keys::ACTS.iter().enumerate() {
        if group != Some(a.group) {
            group = Some(a.group);
            rows.push(Line::from(""));
            rows.push(Line::from(Span::styled(
                format!("── {} ", a.group.name()),
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            )));
        }
        let selected = i == st.sel;
        if selected {
            sel_row = rows.len();
        }
        let keys = match st.capture {
            Some(add) if selected => format!(
                "press a key to {} … (Esc cancels)",
                if add { "add" } else { "use" }
            ),
            _ => app.keys.labels(a.act),
        };
        let (ds, ks) = if selected {
            let s = Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(120, 200, 255))
                .add_modifier(Modifier::BOLD);
            (s, s)
        } else {
            let unbound = app.keys.keys(a.act).is_empty();
            (
                Style::default().fg(Color::White),
                Style::default()
                    .fg(if unbound {
                        app.pal.bad
                    } else {
                        Color::Rgb(120, 200, 255)
                    })
                    .add_modifier(Modifier::BOLD),
            )
        };
        rows.push(Line::from(vec![
            Span::styled(format!("  {:<58}", a.desc), ds),
            Span::styled(format!(" {keys} "), ks),
        ]));
    }
    let mut foot = vec![Line::from(Span::styled(
        "↑↓ or j/k: choose · Enter: set key · Tab: add another key · Backspace: default · Ctrl+R: all defaults · Esc: save & close",
        Style::default().fg(Color::Gray),
    ))];
    for (a, b, k) in app.keys.clashes() {
        foot.push(Line::from(Span::styled(
            format!(
                "⚠ {} does two things here: \"{}\" and \"{}\"",
                k.label(),
                a.info().desc,
                b.info().desc
            ),
            Style::default().fg(app.pal.warn),
        )));
    }
    let inner_h = r.height.saturating_sub(2 + foot.len() as u16) as usize;
    let scroll = sel_row
        .saturating_sub(inner_h / 2)
        .min(rows.len().saturating_sub(inner_h));
    let body: Vec<Line> = rows
        .into_iter()
        .skip(scroll)
        .take(inner_h)
        .chain(foot)
        .collect();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent))
        .title(Span::styled(
            " CONTROLS ",
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(Color::Rgb(12, 14, 20)));
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(body).block(block), r);
}

fn level_color(app: &App, l: AlertLevel) -> Color {
    match l {
        AlertLevel::Info => app.pal.info,
        AlertLevel::Warn => app.pal.warn,
        AlertLevel::Danger => app.pal.bad,
        AlertLevel::Good => app.pal.good,
    }
}

// ------------------------------------------------------------------ title

const LOGO: [&str; 5] = [
    r" ____      _    ___ _     _   _ _____    _    ____  ",
    r"|  _ \    / \  |_ _| |   | | | | ____|  / \  |  _ \ ",
    r"| |_) |  / _ \  | || |   | |_| |  _|   / _ \ | | | |",
    r"|  _ <  / ___ \ | || |___|  _  | |___ / ___ \| |_| |",
    r"|_| \_\/_/   \_\___|_____|_| |_|_____/_/   \_\____/ ",
];

/// The title menu's width; it sits in the middle of the screen.
const TITLE_W: u16 = 78;
/// Width of the title menu's label column.
const TITLE_LABEL: usize = 13;

/// The little train chugging across the title screen: smoke, then track.
fn title_train(app: &App) -> [Line<'static>; 2] {
    let width = 60usize;
    let pos = (app.frame / 2) as usize % (width + 16);
    let train = "▐█▙▄▓▓▓▓▓▓▓▓";
    let mut track: Vec<char> = "═".repeat(width).chars().collect();
    for (i, ch) in train.chars().rev().enumerate() {
        let x = pos as isize - i as isize;
        if x >= 0 && (x as usize) < width {
            track[x as usize] = ch;
        }
    }
    let smoke = if (app.frame / 6).is_multiple_of(2) {
        "  o O"
    } else {
        " O o "
    };
    let mut sm = vec![' '; width];
    for (i, ch) in smoke.chars().enumerate() {
        let x = pos as isize - 6 + i as isize;
        if x >= 0 && (x as usize) < width {
            sm[x as usize] = ch;
        }
    }
    [
        Line::from(Span::styled(
            sm.into_iter().collect::<String>(),
            Style::default().fg(Color::Gray),
        ))
        .centered(),
        Line::from(Span::styled(
            track.into_iter().collect::<String>(),
            Style::default().fg(Color::Rgb(200, 200, 210)),
        ))
        .centered(),
    ]
}

/// What a title row is about, in a sentence or two, for this mode and size.
fn title_blurb(app: &App, row: TitleRow, mode: GameMode, size: MapSize) -> String {
    let mode = || match mode {
        GameMode::Charter => format!(
            "CHARTER: {:.0} minutes to grow the region, then your score. The same seed is the same map, so friends can compare.",
            B::CHARTER_MINUTES
        ),
        GameMode::Sandbox => {
            "SANDBOX: unlimited cash, no clock, no score. Build the network of your dreams."
                .to_string()
        }
        GameMode::Franchise => format!(
            "FRANCHISE: no clock, but the Railway Board reviews you every {:.0} minutes and wants more each time. Fail {} reviews and you're out.",
            B::REVIEW_EVERY_SECS as f64 / 60.0,
            B::REVIEW_STRIKES
        ),
    };
    match row {
        TitleRow::Continue => format!("Pick up where you left off: {}.", app.save_info.clone().unwrap_or_default()),
        TitleRow::Play | TitleRow::Mode => mode(),
        TitleRow::Tutorial => "New here? A guided first railway on its own map, with unlimited cash. Every step shows what to press and where.".into(),
        TitleRow::Size => {
            let (w, h) = size.dims();
            let (lo, hi) = size.towns();
            format!("{} map: {w}×{h} tiles, {lo}–{hi} towns, {} to start. Bigger maps take longer to fill.", size.name(), money(size.start_cash()))
        }
        TitleRow::Seed => app.keys.fill("The same seed always makes the same map: share it and compare. Type to change it, {title_daily} for today's daily map."),
        TitleRow::Handbook => "How everything works: money, towns, signals, junctions and deadlocks, with diagrams.".into(),
        TitleRow::Settings => "Music and effects volume, text size, map zoom, and display options.".into(),
        TitleRow::Controls => "Every key in the game. Pick one and press a new key to change it.".into(),
        TitleRow::Quit => "See you on the line.".into(),
    }
}

fn draw_title(f: &mut Frame, app: &mut App) {
    let area = f.area();
    f.render_widget(
        Block::default().style(Style::default().bg(Color::Rgb(14, 16, 22))),
        area,
    );
    let gold = Color::Rgb(255, 200, 80);
    let dim = Style::default().fg(widgets::FAINT);
    // the header: the big logo when there's room, else one line
    let mut head: Vec<Line> = vec![];
    if area.height >= 38 {
        for (i, l) in LOGO.iter().enumerate() {
            let c = lerp((255, 200, 80), (230, 90, 50), i as f64 / 4.0);
            head.push(
                Line::from(Span::styled(
                    *l,
                    Style::default().fg(rgb(c)).add_modifier(Modifier::BOLD),
                ))
                .centered(),
            );
        }
        head.push(Line::from(""));
    } else {
        head.push(
            Line::from(Span::styled(
                "R A I L H E A D",
                Style::default().fg(gold).add_modifier(Modifier::BOLD),
            ))
            .centered(),
        );
    }
    head.extend(title_train(app));
    head.push(
        Line::from(Span::styled(
            "Lay the track. Signal the junctions. Keep it moving. Watch the towns grow.",
            Style::default()
                .fg(Color::Rgb(170, 180, 200))
                .add_modifier(Modifier::ITALIC),
        ))
        .centered(),
    );

    // the menu: label on the left, details (and clickable choices) on the right
    let t = &app.title;
    let rows = title_rows(app);
    // rows are laid out from x = 0, then the block is centered on what it holds
    let max_w = TITLE_W.min(area.width);
    let left = 0u16;
    let mut menu: Vec<(Line, Option<TitleRow>)> = vec![];
    for row in rows {
        if matches!(row, TitleRow::Mode | TitleRow::Handbook) {
            menu.push((Line::from(""), None));
        }
        let focus = row == t.row;
        let label = match row {
            TitleRow::Continue => "Continue",
            TitleRow::Play => "New game",
            TitleRow::Tutorial => "Tutorial",
            TitleRow::Mode => "Mode",
            TitleRow::Size => "Map size",
            TitleRow::Seed => "Seed",
            TitleRow::Handbook => "Handbook",
            TitleRow::Settings => "Settings",
            TitleRow::Controls => "Controls",
            TitleRow::Quit => "Quit",
        };
        let label_style = if focus {
            Style::default()
                .fg(Color::Black)
                .bg(gold)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(widgets::TEXT)
                .add_modifier(Modifier::BOLD)
        };
        let mut spans = vec![
            Span::styled(
                if focus { " ▶ " } else { "   " },
                Style::default().fg(gold).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {label:<w$}", w = TITLE_LABEL - 1), label_style),
            // chips carry their own leading space: one less keeps their text in line
            Span::raw(
                if matches!(row, TitleRow::Mode | TitleRow::Size | TitleRow::Seed) {
                    " "
                } else {
                    "  "
                },
            ),
        ];
        let detail_x = left + widgets::width(&spans) as u16;
        // choices are drawn as chips; remember where each one is for the mouse
        let mut chips: Vec<(Span<'static>, Option<Hit>)> = vec![];
        let detail = |s: String, st: Style| (Span::styled(s, st), None);
        match row {
            TitleRow::Continue => chips.push(detail(
                app.save_info.clone().unwrap_or_default(),
                Style::default().fg(widgets::SOFT),
            )),
            TitleRow::Play => {
                chips.push(detail(
                    format!(
                        "{} · {} map · seed {}",
                        t.mode.name(),
                        t.size.name(),
                        t.seed
                    ),
                    Style::default().fg(widgets::SOFT),
                ));
                let seed = parse_seed(if t.seed.trim().is_empty() {
                    "1"
                } else {
                    &t.seed
                });
                match t.mode {
                    GameMode::Charter => {
                        if let Some(b) = app.records.get(seed, t.size) {
                            chips.push(detail(format!(" · best {:.0}", b.score), dim));
                            if b.medal > 0 {
                                chips.push(detail(
                                    format!(" ● {}", crate::world::MEDAL_NAMES[b.medal - 1]),
                                    Style::default().fg(rgb(MEDAL_COLORS[b.medal - 1])),
                                ));
                            }
                        }
                    }
                    GameMode::Franchise => {
                        if let Some(n) = app.records.franchise_best(seed, t.size) {
                            chips.push(detail(
                                format!(" · best ⚖ {n} review{}", if n == 1 { "" } else { "s" }),
                                dim,
                            ));
                        }
                    }
                    GameMode::Sandbox => {}
                }
            }
            TitleRow::Tutorial => chips.push(detail(
                "learn the basics, one step at a time".into(),
                if app.played {
                    Style::default().fg(widgets::SOFT)
                } else {
                    Style::default()
                        .fg(Color::Rgb(140, 230, 160))
                        .add_modifier(Modifier::BOLD)
                },
            )),
            TitleRow::Mode => {
                for m in GameMode::ALL {
                    chips.push((
                        widgets::chip(m.name(), t.mode == m, focus),
                        Some(Hit::TitleMode(m)),
                    ));
                    chips.push((Span::raw(" "), None));
                }
            }
            TitleRow::Size => {
                for s in MapSize::ALL {
                    chips.push((
                        widgets::chip(s.name(), t.size == s, focus),
                        Some(Hit::TitleSize(s)),
                    ));
                    chips.push((Span::raw(" "), None));
                }
            }
            TitleRow::Seed => {
                let caret = if focus && (app.frame / 15).is_multiple_of(2) {
                    "▏"
                } else {
                    " "
                };
                chips.push(detail(
                    format!(" {:<22}", format!("{}{caret}", t.seed)),
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::Rgb(40, 46, 64))
                        .add_modifier(Modifier::BOLD),
                ));
                chips.push(detail("  ".into(), dim));
                chips.push((
                    Span::styled(
                        " today's daily ",
                        Style::default().fg(widgets::SKY).bg(Color::Rgb(26, 34, 50)),
                    ),
                    Some(Hit::TitleDaily),
                ));
            }
            TitleRow::Handbook => {
                chips.push(detail("how everything works, with diagrams".into(), dim))
            }
            TitleRow::Settings => chips.push(detail(
                "volume · text size · map zoom · display".into(),
                dim,
            )),
            TitleRow::Controls => chips.push(detail("see and change every key".into(), dim)),
            TitleRow::Quit => {}
        }
        let y = menu.len() as u16; // relative; made absolute once the layout is known
        let mut x = detail_x;
        let room = (left + max_w).saturating_sub(detail_x) as usize;
        let mut used = 0;
        for (span, hit) in chips {
            let w = span.width();
            if used + w > room {
                break; // never wrap or spill off the box
            }
            if let Some(h) = hit {
                app.hits
                    .borrow_mut()
                    .push((Rect::new(x, y, w as u16, 1), h));
            }
            x += w as u16;
            used += w;
            spans.push(span);
        }
        let mut line = Line::from(spans);
        if focus {
            line = line.style(Style::default().bg(Color::Rgb(28, 32, 46)));
        }
        menu.push((line, Some(row)));
    }

    let menu_w = (menu
        .iter()
        .map(|(l, _)| l.width() as u16)
        .max()
        .unwrap_or(0)
        + 1)
    .min(max_w);
    let left = area.x + area.width.saturating_sub(menu_w) / 2;
    // under the menu: what the highlighted row means, records, keys, messages
    let mut foot: Vec<Line> = vec![Line::from("")];
    // the blurb always takes as many lines as the longest one can, so moving
    // between rows never shifts what's under it
    let blurb_w = (menu_w.max(72).min(area.width) as usize).saturating_sub(4);
    let tallest = title_rows(app)
        .into_iter()
        .flat_map(|row| {
            GameMode::ALL
                .into_iter()
                .flat_map(move |m| MapSize::ALL.into_iter().map(move |s| (row, m, s)))
        })
        .map(|(row, m, s)| wrap_words(&title_blurb(app, row, m, s), blurb_w).len())
        .max()
        .unwrap_or(1);
    let blurb = wrap_words(&title_blurb(app, t.row, t.mode, t.size), blurb_w);
    for i in 0..tallest {
        let l = blurb.get(i).cloned().unwrap_or_default();
        foot.push(
            Line::from(Span::styled(
                l,
                Style::default()
                    .fg(Color::Rgb(160, 170, 190))
                    .add_modifier(Modifier::ITALIC),
            ))
            .centered(),
        );
    }
    let r = &app.records;
    if r.charters > 0 || r.franchises > 0 {
        let mut spans = vec![Span::styled(
            format!(
                "{} Charter{} finished  ",
                r.charters,
                if r.charters == 1 { "" } else { "s" }
            ),
            dim,
        )];
        for i in (0..3).rev() {
            spans.push(Span::styled(
                format!(
                    "● {} {}  ",
                    r.medals[i],
                    crate::world::MEDAL_NAMES[i].to_lowercase()
                ),
                Style::default().fg(if r.medals[i] > 0 {
                    rgb(MEDAL_COLORS[i])
                } else {
                    Color::Rgb(70, 74, 90)
                }),
            ));
        }
        if r.franchises > 0 {
            let most = r.franchise.values().max().copied().unwrap_or(0);
            spans.push(Span::styled(
                format!(
                    "⚖ {} Franchise{} (most reviews {most})",
                    r.franchises,
                    if r.franchises == 1 { "" } else { "s" }
                ),
                dim,
            ));
        }
        foot.push(Line::from(""));
        foot.push(Line::from(spans).centered());
    }
    foot.push(Line::from(""));
    foot.push(
        Line::from(Span::styled(
            app.keys.fill("{title_up} {title_down} choose · {title_prev_mode} {title_next_mode} change · {title_start} go · or click · {title_quit} quit"),
            Style::default().fg(Color::Rgb(120, 130, 150)),
        ))
        .centered(),
    );
    if let Some((m, l)) = &app.msg {
        foot.push(Line::from(""));
        foot.push(
            Line::from(Span::styled(
                m.clone(),
                Style::default().fg(level_color(app, *l)),
            ))
            .centered(),
        );
    }

    // lay the three parts out, in the middle of the screen
    let (head_len, menu_len) = (head.len(), menu.len());
    let total = head_len + 1 + menu_len + foot.len();
    let top = area.y + area.height.saturating_sub(total as u16) / 2;
    let head_h = head_len as u16;
    f.render_widget(
        Paragraph::new(head),
        Rect::new(area.x, top, area.width, head_h.min(area.height)),
    );
    let menu_y = top + head_h + 1;
    // the hits recorded above have rows relative to the menu: move them into place
    for (r, h) in app.hits.borrow_mut().iter_mut() {
        if matches!(h, Hit::TitleMode(_) | Hit::TitleSize(_) | Hit::TitleDaily) {
            (r.x, r.y) = (r.x + left, r.y + menu_y);
        }
    }
    let bottom = area.y + area.height;
    for (i, (line, row)) in menu.into_iter().enumerate() {
        let y = menu_y + i as u16;
        if y >= bottom {
            break;
        }
        if let Some(row) = row {
            app.hits
                .borrow_mut()
                .insert(0, (Rect::new(left, y, menu_w, 1), Hit::Title(row)));
        }
        f.render_widget(Paragraph::new(line), Rect::new(left, y, menu_w, 1));
    }
    let foot_y = menu_y + menu_len as u16;
    if foot_y < bottom {
        f.render_widget(
            Paragraph::new(foot),
            Rect::new(area.x, foot_y, area.width, bottom - foot_y),
        );
    }
    let ver = format!("v{} ", env!("CARGO_PKG_VERSION"));
    let r = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(1),
        area.width,
        1,
    );
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(ver, dim)).right_aligned()),
        r,
    );
}

// ------------------------------------------------------------------ game

/// Smallest terminal the game screen can be laid out in.
pub const MIN_W: u16 = 80;
pub const MIN_H: u16 = 24;

/// Side panel and bottom panel sizes for a terminal this big: roomy on big
/// screens, tighter on small ones so the map keeps most of the window.
fn panel_sizes(area: Rect) -> (u16, u16) {
    let side = if area.width >= 130 {
        42
    } else if area.width >= 100 {
        36
    } else {
        30
    };
    let bottom = if area.height >= 40 {
        11
    } else if area.height >= 30 {
        8
    } else {
        6
    };
    (side, bottom)
}

fn draw_too_small(f: &mut Frame, app: &App, area: Rect) {
    let fix = if app.windowed {
        app.keys.fill(
            "Make the text smaller ({zoom_out}; {zoom_reset} for normal) or the window bigger.",
        )
    } else {
        "Maximize the window or make the font smaller.".to_string()
    };
    f.render_widget(
        Block::default().style(Style::default().bg(Color::Rgb(14, 16, 22))),
        area,
    );
    let lines = vec![
        Line::from(Span::styled(
            "RAILHEAD",
            Style::default()
                .fg(Color::Rgb(255, 200, 80))
                .add_modifier(Modifier::BOLD),
        ))
        .centered(),
        Line::from(""),
        Line::from("The window is too small for the railway.").centered(),
        Line::from(format!(
            "Make it at least {MIN_W}×{MIN_H} (it is {}×{}).",
            area.width, area.height
        ))
        .centered(),
        Line::from(Span::styled(fix, Style::default().fg(Color::Gray))).centered(),
    ];
    let top = area.height.saturating_sub(lines.len() as u16) / 2;
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }),
        Rect::new(area.x, area.y + top, area.width, area.height - top),
    );
}

fn draw_game(f: &mut Frame, app: &mut App) {
    let area = f.area();
    if area.width < MIN_W || area.height < MIN_H {
        draw_too_small(f, app, area);
        return;
    }
    let (side_w, bottom_h) = panel_sizes(area);
    let keys_w = hints::panel_width(app, area.width);
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(8),
        Constraint::Length(1),
        Constraint::Length(bottom_h),
    ])
    .split(area);
    let body = Layout::horizontal([
        Constraint::Length(keys_w),
        Constraint::Min(20),
        Constraint::Length(side_w),
    ])
    .split(rows[1]);
    let map = body[1];
    app.map_area = map;
    follow_cursor(app);
    draw_status(f, app, rows[0]);
    draw_map(f, app, map);
    fx::draw_map_fx(f.buffer_mut(), app);
    draw_pops(f, app, map);
    draw_tutorial_marks(f, app, map);
    let looking = (app.cursor, app.selected);
    if app.side_for != looking {
        app.side_for = looking;
        app.side_scroll = 0;
    }
    app.side_max = draw_side(f, app, body[2]);
    app.side_scroll = app.side_scroll.min(app.side_max);
    if keys_w > 0 {
        hints::draw(f, app, body[0]);
    }
    draw_msg(f, app, rows[2]);
    draw_bottom(f, app, rows[3]);
    draw_tutorial(f, app, map);
    fx::draw_headline(f.buffer_mut(), app, map);
    if app.show_overview {
        draw_overview(f, app, map);
    }
    if let Mode::Confirm(c) = &app.mode {
        draw_confirm(f, app, c.clone());
    }
    if app.world.as_ref().is_some_and(|w| w.finished)
        && !app.end_dismissed
        && !matches!(app.mode, Mode::Confirm(_))
    {
        draw_end(f, app);
    }
    if app.show_stats {
        draw_stats(f, app);
    }
}

fn follow_cursor(app: &mut App) {
    let Some(w) = app.world.as_ref() else { return };
    let (vw, vh) = app.view().tiles();
    if std::mem::take(&mut app.center_view) {
        app.cam = Pos::new(app.cursor.x - vw / 2, app.cursor.y - vh / 2);
    }
    // a cursor the mouse put there is already in view: don't scroll under the pointer
    let margin = if app.mouse_cursor {
        0
    } else {
        4.min(vw / 4).min(vh / 4)
    };
    if app.cursor.x < app.cam.x + margin {
        app.cam.x = app.cursor.x - margin;
    }
    if app.cursor.x >= app.cam.x + vw - margin {
        app.cam.x = app.cursor.x - vw + margin + 1;
    }
    if app.cursor.y < app.cam.y + margin {
        app.cam.y = app.cursor.y - margin;
    }
    if app.cursor.y >= app.cam.y + vh - margin {
        app.cam.y = app.cursor.y - vh + margin + 1;
    }
    // A map smaller than the window sits in the middle of it, with sea around.
    app.cam.x = if w.map.w <= vw {
        -(vw - w.map.w) / 2
    } else {
        app.cam.x.clamp(0, w.map.w - vw)
    };
    app.cam.y = if w.map.h <= vh {
        -(vh - w.map.h) / 2
    } else {
        app.cam.y.clamp(0, w.map.h - vh)
    };
}

/// "47:12".
fn fmt_clock(t: f64) -> String {
    let s = t.ceil() as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

/// A Unicode sparkline of `vals` squeezed into `width` characters.
pub fn sparkline(vals: &[f64], width: usize) -> String {
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if vals.is_empty() || width == 0 {
        return String::new();
    }
    let (lo, hi) = vals
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    let n = vals.len().min(width);
    (0..n)
        .map(|i| {
            // average the samples falling into this column
            let a = i * vals.len() / n;
            let b = ((i + 1) * vals.len() / n).max(a + 1);
            let v = vals[a..b].iter().sum::<f64>() / (b - a) as f64;
            let f = if hi > lo { (v - lo) / (hi - lo) } else { 0.5 };
            BARS[((f * 7.0).round() as usize).min(7)]
        })
        .collect()
}

fn fmt_time(t: f64) -> String {
    let s = t as u64;
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let w = app.world.as_ref().unwrap();
    let speed = match app.speed {
        0 => "⏸ PAUSED".to_string(),
        s => format!("{} {}x", "▶".repeat(s.min(4)), B::SPEEDS[s]),
    };
    let st = Style::default().fg(Color::White).bg(Color::Rgb(30, 34, 48));
    let income: f64 = w
        .trains
        .iter()
        .map(|t| t.carrier.income.per_min())
        .sum::<f64>()
        + w.trucks
            .iter()
            .map(|t| t.carrier.income.per_min())
            .sum::<f64>();
    let net = income - w.upkeep_per_min();
    let cash_style = if w.cash < 0.0 {
        st.fg(app.pal.bad).add_modifier(Modifier::BOLD)
    } else {
        st.fg(Color::Rgb(255, 225, 120))
            .add_modifier(Modifier::BOLD)
    };
    let net_style = st.fg(if net >= 0.0 {
        app.pal.good
    } else {
        app.pal.bad
    });
    let speed_style = if app.speed == 0 {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Rgb(240, 200, 60))
            .add_modifier(Modifier::BOLD)
    } else {
        st.add_modifier(Modifier::BOLD)
    };
    let line = Line::from(vec![
        Span::styled(
            " RAILHEAD ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(255, 190, 70))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if area.width >= 120 {
                format!(" seed {} ", w.map.seed)
            } else {
                " ".into()
            },
            st,
        ),
        match w.time_left() {
            _ if w.finished && w.franchise() => Span::styled(
                "│ ⚖ REVOKED ".to_string(),
                st.fg(app.pal.bad).add_modifier(Modifier::BOLD),
            ),
            _ if w.finished => Span::styled(
                "│ ⏱ FINISHED ".to_string(),
                st.fg(app.pal.good).add_modifier(Modifier::BOLD),
            ),
            None if w.franchise() => review_clock(app, w, st),
            Some(left) if left < 300.0 => Span::styled(
                format!("│ ⏱ {} LEFT ", fmt_clock(left)),
                st.fg(app.pal.bad).add_modifier(Modifier::BOLD),
            ),
            Some(left) => Span::styled(format!("│ ⏱ {} left ", fmt_clock(left)), st),
            None => Span::styled(format!("│ SANDBOX T+{} ", fmt_time(w.time)), st),
        },
        Span::styled(format!(" {speed} "), speed_style),
        Span::styled(" │ ", st),
        Span::styled(
            format!("Prosperity {:.0}", w.prosperity()),
            st.fg(Color::Rgb(140, 230, 160))
                .add_modifier(Modifier::BOLD),
        ),
        medal_span(w, st, area.width),
        Span::styled(" │ ", st),
        Span::styled(
            if w.sandbox() {
                "£∞".to_string()
            } else {
                money(w.cash)
            },
            cash_style,
        ),
        Span::styled(
            format!(
                " ({}{}/min) ",
                if net >= 0.0 { "+" } else { "" },
                money(net)
            ),
            net_style,
        ),
        Span::styled(
            if area.width >= 105 {
                format!("│ {} trains · {} trucks ", w.trains.len(), w.trucks.len())
            } else {
                String::new()
            },
            st,
        ),
    ]);
    let mut line = line;
    if app.overlay != Overlay::None {
        line.spans.push(Span::styled(
            format!("│ {} ", app.overlay.name()),
            st.fg(Color::Rgb(150, 200, 255))
                .add_modifier(Modifier::BOLD),
        ));
    }
    if app
        .autosaved_frame
        .is_some_and(|fr| app.frame.saturating_sub(fr) < 60)
    {
        line.spans.push(Span::styled(
            "│ ✔ autosaved ",
            st.fg(Color::Rgb(140, 160, 140)),
        ));
    }
    f.render_widget(Paragraph::new(line).style(st), area);
}

/// Colors of the bronze, silver and gold medals.
pub(super) const MEDAL_COLORS: [(u8, u8, u8); 3] =
    [(214, 140, 70), (205, 212, 225), (255, 210, 80)];

/// 12,340 (a number with thousands separators).
fn thousands(v: f64) -> String {
    money(v).replacen('£', "", 1)
}

/// Franchise: the countdown to the next Board review, red when it would fail.
fn review_clock(app: &App, w: &World, st: Style) -> Span<'static> {
    let Some(r) = w.next_review() else {
        return Span::raw("");
    };
    let failing = w.prosperity() < r.target || w.cash < 0.0;
    let text = format!(
        "│ ⚖ review {} in {} ",
        r.number,
        fmt_clock(r.secs_left as f64)
    );
    if failing && r.secs_left < 180 {
        Span::styled(
            text.to_uppercase(),
            st.fg(app.pal.bad).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(text, st)
    }
}

/// Franchise: what the Board wants next, and the strikes (✖ used, ○ left).
fn review_span(w: &World, st: Style, width: u16) -> Span<'static> {
    let strikes: String = (0..B::REVIEW_STRIKES)
        .map(|i| if i < w.stats.strikes { '✖' } else { '○' })
        .collect();
    let col = if w.stats.strikes > 0 {
        Color::Rgb(240, 120, 100)
    } else {
        Color::Rgb(150, 150, 165)
    };
    match w.next_review() {
        Some(r) if width >= 100 => Span::styled(
            format!(" → needs {} {strikes}", thousands(r.target)),
            st.fg(col),
        ),
        _ => Span::styled(format!(" {strikes}"), st.fg(col)),
    }
}

/// The status bar's medal readout: the next medal and the score it needs.
fn medal_span(w: &World, st: Style, width: u16) -> Span<'static> {
    if w.sandbox() {
        return Span::raw("");
    }
    if w.franchise() {
        return review_span(w, st, width);
    }
    let won = w.stats.medal;
    let targets = w.medal_targets();
    match targets.get(won) {
        _ if width < 100 => Span::styled(
            if won > 0 { " ●" } else { "" }.to_string(),
            st.fg(rgb(MEDAL_COLORS[won.max(1) - 1])),
        ),
        Some(t) => Span::styled(
            format!(
                " → ● {} at {}",
                crate::world::MEDAL_NAMES[won],
                thousands(*t)
            ),
            st.fg(rgb(lerp(MEDAL_COLORS[won], (120, 120, 130), 0.25))),
        ),
        None => Span::styled(
            " ● GOLD".to_string(),
            st.fg(rgb(MEDAL_COLORS[2])).add_modifier(Modifier::BOLD),
        ),
    }
}

fn draw_msg(f: &mut Frame, app: &App, area: Rect) {
    let w = app.world.as_ref().unwrap();
    let mode = match &app.mode {
        Mode::Normal => String::new(),
        Mode::Line { kind, start } => {
            let what = kind.name().to_uppercase();
            match start {
                None => app.keys.fill(&format!(
                    "[{what}] {{confirm}} start here · {{cancel}} leave "
                )),
                Some(s) => match build::plan_path(w, *s, app.cursor, *kind) {
                    Ok(p) => app.keys.fill(&format!(
                        "[{what}] {} tiles, {} · {{confirm}} build · {{cancel}} drop start ",
                        p.len(),
                        money(build::path_cost(w, &p, *kind))
                    )),
                    Err(_) => app
                        .keys
                        .fill(&format!("[{what}] no route · {{cancel}} drop start ")),
                },
            }
        }
        Mode::Double { start: None } => app
            .keys
            .fill("[DOUBLE TRACK] {confirm} start here · {cancel} leave "),
        Mode::Double { start: Some(s) } => match build::plan_double(w, *s, app.cursor) {
            Ok(l) => format!(
                "[DOUBLE TRACK] {} + {} tiles, {} ",
                l.0.len(),
                l.1.len(),
                money(build::double_cost(w, &l))
            ),
            Err(_) => "[DOUBLE TRACK] no route ".into(),
        },
        Mode::Place(Place::Demolish) => app.keys.fill(
            "[DEMOLISH] {confirm} here · hold {demolish} or drag to bulldoze · {cancel} stop ",
        ),
        Mode::Place(p) => app.keys.fill(&format!(
            "[{}] {{confirm}} to place, {{cancel}} to stop ",
            p.name().to_uppercase()
        )),
        Mode::Schedule { picking: true, .. } => app
            .keys
            .fill("[SCHEDULE] move to the stop, {confirm} · {cancel} back "),
        Mode::Schedule { .. } => "[SCHEDULE] ".into(),
        Mode::Confirm(_) => "[CONFIRM] ".into(),
    };
    let mut spans = vec![];
    if !mode.is_empty() {
        spans.push(Span::styled(
            mode,
            Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(120, 200, 255))
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));
    }
    let legend = match app.overlay {
        Overlay::None => None,
        Overlay::Congestion => Some(
            "CONGESTION: busy roads glow bad · red rail = occupied block · magenta train = waiting · digit = trains queued for station",
        ),
        Overlay::Flow => Some(
            "FLOW: brighter track and road = more cargo per minute passing (hover the side panel for the number)",
        ),
        Overlay::Towns => Some(
            "TOWNS: ground tinted by happiness (grey = not on your network) · red = industry wasting output · blue = industry starved of inputs",
        ),
    };
    let fresh = app.frame.saturating_sub(app.msg_frame) < 180 || !matches!(app.mode, Mode::Normal);
    if let (Some((m, l)), true) = (&app.msg, fresh) {
        spans.push(Span::styled(
            m.clone(),
            Style::default().fg(level_color(app, *l)),
        ));
    } else if let Some(lg) = legend {
        spans.push(Span::styled(
            lg,
            Style::default().fg(Color::Rgb(150, 200, 255)),
        ));
    } else if app.key_panel && hints::panel_width(app, f.area().width) == 0 {
        // no room for the keys panel: the keys that matter most, in a line
        if let Some(s) = hints::sections(app).into_iter().next() {
            spans.push(Span::styled(
                format!("{}: ", s.title),
                Style::default()
                    .fg(widgets::SKY)
                    .add_modifier(Modifier::BOLD),
            ));
            for h in s.hints.into_iter().take(5) {
                spans.push(widgets::keycap(
                    &h.keys.unwrap_or_else(|| app.keys.label(h.act)),
                ));
                spans.push(Span::styled(
                    format!(" {}  ", h.what),
                    Style::default().fg(widgets::SOFT),
                ));
            }
        }
    } else if let Some(h) = hint(w).map(|h| app.keys.fill(&h)) {
        spans.push(Span::styled(
            format!("Tip: {h}"),
            Style::default()
                .fg(Color::Rgb(150, 160, 180))
                .add_modifier(Modifier::ITALIC),
        ));
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().bg(Color::Rgb(20, 22, 30))),
        area,
    );
}

/// Context-sensitive tips for new players.
fn hint(w: &World) -> Option<String> {
    if w.time < 5.0 {
        return Some("press {pause} to unpause. Home is the ★ town ({home} jumps there). Towns grow when they get what they want (side panel).".into());
    }
    if w.trains.iter().any(|t| t.deadlocked) {
        return Some("DEADLOCK: select a red train ({next_vehicle}) and reverse it ({reverse}) or scrap it ({demolish}). One-way signals ({signal}) help.".into());
    }
    let hungry = w
        .towns
        .iter()
        .filter(|t| t.served && t.happiness() < B::GROWTH_NEUTRAL)
        .count();
    if hungry > 0 && w.time > 120.0 {
        let (n, verb) = if hungry == 1 {
            ("1 of your towns".to_string(), "isn't")
        } else {
            (format!("{hungry} of your towns"), "aren't")
        };
        return Some(format!(
            "{n} {verb} getting enough to grow. Put the cursor on one to see what it wants."
        ));
    }
    if w.trains.is_empty() && w.time > 240.0 {
        return Some("rail ({rail}) moves far more than trucks. A station ({station}) serves everything within 2 tiles.".into());
    }
    let full = |s: &&crate::world::Site| matches!(s.kind, SiteKind::Industry(k) if k.is_primary() && s.inv[k.output()] >= B::NODE_BUFFER - 0.5);
    let backed_up = w
        .sites
        .iter()
        .filter(full)
        .filter(|s| s.stats.per_min() > 0.0)
        .count();
    if backed_up > 0 {
        let n = if backed_up == 1 {
            "an industry you haul from is".to_string()
        } else {
            format!("{backed_up} industries you haul from are")
        };
        return Some(format!(
            "{n} full (flashing): add vehicles. Select one on the route and press {{duplicate}} for a copy."
        ));
    }
    let wasted = w.sites.iter().filter(full).count();
    if wasted > 0 && w.time > 180.0 {
        return Some(format!(
            "{wasted} industries are full and wasting output. Haul it to whoever turns it into something (see side panel)."
        ));
    }
    if w.towns.iter().filter(|t| !t.served).count() > 0 && w.time > 600.0 {
        return Some(
            "unserved towns (grey labels) don't count toward Prosperity. Connect them!".into(),
        );
    }
    if w.trains.iter().any(|t| t.deadlocked) {
        return Some("DEADLOCK: select a red train ({next_vehicle}) and reverse it ({reverse}) or scrap it ({demolish}). One-way signals ({signal}) help.".into());
    }
    None
}

fn line_glyph(mask: u8, double: bool) -> char {
    let n = mask & Dir::N.bit() != 0;
    let e = mask & Dir::E.bit() != 0;
    let s = mask & Dir::S.bit() != 0;
    let wv = mask & Dir::W.bit() != 0;
    let set: [char; 12] = if double {
        ['·', '║', '═', '╚', '╝', '╔', '╗', '╠', '╣', '╦', '╩', '╬']
    } else {
        ['·', '│', '─', '└', '┘', '┌', '┐', '├', '┤', '┬', '┴', '┼']
    };
    match (n, e, s, wv) {
        (false, false, false, false) => set[0],
        (_, false, _, false) => set[1],
        (false, _, false, _) => set[2],
        (true, true, false, false) => set[3],
        (true, false, false, true) => set[4],
        (false, true, true, false) => set[5],
        (false, false, true, true) => set[6],
        (true, true, true, false) => set[7],
        (true, false, true, true) => set[8],
        (false, true, true, true) => set[9],
        (true, true, false, true) => set[10],
        (true, true, true, true) => set[11],
    }
}

struct Cell {
    ch: char,
    fg: (u8, u8, u8),
    bg: (u8, u8, u8),
    bold: bool,
}

/// Beyond the edge of the map: open sea, darker the further out, with the
/// odd wave. Fills the window when the map is smaller than it.
fn sea_cell(w: &World, p: Pos) -> (char, (u8, u8, u8), (u8, u8, u8)) {
    let dx = (-p.x).max(p.x - (w.map.w - 1)).max(0);
    let dy = (-p.y).max(p.y - (w.map.h - 1)).max(0);
    let d = (dx.max(dy * 2)) as f64;
    let bg = lerp((26, 48, 86), (10, 18, 34), (d / 12.0).min(1.0));
    let h = hash2(w.map.seed ^ 0x5EA, p.x, p.y);
    let ch = if h.is_multiple_of(11) { '~' } else { ' ' };
    (ch, scale(bg, 1.6), bg)
}

/// The land. The background color says what it is; a few faint marks give it
/// texture without competing with the network (`T` turns the marks off).
fn terrain_cell(w: &World, p: Pos, marks: bool) -> Cell {
    let h = hash2(w.map.seed, p.x, p.y);
    let (every, mark, fg, bg) = match w.map.terrain(p) {
        Terrain::Plains => (0, ' ', (0, 0, 0), (50, 72, 40)),
        Terrain::Forest => (4, '♣', (56, 100, 56), (30, 58, 32)),
        Terrain::Hills => (8, '∩', (112, 102, 70), (80, 72, 48)),
        Terrain::Mountains => (3, '▲', (138, 136, 134), (92, 89, 87)),
        Terrain::Water => (7, '~', (72, 108, 165), (30, 56, 100)),
    };
    let ch = if marks && every > 0 && h.is_multiple_of(every) {
        mark
    } else {
        ' '
    };
    Cell {
        ch,
        fg,
        bg,
        bold: false,
    }
}

/// What the map draws, worked out once a frame (the tiles look it up).
struct MapCtx<'a> {
    w: &'a World,
    app: &'a App,
    blink: bool,
    /// Train tiles (train index, is it the head), truck tiles (vehicle id).
    trains: HashMap<Pos, (usize, bool)>,
    trucks: HashMap<Pos, u32>,
    /// The planned route of the track tool, and the selected train's tiles.
    preview: std::collections::HashSet<Pos>,
    sel_tiles: Vec<Pos>,
    occupied: Vec<i32>,
    houses: HashMap<Pos, &'a crate::world::Town>,
    dusk: f64,
    news_sites: Vec<u32>,
}

impl<'a> MapCtx<'a> {
    fn new(app: &'a App) -> MapCtx<'a> {
        let w = app.world.as_ref().unwrap();
        let mut trains = HashMap::new();
        for (i, t) in w.trains.iter().enumerate() {
            for (j, p) in t.tiles.iter().enumerate().rev() {
                trains.insert(*p, (i, j == 0));
            }
        }
        let preview: Vec<Pos> = match &app.mode {
            Mode::Line {
                kind,
                start: Some(s),
            } => build::plan_path(w, *s, app.cursor, *kind).unwrap_or_default(),
            Mode::Double { start: Some(s) } => build::plan_double(w, *s, app.cursor)
                .map(|(a, b)| [a, b].concat())
                .unwrap_or_default(),
            _ => vec![],
        };
        MapCtx {
            w,
            app,
            blink: (app.frame / 8).is_multiple_of(2),
            trains,
            trucks: w.trucks.iter().map(|t| (t.pos, t.carrier.id)).collect(),
            preview: preview.into_iter().collect(),
            sel_tiles: app
                .selected
                .and_then(|id| {
                    w.trains
                        .iter()
                        .find(|t| t.carrier.id == id)
                        .map(|t| t.tiles.clone())
                })
                .unwrap_or_default(),
            occupied: rail::occupied_blocks(w),
            houses: house_tiles(w),
            dusk: fx::dusk(w),
            news_sites: crate::world::events::current(w).map(|n| n.site).collect(),
        }
    }
}

/// One tile as drawn. `cell` is its glyph; when zoomed in, a tile covers
/// several cells and the others continue its track (`conn`, `line`) or a
/// vehicle on it (`body`), or just show the ground.
struct Look {
    cell: Cell,
    /// Directions the track (or road) here runs, by `Dir::bit`.
    conn: u8,
    /// The pieces that continue it sideways and up/down, and their color.
    line: Option<(char, char, (u8, u8, u8))>,
    /// A train or truck here: what fills the rest of its track.
    body: Option<(char, (u8, u8, u8))>,
}

fn tile_look(cx: &MapCtx, p: Pos) -> Look {
    let (w, app) = (cx.w, cx.app);
    let pal = app.pal;
    let mut c = terrain_cell(w, p, app.terrain_marks);
    c.bg = fx::dusk_bg(c.bg, cx.dusk);
    let mut look_line = None;
    let mut conn = 0;
    let mut body = None;
    let pi = w.map.province_at(p);
    if w.map.is_border(p) || {
        let q = Pos::new(p.x - 1, p.y);
        let r = Pos::new(p.x, p.y - 1);
        (w.map.in_bounds(q) && w.map.province_at(q) != pi)
            || (w.map.in_bounds(r) && w.map.province_at(r) != pi)
    } {
        // region border: only just darker, so it reads as a hint, not a line
        c.bg = scale(c.bg, 0.9);
    }
    if matches!(app.overlay, Overlay::Congestion | Overlay::Flow) {
        c.bg = scale(c.bg, 0.6);
    }
    if app.overlay == Overlay::Towns {
        c.bg = scale(c.bg, 0.7);
        if let Some(town) = cx.houses.get(&p) {
            c.bg = if town.served {
                lerp(c.bg, pal.ramp(town.happiness()), 0.6)
            } else {
                lerp(c.bg, (120, 120, 120), 0.5)
            };
        }
    }
    let flow = w.flow.get(w.map.idx(p)).copied().unwrap_or(0.0) as f64;
    let flow_col = |c: &mut Cell| {
        let f = (flow / B::FLOW_OVERLAY_FULL).min(1.0);
        c.fg = lerp((70, 80, 110), (255, 230, 80), f.sqrt());
        c.bold = f > 0.3;
    };

    let t = w.tile(p);
    // Bridges & tunnels.
    if (t.road.present() || t.rail.present()) && t.site.is_none() {
        match w.map.terrain(p) {
            Terrain::Water => c.bg = (92, 70, 44),
            Terrain::Mountains => c.bg = (40, 38, 36),
            _ => {}
        }
    }
    if t.road.present() {
        c.ch = line_glyph(t.road_conn, false);
        c.fg = (185, 160, 115);
        if app.overlay == Overlay::Congestion && t.road == Layer::Built {
            let load = w.road_load[w.map.idx(p)] as f64;
            c.fg = pal.ramp(1.0 - (load / (B::ROAD_TILE_CAPACITY + 2.0)));
            c.bold = load > 0.0;
        }
        if app.overlay == Overlay::Flow {
            flow_col(&mut c);
        }
        conn = t.road_conn;
        look_line = Some(('─', '│', c.fg));
    }
    if t.rail.present() {
        c.ch = line_glyph(t.rail_conn, true);
        c.fg = (225, 225, 235);
        c.bold = true;
        if app.overlay == Overlay::Congestion && t.rail == Layer::Built {
            let b = w.blocks.id.get(w.map.idx(p)).copied().unwrap_or(-1);
            c.fg = if b >= 0 && cx.occupied.contains(&b) {
                pal.ramp(0.0)
            } else {
                pal.ramp(1.0)
            };
        }
        if app.overlay == Overlay::Flow {
            flow_col(&mut c);
        }
        conn = t.rail_conn;
        look_line = Some(('═', '║', c.fg));
    }
    if let Some(sig) = t.signal {
        let b = rail::signal_block_free(w, p);
        c.ch = match sig {
            SignalKind::Both => '◆',
            SignalKind::OneWay(Dir::N) => '↑',
            SignalKind::OneWay(Dir::E) => '→',
            SignalKind::OneWay(Dir::S) => '↓',
            SignalKind::OneWay(Dir::W) => '←',
            SignalKind::Chain => '◈',
            SignalKind::ChainOneWay(Dir::N) => '⇑',
            SignalKind::ChainOneWay(Dir::E) => '⇒',
            SignalKind::ChainOneWay(Dir::S) => '⇓',
            SignalKind::ChainOneWay(Dir::W) => '⇐',
        };
        c.fg = if b { (90, 240, 110) } else { (255, 70, 60) };
        if pal.kind == PaletteKind::ColorBlind {
            c.fg = if b { (90, 170, 255) } else { (255, 150, 0) };
        }
        c.bold = true;
    }
    if let Some(sid) = t.station {
        c.ch = 'S';
        c.fg = (255, 255, 255);
        c.bg = (110, 60, 150);
        c.bold = true;
        if app.overlay == Overlay::Congestion {
            let q = rail::station_queue(w, sid);
            if q > 0 {
                c.ch = char::from_digit(q.min(9) as u32, 10).unwrap();
                c.bg = pal.ramp(1.0 - q as f64 / 4.0);
            }
        }
    }
    if let Some(id) = t.site {
        let s = &w.sites[id as usize];
        c.ch = s.kind.glyph();
        c.bold = true;
        c.fg = match s.kind {
            SiteKind::Industry(k) => industry_color(k),
            SiteKind::Town(ti) => {
                if ti == w.map.home {
                    c.ch = '★';
                }
                (255, 255, 255)
            }
            SiteKind::Hub => (255, 220, 120),
        };
        c.bg = scale(c.bg, 0.55);
        match s.kind {
            SiteKind::Town(ti) => c.bg = town_bg(&w.towns[ti]),
            // primaries you haul from pulse when full: your vehicles can't keep up
            // (ones nobody serves yet just wait; every one flashing was too much)
            SiteKind::Industry(k) => {
                if s.inv[k.output()] >= B::NODE_BUFFER - 0.5
                    && k.is_primary()
                    && s.stats.per_min() > 0.0
                    && cx.blink
                {
                    c.bg = (120, 90, 20);
                }
                if app.overlay == Overlay::Towns {
                    if s.inv[k.output()] >= B::NODE_BUFFER - 0.5 {
                        c.bg = (170, 40, 40); // wasting
                    } else if !k.is_primary() && k.inputs().iter().any(|i| s.inv[*i] < 0.5) {
                        c.bg = (40, 70, 170); // starved of an input
                    }
                }
            }
            SiteKind::Hub => {}
        }
        // news in effect here: the site glows gold
        if cx.news_sites.contains(&id) && app.frame / 10 % 2 == 0 {
            c.bg = (150, 115, 20);
        }
    } else if !t.road.present() && !t.rail.present() {
        // Houses: towns spread out as they grow.
        if let Some(town) = cx.houses.get(&p) {
            c.ch = '▪';
            c.fg = (215, 185, 155);
            // lamps come on as the sun sets
            c.fg = lerp(c.fg, (255, 215, 110), cx.dusk);
            c.bg = town_bg(town);
        }
    }
    if let Some(&id) = cx.trucks.get(&p) {
        c.ch = '▪';
        c.fg = if Some(id) == app.selected {
            (255, 255, 255)
        } else {
            (255, 225, 90)
        };
        c.bold = true;
        body = Some(('▪', c.fg));
    }
    if let Some(&(ti, head)) = cx.trains.get(&p) {
        let tr = &w.trains[ti];
        c.ch = match (tr.kind, head) {
            (TrainKind::Express, true) => '▣',
            (TrainKind::Express, false) => '▒',
            (_, true) => '█',
            _ => '▓',
        };
        c.fg = TRAIN_COLORS[tr.carrier.id as usize % TRAIN_COLORS.len()];
        if tr.deadlocked && cx.blink {
            c.fg = (255, 40, 40);
        }
        if app.overlay == Overlay::Congestion && tr.carrier.state == VState::Blocked {
            c.fg = (255, 80, 255);
        }
        body = Some((
            if tr.kind == TrainKind::Express {
                '▒'
            } else {
                '▓'
            },
            c.fg,
        ));
    }
    if cx.preview.contains(&p) {
        c.bg = (40, 110, 150);
    }
    if cx.sel_tiles.contains(&p) && cx.blink {
        c.bg = (200, 200, 200);
    }
    if t.site.is_some() {
        look_line = None; // sites fill their whole block (track runs on through stations)
    }
    Look {
        cell: c,
        conn,
        line: look_line,
        body,
    }
}

/// The color a tile shows from far out: what's on it, else its ground.
fn far_color(l: &Look) -> (u8, u8, u8) {
    if l.cell.ch != ' ' && (l.cell.bold || l.conn != 0) {
        l.cell.fg
    } else {
        l.cell.bg
    }
}

fn cursor_style() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(Color::Rgb(255, 230, 80))
        .add_modifier(Modifier::BOLD)
}

fn draw_map(f: &mut Frame, app: &App, area: Rect) {
    let w = app.world.as_ref().unwrap();
    let cx = MapCtx::new(app);
    let view = app.view();
    let z = view.zoom;
    let (gx, gy) = z.glyph();
    let buf = f.buffer_mut();
    let sea = |p: Pos| -> ((u8, u8, u8), (u8, u8, u8), char) {
        let (ch, fg, bg) = sea_cell(w, p);
        (fx::dusk_bg(fg, cx.dusk), fx::dusk_bg(bg, cx.dusk), ch)
    };
    // the tiles under one row of cells only need working out once per row
    let mut looks: HashMap<Pos, Look> = HashMap::new();
    for sy in 0..area.height {
        for sx in 0..area.width {
            let Some(cell) = buf.cell_mut((area.x + sx, area.y + sy)) else {
                continue;
            };
            let p = view.tile_at_offset(sx as i32, sy as i32);
            if z == MapZoom::Far {
                // two tiles per cell: the upper one is the top half block
                let q = Pos::new(p.x, p.y + 1);
                let mut col = |p: Pos| {
                    if p == app.cursor {
                        return (255, 230, 80);
                    }
                    if !w.map.in_bounds(p) {
                        return sea(p).1;
                    }
                    far_color(looks.entry(p).or_insert_with(|| tile_look(&cx, p)))
                };
                let (top, bot) = (col(p), col(q));
                cell.set_char('▀')
                    .set_style(Style::default().fg(rgb(top)).bg(rgb(bot)));
                continue;
            }
            // where in the tile's block this cell is
            let (dx, dy) = (
                (sx as i32).rem_euclid(z.cols()),
                (sy as i32).rem_euclid(z.rows()),
            );
            if !w.map.in_bounds(p) {
                let (fg, bg, ch) = sea(p);
                let ch = if (dx, dy) == (gx, gy) { ch } else { ' ' };
                cell.set_char(ch)
                    .set_style(Style::default().fg(rgb(fg)).bg(rgb(bg)));
                continue;
            }
            let look = looks.entry(p).or_insert_with(|| tile_look(&cx, p));
            let mut c = Cell {
                ch: ' ',
                fg: look.cell.fg,
                bg: look.cell.bg,
                bold: look.cell.bold,
            };
            if (dx, dy) == (gx, gy) {
                c.ch = look.cell.ch;
            } else if let Some((across, down, line_fg)) = look.line {
                let has = |d: Dir| look.conn & d.bit() != 0;
                let run = if dy == gy && ((dx > gx && has(Dir::E)) || (dx < gx && has(Dir::W))) {
                    Some(across)
                } else if dx == gx && ((dy > gy && has(Dir::S)) || (dy < gy && has(Dir::N))) {
                    Some(down)
                } else {
                    None
                };
                match (run, look.body) {
                    (Some(_), Some((ch, fg))) => (c.ch, c.fg) = (ch, fg),
                    (Some(ch), None) => (c.ch, c.fg) = (ch, line_fg),
                    _ => {}
                }
            } else if look.body.is_none()
                && look.conn == 0
                && look.cell.ch != ' '
                && !look.cell.bold
            {
                // a marked bit of ground (♣, houses): scatter a few more, so it doesn't go bare
                c.ch = if hash2(w.map.seed ^ 0xD07, p.x * 8 + dx, p.y * 8 + dy).is_multiple_of(3) {
                    look.cell.ch
                } else {
                    ' '
                };
            }
            let mut style = Style::default().fg(rgb(c.fg)).bg(rgb(c.bg));
            if c.bold {
                style = style.add_modifier(Modifier::BOLD);
            }
            if p == app.cursor {
                style = cursor_style();
                if c.ch == ' ' && (dx, dy) == (gx, gy) {
                    c.ch = '┼';
                }
            }
            cell.set_char(c.ch).set_style(style);
        }
        if z != MapZoom::Far && (sy as i32 + 1).rem_euclid(z.rows()) == 0 {
            looks.clear();
        }
    }
    // zoomed in, note each whole tile block so the window can stretch its glyph over it
    if z.cols() > 1 || z.rows() > 1 {
        let (bw, bh) = (z.cols() as u16, z.rows() as u16);
        let mut blocks = app.map_blocks.borrow_mut();
        for by in (0..area.height / bh * bh).step_by(bh as usize) {
            for bx in (0..area.width / bw * bw).step_by(bw as usize) {
                let rect = Rect::new(area.x + bx, area.y + by, bw, bh);
                let cells = (0..bh)
                    .flat_map(|dy| (0..bw).map(move |dx| (dx, dy)))
                    .filter_map(|(dx, dy)| buf.cell((rect.x + dx, rect.y + dy)).cloned())
                    .collect();
                blocks.push(MapBlock {
                    rect,
                    cells,
                    glyph: (gx as u16, gy as u16),
                });
            }
        }
    }

    // Town labels: name and population, colored by how happy the town is.
    if app.labels {
        let tiles_under = |sx: i32, sy: i32| -> Vec<Pos> {
            let p = view.tile_at_offset(sx, sy);
            if z == MapZoom::Far {
                vec![p, Pos::new(p.x, p.y + 1)]
            } else {
                vec![p]
            }
        };
        let hides_network = |p: Pos| {
            w.map.in_bounds(p) && {
                let t = w.tile(p);
                t.site.is_some() || t.rail.present() || t.road.present()
            }
        };
        for (ti, town) in w.towns.iter().enumerate() {
            let star = if ti == w.map.home { "★ " } else { "" };
            let label = format!("{star}{} {}", town.name, short_pop(town.pop));
            let len = label.chars().count() as i32;
            let (tx, ty) = view.offset(town.pos);
            let lx = tx - len / 2;
            // a row just above or below the town (beyond its houses) where the label hides no network
            let r = town_radius(town.pop);
            let reach = |d: i32| view.offset(Pos::new(town.pos.x, town.pos.y + d)).1;
            let rows = [
                reach(-(r + 1)),
                reach(r + 1),
                reach(-(r + 2)),
                reach(r + 2),
                reach(-(r + 3)),
            ];
            let clear = |row: i32| {
                (0..len).all(|i| {
                    tiles_under(lx + i, row)
                        .into_iter()
                        .all(|p| !hides_network(p))
                })
            };
            let Some(ly) = rows.into_iter().find(|row| *row != ty && clear(*row)) else {
                continue;
            };
            if ly < 0 || ly >= area.height as i32 {
                continue;
            }
            let col = if !town.served {
                (190, 190, 190)
            } else {
                app.pal.ramp(town.happiness())
            };
            let mut st = Style::default().fg(rgb(col)).add_modifier(Modifier::BOLD);
            if !town.served {
                st = st
                    .add_modifier(Modifier::ITALIC)
                    .remove_modifier(Modifier::BOLD);
            }
            if z == MapZoom::Far {
                st = st.bg(Color::Rgb(16, 20, 30));
            }
            for (i, ch) in label.chars().enumerate() {
                let x = lx + i as i32;
                if x < 0 || x >= area.width as i32 {
                    continue;
                }
                let under = tiles_under(x, ly);
                if under.contains(&app.cursor) || w.trucks.iter().any(|t| under.contains(&t.pos)) {
                    continue;
                }
                if let Some(cell) = buf.cell_mut((area.x + x as u16, area.y + ly as u16)) {
                    let bg = cell.bg;
                    let st = if z == MapZoom::Far { st } else { st.bg(bg) };
                    cell.set_char(ch).set_style(st);
                }
            }
        }
    }
}

pub(super) fn industry_color(k: IndustryKind) -> (u8, u8, u8) {
    match k {
        IndustryKind::Farm => (200, 240, 90),
        IndustryKind::Forest => (120, 220, 120),
        IndustryKind::CoalMine => (170, 170, 180),
        IndustryKind::IronMine => (230, 140, 110),
        IndustryKind::OilWell => (190, 140, 255),
        IndustryKind::Mill => (250, 230, 150),
        IndustryKind::Sawmill => (220, 170, 110),
        IndustryKind::Steelworks => (150, 200, 255),
        IndustryKind::Factory => (255, 150, 200),
        IndustryKind::Refinery => (255, 190, 90),
    }
}

/// How far a town's houses spread (tiles) for a population.
pub fn town_radius(pop: f64) -> i32 {
    ((pop / 120.0).sqrt() as i32).clamp(1, 7)
}

fn town_bg(t: &crate::world::Town) -> (u8, u8, u8) {
    [(70, 58, 50), (86, 66, 52), (100, 72, 56), (120, 80, 60)][t.tier]
}

/// "460", "2.3k", "12k".
fn short_pop(p: f64) -> String {
    if p < 1000.0 {
        format!("{p:.0}")
    } else if p < 10_000.0 {
        format!("{:.1}k", p / 1000.0)
    } else {
        format!("{:.0}k", p / 1000.0)
    }
}

/// Tiles covered by town houses (drawn on empty land), keyed to their town.
fn house_tiles(w: &World) -> HashMap<Pos, &crate::world::Town> {
    let mut m = HashMap::new();
    for t in &w.towns {
        let r = town_radius(t.pop);
        // fill the disc to about the fraction a town of this size would
        let density = (t.pop / (std::f64::consts::PI * (r * r) as f64 * 120.0)).clamp(0.25, 0.85);
        for dy in -r..=r {
            for dx in -r..=r {
                let p = Pos::new(t.pos.x + dx, t.pos.y + dy);
                if dx * dx + dy * dy > r * r + 1 || !w.map.in_bounds(p) {
                    continue;
                }
                if matches!(w.map.terrain(p), Terrain::Water | Terrain::Mountains) {
                    continue;
                }
                let h = hash2(w.map.seed ^ 0x77, p.x, p.y) % 1000;
                if (h as f64) < density * 1000.0 {
                    m.insert(p, t);
                }
            }
        }
    }
    m
}

// ------------------------------------------------------------------ side panel

fn bar(v: f64, max: f64, width: usize) -> String {
    let f = if max > 0.0 {
        (v / max).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let full = (f * width as f64).round() as usize;
    format!("{}{}", "█".repeat(full), "░".repeat(width - full))
}

fn kv(k: &str, v: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{k:<11}"), Style::default().fg(Color::Gray)),
        Span::raw(v),
    ])
}

fn header(s: impl Into<String>, color: Color) -> Line<'static> {
    Line::from(Span::styled(
        s.into(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    ))
}

/// The side panel. Returns how far it can scroll (0 when it all fits).
fn draw_side(f: &mut Frame, app: &App, area: Rect) -> u16 {
    let w = app.world.as_ref().unwrap();
    let mut lines: Vec<Line> = vec![];
    let accent = Color::Rgb(255, 200, 90);

    if let Some(id) = app.selected
        && let Some((c, kind)) = w.carrier(id)
    {
        vehicle_lines(app, w, id, c, kind, &mut lines);
        lines.push(Line::from(""));
    }

    let p = app.cursor;
    let pi = w.map.province_at(p);
    let t = w.tile(p);

    // What's on this tile.
    let mut what = w.map.terrain(p).name().to_string();
    if let Some(id) = t.site {
        what = w.site_name(id);
    } else if t.rail.present() {
        what = format!("Rail on {what}");
    } else if t.road.present() && t.site.is_none() {
        what = format!("Road on {what}");
    }
    let region = &w.map.provinces[pi].name;
    let place = if what.contains(region.as_str()) {
        format!("  {},{}", p.x, p.y)
    } else {
        format!("  {} · {},{}", region, p.x, p.y)
    };
    lines.push(Line::from(vec![
        Span::styled(
            format!("▌{what}"),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(place, Style::default().fg(Color::DarkGray)),
    ]));
    if let Some(sid) = t.station {
        let s = &w.stations[sid as usize];
        lines.push(header(
            format!("Station \"{}\"", s.name),
            Color::Rgb(200, 150, 255),
        ));
        lines.push(kv(
            "Limit",
            app.keys.fill(&format!(
                "{} train(s)   ({{less}} {{more}} to change)",
                s.limit
            )),
        ));
        lines.push(kv(
            "Queue",
            format!("{} train(s) bound here", rail::station_queue(w, sid)),
        ));
        lines.push(kv(
            "Throughput",
            format!("{:.0} units/min", s.stats.per_min().max(0.0)),
        ));
        lines.push(kv("Revenue", format!("{}/min", money(s.income.per_min()))));
        let served = rail::station_sites(w, s.pos);
        if served.is_empty() {
            lines.push(Line::from(Span::styled(
                format!(
                    "Serves nothing! Put it within {} tiles of a town or industry.",
                    B::STATION_RADIUS
                ),
                Style::default().fg(app.pal.warn),
            )));
        }
        for site in served {
            lines.push(kv("Serves", w.site_name(site)));
        }
    }
    if let Some(sig) = t.signal {
        lines.push(kv("Signal", format!("{sig:?}")));
    }
    if t.rail.present() || t.road.present() {
        let flow = w.flow.get(w.map.idx(p)).copied().unwrap_or(0.0);
        if flow >= 0.5 {
            lines.push(kv("Flow", format!("~{flow:.0} units/min pass here")));
        }
    }
    // The site here, or the town whose houses these are.
    let site = t.site.or_else(|| {
        w.towns
            .iter()
            .filter(|tn| tn.pos.dist2(p) <= town_radius(tn.pop).pow(2) + 1)
            .min_by_key(|tn| tn.pos.dist2(p))
            .map(|tn| tn.site)
    });
    if let Some(id) = site {
        lines.push(Line::from(""));
        site_lines(app, w, id, &mut lines);
    }

    app.hits.borrow_mut().push((area, Hit::Side));
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(Color::Rgb(60, 66, 90)));
    let para = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: true })
        .style(Style::default().bg(Color::Rgb(18, 20, 28)));
    // Big text or a small window: more than fits. Scroll with the wheel, and say so.
    let total = para.line_count(area.width.saturating_sub(1)) as u16;
    let max = total.saturating_sub(area.height);
    let scroll = app.side_scroll.min(max);
    f.render_widget(para.scroll((scroll, 0)), area);
    let marker = Style::default()
        .fg(Color::Black)
        .bg(Color::Rgb(255, 200, 90));
    let tag = |s: &str| Line::from(Span::styled(s.to_string(), marker)).right_aligned();
    if scroll < max {
        let below = max - scroll;
        f.render_widget(
            Paragraph::new(tag(&app
                .keys
                .fill(&format!(" ▼ {below} more · {{side_more}} or scroll ")))),
            Rect::new(area.x + 1, area.bottom() - 1, area.width - 1, 1),
        );
    }
    if scroll > 0 {
        f.render_widget(
            Paragraph::new(tag(" ▲ more ")),
            Rect::new(area.x + 1, area.y, area.width - 1, 1),
        );
    }
    max
}

/// A labelled bar: "FOOD      ████░░░░  12/60".
fn bar_line(label: &str, v: f64, max: f64, color: Color, tail: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {label:<9}"), Style::default().fg(Color::Gray)),
        Span::styled(bar(v, max, 14), Style::default().fg(color)),
        Span::raw(format!(" {tail}")),
    ])
}

fn site_lines(app: &App, w: &World, id: u32, lines: &mut Vec<Line<'static>>) {
    let s = &w.sites[id as usize];
    let stock = Color::Rgb(200, 190, 140);
    match s.kind {
        SiteKind::Town(ti) => {
            let town = &w.towns[ti];
            let home = if ti == w.map.home { " ★ home" } else { "" };
            lines.push(header(
                format!("▌{} — {}{home}", town.name, town.tier_name()),
                Color::Rgb(255, 220, 140),
            ));
            let next = B::TIER_POP
                .get(town.tier + 1)
                .map(|n| format!("  (next tier at {n:.0})"))
                .unwrap_or_default();
            lines.push(kv("Population", format!("{:.0}{next}", town.pop)));
            if !town.served {
                lines.push(Line::from(Span::styled(
                    "Not on your network: doesn't count toward Prosperity.",
                    Style::default().fg(app.pal.warn),
                )));
            }
            let happy = town.happiness();
            let mood = if happy >= B::GROWTH_NEUTRAL {
                "growing"
            } else if town.pop > town.start_pop + 0.5 {
                "shrinking"
            } else {
                "stagnant"
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{:<11}", "Happiness"),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    bar(happy, 1.0, 14),
                    Style::default().fg(app.pal.ramp_color(happy)),
                ),
                Span::raw(format!(" {:.0}% {mood}", happy * 100.0)),
            ]));
            lines.push(Line::from(Span::styled(
                "Wants (per minute):",
                Style::default().fg(Color::Gray),
            )));
            for &c in town.demands() {
                let sat = town.sat[c.index()];
                lines.push(bar_line(
                    c.short(),
                    sat,
                    1.0,
                    app.pal.ramp_color(sat),
                    format!("{:.0}% of {:.0}/min", sat * 100.0, town.demand_per_min(c)),
                ));
            }
            room_lines(w, s, town.demands(), stock, lines);
            if let Some(nt) = crate::world::TIER_DEMANDS.get(town.tier + 1)
                && let Some(c) = nt.iter().find(|c| !town.wants(**c))
            {
                lines.push(kv("Next wants", c.name().to_string()));
            }
            lines.push(kv(
                "Passengers",
                format!(
                    "{:.0} waiting · {:.0} arrived",
                    s.inv[Cargo::Passengers],
                    town.arrived
                ),
            ));
            for c in w
                .contracts
                .iter()
                .filter(|c| c.state == ContractState::Open && c.town == ti)
            {
                lines.push(Line::from(Span::styled(
                    format!(
                        "§ Contract: {:.0}/{:.0} {} · {} left",
                        c.delivered.min(c.amount),
                        c.amount,
                        c.cargo.name().to_lowercase(),
                        fmt_clock((c.deadline - w.time).max(0.0))
                    ),
                    Style::default().fg(Color::Rgb(255, 225, 120)),
                )));
            }
        }
        SiteKind::Industry(k) => {
            lines.push(header(
                format!("▌{}", w.site_name(id)),
                Color::Rgb(140, 220, 255),
            ));
            room_lines(w, s, k.inputs(), stock, lines);
            let out = k.output();
            let full = s.inv[out] >= B::NODE_BUFFER - 0.5;
            lines.push(bar_line(
                out.short(),
                s.inv[out],
                B::NODE_BUFFER,
                if full { app.pal.bad } else { stock },
                format!("{:.0}/{:.0} out", s.inv[out], B::NODE_BUFFER),
            ));
            let rate = B::INDUSTRY_RATE[k.index()] * s.level * 60.0;
            if k.is_primary() {
                lines.push(kv(
                    "Makes",
                    format!("{rate:.0} {}/min", out.name().to_lowercase()),
                ));
                lines.push(kv(
                    "Production",
                    format!(
                        "×{:.2} of ×{:.0} (grows while you keep it hauled out)",
                        s.level,
                        B::LEVEL_MAX
                    ),
                ));
            } else {
                let ins: Vec<&str> = k.inputs().iter().map(|c| c.name()).collect();
                lines.push(kv(
                    "Recipe",
                    format!("{} → {}", ins.join(" + "), out.name()),
                ));
                lines.push(kv("Max", format!("{rate:.0}/min if fed")));
            }
            if full && k.is_primary() {
                lines.push(Line::from(Span::styled(
                    "FULL: output is being wasted!",
                    Style::default().fg(app.pal.bad),
                )));
            }
            let users: Vec<&str> = IndustryKind::ALL
                .iter()
                .filter(|u| u.inputs().contains(&out))
                .map(|u| u.name())
                .collect();
            let towns = if crate::world::TIER_DEMANDS[3].contains(&out) {
                "towns"
            } else {
                ""
            };
            let to: Vec<&str> = users
                .into_iter()
                .chain((!towns.is_empty()).then_some(towns))
                .collect();
            lines.push(kv("Sells to", to.join(", ")));
        }
        SiteKind::Hub => {
            lines.push(header(
                "▌Transfer hub".to_string(),
                Color::Rgb(140, 220, 255),
            ));
            for c in Cargo::ALL {
                if s.inv[c] >= 0.5 {
                    lines.push(bar_line(
                        c.short(),
                        s.inv[c],
                        B::HUB_CAP,
                        stock,
                        format!("{:.0}", s.inv[c]),
                    ));
                }
            }
        }
    }
    lines.push(kv(
        "Throughput",
        format!("{:.0} units/min", s.stats.per_min().max(0.0)),
    ));
}

/// Exactly how much a drop-off would hand over now, per cargo: what "only if
/// it wants ≥N" counts. Bars fill with what's in stock.
fn room_lines(
    w: &World,
    s: &crate::world::Site,
    cargos: &[Cargo],
    color: Color,
    lines: &mut Vec<Line<'static>>,
) {
    if cargos.is_empty() {
        return;
    }
    lines.push(Line::from(Span::styled(
        "Room to unload now:",
        Style::default().fg(Color::Gray),
    )));
    for &c in cargos {
        let (room, most) = (w.accept_space(s, c), w.accept_cap(s, c));
        lines.push(bar_line(
            c.short(),
            most - room,
            most,
            color,
            format!("room {room:.0}/{most:.0}"),
        ));
    }
}

fn vehicle_lines(
    app: &App,
    w: &World,
    id: u32,
    c: &crate::world::Carrier,
    kind: VKind,
    lines: &mut Vec<Line<'static>>,
) {
    let col = rgb(TRAIN_COLORS[id as usize % TRAIN_COLORS.len()]);
    lines.push(header(
        format!("▌{} (selected)", c.name),
        if kind == VKind::Train {
            col
        } else {
            Color::Rgb(255, 225, 90)
        },
    ));
    let state_col = match c.state {
        VState::NoPath | VState::Blocked => app.pal.warn,
        _ => Color::White,
    };
    let mut st = c.state.label().to_string();
    if let Some(t) = w.trains.iter().find(|t| t.carrier.id == id) {
        st = format!("{st}{}", if t.deadlocked { "  ** DEADLOCK **" } else { "" });
        lines.push(kv(
            "Type",
            app.keys.fill(&format!(
                "{} · {} wagons ({:.0} each)   ({{less}} {{more}} at a station)",
                t.kind.name(),
                t.wagons,
                B::WAGON_CAPACITY[t.kind.i()]
            )),
        ));
    }
    lines.push(Line::from(vec![
        Span::styled(format!("{:<11}", "State"), Style::default().fg(Color::Gray)),
        Span::styled(st, Style::default().fg(state_col)),
    ]));
    let upkeep = match w.trains.iter().find(|t| t.carrier.id == id) {
        Some(t) => t.upkeep(),
        None => B::TRUCK_UPKEEP,
    };
    let earn = c.income.per_min();
    lines.push(Line::from(vec![
        Span::styled(
            format!("{:<11}", "Earning"),
            Style::default().fg(Color::Gray),
        ),
        Span::styled(
            format!("{}/min", money(earn)),
            Style::default().fg(if earn >= upkeep {
                app.pal.good
            } else {
                app.pal.warn
            }),
        ),
        Span::raw(format!(
            "  (costs {}/min, {} so far)",
            money(upkeep),
            money(c.lifetime_income)
        )),
    ]));
    lines.push(Line::from(vec![
        Span::styled(format!("{:<11}", "Cargo"), Style::default().fg(Color::Gray)),
        Span::styled(
            bar(c.cargo.total(), c.capacity, 14),
            Style::default().fg(Color::Rgb(200, 190, 140)),
        ),
        Span::raw(format!(" {:.0}/{:.0}", c.cargo.total(), c.capacity)),
    ]));
    let carried: Vec<String> = Cargo::ALL
        .iter()
        .filter(|x| c.cargo[**x] >= 0.5)
        .map(|x| format!("{} {:.0}", x.short(), c.cargo[*x]))
        .collect();
    if !carried.is_empty() {
        lines.push(kv("", carried.join(" ")));
    }
    if c.schedule.stops.is_empty() {
        lines.push(Line::from(Span::styled(
            app.keys.fill("No schedule. Press {edit} to edit."),
            Style::default().fg(app.pal.warn),
        )));
    }
    for (i, s) in c.schedule.stops.iter().enumerate() {
        let mark = if i == c.cur % c.schedule.stops.len().max(1) {
            "▶"
        } else {
            " "
        };
        lines.push(Line::from(Span::styled(
            format!("{mark}{}. {}", i + 1, s.describe(w)),
            Style::default().fg(if mark == "▶" {
                Color::White
            } else {
                Color::Gray
            }),
        )));
    }
    lines.push(Line::from(Span::styled(app.keys.fill("{edit} edit · {duplicate} buy a copy · {reverse} reverse · {demolish} scrap · {deselect} deselect"), Style::default().fg(Color::DarkGray))));
}

// ------------------------------------------------------------------ bottom panel

fn draw_bottom(f: &mut Frame, app: &App, area: Rect) {
    let w = app.world.as_ref().unwrap();
    if let Mode::Schedule {
        vehicle,
        sel,
        picking,
    } = &app.mode
    {
        draw_schedule(f, app, area, *vehicle, *sel, *picking);
        return;
    }
    let open = w
        .contracts
        .iter()
        .filter(|c| c.state == ContractState::Open)
        .count();
    let news = crate::world::events::current(w).count();
    let contracts_tab = match (open, news) {
        (0, 0) => "Contracts & news".to_string(),
        (o, 0) => format!("Contracts ({o})"),
        (o, n) => format!("Contracts ({o}) & news ({n})"),
    };
    let tabs = [
        (Tab::Alerts, "Alerts".to_string()),
        (Tab::Build, "Build".to_string()),
        (Tab::Vehicles, "Vehicles".to_string()),
        (Tab::Contracts, contracts_tab),
    ];
    let mut title = vec![Span::raw(" ")];
    let mut x = area.x + 1;
    for (t, name) in tabs {
        let w = name.chars().count() as u16 + 2;
        app.hits
            .borrow_mut()
            .push((Rect::new(x, area.y, w, 1), Hit::Tab(t)));
        x += w + 1;
        let st = if t == app.tab {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(255, 200, 90))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        title.push(Span::styled(format!(" {name} "), st));
        title.push(Span::raw(" "));
    }
    title.push(Span::styled(
        app.keys.fill("({next_tab}) "),
        Style::default().fg(Color::DarkGray),
    ));
    let block = Block::default()
        .borders(Borders::TOP)
        .title(Line::from(title))
        .border_style(Style::default().fg(Color::Rgb(60, 66, 90)))
        .style(Style::default().bg(Color::Rgb(16, 18, 24)));
    let inner_h = area.height.saturating_sub(1) as usize;
    let lines: Vec<Line> = match app.tab {
        Tab::Alerts => {
            let start = w.alerts.len().saturating_sub(inner_h);
            w.alerts[start..]
                .iter()
                .map(|a| {
                    let pin = if a.pos.is_some() { "◎" } else { " " };
                    Line::from(vec![
                        Span::styled(
                            format!("{} ", fmt_time(a.time)),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled(format!("{pin} "), Style::default().fg(Color::Gray)),
                        Span::styled(
                            app.keys.fill(&a.text),
                            Style::default().fg(level_color(app, a.level)),
                        ),
                    ])
                })
                .collect()
        }
        Tab::Build => {
            for (i, a) in BUILD_ACTS.iter().enumerate().take(inner_h) {
                app.hits.borrow_mut().push((
                    Rect::new(area.x, area.y + 1 + i as u16, area.width, 1),
                    Hit::Act(*a),
                ));
            }
            build_menu(app)
        }
        Tab::Contracts => contract_lines(app, w),
        Tab::Vehicles => {
            let mut v = vec![];
            for t in &w.trains {
                v.push(vehicle_row(app, w, &t.carrier, t.deadlocked));
            }
            for t in &w.trucks {
                v.push(vehicle_row(app, w, &t.carrier, false));
            }
            if v.is_empty() {
                v.push(Line::from(app.keys.fill("No vehicles. {buy_freight} on a town or industry buys a truck, {buy_freight} on a station buys a train.")));
            }
            let sel = app.selected.and_then(|id| {
                w.trains
                    .iter()
                    .map(|t| t.carrier.id)
                    .chain(w.trucks.iter().map(|t| t.carrier.id))
                    .position(|x| x == id)
            });
            let skip = sel.map(|s| s.saturating_sub(inner_h / 2)).unwrap_or(0);
            let ids = w
                .trains
                .iter()
                .map(|t| t.carrier.id)
                .chain(w.trucks.iter().map(|t| t.carrier.id));
            for (i, id) in ids.skip(skip).take(inner_h).enumerate() {
                app.hits.borrow_mut().push((
                    Rect::new(area.x, area.y + 1 + i as u16, area.width, 1),
                    Hit::Vehicle(id),
                ));
            }
            v.into_iter().skip(skip).collect()
        }
    };
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn vehicle_row(app: &App, w: &World, c: &crate::world::Carrier, dead: bool) -> Line<'static> {
    let sel = app.selected == Some(c.id);
    let stop = c
        .stop()
        .map(|s| w.target_name(s.target))
        .unwrap_or("-".into());
    let st = if dead {
        "DEADLOCK".to_string()
    } else {
        c.state.label().to_string()
    };
    let style = if sel {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Rgb(200, 200, 210))
    } else {
        Style::default().fg(Color::White)
    };
    Line::from(Span::styled(
        format!(
            "{:<10} {:>3.0}/{:<3.0} {:>7}/min → {:<26} {}",
            c.name,
            c.cargo.total(),
            c.capacity,
            money(c.income.per_min()),
            stop,
            st
        ),
        style,
    ))
}

/// The Build tab's rows, top to bottom (each is clickable).
const BUILD_ACTS: [Act; 11] = [
    Act::Road,
    Act::Rail,
    Act::Station,
    Act::Signal,
    Act::Chain,
    Act::Double,
    Act::Hub,
    Act::Demolish,
    Act::BuyFreight,
    Act::BuyExpress,
    Act::Duplicate,
];

fn build_menu(app: &App) -> Vec<Line<'static>> {
    let k = |act: Act, what: &str, cost: String| {
        let key = app.keys.label(act);
        Line::from(vec![
            Span::styled(
                format!(" {key:>3} "),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Rgb(255, 200, 90))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {what:<18}"), Style::default().fg(Color::White)),
            Span::styled(cost, Style::default().fg(Color::Gray)),
        ])
    };

    let rc = B::ROAD_COST;
    let tc = B::RAIL_COST;
    let m = money;
    vec![
        k(
            Act::Road,
            "road",
            format!(
                "{}/{}/{} per tile (plain/forest/hill), bridge {}, tunnel {}",
                m(rc[1]),
                m(rc[2]),
                m(rc[3]),
                m(rc[0]),
                m(rc[4])
            ),
        ),
        k(
            Act::Rail,
            "rail track",
            format!(
                "{}/{}/{} per tile, bridge {}, tunnel {}",
                m(tc[1]),
                m(tc[2]),
                m(tc[3]),
                m(tc[0]),
                m(tc[4])
            ),
        ),
        k(
            Act::Station,
            "station",
            format!(
                "{} — on track; serves everything within 2 tiles",
                m(B::STATION_COST)
            ),
        ),
        k(
            Act::Signal,
            "signal (cycle)",
            format!(
                "{} — two-way → one-way → off.  {} = auto-signal network",
                m(B::SIGNAL_COST),
                app.keys.label(Act::AutoSignal)
            ),
        ),
        k(
            Act::Chain,
            "chain signal",
            "on a signal: toggles block ↔ chain (put chains in front of junctions)".to_string(),
        ),
        k(
            Act::Double,
            "double track",
            "two parallel lines, one-way signals every 6 tiles, ready to join up".to_string(),
        ),
        k(
            Act::Hub,
            "transfer hub",
            format!("{} — road↔rail buffer", m(B::HUB_COST)),
        ),
        k(
            Act::Demolish,
            "demolish",
            format!(
                "refunds {:.0}% of the cost · hold the key or drag the mouse to clear a path",
                B::REFUND * 100.0
            ),
        ),
        k(
            Act::BuyFreight,
            "truck / freight",
            format!(
                "truck {} anywhere · freight train {} + {}/wagon at a station",
                m(B::TRUCK_COST),
                m(B::LOCO_COST[0]),
                m(B::WAGON_COST[0])
            ),
        ),
        k(
            Act::BuyExpress,
            "express train",
            format!(
                "{} + {}/wagon — {:.0}× faster, {} wagons max; built for passengers",
                m(B::LOCO_COST[1]),
                m(B::WAGON_COST[1]),
                B::TRAIN_SPEED[1] / B::TRAIN_SPEED[0],
                B::MAX_WAGONS[1]
            ),
        ),
        k(
            Act::Duplicate,
            "copy vehicle",
            "with a vehicle selected: buy another with the same schedule (and wagons)".to_string(),
        ),
        Line::from(Span::styled(
            format!(
                " Running costs per minute: truck {} · freight {} + {}/wagon · express {} + {}/wagon.",
                m(B::TRUCK_UPKEEP),
                m(B::LOCO_UPKEEP[0]),
                m(B::WAGON_UPKEEP[0]),
                m(B::LOCO_UPKEEP[1]),
                m(B::WAGON_UPKEEP[1])
            ),
            Style::default().fg(Color::DarkGray),
        )),
    ]
}

/// Under a gated stop: what the gate sees right now, and whether it can ever open.
fn gate_line(app: &App, s: &Stop, r: &GateReading) -> Line<'static> {
    let what = r.cargo.label().to_lowercase();
    let amount = |v: f64| {
        if v.is_finite() {
            format!("{v:.0}")
        } else {
            "any number".to_string()
        }
    };
    let (now, most) = match s.gate {
        Some(Gate::Waiting(_)) => (
            format!("{} {what} waiting", amount(r.now)),
            format!("holds at most {}", amount(r.most)),
        ),
        _ => (
            format!("wants {} {what} now", amount(r.now)),
            format!("takes at most {}", amount(r.most)),
        ),
    };
    let (verdict, color) = if r.never() {
        (format!(" · needs {:.0}: NEVER opens", r.need), app.pal.bad)
    } else if r.open() {
        (" · open: it goes".to_string(), Color::Rgb(140, 230, 160))
    } else {
        (
            format!(" · needs {:.0}: skipped for now", r.need),
            Color::Rgb(220, 200, 120),
        )
    };
    Line::from(vec![
        Span::styled(
            format!("       ↳ {now} ({most})"),
            Style::default().fg(Color::Gray),
        ),
        Span::styled(verdict, Style::default().fg(color)),
    ])
}

fn draw_schedule(f: &mut Frame, app: &App, area: Rect, vehicle: u32, sel: usize, picking: bool) {
    let w = app.world.as_ref().unwrap();
    let Some((c, kind)) = w.carrier(vehicle) else {
        return;
    };
    let mut lines = vec![];
    if c.schedule.stops.is_empty() {
        lines.push(Line::from(Span::styled(
            app.keys
                .fill("  (empty) — press {sched_add} to add the first stop"),
            Style::default().fg(Color::Gray),
        )));
    }
    for (i, s) in c.schedule.stops.iter().enumerate() {
        let cur = if i == c.cur % c.schedule.stops.len().max(1) {
            "▶"
        } else {
            " "
        };
        let st = if i == sel {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(120, 200, 255))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::from(Span::styled(
            format!(" {cur} {}. {}", i + 1, s.describe(w)),
            st,
        )));
        if let Some(r) = crate::world::vehicles::gate_reading(w, s, &c.cargo) {
            lines.push(gate_line(app, s, &r));
        }
    }
    lines.push(Line::from(""));
    let help = if picking {
        app.keys
            .fill("PICKING: move the cursor onto the stop and press {confirm} · {cancel} cancel")
    } else {
        app.keys.fill("{sched_add} add · {sched_delete} del · {sched_action} action · {sched_cargo}/{sched_cargo_back} cargo · {sched_cond} condition · {sched_timeout} timeout · {sched_time_more}/{sched_time_less} time · {sched_gate} only-if gate · {sched_gate_less}/{sched_gate_more} gate amount · {sched_move_up}/{sched_move_down} move · {sched_go} go now · {sched_close} close")
    };
    lines.push(Line::from(Span::styled(
        help,
        Style::default().fg(Color::Rgb(150, 200, 255)),
    )));
    let what = if kind == VKind::Train {
        "train → stops are stations"
    } else {
        "truck → stops are towns/industries/hubs"
    };
    let block = Block::default()
        .borders(Borders::TOP)
        .title(Span::styled(
            format!(" Schedule: {} ({what}) ", c.name),
            Style::default()
                .fg(Color::Rgb(120, 200, 255))
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(Color::Rgb(16, 18, 24)));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

// ------------------------------------------------------------------ tutorial

/// The guide box in a corner of the map: why, then what to do now.
fn draw_tutorial(f: &mut Frame, app: &App, map: Rect) {
    let (Some(tut), Some(w)) = (app.tutorial.as_ref(), app.world.as_ref()) else {
        return;
    };
    let Some(step) = tut.current() else { return };
    let width = 58.min(map.width);
    let inner = width.saturating_sub(4) as usize;
    let accent = widgets::SKY;
    let km = &app.keys;
    let mut lines = vec![];
    if tut
        .passed_frame
        .is_some_and(|fr| app.frame.saturating_sub(fr) < 90)
    {
        lines.push(Line::from(Span::styled(
            "✔ Done! On to the next step.",
            Style::default()
                .fg(app.pal.good)
                .add_modifier(Modifier::BOLD),
        )));
    }
    lines.extend(widgets::rich_wrap(
        km,
        step.text,
        inner,
        Style::default().fg(widgets::TEXT),
        None,
    ));
    lines.push(Line::from(""));
    if tut.reviewing() {
        let ahead = tutorial::STEPS[tut.reached].title;
        let note = format!(
            "✔ You've done this one. {{tutorial_skip}} pages on (you're up to \"{ahead}\")."
        );
        lines.extend(widgets::rich_wrap(
            km,
            &note,
            inner,
            Style::default().fg(app.pal.good),
            None,
        ));
    } else if let Some(g) = tutorial::guide_now(app, w, tut) {
        lines.push(Line::from(Span::styled(
            "▶ WHAT TO DO",
            Style::default()
                .fg(widgets::GOLD)
                .add_modifier(Modifier::BOLD),
        )));
        lines.extend(widgets::rich_wrap(
            km,
            g.now,
            inner,
            Style::default()
                .fg(Color::Rgb(255, 236, 170))
                .add_modifier(Modifier::BOLD),
            Some(app.frame),
        ));
        if let Some(t) = tut.trouble(w) {
            lines.push(Line::from(""));
            lines.extend(widgets::rich_wrap(
                km,
                t,
                inner,
                Style::default().fg(app.pal.warn),
                None,
            ));
        }
    }
    let next = match () {
        _ if tut.finished() => "{tutorial_skip} close",
        _ if tut.reviewing() => "{tutorial_skip} next",
        _ => "{tutorial_skip} skip step",
    };
    let foot = km.fill(&if tut.step > 0 {
        format!("{{tutorial_back}} back · {next}")
    } else {
        next.to_string()
    });
    let height = (lines.len() as u16 + 2).min(map.height);
    // a corner that hides none of the marked spots, their tags (the row above)
    // or the cursor: bottom-left if it can
    let view = app.view();
    let mut spots: Vec<(i32, i32)> = tutorial::guide_now(app, w, tut)
        .map(|g| g.marks.iter().map(|(p, _)| view.offset(*p)).collect())
        .unwrap_or_default();
    spots.push(view.offset(app.cursor));
    let corners = [
        Rect::new(map.x, map.bottom() - height, width, height),
        Rect::new(map.right() - width, map.bottom() - height, width, height),
        Rect::new(map.x, map.y, width, height),
        Rect::new(map.right() - width, map.y, width, height),
    ];
    let covers = |r: &Rect, (x, y): (i32, i32)| {
        let (x, y) = (map.x as i32 + x, map.y as i32 + y);
        x >= r.x as i32 && x < r.right() as i32 && y >= r.y as i32 - 1 && y < r.bottom() as i32
    };
    let area = corners
        .iter()
        .copied()
        .find(|r| !spots.iter().any(|s| covers(r, *s)))
        .unwrap_or(corners[0]);
    let n = tutorial::STEPS.len();
    let progress: String = (0..n)
        .map(|i| {
            if i < tut.reached {
                '■'
            } else if i == tut.step {
                '▣'
            } else {
                '□'
            }
        })
        .collect();
    let title = format!(
        " TUTORIAL {}/{n} · {}{} ",
        tut.step + 1,
        step.title,
        if tut.reviewing() { " ✔" } else { "" }
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent))
        .title(Span::styled(
            title,
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(
            Line::from(Span::styled(
                format!(" {progress} "),
                Style::default().fg(Color::Rgb(90, 130, 170)),
            ))
            .left_aligned(),
        )
        .title_bottom(
            Line::from(Span::styled(
                format!(" {foot} "),
                Style::default().fg(widgets::SOFT),
            ))
            .right_aligned(),
        )
        .style(Style::default().bg(Color::Rgb(16, 22, 34)));
    f.render_widget(Clear, area);
    f.render_widget(Paragraph::new(lines).block(block), area);
}

/// The tutorial's marked spots on the map: brackets round the tile and a
/// label over it, or an arrow at the map's edge pointing the way.
fn draw_tutorial_marks(f: &mut Frame, app: &App, map: Rect) {
    let (Some(tut), Some(w)) = (app.tutorial.as_ref(), app.world.as_ref()) else {
        return;
    };
    if tut.reviewing() || app.covered() {
        return;
    }
    let Some(g) = tutorial::guide_now(app, w, tut) else {
        return;
    };
    let view = app.view();
    let z = view.zoom;
    let (gx, gy) = z.glyph();
    let pulse = (app.frame / 8).is_multiple_of(2);
    let gold = if pulse {
        Color::Rgb(255, 214, 90)
    } else {
        Color::Rgb(255, 150, 50)
    };
    let tag = Style::default()
        .fg(Color::Black)
        .bg(gold)
        .add_modifier(Modifier::BOLD);
    let buf = f.buffer_mut();
    let put = |buf: &mut ratatui::buffer::Buffer, x: i32, y: i32, s: &str, st: Style| {
        for (i, ch) in s.chars().enumerate() {
            let cx = x + i as i32;
            if cx >= 0
                && y >= 0
                && cx < map.width as i32
                && y < map.height as i32
                && let Some(cell) = buf.cell_mut((map.x + cx as u16, map.y + y as u16))
            {
                cell.set_char(ch).set_style(st);
            }
        }
    };
    for (p, label) in &g.marks {
        let (ox, oy) = view.offset(*p);
        let (left, top) = (ox - gx, oy - gy);
        let (mw, mh) = (map.width as i32, map.height as i32);
        let inside = left >= 0 && top >= 0 && left + z.cols() <= mw && top + z.rows() <= mh;
        if inside {
            // brackets either side of the tile's block, keeping the ground's color
            for (x, ch) in [(left - 1, '▶'), (left + z.cols(), '◀')] {
                if x >= 0 && x < mw {
                    for y in top..top + z.rows() {
                        if let Some(cell) = buf.cell_mut((map.x + x as u16, map.y + y as u16)) {
                            let bg = cell.bg;
                            cell.set_char(ch).set_style(
                                Style::default()
                                    .fg(gold)
                                    .bg(bg)
                                    .add_modifier(Modifier::BOLD),
                            );
                        }
                    }
                }
            }
            let text = format!(" ▼ {label} ");
            let (text, y) = if top >= 1 {
                (text, top - 1)
            } else {
                (format!(" ▲ {label} "), top + z.rows())
            };
            let x = (ox - text.chars().count() as i32 / 2)
                .clamp(0, (mw - text.chars().count() as i32).max(0));
            put(buf, x, y, &text, tag);
        } else {
            // off screen: an arrow on the edge, toward it
            let x = ox.clamp(0, mw - 1);
            let y = oy.clamp(0, mh - 1);
            let arrow = match (
                ox.signum() * (ox < 0 || ox >= mw) as i32,
                (oy < 0) as i32 - (oy >= mh) as i32,
            ) {
                (-1, 1) => '↖',
                (-1, -1) => '↙',
                (1, 1) => '↗',
                (1, -1) => '↘',
                (-1, _) => '←',
                (1, _) => '→',
                (_, 1) => '↑',
                _ => '↓',
            };
            let dist = p.manhattan(app.cursor);
            let text = format!(" {arrow} {label} ({dist} tiles) ");
            let n = text.chars().count() as i32;
            let tx = if ox >= mw {
                mw - n
            } else if ox < 0 {
                0
            } else {
                (x - n / 2).clamp(0, (mw - n).max(0))
            };
            put(buf, tx, y, &text, tag);
        }
    }
}

/// Greedy word wrap, so the box can be sized to its text.
pub(super) fn wrap_words(s: &str, width: usize) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    for word in s.split_whitespace() {
        if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > width {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

// ------------------------------------------------------------------ popups

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    widgets::centered(area, w, h)
}

fn draw_confirm(f: &mut Frame, app: &App, c: Confirm) {
    let w = app.world.as_ref().unwrap();
    let (title, text) = match c {
        Confirm::Quit => (
            "QUIT RAILHEAD?",
            "Your game is autosaved: Continue on the title screen picks it up.".to_string(),
        ),
        Confirm::NewGame => (
            "BACK TO THE TITLE?",
            "This game is autosaved, so you can Continue it later.".to_string(),
        ),
        Confirm::Scrap(id) => (
            "SCRAP IT?",
            format!(
                "Scrap {}? You get nothing back for vehicles.",
                w.carrier(id).map(|c| c.0.name.clone()).unwrap_or_default()
            ),
        ),
    };
    let area = centered(f.area(), 58, 8);
    f.render_widget(Clear, area);
    let block = widgets::popup(title, app.keys.fill("{yes} yes · any other key no"));
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            text,
            Style::default().fg(widgets::TEXT),
        )))
        .wrap(Wrap { trim: true })
        .centered(),
        Rect::new(inner.x + 1, inner.y + 1, inner.width.saturating_sub(2), 2),
    );
    // two buttons, side by side in the middle
    let yes = format!("  Yes ({})  ", app.keys.label(Act::Yes));
    let no = "  No  ".to_string();
    let (yw, nw) = (yes.chars().count() as u16, no.chars().count() as u16);
    let x = inner.x + inner.width.saturating_sub(yw + nw + 4) / 2;
    let y = inner.y + inner.height.saturating_sub(2);
    let yes_r = Rect::new(x, y, yw, 1);
    let no_r = Rect::new(x + yw + 4, y, nw, 1);
    f.render_widget(
        Paragraph::new(Span::styled(
            yes,
            Style::default()
                .fg(Color::Black)
                .bg(widgets::GOLD)
                .add_modifier(Modifier::BOLD),
        )),
        yes_r,
    );
    f.render_widget(
        Paragraph::new(Span::styled(
            no,
            Style::default()
                .fg(widgets::TEXT)
                .bg(Color::Rgb(50, 56, 76))
                .add_modifier(Modifier::BOLD),
        )),
        no_r,
    );
    app.hits.borrow_mut().push((yes_r, Hit::Yes));
    app.hits.borrow_mut().push((no_r, Hit::No));
}

/// The game menu (Esc): resume, save, settings, quit.
fn draw_menu(f: &mut Frame, app: &App) {
    let Some(sel) = app.menu else { return };
    let items = menu_items(app);
    let gaps = items
        .iter()
        .filter(|i| matches!(i, MenuItem::Load | MenuItem::Controls))
        .count() as u16;
    let area = centered(f.area(), 62, items.len() as u16 + gaps + 4);
    f.render_widget(Clear, area);
    let block = widgets::popup(
        "RAILHEAD · PAUSED",
        app.keys
            .fill("{menu_up} {menu_down} choose · {menu_pick} pick · {menu_close} back · or click"),
    );
    let inner = block.inner(area);
    f.render_widget(block, area);
    let mut lines = vec![Line::from("")];
    for (i, item) in items.iter().enumerate() {
        let y = inner.y + lines.len() as u16;
        app.hits
            .borrow_mut()
            .push((Rect::new(inner.x, y, inner.width, 1), Hit::MenuRow(i)));
        let (label, about) = item.label();
        let focus = i == sel;
        let mut line = Line::from(vec![
            Span::styled(
                if focus { " ▶ " } else { "   " },
                Style::default()
                    .fg(widgets::GOLD)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {label:<18}"), widgets::row_style(focus)),
            Span::styled(
                format!("  {about}"),
                Style::default().fg(if focus { widgets::TEXT } else { widgets::FAINT }),
            ),
        ]);
        if focus {
            line = line.style(Style::default().bg(Color::Rgb(28, 32, 46)));
        }
        lines.push(line);
        if matches!(item, MenuItem::Load | MenuItem::Controls) {
            lines.push(Line::from(""));
        }
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// "+£120" rising from vehicles that just got paid, fading as it goes.
fn draw_pops(f: &mut Frame, app: &App, area: Rect) {
    let buf = f.buffer_mut();
    for p in &app.pops {
        let age = app.frame.saturating_sub(p.born);
        let rise = (age / 15) as i32;
        let text = format!("+{}", money(p.amount));
        let (px, py) = app.view().offset(p.pos);
        let x0 = px - text.chars().count() as i32 / 2;
        let y = py - 1 - rise;
        if y < 0 || y >= area.height as i32 {
            continue;
        }
        let fade = 1.0 - age as f64 / POP_FRAMES as f64;
        let col = lerp((90, 90, 70), (255, 225, 110), fade);
        for (i, ch) in text.chars().enumerate() {
            let x = x0 + i as i32;
            if x < 0 || x >= area.width as i32 {
                continue;
            }
            if let Some(cell) = buf.cell_mut((area.x + x as u16, area.y + y as u16)) {
                let bg = cell.bg;
                cell.set_char(ch).set_style(
                    Style::default()
                        .fg(rgb(col))
                        .bg(bg)
                        .add_modifier(Modifier::BOLD),
                );
            }
        }
    }
}

/// `kv` with a wider, indented label for the end screen.
fn kv2(k: &str, v: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {k:<12}"), Style::default().fg(Color::Gray)),
        Span::raw(v),
    ])
}

/// The end screen's award: the medal in big letters (shining), and how the
/// score compares with your best on this map.
fn medal_award(app: &App, w: &World) -> Vec<Line<'static>> {
    let mut v = vec![Line::from("")];
    let medal = w.stats.medal;
    if medal == 0 {
        let t = w.medal_targets()[0];
        v.push(
            Line::from(Span::styled(
                format!("No medal this time: bronze needs {t:.0}."),
                Style::default().fg(Color::Gray),
            ))
            .centered(),
        );
    } else {
        let name = crate::world::MEDAL_NAMES[medal - 1].to_uppercase();
        let spaced: String = name.chars().flat_map(|c| [c, ' ']).collect();
        // a glint runs along the letters
        let glint = (app.frame / 2 % 40) as usize;
        let base = MEDAL_COLORS[medal - 1];
        let mut spans = vec![Span::styled(
            "●  ",
            Style::default().fg(rgb(base)).add_modifier(Modifier::BOLD),
        )];
        for (i, ch) in format!("{spaced} M E D A L").chars().enumerate() {
            let near = (i as i64 - glint as i64).abs() <= 1;
            let col = if near {
                lerp(base, (255, 255, 255), 0.7)
            } else {
                base
            };
            spans.push(Span::styled(
                ch.to_string(),
                Style::default().fg(rgb(col)).add_modifier(Modifier::BOLD),
            ));
        }
        spans.push(Span::styled(
            "  ●",
            Style::default().fg(rgb(base)).add_modifier(Modifier::BOLD),
        ));
        v.push(Line::from(spans).centered());
    }
    let note = match &app.record_note {
        Some(o) if o.new_best && o.previous.is_some() => Some((
            format!(
                "NEW PERSONAL BEST on this map! (was {:.0})",
                o.previous.unwrap_or(0.0)
            ),
            Color::Rgb(255, 210, 90),
        )),
        Some(o) if o.new_best => Some((
            "Your first Charter on this map: that's the score to beat.".to_string(),
            Color::Gray,
        )),
        Some(o) => Some((
            format!(
                "Your best on this map is still {:.0}.",
                o.previous.unwrap_or(0.0)
            ),
            Color::Gray,
        )),
        None => None,
    };
    if let Some((text, col)) = note {
        v.push(
            Line::from(Span::styled(
                text,
                Style::default().fg(col).add_modifier(Modifier::BOLD),
            ))
            .centered(),
        );
    }
    v
}

/// The Franchise end screen's line on the records: most reviews on this map.
fn franchise_award(app: &App) -> Vec<Line<'static>> {
    let note = match &app.record_note {
        Some(o) if o.new_best && o.previous.is_some() => Some((
            format!(
                "NEW PERSONAL BEST on this map! (was {:.0} reviews)",
                o.previous.unwrap_or(0.0)
            ),
            Color::Rgb(255, 210, 90),
        )),
        Some(o) if o.new_best => Some((
            "Your first Franchise on this map: that's the run to beat.".to_string(),
            Color::Gray,
        )),
        Some(o) => Some((
            format!(
                "Your best on this map is still {:.0} reviews.",
                o.previous.unwrap_or(0.0)
            ),
            Color::Gray,
        )),
        None => None,
    };
    let mut v = vec![Line::from("")];
    if let Some((text, col)) = note {
        v.push(
            Line::from(Span::styled(
                text,
                Style::default().fg(col).add_modifier(Modifier::BOLD),
            ))
            .centered(),
        );
    }
    v
}

fn draw_end(f: &mut Frame, app: &App) {
    let w = app.world.as_ref().unwrap();
    let h = (w.towns.len() as u16 + 28).min(f.area().height);
    let area = centered(f.area(), 88, h);
    f.render_widget(Clear, area);
    let start = w.stats.start_prosperity.max(1.0);
    let mult = w.prosperity() / start;
    let passed = w.stats.reviews_passed;
    let quip = match w.stats.medal {
        _ if w.franchise() => match passed {
            0 => "The Board never warmed to you. The station cat was promoted.",
            1..=3 => "A brief tenure. The Board has already printed your successor's cards.",
            4..=7 => "A solid run. They'll be using your timetables for years.",
            _ => "A legend of the line. The Board sacked you, then named the boardroom after you.",
        },
        3 => "A golden age. They'll name a terminus after you.",
        2 => "The region hums. Somewhere a stationmaster weeps with joy.",
        1 => "A respectable network. The board would like more, naturally.",
        _ if mult >= 2.0 => "The towns grew a little. The board expected rather more.",
        _ => "The towns waited. The trains... mostly didn't come.",
    };
    let gold = Color::Rgb(255, 210, 90);
    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            if w.franchise() {
                "F R A N C H I S E   R E V O K E D"
            } else {
                "C H A R T E R   C O M P L E T E"
            },
            Style::default().fg(gold).add_modifier(Modifier::BOLD),
        ))
        .centered(),
        Line::from(Span::styled(
            quip,
            Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::ITALIC),
        ))
        .centered(),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                if w.franchise() {
                    "REVIEWS PASSED "
                } else {
                    "SCORE "
                },
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                if w.franchise() {
                    passed.to_string()
                } else {
                    format!("{:.0}", w.score())
                },
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if w.franchise() {
                    format!(
                        "   held for {} · Prosperity {:.0} → {:.0} ({mult:.1}×)",
                        fmt_time(w.time),
                        start,
                        w.prosperity()
                    )
                } else {
                    format!(
                        "   Prosperity {:.0} → {:.0} ({mult:.1}×)  + contracts {:.0}",
                        start,
                        w.prosperity(),
                        w.stats.contract_bonus
                    )
                },
                Style::default().fg(Color::Gray),
            ),
        ])
        .centered(),
    ];
    let pros: Vec<f64> = w.history.iter().map(|s| s.prosperity).collect();
    lines.splice(
        2..2,
        if w.franchise() {
            franchise_award(app)
        } else {
            medal_award(app, w)
        },
    );
    lines.push(
        Line::from(Span::styled(
            sparkline(&pros, 60),
            Style::default().fg(Color::Rgb(140, 230, 160)),
        ))
        .centered(),
    );
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!(
            "  {:<16} {:<11} {:>7} {:>8}  {}",
            "Town", "Best tier", "Start", "End", "On network"
        ),
        Style::default().fg(Color::Gray),
    )));
    let mut towns: Vec<&crate::world::Town> = w.towns.iter().collect();
    towns.sort_by(|a, b| b.pop.total_cmp(&a.pop));
    for t in towns {
        let col = if t.served {
            app.pal
                .ramp_color((t.pop / t.start_pop - 1.0).clamp(0.0, 4.0) / 4.0)
        } else {
            Color::DarkGray
        };
        lines.push(Line::from(Span::styled(
            format!(
                "  {:<16} {:<11} {:>7.0} {:>8.0}  {}",
                t.name,
                crate::world::TIER_NAMES[t.best_tier],
                t.start_pop,
                t.pop,
                if t.served { "yes" } else { "—" }
            ),
            Style::default().fg(col),
        )));
    }
    lines.push(Line::from(""));
    // Best and worst lines by profit over their lifetime.
    let profit = |c: &crate::world::Carrier, upkeep: f64| {
        c.lifetime_income - upkeep * (w.time - c.born) / 60.0
    };
    let mut fleet: Vec<(String, f64)> = w
        .trains
        .iter()
        .map(|t| (t.carrier.name.clone(), profit(&t.carrier, t.upkeep())))
        .chain(
            w.trucks
                .iter()
                .map(|t| (t.carrier.name.clone(), profit(&t.carrier, B::TRUCK_UPKEEP))),
        )
        .collect();
    fleet.sort_by(|a, b| b.1.total_cmp(&a.1));
    if let (Some(best), Some(worst)) = (fleet.first(), fleet.last()) {
        lines.push(kv2(
            "Best line",
            format!("{} ({} profit)", best.0, money(best.1)),
        ));
        if fleet.len() > 1 {
            lines.push(kv2(
                "Worst line",
                format!("{} ({} profit)", worst.0, money(worst.1)),
            ));
        }
    }
    let s = &w.stats;
    lines.push(kv2(
        "Money",
        format!(
            "earned {} · spent {} · upkeep {} · cash {}",
            money(s.revenue),
            money(s.spent),
            money(s.upkeep),
            money(w.cash)
        ),
    ));
    let done = w
        .contracts
        .iter()
        .filter(|c| c.state == ContractState::Done)
        .count();
    lines.push(kv2(
        "Contracts",
        format!(
            "filled {done} of {} posted, worth +{:.0} score",
            w.contracts.len(),
            s.contract_bonus
        ),
    ));
    lines.push(kv2(
        "Network",
        format!(
            "{} trains, {} trucks, {} stations, {} deadlocks",
            w.trains.len(),
            w.trucks.len(),
            w.stations.iter().filter(|s| !s.removed).count(),
            s.deadlocks
        ),
    ));
    lines.push(kv2(
        "Milestones",
        format!(
            "{} of {} reached",
            crate::world::milestones::reached(w),
            crate::world::milestones::MILESTONES.len()
        ),
    ));
    lines.push(kv2(
        "Seed",
        format!("{} — share it and compare scores!", w.map.seed),
    ));
    lines.push(Line::from(""));
    lines.push(
        Line::from(Span::styled(
            app.keys.fill(
                "{end_new+}: new game     {end_close+}: look at your network     {end_quit}: quit",
            ),
            Style::default().fg(Color::White),
        ))
        .centered(),
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(gold))
        .style(Style::default().bg(Color::Rgb(14, 16, 24)));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn contract_lines(app: &App, w: &World) -> Vec<Line<'static>> {
    let mut v = vec![];
    for n in crate::world::events::current(w) {
        let left = n.end - w.time;
        v.push(Line::from(vec![
            Span::styled(" ▤ ", Style::default().fg(Color::Rgb(236, 226, 200))),
            Span::styled(
                format!("{} ", n.headline),
                Style::default()
                    .fg(Color::Rgb(236, 226, 200))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("{} ", n.body), Style::default().fg(Color::Gray)),
            Span::styled(
                format!("({} left)", fmt_clock(left)),
                Style::default().fg(if left < 60.0 {
                    app.pal.warn
                } else {
                    Color::DarkGray
                }),
            ),
        ]));
    }
    let open: Vec<_> = w
        .contracts
        .iter()
        .filter(|c| c.state == ContractState::Open)
        .collect();
    if open.is_empty() {
        let next = (w.stats.last_contract + B::CONTRACT_EVERY).max(B::CONTRACT_FIRST) - w.time;
        v.push(Line::from(Span::styled(
            format!(" No open contracts. Towns on your network post orders every few minutes (next in ~{:.0}s).", next.max(0.0)),
            Style::default().fg(Color::Gray),
        )));
    }
    for c in open {
        let left = (c.deadline - w.time).max(0.0);
        let col = if left < 60.0 {
            app.pal.bad
        } else {
            Color::White
        };
        v.push(Line::from(vec![
            Span::styled(
                format!(" § {:<34}", c.describe(w)),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                bar(c.delivered, c.amount, 16),
                Style::default().fg(app.pal.ramp_color(c.delivered / c.amount)),
            ),
            Span::raw(format!(
                " {:.0}/{:.0} ",
                c.delivered.min(c.amount),
                c.amount
            )),
            Span::styled(
                format!("{} left ", fmt_clock(left)),
                Style::default().fg(col),
            ),
            Span::styled(
                format!(
                    "→ {} + {:.0} score",
                    money(c.cash_reward()),
                    c.score_reward()
                ),
                Style::default().fg(Color::Rgb(255, 225, 120)),
            ),
        ]));
    }
    let done = w
        .contracts
        .iter()
        .filter(|c| c.state == ContractState::Done)
        .count();
    let failed = w
        .contracts
        .iter()
        .filter(|c| c.state == ContractState::Failed)
        .count();
    v.push(Line::from(Span::styled(
        format!(
            " Filled {done} · missed {failed} · bonus score so far {:.0}",
            w.stats.contract_bonus
        ),
        Style::default().fg(Color::DarkGray),
    )));
    let reached = |i: usize| w.milestones.get(i).is_some_and(|m| m.is_some());
    if let Some((_, m)) = crate::world::milestones::MILESTONES
        .iter()
        .enumerate()
        .find(|(i, _)| !reached(*i))
    {
        let pay = if w.sandbox() {
            String::new()
        } else {
            format!(" for {}", money(m.reward()))
        };
        v.push(Line::from(vec![
            Span::styled(
                " ✦ Next milestone: ",
                Style::default().fg(Color::Rgb(255, 210, 90)),
            ),
            Span::styled(format!("{}{pay}", m.goal), Style::default().fg(Color::Gray)),
        ]));
    }
    v
}

/// Medal targets and how close the score is.
fn medal_lines(w: &World) -> Vec<Line<'static>> {
    if w.sandbox() {
        return vec![];
    }
    if w.franchise() {
        let mut spans = vec![
            Span::styled(
                format!("  {:<12}", "Board"),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!(
                    "⚖ {} passed · {} of {} strikes",
                    w.stats.reviews_passed,
                    w.stats.strikes,
                    B::REVIEW_STRIKES
                ),
                Style::default().fg(Color::White),
            ),
        ];
        if let Some(r) = w.next_review() {
            spans.push(Span::styled(
                format!(
                    "   next: review {} in {} wants {:.0}",
                    r.number,
                    fmt_clock(r.secs_left as f64),
                    r.target
                ),
                Style::default().fg(Color::Gray),
            ));
        }
        return vec![Line::from(spans)];
    }
    let mut spans = vec![Span::styled(
        format!("  {:<12}", "Medals"),
        Style::default().fg(Color::Gray),
    )];
    for (i, t) in w.medal_targets().iter().enumerate() {
        let won = w.stats.medal > i;
        let col = if won {
            rgb(MEDAL_COLORS[i])
        } else {
            Color::DarkGray
        };
        let tick = if won { " ✔" } else { "" };
        spans.push(Span::styled(
            format!("● {} {t:.0}{tick}   ", crate::world::MEDAL_NAMES[i]),
            Style::default().fg(col).add_modifier(if won {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }),
        ));
    }
    spans.push(Span::styled(
        format!("(score {:.0})", w.score()),
        Style::default().fg(Color::Gray),
    ));
    vec![Line::from(spans)]
}

/// Milestones reached, and the next few to aim for with their bounties.
fn milestone_lines(w: &World) -> Vec<Line<'static>> {
    use crate::world::milestones::{MILESTONES, reached};
    let done: Vec<&str> = MILESTONES
        .iter()
        .enumerate()
        .filter(|(i, _)| w.milestones.get(*i).is_some_and(|m| m.is_some()))
        .map(|(_, m)| m.name)
        .collect();
    let next: Vec<String> = MILESTONES
        .iter()
        .enumerate()
        .filter(|(i, _)| !w.milestones.get(*i).is_some_and(|m| m.is_some()))
        .take(4)
        .map(|(_, m)| {
            if w.sandbox() {
                m.goal.to_string()
            } else {
                format!("{} ({})", m.goal, money(m.reward()))
            }
        })
        .collect();
    let gold = Style::default().fg(Color::Rgb(255, 210, 90));
    let mut v = vec![Line::from(vec![
        Span::styled(
            format!("  {:<12}", "Milestones"),
            Style::default().fg(Color::Gray),
        ),
        Span::styled(
            format!("{} of {}  ", reached(w), MILESTONES.len()),
            gold.add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if done.is_empty() {
                "none yet".to_string()
            } else {
                format!("✦ {}", done.join(" · "))
            },
            gold,
        ),
    ])];
    if !next.is_empty() {
        v.push(Line::from(vec![
            Span::styled(
                format!("  {:<12}", "Next up"),
                Style::default().fg(Color::Gray),
            ),
            Span::raw(next.join(" · ")),
        ]));
    }
    v
}

/// The statistics screen (i): graphs over the whole game and the league tables.
fn draw_stats(f: &mut Frame, app: &App) {
    let w = app.world.as_ref().unwrap();
    let area = centered(f.area(), 100, 40);
    f.render_widget(Clear, area);
    let width = 64;
    let h = &w.history;
    let col = |s: &str, v: String| {
        Span::styled(format!("{s:<14}{v:<16}"), Style::default().fg(Color::Gray))
    };
    let graph = |label: &str, now: String, vals: Vec<f64>, color: Color| {
        Line::from(vec![
            col(label, now),
            Span::styled(sparkline(&vals, width), Style::default().fg(color)),
        ])
    };
    let per_min = 60.0 / B::SAMPLE_SECS as f64;
    let peak = h.iter().map(|s| s.prosperity).fold(0.0, f64::max);
    let mut lines = vec![
        Line::from(""),
        graph(
            "Prosperity",
            format!("{:.0} (peak {:.0})", w.prosperity(), peak),
            h.iter().map(|s| s.prosperity).collect(),
            Color::Rgb(140, 230, 160),
        ),
        graph(
            "Cash",
            money(w.cash),
            h.iter().map(|s| s.cash).collect(),
            Color::Rgb(255, 225, 120),
        ),
        graph(
            "Revenue",
            format!(
                "{}/min",
                money(h.last().map(|s| s.revenue * per_min).unwrap_or(0.0))
            ),
            h.iter().skip(1).map(|s| s.revenue).collect(),
            Color::Rgb(120, 200, 255),
        ),
        graph(
            "Delivered",
            format!(
                "{:.0}/min",
                h.last().map(|s| s.delivered * per_min).unwrap_or(0.0)
            ),
            h.iter().skip(1).map(|s| s.delivered).collect(),
            Color::Rgb(220, 170, 255),
        ),
        Line::from(Span::styled(
            format!(
                "              {:<16}one bar ≈ {} · whole game so far",
                "",
                fmt_clock((w.time / width as f64).max(B::SAMPLE_SECS as f64))
            ),
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        header(
            "  Best lines (profit over their lifetime)",
            Color::Rgb(255, 200, 90),
        ),
    ];
    let profit = |c: &crate::world::Carrier, upkeep: f64| {
        c.lifetime_income - upkeep * (w.time - c.born) / 60.0
    };
    let mut fleet: Vec<(String, f64, f64)> = w
        .trains
        .iter()
        .map(|t| {
            (
                t.carrier.name.clone(),
                profit(&t.carrier, t.upkeep()),
                t.carrier.income.per_min() - t.upkeep(),
            )
        })
        .chain(w.trucks.iter().map(|t| {
            (
                t.carrier.name.clone(),
                profit(&t.carrier, B::TRUCK_UPKEEP),
                t.carrier.income.per_min() - B::TRUCK_UPKEEP,
            )
        }))
        .collect();
    fleet.sort_by(|a, b| b.1.total_cmp(&a.1));
    if fleet.is_empty() {
        lines.push(Line::from(Span::styled(
            "  no vehicles yet",
            Style::default().fg(Color::DarkGray),
        )));
    }
    for (name, total, now) in fleet.iter().take(5) {
        let c = if *now >= 0.0 {
            app.pal.good
        } else {
            app.pal.bad
        };
        lines.push(Line::from(vec![
            Span::raw(format!("  {name:<14} {:>10} total  ", money(*total))),
            Span::styled(
                format!(
                    "{}{}/min now",
                    if *now >= 0.0 { "+" } else { "" },
                    money(*now)
                ),
                Style::default().fg(c),
            ),
        ]));
    }
    if fleet.len() > 5 {
        let (name, total, _) = fleet.last().unwrap();
        lines.push(Line::from(Span::styled(
            format!("  … worst: {name} ({})", money(*total)),
            Style::default().fg(Color::DarkGray),
        )));
    }
    lines.push(Line::from(""));
    lines.push(header("  Towns", Color::Rgb(255, 200, 90)));
    let mut towns: Vec<&crate::world::Town> = w.towns.iter().collect();
    towns.sort_by(|a, b| b.pop.total_cmp(&a.pop));
    for t in towns.iter().take(6) {
        let happy = t.happiness();
        lines.push(Line::from(vec![
            Span::raw(format!(
                "  {:<14} {:<10} {:>6.0} ({:+.0}%)  ",
                t.name,
                t.tier_name(),
                t.pop,
                (t.pop / t.start_pop - 1.0) * 100.0
            )),
            Span::styled(
                bar(happy, 1.0, 10),
                Style::default().fg(if t.served {
                    app.pal.ramp_color(happy)
                } else {
                    Color::DarkGray
                }),
            ),
            Span::styled(
                if t.served { "" } else { "  not on network" },
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }
    lines.push(Line::from(""));
    let d = &w.stats.delivered;
    let moved: Vec<String> = Cargo::ALL
        .iter()
        .filter(|c| d[**c] >= 1.0)
        .map(|c| format!("{} {:.0}", c.short(), d[*c]))
        .collect();
    lines.push(kv2(
        "Delivered",
        if moved.is_empty() {
            "nothing yet".into()
        } else {
            moved.join(" · ")
        },
    ));
    lines.extend(medal_lines(w));
    lines.extend(milestone_lines(w));
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            " STATISTICS (any key closes) ",
            Style::default().fg(Color::Rgb(255, 200, 90)),
        ))
        .style(Style::default().bg(Color::Rgb(12, 14, 20)));
    f.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

// ------------------------------------------------------------------ overview

/// How many tiles one overview column covers, so the whole map fits `area`
/// (each cell shows two half-height pixels, so rows cover half as many).
pub fn overview_scale(w: &World, area: Rect) -> f64 {
    let sx = w.map.w as f64 / area.width.max(1) as f64;
    let sy = w.map.h as f64 / area.height.saturating_sub(2).max(1) as f64;
    sx.max(sy).max(1.0)
}

/// The whole map at once, drawn with half blocks. The box is your view.
fn draw_overview(f: &mut Frame, app: &App, area: Rect) {
    let w = app.world.as_ref().unwrap();
    let s = overview_scale(w, area);
    let cols = ((w.map.w as f64 / s).ceil() as u16).min(area.width);
    let rows = ((w.map.h as f64 / s).ceil() as u16).min(area.height.saturating_sub(2));
    let ox = area.x + (area.width - cols) / 2;
    let oy = area.y + 1 + (area.height.saturating_sub(2) - rows) / 2;
    app.hits
        .borrow_mut()
        .push((Rect::new(ox, oy, cols, rows), Hit::Overview(s)));
    f.render_widget(Clear, area);
    f.render_widget(
        Block::default().style(Style::default().bg(Color::Rgb(10, 18, 34))),
        area,
    );
    let houses = house_tiles(w);
    let blink = (app.frame / 8).is_multiple_of(2);
    let (vw, vh) = app.view().tiles();
    let view = (app.cam.x, app.cam.y, app.cam.x + vw, app.cam.y + vh);
    // The color of the most important thing in a patch of tiles.
    let pixel = |x0: f64, y0: f64, x1: f64, y1: f64| -> (u8, u8, u8) {
        let (ax, ay) = (x0.floor() as i32, y0.floor() as i32);
        let (bx, by) = (
            (x1.ceil() as i32).max(ax + 1),
            (y1.ceil() as i32).max(ay + 1),
        );
        let mut best = (0, (0u8, 0u8, 0u8));
        for y in ay..by.min(w.map.h) {
            for x in ax..bx.min(w.map.w) {
                let p = Pos::new(x, y);
                let t = w.tile(p);
                let (rank, col) = if p == app.cursor && blink {
                    (9, (255, 230, 80))
                } else if t.station.is_some() {
                    (8, (200, 140, 255))
                } else if let Some(id) = t.site {
                    match w.sites[id as usize].kind {
                        SiteKind::Town(ti) if ti == w.map.home => (7, (255, 240, 170)),
                        SiteKind::Town(_) => (7, (255, 255, 255)),
                        SiteKind::Industry(k) => (6, industry_color(k)),
                        SiteKind::Hub => (6, (255, 220, 120)),
                    }
                } else if t.rail.present() {
                    (5, (230, 230, 240))
                } else if t.road.present() {
                    (4, (185, 160, 115))
                } else if houses.contains_key(&p) {
                    (3, (160, 120, 95))
                } else {
                    (1, scale(terrain_cell(w, p, false).bg, 1.15))
                };
                let edge = (x == view.0 || x == view.2 - 1) && (view.1..view.3).contains(&y)
                    || (y == view.1 || y == view.3 - 1) && (view.0..view.2).contains(&x);
                let (rank, col) = if edge && rank < 5 {
                    (2, lerp(col, (255, 255, 255), 0.45))
                } else {
                    (rank, col)
                };
                if rank > best.0 {
                    best = (rank, col);
                }
            }
        }
        best.1
    };
    let buf = f.buffer_mut();
    for cy in 0..rows {
        for cx in 0..cols {
            let x0 = cx as f64 * s;
            let y0 = cy as f64 * s;
            let top = pixel(x0, y0, x0 + s, y0 + s / 2.0);
            let bot = if y0 + s / 2.0 < w.map.h as f64 {
                pixel(x0, y0 + s / 2.0, x0 + s, y0 + s)
            } else {
                (10, 18, 34)
            };
            if let Some(cell) = buf.cell_mut((ox + cx, oy + cy)) {
                cell.set_char('▀')
                    .set_style(Style::default().fg(rgb(top)).bg(rgb(bot)));
            }
        }
    }
    // Town names under their towns.
    if app.labels {
        for (ti, town) in w.towns.iter().enumerate() {
            let label = if ti == w.map.home {
                format!("★{}", town.name)
            } else {
                town.name.clone()
            };
            let len = label.chars().count() as i32;
            let x = ox as i32 + (town.pos.x as f64 / s) as i32 - len / 2;
            let y = oy as i32 + (town.pos.y as f64 / s) as i32 + 1;
            if y >= (oy + rows) as i32 {
                continue;
            }
            let col = if town.served {
                app.pal.ramp(town.happiness())
            } else {
                (200, 200, 200)
            };
            for (i, ch) in label.chars().enumerate() {
                let cx = x + i as i32;
                if cx < area.x as i32 || cx >= (area.x + area.width) as i32 {
                    continue;
                }
                if let Some(cell) = buf.cell_mut((cx as u16, y as u16)) {
                    cell.set_char(ch).set_style(
                        Style::default()
                            .fg(rgb(col))
                            .bg(Color::Rgb(16, 20, 30))
                            .add_modifier(Modifier::BOLD),
                    );
                }
            }
        }
    }
    let title = app.keys.fill(&format!(" OVERVIEW · {}×{} map · move to look around · {{overview}} / {{cancel}} / {{confirm}} back to the map ", w.map.w, w.map.h));
    f.render_widget(
        Paragraph::new(
            Line::from(Span::styled(
                title,
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Rgb(120, 200, 255))
                    .add_modifier(Modifier::BOLD),
            ))
            .centered(),
        ),
        Rect::new(area.x, area.y, area.width, 1),
    );
    let legend = "▀ white = towns · purple = stations · light = rail · tan = road · colored = industries · box = what the map shows";
    f.render_widget(
        Paragraph::new(
            Line::from(Span::styled(legend, Style::default().fg(Color::Gray))).centered(),
        ),
        Rect::new(area.x, area.y + area.height - 1, area.width, 1),
    );
}
