//! The static world map: terrain, regions, towns and industries. Generated from a
//! seed; the same seed always yields the same map (see tests at the bottom).

use std::{cmp::Reverse, collections::BinaryHeap};

use serde::{Deserialize, Serialize};

use crate::{
    balance as B, path,
    rng::{Rng, hash2},
    types::{Cargo, Dir, Pos},
};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Terrain {
    Water,
    Plains,
    Forest,
    Hills,
    Mountains,
}

impl Terrain {
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Terrain::Water => "Water (bridge needed)",
            Terrain::Plains => "Plains",
            Terrain::Forest => "Forest",
            Terrain::Hills => "Hills",
            Terrain::Mountains => "Mountains (tunnel needed)",
        }
    }
}

/// Every kind of industry. Primary ones produce on their own; secondary ones
/// turn inputs into an output and stop when the inputs run dry.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum IndustryKind {
    Farm,
    Forest,
    CoalMine,
    IronMine,
    OilWell,
    Mill,
    Sawmill,
    Steelworks,
    Factory,
    Refinery,
}

impl IndustryKind {
    pub const ALL: [IndustryKind; 10] = [
        IndustryKind::Farm,
        IndustryKind::Forest,
        IndustryKind::CoalMine,
        IndustryKind::IronMine,
        IndustryKind::OilWell,
        IndustryKind::Mill,
        IndustryKind::Sawmill,
        IndustryKind::Steelworks,
        IndustryKind::Factory,
        IndustryKind::Refinery,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn output(self) -> Cargo {
        match self {
            IndustryKind::Farm => Cargo::Grain,
            IndustryKind::Forest => Cargo::Logs,
            IndustryKind::CoalMine => Cargo::Coal,
            IndustryKind::IronMine => Cargo::Ore,
            IndustryKind::OilWell => Cargo::Oil,
            IndustryKind::Mill => Cargo::Food,
            IndustryKind::Sawmill => Cargo::Lumber,
            IndustryKind::Steelworks => Cargo::Steel,
            IndustryKind::Factory => Cargo::Goods,
            IndustryKind::Refinery => Cargo::Fuel,
        }
    }

    /// One unit of each input makes one unit of output.
    pub fn inputs(self) -> &'static [Cargo] {
        match self {
            IndustryKind::Mill => &[Cargo::Grain],
            IndustryKind::Sawmill => &[Cargo::Logs],
            IndustryKind::Steelworks => &[Cargo::Coal, Cargo::Ore],
            IndustryKind::Factory => &[Cargo::Steel, Cargo::Lumber],
            IndustryKind::Refinery => &[Cargo::Oil],
            _ => &[],
        }
    }

    pub fn is_primary(self) -> bool {
        self.inputs().is_empty()
    }

    pub fn glyph(self) -> char {
        match self {
            IndustryKind::Farm => 'F',
            IndustryKind::Forest => 'L',
            IndustryKind::CoalMine => 'C',
            IndustryKind::IronMine => 'I',
            IndustryKind::OilWell => 'O',
            IndustryKind::Mill => 'M',
            IndustryKind::Sawmill => 'W',
            IndustryKind::Steelworks => 'X',
            IndustryKind::Factory => 'G',
            IndustryKind::Refinery => 'R',
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            IndustryKind::Farm => "Farm",
            IndustryKind::Forest => "Logging camp",
            IndustryKind::CoalMine => "Coal mine",
            IndustryKind::IronMine => "Iron mine",
            IndustryKind::OilWell => "Oil well",
            IndustryKind::Mill => "Flour mill",
            IndustryKind::Sawmill => "Sawmill",
            IndustryKind::Steelworks => "Steelworks",
            IndustryKind::Factory => "Factory",
            IndustryKind::Refinery => "Refinery",
        }
    }

    /// Terrain this industry likes to sit on (placement tries it first).
    fn likes(self, t: Terrain) -> bool {
        match self {
            IndustryKind::Forest => t == Terrain::Forest,
            IndustryKind::CoalMine | IndustryKind::IronMine => t == Terrain::Hills,
            _ => t == Terrain::Plains,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProvinceInfo {
    pub id: usize,
    pub name: String,
    pub center: Pos,
    pub neighbors: Vec<usize>,
    pub col: i32,
    pub tiles: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeInfo {
    pub kind: IndustryKind,
    pub pos: Pos,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TownInfo {
    pub name: String,
    pub pos: Pos,
    pub pop: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Map {
    pub seed: u64,
    pub w: i32,
    pub h: i32,
    pub terrain: Vec<Terrain>,
    pub prov: Vec<u16>,
    /// Named regions (used for names and labels only).
    pub provinces: Vec<ProvinceInfo>,
    pub nodes: Vec<NodeInfo>,
    pub towns: Vec<TownInfo>,
    /// Index into `towns` of the largest town, where the game starts.
    pub home: usize,
    /// Pre-built road from the farm nearest home, so the first minute isn't empty.
    pub starter_roads: Vec<Vec<Pos>>,
}

impl Map {
    pub fn in_bounds(&self, p: Pos) -> bool {
        p.x >= 0 && p.y >= 0 && p.x < self.w && p.y < self.h
    }

    pub fn idx(&self, p: Pos) -> usize {
        (p.y * self.w + p.x) as usize
    }

    pub fn terrain(&self, p: Pos) -> Terrain {
        self.terrain[self.idx(p)]
    }

    pub fn province_at(&self, p: Pos) -> usize {
        self.prov[self.idx(p)] as usize
    }

    pub fn is_border(&self, p: Pos) -> bool {
        let me = self.province_at(p);
        [Dir::E, Dir::S].iter().any(|d| {
            let q = p.step(*d);
            self.in_bounds(q) && self.province_at(q) != me
        })
    }

    /// Generate a map from a seed. Retries with derived sub-seeds until the
    /// map passes validation, so every seed yields a valid map.
    pub fn generate(seed: u64, size: MapSize) -> Map {
        for attempt in 0..64u64 {
            if let Some(m) = try_generate(seed, attempt, size) {
                return m;
            }
        }
        panic!("map generation failed for seed {seed}");
    }

    /// Validation used by tests and the balance harness: every town and
    /// industry can be reached from home over non-mountain land, and every
    /// kind of industry exists so every chain can be completed.
    pub fn validate(&self) -> Result<(), String> {
        let home = self.towns[self.home].pos;
        let reach = path::flood(self.w, self.h, home, |p| {
            self.terrain(p) != Terrain::Mountains
        });
        for t in &self.towns {
            if !reach[self.idx(t.pos)] {
                return Err(format!("town {} unreachable from home", t.name));
            }
        }
        for n in &self.nodes {
            if !reach[self.idx(n.pos)] {
                return Err(format!("{:?} at {:?} unreachable from home", n.kind, n.pos));
            }
        }
        for k in IndustryKind::ALL {
            if !self.nodes.iter().any(|n| n.kind == k) {
                return Err(format!("no {:?} on the map", k));
            }
        }
        Ok(())
    }
}

fn value_noise(seed: u64, x: f64, y: f64) -> f64 {
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let fx = x - x0 as f64;
    let fy = y - y0 as f64;
    let s = |t: f64| t * t * (3.0 - 2.0 * t);
    let v = |ix, iy| (hash2(seed, ix, iy) % 10_000) as f64 / 10_000.0;
    let a = v(x0, y0) + (v(x0 + 1, y0) - v(x0, y0)) * s(fx);
    let b = v(x0, y0 + 1) + (v(x0 + 1, y0 + 1) - v(x0, y0 + 1)) * s(fx);
    a + (b - a) * s(fy)
}

fn fractal(seed: u64, x: f64, y: f64, scale: f64) -> f64 {
    let mut total = 0.0;
    let mut amp = 1.0;
    let mut norm = 0.0;
    let mut f = 1.0 / scale;
    for o in 0..4 {
        total += value_noise(seed.wrapping_add(o * 7919), x * f, y * f) * amp;
        norm += amp;
        amp *= 0.5;
        f *= 2.0;
    }
    total / norm
}

const NAME_A: [&str; 24] = [
    "Kessel", "Vorn", "Ashen", "Brenn", "Calder", "Dun", "Ember", "Falk", "Grim", "Hollow", "Iron",
    "Jarn", "Kraw", "Lowen", "Marsh", "Nettle", "Oster", "Pike", "Quill", "Raven", "Stur", "Thorn",
    "Umber", "Wolf",
];
const NAME_B: [&str; 16] = [
    "ford", "gate", "moor", "reach", "vale", "wick", "holm", "mark", "stead", "burg", "fell",
    "march", "crag", "field", "haven", "watch",
];

/// How big a map to generate (picked on the title screen).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum MapSize {
    Small,
    #[default]
    Normal,
    Large,
    Huge,
}

impl MapSize {
    pub const ALL: [MapSize; 4] = [
        MapSize::Small,
        MapSize::Normal,
        MapSize::Large,
        MapSize::Huge,
    ];

    pub fn name(self) -> &'static str {
        match self {
            MapSize::Small => "Small",
            MapSize::Normal => "Normal",
            MapSize::Large => "Large",
            MapSize::Huge => "Huge",
        }
    }

    fn spec(self) -> (i32, i32, i32, i32, i32, i32, f64) {
        B::MAP_SIZES[self as usize]
    }

    pub fn dims(self) -> (i32, i32) {
        let s = self.spec();
        (s.0, s.1)
    }

    pub fn towns(self) -> (i32, i32) {
        let s = self.spec();
        (s.4, s.5)
    }

    /// The cash a game on this map starts with.
    pub fn start_cash(self) -> f64 {
        B::START_CASH * B::START_CASH_SCALE[self as usize]
    }

    pub fn next(self) -> MapSize {
        MapSize::ALL[(self as usize + 1) % 4]
    }

    pub fn prev(self) -> MapSize {
        MapSize::ALL[(self as usize + 3) % 4]
    }
}

fn try_generate(seed: u64, attempt: u64, size: MapSize) -> Option<Map> {
    let (w, h, cols, rows, towns_min, towns_max, scale) = size.spec();
    // how much more land than the Normal map (exactly 1 there, so Normal seeds never change)
    let area = (w * h) as f64 / (B::MAP_W * B::MAP_H) as f64;
    let sub = seed
        .wrapping_mul(0x2545_F491_4F6C_DD1D)
        .wrapping_add(attempt);
    let mut rng = Rng::new(sub);
    let n = (w * h) as usize;
    let idx = |p: Pos| (p.y * w + p.x) as usize;

    // terrain from noise
    let mut terrain = vec![Terrain::Plains; n];
    let es = rng.next_u64();
    let ms = rng.next_u64();
    for y in 0..h {
        for x in 0..w {
            let e = fractal(es, x as f64, y as f64 * 1.8, 18.0);
            let m = fractal(ms, x as f64, y as f64 * 1.8, 14.0);
            let t = if e > 0.70 {
                Terrain::Mountains
            } else if e > 0.60 {
                Terrain::Hills
            } else if m > 0.58 {
                Terrain::Forest
            } else {
                Terrain::Plains
            };
            terrain[idx(Pos::new(x, y))] = t;
        }
    }

    // rivers (top to bottom, meandering) and small lakes
    let rivers = rng.range(2, 4) + (w - B::MAP_W).max(0) / 80;
    for _ in 0..rivers {
        let mut x = rng.range(w / 6, w * 5 / 6);
        let wide = rng.chance(0.4);
        for y in 0..h {
            for dx in 0..if wide { 2 } else { 1 } {
                let p = Pos::new((x + dx).clamp(0, w - 1), y);
                terrain[idx(p)] = Terrain::Water;
            }
            if rng.chance(0.45) {
                let nx = (x + rng.range(-1, 2)).clamp(2, w - 3);
                // fill the corner so the river stays 4-connected
                let p = Pos::new(nx, y);
                terrain[idx(p)] = Terrain::Water;
                x = nx;
            }
        }
    }
    let lakes = (rng.range(3, 7) as f64 * area).round() as i32;
    for _ in 0..lakes {
        let c = Pos::new(rng.range(8, w - 8), rng.range(4, h - 4));
        let r = rng.range(1, B::MAX_LAKE_RADIUS + 1);
        for y in -r..=r {
            for x in -r..=r {
                let p = Pos::new(c.x + x, c.y + y);
                if x * x + y * y <= r * r + 1 && p.x >= 0 && p.y >= 0 && p.x < w && p.y < h {
                    terrain[idx(p)] = Terrain::Water;
                }
            }
        }
    }

    // provinces: jittered grid seeds grown with noisy Dijkstra (always contiguous)
    let cw = w / cols;
    let ch = h / rows;
    let mut seeds = Vec::new();
    for c in 0..cols {
        for r in 0..rows {
            let x = c * cw + rng.range(cw / 4, cw * 3 / 4);
            let y = r * ch + rng.range(ch / 4, ch * 3 / 4);
            seeds.push((Pos::new(x, y), c));
        }
    }
    let mut prov = vec![u16::MAX; n];
    let mut dist = vec![u32::MAX; n];
    let mut heap = BinaryHeap::new();
    let ns = rng.next_u64();
    for (i, (p, _)) in seeds.iter().enumerate() {
        dist[idx(*p)] = 0;
        heap.push(Reverse((0u32, i as u16, p.x, p.y)));
    }
    while let Some(Reverse((d, id, x, y))) = heap.pop() {
        let p = Pos::new(x, y);
        if prov[idx(p)] != u16::MAX {
            continue;
        }
        prov[idx(p)] = id;
        for dir in Dir::ALL {
            let q = p.step(dir);
            if q.x < 0 || q.y < 0 || q.x >= w || q.y >= h || prov[idx(q)] != u16::MAX {
                continue;
            }
            let step = 10 + (hash2(ns, q.x, q.y) % 9) as u32;
            let nd = d + step;
            if nd < dist[idx(q)] {
                dist[idx(q)] = nd;
                heap.push(Reverse((nd, id, q.x, q.y)));
            }
        }
    }

    // region info
    let mut provinces: Vec<ProvinceInfo> = Vec::new();
    let mut used_names = Vec::new();
    for (i, (p, c)) in seeds.iter().enumerate() {
        let mut name;
        loop {
            name = format!(
                "{}{}",
                rng.pick(&NAME_A).unwrap(),
                rng.pick(&NAME_B).unwrap()
            );
            if !used_names.contains(&name) {
                break;
            }
        }
        used_names.push(name.clone());
        provinces.push(ProvinceInfo {
            id: i,
            name,
            center: *p,
            neighbors: vec![],
            col: *c,
            tiles: 0,
        });
    }
    for y in 0..h {
        for x in 0..w {
            let p = Pos::new(x, y);
            let a = prov[idx(p)] as usize;
            provinces[a].tiles += 1;
            for d in [Dir::E, Dir::S] {
                let q = p.step(d);
                if q.x < w && q.y < h {
                    let b = prov[idx(q)] as usize;
                    if a != b {
                        if !provinces[a].neighbors.contains(&b) {
                            provinces[a].neighbors.push(b);
                        }
                        if !provinces[b].neighbors.contains(&a) {
                            provinces[b].neighbors.push(a);
                        }
                    }
                }
            }
        }
    }
    for p in provinces.iter_mut() {
        p.neighbors.sort();
    }

    // towns: one per chosen region, at its center; towns never share a border
    let n_towns = rng.range(towns_min, towns_max + 1) as usize;
    let mut order: Vec<usize> = (0..provinces.len()).collect();
    rng.shuffle(&mut order);
    let mut town_regions: Vec<usize> = vec![];
    for &r in &order {
        if town_regions.len() == n_towns {
            break;
        }
        if town_regions
            .iter()
            .any(|t| provinces[*t].neighbors.contains(&r))
        {
            continue;
        }
        town_regions.push(r);
    }
    if town_regions.len() < towns_min as usize {
        return None;
    }
    let mut towns: Vec<TownInfo> = vec![];
    for &r in &town_regions {
        let c = provinces[r].center;
        // clear a little plain for the town to grow on
        for y in -3..=3 {
            for x in -3..=3 {
                let p = Pos::new(c.x + x, c.y + y);
                if p.x >= 0
                    && p.y >= 0
                    && p.x < w
                    && p.y < h
                    && x * x + y * y <= 10
                    && terrain[idx(p)] != Terrain::Water
                {
                    terrain[idx(p)] = Terrain::Plains;
                }
            }
        }
        let pop = rng.range(B::TOWN_START_POP.0, B::TOWN_START_POP.1) as f64;
        towns.push(TownInfo {
            name: provinces[r].name.clone(),
            pos: c,
            pop,
        });
    }
    // The home town is the biggest, and close to wanting its second cargo.
    let home = 0;
    towns[home].pop = B::HOME_TOWN_POP;

    // reachability from home (water is bridgeable, mountains are walls)
    let passable = |p: Pos| terrain[idx(p)] != Terrain::Mountains;
    let reach = path::flood(w, h, towns[home].pos, passable);
    let buildable = |p: Pos| {
        matches!(
            terrain[idx(p)],
            Terrain::Plains | Terrain::Forest | Terrain::Hills
        ) && reach[idx(p)]
    };
    if towns.iter().any(|t| !buildable(t.pos)) {
        return None;
    }

    // industries: fixed counts per kind, random layouts, preferring their terrain
    let mut nodes: Vec<NodeInfo> = Vec::new();
    let town_pos: Vec<Pos> = towns.iter().map(|t| t.pos).collect();
    for kind in IndustryKind::ALL {
        let count = (B::INDUSTRY_COUNT[kind.index()] as f64 * scale)
            .round()
            .max(1.0) as usize;
        for _ in 0..count {
            let mut placed = false;
            for attempt in 0..800 {
                let p = Pos::new(rng.range(2, w - 2), rng.range(2, h - 2));
                if !buildable(p) || (attempt < 400 && !kind.likes(terrain[idx(p)])) {
                    continue;
                }
                let crowded = nodes
                    .iter()
                    .any(|n| n.pos.manhattan(p) < B::MIN_NODE_SPACING)
                    || town_pos.iter().any(|t| t.manhattan(p) < B::TOWN_CLEARANCE);
                if crowded {
                    continue;
                }
                nodes.push(NodeInfo { kind, pos: p });
                placed = true;
                break;
            }
            if !placed {
                return None;
            }
        }
    }

    // starter roads: the farm nearest the mill nearest home, then mill → home.
    // Each runs from where cargo is loaded to where it's delivered.
    let hp = towns[home].pos;
    let nearest = |kind: IndustryKind, to: Pos| {
        nodes
            .iter()
            .filter(|n| n.kind == kind)
            .min_by_key(|n| n.pos.manhattan(to))
            .map(|n| n.pos)
    };
    let mill = nearest(IndustryKind::Mill, hp)?;
    let farm = nearest(IndustryKind::Farm, mill)?;
    let mut starter_roads = Vec::new();
    for (a, b) in [(farm, mill), (mill, hp)] {
        let road = path::astar(
            w,
            h,
            a,
            |p| p == b,
            b,
            10,
            |_, q| {
                let t = terrain[idx(q)];
                Some((B::ROAD_COST[t.index()] * 10.0) as u32)
            },
        )?;
        starter_roads.push(road);
    }

    let map = Map {
        seed,
        w,
        h,
        terrain,
        prov,
        provinces,
        nodes,
        towns,
        home,
        starter_roads,
    };
    map.validate().ok()?;
    Some(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_map() {
        for seed in [1u64, 42, 1337, 987654321] {
            let a = Map::generate(seed, MapSize::Normal);
            let b = Map::generate(seed, MapSize::Normal);
            assert_eq!(a.terrain, b.terrain);
            assert_eq!(a.prov, b.prov);
            assert_eq!(a.nodes.len(), b.nodes.len());
            for (x, y) in a.nodes.iter().zip(b.nodes.iter()) {
                assert_eq!(x.pos, y.pos);
            }
            let names = |m: &Map| m.towns.iter().map(|t| t.name.clone()).collect::<Vec<_>>();
            assert_eq!(names(&a), names(&b));
        }
    }

    #[test]
    fn different_seeds_differ() {
        assert_ne!(
            Map::generate(1, MapSize::Normal).terrain,
            Map::generate(2, MapSize::Normal).terrain
        );
    }

    #[test]
    fn many_maps_are_valid() {
        for seed in 0..150u64 {
            let m = Map::generate(seed, MapSize::Normal);
            m.validate().unwrap_or_else(|e| panic!("seed {seed}: {e}"));
            assert!(
                (B::TOWNS_MIN as usize..=B::TOWNS_MAX as usize).contains(&m.towns.len()),
                "seed {seed}: town count"
            );
            assert!(
                !m.starter_roads.is_empty(),
                "seed {seed} has no starter road"
            );
            let home = m.towns[m.home].pop;
            assert!(
                m.towns.iter().all(|t| t.pop <= home),
                "seed {seed}: home must be the biggest town"
            );
        }
    }

    #[test]
    fn every_size_makes_valid_maps() {
        for size in MapSize::ALL {
            let (w, h) = size.dims();
            let (lo, hi) = size.towns();
            for seed in 0..25u64 {
                let m = Map::generate(seed, size);
                m.validate()
                    .unwrap_or_else(|e| panic!("{size:?} seed {seed}: {e}"));
                assert_eq!((m.w, m.h), (w, h));
                assert!(
                    (lo as usize..=hi as usize).contains(&m.towns.len()),
                    "{size:?} seed {seed}: town count {}",
                    m.towns.len()
                );
            }
        }
    }

    #[test]
    fn bigger_maps_start_richer() {
        assert_eq!(MapSize::Normal.start_cash(), B::START_CASH);
        assert!(
            MapSize::Huge.start_cash() > MapSize::Large.start_cash()
                && MapSize::Large.start_cash() > MapSize::Normal.start_cash()
        );
        let w = crate::world::World::new(
            3,
            crate::world::Settings {
                size: MapSize::Huge,
                ..Default::default()
            },
        );
        assert_eq!(w.cash, MapSize::Huge.start_cash());
    }

    /// Shared seeds must keep making exactly the Normal maps they always made.
    #[test]
    fn normal_maps_never_change() {
        let mut out = 0u64;
        for seed in [1u64, 42, 777] {
            let m = Map::generate(seed, MapSize::Normal);
            out = m
                .terrain
                .iter()
                .map(|t| *t as u64)
                .chain(m.prov.iter().map(|p| *p as u64))
                .chain(m.nodes.iter().map(|n| (n.pos.x * 1000 + n.pos.y) as u64))
                .chain(
                    m.towns
                        .iter()
                        .map(|t| (t.pos.x * 1000 + t.pos.y) as u64 + t.pop as u64),
                )
                .fold(out, |a, v| a.wrapping_mul(1_000_003).wrapping_add(v));
        }
        assert_eq!(out, 12532655422127645351);
    }
}
