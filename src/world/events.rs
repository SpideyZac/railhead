//! News: every few minutes something happens in the region and *The Railhead
//! Gazette* reports it. Most news is an opportunity with a deadline (a town
//! pays double for a while, a farm has a bumper harvest), some is a nuisance
//! (a strike), and a little is a gift. News prefers places on your network,
//! so it lands where you can act on it. Everything is drawn from `world.rng`,
//! so a seed replays the same news.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum NewsKind {
    /// Deliveries to a town pay `NEWS_BOOM_PAY` times as much.
    Boom,
    /// A primary industry produces `NEWS_HARVEST_RATE` times as much.
    Harvest,
    /// A town makes extra passengers, and passengers to it pay extra.
    Festival,
    /// A primary industry's production level goes up for good.
    NewSeam,
    /// A secondary industry stops working for a while.
    Strike,
    /// Cash, scaled by your Prosperity.
    Investor,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct News {
    pub kind: NewsKind,
    /// The town's or industry's site.
    pub site: u32,
    pub start: f64,
    /// When the effect ends (= start for instant news).
    pub end: f64,
    pub headline: String,
    pub body: String,
}

impl News {
    pub fn active(&self, now: f64) -> bool {
        now >= self.start && now < self.end
    }
}

/// How much more an industry makes right now (0 while on strike).
pub fn production_mult(w: &World, site: u32) -> f64 {
    let mut m = 1.0;
    for n in w.news.iter().filter(|n| n.site == site && n.active(w.time)) {
        match n.kind {
            NewsKind::Harvest => m *= B::NEWS_HARVEST_RATE,
            NewsKind::Strike => m = 0.0,
            _ => {}
        }
    }
    m
}

/// How much more a delivery of `c` to `site` pays right now.
pub fn pay_mult(w: &World, site: u32, c: Cargo) -> f64 {
    let mut m = 1.0;
    for n in w.news.iter().filter(|n| n.site == site && n.active(w.time)) {
        match n.kind {
            NewsKind::Boom => m *= B::NEWS_BOOM_PAY,
            NewsKind::Festival if c == Cargo::Passengers => m *= B::NEWS_FESTIVAL_PAY,
            _ => {}
        }
    }
    m
}

/// How many more passengers a town makes right now.
pub fn passenger_mult(w: &World, site: u32) -> f64 {
    let festival = w
        .news
        .iter()
        .any(|n| n.site == site && n.kind == NewsKind::Festival && n.active(w.time));
    if festival {
        B::NEWS_FESTIVAL_PASSENGERS
    } else {
        1.0
    }
}

/// Is a site on the network (a station covers it, or a truck stops there)?
pub fn connected(w: &World, site: u32) -> bool {
    let pos = w.sites[site as usize].pos;
    w.stations
        .iter()
        .any(|s| !s.removed && rail::in_catchment(s.pos, pos))
        || w.trucks.iter().any(|t| {
            t.carrier
                .schedule
                .stops
                .iter()
                .any(|s| s.target == Target::Site(site))
        })
}

/// Once per second: print the next edition when it's due.
pub fn tick_second(w: &mut World) {
    let due = if w.stats.last_news == 0.0 {
        B::NEWS_FIRST
    } else {
        w.stats.last_news + w.stats.news_gap
    };
    let time_to_matter = w.time_left().is_none_or(|left| left > B::NEWS_QUIET_AT_END);
    if w.time < due || !time_to_matter {
        return;
    }
    w.stats.last_news = w.time;
    w.stats.news_gap = B::NEWS_EVERY * (0.75 + 0.5 * w.rng.f64());
    publish(w);
    // the archive only needs recent editions
    if w.news.len() > 40 {
        w.news.drain(0..10);
    }
}

/// Choose a story, apply it and announce it.
fn publish(w: &mut World) {
    let weights: [(NewsKind, u32); 6] = [
        (NewsKind::Boom, 3),
        (NewsKind::Harvest, 3),
        (NewsKind::Festival, 2),
        (NewsKind::NewSeam, 2),
        (NewsKind::Strike, 1),
        (NewsKind::Investor, if w.sandbox() { 0 } else { 1 }),
    ];
    let total: u32 = weights.iter().map(|(_, n)| n).sum();
    let mut roll = w.rng.range(0, total as i32) as u32;
    let mut kind = NewsKind::Boom;
    for (k, n) in weights {
        if roll < n {
            kind = k;
            break;
        }
        roll -= n;
    }
    let busy: Vec<u32> = w
        .news
        .iter()
        .filter(|n| n.active(w.time))
        .map(|n| n.site)
        .collect();
    let towns: Vec<u32> = w
        .towns
        .iter()
        .map(|t| t.site)
        .filter(|s| !busy.contains(s))
        .collect();
    let industries = |primary: bool| -> Vec<u32> {
        w.sites
            .iter()
            .filter(|s| {
                matches!(s.kind, SiteKind::Industry(k) if k.is_primary() == primary)
                    && !busy.contains(&s.id)
            })
            .map(|s| s.id)
            .collect()
    };
    let pool: Vec<u32> = match kind {
        NewsKind::Boom | NewsKind::Festival | NewsKind::Investor => towns,
        NewsKind::Harvest | NewsKind::NewSeam => industries(true),
        NewsKind::Strike => industries(false),
    };
    // Prefer somewhere on the network, where the player can do something about it.
    let near: Vec<u32> = pool.iter().copied().filter(|s| connected(w, *s)).collect();
    let pool = if near.is_empty() { pool } else { near };
    // A strike only hits an industry that's actually working; no network, no strike.
    let pool: Vec<u32> = if kind == NewsKind::Strike {
        pool.into_iter().filter(|s| connected(w, *s)).collect()
    } else {
        pool
    };
    let Some(&site) = w.rng.pick(&pool) else {
        return;
    };

    let name = w.site_name(site);
    let pos = w.sites[site as usize].pos;
    let mins = |s: f64| format!("{:.0} minutes", s / 60.0);
    let (secs, headlines, body): (f64, &[&str], String) = match kind {
        NewsKind::Boom => (
            B::NEWS_BOOM_SECS,
            &[
                "SILVER STRIKE NEAR {name}!",
                "{name} WINS THE COUNTY FAIR",
                "RAILWAY FEVER GRIPS {name}",
                "{name} MAYOR PROMISES \"A NEW AGE\"",
            ],
            format!(
                "Everything delivered to {name} pays ×{:.0} for {}.",
                B::NEWS_BOOM_PAY,
                mins(B::NEWS_BOOM_SECS)
            ),
        ),
        NewsKind::Harvest => (
            B::NEWS_HARVEST_SECS,
            &[
                "BUMPER YEAR AT {name}",
                "{name} STRIKES IT RICH",
                "RECORD OUTPUT AT {name}",
            ],
            format!(
                "{name} makes ×{:.0} for {}. Haul it away before it goes to waste!",
                B::NEWS_HARVEST_RATE,
                mins(B::NEWS_HARVEST_SECS)
            ),
        ),
        NewsKind::Festival => (
            B::NEWS_FESTIVAL_SECS,
            &[
                "{name} HOLDS ITS GRAND FESTIVAL",
                "BRASS BANDS AND BUNTING IN {name}",
                "ALL ROADS LEAD TO {name}",
            ],
            format!(
                "{name} makes ×{:.0} passengers, and passengers to it pay ×{:.0}, for {}.",
                B::NEWS_FESTIVAL_PASSENGERS,
                B::NEWS_FESTIVAL_PAY,
                mins(B::NEWS_FESTIVAL_SECS)
            ),
        ),
        NewsKind::NewSeam => (
            0.0,
            &[
                "NEW SEAM FOUND AT {name}",
                "SURVEYORS DELIGHTED AT {name}",
                "{name} BREAKS NEW GROUND",
            ],
            {
                let s = &mut w.sites[site as usize];
                s.level = (s.level + B::NEWS_SEAM_LEVELS).min(B::LEVEL_MAX);
                format!("{name} now produces ×{:.2} for good.", s.level)
            },
        ),
        NewsKind::Strike => (
            B::NEWS_STRIKE_SECS,
            &[
                "WALKOUT AT {name}",
                "{name} WORKERS DOWN TOOLS",
                "TEA BREAK DISPUTE HALTS {name}",
            ],
            format!(
                "{name} makes nothing for {}. Its inputs pile up; don't send more than it can store.",
                mins(B::NEWS_STRIKE_SECS)
            ),
        ),
        NewsKind::Investor => (
            0.0,
            &[
                "INVESTOR ADMIRES {name} LINE",
                "LONDON MONEY ARRIVES IN {name}",
                "BANKER RIDES YOUR TRAINS, IS CHARMED",
            ],
            {
                let gift = (w.prosperity() * B::NEWS_INVESTOR_PER_POP).max(B::NEWS_INVESTOR_MIN);
                w.cash += gift;
                format!(
                    "A visiting financier was so taken with your railway that they left {} on the table.",
                    money(gift)
                )
            },
        ),
    };
    let pick = w.rng.range(0, headlines.len() as i32) as usize;
    let headline = headlines[pick].replace("{name}", &name.to_uppercase());
    w.news.push(News {
        kind,
        site,
        start: w.time,
        end: w.time + secs,
        headline: headline.clone(),
        body: body.clone(),
    });
    let level = if kind == NewsKind::Strike {
        AlertLevel::Warn
    } else {
        AlertLevel::Good
    };
    w.push_alert(
        level,
        format!("▤ {headline} — {body}"),
        Some(pos),
        Some(Fanfare::News),
    );
}

/// The news in effect right now.
pub fn current(w: &World) -> impl Iterator<Item = &News> {
    w.news.iter().filter(|n| n.active(w.time))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn news_arrives_and_does_what_it_says() {
        let mut w = World::new(11, Settings::default());
        // run to the first edition
        while w.news.is_empty() {
            w.tick();
            assert!(
                w.time < B::NEWS_FIRST + 2.0,
                "first news by {}",
                B::NEWS_FIRST
            );
        }
        assert!(w.alerts.iter().any(|a| a.fanfare == Some(Fanfare::News)));
        // a boom pays more, a harvest makes more, a strike makes nothing
        let town = w.towns[0].site;
        let farm = w
            .sites
            .iter()
            .find(|s| s.kind == SiteKind::Industry(IndustryKind::Farm))
            .unwrap()
            .id;
        let mill = w
            .sites
            .iter()
            .find(|s| s.kind == SiteKind::Industry(IndustryKind::Mill))
            .unwrap()
            .id;
        let t = w.time;
        let mut add = |kind, site| {
            w.news.push(News {
                kind,
                site,
                start: t,
                end: t + 60.0,
                headline: String::new(),
                body: String::new(),
            })
        };
        add(NewsKind::Boom, town);
        add(NewsKind::Harvest, farm);
        add(NewsKind::Strike, mill);
        assert_eq!(pay_mult(&w, town, Cargo::Food), B::NEWS_BOOM_PAY);
        assert_eq!(production_mult(&w, farm), B::NEWS_HARVEST_RATE);
        assert_eq!(production_mult(&w, mill), 0.0);
        w.time += 61.0;
        assert_eq!(pay_mult(&w, town, Cargo::Food), 1.0, "news wears off");
    }

    #[test]
    fn a_charter_gets_a_steady_paper() {
        let mut w = World::new(3, Settings::default());
        for _ in 0..(B::CHARTER_MINUTES * 60.0 * B::TICKS_PER_SEC as f64) as u64 {
            w.tick();
        }
        let n = w
            .alerts
            .iter()
            .filter(|a| a.fanfare == Some(Fanfare::News))
            .count();
        assert!((6..=16).contains(&n), "{n} editions in a Charter");
    }
}
