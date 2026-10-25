//! Terminal front-end: title screen, game screen, input handling.

#[cfg_attr(not(feature = "audio"), allow(dead_code, unused_imports))]
pub mod audio;
mod draw;
pub mod fx;
pub mod handbook;
pub mod hints;
pub mod keys;
pub mod palette;
pub mod prefs;
pub mod records;
pub mod settings;
pub mod tutorial;
pub mod view;
pub mod widgets;
#[cfg(feature = "window")]
pub mod window;

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use keys::{Act, Group};
use palette::{Palette, PaletteKind};
use ratatui::{
    crossterm::event::{
        self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    },
    layout::Rect,
};

use crate::{
    balance as B,
    map::MapSize,
    rng::parse_seed,
    save,
    types::{CargoMask, Pos},
    world::{
        Action, AlertLevel, Cond, Fanfare, GameMode, Settings, SiteKind, Stop, Target, TrainKind,
        World,
        build::{self, LayerKind},
        money, rail,
        vehicles::VKind,
    },
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    Station,
    Signal,
    Hub,
    Demolish,
}

impl Place {
    pub fn name(self) -> &'static str {
        match self {
            Place::Station => "station",
            Place::Signal => "signal",
            Place::Hub => "transfer hub",
            Place::Demolish => "demolish",
        }
    }

    /// The key that picks this tool (pressing it again also places).
    pub fn act(self) -> Act {
        match self {
            Place::Station => Act::Station,
            Place::Signal => Act::Signal,
            Place::Hub => Act::Hub,
            Place::Demolish => Act::Demolish,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Confirm {
    Quit,
    Scrap(u32),
    NewGame,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    Normal,
    Line {
        kind: LayerKind,
        start: Option<Pos>,
    },
    /// Double-track tool: two parallel one-way lines.
    Double {
        start: Option<Pos>,
    },
    Place(Place),
    Schedule {
        vehicle: u32,
        sel: usize,
        picking: bool,
    },
    Confirm(Confirm),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Overlay {
    None,
    Congestion,
    Flow,
    Towns,
}

impl Overlay {
    fn next(self) -> Overlay {
        match self {
            Overlay::None => Overlay::Congestion,
            Overlay::Congestion => Overlay::Flow,
            Overlay::Flow => Overlay::Towns,
            Overlay::Towns => Overlay::None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Overlay::None => "none",
            Overlay::Congestion => "CONGESTION",
            Overlay::Flow => "FLOW",
            Overlay::Towns => "TOWNS",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Alerts,
    Build,
    Vehicles,
    Contracts,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Title,
    Game,
}

pub struct ControlsState {
    /// Index into `keys::ACTS`.
    pub sel: usize,
    /// Waiting for the key to bind: Some(true) adds it, Some(false) replaces.
    pub capture: Option<bool>,
    pub dirty: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TitleRow {
    Continue,
    Play,
    /// The guided first game (its own row so nobody misses it).
    Tutorial,
    Mode,
    Size,
    Seed,
    Handbook,
    Settings,
    Controls,
    Quit,
}

pub struct TitleState {
    pub seed: String,
    pub mode: GameMode,
    /// Start the guided first game (set while the Tutorial row starts one).
    pub tutorial: bool,
    pub size: MapSize,
    /// The highlighted menu row.
    pub row: TitleRow,
}

/// Something on screen the mouse can click, recorded while drawing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Tab(Tab),
    Title(TitleRow),
    /// One of the title screen's choices, clicked directly.
    TitleMode(GameMode),
    TitleSize(MapSize),
    TitleDaily,
    MenuRow(usize),
    /// A Settings row; its ◀ / ▶ (true = up); a volume bar (and where it is).
    Setting(usize),
    SettingStep(usize, bool),
    SettingBar(usize, Rect),
    ZoomChoice(view::MapZoom),
    /// The Yes and No buttons of a question.
    Yes,
    No,
    Chapter(usize),
    /// The overview map; one cell covers this many tiles.
    Overview(f64),
    /// The side panel (the wheel scrolls it when it overflows).
    Side,
    /// A row of the Build tab: clicking it does what its key does.
    Act(Act),
    /// A row of the keys panel: clicking presses the key (the tool stays out).
    Key(Act),
    /// A row of the Vehicles tab: clicking it selects the vehicle.
    Vehicle(u32),
}

/// A "+£120" floating up from a vehicle that just delivered.
pub struct Pop {
    pub id: u32,
    pub pos: Pos,
    pub amount: f64,
    pub born: u64,
}

/// How long a pop floats, in frames (about two seconds).
pub const POP_FRAMES: u64 = 60;

pub struct App {
    pub screen: Screen,
    pub world: Option<World>,
    pub title: TitleState,
    pub cam: Pos,
    pub cursor: Pos,
    pub speed: usize,
    pub last_speed: usize,
    pub mode: Mode,
    pub overlay: Overlay,
    pub show_help: bool,
    pub show_stats: bool,
    /// The whole-map overview is open.
    pub show_overview: bool,
    pub help_scroll: u16,
    /// The handbook chapter on show.
    pub help_chapter: usize,
    pub pal: Palette,
    pub selected: Option<u32>,
    pub tab: Tab,
    pub msg: Option<(String, AlertLevel)>,
    pub msg_frame: u64,
    pub alert_jump: usize,
    pub save_path: PathBuf,
    pub labels: bool,
    /// Faint marks on forests, hills, mountains and water (`T` toggles).
    pub terrain_marks: bool,
    pub quit: bool,
    pub acc: f64,
    pub map_area: Rect,
    pub frame: u64,
    /// The player closed the end screen to look at their network.
    pub end_dismissed: bool,
    /// The guided first game, when the player chose Tutorial.
    pub tutorial: Option<tutorial::Tutorial>,
    pub audio: audio::Audio,
    pub keys: keys::Keymap,
    /// Where the Controls screen saves key bindings.
    pub keys_path: PathBuf,
    /// The Controls screen, when open.
    pub controls: Option<ControlsState>,
    /// What the saved game is, for the title screen's Continue row (read once per visit).
    pub save_info: Option<String>,
    /// Where saves, settings and crash logs go (`platform::data_dir`).
    pub data_dir: PathBuf,
    /// Where preferences and the autosave go (None in tests: nothing is written).
    pub prefs_path: Option<PathBuf>,
    pub autosave_path: Option<PathBuf>,
    /// The player has started a game before (first-timers land on Tutorial).
    pub played: bool,
    /// Game time of the last autosave, and the frame it happened (for the status bar).
    last_autosave: f64,
    pub autosaved_frame: Option<u64>,
    /// The game menu (Esc), with its highlighted row.
    pub menu: Option<usize>,
    /// Floating "+£" over vehicles that just got paid.
    pub pops: Vec<Pop>,
    /// Fireworks, steam and headlines (`fx`).
    pub fx: fx::Fx,
    /// This Charter's result has gone into the records (`records`).
    pub recorded: bool,
    /// What the records said when the Charter ended (for the end screen).
    pub record_note: Option<records::Outcome>,
    /// Personal bests and medals (`records.json`; loaded in `main`).
    pub records: records::Records,
    seen_income: std::collections::HashMap<u32, f64>,
    /// Clickable areas from the last frame drawn (the renderer only has `&App`).
    pub hits: std::cell::RefCell<Vec<(Rect, Hit)>>,
    /// Zoomed-in map tiles from the last frame: each block and the cells the map
    /// drew in it. The window draws a block whose cells nothing covered since
    /// as one big glyph, so tiles fill their square (a terminal can't).
    pub map_blocks: std::cell::RefCell<Vec<view::MapBlock>>,
    /// The mouse put the cursor where it is: the camera shouldn't chase it.
    pub mouse_cursor: bool,
    /// Mouse support on (the terminal hands us clicks). Off gives the mouse back to the terminal.
    pub mouse: bool,
    /// Where the pointer was last seen (for edge scrolling).
    pub mouse_pos: Option<(u16, u16)>,
    edge_frame: u64,
    /// The map grabbed with the right or middle button, to drag it around.
    grab: Option<Grab>,
    /// A real terminal is attached (`ui::run`), so mouse capture can be switched.
    terminal_live: bool,
    /// The front end tells us when keys are let go (the window does; most
    /// terminals don't). Only then can holding a key mean something.
    pub releases: bool,
    /// The demolish key is held down: moving the cursor bulldozes.
    pub dozer_held: bool,
    /// How far the side panel is scrolled, and what it was showing then
    /// (cursor tile, selection): looking at something else starts at the top.
    pub side_scroll: u16,
    pub side_for: (Pos, Option<u32>),
    /// How far it could scroll when last drawn.
    pub side_max: u16,
    last_dozer_sound: u64,
    /// Text size in the game's window (logical px; 0 = the default), and fullscreen.
    pub font_px: f32,
    pub fullscreen: bool,
    /// Running in the game's own window (which can change the text size).
    pub windowed: bool,
    /// A text-size change for the window to make: +1 bigger, -1 smaller, 0 normal.
    pub zoom: Option<i32>,
    /// Settings asked the window to switch fullscreen.
    pub toggle_fullscreen: bool,
    /// How big the map is drawn (separate from the text size; `view`).
    pub map_zoom: view::MapZoom,
    /// Put the cursor in the middle of the view on the next frame (a jump, a new game).
    pub center_view: bool,
    /// The keys panel left of the map is on.
    pub key_panel: bool,
    /// The Settings screen, when open.
    pub settings: Option<settings::SettingsState>,
    /// What the sound cues have already reacted to: alerts raised, revenue earned, and when.
    heard_alerts: u64,
    heard_revenue: f64,
    last_coin: u64,
    last_alert_sound: u64,
}

impl App {
    pub fn new(seed: Option<String>, settings: Settings, save_path: PathBuf) -> App {
        let default_seed = seed.unwrap_or_else(|| {
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            format!("{}", (t % 1_000_000) as u64)
        });
        App {
            screen: Screen::Title,
            world: None,
            title: TitleState {
                seed: default_seed,
                mode: settings.mode,
                tutorial: false,
                size: settings.size,
                row: TitleRow::Play,
            },
            cam: Pos::new(0, 0),
            cursor: Pos::new(0, 0),
            speed: 1,
            last_speed: 1,
            mode: Mode::Normal,
            overlay: Overlay::None,
            show_help: false,
            show_stats: false,
            show_overview: false,
            help_scroll: 0,
            help_chapter: 0,
            pal: Palette::new(PaletteKind::Classic),
            selected: None,
            tab: Tab::Alerts,
            msg: None,
            msg_frame: 0,
            alert_jump: 0,
            save_path,
            labels: true,
            terrain_marks: true,
            quit: false,
            acc: 0.0,
            map_area: Rect::default(),
            frame: 0,
            end_dismissed: false,
            tutorial: None,
            audio: audio::Audio::silent(),
            keys: keys::Keymap::default(),
            keys_path: PathBuf::from("railhead_keys.json"),
            controls: None,
            save_info: None,
            data_dir: std::env::temp_dir(),
            prefs_path: None,
            autosave_path: None,
            played: false,
            last_autosave: 0.0,
            autosaved_frame: None,
            menu: None,
            pops: vec![],
            fx: fx::Fx::default(),
            recorded: false,
            record_note: None,
            records: records::Records::default(),
            seen_income: std::collections::HashMap::new(),
            hits: std::cell::RefCell::new(vec![]),
            map_blocks: std::cell::RefCell::new(vec![]),
            mouse_cursor: false,
            mouse: true,
            mouse_pos: None,
            edge_frame: u64::MAX,
            grab: None,
            terminal_live: false,
            releases: false,
            dozer_held: false,
            side_scroll: 0,
            side_for: (Pos::new(0, 0), None),
            side_max: 0,
            last_dozer_sound: 0,
            font_px: 0.0,
            fullscreen: false,
            windowed: false,
            zoom: None,
            toggle_fullscreen: false,
            map_zoom: view::MapZoom::Normal,
            key_panel: true,
            center_view: false,
            settings: None,
            heard_alerts: 0,
            heard_revenue: 0.0,
            last_coin: 0,
            last_alert_sound: 0,
        }
    }

    /// Save to the autosave slot (not the tutorial, not a finished Charter).
    pub fn autosave(&mut self) {
        let (Some(path), Some(w)) = (self.autosave_path.as_ref(), self.world.as_ref()) else {
            return;
        };
        if self.tutorial.is_some() || w.finished {
            return;
        }
        self.last_autosave = w.time;
        if save::save(w, path).is_ok() {
            self.autosaved_frame = Some(self.frame);
        }
    }

    /// The newest of the manual save and the autosave: what Continue loads.
    pub fn continue_path(&self) -> Option<PathBuf> {
        let modified = |p: &PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
        [Some(self.save_path.clone()), self.autosave_path.clone()]
            .into_iter()
            .flatten()
            .filter_map(|p| modified(&p).map(|t| (t, p)))
            .max_by_key(|(t, _)| *t)
            .map(|(_, p)| p)
    }

    /// Re-read what Continue would load, for the title screen.
    pub fn refresh_save_info(&mut self) {
        self.save_info = self.continue_path().and_then(|p| save::peek(&p));
    }

    /// Is a screen open that the player is reading? Then the clock holds.
    pub fn reading(&self) -> bool {
        self.show_help
            || self.show_stats
            || self.menu.is_some()
            || self.controls.is_some()
            || self.settings.is_some()
    }

    /// Something covers the map, so the mouse doesn't work it.
    fn covered(&self) -> bool {
        self.reading() || self.show_overview || matches!(self.mode, Mode::Confirm(_))
    }

    /// The map on screen, as last laid out: camera, zoom and area.
    pub fn view(&self) -> view::View {
        view::View {
            zoom: self.map_zoom,
            cam: self.cam,
            area: self.map_area,
        }
    }

    /// "+£" pops over vehicles that were paid since the last frame. Payments
    /// a vehicle gets in quick succession (unloading takes a few ticks) add up
    /// into one pop.
    pub fn update_pops(&mut self) {
        let frame = self.frame;
        self.pops
            .retain(|p| frame.saturating_sub(p.born) < POP_FRAMES);
        let Some(w) = self.world.as_ref() else { return };
        let carriers = w
            .trains
            .iter()
            .map(|t| (&t.carrier, t.head()))
            .chain(w.trucks.iter().map(|t| (&t.carrier, t.pos)));
        for (c, pos) in carriers {
            let before = self
                .seen_income
                .insert(c.id, c.lifetime_income)
                .unwrap_or(c.lifetime_income);
            let gain = c.lifetime_income - before;
            if gain < 0.5 {
                continue;
            }
            match self
                .pops
                .iter_mut()
                .find(|p| p.id == c.id && frame.saturating_sub(p.born) < POP_FRAMES / 3)
            {
                Some(p) => {
                    p.amount += gain;
                    p.pos = pos;
                }
                None => self.pops.push(Pop {
                    id: c.id,
                    pos,
                    amount: gain,
                    born: frame,
                }),
            }
        }
    }

    /// Edge scrolling: while the pointer rests on the map's edge (or just
    /// past it), the view keeps sliding that way. The cursor slides with it,
    /// so it stays under the pointer.
    pub fn edge_scroll(&mut self) {
        // once per frame, however many times the loop turns
        if self.frame == self.edge_frame {
            return;
        }
        self.edge_frame = self.frame;
        let Some((x, y)) = self.mouse_pos else { return };
        // only while the mouse is driving: a key press hands the cursor back to the keyboard
        if !self.mouse
            || !self.mouse_cursor
            || self.grab.is_some()
            || self.covered()
            || self.screen != Screen::Game
            || self.world.is_none()
        {
            return;
        }
        let r = self.map_area;
        let (x, y) = (x as i32, y as i32);
        let (left, top, right, bottom) = (
            r.x as i32,
            r.y as i32,
            (r.x + r.width) as i32 - 1,
            (r.y + r.height) as i32 - 1,
        );
        // the band: the map's outer EDGE_BAND cells and the one cell beyond them
        if x < left - 1 || x > right + 1 || y < top - 1 || y > bottom + 1 {
            return;
        }
        let band = EDGE_BAND - 1;
        let dx = if x <= left + band {
            -EDGE_SPEED.0
        } else if x >= right - band {
            EDGE_SPEED.0
        } else {
            0
        };
        let dy = if y <= top + band {
            -EDGE_SPEED.1
        } else if y >= bottom - band {
            EDGE_SPEED.1
        } else {
            0
        };
        if dx == 0 && dy == 0 {
            return;
        }
        // Pan the view, then put the cursor back under the pointer.
        let w = self.world.as_ref().unwrap();
        let (vw, vh) = self.view().tiles();
        if w.map.w > vw {
            self.cam.x = (self.cam.x + dx).clamp(0, w.map.w - vw);
        }
        if w.map.h > vh {
            self.cam.y = (self.cam.y + dy).clamp(0, w.map.h - vh);
        }
        let p = self
            .view()
            .tile_at_offset(x.clamp(left, right) - left, y.clamp(top, bottom) - top);
        self.cursor = Pos::new(p.x.clamp(0, w.map.w - 1), p.y.clamp(0, w.map.h - 1));
        self.mouse_cursor = true;
    }

    /// Back to the title screen (the game in progress is dropped).
    pub fn go_title(&mut self) {
        self.screen = Screen::Title;
        self.world = None;
        self.tutorial = None;
        self.menu = None;
        self.settings = None;
        self.msg = None;
        self.show_help = false;
        self.show_stats = false;
        self.show_overview = false;
        self.mode = Mode::Normal;
        self.selected = None;
        self.pops.clear();
        self.fx = fx::Fx::default();
        self.refresh_save_info();
    }

    /// When a Charter ends (or a Franchise is lost): put the result in the records (once) and keep
    /// what they said for the end screen. Tests have no data folder, so the
    /// records only change in memory there.
    pub fn record_result(&mut self) {
        let Some(w) = self.world.as_ref() else { return };
        if self.recorded || !w.finished || w.sandbox() || self.tutorial.is_some() {
            return;
        }
        self.recorded = true;
        let outcome = if w.franchise() {
            self.records
                .add_franchise(w.map.seed, w.settings.size, w.stats.reviews_passed)
        } else {
            self.records.add(
                w.map.seed,
                w.settings.size,
                w.score().round(),
                w.stats.medal,
            )
        };
        if self.prefs_path.is_some() {
            self.records.save(&self.data_dir.join("records.json"));
        }
        self.record_note = Some(outcome);
    }

    /// Advance the tutorial when the furthest step's check passes. A player
    /// who paged back to reread a step stays there; the guide moves on only
    /// when they were looking at the step that just passed.
    pub fn update_tutorial(&mut self) {
        let (Some(tut), Some(w)) = (self.tutorial.as_ref(), self.world.as_ref()) else {
            return;
        };
        let Some(step) = tutorial::STEPS.get(tut.reached) else {
            return;
        };
        let Some(done) = step.done else { return };
        if done(self, w, tut) {
            let frame = self.frame;
            let tut = self.tutorial.as_mut().unwrap();
            let following = !tut.reviewing();
            tut.reached += 1;
            let next = tutorial::STEPS
                .get(tut.reached)
                .map(|s| s.title)
                .unwrap_or_default();
            if following {
                tut.step = tut.reached;
                tut.passed_frame = Some(frame);
                self.flash(format!("✔ {}! Next: {next}", step.title), AlertLevel::Good);
            } else {
                self.flash(
                    self.keys.fill(&format!(
                        "✔ {}! ({{tutorial_skip}} pages on to: {next})",
                        step.title
                    )),
                    AlertLevel::Good,
                );
            }
            self.audio.play(audio::Sfx::Fanfare);
        }
    }

    pub fn start_game(&mut self, world: World) {
        self.end_dismissed = false;
        self.tutorial = None;
        self.last_autosave = world.time;
        self.seen_income = world
            .trains
            .iter()
            .map(|t| &t.carrier)
            .chain(world.trucks.iter().map(|t| &t.carrier))
            .map(|c| (c.id, c.lifetime_income))
            .collect();
        self.pops.clear();
        self.menu = None;
        self.played = true;
        prefs::save(self);
        self.heard_alerts = world.alert_count;
        self.heard_revenue = world.stats.revenue;
        self.fx = fx::Fx::new(&world);
        self.recorded = world.finished;
        self.record_note = None;
        let cap = world.towns[world.map.home].pos;
        self.world = Some(world);
        self.screen = Screen::Game;
        self.cursor = cap;
        self.center_view = true;
        self.mode = Mode::Normal;
        self.selected = None;
        self.speed = 0; // start paused so the player can look around
        self.last_speed = 1;
        let msg = self.keys.fill("Paused. Look around, then press {pause} to start the clock. {help} opens the Railway Handbook, {deselect} the menu.");
        self.flash(msg, AlertLevel::Info);
    }

    /// Sounds for what the world did since the last frame: new alerts (by
    /// level) and delivery money, each throttled so busy networks don't rattle.
    pub fn sound_cues(&mut self) {
        let Some(w) = self.world.as_ref() else { return };
        let fps = 1000.0 / FRAME_MS as f64;
        if w.alert_count > self.heard_alerts {
            let new = ((w.alert_count - self.heard_alerts) as usize).min(w.alerts.len());
            let worst = w.alerts[w.alerts.len() - new..]
                .iter()
                .map(|a| a.level)
                .max_by_key(|l| match l {
                    AlertLevel::Info => 0,
                    AlertLevel::Good => 1,
                    AlertLevel::Warn => 2,
                    AlertLevel::Danger => 3,
                });
            // big moments have their own sounds
            let fanfare = w.alerts[w.alerts.len() - new..]
                .iter()
                .filter_map(|a| a.fanfare)
                .max_by_key(|f| match f {
                    Fanfare::Medal => 3,
                    Fanfare::Milestone => 2,
                    Fanfare::News => 1,
                    _ => 0,
                });
            self.heard_alerts = w.alert_count;
            let cue = match (fanfare, worst) {
                (Some(Fanfare::Medal), _) => Some(audio::Sfx::Medal),
                (Some(Fanfare::Milestone), _) => Some(audio::Sfx::Fanfare),
                (Some(Fanfare::News), Some(AlertLevel::Good | AlertLevel::Info) | None) => {
                    Some(audio::Sfx::News)
                }
                (_, Some(AlertLevel::Good)) => Some(audio::Sfx::Good),
                (_, Some(AlertLevel::Warn)) => Some(audio::Sfx::Warn),
                (_, Some(AlertLevel::Danger)) => Some(audio::Sfx::Danger),
                _ => None,
            };
            if let Some(cue) = cue
                && self.frame.saturating_sub(self.last_alert_sound) as f64
                    >= B::ALERT_SOUND_GAP * fps
            {
                self.audio
                    .play(if w.finished { audio::Sfx::Fanfare } else { cue });
                self.last_alert_sound = self.frame;
            }
        }
        if w.stats.revenue > self.heard_revenue + 0.5
            && self.frame.saturating_sub(self.last_coin) as f64 >= B::COIN_SOUND_GAP * fps
        {
            self.heard_revenue = w.stats.revenue;
            self.last_coin = self.frame;
            self.audio.play(audio::Sfx::Coin);
        }
    }

    pub fn flash(&mut self, s: impl Into<String>, level: AlertLevel) {
        self.msg = Some((s.into(), level));
        self.msg_frame = self.frame;
    }

    fn result(&mut self, r: Result<String, String>) {
        match r {
            Ok(s) => {
                self.flash(s, AlertLevel::Good);
                self.audio.play(audio::Sfx::Click);
            }
            Err(e) => {
                self.flash(e, AlertLevel::Warn);
                self.audio.play(audio::Sfx::Error);
            }
        }
    }
}

/// The window's normal text size in logical pixels (`App::font_px` 0 means this).
pub const TEXT_DEFAULT: f32 = 16.0;

/// How long one UI frame waits for a key (about 30 frames a second).
const FRAME_MS: u64 = 33;
/// Edge scrolling: how many cells from the map's edge start it, and how many
/// tiles (columns, rows) the view slides per frame. Rows are about twice as
/// tall as columns are wide, so they move half as many.
const EDGE_BAND: i32 = 2;
const EDGE_SPEED: (i32, i32) = (2, 1);

pub fn run(mut app: App) -> std::io::Result<()> {
    app.audio = audio::Audio::open(app.audio.levels);
    let mut terminal = ratatui::init();
    app.terminal_live = true;
    // a terminal may never say a key was let go: wait until it does (in case the window gave up)
    app.releases = false;
    app.dozer_held = false;
    apply_mouse(&app);
    crate::platform::setup_window();
    crate::platform::install_crash_handler(app.data_dir.clone(), false);
    let mut clock = Clock::new();
    let result = (|| -> std::io::Result<()> {
        while !app.quit {
            terminal.draw(|f| draw::draw(f, &mut app))?;
            let timeout = Duration::from_millis(FRAME_MS);
            if event::poll(timeout)? {
                // take everything waiting (a fast mouse sends dozens of moves), then draw once
                loop {
                    match event::read()? {
                        // fit the new size at once, and repaint everything (no stale cells)
                        Event::Resize(_, _) => {
                            terminal.autoresize()?;
                            terminal.clear()?;
                        }
                        ev => handle_event(&mut app, ev),
                    }
                    if !event::poll(Duration::ZERO)? {
                        break;
                    }
                }
            }
            step(&mut app, &mut clock);
        }
        Ok(())
    })();
    let _ = ratatui::crossterm::execute!(
        std::io::stdout(),
        ratatui::crossterm::event::DisableMouseCapture
    );
    ratatui::restore();
    result
}

/// Real time for the game loop: when it started, and when the sim last ran.
pub(crate) struct Clock {
    start: Instant,
    last: Instant,
}

impl Clock {
    pub(crate) fn new() -> Clock {
        let now = Instant::now();
        Clock {
            start: now,
            last: now,
        }
    }
}

/// One input event, from the terminal or the game's window.
pub(crate) fn handle_event(app: &mut App, ev: Event) {
    match ev {
        Event::Key(k) if k.kind == KeyEventKind::Release => {
            app.releases = true;
            if app
                .keys
                .keys(Act::Demolish)
                .iter()
                .any(|b| same_key(b.code, k.code))
            {
                app.dozer_held = false;
            }
        }
        Event::Key(k) => {
            if k.kind == KeyEventKind::Press && app.releases && app.keys.is(Act::Demolish, &k) {
                app.dozer_held = true;
            }
            handle_key(app, k)
        }
        Event::Mouse(m) if app.mouse => handle_mouse(app, m),
        _ => {}
    }
}

/// Everything that happens once per loop turn after input: animations,
/// sound, autosave and running the sim for the real time that passed.
pub(crate) fn step(app: &mut App, clock: &mut Clock) {
    // Frames count real time, not loop turns, so blinking, pops and
    // edge scrolling run at the same speed however busy the input is.
    app.frame = (clock.start.elapsed().as_millis() / FRAME_MS as u128) as u64;
    app.update_tutorial();
    app.sound_cues();
    app.update_pops();
    fx::update(app);
    app.record_result();
    app.edge_scroll();
    let mood = match app.screen {
        Screen::Title => audio::Mood::Title,
        Screen::Game if app.speed == 0 || app.reading() => audio::Mood::Paused,
        Screen::Game => audio::Mood::Running,
    };
    app.audio.mood(mood);
    let now = Instant::now();
    let real = now.duration_since(clock.last).as_secs_f64();
    clock.last = now;
    if app.screen == Screen::Game && app.mode != Mode::Confirm(Confirm::Quit) && !app.reading() {
        if app
            .world
            .as_ref()
            .is_some_and(|w| w.time - app.last_autosave >= B::AUTOSAVE_SECS)
        {
            app.autosave();
        }
        if let Some(w) = app.world.as_mut() {
            app.acc += real * B::SPEEDS[app.speed] * B::TICKS_PER_SEC as f64;
            let mut n = 0;
            while app.acc >= 1.0 && n < 400 {
                w.tick();
                app.acc -= 1.0;
                n += 1;
            }
            if n == 400 {
                app.acc = 0.0;
            }
        }
    }
}

/// Ask the terminal to send us mouse events, or give the mouse back to it.
fn apply_mouse(app: &App) {
    use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
    if !app.terminal_live {
        return;
    }
    let _ = if app.mouse {
        ratatui::crossterm::execute!(std::io::stdout(), EnableMouseCapture)
    } else {
        ratatui::crossterm::execute!(std::io::stdout(), DisableMouseCapture)
    };
}

/// Press whatever key the player bound to `act`, as if they had typed it.
fn press(app: &mut App, act: Act) {
    if let Some(k) = app.keys.keys(act).first().copied() {
        handle_key(app, k.event());
    }
}

/// The map held by a mouse button: where the pointer and the camera were
/// when it was grabbed, and whether it has moved since (a right click that
/// didn't drag still backs out).
#[derive(Clone, Copy, Debug)]
struct Grab {
    button: MouseButton,
    from: (u16, u16),
    cam: Pos,
    moved: bool,
}

/// Drag the grabbed map so the tile that was grabbed stays under the pointer.
fn drag_map(app: &mut App, x: u16, y: u16) {
    let Some(g) = app.grab.as_mut() else { return };
    let (dx, dy) = (x as i32 - g.from.0 as i32, y as i32 - g.from.1 as i32);
    if dx == 0 && dy == 0 && !g.moved {
        return;
    }
    g.moved = true;
    let g = *g;
    let Some(w) = app.world.as_ref() else { return };
    let z = app.map_zoom;
    let (vw, vh) = app.view().tiles();
    let (mut cx, mut cy) = (g.cam.x - dx / z.cols(), g.cam.y - dy * z.stack() / z.rows());
    cx = if w.map.w > vw {
        cx.clamp(0, w.map.w - vw)
    } else {
        app.cam.x
    };
    cy = if w.map.h > vh {
        cy.clamp(0, w.map.h - vh)
    } else {
        app.cam.y
    };
    app.cam = Pos::new(cx, cy);
    let a = app.map_area;
    let p = app.view().tile_at_offset(
        (x as i32 - a.x as i32).clamp(0, a.width as i32 - 1),
        (y as i32 - a.y as i32).clamp(0, a.height as i32 - 1),
    );
    app.cursor = Pos::new(p.x.clamp(0, w.map.w - 1), p.y.clamp(0, w.map.h - 1));
    app.mouse_cursor = true;
}

/// Right click: close what's open, or put down the tool in hand.
fn back_out(app: &mut App) {
    if app.show_help {
        app.show_help = false;
    } else if app.settings.is_some() {
        app.settings = None;
    } else if app.menu.is_some() {
        app.menu = None;
    } else if app.show_overview {
        app.show_overview = false;
    } else if app.show_stats {
        app.show_stats = false;
    } else if app.screen == Screen::Game {
        match app.mode {
            Mode::Normal => app.selected = None,
            Mode::Confirm(_) => press(app, Act::Cancel), // any key other than yes says no
            Mode::Schedule { picking: false, .. } => press(app, Act::SchedClose),
            _ => press(app, Act::Cancel),
        }
    }
}

/// The map tile under a screen cell, if the map is showing there.
fn tile_at(app: &App, x: u16, y: u16) -> Option<Pos> {
    let w = app.world.as_ref()?;
    if app.screen != Screen::Game {
        return None;
    }
    let p = app.view().tile_at(x, y)?;
    w.map.in_bounds(p).then_some(p)
}

/// Mouse: hover moves the cursor (and highlights menu rows), left click
/// does what Enter would there, right click backs out, right or middle drag
/// pans the map, the wheel zooms it; tabs, menus, choices and the handbook's chapters are clickable. It
/// works by pressing the player's own bound keys, so the mouse can never do
/// anything the keyboard can't.
fn handle_mouse(app: &mut App, m: MouseEvent) {
    let (x, y) = (m.column, m.row);
    app.mouse_pos = Some((x, y));
    let hit = app
        .hits
        .borrow()
        .iter()
        .rev()
        .find(|(r, _)| r.contains(ratatui::layout::Position { x, y }))
        .map(|(_, h)| *h);
    // a full-screen panel is open over the map: the map doesn't take clicks
    let covered = app.covered();
    // the Controls screen is keyboard-only (it is all about keys)
    if app.controls.is_some() {
        return;
    }
    match m.kind {
        MouseEventKind::Moved | MouseEventKind::Drag(MouseButton::Left) => {
            // hovering a menu row highlights it, as a mouse user expects
            match hit {
                Some(Hit::Setting(i)) => app.settings.as_mut().unwrap().sel = i,
                Some(Hit::MenuRow(i)) if app.settings.is_none() && !app.show_help => {
                    app.menu = Some(i)
                }
                Some(Hit::Title(row)) if app.settings.is_none() && !app.show_help => {
                    app.title.row = row
                }
                _ => {}
            }
            if covered {
                return;
            }
            if let Some(p) = tile_at(app, x, y) {
                // dragging with the demolish tool (or holding its key) clears the way
                let dozing = app.mode == Mode::Place(Place::Demolish)
                    && (app.dozer_held || matches!(m.kind, MouseEventKind::Drag(_)));
                if dozing {
                    bulldoze_to(app, p);
                }
                app.cursor = p;
                app.mouse_cursor = true;
            }
        }
        MouseEventKind::Down(MouseButton::Left) if app.settings.is_some() && !app.show_help => {
            if let Some(h) = hit {
                settings::click(app, h, x);
            }
        }
        MouseEventKind::Down(MouseButton::Left) => match hit {
            Some(Hit::Tab(t)) if !covered => app.tab = t,
            Some(Hit::Act(a)) if !covered => {
                // a fresh start, so clicking "rail" with the rail tool out doesn't build
                app.mode = Mode::Normal;
                press(app, a);
            }
            Some(Hit::Key(a)) if !app.reading() => press(app, a),
            Some(Hit::Yes) => press(app, Act::Yes),
            Some(Hit::No) => press(app, Act::Cancel),
            Some(Hit::Vehicle(id)) if !covered => {
                app.selected = Some(id);
                if let Some(p) = app.world.as_ref().and_then(|w| w.vehicle_pos(id)) {
                    app.cursor = p;
                    app.mouse_cursor = false;
                }
            }
            Some(Hit::Title(row)) if !app.show_help => {
                app.title.row = row;
                match row {
                    TitleRow::Mode | TitleRow::Size => change_option(&mut app.title, true),
                    TitleRow::Seed => {} // typing goes there; the row just shows it
                    _ => press(app, Act::TitleStart),
                }
            }
            Some(Hit::TitleMode(mode)) if !app.show_help => {
                (app.title.row, app.title.mode) = (TitleRow::Mode, mode)
            }
            Some(Hit::TitleSize(size)) if !app.show_help => {
                (app.title.row, app.title.size) = (TitleRow::Size, size)
            }
            Some(Hit::TitleDaily) if !app.show_help => {
                app.title.row = TitleRow::Seed;
                app.title.seed = daily_seed();
            }
            Some(Hit::MenuRow(i)) if !app.show_help => {
                app.menu = Some(i);
                press(app, Act::MenuPick);
            }
            Some(Hit::Chapter(i)) if app.show_help => {
                app.help_chapter = i;
                app.help_scroll = 0;
            }
            Some(Hit::Overview(s)) if app.show_overview => {
                let r = app
                    .hits
                    .borrow()
                    .iter()
                    .find(|(_, h)| matches!(h, Hit::Overview(_)))
                    .map(|(r, _)| *r)
                    .unwrap();
                if let Some(w) = app.world.as_ref() {
                    let px = (((x - r.x) as f64 + 0.5) * s) as i32;
                    let py = (((y - r.y) as f64 + 0.5) * s) as i32;
                    app.cursor = Pos::new(px.clamp(0, w.map.w - 1), py.clamp(0, w.map.h - 1));
                    app.mouse_cursor = false;
                }
                app.show_overview = false;
            }
            _ if app.show_stats => app.show_stats = false,
            _ if covered => {}
            _ => {
                if let Some(p) = tile_at(app, x, y) {
                    app.cursor = p;
                    app.mouse_cursor = true;
                    let act = match app.mode {
                        Mode::Normal => Act::Select,
                        Mode::Schedule { picking: false, .. } => return,
                        _ => Act::Confirm,
                    };
                    press(app, act);
                    app.mouse_cursor = true;
                }
            }
        },
        // right or middle button on the map grabs it; let go without moving
        // and a right click backs out as usual
        MouseEventKind::Down(b @ (MouseButton::Right | MouseButton::Middle))
            if !covered && tile_at(app, x, y).is_some() =>
        {
            app.grab = Some(Grab {
                button: b,
                from: (x, y),
                cam: app.cam,
                moved: false,
            });
        }
        MouseEventKind::Down(MouseButton::Right) => back_out(app),
        MouseEventKind::Drag(b) if app.grab.is_some_and(|g| g.button == b) => drag_map(app, x, y),
        MouseEventKind::Up(b) if app.grab.is_some_and(|g| g.button == b) => {
            let g = app.grab.take().unwrap();
            if b == MouseButton::Right && !g.moved {
                back_out(app);
            }
        }
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            let down = m.kind == MouseEventKind::ScrollDown;
            if app.show_help {
                app.help_scroll = if down {
                    app.help_scroll.saturating_add(3).min(400)
                } else {
                    app.help_scroll.saturating_sub(3)
                };
            } else if hit == Some(Hit::Side) && !covered {
                // draw_side keeps it from scrolling past the end
                app.side_scroll = if down {
                    app.side_scroll.saturating_add(2)
                } else {
                    app.side_scroll.saturating_sub(2)
                };
            } else if app.screen == Screen::Game && !covered {
                if m.modifiers.contains(KeyModifiers::SHIFT) {
                    // Shift + wheel scrolls, like it always did
                    app.mouse_cursor = false;
                    move_cursor(app, 0, if down { 3 } else { -3 });
                } else if let Some(p) = tile_at(app, x, y) {
                    // the wheel zooms the map around the tile under the pointer
                    app.cursor = p;
                    let z = if down {
                        app.map_zoom.farther()
                    } else {
                        app.map_zoom.closer()
                    };
                    if let Some(z) = z {
                        settings::set_map_zoom(app, z);
                    }
                    app.mouse_cursor = true;
                }
            }
        }
        _ => {}
    }
}

fn handle_key(app: &mut App, k: KeyEvent) {
    app.mouse_cursor = false;
    // text size works on every screen (but a key being bound on the Controls screen is just a key)
    if !app.controls.as_ref().is_some_and(|c| c.capture.is_some()) {
        let zoom = [(Act::ZoomIn, 1), (Act::ZoomOut, -1), (Act::ZoomReset, 0)]
            .into_iter()
            .find(|(a, _)| app.keys.is(*a, &k));
        if let Some((_, z)) = zoom {
            request_zoom(app, z);
            return;
        }
    }
    if app.controls.is_some() {
        controls_key(app, k);
        return;
    }
    if app.settings.is_some() && !app.show_help {
        settings::key(app, &k);
        return;
    }
    match app.screen {
        Screen::Title => title_key(app, k),
        Screen::Game => game_key(app, k),
    }
}

/// Ask the window to change the text size (it knows the screen, so it
/// picks the step and keeps the game's 80x24 cells on screen).
fn request_zoom(app: &mut App, z: i32) {
    if app.windowed {
        app.zoom = Some(z);
    } else {
        app.flash("In a terminal the text size is the terminal's: try its own zoom (often Ctrl + and Ctrl -).", AlertLevel::Info);
    }
}

/// Open the Railway Handbook on the chapter that fits what the player is doing.
pub fn open_handbook(app: &mut App) {
    app.help_chapter = handbook::chapter_for(app);
    app.help_scroll = 0;
    app.show_help = true;
}

fn handbook_key(app: &mut App, k: KeyEvent) {
    let km = &app.keys;
    let n = handbook::chapter_count();
    if km.is(Act::HelpUp, &k) {
        app.help_scroll = app.help_scroll.saturating_sub(1);
    } else if km.is(Act::HelpDown, &k) {
        app.help_scroll = app.help_scroll.saturating_add(1).min(400);
    } else if km.is(Act::HelpPageUp, &k) {
        app.help_scroll = app.help_scroll.saturating_sub(10);
    } else if km.is(Act::HelpPageDown, &k) {
        app.help_scroll = app.help_scroll.saturating_add(10).min(400);
    } else if km.is(Act::HelpPrevChapter, &k) {
        app.help_chapter = (app.help_chapter + n - 1) % n;
        app.help_scroll = 0;
    } else if km.is(Act::HelpNextChapter, &k) {
        app.help_chapter = (app.help_chapter + 1) % n;
        app.help_scroll = 0;
    } else {
        app.show_help = false;
    }
}

fn open_controls(app: &mut App) {
    app.controls = Some(ControlsState {
        sel: 0,
        capture: None,
        dirty: false,
    });
}

/// The Controls screen. Its own keys are fixed (arrows or j/k, Enter, Tab,
/// Backspace, Ctrl+R, Esc) so nobody can lock themselves out.
fn controls_key(app: &mut App, k: KeyEvent) {
    let Some(st) = app.controls.as_mut() else {
        return;
    };
    let act = keys::ACTS[st.sel].act;
    if let Some(add) = st.capture {
        st.capture = None;
        if k.code == KeyCode::Esc {
            app.flash("Not changed.", AlertLevel::Info);
            return;
        }
        let key = keys::Key::from_event(&k);
        st.dirty = true;
        let orphans = app.keys.bind(act, key, add);
        let mut msg = format!("{}: {}", act.info().desc, app.keys.labels(act));
        let level = if orphans.is_empty() {
            AlertLevel::Good
        } else {
            let names: Vec<&str> = orphans.iter().map(|a| a.info().desc).collect();
            msg.push_str(&format!(
                ". {} took that key from \"{}\", which now has none!",
                key.label(),
                names.join("\", \"")
            ));
            AlertLevel::Warn
        };
        app.flash(msg, level);
        return;
    }
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    match k.code {
        KeyCode::Up | KeyCode::Char('k') => st.sel = st.sel.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => st.sel = (st.sel + 1).min(keys::ACTS.len() - 1),
        KeyCode::PageUp | KeyCode::Char('K') => st.sel = st.sel.saturating_sub(10),
        KeyCode::PageDown | KeyCode::Char('J') => st.sel = (st.sel + 10).min(keys::ACTS.len() - 1),
        KeyCode::Enter => st.capture = Some(false),
        KeyCode::Tab => st.capture = Some(true),
        KeyCode::Backspace | KeyCode::Delete => {
            app.keys.reset(act);
            st.dirty = true;
            let msg = format!("{}: back to {}", act.info().desc, app.keys.labels(act));
            app.flash(msg, AlertLevel::Info);
        }
        KeyCode::Char('r') if ctrl => {
            app.keys = keys::Keymap::default();
            st.dirty = true;
            app.flash("Every key is back to its default.", AlertLevel::Info);
        }
        KeyCode::Esc => {
            let dirty = st.dirty;
            app.controls = None;
            if dirty {
                match app.keys.save(&app.keys_path) {
                    Ok(()) => app.flash(
                        format!("Controls saved to {}.", app.keys_path.display()),
                        AlertLevel::Good,
                    ),
                    Err(e) => app.flash(format!("Couldn't save controls: {e}"), AlertLevel::Warn),
                }
            }
        }
        _ => {}
    }
}

/// The rows of the title menu, top to bottom. Continue only shows when there is a save.
pub fn title_rows(app: &App) -> Vec<TitleRow> {
    let mut rows = vec![];
    if app.save_info.is_some() {
        rows.push(TitleRow::Continue);
    }
    rows.extend([
        TitleRow::Play,
        TitleRow::Tutorial,
        TitleRow::Mode,
        TitleRow::Size,
        TitleRow::Seed,
        TitleRow::Handbook,
        TitleRow::Settings,
        TitleRow::Controls,
        TitleRow::Quit,
    ]);
    rows
}

fn change_option(t: &mut TitleState, forward: bool) {
    match t.row {
        TitleRow::Size => {
            t.size = if forward {
                t.size.next()
            } else {
                t.size.prev()
            }
        }
        _ => {
            let all = GameMode::ALL;
            let i = all.iter().position(|m| *m == t.mode).unwrap_or(0);
            t.mode = all[if forward {
                (i + 1) % all.len()
            } else {
                (i + all.len() - 1) % all.len()
            }];
        }
    }
}

fn load_game(app: &mut App, path: Option<PathBuf>) {
    let Some(path) = path else {
        app.flash("No saved game yet.", AlertLevel::Info);
        return;
    };
    match save::load(&path) {
        Ok(w) => {
            app.start_game(w);
            app.flash(
                "Game loaded. Welcome back, stationmaster.",
                AlertLevel::Good,
            );
        }
        Err(e) => app.flash(format!("Load failed: {e}"), AlertLevel::Warn),
    }
}

fn new_game(app: &mut App) {
    let t = &app.title;
    if t.tutorial {
        let w = tutorial::world();
        let tut = tutorial::Tutorial::new(&w);
        app.start_game(w);
        app.tutorial = Some(tut);
    } else {
        let seed = if t.seed.trim().is_empty() {
            "1".to_string()
        } else {
            t.seed.clone()
        };
        let w = World::new(
            parse_seed(&seed),
            Settings {
                mode: t.mode,
                size: t.size,
            },
        );
        app.start_game(w);
        fx::briefing(app);
        if app.records.charters == 0 {
            // playtesters missed the tutorial: say where it is, once per game until a Charter is finished
            let msg = app.keys.fill("New here? The Tutorial (title screen) teaches the basics in five minutes. {help} opens the handbook any time.");
            app.flash(msg, AlertLevel::Info);
        }
    }
}

fn title_key(app: &mut App, k: KeyEvent) {
    if app.show_help {
        handbook_key(app, k);
        return;
    }
    let km = app.keys.clone();
    let rows = title_rows(app);
    if !rows.contains(&app.title.row) {
        app.title.row = TitleRow::Play;
    }
    let at = rows.iter().position(|r| *r == app.title.row).unwrap_or(0);
    if km.is(Act::TitleQuit, &k) {
        app.quit = true;
    } else if km.is(Act::TitleUp, &k) {
        app.title.row = rows[at.saturating_sub(1)];
    } else if km.is(Act::TitleDown, &k) {
        app.title.row = rows[(at + 1).min(rows.len() - 1)];
    } else if km.is(Act::TitleNext, &k) || km.is(Act::TitlePrev, &k) {
        // On the size row this changes the map size; anywhere else, the mode.
        change_option(&mut app.title, km.is(Act::TitleNext, &k));
    } else if km.is(Act::TitleDaily, &k) {
        app.title.seed = daily_seed();
    } else if km.is(Act::TitleControls, &k) {
        open_controls(app);
    } else if km.is(Act::TitleLoad, &k) {
        let p = Some(app.save_path.clone());
        load_game(app, p);
    } else if km.is(Act::TitleStart, &k) {
        match app.title.row {
            TitleRow::Continue => {
                let p = app.continue_path();
                load_game(app, p);
            }
            TitleRow::Handbook => open_handbook(app),
            TitleRow::Controls => open_controls(app),
            TitleRow::Settings => settings::open(app),
            TitleRow::Quit => app.quit = true,
            TitleRow::Tutorial => {
                app.title.tutorial = true;
                new_game(app);
                app.title.tutorial = false;
            }
            TitleRow::Play | TitleRow::Mode | TitleRow::Size | TitleRow::Seed => new_game(app),
        }
    } else {
        // Typing the seed. Bound keys are checked first, so a seed can't contain them.
        let t = &mut app.title;
        match k.code {
            KeyCode::Backspace => {
                t.seed.pop();
            }
            KeyCode::Char(c)
                if (c.is_ascii_alphanumeric() || c == '-')
                    && t.seed.len() < 24
                    && !k.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                t.row = TitleRow::Seed; // show where the typing goes
                t.seed.push(c);
            }
            _ => {}
        }
    }
}

fn move_cursor(app: &mut App, dx: i32, dy: i32) {
    let Some(w) = app.world.as_ref() else { return };
    let to = Pos::new(
        (app.cursor.x + dx).clamp(0, w.map.w - 1),
        (app.cursor.y + dy).clamp(0, w.map.h - 1),
    );
    if app.dozer_held && app.mode == Mode::Place(Place::Demolish) {
        bulldoze_to(app, to);
    }
    app.cursor = to;
}

/// The same physical key, whatever Shift did to it ('x' and 'X').
fn same_key(a: KeyCode, b: KeyCode) -> bool {
    match (a, b) {
        (KeyCode::Char(a), KeyCode::Char(b)) => a.eq_ignore_ascii_case(&b),
        _ => a == b,
    }
}

/// Demolish every tile from the cursor to `to` (holding the demolish key
/// or dragging with the demolish tool), then leave the cursor there.
/// Tiles with nothing on them are skipped without a fuss.
fn bulldoze_to(app: &mut App, to: Pos) {
    let mut p = app.cursor;
    loop {
        if p != app.cursor || p == to {
            bulldoze(app, p);
        }
        if p == to {
            break;
        }
        p = Pos::new(p.x + (to.x - p.x).signum(), p.y + (to.y - p.y).signum());
    }
    app.cursor = to;
}

/// Demolish one tile quietly: only what actually went makes a sound.
fn bulldoze(app: &mut App, p: Pos) {
    let Some(w) = app.world.as_mut() else { return };
    if let Ok(m) = build::demolish(w, p) {
        app.flash(m, AlertLevel::Good);
        if app.frame >= app.last_dozer_sound + 3 {
            app.audio.play(audio::Sfx::Click);
            app.last_dozer_sound = app.frame;
        }
    }
}

/// The cursor step a key asks for, if it's a movement key.
fn movement(km: &keys::Keymap, k: &KeyEvent) -> Option<(i32, i32)> {
    const MOVES: [(Act, i32, i32); 8] = [
        (Act::MoveUp, 0, -1),
        (Act::MoveDown, 0, 1),
        (Act::MoveLeft, -1, 0),
        (Act::MoveRight, 1, 0),
        (Act::MoveUpFast, 0, -8),
        (Act::MoveDownFast, 0, 8),
        (Act::MoveLeftFast, -8, 0),
        (Act::MoveRightFast, 8, 0),
    ];
    MOVES
        .iter()
        .find(|(a, _, _)| km.is(*a, k))
        .map(|(_, dx, dy)| (*dx, *dy))
}

fn game_key(app: &mut App, k: KeyEvent) {
    let km = app.keys.clone();
    if app.show_stats {
        app.show_stats = false;
        return;
    }
    if app.show_overview {
        if let Some((dx, dy)) = movement(&km, &k) {
            // one press moves about two overview cells
            let s = app
                .world
                .as_ref()
                .map(|w| draw::overview_scale(w, app.map_area))
                .unwrap_or(1.0);
            let step = (s * 2.0).round().max(1.0) as i32;
            move_cursor(
                app,
                dx.signum() * step * if dx.abs() > 1 { 4 } else { 1 },
                dy.signum() * step * if dy.abs() > 1 { 4 } else { 1 },
            );
        } else if km.is(Act::Overview, &k) || km.is(Act::Cancel, &k) || km.is(Act::Confirm, &k) {
            app.show_overview = false;
        }
        return;
    }
    if app.show_help {
        handbook_key(app, k);
        return;
    }
    if app.menu.is_some() {
        menu_key(app, k);
        return;
    }
    // End of the Charter.
    if app.world.as_ref().is_some_and(|w| w.finished)
        && !app.end_dismissed
        && !matches!(app.mode, Mode::Confirm(_))
    {
        if km.is(Act::EndNew, &k) {
            app.go_title();
        } else if km.is(Act::EndQuit, &k) {
            app.quit = true;
        } else if km.is(Act::EndClose, &k) {
            app.end_dismissed = true;
        }
        return;
    }
    if let Mode::Confirm(c) = app.mode.clone() {
        if km.is(Act::Yes, &k) {
            let w = app.world.as_mut().unwrap();
            match c {
                Confirm::Quit => {
                    app.autosave();
                    app.quit = true;
                }
                Confirm::NewGame => {
                    app.autosave();
                    app.go_title();
                }
                Confirm::Scrap(id) => {
                    if let Some(m) = w.remove_vehicle(id) {
                        app.flash(m, AlertLevel::Info);
                    }
                    app.selected = None;
                }
            }
        } else {
            app.flash("Cancelled.", AlertLevel::Info);
        }
        app.mode = Mode::Normal;
        return;
    }
    if let Mode::Schedule { picking, .. } = app.mode {
        let game = km.act_in(Group::Game, &k).is_some();
        let picks = picking
            && (movement(&km, &k).is_some() || km.is(Act::Confirm, &k) || km.is(Act::Cancel, &k));
        if picks || (!picking && km.act_in(Group::Schedule, &k).is_some()) {
            schedule_key(app, k);
            return;
        }
        // Anything else isn't lost: pausing, the handbook and moving the
        // cursor work with the editor open, and a map key (a tool, buying
        // a vehicle) closes it and does its job.
        if !game && movement(&km, &k).is_none() {
            if km.act_in(Group::Map, &k).is_none() {
                return;
            }
            app.mode = Mode::Normal;
        }
    }
    if let Some((dx, dy)) = movement(&km, &k) {
        move_cursor(app, dx, dy);
        return;
    }
    // Keys that work in every mode on the map.
    let speeds = [Act::Speed1, Act::Speed2, Act::Speed3, Act::Speed4];
    if km.is(Act::Pause, &k) {
        if app.speed == 0 {
            app.speed = app.last_speed.max(1);
        } else {
            app.last_speed = app.speed;
            app.speed = 0;
        }
    } else if let Some(i) = speeds.iter().position(|a| km.is(*a, &k)) {
        app.speed = i + 1;
        app.last_speed = app.speed;
    } else if km.is(Act::Help, &k) {
        open_handbook(app);
    } else if km.is(Act::Controls, &k) {
        open_controls(app);
    } else if km.is(Act::Overview, &k) {
        app.show_overview = true;
    } else if km.is(Act::Stats, &k) {
        app.show_stats = true;
    } else if km.is(Act::Overlay, &k) {
        app.overlay = app.overlay.next();
        app.flash(format!("Overlay: {}", app.overlay.name()), AlertLevel::Info);
    } else if let Some(item) = shortcut(&km, &k) {
        settings::adjust(app, item, 0);
        if item != settings::Item::Mute {
            let msg = settings::state_label(app, item);
            app.flash(msg, AlertLevel::Info);
        }
    } else if km.is(Act::MapZoomIn, &k) || km.is(Act::MapZoomOut, &k) {
        let z = if km.is(Act::MapZoomIn, &k) {
            app.map_zoom.closer()
        } else {
            app.map_zoom.farther()
        };
        match z {
            Some(z) => settings::set_map_zoom(app, z),
            None => app.flash(
                format!(
                    "The map is as {} as it goes ({}).",
                    if km.is(Act::MapZoomIn, &k) {
                        "close"
                    } else {
                        "far out"
                    },
                    app.map_zoom.name()
                ),
                AlertLevel::Info,
            ),
        }
    } else if km.is(Act::NextTab, &k) {
        app.tab = match app.tab {
            Tab::Alerts => Tab::Build,
            Tab::Build => Tab::Vehicles,
            Tab::Vehicles => Tab::Contracts,
            Tab::Contracts => Tab::Alerts,
        };
    } else if km.is(Act::Save, &k) {
        save_game(app);
    } else if km.is(Act::Load, &k) {
        match save::load(&app.save_path) {
            Ok(w) => {
                app.start_game(w);
                app.flash("Game loaded.", AlertLevel::Good);
            }
            Err(e) => app.flash(format!("Load failed: {e}"), AlertLevel::Warn),
        }
    } else if km.is(Act::AlertLatest, &k) {
        jump_to_alert(app, 0);
    } else if km.is(Act::AlertOlder, &k) {
        app.alert_jump += 1;
        let n = app.alert_jump;
        jump_to_alert(app, n);
    } else if km.is(Act::HomeTown, &k) {
        let w = app.world.as_ref().unwrap();
        app.cursor = w.towns[w.map.home].pos;
        app.center_view = true;
    } else if km.is(Act::SideMore, &k) {
        // draw_side clamps this; past the end it goes back to the top
        let max = app.side_max;
        app.side_scroll = if app.side_scroll >= max {
            0
        } else {
            (app.side_scroll + 4).min(max)
        };
    } else if km.is(Act::TutorialSkip, &k) {
        match app.tutorial.as_mut() {
            Some(t) if t.finished() => {
                app.tutorial = None;
                app.flash("Guide closed. The map is yours: keep building!", AlertLevel::Good);
            }
            Some(t) => t.forward(),
            None => app.flash(km.fill("{tutorial_skip} pages through the Tutorial. To start it, quit to the title screen ({deselect}, Quit to title) and pick Tutorial."), AlertLevel::Info),
        }
    } else if km.is(Act::TutorialBack, &k) {
        match app.tutorial.as_mut() {
            Some(t) => {
                if !t.back() {
                    app.flash("This is the first step of the Tutorial.", AlertLevel::Info);
                }
            }
            None => app.flash(km.fill("{tutorial_back} pages back through the Tutorial. To start it, quit to the title screen ({deselect}, Quit to title) and pick Tutorial."), AlertLevel::Info),
        }
    } else if leaves_tool(app, &k) {
        // Another map key while a tool is out (s for a station while laying
        // track): put the tool away and do what the key says.
        app.mode = Mode::Normal;
        app.msg = None;
        normal_key(app, k);
    } else {
        match app.mode.clone() {
            Mode::Line { kind, start } => line_key(app, k, kind, start),
            Mode::Double { start } => double_key(app, k, start),
            Mode::Place(p) => place_key(app, k, p),
            _ => normal_key(app, k),
        }
    }
}

/// A key that flips one of the settings straight from the game.
fn shortcut(km: &keys::Keymap, k: &KeyEvent) -> Option<settings::Item> {
    use settings::Item;
    [
        (Act::Palette, Item::Palette),
        (Act::Labels, Item::Labels),
        (Act::Sound, Item::Mute),
        (Act::Terrain, Item::Terrain),
        (Act::KeyPanel, Item::KeyPanel),
    ]
    .into_iter()
    .find(|(a, _)| km.is(*a, k))
    .map(|(_, i)| i)
}

/// The key is a map action other than the active tool's own (and not
/// confirm or cancel), so it should leave the tool.
fn leaves_tool(app: &App, k: &KeyEvent) -> bool {
    let own = match &app.mode {
        Mode::Line {
            kind: LayerKind::Rail,
            ..
        } => Act::Rail,
        Mode::Line { .. } => Act::Road,
        Mode::Double { .. } => Act::Double,
        Mode::Place(p) => (*p).act(),
        _ => return false,
    };
    let km = &app.keys;
    !km.is(Act::Confirm, k)
        && !km.is(Act::Cancel, k)
        && km.act_in(Group::Map, k).is_some_and(|a| a != own)
}

/// The rows of the game menu (Esc). The switches live in Settings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuItem {
    Resume,
    Save,
    Load,
    Settings,
    Handbook,
    Controls,
    QuitToTitle,
    QuitGame,
}

pub const MENU: [MenuItem; 8] = [
    MenuItem::Resume,
    MenuItem::Save,
    MenuItem::Load,
    MenuItem::Settings,
    MenuItem::Handbook,
    MenuItem::Controls,
    MenuItem::QuitToTitle,
    MenuItem::QuitGame,
];

/// The game menu's rows.
pub fn menu_items(_app: &App) -> Vec<MenuItem> {
    MENU.to_vec()
}

impl MenuItem {
    /// The row's text, and a few words on it.
    pub fn label(self) -> (&'static str, &'static str) {
        match self {
            MenuItem::Resume => ("Resume", "back to the railway"),
            MenuItem::Save => ("Save game", "keep this railway for later"),
            MenuItem::Load => ("Load saved game", "back to your last save"),
            MenuItem::Settings => ("Settings", "volume, text size, map zoom"),
            MenuItem::Handbook => ("Railway Handbook", "how everything works"),
            MenuItem::Controls => ("Controls", "every key, and change any"),
            MenuItem::QuitToTitle => ("Quit to title", "autosaves; Continue picks it up"),
            MenuItem::QuitGame => ("Quit Railhead", "autosaves too"),
        }
    }
}

fn save_game(app: &mut App) {
    let r = save::save(app.world.as_ref().unwrap(), &app.save_path)
        .map(|_| format!("Saved to {}", app.save_path.display()))
        .map_err(|e| format!("Save failed: {e}"));
    app.result(r);
}

fn menu_key(app: &mut App, k: KeyEvent) {
    let km = app.keys.clone();
    let Some(sel) = app.menu else { return };
    let items = menu_items(app);
    if km.is(Act::MenuUp, &k) {
        app.menu = Some(sel.saturating_sub(1));
    } else if km.is(Act::MenuDown, &k) {
        app.menu = Some((sel + 1).min(items.len() - 1));
    } else if km.is(Act::MenuClose, &k) {
        app.menu = None;
    } else if km.is(Act::MenuPick, &k) {
        match items[sel.min(items.len() - 1)] {
            MenuItem::Resume => app.menu = None,
            MenuItem::Save => {
                save_game(app);
                app.menu = None;
            }
            MenuItem::Load => {
                let p = Some(app.save_path.clone());
                load_game(app, p);
            }
            // the menu stays open underneath, so closing these comes back to it
            MenuItem::Settings => settings::open(app),
            MenuItem::Handbook => open_handbook(app),
            MenuItem::Controls => open_controls(app),
            MenuItem::QuitToTitle => {
                app.autosave();
                app.go_title();
            }
            MenuItem::QuitGame => {
                app.autosave();
                app.quit = true;
            }
        }
    }
}

fn jump_to_alert(app: &mut App, back: usize) {
    let w = app.world.as_ref().unwrap();
    let with_pos: Vec<_> = w.alerts.iter().rev().filter(|a| a.pos.is_some()).collect();
    if with_pos.is_empty() {
        app.flash("No alerts with a location yet.", AlertLevel::Info);
        return;
    }
    let i = back % with_pos.len();
    if back == 0 {
        app.alert_jump = 0;
    }
    let a = with_pos[i];
    app.cursor = a.pos.unwrap();
    app.center_view = true;
    let text = app.keys.fill(&a.text);
    app.flash(format!("[alert {}] {}", i + 1, text), AlertLevel::Info);
}

fn normal_key(app: &mut App, k: KeyEvent) {
    let km = app.keys.clone();
    let cursor = app.cursor;
    if km.is(Act::Deselect, &k) {
        if app.selected.is_some() {
            app.selected = None;
        } else {
            app.menu = Some(0);
        }
    } else if km.is(Act::Quit, &k) {
        app.mode = Mode::Confirm(Confirm::Quit);
    } else if km.is(Act::NewGame, &k) {
        app.mode = Mode::Confirm(Confirm::NewGame);
    } else if km.is(Act::Road, &k) {
        start_line(app, LayerKind::Road);
    } else if km.is(Act::Rail, &k) {
        start_line(app, LayerKind::Rail);
    } else if km.is(Act::Double, &k) {
        app.mode = Mode::Double { start: None };
        app.flash(km.fill("Double track: move to the start and press {confirm}. Builds two one-way lines with signals."), AlertLevel::Info);
    } else if km.is(Act::Station, &k) {
        app.mode = Mode::Place(Place::Station);
    } else if km.is(Act::Signal, &k) {
        app.mode = Mode::Place(Place::Signal);
    } else if km.is(Act::Hub, &k) {
        app.mode = Mode::Place(Place::Hub);
    } else if km.is(Act::Demolish, &k) {
        match app.selected {
            Some(id) => app.mode = Mode::Confirm(Confirm::Scrap(id)),
            None => app.mode = Mode::Place(Place::Demolish),
        }
    } else if km.is(Act::Chain, &k) {
        let w = app.world.as_mut().unwrap();
        let r = build::toggle_chain(w, cursor);
        app.result(r);
    } else if km.is(Act::AutoSignal, &k) {
        let w = app.world.as_mut().unwrap();
        let r = build::auto_signal(w, cursor, 6)
            .map(|n| format!("Auto-signal placed {n} signals on this network."));
        app.result(r);
    } else if km.is(Act::Select, &k) {
        select_at_cursor(app);
    } else if km.is(Act::BuyFreight, &k) {
        new_vehicle(app, TrainKind::Freight);
    } else if km.is(Act::BuyExpress, &k) {
        if app
            .world
            .as_ref()
            .is_some_and(|w| w.tile(cursor).station.is_some())
        {
            new_vehicle(app, TrainKind::Express);
        } else {
            app.flash(
                km.fill("{buy_express} buys an express train: put the cursor on a station."),
                AlertLevel::Info,
            );
        }
    } else if km.is(Act::Edit, &k) {
        if let Some(id) = app.selected {
            app.mode = Mode::Schedule {
                vehicle: id,
                sel: 0,
                picking: false,
            };
        } else {
            app.flash(
                km.fill("Select a vehicle first ({select} on it, or {next_vehicle} to cycle)."),
                AlertLevel::Info,
            );
        }
    } else if km.is(Act::Duplicate, &k) {
        let Some(id) = app.selected else {
            app.flash(km.fill("Select a vehicle first ({select} on it, or {next_vehicle}), then {duplicate} buys another with the same schedule."), AlertLevel::Info);
            return;
        };
        let w = app.world.as_mut().unwrap();
        match w.clone_vehicle(id) {
            Ok(new) => {
                let (name, of) = (
                    w.carrier(new).unwrap().0.name.clone(),
                    w.carrier(id).unwrap().0.name.clone(),
                );
                app.selected = Some(new);
                if let Some(p) = w.vehicle_pos(new) {
                    app.cursor = p;
                }
                app.flash(km.fill(&format!("{name} bought: a copy of {of}, same stops. {{duplicate}} again for another.")), AlertLevel::Good);
                app.audio.play(audio::Sfx::Beep);
            }
            Err(e) => {
                app.flash(e, AlertLevel::Warn);
                app.audio.play(audio::Sfx::Error);
            }
        }
    } else if km.is(Act::NextVehicle, &k) {
        cycle_vehicle(app, false);
    } else if km.is(Act::PrevVehicle, &k) {
        cycle_vehicle(app, true);
    } else if km.is(Act::Reverse, &k) {
        let Some(id) = app.selected else {
            app.flash(km.fill("Select a train first ({next_vehicle} cycles through them), then {reverse} turns it round."), AlertLevel::Info);
            return;
        };
        let w = app.world.as_mut().unwrap();
        let r = rail::reverse_train(w, id);
        app.result(r);
    } else if km.is(Act::More, &k) {
        adjust_more(app, true);
    } else if km.is(Act::Less, &k) {
        adjust_more(app, false);
    } else {
        unbound_key(app, &k);
    }
}

/// A key that means nothing here says so, instead of nothing happening.
fn unbound_key(app: &mut App, k: &KeyEvent) {
    let KeyCode::Char(c) = k.code else { return };
    if k.modifiers.contains(KeyModifiers::CONTROL)
        || k.modifiers.contains(KeyModifiers::ALT)
        || c == ' '
    {
        return;
    }
    let msg = app.keys.fill(&format!("{c} isn't a key here. The Build tab ({{next_tab}}) lists the tools, {{help}} opens the handbook, {{controls}} shows every key."));
    app.flash(msg, AlertLevel::Info);
}

fn adjust_more(app: &mut App, up: bool) {
    let cursor = app.cursor;
    let hint = app.keys.fill("{more} / {less} change a station's train limit (cursor on it) or a selected train's wagons.");
    let w = app.world.as_mut().unwrap();
    // Train selected: wagons. Else station under cursor: train limit.
    if let Some(id) = app.selected
        && w.carrier(id).map(|c| c.1) == Some(VKind::Train)
    {
        let r = rail::change_wagons(w, id, if up { 1 } else { -1 });
        app.result(r);
        return;
    }
    if let Some(sid) = w.tile(cursor).station {
        let s = &mut w.stations[sid as usize];
        s.limit = if up {
            (s.limit + 1).min(9)
        } else {
            s.limit.saturating_sub(1).max(1)
        };
        let msg = format!("{}: train limit {}", s.name, s.limit);
        app.flash(msg, AlertLevel::Good);
    } else {
        app.flash(hint, AlertLevel::Info);
    }
}

fn start_line(app: &mut App, kind: LayerKind) {
    app.mode = Mode::Line { kind, start: None };
    let msg = app.keys.fill(&format!(
        "Build {}: move to the start point and press {{confirm}}.",
        kind.name()
    ));
    app.flash(msg, AlertLevel::Info);
}

fn line_key(app: &mut App, k: KeyEvent, kind: LayerKind, start: Option<Pos>) {
    let km = app.keys.clone();
    let cursor = app.cursor;
    // Pressing the tool's own key again also builds.
    let again = km.is(
        if kind == LayerKind::Rail {
            Act::Rail
        } else {
            Act::Road
        },
        &k,
    );
    if km.is(Act::Cancel, &k) {
        if start.is_some() {
            app.mode = Mode::Line { kind, start: None };
        } else {
            app.mode = Mode::Normal;
            app.msg = None;
        }
    } else if km.is(Act::Confirm, &k) || again {
        let w = app.world.as_mut().unwrap();
        match start {
            None => {
                app.mode = Mode::Line {
                    kind,
                    start: Some(cursor),
                };
                app.flash(
                    km.fill("Now move to the end point and press {confirm} ({cancel} to cancel)."),
                    AlertLevel::Info,
                );
            }
            Some(s) => match build::plan_path(w, s, cursor, kind)
                .and_then(|p| build::commit_path(w, &p, kind).map(|c| (p, c)))
            {
                Ok((path, cost)) => {
                    app.flash(
                        km.fill(&format!(
                            "Built {} tiles for {}. Keep going from here, or {{cancel}}.",
                            path.len(),
                            money(cost)
                        )),
                        AlertLevel::Good,
                    );
                    app.audio.play(audio::Sfx::Click);
                    app.mode = Mode::Line {
                        kind,
                        start: Some(cursor),
                    };
                }
                Err(e) => {
                    app.flash(e, AlertLevel::Warn);
                    app.audio.play(audio::Sfx::Error);
                }
            },
        }
    }
}

fn double_key(app: &mut App, k: KeyEvent, start: Option<Pos>) {
    let km = app.keys.clone();
    let cursor = app.cursor;
    if km.is(Act::Cancel, &k) {
        if start.is_some() {
            app.mode = Mode::Double { start: None };
        } else {
            app.mode = Mode::Normal;
            app.msg = None;
        }
    } else if km.is(Act::Confirm, &k) || km.is(Act::Double, &k) {
        let w = app.world.as_mut().unwrap();
        match start {
            None => {
                app.mode = Mode::Double {
                    start: Some(cursor),
                };
                app.flash(
                    km.fill("Now move to the end and press {confirm} ({cancel} to cancel)."),
                    AlertLevel::Info,
                );
            }
            Some(s) => match build::plan_double(w, s, cursor)
                .and_then(|l| build::commit_double(w, &l).map(|c| (l, c)))
            {
                Ok((l, cost)) => {
                    app.flash(
                        format!("Double track built: {} + {} tiles for {}. Join the ends to your network.", l.0.len(), l.1.len(), money(cost)),
                        AlertLevel::Good,
                    );
                    app.audio.play(audio::Sfx::Click);
                    app.mode = Mode::Normal;
                }
                Err(e) => {
                    app.flash(e, AlertLevel::Warn);
                    app.audio.play(audio::Sfx::Error);
                }
            },
        }
    }
}

fn place_key(app: &mut App, k: KeyEvent, p: Place) {
    let km = app.keys.clone();
    let cursor = app.cursor;
    let own = p.act();
    if km.is(Act::Cancel, &k) {
        app.mode = Mode::Normal;
        app.msg = None;
    } else if p == Place::Demolish
        && k.kind == KeyEventKind::Repeat
        && (km.is(Act::Confirm, &k) || km.is(own, &k))
    {
        // holding the key down keeps clearing (and stays quiet when there's nothing left)
        bulldoze(app, cursor);
    } else if km.is(Act::Confirm, &k) || km.is(own, &k) {
        let w = app.world.as_mut().unwrap();
        let r = match p {
            Place::Station => build::place_station(w, cursor),
            Place::Signal => build::cycle_signal(w, cursor),
            Place::Hub => build::place_site(w, cursor, SiteKind::Hub),
            Place::Demolish => build::demolish(w, cursor),
        };
        app.result(r);
    }
}

fn select_at_cursor(app: &mut App) {
    let w = app.world.as_ref().unwrap();
    let here = w.vehicles_at(app.cursor);
    if here.is_empty() {
        app.selected = None;
        return;
    }
    // cycle through vehicles stacked on this tile
    let next = match app.selected.and_then(|s| here.iter().position(|v| *v == s)) {
        Some(i) => here[(i + 1) % here.len()],
        None => here[0],
    };
    app.selected = Some(next);
}

fn cycle_vehicle(app: &mut App, back: bool) {
    let w = app.world.as_ref().unwrap();
    let mut ids: Vec<u32> = w.trains.iter().map(|t| t.carrier.id).collect();
    ids.extend(w.trucks.iter().map(|t| t.carrier.id));
    if ids.is_empty() {
        let msg = app.keys.fill("No vehicles yet. Press {buy_freight} on a town or industry (truck) or a station (train).");
        app.flash(msg, AlertLevel::Info);
        return;
    }
    let i = app.selected.and_then(|s| ids.iter().position(|v| *v == s));
    let n = ids.len();
    let next = match i {
        Some(i) if back => ids[(i + n - 1) % n],
        Some(i) => ids[(i + 1) % n],
        None => ids[0],
    };
    app.selected = Some(next);
    if let Some(p) = w.vehicle_pos(next) {
        app.cursor = p;
    }
}

fn new_vehicle(app: &mut App, kind: TrainKind) {
    let cursor = app.cursor;
    let km = app.keys.clone();
    let w = app.world.as_mut().unwrap();
    // A new vehicle loads where it was bought; the editor goes straight to
    // picking where it delivers, so a simple route is two key presses.
    let first_stop = |app: &mut App, id: u32, target: Target, what: &str| {
        let w = app.world.as_mut().unwrap();
        let here = w.target_name(target);
        let c = w.carrier_mut(id).unwrap();
        c.schedule.stops.push(Stop::new(target, Action::Load));
        c.cur = 0;
        app.selected = Some(id);
        app.mode = Mode::Schedule {
            vehicle: id,
            sel: 0,
            picking: true,
        };
        app.flash(
            app.keys.fill(&format!("{what} ready, loading at {here}. Now move to where it delivers and press {{confirm}} ({{cancel}} to edit the schedule).")),
            AlertLevel::Good,
        );
    };
    if let Some(sid) = w.tile(cursor).station {
        match rail::buy_train(w, sid, kind) {
            Ok(id) => {
                first_stop(
                    app,
                    id,
                    Target::Station(sid),
                    &format!("{} train", kind.name()),
                );
                app.audio.play(audio::Sfx::Horn);
            }
            Err(e) => app.flash(e, AlertLevel::Warn),
        }
        return;
    }
    if let Some(site) = w.tile(cursor).site {
        match w.buy_truck(site) {
            Ok(id) => {
                first_stop(app, id, Target::Site(site), "Truck");
                app.audio.play(audio::Sfx::Beep);
            }
            Err(e) => app.flash(e, AlertLevel::Warn),
        }
        return;
    }
    app.flash(
        km.fill("Put the cursor on a town, industry or hub (for a truck) or a station (for a train) and press {buy_freight}."),
        AlertLevel::Info,
    );
}

fn schedule_key(app: &mut App, k: KeyEvent) {
    let Mode::Schedule {
        vehicle,
        mut sel,
        picking,
    } = app.mode.clone()
    else {
        return;
    };
    let km = app.keys.clone();
    let cursor = app.cursor;
    let w = app.world.as_mut().unwrap();
    let Some((_, kind)) = w.carrier(vehicle) else {
        app.mode = Mode::Normal;
        return;
    };
    if picking {
        if let Some((dx, dy)) = movement(&km, &k) {
            move_cursor(app, dx, dy);
            return;
        }
        if km.is(Act::Cancel, &k) {
            app.mode = Mode::Schedule {
                vehicle,
                sel,
                picking: false,
            };
        } else if km.is(Act::Confirm, &k) {
            let target = match kind {
                VKind::Train => w.tile(cursor).station.map(Target::Station),
                VKind::Truck => w.tile(cursor).site.map(Target::Site),
            };
            let Some(target) = target else {
                let need = if kind == VKind::Train {
                    "a station"
                } else {
                    "a town, industry or hub"
                };
                app.flash(
                    format!("That's not {need}. Move the cursor onto one."),
                    AlertLevel::Warn,
                );
                app.audio.play(audio::Sfx::Error);
                return;
            };
            let c = w.carrier_mut(vehicle).unwrap();
            // Alternate load/unload as a sensible default.
            let action = match c.schedule.stops.last() {
                Some(s) if s.action == Action::Load => Action::Unload,
                _ => Action::Load,
            };
            let at = (sel + 1).min(c.schedule.stops.len());
            c.schedule.stops.insert(at, Stop::new(target, action));
            if c.schedule.stops.len() == 1 {
                c.cur = 0;
            }
            app.mode = Mode::Schedule {
                vehicle,
                sel: at,
                picking: false,
            };
            let msg = if c.schedule.stops.len() == 2 {
                "Route set, it's on its way! {sched_close} closes the editor. Later, {duplicate} on a selected vehicle buys a copy with the same route."
            } else {
                "Stop added. {sched_action} = load/unload, {sched_cargo} = cargo, {sched_cond} = condition."
            };
            app.flash(km.fill(msg), AlertLevel::Good);
            app.audio.play(audio::Sfx::Click);
        }
        return;
    }
    let c = w.carrier_mut(vehicle).unwrap();
    let n = c.schedule.stops.len();
    let has = n > 0;
    if km.is(Act::SchedClose, &k) {
        app.mode = Mode::Normal;
        return;
    } else if km.is(Act::SchedUp, &k) {
        sel = sel.saturating_sub(1);
    } else if km.is(Act::SchedDown, &k) {
        sel = (sel + 1).min(n.saturating_sub(1));
    } else if km.is(Act::SchedAdd, &k) {
        app.mode = Mode::Schedule {
            vehicle,
            sel,
            picking: true,
        };
        let what = if kind == VKind::Train {
            "station"
        } else {
            "town, industry or hub"
        };
        app.flash(
            km.fill(&format!(
                "Move the cursor to a {what} and press {{confirm}}."
            )),
            AlertLevel::Info,
        );
        return;
    } else if has && km.is(Act::SchedDelete, &k) {
        c.schedule.stops.remove(sel.min(n - 1));
        sel = sel.min(c.schedule.stops.len().saturating_sub(1));
        c.cur = 0;
    } else if has && km.is(Act::SchedAction, &k) {
        let s = &mut c.schedule.stops[sel];
        *s = Stop {
            filter: s.filter,
            ..Stop::new(s.target, s.action.next())
        };
    } else if has && (km.is(Act::SchedCargo, &k) || km.is(Act::SchedCargoBack, &k)) {
        let s = &mut c.schedule.stops[sel];
        let presets = CargoMask::presets();
        let i = presets.iter().position(|m| *m == s.filter).unwrap_or(0);
        let step = if km.is(Act::SchedCargoBack, &k) {
            presets.len() - 1
        } else {
            1
        };
        s.filter = presets[(i + step) % presets.len()];
    } else if has && km.is(Act::SchedCond, &k) {
        let s = &mut c.schedule.stops[sel];
        s.cond = match s.cond {
            Cond::Full => Cond::Empty,
            Cond::Empty => Cond::Time(20.0),
            Cond::Time(_) => Cond::Full,
        };
    } else if has && km.is(Act::SchedTimeout, &k) {
        let s = &mut c.schedule.stops[sel];
        s.timeout = if s.timeout.is_some() {
            None
        } else {
            Some(30.0)
        };
    } else if has && (km.is(Act::SchedTimeMore, &k) || km.is(Act::SchedTimeLess, &k)) {
        let d = if km.is(Act::SchedTimeLess, &k) {
            -5.0
        } else {
            5.0
        };
        let s = &mut c.schedule.stops[sel];
        if let Cond::Time(t) = s.cond {
            s.cond = Cond::Time((t + d).max(5.0));
        } else if let Some(t) = s.timeout {
            s.timeout = Some((t + d).max(5.0));
        }
    } else if has && km.is(Act::SchedGate, &k) {
        use crate::world::vehicles::Gate;
        let s = &mut c.schedule.stops[sel];
        s.gate = match s.gate {
            None => Some(Gate::Waiting(B::GATE_DEFAULT)),
            Some(Gate::Waiting(v)) => Some(Gate::Wants(v)),
            Some(Gate::Wants(_)) => None,
        };
    } else if has && (km.is(Act::SchedGateMore, &k) || km.is(Act::SchedGateLess, &k)) {
        use crate::world::vehicles::Gate;
        let d = if km.is(Act::SchedGateLess, &k) {
            -B::GATE_STEP
        } else {
            B::GATE_STEP
        };
        let s = &mut c.schedule.stops[sel];
        s.gate = match s.gate {
            Some(Gate::Waiting(v)) => Some(Gate::Waiting((v + d).max(B::GATE_STEP))),
            Some(Gate::Wants(v)) => Some(Gate::Wants((v + d).max(B::GATE_STEP))),
            None => None,
        };
    } else if n > 1 && sel > 0 && km.is(Act::SchedMoveUp, &k) {
        c.schedule.stops.swap(sel, sel - 1);
        sel -= 1;
    } else if n > 1 && sel + 1 < n && km.is(Act::SchedMoveDown, &k) {
        c.schedule.stops.swap(sel, sel + 1);
        sel += 1;
    } else if has && km.is(Act::SchedGo, &k) {
        // send the vehicle to this stop now
        c.cur = sel;
        c.state = crate::world::VState::Travel;
        c.checked = true; // the player said go: ignore its gate this time
    }
    app.mode = Mode::Schedule {
        vehicle,
        sel,
        picking: false,
    };
}

/// Today's shared seed, e.g. "daily-2026-10-21" (UTC): everyone gets the same map.
pub fn daily_seed() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    format!("daily-{y:04}-{m:02}-{d:02}")
}

/// Days since 1970-01-01 to (year, month, day). Howard Hinnant's algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

/// Render one frame of a game (seed, after `ticks`) as plain text. Lets agents
/// and scripts "see" the UI without a real terminal: `railhead --snapshot 42 600`.
pub fn snapshot(
    seed: &str,
    ticks: u64,
    width: u16,
    height: u16,
    overlay: usize,
    settings: Settings,
) -> String {
    use ratatui::backend::TestBackend;
    let mut app = App::new(
        Some(seed.into()),
        settings.clone(),
        PathBuf::from("unused.json"),
    );
    let mut w = World::new(parse_seed(seed), settings);
    for _ in 0..ticks {
        w.tick();
    }
    app.start_game(w);
    app.msg = None;
    // replay the last few alerts so a recent headline or firework shows
    let seen = app
        .world
        .as_ref()
        .map_or(0, |w| w.alert_count.saturating_sub(2));
    app.fx = fx::Fx {
        seen,
        ..fx::Fx::default()
    };
    app.speed = 1;
    for frame in [1, 20] {
        app.frame = frame;
        fx::update(&mut app);
    }
    for _ in 0..overlay {
        app.overlay = app.overlay.next();
    }
    let args: Vec<String> = std::env::args().collect();
    let arg = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .map(|s| s.to_ascii_lowercase())
    };
    app.show_overview = args.iter().any(|a| a == "--overview");
    // --zoom far|normal|close|closer: the map zoom
    if let Some(z) = arg("--zoom").and_then(|z| {
        view::MapZoom::ALL
            .into_iter()
            .find(|m| m.name().eq_ignore_ascii_case(&z))
    }) {
        app.map_zoom = z;
    }
    // --screen title|menu|settings|confirm|stats|help|controls: what's open on top
    match arg("--screen").as_deref() {
        Some("title") => app.go_title(),
        Some("menu") => app.menu = Some(0),
        Some("settings") => {
            app.menu = Some(3);
            settings::open(&mut app);
        }
        Some("confirm") => app.mode = Mode::Confirm(Confirm::Quit),
        Some("stats") => app.show_stats = true,
        Some("help") => open_handbook(&mut app),
        Some("controls") => open_controls(&mut app),
        _ => {}
    }
    // --tutorial N: the tutorial's map, on step N
    if let Some(n) = arg("--tutorial").and_then(|n| n.parse::<usize>().ok()) {
        let w = tutorial::world();
        let mut tut = tutorial::Tutorial::new(&w);
        app.start_game(w);
        app.msg = None;
        tut.step = n.min(tutorial::STEPS.len() - 1);
        tut.reached = tut.step;
        app.tutorial = Some(tut);
    }
    let mut term = ratatui::Terminal::new(TestBackend::new(width, height)).unwrap();
    term.draw(|f| draw::draw(f, &mut app)).unwrap();
    let buf = term.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..height {
        for x in 0..width {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;

    fn key(app: &mut App, code: KeyCode) {
        handle_key(app, KeyEvent::new(code, KeyModifiers::NONE));
    }
    fn render(app: &mut App, term: &mut Terminal<TestBackend>) -> String {
        term.draw(|f| draw::draw(f, app)).unwrap();
        let buf = term.backend().buffer().clone();
        buf.content().iter().map(|c| c.symbol()).collect()
    }

    /// Build a working railway using only keypresses.
    #[test]
    fn rail_via_keyboard() {
        let mut app = App::new(
            Some("42".into()),
            Settings::default(),
            std::env::temp_dir().join("rh_ui_test2.json"),
        );
        let mut term = Terminal::new(TestBackend::new(150, 48)).unwrap();
        key(&mut app, KeyCode::Enter);
        let cap = app.cursor;
        // Find a 10-tile buildable rail route starting next to the depot.
        let (a, b) = {
            let w = app.world.as_ref().unwrap();
            let mut found = None;
            'search: for (sx, sy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let a = Pos::new(cap.x + sx, cap.y + sy);
                for (ex, ey) in [
                    (10, 0),
                    (-10, 0),
                    (0, 10),
                    (0, -10),
                    (8, 6),
                    (-8, 6),
                    (8, -6),
                    (-8, -6),
                ] {
                    let b = Pos::new(a.x + ex, a.y + ey);
                    if let Ok(p) = build::plan_path(w, a, b, LayerKind::Rail) {
                        if p.len() >= 8
                            && p.iter()
                                .all(|q| w.tile(*q).road == crate::world::Layer::None)
                        {
                            found = Some((a, b));
                            break 'search;
                        }
                    }
                }
            }
            found.expect("no rail route near the capital")
        };
        // Lay track.
        key(&mut app, KeyCode::Char('t'));
        app.cursor = a;
        key(&mut app, KeyCode::Enter);
        app.cursor = b;
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Esc);
        let w = app.world.as_mut().unwrap();
        for _ in 0..1200 {
            w.tick();
        }
        assert_eq!(w.tile(a).rail, crate::world::Layer::Built);
        // Stations at both ends.
        let stations_before = app.world.as_ref().unwrap().stations.len();
        key(&mut app, KeyCode::Char('s'));
        app.cursor = a;
        key(&mut app, KeyCode::Enter);
        app.cursor = b;
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Esc);
        let w = app.world.as_mut().unwrap();
        for _ in 0..300 {
            w.tick();
        }
        assert_eq!(w.stations.len(), stations_before + 2, "stations not built");
        // Train + schedule A(load) -> B(unload).
        app.cursor = a;
        key(&mut app, KeyCode::Char('n'));
        assert_eq!(app.world.as_ref().unwrap().trains.len(), 1);
        // A is already stop 1 (load); the editor is picking the next stop
        app.cursor = b;
        key(&mut app, KeyCode::Enter); // stop at B
        key(&mut app, KeyCode::Esc);
        // a copy of the train takes the same route
        app.cursor = a;
        key(&mut app, KeyCode::Char('d'));
        let w = app.world.as_ref().unwrap();
        assert_eq!(
            w.trains.len(),
            1,
            "no signals yet: the whole line is one block, taken by the first train"
        );
        // Auto-signal the line, then run.
        app.cursor = a;
        key(&mut app, KeyCode::Char('G'));
        let w = app.world.as_mut().unwrap();
        let mut reached = false;
        for _ in 0..1500 {
            w.tick();
            if w.trains[0].head() == b {
                reached = true;
            }
        }
        assert!(reached, "train never reached the far station");
        assert!(w.stations[0].stats.per_min() + w.stations[1].stats.per_min() >= 0.0);
        app.selected = Some(app.world.as_ref().unwrap().trains[0].carrier.id);
        app.cursor = a;
        let screen = render(&mut app, &mut term);
        assert!(screen.contains("RAILHEAD"));
        if std::env::var("RH_PRINT").is_ok() {
            for row in screen.chars().collect::<Vec<_>>().chunks(150) {
                eprintln!("{}", row.iter().collect::<String>());
            }
        }
    }

    /// Play the whole tutorial with keypresses; every step's check must pass.
    #[test]
    fn tutorial_plays_through() {
        let mut app = App::new(
            Some("x".into()),
            Settings::default(),
            std::env::temp_dir().join("rh_ui_tut.json"),
        );
        let mut term = Terminal::new(TestBackend::new(150, 48)).unwrap();
        key(&mut app, KeyCode::Down); // New game → Tutorial
        assert_eq!(app.title.row, TitleRow::Tutorial);
        assert!(render(&mut app, &mut term).contains("guided first railway"));
        key(&mut app, KeyCode::Enter);
        assert!(
            !app.title.tutorial,
            "the flag only lasts while the tutorial starts"
        );
        assert!(app.tutorial.is_some() && app.world.as_ref().unwrap().sandbox());
        let step = |app: &mut App, term: &mut Terminal<TestBackend>, want: usize| {
            app.update_tutorial();
            assert_eq!(
                app.tutorial.as_ref().unwrap().step,
                want,
                "tutorial stuck before step {want}"
            );
            let screen = render(app, term);
            assert!(screen.contains("TUTORIAL"));
            if want == 5 && std::env::var("RH_PRINT").is_ok() {
                for row in screen.chars().collect::<Vec<_>>().chunks(150) {
                    eprintln!("{}", row.iter().collect::<String>());
                }
            }
        };
        step(&mut app, &mut term, 0);
        key(&mut app, KeyCode::Char(' '));
        step(&mut app, &mut term, 1);
        let industry = app
            .world
            .as_ref()
            .unwrap()
            .sites
            .iter()
            .find(|s| matches!(s.kind, SiteKind::Industry(_)))
            .unwrap()
            .pos;
        app.cursor = industry;
        step(&mut app, &mut term, 2);
        key(&mut app, KeyCode::Home);
        step(&mut app, &mut term, 3);
        // page back to reread a step: its check still passes, but the guide stays put
        key(&mut app, KeyCode::F(4));
        app.update_tutorial();
        assert_eq!(
            (
                app.tutorial.as_ref().unwrap().step,
                app.tutorial.as_ref().unwrap().reached
            ),
            (2, 3)
        );
        assert!(render(&mut app, &mut term).contains("done this one"));
        key(&mut app, KeyCode::F(3)); // and forward again, to where we were
        step(&mut app, &mut term, 3);

        // The stub: station at one end, free track end three tiles on. The
        // line goes to the industry the Tutorial marks, ending beside it.
        let (stub, stub_end) = {
            let w = app.world.as_ref().unwrap();
            let s = w.stations[0].pos;
            (
                s,
                [Pos::new(s.x + 3, s.y), Pos::new(s.x, s.y + 3)]
                    .into_iter()
                    .find(|q| w.tile(*q).rail.present())
                    .unwrap(),
            )
        };
        app.cursor = stub;
        step(&mut app, &mut term, 4);
        key(&mut app, KeyCode::Char('t'));
        app.cursor = stub_end;
        key(&mut app, KeyCode::Enter);
        let target = {
            let w = app.world.as_ref().unwrap();
            let marks = tutorial::guide_now(&app, w, app.tutorial.as_ref().unwrap())
                .unwrap()
                .marks;
            let goal = marks.first().expect("the end of the line is marked").0;
            assert!(
                matches!(
                    w.tile(goal).site.map(|s| w.sites[s as usize].kind),
                    Some(SiteKind::Industry(_))
                ),
                "the line goes to an industry"
            );
            [
                (2, 0),
                (-2, 0),
                (0, 2),
                (0, -2),
                (2, 1),
                (-2, 1),
                (1, 2),
                (1, -2),
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
            ]
            .into_iter()
            .map(|(dx, dy)| Pos::new(goal.x + dx, goal.y + dy))
            .find(|q| {
                w.map.in_bounds(*q)
                    && build::plan_path(w, stub_end, *q, LayerKind::Rail)
                        .is_ok_and(|p| p.len() >= 8)
            })
            .expect("no rail route from the stub to the marked industry")
        };
        app.cursor = target;
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Esc);
        step(&mut app, &mut term, 5);
        key(&mut app, KeyCode::Char('s'));
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Esc);
        step(&mut app, &mut term, 6);
        key(&mut app, KeyCode::Char('n'));
        step(&mut app, &mut term, 7);
        // the far station is already stop 1 (load at the industry); the editor is picking the next
        app.cursor = stub;
        key(&mut app, KeyCode::Enter); // unload it at home
        key(&mut app, KeyCode::Esc);
        step(&mut app, &mut term, 8);
        let mut food = false;
        for _ in 0..30_000 {
            app.world.as_mut().unwrap().tick();
            let t = &app.world.as_ref().unwrap().trains[0].carrier;
            food |= t.cargo[crate::types::Cargo::Food] > 0.0;
            if t.lifetime_income > 0.0 {
                break;
            }
        }
        assert!(food, "the first train brings food home, not an empty ride");
        step(&mut app, &mut term, 9);
        while !matches!(
            app.world.as_ref().unwrap().trains[0].carrier.state,
            crate::world::VState::Working(_)
        ) {
            app.world.as_mut().unwrap().tick();
        }
        let train = app.world.as_ref().unwrap().trains[0].carrier.id;
        while app.selected != Some(train) {
            key(&mut app, KeyCode::Char('v'));
        }
        key(&mut app, KeyCode::Char(']'));
        step(&mut app, &mut term, 10);
        key(&mut app, KeyCode::Esc);
        app.cursor = target;
        key(&mut app, KeyCode::Char('G'));
        step(&mut app, &mut term, 11);
        key(&mut app, KeyCode::Char('o'));
        key(&mut app, KeyCode::Char('o'));
        step(&mut app, &mut term, 12);
        key(&mut app, KeyCode::Char('i'));
        step(&mut app, &mut term, 13);
        assert!(app.tutorial.as_ref().unwrap().finished());
        key(&mut app, KeyCode::Esc); // close the stats screen
        key(&mut app, KeyCode::F(3));
        assert!(app.tutorial.is_none());
    }

    /// Every `{action}` placeholder in the UI text names a real action.
    #[test]
    fn key_placeholders_all_resolve() {
        let km = keys::Keymap::default();
        let check = |s: &str| {
            let filled = km.fill(s);
            assert!(!filled.contains('{'), "unresolved placeholder in: {filled}");
        };
        for c in handbook::chapters(&km) {
            for l in &c.lines {
                check(l);
            }
        }
        for s in tutorial::STEPS {
            check(s.text);
        }
        let mut app = App::new(
            Some("42".into()),
            Settings::default(),
            std::env::temp_dir().join("rh_ui_ph.json"),
        );
        let mut term = Terminal::new(TestBackend::new(150, 48)).unwrap();
        assert!(!render(&mut app, &mut term).contains('{'), "title screen");
        key(&mut app, KeyCode::Enter);
        for a in &app.world.as_ref().unwrap().alerts {
            check(&a.text);
        }
        for tab in 0..4 {
            let screen = render(&mut app, &mut term);
            assert!(!screen.contains("{"), "tab {tab} shows a raw placeholder");
            key(&mut app, KeyCode::Tab);
        }
        key(&mut app, KeyCode::Char('?'));
        let first = app.help_chapter;
        for ch in 0..handbook::chapter_count() {
            let screen = render(&mut app, &mut term);
            assert!(screen.contains("RAILWAY HANDBOOK"));
            assert!(
                !screen.contains('{'),
                "handbook chapter {ch} shows a raw placeholder"
            );
            if std::env::var("RH_PRINT").is_ok_and(|v| v == ch.to_string()) {
                for row in screen.chars().collect::<Vec<_>>().chunks(150) {
                    eprintln!("{}", row.iter().collect::<String>());
                }
            }
            key(&mut app, KeyCode::Right);
        }
        assert_eq!(app.help_chapter, first, "chapters wrap round");
        key(&mut app, KeyCode::Esc);
        assert!(!app.show_help);
        // It opens on the chapter that fits: a station → Stations.
        let st = app.world.as_ref().unwrap().stations[0].pos;
        app.cursor = st;
        key(&mut app, KeyCode::Char('?'));
        assert_eq!(app.help_chapter, handbook::STATIONS);
    }

    /// Rebind a key on the Controls screen, use it, and find it saved.
    #[test]
    fn controls_screen_rebinds_and_saves() {
        let keys_path = std::env::temp_dir().join("rh_ui_keys_test.json");
        let _ = std::fs::remove_file(&keys_path);
        let mut app = App::new(
            Some("42".into()),
            Settings::default(),
            std::env::temp_dir().join("rh_ui_keys_save.json"),
        );
        app.keys_path = keys_path.clone();
        let mut term = Terminal::new(TestBackend::new(150, 48)).unwrap();
        key(&mut app, KeyCode::Char('?')); // title: controls
        assert!(app.controls.is_some());
        assert!(render(&mut app, &mut term).contains("CONTROLS"));
        // Walk down to "rail track tool" and bind it to w.
        let rail = keys::ACTS.iter().position(|a| a.act == Act::Rail).unwrap();
        for _ in 0..rail {
            key(&mut app, KeyCode::Char('j'));
        }
        key(&mut app, KeyCode::Enter);
        let screen = render(&mut app, &mut term);
        assert!(screen.contains("press a key"));
        if std::env::var("RH_PRINT").is_ok() {
            for row in screen.chars().collect::<Vec<_>>().chunks(150) {
                eprintln!("{}", row.iter().collect::<String>());
            }
        }
        key(&mut app, KeyCode::Char('w'));
        // Tab adds a second key, Backspace on another row resets it.
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Char('z'));
        assert_eq!(app.keys.labels(Act::Rail), "w / z");
        key(&mut app, KeyCode::Esc);
        assert!(app.controls.is_none());
        assert!(keys_path.exists(), "closing saves the bindings");
        let (back, problems) = keys::Keymap::load(&keys_path);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(back.labels(Act::Rail), "w / z");

        // In game, w now starts the rail tool and t does nothing.
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Char('t'));
        assert_eq!(app.mode, Mode::Normal);
        key(&mut app, KeyCode::Char('w'));
        assert!(matches!(
            app.mode,
            Mode::Line {
                kind: LayerKind::Rail,
                ..
            }
        ));
        key(&mut app, KeyCode::Esc);
        // The help screen and build menu show the new key.
        key(&mut app, KeyCode::Char('?'));
        let controls = &handbook::chapters(&app.keys)[handbook::CONTROLS];
        assert!(
            controls
                .lines
                .iter()
                .any(|l| l == "k w / z	rail track tool"),
            "the handbook shows the new rail key"
        );
        key(&mut app, KeyCode::Esc);
        // C opens Controls in game too; Ctrl+R resets everything.
        key(&mut app, KeyCode::Char('C'));
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
        );
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.keys.labels(Act::Rail), "t");
        let _ = std::fs::remove_file(&keys_path);
    }

    /// A window bigger than the map shows sea around it, never blank cells;
    /// the overview fits the whole map; tiny windows get a friendly note.
    #[test]
    fn any_window_size_draws_the_whole_screen() {
        use crate::map::MapSize;
        for (size, w, h) in [
            (MapSize::Small, 260u16, 80u16),
            (MapSize::Huge, 150, 46),
            (MapSize::Normal, 80, 24),
        ] {
            let mut app = App::new(
                Some("7".into()),
                Settings {
                    size,
                    ..Settings::default()
                },
                std::env::temp_dir().join("rh_ui_big.json"),
            );
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            app.title.size = size;
            key(&mut app, KeyCode::Enter);
            render(&mut app, &mut term);
            let m = app.map_area;
            let buf = term.backend().buffer().clone();
            for y in m.y..m.y + m.height {
                for x in m.x..m.x + m.width {
                    assert_ne!(
                        buf[(x, y)].bg,
                        ratatui::style::Color::Reset,
                        "{size:?} {w}x{h}: blank map cell at {x},{y}"
                    );
                }
            }
            key(&mut app, KeyCode::Char('z'));
            let screen = render(&mut app, &mut term);
            assert!(screen.contains("OVERVIEW"), "{size:?}: overview opens");
            let before = app.cursor;
            key(&mut app, KeyCode::Right);
            assert!(app.cursor.x > before.x, "moving works in the overview");
            key(&mut app, KeyCode::Esc);
            assert!(!app.show_overview);
        }
        let mut app = App::new(
            Some("7".into()),
            Settings::default(),
            std::env::temp_dir().join("rh_ui_small.json"),
        );
        let mut term = Terminal::new(TestBackend::new(60, 20)).unwrap();
        key(&mut app, KeyCode::Enter);
        assert!(render(&mut app, &mut term).contains("too small"));
    }

    /// Esc opens the game menu; quitting to the title autosaves, and
    /// Continue brings the game back. Deliveries float "+£" pops.
    #[test]
    fn menu_autosave_continue_and_pops() {
        let dir = std::env::temp_dir().join("rh_ui_menu");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = App::new(
            Some("42".into()),
            Settings::default(),
            dir.join("save.json"),
        );
        app.autosave_path = Some(dir.join("autosave.json"));
        app.prefs_path = Some(dir.join("settings.json"));
        let mut term = Terminal::new(TestBackend::new(150, 48)).unwrap();
        key(&mut app, KeyCode::Enter);
        // run until a starter truck gets paid, watching for the pop
        let mut popped = false;
        for _ in 0..6000 {
            app.world.as_mut().unwrap().tick();
            app.frame += 1;
            app.update_pops();
            if !app.pops.is_empty() {
                popped = true;
                app.cursor = app.pops[0].pos;
                assert!(
                    render(&mut app, &mut term).contains("+£"),
                    "the pop is drawn"
                );
                break;
            }
        }
        assert!(popped, "a delivery makes a pop");
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.menu, Some(0));
        assert!(app.reading(), "the clock holds while the menu is open");
        assert!(render(&mut app, &mut term).contains("Quit to title"));
        // Settings, from the menu: switch town labels off and turn the music
        // down, remembered in settings.json
        let row = |app: &App, item| menu_items(app).iter().position(|m| *m == item).unwrap();
        for _ in 0..row(&app, MenuItem::Settings) {
            key(&mut app, KeyCode::Down);
        }
        key(&mut app, KeyCode::Enter);
        assert!(
            app.settings.is_some() && app.menu.is_some(),
            "Settings opens over the menu"
        );
        let screen = render(&mut app, &mut term);
        assert!(screen.contains("SETTINGS") && screen.contains("Music volume"));
        assert!(
            !settings::items(&app).contains(&settings::Item::TextSize),
            "a terminal has its own text size"
        );
        let at = |app: &App, item| {
            settings::items(app)
                .iter()
                .position(|i| *i == item)
                .unwrap()
        };
        key(&mut app, KeyCode::Left); // music is the first row
        assert_eq!(
            app.audio.levels.music,
            B::MUSIC_PERCENT_DEFAULT - B::VOLUME_STEP
        );
        for _ in 0..at(&app, settings::Item::Labels) {
            key(&mut app, KeyCode::Down);
        }
        key(&mut app, KeyCode::Enter);
        assert!(!app.labels);
        let saved = prefs::Prefs::load(&dir.join("settings.json"));
        assert!(!saved.labels && saved.levels().music == B::MUSIC_PERCENT_DEFAULT - B::VOLUME_STEP);
        key(&mut app, KeyCode::Esc);
        assert!(
            app.settings.is_none() && app.menu.is_some(),
            "Esc goes back to the menu"
        );
        // quit to title: autosaved, and Continue is offered and works
        let time = app.world.as_ref().unwrap().time;
        for _ in row(&app, MenuItem::Settings)..row(&app, MenuItem::QuitToTitle) {
            key(&mut app, KeyCode::Down);
        }
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.screen, Screen::Title);
        assert!(dir.join("autosave.json").exists());
        assert!(title_rows(&app).contains(&TitleRow::Continue));
        let t = render(&mut app, &mut term);
        if !t.contains("Continue") {
            for row in t.chars().collect::<Vec<_>>().chunks(150) {
                eprintln!("{}", row.iter().collect::<String>());
            }
        }
        assert!(t.contains("Continue"));
        app.title.row = TitleRow::Continue;
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.screen, Screen::Game);
        assert!(
            (app.world.as_ref().unwrap().time - time).abs() < 0.01,
            "Continue loads the autosave"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The mouse: hover moves the cursor, a click builds, right click backs
    /// out, tabs and menus click, the title's options change on click.
    #[test]
    fn mouse_plays_too() {
        use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let mouse = |app: &mut App, kind: MouseEventKind, x: u16, y: u16| {
            handle_mouse(
                app,
                MouseEvent {
                    kind,
                    column: x,
                    row: y,
                    modifiers: KeyModifiers::NONE,
                },
            );
        };
        let find = |app: &App, want: Hit| {
            app.hits
                .borrow()
                .iter()
                .find(|(_, h)| *h == want)
                .map(|(r, _)| (r.x + 1, r.y))
                .unwrap()
        };
        let mut app = App::new(
            Some("42".into()),
            Settings::default(),
            std::env::temp_dir().join("rh_ui_mouse.json"),
        );
        let mut term = Terminal::new(TestBackend::new(150, 48)).unwrap();
        render(&mut app, &mut term);
        let (x, y) = find(&app, Hit::Title(TitleRow::Size));
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), x, y);
        assert_eq!(
            app.title.size,
            MapSize::Large,
            "clicking the size row changes it"
        );
        app.title.size = MapSize::Normal;
        render(&mut app, &mut term);
        let (x, y) = find(&app, Hit::Title(TitleRow::Play));
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), x, y);
        assert_eq!(app.screen, Screen::Game);
        render(&mut app, &mut term);
        // hover a map cell: the cursor follows the pointer
        let m = app.map_area;
        let (mx, my) = (m.x + 10, m.y + 5);
        mouse(&mut app, MouseEventKind::Moved, mx, my);
        let p = Pos::new(app.cam.x + 10, app.cam.y + 5);
        assert_eq!(app.cursor, p);
        // rail tool: two clicks lay track, right click leaves the tool
        let (a, b) = {
            let w = app.world.as_ref().unwrap();
            let mut found = None;
            'f: for dy in 2..m.height as i32 - 2 {
                for dx in 2..m.width as i32 - 12 {
                    let a = Pos::new(app.cam.x + dx, app.cam.y + dy);
                    let b = Pos::new(a.x + 8, a.y);
                    if build::plan_path(w, a, b, LayerKind::Rail).is_ok_and(|p| {
                        p.len() == 9
                            && p.iter().all(|q| {
                                !w.tile(*q).rail.present()
                                    && !w.tile(*q).road.present()
                                    && w.tile(*q).site.is_none()
                            })
                    }) {
                        found = Some((a, b));
                        break 'f;
                    }
                }
            }
            found.expect("somewhere to lay track")
        };
        let cam = app.cam;
        let at = |p: Pos| (m.x + (p.x - cam.x) as u16, m.y + (p.y - cam.y) as u16);
        key(&mut app, KeyCode::Char('t'));
        let (ax, ay) = at(a);
        let (bx, by) = at(b);
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), ax, ay);
        mouse(&mut app, MouseEventKind::Moved, bx, by);
        render(&mut app, &mut term); // preview follows the pointer
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), bx, by);
        assert!(
            app.world.as_ref().unwrap().tile(b).rail.present(),
            "click, click: track"
        );
        for _ in 0..2 {
            mouse(&mut app, MouseEventKind::Down(MouseButton::Right), bx, by);
            mouse(&mut app, MouseEventKind::Up(MouseButton::Right), bx, by);
        }
        assert_eq!(app.mode, Mode::Normal, "right click leaves the tool");
        // tabs click
        render(&mut app, &mut term);
        let (x, y) = find(&app, Hit::Tab(Tab::Contracts));
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), x, y);
        assert_eq!(app.tab, Tab::Contracts);
        // the menu and the handbook click
        key(&mut app, KeyCode::Esc);
        render(&mut app, &mut term);
        let hb = MENU.iter().position(|i| *i == MenuItem::Handbook).unwrap();
        let (x, y) = find(&app, Hit::MenuRow(hb));
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), x, y);
        assert!(app.show_help);
        render(&mut app, &mut term);
        let (x, y) = find(&app, Hit::Chapter(handbook::DEADLOCKS));
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), x, y);
        assert_eq!(app.help_chapter, handbook::DEADLOCKS);
        mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 0, 0);
        mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 0, 0);
        assert!(!app.show_help && app.menu.is_none());
        // overview: a click jumps there
        key(&mut app, KeyCode::Char('z'));
        render(&mut app, &mut term);
        let r = app
            .hits
            .borrow()
            .iter()
            .find(|(_, h)| matches!(h, Hit::Overview(_)))
            .map(|(r, _)| *r)
            .unwrap();
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            r.x + 1,
            r.y + 1,
        );
        assert!(!app.show_overview);
        assert!(
            app.cursor.x < 6 && app.cursor.y < 8,
            "jumped to the top-left corner: {:?}",
            app.cursor
        );
    }

    /// Resting the pointer on the map's edge scrolls the view that way; a key
    /// press hands control back to the keyboard.
    #[test]
    fn pointer_at_the_edge_scrolls() {
        use ratatui::crossterm::event::{MouseEvent, MouseEventKind};
        let mut app = App::new(
            Some("42".into()),
            Settings {
                size: MapSize::Huge,
                ..Settings::default()
            },
            std::env::temp_dir().join("rh_ui_edge.json"),
        );
        let mut term = Terminal::new(TestBackend::new(150, 46)).unwrap();
        app.title.size = MapSize::Huge;
        key(&mut app, KeyCode::Enter);
        app.cursor = Pos::new(40, 40);
        render(&mut app, &mut term);
        let m = app.map_area;
        let move_to = |app: &mut App, x: u16, y: u16| {
            handle_mouse(
                app,
                MouseEvent {
                    kind: MouseEventKind::Moved,
                    column: x,
                    row: y,
                    modifiers: KeyModifiers::NONE,
                },
            )
        };
        // just past the right edge (on the side panel's border)
        move_to(&mut app, m.x + m.width, m.y + m.height / 2);
        move_to(&mut app, m.x + m.width - 1, m.y + m.height / 2);
        let cam = app.cam;
        for _ in 0..20 {
            app.frame += 1;
            app.edge_scroll();
            render(&mut app, &mut term);
        }
        assert!(
            app.cam.x >= cam.x + 30,
            "scrolled right: {:?} → {:?}",
            cam,
            app.cam
        );
        assert_eq!(app.cam.y, cam.y, "only sideways");
        assert_eq!(
            app.cursor.x,
            app.cam.x + m.width as i32 - 1,
            "the cursor stays under the pointer"
        );
        // many loop turns in one frame (a fast mouse) still scroll only once
        let before = app.cursor;
        app.frame += 1;
        for _ in 0..10 {
            app.edge_scroll();
        }
        assert_eq!(
            app.cursor.x,
            before.x + EDGE_SPEED.0,
            "one step per frame, however fast the input"
        );
        // top-left corner scrolls both ways
        move_to(&mut app, m.x, m.y);
        let cam = app.cam;
        for _ in 0..10 {
            app.frame += 1;
            app.edge_scroll();
            render(&mut app, &mut term);
        }
        assert!(
            app.cam.x < cam.x && app.cam.y < cam.y,
            "scrolled up-left: {:?} → {:?}",
            cam,
            app.cam
        );
        // a key press stops it until the mouse moves again
        key(&mut app, KeyCode::Char('l'));
        render(&mut app, &mut term); // the keyboard's usual margin settles the view once
        let cam = app.cam;
        for _ in 0..10 {
            app.frame += 1;
            app.edge_scroll();
            render(&mut app, &mut term);
        }
        assert_eq!(app.cam, cam, "the keyboard has the cursor now");
        // the middle of the map doesn't scroll
        move_to(&mut app, m.x + m.width / 2, m.y + m.height / 2);
        let cam = app.cam;
        for _ in 0..10 {
            app.frame += 1;
            app.edge_scroll();
            render(&mut app, &mut term);
        }
        assert_eq!(app.cam, cam);
    }

    /// Holding the right (or middle) button on the map and dragging pans it,
    /// the grabbed tile staying under the pointer, without putting down the
    /// tool in hand; a right click that doesn't move still backs out.
    #[test]
    fn dragging_with_the_right_button_pans() {
        use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let mouse = |app: &mut App, kind: MouseEventKind, x: u16, y: u16| {
            handle_mouse(
                app,
                MouseEvent {
                    kind,
                    column: x,
                    row: y,
                    modifiers: KeyModifiers::NONE,
                },
            )
        };
        let mut app = App::new(
            Some("42".into()),
            Settings {
                size: MapSize::Huge,
                ..Settings::default()
            },
            std::env::temp_dir().join("rh_ui_grab.json"),
        );
        let mut term = Terminal::new(TestBackend::new(150, 46)).unwrap();
        app.title.size = MapSize::Huge;
        key(&mut app, KeyCode::Enter);
        app.cursor = Pos::new(90, 60);
        app.center_view = true;
        render(&mut app, &mut term);
        let m = app.map_area;
        let (cx, cy) = (m.x + m.width / 2, m.y + m.height / 2);
        key(&mut app, KeyCode::Char('t'));
        assert_ne!(app.mode, Mode::Normal);
        for (button, zoom) in [
            (MouseButton::Right, view::MapZoom::Normal),
            (MouseButton::Middle, view::MapZoom::Close),
        ] {
            settings::set_map_zoom(&mut app, zoom);
            render(&mut app, &mut term);
            let (cam, under) = (app.cam, app.view().tile_at(cx, cy).unwrap());
            mouse(&mut app, MouseEventKind::Down(button), cx, cy);
            // drag the map up and to the left: we see more to the right and below
            for step in 1..=10u16 {
                mouse(
                    &mut app,
                    MouseEventKind::Drag(button),
                    cx - step * 2,
                    cy - step,
                );
                render(&mut app, &mut term);
            }
            assert!(
                app.cam.x > cam.x && app.cam.y > cam.y,
                "{button:?} {zoom:?}: panned {cam:?} → {:?}",
                app.cam
            );
            assert_eq!(
                app.view().tile_at(cx - 20, cy - 10),
                Some(under),
                "the grabbed tile stays under the pointer"
            );
            assert_eq!(app.cursor, under);
            mouse(&mut app, MouseEventKind::Up(button), cx - 20, cy - 10);
            assert_ne!(app.mode, Mode::Normal, "dragging doesn't put the tool down");
            // and the edge doesn't scroll on its own once let go in the middle
            let cam = app.cam;
            app.frame += 1;
            app.edge_scroll();
            assert_eq!(app.cam, cam);
        }
        // a middle click that doesn't move does nothing; a right click backs out
        mouse(&mut app, MouseEventKind::Down(MouseButton::Middle), cx, cy);
        mouse(&mut app, MouseEventKind::Up(MouseButton::Middle), cx, cy);
        assert_ne!(app.mode, Mode::Normal);
        mouse(&mut app, MouseEventKind::Down(MouseButton::Right), cx, cy);
        mouse(&mut app, MouseEventKind::Up(MouseButton::Right), cx, cy);
        assert_eq!(
            app.mode,
            Mode::Normal,
            "a still right click leaves the tool"
        );
    }

    /// A key never silently does nothing because a tool or the schedule
    /// editor is open (playtest: "s doesn't work, nothing happens").
    #[test]
    fn map_keys_work_from_any_tool() {
        let mut app = App::new(
            Some("42".into()),
            Settings::default(),
            std::env::temp_dir().join("rh_ui_modes.json"),
        );
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Char('t'));
        app.cursor = Pos::new(app.cursor.x + 3, app.cursor.y);
        key(&mut app, KeyCode::Enter);
        assert!(matches!(app.mode, Mode::Line { start: Some(_), .. }));
        key(&mut app, KeyCode::Char('s'));
        assert_eq!(
            app.mode,
            Mode::Place(Place::Station),
            "s leaves the rail tool for the station tool"
        );
        key(&mut app, KeyCode::Char('x'));
        assert_eq!(app.mode, Mode::Place(Place::Demolish));
        key(&mut app, KeyCode::Char('r'));
        assert!(matches!(
            app.mode,
            Mode::Line {
                kind: LayerKind::Road,
                start: None
            }
        ));
        // the schedule editor: pause and moving work with it open, a map key closes it
        key(&mut app, KeyCode::Esc);
        let truck = app.world.as_ref().unwrap().trucks[0].carrier.id;
        app.selected = Some(truck);
        key(&mut app, KeyCode::Char('e'));
        assert!(matches!(app.mode, Mode::Schedule { picking: false, .. }));
        let (speed, cursor) = (app.speed, app.cursor);
        key(&mut app, KeyCode::Char(' '));
        key(&mut app, KeyCode::Left);
        assert_ne!(app.speed, speed, "space pauses with the editor open");
        assert_ne!(app.cursor, cursor);
        assert!(matches!(app.mode, Mode::Schedule { .. }));
        key(&mut app, KeyCode::Char('s'));
        assert_eq!(app.mode, Mode::Place(Place::Station));
    }

    /// Holding the demolish key while moving clears every tile on the way;
    /// letting go stops it. Holding it still keeps clearing the tile.
    #[test]
    fn hold_demolish_to_bulldoze() {
        let mut app = App::new(
            Some("42".into()),
            Settings::default(),
            std::env::temp_dir().join("rh_ui_dozer.json"),
        );
        key(&mut app, KeyCode::Enter);
        let line: Vec<Pos> = {
            let w = app.world.as_mut().unwrap();
            let home = w.towns[w.map.home].pos;
            let (a, b) = (0..40)
                .flat_map(|dy| [-dy, dy])
                .map(|dy| {
                    (
                        Pos::new(home.x - 20, home.y + dy),
                        Pos::new(home.x - 12, home.y + dy),
                    )
                })
                .find(|(a, b)| {
                    build::plan_path(w, *a, *b, LayerKind::Rail)
                        .is_ok_and(|p| p.len() == 9 && p.iter().all(|q| q.y == a.y))
                })
                .expect("a straight line to lay");
            let p = build::plan_path(w, a, b, LayerKind::Rail).unwrap();
            build::commit_path(w, &p, LayerKind::Rail).unwrap();
            p
        };
        let rail_left = |app: &App| {
            line.iter()
                .filter(|p| app.world.as_ref().unwrap().tile(**p).rail.present())
                .count()
        };
        let ev = |code, kind| Event::Key(KeyEvent::new_with_kind(code, KeyModifiers::NONE, kind));
        app.releases = true;
        app.cursor = line[0];
        handle_event(&mut app, ev(KeyCode::Char('x'), KeyEventKind::Press));
        assert_eq!(app.mode, Mode::Place(Place::Demolish));
        assert_eq!(rail_left(&app), 9, "picking the tool demolishes nothing");
        handle_event(&mut app, ev(KeyCode::Char('x'), KeyEventKind::Repeat));
        assert_eq!(
            rail_left(&app),
            8,
            "the key repeating clears the tile under the cursor"
        );
        for _ in 0..4 {
            key(&mut app, KeyCode::Right);
        }
        assert_eq!(rail_left(&app), 4, "moving with the key held bulldozes");
        handle_event(&mut app, ev(KeyCode::Char('x'), KeyEventKind::Release));
        key(&mut app, KeyCode::Right);
        assert_eq!(rail_left(&app), 4, "let go: just moving");
        // dragging the mouse with the tool does it too
        app.selected = None;
        let m = app.map_area;
        app.map_area = Rect::new(0, 0, 200, 80);
        app.cam = Pos::new(0, 0);
        let mouse = |app: &mut App, kind, p: Pos| {
            handle_mouse(
                app,
                MouseEvent {
                    kind,
                    column: p.x as u16,
                    row: p.y as u16,
                    modifiers: KeyModifiers::NONE,
                },
            )
        };
        let here = app.cursor;
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), here);
        mouse(
            &mut app,
            MouseEventKind::Drag(MouseButton::Left),
            *line.last().unwrap(),
        );
        assert_eq!(rail_left(&app), 0);
        app.map_area = m;
    }

    /// The text size keys and menu rows ask the window to zoom; a terminal says to use its own.
    #[test]
    fn zoom_keys_ask_the_window() {
        let mut app = App::new(
            Some("42".into()),
            Settings::default(),
            std::env::temp_dir().join("rh_ui_zoom.json"),
        );
        let ctrl = |app: &mut App, c| {
            handle_key(app, KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
        };
        ctrl(&mut app, '=');
        assert_eq!(app.zoom, None);
        assert!(app.msg.as_ref().unwrap().0.contains("terminal"));
        app.windowed = true;
        ctrl(&mut app, '='); // works on the title screen too
        assert_eq!(app.zoom.take(), Some(1));
        key(&mut app, KeyCode::Enter);
        ctrl(&mut app, '-');
        assert_eq!(app.zoom.take(), Some(-1));
        ctrl(&mut app, '0');
        assert_eq!(app.zoom.take(), Some(0));
        // Settings has a text size row (◀ ▶), and stays open for the next press
        key(&mut app, KeyCode::Esc);
        settings::open(&mut app);
        let row = settings::items(&app)
            .iter()
            .position(|i| *i == settings::Item::TextSize)
            .unwrap();
        for _ in 0..row {
            key(&mut app, KeyCode::Down);
        }
        key(&mut app, KeyCode::Left);
        assert_eq!(app.zoom.take(), Some(-1));
        assert!(app.settings.is_some());
        // the map zooms on its own, and the text doesn't
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Esc);
        assert!(app.menu.is_none() && app.settings.is_none());
        key(&mut app, KeyCode::Char('.'));
        assert_eq!(app.map_zoom, view::MapZoom::Close);
        assert_eq!(app.zoom, None, "map zoom leaves the text alone");
        key(&mut app, KeyCode::Char(','));
        key(&mut app, KeyCode::Char(','));
        assert_eq!(app.map_zoom, view::MapZoom::Far);
    }

    /// Towns and industries both show exactly how much they'd take now.
    #[test]
    fn panels_show_room_to_unload() {
        let mut app = App::new(
            Some("42".into()),
            Settings::default(),
            std::env::temp_dir().join("rh_ui_room.json"),
        );
        let mut term = Terminal::new(TestBackend::new(160, 48)).unwrap();
        key(&mut app, KeyCode::Enter);
        let w = app.world.as_ref().unwrap();
        let mill = w
            .sites
            .iter()
            .find(|s| matches!(s.kind, SiteKind::Industry(crate::map::IndustryKind::Mill)))
            .unwrap()
            .pos;
        app.cursor = w.towns[w.map.home].pos;
        let screen = render(&mut app, &mut term);
        assert!(
            screen.contains("Room to unload now:") && screen.contains("FOOD"),
            "the town's room"
        );
        app.cursor = mill;
        let screen = render(&mut app, &mut term);
        assert!(
            screen.contains("Room to unload now:")
                && screen.contains("GRAN")
                && screen.contains("/120"),
            "the mill's room for grain"
        );
    }

    /// The schedule editor shows what each gate sees, and says when one can never open.
    #[test]
    fn schedule_shows_gate_readings() {
        let mut app = App::new(
            Some("42".into()),
            Settings::default(),
            std::env::temp_dir().join("rh_ui_gate.json"),
        );
        let mut term = Terminal::new(TestBackend::new(160, 48)).unwrap();
        key(&mut app, KeyCode::Enter);
        let id = {
            let w = app.world.as_mut().unwrap();
            let t = w.trucks.iter_mut().find(|t| t.carrier.schedule.stops.iter().any(|s| matches!(s.target, Target::Site(x) if matches!(w.sites[x as usize].kind, SiteKind::Town(_))))).unwrap();
            for s in t.carrier.schedule.stops.iter_mut() {
                s.gate = Some(if s.action == Action::Load {
                    crate::world::vehicles::Gate::Waiting(10.0)
                } else {
                    crate::world::vehicles::Gate::Wants(10_000.0)
                });
            }
            t.carrier.id
        };
        app.selected = Some(id);
        app.mode = Mode::Schedule {
            vehicle: id,
            sel: 0,
            picking: false,
        };
        let screen = render(&mut app, &mut term);
        if std::env::var("RH_PRINT").is_ok() {
            for row in screen.chars().collect::<Vec<_>>().chunks(160).skip(36) {
                eprintln!("{}", row.iter().collect::<String>());
            }
        }
        assert!(
            screen.contains("waiting (holds at most"),
            "a pick-up gate's reading"
        );
        assert!(
            screen.contains("needs 10000: NEVER opens"),
            "a gate the town can't meet says so"
        );
    }

    /// Every map zoom draws, and the mouse finds the tile it points at.
    #[test]
    fn map_zoom_draws_and_clicks() {
        use ratatui::crossterm::event::{MouseEvent, MouseEventKind};
        let mut app = App::new(
            Some("42".into()),
            Settings::default(),
            std::env::temp_dir().join("rh_ui_mapzoom.json"),
        );
        let mut term = Terminal::new(TestBackend::new(150, 48)).unwrap();
        key(&mut app, KeyCode::Enter);
        for z in view::MapZoom::ALL {
            settings::set_map_zoom(&mut app, z);
            let screen = render(&mut app, &mut term);
            assert!(screen.contains("RAILHEAD"), "{z:?} draws");
            let m = app.map_area;
            handle_mouse(
                &mut app,
                MouseEvent {
                    kind: MouseEventKind::Moved,
                    column: m.x + 9,
                    row: m.y + 5,
                    modifiers: KeyModifiers::NONE,
                },
            );
            let want = app.view().tile_at(m.x + 9, m.y + 5).unwrap();
            assert_eq!(app.cursor, want, "{z:?}: the cursor is under the pointer");
            assert_eq!(app.view().tile_at_offset(9, 5), want);
        }
        // the wheel zooms in and out over the map
        settings::set_map_zoom(&mut app, view::MapZoom::Normal);
        let m = app.map_area;
        handle_mouse(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: m.x + 20,
                row: m.y + 10,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert_eq!(app.map_zoom, view::MapZoom::Close);
        handle_mouse(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: m.x + 20,
                row: m.y + 10,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert_eq!(app.map_zoom, view::MapZoom::Normal);
    }

    /// The Build tab's rows are buttons, and so are the Vehicles tab's.
    #[test]
    fn bottom_tabs_are_clickable() {
        let mut app = App::new(
            Some("42".into()),
            Settings::default(),
            std::env::temp_dir().join("rh_ui_click.json"),
        );
        let mut term = Terminal::new(TestBackend::new(150, 48)).unwrap();
        key(&mut app, KeyCode::Enter);
        let click = |app: &mut App, want: Hit| {
            let (x, y) = app
                .hits
                .borrow()
                .iter()
                .find(|(_, h)| *h == want)
                .map(|(r, _)| (r.x + 2, r.y))
                .expect("on screen");
            handle_mouse(
                app,
                MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: x,
                    row: y,
                    modifiers: KeyModifiers::NONE,
                },
            );
        };
        app.tab = Tab::Build;
        render(&mut app, &mut term);
        click(&mut app, Hit::Act(Act::Station));
        assert_eq!(app.mode, Mode::Place(Place::Station));
        render(&mut app, &mut term);
        click(&mut app, Hit::Act(Act::Rail));
        assert!(matches!(
            app.mode,
            Mode::Line {
                kind: LayerKind::Rail,
                start: None
            }
        ));
        render(&mut app, &mut term);
        click(&mut app, Hit::Act(Act::Rail));
        assert!(
            matches!(app.mode, Mode::Line { start: None, .. }),
            "clicking the tool again doesn't start a line off-map"
        );
        app.tab = Tab::Vehicles;
        render(&mut app, &mut term);
        let truck = app.world.as_ref().unwrap().trucks[1].carrier.id;
        click(&mut app, Hit::Vehicle(truck));
        assert_eq!(app.selected, Some(truck));
    }

    /// Zoomed in (a small grid), the side panel says when there's more and scrolls.
    #[test]
    fn side_panel_scrolls_when_it_overflows() {
        let mut app = App::new(
            Some("42".into()),
            Settings::default(),
            std::env::temp_dir().join("rh_ui_side.json"),
        );
        let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
        key(&mut app, KeyCode::Enter);
        let w = app.world.as_ref().unwrap();
        app.cursor = w.towns[w.map.home].pos;
        app.selected = Some(w.trucks[0].carrier.id);
        let screen = render(&mut app, &mut term);
        assert!(app.side_max > 0, "a truck and a town don't fit in 80x24");
        assert!(screen.contains("more"), "the panel says there is more");
        key(&mut app, KeyCode::Char('w'));
        assert!(app.side_scroll > 0);
        render(&mut app, &mut term);
        while app.side_scroll > 0 {
            key(&mut app, KeyCode::Char('w'));
            render(&mut app, &mut term);
        }
        // looking somewhere else starts at the top again
        key(&mut app, KeyCode::Char('w'));
        key(&mut app, KeyCode::Right);
        render(&mut app, &mut term);
        assert_eq!(app.side_scroll, 0);
    }

    #[test]
    fn calendar_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert!(daily_seed().starts_with("daily-20"));
    }

    #[test]
    fn charter_ends_and_shows_the_score() {
        let mut app = App::new(
            Some("9".into()),
            Settings::default(),
            std::env::temp_dir().join("rh_ui_test3.json"),
        );
        let mut term = Terminal::new(TestBackend::new(150, 48)).unwrap();
        key(&mut app, KeyCode::Enter);
        let w = app.world.as_mut().unwrap();
        while !w.finished {
            w.tick();
        }
        assert!((w.time - B::CHARTER_MINUTES * 60.0).abs() < 1.0);
        let screen = render(&mut app, &mut term);
        assert!(screen.contains("C H A R T E R"), "end screen should show");
        key(&mut app, KeyCode::Char('c'));
        let screen = render(&mut app, &mut term);
        assert!(!screen.contains("C H A R T E R"), "c closes the end screen");
    }

    /// The title cycles to Franchise; the review clock shows, and losing it
    /// shows the end screen and goes in the records.
    #[test]
    fn franchise_is_lost_and_recorded() {
        let mut app = App::new(
            Some("9".into()),
            Settings::default(),
            std::env::temp_dir().join("rh_ui_fr.json"),
        );
        let mut term = Terminal::new(TestBackend::new(150, 48)).unwrap();
        while app.title.row != TitleRow::Mode {
            key(&mut app, KeyCode::Down);
        }
        key(&mut app, KeyCode::Right);
        assert_eq!(app.title.mode, GameMode::Franchise);
        assert!(render(&mut app, &mut term).contains("Railway Board reviews you"));
        while app.title.row != TitleRow::Play {
            key(&mut app, KeyCode::Up);
        }
        key(&mut app, KeyCode::Enter);
        assert!(app.world.as_ref().unwrap().franchise());
        app.fx.headlines.clear();
        assert!(render(&mut app, &mut term).contains("review 1 in"));
        let w = app.world.as_mut().unwrap();
        while !w.finished {
            w.tick();
        }
        app.record_result();
        let screen = render(&mut app, &mut term);
        assert!(
            screen.contains("F R A N C H I S E"),
            "end screen should show"
        );
        assert_eq!((app.records.franchises, app.records.charters), (1, 0));
    }

    /// Drives the real key handlers and renderer end to end.
    #[test]
    fn smoke_ui_session() {
        let mut app = App::new(
            Some("42".into()),
            Settings::default(),
            std::env::temp_dir().join("rh_ui_test.json"),
        );
        let mut term = Terminal::new(TestBackend::new(150, 48)).unwrap();
        let title = render(&mut app, &mut term);
        if std::env::var("RH_PRINT").is_ok_and(|v| v == "title") {
            for row in title.chars().collect::<Vec<_>>().chunks(150) {
                eprintln!("{}", row.iter().collect::<String>());
            }
        }
        assert!(title.contains("Seed") && title.contains("New game") && title.contains("Settings"));
        key(&mut app, KeyCode::Enter); // start
        assert_eq!(app.screen, Screen::Game);
        let screen = render(&mut app, &mut term);
        assert!(screen.contains("RAILHEAD") && screen.contains("seed 42"));

        // Road from the capital depot 6 tiles east.
        let cap = app.cursor;
        key(&mut app, KeyCode::Char('r'));
        key(&mut app, KeyCode::Enter);
        for _ in 0..6 {
            key(&mut app, KeyCode::Right);
        }
        render(&mut app, &mut term); // preview path draws
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Esc);
        assert!(
            app.world
                .as_ref()
                .unwrap()
                .tile(Pos::new(cap.x + 3, cap.y))
                .road
                .present()
        );

        // Buy a truck at the capital and give it a stop.
        app.cursor = cap;
        let trucks = app.world.as_ref().unwrap().trucks.len();
        key(&mut app, KeyCode::Char('n'));
        assert_eq!(app.world.as_ref().unwrap().trucks.len(), trucks + 1);
        assert!(
            matches!(app.mode, Mode::Schedule { picking: true, .. }),
            "a new truck loads here and asks where it delivers"
        );
        key(&mut app, KeyCode::Esc); // edit the one-stop schedule instead
        key(&mut app, KeyCode::Char('f'));
        key(&mut app, KeyCode::Char('c'));
        render(&mut app, &mut term);
        key(&mut app, KeyCode::Esc);
        let id = app.selected.unwrap();
        assert_eq!(
            app.world
                .as_ref()
                .unwrap()
                .carrier(id)
                .unwrap()
                .0
                .schedule
                .stops
                .len(),
            1
        );

        // Run time, cycle overlays, open help, toggle palette.
        key(&mut app, KeyCode::Char(' '));
        for _ in 0..300 {
            app.world.as_mut().unwrap().tick();
        }
        for _ in 0..4 {
            key(&mut app, KeyCode::Char('o'));
            render(&mut app, &mut term);
        }
        key(&mut app, KeyCode::Char('P'));
        key(&mut app, KeyCode::Tab);
        render(&mut app, &mut term);
        key(&mut app, KeyCode::Tab);
        render(&mut app, &mut term);
        key(&mut app, KeyCode::Char('?'));
        let help = render(&mut app, &mut term);
        assert!(help.contains("RAILWAY HANDBOOK"));
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Char('i'));
        let stats = render(&mut app, &mut term);
        assert!(stats.contains("STATISTICS") && stats.contains("Prosperity"));
        key(&mut app, KeyCode::Esc);
        assert!(!app.show_stats);
        key(&mut app, KeyCode::Char('a'));
        render(&mut app, &mut term);
    }
}
