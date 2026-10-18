//! Validates the real game content in `assets/data/`.
//! Run `cargo test -p af_data --test content -- --nocapture` to print a roster summary.

use std::path::PathBuf;

use af_data::*;

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/data")
}

#[test]
fn game_content_validates() {
    let d = GameData::load_dir(&data_dir()).unwrap_or_else(|e| panic!("{e}"));
    println!("{}", d.summary());
    assert_eq!(d.factions.len(), 6, "the game ships six factions");
}
