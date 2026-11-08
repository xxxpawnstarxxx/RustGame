//! Save slots, exact resumption, backups, corruption recovery, validation
//! and migration.

mod common;

use common::*;
use medieval_sim::content::CharacterTemplateId;
use medieval_sim::core::time::TimeUnit;
use medieval_sim::entities::EntityKind;
use medieval_sim::persistence::{
    MigrationContext, SAVE_FORMAT_VERSION, SaveManager, SlotStatus, world_from_json, world_to_json,
};
use medieval_sim::simulation::{Command, GameState, ItemLocation};

/// A world with characters on the move, items, a building, settlements,
/// factions, timers and a day of weather behind it.
fn busy_game() -> GameState {
    let mut game = test_game();
    let tiles = walkable_tiles(&game);
    for (i, template) in ["villein_ploughman", "woodward", "yeoman_archer"]
        .into_iter()
        .enumerate()
    {
        game.submit(Command::SpawnCharacter {
            template: CharacterTemplateId::new(template),
            name: None,
            tile: tiles[i * 97 % tiles.len()],
        });
    }
    game.submit(Command::SpawnItem {
        item: "cheese".into_id(),
        quantity: 3,
        location: ItemLocation::Ground(tiles[5]),
    });
    game.run_ticks(5);
    let people: Vec<_> = game
        .world()
        .entities
        .iter()
        .filter(|(_, k)| *k == EntityKind::Character)
        .map(|(id, _)| id)
        .collect();
    for (n, id) in people.iter().enumerate() {
        game.submit(Command::MoveTo {
            entity: *id,
            goal: tiles[(n + 1) * tiles.len() / 4],
            gait: None,
        });
    }
    game.run_ticks(ticks(&game, 2, TimeUnit::Hour));
    game
}

trait IntoId {
    fn into_id(self) -> medieval_sim::content::ItemDefId;
}
impl IntoId for &str {
    fn into_id(self) -> medieval_sim::content::ItemDefId {
        medieval_sim::content::ItemDefId::new(self)
    }
}

fn manager(dir: &std::path::Path) -> SaveManager {
    SaveManager::new(dir, 3)
}

#[test]
fn saved_world_resumes_exactly() {
    let mut original = busy_game();
    let dir = tempfile::tempdir().unwrap();
    let saves = manager(dir.path());
    let meta = saves
        .save(
            "main",
            original.world(),
            original.content(),
            original.calendar(),
        )
        .unwrap();
    assert_eq!(meta.format_version, SAVE_FORMAT_VERSION);
    assert_eq!(meta.counts.characters, 3);
    assert!(meta.counts.settlements >= 1 && meta.counts.factions >= 1);
    assert_eq!(meta.content, original.content().stamp());

    let loaded = saves
        .load("main", original.content(), original.config())
        .unwrap();
    assert!(loaded.recovered_from.is_none() && loaded.migrated_from.is_none());
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    let mut restored = GameState::from_world(
        original.config().clone(),
        original.content().clone(),
        loaded.world,
    )
    .unwrap();
    assert_eq!(
        restored.world(),
        original.world(),
        "derived state is rebuilt identically"
    );

    let span = ticks(&original, 1, TimeUnit::Day);
    original.run_ticks(span);
    restored.run_ticks(span);
    assert_eq!(original.world(), restored.world());
    assert_eq!(
        world_to_json(original.world()).unwrap(),
        world_to_json(restored.world()).unwrap()
    );
}

#[test]
fn slots_are_listed_with_metadata_and_backups_rotate() {
    let mut game = test_game();
    let dir = tempfile::tempdir().unwrap();
    let saves = manager(dir.path());
    for _ in 0..5 {
        game.run_ticks(10);
        saves
            .save("rotating", game.world(), game.content(), game.calendar())
            .unwrap();
    }
    saves
        .save("other", game.world(), game.content(), game.calendar())
        .unwrap();
    let list = saves.list();
    assert_eq!(
        list.iter().map(|s| s.slot.as_str()).collect::<Vec<_>>(),
        vec!["other", "rotating"]
    );
    let rotating = &list[1];
    assert_eq!(rotating.status, SlotStatus::Ok);
    assert_eq!(
        rotating.backups, 3,
        "only the configured number of backups is kept"
    );
    assert_eq!(rotating.metadata.as_ref().unwrap().tick, 50);
    assert!(saves.exists("other"));
    saves.delete("other").unwrap();
    assert!(!saves.exists("other"));
    assert!(SaveManager::validate_slot("../escape").is_err());
    assert!(
        saves
            .save("bad name!", game.world(), game.content(), game.calendar())
            .is_err()
    );
}

#[test]
fn corrupted_saves_recover_from_the_newest_backup() {
    let mut game = test_game();
    let dir = tempfile::tempdir().unwrap();
    let saves = manager(dir.path());
    game.run_ticks(10);
    saves
        .save("slot", game.world(), game.content(), game.calendar())
        .unwrap();
    game.run_ticks(10);
    saves
        .save("slot", game.world(), game.content(), game.calendar())
        .unwrap();

    // Truncate the main world file: checksum no longer matches.
    let world_file = dir.path().join("slot").join("world.json");
    let bytes = std::fs::read(&world_file).unwrap();
    std::fs::write(&world_file, &bytes[..bytes.len() / 2]).unwrap();
    let loaded = saves.load("slot", game.content(), game.config()).unwrap();
    assert!(loaded.recovered_from.is_some());
    assert_eq!(loaded.world.clock.tick(), 10, "previous save restored");
    assert!(loaded.warnings.iter().any(|w| w.contains("checksum")));

    // Corrupt every copy: loading fails with an explanation.
    for entry in walk(&dir.path().join("slot")) {
        if entry.extension().is_some_and(|e| e == "json")
            && entry
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("world")
        {
            std::fs::write(&entry, b"{ not json").unwrap();
        }
    }
    let err = saves
        .load("slot", game.content(), game.config())
        .unwrap_err()
        .to_string();
    assert!(err.contains("no usable save"), "{err}");
    assert!(
        saves
            .load("missing", game.content(), game.config())
            .is_err()
    );
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

#[test]
fn missing_metadata_loads_with_a_warning() {
    let game = test_game();
    let dir = tempfile::tempdir().unwrap();
    let saves = manager(dir.path());
    saves
        .save("slot", game.world(), game.content(), game.calendar())
        .unwrap();
    std::fs::remove_file(dir.path().join("slot").join("meta.json")).unwrap();
    assert_eq!(saves.list()[0].status, SlotStatus::MissingMetadata);
    let loaded = saves.load("slot", game.content(), game.config()).unwrap();
    assert!(
        loaded
            .warnings
            .iter()
            .any(|w| w.contains("metadata missing"))
    );
}

#[test]
fn inconsistent_worlds_fail_validation() {
    let game = test_game();
    let json = world_to_json(game.world()).unwrap();
    let ctx = MigrationContext {
        default_preset: TEST_PRESET.into(),
        default_seed: 1,
    };
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    // A settlement component on an entity that does not exist.
    let settlements = value["world"]["components"]["settlements"]
        .as_array_mut()
        .unwrap();
    let mut ghost = settlements[0].clone();
    ghost[0]["index"] = serde_json::json!(9999);
    settlements.push(ghost);
    let tampered = serde_json::to_string(&value).unwrap();
    let (world, _) = world_from_json(&tampered, &ctx).unwrap();
    let problems = medieval_sim::persistence::validate_world(&world, game.content());
    assert!(
        problems.iter().any(|p| p.contains("missing entity")),
        "{problems:?}"
    );

    let newer = json.replacen(
        &format!("\"format_version\":{SAVE_FORMAT_VERSION}"),
        "\"format_version\":999",
        1,
    );
    assert!(world_from_json(&newer, &ctx).is_err());
}

#[test]
fn version_one_saves_are_migrated() {
    let game = test_game();
    let json = world_to_json(game.world()).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    // Reduce to the Phase 1 layout.
    value["format_version"] = serde_json::json!(1);
    let world = value["world"].as_object_mut().unwrap();
    for key in [
        "meta",
        "map_info",
        "structures",
        "environment",
        "navigation",
    ] {
        world.remove(key);
    }
    let components = world["components"].as_object_mut().unwrap();
    components.retain(|k, _| k == "names" || k == "routines");
    // Phase 1 worlds had no positioned entities or factions.
    let v1 = serde_json::to_string(&value).unwrap();
    let ctx = MigrationContext {
        default_preset: TEST_PRESET.into(),
        default_seed: 1340,
    };
    let (migrated, from) = world_from_json(&v1, &ctx).unwrap();
    assert_eq!(from, Some(1));
    assert!(!migrated.environment.initialized);
    let restored =
        GameState::from_world(game.config().clone(), game.content().clone(), migrated).unwrap();
    assert!(
        restored.world().environment.initialized,
        "environment initialised on restore"
    );
    assert!(
        !restored.world().derived.map.is_empty(),
        "map regenerated from the default preset"
    );
}

#[test]
fn a_changed_generator_is_detected_on_load() {
    let game = test_game();
    let json = world_to_json(game.world()).unwrap();
    let ctx = MigrationContext {
        default_preset: TEST_PRESET.into(),
        default_seed: 1,
    };
    let (mut world, _) = world_from_json(&json, &ctx).unwrap();
    world.map_info.fingerprint = "0000000000000000".into();
    let err = GameState::from_world(game.config().clone(), game.content().clone(), world)
        .err()
        .unwrap();
    assert!(err.to_string().contains("fingerprint"));
}
