use af_data::*;

fn minimal() -> GameData {
    let text = include_str!("fixtures/minimal.ron");
    let pack = GameData::parse_pack("minimal.ron", text).unwrap_or_else(|e| panic!("{e}"));
    GameData::from_packs(vec![("minimal.ron".into(), pack)]).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn minimal_pack_loads() {
    let d = minimal();
    assert_eq!(d.ages.len(), 5);
    let peasant = d.unit_id("peasant").unwrap();
    let hall = d.building_id("hall").unwrap();
    let f = d.faction_id("testers").unwrap();
    assert!(d.trainable_at(hall, f).contains(&peasant));
    let food = d.resource_id("food").unwrap();
    assert_eq!(d.unit(peasant).cost.amounts.get(food), 50.0);
    // Faction gather bonus applies on top of the unit rate.
    let wood = d.resource_id("wood").unwrap();
    assert!((d.gather_rate(peasant, f, wood) - 0.72).abs() < 1e-5);
    assert_eq!(d.age_up_tech(f, 1), d.tech_id("to_banner"));
}

#[test]
fn mobility_and_terrain() {
    let d = minimal();
    let f = d.faction_id("testers").unwrap();
    let water = d.terrain_id("deep_water").unwrap();
    let grass = d.terrain_id("grass").unwrap();
    let peasant = d.unit_id("peasant").unwrap();
    let skiff = d.unit_id("skiff").unwrap();
    let m = d.unit_mobility(peasant, f);
    assert!(m.contains(MoveFlag::Foot) && m.contains(MoveFlag::Amphibious));
    assert!(d.terrain_passable(water, m), "amphibious foot crosses deep water");
    assert!(d.terrain_passable(grass, m));
    assert!((d.terrain_speed(water, m) - 0.7).abs() < 1e-6);
    let hm = d.unit_mobility(skiff, f);
    assert!((d.terrain_speed(water, hm) - 1.0).abs() < 1e-6);
}

#[test]
fn errors_are_reported() {
    let text = include_str!("fixtures/minimal.ron").replace("trained_at: [\"hall\"], squad: 3", "trained_at: [\"nowhere\"], squad: 3");
    let pack = GameData::parse_pack("broken.ron", &text).unwrap();
    let err = GameData::from_packs(vec![("broken.ron".into(), pack)]).unwrap_err();
    let s = err.to_string();
    assert!(s.contains("unknown building `nowhere`"), "{s}");
}

#[test]
fn duplicate_ids_are_reported() {
    let text = include_str!("fixtures/minimal.ron");
    let a = GameData::parse_pack("a.ron", text).unwrap();
    let b = GameData::parse_pack("b.ron", text).unwrap();
    let err = GameData::from_packs(vec![("a.ron".into(), a), ("b.ron".into(), b)]).unwrap_err();
    assert!(err.to_string().contains("duplicate unit id `peasant`"), "{err}");
}
