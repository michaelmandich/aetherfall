//! Draw generated maps to PNG files, for reviewing the generator.
//!
//! ```text
//! cargo run -p af_map --example map_png -- --seed 7 --players 4 --map large \
//!     --water 3 --rivers 2 --site scar --out C:/tmp/map.png
//! cargo run -p af_map --example map_png -- --sheet --seeds 1-12 --vary site \
//!     --players 2 --out C:/tmp/sites.png
//! ```
//!
//! One map is drawn at 2 px per metre (`--scale` to change): each cell in
//! its terrain's colour (`TerrainDef::color`, mountains shaded by how far
//! into the range they are), every node and building as its box in its own
//! colour, each start ringed in white and each ford centre dotted in
//! yellow. `--sheet` draws a contact sheet instead: one tile per seed (and
//! per value of the `--vary` option), labelled, `--tile` pixels wide.
//! Every map's options, rivers, carved routes and anything cut off are
//! printed.

use std::time::Instant;

use af_core::ext::map::{MapOptions, MapSize, RiverCount, SiteChoice, SiteKind, WaterLevel};
use af_core::prelude::*;
use af_core::testing::test_data;
use af_map::{generate_with, GeneratedMap};
use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

const HELP: &str = "map_png: draw generated maps to PNG.
  --seed N            seed (default 1)
  --players P         player slots, 0-8 (default 2)
  --map SIZE          small|medium|large|huge (default medium)
  --map-size M        land side in metres (overrides --map)
  --water W           0-4 or dry|ponds|lakes|coast|seas (default lakes)
  --rivers R          1|2|random (default random)
  --site S            crater|shattered|lake|scar|spire|random (default random)
  --out PATH          output PNG (default map.png)
  --scale S           pixels per metre for one map (default 2)
  --crop X,Y,R        one map: only the square of half side R m round X,Y
  --sheet             a contact sheet instead of one map
  --seeds A-B         sheet: seeds A to B (default the one seed)
  --vary OPT          sheet: one row per value of site|water|rivers|map|players
  --tile PX           sheet: tile width in pixels (default 360)
  --cols N            sheet: tiles per row (default: per --vary value, else 4)";

/// An RGB canvas.
struct Canvas {
    w: usize,
    h: usize,
    px: Vec<[u8; 3]>,
}

impl Canvas {
    fn new(w: usize, h: usize, fill: [u8; 3]) -> Canvas {
        Canvas { w, h, px: vec![fill; w * h] }
    }

    fn set(&mut self, x: i32, y: i32, c: [u8; 3]) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.px[y as usize * self.w + x as usize] = c;
        }
    }

    /// The square of half side `r` pixels round `c` (clipped to the canvas).
    fn crop(&self, c: Vec2, r: f32) -> Canvas {
        let x0 = (c.x - r).max(0.0) as usize;
        let y0 = (c.y - r).max(0.0) as usize;
        let x1 = ((c.x + r) as usize).min(self.w);
        let y1 = ((c.y + r) as usize).min(self.h);
        let mut out = Canvas::new(x1.saturating_sub(x0).max(1), y1.saturating_sub(y0).max(1), [0, 0, 0]);
        for y in y0..y1 {
            for x in x0..x1 {
                out.px[(y - y0) * out.w + x - x0] = self.px[y * self.w + x];
            }
        }
        out
    }

    fn blit(&mut self, other: &Canvas, ox: usize, oy: usize) {
        for y in 0..other.h {
            for x in 0..other.w {
                self.set((ox + x) as i32, (oy + y) as i32, other.px[y * other.w + x]);
            }
        }
    }

    /// Text in a 3 x 5 pixel font scaled by `k`, top-left at (`x`, `y`).
    fn text(&mut self, s: &str, x: i32, y: i32, k: i32, c: [u8; 3]) {
        for (n, ch) in s.chars().enumerate() {
            let bits = glyph(ch.to_ascii_uppercase());
            for row in 0..5 {
                for col in 0..3 {
                    if bits & (1 << (14 - (row * 3 + col))) != 0 {
                        for dy in 0..k {
                            for dx in 0..k {
                                self.set(x + (n as i32 * 4 + col) * k + dx, y + row * k + dy, c);
                            }
                        }
                    }
                }
            }
        }
    }

    fn save(self, path: &str) -> Result<(), String> {
        let mut rgba = Vec::with_capacity(self.w * self.h * 4);
        // PNG rows run top down; the map's +z (north) is up.
        for y in (0..self.h).rev() {
            for x in 0..self.w {
                let [r, g, b] = self.px[y * self.w + x];
                rgba.extend_from_slice(&[r, g, b, 255]);
            }
        }
        let size = Extent3d { width: self.w as u32, height: self.h as u32, depth_or_array_layers: 1 };
        let image = Image::new(size, TextureDimension::D2, rgba, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
        let dynamic = image.try_into_dynamic().map_err(|e| e.to_string())?;
        dynamic.save(path).map_err(|e| e.to_string())
    }
}

/// 3 x 5 glyphs, row-major from the top-left bit (bit 14).
fn glyph(c: char) -> u16 {
    match c {
        '0' => 0b111_101_101_101_111,
        '1' => 0b010_110_010_010_111,
        '2' => 0b111_001_111_100_111,
        '3' => 0b111_001_111_001_111,
        '4' => 0b101_101_111_001_001,
        '5' => 0b111_100_111_001_111,
        '6' => 0b111_100_111_101_111,
        '7' => 0b111_001_010_010_010,
        '8' => 0b111_101_111_101_111,
        '9' => 0b111_101_111_001_111,
        'A' => 0b010_101_111_101_101,
        'B' => 0b110_101_110_101_110,
        'C' => 0b011_100_100_100_011,
        'D' => 0b110_101_101_101_110,
        'E' => 0b111_100_110_100_111,
        'F' => 0b111_100_110_100_100,
        'G' => 0b011_100_101_101_011,
        'H' => 0b101_101_111_101_101,
        'I' => 0b111_010_010_010_111,
        'J' => 0b001_001_001_101_010,
        'K' => 0b101_101_110_101_101,
        'L' => 0b100_100_100_100_111,
        'M' => 0b101_111_111_101_101,
        'N' => 0b110_101_101_101_101,
        'O' => 0b010_101_101_101_010,
        'P' => 0b110_101_110_100_100,
        'Q' => 0b010_101_101_110_011,
        'R' => 0b110_101_110_101_101,
        'S' => 0b011_100_010_001_110,
        'T' => 0b111_010_010_010_010,
        'U' => 0b101_101_101_101_111,
        'V' => 0b101_101_101_101_010,
        'W' => 0b101_101_111_111_101,
        'X' => 0b101_101_010_101_101,
        'Y' => 0b101_101_010_010_010,
        'Z' => 0b111_001_010_100_111,
        '-' => 0b000_000_111_000_000,
        ':' => 0b000_010_000_010_000,
        '.' => 0b000_000_000_000_010,
        '/' => 0b001_001_010_100_100,
        '!' => 0b010_010_010_000_010,
        _ => 0,
    }
}

fn rgb(c: [f32; 3]) -> [u8; 3] {
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn shade(c: [u8; 3], k: f32) -> [u8; 3] {
    c.map(|v| (v as f32 * k).clamp(0.0, 255.0) as u8)
}

/// Draw `map` at `scale` pixels per metre.
fn render(data: &GameData, map: &GeneratedMap, scale: f32) -> Canvas {
    let side = (map.size * scale).round() as usize;
    let mut c = Canvas::new(side, side, [0, 0, 0]);
    let w = map.width as i32;
    let colors: Vec<[u8; 3]> = data.terrain_ids().map(|t| rgb(data.terrain_def(t).color)).collect();
    let mountain = data.terrain_id("mountain");
    let at = |x: i32, y: i32| map.terrain[(y.clamp(0, w - 1) * w + x.clamp(0, w - 1)) as usize];
    // Mountains: lighter the further into the range (peaks), darker at the
    // foot, so ranges, saddles and passes read.
    let depth = |x: i32, y: i32| -> f32 {
        let mut n = 0;
        for dy in -3..=3 {
            for dx in -3..=3 {
                n += (Some(at(x + dx, y + dy)) == mountain) as i32;
            }
        }
        n as f32 / 49.0
    };
    for py in 0..side {
        for px in 0..side {
            let (x, y) = (((px as f32 + 0.5) / scale) as i32, ((py as f32 + 0.5) / scale) as i32);
            let t = at(x, y);
            let mut col = colors.get(t.index()).copied().unwrap_or([255, 0, 255]);
            if Some(t) == mountain {
                col = shade(col, 0.7 + 0.55 * depth(x, y));
            }
            c.px[py * side + px] = col;
        }
    }
    let mut boxes = |obb: Obb, col: [u8; 3]| {
        let (lo, hi) = obb.aabb();
        let (x0, y0) = ((lo.x * scale).floor() as i32, (lo.y * scale).floor() as i32);
        let (x1, y1) = ((hi.x * scale).ceil() as i32, (hi.y * scale).ceil() as i32);
        let mut any = false;
        for py in y0..=y1 {
            for px in x0..=x1 {
                if obb.contains(Vec2::new(px as f32 + 0.5, py as f32 + 0.5) / scale) {
                    c.set(px, py, col);
                    any = true;
                }
            }
        }
        if !any {
            c.set((obb.center.x * scale) as i32, (obb.center.y * scale) as i32, col);
        }
    };
    for n in &map.nodes {
        let def = data.node(n.kind);
        boxes(Obb::from_size(n.pos, def.size, n.yaw), rgb(def.color));
    }
    for b in &map.buildings {
        boxes(Obb::from_size(b.pos, data.building(b.kind).size, b.yaw), [250, 250, 250]);
    }
    for r in &map.features.rivers {
        for f in &r.fords {
            let p = r.points[*f] * scale;
            for d in 0..(scale * 1.5).max(1.0) as i32 {
                c.set(p.x as i32 + d, p.y as i32, [255, 220, 40]);
                c.set(p.x as i32, p.y as i32 + d, [255, 220, 40]);
            }
        }
    }
    for s in &map.starts {
        let r = 6.0 * scale.max(0.5);
        for k in 0..64 {
            let a = k as f32 / 64.0 * std::f32::consts::TAU;
            let p = *s * scale + Vec2::new(a.cos(), a.sin()) * r;
            c.set(p.x as i32, p.y as i32, [255, 255, 255]);
        }
    }
    c
}

/// What a tile's label and the log say about a map.
fn describe(map: &GeneratedMap, seed: u64, players: usize) -> String {
    let o = map.features.options;
    format!("S{seed} P{players} {} W{} R{} {}", o.size.name(), o.water.level(), map.features.rivers.len(), o.site.id())
}

struct Args {
    seed: u64,
    players: usize,
    options: MapOptions,
    map_size: f32,
    out: String,
    scale: f32,
    crop: Option<(f32, f32, f32)>,
    sheet: bool,
    seeds: (u64, u64),
    vary: Option<String>,
    tile: usize,
    cols: Option<usize>,
}

fn parse() -> Result<Args, String> {
    let mut a = Args {
        seed: 1,
        players: 2,
        options: MapOptions::default(),
        map_size: 0.0,
        out: "map.png".to_string(),
        scale: 2.0,
        crop: None,
        sheet: false,
        seeds: (1, 1),
        vary: None,
        tile: 360,
        cols: None,
    };
    let mut seeds_given = false;
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--help" | "-h" => return Err(HELP.to_string()),
            "--seed" => a.seed = value()?.parse().map_err(|e| format!("--seed: {e}"))?,
            "--players" => a.players = value()?.parse().map_err(|e| format!("--players: {e}"))?,
            "--map" => a.options.size = MapSize::parse(&value()?).ok_or("--map: small|medium|large|huge")?,
            "--map-size" => a.map_size = value()?.parse().map_err(|e| format!("--map-size: {e}"))?,
            "--water" => a.options.water = WaterLevel::parse(&value()?).ok_or("--water: 0-4")?,
            "--rivers" => a.options.rivers = RiverCount::parse(&value()?).ok_or("--rivers: 1|2|random")?,
            "--site" => a.options.site = SiteChoice::parse(&value()?).ok_or("--site: crater|shattered|lake|scar|spire|random")?,
            "--out" => a.out = value()?,
            "--scale" => a.scale = value()?.parse().map_err(|e| format!("--scale: {e}"))?,
            "--crop" => {
                let v: Vec<f32> = value()?.split(',').map(|s| s.trim().parse::<f32>()).collect::<Result<_, _>>().map_err(|e| format!("--crop: {e}"))?;
                let [x, y, r] = v[..] else { return Err("--crop X,Y,R".to_string()) };
                a.crop = Some((x, y, r));
            }
            "--sheet" => a.sheet = true,
            "--seeds" => {
                let v = value()?;
                let (lo, hi) = v.split_once('-').unwrap_or((&v, &v));
                a.seeds = (lo.parse().map_err(|e| format!("--seeds: {e}"))?, hi.parse().map_err(|e| format!("--seeds: {e}"))?);
                seeds_given = true;
            }
            "--vary" => a.vary = Some(value()?),
            "--tile" => a.tile = value()?.parse().map_err(|e| format!("--tile: {e}"))?,
            "--cols" => a.cols = Some(value()?.parse().map_err(|e| format!("--cols: {e}"))?),
            other => return Err(format!("unknown flag {other}\n{HELP}")),
        }
    }
    if !seeds_given {
        a.seeds = (a.seed, a.seed);
    }
    Ok(a)
}

/// `n` AI players cycling through every faction.
fn setup(data: &GameData, players: usize, seed: u64, map_size: f32) -> MatchSetup {
    let players = (0..players)
        .map(|i| PlayerSetup {
            name: format!("P{i}"),
            faction: FactionId((i % data.factions.len().max(1)) as u8),
            team: i as u8,
            controller: Controller::Ai,
            color: None,
        })
        .collect();
    MatchSetup { seed, players, map_size, ..MatchSetup::default() }
}

fn generate_logged(data: &GameData, players: usize, seed: u64, map_size: f32, options: &MapOptions) -> GeneratedMap {
    let started = Instant::now();
    let map = generate_with(data, &setup(data, players, seed, map_size), options);
    println!(
        "{}: {} m, {} nodes, {} river(s) {:?}, mountains {:?}, carved {:?}, cut off {:?} ({:.0} ms)",
        describe(&map, seed, players),
        map.size,
        map.nodes.len(),
        map.features.rivers.len(),
        map.features.rivers.iter().map(|r| (r.points.len() as f32 * 0.75).round() as u32).collect::<Vec<_>>(),
        map.features.mountains.iter().map(|m| (m.spine.len().saturating_sub(1) as u32 * 2, m.half_widths.iter().copied().fold(0.0f32, f32::max).round() as u32, m.spine[0].round())).collect::<Vec<_>>(),
        map.carved,
        map.cut_off,
        started.elapsed().as_secs_f32() * 1000.0
    );
    map
}

fn main() {
    let a = match parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let data = test_data();
    if !a.sheet {
        let map = generate_logged(&data, a.players, a.seed, a.map_size, &a.options);
        let mut canvas = render(&data, &map, a.scale);
        if let Some((x, y, r)) = a.crop {
            canvas = canvas.crop(Vec2::new(x, y) * a.scale, r * a.scale);
        }
        if let Err(e) = canvas.save(&a.out) {
            eprintln!("saving {}: {e}", a.out);
            std::process::exit(1);
        }
        println!("wrote {}", a.out);
        return;
    }

    // One row of tiles per value of the varied option, one column per seed.
    let base = (a.options, a.players);
    let variants: Vec<(MapOptions, usize)> = match a.vary.as_deref() {
        None => vec![base],
        Some("site") => SiteKind::ALL.iter().map(|k| (MapOptions { site: SiteChoice::Kind(*k), ..base.0 }, base.1)).collect(),
        Some("water") => WaterLevel::ALL.iter().map(|w| (MapOptions { water: *w, ..base.0 }, base.1)).collect(),
        Some("rivers") => [RiverCount::One, RiverCount::Two].iter().map(|r| (MapOptions { rivers: *r, ..base.0 }, base.1)).collect(),
        Some("map") => MapSize::ALL.iter().map(|s| (MapOptions { size: *s, ..base.0 }, base.1)).collect(),
        Some("players") => [2, 3, 4, 6, 8].iter().map(|p| (base.0, *p)).collect(),
        Some(other) => {
            eprintln!("--vary: site|water|rivers|map|players, not {other}");
            std::process::exit(2);
        }
    };
    let seeds: Vec<u64> = (a.seeds.0..=a.seeds.1).collect();
    let cols = a.cols.unwrap_or(if a.vary.is_some() { seeds.len() } else { seeds.len().min(4) }).max(1);
    let jobs: Vec<(MapOptions, usize, u64)> = variants.iter().flat_map(|(o, p)| seeds.iter().map(move |s| (*o, *p, *s))).collect();
    let rows = jobs.len().div_ceil(cols);
    let label = 16;
    let (tw, th) = (a.tile, a.tile + label);
    let mut sheet = Canvas::new(cols * (tw + 4) + 4, rows * (th + 4) + 4, [30, 30, 34]);
    for (k, (options, players, seed)) in jobs.iter().enumerate() {
        let map = generate_logged(&data, *players, *seed, a.map_size, options);
        let tile = render(&data, &map, a.tile as f32 / map.size);
        let (col, row) = (k % cols, k / cols);
        // The sheet is saved upside down (north up), so the label goes
        // above the tile in canvas terms: at the tile's top edge.
        let (ox, oy) = (4 + col * (tw + 4), 4 + (rows - 1 - row) * (th + 4));
        sheet.blit(&tile, ox, oy);
        let mut text = Canvas::new(tw, label, [30, 30, 34]);
        let note = describe(&map, *seed, *players) + if map.cut_off.is_empty() { "" } else { " CUT!" };
        text.text(&note, 2, 3, 2, if map.carved.is_empty() { [230, 230, 230] } else { [255, 170, 60] });
        // Flip the label so it reads the right way up once saved.
        let mut flipped = Canvas::new(tw, label, [30, 30, 34]);
        for y in 0..label {
            for x in 0..tw {
                flipped.px[(label - 1 - y) * tw + x] = text.px[y * tw + x];
            }
        }
        sheet.blit(&flipped, ox, oy + a.tile);
    }
    if let Err(e) = sheet.save(&a.out) {
        eprintln!("saving {}: {e}", a.out);
        std::process::exit(1);
    }
    println!("wrote {} ({} maps)", a.out, jobs.len());
}
