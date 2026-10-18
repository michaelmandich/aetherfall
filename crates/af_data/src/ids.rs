//! Typed indices into the [`crate::GameData`] tables, and the [`Mobility`]
//! bit set. All are `Copy` and cheap to store in components.

use crate::schema::MoveFlag;

macro_rules! id_type {
    ($(#[$m:meta])* $name:ident, $t:ty) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub $t);

        impl $name {
            #[inline]
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

id_type!(
    /// Index into `GameData::resources`.
    ResourceId, u8
);
id_type!(
    /// Index into `GameData::terrain`.
    TerrainId, u8
);
id_type!(
    /// Index into `GameData::nodes`.
    NodeTypeId, u16
);
id_type!(
    /// Index into `GameData::weapons`.
    WeaponId, u16
);
id_type!(
    /// Index into `GameData::units`.
    UnitTypeId, u16
);
id_type!(
    /// Index into `GameData::buildings`.
    BuildingTypeId, u16
);
id_type!(
    /// Index into `GameData::techs`.
    TechId, u16
);
id_type!(
    /// Index into `GameData::factions`.
    FactionId, u8
);

/// Bit set of [`MoveFlag`]s: a unit's locomotion plus any faction terrain
/// traits that apply to it, or the set of flags a terrain admits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Mobility(pub u16);

impl Mobility {
    pub const NONE: Mobility = Mobility(0);

    #[inline]
    pub fn of(flag: MoveFlag) -> Mobility {
        Mobility(1 << (flag as u16))
    }
    #[inline]
    pub fn contains(self, flag: MoveFlag) -> bool {
        self.0 & Mobility::of(flag).0 != 0
    }
    #[inline]
    pub fn intersects(self, other: Mobility) -> bool {
        self.0 & other.0 != 0
    }
    #[inline]
    pub fn with(self, flag: MoveFlag) -> Mobility {
        Mobility(self.0 | Mobility::of(flag).0)
    }
    #[inline]
    pub fn union(self, other: Mobility) -> Mobility {
        Mobility(self.0 | other.0)
    }
    pub fn is_flying(self) -> bool {
        self.contains(MoveFlag::Flying)
    }
}
