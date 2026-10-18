//! What the AI may still spend during one think.
//!
//! Commands are paid for by af_economy on the next tick, so within a think
//! the AI tracks its own running total. Purchases can be told to leave part
//! of the age-up reserve untouched.

use af_core::prelude::*;

/// Saving looks at most this far ahead (seconds): a resource that would take
/// longer to collect does not hold the others back any further.
pub const SAVE_HORIZON: f32 = 150.0;
/// Income counted on for saving, as a share of the measured income (the rest
/// is margin for spending and for the estimate lagging behind).
const INCOME_TRUST: f32 = 0.8;
/// Incomes below this (per second) count as none.
const MIN_INCOME: f32 = 0.05;

/// What to keep of each resource now so that `cost` is affordable when its
/// slowest resource is: the bottleneck (the resource that takes longest to
/// collect at `income` per second) is kept whole, and every other resource
/// only as much as its income will not have refilled by then. Resources
/// arriving early may be spent meanwhile; with no income every shortfall is
/// kept whole.
pub fn just_in_time_reserve(cost: &ResAmounts, have: &ResAmounts, income: &ResAmounts) -> ResAmounts {
    let trusted = |i: usize| (income.0[i] * INCOME_TRUST).max(0.0);
    let mut wait: f32 = 0.0;
    for i in 0..MAX_RESOURCES {
        let missing = cost.0[i] - have.0[i];
        if missing > 0.0 {
            let rate = trusted(i);
            wait = wait.max(if rate >= MIN_INCOME { missing / rate } else { f32::INFINITY });
        }
    }
    let wait = wait.min(SAVE_HORIZON);
    let mut keep = ResAmounts::ZERO;
    for i in 0..MAX_RESOURCES {
        let rate = if trusted(i) >= MIN_INCOME { trusted(i) } else { 0.0 };
        keep.0[i] = (cost.0[i] - rate * wait).clamp(0.0, cost.0[i].max(0.0));
    }
    keep
}

/// The running budget of one think.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Budget {
    /// Stockpile minus what this think already committed.
    pub have: ResAmounts,
    /// Saved for the next age-up.
    pub reserve: ResAmounts,
}

impl Budget {
    pub fn new(have: ResAmounts, reserve: ResAmounts) -> Budget {
        Budget { have, reserve }
    }

    /// Can `cost` be paid while keeping `reserve_share` (0..=1) of the reserve?
    pub fn affords(&self, cost: &ResAmounts, reserve_share: f32) -> bool {
        self.have
            .0
            .iter()
            .zip(cost.0.iter())
            .zip(self.reserve.0.iter())
            .all(|((have, need), keep)| *need <= 0.0 || have - keep * reserve_share + 1e-3 >= *need)
    }

    /// What is missing to pay `cost` while keeping `reserve_share` of the reserve.
    pub fn shortfall(&self, cost: &ResAmounts, reserve_share: f32) -> ResAmounts {
        let mut out = ResAmounts::ZERO;
        for (i, need) in cost.0.iter().enumerate() {
            if *need > 0.0 {
                out.0[i] = (need + self.reserve.0[i] * reserve_share - self.have.0[i]).max(0.0);
            }
        }
        out
    }

    pub fn spend(&mut self, cost: &ResAmounts) {
        self.have.sub_all(cost);
    }

    /// Stop setting aside `cost` (it was just paid for out of the reserve).
    pub fn release(&mut self, cost: &ResAmounts) {
        for (keep, paid) in self.reserve.0.iter_mut().zip(cost.0.iter()) {
            *keep = (*keep - paid).max(0.0);
        }
    }

    /// Spend `cost` if [`Budget::affords`] it.
    pub fn try_spend(&mut self, cost: &ResAmounts, reserve_share: f32) -> bool {
        let ok = self.affords(cost, reserve_share);
        if ok {
            self.spend(cost);
        }
        ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn amounts(v: &[(usize, f32)]) -> ResAmounts {
        let mut a = ResAmounts::ZERO;
        for (i, x) in v {
            a.0[*i] = *x;
        }
        a
    }

    #[test]
    fn reserve_share_is_respected() {
        let mut b = Budget::new(amounts(&[(0, 300.0), (1, 100.0)]), amounts(&[(0, 200.0)]));
        let food = amounts(&[(0, 150.0)]);
        let wood = amounts(&[(1, 100.0)]);
        assert!(b.affords(&food, 0.0));
        assert!(b.affords(&food, 0.5));
        assert!(!b.affords(&food, 1.0));
        // The reserve only guards the resources it holds.
        assert!(b.affords(&wood, 1.0));
        assert!(b.try_spend(&wood, 1.0));
        assert!(!b.try_spend(&wood, 0.0));
    }

    #[test]
    fn just_in_time_reserve_keeps_the_bottleneck_whole() {
        // 800 food and 200 copper; 200 food and 50 copper in hand.
        let cost = amounts(&[(0, 800.0), (3, 200.0)]);
        let have = amounts(&[(0, 200.0), (3, 50.0)]);
        // Food comes in at 5/s (4/s trusted): 150 s to go. Copper at 2.5/s
        // (2/s trusted) needs 75 s, so only 200 - 2 * 150 < 0 of it is kept.
        let income = amounts(&[(0, 5.0), (3, 2.5)]);
        let keep = just_in_time_reserve(&cost, &have, &income);
        assert!((keep.0[0] - 200.0).abs() < 1e-3, "food: all in hand is kept: {keep:?}");
        assert_eq!(keep.0[3], 0.0, "copper refills in time");
        // Slow copper (0.5/s, 0.4 trusted, 375 s to go): it is the bottleneck
        // now and none of it may be spent; food is kept only as far as it
        // will not refill within the saving horizon.
        let income = amounts(&[(0, 5.0), (3, 0.5)]);
        let keep = just_in_time_reserve(&cost, &have, &income);
        assert!(keep.0[3] >= have.0[3], "{keep:?}");
        assert!((keep.0[0] - (800.0 - 4.0 * SAVE_HORIZON)).abs() < 1e-3, "{keep:?}");
        // No income at all: everything is kept, as before saving got smarter.
        assert_eq!(just_in_time_reserve(&cost, &have, &ResAmounts::ZERO), cost);
        // Already affordable: everything is kept for the purchase.
        assert_eq!(just_in_time_reserve(&cost, &cost, &income), cost);
    }
}
