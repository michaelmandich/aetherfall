//! Deterministic simulation RNG (SplitMix64). Use `SimRng` for anything in
//! the sim (scatter, AI choices) so seeded matches replay identically.

use bevy::prelude::*;

#[derive(Resource, Clone, Debug)]
pub struct SimRng {
    state: u64,
}

impl Default for SimRng {
    fn default() -> Self {
        SimRng::new(0x5EED)
    }
}

impl SimRng {
    pub fn new(seed: u64) -> SimRng {
        SimRng { state: seed ^ 0x9E37_79B9_7F4A_7C15 }
    }
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in [0, 1).
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }
    /// Uniform integer in [0, n).
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { (self.next_u64() % n as u64) as u32 }
    }
    pub fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }
    /// Random point in a disc.
    pub fn in_disc(&mut self, radius: f32) -> Vec2 {
        let a = self.range(0.0, std::f32::consts::TAU);
        let r = radius * self.f32().sqrt();
        Vec2::new(a.cos(), a.sin()) * r
    }
    /// Independent child generator (e.g. one per map feature).
    pub fn fork(&mut self) -> SimRng {
        SimRng::new(self.next_u64())
    }
}
