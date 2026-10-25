//! Small shared value types: positions, directions, cargo, inventories.

use std::ops::{Index, IndexMut};

use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize, PartialOrd, Ord,
)]
pub struct Pos {
    pub x: i32,
    pub y: i32,
}

impl Pos {
    pub const fn new(x: i32, y: i32) -> Self {
        Pos { x, y }
    }

    pub fn step(self, d: Dir) -> Pos {
        let (dx, dy) = d.delta();
        Pos::new(self.x + dx, self.y + dy)
    }

    pub fn manhattan(self, o: Pos) -> i32 {
        (self.x - o.x).abs() + (self.y - o.y).abs()
    }

    pub fn dist2(self, o: Pos) -> i32 {
        let dx = self.x - o.x;
        let dy = self.y - o.y;
        dx * dx + dy * dy
    }

    /// Direction from self to an orthogonal neighbor.
    pub fn dir_to(self, o: Pos) -> Option<Dir> {
        Dir::ALL.into_iter().find(|&d| self.step(d) == o)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Dir {
    N,
    E,
    S,
    W,
}

impl Dir {
    pub const ALL: [Dir; 4] = [Dir::N, Dir::E, Dir::S, Dir::W];

    pub fn delta(self) -> (i32, i32) {
        match self {
            Dir::N => (0, -1),
            Dir::E => (1, 0),
            Dir::S => (0, 1),
            Dir::W => (-1, 0),
        }
    }

    pub fn opposite(self) -> Dir {
        match self {
            Dir::N => Dir::S,
            Dir::E => Dir::W,
            Dir::S => Dir::N,
            Dir::W => Dir::E,
        }
    }

    pub fn bit(self) -> u8 {
        1 << (self as u8)
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// Everything that can sit in a stockpile or ride in a vehicle.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize, PartialOrd, Ord)]
pub enum Cargo {
    Grain,
    Logs,
    Coal,
    Ore,
    Oil,
    Food,
    Lumber,
    Steel,
    Goods,
    Fuel,
    Passengers,
}

pub const NUM_CARGO: usize = 11;

impl Cargo {
    pub const ALL: [Cargo; NUM_CARGO] = [
        Cargo::Grain,
        Cargo::Logs,
        Cargo::Coal,
        Cargo::Ore,
        Cargo::Oil,
        Cargo::Food,
        Cargo::Lumber,
        Cargo::Steel,
        Cargo::Goods,
        Cargo::Fuel,
        Cargo::Passengers,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Cargo::Grain => "Grain",
            Cargo::Logs => "Logs",
            Cargo::Coal => "Coal",
            Cargo::Ore => "Iron ore",
            Cargo::Oil => "Oil",
            Cargo::Food => "Food",
            Cargo::Lumber => "Lumber",
            Cargo::Steel => "Steel",
            Cargo::Goods => "Goods",
            Cargo::Fuel => "Fuel",
            Cargo::Passengers => "Passengers",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Cargo::Grain => "GRAN",
            Cargo::Logs => "LOGS",
            Cargo::Coal => "COAL",
            Cargo::Ore => "ORE",
            Cargo::Oil => "OIL",
            Cargo::Food => "FOOD",
            Cargo::Lumber => "LUMB",
            Cargo::Steel => "STEL",
            Cargo::Goods => "GOOD",
            Cargo::Fuel => "FUEL",
            Cargo::Passengers => "PASS",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

/// A set of cargo kinds, used by schedule stops ("load coal+ore").
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct CargoMask(pub u16);

impl CargoMask {
    pub const ALL: CargoMask = CargoMask((1 << NUM_CARGO) - 1);
    pub const NONE: CargoMask = CargoMask(0);

    pub fn single(c: Cargo) -> Self {
        CargoMask(c.bit())
    }

    pub fn has(self, c: Cargo) -> bool {
        self.0 & c.bit() != 0
    }

    pub fn with(self, c: Cargo) -> Self {
        CargoMask(self.0 | c.bit())
    }

    pub fn label(self) -> String {
        match self {
            CargoMask::ALL => "everything".into(),
            _ => Cargo::ALL
                .iter()
                .filter(|c| self.has(**c))
                .map(|c| c.name())
                .collect::<Vec<_>>()
                .join("+"),
        }
    }

    /// The presets the schedule editor cycles through.
    pub fn presets() -> Vec<CargoMask> {
        let mut v = vec![CargoMask::ALL];
        v.extend(Cargo::ALL.iter().map(|c| CargoMask::single(*c)));
        v
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Inv(pub [f64; NUM_CARGO]);

impl Index<Cargo> for Inv {
    type Output = f64;

    fn index(&self, c: Cargo) -> &f64 {
        &self.0[c as usize]
    }
}
impl IndexMut<Cargo> for Inv {
    fn index_mut(&mut self, c: Cargo) -> &mut f64 {
        &mut self.0[c as usize]
    }
}

impl Inv {
    pub fn total(&self) -> f64 {
        self.0.iter().sum()
    }

    pub fn total_mask(&self, m: CargoMask) -> f64 {
        Cargo::ALL
            .iter()
            .filter(|c| m.has(**c))
            .map(|c| self[*c])
            .sum()
    }

    pub fn take(&mut self, c: Cargo, amt: f64) -> f64 {
        let got = amt.min(self[c]).max(0.0);
        self[c] -= got;
        got
    }
}

/// Where and when the units in one inventory slot were loaded (weighted
/// averages), so a delivery can be paid by distance and speed.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Lot {
    pub x: f64,
    pub y: f64,
    /// Game time the cargo was first loaded.
    pub t: f64,
}

impl Lot {
    pub fn at(p: Pos, t: f64) -> Lot {
        Lot {
            x: p.x as f64,
            y: p.y as f64,
            t,
        }
    }

    pub fn dist(&self, p: Pos) -> f64 {
        ((self.x - p.x as f64).powi(2) + (self.y - p.y as f64).powi(2)).sqrt()
    }
}

/// One `Lot` per cargo kind, travelling alongside an `Inv`.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Origins(pub [Lot; NUM_CARGO]);

impl Origins {
    pub fn get(&self, c: Cargo) -> Lot {
        self.0[c as usize]
    }

    /// Blend `amt` units from `lot` into a slot that already holds `have` units.
    pub fn mix(&mut self, c: Cargo, have: f64, amt: f64, lot: Lot) {
        let total = have + amt;
        if total <= 1e-9 {
            return;
        }
        let f = amt / total;
        let l = &mut self.0[c as usize];
        l.x += (lot.x - l.x) * f;
        l.y += (lot.y - l.y) * f;
        l.t += (lot.t - l.t) * f;
    }
}
