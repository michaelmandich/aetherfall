//! The large-scale plan every other phase works around: the map's size and
//! its ocean bands ([`Land`]), the resolved options, the starts on a ring,
//! the meteor sites, the contested spots and the shared start template.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use af_core::ext::map::{MapEdge, MapOptions, ResolvedMapOptions, RiverCount, SiteChoice, SiteKind, WaterLevel};
use af_core::prelude::*;
use bevy::prelude::*;

use super::sites::{Site, SPOT_CRATER_MIN};
use super::starts::{Frame, StartTemplate};
use super::{polar, Ids};

/// Start positions sit on a circle of this fraction of the land side...
const START_RING: f32 = 0.36;
/// ...but at least this far inside the land square's edges (small maps).
const START_EDGE_MIN: f32 = 30.0;
/// Contested spots sit on the bisectors this far out (as a fraction of the
/// land side), further when the meteor site needs the room.
const SPOT_RING: f32 = 0.235;
/// Reserved radius around a contested spot between neighbours.
pub(crate) const SPOT_RADIUS: f32 = 12.0;
/// Contested spots keep this far outside the main site's rim.
const SPOT_SITE_GAP: f32 = 3.0;
/// An ocean band is this fraction of the land side deep.
const OCEAN_BAND: f32 = 0.17;
/// Salt for the stream the random options are resolved from.
const OPTIONS_SALT: u64 = 0x4D41_505F_4F50_5453;

/// `options` with every random choice made from `seed` (its own stream, so
/// the choice never depends on anything else the generator does).
pub fn resolve_map_options(options: &MapOptions, seed: u64) -> ResolvedMapOptions {
    let mut rng = SimRng::new(seed ^ OPTIONS_SALT);
    let rivers = match options.rivers {
        RiverCount::One => 1,
        RiverCount::Two => 2,
        RiverCount::Random => 1 + rng.below(2) as u8,
    };
    let site = match options.site {
        SiteChoice::Kind(k) => k,
        SiteChoice::Random => SiteKind::ALL[rng.below(SiteKind::ALL.len() as u32) as usize],
    };
    ResolvedMapOptions { size: options.size, water: options.water, rivers, site }
}

/// The map's shape: a square of side `size` holding a square of land of side
/// `side` (where the starts, sites and wilds are laid out) plus an ocean
/// band beyond each ocean edge. Land between the square and a land edge (the
/// map is square, so a map with ocean on one side has land strips on two
/// others) is ordinary wilds.
#[derive(Clone, Debug)]
pub(crate) struct Land {
    /// Map side; the map spans `0..size` on x and z.
    pub size: f32,
    /// Side of the land square.
    pub side: f32,
    /// Min corner of the land square.
    pub square: Vec2,
    /// The land rectangle: the map minus its ocean bands. Every coast lies
    /// outside it.
    pub min: Vec2,
    pub max: Vec2,
    /// Depth of each ocean band (0 without ocean).
    pub band: f32,
    /// Map edges that are ocean.
    pub ocean: Vec<MapEdge>,
}

impl Land {
    /// The land for a square of `side` with ocean beyond the `ocean` edges.
    pub fn new(side: f32, ocean: Vec<MapEdge>) -> Land {
        let band = if ocean.is_empty() { 0.0 } else { (side * OCEAN_BAND).round() };
        let has = |e: MapEdge| ocean.contains(&e);
        let pair = |a: MapEdge, b: MapEdge| has(a) as u8 + has(b) as u8;
        let bands = pair(MapEdge::West, MapEdge::East).max(pair(MapEdge::South, MapEdge::North));
        let size = side + band * bands as f32;
        let min = Vec2::new(if has(MapEdge::West) { band } else { 0.0 }, if has(MapEdge::South) { band } else { 0.0 });
        let max = Vec2::new(if has(MapEdge::East) { size - band } else { size }, if has(MapEdge::North) { size - band } else { size });
        // Centred in the land rectangle, which is `side` wide across every
        // ocean edge and wider along it.
        let square = min + (max - min - Vec2::splat(side)) * 0.5;
        Land { size, side, square, min, max, band, ocean }
    }

    /// The land for `water`, with the ocean edges drawn from `rng`: one for
    /// `Coast`, two (adjacent or opposite) for `Seas`.
    pub fn plan(side: f32, water: WaterLevel, rng: &mut SimRng) -> Land {
        let first = MapEdge::ALL[rng.below(4) as usize];
        let ocean = match water.ocean_edges() {
            0 => Vec::new(),
            1 => vec![first],
            _ => {
                let second = if rng.chance(0.5) {
                    opposite(first)
                } else {
                    let [a, b] = adjacent(first);
                    if rng.chance(0.5) { a } else { b }
                };
                vec![first, second]
            }
        };
        Land::new(side, ocean)
    }

    /// Centre of the land square: the layout centre.
    pub fn center(&self) -> Vec2 {
        self.square + Vec2::splat(self.side * 0.5)
    }

    pub fn is_ocean(&self, e: MapEdge) -> bool {
        self.ocean.contains(&e)
    }

    /// Distance from `p` to the nearest map edge that is land (infinite
    /// when every edge is ocean, which never happens).
    pub fn land_edge_distance(&self, p: Vec2) -> f32 {
        MapEdge::ALL.into_iter().filter(|e| !self.is_ocean(*e)).map(|e| self.edge_distance(p, e)).fold(f32::INFINITY, f32::min)
    }

    /// Distance from `p` to map edge `e`.
    pub fn edge_distance(&self, p: Vec2, e: MapEdge) -> f32 {
        match e {
            MapEdge::West => p.x,
            MapEdge::East => self.size - p.x,
            MapEdge::South => p.y,
            MapEdge::North => self.size - p.y,
        }
    }

    /// How far `p` lies beyond the land square toward ocean edge `e`
    /// (negative inside the square).
    pub fn beyond_square(&self, p: Vec2, e: MapEdge) -> f32 {
        match e {
            MapEdge::West => self.square.x - p.x,
            MapEdge::East => p.x - (self.square.x + self.side),
            MapEdge::South => self.square.y - p.y,
            MapEdge::North => p.y - (self.square.y + self.side),
        }
    }
}

pub(crate) fn opposite(e: MapEdge) -> MapEdge {
    match e {
        MapEdge::West => MapEdge::East,
        MapEdge::East => MapEdge::West,
        MapEdge::South => MapEdge::North,
        MapEdge::North => MapEdge::South,
    }
}

fn adjacent(e: MapEdge) -> [MapEdge; 2] {
    match e {
        MapEdge::West | MapEdge::East => [MapEdge::South, MapEdge::North],
        MapEdge::South | MapEdge::North => [MapEdge::West, MapEdge::East],
    }
}

/// The large-scale plan every other phase works around.
pub(crate) struct Layout {
    pub size: f32,
    pub land: Land,
    pub options: ResolvedMapOptions,
    /// The land square's centre: the main meteor site and the start ring's
    /// centre.
    pub center: Vec2,
    pub starts: Vec<Vec2>,
    /// Math angles halfway between neighbouring starts.
    pub bisectors: Vec<f32>,
    /// Contested spot per bisector.
    pub spots: Vec<Vec2>,
    /// Reserved radius of each spot.
    pub spot_radius: Vec<f32>,
    /// Every meteor site, the main one (at the centre) first. With
    /// `Shattered`, the others are the small craters at the spots, in spot
    /// order.
    pub sites: Vec<Site>,
    pub template: StartTemplate,
    /// World centre of each start's aether crystal cluster.
    pub start_aether: Vec<Vec2>,
}

impl Layout {
    pub fn new(data: &GameData, ids: &Ids, players: usize, land: Land, options: ResolvedMapOptions, rng: &mut SimRng) -> Layout {
        let center = land.center();
        let side = land.side;
        let rotation = rng.range(0.0, TAU);
        let start_ring = (side * START_RING).min(side * 0.5 - START_EDGE_MIN);
        let start_angles: Vec<f32> = (0..players).map(|i| rotation + i as f32 * TAU / players as f32).collect();
        let starts: Vec<Vec2> = start_angles.iter().map(|a| center + polar(*a) * start_ring).collect();
        let bisectors = match players {
            0 => vec![rotation],
            1 => vec![rotation + PI],
            2 => vec![rotation + FRAC_PI_2, rotation - FRAC_PI_2],
            n => (0..n).map(|i| rotation + (i as f32 + 0.5) * TAU / n as f32).collect(),
        };

        let main = Site::plan_main(options.site, side, center, &start_angles, &bisectors, start_ring, &mut rng.fork());
        // Every start's aether cluster stops short of the main site's rim
        // along its corridor; the template is shared, so use the widest.
        let reach = start_angles.iter().map(|a| main.reach_toward(*a)).fold(main.reach_toward(rotation), f32::max);
        let template = StartTemplate::new(data, ids, &mut rng.fork(), start_ring, reach);

        // Contested spots: on the bisectors, pushed out past the main site
        // where it reaches that far (both neighbours stay equidistant).
        let shattered = options.site == SiteKind::Shattered;
        let spot_crater = (side * super::sites::SPOT_CRATER_RING).max(SPOT_CRATER_MIN);
        let one_radius = if shattered { spot_crater + super::sites::SPOT_RIM_HALF + 2.0 } else { SPOT_RADIUS };
        let spot_radius = vec![one_radius; bisectors.len()];
        let spots: Vec<Vec2> = bisectors
            .iter()
            .map(|b| {
                let ring = (side * SPOT_RING).max(main.reach_toward(*b) + SPOT_SITE_GAP + one_radius);
                center + polar(*b) * ring
            })
            .collect();

        let mut sites = vec![main];
        if shattered {
            for (k, spot) in spots.iter().enumerate() {
                // Gaps toward the two starts the spot lies between (for one
                // player: toward the start and toward the centre).
                let toward: Vec<Vec2> = match players {
                    0 => vec![center],
                    1 => vec![starts[0], center],
                    2 => starts.clone(),
                    n => vec![starts[k], starts[(k + 1) % n]],
                };
                sites.push(Site::spot_crater(*spot, spot_crater, &toward));
            }
        }

        let mut layout = Layout {
            size: land.size,
            land,
            options,
            center,
            starts,
            bisectors,
            spots,
            spot_radius,
            sites,
            template,
            start_aether: Vec::new(),
        };
        layout.start_aether = (0..players).map(|i| layout.frame(i).point(layout.template.aether)).collect();
        layout
    }

    /// Start `i`'s local frame: x toward the map centre, y to its left.
    pub fn frame(&self, i: usize) -> Frame {
        Frame::new(self.starts[i], self.center)
    }

    /// The main meteor site (at the centre).
    pub fn main(&self) -> &Site {
        &self.sites[0]
    }

    /// The corridor segments, start -> centre.
    pub fn corridors(&self) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
        self.starts.iter().map(|s| (*s, self.center))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocean_bands_grow_the_map_around_a_whole_land_square() {
        let l = Land::new(200.0, vec![MapEdge::West]);
        assert_eq!((l.band, l.size), (34.0, 234.0));
        assert_eq!((l.min, l.max), (Vec2::new(34.0, 0.0), Vec2::splat(234.0)));
        assert_eq!(l.square, Vec2::new(34.0, 17.0));
        assert_eq!(l.center(), Vec2::new(134.0, 117.0));

        let corner = Land::new(200.0, vec![MapEdge::North, MapEdge::East]);
        assert_eq!(corner.size, 234.0);
        assert_eq!((corner.min, corner.max, corner.square), (Vec2::ZERO, Vec2::splat(200.0), Vec2::ZERO));

        let strait = Land::new(200.0, vec![MapEdge::South, MapEdge::North]);
        assert_eq!(strait.size, 268.0);
        assert_eq!((strait.min, strait.max), (Vec2::new(0.0, 34.0), Vec2::new(268.0, 234.0)));
        assert_eq!(strait.square, Vec2::new(34.0, 34.0));
        assert_eq!(strait.land_edge_distance(Vec2::new(10.0, 100.0)), 10.0);
        assert!(strait.beyond_square(Vec2::new(100.0, 250.0), MapEdge::North) == 16.0);

        let dry = Land::new(200.0, Vec::new());
        assert_eq!((dry.size, dry.band, dry.square), (200.0, 0.0, Vec2::ZERO));
    }

    #[test]
    fn random_options_resolve_from_the_seed() {
        let random = MapOptions::default();
        let picked: Vec<ResolvedMapOptions> = (0..64).map(|seed| resolve_map_options(&random, seed)).collect();
        assert_eq!(picked, (0..64).map(|seed| resolve_map_options(&random, seed)).collect::<Vec<_>>());
        assert!(picked.iter().any(|o| o.rivers == 1) && picked.iter().any(|o| o.rivers == 2));
        for kind in SiteKind::ALL {
            assert!(picked.iter().any(|o| o.site == kind), "{kind:?} never picked");
        }
        let fixed = MapOptions { rivers: RiverCount::Two, site: SiteChoice::Kind(SiteKind::Spire), ..random };
        assert!((0..16).all(|seed| resolve_map_options(&fixed, seed) == ResolvedMapOptions { size: fixed.size, water: fixed.water, rivers: 2, site: SiteKind::Spire }));
    }
}
