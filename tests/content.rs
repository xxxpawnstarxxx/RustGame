//! Content loading, validation and extensibility through JSON alone.

mod common;

use common::*;
use medieval_sim::content::ContentRegistry;
use medieval_sim::core::SimError;
use std::path::Path;

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn every_category_has_shipped_definitions() {
    let config = test_config();
    let content = shipped_content(&config);
    for (category, count) in content.category_counts() {
        assert!(count > 0, "category '{category}' is empty");
    }
    assert_eq!(content.manifest().id, "base");
    assert!(content.warnings().is_empty(), "{:?}", content.warnings());
    assert_eq!(content.stamp().fingerprint.len(), 16);
}

#[test]
fn new_content_needs_only_json() {
    let config = test_config();
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&root().join("data"), dir.path());
    std::fs::write(
        dir.path().join("items").join("extra.json"),
        r#"[{ "id": "pewter_jug", "name": "Pewter jug", "category": "Goods", "weight_kg": 0.8, "base_value_pence": 20 }]"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("characters").join("extra.json"),
        r#"[{ "id": "tavern_keeper", "name": "Tavern keeper", "body": "human", "age_years": [25, 60],
             "starting_items": [{ "item": "pewter_jug", "quantity": 1 }] }]"#,
    )
    .unwrap();
    let content = ContentRegistry::load(dir.path(), &config).unwrap();
    assert!(content.items().contains("pewter_jug"));
    let keeper = content.characters().get("tavern_keeper").unwrap();
    assert!(
        (keeper.walking_speed_mps - 1.35).abs() < 1e-6,
        "defaults applied"
    );
    assert!(keeper.can_climb && !keeper.can_swim);
    let shipped = shipped_content(&config);
    assert_ne!(content.fingerprint(), shipped.fingerprint());
}

#[test]
fn broken_references_are_reported_with_their_file() {
    let config = test_config();
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&root().join("data"), dir.path());
    std::fs::write(
        dir.path().join("weapons").join("bad.json"),
        r#"[{ "id": "laser", "item": "laser_gun", "class": "Sword", "damage": [{ "kind": "Cut", "power": 1 }],
              "reach_m": 1, "hands": 3, "skill": "photonics" }]"#,
    )
    .unwrap();
    let Err(SimError::ContentValidation(problems)) = ContentRegistry::load(dir.path(), &config)
    else {
        panic!("expected validation failure");
    };
    let mine: Vec<_> = problems
        .iter()
        .filter(|p| p.contains("data/weapons/bad.json"))
        .collect();
    assert!(mine.iter().any(|p| p.contains("laser_gun")), "{problems:?}");
    assert!(mine.iter().any(|p| p.contains("photonics")), "{problems:?}");
    assert!(mine.iter().any(|p| p.contains("hands")), "{problems:?}");
}

#[test]
fn anachronistic_technology_is_rejected() {
    let config = test_config();
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&root().join("data"), dir.path());
    std::fs::write(
        dir.path().join("items").join("anachronism.json"),
        r#"[{ "id": "printed_bible", "name": "Printed bible", "category": "Goods", "weight_kg": 3,
              "base_value_pence": 500, "technology": "printing_press" }]"#,
    )
    .unwrap();
    let Err(SimError::ContentValidation(problems)) = ContentRegistry::load(dir.path(), &config)
    else {
        panic!("expected validation failure");
    };
    assert!(
        problems
            .iter()
            .any(|p| p.contains("printing_press") && p.contains("not available"))
    );
}

#[test]
fn duplicate_ids_across_files_are_rejected() {
    let config = test_config();
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&root().join("data"), dir.path());
    std::fs::write(
        dir.path().join("skills").join("dupe.json"),
        r#"[{ "id": "smithing", "name": "Smithing again", "category": "Craft" }]"#,
    )
    .unwrap();
    let Err(SimError::ContentValidation(problems)) = ContentRegistry::load(dir.path(), &config)
    else {
        panic!("expected validation failure");
    };
    assert!(
        problems
            .iter()
            .any(|p| p.contains("duplicate id") && p.contains("smithing"))
    );
}

#[test]
fn missing_categories_and_manifest_are_tolerated() {
    let config = test_config();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("activities")).unwrap();
    std::fs::write(
        dir.path().join("activities").join("a.json"),
        r#"[{ "id": "sleep", "name": "Sleep", "category": "Rest" }]"#,
    )
    .unwrap();
    let content = ContentRegistry::load(dir.path(), &config).unwrap();
    assert_eq!(content.manifest().id, "unversioned");
    assert!(content.warnings().iter().any(|w| w.contains("manifest")));
    assert!(content.warnings().iter().any(|w| w.contains("'items'")));
}
