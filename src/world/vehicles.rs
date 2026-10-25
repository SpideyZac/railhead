//! Schedules, cargo handling shared by trucks and trains, and truck movement.

use super::*;
use crate::{
    path,
    types::{CargoMask, Dir, Lot, Origins},
};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Target {
    Site(u32),
    Station(u32),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Action {
    Load,
    Unload,
    /// Unload what the stop accepts, then load: one stop for two-way runs.
    Swap,
}

impl Action {
    /// The cycle the schedule editor's `l` key steps through.
    pub fn next(self) -> Action {
        match self {
            Action::Load => Action::Unload,
            Action::Unload => Action::Swap,
            Action::Swap => Action::Load,
        }
    }

    /// The passes `transfer` makes, in order.
    fn phases(self) -> &'static [Action] {
        match self {
            Action::Load => &[Action::Load],
            Action::Unload => &[Action::Unload],
            Action::Swap => &[Action::Unload, Action::Load],
        }
    }
}

/// Primary departure condition. A stop also has an optional timeout;
/// whichever comes first wins.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum Cond {
    Full,
    Empty,
    Time(f64),
}

/// "Only go here if ...": checked when a vehicle sets off for the stop. A
/// stop whose gate is shut is skipped. Counts only the stop's cargo filter.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum Gate {
    /// At least this many units are waiting to be picked up there.
    Waiting(f64),
    /// The stop will take at least this many units.
    Wants(f64),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stop {
    pub target: Target,
    pub action: Action,
    pub filter: CargoMask,
    pub cond: Cond,
    pub timeout: Option<f64>,
    #[serde(default)]
    pub gate: Option<Gate>,
}

impl Stop {
    pub fn new(target: Target, action: Action) -> Stop {
        match action {
            Action::Load => Stop {
                target,
                action,
                filter: CargoMask::ALL,
                cond: Cond::Full,
                timeout: Some(30.0),
                gate: None,
            },
            Action::Unload => Stop {
                target,
                action,
                filter: CargoMask::ALL,
                cond: Cond::Empty,
                timeout: Some(30.0),
                gate: None,
            },
            Action::Swap => Stop {
                target,
                action,
                filter: CargoMask::ALL,
                cond: Cond::Full,
                timeout: Some(20.0),
                gate: None,
            },
        }
    }

    pub fn describe(&self, w: &World) -> String {
        let verb = match self.action {
            Action::Load => "load",
            Action::Unload => "unload",
            Action::Swap => "drop off + pick up",
        };
        let cond = match self.cond {
            Cond::Full => "full".to_string(),
            Cond::Empty => "empty".to_string(),
            Cond::Time(t) => format!("{t:.0}s"),
        };
        let to = self
            .timeout
            .map(|t| format!(" / {t:.0}s max"))
            .unwrap_or_default();
        let gate = match self.gate {
            None => String::new(),
            Some(Gate::Waiting(n)) => format!(" · only if ≥{n:.0} waiting"),
            Some(Gate::Wants(n)) => format!(" · only if it wants ≥{n:.0}"),
        };
        format!(
            "{}: {verb} {} until {cond}{to}{gate}",
            w.target_name(self.target),
            self.filter.label()
        )
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Schedule {
    pub stops: Vec<Stop>,
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum VState {
    Idle,
    Travel,
    /// At a stop, loading/unloading; seconds spent there.
    Working(f64),
    NoPath,
    /// Destination station is at its train limit.
    WaitLimit,
    /// Waiting at a signal for a block to clear.
    Blocked,
    /// Every stop's gate is shut: nothing needs a visit right now.
    Gated,
}

impl VState {
    pub fn label(self) -> &'static str {
        match self {
            VState::Idle => "idle (no schedule)",
            VState::Travel => "travelling",
            VState::Working(_) => "loading/unloading",
            VState::NoPath => "NO PATH to next stop",
            VState::WaitLimit => "waiting: station full (train limit)",
            VState::Blocked => "waiting at signal",
            VState::Gated => "waiting: no stop needs a visit",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Carrier {
    pub id: u32,
    pub name: String,
    pub cargo: Inv,
    pub capacity: f64,
    pub schedule: Schedule,
    pub cur: usize,
    pub state: VState,
    /// Seconds spent blocked/queued without moving (jam detection).
    pub stuck: f64,
    pub retry: f64,
    pub warned: bool,
    /// Where the cargo aboard was loaded (for delivery pay).
    #[serde(default)]
    pub origins: Origins,
    /// Pounds earned per minute (rolling) and in total.
    #[serde(default)]
    pub income: Throughput,
    #[serde(default)]
    pub lifetime_income: f64,
    /// Game time it was bought (for profit over its lifetime).
    #[serde(default)]
    pub born: f64,
    /// Has the current stop's gate been checked since setting off?
    #[serde(default)]
    pub checked: bool,
}

impl Carrier {
    pub fn new(id: u32, name: String, capacity: f64) -> Carrier {
        Carrier {
            id,
            name,
            cargo: Inv::default(),
            capacity,
            schedule: Schedule::default(),
            cur: 0,
            state: VState::Idle,
            stuck: 0.0,
            retry: 0.0,
            warned: false,
            origins: Origins::default(),
            income: Throughput::default(),
            lifetime_income: 0.0,
            born: 0.0,
            checked: false,
        }
    }

    pub fn stop(&self) -> Option<&Stop> {
        if self.schedule.stops.is_empty() {
            None
        } else {
            self.schedule
                .stops
                .get(self.cur % self.schedule.stops.len())
        }
    }

    pub fn advance(&mut self) {
        if !self.schedule.stops.is_empty() {
            self.cur = (self.cur + 1) % self.schedule.stops.len();
        }
        self.checked = false;
        self.state = VState::Travel;
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Truck {
    pub carrier: Carrier,
    pub pos: Pos,
    pub path: Vec<Pos>,
    pub progress: f64,
    pub path_version: u64,
    #[serde(default)]
    pub path_dest: Pos,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VKind {
    Truck,
    Train,
}

impl World {
    pub fn carrier(&self, id: u32) -> Option<(&Carrier, VKind)> {
        if let Some(t) = self.trucks.iter().find(|t| t.carrier.id == id) {
            return Some((&t.carrier, VKind::Truck));
        }
        self.trains
            .iter()
            .find(|t| t.carrier.id == id)
            .map(|t| (&t.carrier, VKind::Train))
    }

    pub fn carrier_mut(&mut self, id: u32) -> Option<&mut Carrier> {
        if let Some(t) = self.trucks.iter_mut().find(|t| t.carrier.id == id) {
            return Some(&mut t.carrier);
        }
        self.trains
            .iter_mut()
            .find(|t| t.carrier.id == id)
            .map(|t| &mut t.carrier)
    }

    pub fn vehicle_pos(&self, id: u32) -> Option<Pos> {
        if let Some(t) = self.trucks.iter().find(|t| t.carrier.id == id) {
            return Some(t.pos);
        }
        self.trains
            .iter()
            .find(|t| t.carrier.id == id)
            .map(|t| t.tiles[0])
    }

    pub fn vehicles_at(&self, p: Pos) -> Vec<u32> {
        let mut v: Vec<u32> = self
            .trains
            .iter()
            .filter(|t| t.tiles.contains(&p))
            .map(|t| t.carrier.id)
            .collect();
        v.extend(
            self.trucks
                .iter()
                .filter(|t| t.pos == p)
                .map(|t| t.carrier.id),
        );
        v
    }

    /// Scrap a vehicle (its cargo is lost).
    pub fn remove_vehicle(&mut self, id: u32) -> Option<String> {
        let carrier = self.carrier(id)?.0.clone();
        let value = match self.trains.iter().find(|t| t.carrier.id == id) {
            Some(t) => t.value(),
            None => B::TRUCK_COST,
        };
        self.trucks.retain(|t| t.carrier.id != id);
        self.trains.retain(|t| t.carrier.id != id);
        let back = self.refund(value);
        rail::rebuild_blocks(self);
        let lost = carrier.cargo.total();
        let tail = if lost >= 0.5 {
            format!(" ({lost:.0} units of cargo lost)")
        } else {
            String::new()
        };
        Some(format!(
            "{} scrapped for {}{tail}.",
            carrier.name,
            money(back)
        ))
    }

    pub fn target_name(&self, t: Target) -> String {
        match t {
            Target::Site(id) => self.site_name(id),
            Target::Station(id) => self
                .stations
                .get(id as usize)
                .map(|s| s.name.clone())
                .unwrap_or("?".into()),
        }
    }

    pub fn spawn_truck(&mut self, site: u32) -> u32 {
        let id = self.new_id();
        let n = self.trucks.len() + 1;
        let mut c = Carrier::new(id, format!("Truck {n}"), B::TRUCK_CAPACITY);
        c.state = VState::Idle;
        c.born = self.time;
        let pos = self.sites[site as usize].pos;
        self.trucks.push(Truck {
            carrier: c,
            pos,
            path: vec![],
            progress: 0.0,
            path_version: 0,
            path_dest: pos,
        });
        id
    }

    /// Buy a truck at any town, industry or hub.
    pub fn buy_truck(&mut self, site: u32) -> Result<u32, String> {
        self.spend(B::TRUCK_COST, "a truck")?;
        Ok(self.spawn_truck(site))
    }

    /// Buy another truck like `id`, with the same schedule, at its first stop.
    pub fn clone_truck(&mut self, id: u32) -> Result<u32, String> {
        let Some(t) = self.trucks.iter().find(|t| t.carrier.id == id) else {
            return Err("Select a truck.".into());
        };
        let schedule = t.carrier.schedule.clone();
        let Some((stop, site)) =
            schedule
                .stops
                .iter()
                .enumerate()
                .find_map(|(i, s)| match s.target {
                    Target::Site(site) if self.sites[site as usize].state != Layer::None => {
                        Some((i, site))
                    }
                    _ => None,
                })
        else {
            return Err("Give it a schedule first: a copy takes the same stops.".into());
        };
        let new = self.buy_truck(site)?;
        let c = self.carrier_mut(new).unwrap();
        c.schedule = schedule;
        c.cur = stop;
        Ok(new)
    }

    /// Buy another of the vehicle `id` (truck or train) with its schedule.
    pub fn clone_vehicle(&mut self, id: u32) -> Result<u32, String> {
        match self.carrier(id).map(|c| c.1) {
            Some(VKind::Truck) => self.clone_truck(id),
            Some(VKind::Train) => rail::clone_train(self, id),
            None => Err("Select a vehicle to copy.".into()),
        }
    }

    pub fn road_passable(&self, from: Pos, d: Dir) -> bool {
        let to = from.step(d);
        if !self.map.in_bounds(to) {
            return false;
        }
        let a = self.tile(from);
        let b = self.tile(to);
        a.road_conn & d.bit() != 0 && b.road == Layer::Built && a.road == Layer::Built
    }

    pub fn road_route(&self, from: Pos, to: Pos) -> Option<Vec<Pos>> {
        path::astar(
            self.map.w,
            self.map.h,
            from,
            |p| p == to,
            to,
            10,
            |a, b| {
                let d = a.dir_to(b)?;
                if self.road_passable(a, d) {
                    Some(10)
                } else {
                    None
                }
            },
        )
    }
}

/// What one stop's worth of loading/unloading achieved.
#[derive(Clone, Copy, Debug, Default)]
pub struct Moved {
    pub units: f64,
    /// Pounds earned by deliveries.
    pub paid: f64,
}

/// Move goods between a vehicle and a site according to a stop, spending at
/// most `rate` units/sec. Industries and towns are customers: they pay for
/// what they accept and never hand it back (industries only sell their
/// output, towns only their passengers). Hubs store anything and remember
/// where it came from.
pub fn transfer(
    w: &mut World,
    site_id: u32,
    c: &mut Carrier,
    stop: &Stop,
    rate: f64,
    dt: f64,
) -> Moved {
    let mut out = Moved::default();
    let site = &w.sites[site_id as usize];
    if site.state != Layer::Built {
        return out;
    }
    let customer = w.is_customer(site);
    let (spos, skind) = (site.pos, site.kind);
    let now = w.time;
    let mut budget = rate * dt;
    for (phase, cargo) in stop
        .action
        .phases()
        .iter()
        .flat_map(|p| Cargo::ALL.iter().map(move |c| (*p, *c)))
    {
        if budget <= 1e-9 || !stop.filter.has(cargo) {
            continue;
        }
        match phase {
            Action::Load => {
                if !w.can_load(&w.sites[site_id as usize], cargo) {
                    continue;
                }
                let space = (c.capacity - c.cargo.total()).max(0.0);
                let s = &mut w.sites[site_id as usize];
                let amt = budget.min(space).min(s.inv[cargo]);
                if amt > 0.0 {
                    let lot = if skind == SiteKind::Hub {
                        s.origins.get(cargo)
                    } else {
                        Lot::at(spos, now)
                    };
                    s.inv[cargo] -= amt;
                    c.origins.mix(cargo, c.cargo[cargo], amt, lot);
                    c.cargo[cargo] += amt;
                    budget -= amt;
                    out.units += amt;
                }
            }
            Action::Unload => {
                let space = w.accept_space(&w.sites[site_id as usize], cargo);
                let amt = budget.min(space).min(c.cargo[cargo]);
                if amt <= 0.0 {
                    continue;
                }
                let lot = c.origins.get(cargo);
                // Passengers never get off where they got on.
                if cargo == Cargo::Passengers && lot.dist(spos) < B::PAY_MIN_DIST {
                    continue;
                }
                c.cargo[cargo] -= amt;
                budget -= amt;
                out.units += amt;
                match skind {
                    SiteKind::Town(t) => {
                        if cargo == Cargo::Passengers {
                            w.towns[t].arrived += amt;
                        } else {
                            w.towns[t].stock[cargo] += amt;
                        }
                        contracts::credit(w, t, cargo, amt);
                    }
                    SiteKind::Industry(_) => w.sites[site_id as usize].inv[cargo] += amt,
                    SiteKind::Hub => {
                        let s = &mut w.sites[site_id as usize];
                        s.origins.mix(cargo, s.inv[cargo], amt, lot);
                        s.inv[cargo] += amt;
                    }
                }
                if customer {
                    w.stats.delivered[cargo] += amt;
                    out.paid += economy::delivery_pay(cargo, amt, lot, spos, now)
                        * events::pay_mult(w, site_id, cargo);
                }
            }
            Action::Swap => {} // never a phase: `phases()` splits it into unload + load
        }
    }
    if out.paid > 0.0 {
        w.cash += out.paid;
        w.stats.revenue += out.paid;
        c.lifetime_income += out.paid;
    }
    let sec = w.now_sec();
    w.sites[site_id as usize].stats.add(sec, out.units);
    c.income.add(sec, out.paid);
    out
}

/// Should a vehicle leave its current stop?
pub fn should_leave(c: &Carrier, stop: &Stop, elapsed: f64, site_has_more: bool) -> bool {
    if let Some(t) = stop.timeout
        && elapsed >= t
    {
        return true;
    }
    match stop.cond {
        Cond::Full => c.cargo.total() >= c.capacity - 0.05,
        Cond::Empty => match stop.action {
            Action::Unload => c.cargo.total_mask(stop.filter) < 0.05,
            // "load until empty" = until the site has nothing more to give
            Action::Load | Action::Swap => !site_has_more,
        },
        Cond::Time(t) => elapsed >= t,
    }
}

pub fn tick_trucks(w: &mut World, dt: f64) {
    for l in w.road_load.iter_mut() {
        *l = 0;
    }
    for t in &w.trucks {
        let i = w.map.idx(t.pos);
        w.road_load[i] = w.road_load[i].saturating_add(1);
    }
    for s in w.sites.iter_mut() {
        s.bays_used = 0;
    }
    let mut trucks = std::mem::take(&mut w.trucks);
    for t in trucks.iter_mut() {
        step_truck(w, t, dt);
    }
    // keep any trucks spawned during the step
    trucks.append(&mut w.trucks);
    w.trucks = trucks;
}

fn step_truck(w: &mut World, t: &mut Truck, dt: f64) {
    let Some(stop) = t.carrier.stop().cloned() else {
        t.carrier.state = VState::Idle;
        return;
    };
    if !check_gates(w, &mut t.carrier) {
        return;
    }
    let Target::Site(site_id) = stop.target else {
        t.carrier.state = VState::NoPath;
        return;
    };
    let dest = w.sites[site_id as usize].pos;
    match t.carrier.state {
        VState::Idle => t.carrier.state = VState::Travel,
        VState::Working(elapsed) => {
            let site = &w.sites[site_id as usize];
            if site.bays_used >= B::SITE_BAYS {
                t.carrier.stuck += dt; // queued at a busy site
                return;
            }
            let rate = if site.kind == SiteKind::Hub {
                B::HUB_LOAD_RATE
            } else {
                B::SITE_LOAD_RATE
            };
            w.sites[site_id as usize].bays_used += 1;
            let moved = transfer(w, site_id, &mut t.carrier, &stop, rate, dt).units;
            let more = moved > 1e-6;
            let elapsed = elapsed + dt;
            if should_leave(&t.carrier, &stop, elapsed, more) {
                t.carrier.advance();
                t.path.clear();
            } else {
                t.carrier.state = VState::Working(elapsed);
            }
            t.carrier.stuck = 0.0;
        }
        VState::NoPath | VState::Gated => {
            t.carrier.retry -= dt;
            if t.carrier.retry <= 0.0 {
                t.carrier.state = VState::Travel;
                t.path.clear();
            }
        }
        _ => {
            if t.pos == dest {
                t.carrier.state = VState::Working(0.0);
                t.progress = 0.0;
                return;
            }
            if t.path.is_empty() || t.path_version != w.net_version || t.path_dest != dest {
                match w.road_route(t.pos, dest) {
                    Some(p) => {
                        t.path = p.into_iter().skip(1).collect();
                        t.path.reverse(); // pop from the end
                        t.path_version = w.net_version;
                        t.path_dest = dest;
                        t.carrier.warned = false;
                    }
                    None => {
                        t.carrier.state = VState::NoPath;
                        t.carrier.retry = 3.0;
                        if !t.carrier.warned {
                            t.carrier.warned = true;
                            let msg = format!(
                                "{} can't find a road to {}.",
                                t.carrier.name,
                                w.target_name(stop.target)
                            );
                            w.alert(AlertLevel::Warn, msg, Some(t.pos));
                        }
                        return;
                    }
                }
            }
            t.carrier.state = VState::Travel;
            let load = w.road_load[w.map.idx(t.pos)] as f64;
            let jam = (load - B::ROAD_TILE_CAPACITY).max(0.0);
            let speed = B::TRUCK_SPEED / (1.0 + jam * B::ROAD_JAM_SLOWDOWN);
            t.progress += speed * dt;
            while t.progress >= 1.0 {
                let Some(&next) = t.path.last() else { break };
                let Some(d) = t.pos.dir_to(next) else {
                    t.path.clear();
                    break;
                };
                if !w.road_passable(t.pos, d) {
                    t.path.clear();
                    break;
                }
                t.path.pop();
                t.pos = next;
                t.progress -= 1.0;
                w.add_flow(next, t.carrier.cargo.total());
                if t.pos == dest {
                    t.carrier.state = VState::Working(0.0);
                    t.progress = 0.0;
                    break;
                }
            }
        }
    }
}

/// What a stop's gate sees right now, to decide and to show the player.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GateReading {
    /// Units waiting there (a pick-up gate) or room for more (a drop-off gate).
    pub now: f64,
    /// The most there can ever be (a town only stocks a few minutes of need):
    /// a gate asking for more never opens.
    pub most: f64,
    /// What the gate asks for.
    pub need: f64,
    /// The cargo counted.
    pub cargo: CargoMask,
}

impl GateReading {
    pub fn open(&self) -> bool {
        self.now > 0.0 && self.now >= self.need - 1e-6
    }

    pub fn never(&self) -> bool {
        self.most < self.need - 1e-6
    }
}

/// The cargo a gate counts. Waiting: the stop's cargo filter. Wants: what
/// the vehicle has aboard from that filter, since that's what it would drop
/// off (passengers only when it carries some: a town takes any number of
/// them); with nothing aboard, the filter.
fn gate_cargo(stop: &Stop, aboard: &Inv) -> CargoMask {
    let in_filter = || Cargo::ALL.into_iter().filter(|c| stop.filter.has(*c));
    if matches!(stop.gate, Some(Gate::Wants(_))) {
        let carried = in_filter()
            .filter(|c| aboard[*c] > 0.05)
            .fold(CargoMask::NONE, |m, c| m.with(c));
        if carried != CargoMask::NONE {
            return carried;
        }
        if stop.filter != CargoMask::single(Cargo::Passengers) {
            return in_filter()
                .filter(|c| *c != Cargo::Passengers)
                .fold(CargoMask::NONE, |m, c| m.with(c));
        }
    }
    stop.filter
}

/// Read a stop's gate for a vehicle carrying `aboard` (None: it has no gate).
pub fn gate_reading(w: &World, stop: &Stop, aboard: &Inv) -> Option<GateReading> {
    let gate = stop.gate?;
    let sites = match stop.target {
        Target::Site(id) => vec![id],
        Target::Station(sid) => match w.stations.get(sid as usize) {
            Some(s) if !s.removed => rail::station_sites(w, s.pos),
            _ => vec![],
        },
    };
    let cargo = gate_cargo(stop, aboard);
    // Totals over the stop's sites and cargos: (amount now, the most it can
    // be, and which cargos the stop deals in at all, to name them).
    let total = |f: &dyn Fn(&crate::world::Site, Cargo) -> (f64, f64)| {
        let mut sum = (0.0, 0.0, CargoMask::NONE);
        for s in &sites {
            for c in Cargo::ALL.into_iter().filter(|c| cargo.has(*c)) {
                let (now, most) = f(&w.sites[*s as usize], c);
                sum = (
                    sum.0 + now,
                    sum.1 + most,
                    if most > 0.0 { sum.2.with(c) } else { sum.2 },
                );
            }
        }
        sum
    };
    let ((now, most, dealt), need) = match gate {
        Gate::Waiting(n) => (
            total(&|s, c| {
                if w.can_load(s, c) {
                    (s.inv[c], w.hold_cap(s, c))
                } else {
                    (0.0, 0.0)
                }
            }),
            n,
        ),
        Gate::Wants(n) => (total(&|s, c| (w.accept_space(s, c), w.accept_cap(s, c))), n),
    };
    let cargo = if dealt == CargoMask::NONE {
        cargo
    } else {
        dealt
    };
    Some(GateReading {
        now,
        most,
        need,
        cargo,
    })
}

/// Is a stop's gate open right now for a vehicle carrying `aboard` (no gate = always open)?
/// The gate means exactly what it says: one asking for more than the stop can
/// ever hold never opens (the schedule editor says so).
pub fn gate_open(w: &World, stop: &Stop, aboard: &Inv) -> bool {
    gate_reading(w, stop, aboard).is_none_or(|r| r.open())
}

/// Move a vehicle on to the next stop whose gate is open (starting with the
/// current one). Returns false when every gate is shut.
pub fn pick_open_stop(w: &World, c: &mut Carrier) -> bool {
    let n = c.schedule.stops.len();
    for i in 0..n {
        let k = (c.cur + i) % n;
        if gate_open(w, &c.schedule.stops[k], &c.cargo) {
            c.cur = k;
            return true;
        }
    }
    false
}

/// Shared by trucks and trains when they set off: skip stops whose gate is
/// shut. Returns true if the vehicle should go on this tick (false = it is
/// now waiting, or its stop changed and must be re-read next tick).
pub fn check_gates(w: &World, c: &mut Carrier) -> bool {
    if c.checked || !matches!(c.state, VState::Travel | VState::Idle) {
        return true;
    }
    let before = c.cur;
    if !pick_open_stop(w, c) {
        c.state = VState::Gated;
        c.retry = 3.0;
        return false;
    }
    c.checked = true;
    c.cur == before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copied_truck_takes_the_same_route() {
        let mut w = World::new(42, Settings::default());
        let first = w.trucks[0].carrier.id;
        let (cash, n) = (w.cash, w.trucks.len());
        let copy = w.clone_vehicle(first).unwrap();
        assert_eq!(w.trucks.len(), n + 1);
        assert_eq!(w.cash, cash - B::TRUCK_COST);
        let stops = |id| {
            w.carrier(id)
                .unwrap()
                .0
                .schedule
                .stops
                .iter()
                .map(|s| s.target)
                .collect::<Vec<_>>()
        };
        assert_eq!(stops(copy), stops(first));
        let Target::Site(at) = w.carrier(first).unwrap().0.schedule.stops[0].target else {
            panic!()
        };
        assert_eq!(
            w.vehicle_pos(copy),
            Some(w.sites[at as usize].pos),
            "starts at its first stop"
        );
        // nothing to copy from a truck without stops
        let bare = w.buy_truck(at).unwrap();
        assert!(w.clone_vehicle(bare).is_err());
    }

    #[test]
    fn gated_stops_are_skipped_and_all_shut_means_wait() {
        let mut w = World::new(5, Settings::default());
        let farms: Vec<u32> = w
            .sites
            .iter()
            .filter(|s| matches!(s.kind, SiteKind::Industry(crate::map::IndustryKind::Farm)))
            .map(|s| s.id)
            .collect();
        let (a, b) = (farms[0], farms[1]);
        w.sites[a as usize].inv[Cargo::Grain] = 5.0;
        w.sites[b as usize].inv[Cargo::Grain] = 50.0;
        let mut c = Carrier::new(1, "T".into(), 15.0);
        let mut sa = Stop::new(Target::Site(a), Action::Load);
        sa.gate = Some(Gate::Waiting(20.0));
        let mut sb = Stop::new(Target::Site(b), Action::Load);
        sb.gate = Some(Gate::Waiting(20.0));
        c.schedule.stops = vec![sa, sb];
        c.state = VState::Travel;
        assert!(!check_gates(&w, &mut c), "stop changed: re-read next tick");
        assert_eq!(c.cur, 1, "farm A has too little grain, so go to farm B");
        assert!(check_gates(&w, &mut c), "checked once per leg");
        // Now nothing qualifies.
        w.sites[b as usize].inv[Cargo::Grain] = 0.0;
        c.advance();
        assert!(!check_gates(&w, &mut c));
        assert_eq!(c.state, VState::Gated);
    }

    /// "Unload everything · only if it wants ≥10000" at a small town: the
    /// gate means what it says and never opens (it used to, because a town
    /// takes any number of passengers, and the train carried none). What it
    /// counts is what the train would drop off.
    #[test]
    fn gates_count_what_would_be_dropped_off() {
        let mut w = World::new(42, Settings::default());
        let ti = w.towns.iter().position(|t| t.wants(Cargo::Food)).unwrap();
        let site = w
            .sites
            .iter()
            .find(|s| s.kind == SiteKind::Town(ti))
            .unwrap()
            .id;
        w.towns[ti].stock[Cargo::Food] = 0.0;
        let most = w.accept_cap(&w.sites[site as usize], Cargo::Food);
        let mut food = Inv::default();
        food[Cargo::Food] = 30.0;
        let mut stop = Stop::new(Target::Site(site), Action::Unload); // everything
        stop.gate = Some(Gate::Wants(10_000.0));
        let r = gate_reading(&w, &stop, &food).unwrap();
        assert_eq!(
            r.cargo,
            CargoMask::single(Cargo::Food),
            "counts the food aboard, not passengers"
        );
        assert!(!r.open() && r.never(), "{r:?}");
        assert!(
            !gate_open(&w, &stop, &Inv::default()),
            "empty, it counts the goods the town wants, still not 10000"
        );
        // a gate the town can meet opens while it's short, and shuts once stocked
        stop.gate = Some(Gate::Wants(most / 2.0));
        assert!(gate_open(&w, &stop, &food));
        w.towns[ti].stock[Cargo::Food] = most * 0.9;
        assert!(!gate_open(&w, &stop, &food));
        // carrying passengers, the town always has room for them
        let mut pass = Inv::default();
        pass[Cargo::Passengers] = 10.0;
        stop.gate = Some(Gate::Wants(10_000.0));
        assert!(gate_open(&w, &stop, &pass));
    }
}
