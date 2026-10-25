//! Key bindings. Every key the game reacts to is an `Act` with a name, a
//! group (the screen or mode it works in) and default keys. Players can
//! rebind them on the Controls screen or in `keys.json` in the data folder (`platform::data_dir`).
//!
//! Defaults stick to keys every keyboard has, compact ones included: letters,
//! digits, Space, Enter, Esc, Tab, Backspace, arrows (with `hjkl` as a
//! fallback) and Ctrl+letter. F-keys, Home, PgUp/PgDn, Delete and `[ ]` are
//! only extra bindings.
//!
//! UI text names keys through placeholders: `keys.fill("press {rail}")` gives
//! "press t", or whatever the player bound. `{act+}` lists every key.

use std::{collections::BTreeMap, path::Path};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    /// Every screen (window keys like the text size).
    Anywhere,
    Title,
    Move,
    Game,
    Map,
    Tool,
    Schedule,
    Confirm,
    End,
    Help,
    Menu,
}

impl Group {
    pub fn name(self) -> &'static str {
        match self {
            Group::Anywhere => "Anywhere (text size works in the game's window)",
            Group::Title => "Title screen",
            Group::Move => "Moving the cursor",
            Group::Game => "Anywhere in the game",
            Group::Map => "On the map (no tool active)",
            Group::Tool => "While building (track, road, stations...) and picking stops",
            Group::Schedule => "Schedule editor",
            Group::Confirm => "Yes/no questions",
            Group::End => "End of the Charter",
            Group::Help => "Railway Handbook",
            Group::Menu => "Menus and Settings",
        }
    }
}

/// Groups that are live at the same time: a key may only mean one thing in each.
const LIVE_TOGETHER: &[&[Group]] = &[
    &[Group::Anywhere, Group::Title],
    &[Group::Anywhere, Group::Move, Group::Game, Group::Map],
    &[Group::Anywhere, Group::Move, Group::Game, Group::Tool],
    &[Group::Anywhere, Group::Schedule],
    &[Group::Anywhere, Group::Confirm],
    &[Group::Anywhere, Group::End],
    &[Group::Anywhere, Group::Help],
    &[Group::Anywhere, Group::Menu],
];

macro_rules! acts {
    ($($v:ident $name:literal $group:ident $desc:literal [$($k:literal),*];)*) => {
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum Act { $($v),* }
        pub struct ActInfo {
            pub act: Act,
            pub name: &'static str,
            pub group: Group,
            pub desc: &'static str,
            pub defaults: &'static [&'static str],
        }
        pub static ACTS: &[ActInfo] = &[
            $(ActInfo { act: Act::$v, name: $name, group: Group::$group, desc: $desc, defaults: &[$($k),*] }),*
        ];
    };
}

acts! {
    ZoomIn "zoom_in" Anywhere "bigger text (all of it; the map has its own zoom)" ["ctrl+=", "ctrl++"];
    ZoomOut "zoom_out" Anywhere "smaller text" ["ctrl+-"];
    ZoomReset "zoom_reset" Anywhere "normal text size" ["ctrl+0"];

    TitleStart "title_start" Title "start the game" ["enter"];
    TitleNext "title_next_mode" Title "change the chosen option (mode, map size) forward" ["right", "tab"];
    TitlePrev "title_prev_mode" Title "change the chosen option back" ["left", "backtab"];
    TitleUp "title_up" Title "menu: up" ["up"];
    TitleDown "title_down" Title "menu: down" ["down"];
    TitleDaily "title_daily" Title "use today's daily seed" ["ctrl+d", "f2"];
    TitleLoad "title_load" Title "load the saved game" ["ctrl+l", "f9"];
    TitleControls "title_controls" Title "change controls" ["?", "ctrl+k"];
    TitleQuit "title_quit" Title "quit" ["esc"];

    MoveUp "move_up" Move "cursor up" ["up", "k"];
    MoveDown "move_down" Move "cursor down" ["down", "j"];
    MoveLeft "move_left" Move "cursor left" ["left", "h"];
    MoveRight "move_right" Move "cursor right" ["right", "l"];
    MoveUpFast "move_up_fast" Move "cursor up 8 tiles" ["K", "shift+up", "ctrl+up", "pageup"];
    MoveDownFast "move_down_fast" Move "cursor down 8 tiles" ["J", "shift+down", "ctrl+down", "pagedown"];
    MoveLeftFast "move_left_fast" Move "cursor left 8 tiles" ["H", "shift+left", "ctrl+left"];
    MoveRightFast "move_right_fast" Move "cursor right 8 tiles" ["L", "shift+right", "ctrl+right"];

    Pause "pause" Game "pause / resume (you can build while paused)" ["space"];
    Speed1 "speed_1" Game "speed 1x" ["1"];
    Speed2 "speed_2" Game "speed 2x" ["2"];
    Speed3 "speed_3" Game "speed 4x" ["3"];
    Speed4 "speed_4" Game "speed 8x" ["4"];
    Help "help" Game "Railway Handbook (the guide)" ["?", "f1"];
    Controls "controls" Game "change controls" ["C"];
    Stats "stats" Game "statistics screen" ["i"];
    Overlay "overlay" Game "overlay: congestion → flow → towns → none" ["o"];
    Palette "palette" Game "color-blind palette on/off" ["P"];
    Labels "labels" Game "town labels on/off" ["N"];
    Terrain "terrain" Game "terrain marks on/off" ["T"];
    Overview "overview" Game "whole-map overview" ["z"];
    MapZoomIn "map_zoom_in" Game "zoom the map in (text stays the same size)" ["."];
    MapZoomOut "map_zoom_out" Game "zoom the map out" [","];
    KeyPanel "key_panel" Game "keys panel (left of the map) on/off" ["/"];
    Sound "sound" Game "mute / unmute all sound (volumes are in Settings)" ["M"];
    NextTab "next_tab" Game "bottom panel: Alerts → Build → Vehicles → Contracts" ["tab"];
    Save "save" Game "save" ["ctrl+s", "f5"];
    Load "load" Game "load" ["ctrl+l", "f9"];
    AlertLatest "alert_latest" Game "jump to the latest alert" ["a"];
    AlertOlder "alert_older" Game "step back through older alerts" ["A"];
    HomeTown "home" Game "jump to your home town (★)" ["b", "home"];
    SideMore "side_more" Game "side panel: show more when it doesn't fit (back to the top at the end)" ["w"];
    TutorialSkip "tutorial_skip" Game "tutorial: next step (skips this one) / close the guide" ["ctrl+t", "f3"];
    TutorialBack "tutorial_back" Game "tutorial: back a step, to read it again" ["ctrl+b", "f4"];

    Quit "quit" Map "quit" ["q"];
    NewGame "new_game" Map "new game" ["Q"];
    Road "road" Map "road tool" ["r"];
    Rail "rail" Map "rail track tool" ["t"];
    Double "double" Map "double-track tool" ["D"];
    Station "station" Map "place stations" ["s"];
    Signal "signal" Map "place / cycle signals" ["g"];
    Hub "hub" Map "place transfer hubs" ["u"];
    Demolish "demolish" Map "demolish tool, or scrap the selected vehicle" ["x", "delete", "backspace"];
    Chain "chain" Map "switch a signal between block and chain" ["c"];
    AutoSignal "auto_signal" Map "auto-signal the network under the cursor" ["G"];
    Select "select" Map "select the vehicle under the cursor" ["enter"];
    Deselect "deselect" Map "deselect, or open the game menu" ["esc"];
    BuyFreight "buy_freight" Map "buy a truck (at a site) or freight train (at a station)" ["n"];
    BuyExpress "buy_express" Map "buy an express train (at a station)" ["m"];
    Edit "edit" Map "edit the selected vehicle's schedule" ["e"];
    Duplicate "duplicate" Map "buy another of the selected vehicle, same schedule" ["d"];
    NextVehicle "next_vehicle" Map "select the next vehicle" ["v"];
    PrevVehicle "prev_vehicle" Map "select the previous vehicle" ["V"];
    Reverse "reverse" Map "reverse the selected train" ["R"];
    More "more" Map "one more wagon (train selected) / train limit (station)" ["+", "=", "]"];
    Less "less" Map "one less wagon / train limit" ["-", "["];

    Confirm "confirm" Tool "place / build here / pick this stop" ["enter"];
    Cancel "cancel" Tool "cancel / leave the tool" ["esc"];

    SchedUp "sched_up" Schedule "previous stop" ["up", "k"];
    SchedDown "sched_down" Schedule "next stop" ["down", "j"];
    SchedAdd "sched_add" Schedule "add a stop (then pick it on the map)" ["a"];
    SchedDelete "sched_delete" Schedule "delete the stop" ["d", "delete"];
    SchedAction "sched_action" Schedule "load → unload → drop off + pick up" ["l"];
    SchedCargo "sched_cargo" Schedule "next cargo filter" ["f"];
    SchedCargoBack "sched_cargo_back" Schedule "previous cargo filter" ["F"];
    SchedCond "sched_cond" Schedule "leave condition: full → empty → time" ["c"];
    SchedTimeout "sched_timeout" Schedule "timeout on/off" ["t"];
    SchedTimeMore "sched_time_more" Schedule "+5 seconds" ["+", "="];
    SchedTimeLess "sched_time_less" Schedule "-5 seconds" ["-"];
    SchedGate "sched_gate" Schedule "only-if gate: waiting → wants → off" ["o"];
    SchedGateMore "sched_gate_more" Schedule "raise the gate amount" [">", "."];
    SchedGateLess "sched_gate_less" Schedule "lower the gate amount" ["<", ","];
    SchedMoveUp "sched_move_up" Schedule "move the stop up" ["K"];
    SchedMoveDown "sched_move_down" Schedule "move the stop down" ["J"];
    SchedGo "sched_go" Schedule "go to this stop now" ["g"];
    SchedClose "sched_close" Schedule "close the editor" ["esc", "e"];

    Yes "yes" Confirm "yes (any other key is no)" ["y", "Y", "enter"];

    EndNew "end_new" End "new game" ["n", "enter"];
    EndClose "end_close" End "close and look at your network" ["esc", "c"];
    EndQuit "end_quit" End "quit" ["q"];

    HelpUp "help_up" Help "scroll up" ["up", "k"];
    HelpDown "help_down" Help "scroll down" ["down", "j"];
    HelpPageUp "help_page_up" Help "scroll up a page" ["K", "pageup", "shift+up"];
    HelpPageDown "help_page_down" Help "scroll down a page" ["J", "pagedown", "shift+down"];
    HelpPrevChapter "help_prev_chapter" Help "previous chapter" ["left", "h", "backtab"];
    HelpNextChapter "help_next_chapter" Help "next chapter" ["right", "l", "tab"];

    MenuUp "menu_up" Menu "up" ["up", "k"];
    MenuDown "menu_down" Menu "down" ["down", "j"];
    MenuLeft "menu_left" Menu "Settings: turn down / previous choice" ["left", "h"];
    MenuRight "menu_right" Menu "Settings: turn up / next choice" ["right", "l"];
    MenuPick "menu_pick" Menu "choose / switch on or off" ["enter", "space"];
    MenuClose "menu_close" Menu "close the menu" ["esc"];
}

impl Act {
    pub fn info(self) -> &'static ActInfo {
        &ACTS[self as usize]
    }
}

/// One key, with the modifiers that matter. For characters, Shift is part of
/// the character itself ('A', '?'), so only Ctrl and Alt are kept.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Key {
    pub code: KeyCode,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

const NAMED: &[(&str, KeyCode, &str)] = &[
    ("space", KeyCode::Char(' '), "Space"),
    ("enter", KeyCode::Enter, "Enter"),
    ("esc", KeyCode::Esc, "Esc"),
    ("tab", KeyCode::Tab, "Tab"),
    ("backtab", KeyCode::BackTab, "Shift+Tab"),
    ("backspace", KeyCode::Backspace, "Backspace"),
    ("delete", KeyCode::Delete, "Del"),
    ("insert", KeyCode::Insert, "Ins"),
    ("up", KeyCode::Up, "↑"),
    ("down", KeyCode::Down, "↓"),
    ("left", KeyCode::Left, "←"),
    ("right", KeyCode::Right, "→"),
    ("home", KeyCode::Home, "Home"),
    ("end", KeyCode::End, "End"),
    ("pageup", KeyCode::PageUp, "PgUp"),
    ("pagedown", KeyCode::PageDown, "PgDn"),
];

impl Key {
    /// A key event that this key matches (the mouse "presses" bound keys with it).
    pub fn event(&self) -> KeyEvent {
        let mut m = KeyModifiers::NONE;
        if self.ctrl {
            m |= KeyModifiers::CONTROL;
        }
        if self.alt {
            m |= KeyModifiers::ALT;
        }
        if self.shift {
            m |= KeyModifiers::SHIFT;
        }
        KeyEvent::new(self.code, m)
    }

    /// Parse "a", "A", "?", "space", "ctrl+s", "shift+up", "f5".
    pub fn parse(s: &str) -> Option<Key> {
        let mut k = Key {
            code: KeyCode::Null,
            ctrl: false,
            alt: false,
            shift: false,
        };
        let mut rest = s;
        loop {
            let lower = rest.to_ascii_lowercase();
            if rest.chars().count() > 1 && lower.starts_with("ctrl+") {
                k.ctrl = true;
                rest = &rest[5..];
            } else if rest.chars().count() > 1 && lower.starts_with("alt+") {
                k.alt = true;
                rest = &rest[4..];
            } else if rest.chars().count() > 1 && lower.starts_with("shift+") {
                k.shift = true;
                rest = &rest[6..];
            } else {
                break;
            }
        }
        let mut chars = rest.chars();
        k.code = match (chars.next(), chars.next()) {
            (Some(c), None) => KeyCode::Char(if k.ctrl { c.to_ascii_lowercase() } else { c }),
            _ => {
                let lower = rest.to_ascii_lowercase();
                if let Some((_, code, _)) = NAMED.iter().find(|(n, _, _)| *n == lower) {
                    *code
                } else {
                    let n = lower
                        .strip_prefix('f')
                        .and_then(|n| n.parse::<u8>().ok())
                        .filter(|n| (1..=24).contains(n))?;
                    KeyCode::F(n)
                }
            }
        };
        if matches!(k.code, KeyCode::Char(_) | KeyCode::BackTab) {
            k.shift = false;
        }
        Some(k)
    }

    pub fn from_event(e: &KeyEvent) -> Key {
        let ctrl = e.modifiers.contains(KeyModifiers::CONTROL);
        let alt = e.modifiers.contains(KeyModifiers::ALT);
        match e.code {
            KeyCode::Char(c) => Key {
                code: KeyCode::Char(if ctrl { c.to_ascii_lowercase() } else { c }),
                ctrl,
                alt,
                shift: false,
            },
            KeyCode::BackTab => Key {
                code: KeyCode::BackTab,
                ctrl,
                alt,
                shift: false,
            },
            code => Key {
                code,
                ctrl,
                alt,
                shift: e.modifiers.contains(KeyModifiers::SHIFT),
            },
        }
    }

    pub fn matches(&self, e: &KeyEvent) -> bool {
        *self == Key::from_event(e)
    }

    fn mods(&self, out: &mut String, ctrl: &str, alt: &str, shift: &str) {
        if self.ctrl {
            out.push_str(ctrl);
        }
        if self.alt {
            out.push_str(alt);
        }
        if self.shift {
            out.push_str(shift);
        }
    }

    /// The config-file spelling ("ctrl+s"), which `parse` reads back.
    pub fn spec(&self) -> String {
        let mut s = String::new();
        self.mods(&mut s, "ctrl+", "alt+", "shift+");
        match self.code {
            KeyCode::Char(' ') => s.push_str("space"),
            KeyCode::Char(c) => s.push(c),
            KeyCode::F(n) => s.push_str(&format!("f{n}")),
            code => s.push_str(
                NAMED
                    .iter()
                    .find(|(_, c, _)| *c == code)
                    .map(|(n, _, _)| *n)
                    .unwrap_or("?"),
            ),
        }
        s
    }

    /// What the player sees ("Ctrl+S", "↑", "Space").
    pub fn label(&self) -> String {
        let mut s = String::new();
        self.mods(&mut s, "Ctrl+", "Alt+", "Shift+");
        match self.code {
            KeyCode::Char(' ') => s.push_str("Space"),
            KeyCode::Char(c) if self.ctrl || self.alt => s.push(c.to_ascii_uppercase()),
            KeyCode::Char(c) => s.push(c),
            KeyCode::F(n) => s.push_str(&format!("F{n}")),
            code => s.push_str(
                NAMED
                    .iter()
                    .find(|(_, c, _)| *c == code)
                    .map(|(_, _, l)| *l)
                    .unwrap_or("?"),
            ),
        }
        s
    }
}

#[derive(Clone, Debug)]
pub struct Keymap {
    keys: Vec<Vec<Key>>,
}

impl Default for Keymap {
    fn default() -> Keymap {
        Keymap {
            keys: ACTS.iter().map(|a| defaults(a.act)).collect(),
        }
    }
}

fn defaults(a: Act) -> Vec<Key> {
    a.info()
        .defaults
        .iter()
        .map(|s| Key::parse(s).expect("bad default key"))
        .collect()
}

impl Keymap {
    pub fn is(&self, a: Act, e: &KeyEvent) -> bool {
        self.keys[a as usize].iter().any(|k| k.matches(e))
    }

    /// The action of `group` this key is bound to, if any.
    pub fn act_in(&self, group: Group, e: &KeyEvent) -> Option<Act> {
        ACTS.iter()
            .filter(|i| i.group == group)
            .map(|i| i.act)
            .find(|a| self.is(*a, e))
    }

    pub fn keys(&self, a: Act) -> &[Key] {
        &self.keys[a as usize]
    }

    /// The first key bound to `a`, for hints. "(unbound)" if none.
    pub fn label(&self, a: Act) -> String {
        self.keys(a)
            .first()
            .map(|k| k.label())
            .unwrap_or_else(|| "(unbound)".into())
    }

    /// Every key bound to `a`: "? / F1".
    pub fn labels(&self, a: Act) -> String {
        if self.keys(a).is_empty() {
            return "(unbound)".into();
        }
        self.keys(a)
            .iter()
            .map(|k| k.label())
            .collect::<Vec<_>>()
            .join(" / ")
    }

    /// Replace `{act}` with its first key and `{act+}` with all its keys.
    /// Unknown names are left as they are.
    pub fn fill(&self, s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut rest = s;
        while let Some(i) = rest.find('{') {
            out.push_str(&rest[..i]);
            let after = &rest[i + 1..];
            let Some(j) = after.find('}') else {
                out.push_str(&rest[i..]);
                return out;
            };
            let name = &after[..j];
            let (base, all) = match name.strip_suffix('+') {
                Some(b) => (b, true),
                None => (name, false),
            };
            match ACTS.iter().find(|a| a.name == base) {
                Some(a) if all => out.push_str(&self.labels(a.act)),
                Some(a) => out.push_str(&self.label(a.act)),
                None => out.push_str(&rest[i..i + j + 2]),
            }
            rest = &after[j + 1..];
        }
        out.push_str(rest);
        out
    }

    /// Bind `key` to `a`, replacing its keys (or adding one when `add`). The
    /// key is taken off any other action live at the same time; those that end
    /// up with no key at all are returned so the player can be warned.
    pub fn bind(&mut self, a: Act, key: Key, add: bool) -> Vec<Act> {
        let mut orphaned = vec![];
        for other in ACTS
            .iter()
            .map(|i| i.act)
            .filter(|o| *o != a && clash(*o, a))
        {
            let ks = &mut self.keys[other as usize];
            let before = ks.len();
            ks.retain(|k| *k != key);
            if ks.is_empty() && before > 0 {
                orphaned.push(other);
            }
        }
        let ks = &mut self.keys[a as usize];
        if add {
            if !ks.contains(&key) {
                ks.push(key);
            }
        } else {
            *ks = vec![key];
        }
        orphaned
    }

    pub fn reset(&mut self, a: Act) {
        self.keys[a as usize] = defaults(a);
    }

    /// Pairs of actions that share a key while both are live.
    pub fn clashes(&self) -> Vec<(Act, Act, Key)> {
        let mut out = vec![];
        for (i, a) in ACTS.iter().enumerate() {
            for b in &ACTS[i + 1..] {
                if !clash(a.act, b.act) {
                    continue;
                }
                for k in self.keys(a.act) {
                    if self.keys(b.act).contains(k) {
                        out.push((a.act, b.act, *k));
                    }
                }
            }
        }
        out
    }

    /// Read bindings from a JSON file of `{"action": ["key", ...]}`. Actions
    /// missing from the file keep their defaults. Returns problems to show.
    pub fn load(path: &Path) -> (Keymap, Vec<String>) {
        let mut km = Keymap::default();
        let mut problems = vec![];
        let Ok(text) = std::fs::read_to_string(path) else {
            return (km, problems);
        };
        let map: BTreeMap<String, Vec<String>> = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                problems.push(format!(
                    "{} couldn't be read ({e}); using default keys.",
                    path.display()
                ));
                return (km, problems);
            }
        };
        for (name, specs) in map {
            let Some(info) = ACTS.iter().find(|a| a.name == name) else {
                problems.push(format!("unknown action \"{name}\""));
                continue;
            };
            let mut ks = vec![];
            for s in specs {
                match Key::parse(&s) {
                    Some(k) => ks.push(k),
                    None => problems.push(format!("unknown key \"{s}\" for {name}")),
                }
            }
            km.keys[info.act as usize] = ks;
        }
        for (a, b, k) in km.clashes() {
            problems.push(format!(
                "{} is bound to both {} and {}",
                k.label(),
                a.info().name,
                b.info().name
            ));
        }
        (km, problems)
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        // A BTreeMap would sort by name; keep the table order so the file reads like the Controls screen.
        let mut out = String::from("{\n");
        for (i, a) in ACTS.iter().enumerate() {
            let specs: Vec<String> = self
                .keys(a.act)
                .iter()
                .map(|k| serde_json::to_string(&k.spec()).unwrap())
                .collect();
            let comma = if i + 1 < ACTS.len() { "," } else { "" };
            out.push_str(&format!(
                "  \"{}\": [{}]{comma}\n",
                a.name,
                specs.join(", ")
            ));
        }
        out.push_str("}\n");
        std::fs::write(path, out)
    }
}

/// Are these two actions ever live at the same time?
fn clash(a: Act, b: Act) -> bool {
    let (ga, gb) = (a.info().group, b.info().group);
    LIVE_TOGETHER
        .iter()
        .any(|set| set.contains(&ga) && set.contains(&gb))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, m)
    }

    #[test]
    fn table_is_in_enum_order_and_defaults_parse() {
        for (i, a) in ACTS.iter().enumerate() {
            assert_eq!(a.act as usize, i, "{} out of order", a.name);
            assert!(!a.defaults.is_empty(), "{} has no default key", a.name);
            for d in a.defaults {
                let k =
                    Key::parse(d).unwrap_or_else(|| panic!("{} default {d} doesn't parse", a.name));
                assert_eq!(Key::parse(&k.spec()), Some(k), "{d} round trip");
            }
        }
        let names: std::collections::BTreeSet<_> = ACTS.iter().map(|a| a.name).collect();
        assert_eq!(names.len(), ACTS.len(), "action names must be unique");
    }

    #[test]
    fn defaults_never_clash() {
        assert_eq!(Keymap::default().clashes(), vec![]);
    }

    /// Every action can be reached without F-keys, Home, PgUp/PgDn, Delete or `[ ]`.
    #[test]
    fn defaults_work_on_compact_keyboards() {
        let km = Keymap::default();
        for a in ACTS {
            let portable = km.keys(a.act).iter().any(|k| match k.code {
                KeyCode::Char(c) => c.is_ascii_alphanumeric() || " ?+-=<>,./".contains(c),
                KeyCode::Enter
                | KeyCode::Esc
                | KeyCode::Tab
                | KeyCode::BackTab
                | KeyCode::Backspace => true,
                KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right => true,
                _ => false,
            });
            assert!(portable, "{} needs a key every keyboard has", a.name);
        }
        // and every arrow-key action has a letter too (60% boards hide arrows on a layer)
        for a in ACTS.iter().filter(|a| {
            km.keys(a.act).iter().any(|k| {
                matches!(
                    k.code,
                    KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right
                )
            })
        }) {
            if a.group == Group::Title {
                continue; // the title types letters into the seed; Tab covers it
            }
            assert!(
                km.keys(a.act)
                    .iter()
                    .any(|k| matches!(k.code, KeyCode::Char(_))),
                "{} has arrows but no letter",
                a.name
            );
        }
    }

    #[test]
    fn matching() {
        let km = Keymap::default();
        assert!(km.is(Act::Save, &ev(KeyCode::Char('s'), KeyModifiers::CONTROL)));
        assert!(!km.is(Act::Station, &ev(KeyCode::Char('s'), KeyModifiers::CONTROL)));
        assert!(km.is(Act::Station, &ev(KeyCode::Char('s'), KeyModifiers::NONE)));
        assert!(km.is(Act::Double, &ev(KeyCode::Char('D'), KeyModifiers::SHIFT)));
        assert!(km.is(Act::MoveUpFast, &ev(KeyCode::Up, KeyModifiers::SHIFT)));
        assert!(!km.is(Act::MoveUp, &ev(KeyCode::Up, KeyModifiers::SHIFT)));
        assert!(km.is(Act::Help, &ev(KeyCode::Char('?'), KeyModifiers::SHIFT)));
        assert!(km.is(Act::TitlePrev, &ev(KeyCode::BackTab, KeyModifiers::SHIFT)));
    }

    #[test]
    fn fill_and_labels() {
        let km = Keymap::default();
        assert_eq!(
            km.fill("press {rail}, {help+} or {save}"),
            "press t, ? / F1 or Ctrl+S"
        );
        assert_eq!(km.fill("{pause} {nope} {"), "Space {nope} {");
    }

    #[test]
    fn rebinding_steals_and_round_trips() {
        let mut km = Keymap::default();
        let t = Key::parse("t").unwrap();
        let orphans = km.bind(Act::Station, t, false);
        assert_eq!(orphans, vec![Act::Rail], "rail lost its only key");
        assert_eq!(
            km.keys(Act::SchedTimeout),
            &[t],
            "a different screen keeps its own t"
        );
        km.reset(Act::Rail);
        assert!(km.clashes().len() == 1);
        let path = std::env::temp_dir().join("rh_keys_test.json");
        km.reset(Act::Station);
        km.bind(Act::Pause, Key::parse("p").unwrap(), true);
        km.save(&path).unwrap();
        let (back, problems) = Keymap::load(&path);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(back.labels(Act::Pause), "Space / p");
        std::fs::write(&path, r#"{"pause": ["bogus"], "fly": ["f"]}"#).unwrap();
        let (_, problems) = Keymap::load(&path);
        assert_eq!(problems.len(), 2);
    }
}
