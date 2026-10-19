//! Which units, buildings and resource nodes are on the map the most, over
//! real AI matches with every faction: the priority list for replacing
//! boxes with models.
//!
//!   cargo run --release -p aetherfall --example asset_census
//!   cargo run --release -p aetherfall --example asset_census -- --minutes 30 --seeds 3
//!
//! It plays a 1v1 for every pair of neighbouring factions and a six-player
//! free-for-all per seed, samples every entity every 10 sim seconds, and
//! ranks by presence (how many are on the map at an average moment, over
//! all matches), with the share of matches each one appears in.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use aetherfall::*;
use af_core::prelude::*;
use bevy::prelude::*;

#[derive(Default)]
struct Tally {
    /// Sum over samples of how many were on the map.
    count: f64,
    /// Matches it appeared in.
    matches: BTreeSet<usize>,
    name: String,
}

fn main() {
    let mut minutes = 25.0f64;
    let mut seeds = 2u64;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--minutes" => minutes = args.next().and_then(|v| v.parse().ok()).expect("--minutes N"),
            "--seeds" => seeds = args.next().and_then(|v| v.parse().ok()).expect("--seeds N"),
            other => panic!("unknown argument {other}"),
        }
    }
    let data = Arc::new(GameData::load_default().unwrap_or_else(|e| panic!("{e}")));
    let ids: Vec<String> = data.factions.iter().map(|f| f.id.clone()).collect();
    let mut matches: Vec<(u64, Vec<String>)> = Vec::new();
    for seed in 1..=seeds {
        for (i, a) in ids.iter().enumerate() {
            matches.push((seed * 100 + i as u64, vec![a.clone(), ids[(i + 1) % ids.len()].clone()]));
        }
        matches.push((seed * 100 + 50, ids.clone()));
    }

    let ticks = (minutes * 60.0 * SIM_HZ) as u64;
    let every = (10.0 * SIM_HZ) as u64;
    let mut tally: BTreeMap<String, Tally> = BTreeMap::new();
    let mut samples_total = 0.0f64;
    for (m, (seed, factions)) in matches.iter().enumerate() {
        let opts = SkirmishOptions { seed: *seed, players: factions.len(), factions: factions.clone(), human: false, ..Default::default() };
        let setup = skirmish_setup(&data, &opts).expect("setup");
        let mut app = headless_app(data.clone(), setup, 30);
        let mut samples = 0u32;
        run_headless(&mut app, ticks, every, |app| {
            samples += 1;
            let world = app.world_mut();
            let mut add = |key: String, name: &str| {
                let t = tally.entry(key).or_default();
                t.count += 1.0;
                t.matches.insert(m);
                if t.name.is_empty() {
                    t.name = name.to_string();
                }
            };
            let mut units = world.query_filtered::<&Unit, Without<Dead>>();
            for u in units.iter(world) {
                let d = data.unit(u.kind);
                add(format!("unit     {}", d.id), &d.name);
            }
            let mut buildings = world.query_filtered::<&Building, Without<Dead>>();
            for b in buildings.iter(world) {
                let d = data.building(b.kind);
                add(format!("building {}", d.id), &d.name);
            }
            let mut nodes = world.query_filtered::<&ResourceNode, Without<Dead>>();
            for n in nodes.iter(world) {
                let d = data.node(n.kind);
                add(format!("node     {}", d.id), &d.name);
            }
        });
        samples_total += samples as f64;
        eprintln!("match {}/{} done: {} ({} samples)", m + 1, matches.len(), factions.join(" vs "), samples);
    }

    let mut rows: Vec<(&String, &Tally)> = tally.iter().collect();
    rows.sort_by(|a, b| b.1.count.partial_cmp(&a.1.count).unwrap_or(std::cmp::Ordering::Equal));
    println!("{} matches x {minutes} min, all six factions; presence = number on the map at an average moment", matches.len());
    println!("{:>4}  {:<36} {:<28} {:>10} {:>9}", "rank", "kind / id", "name", "presence", "in games");
    for (i, (key, t)) in rows.iter().enumerate() {
        let per_match = t.count / samples_total;
        println!(
            "{:>4}  {:<36} {:<28} {:>10.1} {:>8.0}%",
            i + 1,
            key,
            t.name,
            per_match,
            100.0 * t.matches.len() as f64 / matches.len() as f64
        );
    }
}
