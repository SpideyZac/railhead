//! Headless balance harness: fast-forward many seeded games without a UI
//! and print statistics. Two policies: `idle` (do nothing) and `builder`
//! (a simple scripted player that trucks whole supply chains).

use crate::{
    balance as B,
    map::IndustryKind,
    types::{Cargo, CargoMask, Pos},
    world::{
        Action, Settings, SiteKind, Stop, Target, TrainKind, World,
        build::{self, LayerKind},
        rail,
    },
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Policy {
    Idle,
    /// Trucks only.
    Builder,
    /// Rail for long hauls (plus an express passenger line), trucks for short ones.
    Rail,
}

pub struct SimOpts {
    pub games: u64,
    pub ticks: u64,
    pub first_seed: u64,
    pub policies: Vec<Policy>,
    pub verbose: bool,
    pub size: crate::map::MapSize,
    pub mode: crate::world::GameMode,
}

pub struct RunResult {
    pub seed: u64,
    pub start_prosperity: f64,
    pub prosperity: f64,
    pub score: f64,
    pub contracts: u32,
    pub delivered: f64,
    pub cash: f64,
    pub revenue: f64,
    pub trucks: usize,
    pub trains: usize,
    pub best_tier: usize,
    /// Every town's starting population (what medals are measured against).
    pub region: f64,
    pub medal: usize,
    pub milestones: usize,
    /// Franchise: reviews passed, and when the franchise was lost (game minutes).
    pub reviews_passed: u32,
    pub revoked_at: Option<f64>,
}

pub fn run(opts: &SimOpts) {
    for &policy in &opts.policies {
        println!(
            "\n=== policy {:?} · {} games · {} ticks ({:.0} game-min) ===",
            policy,
            opts.games,
            opts.ticks,
            opts.ticks as f64 * B::TICK_DT / 60.0
        );
        println!(
            "{:>8} {:>9} {:>9} {:>6} {:>8} {:>5} {:>9} {:>9} {:>8} {:>6} {:>6} {:>10} {:>7} {:>6} {:>5}",
            "seed",
            "pros.0",
            "pros.end",
            "x",
            "score",
            "cntr",
            "delivered",
            "revenue",
            "cash",
            "trucks",
            "trains",
            "best tier",
            "region",
            "medal",
            "miles"
        );
        let mut results = vec![];
        for g in 0..opts.games {
            let r = run_one(opts.first_seed + g, opts, policy);
            println!(
                "{:>8} {:>9.0} {:>9.0} {:>6.2} {:>8.0} {:>5} {:>9.0} {:>9.0} {:>8.0} {:>6} {:>6} {:>10} {:>7.0} {:>6} {:>5}",
                r.seed,
                r.start_prosperity,
                r.prosperity,
                r.prosperity / r.start_prosperity.max(1.0),
                r.score,
                r.contracts,
                r.delivered,
                r.revenue,
                r.cash,
                r.trucks,
                r.trains,
                crate::world::TIER_NAMES[r.best_tier],
                r.region,
                ["-", "bronze", "silver", "gold"][r.medal],
                r.milestones
            );
            results.push(r);
        }
        let n = results.len().max(1) as f64;
        let mult = results
            .iter()
            .map(|r| r.prosperity / r.start_prosperity.max(1.0))
            .sum::<f64>()
            / n;
        let pros = results.iter().map(|r| r.prosperity).sum::<f64>() / n;
        let cash = results.iter().map(|r| r.cash).sum::<f64>() / n;
        let score = results.iter().map(|r| r.score).sum::<f64>() / n;
        println!(
            "--- {:?}: avg score {:.0}, avg Prosperity {:.0} ({:.2}x start), avg final cash {:.0}",
            policy, score, pros, mult, cash
        );
        let medals: Vec<usize> = (0..4)
            .map(|m| results.iter().filter(|r| r.medal == m).count())
            .collect();
        let ratio = results
            .iter()
            .map(|r| r.score / r.region.max(1.0))
            .sum::<f64>()
            / n;
        println!(
            "    medals: none {} · bronze {} · silver {} · gold {} · score/region {ratio:.2}",
            medals[0], medals[1], medals[2], medals[3]
        );
        if opts.mode == crate::world::GameMode::Franchise {
            let passed: Vec<String> = results
                .iter()
                .map(|r| match r.revoked_at {
                    Some(t) => format!("{} (out at {t:.0}m)", r.reviews_passed),
                    None => format!("{}+", r.reviews_passed),
                })
                .collect();
            println!("    franchise reviews passed: {}", passed.join(" · "));
        }
    }
}

/// Play one game with a bot and return the world as it ends.
pub fn play(seed: u64, opts: &SimOpts, policy: Policy) -> World {
    let mut w = World::new(
        seed,
        Settings {
            size: opts.size,
            mode: opts.mode,
        },
    );
    let mut seen_alerts = 0;
    for _ in 0..opts.ticks {
        if w.finished && w.franchise() {
            break;
        }
        if opts.verbose && w.alerts.len() != seen_alerts {
            for a in w.alerts.iter().skip(seen_alerts.min(w.alerts.len())) {
                eprintln!(
                    "    {:>6.0}s {}",
                    a.time,
                    crate::ui::keys::Keymap::default().fill(&a.text)
                );
            }
            seen_alerts = w.alerts.len();
        }
        if policy != Policy::Idle && w.ticks.is_multiple_of(5 * B::TICKS_PER_SEC) {
            if policy == Policy::Rail {
                think_rail(&mut w);
            }
            think(&mut w, policy);
        }
        w.tick();
    }
    if opts.verbose {
        for t in &w.trains {
            let c = &t.carrier;
            let stops: Vec<String> = c
                .schedule
                .stops
                .iter()
                .map(|s| w.target_name(s.target))
                .collect();
            eprintln!(
                "    {} {:?} {:?} earned {:.0} cargo {:.0}/{:.0} [{}]",
                c.name,
                t.kind,
                c.state,
                c.lifetime_income,
                c.cargo.total(),
                c.capacity,
                stops.join(" -> ")
            );
        }
    }
    w
}

pub fn run_one(seed: u64, opts: &SimOpts, policy: Policy) -> RunResult {
    let w = play(seed, opts, policy);
    RunResult {
        seed,
        start_prosperity: w.stats.start_prosperity,
        prosperity: w.prosperity(),
        score: w.score(),
        contracts: w.stats.contracts_done,
        delivered: w.stats.delivered.total(),
        cash: w.cash,
        revenue: w.stats.revenue,
        trucks: w.trucks.len(),
        trains: w.trains.len(),
        best_tier: w.towns.iter().map(|t| t.best_tier).max().unwrap_or(0),
        region: w.towns.iter().map(|t| t.start_pop).sum(),
        medal: w.stats.medal,
        milestones: crate::world::milestones::reached(&w),
        reviews_passed: w.stats.reviews_passed,
        revoked_at: (w.franchise() && w.finished).then_some(w.time / 60.0),
    }
}

/// Cash the bot keeps in hand so upkeep never sends it into the red.
const RESERVE: f64 = 600.0;
/// The bot ignores routes longer than this (Manhattan tiles).
const MAX_ROUTE: i32 = 70;

/// Is there already a truck hauling from `a` to `b`?
fn has_route(w: &World, a: u32, b: u32) -> bool {
    w.trucks.iter().any(|t| {
        let s = &t.carrier.schedule.stops;
        s.len() == 2 && s[0].target == Target::Site(a) && s[1].target == Target::Site(b)
    })
}

/// Every haul the bot would like: raw inputs to each secondary industry,
/// and finished goods to the nearest town that wants them. Shortest first.
fn wanted_routes(w: &World) -> Vec<(u32, u32, Cargo)> {
    let industries: Vec<(u32, IndustryKind)> = w
        .sites
        .iter()
        .filter_map(|s| match s.kind {
            SiteKind::Industry(k) => Some((s.id, k)),
            _ => None,
        })
        .collect();
    let pos = |id: u32| w.sites[id as usize].pos;
    let mut routes = vec![];
    for &(dst, k) in &industries {
        for &c in k.inputs() {
            let src = industries
                .iter()
                .filter(|(_, sk)| sk.output() == c)
                .min_by_key(|(s, _)| pos(*s).manhattan(pos(dst)));
            if let Some(&(src, _)) = src {
                routes.push((src, dst, c));
            }
        }
        if k.is_primary() || k == IndustryKind::Steelworks {
            continue;
        }
        let out = k.output();
        if let Some(t) = w
            .towns
            .iter()
            .filter(|t| t.wants(out))
            .min_by_key(|t| t.pos.manhattan(pos(dst)))
        {
            routes.push((dst, t.site, out));
        }
    }
    routes.retain(|(a, b, _)| pos(*a).manhattan(pos(*b)) <= MAX_ROUTE);
    routes.sort_by_key(|(a, b, _)| pos(*a).manhattan(pos(*b)));
    routes
}

/// Build one missing truck route per think (road + truck), if affordable.
fn think(w: &mut World, policy: Policy) {
    // Rescue: if any train is deadlocked, reverse it.
    let dead: Vec<u32> = w
        .trains
        .iter()
        .filter(|t| t.deadlocked)
        .map(|t| t.carrier.id)
        .collect();
    for id in dead {
        let _ = rail::reverse_train(w, id);
    }
    for (a, b, c) in wanted_routes(w) {
        if has_route(w, a, b) || (policy == Policy::Rail && rail_serves(w, a, b)) {
            continue;
        }
        let (pa, pb) = (w.sites[a as usize].pos, w.sites[b as usize].pos);
        if w.road_route(pa, pb).is_none() {
            let Ok(path) = build::plan_path(w, pa, pb, LayerKind::Road) else {
                continue;
            };
            if build::path_cost(w, &path, LayerKind::Road) + B::TRUCK_COST + RESERVE > w.cash {
                return; // save up
            }
            if build::commit_path(w, &path, LayerKind::Road).is_err() {
                return;
            }
        }
        if w.cash < B::TRUCK_COST + RESERVE || w.buy_truck(a).is_err() {
            return;
        }
        let mut load = Stop::new(Target::Site(a), Action::Load);
        load.filter = CargoMask::single(c);
        let mut unload = Stop::new(Target::Site(b), Action::Unload);
        unload.filter = CargoMask::single(c);
        w.trucks.last_mut().unwrap().carrier.schedule.stops = vec![load, unload];
        return;
    }
}

/// Routes at least this long (Manhattan tiles) go by rail in the Rail policy.
const RAIL_MIN: i32 = 22;
/// The Rail policy only upgrades routes to rail with at least this much cash.
const RAIL_UPGRADE_CASH: f64 = 4000.0;

fn long(w: &World, a: u32, b: u32) -> bool {
    w.sites[a as usize].pos.manhattan(w.sites[b as usize].pos) >= RAIL_MIN
}

/// Does a train already run from a station serving `a` to one serving `b`?
fn rail_serves(w: &World, a: u32, b: u32) -> bool {
    let serves = |t: Target, site: u32| match t {
        Target::Station(s) => rail::station_sites(w, w.stations[s as usize].pos).contains(&site),
        _ => false,
    };
    w.trains.iter().any(|t| {
        let s = &t.carrier.schedule.stops;
        s.len() == 2 && serves(s[0].target, a) && serves(s[1].target, b)
    })
}

/// Free tiles within a station's reach of `site`, nearest to `toward` first.
fn station_spots(w: &World, site: u32, toward: Pos) -> Vec<Pos> {
    let c = w.sites[site as usize].pos;
    let r = B::STATION_RADIUS;
    let mut v: Vec<Pos> = (-r..=r)
        .flat_map(|dy| (-r..=r).map(move |dx| Pos::new(c.x + dx, c.y + dy)))
        .filter(|p| w.map.in_bounds(*p))
        .filter(|p| {
            matches!(
                w.map.terrain(*p),
                crate::map::Terrain::Plains
                    | crate::map::Terrain::Forest
                    | crate::map::Terrain::Hills
            )
        })
        .filter(|p| w.tile(*p).site.is_none() && !w.tile(*p).rail.present())
        .collect();
    v.sort_by_key(|p| (p.manhattan(toward), p.y, p.x));
    v.truncate(5);
    v
}

/// Build a dedicated single-track line with a station at each end and one
/// train shuttling between them. Returns false if it couldn't (or can't afford it yet).
fn build_line(w: &mut World, a: u32, b: u32, kind: TrainKind, load: Stop, unload: Stop) -> bool {
    let (pa, pb) = (w.sites[a as usize].pos, w.sites[b as usize].pos);
    let mut best: Option<(f64, Vec<Pos>)> = None;
    for sa in station_spots(w, a, pb) {
        for sb in station_spots(w, b, pa) {
            if let Some(path) = build::plan_separate(w, sa, sb) {
                let cost = build::path_cost(w, &path, LayerKind::Rail);
                if best.as_ref().is_none_or(|(c, _)| cost < *c) {
                    best = Some((cost, path));
                }
            }
        }
    }
    let Some((cost, path)) = best else {
        return false;
    };
    let k = kind.i();
    let total = cost
        + 2.0 * B::STATION_COST
        + B::LOCO_COST[k]
        + B::WAGON_COST[k] * B::DEFAULT_WAGONS[k] as f64;
    if total + RESERVE * 2.0 > w.cash || build::commit_path(w, &path, LayerKind::Rail).is_err() {
        return false;
    }
    let (Ok(_), Ok(_)) = (
        build::place_station(w, path[0]),
        build::place_station(w, *path.last().unwrap()),
    ) else {
        return false;
    };
    let (s0, s1) = (
        w.tile(path[0]).station.unwrap(),
        w.tile(*path.last().unwrap()).station.unwrap(),
    );
    let Ok(id) = rail::buy_train(w, s0, kind) else {
        return false;
    };
    let stops = vec![
        Stop {
            target: Target::Station(s0),
            ..load
        },
        Stop {
            target: Target::Station(s1),
            ..unload
        },
    ];
    w.carrier_mut(id).unwrap().schedule.stops = stops;
    true
}

/// The Rail policy: everything starts as a truck route (see `think`); with
/// spare cash, upgrade the longest truck route to its own rail line and scrap
/// its trucks. Then add an express passenger line between big towns.
fn think_rail(w: &mut World) {
    // Lengthen trains whose pickup is overflowing (only possible while stopped at a station).
    let grow: Vec<u32> = w
        .trains
        .iter()
        .filter(|t| matches!(t.carrier.state, crate::world::VState::Working(_)) && t.carrier.cur == 0)
        .filter(|t| {
            let Some(Target::Station(s)) = t.carrier.schedule.stops.first().map(|s| s.target) else { return false };
            rail::station_sites(w, w.stations[s as usize].pos).iter().any(|id| {
                let site = &w.sites[*id as usize];
                matches!(site.kind, SiteKind::Industry(k) if site.inv[k.output()] > B::NODE_BUFFER * 0.8)
            })
        })
        .map(|t| t.carrier.id)
        .collect();
    for id in grow {
        if w.cash > RESERVE * 2.0 {
            let _ = rail::change_wagons(w, id, 1);
        }
    }
    if w.cash < RAIL_UPGRADE_CASH {
        return;
    }
    let mut routes: Vec<(u32, u32, Cargo)> = wanted_routes(w)
        .into_iter()
        .filter(|(a, b, _)| long(w, *a, *b) && has_route(w, *a, *b) && !rail_serves(w, *a, *b))
        .collect();
    routes.sort_by_key(|(a, b, _)| {
        std::cmp::Reverse(w.sites[*a as usize].pos.manhattan(w.sites[*b as usize].pos))
    });
    if let Some((a, b, c)) = routes.into_iter().next() {
        let mut load = Stop::new(Target::Site(a), Action::Load);
        load.filter = CargoMask::single(c);
        load.timeout = Some(90.0);
        let mut unload = Stop::new(Target::Site(b), Action::Unload);
        unload.filter = CargoMask::single(c);
        if build_line(w, a, b, TrainKind::Freight, load, unload) {
            let trucks: Vec<u32> = w
                .trucks
                .iter()
                .filter(|t| {
                    t.carrier
                        .schedule
                        .stops
                        .first()
                        .is_some_and(|s| s.target == Target::Site(a))
                        && t.carrier
                            .schedule
                            .stops
                            .get(1)
                            .is_some_and(|s| s.target == Target::Site(b))
                })
                .map(|t| t.carrier.id)
                .collect();
            for id in trucks {
                w.remove_vehicle(id);
            }
        }
        return; // one upgrade per think
    }
    // Everything is on rail: link the two biggest towns on the network by express.
    let mut towns: Vec<&crate::world::Town> = w.towns.iter().filter(|t| t.served).collect();
    towns.sort_by(|x, y| y.pop.total_cmp(&x.pop));
    if let [t1, t2, ..] = towns[..] {
        let (a, b) = (t1.site, t2.site);
        if !rail_serves(w, a, b) && !rail_serves(w, b, a) && long(w, a, b) {
            let mut s = Stop::new(Target::Site(a), Action::Swap);
            s.filter = CargoMask::single(Cargo::Passengers);
            build_line(w, a, b, TrainKind::Express, s.clone(), s);
        }
    }
}
