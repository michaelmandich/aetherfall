//! No-overlap resolution (`SimSet::Movement`, after steering), the core rule
//! of the game: boxes never overlap.
//!
//! Each tick, from the positions steering produced, a few Gauss-Seidel
//! iterations of:
//! 1. ground unit vs ground unit: SAT push-out split by "resistance"
//!    (footprint area x state: idle units, and units fighting in place when
//!    an ally walks into them, yield to walkers and step aside off their
//!    line; units standing on Hold or StandGround resist), over the
//!    candidate pairs of [`Contacts`]. A unit an obstacle or the terrain has
//!    pushed back this tick is braced against it: a push driving it back
//!    into that wall meets many times its resistance, so a crowd squeezing
//!    past it makes room among itself instead of shoving it into the wall
//!    over and over, while a push along the wall moves it as usual (an idle
//!    rider lying across a corridor still gets walked along it). Units step
//!    aside only in the first pass, which meets this tick's shoves; later
//!    passes only push out, so a dense crowd cannot amplify the sidesteps
//!    into big jumps;
//!
//!    Of two walkers meeting head-on (any owners), the younger *gives way*
//!    for the tick, unless the other gave way lately (within
//!    [`GIVE_WAY_MEMORY`]): then that one gives way again, whatever their
//!    ages, so a file that gave way to a column goes on giving way to the
//!    rest of it instead of the right of way flipping with every new pair
//!    that meets. Every walker following close behind one that gives way,
//!    in its lane, gives way too: they yield to the walker they give way to
//!    and step aside, and each yields to the one ahead of it in its file as
//!    that one is backed into it, so the walker walks the whole file back
//!    out of a gap one unit wide (between two farms, two trees) instead of
//!    the pair, or a pair of columns, deadlocking in it, or the front of the
//!    file being squeezed between the walker and its own followers. A unit
//!    giving way that is braced and in the walker's lane (the gap is too
//!    narrow to pass beside it) is pushed straight back along the walker's
//!    heading, a little per pass, rather than squeezed sideways into a jam
//!    past it, even once the two stand side by side in the gap; one only
//!    grazing it from the next lane of a wider passage is pushed across,
//!    into its own lane, and the two pass. Likewise two walkers going the
//!    same way that reach a gap abreast, too narrow for both (each pinned
//!    against a wall on its side), are parted along their way into single
//!    file, and any two units squeezed between two walls (each pushed back
//!    by its own) slip apart along another axis instead of being pushed into
//!    the walls and straight back every pass;
//! 2. ground unit vs static obstacles (buildings, foundations, blocking
//!    nodes): full push-out; a gate never blocks its own team. A walker
//!    whose push-out would shove it straight back against its heading, while
//!    a short sideways step clears the box, takes the step instead: it slips
//!    round the corner its path line grazes;
//! 3. ground unit vs impassable terrain cells and the map edge: full
//!    push-out, with the same slip round a corner of the terrain (the step
//!    leads onto open ground, not along the face of a cliff): two units
//!    reaching the mouth of a gap in a cliff abreast, each half in it, slip
//!    in one after the other instead of each being held back by a corner of
//!    the gap for good.
//!
//! A walker pressed against a walker ahead of it going its way (ahead along
//! both their headings, so no two wait on each other) that moves on (more
//! than [`QUEUE_FLOW`] of its steps go through, or it waits so itself) is
//! *waiting its turn*: `track_progress` does not count it as stuck
//! (`Mover::waiting`). A crowd queueing at a gap keeps its orders however
//! long the queue takes, instead of giving up within seconds and standing
//! idle in and around the gap for good; a queue whose front is stuck is
//! stuck too, and gives up as before. And a walker touching an arrived
//! groupmate counts as arrived (crowd arrival) only outside a passage
//! ([`in_passage`]), or with its goal in one: on its way through a gap to a
//! point past it, it would stop in the gap the rest of the crowd still
//! needs.
//!
//! Then the final word on obstacles: a ground unit's centre never ends on
//! terrain it cannot enter, and a unit never ends wedged into a static
//! obstacle (pushed out of one box into another, in a slot narrower than
//! itself); it slides back along x or z, else stays where (and as) the tick
//! started. A unit that started the tick inside an obstacle it cannot be
//! pushed out of (a foundation placed over it in a tight spot) is set down
//! on the nearest open ground clear of other units instead.
//!
//! And last the final word on pairs, which makes the rule hold at the end
//! of every tick: the passes stop at a cap, and a crowd jammed against a
//! wall, a unit the final word on obstacles set back into another, or a
//! long box turned in a tight spot can leave a pair overlapping. Each pair
//! still deeper than [`FINAL_TOLERANCE`] is parted by [`Parter`] without
//! driving any other pair past it or any unit into an obstacle, and
//! without a jump the player would see where a gentler cure exists: a
//! pair that began the tick apart by undoing a turn, a short push, or
//! sending a unit back toward where it began; a pair that began it
//! overlapping (trained onto one another, say) by any push that parts it,
//! else by easing it apart a little each tick, and only after some ticks
//! by setting a unit down on a free spot, nearby first. A pair nothing
//! parts has a unit held back toward where it started the tick, no farther
//! than needed, so it ends no deeper than it began (see
//! [`Parter::hold_back`]); one even that cannot bring back (shoved by a
//! unit pushed out of a new foundation) has a unit set down nearby.
//! Flyers only push each other, softly, and stay inside the map.

use af_core::ext::movement::CollisionStats;
use af_core::prelude::af_geom::Frame;
use af_core::prelude::*;
use bevy::prelude::*;

use crate::bodies::BodyGrid;
use crate::mover::{is_moving, Mover};
use crate::obstacles::ObstacleRegistry;

/// Solver passes per tick: at least the minimum, then more while some pair
/// still overlaps deeper than the tolerance (dense crowds), up to the cap.
const MIN_ITERATIONS: usize = 3;
const MAX_ITERATIONS: usize = 24;
const OVERLAP_TOLERANCE: f32 = 0.01;
/// Cell size of the body grid that finds candidate pairs (metres): about a
/// unit's width, so a cell holds few bodies and a body spans few cells.
const GRID_CELL: f32 = 1.5;
/// Bodies whose bounds come this close are candidate pairs (metres). The
/// list holds until some body has been pushed half this far.
const CONTACT_MARGIN: f32 = 0.3;
/// Extra separation added to every push so touching boxes do not re-overlap
/// through rounding (metres).
const SKIN: f32 = 0.002;
/// Resistance factor of a unit standing idle (yields to movers, even to a
/// worker wading through a blob of idle cavalry several times its size).
const IDLE_RESIST: f32 = 0.1;
/// Resistance factor of a walker giving way (see the module docs).
const GIVE_WAY_RESIST: f32 = 0.1;
/// Ticks a walker that gave way goes on giving way to walkers it meets
/// head-on, whatever their ages (see the module docs).
const GIVE_WAY_MEMORY: u8 = 60;
/// A walker that got no more than this share of its steps through lately
/// (`Mover::flow`), and does not wait its turn itself, is stuck: the queue
/// behind it is not waiting (see the module docs).
const QUEUE_FLOW: f32 = 0.2;
/// Walkers whose headings agree at least this much (cosine) walk in file:
/// one following close behind a unit that gives way gives way too.
const IN_FILE: f32 = 0.5;
/// A unit this much of the two half-widths to one side of another walks in
/// its file (in its lane), farther in the next lane.
const SAME_LANE: f32 = 0.75;
/// Most units down a file that give way behind the one met head-on.
const MAX_FILE: usize = 8;
/// Two wall pushes this alike (cosine) count as one wall.
const SAME_WALL: f32 = 0.9;
/// A push this much against a wall's push (cosine) drives a unit back into it.
const INTO_WALL: f32 = 0.3;
/// Two walkers pinned abreast fall into single file only while neither is
/// in more candidate pairs than this (see `fall_in_line`).
const MAX_FILE_CONTACTS: u8 = 3;
/// ...and only while both are at most this wide (half-width, metres): riders,
/// infantry and workers, not giants and engines.
const FILE_HALF_WIDTH: f32 = 0.5;
/// A braced unit giving way is pushed back along the other's heading by up
/// to this much per solver pass (metres).
const BACK_OUT: f32 = 0.1;
/// Resistance factor of a unit standing on Hold or StandGround.
const HOLD_RESIST: f32 = 20.0;
/// An idle unit shoved by a walker also steps sideways off the walker's
/// line by this fraction of the shove (instead of being bulldozed ahead).
/// First solver pass only.
const SIDESTEP: f32 = 1.0;
/// Fraction of an overlap flyers resolve per iteration.
const FLYER_SOFTNESS: f32 = 0.3;
/// Walkers whose headings meet at least this head-on (cosine) give way.
const HEAD_ON: f32 = -0.5;
/// Deepest overlap with a static obstacle a unit may end its tick in when
/// a spot without one is at hand (metres).
const WEDGE_TOLERANCE: f32 = 0.05;
/// A walker pushed back against its heading by an obstacle slips sideways
/// round it instead when a step this short clears it (metres).
const CORNER_SLIP: f32 = 0.35;
/// How far a unit trapped inside an obstacle may be set down outside it (metres).
const EJECT_RADIUS: f32 = 8.0;
/// A push-out within this angle of straight back (cosine) opposes the heading;
/// an axis this far from the heading's line counts as sideways.
const HEAD_ON_PUSH: f32 = 0.5;
/// Groupmates whose point goals are this close share a crowd (metres).
const CROWD_GOAL_TOLERANCE: f32 = 1.0;
/// A spot with ground a unit cannot use within this many nav cells on
/// either side of it (across x or across z) is in a passage.
const PASSAGE_CELLS: i32 = 2;
/// Deepest overlap two ground units may keep at the end of a tick (metres).
/// The solver passes nearly always get there; the few pairs they leave
/// deeper (a crowd squeezed against a wall past what the passes untangle, a
/// unit the final word on obstacles set back into another, a turn in a
/// tight spot) are parted by [`Parter`]. Combat's turns in place never cut
/// deeper than this either, so no tick ends with boxes clipping visibly.
const FINAL_TOLERANCE: f32 = 0.05;
/// Rounds of that parting; each parts every pair still too deep.
const FINAL_ROUNDS: usize = 4;
/// Deepest overlap along another axis than the push-out's that the parting
/// slides a pair apart along (metres; a pair deeper than that along every
/// axis is set down apart instead).
const MAX_SLIDE: f32 = 1.5;
/// Two axes this parallel (|cosine|) count as one.
const SAME_AXIS: f32 = 0.999;
/// Farthest a unit is set down to part a pair nothing nearer parts (metres):
/// the last resort, a jump the player sees, so a short one. Only a unit
/// that has begun [`SET_DOWN_PATIENCE`] ticks in a row overlapping another
/// gets it; one with no spot of its own to go back to (just trained, set
/// down out of a new foundation), or overlapped for [`STRANDED_TICKS`],
/// looks as far as [`EJECT_RADIUS`].
const SET_DOWN_RADIUS: f32 = 1.5;
/// Ticks in a row a unit must have begun overlapping another deeper than
/// [`FINAL_TOLERANCE`] before it may be set down nearby (before then the
/// pair is eased apart a little each tick where there is room).
const SET_DOWN_PATIENCE: u16 = 3;
/// Ticks in a row of such an overlap after which the unit may be set down
/// as far as [`EJECT_RADIUS`]: better a jump than boxes clipping for good.
const STRANDED_TICKS: u16 = 30;
/// Farthest the final word pushes a unit of a pair that began the tick
/// apart, from where the passes left it, in all (metres; about a tick of
/// the fastest walk): deeper ones are parted by holding a unit back, which
/// nobody sees, rather than by a shove they do.
const FRESH_PUSH: f32 = 0.35;
/// Fractions of the push-out tried when easing apart a pair that began the
/// tick overlapping and has no room to part all at once.
const EASE_FRACTIONS: [f32; 2] = [0.5, 0.25];
/// Fractions of the way back to where it started the tick that a unit is
/// held back by, least first (see [`Parter::hold_back`]).
const HOLD_STEPS: [f32; 4] = [0.25, 0.5, 0.75, 1.0];
/// Fractions of the way back a unit is sent to part a pair it walked into
/// this tick, least first.
const BACK_OFF_STEPS: [f32; 2] = [0.5, 1.0];
/// Spacing of the spots tried all round for that (metres)...
const RELOCATE_STEP: f32 = 0.5;
/// ...after spots farther along the push, this far apart, up to this far (metres).
const RELOCATE_ALONG_STEP: f32 = 0.1;
const RELOCATE_ALONG: f32 = 2.0;
/// The parting checks the way of a unit it moves this often (metres): a
/// unit never passes through a static obstacle thicker than this.
const SWEEP_STEP: f32 = 0.2;
/// Ticks before the parting looks again for a free spot for a unit it
/// found none for (the crowd or the clutter around it rarely clears sooner)...
const RELOCATE_RETRY_TICKS: u8 = 15;
/// ...or for one still overlapping another (it keeps looking until parted).
const OVERLAPPED_RETRY_TICKS: u8 = 5;
/// Relocation searches per tick at most (each may test every spot, so a
/// pathological tick stays cheap; what is left waits for the next tick).
const MAX_RELOCATION_SEARCHES: usize = 16;
/// Area fraction a settled crowd fills (sizes the crowd-arrival radius).
const CROWD_PACKING: f32 = 0.5;
/// Slack added to the crowd-arrival radius (metres).
const CROWD_MARGIN: f32 = 0.5;

#[derive(Clone, Copy, Debug)]
struct Body {
    entity: Entity,
    /// `Mover::born`: of two bodies, the greater is the younger unit.
    born: u64,
    pos: Vec2,
    /// Position at the start of the tick (always valid ground).
    start: Vec2,
    half: Vec2,
    /// The box's facing and unit axes (the passes never turn a box; the
    /// final word may undo this tick's turn).
    yaw: f32,
    right: Vec2,
    forward: Vec2,
    /// Facing at the start of the tick (to undo a turn that wedged the unit).
    start_yaw: f32,
    /// The solve undid this tick's turn: write `start_yaw` back.
    restore_yaw: bool,
    /// Half extents of the box's axis-aligned bounds.
    ext: Vec2,
    resist: f32,
    /// Standing idle: gets out of a walker's way.
    yields: bool,
    /// Walking, but giving way this tick (see the module docs).
    gives_way: bool,
    /// Gave way lately (`Mover::give_way`): of a head-on pair, it gives way again.
    gave_way: bool,
    /// Pressed this tick against a walker ahead of it going its way that
    /// moves on: waiting its turn (`Mover::waiting`).
    waits: bool,
    /// Moves on: more than [`QUEUE_FLOW`] of steering's steps went through
    /// lately (`Mover::flow`), or it waited its turn last tick.
    moves_on: bool,
    /// Unit direction steering moved the unit in this tick (zero when standing).
    heading: Vec2,
    mobility: u16,
    team: u8,
    owner: PlayerId,
    /// Point goal being walked to / already reached (crowd arrival).
    heading_to: Option<Vec2>,
    arrived_at: Option<Vec2>,
    /// Crowd arrival only counts within this distance of the goal, so a
    /// column touching the crowd's edge does not freeze into a tail.
    crowd_radius: f32,
    /// Touched an arrived groupmate with the same point goal.
    crowd: bool,
    /// Push out of impassable terrain (off when the tick started on some:
    /// the unit then walks out on its own).
    on_valid_ground: bool,
    /// Where the last obstacle and terrain push-out found nothing to do.
    /// Both depend on the position alone, so they are skipped while the
    /// unit is still there.
    clear_at: Option<Vec2>,
    /// Directions (unit, away from the wall) obstacles or the terrain
    /// pushed the unit back this tick; `ZERO` = unused slot.
    braces: [Vec2; 2],
    /// Standing still and fighting (not holding): steps aside for allied
    /// walkers like an idle unit, holds its ground against enemies.
    fights_in_place: bool,
    /// Candidate pairs it is in this tick (how crowded it stands; saturating).
    contacts: u8,
    /// Deepest a static obstacle or terrain it cannot enter may hold it at
    /// the end of the final word on pairs: the wedge tolerance, or how deep
    /// it already was when the parting first moved it (`NAN`: not yet known).
    wedge: f32,
    /// The parting may look for a free spot to set it down this tick (it
    /// found none for it lately: see `Mover::relocate_wait`).
    may_relocate: bool,
    /// The parting looked for a free spot for it this tick and found none.
    relocation_failed: bool,
    /// Has no spot of its own to go back to: just trained (it appears where
    /// the building put it, maybe on others), or set down out of a building
    /// laid over it this tick. The parting looks farther for room for it.
    placed_anew: bool,
    /// Where the solver passes (and the final word on obstacles) left it,
    /// before the final word on pairs (for [`CollisionStats`]).
    solved: Vec2,
    /// The final word on pairs held it back toward where it started: from
    /// where (it stood when first held back) and how far of the way
    /// (one of [`HOLD_STEPS`]; 0 while not held).
    held: bool,
    hold_from: Vec2,
    hold_t: f32,
    /// The final word sent it back toward where it started the tick, to
    /// part a pair it walked into (a correction, but no jump).
    backed_off: bool,
    /// Began the tick overlapping another unit deeper than
    /// [`FINAL_TOLERANCE`], and how many ticks in a row it has (this one
    /// included; see `Mover::overlapped_for`). Such a unit keeps the
    /// parting's strong measures until it is apart.
    overlapped: bool,
    overlapped_for: u16,
    /// How far obstacles and the terrain pushed it this tick in all (metres),
    /// and whether the final word on obstacles set it down out of one.
    walled: f32,
    ejected: bool,
}

impl Body {
    #[inline]
    fn frame(&self) -> Frame {
        Frame { center: self.pos, half: self.half, right: self.right, forward: self.forward }
    }
    #[inline]
    fn bounds(&self) -> (Vec2, Vec2) {
        (self.pos - self.ext, self.pos + self.ext)
    }

    /// An obstacle or the terrain pushed the unit back this tick.
    #[inline]
    fn braced(&self) -> bool {
        self.braces[0] != Vec2::ZERO
    }

    /// Remember that a wall pushed the unit along `push` (two directions at most).
    fn brace(&mut self, push: Vec2) {
        let Some(n) = push.try_normalize() else { return };
        if self.braces.iter().any(|b| b.dot(n) > SAME_WALL) {
            return;
        }
        if let Some(slot) = self.braces.iter_mut().find(|b| **b == Vec2::ZERO) {
            *slot = n;
        }
    }

    /// Would moving along `push` drive the unit back into a wall it is braced against?
    #[inline]
    fn pushed_into_wall(&self, push: Vec2) -> bool {
        let limit = -INTO_WALL * push.length();
        self.braces.iter().any(|b| *b != Vec2::ZERO && push.dot(*b) < limit)
    }

    /// See [`Body::wedge`] (the tolerance while it is not known yet).
    #[inline]
    fn wedge_limit(&self) -> f32 {
        if self.wedge.is_nan() { WEDGE_TOLERANCE } else { self.wedge }
    }

    /// Steering turned the box this tick.
    #[inline]
    fn turned(&self) -> bool {
        self.yaw != self.start_yaw
    }

    /// The box at `at`, facing as it did at the start of the tick.
    #[inline]
    fn start_frame(&self, at: Vec2) -> Frame {
        Obb::new(at, self.half, self.start_yaw).frame()
    }

    /// Undo this tick's turn (the facing written back is the start one).
    fn unturn(&mut self) {
        let frame = self.start_frame(self.pos);
        (self.yaw, self.right, self.forward, self.ext) = (self.start_yaw, frame.right, frame.forward, frame.extents());
        self.restore_yaw = true;
    }
}

/// The pairs of bodies that may touch during this tick's solver passes:
/// every pair whose bounds come within [`CONTACT_MARGIN`] of each other.
/// Two bodies that were farther apart cannot touch before one of them has
/// moved half the margin, so the list is rebuilt only then (a body thrown
/// out of a new foundation, a big shove), not on every pass.
#[derive(Default)]
struct Contacts {
    /// Body index pairs, each once, in grid order.
    pairs: Vec<(u32, u32)>,
    /// Body positions and bounds half extents the list was built from
    /// (empty: no list).
    built_at: Vec<Vec2>,
    built_ext: Vec<Vec2>,
    bounds: Vec<(Vec2, Vec2)>,
    grid: BodyGrid,
    /// Per-body visit stamps for [`Contacts::near`] (a body sits in several cells).
    seen: Vec<u32>,
    stamp: u32,
}

impl Contacts {
    /// Forget the list (the bodies are about to change).
    fn invalidate(&mut self) {
        self.built_at.clear();
    }

    /// Does `f` hold for some body whose box may now touch the bounds
    /// `min`..`max` (see [`Contacts::near`]; it may see a body twice)? Bodies
    /// moved farther than half the contact margin since the list was built
    /// are the caller's to check.
    fn any_near(&self, min: Vec2, max: Vec2, mut f: impl FnMut(u32) -> bool) -> bool {
        let grow = Vec2::splat(CONTACT_MARGIN * 0.5);
        let [x0, y0, x1, y1] = self.grid.range(min - grow, max + grow);
        (y0..=y1).any(|y| (x0..=x1).any(|x| self.grid.items(self.grid.cell_index(x, y)).iter().any(|&i| f(i))))
    }

    /// Bodies whose box may now touch the bounds `min`..`max`, each once,
    /// into `out` (cleared first): those the grid lists there, which holds
    /// every body still within half the contact margin of where the list
    /// was built, and every body in `moved`, which may have gone farther.
    fn near(&mut self, min: Vec2, max: Vec2, moved: &[u32], out: &mut Vec<u32>) {
        out.clear();
        if self.seen.len() < self.built_at.len() {
            self.seen.resize(self.built_at.len(), 0);
        }
        self.stamp = self.stamp.wrapping_add(1);
        if self.stamp == 0 {
            self.seen.iter_mut().for_each(|s| *s = 0);
            self.stamp = 1;
        }
        let grow = Vec2::splat(CONTACT_MARGIN * 0.5);
        let [x0, y0, x1, y1] = self.grid.range(min - grow, max + grow);
        for y in y0..=y1 {
            for x in x0..=x1 {
                for &i in self.grid.items(self.grid.cell_index(x, y)) {
                    if let Some(s) = self.seen.get_mut(i as usize)
                        && *s != self.stamp
                    {
                        *s = self.stamp;
                        out.push(i);
                    }
                }
            }
        }
        for &i in moved {
            if let Some(s) = self.seen.get_mut(i as usize)
                && *s != self.stamp
            {
                *s = self.stamp;
                out.push(i);
            }
        }
    }

    /// Rebuild the list unless it still holds for `bodies`: unless every
    /// body's bounds still lie within those it was built from grown by half
    /// the margin, however it got there (a move, or a turn undone, which
    /// grows the bounds of a long box in place).
    fn refresh(&mut self, bodies: &[Body], origin: Vec2, size: Vec2) {
        let grow = Vec2::splat(CONTACT_MARGIN * 0.5);
        let holds = self.built_at.len() == bodies.len()
            && bodies.iter().zip(self.built_at.iter().zip(&self.built_ext)).all(|(b, (at, ext))| ((b.pos - *at).abs() + (b.ext - *ext).max(Vec2::ZERO)).cmple(grow).all());
        if holds {
            return;
        }
        self.built_at.clear();
        self.built_at.extend(bodies.iter().map(|b| b.pos));
        self.built_ext.clear();
        self.built_ext.extend(bodies.iter().map(|b| b.ext));
        self.bounds.clear();
        self.bounds.extend(bodies.iter().map(|b| {
            let (min, max) = b.bounds();
            (min - grow, max + grow)
        }));
        self.grid.rebuild(origin, size, GRID_CELL, &self.bounds);
        self.pairs.clear();
        for &c in self.grid.occupied() {
            let c = c as usize;
            let items = self.grid.items(c);
            for (k, &a) in items.iter().enumerate() {
                for &b in &items[k + 1..] {
                    let ((amin, amax), (bmin, bmax)) = (self.bounds[a as usize], self.bounds[b as usize]);
                    if amin.x >= bmax.x || bmin.x >= amax.x || amin.y >= bmax.y || bmin.y >= amax.y {
                        continue;
                    }
                    // Each pair once: in the cell holding the min corner of the overlap.
                    let (cx, cy) = self.grid.cell_xy(amin.max(bmin));
                    if self.grid.cell_index(cx, cy) == c {
                        self.pairs.push((a, b));
                    }
                }
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct CollisionScratch {
    ground: Vec<Body>,
    air: Vec<Body>,
    ground_contacts: Contacts,
    air_contacts: Contacts,
    /// (owner, goal cell) of every unit with a point goal, sorted; (owner,
    /// place) of every unit standing by (see [`stands_by`]), by owner.
    groups: Vec<(PlayerId, i32, i32)>,
    standing: Vec<(PlayerId, Vec2)>,
    /// Per-obstacle visit stamps (a box sits in several hash cells).
    static_seen: Vec<u32>,
    static_stamp: u32,
    /// The final word on pairs: those still too deep, the bodies it moved
    /// this round, neighbour scratch and the spots tried for a relocation.
    deep: Vec<(u32, u32)>,
    moved: Vec<u32>,
    near: Vec<u32>,
    spots: Vec<Vec2>,
    ring: Vec<Vec2>,
}

type CollideQuery<'a> = (
    Entity,
    &'a mut Position,
    &'a mut Facing,
    &'a PrevFacing,
    &'a Footprint,
    &'a Mobile,
    &'a Owner,
    &'a PathState,
    Option<&'a MoveGoal>,
    Option<&'a Stance>,
    Option<&'a Orders>,
    Has<AttackTarget>,
    &'a mut Mover,
);

pub(crate) fn resolve_collisions(
    nav: Res<Nav>,
    reg: Res<ObstacleRegistry>,
    players: Res<Players>,
    clock: Res<SimClock>,
    mut stats: ResMut<CollisionStats>,
    mut scratch: Local<CollisionScratch>,
    mut units: Query<CollideQuery, Without<Dead>>,
) {
    let s = &mut *scratch;
    s.ground.clear();
    s.air.clear();
    for (entity, pos, facing, prev_facing, fp, mob, owner, path, goal, stance, orders, fighting, mover) in &units {
        let holds_position = stance == Some(&Stance::StandGround) || orders.and_then(Orders::current) == Some(&Order::Hold);
        let moving = is_moving(goal, path);
        let (factor, yields) = resistance(moving, holds_position, fighting);
        let point_goal = goal.filter(|g| g.follow.is_none());
        let frame = Obb::new(pos.0, fp.half, facing.0).frame();
        let body = Body {
            entity,
            born: mover.born,
            pos: pos.0,
            start: mover.tick_start,
            half: fp.half,
            yaw: facing.0,
            right: frame.right,
            forward: frame.forward,
            start_yaw: prev_facing.0,
            restore_yaw: false,
            ext: frame.extents(),
            resist: 4.0 * fp.half.x * fp.half.y * factor + 1e-4,
            yields,
            gives_way: false,
            gave_way: mover.give_way > 0,
            waits: false,
            moves_on: mover.flow > QUEUE_FLOW || mover.waiting,
            heading: mover.intended.normalize_or_zero(),
            mobility: mob.mobility.0,
            team: players.team(owner.0),
            owner: owner.0,
            heading_to: point_goal.filter(|_| path.status == PathStatus::Following).map(|g| g.target),
            arrived_at: point_goal.filter(|_| path.status == PathStatus::Arrived).map(|g| g.target),
            // Arrive distance for now; grown by the group size below.
            crowd_radius: point_goal.map_or(0.0, |g| g.arrive),
            crowd: false,
            on_valid_ground: nav.terrain_passable(nav.cell_of(mover.tick_start), mob.mobility.0),
            clear_at: None,
            braces: [Vec2::ZERO; 2],
            fights_in_place: fighting && !moving && !holds_position,
            contacts: 0,
            wedge: f32::NAN,
            may_relocate: mover.relocate_wait == 0,
            relocation_failed: false,
            placed_anew: mover.fresh,
            solved: pos.0,
            held: false,
            hold_from: pos.0,
            hold_t: 0.0,
            backed_off: false,
            overlapped: false,
            overlapped_for: mover.overlapped_for,
            walled: 0.0,
            ejected: false,
        };
        if mob.flying {
            s.air.push(body);
        } else {
            s.ground.push(body);
        }
    }
    // Stable order: independent of archetype layout. Not an age order
    // (Entity ordering is opaque): `born` tells which unit is younger.
    s.ground.sort_unstable_by_key(|b| b.entity);
    s.air.sort_unstable_by_key(|b| b.entity);
    size_crowds(&mut s.groups, &mut s.standing, &mut s.ground, &mut s.air);
    if s.static_seen.len() < reg.capacity() {
        s.static_seen.resize(reg.capacity(), 0);
    }

    let (origin, size) = (nav.origin(), nav.world_size());
    s.ground_contacts.invalidate();
    s.air_contacts.invalidate();
    s.ground_contacts.refresh(&s.ground, origin, size);
    mark_giving_way(&mut s.ground, &s.ground_contacts.pairs);
    let overlapped = mark_overlapped(&mut s.ground, &s.ground_contacts.pairs);
    let parting = s.solve_ground(&nav, &reg);
    record(&mut stats, &s.ground, &parting, overlapped, clock.elapsed);
    // No crowd arrival in a passage on the way to a goal past it: the
    // walker would stand in the gap the rest of the crowd still needs.
    for b in s.ground.iter_mut().filter(|b| b.crowd) {
        b.crowd = !in_passage(&nav, b.pos, b.mobility) || b.heading_to.is_some_and(|goal| in_passage(&nav, goal, b.mobility));
    }
    for _ in 0..MIN_ITERATIONS {
        separate_pairs(&mut s.air, &mut s.air_contacts, origin, size, FLYER_SOFTNESS, false);
    }
    for b in &mut s.air {
        b.pos = clamp_to_map(b.pos, b.ext, &nav);
    }

    for b in s.ground.iter().chain(&s.air) {
        let Ok((_, mut pos, mut facing, .., mut mover)) = units.get_mut(b.entity) else { continue };
        if pos.0 != b.pos {
            pos.0 = b.pos;
        }
        if b.restore_yaw {
            facing.0 = b.start_yaw;
        }
        if mover.crowd_contact != b.crowd {
            mover.crowd_contact = b.crowd;
        }
        if mover.fresh {
            mover.fresh = false;
        }
        let wait = match (b.relocation_failed, b.overlapped) {
            (true, true) => OVERLAPPED_RETRY_TICKS,
            (true, false) => RELOCATE_RETRY_TICKS,
            (false, _) => mover.relocate_wait.saturating_sub(1),
        };
        if mover.relocate_wait != wait {
            mover.relocate_wait = wait;
        }
        if mover.overlapped_for != b.overlapped_for {
            mover.overlapped_for = b.overlapped_for;
        }
        let give_way = if b.gives_way { GIVE_WAY_MEMORY } else { mover.give_way.saturating_sub(1) };
        if mover.give_way != give_way {
            mover.give_way = give_way;
        }
        if mover.waiting != b.waits {
            mover.waiting = b.waits;
        }
    }
}

/// Add the tick's ground solve to `stats`: what the final word on pairs did
/// (`parting`), how far it moved units, and who jumped, by what.
fn record(stats: &mut CollisionStats, ground: &[Body], parting: &Parting, overlapped: u64, now: f32) {
    stats.ticks += 1;
    stats.overlapped += overlapped;
    stats.held += u64::from(parting.held);
    stats.parted += u64::from(parting.pushed + parting.unturned + parting.relocated + parting.reverted + parting.eased);
    stats.relocated += u64::from(parting.relocated);
    stats.left += u64::from(parting.left);
    stats.max_relocation = stats.max_relocation.max(parting.farthest_relocation);
    // Units the final word moved, but for those it only took back toward
    // where they started (held back, sent back): no jump.
    let corrected = || ground.iter().filter(|b| !b.held && !b.backed_off && b.pos != b.solved);
    let correction = corrected().map(|b| b.pos.distance(b.solved)).fold(0.0f32, f32::max);
    if correction > stats.max_correction {
        (stats.max_correction, stats.max_correction_at) = (correction, now);
    }
    stats.max_final_move = corrected().map(|b| b.pos.distance(b.start)).fold(stats.max_final_move, f32::max);
    for b in ground.iter().filter(|b| b.pos.distance(b.start) > CollisionStats::JUMP) {
        if b.pos != b.solved && !b.held && !b.backed_off {
            stats.jumps_final += 1;
        } else if b.ejected || b.walled > CollisionStats::JUMP * 0.5 {
            stats.jumps_eject += 1;
        } else {
            stats.jumps_passes += 1;
        }
    }
}

impl CollisionScratch {
    /// Passes of pairs (the first one with sidesteps), then obstacles and
    /// terrain, until no pair overlaps past the tolerance or
    /// [`MAX_ITERATIONS`] are done; then the final word of
    /// [`keep_on_valid_ground`].
    fn solve_ground(&mut self, nav: &NavGrid, reg: &ObstacleRegistry) -> Parting {
        let (origin, size) = (nav.origin(), nav.world_size());
        let cell = CellBoxes::new(nav);
        for pass in 1..=MAX_ITERATIONS {
            let deepest = separate_pairs(&mut self.ground, &mut self.ground_contacts, origin, size, 1.0, pass == 1);
            // Obstacles and terrain always get the last word.
            for b in &mut self.ground {
                if b.clear_at == Some(b.pos) {
                    continue;
                }
                self.static_stamp = self.static_stamp.wrapping_add(1);
                if self.static_stamp == 0 {
                    self.static_seen.iter_mut().for_each(|v| *v = 0);
                    self.static_stamp = 1;
                }
                let before = b.pos;
                push_out_static(b, reg, &mut self.static_seen, self.static_stamp);
                push_out_terrain(b, nav, &cell);
                b.clear_at = (b.pos == before).then_some(before);
                b.walled += b.pos.distance(before);
                b.brace(b.pos - before);
            }
            if pass >= MIN_ITERATIONS && deepest < OVERLAP_TOLERANCE {
                break;
            }
        }
        // Units set down out of a new foundation land clear of the others.
        self.ground_contacts.refresh(&self.ground, origin, size);
        self.moved.clear();
        for i in 0..self.ground.len() {
            let mut b = self.ground[i];
            let (bodies, contacts, moved) = (&self.ground, &self.ground_contacts, &self.moved);
            let ejected = keep_on_valid_ground(&mut b, nav, reg, &|frame: &Frame| crowds(bodies, contacts, moved, i, frame));
            b.placed_anew |= ejected;
            b.ejected = ejected;
            self.ground[i] = b;
            if ejected {
                self.moved.push(i as u32);
            }
        }
        for b in &mut self.ground {
            b.solved = b.pos;
        }
        let parting = self.part_deep_pairs(nav, reg);
        if parting != Parting::default() {
            debug!(
                target: "af_movement::collision",
                "final word on pairs: {} pushed apart, {} turns undone, {} units set down nearby, {} sent back, {} eased apart, {} held back, {} left",
                parting.pushed,
                parting.unturned,
                parting.relocated,
                parting.reverted,
                parting.eased,
                parting.held,
                parting.left
            );
        }
        parting
    }

    /// The final word on pairs: part every pair of ground units still
    /// overlapping deeper than [`FINAL_TOLERANCE`] (see [`Parter`]), in
    /// rounds until none is left or [`FINAL_ROUNDS`] are done.
    fn part_deep_pairs(&mut self, nav: &NavGrid, reg: &ObstacleRegistry) -> Parting {
        let mut done = Parting::default();
        let (origin, size) = (nav.origin(), nav.world_size());
        let mut parter = Parter {
            bodies: &mut self.ground,
            contacts: &mut self.ground_contacts,
            moved: &mut self.moved,
            near: &mut self.near,
            spots: &mut self.spots,
            ring: &mut self.ring,
            world: Surroundings { nav, reg, cells: CellBoxes::new(nav) },
        };
        // Relocation searches tried this tick (each may test every spot).
        let mut relocations = 0;
        for _ in 0..FINAL_ROUNDS {
            parter.contacts.refresh(parter.bodies, origin, size);
            self.deep.clear();
            let bodies = &*parter.bodies;
            self.deep.extend(parter.contacts.pairs.iter().copied().filter(|&(a, b)| pair_depth(&bodies[a as usize], &bodies[b as usize]) > FINAL_TOLERANCE));
            if self.deep.is_empty() {
                return done;
            }
            parter.moved.clear();
            for &(a, b) in &self.deep {
                let (a, b) = (a as usize, b as usize);
                if pair_depth(&parter.bodies[a], &parter.bodies[b]) <= FINAL_TOLERANCE {
                    continue;
                }
                let (pa, pb) = (parter.bodies[a].pos, parter.bodies[b].pos);
                match parter.part(a, b, &mut relocations) {
                    Some(Parted::Pushed) => done.pushed += 1,
                    Some(Parted::Unturned) => done.unturned += 1,
                    Some(Parted::Relocated) => {
                        done.relocated += 1;
                        let jump = parter.bodies[a].pos.distance(pa).max(parter.bodies[b].pos.distance(pb));
                        done.farthest_relocation = done.farthest_relocation.max(jump);
                    }
                    Some(Parted::Reverted) => done.reverted += 1,
                    Some(Parted::Eased) => done.eased += 1,
                    None => {}
                }
            }
        }
        // What no round could part may at least get no deeper than it was
        // when the tick began: hold units back toward where they started.
        parter.contacts.refresh(parter.bodies, origin, size);
        self.deep.clear();
        let bodies = &*parter.bodies;
        self.deep.extend(parter.contacts.pairs.iter().copied().filter(|&(a, b)| pair_depth(&bodies[a as usize], &bodies[b as usize]) > FINAL_TOLERANCE));
        parter.moved.clear();
        done.held = parter.hold_back(&mut self.deep);
        // The last resort, for a pair holding back could not bring back to
        // how deep it began: one a unit pushed out of a new foundation (it
        // has no start to go back to) shoved together.
        parter.contacts.refresh(parter.bodies, origin, size);
        self.deep.clear();
        let bodies = &*parter.bodies;
        // Same bound `hold_back` accepts: a pair it left no deeper than it
        // began (or than the tolerance) is done, not a case for a jump.
        self.deep.extend(parter.contacts.pairs.iter().copied().filter(|&(a, b)| {
            let (x, y) = (&bodies[a as usize], &bodies[b as usize]);
            pair_depth(x, y) > start_depth(x, y).max(FINAL_TOLERANCE) + 1e-4
        }));
        parter.moved.clear();
        for &(a, b) in &self.deep {
            let (pa, pb) = (parter.bodies[a as usize].pos, parter.bodies[b as usize].pos);
            match parter.rescue(a as usize, b as usize, &mut relocations) {
                Some(Parted::Relocated) => {
                    done.relocated += 1;
                    let jump = parter.bodies[a as usize].pos.distance(pa).max(parter.bodies[b as usize].pos.distance(pb));
                    done.farthest_relocation = done.farthest_relocation.max(jump);
                }
                Some(_) => done.pushed += 1,
                None => {}
            }
        }
        // Holding back may have undone what eased an overlap the tick began
        // with: ease each such pair once more where there is room, so it
        // never stays as deep as it began while room is at hand.
        parter.contacts.refresh(parter.bodies, origin, size);
        self.deep.clear();
        let bodies = &*parter.bodies;
        self.deep.extend(parter.contacts.pairs.iter().copied().filter(|&(a, b)| {
            let (x, y) = (&bodies[a as usize], &bodies[b as usize]);
            pair_depth(x, y) > FINAL_TOLERANCE && start_depth(x, y) > FINAL_TOLERANCE
        }));
        parter.moved.clear();
        for &(a, b) in &self.deep {
            if parter.ease(a as usize, b as usize) {
                done.eased += 1;
            }
        }
        // Whatever is left stays for the next tick, no deeper than it began.
        parter.contacts.refresh(parter.bodies, origin, size);
        let bodies = &*parter.bodies;
        for &(a, b) in &parter.contacts.pairs {
            let (x, y) = (&bodies[a as usize], &bodies[b as usize]);
            let depth = pair_depth(x, y);
            if depth > FINAL_TOLERANCE {
                done.left += 1;
                debug!(target: "af_movement::collision", "{} and {} still overlap by {depth:.3} m: no room to part them", x.entity, y.entity);
            }
        }
        done
    }
}

/// What the final word on pairs did in one tick: pairs parted by a push,
/// by undoing a turn, by setting a unit down nearby, by sending one back
/// toward where it started the tick, pairs eased apart part of the way;
/// units held back; and pairs left too deep (no room for any of it, and no
/// deeper than at the start of the tick).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Parting {
    pushed: u32,
    unturned: u32,
    relocated: u32,
    reverted: u32,
    eased: u32,
    /// Units held back toward where (and as) they started the tick, so that
    /// no pair left too deep got deeper than it began (see
    /// [`Parter::hold_back`]).
    held: u32,
    left: u32,
    /// Farthest a relocation set a unit down (metres).
    farthest_relocation: f32,
}

/// How [`Parter::part`] parted a pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Parted {
    Pushed,
    Unturned,
    Relocated,
    Reverted,
    Eased,
}

/// Depth of the overlap of two bodies' boxes (0 when apart).
#[inline]
fn pair_depth(a: &Body, b: &Body) -> f32 {
    let ((amin, amax), (bmin, bmax)) = (a.bounds(), b.bounds());
    if amin.x >= bmax.x || bmin.x >= amax.x || amin.y >= bmax.y || bmin.y >= amax.y {
        return 0.0;
    }
    a.frame().mtv(&b.frame()).map_or(0.0, |m| m.length())
}

/// What a ground unit may stand on: the map, its terrain and the static obstacles.
struct Surroundings<'a> {
    nav: &'a NavGrid,
    reg: &'a ObstacleRegistry,
    cells: CellBoxes,
}

impl Surroundings<'_> {
    /// May `b` stand with its box at `frame`: inside the map, its centre
    /// on terrain it can enter (when it started the tick on such), and
    /// no deeper than [`WEDGE_TOLERANCE`] into terrain it cannot enter or
    /// a static obstacle (its own team's gates aside)?
    fn admits(&self, b: &Body, frame: &Frame) -> bool {
        let at = frame.center;
        let ext = frame.extents();
        if clamp_to_map(at, ext, self.nav) != at {
            return false;
        }
        let wedge = b.wedge_limit();
        if b.on_valid_ground && (!self.nav.terrain_passable(self.nav.cell_of(at), b.mobility) || self.terrain_depth(b, frame) > wedge) {
            return false;
        }
        static_depth(b, frame, at, self.reg) <= wedge
    }

    /// May `b` go back to where and as it started the tick? The last tick
    /// left it there within the wedge tolerance of every static obstacle
    /// (and terrain does not change), so only an obstacle placed since (a
    /// foundation laid over that spot) forbids it.
    fn still_admits_start(&self, b: &Body) -> bool {
        static_depth(b, &b.start_frame(b.start), b.start, self.reg) <= WEDGE_TOLERANCE + 1e-3
    }

    /// How deep `b` stands in static obstacles and terrain it cannot enter.
    fn wedged(&self, b: &Body) -> f32 {
        let frame = b.frame();
        let terrain = if b.on_valid_ground { self.terrain_depth(b, &frame) } else { 0.0 };
        static_depth(b, &frame, b.pos, self.reg).max(terrain)
    }

    /// Can `b` slide in a straight line from where it stands (and as it
    /// faces now) to its box at `to` without its box passing through a
    /// static obstacle or its centre over terrain it cannot enter? The
    /// way is checked every [`SWEEP_STEP`], and may cut no deeper into an
    /// obstacle than [`WEDGE_TOLERANCE`] or than the box already does where
    /// it stands. So a unit is never parted through a wall into a pen, or
    /// out of one.
    fn sweeps(&self, b: &Body, to: &Frame) -> bool {
        let way = to.center - b.pos;
        let steps = (way.length() / SWEEP_STEP).ceil() as i32;
        if steps <= 1 {
            return true;
        }
        let here = Frame { center: b.pos, ..*to };
        let allowed = static_depth(b, &here, b.pos, self.reg).max(b.wedge_limit());
        (1..steps).all(|k| {
            let at = b.pos + way * (k as f32 / steps as f32);
            let ground = !b.on_valid_ground || self.nav.terrain_passable(self.nav.cell_of(at), b.mobility);
            ground && static_depth(b, &here, at, self.reg) <= allowed
        })
    }

    /// Deepest overlap of the box `frame` with a terrain cell `b` cannot enter.
    fn terrain_depth(&self, b: &Body, frame: &Frame) -> f32 {
        let ext = frame.extents();
        let (c0, c1) = (self.nav.cell_of(frame.center - ext), self.nav.cell_of(frame.center + ext));
        let mut deepest = 0.0f32;
        for y in c0.y..=c1.y {
            for x in c0.x..=c1.x {
                let c = IVec2::new(x, y);
                if self.nav.terrain_passable(c, b.mobility) {
                    continue;
                }
                if let Some(m) = frame.mtv(&self.cells.at(self.nav.cell_center(c))) {
                    deepest = deepest.max(m.length());
                }
            }
        }
        deepest
    }
}

/// Parts the pairs the solver passes left too deep. A pair that began the
/// tick apart (a *fresh* overlap: one walked into the other this tick) is
/// parted so that it ends no deeper than [`FINAL_TOLERANCE`] while no other
/// pair gets deeper than that and no unit goes where
/// [`Surroundings::admits`] forbids or passes through an obstacle on its
/// way: every pair it parts stays parted, so it never makes things worse.
/// It tries, in turn, undoing a turn of either unit this tick (a long box
/// swung into a neighbour in a tight spot, which pushes cannot undo);
/// pushing the two apart along the push-out by their resistance shares,
/// then the lighter alone, then the heavier, and the same along the other
/// axes of either box (out of a slot lengthwise), as long as the push is no
/// longer than [`FRESH_PUSH`]; and sending the one that moved farther part
/// of the way, then all the way, back to where (and as) it started, which
/// the player does not see as a jump. A unit with no spot of its own to go
/// back to (just trained, or set down out of a new foundation) is set down
/// on the nearest free spot instead.
///
/// A pair that began the tick overlapping (trained onto one another, or
/// left so by a tick with no room) is parted the same way where it can be,
/// pushes of any length allowed; else it is *eased* apart: pushed apart
/// part of the way, so it ends shallower than it is, as far as no other
/// pair gets deeper than the tolerance (or than it is). The longer such an
/// overlap lasts the bolder the parting: after [`SET_DOWN_PATIENCE`] ticks
/// a unit of it may be set down on a free spot within [`SET_DOWN_RADIUS`],
/// after [`STRANDED_TICKS`] within [`EJECT_RADIUS`]. What none of that
/// parts is held back ([`Parter::hold_back`]), and then eased once more.
struct Parter<'a> {
    bodies: &'a mut [Body],
    contacts: &'a mut Contacts,
    /// Bodies moved this round (the contact grid may not list them where they are now).
    moved: &'a mut Vec<u32>,
    near: &'a mut Vec<u32>,
    spots: &'a mut Vec<Vec2>,
    /// Offsets of the spots all round tried for a relocation, nearest first.
    ring: &'a mut Vec<Vec2>,
    world: Surroundings<'a>,
}

/// Offsets within [`EJECT_RADIUS`] on a [`RELOCATE_STEP`] lattice, nearest first.
fn relocation_ring() -> Vec<Vec2> {
    let steps = (EJECT_RADIUS / RELOCATE_STEP).floor() as i32;
    let mut ring: Vec<Vec2> = (-steps..=steps)
        .flat_map(|y| (-steps..=steps).map(move |x| Vec2::new(x as f32, y as f32) * RELOCATE_STEP))
        .filter(|d| *d != Vec2::ZERO && d.length() <= EJECT_RADIUS + 1e-4)
        .collect();
    ring.sort_by(|p, q| p.length_squared().total_cmp(&q.length_squared()).then(p.x.total_cmp(&q.x)).then(p.y.total_cmp(&q.y)));
    ring
}

/// The ways to push a pair apart: along its push-out `m` (which moves `a`
/// out of `b`), then along the other axes of either box on which they
/// overlap less than [`MAX_SLIDE`], shallowest first; each as (overlap
/// along it, the push that leaves [`FINAL_TOLERANCE`] / 2 of it). Returns
/// them and how many there are.
fn push_outs(fa: &Frame, fb: &Frame, m: Vec2) -> ([(f32, Vec2); 5], usize) {
    let depth = m.length();
    let mut pushes = [(depth, m * ((depth - FINAL_TOLERANCE * 0.5) / depth)); 5];
    let mut n = 1;
    for axis in [fa.right, fa.forward, fb.right, fb.forward] {
        let along = fa.overlap_along(fb, axis);
        if along <= depth || along > MAX_SLIDE || pushes[..n].iter().any(|(_, p)| p.normalize_or_zero().dot(axis).abs() > SAME_AXIS) {
            continue;
        }
        let dir = if (fa.center - fb.center).dot(axis) < 0.0 { -axis } else { axis };
        pushes[n] = (along, dir * (along - FINAL_TOLERANCE * 0.5));
        n += 1;
    }
    pushes[1..n].sort_by(|p, q| p.0.total_cmp(&q.0));
    (pushes, n)
}

impl Parter<'_> {
    /// Part `a` and `b` (see the type docs); `None` when nothing fits.
    /// `searches` counts the relocation searches of the tick (at most
    /// [`MAX_RELOCATION_SEARCHES`]).
    fn part(&mut self, a: usize, b: usize, searches: &mut usize) -> Option<Parted> {
        let m = self.bodies[a].frame().mtv(&self.bodies[b].frame())?;
        self.learn_wedges(a, b);
        let depth = m.length();
        let away = m / depth; // moves `a` out of `b`
        let began = start_depth(&self.bodies[a], &self.bodies[b]);
        let inherited = began > FINAL_TOLERANCE;
        let (ra, rb) = (pushed_resist(&self.bodies[a], &self.bodies[b], m), pushed_resist(&self.bodies[b], &self.bodies[a], -m));
        // The lighter one first; the younger of two alike.
        let a_first = ra < rb || (ra == rb && younger(&self.bodies[a], &self.bodies[b]));
        let (light, heavy, sign) = if a_first { (a, b, 1.0) } else { (b, a, -1.0) };

        for who in [light, heavy] {
            if self.bodies[who].turned() {
                let frame = self.bodies[who].start_frame(self.bodies[who].pos);
                if self.fits(who, &frame, None) {
                    self.bodies[who].unturn();
                    self.note_moved(who);
                    return Some(Parted::Unturned);
                }
            }
        }

        // Along the push-out, else along another axis of either box (out of
        // a slot between two walls lengthwise, rather than into the walls),
        // shallowest first. A pair that began the tick apart is only shoved
        // a short way, however many of its pairs shove a unit: past that,
        // holding one back is the gentler cure.
        let short = |body: &Body, at: Vec2| inherited || at.distance(body.solved) <= FRESH_PUSH + 1e-4;
        let share_a = rb / (ra + rb);
        let (pushes, n) = push_outs(&self.bodies[a].frame(), &self.bodies[b].frame(), m);
        for &(_, push) in &pushes[..n] {
            let (pa, pb) = (self.bodies[a].pos + push * share_a, self.bodies[b].pos - push * (1.0 - share_a));
            let (fa, fb) = (self.frame_at(a, pa), self.frame_at(b, pb));
            let both_short = short(&self.bodies[a], pa) && short(&self.bodies[b], pb);
            if both_short && fa.mtv(&fb).is_none_or(|m| m.length() <= FINAL_TOLERANCE) && self.fits(a, &fa, Some(b)) && self.fits(b, &fb, Some(a)) {
                self.place(a, pa);
                self.place(b, pb);
                return Some(Parted::Pushed);
            }
            for (who, dir) in [(light, sign), (heavy, -sign)] {
                let at = self.bodies[who].pos + push * dir;
                if short(&self.bodies[who], at) && self.fits(who, &self.frame_at(who, at), None) {
                    self.place(who, at);
                    return Some(Parted::Pushed);
                }
            }
        }

        if !inherited {
            // Walked into each other this tick: the one that moved farther
            // goes part of the way back, else all the way (and as it faced).
            for who in farther_first(&self.bodies[a], &self.bodies[b], a, b) {
                let body = &self.bodies[who];
                if body.pos == body.start && !body.turned() {
                    continue;
                }
                let from = body.pos;
                for t in BACK_OFF_STEPS {
                    let (at, frame) = self.back_toward_start(who, from, t);
                    if self.fits(who, &frame, None) {
                        debug!(target: "af_movement::collision", "{} sent {:.0}% of the way back to where it started the tick to part it from a unit it overlapped by {depth:.3} m", self.bodies[who].entity, t * 100.0);
                        self.place(who, at);
                        self.bodies[who].backed_off = true;
                        if t >= 1.0 {
                            self.bodies[who].unturn();
                        }
                        return Some(Parted::Reverted);
                    }
                }
            }
        } else if self.ease(a, b) {
            return Some(Parted::Eased);
        }

        for (who, dir) in [(light, sign), (heavy, -sign)] {
            let Some(radius) = self.set_down_radius(who, inherited) else { continue };
            // Stranded just now: look at once, whatever the last look found.
            let stranded = inherited && self.bodies[who].overlapped_for == STRANDED_TICKS;
            if !(self.bodies[who].may_relocate || stranded) || *searches >= MAX_RELOCATION_SEARCHES {
                continue;
            }
            *searches += 1;
            let Some(at) = self.free_spot(who, away * dir, depth, radius) else {
                // Nothing free near it: not worth another look for a while.
                self.bodies[who].may_relocate = false;
                self.bodies[who].relocation_failed = true;
                continue;
            };
            debug!(target: "af_movement::collision", "{} set down {:.2} m away to part it from a unit it overlapped by {depth:.3} m", self.bodies[who].entity, at.distance(self.bodies[who].pos));
            self.place(who, at);
            return Some(Parted::Relocated);
        }
        None
    }

    /// The last resort for a pair that would end the tick deeper than it
    /// began although holding back did what it could (a unit pushed out of
    /// a new foundation shoved it together, and has no start to go back
    /// to): a push that parts it and leaves the unit within
    /// [`SET_DOWN_RADIUS`] of where the solver passes left it, else
    /// setting the lighter, then the heavier, down on the nearest free spot,
    /// within [`SET_DOWN_RADIUS`] if there is one, else within
    /// [`EJECT_RADIUS`].
    fn rescue(&mut self, a: usize, b: usize, searches: &mut usize) -> Option<Parted> {
        let m = self.bodies[a].frame().mtv(&self.bodies[b].frame())?;
        self.learn_wedges(a, b);
        let depth = m.length();
        let (ra, rb) = (pushed_resist(&self.bodies[a], &self.bodies[b], m), pushed_resist(&self.bodies[b], &self.bodies[a], -m));
        let (light, heavy, sign) = if ra < rb { (a, b, 1.0) } else { (b, a, -1.0) };
        let (pushes, n) = push_outs(&self.bodies[a].frame(), &self.bodies[b].frame(), m);
        for &(_, push) in &pushes[..n] {
            for (who, dir) in [(light, sign), (heavy, -sign)] {
                let at = self.bodies[who].pos + push * dir;
                if at.distance(self.bodies[who].solved) <= SET_DOWN_RADIUS && self.fits(who, &self.frame_at(who, at), None) {
                    self.place(who, at);
                    return Some(Parted::Pushed);
                }
            }
        }
        let away = m / depth;
        for radius in [SET_DOWN_RADIUS, EJECT_RADIUS] {
            for (who, dir) in [(light, sign), (heavy, -sign)] {
                if *searches >= MAX_RELOCATION_SEARCHES {
                    return None;
                }
                *searches += 1;
                if let Some(at) = self.free_spot(who, away * dir, depth, radius) {
                    debug!(target: "af_movement::collision", "{} set down {:.2} m away: shoved {depth:.3} m into another with no way back", self.bodies[who].entity, at.distance(self.bodies[who].pos));
                    self.place(who, at);
                    return Some(Parted::Relocated);
                }
            }
        }
        None
    }

    /// Learn how deep `a` and `b` already stand in obstacles (see [`Body::wedge`]).
    fn learn_wedges(&mut self, a: usize, b: usize) {
        for who in [a, b] {
            if self.bodies[who].wedge.is_nan() {
                self.bodies[who].wedge = self.world.wedged(&self.bodies[who]).max(WEDGE_TOLERANCE);
            }
        }
    }

    /// How far body `who`, of a pair that began the tick overlapping
    /// (`inherited`) or not, may be set down to part it: within
    /// [`EJECT_RADIUS`] when it has no spot of its own to go back to (just
    /// trained, or its start taken by a new building) or has been
    /// overlapping for [`STRANDED_TICKS`]; within [`SET_DOWN_RADIUS`] once
    /// it has been overlapping for [`SET_DOWN_PATIENCE`]; else not at all.
    fn set_down_radius(&self, who: usize, inherited: bool) -> Option<f32> {
        let body = &self.bodies[who];
        if body.placed_anew || !self.world.still_admits_start(body) || (inherited && body.overlapped_for >= STRANDED_TICKS) {
            Some(EJECT_RADIUS)
        } else if inherited && body.overlapped_for >= SET_DOWN_PATIENCE {
            Some(SET_DOWN_RADIUS)
        } else {
            None
        }
    }

    /// Body `who` `t` of the way from `from` back to where it started the
    /// tick (facing as it does now, or as it started once all the way):
    /// the spot and its box.
    fn back_toward_start(&self, who: usize, from: Vec2, t: f32) -> (Vec2, Frame) {
        let body = &self.bodies[who];
        if t >= 1.0 {
            (body.start, body.start_frame(body.start))
        } else {
            let at = from.lerp(body.start, t);
            (at, self.frame_at(who, at))
        }
    }

    /// Ease apart `a` and `b`, a pair that began the tick overlapping: push
    /// them apart part of the way (see [`EASE_FRACTIONS`]; both by their
    /// resistance shares, else the lighter, else the heavier alone) so the
    /// pair ends shallower while no other pair of either gets deeper than
    /// [`FINAL_TOLERANCE`], or than it is, and neither goes where the
    /// surroundings forbid. True when it eased them.
    fn ease(&mut self, a: usize, b: usize) -> bool {
        let Some(m) = self.bodies[a].frame().mtv(&self.bodies[b].frame()) else { return false };
        self.learn_wedges(a, b);
        let depth = m.length();
        let (ra, rb) = (pushed_resist(&self.bodies[a], &self.bodies[b], m), pushed_resist(&self.bodies[b], &self.bodies[a], -m));
        let (light, heavy, sign) = if ra < rb { (a, b, 1.0) } else { (b, a, -1.0) };
        let share_a = rb / (ra + rb);
        let (pushes, n) = push_outs(&self.bodies[a].frame(), &self.bodies[b].frame(), m);
        let shallower = |fa: &Frame, fb: &Frame| fa.mtv(fb).map_or(0.0, |m| m.length()) < depth - 1e-3;
        for fraction in EASE_FRACTIONS {
            for &(_, push) in &pushes[..n] {
                let push = push * fraction;
                let (pa, pb) = (self.bodies[a].pos + push * share_a, self.bodies[b].pos - push * (1.0 - share_a));
                let (fa, fb) = (self.frame_at(a, pa), self.frame_at(b, pb));
                if shallower(&fa, &fb) && self.no_worse(a, &fa, b) && self.no_worse(b, &fb, a) {
                    self.place(a, pa);
                    self.place(b, pb);
                    return true;
                }
                for (who, other, dir) in [(light, heavy, sign), (heavy, light, -sign)] {
                    let at = self.bodies[who].pos + push * dir;
                    let frame = self.frame_at(who, at);
                    if shallower(&frame, &self.bodies[other].frame()) && self.no_worse(who, &frame, other) {
                        self.place(who, at);
                        return true;
                    }
                }
            }
        }
        false
    }

    /// The box of body `who` (as it faces now) at `at`.
    #[inline]
    fn frame_at(&self, who: usize, at: Vec2) -> Frame {
        Frame { center: at, ..self.bodies[who].frame() }
    }

    /// May body `who` stand with its box at `frame`: the surroundings admit
    /// it, it gets there from where it stands without passing through an
    /// obstacle or terrain it cannot enter (see [`Surroundings::sweeps`]),
    /// and it overlaps no other ground unit (`skip` aside) deeper than
    /// [`FINAL_TOLERANCE`]?
    fn fits(&mut self, who: usize, frame: &Frame, skip: Option<usize>) -> bool {
        self.fits_within(who, frame, skip, false)
    }

    /// Like [`Parter::fits`], but another unit (`skip` aside) may stay as
    /// deep in `who` as it is now, if that is deeper than the tolerance.
    fn no_worse(&mut self, who: usize, frame: &Frame, skip: usize) -> bool {
        self.fits_within(who, frame, Some(skip), true)
    }

    /// [`Parter::fits`], or with `as_now` [`Parter::no_worse`].
    fn fits_within(&mut self, who: usize, frame: &Frame, skip: Option<usize>, as_now: bool) -> bool {
        if !self.world.admits(&self.bodies[who], frame) {
            return false;
        }
        let ext = frame.extents();
        let me = self.bodies[who].frame();
        self.contacts.near(frame.center - ext, frame.center + ext, self.moved, self.near);
        let clear = self.near.iter().map(|&j| j as usize).filter(|&j| j != who && Some(j) != skip).all(|j| {
            let other = &self.bodies[j];
            let (omin, omax) = other.bounds();
            let apart = omin.x >= frame.center.x + ext.x || frame.center.x - ext.x >= omax.x || omin.y >= frame.center.y + ext.y || frame.center.y - ext.y >= omax.y;
            let allowed = if as_now { me.mtv(&other.frame()).map_or(0.0, |m| m.length()).max(FINAL_TOLERANCE) + 1e-4 } else { FINAL_TOLERANCE };
            apart || frame.mtv(&other.frame()).is_none_or(|m| m.length() <= allowed)
        });
        // The way there last: the dearest test, and the rarest to fail.
        clear && self.world.sweeps(&self.bodies[who], frame)
    }

    /// The nearest spot within `radius` where body `who`, overlapping
    /// another by `depth` along `away` (unit, out of it), fits: farther
    /// along `away` first, then anywhere, nearest first.
    fn free_spot(&mut self, who: usize, away: Vec2, depth: f32, radius: f32) -> Option<Vec2> {
        let from = self.bodies[who].pos;
        if self.ring.is_empty() {
            *self.ring = relocation_ring();
        }
        let radius = radius + 1e-4;
        self.spots.clear();
        let along = (RELOCATE_ALONG / RELOCATE_ALONG_STEP).round() as i32;
        self.spots.extend((1..=along).map(|k| away * (depth + k as f32 * RELOCATE_ALONG_STEP)).take_while(|d| d.length() <= radius).map(|d| from + d));
        self.spots.extend(self.ring.iter().take_while(|d| d.length() <= radius).map(|d| from + *d));
        let spots = std::mem::take(self.spots);
        let found = spots.iter().copied().find(|&at| self.fits(who, &self.frame_at(who, at), None));
        *self.spots = spots;
        found
    }

    /// For the pairs in `deep` still deeper than [`FINAL_TOLERANCE`] and
    /// deeper than their boxes were at the start of the tick: hold one of
    /// the pair (the one that moved farther) back toward where it started,
    /// the least of [`HOLD_STEPS`] of the way that parts the pair that far
    /// (all the way, and as it faced, when none does), where its
    /// surroundings admit it, and check its new pairs the same way, until
    /// every pair too deep is no deeper than it began. Each unit only ever
    /// goes farther back, at most all the way, and as the start of the tick
    /// was the end of the last one, no pair ever ends a tick deeper than
    /// the tolerance unless something outside the solver put it there (a
    /// unit trained onto another, a foundation laid on them), and then
    /// never deeper than that. Held back part of the way, a unit in a crowd
    /// still moves on a little instead of standing still. Returns how many
    /// units it held back.
    fn hold_back(&mut self, deep: &mut Vec<(u32, u32)>) -> u32 {
        let mut held = 0;
        while let Some((a, b)) = deep.pop() {
            let (a, b) = (a as usize, b as usize);
            let now = pair_depth(&self.bodies[a], &self.bodies[b]);
            let limit = start_depth(&self.bodies[a], &self.bodies[b]).max(FINAL_TOLERANCE) + 1e-4;
            if now <= limit {
                continue;
            }
            let [first, second] = farther_first(&self.bodies[a], &self.bodies[b], a, b);
            let Some((who, at, t)) = self.hold_step(first, second, limit).or_else(|| self.hold_step(second, first, limit)) else {
                continue;
            };
            let body = &mut self.bodies[who];
            if !body.held {
                (body.held, body.hold_from) = (true, body.pos);
                held += 1;
            }
            body.hold_t = t;
            self.place(who, at);
            if t >= 1.0 {
                self.bodies[who].unturn();
            }
            deep.push((a as u32, b as u32));
            let frame = self.bodies[who].frame();
            let ext = frame.extents();
            self.contacts.near(frame.center - ext, frame.center + ext, self.moved, self.near);
            for &j in self.near.iter() {
                if j as usize != who && pair_depth(&self.bodies[who], &self.bodies[j as usize]) > FINAL_TOLERANCE {
                    deep.push((who as u32, j));
                }
            }
        }
        held
    }

    /// The next step back for body `u` (see [`Parter::hold_back`]) that
    /// leaves it no deeper than `limit` in `other`, the least such, else
    /// all the way back: `(u, where, fraction of the way)`. `None` when it
    /// can go no farther back: it is there already, or no step admits it.
    fn hold_step(&mut self, u: usize, other: usize, limit: f32) -> Option<(usize, Vec2, f32)> {
        let body = &self.bodies[u];
        if body.hold_t >= 1.0 || (body.pos == body.start && !body.turned()) {
            return None;
        }
        let (from, done) = (if body.held { body.hold_from } else { body.pos }, body.hold_t);
        let mut whole_way = None;
        for t in HOLD_STEPS.into_iter().filter(|t| *t > done + 1e-6) {
            let (at, frame) = self.back_toward_start(u, from, t);
            if !self.may_hold_at(u, &frame, t >= 1.0) {
                continue;
            }
            if frame.mtv(&self.bodies[other].frame()).is_none_or(|m| m.length() <= limit) {
                return Some((u, at, t));
            }
            if t >= 1.0 {
                whole_way = Some((u, at, t));
            }
        }
        whole_way
    }

    /// May body `u` stand held back with its box at `frame` (all the way
    /// back when `start`)? Not where an obstacle was placed since the tick
    /// began (its start), or where the surroundings forbid (on the way
    /// back), nor where its box would cut too deep into a unit that cannot
    /// go back itself (one set down out of a new foundation): that pair
    /// could end deeper than it began.
    fn may_hold_at(&mut self, u: usize, frame: &Frame, start: bool) -> bool {
        let body = &self.bodies[u];
        let admitted = if start { self.world.still_admits_start(body) } else { self.world.admits(body, frame) };
        if !admitted {
            return false;
        }
        let ext = frame.extents();
        self.contacts.near(frame.center - ext, frame.center + ext, self.moved, self.near);
        self.near.iter().map(|&j| j as usize).filter(|&j| j != u).all(|j| {
            let other = &self.bodies[j];
            let stuck_here = (other.pos != other.start || other.yaw != other.start_yaw) && !self.world.still_admits_start(other);
            !stuck_here || frame.mtv(&other.frame()).is_none_or(|m| m.length() <= FINAL_TOLERANCE)
        })
    }

    fn place(&mut self, who: usize, at: Vec2) {
        self.bodies[who].pos = at;
        self.bodies[who].clear_at = None;
        self.bodies[who].backed_off = false;
        // Set down elsewhere: no longer held back toward its start (and the
        // move counts as a correction in the stats).
        self.bodies[who].held = false;
        self.note_moved(who);
    }

    fn note_moved(&mut self, who: usize) {
        if !self.moved.contains(&(who as u32)) {
            self.moved.push(who as u32);
        }
    }
}

/// Bodies `a` and `b` (at indices `ia`, `ib`), the one that moved farther
/// this tick first (a turn counts as a metre).
fn farther_first(a: &Body, b: &Body, ia: usize, ib: usize) -> [usize; 2] {
    let away = |u: &Body| u.pos.distance(u.start) + if u.turned() { 1.0 } else { 0.0 };
    if away(a) >= away(b) { [ia, ib] } else { [ib, ia] }
}

/// How hard a unit resists pushes (a factor on its footprint area) and
/// whether it steps out of walkers' way. Walking units push and get pushed
/// evenly, even on StandGround (stance or Hold resist only while standing).
fn resistance(moving: bool, holds_position: bool, fighting: bool) -> (f32, bool) {
    if moving || (fighting && !holds_position) {
        (1.0, false)
    } else if holds_position {
        (HOLD_RESIST, false)
    } else {
        (IDLE_RESIST, true)
    }
}

/// Grow each walker's crowd radius to fit everyone of its owner heading to
/// (or arrived at) the same goal cell, and every other unit of its owner
/// standing about the goal ([`stands_by`]: say the groupmates of an
/// earlier order to that spot, arrived and gone idle there). `standing` is
/// scratch.
fn size_crowds(groups: &mut Vec<(PlayerId, i32, i32)>, standing: &mut Vec<(PlayerId, Vec2)>, ground: &mut [Body], air: &mut [Body]) {
    let key = |b: &Body, goal: Vec2| {
        let cell = (goal / CROWD_GOAL_TOLERANCE).floor();
        (b.owner, cell.x as i32, cell.y as i32)
    };
    groups.clear();
    groups.extend(ground.iter().chain(air.iter()).filter_map(|b| b.heading_to.or(b.arrived_at).map(|g| key(b, g))));
    groups.sort_unstable();
    standing.clear();
    standing.extend(ground.iter().chain(air.iter()).filter(|b| stands_by(b)).map(|b| (b.owner, b.pos)));
    standing.sort_unstable_by_key(|(owner, _)| *owner);
    for b in ground.iter_mut().chain(air.iter_mut()) {
        let Some(goal) = b.heading_to else { continue };
        let k = key(b, goal);
        let mates = groups.partition_point(|g| *g <= k) - groups.partition_point(|g| *g < k);
        let area = 4.0 * b.half.x * b.half.y;
        let grown = |n: usize| (n as f32 * area / (CROWD_PACKING * std::f32::consts::PI)).sqrt() + CROWD_MARGIN;
        // Units of its owner standing about the goal fill the crowd too:
        // those within its radius, which they grow (twice over settles it
        // for the blobs goals gather).
        let own = &standing[standing.partition_point(|(o, _)| *o < b.owner)..standing.partition_point(|(o, _)| *o <= b.owner)];
        let mut others = 0;
        for _ in 0..2 {
            let reach = b.crowd_radius + grown(mates + others);
            others = own.iter().filter(|(_, at)| at.distance(goal) <= reach).count();
        }
        b.crowd_radius += grown(mates + others);
    }
}

/// `body` stands where it is with no point to walk to or arrived at (idle,
/// or at work on something): a walker heading to a point it stands near
/// counts it as part of the crowd there (see [`same_crowd`]).
#[inline]
fn stands_by(body: &Body) -> bool {
    body.yields && body.heading_to.is_none() && body.arrived_at.is_none()
}

/// One pass over every overlapping pair, each pair once, with idle units
/// also stepping aside when `sidesteps`. Returns the deepest overlap it
/// resolved.
fn separate_pairs(bodies: &mut [Body], contacts: &mut Contacts, origin: Vec2, size: Vec2, softness: f32, sidesteps: bool) -> f32 {
    let mut deepest = 0.0f32;
    if bodies.len() < 2 {
        return deepest;
    }
    contacts.refresh(bodies, origin, size);
    for &(a, b) in &contacts.pairs {
        let (a, b) = (a as usize, b as usize);
        let ((amin, amax), (bmin, bmax)) = (bodies[a].bounds(), bodies[b].bounds());
        if amin.x >= bmax.x || bmin.x >= amax.x || amin.y >= bmax.y || bmin.y >= amax.y {
            continue;
        }
        deepest = deepest.max(resolve_pair(bodies, a, b, softness, sidesteps));
    }
    deepest
}

/// Push two overlapping units apart, each moving in inverse proportion to
/// its resistance (and an idle one shoved by a walker stepping aside when
/// `sidesteps`). Returns the overlap depth (0 when apart).
fn resolve_pair(bodies: &mut [Body], a: usize, b: usize, softness: f32, sidesteps: bool) -> f32 {
    let (fa, fb) = (bodies[a].frame(), bodies[b].frame());
    let Some(m) = fa.mtv(&fb) else { return 0.0 };
    let depth = m.length();
    let m = back_out(&bodies[a], &bodies[b], &fa, &fb)
        .or_else(|| fall_in_line(&bodies[a], &bodies[b], &fa, &fb, m))
        .or_else(|| slip_apart(&bodies[a], &bodies[b], &fa, &fb, m))
        .unwrap_or(m);
    let push = with_skin(m) * softness;
    let (resist_a, resist_b) = (pushed_resist(&bodies[a], &bodies[b], push), pushed_resist(&bodies[b], &bodies[a], -push));
    let share_a = resist_b / (resist_a + resist_b);
    let (step_a, step_b) = if sidesteps {
        let (len_a, len_b) = (push.length() * share_a, push.length() * (1.0 - share_a));
        (sidestep(&bodies[a], &bodies[b], len_a), sidestep(&bodies[b], &bodies[a], len_b))
    } else {
        (Vec2::ZERO, Vec2::ZERO)
    };
    bodies[a].pos += push * share_a + step_a;
    bodies[b].pos -= push * (1.0 - share_a) - step_b;
    note_crowd(bodies, a, b);
    note_crowd(bodies, b, a);
    for (u, v) in [(a, b), (b, a)] {
        if queues_behind(&bodies[u], &bodies[v]) {
            bodies[u].waits = true;
        }
    }
    depth
}

/// `body` walks into `ahead`, a walker going its way, ahead of it along
/// both their headings, that moves on: it waits its turn in a queue (see
/// the module docs).
#[inline]
fn queues_behind(body: &Body, ahead: &Body) -> bool {
    let rel = ahead.pos - body.pos;
    walking(body) && walking(ahead) && ahead.moves_on && body.heading.dot(ahead.heading) >= IN_FILE && rel.dot(body.heading) > 0.0 && rel.dot(ahead.heading) > 0.0
}

/// How hard `body` resists being moved along `push` by `other` this pass:
/// by area and state (barely when it gives way to it, like an idle unit
/// when it fights in place and an ally walks into it), many times over
/// when `push` drives it back into a wall it is braced against (a push
/// along the wall, or straight back out of a gap, meets no extra
/// resistance).
#[inline]
fn pushed_resist(body: &Body, other: &Body, push: Vec2) -> f32 {
    let resist = if yields_to(body, other) || backs_off_for(body, other) {
        body.resist * GIVE_WAY_RESIST
    } else if makes_way_for(body, other) {
        body.resist * IDLE_RESIST
    } else {
        body.resist
    };
    if body.pushed_into_wall(push) { resist * HOLD_RESIST } else { resist }
}

/// `body` fights in place and `walker`, an ally, walks into it.
#[inline]
fn makes_way_for(body: &Body, walker: &Body) -> bool {
    body.fights_in_place && walker.team == body.team && walking(walker)
}

/// `yielder` gives way and `walker`, which does not, walks at it.
#[inline]
fn yields_to(yielder: &Body, walker: &Body) -> bool {
    yielder.gives_way && !walker.gives_way && walking(walker) && (yielder.pos - walker.pos).dot(walker.heading) > 0.0
}

/// `body` gives way, and so does `leader`, a unit ahead of it: backed out,
/// the leader backs the file out behind it instead of being squeezed
/// between it and the walker it gives way to.
#[inline]
fn backs_off_for(body: &Body, leader: &Body) -> bool {
    body.gives_way && leader.gives_way && walking(leader) && (leader.pos - body.pos).dot(body.heading) > 0.0
}

/// One of the pair gives way to the other, is braced and stands in the
/// other's lane (too little room to pass beside it): a push along the
/// walker's heading, which walks the yielder back out of the gap, at most
/// [`BACK_OUT`] per pass (later passes go on; else `None`: the usual
/// push-out). Like an MTV, it moves `a` out of `b`. Even a pair already
/// squeezed side by side in the gap is parted so, instead of jamming there
/// for good; a pair in two lanes of a wider passage only grazing each
/// other is pushed apart across, into its lanes, and passes.
fn back_out(a: &Body, b: &Body, fa: &Frame, fb: &Frame) -> Option<Vec2> {
    let (yielder, walker) = if yields_to(a, b) {
        (a, b)
    } else if yields_to(b, a) {
        (b, a)
    } else {
        return None;
    };
    let lane = (yielder.half.x + walker.half.x) * SAME_LANE;
    if !yielder.braced() || walker.heading.perp_dot(yielder.pos - walker.pos).abs() >= lane {
        return None;
    }
    let axis = walker.heading;
    let depth = fa.overlap_along(fb, axis);
    if depth <= 0.0 {
        return None;
    }
    let away = if (fa.center - fb.center).dot(axis) < 0.0 { -axis } else { axis };
    Some(away * depth.min(BACK_OUT))
}

/// Two walkers going the same way squeezed abreast into a gap too narrow
/// for both (the push-out `mtv`, sideways, would drive each into a wall it
/// is braced against): the push that parts
/// them along their way instead, at most [`BACK_OUT`] per pass, so they fall
/// into single file rather than jamming side by side. Like an MTV, it
/// moves `a` out of `b`.
fn fall_in_line(a: &Body, b: &Body, fa: &Frame, fb: &Frame, mtv: Vec2) -> Option<Vec2> {
    // Only when the push-out would drive both into their walls: no room abreast.
    let pinned = a.pushed_into_wall(mtv) && b.pushed_into_wall(-mtv);
    // Not in a crowd either: there the units ahead and behind have no room
    // to give, and parting the pair along its way only packs the column.
    let alone = a.contacts <= MAX_FILE_CONTACTS && b.contacts <= MAX_FILE_CONTACTS;
    // Nor with a giant or an engine filling the gap: it is not shoved along.
    let small = a.half.x.max(b.half.x) <= FILE_HALF_WIDTH;
    let same_way = walking(a) && walking(b) && a.heading.dot(b.heading) >= IN_FILE;
    if !(pinned && alone && small && same_way) {
        return None;
    }
    let axis = (a.heading + b.heading).try_normalize()?;
    if mtv.dot(axis).abs() > IN_FILE * mtv.length() {
        // Already parted along their way.
        return None;
    }
    let depth = fa.overlap_along(fb, axis);
    if depth <= 0.0 {
        return None;
    }
    let away = if (fa.center - fb.center).dot(axis) < 0.0 { -axis } else { axis };
    Some(away * depth.min(BACK_OUT))
}

/// Two units squeezed between two walls (the push-out `mtv` would drive
/// each into a wall it is braced against: a gap too narrow for both side
/// by side, whatever they are doing): the push that parts them along
/// another axis of either box that drives neither into its wall, the
/// shallowest such, at most [`BACK_OUT`] per pass. Otherwise every pass
/// would push them into the walls and the walls straight back, and the
/// pair would end the tick as deep as it began. Like an MTV, it moves `a`
/// out of `b`.
fn slip_apart(a: &Body, b: &Body, fa: &Frame, fb: &Frame, mtv: Vec2) -> Option<Vec2> {
    if !(a.pushed_into_wall(mtv) && b.pushed_into_wall(-mtv)) {
        return None;
    }
    let across = mtv.normalize_or_zero();
    [fa.right, fa.forward, fb.right, fb.forward]
        .into_iter()
        .filter(|axis| axis.dot(across).abs() < SAME_AXIS)
        .filter_map(|axis| {
            let depth = fa.overlap_along(fb, axis);
            let push = (if (fa.center - fb.center).dot(axis) < 0.0 { -axis } else { axis }) * depth;
            (depth > 0.0 && !a.pushed_into_wall(push) && !b.pushed_into_wall(-push)).then_some((depth, push))
        })
        .min_by(|p, q| p.0.total_cmp(&q.0))
        .map(|(depth, push)| push * (depth.min(BACK_OUT) / depth))
}

/// Flag the bodies that began the tick overlapping another deeper than
/// [`FINAL_TOLERANCE`] (as they stood and faced then), among the candidate
/// `pairs` (every pair overlapping now is one), and count the ticks in a
/// row they have; returns how many are flagged.
fn mark_overlapped(bodies: &mut [Body], pairs: &[(u32, u32)]) -> u64 {
    for &(a, b) in pairs {
        if start_depth(&bodies[a as usize], &bodies[b as usize]) > FINAL_TOLERANCE {
            bodies[a as usize].overlapped = true;
            bodies[b as usize].overlapped = true;
        }
    }
    let mut flagged = 0;
    for b in bodies.iter_mut() {
        b.overlapped_for = if b.overlapped { b.overlapped_for.saturating_add(1) } else { 0 };
        flagged += u64::from(b.overlapped);
    }
    flagged
}

/// Depth of the overlap of two bodies' boxes as they stood and faced at the
/// start of the tick (0 when apart).
#[inline]
fn start_depth(a: &Body, b: &Body) -> f32 {
    a.start_frame(a.start).mtv(&b.start_frame(b.start)).map_or(0.0, |m| m.length())
}

/// Decide who gives way this tick (see the module docs), from the
/// candidate pairs: the younger of two walkers meeting head-on, then, down
/// its file, the walkers following close behind it in its lane.
fn mark_giving_way(bodies: &mut [Body], pairs: &[(u32, u32)]) {
    for &(a, b) in pairs {
        let (a, b) = (a as usize, b as usize);
        for i in [a, b] {
            bodies[i].contacts = bodies[i].contacts.saturating_add(1);
        }
        if head_on(&bodies[a], &bodies[b]) {
            // One that gave way lately gives way again: a file keeps giving
            // way to the column it met until that column has passed.
            let young = match (bodies[a].gave_way, bodies[b].gave_way) {
                (true, false) => a,
                (false, true) => b,
                _ if younger(&bodies[a], &bodies[b]) => a,
                _ => b,
            };
            bodies[young].gives_way = true;
        }
    }
    for _ in 0..MAX_FILE {
        let mut spread = false;
        for &(a, b) in pairs {
            for (ahead, behind) in [(a as usize, b as usize), (b as usize, a as usize)] {
                if bodies[ahead].gives_way && !bodies[behind].gives_way && follows(&bodies[behind], &bodies[ahead]) {
                    bodies[behind].gives_way = true;
                    spread = true;
                }
            }
        }
        if !spread {
            break;
        }
    }
}

/// `a` is a younger unit than `b` (got its movement state later). Not the
/// entity order the bodies are sorted in: that is opaque bits, a recycled
/// slot's generation first, and does not follow spawn order.
#[inline]
fn younger(a: &Body, b: &Body) -> bool {
    a.born > b.born
}

/// `body` is walking (not standing, so neither idle nor holding).
#[inline]
fn walking(body: &Body) -> bool {
    !body.yields && body.heading != Vec2::ZERO
}

/// Two walkers heading at each other, each with the other ahead of it.
#[inline]
fn head_on(a: &Body, b: &Body) -> bool {
    let rel = b.pos - a.pos;
    walking(a)
        && walking(b)
        && a.heading.dot(b.heading) <= HEAD_ON
        && rel.dot(a.heading) > 0.0
        && -rel.dot(b.heading) > 0.0
}

/// `behind` walks the same way as `ahead`, lined up behind it (not in the
/// next lane: units passing side by side do not give way for each other).
#[inline]
fn follows(behind: &Body, ahead: &Body) -> bool {
    let rel = ahead.pos - behind.pos;
    let lane = (behind.half.x + ahead.half.x) * SAME_LANE;
    walking(behind) && behind.heading.dot(ahead.heading) >= IN_FILE && rel.dot(behind.heading) > 0.0 && behind.heading.perp_dot(rel).abs() < lane
}

/// `walker` is heading to the point where groupmate `settled` already
/// arrived, or where it stands (see [`stands_by`]) within the walker's
/// crowd radius: the crowd at that point is already there.
#[inline]
fn same_crowd(walker: &Body, settled: &Body) -> bool {
    let Some(to) = walker.heading_to.filter(|_| walker.owner == settled.owner) else { return false };
    match settled.arrived_at {
        Some(at) => to.distance(at) <= CROWD_GOAL_TOLERANCE,
        None => stands_by(settled) && settled.pos.distance(to) <= walker.crowd_radius,
    }
}

/// Extra sideways step of `idle` (when it stands idle, gives way, or fights
/// in place in an ally's way) shoved `amount` by `walker`: away from the
/// walker's line of travel (to the walker's right when dead ahead).
#[inline]
fn sidestep(idle: &Body, walker: &Body, amount: f32) -> Vec2 {
    if !(idle.yields || idle.gives_way || makes_way_for(idle, walker)) || walker.heading == Vec2::ZERO {
        return Vec2::ZERO;
    }
    let right = Vec2::new(walker.heading.y, -walker.heading.x);
    let side = if walker.heading.perp_dot(idle.pos - walker.pos) > 0.0 { -1.0 } else { 1.0 };
    right * side * amount * SIDESTEP
}

/// Is `p` in a passage: ground a unit of `mobility` cannot use (terrain,
/// static obstacles) within [`PASSAGE_CELLS`] on both sides of it, across
/// x or across z?
fn in_passage(nav: &NavGrid, p: Vec2, mobility: u16) -> bool {
    let shut = |q: Vec2| {
        let c = nav.cell_of(q);
        !nav.terrain_passable(c, mobility) || nav.is_walk_blocked(c)
    };
    let walled = |d: Vec2| (1..=PASSAGE_CELLS).any(|k| shut(p + d * (k as f32 * nav.cell_size())));
    (walled(Vec2::X) && walled(Vec2::NEG_X)) || (walled(Vec2::Y) && walled(Vec2::NEG_Y))
}

/// `walker` touched `other`: if `other` already arrived where `walker` is
/// heading (same owner) and `walker` is within the crowd's radius, `walker`
/// is part of the crowd at the goal.
fn note_crowd(bodies: &mut [Body], walker: usize, other: usize) {
    let w = &bodies[walker];
    if same_crowd(w, &bodies[other]) && w.heading_to.is_some_and(|to| w.pos.distance(to) <= w.crowd_radius) {
        bodies[walker].crowd = true;
    }
}

#[inline]
fn with_skin(m: Vec2) -> Vec2 {
    let len = m.length();
    if len > 0.0 { m * ((len + SKIN) / len) } else { m }
}

/// Fully push a ground unit out of every overlapping static obstacle except
/// its own team's gates.
fn push_out_static(b: &mut Body, reg: &ObstacleRegistry, seen: &mut [u32], stamp: u32) {
    let (min, max) = b.bounds();
    let (x0, y0, x1, y1) = reg.cell_range(min, max);
    for y in y0..=y1 {
        for x in x0..=x1 {
            for &slot in reg.cell(x, y) {
                let Some(mark) = seen.get_mut(slot as usize) else { continue };
                if *mark == stamp {
                    continue;
                }
                *mark = stamp;
                let Some(sb) = reg.get(slot) else { continue };
                if sb.gate_team == Some(b.team) {
                    continue;
                }
                let (min, max) = b.bounds();
                if sb.min.x >= max.x || min.x >= sb.max.x || sb.min.y >= max.y || min.y >= sb.max.y {
                    continue;
                }
                let me = b.frame();
                if let Some(m) = me.mtv(&sb.frame) {
                    b.pos += with_skin(corner_slip(&me, &sb.frame, m, b.heading).unwrap_or(m));
                }
            }
        }
    }
}

/// The sideways step that clears `me` of `wall` when the push-out `mtv`
/// would shove a unit walking along `heading` straight back, if one no
/// longer than [`CORNER_SLIP`] does: it slips round the corner.
fn corner_slip(me: &Frame, wall: &Frame, mtv: Vec2, heading: Vec2) -> Option<Vec2> {
    if heading == Vec2::ZERO || mtv.dot(heading) > -HEAD_ON_PUSH * mtv.length() {
        return None;
    }
    let away = me.center - wall.center;
    [me.right, me.forward, wall.right, wall.forward]
        .into_iter()
        .filter(|axis| axis.dot(heading).abs() <= HEAD_ON_PUSH)
        .map(|axis| (me.overlap_along(wall, axis), if away.dot(axis) < 0.0 { -axis } else { axis }))
        .filter(|(depth, _)| *depth > 0.0 && *depth <= CORNER_SLIP)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(depth, dir)| dir * depth)
}

/// The box of any nav grid cell, moved onto a cell with [`CellBoxes::at`].
struct CellBoxes {
    template: Frame,
}

impl CellBoxes {
    fn new(nav: &NavGrid) -> CellBoxes {
        CellBoxes { template: Obb::new(Vec2::ZERO, Vec2::splat(nav.cell_size() * 0.5), 0.0).frame() }
    }

    #[inline]
    fn at(&self, center: Vec2) -> Frame {
        Frame { center, ..self.template }
    }
}

/// Fully push a ground unit out of terrain cells it cannot enter (the map
/// edge included).
fn push_out_terrain(b: &mut Body, nav: &NavGrid, cells: &CellBoxes) {
    if !b.on_valid_ground {
        return;
    }
    let (min, max) = b.bounds();
    let (c0, c1) = (nav.cell_of(min), nav.cell_of(max));
    for y in c0.y..=c1.y {
        for x in c0.x..=c1.x {
            let c = IVec2::new(x, y);
            if nav.terrain_passable(c, b.mobility) {
                continue;
            }
            let (me, cell) = (b.frame(), cells.at(nav.cell_center(c)));
            if let Some(m) = me.mtv(&cell) {
                // A corner of the terrain only: the step leads onto open ground.
                let open = |step: &Vec2| nav.terrain_passable(nav.cell_of(cell.center + step.normalize_or_zero() * nav.cell_size()), b.mobility);
                b.pos += with_skin(corner_slip(&me, &cell, m, b.heading).filter(open).unwrap_or(m));
            }
        }
    }
}

/// Inside the map, never with the centre on terrain the unit cannot enter
/// and, where a spot without it is at hand, not wedged into a static
/// obstacle: slide back along one axis, else stay where (and as) the tick
/// started, else (trapped inside since before the tick) the nearest open
/// spot. The terrain rule always holds.
/// Returns whether the unit was set down elsewhere (trapped in an obstacle);
/// `crowded` tells whether a box there would overlap another unit.
fn keep_on_valid_ground(b: &mut Body, nav: &NavGrid, reg: &ObstacleRegistry, crowded: &dyn Fn(&Frame) -> bool) -> bool {
    b.pos = clamp_to_map(b.pos, b.ext, nav);
    let ground = |p: Vec2| !b.on_valid_ground || nav.terrain_passable(nav.cell_of(p), b.mobility);
    let frame = b.frame();
    let candidates = [b.pos, Vec2::new(b.pos.x, b.start.y), Vec2::new(b.start.x, b.pos.y), b.start];
    if let Some(&p) = candidates.iter().find(|p| ground(**p) && static_depth(b, &frame, **p, reg) <= WEDGE_TOLERANCE) {
        b.pos = p;
        return false;
    }
    // Turning in a tight spot can wedge a corner in too: undo the whole tick.
    let start = b.start_frame(b.start);
    if static_depth(b, &start, b.start, reg) <= WEDGE_TOLERANCE {
        b.pos = b.start;
        b.unturn();
    } else if let Some(p) = open_spot_near(b, &frame, nav, reg, crowded) {
        b.pos = clamp_to_map(p, b.ext, nav);
        return true;
    } else if let Some(&p) = candidates.iter().find(|p| ground(**p)) {
        b.pos = p;
    }
    false
}

/// The nearest nav cell centre within [`EJECT_RADIUS`] where `b`, with box
/// `frame`, stands on terrain it can enter clear of every static obstacle,
/// and clear of other units (`crowded`) if there is such a cell: two units
/// under one new foundation are set down side by side, not on one spot.
fn open_spot_near(b: &Body, frame: &Frame, nav: &NavGrid, reg: &ObstacleRegistry, crowded: &dyn Fn(&Frame) -> bool) -> Option<Vec2> {
    let c = nav.cell_of(b.pos);
    let rings = (EJECT_RADIUS / nav.cell_size()).ceil() as i32;
    let open = |p: Vec2| nav.terrain_passable(nav.cell_of(p), b.mobility) && static_depth(b, frame, p, reg) <= WEDGE_TOLERANCE;
    let nearest = |free: &dyn Fn(Vec2) -> bool| {
        (1..=rings).find_map(|r| {
            let ring = (-r..=r).flat_map(|dy| (-r..=r).map(move |dx| IVec2::new(dx, dy))).filter(|d| d.x.abs() == r || d.y.abs() == r);
            ring.map(|d| nav.cell_center(c + d))
                .filter(|p| nav.in_bounds(nav.cell_of(*p)) && open(*p) && free(*p))
                .min_by(|p, q| p.distance_squared(b.pos).total_cmp(&q.distance_squared(b.pos)))
        })
    };
    nearest(&|p| !crowded(&Frame { center: p, ..*frame })).or_else(|| nearest(&|_| true))
}

/// Would a box at `frame` overlap a ground unit other than body `who`
/// deeper than [`FINAL_TOLERANCE`]? `moved`: bodies the contact grid may
/// not list where they are now.
fn crowds(bodies: &[Body], contacts: &Contacts, moved: &[u32], who: usize, frame: &Frame) -> bool {
    let ext = frame.extents();
    let (min, max) = (frame.center - ext, frame.center + ext);
    let hit = |j: usize| {
        let other = &bodies[j];
        let (omin, omax) = other.bounds();
        let apart = omin.x >= max.x || min.x >= omax.x || omin.y >= max.y || min.y >= omax.y;
        j != who && !apart && frame.mtv(&other.frame()).is_some_and(|m| m.length() > FINAL_TOLERANCE)
    };
    contacts.any_near(min, max, |j| hit(j as usize)) || moved.iter().any(|&j| hit(j as usize))
}

/// Deepest overlap of `b` with a static obstacle (its own team's gates
/// excepted) when its box `frame` stands at `at`.
fn static_depth(b: &Body, frame: &Frame, at: Vec2, reg: &ObstacleRegistry) -> f32 {
    let me = Frame { center: at, ..*frame };
    let ext = me.extents();
    let (min, max) = (at - ext, at + ext);
    let (x0, y0, x1, y1) = reg.cell_range(min, max);
    let mut deepest = 0.0f32;
    for y in y0..=y1 {
        for x in x0..=x1 {
            for sb in reg.cell(x, y).iter().filter_map(|slot| reg.get(*slot)) {
                let apart = sb.min.x >= max.x || min.x >= sb.max.x || sb.min.y >= max.y || min.y >= sb.max.y;
                if apart || sb.gate_team == Some(b.team) {
                    continue;
                }
                if let Some(m) = me.mtv(&sb.frame) {
                    deepest = deepest.max(m.length());
                }
            }
        }
    }
    deepest
}

/// `p` moved so that a box with bounds half extents `ext` there stays inside the map.
fn clamp_to_map(p: Vec2, ext: Vec2, nav: &NavGrid) -> Vec2 {
    let lo = nav.origin() + ext;
    let hi = nav.origin() + nav.world_size() - ext;
    Vec2::new(p.x.max(lo.x).min(hi.x.max(lo.x)), p.y.max(lo.y).min(hi.y.max(lo.y)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_standing_units_hold_their_ground() {
        assert_eq!(resistance(false, true, false), (HOLD_RESIST, false), "standing on StandGround/Hold");
        assert_eq!(resistance(false, true, true), (HOLD_RESIST, false), "holding while shooting");
        assert_eq!(resistance(true, true, false), (1.0, false), "walking on StandGround pushes like anyone");
        assert_eq!(resistance(true, false, false), (1.0, false));
        assert_eq!(resistance(false, false, true), (1.0, false), "fighting in place");
        assert_eq!(resistance(false, false, false), (IDLE_RESIST, true), "idle units yield");
    }

    /// A worker-sized walker at `pos` heading along `heading` (zero: idle).
    fn body(id: u32, pos: Vec2, heading: Vec2, team: u8) -> Body {
        let frame = Obb::new(pos, Vec2::splat(0.3), 0.0).frame();
        let idle = heading == Vec2::ZERO;
        Body {
            entity: Entity::from_raw_u32(id).expect("entity"),
            born: u64::from(id),
            pos,
            start: pos,
            half: Vec2::splat(0.3),
            yaw: 0.0,
            right: frame.right,
            forward: frame.forward,
            start_yaw: 0.0,
            restore_yaw: false,
            ext: frame.extents(),
            resist: 0.36 * if idle { IDLE_RESIST } else { 1.0 },
            yields: idle,
            gives_way: false,
            gave_way: false,
            waits: false,
            moves_on: false,
            heading,
            mobility: 1,
            team,
            owner: PlayerId(team),
            heading_to: None,
            arrived_at: None,
            crowd_radius: 0.0,
            crowd: false,
            on_valid_ground: true,
            clear_at: None,
            braces: [Vec2::ZERO; 2],
            fights_in_place: false,
            contacts: 0,
            wedge: f32::NAN,
            may_relocate: true,
            relocation_failed: false,
            placed_anew: false,
            solved: pos,
            held: false,
            hold_from: pos,
            hold_t: 0.0,
            backed_off: false,
            overlapped: false,
            overlapped_for: 0,
            walled: 0.0,
            ejected: false,
        }
    }

    /// A unit a wall pushed back resists being pushed into it again, not
    /// being pushed along it or away from it; two walls brace it both ways.
    #[test]
    fn bracing_resists_only_pushes_into_the_wall() {
        let mut b = body(1, Vec2::ZERO, Vec2::ZERO, 0);
        assert!(!b.braced() && !b.pushed_into_wall(Vec2::NEG_X));
        b.brace(Vec2::new(0.02, 0.0)); // a wall on its left pushed it right
        b.brace(Vec2::new(0.05, 0.001)); // the same wall again
        assert!(b.braced() && b.braces[1] == Vec2::ZERO, "{:?}", b.braces);
        assert!(b.pushed_into_wall(Vec2::new(-0.1, 0.0)));
        assert!(!b.pushed_into_wall(Vec2::new(0.0, 0.1)), "along the wall");
        assert!(!b.pushed_into_wall(Vec2::new(0.1, 0.0)), "away from it");
        b.brace(Vec2::new(-0.03, 0.0)); // a wall on its right too: a corridor
        assert!(b.pushed_into_wall(Vec2::new(0.1, 0.0)) && !b.pushed_into_wall(Vec2::new(0.0, -0.1)));
        let walker = body(2, Vec2::new(0.0, -0.5), Vec2::Y, 0);
        let along = pushed_resist(&b, &walker, Vec2::new(0.0, 0.1));
        let into = pushed_resist(&b, &walker, Vec2::new(0.1, 0.0));
        assert!((into / along - HOLD_RESIST).abs() < 1e-3, "{along} {into}");
    }

    /// Two walkers going the same way, pinned abreast between two walls, are
    /// parted along their way (into single file); free ones, or ones parted
    /// along their way already, keep the usual push-out.
    #[test]
    fn walkers_pinned_abreast_fall_in_line() {
        let mut a = body(1, Vec2::new(0.0, 0.0), Vec2::X, 0);
        let mut b = body(2, Vec2::new(0.2, 0.5), Vec2::X, 0);
        let (fa, fb) = (a.frame(), b.frame());
        let mtv = fa.mtv(&fb).expect("overlapping");
        assert!(mtv.x.abs() < 1e-6, "sideways: {mtv}");
        assert_eq!(fall_in_line(&a, &b, &fa, &fb, mtv), None, "not braced: room to step aside");
        a.brace(Vec2::Y); // a wall below a
        b.brace(Vec2::NEG_Y); // a wall above b
        let push = fall_in_line(&a, &b, &fa, &fb, mtv).expect("pinned abreast");
        assert!(push.y.abs() < 1e-6 && push.x < 0.0 && push.length() <= BACK_OUT + 1e-6, "the rear one falls back: {push}");
        assert_eq!(fall_in_line(&a, &b, &fa, &fb, Vec2::new(-0.1, 0.0)), None, "already along their way");
        b.heading = Vec2::NEG_X;
        assert_eq!(fall_in_line(&a, &b, &fa, &fb, mtv), None, "not the same way");
    }

    /// A foot unit of half extents `half` standing at `pos` facing `yaw`
    /// (as at the start of the tick), id `id`.
    fn sized(id: u32, pos: Vec2, half: Vec2, yaw: f32) -> Body {
        let frame = Obb::new(pos, half, yaw).frame();
        Body {
            half,
            yaw,
            start_yaw: yaw,
            right: frame.right,
            forward: frame.forward,
            ext: frame.extents(),
            resist: 4.0 * half.x * half.y * IDLE_RESIST,
            mobility: Mobility::of(MoveFlag::Foot).0,
            ..body(id, pos, Vec2::ZERO, 0)
        }
    }

    const PEASANT: Vec2 = Vec2::splat(0.3);
    const RIDER: Vec2 = Vec2::new(0.45, 0.95);
    const RAM: Vec2 = Vec2::new(0.9, 1.6);

    /// A 40 m grass grid with these solid obstacles, and their registry.
    fn surroundings(obstacles: &[Obb]) -> (NavGrid, ObstacleRegistry) {
        let data = af_core::testing::test_data();
        let mut nav = af_core::testing::new_nav(&data, 40, 40, data.terrain_id("grass").expect("grass"));
        let obstacles: Vec<Obstacle> = obstacles.iter().map(|obb| Obstacle { obb: *obb, gate_team: None }).collect();
        let reg = ObstacleRegistry::with(&mut nav, &obstacles);
        (nav, reg)
    }

    /// Part the deep pairs among `bodies`: what it did, and where they ended.
    fn part(bodies: Vec<Body>, nav: &NavGrid, reg: &ObstacleRegistry) -> (Parting, Vec<Body>) {
        let mut s = CollisionScratch { ground: bodies, ..CollisionScratch::default() };
        let done = s.part_deep_pairs(nav, reg);
        (done, s.ground)
    }

    fn deepest_pair(bodies: &[Body]) -> f32 {
        let mut deepest = 0.0f32;
        for (i, a) in bodies.iter().enumerate() {
            for b in &bodies[i + 1..] {
                deepest = deepest.max(pair_depth(a, b));
            }
        }
        deepest
    }

    fn deepest_static(bodies: &[Body], reg: &ObstacleRegistry) -> f32 {
        bodies.iter().map(|b| static_depth(b, &b.frame(), b.pos, reg)).fold(0.0, f32::max)
    }

    /// Walls along y = `y0` and `y1` (their inner faces) from x = 10 to 30.
    fn corridor(y0: f32, y1: f32) -> [Obb; 2] {
        [Obb::new(Vec2::new(20.0, y0 - 0.5), Vec2::new(10.0, 0.5), 0.0), Obb::new(Vec2::new(20.0, y1 + 0.5), Vec2::new(10.0, 0.5), 0.0)]
    }

    /// Run the final word on pairs over `bodies` for `ticks` ticks in a row,
    /// each starting where the last one ended (as the game does): what each
    /// tick did and the farthest it moved a unit, and where they ended.
    fn settle(mut bodies: Vec<Body>, nav: &NavGrid, reg: &ObstacleRegistry, ticks: usize) -> (Vec<(Parting, f32)>, Vec<Body>) {
        let mut s = CollisionScratch::default();
        let (mut log, mut waits) = (Vec::new(), Vec::new());
        for _ in 0..ticks {
            for b in &mut bodies {
                let frame = b.start_frame(b.pos);
                (b.start, b.start_yaw, b.yaw, b.right, b.forward, b.ext) = (b.pos, b.yaw, b.yaw, frame.right, frame.forward, frame.extents());
                (b.held, b.hold_t, b.overlapped, b.restore_yaw, b.wedge, b.relocation_failed) = (false, 0.0, false, false, f32::NAN, false);
            }
            s.ground = std::mem::take(&mut bodies);
            s.ground_contacts.invalidate();
            s.ground_contacts.refresh(&s.ground, nav.origin(), nav.world_size());
            mark_overlapped(&mut s.ground, &s.ground_contacts.pairs);
            let done = s.part_deep_pairs(nav, reg);
            let farthest = s.ground.iter().map(|b| b.pos.distance(b.start)).fold(0.0, f32::max);
            log.push((done, farthest));
            bodies = std::mem::take(&mut s.ground);
            // The relocation wait, as the system keeps it between ticks.
            waits.resize(bodies.len(), 0);
            for (b, wait) in bodies.iter_mut().zip(&mut waits) {
                *wait = match (b.relocation_failed, b.overlapped) {
                    (true, true) => OVERLAPPED_RETRY_TICKS,
                    (true, false) => RELOCATE_RETRY_TICKS,
                    (false, _) => wait.saturating_sub(1),
                };
                b.may_relocate = *wait == 0;
                b.placed_anew = false;
            }
        }
        (log, bodies)
    }

    /// A jumble of peasants, riders and rams beside a building, dropped
    /// anyhow (all of them overlapping as the first tick begins): within a
    /// second no pair overlaps past the tolerance and nobody stands in the
    /// building; no tick makes the deepest pair deeper; and nobody jumps
    /// farther in a tick than a push plus a short set-down, but for one
    /// stranded for long.
    #[test]
    fn parting_leaves_no_pair_too_deep() {
        let house = Obb::new(Vec2::new(26.0, 20.0), Vec2::splat(3.0), 0.3);
        let (nav, reg) = surroundings(&[house]);
        let mut seed = 7u32;
        let mut next = || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        let mut bodies = Vec::new();
        while bodies.len() < 24 {
            let half = [PEASANT, PEASANT, RIDER, RAM][(next() * 4.0) as usize % 4];
            let b = sized(bodies.len() as u32 + 1, Vec2::new(8.0 + next() * 14.0, 13.0 + next() * 14.0), half, next() * 6.0);
            if static_depth(&b, &b.frame(), b.pos, &reg) == 0.0 {
                bodies.push(b);
            }
        }
        let before = deepest_pair(&bodies);
        assert!(before > 0.5, "a jumble ({before})");
        let (log, after) = settle(bodies, &nav, &reg, SIM_HZ as usize);
        assert!(deepest_pair(&after) <= FINAL_TOLERANCE, "{} after {log:?}", deepest_pair(&after));
        assert!(deepest_static(&after, &reg) <= WEDGE_TOLERANCE);
        assert_eq!(log.last().map(|(done, _)| done.left), Some(0));
        for (tick, (done, farthest)) in log.iter().enumerate() {
            let bound = if tick + 1 >= STRANDED_TICKS as usize { EJECT_RADIUS } else { MAX_SLIDE + SET_DOWN_RADIUS + RAM.y };
            assert!(*farthest <= bound + 1e-3, "tick {tick}: {farthest} m ({done:?})");
        }
    }

    /// Two riders side by side in a corridor too narrow for both are slid
    /// apart along it, not pushed into its walls nor set down elsewhere.
    #[test]
    fn a_pair_squeezed_between_walls_slides_apart_along_them() {
        let (nav, reg) = surroundings(&corridor(19.5, 20.9));
        let along = std::f32::consts::FRAC_PI_2; // long axis along x
        let bodies = vec![sized(1, Vec2::new(20.0, 20.0), RIDER, along), sized(2, Vec2::new(21.0, 20.4), RIDER, along)];
        let (done, after) = part(bodies, &nav, &reg);
        assert_eq!((done.pushed, done.relocated, done.left), (1, 0, 0), "{done:?}");
        assert!(deepest_pair(&after) <= FINAL_TOLERANCE && deepest_static(&after, &reg) <= WEDGE_TOLERANCE);
        assert!((after[0].pos.y - 20.0).abs() < 1e-4 && (after[1].pos.y - 20.4).abs() < 1e-4, "{:?} {:?}", after[0].pos, after[1].pos);
    }

    /// A box steering swung into a neighbour this tick is turned back
    /// rather than pushed.
    #[test]
    fn a_turn_into_a_neighbour_is_undone() {
        let (nav, reg) = surroundings(&[]);
        let mut turned = sized(1, Vec2::new(20.0, 20.0), RIDER, 0.0);
        let swung = Obb::new(turned.pos, RIDER, 0.8).frame();
        (turned.yaw, turned.right, turned.forward, turned.ext) = (0.8, swung.right, swung.forward, swung.extents());
        let beside = sized(2, Vec2::new(21.0, 20.6), PEASANT, 0.0);
        assert!(pair_depth(&turned, &beside) > FINAL_TOLERANCE && turned.start_frame(turned.pos).mtv(&beside.frame()).is_none());
        let (done, after) = part(vec![turned, beside], &nav, &reg);
        assert_eq!((done.unturned, done.pushed), (1, 0), "{done:?}");
        assert!(after[0].restore_yaw && after[0].yaw == 0.0 && after[0].pos == Vec2::new(20.0, 20.0) && after[1].pos == Vec2::new(21.0, 20.6));
    }

    /// A rider that walked deep into a standing one this tick (deeper than
    /// short shoves of both part) is sent back, which nobody sees, not
    /// shoved through or set down elsewhere; one only a little into it is
    /// parted by short shoves.
    #[test]
    fn a_walker_deep_in_another_is_sent_back_not_shoved() {
        let (nav, reg) = surroundings(&[]);
        let standing = sized(1, Vec2::new(20.0, 20.0), RIDER, 0.0);
        let walker = Body { start: Vec2::new(20.0, 17.5), ..sized(2, Vec2::new(20.0, 19.0), RIDER, 0.0) };
        assert!(pair_depth(&standing, &walker) > 2.0 * FRESH_PUSH + FINAL_TOLERANCE);
        let (done, after) = part(vec![standing, walker], &nav, &reg);
        assert_eq!((done.reverted, done.pushed, done.relocated, done.left), (1, 0, 0, 0), "{done:?}");
        assert_eq!(after[0].pos, Vec2::new(20.0, 20.0), "the standing one is not shoved");
        assert!(after[1].pos.y < 19.0 && after[1].pos.y >= 17.5 && deepest_pair(&after) <= FINAL_TOLERANCE, "{:?}", after[1].pos);

        let walker = Body { start: Vec2::new(20.0, 17.5), ..sized(2, Vec2::new(20.0, 18.5), RIDER, 0.0) };
        let (done, after) = part(vec![standing, walker], &nav, &reg);
        assert_eq!((done.pushed, done.reverted), (1, 0), "{done:?}");
        assert!(after.iter().all(|b| b.pos.distance(b.solved) <= FRESH_PUSH + 1e-4) && deepest_pair(&after) <= FINAL_TOLERANCE);
    }

    /// One peasant too many in a row filling a corridor, overlapping its
    /// neighbours as the tick begins: no push fits (the row along it, the
    /// walls across), easing gains a few centimetres, and no spot is free
    /// within [`SET_DOWN_RADIUS`]; it stays, no deeper, until it has been
    /// stranded for [`STRANDED_TICKS`], then is set down at the end of the
    /// row. One trained there (no spot of its own) goes there at once.
    #[test]
    fn a_unit_with_no_room_to_push_is_set_down_nearby() {
        let (nav, reg) = surroundings(&corridor(19.6, 20.4));
        let mut bodies: Vec<Body> = (0..10).map(|i| sized(i + 1, Vec2::new(17.0 + i as f32 * 0.62, 20.0), PEASANT, 0.0)).collect();
        bodies.push(sized(11, Vec2::new(19.79, 20.0), PEASANT, 0.0));
        let at_end = |after: &[Body]| after.iter().filter(|b| b.pos.x < 16.8 || b.pos.x > 22.9).count();

        let (log, after) = settle(bodies.clone(), &nav, &reg, STRANDED_TICKS as usize);
        let (stranded, waited) = log.split_last().expect("ticks");
        // Its neighbours ease off it as far as they can, a few cm.
        assert!(waited.iter().all(|(done, farthest)| done.left > 0 && done.relocated == 0 && *farthest < 0.1), "{waited:?}");
        assert_eq!((stranded.0.relocated, stranded.0.left), (1, 0), "{stranded:?}");
        assert!(deepest_pair(&after) <= FINAL_TOLERANCE && deepest_static(&after, &reg) <= WEDGE_TOLERANCE);
        assert_eq!(at_end(&after), 1, "one at an end of the row: {:?}", after.iter().map(|b| b.pos).collect::<Vec<_>>());

        bodies[10].placed_anew = true;
        let (done, after) = part(bodies, &nav, &reg);
        assert_eq!((done.relocated, done.left, at_end(&after)), (1, 0, 1), "{done:?}");
    }

    /// A ram trained half onto a bombard, riders standing close at either
    /// end and in a row above (round 4: such a pair stayed 0.3 m deep for
    /// 1478 ticks, the parting given one tick to find room for it and the
    /// hold-back keeping it as deep as it began). No push parts it at once
    /// and there is no room to set either down nearby, but there is room
    /// to give a little at a time: it is eased apart within a few ticks,
    /// and nobody jumps.
    #[test]
    fn an_overlap_left_from_the_last_tick_is_eased_apart() {
        let wall = Obb::new(Vec2::new(20.0, 18.0), Vec2::new(10.0, 0.5), 0.0);
        let (nav, reg) = surroundings(&[wall]);
        const BOMBARD: Vec2 = Vec2::new(0.8, 1.2);
        let across = std::f32::consts::FRAC_PI_2; // long axes along x
        let ram = sized(1, Vec2::new(20.0, 19.75), RAM, across); // x 18.4..21.6
        let bombard = sized(2, Vec2::new(22.5, 19.75), BOMBARD, across); // x 21.3..23.7
        let mut bodies = vec![ram, bombard, sized(3, Vec2::new(17.9, 19.75), RIDER, 0.0), sized(4, Vec2::new(24.35, 19.75), RIDER, 0.0)];
        for i in 0..8 {
            bodies.push(sized(5 + i, Vec2::new(16.9 + i as f32, 21.75), RIDER, 0.0));
        }
        let began = pair_depth(&bodies[0], &bodies[1]);
        assert!((began - 0.3).abs() < 1e-3 && deepest_pair(&bodies[2..]) == 0.0 && deepest_static(&bodies, &reg) <= WEDGE_TOLERANCE, "{began}");
        let (log, after) = settle(bodies, &nav, &reg, 10);
        let parted_at = log.iter().position(|(done, _)| done.left == 0).expect("parted within 10 ticks");
        assert!(log[0].0.eased > 0, "no room to part with one push: {:?}", log[0]);
        assert!(parted_at <= 3 && log.iter().all(|(done, _)| done.relocated == 0), "{log:?}");
        assert!(deepest_pair(&after) <= FINAL_TOLERANCE && deepest_static(&after, &reg) <= WEDGE_TOLERANCE, "{log:?}");
        assert!(log.iter().all(|(_, farthest)| *farthest <= began), "no jumps: {log:?}");
    }

    /// Where nothing fits (two peasants on one spot in a pen with room for
    /// one), nothing moves, tick after tick, stranded or not: the parting
    /// never makes things worse.
    #[test]
    fn where_nothing_fits_nothing_moves() {
        let walls = [
            Obb::new(Vec2::new(20.0, 19.2), Vec2::new(1.0, 0.5), 0.0),
            Obb::new(Vec2::new(20.0, 20.8), Vec2::new(1.0, 0.5), 0.0),
            Obb::new(Vec2::new(19.2, 20.0), Vec2::new(0.5, 1.0), 0.0),
            Obb::new(Vec2::new(20.8, 20.0), Vec2::new(0.5, 1.0), 0.0),
        ];
        let (nav, reg) = surroundings(&walls);
        let a = sized(1, Vec2::new(20.0, 20.0), PEASANT, 0.0);
        let b = sized(2, Vec2::new(20.0, 20.05), PEASANT, 0.0);
        let (done, after) = part(vec![a, b], &nav, &reg);
        assert_eq!(done, Parting { left: 1, ..Parting::default() });
        assert_eq!((after[0].pos, after[1].pos), (Vec2::new(20.0, 20.0), Vec2::new(20.0, 20.05)));
        let (log, after) = settle(vec![a, b], &nav, &reg, STRANDED_TICKS as usize + 5);
        assert!(log.iter().all(|(done, farthest)| done.left == 1 && *farthest == 0.0), "{log:?}");
        assert_eq!((after[0].pos, after[1].pos), (Vec2::new(20.0, 20.0), Vec2::new(20.0, 20.05)));
    }

    /// A peasant that walked from `from` to `to` this tick.
    fn walked(id: u32, from: Vec2, to: Vec2) -> Body {
        Body { start: from, ..sized(id, to, PEASANT, 0.0) }
    }

    /// Hold back `deep` among `bodies`: how many were held, and where all ended.
    fn hold(mut bodies: Vec<Body>, obstacles: &[Obb], deep: (u32, u32)) -> (u32, Vec<Body>) {
        let (nav, reg) = surroundings(obstacles);
        let mut contacts = Contacts::default();
        contacts.refresh(&bodies, nav.origin(), nav.world_size());
        let (mut moved, mut near, mut spots, mut ring) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut parter = Parter {
            bodies: &mut bodies,
            contacts: &mut contacts,
            moved: &mut moved,
            near: &mut near,
            spots: &mut spots,
            ring: &mut ring,
            world: Surroundings { nav: &nav, reg: &reg, cells: CellBoxes::new(&nav) },
        };
        let held = parter.hold_back(&mut vec![deep]);
        (held, bodies)
    }

    /// Holding back takes the unit that moved farther back toward where it
    /// started the tick only as far as parts the pair (so it still moves on
    /// a little), then whoever that lands it on, until no pair too deep is
    /// deeper than it began; all the way back, and as it faced, when less
    /// is not enough; and the other one when the farther one cannot go back
    /// (a new building covers its way).
    #[test]
    fn holding_back_leaves_no_pair_deeper_than_it_began() {
        let bodies = vec![
            walked(1, Vec2::new(20.0, 20.0), Vec2::new(20.5, 20.0)),
            walked(2, Vec2::new(21.6, 20.0), Vec2::new(21.0, 20.0)), // into 1, farther
            walked(3, Vec2::new(22.4, 20.0), Vec2::new(21.9, 20.0)), // toward where 2 started
        ];
        let (held, after) = hold(bodies, &[], (0, 1));
        assert_eq!(held, 1);
        let at: Vec<Vec2> = after.iter().map(|b| b.pos).collect();
        assert_eq!(at, [Vec2::new(20.5, 20.0), Vec2::new(21.15, 20.0), Vec2::new(21.9, 20.0)], "a quarter of the way back is enough");
        assert!(deepest_pair(&after) <= FINAL_TOLERANCE && !after[1].restore_yaw);

        // A rider that only turned into a peasant walking up beside it: no
        // part of the way back undoes a turn, so it goes all the way, and
        // turns back.
        let mut turned = sized(1, Vec2::new(20.0, 20.0), RIDER, 0.0);
        let swung = Obb::new(turned.pos, RIDER, 0.8).frame();
        (turned.yaw, turned.right, turned.forward, turned.ext) = (0.8, swung.right, swung.forward, swung.extents());
        let beside = Body { start: Vec2::new(21.3, 20.6), ..sized(2, Vec2::new(21.0, 20.6), PEASANT, 0.0) };
        assert!(pair_depth(&turned, &beside) > FINAL_TOLERANCE);
        let (held, after) = hold(vec![turned, beside], &[], (0, 1));
        assert_eq!(held, 1);
        assert!(after[0].pos == after[0].start && after[0].restore_yaw && after[0].yaw == 0.0 && after[1].pos == Vec2::new(21.0, 20.6));
        assert!(deepest_pair(&after) <= FINAL_TOLERANCE);

        // A house laid over where the farther one started and its way back:
        // the other goes back instead.
        let house = Obb::new(Vec2::new(22.52, 20.0), Vec2::splat(1.2), 0.0);
        let bodies = vec![walked(1, Vec2::new(20.0, 20.0), Vec2::new(20.5, 20.0)), walked(2, Vec2::new(21.6, 20.0), Vec2::new(21.0, 20.0))];
        let (held, after) = hold(bodies, &[house], (0, 1));
        assert_eq!(held, 1);
        assert_eq!((after[0].pos, after[1].pos), (Vec2::new(20.375, 20.0), Vec2::new(21.0, 20.0)));
    }

    /// A peasant pushed out of a house laid over where it stood shoves the
    /// next one into a third standing against a wall (round 5: horse archers
    /// by a new laboratory ended a tick 0.48 m deep). No short push parts
    /// the pair, and holding back cannot either: the shover has nowhere to
    /// go back to, and the one it shoved only goes back into it. The last
    /// resort slides the pair apart sideways, so it ends no deeper than it
    /// began.
    #[test]
    fn a_pair_shoved_together_with_no_way_back_is_still_parted() {
        let house = Obb::new(Vec2::new(12.0, 20.0), Vec2::splat(2.0), 0.0);
        let wall = Obb::new(Vec2::new(16.15, 20.0), Vec2::new(0.5, 2.0), 0.0);
        let (nav, reg) = surroundings(&[house, wall]);
        let shover = walked(1, Vec2::new(13.9, 20.0), Vec2::new(14.3, 20.0));
        let shoved = walked(2, Vec2::new(14.5, 20.0), Vec2::new(14.9, 20.0));
        let standing = sized(3, Vec2::new(15.35, 20.0), PEASANT, 0.0);
        assert!(static_depth(&shover, &shover.start_frame(shover.start), shover.start, &reg) > WEDGE_TOLERANCE, "the house covers its start");
        assert!(pair_depth(&shoved, &standing) > 0.1 && start_depth(&shoved, &standing) == 0.0);
        let (done, after) = part(vec![shover, shoved, standing], &nav, &reg);
        assert_eq!(done.left, 0, "{done:?}");
        assert!(deepest_pair(&after) <= FINAL_TOLERANCE && deepest_static(&after, &reg) <= WEDGE_TOLERANCE, "{done:?}");
        let moved = after.iter().zip([shover, shoved, standing]).map(|(b, was)| b.pos.distance(was.pos)).fold(0.0, f32::max);
        assert!(moved <= SET_DOWN_RADIUS, "{moved} m");
    }

    /// The candidate pairs are rebuilt when a box's bounds grow past what
    /// the list was built for, as when a turn is undone, not only when it
    /// moves: a long box turned back across a neighbour's way is paired
    /// with it.
    #[test]
    fn contacts_follow_a_box_whose_bounds_grow() {
        let (nav, _) = surroundings(&[]);
        let along = std::f32::consts::FRAC_PI_2;
        // A ram lying along x, a peasant well clear of its side...
        let mut ram = sized(1, Vec2::new(20.0, 20.0), RAM, along);
        let peasant = sized(2, Vec2::new(20.0, 21.6), PEASANT, 0.0);
        let mut contacts = Contacts::default();
        contacts.refresh(&[ram, peasant], nav.origin(), nav.world_size());
        assert!(contacts.pairs.is_empty());
        // ...then turned to stand along z, without moving: it reaches the peasant.
        ram.start_yaw = 0.0;
        ram.unturn();
        assert!(pair_depth(&ram, &peasant) > 0.0);
        contacts.refresh(&[ram, peasant], nav.origin(), nav.world_size());
        assert_eq!(contacts.pairs, [(0, 1)]);
    }

    /// The younger of two walkers meeting head-on gives way, with the walker
    /// filing in behind it in its lane, but not one passing in the next lane
    /// or one already past.
    #[test]
    fn the_younger_of_a_head_on_pair_and_its_file_give_way() {
        let mut bodies = vec![
            body(1, Vec2::new(0.0, 0.0), Vec2::Y, 0),   // walks north
            body(2, Vec2::new(0.0, 0.62), -Vec2::Y, 1), // walks south into it (an enemy)
            body(3, Vec2::new(0.05, 1.24), -Vec2::Y, 1), // files in behind 2
            body(4, Vec2::new(0.9, 1.3), -Vec2::Y, 1),  // in the next lane
            body(5, Vec2::new(0.0, -0.62), -Vec2::Y, 0), // behind 1, walking away
        ];
        // The candidate pairs: bodies whose bounds come within the contact margin.
        let near = |a: &Body, b: &Body| (a.pos - b.pos).abs().max_element() < 0.6 + CONTACT_MARGIN;
        let pairs: Vec<(u32, u32)> = (0..5u32).flat_map(|a| (a + 1..5).map(move |b| (a, b))).filter(|&(a, b)| near(&bodies[a as usize], &bodies[b as usize])).collect();
        mark_giving_way(&mut bodies, &pairs);
        let giving: Vec<bool> = bodies.iter().map(|b| b.gives_way).collect();
        assert_eq!(giving, [false, true, true, false, false]);
        assert!(yields_to(&bodies[1], &bodies[0]) && !yields_to(&bodies[0], &bodies[1]));
    }

    /// Age decides which of two walkers meeting head-on gives way, not the
    /// order the solver lists them in (by entity, which does not follow
    /// spawn order: a fresh slot's higher index sorts first, a recycled
    /// slot's generation outweighs any index): the younger gives way,
    /// listed first or second.
    #[test]
    fn the_younger_walker_gives_way_whatever_the_entity_order() {
        for younger_first in [false, true] {
            let mut bodies = vec![body(1, Vec2::ZERO, Vec2::Y, 0), body(2, Vec2::new(0.0, 0.62), -Vec2::Y, 1)];
            if younger_first {
                (bodies[0].born, bodies[1].born) = (2, 1);
            }
            mark_giving_way(&mut bodies, &[(0, 1)]);
            let young = usize::from(!younger_first);
            assert!(bodies[young].gives_way && !bodies[1 - young].gives_way, "younger listed first: {younger_first}");
        }
        // Listed as the solver lists them: two units spawned one after the
        // other, sorted by entity.
        let mut world = World::new();
        let (old, young) = (world.spawn_empty().id(), world.spawn_empty().id());
        let mut bodies = vec![
            Body { entity: old, born: 0, ..body(1, Vec2::ZERO, Vec2::Y, 0) },
            Body { entity: young, born: 1, ..body(2, Vec2::new(0.0, 0.62), -Vec2::Y, 1) },
        ];
        bodies.sort_unstable_by_key(|b| b.entity);
        mark_giving_way(&mut bodies, &[(0, 1)]);
        let giving: Vec<Entity> = bodies.iter().filter(|b| b.gives_way).map(|b| b.entity).collect();
        assert_eq!(giving, [young], "spawned first: {old}, then {young}");
    }

    /// Of two alike units the final word on pairs moves the younger first,
    /// listed first or second: of two idle peasants that turned into one
    /// another this tick, the younger has its turn undone.
    #[test]
    fn of_two_alike_the_younger_is_parted_first() {
        let (nav, reg) = surroundings(&[]);
        let turned = |id: u32, x: f32| Body { start_yaw: 0.0, ..sized(id, Vec2::new(x, 20.0), PEASANT, std::f32::consts::FRAC_PI_4) };
        for younger_first in [false, true] {
            let mut bodies = vec![turned(1, 20.0), turned(2, 20.72)];
            if younger_first {
                (bodies[0].born, bodies[1].born) = (2, 1);
            }
            assert!(pair_depth(&bodies[0], &bodies[1]) > FINAL_TOLERANCE && start_depth(&bodies[0], &bodies[1]) == 0.0);
            let (done, after) = part(bodies, &nav, &reg);
            assert_eq!(done, Parting { unturned: 1, ..Parting::default() });
            let young = usize::from(!younger_first);
            assert!(after[young].restore_yaw && !after[1 - young].restore_yaw, "younger listed first: {younger_first}");
        }
    }

    /// A foot walker of `body`'s size heading along `heading` (it cannot
    /// enter cliffs).
    fn foot(id: u32, pos: Vec2, heading: Vec2) -> Body {
        Body { mobility: Mobility::of(MoveFlag::Foot).0, ..body(id, pos, heading, 0) }
    }

    /// A 40 m grass grid with a cliff band over x = 20..26 but for the
    /// rows `gap` (a gap through it).
    fn cliff_band(gap: std::ops::Range<i32>) -> NavGrid {
        let data = af_core::testing::test_data();
        let mut nav = af_core::testing::new_nav(&data, 40, 40, data.terrain_id("grass").expect("grass"));
        let cliff = data.terrain_id("cliff").expect("cliff");
        for y in (0..40).filter(|y| !gap.contains(y)) {
            for x in 20..26 {
                nav.set_terrain(IVec2::new(x, y), cliff.0);
            }
        }
        nav
    }

    /// A walker whose nose touches the cliff beside the mouth of a gap
    /// one cell wide, a fifth of its box below the gap, slips sideways
    /// into the gap instead of being pushed straight back; one as close to
    /// the gap's corner but pressed against the face of the cliff (the
    /// step would lead along it, onto cliff) is pushed straight back.
    #[test]
    fn a_walker_slips_round_a_corner_of_the_terrain() {
        let nav = cliff_band(20..21);
        let cells = CellBoxes::new(&nav);
        // Box over x 19.45..20.05, y 19.8..20.4: 0.05 m into cell (20, 19)
        // across, 0.2 m along; it slips 0.2 m up, all of it in the gap.
        let mut mouth = foot(1, Vec2::new(19.75, 20.1), Vec2::X);
        push_out_terrain(&mut mouth, &nav, &cells);
        assert!((mouth.pos.x - 19.75).abs() < 1e-4 && (mouth.pos.y - 20.3).abs() < 0.01, "into the gap: {}", mouth.pos);
        // Box over y 18.85..19.45: 0.15 m over cell (20, 18), the rest
        // against cell (20, 19), cliff too.
        let mut face = foot(2, Vec2::new(19.75, 19.15), Vec2::X);
        push_out_terrain(&mut face, &nav, &cells);
        assert!((face.pos.y - 19.15).abs() < 1e-4 && face.pos.x < 19.71, "pushed back: {}", face.pos);
    }

    /// Of two walkers meeting head-on, one that gave way lately gives way
    /// again although it is the elder, and the walker filing in behind it
    /// with it; when both or neither did, the younger gives way.
    #[test]
    fn a_walker_that_gave_way_lately_gives_way_again() {
        let meet = |gave: [bool; 2]| {
            let mut bodies = vec![
                Body { gave_way: gave[0], ..body(1, Vec2::new(0.0, 0.0), Vec2::Y, 0) },
                Body { gave_way: gave[1], ..body(2, Vec2::new(0.0, 0.62), -Vec2::Y, 1) },
                body(3, Vec2::new(0.05, -0.62), Vec2::Y, 0), // files in behind 1
            ];
            mark_giving_way(&mut bodies, &[(0, 1), (0, 2)]);
            bodies.iter().map(|b| b.gives_way).collect::<Vec<_>>()
        };
        assert_eq!(meet([true, false]), [true, false, true], "the elder gave way lately");
        assert_eq!(meet([false, false]), [false, true, false]);
        assert_eq!(meet([true, true]), [false, true, false]);
    }

    /// A unit giving way yields to the unit ahead of it in its file when
    /// that one gives way too (backed into it, it backs off), not to the
    /// one behind it; and one that does not give way holds.
    #[test]
    fn a_file_giving_way_backs_off_behind_its_front() {
        let mut front = body(1, Vec2::new(0.0, 0.6), Vec2::Y, 0);
        let mut behind = body(2, Vec2::new(0.05, 0.0), Vec2::Y, 0);
        (front.gives_way, behind.gives_way) = (true, true);
        let back = Vec2::new(0.0, -0.1);
        assert!((pushed_resist(&behind, &front, back) - behind.resist * GIVE_WAY_RESIST).abs() < 1e-6);
        assert_eq!(pushed_resist(&front, &behind, -back), front.resist);
        behind.gives_way = false;
        assert_eq!(pushed_resist(&behind, &front, back), behind.resist);
    }

    /// A braced unit giving way in the walker's lane is backed out along
    /// the walker's heading; one grazing it from the next lane is not (the
    /// usual push-out parts the two across, into their lanes).
    #[test]
    fn only_a_yielder_in_the_walkers_lane_is_backed_out() {
        let walker = body(1, Vec2::ZERO, Vec2::X, 0);
        let yielder = |at: Vec2| {
            let mut y = body(2, at, -Vec2::X, 1);
            y.gives_way = true;
            y.brace(Vec2::NEG_Y); // a wall on its far side
            y
        };
        let in_lane = yielder(Vec2::new(0.55, 0.1));
        let push = back_out(&in_lane, &walker, &in_lane.frame(), &walker.frame()).expect("backed out");
        assert!(push.x > 0.0 && push.y == 0.0 && push.length() <= BACK_OUT + 1e-6, "{push}");
        let next_lane = yielder(Vec2::new(0.5, 0.5));
        assert!(pair_depth(&next_lane, &walker) > 0.0);
        assert_eq!(back_out(&next_lane, &walker, &next_lane.frame(), &walker.frame()), None);
    }

    /// A walker pressed against one ahead of it going its way waits its
    /// turn, the one ahead does not; nor does one pressed against a unit
    /// standing, coming the other way, or stuck (its steps not going
    /// through, and not waiting itself); and of two converging at an angle,
    /// neither waits on the other.
    #[test]
    fn a_walker_pressed_against_one_moving_on_ahead_waits() {
        let moving_on = |id: u32, at: Vec2, heading: Vec2| Body { moves_on: true, ..body(id, at, heading, 0) };
        let mut bodies = vec![moving_on(1, Vec2::ZERO, Vec2::X), moving_on(2, Vec2::new(0.55, 0.1), Vec2::X)];
        resolve_pair(&mut bodies, 0, 1, 1.0, false);
        assert!(bodies[0].waits && !bodies[1].waits);
        let behind = moving_on(1, Vec2::ZERO, Vec2::X);
        assert!(!queues_behind(&behind, &moving_on(2, Vec2::new(0.55, 0.0), Vec2::ZERO)), "standing");
        assert!(!queues_behind(&behind, &moving_on(2, Vec2::new(0.55, 0.0), -Vec2::X)), "oncoming");
        assert!(!queues_behind(&behind, &body(2, Vec2::new(0.55, 0.0), Vec2::X, 0)), "stuck");
        // Converging at 60 degrees, each ahead of the other along its own heading.
        let (up, down) = (Vec2::new(0.866, 0.5), Vec2::new(0.866, -0.5));
        let (a, b) = (moving_on(1, Vec2::ZERO, up), moving_on(2, Vec2::new(0.0, 0.6), down));
        assert!(!queues_behind(&a, &b) && !queues_behind(&b, &a));
    }

    /// A spot with a cliff or buildings close on both sides is in a
    /// passage; one beside a single face, or in the open, is not.
    #[test]
    fn a_passage_has_walls_close_on_both_sides() {
        let mobility = Mobility::of(MoveFlag::Foot).0;
        let nav = cliff_band(20..22);
        assert!(in_passage(&nav, Vec2::new(23.0, 21.0), mobility), "in the gap");
        assert!(!in_passage(&nav, Vec2::new(17.5, 21.0), mobility), "before its mouth");
        assert!(!in_passage(&nav, Vec2::new(19.5, 10.5), mobility), "beside the cliff");
        let (nav, _) = surroundings(&corridor(19.6, 20.4));
        assert!(in_passage(&nav, Vec2::new(20.0, 20.0), mobility), "between two walls");
        assert!(!in_passage(&nav, Vec2::new(20.0, 25.0), mobility));
    }
}
