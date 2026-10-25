//! The simulation. `World` owns all game state and advances it with `tick()`.
//! It knows nothing about the terminal, so it can run headless.

pub mod build;
pub mod contracts;
pub mod economy;
pub mod events;
pub mod milestones;
pub mod rail;
pub mod vehicles;

pub use rail::{SignalKind, Train, TrainKind};
use serde::{Deserialize, Serialize};
pub use vehicles::{Action, Carrier, Cond, Stop, Target, Truck, VState};

use crate::{
    balance as B,
    map::{IndustryKind, Map},
    rng::Rng,
    types::{Cargo, Inv, NUM_CARGO, Origins, Pos},
};

/// Build state of a tile layer or structure.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Layer {
    #[default]
    None,
    Built,
}

impl Layer {
    pub fn present(self) -> bool {
        self != Layer::None
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Tile {
    pub road: Layer,
    pub rail: Layer,
    /// Connection bitmasks (Dir::bit) so parallel lines don't auto-join.
    pub road_conn: u8,
    pub rail_conn: u8,
    pub signal: Option<SignalKind>,
    pub site: Option<u32>,
    pub station: Option<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum SiteKind {
    Industry(IndustryKind),
    /// Index into `World::towns`.
    Town(usize),
    Hub,
}

impl SiteKind {
    pub fn name(self) -> &'static str {
        match self {
            SiteKind::Industry(k) => k.name(),
            SiteKind::Town(_) => "Town",
            SiteKind::Hub => "Transfer hub",
        }
    }

    pub fn glyph(self) -> char {
        match self {
            SiteKind::Industry(k) => k.glyph(),
            SiteKind::Town(_) => '⌂',
            SiteKind::Hub => 'H',
        }
    }
}

/// Rolling per-second counters for the "units moved per minute" readout.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Throughput {
    pub buckets: Vec<f64>, // 60 one-second buckets
    pub cur: usize,
    pub last_sec: u64,
}

impl Throughput {
    pub fn add(&mut self, now_sec: u64, amt: f64) {
        self.roll(now_sec);
        self.buckets[self.cur] += amt;
    }

    pub fn roll(&mut self, now_sec: u64) {
        if self.buckets.len() != 60 {
            self.buckets = vec![0.0; 60];
            self.last_sec = now_sec;
        }
        let steps = now_sec.saturating_sub(self.last_sec).min(60);
        for _ in 0..steps {
            self.cur = (self.cur + 1) % 60;
            self.buckets[self.cur] = 0.0;
        }
        self.last_sec = now_sec;
    }

    pub fn per_min(&self) -> f64 {
        self.buckets.iter().sum()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Site {
    pub id: u32,
    pub kind: SiteKind,
    pub pos: Pos,
    /// Region the site sits in (for names).
    pub province: usize,
    /// Industries: output and input buffers. Towns: waiting passengers. Hubs: anything.
    pub inv: Inv,
    pub state: Layer,
    /// Hubs: where the cargo passing through was first loaded.
    #[serde(default)]
    pub origins: Origins,
    /// Industries: production multiplier (primary ones grow it by being hauled out).
    #[serde(default = "one")]
    pub level: f64,
    /// Seconds this minute the output buffer sat full.
    #[serde(default)]
    pub full_secs: f64,
    pub stats: Throughput,
    /// Trucks currently being served (for bay limits).
    #[serde(skip)]
    pub bays_used: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Station {
    pub id: u32,
    pub name: String,
    pub pos: Pos,
    pub limit: u32,
    pub stats: Throughput,
    /// £ earned by deliveries made here.
    #[serde(default)]
    pub income: Throughput,
    pub removed: bool,
}

pub const TIER_NAMES: [&str; 4] = ["Hamlet", "Town", "City", "Metropolis"];

/// Charter medals, in order (see `World::medal_targets`).
pub const MEDAL_NAMES: [&str; 3] = ["Bronze", "Silver", "Gold"];

/// Cargo each tier asks for (each tier adds one).
pub const TIER_DEMANDS: [&[Cargo]; 4] = [
    &[Cargo::Food],
    &[Cargo::Food, Cargo::Lumber],
    &[Cargo::Food, Cargo::Lumber, Cargo::Goods],
    &[Cargo::Food, Cargo::Lumber, Cargo::Goods, Cargo::Fuel],
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Town {
    pub name: String,
    pub site: u32,
    pub pos: Pos,
    pub pop: f64,
    pub start_pop: f64,
    /// 0 = Hamlet .. 3 = Metropolis.
    pub tier: usize,
    /// Highest tier ever reached (for the end screen).
    pub best_tier: usize,
    /// Delivered goods waiting to be used up.
    pub stock: Inv,
    /// Satisfaction 0..1 per cargo (only demanded cargo matters).
    pub sat: [f64; NUM_CARGO],
    /// Does a station or truck of yours serve this town?
    pub served: bool,
    /// Passengers who arrived here.
    pub arrived: f64,
}

impl Town {
    pub fn demands(&self) -> &'static [Cargo] {
        TIER_DEMANDS[self.tier]
    }

    pub fn wants(&self, c: Cargo) -> bool {
        self.demands().contains(&c)
    }

    /// Units per minute this town wants of `c` (0 if its tier doesn't ask for it).
    pub fn demand_per_min(&self, c: Cargo) -> f64 {
        if self.wants(c) {
            self.pop / 1000.0 * B::DEMAND_PER_1000[c.index()]
        } else {
            0.0
        }
    }

    /// Average satisfaction over what the town currently wants.
    pub fn happiness(&self) -> f64 {
        let d = self.demands();
        d.iter().map(|c| self.sat[c.index()]).sum::<f64>() / d.len() as f64
    }

    pub fn tier_name(&self) -> &'static str {
        TIER_NAMES[self.tier]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum AlertLevel {
    Info,
    Warn,
    Danger,
    Good,
}

/// Big moments the UI celebrates: fireworks, a headline, a fanfare.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Fanfare {
    /// A town reached a new tier.
    TierUp,
    Milestone,
    /// A Charter medal (bronze, silver, gold).
    Medal,
    /// The Railhead Gazette (`events`).
    News,
    Contract,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Alert {
    pub time: f64,
    pub text: String,
    pub pos: Option<Pos>,
    pub level: AlertLevel,
    /// Set for big moments the UI should make a fuss about.
    #[serde(default)]
    pub fanfare: Option<Fanfare>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum GameMode {
    /// Scored: a fixed number of minutes to grow the region.
    #[default]
    Charter,
    /// Unscored: unlimited cash, no clock.
    Sandbox,
    /// No clock, but the Railway Board reviews the railway every few minutes
    /// against a rising target; three failed reviews and the franchise is lost.
    Franchise,
}

impl GameMode {
    /// The order the title screen cycles through.
    pub const ALL: [GameMode; 3] = [GameMode::Charter, GameMode::Franchise, GameMode::Sandbox];

    pub fn name(self) -> &'static str {
        match self {
            GameMode::Charter => "Charter",
            GameMode::Sandbox => "Sandbox",
            GameMode::Franchise => "Franchise",
        }
    }
}

/// The Railway Board's next review in a Franchise game.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Review {
    /// 1 for the first review.
    pub number: u32,
    pub secs_left: u64,
    /// The Prosperity the Board wants to see.
    pub target: f64,
}

/// Game setup chosen on the title screen.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub mode: GameMode,
    #[serde(default)]
    pub size: crate::map::MapSize,
}

/// A periodic snapshot for the end screen and statistics graphs.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Sample {
    pub time: f64,
    pub prosperity: f64,
    pub cash: f64,
    /// £ earned since the previous sample.
    pub revenue: f64,
    /// Units delivered since the previous sample.
    pub delivered: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct World {
    pub map: Map,
    pub time: f64,
    pub ticks: u64,
    pub rng: Rng,
    pub tiles: Vec<Tile>,
    pub sites: Vec<Site>,
    pub towns: Vec<Town>,
    pub stations: Vec<Station>,
    pub trucks: Vec<Truck>,
    pub trains: Vec<Train>,
    pub alerts: Vec<Alert>,
    /// Alerts raised so far (the list itself is trimmed); the UI plays a sound for new ones.
    #[serde(default)]
    pub alert_count: u64,
    pub settings: Settings,
    pub next_id: u32,
    /// Bumped whenever roads/rail change so vehicles re-path.
    pub net_version: u64,
    /// Derived rail block ids per tile (-1 = none). Rebuilt on load/change.
    #[serde(skip)]
    pub blocks: rail::Blocks,
    /// Vehicles per road tile this tick (congestion).
    #[serde(skip)]
    pub road_load: Vec<u8>,
    /// Cargo passing each tile, a moving average in units per minute (flow overlay).
    #[serde(skip)]
    pub flow: Vec<f32>,
    pub cash: f64,
    pub stats: GameStats,
    /// Charter over: the sim stops and the end screen shows.
    #[serde(default)]
    pub finished: bool,
    #[serde(default)]
    pub history: Vec<Sample>,
    #[serde(default)]
    pub contracts: Vec<contracts::Contract>,
    /// When each of `milestones::MILESTONES` was reached.
    #[serde(default)]
    pub milestones: Vec<Option<f64>>,
    /// The Railhead Gazette: recent news, some of it still in effect.
    #[serde(default)]
    pub news: Vec<events::News>,
}

/// Lifetime counters for the end screen and balance harness.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GameStats {
    pub delivered: Inv,
    pub deadlocks: u32,
    pub revenue: f64,
    pub spent: f64,
    pub upkeep: f64,
    pub start_prosperity: f64,
    /// Extra score from completed contracts.
    #[serde(default)]
    pub contract_bonus: f64,
    #[serde(default)]
    pub contracts_done: u32,
    #[serde(default)]
    pub last_contract: f64,
    #[serde(default)]
    pub revenue_at_sample: f64,
    #[serde(default)]
    pub delivered_at_sample: f64,
    /// When the last news came out, and how long until the next.
    #[serde(default)]
    pub last_news: f64,
    #[serde(default)]
    pub news_gap: f64,
    /// Charter medals won so far: 0 none, 1 bronze, 2 silver, 3 gold.
    #[serde(default)]
    pub medal: usize,
    /// Franchise: Board reviews held, passed, and failed (strikes).
    #[serde(default)]
    pub reviews: u32,
    #[serde(default)]
    pub reviews_passed: u32,
    #[serde(default)]
    pub strikes: u32,
}

impl World {
    pub fn new(seed: u64, settings: Settings) -> World {
        let map = Map::generate(seed, settings.size);
        let cash = settings.size.start_cash();
        let n = (map.w * map.h) as usize;
        let rng = Rng::new(seed.wrapping_add(0xA11CE));
        let mut w = World {
            tiles: vec![Tile::default(); n],
            sites: vec![],
            towns: vec![],
            stations: vec![],
            trucks: vec![],
            trains: vec![],
            alerts: vec![],
            alert_count: 0,
            settings,
            next_id: 1,
            net_version: 0,
            blocks: rail::Blocks::default(),
            road_load: vec![0; n],
            flow: vec![0.0; n],
            stats: GameStats::default(),
            finished: false,
            history: vec![],
            contracts: vec![],
            milestones: vec![],
            news: vec![],
            cash,
            time: 0.0,
            ticks: 0,
            rng,
            map,
        };

        for ti in 0..w.map.towns.len() {
            let info = w.map.towns[ti].clone();
            let site = w.add_site(SiteKind::Town(ti), info.pos, Layer::Built);
            let mut town = Town {
                name: info.name,
                site,
                pos: info.pos,
                pop: info.pop,
                start_pop: info.pop,
                tier: 0,
                best_tier: 0,
                stock: Inv::default(),
                sat: [0.0; NUM_CARGO],
                served: false,
                arrived: 0.0,
            };
            town.tier = economy::tier_for(town.pop, 0);
            town.best_tier = town.tier;
            w.towns.push(town);
        }
        for ni in 0..w.map.nodes.len() {
            let n = w.map.nodes[ni].clone();
            w.add_site(SiteKind::Industry(n.kind), n.pos, Layer::Built);
        }

        // Pre-built starter roads (farm -> mill -> home), a truck on each, and a station stub at home.
        let roads = w.map.starter_roads.clone();
        for r in &roads {
            build::lay_path(&mut w, r, build::LayerKind::Road);
        }
        for r in &roads {
            let (Some(a), Some(b)) = (w.tile(r[0]).site, w.tile(*r.last().unwrap()).site) else {
                continue;
            };
            let mut load = Stop::new(Target::Site(a), Action::Load);
            if let SiteKind::Industry(k) = w.sites[a as usize].kind {
                load.filter = crate::types::CargoMask::single(k.output());
            }
            w.spawn_truck(a);
            w.trucks.last_mut().unwrap().carrier.schedule.stops =
                vec![load, Stop::new(Target::Site(b), Action::Unload)];
        }
        build::starter_station(&mut w);
        w.rebuild_derived();
        economy::update_served(&mut w);
        w.stats.start_prosperity = w.prosperity();
        w.history.push(Sample {
            time: 0.0,
            prosperity: w.stats.start_prosperity,
            cash: w.cash,
            revenue: 0.0,
            delivered: 0.0,
        });
        let home = &w.towns[w.map.home];
        let (name, pos) = (home.name.clone(), home.pos);
        w.alert(
            AlertLevel::Info,
            format!("Welcome to {name}! Feed your towns and they grow. Two trucks are already running grain to the mill and flour to town. Press {{help}} for help."),
            Some(pos),
        );
        w
    }

    /// Recompute caches that aren't saved (after load or network edits).
    pub fn rebuild_derived(&mut self) {
        let n = (self.map.w * self.map.h) as usize;
        self.road_load = vec![0; n];
        self.flow = vec![0.0; n];
        rail::rebuild_blocks(self);
    }

    pub fn new_id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    pub fn add_site(&mut self, kind: SiteKind, pos: Pos, state: Layer) -> u32 {
        let id = self.sites.len() as u32;
        let province = self.map.province_at(pos);
        self.sites.push(Site {
            id,
            kind,
            pos,
            province,
            inv: Inv::default(),
            state,
            origins: Origins::default(),
            level: 1.0,
            full_secs: 0.0,
            stats: Throughput::default(),
            bays_used: 0,
        });
        let i = self.map.idx(pos);
        self.tiles[i].site = Some(id);
        // Sites double as road endpoints so trucks can reach them.
        if self.tiles[i].road == Layer::None {
            self.tiles[i].road = state;
        }
        id
    }

    /// Record `units` of cargo passing tile `p` (for the flow overlay; never read by the sim).
    pub fn add_flow(&mut self, p: Pos, units: f64) {
        let i = self.map.idx(p);
        if let Some(f) = self.flow.get_mut(i) {
            *f += units as f32;
        }
    }

    pub fn tile(&self, p: Pos) -> &Tile {
        &self.tiles[self.map.idx(p)]
    }

    pub fn tile_mut(&mut self, p: Pos) -> &mut Tile {
        let i = self.map.idx(p);
        &mut self.tiles[i]
    }

    /// Display name of a site: "Ashford", "Ashford Coal mine", "Ashford hub".
    pub fn site_name(&self, id: u32) -> String {
        let s = &self.sites[id as usize];
        let region = &self.map.provinces[s.province].name;
        match s.kind {
            SiteKind::Town(t) => self.towns[t].name.clone(),
            SiteKind::Industry(k) => format!("{region} {}", k.name()),
            SiteKind::Hub => format!("{region} hub"),
        }
    }

    /// Can a vehicle pick `c` up here?
    pub fn can_load(&self, site: &Site, c: Cargo) -> bool {
        match site.kind {
            SiteKind::Industry(k) => k.output() == c,
            SiteKind::Town(_) => c == Cargo::Passengers,
            SiteKind::Hub => true,
        }
    }

    /// How many more units of `c` this site will take right now.
    pub fn accept_space(&self, site: &Site, c: Cargo) -> f64 {
        match site.kind {
            SiteKind::Industry(k) => {
                if k.inputs().contains(&c) {
                    (B::INPUT_BUFFER - site.inv[c]).max(0.0)
                } else {
                    0.0
                }
            }
            SiteKind::Town(t) => {
                let town = &self.towns[t];
                if c == Cargo::Passengers {
                    f64::INFINITY
                } else {
                    (town.demand_per_min(c) * B::TOWN_STOCK_MINUTES - town.stock[c]).max(0.0)
                }
            }
            SiteKind::Hub => (B::HUB_CAP - site.inv[c]).max(0.0),
        }
    }

    /// The most units of `c` this site will ever take at once (its whole
    /// stockpile empty); `accept_space` is never more than this.
    pub fn accept_cap(&self, site: &Site, c: Cargo) -> f64 {
        match site.kind {
            SiteKind::Industry(k) => {
                if k.inputs().contains(&c) {
                    B::INPUT_BUFFER
                } else {
                    0.0
                }
            }
            SiteKind::Town(t) => {
                if c == Cargo::Passengers {
                    f64::INFINITY
                } else {
                    self.towns[t].demand_per_min(c) * B::TOWN_STOCK_MINUTES
                }
            }
            SiteKind::Hub => B::HUB_CAP,
        }
    }

    /// The most units of `c` that can wait at this site to be picked up.
    pub fn hold_cap(&self, site: &Site, c: Cargo) -> f64 {
        match site.kind {
            SiteKind::Industry(k) => {
                if k.output() == c {
                    B::NODE_BUFFER
                } else {
                    0.0
                }
            }
            SiteKind::Town(t) => {
                if c == Cargo::Passengers {
                    (self.towns[t].pop / 1000.0 * B::PASSENGER_WAIT_PER_1000)
                        .max(B::PASSENGER_WAIT_MIN)
                } else {
                    0.0
                }
            }
            SiteKind::Hub => B::HUB_CAP,
        }
    }

    /// Does delivering cargo to this site pay (it's a customer), or does it
    /// just sit there waiting to be picked up again (a hub)?
    pub fn is_customer(&self, site: &Site) -> bool {
        !matches!(site.kind, SiteKind::Hub)
    }

    /// The score: total population of every town your network serves.
    pub fn prosperity(&self) -> f64 {
        self.towns.iter().filter(|t| t.served).map(|t| t.pop).sum()
    }

    /// Raise an alert. Text may name keys as `{action}` placeholders (see
    /// `ui::keys`); the UI shows the player's own binding there.
    pub fn alert(&mut self, level: AlertLevel, text: String, pos: Option<Pos>) {
        self.push_alert(level, text, pos, None);
    }

    /// A good alert the UI celebrates (see `Fanfare`).
    pub fn celebrate(&mut self, text: String, pos: Option<Pos>, fanfare: Fanfare) {
        self.push_alert(AlertLevel::Good, text, pos, Some(fanfare));
    }

    pub fn push_alert(
        &mut self,
        level: AlertLevel,
        text: String,
        pos: Option<Pos>,
        fanfare: Option<Fanfare>,
    ) {
        self.alerts.push(Alert {
            time: self.time,
            text,
            pos,
            level,
            fanfare,
        });
        self.alert_count += 1;
        if self.alerts.len() > 300 {
            self.alerts.drain(0..100);
        }
    }

    pub fn sandbox(&self) -> bool {
        self.settings.mode == GameMode::Sandbox
    }

    pub fn franchise(&self) -> bool {
        self.settings.mode == GameMode::Franchise
    }

    /// Seconds left on the Charter clock (None in Sandbox and Franchise).
    pub fn time_left(&self) -> Option<f64> {
        (self.settings.mode == GameMode::Charter)
            .then(|| (B::CHARTER_MINUTES * 60.0 - self.time).max(0.0))
    }

    /// The Prosperity the Board wants at review `n` (1-based). Like medals it
    /// scales with the region's starting population, and it keeps rising.
    pub fn review_target(&self, n: u32) -> f64 {
        let region: f64 = self.towns.iter().map(|t| t.start_pop).sum();
        let mult = B::REVIEW_FIRST_MULT * B::REVIEW_GROWTH.powi(n.max(1) as i32 - 1);
        (region * mult / 10.0).round() * 10.0
    }

    /// The next Board review (None unless this is a Franchise still running).
    pub fn next_review(&self) -> Option<Review> {
        if !self.franchise() || self.finished {
            return None;
        }
        let number = self.stats.reviews + 1;
        let due = number as u64 * B::REVIEW_EVERY_SECS;
        Some(Review {
            number,
            secs_left: due.saturating_sub(self.now_sec()),
            target: self.review_target(number),
        })
    }

    /// Scores for the Charter's bronze, silver and gold medals. They scale
    /// with how many people live in the region, so every map is as hard.
    pub fn medal_targets(&self) -> [f64; 3] {
        let region: f64 = self.towns.iter().map(|t| t.start_pop).sum();
        B::MEDAL_REGION_MULT.map(|m| (region * m / 100.0).round() * 100.0)
    }

    /// The final score: Prosperity plus the contract bonus.
    pub fn score(&self) -> f64 {
        self.prosperity() + self.stats.contract_bonus
    }

    /// Pay for something, or explain why you can't. Sandbox pays for everything.
    pub fn spend(&mut self, amount: f64, what: &str) -> Result<(), String> {
        if self.sandbox() {
            self.stats.spent += amount;
            return Ok(());
        }
        if amount > self.cash + 1e-9 {
            return Err(format!(
                "Not enough cash: {what} costs {}, you have {}.",
                money(amount),
                money(self.cash)
            ));
        }
        self.cash -= amount;
        self.stats.spent += amount;
        Ok(())
    }

    /// Give back part of what something cost.
    pub fn refund(&mut self, cost: f64) -> f64 {
        let r = cost * B::REFUND;
        self.cash += r;
        self.stats.spent -= r;
        r
    }

    /// Running costs of the whole fleet, £ per minute.
    pub fn upkeep_per_min(&self) -> f64 {
        let trains: f64 = self.trains.iter().map(|t| t.upkeep()).sum();
        trains + self.trucks.len() as f64 * B::TRUCK_UPKEEP
    }

    pub fn now_sec(&self) -> u64 {
        self.ticks / B::TICKS_PER_SEC
    }

    /// Advance the simulation by one tick.
    pub fn tick(&mut self) {
        if self.finished {
            return;
        }
        let dt = B::TICK_DT;
        self.ticks += 1;
        self.time += dt;
        let whole_second = self.ticks.is_multiple_of(B::TICKS_PER_SEC);

        economy::tick(self, dt);
        economy::pay_upkeep(self, dt);
        vehicles::tick_trucks(self, dt);
        rail::tick_trains(self, dt);
        if whole_second {
            economy::tick_second(self);
            contracts::tick_second(self);
            events::tick_second(self);
            milestones::tick_second(self);
            rail::check_deadlocks(self);
            self.tick_clock();
            // Flow decays so it reads as roughly units per minute.
            for f in self.flow.iter_mut() {
                *f *= 1.0 - 1.0 / 60.0;
            }
        }
    }

    /// Once per second: history samples, Charter countdown alerts and the end.
    fn tick_clock(&mut self) {
        let sec = self.now_sec();
        if sec.is_multiple_of(B::SAMPLE_SECS) {
            // per-interval amounts, not running totals
            let (rev, del) = (self.stats.revenue, self.stats.delivered.total());
            let s = Sample {
                time: self.time,
                prosperity: self.prosperity(),
                cash: self.cash,
                revenue: rev - self.stats.revenue_at_sample,
                delivered: del - self.stats.delivered_at_sample,
            };
            self.history.push(s);
            self.stats.revenue_at_sample = rev;
            self.stats.delivered_at_sample = del;
        }
        if self.franchise() {
            self.tick_board();
            return;
        }
        let Some(left) = self.time_left() else { return };
        // medals as they're won, with a fuss
        let targets = self.medal_targets();
        while self.stats.medal < 3 && self.score() >= targets[self.stats.medal] {
            self.stats.medal += 1;
            let won = self.stats.medal;
            let next = match targets.get(won) {
                Some(t) => format!(" Next: {} at {t:.0}.", MEDAL_NAMES[won]),
                None => " The highest honour the Railway Board gives!".into(),
            };
            let msg = format!(
                "● {} MEDAL! Your score passed {:.0}.{next}",
                MEDAL_NAMES[won - 1].to_uppercase(),
                targets[won - 1]
            );
            let home = self.towns[self.map.home].pos;
            self.celebrate(msg, Some(home), Fanfare::Medal);
        }
        let left = left.round() as u64;
        if [15 * 60, 5 * 60, 60].contains(&left) {
            let msg = format!(
                "Charter clock: {} left! Prosperity stands at {:.0}.",
                if left >= 120 {
                    format!("{} minutes", left / 60)
                } else {
                    "ONE MINUTE".into()
                },
                self.prosperity()
            );
            self.alert(AlertLevel::Warn, msg, None);
        }
        if left == 0 {
            self.finished = true;
            let msg = format!("The Charter is complete! Final score: {:.0}.", self.score());
            self.alert(AlertLevel::Good, msg, None);
        }
    }

    /// Once per second in a Franchise: warn before a review the railway would
    /// fail, then hold it when it's due. Three strikes and the game is over.
    fn tick_board(&mut self) {
        let Some(r) = self.next_review() else { return };
        let pros = self.prosperity();
        let short = pros < r.target;
        let red = self.cash < 0.0;
        if B::REVIEW_WARN_AT.contains(&r.secs_left) && (short || red) {
            let why = match (short, red) {
                (true, true) => format!(
                    "Prosperity {pros:.0} of {:.0}, and the books are in the red",
                    r.target
                ),
                (true, false) => format!("Prosperity {pros:.0} of {:.0} needed", r.target),
                _ => "the books are in the red".to_string(),
            };
            let when = if r.secs_left >= 60 {
                format!("{} minutes", r.secs_left / 60)
            } else {
                format!("{} seconds", r.secs_left)
            };
            self.alert(
                AlertLevel::Warn,
                format!("⚖ The Board reviews your railway in {when}: {why}."),
                None,
            );
        }
        if r.secs_left > 0 {
            return;
        }
        self.stats.reviews += 1;
        let home = self.towns[self.map.home].pos;
        if !short && !red {
            self.stats.reviews_passed += 1;
            let next = self.review_target(r.number + 1);
            let msg = format!(
                "⚖ REVIEW {} PASSED! Prosperity {pros:.0}, the Board wanted {:.0}. Next time: {next:.0}.",
                r.number, r.target
            );
            self.celebrate(msg, Some(home), Fanfare::Milestone);
            return;
        }
        self.stats.strikes += 1;
        let why = if short {
            format!("Prosperity {pros:.0}, the Board wanted {:.0}", r.target)
        } else {
            "your books are in the red".to_string()
        };
        if self.stats.strikes >= B::REVIEW_STRIKES {
            self.finished = true;
            let msg = format!(
                "⚖ FRANCHISE REVOKED. Review {} failed ({why}). You ran the railway for {:.0} minutes and passed {} reviews.",
                r.number,
                self.time / 60.0,
                self.stats.reviews_passed
            );
            self.alert(AlertLevel::Danger, msg, Some(home));
        } else {
            let left = B::REVIEW_STRIKES - self.stats.strikes;
            let msg = format!(
                "⚖ REVIEW {} FAILED: {why}. Strike {} of {}: {left} more and the franchise is gone.",
                r.number,
                self.stats.strikes,
                B::REVIEW_STRIKES
            );
            self.alert(AlertLevel::Danger, msg, Some(home));
        }
    }
}

fn one() -> f64 {
    1.0
}

/// Format cash for display: £12,340 (negative as -£450).
pub fn money(v: f64) -> String {
    let n = v.round() as i64;
    let digits = n.abs().to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 {
        format!("-£{out}")
    } else {
        format!("£{out}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pass a review, then fail three in a row: the franchise is revoked.
    #[test]
    fn franchise_reviews_until_three_strikes() {
        let mut w = World::new(
            5,
            Settings {
                mode: GameMode::Franchise,
                ..Settings::default()
            },
        );
        assert!(w.time_left().is_none(), "no Charter clock");
        let r = w.next_review().unwrap();
        assert_eq!((r.number, r.secs_left), (1, B::REVIEW_EVERY_SECS));
        assert!(
            w.review_target(2) > r.target && r.target > 0.0,
            "targets rise"
        );
        let home = w.map.home;
        while w.next_review().unwrap().secs_left > 1 {
            w.tick();
        }
        w.cash = w.cash.max(0.0);
        w.towns[home].pop = r.target * 2.0;
        w.towns[home].served = true;
        while w.stats.reviews < 1 {
            w.tick();
        }
        assert_eq!((w.stats.reviews_passed, w.stats.strikes), (1, 0));
        w.towns[home].pop = 1.0;
        while !w.finished {
            w.tick();
            assert!(w.time < 3600.0, "the Board never ended it");
        }
        assert_eq!(
            (w.stats.reviews, w.stats.reviews_passed, w.stats.strikes),
            (4, 1, B::REVIEW_STRIKES)
        );
        assert!((w.time - 4.0 * B::REVIEW_EVERY_SECS as f64).abs() < 1.0);
        assert!(w.next_review().is_none());
        assert!(
            w.alerts
                .iter()
                .any(|a| a.text.contains("FRANCHISE REVOKED"))
        );
    }
}
