//! Helpers shared by the integration tests.
#![allow(dead_code)]

use medieval_sim::content::{ContentRegistry, WorldPresetId};
use medieval_sim::core::config::{ConfigurationManager, GameConfig};
use medieval_sim::core::ids::EntityId;
use medieval_sim::core::time::TimeUnit;
use medieval_sim::entities::EntityKind;
use medieval_sim::simulation::{EventKind, GameState, NewWorldOptions};
use medieval_sim::world::TilePos;
use std::path::Path;

pub const TEST_PRESET: &str = "test_small";
pub const FIXED_CREATION_TIME: u64 = 1_700_000_000;

pub fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// The shipped configuration, pointed at the small test map.
pub fn test_config() -> GameConfig {
    let mut config = ConfigurationManager::load(&root().join("config"))
        .expect("shipped config is valid")
        .config()
        .clone();
    config.simulation.default_world_preset = TEST_PRESET.into();
    config
}

pub fn shipped_content(config: &GameConfig) -> ContentRegistry {
    ContentRegistry::load(&root().join("data"), config)
        .unwrap_or_else(|e| panic!("shipped content is invalid: {e}"))
}

pub fn options(seed: u64) -> NewWorldOptions {
    NewWorldOptions {
        name: "Test world".into(),
        seed,
        preset: WorldPresetId::new(TEST_PRESET),
        created_unix: FIXED_CREATION_TIME,
    }
}

pub fn game_with(config: GameConfig, seed: u64) -> GameState {
    let content = shipped_content(&config);
    GameState::new_world(config, content, &options(seed)).expect("world is created")
}

/// A new test world.
pub fn test_game() -> GameState {
    game_with(test_config(), 1340)
}

pub fn ticks(game: &GameState, amount: u64, unit: TimeUnit) -> u64 {
    game.world()
        .clock
        .ticks_for(game.calendar().duration(amount, unit))
}

pub fn count_events(game: &GameState, kind: EventKind) -> u64 {
    game.event_log().counts().get(&kind).copied().unwrap_or(0)
}

/// The newest entity of a kind.
pub fn newest(game: &GameState, kind: EntityKind) -> Option<EntityId> {
    game.world()
        .entities
        .iter()
        .filter(|(_, k)| *k == kind)
        .map(|(id, _)| id)
        .max_by_key(|id| (id.generation(), id.index()))
}

/// Walkable open lowland tiles (no structure, water or cliff), row-major.
pub fn walkable_tiles(game: &GameState) -> Vec<TilePos> {
    use medieval_sim::navigation::{Abilities, TraversalMode, traversal};
    let world = game.world();
    world
        .derived
        .map
        .positions()
        .filter(|p| {
            traversal(world, &game.config().movement, *p, Abilities::default())
                .is_some_and(|t| t.mode == TraversalMode::Walk)
        })
        .collect()
}

/// Runs until `done` returns true or `limit` ticks pass; returns ticks run.
pub fn run_until(
    game: &mut GameState,
    limit: u64,
    mut done: impl FnMut(&GameState) -> bool,
) -> u64 {
    for n in 0..limit {
        if done(game) {
            return n;
        }
        game.step_tick();
    }
    limit
}
