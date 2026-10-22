//! Aetherfall — box-graphics RTS.
//!
//! Windowed (default): opens the skirmish menu.
//!   cargo run -p aetherfall
//!   cargo run -p aetherfall -- --quick --faction ironhold --players 3
//!   cargo run -p aetherfall -- --quick --spectate --players 6 --reveal --speed 4
//!   cargo run -p aetherfall -- --quick --map large --water coast --rivers 2 --site lake
//!   cargo run -p aetherfall -- --quick --screenshot shot.png --screenshot-at 20
//!
//! Headless (no window, as fast as the CPU allows):
//!   cargo run -p aetherfall -- --headless --players 2 --minutes 15 --report-every 60
//!
//! Options:
//!   --headless              run the simulation without a window
//!   --quick                 skip the menu and start a skirmish (else the menu
//!                           opens with these options chosen)
//!   --spectate              no human player (everyone AI; view reveals all)
//!   --players N             number of players (1-8, default 2)
//!   --faction ID            faction for slot 0 (repeat --faction for more slots,
//!                           or --factions a,b,c)
//!   --teams 0,0,1,1         team per slot (default: free for all)
//!   --seed N                map/AI seed (default 1)
//!   --map SIZE              map size: small | medium (default) | large | huge
//!   --water W               water, 0-4 or its name: 0 dry (rivers only) | 1 ponds
//!                           | 2 lakes (default) | 3 coast (one sea edge) | 4 seas
//!                           (two sea edges)
//!   --rivers N              rivers: 1 | 2 | random (default: one or two)
//!   --site KIND             meteor site: crater | shattered | lake | scar | spire
//!                           | random (default)
//!   --map-size M            map side in metres, 160-1024, instead of --map
//!                           (default: by player count and --map)
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
//!   --camera SPOT[,DIST[,TURN[,PITCH]]]  frame the view once the map exists:
//!                           x,z | crater | centre | start | p<N> (player N's
//!                           start) | site | site<N> (meteor site N, 0 the main
//!                           one; framed whole without DIST) | overview (the
//!                           whole map), optionally at DIST metres (5-6000;
//!                           the wheel zooms 25-150, up to 210 on huge maps),
//!                           seen from TURN degrees round (clockwise from
//!                           above) and PITCH degrees above the horizon (10-85;
//!                           the game's is 54). Leave a value empty to keep it:
//!                           overview,,45 lines the map up with the screen
//!   --no-hud                hide the HUD (screenshots of the world alone)
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
//!                           for player 0 and of B for player 1 fight on the open
//!                           ground nearest the main meteor site (reveals the map)
//!   --autoplay              the AI plays the local player's slot (its HUD shows)
//!   --pause-at S            pause the game at S seconds (shots still due are
//!                           then taken, paused, one a frame)
//!   --frame-stats S         log frame times (mean, percentiles, fps) and how
//!                           many units, and buildings and nodes, are drawn
//!                           as models, every S real seconds
//!   --no-models             draw everything as boxes (no glTF models)
//!   --visuals DIR           also read the visual registry files in assets/DIR
//!                           (on top of assets/visuals/, its entries win): try
//!                           models such as the art pipeline's placeholders
//!                           (--visuals visuals-placeholders)
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
//!                             hud on|off         show / hide the HUD
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
use af_core::ext::map::{MapSize, RiverCount, SiteChoice, WaterLevel};
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

/// The value of a map option flag, parsed by its `MapOptions` type.
fn map_option<T>(it: &mut impl Iterator<Item = String>, name: &str, parse: fn(&str) -> Option<T>, want: &str) -> Result<T, String> {
    let value = next(it, name)?;
    parse(&value).ok_or_else(|| format!("{name} wants {want}, got `{value}`"))
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
            "--map" => a.opts.map.size = map_option(&mut it, "--map", MapSize::parse, "small|medium|large|huge")?,
            "--water" => a.opts.map.water = map_option(&mut it, "--water", WaterLevel::parse, "0-4 or dry|ponds|lakes|coast|seas")?,
            "--rivers" => a.opts.map.rivers = map_option(&mut it, "--rivers", RiverCount::parse, "1|2|random")?,
            "--site" => a.opts.map.site = map_option(&mut it, "--site", SiteChoice::parse, "crater|shattered|lake|scar|spire|random")?,
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

/// The headless app for the command line's match: its setup and map options.
fn headless_cli_app(data: Arc<GameData>, setup: MatchSetup, args: &Args) -> App {
    let mut app = headless_app(data, setup, 30);
    app.insert_resource(args.opts.map);
    app
}

fn run_headless_cli(data: Arc<GameData>, setup: MatchSetup, args: &Args) -> AppExit {
    let started = std::time::Instant::now();
    let mut app = headless_cli_app(data, setup, args);
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
    if let Some(dir) = dev.visuals().filter(|d| !assets.join(d).is_dir()) {
        eprintln!("aetherfall: --visuals: no folder `{}` in {}", dir.display(), assets.display());
        return AppExit::error();
    }
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
    // The menu starts from both (and replaces both when a match starts).
    app.insert_resource(setup);
    app.insert_resource(args.opts.map);
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

    #[test]
    fn map_flags_parse_into_the_map_options() {
        use af_core::ext::map::{MapOptions, SiteKind};
        assert_eq!(parse(&[]).unwrap().opts.map, MapOptions::default());
        let a = parse(&["--map", "Large", "--water", "3", "--rivers", "2", "--site", "shattered", "--map-size", "300"]).unwrap();
        let want = MapOptions { size: MapSize::Large, water: WaterLevel::Coast, rivers: RiverCount::Two, site: SiteChoice::Kind(SiteKind::Shattered) };
        assert_eq!(a.opts.map, want);
        assert_eq!(a.opts.map_size, 300.0, "--map-size in metres stays");
        // Names or levels for the water; random choices; headless too.
        let b = parse(&["--headless", "--water", "seas", "--rivers", "random", "--site", "random", "--map", "huge"]).unwrap();
        assert_eq!((b.opts.map.water, b.opts.map.rivers, b.opts.map.site, b.opts.map.size), (WaterLevel::Seas, RiverCount::Random, SiteChoice::Random, MapSize::Huge));
        assert_eq!(parse(&["--water", "dry"]).unwrap().opts.map.water, WaterLevel::Dry);
        assert_eq!(parse(&["--rivers", "one"]).unwrap().opts.map.rivers, RiverCount::One);
        for (flag, bad) in [("--map", "gigantic"), ("--water", "5"), ("--water", "flood"), ("--rivers", "3"), ("--site", "volcano")] {
            let err = parse(&[flag, bad]).err().unwrap_or_else(|| panic!("{flag} {bad} was accepted"));
            assert!(err.starts_with(flag) && err.contains(bad), "{err}");
        }
        assert_eq!(parse(&["--site"]).err().unwrap(), "--site needs a value");
    }

    /// The map options reach the match next to its setup, headless as well.
    #[test]
    fn headless_runs_insert_the_map_options() {
        use af_core::ext::map::MapOptions;
        let a = parse(&["--headless", "--map", "small", "--water", "ponds", "--rivers", "1", "--site", "spire", "--players", "3"]).unwrap();
        let data = af_core::testing::test_data();
        let setup = skirmish_setup(&data, &a.opts).unwrap();
        let app = headless_cli_app(data, setup, &a);
        assert_eq!(app.world().get_resource::<MapOptions>(), Some(&a.opts.map));
        assert_eq!(app.world().resource::<MatchSetup>().players.len(), 3);
    }

    /// A run that starts on the menu shows the land side af_map will build
    /// for --map-size, also when that is not the side asked for.
    #[test]
    fn the_menu_shows_the_map_size_the_generator_builds() {
        let data = af_core::testing::test_data();
        for metres in ["100", "203.6", "260"] {
            let a = parse(&["--map-size", metres, "--water", "dry"]).unwrap();
            let setup = skirmish_setup(&data, &a.opts).unwrap();
            let built = af_map::generate_with(&data, &setup, &a.opts.map).size;
            let menu = af_ui::menu::MenuSettings::from_setup(&setup, a.opts.map);
            assert_eq!(menu.map_metres, built, "--map-size {metres}");
        }
        let (min, max) = af_ui::menu::MAP_METRES_RANGE;
        let help = include_str!("main.rs");
        assert!(help.contains(&format!("--map-size M            map side in metres, {min}-{max}")), "the --help range is af_map's");
    }

    /// Every flag the parser takes is in the --help text.
    #[test]
    fn help_lists_the_map_flags() {
        let help = include_str!("main.rs");
        for flag in ["--map SIZE", "--water W", "--rivers N", "--site KIND", "--map-size M", "--no-hud"] {
            assert!(help.lines().take_while(|l| l.starts_with("//!")).any(|l| l.contains(flag)), "{flag} missing from --help");
        }
    }
}
