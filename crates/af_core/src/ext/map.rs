//! Shared items added by af_map (other crates may use them).
//! Only af_map edits this file. Never modify items in other af_core files.

use bevy::prelude::*;

/// The set in which af_map builds the match world (map, players, every match
/// resource and the start entities) in `OnEnter(GameState::Playing)`, and
/// tears it down again in `OnExit(GameState::Playing)`.
///
/// * An `OnEnter(GameState::Playing)` system that needs `Players`, `Nav`,
///   `MapInfo` or the start entities runs `.after(MapSetupSet)`; Bevy
///   applies af_map's commands in between.
/// * An `OnExit(GameState::Playing)` system that still needs them runs
///   `.before(MapSetupSet)`.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MapSetupSet;

/// How the next match's map is generated (`docs/design/maps-and-terrain.md`).
///
/// A resource next to `MatchSetup`: the menu and the CLI insert it, af_map
/// reads it in `OnEnter(GameState::Playing)`; a missing resource means
/// `MapOptions::default()`. `MatchSetup::map_size`, when not 0, overrides
/// [`MapOptions::size`]. The "random" choices are resolved from the match
/// seed and published, resolved, in [`MapFeatures::options`].
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MapOptions {
    pub size: MapSize,
    pub water: WaterLevel,
    pub rivers: RiverCount,
    pub site: SiteChoice,
}

/// Land size relative to the player count's base size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MapSize {
    Small,
    #[default]
    Medium,
    Large,
    Huge,
}

impl MapSize {
    pub const ALL: [MapSize; 4] = [MapSize::Small, MapSize::Medium, MapSize::Large, MapSize::Huge];

    /// Multiplier on the player count's base land side.
    pub fn factor(self) -> f32 {
        match self {
            MapSize::Small => 0.8,
            MapSize::Medium => 1.0,
            MapSize::Large => 1.3,
            MapSize::Huge => 1.6,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            MapSize::Small => "Small",
            MapSize::Medium => "Medium",
            MapSize::Large => "Large",
            MapSize::Huge => "Huge",
        }
    }

    /// Parse a CLI/menu name (case-insensitive).
    pub fn parse(s: &str) -> Option<MapSize> {
        MapSize::ALL.into_iter().find(|v| v.name().eq_ignore_ascii_case(s.trim()))
    }

    /// Side of the land square in metres for `players` player slots: the
    /// base side (224 m for up to two players, 32 m more per player, 384 m
    /// from seven) times [`MapSize::factor`], rounded to a multiple of 8 m
    /// and clamped to 160-1024 m. Ocean bands add to this (see
    /// [`MapFeatures::land_min`]).
    pub fn land_side(self, players: usize) -> f32 {
        let base = match players {
            0..=2 => 224.0,
            3 => 256.0,
            4 => 288.0,
            5 => 320.0,
            6 => 352.0,
            _ => 384.0,
        };
        ((base * self.factor() / 8.0).round() * 8.0).clamp(160.0, 1024.0)
    }
}

/// How much standing water the map has besides its rivers, a 0-4 scale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WaterLevel {
    /// Rivers only.
    Dry,
    /// A pond or two.
    Ponds,
    /// Several lakes.
    #[default]
    Lakes,
    /// One map edge is ocean, plus a lake or two.
    Coast,
    /// Two map edges are ocean, plus a few lakes.
    Seas,
}

impl WaterLevel {
    pub const ALL: [WaterLevel; 5] = [WaterLevel::Dry, WaterLevel::Ponds, WaterLevel::Lakes, WaterLevel::Coast, WaterLevel::Seas];

    /// Position on the 0-4 scale.
    pub fn level(self) -> u8 {
        self as u8
    }

    /// The level at `n` on the 0-4 scale (clamped).
    pub fn from_level(n: u8) -> WaterLevel {
        WaterLevel::ALL[(n as usize).min(4)]
    }

    /// How many map edges are ocean.
    pub fn ocean_edges(self) -> usize {
        match self {
            WaterLevel::Coast => 1,
            WaterLevel::Seas => 2,
            _ => 0,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            WaterLevel::Dry => "Dry",
            WaterLevel::Ponds => "Ponds",
            WaterLevel::Lakes => "Lakes",
            WaterLevel::Coast => "Coast",
            WaterLevel::Seas => "Seas",
        }
    }

    /// Parse a level number (0-4) or a name (case-insensitive).
    pub fn parse(s: &str) -> Option<WaterLevel> {
        let s = s.trim();
        if let Ok(n) = s.parse::<u8>() {
            return (n <= 4).then(|| WaterLevel::from_level(n));
        }
        WaterLevel::ALL.into_iter().find(|v| v.name().eq_ignore_ascii_case(s))
    }
}

/// How many rivers cross the map.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RiverCount {
    /// One or two, from the seed.
    #[default]
    Random,
    One,
    Two,
}

impl RiverCount {
    pub const ALL: [RiverCount; 3] = [RiverCount::Random, RiverCount::One, RiverCount::Two];

    pub fn name(self) -> &'static str {
        match self {
            RiverCount::Random => "Random",
            RiverCount::One => "One",
            RiverCount::Two => "Two",
        }
    }

    /// Parse "1", "2", "one", "two" or "random" (case-insensitive).
    pub fn parse(s: &str) -> Option<RiverCount> {
        match s.trim().to_ascii_lowercase().as_str() {
            "1" | "one" => Some(RiverCount::One),
            "2" | "two" => Some(RiverCount::Two),
            "random" => Some(RiverCount::Random),
            _ => None,
        }
    }
}

/// How the star fell: the shape of the meteor site(s).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SiteKind {
    /// A central crater with a cliff rim and ramp gaps (the classic).
    Crater,
    /// A smaller central crater; the rest fell at the contested spots,
    /// each a small rimmed crater.
    Shattered,
    /// The crater is flooded: an island in the middle, ford causeways.
    Lake,
    /// Three basins in a line through the centre, walled along both sides.
    Scar,
    /// No rim: an impassable crystal spire with the nodes in a ring round it.
    Spire,
}

impl SiteKind {
    pub const ALL: [SiteKind; 5] = [SiteKind::Crater, SiteKind::Shattered, SiteKind::Lake, SiteKind::Scar, SiteKind::Spire];

    /// Short id used by the CLI.
    pub fn id(self) -> &'static str {
        match self {
            SiteKind::Crater => "crater",
            SiteKind::Shattered => "shattered",
            SiteKind::Lake => "lake",
            SiteKind::Scar => "scar",
            SiteKind::Spire => "spire",
        }
    }

    /// The name players see.
    pub fn name(self) -> &'static str {
        match self {
            SiteKind::Crater => "The Starfall Crater",
            SiteKind::Shattered => "The Shattered Fall",
            SiteKind::Lake => "The Crater Lake",
            SiteKind::Scar => "The Skyfall Scar",
            SiteKind::Spire => "The Starspire",
        }
    }

    /// Parse an id (case-insensitive).
    pub fn parse(s: &str) -> Option<SiteKind> {
        SiteKind::ALL.into_iter().find(|v| v.id().eq_ignore_ascii_case(s.trim()))
    }
}

/// The meteor site option: one kind, or random from the seed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SiteChoice {
    #[default]
    Random,
    Kind(SiteKind),
}

impl SiteChoice {
    /// Parse "random" or a [`SiteKind`] id.
    pub fn parse(s: &str) -> Option<SiteChoice> {
        if s.trim().eq_ignore_ascii_case("random") { Some(SiteChoice::Random) } else { SiteKind::parse(s).map(SiteChoice::Kind) }
    }

    pub fn name(self) -> &'static str {
        match self {
            SiteChoice::Random => "Random",
            SiteChoice::Kind(k) => k.name(),
        }
    }
}

/// [`MapOptions`] with every random choice made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedMapOptions {
    pub size: MapSize,
    pub water: WaterLevel,
    /// 1 or 2.
    pub rivers: u8,
    pub site: SiteKind,
}

/// One map edge. The map spans `0..size` on x and z.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MapEdge {
    /// x = 0.
    West,
    /// x = size.
    East,
    /// z = 0.
    South,
    /// z = size.
    North,
}

impl MapEdge {
    pub const ALL: [MapEdge; 4] = [MapEdge::West, MapEdge::East, MapEdge::South, MapEdge::North];
}

/// One meteor site.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeteorSite {
    pub kind: SiteKind,
    pub center: Vec2,
    /// Radius of the site's rim (or, without a rim, of its node ring).
    pub radius: f32,
}

/// One river, source to mouth.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RiverLine {
    /// Centreline, a point every ~0.75 m, source first.
    pub points: Vec<Vec2>,
    /// Half-width of the deep core at each point (the shallow banks reach
    /// beyond it).
    pub half_widths: Vec<f32>,
    /// Centres of the fords, as indices into `points`.
    pub fords: Vec<usize>,
}

/// One mountain range or massif.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MountainRange {
    /// Spine points (one for a round massif).
    pub spine: Vec<Vec2>,
    /// Half-width at each spine point.
    pub half_widths: Vec<f32>,
}

/// What af_map generated besides the terrain grid: the resolved options and
/// the large features, for the HUD, the renderer (river flow, site looks)
/// and tests. Inserted with `MapInfo` in `OnEnter(GameState::Playing)` and
/// removed on exit. `MapInfo::crater_center`/`crater_radius` are
/// `sites[0]`.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct MapFeatures {
    pub options: ResolvedMapOptions,
    /// The land rectangle (the map minus its ocean bands), min and max.
    pub land_min: Vec2,
    pub land_max: Vec2,
    /// Map edges that are ocean.
    pub ocean_edges: Vec<MapEdge>,
    /// Every meteor site, the main one (at the layout centre) first.
    pub sites: Vec<MeteorSite>,
    pub rivers: Vec<RiverLine>,
    pub mountains: Vec<MountainRange>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_names_round_trip() {
        for s in MapSize::ALL {
            assert_eq!(MapSize::parse(&s.name().to_ascii_uppercase()), Some(s));
        }
        for w in WaterLevel::ALL {
            assert_eq!(WaterLevel::parse(w.name()), Some(w));
            assert_eq!(WaterLevel::parse(&w.level().to_string()), Some(w));
            assert_eq!(WaterLevel::from_level(w.level()), w);
        }
        assert_eq!(WaterLevel::parse("5"), None);
        for r in RiverCount::ALL {
            assert_eq!(RiverCount::parse(r.name()), Some(r));
        }
        assert_eq!(RiverCount::parse("2"), Some(RiverCount::Two));
        for k in SiteKind::ALL {
            assert_eq!(SiteChoice::parse(k.id()), Some(SiteChoice::Kind(k)));
        }
        assert_eq!(SiteChoice::parse("Random"), Some(SiteChoice::Random));
        assert_eq!(SiteChoice::parse("volcano"), None);
        assert_eq!(MapOptions::default().water, WaterLevel::Lakes);
    }

    #[test]
    fn land_sides_follow_the_size_table() {
        // docs/design/maps-and-terrain.md, section 1.1.
        let table = [(2, [176.0, 224.0, 288.0, 360.0]), (4, [232.0, 288.0, 376.0, 464.0]), (8, [304.0, 384.0, 496.0, 616.0])];
        for (players, sides) in table {
            for (size, side) in MapSize::ALL.into_iter().zip(sides) {
                assert_eq!(size.land_side(players), side, "{} for {players} players", size.name());
            }
        }
        assert_eq!(MapSize::Medium.land_side(0), 224.0);
        assert_eq!(MapSize::Huge.land_side(40), 616.0);
    }
}
