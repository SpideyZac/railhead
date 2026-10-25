//! The guided first game. A list of steps, each with a check that looks at
//! what the player has built (read-only); when it passes, the next step shows.
//! Runs on a fixed seed in Sandbox mode so nobody goes broke while learning.
//!
//! Every step says why it matters (`text`) and then, through `guide`, what to
//! do *right now*: one short instruction that follows the player through the
//! step (pick up the tool → start here → end there), the keys for it (lit in
//! the keys panel) and the spots on the map to look at (marked on the map,
//! with an arrow at the edge when they're out of view).

use super::{App, Mode, Overlay, Place, keys::Act};
use crate::{
    balance as B,
    map::IndustryKind,
    types::Pos,
    world::{
        Action, AlertLevel, GameMode, Settings, SiteKind, Target, VState, World,
        build::{self, LayerKind},
        rail,
        vehicles::VKind,
    },
};

/// Every tutorial game uses this map.
pub const SEED: &str = "lesson";

pub struct Step {
    pub title: &'static str,
    /// What this step is about, in plain words. May name keys as `{action}`
    /// placeholders (see `keys::Keymap::fill`).
    pub text: &'static str,
    /// What to do right now, how, and where.
    pub guide: fn(&App, &World, &Tutorial) -> Guide,
    /// `None` for the last step, which waits for the player to close it.
    pub done: Option<fn(&App, &World, &Tutorial) -> bool>,
}

/// The instruction of the moment.
pub struct Guide {
    /// One short sentence; may use `{action}` placeholders.
    pub now: &'static str,
    /// The keys it needs, and a word on each (the keys panel shows them lit).
    pub keys: Vec<(Act, &'static str)>,
    /// Places on the map to look at, each with a short label.
    pub marks: Vec<(Pos, &'static str)>,
}

fn guide(
    now: &'static str,
    keys: Vec<(Act, &'static str)>,
    marks: Vec<(Pos, &'static str)>,
) -> Guide {
    Guide { now, keys, marks }
}

/// The tutorial's world: a Sandbox on `SEED`, with only the grain truck. The
/// truck that would take the mill's food home is left out: that's the job the
/// player's first train does.
pub fn world() -> World {
    let mut w = World::new(
        crate::rng::parse_seed(SEED),
        Settings {
            mode: GameMode::Sandbox,
            ..Settings::default()
        },
    );
    let to_town: Vec<u32> = w
        .trucks
        .iter()
        .filter(|t| t.carrier.schedule.stops.iter().any(|s| matches!(s.target, Target::Site(x) if matches!(w.sites[x as usize].kind, SiteKind::Town(_)))))
        .map(|t| t.carrier.id)
        .collect();
    for id in to_town {
        w.remove_vehicle(id);
    }
    // and its road: tiles of the starter road into a town that no other
    // starter road uses (never a site, never under the station's track)
    let roads = w.map.starter_roads.clone();
    let into_town = |r: &Vec<Pos>, w: &World| {
        r.last()
            .and_then(|p| w.tile(*p).site)
            .is_some_and(|s| matches!(w.sites[s as usize].kind, SiteKind::Town(_)))
    };
    let keep: Vec<Pos> = roads
        .iter()
        .filter(|r| !into_town(r, &w))
        .flatten()
        .copied()
        .collect();
    let to_clear: Vec<Pos> = roads
        .iter()
        .filter(|r| into_town(r, &w))
        .flatten()
        .copied()
        .collect();
    for p in &to_clear {
        let t = w.tile(*p);
        if !keep.contains(p) && t.site.is_none() && !t.rail.present() && t.road.present() {
            let _ = build::demolish(&mut w, *p);
        }
    }
    let home = &w.towns[w.map.home];
    let (name, pos) = (home.name.clone(), home.pos);
    w.alerts.clear();
    w.alert(
        AlertLevel::Info,
        format!("Welcome to {name}! A truck is already taking grain to the flour mill. Your first train will bring the food home."),
        Some(pos),
    );
    w
}

pub struct Tutorial {
    /// The step on screen. Below `reached` when the player paged back to reread one.
    pub step: usize,
    /// The furthest step reached: the one whose check is being watched.
    pub reached: usize,
    /// Rail tiles at the start (sorted), so "lay track" counts only new track.
    rail_start: Vec<Pos>,
    /// The starter station by home, and the free end of its stub of track.
    stub: Option<u32>,
    stub_end: Option<Pos>,
    /// A good town to run the first line to.
    target: Option<Pos>,
    /// Frame the current step was finished on (for the ✔ flash).
    pub passed_frame: Option<u64>,
}

impl Tutorial {
    pub fn new(w: &World) -> Tutorial {
        let mut rail_start: Vec<Pos> = rail_tiles(w);
        rail_start.sort_by_key(|p| (p.y, p.x));
        let stub = w.stations.first();
        // the stub's free end: the rail tile with one connection that isn't the station
        let stub_end = stub.and_then(|s| {
            rail_start
                .iter()
                .copied()
                .filter(|p| {
                    *p != s.pos
                        && w.tile(*p).rail_conn.count_ones() == 1
                        && w.tile(*p).station.is_none()
                })
                .min_by_key(|p| p.dist2(s.pos))
        });
        let target =
            Tutorial::first_line_goal(w, stub.map(|s| s.pos), stub_end.or(stub.map(|s| s.pos)));
        Tutorial {
            step: 0,
            reached: 0,
            rail_start,
            stub: stub.map(|s| s.id),
            stub_end,
            target,
            passed_frame: None,
        }
    }

    /// Where the first line goes: the nearest industry a good line away whose
    /// goods something by the home station buys (food for the town, grain for
    /// a mill), so the train has a load to bring home. Else any industry.
    fn first_line_goal(w: &World, station: Option<Pos>, from: Option<Pos>) -> Option<Pos> {
        let (station, from) = (station?, from?);
        let buyers = rail::station_sites(w, station);
        let sells_home = |k: IndustryKind| {
            buyers
                .iter()
                .any(|b| w.accept_cap(&w.sites[*b as usize], k.output()) > 0.0)
        };
        // a mill only makes food while someone brings it grain (the starter trucks do)
        let fed = |id: u32| {
            w.trucks.iter().any(|t| {
                t.carrier
                    .schedule
                    .stops
                    .iter()
                    .any(|s| s.target == Target::Site(id) && s.action == Action::Unload)
            })
        };
        let mut found: Vec<((bool, bool, bool, i32), Pos)> = w
            .sites
            .iter()
            .filter(|s| !buyers.contains(&s.id) && s.pos.manhattan(from) >= 8)
            .filter_map(|s| match s.kind {
                SiteKind::Industry(k) => {
                    let d = s.pos.manhattan(from);
                    Some((
                        (!sells_home(k), !(k.is_primary() || fed(s.id)), d > 45, d),
                        s.pos,
                    ))
                }
                _ => None,
            })
            .collect();
        // wanted at home and making it now first, then the nearest
        found.sort_by_key(|(rank, p)| (*rank, p.y, p.x));
        found.first().map(|(_, p)| *p)
    }

    pub fn current(&self) -> Option<&'static Step> {
        STEPS.get(self.step)
    }

    pub fn finished(&self) -> bool {
        self.step + 1 >= STEPS.len()
    }

    /// Showing a step that is already done (the player paged back).
    pub fn reviewing(&self) -> bool {
        self.step < self.reached
    }

    /// Page back one step to read it again.
    pub fn back(&mut self) -> bool {
        if self.step == 0 {
            return false;
        }
        self.step -= 1;
        self.passed_frame = None;
        true
    }

    /// Page forward: through steps already done, or skip the current one.
    pub fn forward(&mut self) {
        self.step = (self.step + 1).min(STEPS.len() - 1);
        self.reached = self.reached.max(self.step);
        self.passed_frame = None;
    }

    /// A nudge when the player is stuck on the current step.
    pub fn trouble(&self, w: &World) -> Option<&'static str> {
        match self.step {
            STEP_DELIVERY if w.trains.iter().any(|t| t.carrier.state == VState::NoPath) => {
                Some("Your train says NO PATH: are both stations on the same piece of track?")
            }
            STEP_STATION
                if self
                    .new_stations(w)
                    .any(|s| rail::station_sites(w, s).is_empty()) =>
            {
                Some(
                    "That station serves nothing: it must be within 2 tiles of a town or industry. Demolish it ({demolish}) and try nearer.",
                )
            }
            _ => None,
        }
    }

    /// Track the player laid (not the starter stub).
    fn new_rail<'a>(&'a self, w: &'a World) -> impl Iterator<Item = Pos> + 'a {
        rail_tiles(w).into_iter().filter(|p| {
            self.rail_start
                .binary_search_by_key(&(p.y, p.x), |q| (q.y, q.x))
                .is_err()
        })
    }

    /// Stations the player built (not the starter one).
    fn new_stations<'a>(&'a self, w: &'a World) -> impl Iterator<Item = Pos> + 'a {
        w.stations
            .iter()
            .filter(move |s| !s.removed && Some(s.id) != self.stub)
            .map(|s| s.pos)
    }

    fn stub_pos(&self, w: &World) -> Option<Pos> {
        self.stub
            .and_then(|id| w.stations.iter().find(|s| s.id == id && !s.removed))
            .map(|s| s.pos)
    }

    /// Where a station on the new line would serve something, nearest the far town.
    fn station_spot(&self, w: &World) -> Option<Pos> {
        let goal = self.target?;
        self.new_rail(w)
            .filter(|p| {
                let t = w.tile(*p);
                t.station.is_none()
                    && t.signal.is_none()
                    && t.rail_conn.count_ones() <= 2
                    && !rail::station_sites(w, *p).is_empty()
            })
            .min_by_key(|p| (p.dist2(goal), p.y, p.x))
    }

    /// A plain bit of the new line about halfway between its stations, for a signal.
    fn signal_spot(&self, w: &World) -> Option<Pos> {
        let a = self.stub_pos(w)?;
        let b = self.new_stations(w).next()?;
        self.new_rail(w)
            .filter(|p| {
                let t = w.tile(*p);
                t.station.is_none()
                    && t.signal.is_none()
                    && t.rail_conn.count_ones() == 2
                    && p.dist2(a) > 4
                    && p.dist2(b) > 4
            })
            .min_by_key(|p| ((p.manhattan(a) - p.manhattan(b)).abs(), p.y, p.x))
    }

    fn train_head(w: &World) -> Option<Pos> {
        w.trains.first().map(|t| t.head())
    }
}

/// What the current step wants right now (for the keys panel and the map).
pub fn guide_now(app: &App, w: &World, tut: &Tutorial) -> Option<Guide> {
    let step = tut.current()?;
    Some((step.guide)(app, w, tut))
}

/// The keys the current step wants pressed, with what each does.
pub fn keys_now(app: &App, w: &World, tut: &Tutorial) -> Vec<(Act, &'static str)> {
    guide_now(app, w, tut).map(|g| g.keys).unwrap_or_default()
}

fn rail_tiles(w: &World) -> Vec<Pos> {
    (0..w.map.h)
        .flat_map(|y| (0..w.map.w).map(move |x| Pos::new(x, y)))
        .filter(|p| w.tile(*p).rail.present())
        .collect()
}

fn on_industry(app: &App, w: &World) -> bool {
    matches!(
        w.tile(app.cursor).site.map(|s| w.sites[s as usize].kind),
        Some(SiteKind::Industry(_))
    )
}

/// The industry nearest home (to show first).
fn near_industry(w: &World) -> Option<Pos> {
    let home = w.towns[w.map.home].pos;
    w.sites
        .iter()
        .filter(|s| matches!(s.kind, SiteKind::Industry(_)))
        .map(|s| s.pos)
        .min_by_key(|p| (p.dist2(home), p.y, p.x))
}

fn selected_train(app: &App, w: &World) -> bool {
    app.selected
        .and_then(|id| w.carrier(id))
        .is_some_and(|(_, k)| k == VKind::Train)
}

const STEP_STATION: usize = 5;
const STEP_DELIVERY: usize = 8;

pub static STEPS: &[Step] = &[
    Step {
        title: "Welcome",
        text: "You run the railway in a quiet region. Bring towns what they want and they grow. \
               The more people live in towns you serve, the higher your score (Prosperity). \
               The game starts paused, so you can look around first.",
        guide: |_, _, _| {
            guide(
                "Press {pause} to start the clock.",
                vec![(Act::Pause, "start the clock")],
                vec![],
            )
        },
        done: Some(|app, _, _| app.speed > 0),
    },
    Step {
        title: "Look around",
        text: "Letters on the map are industries (F is a farm, C a coal mine, M a flour mill...). \
               Little ▪ dots are houses, and ★ is your home town. The panel on the right always \
               describes whatever the yellow cursor is on.",
        guide: |_, w, _| {
            guide(
                "Move the cursor onto the marked farm (arrow keys, or point with the mouse), then read the right-hand panel.",
                vec![(Act::MoveUp, "move the cursor")],
                near_industry(w)
                    .map(|p| vec![(p, "look here")])
                    .unwrap_or_default(),
            )
        },
        done: Some(|app, w, _| on_industry(app, w)),
    },
    Step {
        title: "Home",
        text: "The panel shows what an industry makes and who buys it. Towns show what they want, \
               and how happy they are with what they get.",
        guide: |_, w, _| {
            guide(
                "Press {home} to jump back to your home town ★.",
                vec![(Act::HomeTown, "jump home")],
                vec![(w.towns[w.map.home].pos, "home")],
            )
        },
        done: Some(|app, w, _| w.tile(app.cursor).site == Some(w.towns[w.map.home].site)),
    },
    Step {
        title: "The station",
        text: "A truck already takes grain to the flour mill, but nothing brings the food home \
               yet. Trucks are slow; trains carry far more. Next to home is a short piece of track with a station (the purple S).",
        guide: |_, w, tut| {
            guide(
                "Move the cursor onto the station S.",
                vec![(Act::MoveUp, "move the cursor")],
                tut.stub_pos(w)
                    .map(|p| vec![(p, "station")])
                    .unwrap_or_default(),
            )
        },
        done: Some(|app, w, _| w.tile(app.cursor).station.is_some()),
    },
    Step {
        title: "Lay track",
        text: "Track takes two presses: one where it starts, one where it ends. The game finds \
               the route in between by itself, round hills and water. Let's run a line out to the \
               marked industry: it makes something that's wanted back home.",
        guide: |app, _, tut| {
            let start = tut
                .stub_end
                .map(|p| vec![(p, "start here")])
                .unwrap_or_default();
            let end: Vec<(Pos, &str)> = tut
                .target
                .map(|p| vec![(p, "end near here")])
                .unwrap_or_default();
            let both = [
                start.clone(),
                end.iter()
                    .map(|(p, _)| (*p, "the line goes here"))
                    .collect(),
            ]
            .concat();
            match app.mode {
                Mode::Line {
                    kind: LayerKind::Rail,
                    start: None,
                } => guide(
                    "Move onto the end of the track (marked) and press {confirm}: that's where the new line starts.",
                    vec![(Act::Confirm, "start here")],
                    both,
                ),
                Mode::Line {
                    kind: LayerKind::Rail,
                    start: Some(_),
                } => guide(
                    "Now move next to the marked industry (the blue path shows the route) and press {confirm} to build it.",
                    vec![
                        (Act::Confirm, "build to here"),
                        (Act::Cancel, "undo the start"),
                    ],
                    end,
                ),
                _ => guide(
                    "Press {rail} to pick up the track tool.",
                    vec![(Act::Rail, "track tool")],
                    both,
                ),
            }
        },
        done: Some(|_, w, tut| tut.new_rail(w).count() >= 8),
    },
    Step {
        title: "A second station",
        text: "A station loads and unloads for every town and industry within 2 tiles of it. \
               Your new line needs one at the far end, beside the industry, to pick up what it makes.",
        guide: |app, w, tut| {
            let spot = tut
                .station_spot(w)
                .map(|p| vec![(p, "station here")])
                .unwrap_or_default();
            match app.mode {
                Mode::Place(Place::Station) => guide(
                    "Move onto the marked piece of your new track and press {confirm} to build the station.",
                    vec![
                        (Act::Confirm, "place the station"),
                        (Act::Cancel, "put the tool away"),
                    ],
                    spot,
                ),
                _ => guide(
                    "Press {station} to pick up the station tool.",
                    vec![(Act::Station, "station tool")],
                    spot,
                ),
            }
        },
        done: Some(|_, w, tut| {
            tut.new_stations(w)
                .any(|p| !rail::station_sites(w, p).is_empty())
        }),
    },
    Step {
        title: "Buy a train",
        text: "Trains are bought at a station. A freight train carries goods; an express \
               ({buy_express}) is faster and made for passengers. Both carry anything.",
        guide: |app, w, tut| {
            let marks = tut
                .new_stations(w)
                .take(1)
                .map(|p| (p, "buy here"))
                .collect();
            if w.tile(app.cursor).station.is_some() {
                guide(
                    "Press {buy_freight} to buy a freight train at this station.",
                    vec![(Act::BuyFreight, "buy a train")],
                    marks,
                )
            } else {
                guide(
                    "Move onto your new station (marked).",
                    vec![(Act::MoveUp, "move the cursor")],
                    marks,
                )
            }
        },
        done: Some(|_, w, _| !w.trains.is_empty()),
    },
    Step {
        title: "Its route",
        text: "A train's schedule is the list of stations it visits, round and round. Stop 1 is \
               already set: the station you bought it at, by the industry, where it loads. It \
               needs somewhere to take the load: back home, where it's wanted.",
        guide: |app, w, tut| {
            let home = tut
                .stub_pos(w)
                .map(|p| vec![(p, "stop 2")])
                .unwrap_or_default();
            match app.mode {
                Mode::Schedule { picking: true, .. } => guide(
                    "Move onto the station by your home town (marked) and press {confirm} to make it stop 2.",
                    vec![
                        (Act::Confirm, "add this stop"),
                        (Act::Cancel, "back to the list"),
                    ],
                    home,
                ),
                Mode::Schedule { .. } => guide(
                    "Press {sched_add} to add a stop, then pick the station by home.",
                    vec![(Act::SchedAdd, "add a stop")],
                    home,
                ),
                _ if selected_train(app, w) => guide(
                    "Press {edit} to open the train's schedule.",
                    vec![(Act::Edit, "edit the schedule")],
                    home,
                ),
                _ => guide(
                    "Press {next_vehicle} until your train blinks, then {edit} to open its schedule.",
                    vec![
                        (Act::NextVehicle, "select the train"),
                        (Act::Edit, "edit the schedule"),
                    ],
                    home,
                ),
            }
        },
        done: Some(|_, w, _| {
            w.trains.iter().any(|t| {
                let s = &t.carrier.schedule.stops;
                s.len() >= 2 && s.iter().any(|a| s.iter().any(|b| a.target != b.target))
            })
        }),
    },
    Step {
        title: "First delivery",
        text: "That's it: the train now runs between the two stations on its own. You're paid \
               whenever it unloads something a place wants. Longer, quicker trips pay more.",
        guide: |app, w, _| {
            let train = Tutorial::train_head(w)
                .map(|p| vec![(p, "your train")])
                .unwrap_or_default();
            match app.mode {
                Mode::Schedule { .. } => guide(
                    "Press {sched_close} to close the schedule, then speed up time with {speed_4}.",
                    vec![
                        (Act::SchedClose, "close the editor"),
                        (Act::Speed4, "fastest"),
                    ],
                    train,
                ),
                _ => guide(
                    "Speed up time with {speed_3} or {speed_4} and watch for the first +£.",
                    vec![(Act::Speed3, "faster"), (Act::Speed4, "fastest")],
                    train,
                ),
            }
        },
        done: Some(|_, w, _| w.trains.iter().any(|t| t.carrier.lifetime_income > 0.0)),
    },
    Step {
        title: "Longer trains",
        text: "More wagons carry more on every trip. Wagons can only be added while the train is \
               stopped at a station.",
        guide: |app, w, _| {
            let train = Tutorial::train_head(w)
                .map(|p| vec![(p, "your train")])
                .unwrap_or_default();
            let stopped = w
                .trains
                .first()
                .is_some_and(|t| matches!(t.carrier.state, VState::Working(_)));
            if !selected_train(app, w) {
                guide(
                    "Select your train: press {next_vehicle} until it blinks (or click it).",
                    vec![(Act::NextVehicle, "select the train")],
                    train,
                )
            } else if !stopped {
                guide(
                    "Wait for it to stop at a station (the side panel says Loading or Unloading).",
                    vec![(Act::Speed4, "hurry it up")],
                    train,
                )
            } else {
                guide(
                    "It's at a station: press {more} to add a wagon.",
                    vec![(Act::More, "add a wagon")],
                    train,
                )
            }
        },
        done: Some(|_, w, _| {
            w.trains
                .iter()
                .any(|t| t.wagons > B::DEFAULT_WAGONS[t.kind.i()])
        }),
    },
    Step {
        title: "Signals",
        text: "Two trains on one track would run into each other, so the game lets only one in at \
               a time. A signal splits the track into sections; each section holds one train. \
               Then a second train can follow the first. A signal shows green ◆ when the section \
               after it is empty, and red while a train is in it.",
        guide: |app, w, tut| {
            let spot = tut
                .signal_spot(w)
                .map(|p| vec![(p, "signal here")])
                .unwrap_or_default();
            match app.mode {
                Mode::Place(Place::Signal) => guide(
                    "Move onto the marked track, halfway along your line, and press {confirm} to put up a signal.",
                    vec![
                        (Act::Confirm, "place a signal"),
                        (Act::Cancel, "put the tool away"),
                    ],
                    spot,
                ),
                _ => guide(
                    "Press {signal} to pick up the signal tool.",
                    vec![
                        (Act::Signal, "signal tool"),
                        (Act::AutoSignal, "or: signal it all"),
                    ],
                    spot,
                ),
            }
        },
        done: Some(|_, w, _| w.tiles.iter().any(|t| t.signal.is_some())),
    },
    Step {
        title: "See the flow",
        text: "Overlays color the map to show what's going on. FLOW lights up the track that carries \
               the most; TOWNS shows which towns are happy.",
        guide: |_, _, _| {
            guide(
                "Press {overlay} until the top bar says FLOW (two presses).",
                vec![(Act::Overlay, "next overlay")],
                vec![],
            )
        },
        done: Some(|app, _, _| app.overlay == Overlay::Flow),
    },
    Step {
        title: "The numbers",
        text: "The statistics screen graphs your Prosperity, cash and deliveries over the game.",
        guide: |_, _, _| {
            guide(
                "Press {stats} to open it (any key closes it again).",
                vec![(Act::Stats, "statistics")],
                vec![],
            )
        },
        done: Some(|app, _, _| app.show_stats),
    },
    Step {
        title: "You're ready",
        text: "That's the game: find what towns want, bring it by rail, keep it moving. Feed \
               industries what they need to make goods, and watch for contracts ({next_tab}) that \
               pay a bonus. Next, try a Charter: 60 minutes, and a score at the end. The keys \
               panel on the left always shows what you can do; {help} opens the Railway Handbook.",
        guide: |_, _, _| {
            guide(
                "Press {tutorial_skip} to close this guide and keep building.",
                vec![
                    (Act::TutorialSkip, "close the guide"),
                    (Act::Help, "the handbook"),
                ],
                vec![],
            )
        },
        done: None,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The tutorial map has a stub to start from and an industry to go to,
    /// whose goods are wanted by something at the home station.
    #[test]
    fn the_first_line_has_somewhere_to_go() {
        let w = world();
        let t = Tutorial::new(&w);
        let (end, target) = (t.stub_end.expect("a stub end"), t.target.expect("a target"));
        assert!(w.tile(end).rail.present() && w.tile(end).station.is_none());
        assert!(
            (8..=45).contains(&end.manhattan(target)),
            "a line worth building, not a trek"
        );
        let Some(SiteKind::Industry(k)) = w.tile(target).site.map(|s| w.sites[s as usize].kind)
        else {
            panic!("the line goes to an industry")
        };
        let home = rail::station_sites(&w, w.stations[0].pos);
        assert!(
            home.iter()
                .any(|s| w.accept_cap(&w.sites[*s as usize], k.output()) > 0.0),
            "home buys its {:?}",
            k.output()
        );
        assert_eq!(t.new_rail(&w).count(), 0, "nothing new yet");
        assert!(w.trucks.iter().all(|t| t.carrier.schedule.stops.iter().all(|s| !matches!(s.target, Target::Site(x) if matches!(w.sites[x as usize].kind, SiteKind::Town(_))))), "no truck takes food home: the train does");
        assert_eq!(w.trucks.len(), 1, "the grain truck still feeds the mill");
        // and no road from the mill to town: only the farm road is left
        let farm_road: Vec<Pos> = w
            .map
            .starter_roads
            .iter()
            .filter(|r| {
                r.last()
                    .and_then(|p| w.tile(*p).site)
                    .is_some_and(|s| !matches!(w.sites[s as usize].kind, SiteKind::Town(_)))
            })
            .flatten()
            .copied()
            .collect();
        let stray: Vec<Pos> = (0..w.map.h)
            .flat_map(|y| (0..w.map.w).map(move |x| Pos::new(x, y)))
            .filter(|p| {
                w.tile(*p).road.present() && w.tile(*p).site.is_none() && !farm_road.contains(p)
            })
            .collect();
        assert!(stray.is_empty(), "road left over: {stray:?}");
    }
}
