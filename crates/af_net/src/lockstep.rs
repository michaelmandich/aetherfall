//! Server-paced lockstep, the pure part: which turns a client holds, which
//! tick it may run next, and how much the clock may advance this frame.
//!
//! The server runs the match clock. Every sim tick it broadcasts a
//! [`Turn`](crate::protocol::ServerMsg::Turn): the commands (from every
//! player) that execute in that tick, in one order for everyone. A client
//! runs tick `n` once it holds turn `n`, so every client plays the same
//! commands in the same ticks and the simulation stays identical. A client
//! that lags behind just runs later and catches up at a higher speed; it
//! never holds the others up.
//!
//! [`TurnBuffer`] keeps the turns received and not yet run. [`Pacer`] turns
//! the frame's real time into the virtual time the fixed clock may consume:
//! at most as far as the turns held allow (a stall when none are), faster
//! when a backlog has built up, and as fast as the machine can go when
//! rejoining a match from far behind.

use std::collections::BTreeMap;
use std::time::Duration;

use af_core::prelude::*;

/// Turns received and not yet executed.
#[derive(Debug, Default)]
pub struct TurnBuffer {
    turns: BTreeMap<u64, Vec<PlayerCommand>>,
    /// The next tick to execute (the first tick is 1).
    next: u64,
    /// The highest tick a turn was received for.
    highest: u64,
}

impl TurnBuffer {
    pub fn new() -> TurnBuffer {
        TurnBuffer { turns: BTreeMap::new(), next: 1, highest: 0 }
    }

    pub fn insert(&mut self, tick: u64, commands: Vec<PlayerCommand>) {
        if tick < self.next {
            return; // already run (a resend after a resume)
        }
        self.highest = self.highest.max(tick);
        self.turns.insert(tick, commands);
    }

    /// The next tick to execute.
    pub fn next_tick(&self) -> u64 {
        self.next
    }

    /// The highest tick a turn arrived for.
    pub fn highest(&self) -> u64 {
        self.highest
    }

    /// How many ticks from `next` on can run now: the length of the run of
    /// turns held with no gap.
    pub fn ready(&self) -> u64 {
        let mut t = self.next;
        while self.turns.contains_key(&t) {
            t += 1;
        }
        t - self.next
    }

    /// Take the next tick's commands (only when [`ready`](Self::ready) > 0).
    pub fn take_next(&mut self) -> Option<Vec<PlayerCommand>> {
        let cmds = self.turns.remove(&self.next)?;
        self.next += 1;
        Some(cmds)
    }
}

/// Lockstep pacing thresholds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pacer {
    /// Turns held beyond which the clock runs at `catch_up_speed`.
    pub catch_up_after: u64,
    pub catch_up_speed: f64,
    /// Turns held beyond which the clock runs as fast as it can (a resume).
    pub rush_after: u64,
    /// Most ticks run per frame while rushing.
    pub rush_ticks_per_frame: u64,
}

impl Default for Pacer {
    fn default() -> Self {
        Pacer { catch_up_after: 6, catch_up_speed: 1.5, rush_after: 90, rush_ticks_per_frame: 30 }
    }
}

impl Pacer {
    /// Virtual time the fixed clock may be fed this frame: `real_dt` (times
    /// the catch-up speed when `ready` turns are a backlog), capped so the
    /// fixed clock, holding `overstep` already, runs at most `ready` ticks of
    /// `timestep`. Zero stalls the sim until the next turn arrives.
    pub fn virtual_delta(&self, real_dt: Duration, overstep: Duration, timestep: Duration, ready: u64) -> Duration {
        if ready == 0 {
            return Duration::ZERO;
        }
        let cap = timestep.saturating_mul(ready as u32).saturating_sub(overstep);
        let wanted = if ready > self.rush_after {
            timestep.saturating_mul(self.rush_ticks_per_frame as u32)
        } else if ready > self.catch_up_after {
            real_dt.mul_f64(self.catch_up_speed)
        } else {
            real_dt
        };
        wanted.min(cap)
    }

    pub fn catching_up(&self, ready: u64) -> bool {
        ready > self.catch_up_after
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(p: u8) -> PlayerCommand {
        PlayerCommand::new(PlayerId(p), vec![], CommandKind::Stop)
    }

    #[test]
    fn turns_run_in_order_without_gaps() {
        let mut b = TurnBuffer::new();
        assert_eq!(b.ready(), 0);
        b.insert(2, vec![cmd(1)]);
        assert_eq!(b.ready(), 0, "tick 1 is missing");
        b.insert(1, vec![]);
        assert_eq!(b.ready(), 2);
        assert_eq!(b.take_next().unwrap(), vec![]);
        assert_eq!(b.take_next().unwrap(), vec![cmd(1)]);
        assert!(b.take_next().is_none());
        assert_eq!(b.next_tick(), 3);
        b.insert(1, vec![cmd(9)]);
        assert_eq!(b.ready(), 0, "a turn already run is ignored");
        assert_eq!(b.highest(), 2);
    }

    #[test]
    fn pacing_stalls_caps_and_catches_up() {
        let p = Pacer::default();
        let dt = Duration::from_secs_f64(1.0 / 30.0);
        let frame = Duration::from_millis(16);
        assert_eq!(p.virtual_delta(frame, Duration::ZERO, dt, 0), Duration::ZERO, "no turn: stall");
        assert_eq!(p.virtual_delta(frame, Duration::ZERO, dt, 3), frame, "turns in hand: real time");
        // One turn, 30 ms already accumulated: only 3.3 ms more may be consumed.
        assert_eq!(p.virtual_delta(frame, Duration::from_millis(30), dt, 1), dt - Duration::from_millis(30));
        assert_eq!(p.virtual_delta(frame, Duration::ZERO, dt, 10), frame.mul_f64(1.5), "backlog: faster");
        assert!(p.catching_up(10) && !p.catching_up(2));
        // Far behind: a whole second of ticks per frame, but never more than are held.
        assert_eq!(p.virtual_delta(frame, Duration::ZERO, dt, 1000), dt * 30);
        assert_eq!(p.virtual_delta(frame, Duration::ZERO, dt, 91), dt * 30);
        assert_eq!(p.virtual_delta(frame, Duration::ZERO, dt, 95).as_nanos() / dt.as_nanos(), 30);
    }
}
