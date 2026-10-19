//! Development tooling for the windowed game: the dev flags listed under
//! `--help` (screenshot series, camera framing and close-ups, a selection
//! made after a delay that the camera can follow and shots can be timed
//! from, a building held in placement mode, a held hover and a one-off
//! press of a UI button, a stand-in mouse, a staged battle, an AI-played
//! local slot, a pause, frame-time statistics, boxes instead of models,
//! uncapped frames, and a [`script`] of keys, clicks and presses).
//! Each piece is registered only when its flag is given, so a normal run
//! adds nothing. Binary-only: the library (`aetherfall::*`) knows nothing
//! of it.

mod script;

use std::path::PathBuf;

use af_core::ext::input::CursorOverride;
use af_core::ext::map::MapSetupSet;
use af_core::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::window::{PresentMode, PrimaryWindow};

/// Seconds a `--select` waits for by default (the start entities exist).
const DEFAULT_SELECT_AT: f32 = 1.0;
/// Units per side a `--battle-demo` stages by default, and at most.
const DEFAULT_BATTLE_COUNT: u32 = 8;
const MAX_BATTLE_COUNT: u32 = 40;
/// `--camera` may frame closer than the mouse wheel can zoom, for close-ups
/// of single units (metres from the focus).
const CLOSE_UP_DISTANCE: f32 = 5.0;
/// A placed wall line starts this far left of the cursor (metres).
const WALL_PREVIEW_LENGTH: f32 = 14.0;

/// The dev flags as given on the command line (content ids unresolved).
#[derive(Debug, Default)]
pub struct DevArgs {
    screenshot: Option<PathBuf>,
    shots: Option<String>,
    shot_at: Option<f32>,
    /// `--shot-at +S`: S seconds after the `--select` pick is made.
    shot_after_select: bool,
    shot_every: Option<f32>,
    shot_count: Option<u32>,
    /// Start on the menu even when taking screenshots.
    pub menu: bool,
    camera: Option<CameraArg>,
    select: Option<String>,
    select_at: Option<f32>,
    follow: bool,
    place: Option<String>,
    hover: Option<String>,
    press: Option<String>,
    press_at: Option<f32>,
    cursor: Option<Vec2>,
    battle: Option<String>,
    autoplay: bool,
    pause_at: Option<f32>,
    script: Option<String>,
    frame_stats: Option<f32>,
    no_models: bool,
    no_vsync: bool,
}

impl DevArgs {
    /// Consume `arg` (and its value from `it`) if it is a dev flag.
    pub fn take(&mut self, arg: &str, it: &mut impl Iterator<Item = String>) -> Result<bool, String> {
        let mut value = || it.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg {
            "--screenshot" => self.screenshot = Some(PathBuf::from(value()?)),
            "--screenshot-at" | "--shot-at" => {
                let v = value()?;
                let after = v.strip_prefix('+');
                self.shot_after_select = after.is_some();
                self.shot_at = Some(number(arg, after.unwrap_or(&v))?);
            }
            "--shots" => self.shots = Some(value()?),
            "--shot-every" => self.shot_every = Some(number(arg, &value()?)?),
            "--shot-count" => self.shot_count = Some(number(arg, &value()?)?),
            "--menu" => self.menu = true,
            "--camera" => self.camera = Some(CameraArg::parse(&value()?)?),
            "--select" => self.select = Some(value()?),
            "--select-at" => self.select_at = Some(number(arg, &value()?)?),
            "--follow" => self.follow = true,
            "--place" => self.place = Some(value()?),
            "--hover" => self.hover = Some(value()?),
            "--press" => self.press = Some(value()?),
            "--press-at" => self.press_at = Some(number(arg, &value()?)?),
            "--cursor" => self.cursor = Some(pair(arg, &value()?)?),
            "--battle-demo" => self.battle = Some(value()?),
            "--autoplay" => self.autoplay = true,
            "--pause-at" => self.pause_at = Some(number(arg, &value()?)?),
            "--frame-stats" => self.frame_stats = Some(number(arg, &value()?)?),
            "--no-models" => self.no_models = true,
            "--no-vsync" => self.no_vsync = true,
            "--script" => self.script = Some(value()?),
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Whether any dev flag was given (they all need a window).
    pub fn any(&self) -> bool {
        self.screenshot.is_some()
            || self.shots.is_some()
            || self.menu
            || self.camera.is_some()
            || self.select.is_some()
            || self.follow
            || self.place.is_some()
            || self.hover.is_some()
            || self.press.is_some()
            || self.cursor.is_some()
            || self.battle.is_some()
            || self.autoplay
            || self.pause_at.is_some()
            || self.frame_stats.is_some()
            || self.no_models
            || self.no_vsync
            || self.script.is_some()
    }

    /// Whether the run takes screenshots or plays a script (and should
    /// start a match at once).
    pub fn takes_shots(&self) -> bool {
        self.screenshot.is_some() || self.shots.is_some() || self.script.is_some()
    }

    /// Check the flags against the content and the match, and adjust the
    /// match for them (an AI-played local slot, a revealed map for a
    /// battle demo).
    pub fn plan(self, data: &GameData, setup: &mut MatchSetup) -> Result<DevPlan, String> {
        let script = self.script.as_deref().map(|s| script::parse(data, s)).transpose()?;
        if script.is_some() && self.menu {
            return Err("--script plays a match (drop --menu)".into());
        }
        let mut shots = self.shot_plan()?;
        let script_shots = script.as_deref().map(script::shots).unwrap_or_default();
        if !script_shots.is_empty() {
            if shots.as_ref().is_some_and(|s| s.clock == ShotClock::AfterSelect) {
                return Err("--shot-at +S times shots from the --select pick; a --script times its own (drop the +)".into());
            }
            let plan = shots.get_or_insert_with(|| ShotPlan { shots: Vec::new(), next: 0, clock: ShotClock::Sim });
            plan.shots.extend(script_shots);
            plan.shots.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        let select = self.select.as_deref().map(|s| Pick::parse(data, s)).transpose()?;
        if self.follow && select.is_none() {
            return Err("--follow follows the --select pick (add --select)".into());
        }
        if self.shot_after_select && select.is_none() {
            return Err("--shot-at +S counts from the --select pick (add --select)".into());
        }
        let place = self.place.as_deref().map(|id| lookup(id, data.building_id(id), "building", data.buildings.iter().map(|b| &b.id))).transpose()?;
        if place.is_some() && setup.local_player.is_none() {
            return Err("--place needs a local player (drop --spectate)".into());
        }
        let battle = self.battle.as_deref().map(|s| BattlePlan::parse(data, s)).transpose()?;
        if battle.is_some() {
            if setup.players.len() < 2 {
                return Err("--battle-demo needs at least two players".into());
            }
            setup.reveal_map = true;
        }
        if self.autoplay {
            let Some(slot) = setup.local_player.and_then(|i| setup.players.get_mut(i)) else {
                return Err("--autoplay needs a local player (drop --spectate)".into());
            };
            slot.controller = Controller::Ai;
        }
        let virtual_cursor = (shots.is_some() || place.is_some() || self.cursor.is_some() || script.is_some()).then_some(self.cursor);
        Ok(DevPlan {
            shots,
            camera: self.camera,
            select: select.map(|pick| SelectPlan { pick, at: self.select_at.unwrap_or(DEFAULT_SELECT_AT) }),
            follow: self.follow,
            place,
            hover: self.hover,
            press: self.press.map(|label| PressPlan { label, at: self.press_at.unwrap_or(self.select_at.unwrap_or(DEFAULT_SELECT_AT) + 0.5) }),
            virtual_cursor,
            battle,
            pause_at: self.pause_at,
            frame_stats: match self.frame_stats {
                Some(every) if !(every.is_finite() && every > 0.0) => return Err("--frame-stats wants a positive number of seconds".into()),
                every => every,
            },
            no_models: self.no_models,
            no_vsync: self.no_vsync,
            script: script.map(script::Script::new),
        })
    }

    fn shot_plan(&self) -> Result<Option<ShotPlan>, String> {
        let at = self.shot_at.unwrap_or(10.0);
        let clock = match (self.menu, self.shot_after_select) {
            (true, true) => return Err("--shot-at +S counts from a --select pick in a match (drop --menu)".into()),
            (false, true) => ShotClock::AfterSelect,
            (true, false) => ShotClock::Real,
            (false, false) => ShotClock::Sim,
        };
        let shots = match (&self.screenshot, &self.shots) {
            (Some(_), Some(_)) => return Err("use either --screenshot or --shots, not both".into()),
            (Some(path), None) => vec![(at, path.clone())],
            (None, Some(prefix)) => {
                let every = self.shot_every.unwrap_or(60.0);
                if !every.is_finite() || every <= 0.0 {
                    return Err("--shot-every must be positive".into());
                }
                shot_series(prefix, at, every, self.shot_count.unwrap_or(1))
            }
            (None, None) => return Ok(None),
        };
        Ok(Some(ShotPlan { shots, next: 0, clock }))
    }
}

/// What [`install`] registers, checked against the content.
pub struct DevPlan {
    shots: Option<ShotPlan>,
    camera: Option<CameraArg>,
    select: Option<SelectPlan>,
    /// Keep the camera on the selection.
    follow: bool,
    place: Option<BuildingTypeId>,
    hover: Option<String>,
    press: Option<PressPlan>,
    /// `Some`: a stand-in mouse replaces the real one, at this position
    /// (`None` = the window centre).
    virtual_cursor: Option<Option<Vec2>>,
    battle: Option<BattlePlan>,
    /// Sim seconds at which to pause the game.
    pause_at: Option<f32>,
    /// Real seconds between frame-time reports.
    frame_stats: Option<f32>,
    /// Draw every unit as boxes (no models).
    no_models: bool,
    /// Present frames as fast as they are made.
    no_vsync: bool,
    script: Option<script::Script>,
}

impl DevPlan {
    /// How the window presents frames: synced to the display unless
    /// `--no-vsync` (to measure frame times above the refresh rate).
    pub fn present_mode(&self) -> PresentMode {
        if self.no_vsync { PresentMode::AutoNoVsync } else { PresentMode::AutoVsync }
    }
}

/// Register the systems the plan needs (and nothing else).
pub fn install(app: &mut App, plan: DevPlan) {
    if let Some(script) = plan.script {
        app.insert_resource(script)
            .add_systems(PreUpdate, script::script_input.after(bevy::input::InputSystems).before(place_virtual_cursor))
            .add_systems(
                Update,
                script::script_world
                    .after(ClientSet::Input)
                    .before(ClientSet::Ui)
                    .run_if(resource_exists::<MapInfo>.and_then(resource_exists::<Players>)),
            );
    }
    if let Some(shots) = plan.shots {
        app.insert_resource(shots).add_systems(Update, take_shots);
    }
    if let Some(at) = plan.virtual_cursor {
        app.insert_resource(VirtualCursor(at)).init_resource::<CursorOverride>().add_systems(PreUpdate, place_virtual_cursor);
    }
    if let Some(camera) = plan.camera {
        app.insert_resource(camera)
            .add_systems(Update, frame_camera.after(ClientSet::Input).before(ClientSet::Ui).run_if(resource_exists::<MapInfo>.and_then(resource_exists::<Players>)));
    }
    if let Some(select) = plan.select {
        app.insert_resource(select).add_systems(Update, apply_selection.after(ClientSet::Input).before(ClientSet::Ui).run_if(resource_exists::<Players>));
    }
    if plan.follow {
        // Before af_input places the camera from the rig this frame.
        app.add_systems(Update, follow_selection.before(ClientSet::Input).run_if(resource_exists::<PickedAt>));
    }
    if let Some(kind) = plan.place {
        app.insert_resource(PlacePlan(kind)).add_systems(Update, hold_placement.after(ClientSet::Input).before(ClientSet::Ui).run_if(resource_exists::<Players>));
    }
    if let Some(label) = plan.hover {
        app.insert_resource(HoverPlan(label)).add_systems(Update, hold_hover.in_set(ClientSet::Input));
    }
    if let Some(press) = plan.press {
        // After the held hover, or it would turn the press back into a hover.
        app.insert_resource(press).add_systems(Update, press_button.in_set(ClientSet::Input).after(hold_hover));
    }
    if let Some(battle) = plan.battle {
        app.insert_resource(battle).add_systems(OnEnter(GameState::Playing), stage_battle.after(MapSetupSet));
    }
    if let Some(at) = plan.pause_at {
        app.insert_resource(PausePlan(at)).add_systems(Update, pause_game.before(ClientSet::Ui));
    }
    if let Some(every) = plan.frame_stats {
        app.insert_resource(FrameStats { every, ..default() }).add_systems(Last, report_frame_stats);
    }
    if plan.no_models {
        // An empty library: af_render loads none and draws boxes.
        app.insert_resource(af_render::model::ModelLibrary::default());
    }
}

// ---------------------------------------------------------------------------
// Argument helpers
// ---------------------------------------------------------------------------

fn number<T: std::str::FromStr>(flag: &str, s: &str) -> Result<T, String>
where
    T::Err: std::fmt::Display,
{
    s.trim().parse().map_err(|e| format!("{flag} `{s}`: {e}"))
}

fn pair(flag: &str, s: &str) -> Result<Vec2, String> {
    match s.split(',').map(|v| number::<f32>(flag, v)).collect::<Result<Vec<_>, _>>()?.as_slice() {
        [x, y] => Ok(Vec2::new(*x, *y)),
        _ => Err(format!("{flag} wants two numbers `x,y`, got `{s}`")),
    }
}

/// A content id, or an error listing the valid ones.
fn lookup<'a, T>(id: &str, found: Option<T>, what: &str, all: impl Iterator<Item = &'a String>) -> Result<T, String> {
    found.ok_or_else(|| format!("unknown {what} `{id}` (have: {})", all.map(String::as_str).collect::<Vec<_>>().join(", ")))
}

/// The screenshots of `--shots prefix`: `count` of them from `at` sim
/// seconds, `every` seconds apart, named `<prefix>_<index>_<seconds>s.png`.
fn shot_series(prefix: &str, at: f32, every: f32, count: u32) -> Vec<(f32, PathBuf)> {
    (0..count.max(1))
        .map(|i| {
            let t = at + every * i as f32;
            let secs = if t.fract() == 0.0 { format!("{t:.0}") } else { format!("{t:.1}") };
            (t, PathBuf::from(format!("{prefix}_{i:02}_{secs}s.png")))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Screenshots and the stand-in mouse
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShotClock {
    /// Sim seconds (`SimClock`): the match's own time, whatever its speed.
    Sim,
    /// Real seconds since start-up (the menu, where the sim does not run).
    Real,
    /// Sim seconds since the `--select` pick was made (none before that).
    AfterSelect,
}

/// When (sim seconds) the `--select` pick was made.
#[derive(Resource, Clone, Copy, Debug)]
struct PickedAt(f32);

/// Screenshots still to take, in time order; the app exits after the last.
#[derive(Resource, Debug)]
struct ShotPlan {
    shots: Vec<(f32, PathBuf)>,
    next: usize,
    clock: ShotClock,
}

/// Take the screenshots that are due. While the game is paused its sim
/// clock stands still, so the shots still to come are taken one a frame.
fn take_shots(
    mut commands: Commands,
    mut plan: ResMut<ShotPlan>,
    sim: Res<SimClock>,
    real: Res<Time<Real>>,
    virt: Res<Time<Virtual>>,
    picked: Option<Res<PickedAt>>,
) {
    let (now, frozen) = match plan.clock {
        ShotClock::Sim => (sim.elapsed, virt.is_paused()),
        ShotClock::Real => (real.elapsed_secs(), false),
        ShotClock::AfterSelect => match picked {
            Some(at) => (sim.elapsed - at.0, virt.is_paused()),
            None => return,
        },
    };
    let mut taken = 0;
    while let Some((at, path)) = plan.shots.get(plan.next).cloned() {
        if now < at && !(frozen && taken == 0) {
            break;
        }
        taken += 1;
        plan.next += 1;
        let mut shot = commands.spawn(Screenshot::primary_window());
        shot.observe(save_to_disk(path));
        if plan.next == plan.shots.len() {
            shot.observe(|_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                exit.write(AppExit::Success);
            });
        }
    }
}

/// `--frame-stats`: frame times (milliseconds) since the last report.
#[derive(Resource, Default)]
struct FrameStats {
    every: f32,
    since: f32,
    frames: Vec<f32>,
}

impl FrameStats {
    /// `count frames: mean, p50, p95, p99, max` of `frames` (ms), sorting them.
    fn summary(frames: &mut [f32]) -> String {
        if frames.is_empty() {
            return "no frames".into();
        }
        frames.sort_by(f32::total_cmp);
        let at = |q: f32| frames[((frames.len() - 1) as f32 * q).round() as usize];
        let mean = frames.iter().sum::<f32>() / frames.len() as f32;
        format!(
            "{} frames: mean {mean:.2} ms ({:.0} fps), p50 {:.2}, p95 {:.2}, p99 {:.2}, max {:.2}",
            frames.len(),
            1000.0 / mean,
            at(0.5),
            at(0.95),
            at(0.99),
            at(1.0)
        )
    }
}

/// Log frame-time statistics every `--frame-stats` seconds of real time,
/// with the match clock and how many units there are (and drawn as models).
fn report_frame_stats(
    time: Res<Time<Real>>,
    clock: Option<Res<SimClock>>,
    mut stats: ResMut<FrameStats>,
    units: Query<(), (With<Unit>, Without<Dead>)>,
    models: Query<(), With<af_render::model::ModelUnit>>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    stats.frames.push(dt * 1000.0);
    stats.since += dt;
    if stats.since < stats.every {
        return;
    }
    let summary = FrameStats::summary(&mut stats.frames);
    let sim = clock.map_or(0.0, |c| c.elapsed);
    info!("frame stats at {sim:.0} s sim: {summary}; {} units, {} drawn as models", units.iter().count(), models.iter().count());
    stats.frames.clear();
    stats.since = 0.0;
}

/// `--pause-at`: sim seconds at which the game pauses (once).
#[derive(Resource)]
struct PausePlan(f32);

fn pause_game(plan: Res<PausePlan>, sim: Res<SimClock>, mut time: ResMut<Time<Virtual>>, mut done: Local<bool>) {
    if !*done && sim.elapsed >= plan.0 {
        *done = true;
        time.pause();
    }
}

/// Where the stand-in mouse sits (`None` = the window centre).
#[derive(Resource)]
struct VirtualCursor(Option<Vec2>);

/// Keep the real mouse out of a scripted run: af_input and af_ui read the
/// cursor from `CursorOverride` (no edge scrolling, no stray hover).
fn place_virtual_cursor(at: Res<VirtualCursor>, windows: Query<&Window, With<PrimaryWindow>>, mut stand_in: ResMut<CursorOverride>) {
    let Ok(window) = windows.single() else { return };
    let p = at.0.unwrap_or(window.size() * 0.5);
    if stand_in.0 != Some(p) {
        stand_in.0 = Some(p);
    }
}

// ---------------------------------------------------------------------------
// Camera
// ---------------------------------------------------------------------------

/// A named or explicit spot for `--camera`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Spot {
    Point(Vec2),
    Crater,
    Centre,
    /// A player's start; `None` = the viewer's (else player 0's).
    Start(Option<u8>),
}

/// `--camera <spot>[,distance[,turn]]`.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
struct CameraArg {
    spot: Spot,
    distance: Option<f32>,
    /// Degrees to turn the view from the game's fixed angle (clockwise
    /// seen from above), to look at the spot from another side.
    turn: Option<f32>,
}

impl CameraArg {
    fn parse(s: &str) -> Result<CameraArg, String> {
        const FLAG: &str = "--camera";
        let parts: Vec<&str> = s.split(',').map(str::trim).collect();
        let named = match parts[0] {
            "crater" => Some(Spot::Crater),
            "centre" | "center" => Some(Spot::Centre),
            "start" => Some(Spot::Start(None)),
            p if p.starts_with('p') && p.len() > 1 => Some(Spot::Start(Some(number(FLAG, &p[1..])?))),
            _ => None,
        };
        let (spot, rest) = match named {
            Some(spot) => (spot, &parts[1..]),
            None if parts.len() >= 2 => (Spot::Point(Vec2::new(number(FLAG, parts[0])?, number(FLAG, parts[1])?)), &parts[2..]),
            None => return Err(format!("{FLAG} wants `x,z[,distance[,turn]]` or crater|centre|start|p<N>[,distance[,turn]], got `{s}`")),
        };
        let (distance, turn) = match rest {
            [] => (None, None),
            [d] => (Some(number(FLAG, d)?), None),
            [d, t] => (Some(number(FLAG, d)?), Some(number(FLAG, t)?)),
            _ => return Err(format!("{FLAG}: too many values in `{s}`")),
        };
        Ok(CameraArg { spot, distance, turn })
    }

    /// The ground point the camera should look at.
    fn focus(&self, map: &MapInfo, players: &Players, viewer: Option<PlayerId>) -> Vec2 {
        match self.spot {
            Spot::Point(p) => p,
            Spot::Crater => map.crater_center,
            Spot::Centre => map.size * 0.5,
            Spot::Start(slot) => {
                let id = slot.map(PlayerId).or(viewer).unwrap_or(PlayerId(0));
                players.try_get(id).map_or(map.size * 0.5, |p| p.start_pos)
            }
        }
    }

    /// Point the camera rig at the spot (and zoom to the distance and turn
    /// the view, if given).
    fn apply(&self, map: &MapInfo, players: &Players, viewer: Option<PlayerId>, rig: &mut CameraRig) {
        rig.focus = self.focus(map, players, viewer).clamp(Vec2::ZERO, map.size);
        if let Some(d) = self.distance {
            rig.distance = d.clamp(CLOSE_UP_DISTANCE, af_input::MAX_DISTANCE);
        }
        if let Some(t) = self.turn {
            rig.yaw = af_geom::wrap_angle(af_input::VIEW_YAW - t.to_radians());
        }
    }
}

/// Once, after af_input has centred the camera on the match start.
fn frame_camera(arg: Res<CameraArg>, map: Res<MapInfo>, players: Res<Players>, local: Res<LocalPlayer>, mut rig: ResMut<CameraRig>, mut done: Local<bool>) {
    if *done {
        return;
    }
    *done = true;
    arg.apply(&map, &players, local.0, &mut rig);
}

// ---------------------------------------------------------------------------
// Selection
// ---------------------------------------------------------------------------

/// What `--select` picks among the viewer's (else player 0's) entities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pick {
    /// The main hall (a complete TownCenter-tagged building first).
    Hall,
    /// One worker, preferably one carrying a load.
    Worker,
    Workers,
    /// Every unit that is not a worker.
    Army,
    /// The foundation furthest along.
    Foundation,
    /// The building with the longest production queue.
    Busy,
    /// The first building of a type (complete ones first).
    Building(BuildingTypeId),
    /// Every unit of a type.
    Units(UnitTypeId),
    /// One unit busy so (`Activity`).
    Doing(Doing),
    /// One worker carrying a load (of this resource) home: holding it, not
    /// busy at work (filling it, or building with it in its pack).
    Carrying(Option<ResourceId>),
}

/// What a `doing:<what>` pick looks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Doing {
    /// Exactly this activity.
    Is(Activity),
    /// Gathering in any style.
    Gathering,
}

impl Doing {
    const NAMES: &str = "idle|gather|forage|farm|chop|mine|dig|chip|smash|build|repair|heal|fight";

    fn parse(s: &str) -> Option<Doing> {
        let style = WorkStyle::ALL.into_iter().find(|w| format!("{w:?}").eq_ignore_ascii_case(s));
        Some(match s {
            _ if style.is_some() => Doing::Is(Activity::Gather(style?)),
            "gather" => Doing::Gathering,
            "idle" => Doing::Is(Activity::Idle),
            "build" => Doing::Is(Activity::Build),
            "repair" => Doing::Is(Activity::Repair),
            "heal" => Doing::Is(Activity::Heal),
            "fight" => Doing::Is(Activity::Fight),
            _ => return None,
        })
    }

    fn matches(self, activity: Activity) -> bool {
        match self {
            Doing::Is(a) => a == activity,
            Doing::Gathering => matches!(activity, Activity::Gather(_)),
        }
    }
}

impl Pick {
    fn parse(data: &GameData, s: &str) -> Result<Pick, String> {
        Ok(match s.split_once(':') {
            Some(("building", id)) => Pick::Building(lookup(id, data.building_id(id), "building", data.buildings.iter().map(|b| &b.id))?),
            Some(("unit", id)) => Pick::Units(lookup(id, data.unit_id(id), "unit", data.units.iter().map(|u| &u.id))?),
            Some(("doing", what)) => Pick::Doing(Doing::parse(what).ok_or_else(|| format!("--select doing:`{what}`: want doing:{}", Doing::NAMES))?),
            Some(("carrying", "any")) => Pick::Carrying(None),
            Some(("carrying", id)) => Pick::Carrying(Some(lookup(id, data.resource_id(id), "resource", data.resources.iter().map(|r| &r.id))?)),
            None => match s {
                "hall" => Pick::Hall,
                "worker" => Pick::Worker,
                "workers" => Pick::Workers,
                "army" => Pick::Army,
                "foundation" => Pick::Foundation,
                "busy" => Pick::Busy,
                _ => {
                    return Err(format!(
                        "--select `{s}`: want hall|worker|workers|army|foundation|busy|building:<id>|unit:<id>|doing:<what>|carrying:<resource|any>"
                    ));
                }
            },
            _ => return Err(format!("--select `{s}`: want building:<id>, unit:<id>, doing:<what> or carrying:<resource|any>")),
        })
    }
}

#[derive(Resource, Clone, Copy, Debug)]
struct SelectPlan {
    pick: Pick,
    /// Sim seconds.
    at: f32,
}

/// One candidate for a pick, with the facts the picks sort by.
#[derive(Clone, Copy, Debug)]
struct Candidate {
    entity: Entity,
    pos: Vec2,
    unit: Option<UnitTypeId>,
    building: Option<BuildingTypeId>,
    worker: bool,
    /// The load a worker holds.
    load: Option<ResourceId>,
    activity: Activity,
    hall: bool,
    /// Construction progress while a foundation.
    foundation: Option<f32>,
    queued: usize,
}

/// The entities `pick` selects among one player's `candidates`.
fn pick_entities(pick: Pick, candidates: &[Candidate]) -> Vec<Entity> {
    let mut sorted = candidates.to_vec();
    sorted.sort_by_key(|c| c.entity);
    let mut units = sorted.iter().filter(|c| c.unit.is_some());
    let buildings = || sorted.iter().filter(|c| c.building.is_some());
    let first = |best: Option<&Candidate>| best.map(|c| c.entity).into_iter().collect();
    match pick {
        Pick::Hall => first(buildings().filter(|c| c.hall).min_by_key(|c| c.foundation.is_some())),
        Pick::Worker => first(units.filter(|c| c.worker).min_by_key(|c| c.load.is_none())),
        Pick::Workers => units.filter(|c| c.worker).map(|c| c.entity).collect(),
        Pick::Army => units.filter(|c| !c.worker).map(|c| c.entity).collect(),
        Pick::Foundation => first(buildings().filter_map(|c| Some((c, c.foundation?))).max_by(|a, b| a.1.total_cmp(&b.1)).map(|(c, _)| c)),
        Pick::Busy => first(buildings().filter(|c| c.queued > 0).max_by_key(|c| c.queued)),
        Pick::Building(kind) => first(buildings().filter(|c| c.building == Some(kind)).min_by_key(|c| c.foundation.is_some())),
        Pick::Units(kind) => units.filter(|c| c.unit == Some(kind)).map(|c| c.entity).collect(),
        Pick::Doing(doing) => first(units.filter(|c| doing.matches(c.activity)).min_by_key(|c| !c.worker)),
        Pick::Carrying(load) => {
            let home = |c: &&Candidate| c.load.is_some() && (load.is_none() || c.load == load) && c.activity == Activity::Idle;
            first(units.find(home))
        }
    }
}

/// What picks choose from: one player's living units and buildings, and
/// the current selection.
#[derive(SystemParam)]
#[allow(clippy::type_complexity)]
struct PickWorld<'w, 's> {
    units: Query<'w, 's, (Entity, &'static Unit, &'static Owner, &'static Position, Option<&'static Gatherer>, &'static Activity), Without<Dead>>,
    buildings: Query<
        'w,
        's,
        (Entity, &'static Building, &'static Owner, &'static Position, &'static Stats, Option<&'static UnderConstruction>, Option<&'static ProductionQueue>),
        Without<Dead>,
    >,
    selected: Query<'w, 's, Entity, With<Selected>>,
}

impl PickWorld<'_, '_> {
    fn candidates(&self, me: PlayerId) -> Vec<Candidate> {
        let blank = |entity, pos| Candidate {
            entity,
            pos,
            unit: None,
            building: None,
            worker: false,
            load: None,
            activity: Activity::Idle,
            hall: false,
            foundation: None,
            queued: 0,
        };
        let mut candidates: Vec<Candidate> = self
            .units
            .iter()
            .filter(|(_, _, o, ..)| o.0 == me)
            .map(|(e, u, _, p, g, a)| Candidate {
                unit: Some(u.kind),
                worker: g.is_some(),
                load: g.and_then(|g| g.carrying.filter(|_| g.amount > 0.0)),
                activity: *a,
                ..blank(e, p.0)
            })
            .collect();
        candidates.extend(self.buildings.iter().filter(|(_, _, o, ..)| o.0 == me).map(|(e, b, _, p, st, uc, q)| Candidate {
            building: Some(b.kind),
            hall: st.tags.contains(Tag::TownCenter),
            foundation: uc.map(|c| c.progress),
            queued: q.map_or(0, |q| q.items.len()),
            ..blank(e, p.0)
        }));
        candidates
    }

    /// Select what `pick` picks among `me`'s entities, replacing the
    /// selection. `None` (and nothing changed) when nothing matches; else
    /// how many were picked and where they stand on average.
    fn select(&self, commands: &mut Commands, pick: Pick, me: PlayerId) -> Option<(usize, Vec2)> {
        let candidates = self.candidates(me);
        let picked = pick_entities(pick, &candidates);
        if picked.is_empty() {
            return None;
        }
        for e in self.selected.iter().filter(|e| !picked.contains(e)) {
            commands.entity(e).try_remove::<Selected>();
        }
        for e in &picked {
            commands.entity(*e).try_insert(Selected);
        }
        let points: Vec<Vec2> = candidates.iter().filter(|c| picked.contains(&c.entity)).map(|c| c.pos).collect();
        Some((picked.len(), points.iter().sum::<Vec2>() / points.len().max(1) as f32))
    }
}

/// Make the selection once its time has come and something matches
/// (retrying each frame until then), and look at it unless `--camera`
/// frames the view.
#[allow(clippy::too_many_arguments)]
fn apply_selection(
    mut commands: Commands,
    plan: Res<SelectPlan>,
    clock: Res<SimClock>,
    local: Res<LocalPlayer>,
    camera: Option<Res<CameraArg>>,
    picks: PickWorld,
    mut rig: ResMut<CameraRig>,
    mut done: Local<bool>,
) {
    if *done || clock.elapsed < plan.at {
        return;
    }
    let Some((n, centre)) = picks.select(&mut commands, plan.pick, local.0.unwrap_or(PlayerId(0))) else { return };
    *done = true;
    commands.insert_resource(PickedAt(clock.elapsed));
    if camera.is_none() {
        rig.focus = centre;
    }
    info!("dev: selected {n} ({:?})", plan.pick);
}

/// The units `--follow` keeps the camera on.
type Followed = (With<Selected>, With<Unit>, Without<Dead>);

/// `--follow`: from the `--select` pick on, keep the camera on the selected
/// units (where they are drawn), as long as any of them lives.
fn follow_selection(selected: Query<&Transform, Followed>, mut rig: ResMut<CameraRig>) {
    let (sum, n) = selected.iter().fold((Vec2::ZERO, 0), |(sum, n), t| (sum + t.translation.xz(), n + 1));
    if n > 0 {
        rig.focus = sum / n as f32;
    }
}

// ---------------------------------------------------------------------------
// Placement and hover
// ---------------------------------------------------------------------------

#[derive(Resource, Clone, Copy)]
struct PlacePlan(BuildingTypeId);

/// Put the local player in placement mode for the building (once). A wall
/// line is started [`WALL_PREVIEW_LENGTH`] left of the cursor, so its
/// preview runs up to the cursor.
fn hold_placement(plan: Res<PlacePlan>, data: Res<Data>, cursor: Res<CursorWorld>, rig: Res<CameraRig>, mut mode: ResMut<InputMode>, mut done: Local<bool>) {
    if *done {
        return;
    }
    let kind = plan.0;
    if !data.building(kind).wall {
        *mode = InputMode::Place { kind, facing: 0.0 };
        *done = true;
    } else if let Some(ground) = cursor.ground {
        let (_, right) = af_input::view_axes(rig.yaw);
        *mode = InputMode::PlaceWall { kind, start: Some(ground - right * WALL_PREVIEW_LENGTH) };
        *done = true;
    }
}

/// `--press`: the label of a UI button to press once, and when (sim seconds).
#[derive(Resource, Clone, Debug)]
struct PressPlan {
    label: String,
    at: f32,
}

/// Does a button's text read `label` (line breaks and spacing aside)?
fn reads(text: &str, label: &str) -> bool {
    text.split_whitespace().eq(label.split_whitespace())
}

/// The HUD's buttons, found by the text they read.
#[derive(SystemParam)]
struct Buttons<'w, 's> {
    labels: Query<'w, 's, (&'static ChildOf, &'static Text, &'static InheritedVisibility)>,
    buttons: Query<'w, 's, &'static mut Interaction, With<Button>>,
}

impl Buttons<'_, '_> {
    /// Press the first shown button labelled so, as a click on it would.
    /// False when there is none (yet).
    fn press(&mut self, label: &str) -> bool {
        for (parent, text, shown) in &self.labels {
            if shown.get()
                && reads(&text.0, label)
                && let Ok(mut interaction) = self.buttons.get_mut(parent.parent())
            {
                *interaction = Interaction::Pressed;
                return true;
            }
        }
        false
    }

    /// Mark every button labelled so hovered (unless it is being pressed).
    fn hover(&mut self, label: &str) {
        for (parent, text, _) in &self.labels {
            if reads(&text.0, label)
                && let Ok(mut interaction) = self.buttons.get_mut(parent.parent())
                && *interaction != Interaction::Pressed
            {
                *interaction = Interaction::Hovered;
            }
        }
    }
}

/// Press the first button labelled so, once its time has come and it is
/// shown (retrying each frame until then), as a click on it would.
fn press_button(plan: Res<PressPlan>, clock: Res<SimClock>, mut buttons: Buttons, mut done: Local<bool>) {
    if *done || clock.elapsed < plan.at {
        return;
    }
    if buttons.press(&plan.label) {
        *done = true;
        info!("dev: pressed {:?}", plan.label);
    }
}

/// The button label `--hover` keeps hovered.
#[derive(Resource)]
struct HoverPlan(String);

/// Hold every button labelled so "hovered" (Bevy resets them from the real
/// mouse every frame) so their tooltips show: a unit's name labels both its
/// command card button and its slot in a production queue.
fn hold_hover(plan: Res<HoverPlan>, mut buttons: Buttons) {
    buttons.hover(&plan.0);
}

// ---------------------------------------------------------------------------
// Battle demo
// ---------------------------------------------------------------------------

/// `--battle-demo a,b[,count]`: `count` units of type `a` for player 0
/// against as many of `b` for player 1.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
struct BattlePlan {
    a: UnitTypeId,
    b: UnitTypeId,
    count: u32,
}

impl BattlePlan {
    fn parse(data: &GameData, s: &str) -> Result<BattlePlan, String> {
        let unit = |id: &str| lookup(id, data.unit_id(id), "unit", data.units.iter().map(|u| &u.id));
        match s.split(',').map(str::trim).collect::<Vec<_>>().as_slice() {
            [a, b] => Ok(BattlePlan { a: unit(a)?, b: unit(b)?, count: DEFAULT_BATTLE_COUNT }),
            [a, b, n] => {
                let count: u32 = number("--battle-demo", n)?;
                Ok(BattlePlan { a: unit(a)?, b: unit(b)?, count: count.clamp(1, MAX_BATTLE_COUNT) })
            }
            _ => Err(format!("--battle-demo wants `unit_a,unit_b[,count]`, got `{s}`")),
        }
    }
}

/// Grid slots for a block of units around `centre`, rows across `facing`
/// (the front row nearest the enemy), `spacing` apart. Yields more slots
/// than `count` so blocked ones can be skipped.
fn formation(centre: Vec2, facing: f32, count: u32, spacing: f32) -> impl Iterator<Item = Vec2> {
    let cols = (count as f32).sqrt().ceil().max(1.0) as i32;
    let (forward, right) = (af_geom::forward(facing), af_geom::right(facing));
    (0..).take((count as usize) * 3 + 8).map(move |i: i32| {
        let (row, col) = (i / cols, i % cols);
        let across = (col as f32 - (cols - 1) as f32 * 0.5) * spacing;
        centre + right * across - forward * row as f32 * spacing
    })
}

/// Spawn one side: `count` units of `kind` for `owner` in a block at
/// `centre` facing `facing`, on open ground and not overlapping each other.
#[allow(clippy::too_many_arguments)]
fn spawn_side(commands: &mut Commands, data: &GameData, nav: &NavGrid, owner: &PlayerState, kind: UnitTypeId, centre: Vec2, facing: f32, count: u32) -> Vec<Entity> {
    let def = data.unit(kind);
    let footprint = Footprint::from_size(def.size);
    let mobility = data.unit_mobility(kind, owner.faction);
    let radius = footprint.half.x.max(0.2);
    let spacing = footprint.half.max_element() * 2.0 + 0.8;
    let mut placed: Vec<Obb> = Vec::new();
    let mut out = Vec::new();
    for slot in formation(centre, facing, count, spacing) {
        if out.len() == count as usize {
            break;
        }
        let Some(open) = nav.nearest_open(slot, mobility.0, radius, owner.team, 12.0) else { continue };
        // Keep the exact slot when its own cell is clear (cell centres would stack units).
        let pos = if nav.cell_of(open) == nav.cell_of(slot) { slot } else { open };
        let obb = footprint.obb(pos, facing);
        if placed.iter().any(|o| o.overlaps(&obb)) {
            continue;
        }
        placed.push(obb);
        out.push(spawn_unit(commands, data, owner, kind, pos, facing));
    }
    out
}

/// Longest weapon range of a unit type (0 without weapons).
fn max_range(data: &GameData, kind: UnitTypeId) -> f32 {
    data.unit(kind).weapon_ids.iter().map(|w| data.weapon(*w).range).fold(0.0, f32::max)
}

/// Stage the battle at the crater once the map is built: player 0's block
/// west of it, player 1's east, a little more than weapon range apart,
/// attack-moving into each other.
fn stage_battle(mut commands: Commands, plan: Res<BattlePlan>, data: Res<Data>, players: Res<Players>, nav: Res<Nav>, map: Res<MapInfo>, mut inbox: ResMut<CommandInbox>) {
    let (Some(p0), Some(p1)) = (players.try_get(PlayerId(0)), players.try_get(PlayerId(1))) else { return };
    if !players.hostile(p0.id, p1.id) {
        warn!("battle demo: players 0 and 1 are allies; their units will not fight");
    }
    let centre = if map.crater_radius > 0.0 { map.crater_center } else { map.size * 0.5 };
    let gap = (max_range(&data, plan.a).max(max_range(&data, plan.b)) + 8.0).clamp(14.0, 40.0);
    let (west, east) = (centre - Vec2::X * gap * 0.5, centre + Vec2::X * gap * 0.5);
    let a = spawn_side(&mut commands, &data, &nav, p0, plan.a, west, af_geom::yaw_of(Vec2::X), plan.count);
    let b = spawn_side(&mut commands, &data, &nav, p1, plan.b, east, af_geom::yaw_of(Vec2::NEG_X), plan.count);
    info!("battle demo: {} {} vs {} {} at {centre}", a.len(), data.unit(plan.a).name, b.len(), data.unit(plan.b).name);
    inbox.push(PlayerCommand::new(p0.id, a, CommandKind::AttackMove(east)));
    inbox.push(PlayerCommand::new(p1.id, b, CommandKind::AttackMove(west)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use af_core::testing::{flat_world, run_ticks, sim_app, test_data, with_commands};

    fn args(list: &[&str]) -> Result<DevArgs, String> {
        let mut dev = DevArgs::default();
        let mut it = list.iter().map(|s| s.to_string());
        while let Some(arg) = it.next() {
            if !dev.take(&arg, &mut it)? {
                return Err(format!("not a dev flag: {arg}"));
            }
        }
        Ok(dev)
    }

    fn setup(players: usize, local: Option<usize>) -> MatchSetup {
        let slot = |i: usize| PlayerSetup { name: format!("P{i}"), faction: FactionId(i as u8 % 6), team: i as u8, controller: Controller::Ai, color: None };
        MatchSetup { players: (0..players).map(slot).collect(), local_player: local, ..default() }
    }

    #[test]
    fn frame_stats_summarise_and_need_a_positive_period() {
        let mut frames = vec![10.0, 30.0, 20.0, 16.0, 14.0];
        let s = FrameStats::summary(&mut frames);
        assert!(s.starts_with("5 frames: mean 18.00 ms (56 fps), p50 16.00"), "{s}");
        assert!(s.ends_with("max 30.00"), "{s}");
        assert_eq!(FrameStats::summary(&mut []), "no frames");
        let data = test_data();
        assert!(args(&["--frame-stats", "0"]).unwrap().plan(&data, &mut setup(2, None)).is_err());
        let plan = args(&["--frame-stats", "5", "--no-models"]).unwrap().plan(&data, &mut setup(2, None)).unwrap();
        assert_eq!((plan.frame_stats, plan.no_models), (Some(5.0), true));
        assert_eq!(plan.present_mode(), PresentMode::AutoVsync);
        assert_eq!(args(&["--no-vsync"]).unwrap().plan(&data, &mut setup(2, None)).unwrap().present_mode(), PresentMode::AutoNoVsync);
    }

    #[test]
    fn camera_spots_parse() {
        assert_eq!(CameraArg::parse("120,80").unwrap(), CameraArg { spot: Spot::Point(Vec2::new(120.0, 80.0)), distance: None, turn: None });
        assert_eq!(CameraArg::parse("start,12,90").unwrap(), CameraArg { spot: Spot::Start(None), distance: Some(12.0), turn: Some(90.0) });
        assert_eq!(CameraArg::parse("1,2,30,-45").unwrap().turn, Some(-45.0));
        assert_eq!(CameraArg::parse("10, 20, 90").unwrap().distance, Some(90.0));
        assert_eq!(CameraArg::parse("crater,70").unwrap(), CameraArg { spot: Spot::Crater, distance: Some(70.0), turn: None });
        assert_eq!(CameraArg::parse("p3").unwrap().spot, Spot::Start(Some(3)));
        assert_eq!(CameraArg::parse("start").unwrap().spot, Spot::Start(None));
        assert!(CameraArg::parse("12").is_err());
        assert!(CameraArg::parse("crater,1,2,3").is_err());
        assert!(CameraArg::parse("x,y").is_err());
    }

    #[test]
    fn shot_series_are_named_and_timed() {
        let dev = args(&["--shots", "out/late", "--shot-at", "300", "--shot-every", "150", "--shot-count", "3"]).unwrap();
        let plan = dev.shot_plan().unwrap().unwrap();
        let times: Vec<f32> = plan.shots.iter().map(|(t, _)| *t).collect();
        assert_eq!(times, vec![300.0, 450.0, 600.0]);
        assert_eq!(plan.shots[2].1, PathBuf::from("out/late_02_600s.png"));
        assert_eq!(plan.clock, ShotClock::Sim);
        // The single screenshot keeps its old flags; on the menu time is real.
        let single = args(&["--screenshot", "a.png", "--screenshot-at", "4", "--menu"]).unwrap().shot_plan().unwrap().unwrap();
        assert_eq!((single.shots.len(), single.shots[0].0, single.clock), (1, 4.0, ShotClock::Real));
        assert!(args(&["--screenshot", "a.png", "--shots", "b"]).unwrap().shot_plan().is_err());
        assert!(args(&["--shots", "b", "--shot-every", "0"]).unwrap().shot_plan().is_err());
        assert!(args(&["--shot-count", "x"]).is_err());
        // `+S`: from the --select pick on.
        let after = args(&["--shots", "c", "--shot-at", "+0.5", "--shot-every", "0.25", "--shot-count", "2"]).unwrap().shot_plan().unwrap().unwrap();
        assert_eq!(after.clock, ShotClock::AfterSelect);
        assert_eq!(after.shots, vec![(0.5, PathBuf::from("c_00_0.5s.png")), (0.75, PathBuf::from("c_01_0.8s.png"))]);
        assert!(args(&["--shots", "c", "--shot-at", "+1", "--menu"]).unwrap().shot_plan().is_err());
        let data = test_data();
        let err = args(&["--shots", "c", "--shot-at", "+1"]).unwrap().plan(&data, &mut setup(2, Some(0))).err().unwrap_or_default();
        assert!(err.contains("add --select"), "{err}");
        let err = args(&["--shots", "c", "--shot-at", "+1", "--select", "worker", "--script", "1 shot a.png"]).unwrap().plan(&data, &mut setup(2, Some(0))).err();
        assert!(err.unwrap_or_default().contains("drop the +"));
    }

    #[test]
    fn plans_check_ids_and_adjust_the_match() {
        let data = test_data();
        let mut s = setup(2, Some(0));
        let plan = args(&["--battle-demo", "spearman,archer,5", "--autoplay", "--select", "unit:spearman", "--place", "house"]).unwrap().plan(&data, &mut s).unwrap();
        assert!(s.reveal_map, "battle demos reveal the map");
        assert_eq!(s.players[0].controller, Controller::Ai, "the local slot plays itself");
        assert_eq!(plan.battle.unwrap().count, 5);
        assert_eq!(plan.select.unwrap().pick, Pick::Units(data.unit_id("spearman").unwrap()));
        assert_eq!(plan.virtual_cursor, Some(None), "placing points a stand-in mouse at the centre");
        assert!(plan.shots.is_none() && plan.camera.is_none());

        let err = |list: &[&str], s: &mut MatchSetup| args(list).unwrap().plan(&data, s).err().unwrap_or_default();
        assert!(err(&["--battle-demo", "spearman,nobody"], &mut setup(2, None)).contains("unknown unit `nobody`"));
        assert!(err(&["--battle-demo", "spearman,archer"], &mut setup(1, None)).contains("two players"));
        assert!(err(&["--place", "house"], &mut setup(2, None)).contains("local player"));
        assert!(err(&["--autoplay"], &mut setup(2, None)).contains("local player"));
        assert!(err(&["--select", "everything"], &mut setup(2, None)).contains("want hall"));
        assert!(err(&["--select", "building:castle_of_glass"], &mut setup(2, None)).contains("unknown building"));
        assert!(!args(&[]).unwrap().any());
        let pressing = args(&["--select", "worker", "--select-at", "3", "--press", "Economy"]).unwrap().plan(&data, &mut setup(2, Some(0))).unwrap();
        assert_eq!(pressing.press.map(|p| p.at), Some(3.5), "a press follows the selection");
        assert!(reads("Raise the\nBanners", "Raise the Banners") && !reads("Raise", "Raise the Banners"));
    }

    #[test]
    fn scripts_plan_their_shots_and_need_a_match() {
        let data = test_data();
        let dev = args(&["--script", "2 key Z; 3 shot b.png; 1 shot a.png", "--screenshot", "c.png", "--screenshot-at", "2.5"]).unwrap();
        assert!(dev.takes_shots(), "a script starts the match at once");
        let plan = dev.plan(&data, &mut setup(2, Some(0))).unwrap();
        let shots = plan.shots.unwrap();
        let order: Vec<(f32, PathBuf)> = shots.shots.clone();
        assert_eq!(order, vec![(1.0, "a.png".into()), (2.5, "c.png".into()), (3.0, "b.png".into())], "merged in time order");
        assert_eq!(shots.clock, ShotClock::Sim);
        assert_eq!(plan.virtual_cursor, Some(None), "a script points a stand-in mouse");
        assert!(plan.script.is_some());
        let err = args(&["--script", "1 key Z", "--menu"]).unwrap().plan(&data, &mut setup(2, Some(0))).err().unwrap_or_default();
        assert!(err.contains("drop --menu"), "{err}");
        let err = args(&["--script", "1 fly away"]).unwrap().plan(&data, &mut setup(2, Some(0))).err().unwrap_or_default();
        assert!(err.contains("unknown action"), "{err}");
    }

    #[test]
    fn picks_choose_the_right_entities() {
        let mut world = World::new();
        let e: Vec<Entity> = (0..6).map(|_| world.spawn_empty().id()).collect();
        let blank = |i: usize| Candidate {
            entity: e[i],
            pos: Vec2::splat(i as f32),
            unit: None,
            building: None,
            worker: false,
            load: None,
            activity: Activity::Idle,
            hall: false,
            foundation: None,
            queued: 0,
        };
        let (peasant, spear, tc, barracks) = (UnitTypeId(0), UnitTypeId(1), BuildingTypeId(0), BuildingTypeId(1));
        let wood = Some(ResourceId(1));
        let c = vec![
            Candidate { unit: Some(peasant), worker: true, activity: Activity::Gather(WorkStyle::Chop), ..blank(0) },
            Candidate { unit: Some(peasant), worker: true, load: wood, ..blank(1) },
            Candidate { unit: Some(spear), activity: Activity::Fight, ..blank(2) },
            Candidate { building: Some(tc), hall: true, queued: 2, ..blank(3) },
            Candidate { building: Some(barracks), foundation: Some(0.4), ..blank(4) },
            Candidate { building: Some(barracks), queued: 5, ..blank(5) },
        ];
        assert_eq!(pick_entities(Pick::Worker, &c), vec![e[1]], "a worker with a load first");
        let mut workers = vec![e[0], e[1]];
        workers.sort();
        assert_eq!(pick_entities(Pick::Workers, &c), workers, "in entity order");
        assert_eq!(pick_entities(Pick::Army, &c), vec![e[2]]);
        assert_eq!(pick_entities(Pick::Hall, &c), vec![e[3]]);
        assert_eq!(pick_entities(Pick::Foundation, &c), vec![e[4]]);
        assert_eq!(pick_entities(Pick::Busy, &c), vec![e[5]]);
        assert_eq!(pick_entities(Pick::Building(barracks), &c), vec![e[5]], "complete before foundations");
        assert_eq!(pick_entities(Pick::Units(spear), &c), vec![e[2]]);
        assert!(pick_entities(Pick::Units(UnitTypeId(9)), &c).is_empty());
        assert_eq!(pick_entities(Pick::Doing(Doing::Is(Activity::Gather(WorkStyle::Chop))), &c), vec![e[0]]);
        assert_eq!(pick_entities(Pick::Doing(Doing::Gathering), &c), vec![e[0]]);
        assert_eq!(pick_entities(Pick::Doing(Doing::Is(Activity::Fight)), &c), vec![e[2]]);
        assert_eq!(pick_entities(Pick::Doing(Doing::Is(Activity::Idle)), &c), vec![e[1]], "a worker first");
        assert!(pick_entities(Pick::Doing(Doing::Is(Activity::Build)), &c).is_empty());
        assert_eq!(pick_entities(Pick::Carrying(wood), &c), vec![e[1]]);
        assert_eq!(pick_entities(Pick::Carrying(None), &c), vec![e[1]]);
        assert!(pick_entities(Pick::Carrying(Some(ResourceId(0))), &c).is_empty());
        let mut busy = c.clone();
        busy[1].activity = Activity::Gather(WorkStyle::Chop);
        assert!(pick_entities(Pick::Carrying(wood), &busy).is_empty(), "still filling its load at work");
        busy[1].activity = Activity::Build;
        assert!(pick_entities(Pick::Carrying(wood), &busy).is_empty(), "building, the load in its pack");
    }

    #[test]
    fn activity_and_cargo_picks_parse() {
        let data = test_data();
        let pick = |s: &str| Pick::parse(&data, s);
        assert_eq!(pick("doing:chop").unwrap(), Pick::Doing(Doing::Is(Activity::Gather(WorkStyle::Chop))));
        assert_eq!(pick("doing:smash").unwrap(), Pick::Doing(Doing::Is(Activity::Gather(WorkStyle::Smash))));
        assert_eq!(pick("doing:gather").unwrap(), Pick::Doing(Doing::Gathering));
        assert_eq!(pick("doing:repair").unwrap(), Pick::Doing(Doing::Is(Activity::Repair)));
        assert_eq!(pick("carrying:wood").unwrap(), Pick::Carrying(data.resource_id("wood")));
        assert_eq!(pick("carrying:any").unwrap(), Pick::Carrying(None));
        assert!(pick("doing:dance").unwrap_err().contains("doing:idle|gather"));
        assert!(pick("carrying:gold").unwrap_err().contains("unknown resource `gold`"));
    }

    #[test]
    fn follow_needs_a_pick_and_close_ups_frame_closer_than_the_wheel() {
        let data = test_data();
        let err = args(&["--follow"]).unwrap().plan(&data, &mut setup(2, Some(0))).err().unwrap_or_default();
        assert!(err.contains("add --select"), "{err}");
        let plan = args(&["--follow", "--select", "doing:chop"]).unwrap().plan(&data, &mut setup(2, Some(0))).unwrap();
        assert!(plan.follow);
        let mut rig = CameraRig::default();
        let map = MapInfo { size: Vec2::splat(100.0), ..default() };
        CameraArg::parse("50,50,12").unwrap().apply(&map, &Players::default(), None, &mut rig);
        assert_eq!(rig.distance, 12.0);
        CameraArg::parse("50,50,1").unwrap().apply(&map, &Players::default(), None, &mut rig);
        assert_eq!(rig.distance, CLOSE_UP_DISTANCE);
        let yaw = rig.yaw;
        CameraArg::parse("50,50,12,180").unwrap().apply(&map, &Players::default(), None, &mut rig);
        assert!((af_geom::wrap_angle(rig.yaw - af_input::VIEW_YAW).abs() - std::f32::consts::PI).abs() < 1e-5, "looking from the other side");
        assert_eq!(yaw, CameraRig::default().yaw, "no turn given: the yaw is left alone");
    }

    #[test]
    fn battle_sides_spawn_apart_without_overlap() {
        let data = test_data();
        let mut app = sim_app(data.clone());
        flat_world(&mut app, 128.0, &[("ironhold", 0), ("dune", 1)]);
        let (a, b) = (data.unit_id("spearman").unwrap(), data.unit_id("archer").unwrap());
        let (west, east) = (Vec2::new(50.0, 64.0), Vec2::new(78.0, 64.0));
        let nav = app.world().resource::<Nav>().0.clone();
        let (left, right) = with_commands(&mut app, |c, data, players| {
            let side = |c: &mut Commands, owner: u8, kind, centre, toward: Vec2| spawn_side(c, data, &nav, players.get(PlayerId(owner)), kind, centre, af_geom::yaw_of(toward), 9);
            (side(c, 0, a, west, Vec2::X), side(c, 1, b, east, Vec2::NEG_X))
        });
        assert_eq!((left.len(), right.len()), (9, 9));
        run_ticks(&mut app, 1);
        let world = app.world_mut();
        let mut q = world.query::<(Entity, &Owner, &Position, &Facing, &Footprint)>();
        let boxes: Vec<(Entity, PlayerId, Obb)> = q.iter(world).map(|(e, o, p, f, fp)| (e, o.0, fp.obb(p.0, f.0))).collect();
        for (i, (_, _, x)) in boxes.iter().enumerate() {
            for (_, _, y) in &boxes[i + 1..] {
                assert!(!x.overlaps(y), "spawned boxes never overlap");
            }
        }
        let side = |owner: u8| boxes.iter().filter(|(_, o, _)| o.0 == owner).map(|(_, _, b)| b.center.x).fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
        assert!(side(0).1 < side(1).0, "player 0 west of player 1");
        assert!(left.iter().chain(&right).all(|e| boxes.iter().any(|(b, ..)| b == e)));
    }
}
