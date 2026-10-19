//! Which clip a modelled unit plays, frame by frame. Pure logic: it turns
//! what the simulation says about a unit into "play this clip" commands
//! (`drive.rs` hands them to Bevy's `AnimationTransitions`), and only when
//! something changes.
//!
//! A unit is always in a looping *base* state ([`base_clip`]): working,
//! walking or jogging ([`gait`]; rate matched to its speed), fighting
//! between swings, carrying, or idle. One-shot clips ([`Shot`]) play over it and hand back to it when
//! they end: Attack (started with the sim's weapon windup so the blow lands
//! with the hit), HitReact, Drop, Spawn, Cheer and an occasional
//! IdleFidget. A one-shot the model lacks is skipped; a missing base clip
//! falls back along [`Clip::fallback`].

use af_core::prelude::*;

use super::clips::{Clip, ClipTable};

/// Crossfade between clips (seconds).
pub const FADE: f32 = 0.2;
/// Crossfade into an attack: short, so the windup reads.
pub const ATTACK_FADE: f32 = 0.1;
/// A unit starts walking above this ground speed (m/s) and stops below
/// [`WALK_STOP`], so collision nudges do not flicker the legs.
pub const WALK_START: f32 = 0.35;
pub const WALK_STOP: f32 = 0.15;
/// Walk playback rates are kept within this range.
pub const WALK_RATE: (f32, f32) = (0.4, 3.0);
/// A walking unit changes gait (Walk <-> Run) only when the other one plays
/// this much closer to its natural rate (in |ln rate|), so speed wobble
/// near the crossover does not flicker the legs.
pub const GAIT_SLACK: f32 = 0.2;
/// The attack clip is sped up or slowed down at most this much to land its
/// hit with the weapon's windup.
pub const ATTACK_RATE: (f32, f32) = (0.5, 2.0);
/// Idle units fidget every `FIDGET_DELAY.0 .. FIDGET_DELAY.0 + FIDGET_DELAY.1` seconds.
pub const FIDGET_DELAY: (f32, f32) = (7.0, 8.0);
/// Clip seconds of a Drop / Spawn that play even if the unit sets off at once.
pub const DROP_HOLD: f32 = 0.3;
pub const SPAWN_HOLD: f32 = 0.4;
/// A walk's rate is updated once it is this far (fraction) off.
const RATE_SLACK: f32 = 0.03;
/// Hit points lost before a hit counts (regeneration rounding).
const HURT: f32 = 0.01;

/// A clip that plays once over the base state, lowest priority first: a
/// request replaces the one playing only if it ranks higher (an attack
/// also restarts an attack).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Shot {
    Fidget,
    Cheer,
    Spawn,
    Drop,
    HitReact,
    Attack,
}

impl Shot {
    pub fn clip(self) -> Clip {
        match self {
            Shot::Fidget => Clip::IdleFidget,
            Shot::Cheer => Clip::Cheer,
            Shot::Spawn => Clip::Spawn,
            Shot::Drop => Clip::Drop,
            Shot::HitReact => Clip::HitReact,
            Shot::Attack => Clip::Attack,
        }
    }

    /// Clip seconds after which a change of base state, or walking, cuts
    /// it short: at once for idle flourishes, never for a flinch, after the
    /// blow for an attack.
    fn hold(self, duration: f32, hit: Option<f32>) -> f32 {
        match self {
            Shot::Fidget | Shot::Cheer => 0.0,
            Shot::Spawn => SPAWN_HOLD,
            Shot::Drop => DROP_HOLD,
            Shot::HitReact => f32::INFINITY,
            Shot::Attack => hit.unwrap_or(duration * 0.5),
        }
    }
}

/// The looping state a unit is in.
pub fn base_clip(activity: Activity, walking: bool, carrying: bool) -> Clip {
    match activity {
        Activity::Gather(style) => Clip::work(style),
        Activity::Build => Clip::Build,
        Activity::Repair => Clip::Repair,
        _ if walking => {
            if carrying {
                Clip::CarryWalk
            } else {
                Clip::Walk
            }
        }
        Activity::Heal => Clip::Heal,
        Activity::Fight => Clip::CombatIdle,
        Activity::Idle if carrying => Clip::CarryIdle,
        Activity::Idle => Clip::Idle,
    }
}

/// A weapon windup the simulation started.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Swing {
    /// Seconds since it started.
    pub elapsed: f32,
    /// Its full length: the blow lands when it is over.
    pub windup: f32,
}

/// What the simulation says about a unit this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inputs {
    pub activity: Activity,
    /// Ground speed (m/s).
    pub speed: f32,
    /// The load held (none when empty).
    pub carrying: Option<ResourceId>,
    pub hp: f32,
    /// In-game metres per model metre (the model's display scale).
    pub scale: f32,
    /// A windup started since the last frame.
    pub swing: Option<Swing>,
    /// Just trained.
    pub spawned: bool,
    /// Its owner just reached a new age.
    pub cheer: bool,
    /// Seconds since the last frame (game time).
    pub dt: f32,
}

/// What to do with the unit's animation player.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Keep,
    /// Crossfade to `clip` over `fade` seconds, at `rate`, from `seek`
    /// seconds into it; `once` clips play once and hold their last pose.
    Play { clip: Clip, once: bool, rate: f32, seek: f32, fade: f32 },
    /// Change the playing clip's rate.
    Rate(f32),
    /// Stop everything: the model has no clip for this, not even Idle.
    Rest,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Playing {
    shot: Shot,
    /// Clip seconds played.
    time: f32,
    rate: f32,
    duration: f32,
    hold: f32,
    /// The base state it started over.
    base: Clip,
    /// The load being put down (Drop).
    load: Option<ResourceId>,
}

/// One unit's animation state.
#[derive(Clone, Debug, PartialEq)]
pub struct Animator {
    /// The clip the player plays (`None`: nothing yet, or the bind pose).
    main: Option<Clip>,
    rate: f32,
    walking: bool,
    /// Gait while walking without a load: Walk or Run (see [`gait`]).
    gait: Clip,
    base: Clip,
    shot: Option<Playing>,
    last_hp: Option<f32>,
    last_load: Option<ResourceId>,
    fidget_in: f32,
    seed: u32,
}

impl Animator {
    /// `seed` (any number: an entity index will do) varies the idle
    /// fidgets between units.
    pub fn new(seed: u32) -> Animator {
        // Scramble it (murmur3's finaliser), so nearby seeds diverge at once.
        let mut seed = seed ^ 0x9e37_79b9;
        seed = (seed ^ (seed >> 16)).wrapping_mul(0x85eb_ca6b);
        seed = (seed ^ (seed >> 13)).wrapping_mul(0xc2b2_ae35);
        seed ^= seed >> 16;
        let mut a = Animator {
            main: None,
            rate: 1.0,
            walking: false,
            gait: Clip::Walk,
            base: Clip::Idle,
            shot: None,
            last_hp: None,
            last_load: None,
            fidget_in: 0.0,
            seed: seed | 1,
        };
        a.fidget_in = a.fidget_delay();
        a
    }

    /// The one-shot playing, if any.
    pub fn shot(&self) -> Option<Shot> {
        self.shot.map(|p| p.shot)
    }

    /// The clip the player is playing.
    pub fn main(&self) -> Option<Clip> {
        self.main
    }

    /// The looping state the unit is in, as asked for (before falling back
    /// for a clip the model lacks).
    pub fn base(&self) -> Clip {
        self.base
    }

    /// The load to show in the hands: the one carried, or the one being
    /// put down during the first half of a Drop.
    pub fn shown_load(&self, carrying: Option<ResourceId>) -> Option<ResourceId> {
        carrying.or(self.shot.filter(|p| p.shot == Shot::Drop && p.time < p.duration * 0.5).and_then(|p| p.load))
    }

    /// Advance one frame.
    pub fn step(&mut self, f: &Inputs, clips: &ClipTable) -> Command {
        self.walking = if self.walking { f.speed > WALK_STOP } else { f.speed > WALK_START };
        let mut base = base_clip(f.activity, self.walking, f.carrying.is_some());
        if base == Clip::Walk {
            self.gait = gait(self.gait, f.speed, f.scale, clips);
            base = self.gait;
        }
        if base != Clip::Idle && self.base == Clip::Idle {
            self.fidget_in = self.fidget_delay();
        }
        self.base = base;

        // What the frame asks for, highest priority wins.
        let mut request: Option<(Shot, f32, f32)> = None;
        let mut ask = |shot: Shot, rate: f32, seek: f32| {
            if request.is_none_or(|(s, _, _)| shot > s) {
                request = Some((shot, rate, seek));
            }
        };
        if let Some(swing) = f.swing {
            let rate = clips
                .info(Clip::Attack)
                .and_then(|i| i.hit)
                .filter(|_| swing.windup > 0.0)
                .map_or(1.0, |hit| (hit / swing.windup).clamp(ATTACK_RATE.0, ATTACK_RATE.1));
            ask(Shot::Attack, rate, swing.elapsed.max(0.0) * rate);
        }
        let calm = matches!(base, Clip::Idle | Clip::Walk | Clip::Run | Clip::CarryIdle | Clip::CarryWalk);
        if calm && self.last_hp.is_some_and(|hp| f.hp < hp - HURT) {
            ask(Shot::HitReact, 1.0, 0.0);
        }
        let dropped = self.last_load.filter(|_| f.carrying.is_none());
        if dropped.is_some() {
            ask(Shot::Drop, 1.0, 0.0);
        }
        if f.spawned {
            ask(Shot::Spawn, 1.0, 0.0);
        }
        if f.cheer && base == Clip::Idle {
            ask(Shot::Cheer, 1.0, 0.0);
        }
        if base == Clip::Idle && self.shot.is_none() {
            self.fidget_in -= f.dt;
            if self.fidget_in <= 0.0 {
                self.fidget_in = self.fidget_delay();
                ask(Shot::Fidget, 1.0, 0.0);
            }
        }
        self.last_hp = Some(f.hp);
        self.last_load = f.carrying;

        // The one-shot playing runs out, or gives way to a new state.
        let mut ended = false;
        if let Some(p) = &mut self.shot {
            p.time += f.dt * p.rate;
            let moved_on = base != p.base || matches!(base, Clip::Walk | Clip::Run | Clip::CarryWalk);
            if p.time >= p.duration || (moved_on && p.time >= p.hold) {
                self.shot = None;
                ended = true;
            }
        }

        if let Some((shot, rate, seek)) = request {
            let clip = shot.clip();
            let outranks = self.shot.is_none_or(|p| shot > p.shot || (shot == Shot::Attack && p.shot == Shot::Attack));
            if let Some(info) = clips.info(clip).filter(|_| outranks) {
                let seek = seek.min(info.duration);
                let load = if shot == Shot::Drop { dropped } else { None };
                self.shot = Some(Playing { shot, time: seek, rate, duration: info.duration, hold: shot.hold(info.duration, info.hit), base, load });
                self.main = Some(clip);
                self.rate = rate;
                let fade = if shot == Shot::Attack { ATTACK_FADE } else { FADE };
                return Command::Play { clip, once: true, rate, seek, fade };
            }
        }
        if self.shot.is_some() {
            return Command::Keep;
        }

        let Some(clip) = clips.resolve(base) else {
            return if self.main.take().is_some() { Command::Rest } else { Command::Keep };
        };
        let rate = base_rate(clip, f.speed, f.scale, clips);
        if ended || self.main != Some(clip) {
            self.main = Some(clip);
            self.rate = rate;
            return Command::Play { clip, once: false, rate, seek: 0.0, fade: FADE };
        }
        if (rate - self.rate).abs() > RATE_SLACK * self.rate {
            self.rate = rate;
            return Command::Rate(rate);
        }
        Command::Keep
    }

    /// Seconds to the next fidget (xorshift on the unit's seed).
    fn fidget_delay(&mut self) -> f32 {
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        FIDGET_DELAY.0 + FIDGET_DELAY.1 * (x >> 8) as f32 / (1u32 << 24) as f32
    }
}

/// The gait of a unit walking without a load: Walk or Run, whichever plays
/// closest to its natural rate at this ground speed once each is matched to
/// it (so the feet still hold the ground either way). A fast walker jogs
/// instead of scurrying: the peasant's 2.4 m/s needs Walk at 2.3x but Run
/// at 0.73x. Keeps `current` unless the other is better by [`GAIT_SLACK`];
/// Walk when the model lacks either clip or its speed.
pub fn gait(current: Clip, speed: f32, scale: f32, clips: &ClipTable) -> Clip {
    let off = |c: Clip| {
        clips
            .info(c)
            .and_then(|i| i.speed)
            .map(|s| s * scale)
            .filter(|s| *s > 0.0 && speed > 0.0)
            .map(|s| (speed / s).ln().abs())
    };
    let (Some(walk), Some(run)) = (off(Clip::Walk), off(Clip::Run)) else { return Clip::Walk };
    match current {
        Clip::Run if walk + GAIT_SLACK < run => Clip::Walk,
        Clip::Run => Clip::Run,
        _ if run + GAIT_SLACK < walk => Clip::Run,
        _ => Clip::Walk,
    }
}

/// Playback rate of a looping clip: walk cycles match the ground speed
/// (`speed` m/s against the clip's own speed at the display `scale`),
/// everything else plays at 1.
pub fn base_rate(clip: Clip, speed: f32, scale: f32, clips: &ClipTable) -> f32 {
    if !matches!(clip, Clip::Walk | Clip::CarryWalk | Clip::Run) {
        return 1.0;
    }
    match clips.info(clip).and_then(|i| i.speed).map(|s| s * scale).filter(|s| *s > 0.0) {
        Some(matched) => (speed / matched).clamp(WALK_RATE.0, WALK_RATE.1),
        None => 1.0,
    }
}

/// Seconds a corpse lies still before it sinks.
pub const CORPSE_HOLD: f32 = 4.0;
/// Seconds it takes to sink out of sight.
pub const CORPSE_SINK: f32 = 2.0;

/// How far below its resting place a corpse `age` seconds old is drawn
/// (sinking `depth` metres in all), or `None` once it is gone.
pub fn corpse_drop(age: f32, depth: f32) -> Option<f32> {
    let sinking = age - CORPSE_HOLD;
    if sinking >= CORPSE_SINK {
        return None;
    }
    Some(depth * (sinking / CORPSE_SINK).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::clips::ClipInfo;

    const DT: f32 = 1.0 / 60.0;
    const WOOD: Option<ResourceId> = Some(ResourceId(1));

    fn info(duration: f32) -> ClipInfo {
        ClipInfo { duration, speed: None, hit: None, reach: None }
    }

    /// Everything the peasant contract lists, with its timings.
    fn full() -> ClipTable {
        ClipTable::new(|c| {
            Some(match c {
                Clip::Walk => ClipInfo { duration: 1.08, speed: Some(1.4), ..info(1.08) },
                Clip::CarryWalk => ClipInfo { duration: 1.1, speed: Some(1.2), ..info(1.1) },
                Clip::Attack => ClipInfo { duration: 1.0, hit: Some(0.5), ..info(1.0) },
                Clip::Drop => info(0.7),
                Clip::HitReact => info(0.4),
                Clip::Spawn => info(1.0),
                Clip::Heal | Clip::Run => return None,
                _ => info(2.0),
            })
        })
    }

    fn only(clips: &[Clip]) -> ClipTable {
        ClipTable::new(|c| clips.contains(&c).then(|| if c == Clip::Walk { ClipInfo { speed: Some(1.4), ..info(1.08) } } else { info(1.0) }))
    }

    fn idle() -> Inputs {
        Inputs { activity: Activity::Idle, speed: 0.0, carrying: None, hp: 30.0, scale: 1.2 / 1.65, swing: None, spawned: false, cheer: false, dt: DT }
    }

    fn played(c: Command) -> Option<Clip> {
        match c {
            Command::Play { clip, .. } => Some(clip),
            _ => None,
        }
    }

    /// Steps `n` frames of `f`, returning the commands that were not Keep.
    fn run(a: &mut Animator, f: Inputs, n: usize, clips: &ClipTable) -> Vec<Command> {
        (0..n).map(|_| a.step(&f, clips)).filter(|c| *c != Command::Keep).collect()
    }

    #[test]
    fn the_base_state_follows_the_task_then_movement_then_cargo() {
        assert_eq!(base_clip(Activity::Gather(WorkStyle::Chop), false, false), Clip::Chop);
        assert_eq!(base_clip(Activity::Gather(WorkStyle::Farm), true, true), Clip::Farm, "work wins over a nudge");
        assert_eq!(base_clip(Activity::Build, false, false), Clip::Build);
        assert_eq!(base_clip(Activity::Repair, false, false), Clip::Repair);
        assert_eq!(base_clip(Activity::Idle, true, false), Clip::Walk);
        assert_eq!(base_clip(Activity::Idle, true, true), Clip::CarryWalk);
        assert_eq!(base_clip(Activity::Fight, true, false), Clip::Walk, "chasing");
        assert_eq!(base_clip(Activity::Fight, false, false), Clip::CombatIdle);
        assert_eq!(base_clip(Activity::Heal, false, false), Clip::Heal);
        assert_eq!(base_clip(Activity::Idle, false, true), Clip::CarryIdle);
        assert_eq!(base_clip(Activity::Idle, false, false), Clip::Idle);
    }

    #[test]
    fn clips_change_only_on_state_changes() {
        let clips = full();
        let mut a = Animator::new(1);
        let first = a.step(&idle(), &clips);
        assert_eq!(first, Command::Play { clip: Clip::Idle, once: false, rate: 1.0, seek: 0.0, fade: FADE });
        assert!(run(&mut a, idle(), 60, &clips).is_empty(), "a second of idling changes nothing");
        let chop = Inputs { activity: Activity::Gather(WorkStyle::Chop), ..idle() };
        assert_eq!(played(a.step(&chop, &clips)), Some(Clip::Chop));
        assert!(run(&mut a, chop, 600, &clips).is_empty(), "no fidgeting at work");
    }

    #[test]
    fn walking_matches_the_ground_speed_with_hysteresis() {
        let clips = full();
        let mut a = Animator::new(1);
        a.step(&idle(), &clips);
        // A collision nudge is no walk.
        assert!(run(&mut a, Inputs { speed: 0.3, ..idle() }, 5, &clips).is_empty());
        let walk = Inputs { speed: 2.4, ..idle() };
        let Command::Play { clip: Clip::Walk, rate, .. } = a.step(&walk, &clips) else { panic!("walks") };
        // 1.4 m/s at 1.65 m is 1.02 m/s at 1.2 m: 2.4 m/s needs 2.36x.
        assert!((rate - 2.4 / (1.4 * 1.2 / 1.65)).abs() < 1e-4, "{rate}");
        // Slowing a little is a rate change, not a new clip.
        let Command::Rate(r) = a.step(&Inputs { speed: 1.8, ..idle() }, &clips) else { panic!("rate") };
        assert!(r < rate);
        assert_eq!(a.step(&Inputs { speed: 1.81, ..idle() }, &clips), Command::Keep, "within slack");
        // Easing to a crawl still walks; stopping below the threshold idles.
        assert!(!matches!(a.step(&Inputs { speed: 0.2, ..idle() }, &clips), Command::Play { .. }));
        assert_eq!(played(a.step(&Inputs { speed: 0.1, ..idle() }, &clips)), Some(Clip::Idle));
        // Carrying walks with the load.
        assert_eq!(played(a.step(&Inputs { speed: 2.4, carrying: WOOD, ..idle() }, &clips)), Some(Clip::CarryWalk));
        assert_eq!(base_rate(Clip::Walk, 50.0, 1.0, &clips), WALK_RATE.1, "clamped");
        assert_eq!(base_rate(Clip::Idle, 2.0, 1.0, &clips), 1.0);
    }

    /// The peasant manifest's locomotion speeds (m/s at model scale 1).
    fn with_run() -> ClipTable {
        ClipTable::new(|c| {
            Some(match c {
                Clip::Walk => ClipInfo { speed: Some(1.429), ..info(1.0333) },
                Clip::Run => ClipInfo { speed: Some(4.549), ..info(0.6333) },
                Clip::CarryWalk => ClipInfo { speed: Some(1.349), ..info(1.1) },
                Clip::Heal => return None,
                _ => info(2.0),
            })
        })
    }

    #[test]
    fn fast_walkers_jog_with_the_run_clip_slowed_to_their_speed() {
        let clips = with_run();
        let scale = 1.2 / 1.65;
        // The peasant's 2.4 m/s: Run at ~0.73x, feet matched to the ground.
        assert_eq!(gait(Clip::Walk, 2.4, scale, &clips), Clip::Run);
        let mut a = Animator::new(1);
        a.step(&idle(), &clips);
        let Command::Play { clip: Clip::Run, rate, .. } = a.step(&Inputs { speed: 2.4, ..idle() }, &clips) else { panic!("jogs") };
        assert!((rate - 2.4 / (4.549 * scale)).abs() < 1e-4 && (rate - 0.73).abs() < 0.01, "{rate}");
        // Slowed on marsh to 1.3 m/s: a walk at about 1.25x reads better.
        assert_eq!(played(a.step(&Inputs { speed: 1.3, ..idle() }, &clips)), Some(Clip::Walk));
        // Near the crossover (~1.86 m/s) the gait holds: no flicker.
        assert_eq!(gait(Clip::Walk, 1.95, scale, &clips), Clip::Walk);
        assert_eq!(gait(Clip::Run, 1.75, scale, &clips), Clip::Run);
        // A load is carried at a walk.
        assert_eq!(played(a.step(&Inputs { speed: 2.4, carrying: WOOD, ..idle() }, &clips)), Some(Clip::CarryWalk));
        // A model without a Run clip keeps walking.
        assert_eq!(gait(Clip::Walk, 2.4, scale, &full()), Clip::Walk);
    }

    #[test]
    fn the_attack_lands_with_the_windup_and_ends_in_combat_idle() {
        let clips = full();
        let mut a = Animator::new(1);
        let fight = Inputs { activity: Activity::Fight, ..idle() };
        assert_eq!(played(a.step(&fight, &clips)), Some(Clip::CombatIdle));
        // Noticed 0.1 s into a 0.5 s windup: the clip's 0.5 s hit is 0.4 s away.
        let swing = Inputs { swing: Some(Swing { elapsed: 0.1, windup: 0.5 }), ..fight };
        assert_eq!(a.step(&swing, &clips), Command::Play { clip: Clip::Attack, once: true, rate: 1.0, seek: 0.1, fade: ATTACK_FADE });
        assert_eq!(a.shot(), Some(Shot::Attack));
        // A longer windup slows the clip so the blow still lands on time.
        let mut b = Animator::new(1);
        b.step(&fight, &clips);
        let Command::Play { rate, seek, .. } = b.step(&Inputs { swing: Some(Swing { elapsed: 0.2, windup: 0.8 }), ..fight }, &clips) else { panic!() };
        assert!((rate - 0.625).abs() < 1e-5 && (seek - 0.125).abs() < 1e-5, "{rate} {seek}");
        // The rest of the 1 s clip plays, then back to the guard.
        let cmds = run(&mut a, fight, 60, &clips);
        assert_eq!(cmds.iter().filter_map(|c| played(*c)).collect::<Vec<_>>(), [Clip::CombatIdle]);
        assert_eq!(a.shot(), None);
        // A new windup restarts the attack.
        assert_eq!(played(a.step(&swing, &clips)), Some(Clip::Attack));
        assert_eq!(played(a.step(&swing, &clips)), Some(Clip::Attack), "restarts");
    }

    #[test]
    fn an_attack_gives_way_to_a_new_state_only_after_the_blow() {
        let clips = full();
        let mut a = Animator::new(1);
        let fight = Inputs { activity: Activity::Fight, ..idle() };
        a.step(&fight, &clips);
        a.step(&Inputs { swing: Some(Swing { elapsed: 0.0, windup: 0.5 }), ..fight }, &clips);
        // The foe died: idle now, but the swing goes on until the hit.
        let mut until = 0.0;
        while a.shot() == Some(Shot::Attack) {
            a.step(&idle(), &clips);
            until += DT;
        }
        assert!((until - 0.5).abs() < 2.0 * DT, "cut at the hit: {until}");
        assert_eq!(a.main(), Some(Clip::Idle));
    }

    #[test]
    fn hits_flinch_only_a_unit_that_is_not_busy() {
        let clips = full();
        let mut a = Animator::new(1);
        a.step(&idle(), &clips);
        assert_eq!(played(a.step(&Inputs { hp: 27.0, ..idle() }, &clips)), Some(Clip::HitReact));
        // The flinch plays out (0.4 s) and is not restarted by more hits.
        let hurt = Inputs { hp: 20.0, ..idle() };
        let cmds = run(&mut a, hurt, 30, &clips);
        assert_eq!(cmds.iter().filter_map(|c| played(*c)).collect::<Vec<_>>(), [Clip::Idle]);

        let mut worker = Animator::new(1);
        let chop = Inputs { activity: Activity::Gather(WorkStyle::Chop), ..idle() };
        worker.step(&chop, &clips);
        assert_eq!(worker.step(&Inputs { hp: 10.0, ..chop }, &clips), Command::Keep, "busy chopping");
        let mut fighter = Animator::new(1);
        let fight = Inputs { activity: Activity::Fight, ..idle() };
        fighter.step(&fight, &clips);
        assert_eq!(fighter.step(&Inputs { hp: 10.0, ..fight }, &clips), Command::Keep, "busy fighting");
    }

    #[test]
    fn a_deposit_drops_the_load_and_shows_it_leaving_the_hands() {
        let clips = full();
        let mut a = Animator::new(1);
        let carry = Inputs { carrying: WOOD, speed: 2.4, ..idle() };
        a.step(&carry, &clips);
        assert_eq!(a.shown_load(WOOD), WOOD);
        // Deposited on arrival, still moving: the drop plays its first 0.3 s.
        let walk = Inputs { speed: 2.4, ..idle() };
        assert_eq!(played(a.step(&walk, &clips)), Some(Clip::Drop));
        assert_eq!(a.shown_load(None), WOOD, "the load is still in the hands");
        let cmds = run(&mut a, walk, 30, &clips);
        assert_eq!(cmds.iter().filter_map(|c| played(*c)).collect::<Vec<_>>(), [Clip::Walk]);
        assert_eq!(a.shown_load(None), None);
        // Standing still, it plays out in full.
        let mut b = Animator::new(1);
        b.step(&Inputs { carrying: WOOD, ..idle() }, &clips);
        assert_eq!(played(b.step(&idle(), &clips)), Some(Clip::Drop));
        let frames = run(&mut b, idle(), 60, &clips);
        assert_eq!(frames.iter().filter_map(|c| played(*c)).collect::<Vec<_>>(), [Clip::Idle]);
    }

    #[test]
    fn spawn_cheer_and_fidget_play_once() {
        let clips = full();
        let mut a = Animator::new(7);
        assert_eq!(played(a.step(&Inputs { spawned: true, ..idle() }, &clips)), Some(Clip::Spawn));
        run(&mut a, idle(), 70, &clips);
        assert_eq!(a.main(), Some(Clip::Idle));
        assert_eq!(played(a.step(&Inputs { cheer: true, ..idle() }, &clips)), Some(Clip::Cheer));
        // Cheering is for idle units only.
        let mut busy = Animator::new(7);
        let chop = Inputs { activity: Activity::Gather(WorkStyle::Chop), ..idle() };
        busy.step(&chop, &clips);
        assert_eq!(busy.step(&Inputs { cheer: true, ..chop }, &clips), Command::Keep);
        // Idle long enough, everyone fidgets now and then.
        let mut b = Animator::new(3);
        let fidgets = run(&mut b, idle(), 60 * 40, &clips).into_iter().filter(|c| played(*c) == Some(Clip::IdleFidget)).count();
        assert!((2..=6).contains(&fidgets), "{fidgets} fidgets in 40 s");
        // Units fidget out of step.
        let mut c = Animator::new(4);
        let first = |x: &mut Animator| (0..60 * 20).position(|_| played(x.step(&idle(), &clips)) == Some(Clip::IdleFidget));
        assert_ne!(first(&mut Animator::new(3)), first(&mut c));
    }

    #[test]
    fn missing_clips_fall_back_and_missing_one_shots_are_skipped() {
        // The raw Meshy model: a walk and nothing else.
        let clips = only(&[Clip::Walk]);
        let mut a = Animator::new(1);
        assert_eq!(a.step(&idle(), &clips), Command::Keep, "no idle at all: bind pose");
        assert_eq!(played(a.step(&Inputs { speed: 2.4, carrying: WOOD, ..idle() }, &clips)), Some(Clip::Walk), "CarryWalk -> Walk");
        assert_eq!(a.step(&Inputs { speed: 0.0, ..idle() }, &clips), Command::Rest, "back to the bind pose");
        assert_eq!(a.step(&Inputs { hp: 1.0, swing: Some(Swing { elapsed: 0.0, windup: 0.5 }), spawned: true, ..idle() }, &clips), Command::Keep);

        let clips = only(&[Clip::Walk, Clip::Idle]);
        let mut b = Animator::new(1);
        assert_eq!(played(b.step(&idle(), &clips)), Some(Clip::Idle));
        for (activity, carrying) in [(Activity::Gather(WorkStyle::Smash), None), (Activity::Repair, None), (Activity::Fight, None), (Activity::Idle, WOOD)] {
            assert_eq!(b.step(&Inputs { activity, carrying, ..idle() }, &clips), Command::Keep, "{activity:?}: still Idle");
        }
    }

    #[test]
    fn corpses_lie_still_then_sink_away() {
        assert_eq!(corpse_drop(0.0, 1.2), Some(0.0));
        assert_eq!(corpse_drop(CORPSE_HOLD, 1.2), Some(0.0));
        let mid = corpse_drop(CORPSE_HOLD + CORPSE_SINK * 0.5, 1.2).unwrap();
        assert!((mid - 0.6).abs() < 1e-5);
        assert_eq!(corpse_drop(CORPSE_HOLD + CORPSE_SINK, 1.2), None);
    }
}
