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

/// Each core resource is gathered the way `docs/art/peasant.md` animates it.
#[test]
fn core_resources_declare_their_work_style() {
    let d = GameData::load_dir(&data_dir()).unwrap_or_else(|e| panic!("{e}"));
    let expected = [
        ("food", WorkStyle::Forage),
        ("wood", WorkStyle::Chop),
        ("stone", WorkStyle::Mine),
        ("copper", WorkStyle::Mine),
        ("iron", WorkStyle::Mine),
        ("silicon", WorkStyle::Dig),
        ("aether", WorkStyle::Chip),
        ("adamant", WorkStyle::Smash),
    ];
    for (id, style) in expected {
        let r = d.resource_id(id).unwrap_or_else(|| panic!("resource {id}"));
        assert_eq!(d.resource(r).work, style, "{id}");
    }
    assert_eq!(d.resources.len(), expected.len(), "every resource is listed here");
}
