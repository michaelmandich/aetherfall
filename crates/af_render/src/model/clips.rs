//! The clips the engine knows by name, and what it plays when a model does
//! not ship one.

use af_core::prelude::*;

use super::manifest::ModelManifest;

/// Every clip the engine selects (names are exact, see `docs/art/peasant.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Clip {
    Idle,
    IdleFidget,
    Walk,
    Run,
    CarryWalk,
    CarryIdle,
    Drop,
    Spawn,
    Death,
    HitReact,
    Forage,
    Farm,
    Chop,
    Mine,
    Dig,
    Chip,
    Smash,
    Build,
    Repair,
    /// Mending an ally (healers; not in the peasant's set).
    Heal,
    Attack,
    CombatIdle,
    Cheer,
    Acknowledge,
}

impl Clip {
    pub const COUNT: usize = 24;
    pub const ALL: [Clip; Clip::COUNT] = [
        Clip::Idle,
        Clip::IdleFidget,
        Clip::Walk,
        Clip::Run,
        Clip::CarryWalk,
        Clip::CarryIdle,
        Clip::Drop,
        Clip::Spawn,
        Clip::Death,
        Clip::HitReact,
        Clip::Forage,
        Clip::Farm,
        Clip::Chop,
        Clip::Mine,
        Clip::Dig,
        Clip::Chip,
        Clip::Smash,
        Clip::Build,
        Clip::Repair,
        Clip::Heal,
        Clip::Attack,
        Clip::CombatIdle,
        Clip::Cheer,
        Clip::Acknowledge,
    ];

    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }

    /// The clip's name in manifests (and, by default, in the glTF file).
    pub fn name(self) -> &'static str {
        match self {
            Clip::Idle => "Idle",
            Clip::IdleFidget => "IdleFidget",
            Clip::Walk => "Walk",
            Clip::Run => "Run",
            Clip::CarryWalk => "CarryWalk",
            Clip::CarryIdle => "CarryIdle",
            Clip::Drop => "Drop",
            Clip::Spawn => "Spawn",
            Clip::Death => "Death",
            Clip::HitReact => "HitReact",
            Clip::Forage => "Forage",
            Clip::Farm => "Farm",
            Clip::Chop => "Chop",
            Clip::Mine => "Mine",
            Clip::Dig => "Dig",
            Clip::Chip => "Chip",
            Clip::Smash => "Smash",
            Clip::Build => "Build",
            Clip::Repair => "Repair",
            Clip::Heal => "Heal",
            Clip::Attack => "Attack",
            Clip::CombatIdle => "CombatIdle",
            Clip::Cheer => "Cheer",
            Clip::Acknowledge => "Acknowledge",
        }
    }

    pub fn from_name(name: &str) -> Option<Clip> {
        Clip::ALL.into_iter().find(|c| c.name() == name)
    }

    /// Plays once (and is skipped when missing), rather than looping as a
    /// state the unit stays in.
    pub fn once(self) -> bool {
        matches!(
            self,
            Clip::IdleFidget | Clip::Drop | Clip::Spawn | Clip::Death | Clip::HitReact | Clip::Attack | Clip::Cheer | Clip::Acknowledge
        )
    }

    /// What a looping clip falls back to when the model lacks it. `None`
    /// for `Idle` (the bind pose is all that is left) and for clips that
    /// play once (they are skipped instead).
    pub fn fallback(self) -> Option<Clip> {
        match self {
            Clip::Walk | Clip::CarryIdle | Clip::CombatIdle | Clip::Heal => Some(Clip::Idle),
            Clip::Run | Clip::CarryWalk => Some(Clip::Walk),
            Clip::Forage | Clip::Farm | Clip::Chop | Clip::Mine | Clip::Dig | Clip::Chip | Clip::Smash | Clip::Build => Some(Clip::Idle),
            // Repairing is building's kneeling cousin, with the same hammer.
            Clip::Repair => Some(Clip::Build),
            _ => None,
        }
    }

    /// The work clip of a gathering style.
    pub fn work(style: WorkStyle) -> Clip {
        match style {
            WorkStyle::Forage => Clip::Forage,
            WorkStyle::Farm => Clip::Farm,
            WorkStyle::Chop => Clip::Chop,
            WorkStyle::Mine => Clip::Mine,
            WorkStyle::Dig => Clip::Dig,
            WorkStyle::Chip => Clip::Chip,
            WorkStyle::Smash => Clip::Smash,
        }
    }
}

/// Timing of a clip a model ships (from its manifest).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClipInfo {
    /// Seconds at rate 1.
    pub duration: f32,
    /// Ground speed matched at rate 1, at the model's own size (m/s).
    pub speed: Option<f32>,
    /// Seconds into the clip where the blow lands.
    pub hit: Option<f32>,
    /// How far ahead the work lands, at the model's own size (m).
    pub reach: Option<f32>,
}

/// The clips one model has, and for every engine clip which of them plays.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipTable {
    info: [Option<ClipInfo>; Clip::COUNT],
    resolved: [Option<Clip>; Clip::COUNT],
}

impl ClipTable {
    /// A table from what the model has (`info` of each clip it ships).
    pub fn new(mut info: impl FnMut(Clip) -> Option<ClipInfo>) -> ClipTable {
        let info: [Option<ClipInfo>; Clip::COUNT] = std::array::from_fn(|i| info(Clip::ALL[i]));
        let resolved = std::array::from_fn(|i| {
            let mut clip = Some(Clip::ALL[i]);
            while let Some(c) = clip {
                if info[c.index()].is_some() {
                    return Some(c);
                }
                clip = c.fallback();
            }
            None
        });
        ClipTable { info, resolved }
    }

    /// The clips a manifest lists, keeping those `available` (the glTF
    /// file has their animation).
    pub fn from_manifest(manifest: &ModelManifest, mut available: impl FnMut(Clip) -> bool) -> ClipTable {
        ClipTable::new(|clip| {
            let spec = manifest.clip(clip).filter(|_| available(clip))?;
            Some(ClipInfo { duration: spec.duration, speed: spec.speed, hit: spec.hit, reach: spec.reach })
        })
    }

    /// Does the model ship `clip` itself?
    pub fn has(&self, clip: Clip) -> bool {
        self.info[clip.index()].is_some()
    }

    pub fn info(&self, clip: Clip) -> Option<&ClipInfo> {
        self.info[clip.index()].as_ref()
    }

    /// The clip that plays for `clip`: itself, else its fallbacks in turn;
    /// `None` when nothing in the chain is there.
    pub fn resolve(&self, clip: Clip) -> Option<Clip> {
        self.resolved[clip.index()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(clips: &[Clip]) -> ClipTable {
        ClipTable::new(|c| clips.contains(&c).then_some(ClipInfo { duration: 1.0, speed: None, hit: None, reach: None }))
    }

    #[test]
    fn names_round_trip_and_the_list_is_complete() {
        for (i, clip) in Clip::ALL.into_iter().enumerate() {
            assert_eq!(clip.index(), i);
            assert_eq!(Clip::from_name(clip.name()), Some(clip));
        }
        assert_eq!(Clip::from_name("Walking"), None);
    }

    #[test]
    fn missing_clips_fall_back_as_the_contract_says() {
        let only_walk = table(&[Clip::Walk]);
        assert_eq!(only_walk.resolve(Clip::Walk), Some(Clip::Walk));
        assert_eq!(only_walk.resolve(Clip::CarryWalk), Some(Clip::Walk));
        assert_eq!(only_walk.resolve(Clip::Run), Some(Clip::Walk));
        assert_eq!(only_walk.resolve(Clip::Idle), None, "no idle: the bind pose");
        assert_eq!(only_walk.resolve(Clip::CarryIdle), None);
        assert_eq!(only_walk.resolve(Clip::Chop), None);

        let idle_too = table(&[Clip::Walk, Clip::Idle, Clip::Build]);
        assert_eq!(idle_too.resolve(Clip::CarryIdle), Some(Clip::Idle));
        assert_eq!(idle_too.resolve(Clip::CombatIdle), Some(Clip::Idle));
        for style in WorkStyle::ALL {
            assert_eq!(idle_too.resolve(Clip::work(style)), Some(Clip::Idle), "{style:?}");
        }
        assert_eq!(idle_too.resolve(Clip::Repair), Some(Clip::Build));
        assert_eq!(idle_too.resolve(Clip::Build), Some(Clip::Build));
        // Clips that play once never stand in for each other.
        for clip in Clip::ALL.into_iter().filter(|c| c.once()) {
            assert_eq!(idle_too.resolve(clip), None, "{clip:?}");
            assert_eq!(clip.fallback(), None, "{clip:?}");
        }
    }

    #[test]
    fn every_work_style_has_its_own_clip() {
        let clips: Vec<Clip> = WorkStyle::ALL.into_iter().map(Clip::work).collect();
        for (i, a) in clips.iter().enumerate() {
            assert!(!a.once());
            assert!(clips[i + 1..].iter().all(|b| a != b));
        }
    }
}
