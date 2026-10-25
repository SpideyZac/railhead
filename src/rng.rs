//! Tiny deterministic RNG (SplitMix64). We roll our own so that a seed
//! produces the same map forever, regardless of crate versions.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform float in [0, 1).
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform integer in [lo, hi). Returns lo if the range is empty.
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() % (hi - lo) as u64) as i32
    }

    pub fn chance(&mut self, p: f64) -> bool {
        self.f64() < p
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            Some(&items[self.range(0, items.len() as i32) as usize])
        }
    }

    /// Fisher–Yates shuffle.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.range(0, i as i32 + 1) as usize;
            items.swap(i, j);
        }
    }
}

/// Stateless hash of a coordinate + seed, handy for noise and sparse decorations.
pub fn hash2(seed: u64, x: i32, y: i32) -> u64 {
    let mut r =
        Rng::new(seed ^ ((x as u64) << 32) ^ (y as u32 as u64).wrapping_mul(0x632B_E59B_D9B4_E019));
    r.next_u64()
}

/// Parse a user-typed seed: numbers are used directly, words are hashed.
pub fn parse_seed(s: &str) -> u64 {
    let s = s.trim();
    if let Ok(n) = s.parse::<u64>() {
        return n;
    }
    // FNV-1a so "banana" is always the same map.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.to_lowercase().bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h % 1_000_000_000
}
