//! Personal records: the best Charter score and the most Franchise reviews
//! passed on every map you've played (by seed and size), and how many medals
//! you've won. Saved as `records.json` in the data folder; a missing or
//! broken file just means no records yet.

use std::{collections::BTreeMap, path::Path};

use serde::{Deserialize, Serialize};

use crate::map::MapSize;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Records {
    /// Best result per map, keyed by `key(seed, size)`.
    #[serde(default)]
    pub best: BTreeMap<String, Best>,
    /// Charters played to the end.
    #[serde(default)]
    pub charters: u32,
    /// Medals won: bronze, silver, gold (a gold Charter counts once, as gold).
    #[serde(default)]
    pub medals: [u32; 3],
    /// Most Board reviews passed in a Franchise, per map (same keys as `best`).
    #[serde(default)]
    pub franchise: BTreeMap<String, u32>,
    /// Franchises played until the Board took them away.
    #[serde(default)]
    pub franchises: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Best {
    pub score: f64,
    /// 0 none, 1 bronze, 2 silver, 3 gold.
    pub medal: usize,
}

/// How a finished Charter compares with the records.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    /// The best score on this map before this game, if it had been played.
    pub previous: Option<f64>,
    pub new_best: bool,
}

pub fn key(seed: u64, size: MapSize) -> String {
    format!("{seed}/{}", size.name().to_lowercase())
}

impl Records {
    pub fn load(path: &Path) -> Records {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }

    pub fn get(&self, seed: u64, size: MapSize) -> Option<&Best> {
        self.best.get(&key(seed, size))
    }

    /// Add a finished Charter.
    pub fn add(&mut self, seed: u64, size: MapSize, score: f64, medal: usize) -> Outcome {
        self.charters += 1;
        if medal > 0 {
            self.medals[medal - 1] += 1;
        }
        let k = key(seed, size);
        let previous = self.best.get(&k).map(|b| b.score);
        let new_best = previous.is_none_or(|p| score > p);
        if new_best {
            self.best.insert(k, Best { score, medal });
        }
        Outcome { previous, new_best }
    }

    pub fn franchise_best(&self, seed: u64, size: MapSize) -> Option<u32> {
        self.franchise.get(&key(seed, size)).copied()
    }

    /// Add a lost Franchise; `previous` and the comparison are in reviews passed.
    pub fn add_franchise(&mut self, seed: u64, size: MapSize, passed: u32) -> Outcome {
        self.franchises += 1;
        let k = key(seed, size);
        let previous = self.franchise.get(&k).copied();
        let new_best = previous.is_none_or(|p| passed > p);
        if new_best {
            self.franchise.insert(k, passed);
        }
        Outcome {
            previous: previous.map(f64::from),
            new_best,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_keep_the_best_and_count_medals() {
        let mut r = Records::default();
        let o = r.add(42, MapSize::Normal, 5000.0, 2);
        assert_eq!(
            o,
            Outcome {
                previous: None,
                new_best: true
            }
        );
        let o = r.add(42, MapSize::Normal, 4000.0, 1);
        assert_eq!(
            o,
            Outcome {
                previous: Some(5000.0),
                new_best: false
            }
        );
        assert_eq!(
            r.get(42, MapSize::Normal).unwrap().medal,
            2,
            "the best game's medal stays"
        );
        assert!(
            r.get(42, MapSize::Large).is_none(),
            "sizes are different maps"
        );
        assert_eq!((r.charters, r.medals), (2, [1, 1, 0]));
        let o = r.add_franchise(42, MapSize::Normal, 4);
        assert_eq!(
            o,
            Outcome {
                previous: None,
                new_best: true
            }
        );
        assert_eq!(
            r.add_franchise(42, MapSize::Normal, 3),
            Outcome {
                previous: Some(4.0),
                new_best: false
            }
        );
        assert_eq!(
            (
                r.franchise_best(42, MapSize::Normal),
                r.franchises,
                r.charters
            ),
            (Some(4), 2, 2),
            "franchises are kept apart"
        );
        let path = std::env::temp_dir().join("rh_records_test.json");
        r.save(&path);
        assert_eq!(Records::load(&path).best.len(), 1);
        std::fs::write(&path, "nonsense").unwrap();
        assert_eq!(Records::load(&path).charters, 0, "broken file → no records");
        let _ = std::fs::remove_file(path);
    }
}
