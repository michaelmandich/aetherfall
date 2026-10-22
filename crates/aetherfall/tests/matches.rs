//! Whole-game integration tests: real content, every sim crate, no window.
//!
//! Fast tests run by default. The long AI-vs-AI matches are `#[ignore]`d;
//! run them with `cargo test -p aetherfall --release -- --ignored --nocapture`.

use std::sync::Arc;

use aetherfall::*;
use af_core::prelude::*;
use bevy::prelude::*;

fn data() -> Arc<GameData> {
    af_core::testing::test_data()
}

fn ai_match(players: usize, factions: &[&str], seed: u64) -> App {
    let data = data();
    let opts = SkirmishOptions {
        seed,
        players,
        factions: factions.iter().map(|s| s.to_string()).collect(),
        human: false,
        ..Default::default()
    };
    let setup = skirmish_setup(&data, &opts).expect("setup");
    headless_app(data, setup, 30)
}

fn run_secs(app: &mut App, secs: f32) {
    let ticks = (secs * SIM_HZ as f32) as u64;
    run_headless(app, ticks, 0, |_| {});
}

/// Every faction starts with a complete main hall and its start units.
#[test]
fn every_faction_starts() {
    let data = data();
    for f in &data.factions {
        let mut app = ai_match(2, &[f.id.as_str(), f.id.as_str()], 3);
        run_secs(&mut app, 1.0);
        let players = app.world().resource::<Players>().clone();
        for p in &players.list {
            let halls = data
                .building_ids()
                .filter(|b| data.building(*b).has_tag(Tag::TownCenter))
                .map(|b| p.buildings_owned[b.index()])
                .sum::<u32>();
            assert!(halls >= 1, "{}: player {} has no complete main hall", f.id, p.id.0);
            let want: u32 = f.start.units.iter().map(|(_, n)| *n).sum();
            let have: u32 = p.units_owned.iter().sum();
            assert!(have >= want, "{}: player {} has {have} units, start lists {want}", f.id, p.id.0);
            assert!(p.pop_cap > 0, "{}: no population capacity at start", f.id);
        }
    }
}

/// The map options of a skirmish reach the match next to its setup and
/// the match runs; once the map is built from them, its published
/// features carry the options asked for.
#[test]
fn map_options_reach_the_match() {
    use af_core::ext::map::{MapFeatures, MapOptions, MapSize, RiverCount, SiteChoice, SiteKind, WaterLevel};
    let map = MapOptions { size: MapSize::Small, water: WaterLevel::Coast, rivers: RiverCount::Two, site: SiteChoice::Kind(SiteKind::Spire) };
    let opts = SkirmishOptions { seed: 5, players: 2, human: false, map, ..Default::default() };
    let mut app = skirmish_app(data(), &opts, 30).expect("setup");
    run_secs(&mut app, 1.0);
    assert_eq!(app.world().get_resource::<MapOptions>(), Some(&map));
    assert!(app.world().resource::<SimClock>().tick > 0, "the match runs");
    if let Some(features) = app.world().get_resource::<MapFeatures>() {
        let o = features.options;
        assert_eq!((o.size, o.water, o.rivers, o.site), (MapSize::Small, WaterLevel::Coast, 2, SiteKind::Spire));
    }
}

/// Two AIs build an economy in the first minutes: they gather, train
/// workers, construct buildings.
#[test]
fn ai_builds_an_economy() {
    let mut app = ai_match(2, &["ironhold", "sylvan"], 7);
    run_secs(&mut app, 240.0);
    let report = match_report(app.world_mut());
    let players = app.world().resource::<Players>().clone();
    for p in &players.list {
        assert!(p.stats.gathered.total() > 400.0, "player {} gathered too little\n{report}", p.id.0);
        assert!(p.stats.units_trained >= 4, "player {} trained too few units\n{report}", p.id.0);
        assert!(p.stats.buildings_built >= 2, "player {} built too few buildings\n{report}", p.id.0);
    }
}

/// No two ground units overlap (beyond a small tolerance) after a busy
/// period: the "boxes never clip" rule.
#[test]
fn ground_units_do_not_overlap() {
    let mut app = ai_match(2, &["dune", "tide"], 11);
    run_secs(&mut app, 150.0);
    let world = app.world_mut();
    let mut q = world.query_filtered::<(Entity, &Position, &Facing, &Footprint, &Mobile), (With<Unit>, Without<Dead>)>();
    let bodies: Vec<_> = q.iter(world).filter(|(_, _, _, _, m)| !m.flying).map(|(e, p, f, fp, _)| (e, fp.obb(p.0, f.0))).collect();
    let mut worst = 0.0f32;
    for i in 0..bodies.len() {
        for j in i + 1..bodies.len() {
            if let Some(m) = bodies[i].1.mtv(&bodies[j].1) {
                worst = worst.max(m.length());
            }
        }
    }
    assert!(worst < 0.15, "units overlap by up to {worst:.3} m");
}

/// The same seed replays identically (lockstep groundwork).
#[test]
fn deterministic_replay() {
    let snapshot = |seed| {
        let mut app = ai_match(2, &["steppe", "starborn"], seed);
        run_secs(&mut app, 90.0);
        let world = app.world_mut();
        let mut q = world.query_filtered::<(&Unit, &Position), Without<Dead>>();
        let mut units: Vec<(u16, i64, i64)> = q.iter(world).map(|(u, p)| (u.kind.0, (p.0.x * 100.0) as i64, (p.0.y * 100.0) as i64)).collect();
        units.sort();
        let res: Vec<i64> = world.resource::<Players>().list.iter().flat_map(|p| p.resources.0.map(|r| r as i64)).collect();
        (units, res)
    };
    assert_eq!(snapshot(5), snapshot(5));
}

/// Six AIs, one per faction, run for a while without stalling or panicking.
#[test]
fn six_factions_run() {
    let mut app = ai_match(6, &["ironhold", "sylvan", "dune", "tide", "steppe", "starborn"], 2);
    run_secs(&mut app, 120.0);
    let clock = *app.world().resource::<SimClock>();
    assert!(clock.elapsed >= 119.0, "sim stalled at {}s", clock.elapsed);
}

/// A long 1v1: the AIs age up and fight.
#[test]
#[ignore]
fn long_ai_match_ages_and_fights() {
    let mut app = ai_match(2, &["ironhold", "dune"], 9);
    run_headless(&mut app, (25.0 * 60.0 * SIM_HZ) as u64, (300.0 * SIM_HZ) as u64, |app| println!("{}", match_report(app.world_mut())));
    let report = match_report(app.world_mut());
    println!("{report}");
    let players = app.world().resource::<Players>().clone();
    assert!(players.list.iter().all(|p| p.age >= 2), "nobody aged up\n{report}");
    assert!(players.list.iter().any(|p| p.stats.kills > 0), "no fighting happened\n{report}");
}

/// Every faction pairing can play for ten minutes.
#[test]
#[ignore]
fn all_factions_ten_minutes() {
    let data = data();
    let ids: Vec<String> = data.factions.iter().map(|f| f.id.clone()).collect();
    for (i, a) in ids.iter().enumerate() {
        let b = &ids[(i + 1) % ids.len()];
        let mut app = ai_match(2, &[a.as_str(), b.as_str()], i as u64 + 20);
        run_secs(&mut app, 600.0);
        let report = match_report(app.world_mut());
        println!("{report}");
        let players = app.world().resource::<Players>().clone();
        for p in &players.list {
            assert!(p.stats.units_trained > 10, "{a} vs {b}: player {} stalled\n{report}", p.id.0);
        }
    }
}
