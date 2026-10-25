//! Milestones: "railway firsts" that pay a bounty the moment you reach them.
//! They give every stage of a game a next goal (your first train, your first
//! city, every town on the network...) and an early-game cash boost for
//! getting the network going. Each is a read-only check on the `World`.

use super::*;

pub struct Milestone {
    pub name: &'static str,
    /// What to do, shown in the statistics screen before it's reached.
    pub goal: &'static str,
    /// Flavor for the announcement.
    pub quip: &'static str,
    /// Bounty size: index into `balance::MILESTONE_CASH`.
    pub size: usize,
    check: fn(&World) -> bool,
}

impl Milestone {
    pub fn reward(&self) -> f64 {
        B::MILESTONE_CASH[self.size]
    }
}

fn delivered(w: &World, c: Cargo) -> f64 {
    w.stats.delivered[c]
}

fn towns_served(w: &World) -> usize {
    w.towns.iter().filter(|t| t.served).count()
}

fn signals(w: &World, chain: bool) -> usize {
    w.tiles
        .iter()
        .filter(|t| match t.signal {
            Some(SignalKind::Chain | SignalKind::ChainOneWay(_)) => true,
            Some(_) => !chain,
            None => false,
        })
        .count()
}

pub static MILESTONES: &[Milestone] = &[
    Milestone {
        name: "All aboard",
        goal: "Buy your first train",
        quip: "A whistle echoes down the valley.",
        size: 0,
        check: |w| !w.trains.is_empty(),
    },
    Milestone {
        name: "Joined up",
        goal: "Put 3 towns on your network",
        quip: "Neighbours who had never met now share a timetable of gossip.",
        size: 0,
        check: |w| towns_served(w) >= 3,
    },
    Milestone {
        name: "Daily bread",
        goal: "Deliver 500 food",
        quip: "Not a single loaf goes stale on your watch.",
        size: 0,
        check: |w| delivered(w, Cargo::Food) >= 500.0,
    },
    Milestone {
        name: "Timber!",
        goal: "Deliver 200 lumber",
        quip: "Every new porch in the county smells of your sawdust.",
        size: 0,
        check: |w| delivered(w, Cargo::Lumber) >= 200.0,
    },
    Milestone {
        name: "Rush hour",
        goal: "Deliver 100 passengers",
        quip: "Commuters now complain about your trains, which means they rely on them.",
        size: 0,
        check: |w| delivered(w, Cargo::Passengers) >= 100.0,
    },
    Milestone {
        name: "Signalman",
        goal: "Have 12 signals on your railway",
        quip: "Red, green, red, green. You dream in semaphore now.",
        size: 0,
        check: |w| signals(w, false) >= 12,
    },
    Milestone {
        name: "Chain gang",
        goal: "Place a chain signal",
        quip: "Trains now wait politely outside junctions instead of in them.",
        size: 0,
        check: |w| signals(w, true) >= 1,
    },
    Milestone {
        name: "Express service",
        goal: "Buy an express train",
        quip: "Top hats ripple through first class.",
        size: 0,
        check: |w| w.trains.iter().any(|t| t.kind == TrainKind::Express),
    },
    Milestone {
        name: "Bright lights",
        goal: "Grow a town into a City",
        quip: "Gas lamps, a bandstand, and someone's already opened a theatre.",
        size: 1,
        check: |w| w.towns.iter().any(|t| t.best_tier >= 2),
    },
    Milestone {
        name: "The web",
        goal: "Put 5 towns on your network",
        quip: "Maps of the county now have your lines drawn on them.",
        size: 1,
        check: |w| towns_served(w) >= 5,
    },
    Milestone {
        name: "Forged in fire",
        goal: "Deliver 150 steel",
        quip: "The steelworks glows all night, and the factory hums all day.",
        size: 1,
        check: |w| delivered(w, Cargo::Steel) >= 150.0,
    },
    Milestone {
        name: "Made to order",
        goal: "Deliver 150 goods",
        quip: "Shop windows fill with things nobody knew they needed.",
        size: 1,
        check: |w| delivered(w, Cargo::Goods) >= 150.0,
    },
    Milestone {
        name: "Fill her up",
        goal: "Deliver 100 fuel",
        quip: "The first motor car in town putters past. Nobody is impressed.",
        size: 1,
        check: |w| delivered(w, Cargo::Fuel) >= 100.0,
    },
    Milestone {
        name: "Rolling stock",
        goal: "Run 6 trains at once",
        quip: "The engine shed needs an engine shed.",
        size: 1,
        check: |w| w.trains.len() >= 6,
    },
    Milestone {
        name: "Contractor",
        goal: "Fill 3 contracts",
        quip: "The mayors have your address now. All of them.",
        size: 1,
        check: |w| w.stats.contracts_done >= 3,
    },
    Milestone {
        name: "Boomtown",
        goal: "Grow a primary industry to full production",
        quip: "They're hiring at the gate, and the gate is new.",
        size: 1,
        check: |w| w.sites.iter().any(|s| s.level >= B::LEVEL_MAX),
    },
    Milestone {
        name: "Grand Central",
        goal: "Move 150 units a minute through one station",
        quip: "Porters, pigeons and pickpockets: a proper station at last.",
        size: 2,
        check: |w| {
            w.stations
                .iter()
                .any(|s| !s.removed && s.stats.per_min() >= 150.0)
        },
    },
    Milestone {
        name: "Every last one",
        goal: "Put every town on your network",
        quip: "No town in the region is more than a ticket away.",
        size: 2,
        check: |w| towns_served(w) == w.towns.len(),
    },
    Milestone {
        name: "Metropolis",
        goal: "Grow a town into a Metropolis",
        quip: "Skyline, smog and a tram strike. Progress!",
        size: 2,
        check: |w| w.towns.iter().any(|t| t.best_tier >= 3),
    },
    Milestone {
        name: "Tycoon",
        goal: "Earn £25,000 in deliveries",
        quip: "Your portrait now hangs in the boardroom, slightly crooked.",
        size: 2,
        check: |w| w.stats.revenue >= 25_000.0,
    },
];

/// Every couple of seconds: pay out any milestone just reached.
pub fn tick_second(w: &mut World) {
    if !w.now_sec().is_multiple_of(2) {
        return;
    }
    w.milestones.resize(MILESTONES.len(), None);
    for (i, m) in MILESTONES.iter().enumerate() {
        if w.milestones[i].is_some() || !(m.check)(w) {
            continue;
        }
        w.milestones[i] = Some(w.time);
        w.cash += m.reward();
        let pay = if w.sandbox() {
            String::new()
        } else {
            format!(" +{}", money(m.reward()))
        };
        let msg = format!("✦ MILESTONE: {}!{pay} — {}", m.name, m.quip);
        let home = w.towns[w.map.home].pos;
        w.celebrate(msg, Some(home), Fanfare::Milestone);
    }
}

/// How many milestones this game has reached.
pub fn reached(w: &World) -> usize {
    w.milestones.iter().filter(|m| m.is_some()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn milestones_pay_once() {
        let mut w = World::new(7, Settings::default());
        let cash = w.cash;
        w.stats.delivered[Cargo::Food] = 600.0;
        w.ticks = 20;
        tick_second(&mut w);
        let i = MILESTONES
            .iter()
            .position(|m| m.name == "Daily bread")
            .unwrap();
        assert!(w.milestones[i].is_some());
        assert_eq!(w.cash, cash + B::MILESTONE_CASH[0]);
        assert!(w.alerts.last().unwrap().fanfare == Some(Fanfare::Milestone));
        tick_second(&mut w);
        assert_eq!(w.cash, cash + B::MILESTONE_CASH[0], "paid only once");
        assert_eq!(reached(&w), 1);
    }
}
