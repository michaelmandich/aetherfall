//! Play a seeded all-AI match on the referee app and print the state hash
//! every few ticks, so two machines (x86 and ARM, Windows and macOS and
//! Linux) can be compared line for line: the same output means the same
//! match, the first differing line says when they parted.
//!
//! ```text
//! cargo run --release -p af_sim --example sim_hashes -- --seed 7 --players 4 --minutes 15 --every 30
//! cargo run --release -p af_sim --example sim_hashes -- --seed 7 --players 4 --dump 18000
//! ```
//!
//! `--dump TICK` prints every player and every living unit, building and
//! node at that tick (the rows `sim_hash` folds), to diff two machines'
//! worlds and find the first thing that differs.
//!
//! `--mark SALT` stands in for a client's own components (`Selected`, the
//! visuals of what its player sees): every second it adds or removes an
//! empty marker on a salt-picked half of the entities, which moves them
//! between archetypes. Runs with different salts (or none) must print the
//! same hashes; where they part, some sim system depends on query order.
//! `--factions a,b,c` pins the factions per slot.

use std::sync::Arc;

use af_core::hash::{sim_hash, Fnv};
use af_core::prelude::*;
use af_sim::{referee_app, skirmish_setup, SkirmishOptions};
use bevy::prelude::*;

/// The empty client-only component `--mark` moves entities around with.
#[derive(Component)]
struct Marker;

fn main() {
    let mut seed = 7u64;
    let mut players = 4usize;
    let mut factions = Vec::new();
    let mut minutes = 15.0f64;
    let mut every = 30u64;
    let mut dump: Option<u64> = None;
    let mut mark: Option<u64> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| panic!("{arg} needs a value"));
        match arg.as_str() {
            "--seed" => seed = value().parse().expect("--seed N"),
            "--players" => players = value().parse().expect("--players N"),
            "--factions" => factions = value().split(',').map(str::to_string).collect(),
            "--minutes" => minutes = value().parse().expect("--minutes M"),
            "--every" => every = value().parse().expect("--every TICKS"),
            "--dump" => dump = Some(value().parse().expect("--dump TICK")),
            "--mark" => mark = Some(value().parse().expect("--mark SALT")),
            other => panic!("unknown argument `{other}`"),
        }
    }
    let dir = af_data::find_data_dir().expect("an assets/data folder");
    let data = Arc::new(GameData::load_dir(&dir).expect("game content"));
    eprintln!("content {:016x}, {} {}", af_data::content_hash(&dir).unwrap_or(0), std::env::consts::OS, std::env::consts::ARCH);
    let opts = SkirmishOptions { seed, players, factions, human: false, ..SkirmishOptions::default() };
    let setup = skirmish_setup(&data, &opts).expect("setup");
    let mut app = referee_app(data.clone(), setup);
    let ticks = dump.unwrap_or((minutes * 60.0 * SIM_HZ as f64) as u64);
    loop {
        let tick = app.world().resource::<SimClock>().tick;
        if every > 0 && tick % every == 0 && dump.is_none() {
            println!("{tick} {:016x}", sim_hash(app.world_mut()));
        }
        if tick >= ticks || app.world().resource::<GameOutcome>().ended {
            break;
        }
        if let Some(salt) = mark.filter(|_| tick % SIM_HZ as u64 == 0) {
            remark(app.world_mut(), salt, tick);
        }
        app.update();
    }
    if dump.is_some() {
        print_world(app.world_mut(), &data);
    }
}

/// Give a salt-picked half of the units, buildings and nodes the marker and
/// take it off the rest; the half changes every ten seconds.
fn remark(world: &mut World, salt: u64, tick: u64) {
    let mut q = world.query_filtered::<(Entity, Has<Marker>), Or<(With<Unit>, With<Building>, With<ResourceNode>)>>();
    let flips: Vec<(Entity, bool)> = q
        .iter(world)
        .filter_map(|(e, has)| {
            let want = Fnv::new().u64(salt).u64(e.to_bits()).u64(tick / (10 * SIM_HZ as u64)).finish() & 1 == 1;
            (want != has).then_some((e, want))
        })
        .collect();
    for (e, want) in flips {
        if want {
            world.entity_mut(e).insert(Marker);
        } else {
            world.entity_mut(e).remove::<Marker>();
        }
    }
}

fn print_world(world: &mut World, data: &GameData) {
    let tick = world.resource::<SimClock>().tick;
    println!("tick {tick} hash {:016x}", sim_hash(world));
    for p in &world.resource::<Players>().list.clone() {
        let res: Vec<String> = p.resources.0.iter().map(|r| format!("{:08x}", r.to_bits())).collect();
        println!("P{} age {} pop {}/{} kills {} lost {} res {}", p.id.0, p.age, p.pop_used, p.pop_cap, p.stats.kills, p.stats.units_lost, res.join(" "));
    }
    let mut rows = Vec::new();
    let mut units = world.query_filtered::<(Entity, &Unit, &Owner, &Position, &Facing, Option<&Health>), Without<Dead>>();
    for (e, u, o, p, f, hp) in units.iter(world) {
        rows.push(format!("{:>10} unit {:<16} P{} pos {:08x},{:08x} ({:.3},{:.3}) face {:08x} hp {:08x}", e.to_bits(), data.unit(u.kind).id, o.0 .0, p.0.x.to_bits(), p.0.y.to_bits(), p.0.x, p.0.y, f.0.to_bits(), hp.map_or(0, |h| h.hp.to_bits())));
    }
    let mut buildings = world.query_filtered::<(Entity, &Building, &Owner, &Position, Option<&Health>), Without<Dead>>();
    for (e, b, o, p, hp) in buildings.iter(world) {
        rows.push(format!("{:>10} bldg {:<16} P{} pos {:08x},{:08x} hp {:08x}", e.to_bits(), data.building(b.kind).id, o.0 .0, p.0.x.to_bits(), p.0.y.to_bits(), hp.map_or(0, |h| h.hp.to_bits())));
    }
    let mut nodes = world.query_filtered::<(Entity, &ResourceNode, &Position), Without<Dead>>();
    for (e, n, p) in nodes.iter(world) {
        rows.push(format!("{:>10} node {:<16} pos {:08x},{:08x} left {:08x}", e.to_bits(), n.kind.0, p.0.x.to_bits(), p.0.y.to_bits(), n.remaining.to_bits()));
    }
    rows.sort();
    for r in rows {
        println!("{r}");
    }
}
