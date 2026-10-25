//! Contracts: towns post short-term orders ("120 lumber to Ashford within 8
//! minutes"). Filling one pays cash, grows the town at once and adds to the
//! final score. Missing one costs nothing but the chance.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ContractState {
    Open,
    Done,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Contract {
    pub town: usize,
    pub cargo: Cargo,
    pub amount: f64,
    pub delivered: f64,
    pub posted: f64,
    pub deadline: f64,
    pub state: ContractState,
}

impl Contract {
    pub fn cash_reward(&self) -> f64 {
        self.amount * B::CONTRACT_CASH_PER_UNIT
    }

    pub fn score_reward(&self) -> f64 {
        self.amount * B::CONTRACT_SCORE_PER_UNIT
    }

    pub fn describe(&self, w: &World) -> String {
        format!(
            "{:.0} {} to {}",
            self.amount,
            self.cargo.name().to_lowercase(),
            w.towns[self.town].name
        )
    }
}

/// Count a delivery toward any open contract it fills.
pub fn credit(w: &mut World, town: usize, cargo: Cargo, amt: f64) {
    for c in w
        .contracts
        .iter_mut()
        .filter(|c| c.state == ContractState::Open && c.town == town && c.cargo == cargo)
    {
        c.delivered += amt;
    }
}

/// Once per second: post new contracts, pay out finished ones, expire late ones.
pub fn tick_second(w: &mut World) {
    let now = w.time;
    for i in 0..w.contracts.len() {
        if w.contracts[i].state != ContractState::Open {
            continue;
        }
        let c = w.contracts[i].clone();
        let pos = w.towns[c.town].pos;
        if c.delivered >= c.amount - 1e-6 {
            w.contracts[i].state = ContractState::Done;
            w.cash += c.cash_reward();
            w.stats.contract_bonus += c.score_reward();
            w.stats.contracts_done += 1;
            w.towns[c.town].pop *= 1.0 + B::CONTRACT_GROWTH;
            let msg = format!(
                "✔ CONTRACT FILLED: {}! +{} and +{:.0} score. The town is booming.",
                c.describe(w),
                money(c.cash_reward()),
                c.score_reward()
            );
            w.celebrate(msg, Some(pos), Fanfare::Contract);
        } else if now >= c.deadline {
            w.contracts[i].state = ContractState::Failed;
            let msg = format!(
                "✘ Contract missed: {} ({:.0} of {:.0} arrived in time).",
                c.describe(w),
                c.delivered,
                c.amount
            );
            w.alert(AlertLevel::Warn, msg, Some(pos));
        }
    }
    let open = w
        .contracts
        .iter()
        .filter(|c| c.state == ContractState::Open)
        .count();
    let due = now >= B::CONTRACT_FIRST
        && (w.contracts.is_empty() || now - w.stats.last_contract >= B::CONTRACT_EVERY);
    let time_to_finish = w
        .time_left()
        .is_none_or(|left| left > B::CONTRACT_MINUTES * 60.0);
    if open < B::CONTRACT_MAX_OPEN && due && time_to_finish {
        post(w);
    }
}

/// Post one contract for a town on the network (or any town if none are),
/// asking for something it wants or for passengers.
fn post(w: &mut World) {
    w.stats.last_contract = w.time;
    let served: Vec<usize> = (0..w.towns.len()).filter(|t| w.towns[*t].served).collect();
    let pool: Vec<usize> = if served.is_empty() {
        (0..w.towns.len()).collect()
    } else {
        served
    };
    let Some(&town) = w.rng.pick(&pool) else {
        return;
    };
    let mut wants: Vec<Cargo> = w.towns[town].demands().to_vec();
    wants.push(Cargo::Passengers);
    let busy: Vec<Cargo> = w
        .contracts
        .iter()
        .filter(|c| c.state == ContractState::Open && c.town == town)
        .map(|c| c.cargo)
        .collect();
    wants.retain(|c| !busy.contains(c));
    let Some(&cargo) = w.rng.pick(&wants) else {
        return;
    };
    let t = &w.towns[town];
    let raw = if cargo == Cargo::Passengers {
        t.pop / 1000.0 * B::CONTRACT_PASSENGERS_PER_1000
    } else {
        t.demand_per_min(cargo) * B::CONTRACT_DEMAND_MINUTES
    };
    let amount = ((raw.max(B::CONTRACT_MIN) / 10.0).round() * 10.0).max(B::CONTRACT_MIN);
    let c = Contract {
        town,
        cargo,
        amount,
        delivered: 0.0,
        posted: w.time,
        deadline: w.time + B::CONTRACT_MINUTES * 60.0,
        state: ContractState::Open,
    };
    let msg = format!(
        "§ NEW CONTRACT: {} within {:.0} minutes. Pays {} and +{:.0} score.",
        c.describe(w),
        B::CONTRACT_MINUTES,
        money(c.cash_reward()),
        c.score_reward()
    );
    let pos = w.towns[town].pos;
    w.contracts.push(c);
    w.alert(AlertLevel::Info, msg, Some(pos));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contracts_post_pay_and_expire() {
        let mut w = World::new(5, Settings::default());
        for _ in 0..((B::CONTRACT_FIRST as u64 + 2) * B::TICKS_PER_SEC) {
            w.tick();
        }
        assert_eq!(w.contracts.len(), 1, "first contract posted");
        let c = w.contracts[0].clone();
        let (cash, pop) = (w.cash, w.towns[c.town].pop);
        credit(&mut w, c.town, c.cargo, c.amount);
        for _ in 0..B::TICKS_PER_SEC {
            w.tick();
        }
        assert_eq!(w.contracts[0].state, ContractState::Done);
        assert!(w.cash > cash + c.cash_reward() * 0.9, "paid out");
        assert!(
            w.towns[c.town].pop > pop * (1.0 + B::CONTRACT_GROWTH * 0.9),
            "town boomed"
        );
        assert!(w.score() >= w.prosperity() + c.score_reward() - 1e-6);
        // The next one is left to expire.
        for _ in
            0..((B::CONTRACT_EVERY + B::CONTRACT_MINUTES * 60.0 + 5.0) as u64 * B::TICKS_PER_SEC)
        {
            w.tick();
        }
        assert!(
            w.contracts.iter().any(|c| c.state == ContractState::Failed),
            "a late contract fails"
        );
    }
}
