//! Aetherfall — box-graphics RTS.
//!
//! Windowed (default): opens the skirmish menu.
//!   cargo run -p aetherfall
//!   cargo run -p aetherfall -- --quick --faction ironhold --players 3
//!   cargo run -p aetherfall -- --quick --spectate --players 6 --reveal --speed 4
//!   cargo run -p aetherfall -- --quick --screenshot shot.png --screenshot-at 20
//!
//! Headless (no window, as fast as the CPU allows):
//!   cargo run -p aetherfall -- --headless --players 2 --minutes 15 --report-every 60
//!
//! Options:
//!   --headless              run the simulation without a window
//!   --quick                 skip the menu and start a skirmish
//!   --spectate              no human player (everyone AI; view reveals all)
//!   --players N             number of players (1-8, default 2)
//!   --faction ID            faction for slot 0 (repeat --faction for more slots,
//!                           or --factions a,b,c)
//!   --teams 0,0,1,1         team per slot (default: free for all)
//!   --seed N                map/AI seed (default 1)
//!   --map-size M            square map side in metres (default: by player count)
//!   --no-fog / --reveal     fog of war off / map revealed
//!   --start-age N           start every player in age N (debug)
//!   --res-mult X            multiply starting resources (debug)
//!   --speed X               game speed multiplier (windowed; 0 < X <= 100)
//!   --minutes M / --ticks N headless length (default 20 minutes)
//!   --report-every S        headless: print a report every S sim seconds (default 120)
//!
//! Dev and screenshot tooling (windowed; times are sim seconds):
//!   --screenshot PATH       save one screenshot and exit
//!   --screenshot-at S       ... after S seconds (default 10; alias --shot-at;
//!                           +S: S seconds after the --select pick is made)
//!   --shots PREFIX          a series instead: PREFIX_<i>_<t>s.png, the first at
//!                           --shot-at, then every --shot-every S (default 60),
//!                           --shot-count N of them (default 1); exits after the last
//!   --menu                  stay on the menu while taking screenshots (times
//!                           are then real seconds)
//!   --camera SPOT[,DIST[,TURN]]  frame the view once the map exists: x,z or
//!                           crater | centre | start | p<N>, optionally at
//!                           DIST metres (5-150; the wheel zooms 25-150) and
//!                           seen from TURN degrees round (clockwise from above)
//!   --select WHAT           select once, after --select-at S (default 1), and
//!                           look at it unless --camera is given: hall | worker
//!                           | workers | army | foundation | busy (longest
//!                           queue) | building:<id> | unit:<id> | doing:<what>
//!                           (one unit busy so: idle, gather, forage, farm,
//!                           chop, mine, dig, chip, smash, build, repair, heal,
//!                           fight) | carrying:<resource|any> (one worker
//!                           taking a load home)
//!   --follow                keep the camera on the --select pick
//!   --place BUILDING        hold the local player in placement mode for a
//!                           building id (walls: a line ending at the cursor)
//!   --hover LABEL           keep the UI buttons with this label hovered (tooltips)
//!   --press LABEL           press the UI button with this label once, at
//!                           --press-at S (default half a second after --select)
//!   --cursor X,Y            where the stand-in mouse points (logical px; default
//!                           the window centre). Screenshot and --place runs
//!                           always ignore the real mouse.
//!   --battle-demo A,B[,N]   once the map is built, N (default 8) units of type A
//!                           for player 0 and of B for player 1 fight at the
//!                           crater (reveals the map)
//!   --autoplay              the AI plays the local player's slot (its HUD shows)
//!   --pause-at S            pause the game at S seconds (shots still due are
//!                           then taken, paused, one a frame)
//!   --frame-stats S         log frame times (mean, percentiles, fps) and unit
//!                           counts every S real seconds
//!   --no-models             draw every unit as boxes (no glTF models)
//!   --no-vsync              present frames as fast as they are made
//!   --script "STEPS"        play the match from a script: steps `<S> <action>`
//!                           separated by `;`, where an action is
//!                             key <combo>        Q, 1, ctrl+1, shift+R, Esc, Space, F1...
//!                             move <x>,<y>       the stand-in mouse (logical px)
//!                             click [left|right] [shift|ctrl]   in the world
//!                             down/up [left|right]              (drags)
//!                             press <label>      a HUD button, as --press
//!                             select <what>      as --select
//!                             camera <spot>      as --camera
//!                             defeat <player>    delete all that player owns
//!                             damage <what> <f>  set what `select` would pick to
//!                                                fraction f of its hit points
//!                             shot <path>        a screenshot (exits after the last)
//!                           e.g. --script "1 select workers; 1.5 key Z; 2 key W;
//!                                2.5 move 700,500; 2.6 click; 3 shot house.png"

mod dev;

use std::path::Path;
use std::sync::Arc;

use aetherfall::*;
use af_core::prelude::*;
use bevy::prelude::*;
use bevy::window::WindowResolution;

#[derive(Debug, Default)]
struct Args {
    headless: bool,
    quick: bool,
    opts: SkirmishOptions,
    speed: f32,
    ticks: u64,
    report_every_secs: f32,
    dev: dev::DevArgs,
}

/// The fastest `--speed` accepted. Bevy panics on a negative or non-finite
/// virtual-clock speed (`Time<Virtual>::set_relative_speed`) and on a scaled
/// frame delta too large for a `Duration`; this bound stays far below that.
const MAX_SPEED: f32 = 100.0;

/// `--speed`: a finite multiplier in `(0, MAX_SPEED]`, so a bad value is a
/// clean `aetherfall: ...` error rather than a Bevy assert on the first frame.
fn parse_speed(value: &str) -> Result<f32, String> {
    let speed: f32 = value.parse().map_err(|e| format!("--speed: {e}"))?;
    if !(speed.is_finite() && speed > 0.0 && speed <= MAX_SPEED) {
        return Err(format!("--speed must be a number above 0 and at most {MAX_SPEED} (got `{value}`)"));
    }
    Ok(speed)
}

fn next(it: &mut impl Iterator<Item = String>, name: &str) -> Result<String, String> {
    it.next().ok_or_else(|| format!("{name} needs a value"))
}

/// Parse the command line (without the program name).
fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut a = Args { speed: 1.0, ticks: 20 * 60 * 30, report_every_secs: 120.0, ..Default::default() };
    a.opts = SkirmishOptions::default();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--headless" => a.headless = true,
            "--quick" => a.quick = true,
            "--spectate" => a.opts.human = false,
            "--players" => a.opts.players = next(&mut it, "--players")?.parse().map_err(|e| format!("--players: {e}"))?,
            "--faction" => a.opts.factions.push(next(&mut it, "--faction")?),
            "--factions" => a.opts.factions.extend(next(&mut it, "--factions")?.split(',').map(|s| s.trim().to_string())),
            "--teams" => {
                a.opts.teams = Some(
                    next(&mut it, "--teams")?
                        .split(',')
                        .map(|s| s.trim().parse::<u8>().map_err(|e| format!("--teams: {e}")))
                        .collect::<Result<_, _>>()?,
                )
            }
            "--seed" => a.opts.seed = next(&mut it, "--seed")?.parse().map_err(|e| format!("--seed: {e}"))?,
            "--map-size" => a.opts.map_size = next(&mut it, "--map-size")?.parse().map_err(|e| format!("--map-size: {e}"))?,
            "--no-fog" => a.opts.fog = false,
            "--reveal" => a.opts.reveal = true,
            "--start-age" => a.opts.start_age = next(&mut it, "--start-age")?.parse().map_err(|e| format!("--start-age: {e}"))?,
            "--res-mult" => a.opts.res_mult = next(&mut it, "--res-mult")?.parse().map_err(|e| format!("--res-mult: {e}"))?,
            "--speed" => a.speed = parse_speed(&next(&mut it, "--speed")?)?,
            "--minutes" => {
                let m: f64 = next(&mut it, "--minutes")?.parse().map_err(|e| format!("--minutes: {e}"))?;
                a.ticks = (m * 60.0 * SIM_HZ) as u64;
            }
            "--ticks" => a.ticks = next(&mut it, "--ticks")?.parse().map_err(|e| format!("--ticks: {e}"))?,
            "--report-every" => a.report_every_secs = next(&mut it, "--report-every")?.parse().map_err(|e| format!("--report-every: {e}"))?,
            "-h" | "--help" => {
                // The module docs above, keeping the indentation of continued lines.
                let docs = include_str!("main.rs").lines().take_while(|l| l.starts_with("//!"));
                println!("{}", docs.map(|l| l.strip_prefix("//! ").unwrap_or(l.trim_start_matches("//!"))).collect::<Vec<_>>().join("\n"));
                std::process::exit(0);
            }
            other => {
                if !a.dev.take(other, &mut it)? {
                    return Err(format!("unknown argument `{other}` (try --help)"));
                }
            }
        }
    }
    if a.headless {
        if a.dev.any() {
            return Err("the dev and screenshot flags need a window (drop --headless)".into());
        }
        a.opts.human = false;
    }
    // A screenshot run starts the match at once unless it is of the menu.
    if a.dev.takes_shots() && !a.dev.menu {
        a.quick = true;
    }
    Ok(a)
}

fn main() -> AppExit {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("aetherfall: {e}");
            return AppExit::error();
        }
    };
    // The content and every other asset (models) come from the same
    // `assets/` folder, wherever the game finds it.
    let Some(data_dir) = af_data::find_data_dir() else {
        eprintln!("aetherfall: could not find an `assets/data` folder (run from the project, or set BEVY_ASSET_ROOT to it)");
        return AppExit::error();
    };
    let data = match GameData::load_dir(&data_dir) {
        Ok(d) => Arc::new(d),
        Err(e) => {
            eprintln!("aetherfall: game content failed to load\n{e}");
            return AppExit::error();
        }
    };
    let setup = match skirmish_setup(&data, &args.opts) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("aetherfall: {e}");
            return AppExit::error();
        }
    };
    if args.headless {
        run_headless_cli(data, setup, &args)
    } else {
        let assets = data_dir.parent().unwrap_or(&data_dir).to_path_buf();
        run_windowed(data, setup, args, &assets)
    }
}

fn run_headless_cli(data: Arc<GameData>, setup: MatchSetup, args: &Args) -> AppExit {
    let started = std::time::Instant::now();
    let mut app = headless_app(data, setup, 30);
    let every = (args.report_every_secs * SIM_HZ as f32) as u64;
    run_headless(&mut app, args.ticks, every, |app| println!("{}", match_report(app.world_mut())));
    println!("{}", match_report(app.world_mut()));
    let clock = *app.world().resource::<SimClock>();
    println!(
        "simulated {:.0}s ({} ticks) in {:.1}s wall ({:.0}x real time)",
        clock.elapsed,
        clock.tick,
        started.elapsed().as_secs_f32(),
        clock.elapsed / started.elapsed().as_secs_f32().max(0.001)
    );
    AppExit::Success
}

/// `assets`: the asset folder (the one holding the content folder).
fn run_windowed(data: Arc<GameData>, mut setup: MatchSetup, args: Args, assets: &Path) -> AppExit {
    let dev = match args.dev.plan(&data, &mut setup) {
        Ok(plan) => plan,
        Err(e) => {
            eprintln!("aetherfall: {e}");
            return AppExit::error();
        }
    };
    let mut app = App::new();
    app.set_error_handler(bevy::ecs::error::warn);
    // Left alone, Bevy looks for `assets/` under BEVY_ASSET_ROOT, else
    // CARGO_MANIFEST_DIR (`crates/aetherfall` under `cargo run`), else next
    // to the executable. Point it at the folder the content came from, so
    // a fresh clone needs no per-machine BEVY_ASSET_ROOT.
    let assets = AssetPlugin { file_path: assets.to_string_lossy().into_owned(), ..default() };
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Aetherfall".into(),
                    resolution: WindowResolution::new(1600, 900),
                    present_mode: dev.present_mode(),
                    ..default()
                }),
                ..default()
            })
            .set(assets),
    );
    app.insert_state(if args.quick { GameState::Playing } else { GameState::Menu });
    app.insert_resource(Data(data));
    app.insert_resource(setup);
    add_sim_plugins(&mut app);
    add_client_plugins(&mut app);
    let speed = args.speed;
    if speed != 1.0 {
        app.add_systems(Startup, move |mut t: ResMut<Time<Virtual>>| t.set_relative_speed(speed));
    }
    dev::install(&mut app, dev);
    app.run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    fn parse(list: &[&str]) -> Result<Args, String> {
        parse_args(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn speed_flag_accepts_positive_multipliers() {
        assert_eq!(parse(&[]).unwrap().speed, 1.0);
        assert_eq!(parse(&["--speed", "4"]).unwrap().speed, 4.0);
        assert_eq!(parse(&["--speed", "0.5"]).unwrap().speed, 0.5);
        assert_eq!(parse(&["--speed", "100"]).unwrap().speed, MAX_SPEED);
    }

    /// Each of these used to parse, and then Bevy panicked on the first frame
    /// ("tried to go back in time" / "tried to go infinitely fast" / a scaled
    /// delta overflowing `Duration`) instead of the usual `aetherfall:` error.
    #[test]
    fn speed_flag_rejects_values_bevy_cannot_take() {
        for bad in ["-1", "-0.5", "nan", "NaN", "inf", "-inf", "infinity", "1e39", "1e30", "0", "-0", "100.5"] {
            let err = parse(&["--speed", bad]).err().unwrap_or_else(|| panic!("--speed {bad} was accepted"));
            assert!(err.starts_with("--speed"), "--speed {bad}: {err}");
        }
        assert!(parse(&["--speed", "fast"]).is_err());
        assert_eq!(parse(&["--speed"]).err().unwrap(), "--speed needs a value");
    }

    /// The rejected values really do crash Bevy's virtual clock, and the
    /// accepted range does not, even over the longest frame it allows.
    #[test]
    fn accepted_speeds_run_the_virtual_clock() {
        for bad in [-1.0, f32::NAN, f32::INFINITY] {
            let panicked = std::panic::catch_unwind(|| Time::<Virtual>::default().set_relative_speed(bad)).is_err();
            assert!(panicked, "Bevy took relative speed {bad}");
        }
        for speed in [0.01, 0.5, 8.0, MAX_SPEED] {
            assert_eq!(parse_speed(&speed.to_string()).unwrap(), speed);
            let mut app = App::new();
            app.add_plugins(MinimalPlugins);
            app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(3600)));
            app.world_mut().resource_mut::<Time<Virtual>>().set_relative_speed(speed);
            app.update();
            app.update();
            let virt = app.world().resource::<Time<Virtual>>();
            assert!(virt.delta_secs() > 0.0 && virt.delta_secs() <= virt.max_delta().as_secs_f32() * speed + 1e-3, "speed {speed}: delta {}", virt.delta_secs());
        }
    }

    #[test]
    fn other_flags_still_parse() {
        let a = parse(&["--headless", "--players", "3", "--factions", "ironhold,dune", "--seed", "4", "--minutes", "2"]).unwrap();
        assert!(a.headless && !a.opts.human);
        assert_eq!((a.opts.players, a.opts.seed, a.ticks), (3, 4, (2.0 * 60.0 * SIM_HZ) as u64));
        assert_eq!(a.opts.factions, ["ironhold", "dune"]);
        assert!(parse(&["--bogus"]).unwrap_err().starts_with("unknown argument"));
    }
}
