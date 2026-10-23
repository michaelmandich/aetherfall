//! A digest of the match state, so lockstep peers (and the ranked server)
//! can tell that they still agree.
//!
//! [`sim_hash`] folds the clock, every player's resources, age and
//! standing, and every living unit's, building's and node's identity,
//! position, facing and hit points into one `u64`, in entity order. Two
//! worlds that replay the same match give the same hash on the same tick;
//! a world that drifted gives another within a tick or two of the drift.
//!
//! The hash is FNV-1a over the raw bits (`f32::to_bits`), so it is the same
//! on every platform and build, and never compares floats for equality
//! across machines: a match is either bit-identical or desynced.

use bevy::prelude::*;

use crate::components::{Building, Dead, Facing, Gatherer, Health, Owner, Position, ResourceNode, Unit};
use crate::resources::{Players, SimClock};

/// FNV-1a over 64 bits: tiny, stable, and good enough to tell two matches apart.
#[derive(Clone, Copy, Debug)]
pub struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    pub fn new() -> Fnv {
        Fnv::default()
    }

    pub fn bytes(&mut self, bytes: &[u8]) -> &mut Fnv {
        for &b in bytes {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
        self
    }

    pub fn u8(&mut self, v: u8) -> &mut Fnv {
        self.bytes(&[v])
    }

    pub fn u16(&mut self, v: u16) -> &mut Fnv {
        self.bytes(&v.to_le_bytes())
    }

    pub fn u32(&mut self, v: u32) -> &mut Fnv {
        self.bytes(&v.to_le_bytes())
    }

    pub fn u64(&mut self, v: u64) -> &mut Fnv {
        self.bytes(&v.to_le_bytes())
    }

    pub fn f32(&mut self, v: f32) -> &mut Fnv {
        self.u32(v.to_bits())
    }

    pub fn str(&mut self, s: &str) -> &mut Fnv {
        self.u32(s.len() as u32).bytes(s.as_bytes())
    }

    pub fn finish(&self) -> u64 {
        self.0
    }
}

/// The digest of the match in `world` right now (see the module docs).
pub fn sim_hash(world: &mut World) -> u64 {
    let mut h = Fnv::new();
    if let Some(clock) = world.get_resource::<SimClock>() {
        h.u64(clock.tick);
    }
    if let Some(players) = world.get_resource::<Players>() {
        h.u8(players.list.len() as u8);
        for p in &players.list {
            for amount in p.resources.0 {
                h.f32(amount);
            }
            h.u32(p.pop_used).u32(p.pop_cap).u8(p.age).u8(p.defeated as u8);
            h.u32(p.stats.kills).u32(p.stats.units_lost);
        }
    }
    let mut rows: Vec<(Entity, u8, u16, u8, u32, u32, u32, u32)> = Vec::new();
    let mut units = world.query_filtered::<(Entity, &Unit, &Owner, &Position, &Facing, Option<&Health>, Option<&Gatherer>), Without<Dead>>();
    for (e, u, o, p, f, hp, cargo) in units.iter(world) {
        let hp_and_cargo = hp.map_or(0, |h| h.hp.to_bits()) ^ cargo.map_or(0, |g| g.amount.to_bits().rotate_left(16));
        rows.push((e, 0, u.kind.0, o.0 .0, p.0.x.to_bits(), p.0.y.to_bits(), f.0.to_bits(), hp_and_cargo));
    }
    let mut buildings = world.query_filtered::<(Entity, &Building, &Owner, &Position, &Facing, Option<&Health>), Without<Dead>>();
    for (e, b, o, p, f, hp) in buildings.iter(world) {
        rows.push((e, 1, b.kind.0, o.0 .0, p.0.x.to_bits(), p.0.y.to_bits(), f.0.to_bits(), hp.map_or(0, |h| h.hp.to_bits())));
    }
    // A node's facing too: it is an obstacle's box, so a map generated a
    // bit apart shows at once rather than when someone walks into it.
    let mut nodes = world.query_filtered::<(Entity, &ResourceNode, &Position, Option<&Facing>), Without<Dead>>();
    for (e, n, p, f) in nodes.iter(world) {
        rows.push((e, 2, n.kind.0, 0, p.0.x.to_bits(), p.0.y.to_bits(), n.remaining.to_bits(), f.map_or(0, |f| f.0.to_bits())));
    }
    rows.sort_unstable();
    h.u32(rows.len() as u32);
    for (e, kind, type_index, owner, a, b, c, d) in rows {
        h.u64(e.to_bits()).u8(kind).u16(type_index).u8(owner).u32(a).u32(b).u32(c).u32(d);
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use crate::testing::{flat_world, run_ticks, sim_app, test_data, with_commands};

    #[test]
    fn fnv_is_stable() {
        // The reference FNV-1a 64 vectors.
        assert_eq!(Fnv::new().finish(), 0xcbf2_9ce4_8422_2325);
        assert_eq!(Fnv::new().bytes(b"a").finish(), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(Fnv::new().str("foobar").finish(), Fnv::new().u32(6).bytes(b"foobar").finish());
    }

    #[test]
    fn same_world_same_hash_and_a_change_shows() {
        let data = test_data();
        let mut app = sim_app(data.clone());
        let players = flat_world(&mut app, 64.0, &[("ironhold", 0), ("dune", 1)]);
        let unit = with_commands(&mut app, |c, data, ps| spawn_unit(c, data, &ps.list[0], data.unit_id("peasant").unwrap(), Vec2::new(10.0, 10.0), 0.0));
        run_ticks(&mut app, 1);
        let a = sim_hash(app.world_mut());
        assert_eq!(a, sim_hash(app.world_mut()), "hashing is a pure read");
        app.world_mut().get_mut::<Position>(unit).unwrap().0.x += 0.001;
        assert_ne!(a, sim_hash(app.world_mut()), "a moved unit changes the hash");
        app.world_mut().get_mut::<Position>(unit).unwrap().0.x -= 0.001;
        assert_eq!(a, sim_hash(app.world_mut()));
        app.world_mut().resource_mut::<Players>().get_mut(players[1]).resources.0[0] += 1.0;
        assert_ne!(a, sim_hash(app.world_mut()), "a player's resources change the hash");
    }
}
