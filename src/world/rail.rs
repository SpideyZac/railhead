//! Rail: blocks, signals, stations, trains, deadlock detection.
//!
//! Model (kept deliberately simple, per the spec's scope rules):
//! - Track lives on tiles with explicit connection bits, so parallel lines
//!   don't merge. A tile with 3+ connections is a junction.
//! - A **signal** sits on a plain 2-connection tile. Blocks are the connected
//!   runs of track between signal tiles (signal tiles belong to no block).
//! - A train may only step onto a signal tile if the block beyond it is free;
//!   it then reserves that block. Two trains never share a block.
//! - One-way signals (`OneWay(d)`) may only be *exited* in direction `d`.
//! - Trains reverse only at stations or dead ends (or when the player says so).
//! - Deadlock = a cycle in the "train A waits for the block held by train B" graph.

use std::{cmp::Reverse, collections::BinaryHeap};

use super::*;
use crate::types::Dir;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum SignalKind {
    Both,
    OneWay(Dir),
    /// Chain signals: only let a train through if it can also get out of the
    /// block beyond (past the next signal). Put them in front of junctions.
    Chain,
    ChainOneWay(Dir),
}

impl SignalKind {
    pub fn one_way(self) -> Option<Dir> {
        match self {
            SignalKind::OneWay(d) | SignalKind::ChainOneWay(d) => Some(d),
            _ => None,
        }
    }

    pub fn is_chain(self) -> bool {
        matches!(self, SignalKind::Chain | SignalKind::ChainOneWay(_))
    }

    /// The same signal, switched between block and chain.
    pub fn toggled_chain(self) -> SignalKind {
        match self {
            SignalKind::Both => SignalKind::Chain,
            SignalKind::Chain => SignalKind::Both,
            SignalKind::OneWay(d) => SignalKind::ChainOneWay(d),
            SignalKind::ChainOneWay(d) => SignalKind::OneWay(d),
        }
    }
}

/// The two train types.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum TrainKind {
    #[default]
    Freight,
    Express,
}

impl TrainKind {
    pub fn i(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            TrainKind::Freight => "Freight",
            TrainKind::Express => "Express",
        }
    }
}

/// Derived (not saved): rail block id per tile, -1 for none / signal tiles.
#[derive(Clone, Debug, Default)]
pub struct Blocks {
    pub id: Vec<i32>,
    pub count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Train {
    pub carrier: Carrier,
    #[serde(default)]
    pub kind: TrainKind,
    /// Occupied tiles, head first. Freshly spawned trains are "compressed"
    /// (all entries equal) and unfold as they move.
    pub tiles: Vec<Pos>,
    pub wagons: usize,
    pub deadlocked: bool,
    /// Remaining path, stored reversed (next tile is `last()`).
    pub path: Vec<Pos>,
    pub progress: f64,
    pub path_version: u64,
    /// Station this train is committed to (counts toward its train limit).
    pub bound: Option<u32>,
    /// Block reserved beyond the signal the head is standing on.
    pub reserved: Option<i32>,
    /// Extra blocks held after passing a chain signal (the way out).
    #[serde(default)]
    pub chain_reserved: Vec<i32>,
    /// Train we're waiting on (for the deadlock graph).
    pub waiting_on: Option<u32>,
}

impl Train {
    /// Running cost, £ per minute.
    pub fn upkeep(&self) -> f64 {
        B::LOCO_UPKEEP[self.kind.i()] + B::WAGON_UPKEEP[self.kind.i()] * self.wagons as f64
    }

    /// What the train cost to buy, as it is now.
    pub fn value(&self) -> f64 {
        B::LOCO_COST[self.kind.i()] + B::WAGON_COST[self.kind.i()] * self.wagons as f64
    }

    pub fn head(&self) -> Pos {
        self.tiles[0]
    }

    /// Direction of travel (None when compressed on one tile).
    pub fn heading(&self) -> Option<Dir> {
        self.tiles
            .iter()
            .find(|p| **p != self.tiles[0])
            .and_then(|second| second.dir_to(self.tiles[0]))
    }
}

pub fn block_at(w: &World, p: Pos) -> i32 {
    w.blocks.id.get(w.map.idx(p)).copied().unwrap_or(-1)
}

fn rail_ok(t: &Tile) -> bool {
    t.rail == Layer::Built
}

/// Can a train step from `from` in direction `d`?
pub fn can_step(w: &World, from: Pos, d: Dir) -> bool {
    let to = from.step(d);
    if !w.map.in_bounds(to) {
        return false;
    }
    let a = w.tile(from);
    let b = w.tile(to);
    if a.rail_conn & d.bit() == 0 || !rail_ok(a) || !rail_ok(b) {
        return false;
    }
    if let Some(sd) = a.signal.and_then(|s| s.one_way())
        && sd != d
    {
        return false;
    }
    true
}

pub fn rebuild_blocks(w: &mut World) {
    let n = (w.map.w * w.map.h) as usize;
    let mut id = vec![-1i32; n];
    let mut count = 0;
    for start in 0..n {
        let t = &w.tiles[start];
        if id[start] >= 0 || !rail_ok(t) || t.signal.is_some() {
            continue;
        }
        let b = count as i32;
        count += 1;
        let mut stack = vec![start];
        id[start] = b;
        while let Some(i) = stack.pop() {
            let p = Pos::new(i as i32 % w.map.w, i as i32 / w.map.w);
            for d in Dir::ALL {
                if w.tiles[i].rail_conn & d.bit() == 0 {
                    continue;
                }
                let q = p.step(d);
                if !w.map.in_bounds(q) {
                    continue;
                }
                let j = w.map.idx(q);
                let tq = &w.tiles[j];
                if id[j] < 0
                    && rail_ok(tq)
                    && tq.signal.is_none()
                    && tq.rail_conn & d.opposite().bit() != 0
                {
                    id[j] = b;
                    stack.push(j);
                }
            }
        }
    }
    w.blocks = Blocks { id, count };
}

/// Owner (train id) of each block this tick: by occupied tiles or reservation.
fn block_owners(w: &World) -> Vec<Option<u32>> {
    let mut own = vec![None; w.blocks.count];
    for t in &w.trains {
        for p in &t.tiles {
            let b = block_at(w, *p);
            if b >= 0 {
                own[b as usize] = Some(t.carrier.id);
            }
        }
        for r in t.reserved.iter().chain(t.chain_reserved.iter()) {
            if *r >= 0 && (*r as usize) < own.len() {
                own[*r as usize] = Some(t.carrier.id);
            }
        }
    }
    own
}

pub fn occupied_blocks(w: &World) -> Vec<i32> {
    block_owners(w)
        .iter()
        .enumerate()
        .filter(|(_, o)| o.is_some())
        .map(|(i, _)| i as i32)
        .collect()
}

/// For drawing: is the block a signal guards free?
pub fn signal_block_free(w: &World, p: Pos) -> bool {
    let t = w.tile(p);
    let owners = block_owners(w);
    let busy = |q: Pos| {
        let b = block_at(w, q);
        b >= 0 && owners.get(b as usize).copied().flatten().is_some()
    };
    let train_on = w.trains.iter().any(|tr| tr.tiles.contains(&p));
    match t.signal {
        Some(s) if s.one_way().is_some() => !train_on && !busy(p.step(s.one_way().unwrap())),
        _ => {
            !train_on
                && Dir::ALL
                    .iter()
                    .filter(|d| t.rail_conn & d.bit() != 0)
                    .all(|d| !busy(p.step(*d)))
        }
    }
}

/// Is `site` within the catchment of a station at `station`?
pub fn in_catchment(station: Pos, site: Pos) -> bool {
    (station.x - site.x).abs() <= B::STATION_RADIUS
        && (station.y - site.y).abs() <= B::STATION_RADIUS
}

/// Every town, industry and hub a station at `p` serves, nearest first.
pub fn station_sites(w: &World, p: Pos) -> Vec<u32> {
    let mut v: Vec<(i32, u32)> = w
        .sites
        .iter()
        .filter(|s| s.state == Layer::Built && in_catchment(p, s.pos))
        .map(|s| (s.pos.dist2(p), s.id))
        .collect();
    v.sort();
    v.into_iter().map(|(_, id)| id).collect()
}

/// Trains committed to (or waiting to commit to) a station, excluding those already there.
pub fn station_queue(w: &World, sid: u32) -> usize {
    let pos = w.stations[sid as usize].pos;
    w.trains
        .iter()
        .filter(|t| {
            let wants = t.bound == Some(sid)
                || (t.carrier.state == VState::WaitLimit
                    && t.carrier.stop().map(|s| s.target) == Some(Target::Station(sid)));
            wants && t.head() != pos
        })
        .count()
}

fn bound_count(w: &World, sid: u32, except: u32) -> u32 {
    w.trains
        .iter()
        .filter(|t| t.carrier.id != except && t.bound == Some(sid))
        .count() as u32
}

/// Dijkstra over (tile, heading). Returns tiles after `start` up to `goal`.
fn find_path(
    w: &World,
    start: Pos,
    heading: Option<Dir>,
    goal: Pos,
    self_id: u32,
    owners: &[Option<u32>],
) -> Option<(u32, Vec<Pos>)> {
    let wd = w.map.w;
    let n = (w.map.w * w.map.h) as usize * 4;
    let key = |p: Pos, d: Dir| (p.y * wd + p.x) as usize * 4 + d.index();
    let mut best = vec![u32::MAX; n];
    let mut prev = vec![u32::MAX; n];
    let mut heap = BinaryHeap::new();
    for d in Dir::ALL {
        // first step may not reverse the current heading
        if let Some(h) = heading
            && d == h.opposite()
        {
            continue;
        }
        if !can_step(w, start, d) {
            continue;
        }
        let q = start.step(d);
        let k = key(q, d);
        best[k] = 10;
        heap.push(Reverse((10u32, q.x, q.y, d as u8)));
    }
    let dirs = [Dir::N, Dir::E, Dir::S, Dir::W];
    while let Some(Reverse((g, x, y, di))) = heap.pop() {
        let p = Pos::new(x, y);
        let d = dirs[di as usize];
        let k = key(p, d);
        if g > best[k] {
            continue;
        }
        if p == goal {
            let mut path = vec![p];
            let mut cur = k;
            while prev[cur] != u32::MAX {
                cur = prev[cur] as usize;
                let ti = cur / 4;
                path.push(Pos::new(ti as i32 % wd, ti as i32 / wd));
            }
            path.reverse();
            return Some((g, path));
        }
        for nd in Dir::ALL {
            if nd == d.opposite() || !can_step(w, p, nd) {
                continue;
            }
            let q = p.step(nd);
            let tq = w.tile(q);
            let mut c = 10;
            if tq.station.is_some() && q != goal {
                c += 40; // don't route through other stations if avoidable
            }
            let b = block_at(w, q);
            if b >= 0
                && let Some(o) = owners.get(b as usize).copied().flatten()
                && o != self_id
            {
                c += 30; // prefer free blocks (lets loops & double track work)
            }
            let ng = g + c;
            let nk = key(q, nd);
            if ng < best[nk] {
                best[nk] = ng;
                prev[nk] = k as u32;
                heap.push(Reverse((ng, q.x, q.y, nd as u8)));
            }
        }
    }
    None
}

fn can_reverse_here(w: &World, t: &Train) -> bool {
    let h = w.tile(t.head());
    h.station.is_some() || h.rail_conn.count_ones() <= 1 || t.tiles.iter().all(|p| *p == t.tiles[0])
}

/// Plan a route to `goal`, reversing the train if that's the only/better way.
fn plan(w: &World, t: &mut Train, goal: Pos, allow_reverse: bool) -> bool {
    let owners = block_owners(w);
    let fwd = find_path(w, t.head(), t.heading(), goal, t.carrier.id, &owners);
    let mut rev = None;
    if allow_reverse && t.tiles.len() > 1 {
        let mut r = t.tiles.clone();
        r.reverse();
        let tmp = Train {
            tiles: r.clone(),
            ..t.clone()
        };
        rev = find_path(w, tmp.head(), tmp.heading(), goal, t.carrier.id, &owners)
            .map(|(c, p)| (c, p, r));
    }
    let choose_rev = match (&fwd, &rev) {
        (Some((cf, _)), Some((cr, _, _))) => cr < cf,
        (None, Some(_)) => true,
        _ => false,
    };
    if choose_rev {
        let (_, p, r) = rev.unwrap();
        t.tiles = r;
        t.path = p.into_iter().rev().collect();
    } else if let Some((_, p)) = fwd {
        t.path = p.into_iter().rev().collect();
    } else {
        return false;
    }
    t.path_version = w.net_version;
    true
}

pub fn buy_train(w: &mut World, sid: u32, kind: TrainKind) -> Result<u32, String> {
    let st = &w.stations[sid as usize];
    if st.removed {
        return Err("That station was removed.".into());
    }
    let pos = st.pos;
    let b = block_at(w, pos);
    if w.trains
        .iter()
        .any(|t| t.tiles.iter().any(|p| block_at(w, *p) == b))
    {
        return Err("Another train is in this block. Wait for it to leave.".into());
    }
    let k = kind.i();
    let what = format!("a {} train", kind.name().to_lowercase());
    w.spend(
        B::LOCO_COST[k] + B::WAGON_COST[k] * B::DEFAULT_WAGONS[k] as f64,
        &what,
    )?;
    let id = w.new_id();
    let n = w.trains.len() + 1;
    let wagons = B::DEFAULT_WAGONS[k];
    let label = if kind == TrainKind::Express {
        "Express"
    } else {
        "Train"
    };
    let mut carrier = Carrier::new(
        id,
        format!("{label} {n}"),
        wagons as f64 * B::WAGON_CAPACITY[k],
    );
    carrier.born = w.time;
    w.trains.push(Train {
        carrier,
        kind,
        tiles: vec![pos; wagons + 1],
        wagons,
        deadlocked: false,
        path: vec![],
        progress: 0.0,
        path_version: 0,
        bound: None,
        reserved: None,
        chain_reserved: vec![],
        waiting_on: None,
    });
    Ok(id)
}

/// Buy another train like `id`: same kind, wagons and schedule, waiting at
/// the first of its stations that has room. Wagons it can't afford are left off.
pub fn clone_train(w: &mut World, id: u32) -> Result<u32, String> {
    let Some(t) = w.trains.iter().find(|t| t.carrier.id == id) else {
        return Err("Select a train.".into());
    };
    let (kind, wagons, schedule) = (t.kind, t.wagons, t.carrier.schedule.clone());
    let stations: Vec<(usize, u32)> = schedule
        .stops
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s.target {
            Target::Station(sid) if !w.stations[sid as usize].removed => Some((i, sid)),
            _ => None,
        })
        .collect();
    if stations.is_empty() {
        return Err("Give it a schedule first: a copy takes the same stops.".into());
    }
    let mut last = Err(String::new());
    for (stop, sid) in stations {
        last = buy_train(w, sid, kind).map(|new| (new, stop));
        if last.is_ok() || !last.as_ref().unwrap_err().starts_with("Another train") {
            break;
        }
    }
    let (new, stop) = last.map_err(|e| {
        if e.starts_with("Another train") {
            "Every station on its schedule has a train in the way. Try again in a moment."
                .to_string()
        } else {
            e
        }
    })?;
    let k = kind.i();
    let i = w.trains.iter().position(|t| t.carrier.id == new).unwrap();
    while w.trains[i].wagons < wagons && w.spend(B::WAGON_COST[k], "a wagon").is_ok() {
        let tail = *w.trains[i].tiles.last().unwrap();
        let t = &mut w.trains[i];
        t.tiles.push(tail);
        t.wagons += 1;
        t.carrier.capacity = t.wagons as f64 * B::WAGON_CAPACITY[k];
    }
    let c = &mut w.trains[i].carrier;
    c.schedule = schedule;
    c.cur = stop;
    Ok(new)
}

pub fn reverse_train(w: &mut World, id: u32) -> Result<String, String> {
    let Some(t) = w.trains.iter_mut().find(|t| t.carrier.id == id) else {
        return Err("Only trains can reverse.".into());
    };
    t.tiles.reverse();
    t.path.clear();
    t.reserved = None;
    t.chain_reserved.clear();
    t.deadlocked = false;
    t.waiting_on = None;
    t.carrier.stuck = 0.0;
    t.carrier.warned = false;
    if !matches!(t.carrier.state, VState::Working(_)) {
        t.carrier.state = VState::Travel;
    }
    Ok(format!("{} reversed.", t.carrier.name))
}

pub fn change_wagons(w: &mut World, id: u32, delta: i32) -> Result<String, String> {
    let Some(i) = w.trains.iter().position(|t| t.carrier.id == id) else {
        return Err("Select a train.".into());
    };
    let head = w.trains[i].head();
    if w.tile(head).station.is_none() || !matches!(w.trains[i].carrier.state, VState::Working(_)) {
        return Err("Wagons can only be changed while the train is stopped at a station.".into());
    }
    let cur = w.trains[i].wagons as i32;
    let k = w.trains[i].kind.i();
    let new = (cur + delta).clamp(1, B::MAX_WAGONS[k] as i32) as usize;
    if new == cur as usize {
        return Err(format!(
            "{} trains have 1 to {} wagons.",
            w.trains[i].kind.name(),
            B::MAX_WAGONS[k]
        ));
    }
    if new > cur as usize {
        w.spend(B::WAGON_COST[k], "a wagon")?;
        let tail = *w.trains[i].tiles.last().unwrap();
        w.trains[i].tiles.push(tail);
    } else {
        let cap = new as f64 * B::WAGON_CAPACITY[k];
        if w.trains[i].carrier.cargo.total() > cap {
            return Err("Unload first: the cargo won't fit in fewer wagons.".into());
        }
        w.trains[i].tiles.pop();
        w.refund(B::WAGON_COST[k]);
    }
    let t = &mut w.trains[i];
    t.wagons = new;
    t.carrier.capacity = new as f64 * B::WAGON_CAPACITY[k];
    Ok(format!(
        "{} now has {} wagons ({:.0} capacity).",
        t.carrier.name, new, t.carrier.capacity
    ))
}

pub fn tick_trains(w: &mut World, dt: f64) {
    // Pull each train out in turn so the others stay visible in `w.trains`
    // for block/collision checks, then put it back in the same slot.
    for i in 0..w.trains.len() {
        let mut t = w.trains.remove(i);
        step_train(w, &mut t, dt);
        w.trains.insert(i, t);
    }
}

fn step_train(w: &mut World, t: &mut Train, dt: f64) {
    let Some(stop) = t.carrier.stop().cloned() else {
        t.carrier.state = VState::Idle;
        t.bound = None;
        return;
    };
    if !vehicles::check_gates(w, &mut t.carrier) {
        t.bound = None;
        return;
    }
    let Target::Station(sid) = stop.target else {
        t.carrier.state = VState::NoPath;
        return;
    };
    let station = w.stations[sid as usize].clone();
    if station.removed {
        t.carrier.state = VState::NoPath;
        t.bound = None;
        return;
    }
    let dest = station.pos;
    match t.carrier.state {
        VState::Idle => t.carrier.state = VState::Travel,
        VState::Working(elapsed) => {
            if t.head() != dest {
                t.carrier.state = VState::Travel;
                return;
            }
            // Share the station's loading speed across everything in its catchment.
            let mut rate_left = B::STATION_LOAD_RATE;
            let mut moved = 0.0;
            for site in station_sites(w, dest) {
                if rate_left <= 1e-9 {
                    break;
                }
                let rate = if w.sites[site as usize].kind == SiteKind::Hub {
                    rate_left.min(B::HUB_LOAD_RATE)
                } else {
                    rate_left
                };
                let m = vehicles::transfer(w, site, &mut t.carrier, &stop, rate, dt);
                rate_left -= m.units / dt;
                moved += m.units;
                let now = w.now_sec();
                w.stations[sid as usize].stats.add(now, m.units);
                w.stations[sid as usize].income.add(now, m.paid);
            }
            let elapsed = elapsed + dt;
            if vehicles::should_leave(&t.carrier, &stop, elapsed, moved > 1e-6) {
                t.carrier.advance();
                t.bound = None;
                t.path.clear();
            } else {
                t.carrier.state = VState::Working(elapsed);
            }
        }
        VState::NoPath | VState::WaitLimit | VState::Gated => {
            t.carrier.retry -= dt;
            if t.carrier.retry <= 0.0 {
                t.carrier.state = VState::Travel;
                t.path.clear();
            }
        }
        VState::Travel | VState::Blocked => {
            if t.head() == dest {
                t.carrier.state = VState::Working(0.0);
                t.bound = Some(sid);
                t.progress = 0.0;
                return;
            }
            // Respect the destination's train limit before committing.
            if t.bound != Some(sid) {
                if bound_count(w, sid, t.carrier.id) >= station.limit {
                    t.carrier.state = VState::WaitLimit;
                    t.carrier.retry = 1.0;
                    return;
                }
                t.bound = Some(sid);
            }
            if t.path.is_empty() || t.path_version != w.net_version || t.path.first() != Some(&dest)
            {
                let rev = can_reverse_here(w, t);
                if !plan(w, t, dest, rev) {
                    t.carrier.state = VState::NoPath;
                    t.carrier.retry = 3.0;
                    t.bound = None;
                    if !t.carrier.warned {
                        t.carrier.warned = true;
                        let msg = format!("{} has no route to {}.", t.carrier.name, station.name);
                        w.alert(AlertLevel::Warn, msg, Some(t.head()));
                    }
                    return;
                }
                t.carrier.warned = false;
            }
            advance_train(w, t, dt, dest);
        }
    }
}

fn advance_train(w: &mut World, t: &mut Train, dt: f64, dest: Pos) {
    let speed = B::TRAIN_SPEED[t.kind.i()];
    t.progress = (t.progress + speed * dt).min(2.0);
    let owners = block_owners(w);
    let me = t.carrier.id;
    let free_for_me = |b: i32| {
        b < 0
            || owners
                .get(b as usize)
                .copied()
                .flatten()
                .is_none_or(|o| o == me)
    };
    while t.progress >= 1.0 {
        let Some(&next) = t.path.last() else { break };
        let Some(d) = t.head().dir_to(next) else {
            t.path.clear();
            break;
        };
        if !can_step(w, t.head(), d) {
            t.path.clear(); // track changed under us; re-plan next tick
            break;
        }
        // Physical collision with another train's tiles.
        if let Some(other) = w.trains.iter().find(|o| o.tiles.contains(&next)) {
            block(t, other.carrier.id);
            return;
        }
        let nt = w.tile(next);
        if let Some(sig) = nt.signal {
            // Need the block beyond the signal (and, for a chain signal, the way out of it).
            let beyond = t.path.get(t.path.len().wrapping_sub(2)).copied();
            let b = beyond.map(|q| block_at(w, q)).unwrap_or(-1);
            let exits = if sig.is_chain() {
                chain_exits(w, &t.path)
            } else {
                vec![]
            };
            if let Some(busy) = std::iter::once(b)
                .chain(exits.iter().copied())
                .find(|x| !free_for_me(*x))
            {
                let holder = owners[busy as usize].unwrap();
                block(t, holder);
                return;
            }
            t.reserved = Some(b);
            t.chain_reserved = exits;
        } else {
            let b = block_at(w, next);
            if !free_for_me(b) {
                let holder = owners[b as usize].unwrap();
                block(t, holder);
                return;
            }
            if t.reserved == Some(b) {
                t.reserved = None; // now physically inside it
            }
            t.chain_reserved.retain(|x| *x != b);
        }
        // Move.
        t.path.pop();
        t.tiles.insert(0, next);
        t.tiles.pop();
        t.progress -= 1.0;
        w.add_flow(next, t.carrier.cargo.total());
        t.carrier.state = VState::Travel;
        t.carrier.stuck = 0.0;
        t.waiting_on = None;
        t.deadlocked = false;
        if next == dest {
            t.carrier.state = VState::Working(0.0);
            t.progress = 0.0;
            t.path.clear();
            return;
        }
    }
    if t.carrier.state == VState::Blocked {
        t.carrier.stuck += dt;
    }
}

/// Blocks a train passing the chain signal at the front of `path` (stored
/// reversed, next tile last) must also find free: past each following signal,
/// for as long as those signals are chain signals too.
fn chain_exits(w: &World, path: &[Pos]) -> Vec<i32> {
    let fwd: Vec<Pos> = path.iter().rev().copied().collect();
    let mut out = vec![];
    for i in 2..fwd.len() {
        let Some(sig) = w.tile(fwd[i]).signal else {
            continue;
        };
        let Some(&after) = fwd.get(i + 1) else { break };
        let b = block_at(w, after);
        if b >= 0 {
            out.push(b);
        }
        if !sig.is_chain() {
            break;
        }
    }
    out
}

fn block(t: &mut Train, holder: u32) {
    t.carrier.state = VState::Blocked;
    t.waiting_on = Some(holder);
    t.progress = t.progress.min(1.0);
    t.carrier.stuck += B::TICK_DT;
}

/// Once per second: find wait-for cycles (deadlocks) and long jams.
pub fn check_deadlocks(w: &mut World) {
    let ids: Vec<u32> = w.trains.iter().map(|t| t.carrier.id).collect();
    let waits: Vec<Option<u32>> = w
        .trains
        .iter()
        .map(|t| {
            if t.carrier.state == VState::Blocked {
                t.waiting_on
            } else {
                None
            }
        })
        .collect();
    let idx = |id: u32| ids.iter().position(|x| *x == id);
    let mut in_cycle = vec![false; ids.len()];
    for start in 0..ids.len() {
        // Walk the chain; a cycle exists if we revisit a node.
        let mut seen = vec![start];
        let mut cur = start;
        while let Some(next) = waits[cur].and_then(idx) {
            if let Some(pos) = seen.iter().position(|s| *s == next) {
                for s in &seen[pos..] {
                    in_cycle[*s] = true;
                }
                break;
            }
            seen.push(next);
            cur = next;
        }
    }
    let mut newly = vec![];
    for (i, t) in w.trains.iter_mut().enumerate() {
        if in_cycle[i] && !t.deadlocked {
            newly.push((t.carrier.name.clone(), t.head()));
        }
        t.deadlocked = in_cycle[i];
    }
    if !newly.is_empty() {
        w.stats.deadlocks += 1;
        let names: Vec<String> = newly.iter().map(|n| n.0.clone()).collect();
        w.alert(
            AlertLevel::Danger,
            format!("DEADLOCK! {} are waiting on each other forever. Select one ({{next_vehicle}}) and reverse ({{reverse}}) or scrap ({{demolish}}) it.", names.join(", ")),
            Some(newly[0].1),
        );
    }
    // Jams: long waits that aren't (yet) deadlocks.
    let mut jams = vec![];
    for t in w.trains.iter_mut() {
        if t.carrier.state == VState::Blocked
            && t.carrier.stuck > B::JAM_WARN_SECS
            && !t.carrier.warned
            && !t.deadlocked
        {
            t.carrier.warned = true;
            jams.push((t.carrier.name.clone(), t.head()));
        }
    }
    for (name, pos) in jams {
        w.alert(
            AlertLevel::Warn,
            format!(
                "{name} has been stuck at a signal for {:.0}s. Jam ahead?",
                B::JAM_WARN_SECS
            ),
            Some(pos),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::build::{self, LayerKind};

    /// Lay a straight built rail line of `len` tiles eastwards from `a` (forced, any terrain),
    /// clearing any sites in the way.
    fn line(w: &mut World, a: Pos, len: i32) -> Vec<Pos> {
        let path: Vec<Pos> = (0..len).map(|i| Pos::new(a.x + i, a.y)).collect();
        for p in &path {
            w.tile_mut(*p).site = None;
        }
        build::lay_path(w, &path, LayerKind::Rail);
        rebuild_blocks(w);
        path
    }

    fn station(w: &mut World, p: Pos, name: &str) -> u32 {
        let id = w.stations.len() as u32;
        w.stations.push(Station {
            id,
            name: name.into(),
            pos: p,
            limit: 1,
            stats: Throughput::default(),
            income: Throughput::default(),
            removed: false,
        });
        w.tile_mut(p).station = Some(id);
        id
    }

    #[test]
    fn train_shuttles_between_two_stations() {
        let mut w = World::new(5, Settings::default());
        let a = Pos::new(20, 10);
        let path = line(&mut w, a, 20);
        let sa = station(&mut w, path[0], "A");
        let sb = station(&mut w, *path.last().unwrap(), "B");
        let id = buy_train(&mut w, sa, TrainKind::Freight).unwrap();
        let c = w.carrier_mut(id).unwrap();
        let mut s1 = Stop::new(Target::Station(sa), Action::Load);
        s1.cond = Cond::Time(2.0);
        let mut s2 = Stop::new(Target::Station(sb), Action::Unload);
        s2.cond = Cond::Time(2.0);
        c.schedule.stops = vec![s1, s2];
        let mut reached_b = false;
        let mut back_at_a = false;
        for _ in 0..400 {
            w.tick();
            let t = &w.trains[0];
            if t.head() == path[19] {
                reached_b = true;
            }
            if reached_b && t.head() == path[0] {
                back_at_a = true;
            }
        }
        assert!(reached_b, "train never reached B");
        assert!(back_at_a, "train never reversed back to A");
    }

    #[test]
    fn head_on_trains_on_single_track_deadlock_and_are_detected() {
        let mut w = World::new(5, Settings::default());
        let a = Pos::new(20, 12);
        let path = line(&mut w, a, 24);
        // signals every 6 tiles
        for i in [6usize, 12, 18] {
            w.tile_mut(path[i]).signal = Some(SignalKind::Both);
        }
        rebuild_blocks(&mut w);
        let sa = station(&mut w, path[0], "West");
        let sb = station(&mut w, path[23], "East");
        let t1 = buy_train(&mut w, sa, TrainKind::Freight).unwrap();
        let t2 = buy_train(&mut w, sb, TrainKind::Freight).unwrap();
        w.carrier_mut(t1).unwrap().schedule.stops =
            vec![Stop::new(Target::Station(sb), Action::Unload)];
        w.carrier_mut(t2).unwrap().schedule.stops =
            vec![Stop::new(Target::Station(sa), Action::Unload)];
        // Station limits must allow both to commit.
        w.stations[sa as usize].limit = 2;
        w.stations[sb as usize].limit = 2;
        let mut detected = false;
        for _ in 0..600 {
            w.tick();
            if w.trains.iter().all(|t| t.deadlocked) {
                detected = true;
                break;
            }
        }
        assert!(detected, "deadlock not detected");
        assert!(w.alerts.iter().any(|a| a.text.contains("DEADLOCK")));
        // Recovery: reversing one train lets the other through eventually.
        reverse_train(&mut w, t1).unwrap();
        w.carrier_mut(t1).unwrap().schedule.stops =
            vec![Stop::new(Target::Station(sa), Action::Unload)];
        for _ in 0..600 {
            w.tick();
        }
        assert!(
            !w.trains.iter().any(|t| t.deadlocked),
            "still deadlocked after recovery"
        );
    }

    #[test]
    fn train_limit_holds_second_train() {
        let mut w = World::new(5, Settings::default());
        let path = line(&mut w, Pos::new(20, 16), 30);
        for i in [8usize, 16, 24] {
            w.tile_mut(path[i]).signal = Some(SignalKind::Both);
        }
        rebuild_blocks(&mut w);
        let home = station(&mut w, path[0], "Home");
        let far = station(&mut w, path[29], "Far");
        let mid = station(&mut w, path[12], "Mid");
        let t1 = buy_train(&mut w, home, TrainKind::Freight).unwrap();
        let t2 = buy_train(&mut w, mid, TrainKind::Freight).unwrap();
        // Both want Far (limit 1); t1 must wait while t2 is bound there.
        let mut s = Stop::new(Target::Station(far), Action::Unload);
        s.cond = Cond::Time(500.0);
        s.timeout = None;
        w.carrier_mut(t2).unwrap().schedule.stops = vec![s.clone()];
        for _ in 0..5 {
            w.tick();
        }
        w.carrier_mut(t1).unwrap().schedule.stops = vec![s];
        for _ in 0..300 {
            w.tick();
        }
        let a = w.trains.iter().find(|t| t.carrier.id == t1).unwrap();
        assert_eq!(a.carrier.state, VState::WaitLimit);
        assert_eq!(station_queue(&w, far), 1);
    }

    #[test]
    fn one_way_signal_blocks_wrong_direction() {
        let mut w = World::new(5, Settings::default());
        let path = line(&mut w, Pos::new(20, 14), 10);
        w.tile_mut(path[5]).signal = Some(SignalKind::OneWay(Dir::E));
        rebuild_blocks(&mut w);
        assert!(can_step(&w, path[5], Dir::E));
        assert!(!can_step(&w, path[5], Dir::W));
    }

    /// A train at a block signal waits in front of the busy block; at a chain
    /// signal it waits one block earlier, keeping the block between clear.
    fn stop_point(first: SignalKind) -> usize {
        let mut w = World::new(5, Settings::default());
        let path = line(&mut w, Pos::new(20, 22), 24);
        w.tile_mut(path[6]).signal = Some(first);
        w.tile_mut(path[12]).signal = Some(SignalKind::Both);
        rebuild_blocks(&mut w);
        let home = station(&mut w, path[0], "Home");
        let far = station(&mut w, path[23], "Far");
        let parked = station(&mut w, path[18], "Siding");
        w.stations[far as usize].limit = 2;
        buy_train(&mut w, parked, TrainKind::Freight).unwrap(); // sits in the last block, no schedule
        let a = buy_train(&mut w, home, TrainKind::Freight).unwrap();
        w.carrier_mut(a).unwrap().schedule.stops =
            vec![Stop::new(Target::Station(far), Action::Unload)];
        for _ in 0..600 {
            w.tick();
        }
        let head = w.trains.iter().find(|t| t.carrier.id == a).unwrap().head();
        path.iter().position(|p| *p == head).unwrap()
    }

    #[test]
    fn chain_signal_holds_trains_back_a_block() {
        assert_eq!(
            stop_point(SignalKind::Both),
            11,
            "block signal: wait at the busy block"
        );
        assert_eq!(
            stop_point(SignalKind::Chain),
            5,
            "chain signal: wait before the chain"
        );
    }

    /// Ticks for a fresh train of `kind` to run the length of a 30-tile line.
    fn run_time(kind: TrainKind) -> u32 {
        let mut w = World::new(5, Settings::default());
        let path = line(&mut w, Pos::new(20, 18), 30);
        let a = station(&mut w, path[0], "A");
        let b = station(&mut w, path[29], "B");
        let id = buy_train(&mut w, a, kind).unwrap();
        w.carrier_mut(id).unwrap().schedule.stops =
            vec![Stop::new(Target::Station(b), Action::Unload)];
        for i in 0..2000 {
            w.tick();
            if w.trains[0].head() == path[29] {
                return i;
            }
        }
        panic!("{kind:?} never arrived");
    }

    #[test]
    fn express_trains_are_faster() {
        assert!(run_time(TrainKind::Express) * 3 < run_time(TrainKind::Freight) * 2);
    }

    #[test]
    fn swap_drops_off_then_picks_up_passengers() {
        let mut w = World::new(5, Settings::default());
        let town = w.towns[0].site;
        w.sites[town as usize].inv[Cargo::Passengers] = 30.0;
        let mut c = Carrier::new(1, "T".into(), 40.0);
        c.cargo[Cargo::Passengers] = 20.0;
        let far = crate::types::Lot::at(Pos::new(w.towns[0].pos.x + 40, w.towns[0].pos.y), w.time);
        c.origins.0[Cargo::Passengers as usize] = far;
        let stop = Stop::new(Target::Site(town), Action::Swap);
        let mut paid = 0.0;
        for _ in 0..40 {
            paid += vehicles::transfer(
                &mut w,
                town,
                &mut c,
                &stop,
                B::STATION_LOAD_RATE,
                B::TICK_DT,
            )
            .paid;
        }
        assert!(paid > 0.0, "arriving passengers pay");
        assert!(w.towns[0].arrived >= 19.9, "they got off");
        assert!(c.cargo[Cargo::Passengers] > 25.0, "and new ones got on");
        let lot = c.origins.get(Cargo::Passengers);
        assert!(
            lot.dist(w.towns[0].pos) < 1.0,
            "the new passengers start their trip here"
        );
    }
}
