//! Validate content and print a roster summary.
//!
//!   cargo run -p af_data --example check                         # all of assets/data
//!   cargo run -p af_data --example check -- --only factions/ironhold.ron
//!        (core/** plus only the listed files; repeatable)
//!   cargo run -p af_data --example check -- --extra <dir>        # also load every .ron in <dir>
//!   add --summary to print the per-faction roster table
use std::path::PathBuf;

use af_data::GameData;

fn main() {
    let mut only: Vec<String> = Vec::new();
    let mut extra: Vec<PathBuf> = Vec::new();
    let mut summary = false;
    let mut dir: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--only" => only.push(args.next().expect("--only <file>").replace('\\', "/")),
            "--extra" => extra.push(PathBuf::from(args.next().expect("--extra <dir>"))),
            "--summary" => summary = true,
            "--dir" => dir = Some(PathBuf::from(args.next().expect("--dir <dir>"))),
            other => panic!("unknown argument {other}"),
        }
    }
    let dir = dir.unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/data"));
    let mut packs = match GameData::read_packs(&dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    if !only.is_empty() {
        packs.retain(|(src, _)| src.starts_with("core/") || only.iter().any(|o| o == src));
    }
    for d in &extra {
        match GameData::read_packs(d) {
            Ok(p) => packs.extend(p.into_iter().map(|(s, p)| (format!("extra/{s}"), p))),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
    }
    let files: Vec<String> = packs.iter().map(|(s, _)| s.clone()).collect();
    match GameData::from_packs(packs) {
        Ok(d) => {
            println!("OK: {} files: {}", files.len(), files.join(", "));
            if summary {
                println!("{}", d.summary());
            } else {
                println!("{}", d.summary().lines().next().unwrap_or(""));
            }
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
