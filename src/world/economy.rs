//! Industries make things, towns want things, pay and running costs.

use super::*;

pub fn tick(w: &mut World, dt: f64) {
    for i in 0..w.sites.len() {
        if w.sites[i].state != Layer::Built {
            continue;
        }
        if let SiteKind::Industry(k) = w.sites[i].kind {
            let mult = events::production_mult(w, i as u32);
            produce(&mut w.sites[i], k, mult, dt);
        }
    }
    for t in 0..w.towns.len() {
        tick_town(w, t, dt);
    }
}

/// Primary industries fill their output buffer; secondary ones also burn one
/// unit of each input per unit of output.
fn produce(s: &mut Site, k: IndustryKind, mult: f64, dt: f64) {
    let out = k.output();
    if s.inv[out] >= B::NODE_BUFFER - 1e-6 {
        s.full_secs += dt;
    }
    let mut amt =
        (B::INDUSTRY_RATE[k.index()] * s.level * mult * dt).min(B::NODE_BUFFER - s.inv[out]);
    for c in k.inputs() {
        amt = amt.min(s.inv[*c]);
    }
    if amt <= 0.0 {
        return;
    }
    for c in k.inputs() {
        s.inv[*c] -= amt;
    }
    s.inv[out] += amt;
}

/// Tier for a population, given the current tier (shrinking needs to go
/// below `TIER_HYSTERESIS` of the threshold to drop a tier).
pub fn tier_for(pop: f64, current: usize) -> usize {
    let mut t = 0;
    for (i, th) in B::TIER_POP.iter().enumerate() {
        let need = if i <= current {
            th * B::TIER_HYSTERESIS
        } else {
            *th
        };
        if pop >= need {
            t = i;
        }
    }
    t
}

fn tick_town(w: &mut World, ti: usize, dt: f64) {
    let site = w.towns[ti].site as usize;
    let festival = events::passenger_mult(w, site as u32);
    let town = &mut w.towns[ti];
    // Use up the stock; satisfaction follows how much of the demand was met.
    for &c in town.demands() {
        let need = town.demand_per_min(c) * dt / 60.0;
        let got = town.stock.take(c, need);
        let met = if need > 0.0 { got / need } else { 1.0 };
        let s = &mut town.sat[c.index()];
        *s += (met - *s) * dt / B::SATISFACTION_SECS;
    }
    let happy = town.happiness();
    // Grow when happy, shrink when not (never below the starting size).
    let rate = if happy >= B::GROWTH_NEUTRAL {
        B::GROWTH_PER_MIN * (happy - B::GROWTH_NEUTRAL) / (1.0 - B::GROWTH_NEUTRAL)
    } else {
        -B::SHRINK_PER_MIN * (B::GROWTH_NEUTRAL - happy) / B::GROWTH_NEUTRAL
    };
    town.pop = (town.pop * (1.0 + rate * dt / 60.0)).max(town.start_pop);
    // Passengers want to go somewhere.
    let made =
        town.pop / 1000.0 * B::PASSENGERS_PER_1000 * (0.5 + 0.5 * happy) * festival * dt / 60.0;
    let cap = w.hold_cap(&w.sites[site], Cargo::Passengers);
    let wait = &mut w.sites[site].inv[Cargo::Passengers];
    *wait = (*wait + made).min(cap);
}

/// Once per second: tier changes and who's served.
pub fn tick_second(w: &mut World) {
    for ti in 0..w.towns.len() {
        let t = &w.towns[ti];
        let new = tier_for(t.pop, t.tier);
        if new == t.tier {
            continue;
        }
        let (name, pos, old) = (t.name.clone(), t.pos, t.tier);
        let town = &mut w.towns[ti];
        town.tier = new;
        town.best_tier = town.best_tier.max(new);
        if new > old {
            // The new demand starts unmet: that's the next problem.
            let mut msgs = vec![];
            for &c in TIER_DEMANDS[new] {
                if !TIER_DEMANDS[old].contains(&c) {
                    town.sat[c.index()] = 0.0;
                    msgs.push(format!(
                        "★ {name} is now a {}! Population {:.0}. It wants {} too — deliver it or growth stalls.",
                        TIER_NAMES[new],
                        town.pop,
                        c.name().to_lowercase()
                    ));
                }
            }
            for m in msgs {
                w.celebrate(m, Some(pos), Fanfare::TierUp);
            }
        } else {
            w.alert(
                AlertLevel::Warn,
                format!("{name} has shrunk back to a {}.", TIER_NAMES[new]),
                Some(pos),
            );
        }
    }
    if w.now_sec().is_multiple_of(5) {
        update_served(w);
    }
    if w.now_sec().is_multiple_of(60) {
        adjust_levels(w);
    }
}

/// Once a minute: primary industries you keep hauled out produce more;
/// ones left full (wasting) slow back down.
fn adjust_levels(w: &mut World) {
    for i in 0..w.sites.len() {
        let SiteKind::Industry(k) = w.sites[i].kind else {
            continue;
        };
        let s = &mut w.sites[i];
        let full = std::mem::take(&mut s.full_secs);
        if !k.is_primary() {
            continue;
        }
        let old = s.level;
        let hauled = s.stats.per_min() > 0.5;
        if full < B::LEVEL_GROW_IF_FULL_UNDER && hauled {
            s.level = (s.level + B::LEVEL_STEP).min(B::LEVEL_MAX);
        } else if full > B::LEVEL_SHRINK_IF_FULL_OVER {
            s.level = (s.level - B::LEVEL_STEP).max(1.0);
        }
        if s.level.floor() > old.floor() {
            let (name, pos, lvl) = (w.site_name(i as u32), w.sites[i].pos, w.sites[i].level);
            w.alert(
                AlertLevel::Good,
                format!("{name} is booming: production x{lvl:.0}. Keep it hauled!"),
                Some(pos),
            );
        }
    }
}

/// A town is served if one of your stations covers it or a truck stops there.
pub fn update_served(w: &mut World) {
    for ti in 0..w.towns.len() {
        let site = w.towns[ti].site;
        let pos = w.towns[ti].pos;
        let by_rail = w
            .stations
            .iter()
            .any(|s| !s.removed && rail::in_catchment(s.pos, pos));
        let by_road = w.trucks.iter().any(|t| {
            t.carrier
                .schedule
                .stops
                .iter()
                .any(|s| s.target == Target::Site(site))
        });
        let was = w.towns[ti].served;
        w.towns[ti].served = by_rail || by_road;
        if !was && w.towns[ti].served && w.time > 1.0 {
            let name = w.towns[ti].name.clone();
            w.alert(
                AlertLevel::Good,
                format!("{name} is now on your network. Its people count toward Prosperity."),
                Some(pos),
            );
        }
    }
}

/// Speed factor for cargo `c` that has been in transit `secs` seconds.
pub fn freshness(c: Cargo, secs: f64) -> f64 {
    let late = (secs - B::PAY_GRACE_SECS[c.index()]).max(0.0);
    (1.0 - late / B::PAY_DECAY_SECS[c.index()]).max(B::PAY_FLOOR)
}

/// What a delivery of `amt` units of `c`, loaded at `lot`, pays at `to`:
/// units x rate x straight-line distance x freshness.
pub fn delivery_pay(c: Cargo, amt: f64, lot: crate::types::Lot, to: Pos, now: f64) -> f64 {
    let dist = lot.dist(to);
    if dist < B::PAY_MIN_DIST {
        return 0.0;
    }
    amt * B::PAY_RATE[c.index()] * dist * freshness(c, now - lot.t)
}

/// Every tick: the fleet's running costs.
pub fn pay_upkeep(w: &mut World, dt: f64) {
    if w.sandbox() {
        return;
    }
    let cost = w.upkeep_per_min() * dt / 60.0;
    let was_positive = w.cash >= 0.0;
    w.cash -= cost;
    w.stats.upkeep += cost;
    if was_positive && w.cash < 0.0 {
        w.alert(
            AlertLevel::Danger,
            "IN THE RED! Running costs have emptied the till. Scrap idle vehicles or deliver more."
                .into(),
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Lot;

    #[test]
    fn longer_and_faster_hauls_pay_more() {
        let from = Lot::at(Pos::new(0, 0), 0.0);
        let near = delivery_pay(Cargo::Food, 10.0, from, Pos::new(10, 0), 10.0);
        let far = delivery_pay(Cargo::Food, 10.0, from, Pos::new(40, 0), 10.0);
        let slow = delivery_pay(Cargo::Food, 10.0, from, Pos::new(40, 0), 400.0);
        assert!(far > near * 3.5, "distance should scale pay");
        assert!(slow < far, "late cargo should pay less");
        assert!(slow >= far * B::PAY_FLOOR - 1e-9);
        assert_eq!(
            delivery_pay(Cargo::Food, 10.0, from, Pos::new(2, 0), 10.0),
            0.0,
            "tiny hops pay nothing"
        );
    }

    #[test]
    fn spending_is_refused_when_broke() {
        let mut w = World::new(3, Settings::default());
        w.cash = 100.0;
        assert!(w.spend(150.0, "a thing").is_err());
        assert_eq!(w.cash, 100.0);
        assert!(w.spend(60.0, "a thing").is_ok());
        assert_eq!(w.cash, 40.0);
    }

    #[test]
    fn starter_trucks_earn_money() {
        let mut w = World::new(42, Settings::default());
        for _ in 0..6000 {
            w.tick();
        }
        assert!(
            w.stats.revenue > 0.0,
            "the starter trucks should have been paid for a delivery"
        );
    }

    #[test]
    fn secondary_industry_needs_every_input() {
        let mut s = World::new(1, Settings::default()).sites[0].clone();
        s.inv = Inv::default();
        s.inv[Cargo::Steel] = 10.0;
        produce(&mut s, IndustryKind::Factory, 1.0, 1.0);
        assert_eq!(s.inv[Cargo::Goods], 0.0, "no lumber, no goods");
        s.inv[Cargo::Lumber] = 10.0;
        produce(&mut s, IndustryKind::Factory, 1.0, 1.0);
        assert!(s.inv[Cargo::Goods] > 0.0);
        assert!(s.inv[Cargo::Steel] < 10.0 && s.inv[Cargo::Lumber] < 10.0);
    }

    #[test]
    fn fed_town_grows_and_unserved_town_does_not() {
        let mut w = World::new(7, Settings::default());
        let fed = 0;
        let lonely = w
            .towns
            .iter()
            .position(|t| !t.served)
            .expect("some town starts unserved");
        let start = (w.towns[fed].pop, w.towns[lonely].pop);
        for _ in 0..(600 * B::TICKS_PER_SEC) {
            for c in [Cargo::Food, Cargo::Lumber] {
                let need = w.towns[fed].demand_per_min(c) * 4.0;
                w.towns[fed].stock[c] = need;
            }
            w.tick();
        }
        assert!(
            w.towns[fed].pop > start.0 * 1.3,
            "a fed town should grow a lot in 10 minutes"
        );
        assert_eq!(
            w.towns[lonely].pop, start.1,
            "an unserved town stays at its starting size"
        );
    }

    #[test]
    fn hauled_industries_grow_and_neglected_ones_do_not() {
        let mut w = World::new(5, Settings::default());
        let farms: Vec<usize> = (0..w.sites.len())
            .filter(|i| w.sites[*i].kind == SiteKind::Industry(IndustryKind::Farm))
            .collect();
        let (kept, left) = (farms[0], farms[1]);
        for _ in 0..(300 * B::TICKS_PER_SEC) {
            // someone keeps farm 0 empty
            let now = w.now_sec();
            let s = &mut w.sites[kept];
            let g = s.inv.take(Cargo::Grain, 1000.0);
            s.stats.add(now, g);
            w.tick();
        }
        assert!(
            w.sites[kept].level >= 2.0,
            "hauled farm grew to {}",
            w.sites[kept].level
        );
        assert_eq!(w.sites[left].level, 1.0, "neglected farm stays at 1");
    }

    #[test]
    fn tiers_have_hysteresis() {
        assert_eq!(tier_for(499.0, 0), 0);
        assert_eq!(tier_for(500.0, 0), 1);
        assert_eq!(tier_for(470.0, 1), 1, "no flicker just under the line");
        assert_eq!(tier_for(440.0, 1), 0);
    }
}
