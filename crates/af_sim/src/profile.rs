//! Wall-clock cost of each `SimSet`, for performance work.
//!
//! [`add_sim_profiler`] puts a marker system on every boundary of the
//! chained `SimSet`s in `FixedUpdate` (before `Prepare`, between each pair,
//! after `Cleanup`). Each marker charges the time since the previous one to
//! the set it closes, so a set's figure includes its command flushes. The
//! markers only touch [`SimProfile`], so they never wait on sim systems
//! beyond the set ordering itself.
//!
//! ```ignore
//! let mut app = headless_app(data, setup, 30);
//! add_sim_profiler(&mut app);
//! run_headless(&mut app, ticks, 0, |_| {});
//! println!("{}", app.world().resource::<SimProfile>().report(None));
//! ```

use std::fmt::Write as _;
use std::time::Duration;

use af_core::prelude::*;
use bevy::platform::time::Instant;
use bevy::prelude::*;

/// The chained simulation sets, in schedule order.
pub const SIM_SETS: [SimSet; 12] = [
    SimSet::Prepare,
    SimSet::Ai,
    SimSet::Commands,
    SimSet::Orders,
    SimSet::Economy,
    SimSet::Pathing,
    SimSet::Movement,
    SimSet::Combat,
    SimSet::Projectiles,
    SimSet::Damage,
    SimSet::Vision,
    SimSet::Cleanup,
];

/// Per-tick time of every `SimSet` (microseconds), one row per sim tick.
#[derive(Resource, Debug, Default)]
pub struct SimProfile {
    /// `ticks[t][s]`: time of `SIM_SETS[s]` in profiled tick `t`.
    pub ticks: Vec<[u32; SIM_SETS.len()]>,
    open: Option<Instant>,
}

/// Summary of one set over a range of ticks (microseconds).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SetStats {
    pub mean: f64,
    pub p99: u32,
    pub max: u32,
    /// Profiled tick (index into `SimProfile::ticks`) of the maximum.
    pub max_tick: usize,
}

impl SimProfile {
    /// Mean, 99th percentile and maximum (and when it was) of set `s` over
    /// `range` of the profiled ticks (all of them for `None`).
    pub fn stats(&self, s: usize, range: Option<std::ops::Range<usize>>) -> SetStats {
        let first = range.as_ref().map_or(0, |r| r.start.min(self.ticks.len()));
        let rows = match range {
            Some(r) => &self.ticks[first..r.end.min(self.ticks.len())],
            None => &self.ticks[..],
        };
        if rows.is_empty() {
            return SetStats::default();
        }
        let max_tick = first + rows.iter().enumerate().max_by_key(|(i, t)| (t[s], std::cmp::Reverse(*i))).map_or(0, |(i, _)| i);
        let mut v: Vec<u32> = rows.iter().map(|t| t[s]).collect();
        v.sort_unstable();
        let mean = v.iter().map(|&x| f64::from(x)).sum::<f64>() / v.len() as f64;
        SetStats { mean, p99: v[(v.len() - 1) * 99 / 100], max: v[v.len() - 1], max_tick }
    }

    /// Sum over every set of the mean time per tick, over `range`.
    pub fn mean_tick(&self, range: Option<std::ops::Range<usize>>) -> Duration {
        let us: f64 = (0..SIM_SETS.len()).map(|s| self.stats(s, range.clone()).mean).sum();
        Duration::from_secs_f64(us / 1e6)
    }

    /// A table of every set's mean / p99 / max in milliseconds over `range`,
    /// and the profiled tick of each maximum.
    pub fn report(&self, range: Option<std::ops::Range<usize>>) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "{:<12} {:>9} {:>9} {:>9} {:>8}", "set", "mean ms", "p99 ms", "max ms", "at tick");
        for (i, set) in SIM_SETS.iter().enumerate() {
            let st = self.stats(i, range.clone());
            let _ = writeln!(s, "{:<12} {:>9.3} {:>9.3} {:>9.3} {:>8}", format!("{set:?}"), st.mean / 1e3, f64::from(st.p99) / 1e3, f64::from(st.max) / 1e3, st.max_tick);
        }
        let _ = writeln!(s, "{:<12} {:>9.3}", "tick", self.mean_tick(range).as_secs_f64() * 1e3);
        s
    }
}

/// Measure every `SimSet` of `app`'s `FixedUpdate` into [`SimProfile`].
pub fn add_sim_profiler(app: &mut App) {
    // The sets' own run condition, so ticks without a match are not counted.
    let running = || in_state(GameState::Playing).and_then(resource_exists::<Players>).and_then(resource_exists::<Nav>);
    app.init_resource::<SimProfile>();
    app.add_systems(FixedUpdate, open_tick.before(SIM_SETS[0]).run_if(running()));
    for (i, set) in SIM_SETS.iter().enumerate() {
        let close = close_set(i).after(*set).run_if(running());
        match SIM_SETS.get(i + 1) {
            Some(next) => app.add_systems(FixedUpdate, close.before(*next)),
            None => app.add_systems(FixedUpdate, close),
        };
    }
}

fn open_tick(mut profile: ResMut<SimProfile>) {
    profile.open = Some(Instant::now());
}

/// The marker closing set `index`: charges it the time since the last marker.
fn close_set(index: usize) -> impl FnMut(ResMut<SimProfile>) {
    move |mut profile: ResMut<SimProfile>| {
        let now = Instant::now();
        let Some(open) = profile.open.replace(now) else { return };
        let us = now.duration_since(open).as_micros().min(u128::from(u32::MAX)) as u32;
        if index == 0 {
            profile.ticks.push([0; SIM_SETS.len()]);
        }
        if let Some(row) = profile.ticks.last_mut() {
            row[index] = us;
        }
    }
}
