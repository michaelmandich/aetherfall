//! Ratings. Everyone starts at [`START`]; a result moves ratings by the
//! classic Elo rule with [`K`]: beating a stronger player pays more,
//! losing to a weaker one costs more, and the sum of all changes is zero
//! when the teams are the same size.
//!
//! With more than two teams (1v1v1) the winner is rated against each
//! losing team in turn, so a free-for-all win against two equals pays
//! about twice a duel. Teams are rated by their average.

/// A new account's rating.
pub const START: i32 = 1600;
/// The most a single result can move a rating (per opponent team).
pub const K: f64 = 32.0;

/// The chance of `a` beating `b`, by rating.
pub fn expected(a: i32, b: i32) -> f64 {
    1.0 / (1.0 + 10f64.powf((b - a) as f64 / 400.0))
}

/// Rating change of every player (same order as `players`: `(team,
/// rating)`) when `winning_team` won; all zero without a winner.
pub fn changes(players: &[(u8, i32)], winning_team: Option<u8>) -> Vec<i32> {
    let Some(winner) = winning_team else { return vec![0; players.len()] };
    let mut teams: Vec<u8> = players.iter().map(|(t, _)| *t).collect();
    teams.sort_unstable();
    teams.dedup();
    if !teams.contains(&winner) || teams.len() < 2 {
        return vec![0; players.len()];
    }
    let average = |team: u8| -> i32 {
        let ratings: Vec<i32> = players.iter().filter(|(t, _)| *t == team).map(|(_, r)| *r).collect();
        (ratings.iter().sum::<i32>() as f64 / ratings.len() as f64).round() as i32
    };
    let winning_average = average(winner);
    let losses: Vec<(u8, i32)> = teams
        .iter()
        .filter(|t| **t != winner)
        .map(|t| (*t, (K * (1.0 - expected(winning_average, average(*t)))).round() as i32))
        .collect();
    players
        .iter()
        .map(|(team, _)| if *team == winner { losses.iter().map(|(_, d)| *d).sum() } else { -losses.iter().find(|(t, _)| t == team).map_or(0, |(_, d)| *d) })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_duel_moves_sixteen_each_way() {
        assert_eq!(changes(&[(0, 1600), (1, 1600)], Some(0)), vec![16, -16]);
        assert_eq!(changes(&[(0, 1600), (1, 1600)], Some(1)), vec![-16, 16]);
        assert_eq!(changes(&[(0, 1600), (1, 1600)], None), vec![0, 0]);
    }

    #[test]
    fn upsets_pay_more_than_expected_wins() {
        // A 1400 beating a 1800: the underdog gains a lot, the favourite loses as much.
        let upset = changes(&[(0, 1400), (1, 1800)], Some(0));
        assert_eq!(upset, vec![29, -29]);
        let expected_win = changes(&[(0, 1800), (1, 1400)], Some(0));
        assert_eq!(expected_win, vec![3, -3]);
        assert!(expected(1800, 1400) > 0.9);
    }

    #[test]
    fn free_for_all_rates_the_winner_against_each_loser() {
        let c = changes(&[(0, 1600), (1, 1600), (2, 1600)], Some(1));
        assert_eq!(c, vec![-16, 32, -16]);
        assert_eq!(c.iter().sum::<i32>(), 0, "zero-sum");
    }

    #[test]
    fn teams_are_rated_by_their_average() {
        let c = changes(&[(0, 1700), (0, 1500), (1, 1600), (1, 1600)], Some(1));
        assert_eq!(c, vec![-16, -16, 16, 16]);
        let c = changes(&[(0, 1800), (0, 1800), (1, 1500), (1, 1500)], Some(1));
        assert_eq!(c[2], c[3]);
        assert!(c[2] > 25, "{c:?}");
        assert_eq!(c.iter().sum::<i32>(), 0);
    }

    #[test]
    fn a_winner_that_played_nobody_changes_nothing() {
        assert_eq!(changes(&[(0, 1600), (0, 1600)], Some(0)), vec![0, 0]);
        assert_eq!(changes(&[(0, 1600), (1, 1600)], Some(5)), vec![0, 0]);
    }
}
