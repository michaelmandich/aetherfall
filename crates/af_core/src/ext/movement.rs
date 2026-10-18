//! Shared items added by af_movement (other crates may use them).
//! Only af_movement edits this file. Never modify items in other af_core files.

#[allow(unused_imports)]
use crate::prelude::*;
#[allow(unused_imports)]
use bevy::prelude::*;

/// What the collision solver's final word on pairs did since the match
/// started: the bookkeeping behind the no-overlap rule, for tests, tuning
/// and the headless report. Written by af_movement at the end of every
/// tick's collision solve (cheap counters, no per-unit state); nothing in
/// the simulation reads it.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct CollisionStats {
    /// Ticks solved.
    pub ticks: u64,
    /// Unit-ticks a unit was held back toward where it started the tick
    /// (wholly or part of the way), so that no pair ended deeper than it
    /// began: the crowd stood still there.
    pub held: u64,
    /// Pairs the final word parted, pair-ticks (pushed apart, a turn undone,
    /// a unit set down nearby or sent back, eased apart part of the way).
    pub parted: u64,
    /// Unit-ticks a unit was set down on a free spot rather than pushed: the
    /// last resort, a visible jump.
    pub relocated: u64,
    /// Farthest the final word moved one unit in one tick (metres, from
    /// where the solver passes left it; units it only took back toward
    /// where they began the tick, held or sent back, aside), and when (sim
    /// seconds).
    pub max_correction: f32,
    pub max_correction_at: f32,
    /// Farthest one relocation set a unit down (metres).
    pub max_relocation: f32,
    /// Pair-ticks left deeper than the final tolerance (every one no deeper
    /// than the pair began the tick).
    pub left: u64,
    /// Unit-ticks of units that began the tick overlapping another deeper
    /// than the final tolerance (trained onto it, a foundation laid on
    /// them): how long such overlaps last.
    pub overlapped: u64,
    /// Unit-ticks a ground unit ended farther than [`Self::JUMP`] from where
    /// it began the tick (the fastest walker covers about 0.3 m), by what
    /// moved it last: the final word on pairs (but for taking it back toward
    /// where it began), the obstacles (pushed or set down out of a new
    /// foundation), or the solver passes alone.
    pub jumps_final: u64,
    pub jumps_eject: u64,
    pub jumps_passes: u64,
    /// Farthest a unit the final word on pairs moved (as for
    /// [`Self::max_correction`]) ended from where it began its tick
    /// (metres): the jump the player sees, walk and shoves included.
    pub max_final_move: f32,
}

impl CollisionStats {
    /// A one-tick move past this is a visible jump (metres).
    pub const JUMP: f32 = 0.4;

    /// One line for reports.
    pub fn describe(&self) -> String {
        format!(
            "{} ticks: {} unit-ticks held back, {} parted, {} set down (farthest {:.2} m), largest correction {:.2} m at t={:.1}s, largest one-tick move of a unit it moved {:.2} m, {} pair-ticks left too deep, {} unit-ticks overlapped at tick start; jumps >{} m by the final word {}, out of foundations {}, by the passes {}",
            self.ticks,
            self.held,
            self.parted,
            self.relocated,
            self.max_relocation,
            self.max_correction,
            self.max_correction_at,
            self.max_final_move,
            self.left,
            self.overlapped,
            Self::JUMP,
            self.jumps_final,
            self.jumps_eject,
            self.jumps_passes
        )
    }

    /// Add another match's counts (the maxima are kept).
    pub fn add(&mut self, other: &CollisionStats) {
        self.ticks += other.ticks;
        self.held += other.held;
        self.parted += other.parted;
        self.relocated += other.relocated;
        self.left += other.left;
        self.overlapped += other.overlapped;
        self.jumps_final += other.jumps_final;
        self.jumps_eject += other.jumps_eject;
        self.jumps_passes += other.jumps_passes;
        self.max_relocation = self.max_relocation.max(other.max_relocation);
        self.max_final_move = self.max_final_move.max(other.max_final_move);
        if other.max_correction > self.max_correction {
            (self.max_correction, self.max_correction_at) = (other.max_correction, other.max_correction_at);
        }
    }
}
