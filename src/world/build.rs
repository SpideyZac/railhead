//! Construction: planning and laying roads/rail, stations, hubs, signals and
//! demolition. Building is instant.

use super::*;
use crate::{map::Terrain, path, types::Dir};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum LayerKind {
    Road,
    Rail,
}

impl LayerKind {
    pub fn name(self) -> &'static str {
        match self {
            LayerKind::Road => "road",
            LayerKind::Rail => "rail",
        }
    }
}

pub fn layer_cost(t: Terrain, kind: LayerKind) -> f64 {
    match kind {
        LayerKind::Road => B::ROAD_COST[t.index()],
        LayerKind::Rail => B::RAIL_COST[t.index()],
    }
}

pub fn site_cost(kind: SiteKind) -> f64 {
    match kind {
        SiteKind::Hub => B::HUB_COST,
        _ => 0.0,
    }
}

fn layer(t: &Tile, k: LayerKind) -> Layer {
    match k {
        LayerKind::Road => t.road,
        LayerKind::Rail => t.rail,
    }
}
fn layer_mut(t: &mut Tile, k: LayerKind) -> &mut Layer {
    match k {
        LayerKind::Road => &mut t.road,
        LayerKind::Rail => &mut t.rail,
    }
}

/// Lay a layer along a path and connect consecutive tiles.
pub fn lay_path(w: &mut World, path: &[Pos], kind: LayerKind) {
    for (i, p) in path.iter().enumerate() {
        *layer_mut(w.tile_mut(*p), kind) = Layer::Built;
        if i + 1 < path.len()
            && let Some(d) = p.dir_to(path[i + 1])
        {
            connect(w, *p, d, kind);
        }
    }
    if kind == LayerKind::Rail {
        rail::rebuild_blocks(w);
    }
    w.net_version += 1;
}

fn connect(w: &mut World, p: Pos, d: Dir, kind: LayerKind) {
    let q = p.step(d);
    match kind {
        LayerKind::Road => {
            w.tile_mut(p).road_conn |= d.bit();
            w.tile_mut(q).road_conn |= d.opposite().bit();
        }
        LayerKind::Rail => {
            w.tile_mut(p).rail_conn |= d.bit();
            w.tile_mut(q).rail_conn |= d.opposite().bit();
        }
    }
}

/// Find a route for a new road/rail line from `a` to `b`: cheap to build,
/// but also short and straight (`ROUTE_*` in balance.rs). Existing track is
/// reused only when it's about as direct, and rail never runs through a
/// station unless the line starts or ends there.
pub fn plan_path(w: &World, a: Pos, b: Pos, kind: LayerKind) -> Result<Vec<Pos>, String> {
    if !w.map.in_bounds(a) || !w.map.in_bounds(b) {
        return Err("That's off the map.".into());
    }
    let blocked_by_site = |p: Pos| kind == LayerKind::Rail && w.tile(p).site.is_some();
    if blocked_by_site(a) || blocked_by_site(b) {
        return Err("Rail can't go through buildings (place a station next to them).".into());
    }
    let plains = layer_cost(Terrain::Plains, kind);
    let reuse = ((B::ROUTE_TILE + plains * B::ROUTE_REUSE) * 10.0) as u32;
    let route = path::astar_turns(
        w.map.w,
        w.map.h,
        a,
        b,
        reuse,
        (B::ROUTE_TURN * 10.0) as u32,
        |_, q| {
            let t = w.tile(q);
            if blocked_by_site(q)
                || (kind == LayerKind::Rail && t.station.is_some() && q != a && q != b)
            {
                return None;
            }
            if layer(t, kind) == Layer::Built {
                return Some(reuse);
            }
            Some(((layer_cost(w.map.terrain(q), kind) + B::ROUTE_TILE) * 10.0) as u32)
        },
    );
    route.ok_or_else(|| "No buildable route between those points.".into())
}

pub fn path_cost(w: &World, path: &[Pos], kind: LayerKind) -> f64 {
    path.iter()
        .filter(|p| layer(w.tile(**p), kind) == Layer::None)
        .map(|p| layer_cost(w.map.terrain(*p), kind))
        .sum()
}

/// Pay for and build a planned route. Returns what the new tiles cost.
pub fn commit_path(w: &mut World, path: &[Pos], kind: LayerKind) -> Result<f64, String> {
    let cost = path_cost(w, path, kind);
    w.spend(cost, &format!("this {}", kind.name()))?;
    lay_path(w, path, kind);
    Ok(cost)
}

pub fn place_station(w: &mut World, p: Pos) -> Result<String, String> {
    let t = w.tile(p);
    if !t.rail.present() {
        return Err("Stations go on rail track. Lay track first.".into());
    }
    if t.station.is_some() {
        return Err("There's already a station here.".into());
    }
    if t.rail_conn.count_ones() > 2 {
        return Err("Stations can't sit on junctions.".into());
    }
    w.spend(B::STATION_COST, "a station")?;
    let id = w.stations.len() as u32;
    let name = station_name(w, p);
    w.stations.push(Station {
        id,
        name: name.clone(),
        pos: p,
        limit: 1,
        stats: Throughput::default(),
        income: Throughput::default(),
        removed: false,
    });
    w.tile_mut(p).station = Some(id);
    w.net_version += 1;
    Ok(format!("Station \"{name}\" is open."))
}

pub fn place_site(w: &mut World, p: Pos, kind: SiteKind) -> Result<String, String> {
    let t = w.tile(p);
    if t.site.is_some() || t.rail.present() {
        return Err("Tile is occupied.".into());
    }
    if matches!(w.map.terrain(p), Terrain::Water | Terrain::Mountains) {
        return Err("Buildings need solid, flat-ish ground.".into());
    }
    w.spend(site_cost(kind), kind.name())?;
    w.add_site(kind, p, Layer::Built);
    w.net_version += 1;
    Ok(format!("{} built.", kind.name()))
}

/// Cycle the signal on a rail tile: none -> two-way -> one-way (each way) -> none.
pub fn cycle_signal(w: &mut World, p: Pos) -> Result<String, String> {
    let t = w.tile(p).clone();
    if !t.rail.present() {
        return Err("Signals go on rail track.".into());
    }
    if t.rail_conn.count_ones() != 2 || t.station.is_some() {
        return Err(
            "Signals need a plain piece of track (not a junction, end, or station).".into(),
        );
    }
    let dirs: Vec<Dir> = Dir::ALL
        .into_iter()
        .filter(|d| t.rail_conn & d.bit() != 0)
        .collect();
    // one-way signals name the direction trains may travel; chain signals stay chain
    let chain = t.signal.is_some_and(|s| s.is_chain());
    let plain = t.signal.map(|s| if chain { s.toggled_chain() } else { s });
    let next = match plain {
        None => Some(SignalKind::Both),
        Some(SignalKind::Both) => Some(SignalKind::OneWay(dirs[0])),
        Some(SignalKind::OneWay(d)) if d == dirs[0] => Some(SignalKind::OneWay(dirs[1])),
        _ => None,
    }
    .map(|s| if chain { s.toggled_chain() } else { s });
    if t.signal.is_none() {
        w.spend(B::SIGNAL_COST, "a signal")?;
    }
    w.tile_mut(p).signal = next;
    rail::rebuild_blocks(w);
    w.net_version += 1;
    Ok(signal_message(next))
}

fn signal_message(s: Option<SignalKind>) -> String {
    match s {
        None => "Signal removed.".into(),
        Some(SignalKind::Both) => "Two-way block signal. (c makes it a chain signal.)".into(),
        Some(SignalKind::OneWay(d)) => format!("One-way signal: trains may only travel {:?}.", d),
        Some(SignalKind::Chain) => {
            "Two-way CHAIN signal: trains wait here unless they can get out the other side.".into()
        }
        Some(SignalKind::ChainOneWay(d)) => format!(
            "One-way CHAIN signal ({:?}): trains wait here unless they can get out the other side.",
            d
        ),
    }
}

/// Switch the signal on a tile between block and chain (free).
pub fn toggle_chain(w: &mut World, p: Pos) -> Result<String, String> {
    let Some(s) = w.tile(p).signal else {
        return Err("Put the cursor on a signal first (g places one).".into());
    };
    let next = s.toggled_chain();
    w.tile_mut(p).signal = Some(next);
    w.net_version += 1;
    Ok(signal_message(Some(next)))
}

/// Auto-signal helper: walk the line through `p` and place two-way signals
/// every `spacing` tiles, plus signals guarding every junction it touches.
pub fn auto_signal(w: &mut World, p: Pos, spacing: usize) -> Result<usize, String> {
    if !w.tile(p).rail.present() {
        return Err("Put the cursor on rail track first.".into());
    }
    // Collect the connected rail network from p.
    let seen = path::flood(w.map.w, w.map.h, p, |q| w.tile(q).rail.present());
    let mut placed = 0;
    let candidates: Vec<Pos> = (0..w.map.h)
        .flat_map(|y| (0..w.map.w).map(move |x| Pos::new(x, y)))
        .filter(|q| seen[w.map.idx(*q)])
        .collect();
    let is_plain = |w: &World, q: Pos| {
        let t = w.tile(q);
        t.rail_conn.count_ones() == 2 && t.station.is_none() && t.signal.is_none()
    };
    let is_special = |w: &World, q: Pos| {
        let t = w.tile(q);
        t.rail_conn.count_ones() >= 3 || t.station.is_some()
    };
    let mut want: Vec<Pos> = Vec::new();
    // Junction and station guards: the first plain tile out from each.
    for &q in &candidates {
        if !is_special(w, q) {
            continue;
        }
        for d in Dir::ALL {
            if w.tile(q).rail_conn & d.bit() == 0 {
                continue;
            }
            let n = q.step(d);
            if is_plain(w, n) && !want.contains(&n) {
                want.push(n);
            }
        }
    }
    // Spacing along long plain runs: distance-from-special via BFS.
    let mut dist = vec![usize::MAX; candidates.len()];
    let index_of = |q: Pos| {
        candidates
            .binary_search_by_key(&(q.y, q.x), |c| (c.y, c.x))
            .ok()
    };
    let mut queue = std::collections::VecDeque::new();
    for (i, q) in candidates.iter().enumerate() {
        if is_special(w, *q)
            || want.contains(q)
            || w.tile(*q).signal.is_some()
            || w.tile(*q).rail_conn.count_ones() == 1
        {
            dist[i] = 0;
            queue.push_back(*q);
        }
    }
    while let Some(q) = queue.pop_front() {
        let dq = dist[index_of(q).unwrap()];
        for d in Dir::ALL {
            if w.tile(q).rail_conn & d.bit() == 0 {
                continue;
            }
            let n = q.step(d);
            if let Some(ni) = index_of(n)
                && dist[ni] == usize::MAX
            {
                let nd = if dq + 1 >= spacing && is_plain(w, n) {
                    0
                } else {
                    dq + 1
                };
                if nd == 0 {
                    want.push(n);
                }
                dist[ni] = nd;
                queue.push_back(n);
            }
        }
    }
    for q in want {
        if w.tile(q).signal.is_none()
            && is_plain(w, q)
            && w.spend(B::SIGNAL_COST, "a signal").is_ok()
        {
            w.tile_mut(q).signal = Some(SignalKind::Both);
            placed += 1;
        }
    }
    rail::rebuild_blocks(w);
    w.net_version += 1;
    Ok(placed)
}

/// Remove the top-most thing on a tile. Returns a message.
pub fn demolish(w: &mut World, p: Pos) -> Result<String, String> {
    if w.trains.iter().any(|t| t.tiles.contains(&p)) {
        return Err("A train is standing here. Move or remove it first.".into());
    }
    let t = w.tile(p).clone();
    if let Some(sid) = t.station {
        w.stations[sid as usize].removed = true;
        w.tile_mut(p).station = None;
        w.net_version += 1;
        w.refund(B::STATION_COST);
        return Ok("Station removed.".into());
    }
    if t.signal.is_some() {
        w.tile_mut(p).signal = None;
        w.refund(B::SIGNAL_COST);
        rail::rebuild_blocks(w);
        w.net_version += 1;
        return Ok("Signal removed.".into());
    }
    if let Some(id) = t.site {
        let kind = w.sites[id as usize].kind;
        if !matches!(kind, SiteKind::Hub) {
            return Err("Towns and industries can't be demolished.".into());
        }
        w.sites[id as usize].state = Layer::None;
        w.tile_mut(p).site = None;
        if t.road_conn == 0 {
            w.tile_mut(p).road = Layer::None;
        }
        w.net_version += 1;
        w.refund(site_cost(kind));
        return Ok(format!("{} demolished.", kind.name()));
    }
    for k in [LayerKind::Rail, LayerKind::Road] {
        if layer(&t, k).present() {
            w.refund(layer_cost(w.map.terrain(p), k));
            clear_layer(w, p, k);
            return Ok("Demolished.".into());
        }
    }
    Err("Nothing to demolish here.".into())
}

fn clear_layer(w: &mut World, p: Pos, k: LayerKind) {
    *layer_mut(w.tile_mut(p), k) = Layer::None;
    for d in Dir::ALL {
        let q = p.step(d);
        if !w.map.in_bounds(q) {
            continue;
        }
        match k {
            LayerKind::Road => {
                w.tile_mut(p).road_conn &= !d.bit();
                w.tile_mut(q).road_conn &= !d.opposite().bit();
            }
            LayerKind::Rail => {
                w.tile_mut(p).rail_conn &= !d.bit();
                w.tile_mut(q).rail_conn &= !d.opposite().bit();
            }
        }
    }
    if k == LayerKind::Rail {
        w.tile_mut(p).signal = None;
        if let Some(sid) = w.tile(p).station {
            w.stations[sid as usize].removed = true;
            w.tile_mut(p).station = None;
        }
        rail::rebuild_blocks(w);
    }
    w.net_version += 1;
}

/// Name a new station after what it serves: the town if there is one, else
/// the nearest industry or hub, else "<region> Halt".
fn station_name(w: &World, p: Pos) -> String {
    let region = &w.map.provinces[w.map.province_at(p)].name;
    let sites = rail::station_sites(w, p);
    let town = sites.iter().find_map(|s| match w.sites[*s as usize].kind {
        SiteKind::Town(t) => Some(w.towns[t].name.clone()),
        _ => None,
    });
    let base = town.unwrap_or_else(|| match sites.first().map(|s| w.sites[*s as usize].kind) {
        Some(SiteKind::Industry(k)) => format!("{region} {}", k.name()),
        Some(SiteKind::Hub) => format!("{region} Hub"),
        _ => format!("{region} Halt"),
    });
    let n = w
        .stations
        .iter()
        .filter(|s| s.name.starts_with(&base))
        .count();
    if n == 0 {
        base
    } else {
        format!("{base} {}", n + 1)
    }
}

/// The free station stub at the home town: a short straight piece of track
/// two tiles from the town center (inside its catchment), with a station on it.
pub fn starter_station(w: &mut World) {
    let home = w.towns[w.map.home].pos;
    let ok = |w: &World, q: Pos| {
        w.map.in_bounds(q)
            && matches!(
                w.map.terrain(q),
                Terrain::Plains | Terrain::Forest | Terrain::Hills
            )
            && w.tile(q).site.is_none()
            && !w.tile(q).road.present()
    };
    for (dx, dy, d) in [
        (0, 2, Dir::E),
        (0, -2, Dir::E),
        (2, 0, Dir::S),
        (-2, 0, Dir::S),
    ] {
        for shift in [0, -1, -2, 1] {
            let start = Pos::new(
                home.x + dx + if d == Dir::E { shift } else { 0 },
                home.y + dy + if d == Dir::S { shift } else { 0 },
            );
            let path: Vec<Pos> = (0..4)
                .map(|i| (0..i).fold(start, |p, _| p.step(d)))
                .collect();
            if path.iter().all(|q| ok(w, *q)) && rail::in_catchment(path[0], home) {
                lay_path(w, &path, LayerKind::Rail);
                let id = w.stations.len() as u32;
                let name = station_name(w, path[0]);
                w.stations.push(Station {
                    id,
                    name,
                    pos: path[0],
                    limit: 1,
                    stats: Throughput::default(),
                    income: Throughput::default(),
                    removed: false,
                });
                w.tile_mut(path[0]).station = Some(id);
                return;
            }
        }
    }
}

/// A short, straight rail route from `a` to `b` that never touches existing track or
/// buildings, so it becomes a line of its own.
pub fn plan_separate(w: &World, a: Pos, b: Pos) -> Option<Vec<Pos>> {
    let free = |q: Pos| !w.tile(q).rail.present() && w.tile(q).site.is_none();
    if !free(a) || !free(b) {
        return None;
    }
    let min = ((layer_cost(Terrain::Plains, LayerKind::Rail) + B::ROUTE_TILE) * 10.0) as u32;
    path::astar_turns(
        w.map.w,
        w.map.h,
        a,
        b,
        min,
        (B::ROUTE_TURN * 10.0) as u32,
        |_, q| {
            free(q).then(|| {
                ((layer_cost(w.map.terrain(q), LayerKind::Rail) + B::ROUTE_TILE) * 10.0) as u32
            })
        },
    )
}

/// Plan a double-track mainline from `a` to `b`: the first line is the usual
/// cheapest route, the second hugs it one tile to the side. Neither line may
/// touch existing track, so the two stay separate.
pub fn plan_double(w: &World, a: Pos, b: Pos) -> Result<(Vec<Pos>, Vec<Pos>), String> {
    if a.manhattan(b) < 4 {
        return Err("Double track needs a longer run (at least 4 tiles).".into());
    }
    let free = |q: Pos| !w.tile(q).rail.present() && w.tile(q).site.is_none();
    let first = plan_separate(w, a, b)
        .ok_or("No route for double track there (it can't cross existing track).")?;
    let n = (w.map.w * w.map.h) as usize;
    let mut on_first = vec![false; n];
    for p in &first {
        on_first[w.map.idx(*p)] = true;
    }
    let beside = |q: Pos| {
        Dir::ALL.iter().any(|d| {
            let r = q.step(*d);
            w.map.in_bounds(r) && on_first[w.map.idx(r)]
        })
    };
    // The second line starts and ends beside the first line's ends.
    let side = |p: Pos| {
        Dir::ALL
            .iter()
            .map(|d| p.step(*d))
            .find(|q| w.map.in_bounds(*q) && free(*q) && !on_first[w.map.idx(*q)])
    };
    let (a2, b2) = (
        side(a).ok_or("No room beside the start.")?,
        side(b).ok_or("No room beside the end.")?,
    );
    let second = path::astar(
        w.map.w,
        w.map.h,
        a2,
        |p| p == b2,
        b2,
        3,
        |_, q| {
            if !free(q) || on_first[w.map.idx(q)] {
                return None;
            }
            let base = (layer_cost(w.map.terrain(q), LayerKind::Rail) * 10.0) as u32 + 4;
            Some(if beside(q) { base } else { base * 4 }) // hug the first line
        },
    )
    .ok_or("No room for the second line alongside.")?;
    Ok((first, second))
}

/// Cost of a double-track plan: both lines plus their signals.
pub fn double_cost(w: &World, lines: &(Vec<Pos>, Vec<Pos>)) -> f64 {
    let signals = (lines.0.len() + lines.1.len()) / B::DOUBLE_SIGNAL_SPACING;
    path_cost(w, &lines.0, LayerKind::Rail)
        + path_cost(w, &lines.1, LayerKind::Rail)
        + signals as f64 * B::SIGNAL_COST
}

/// Build a double-track plan: line one runs a→b, line two runs back, each
/// with one-way signals every `DOUBLE_SIGNAL_SPACING` tiles.
pub fn commit_double(w: &mut World, lines: &(Vec<Pos>, Vec<Pos>)) -> Result<f64, String> {
    let cost = double_cost(w, lines);
    w.spend(cost, "this double track")?;
    lay_path(w, &lines.0, LayerKind::Rail);
    lay_path(w, &lines.1, LayerKind::Rail);
    let back: Vec<Pos> = lines.1.iter().rev().copied().collect();
    for line in [&lines.0, &back] {
        // skip the ends so the player can join them up freely
        for i in (2..line.len().saturating_sub(2)).step_by(B::DOUBLE_SIGNAL_SPACING) {
            let (p, next) = (line[i], line[i + 1]);
            if w.tile(p).rail_conn.count_ones() == 2
                && w.tile(p).station.is_none()
                && let Some(d) = p.dir_to(next)
            {
                w.tile_mut(p).signal = Some(SignalKind::OneWay(d));
            }
        }
    }
    rail::rebuild_blocks(w);
    w.net_version += 1;
    Ok(cost)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_track_makes_two_one_way_lines() {
        let mut w = World::new(11, Settings::default());
        w.cash = 1e9;
        // find a clear stretch of land
        let (a, b) = (0..w.map.h - 1)
            .flat_map(|y| (0..w.map.w - 30).map(move |x| (Pos::new(x, y), Pos::new(x + 25, y))))
            .find(|(a, b)| plan_double(&w, *a, *b).is_ok())
            .expect("some clear stretch");
        let lines = plan_double(&w, a, b).unwrap();
        assert!(
            lines.0.iter().all(|p| !lines.1.contains(p)),
            "the lines never share a tile"
        );
        commit_double(&mut w, &lines).unwrap();
        let one_ways = lines
            .0
            .iter()
            .chain(lines.1.iter())
            .filter(|p| matches!(w.tile(**p).signal, Some(SignalKind::OneWay(_))))
            .count();
        assert!(one_ways >= 4, "both lines get one-way signals");
        // the two lines are separate pieces of track
        assert_ne!(
            rail::block_at(&w, lines.0[1]),
            rail::block_at(&w, lines.1[1])
        );
    }

    /// A clear patch of buildable land, `w`x`h`, with nothing built on it.
    fn clear_patch(w: &World, pw: i32, ph: i32) -> Pos {
        let ok = |p: Pos| {
            let t = w.tile(p);
            matches!(w.map.terrain(p), Terrain::Plains)
                && t.site.is_none()
                && !t.rail.present()
                && !t.road.present()
        };
        (1..w.map.h - ph - 1)
            .flat_map(|y| (1..w.map.w - pw - 1).map(move |x| Pos::new(x, y)))
            .find(|o| (0..ph).all(|dy| (0..pw).all(|dx| ok(Pos::new(o.x + dx, o.y + dy)))))
            .expect("a clear patch of plains")
    }

    /// The owner's complaint: a new line that meets existing track used to
    /// hop onto it (it was almost free) and ride it, even back past a
    /// station. Now it crosses straight over.
    #[test]
    fn new_lines_cross_old_ones_instead_of_riding_them() {
        let mut w = World::new(
            11,
            Settings {
                mode: crate::world::GameMode::Sandbox,
                ..Settings::default()
            },
        );
        let o = clear_patch(&w, 14, 13);
        // an existing north-south line with a station at its south end
        let old = plan_path(
            &w,
            Pos::new(o.x + 3, o.y),
            Pos::new(o.x + 3, o.y + 12),
            LayerKind::Rail,
        )
        .unwrap();
        commit_path(&mut w, &old, LayerKind::Rail).unwrap();
        place_station(&mut w, Pos::new(o.x + 3, o.y + 12)).unwrap();
        // a new east-west line across it
        let (a, b) = (Pos::new(o.x, o.y + 6), Pos::new(o.x + 13, o.y + 6));
        let p = plan_path(&w, a, b, LayerKind::Rail).unwrap();
        assert_eq!(p.len(), 14, "straight across: {p:?}");
        assert!(p.iter().all(|q| q.y == a.y), "never rides the old line");
        // a new line running alongside the old one, two tiles away, stays its own line
        let (a, b) = (Pos::new(o.x + 5, o.y + 1), Pos::new(o.x + 5, o.y + 11));
        let side = plan_path(&w, a, b, LayerKind::Rail).unwrap();
        assert!(
            side.iter().all(|q| q.x == a.x),
            "runs parallel instead of hopping onto the old line: {side:?}"
        );
        // and it won't run through a station that isn't one of its ends
        let p = plan_path(
            &w,
            Pos::new(o.x + 2, o.y + 12),
            Pos::new(o.x + 4, o.y + 12),
            LayerKind::Rail,
        )
        .unwrap();
        assert!(
            !p.contains(&Pos::new(o.x + 3, o.y + 12)),
            "goes round the station: {p:?}"
        );
    }

    /// Routes are straight: one bend for an L-shaped trip, not a staircase.
    #[test]
    fn routes_are_straight() {
        let w = World::new(11, Settings::default());
        let o = clear_patch(&w, 16, 10);
        let p = plan_path(&w, o, Pos::new(o.x + 15, o.y + 9), LayerKind::Rail).unwrap();
        let bends = p
            .windows(3)
            .filter(|t| (t[1].x - t[0].x, t[1].y - t[0].y) != (t[2].x - t[1].x, t[2].y - t[1].y))
            .count();
        assert_eq!(p.len(), 25, "shortest length");
        assert_eq!(bends, 1, "a single bend: {p:?}");
    }
}
