//! Save format versions and migrations.
//!
//! Migrations operate on the untyped JSON of the world file so that old
//! layouts can be upgraded before typed deserialisation. Each migration
//! upgrades exactly one version; they are applied in sequence.
//!
//! | Version | Change |
//! |---------|--------|
//! | 1 | Phase 1: clock, RNG, entities, names, routines, timers, queues, systems, ambient |
//! | 2 | world metadata, map info, structures, environment, navigation epochs, and components for positions, movers, characters, items, settlements, factions and buildings |

use crate::core::error::{SimError, SimResult};
use crate::environment::EnvironmentState;
use crate::navigation::NavigationState;
use crate::world::StructureLayer;
use serde_json::{Value, json};

/// Current save format version.
pub const SAVE_FORMAT_VERSION: u32 = 2;

/// Defaults used to fill in information older saves did not record.
#[derive(Debug, Clone)]
pub struct MigrationContext {
    pub default_preset: String,
    pub default_seed: u64,
}

fn object<'a>(
    value: &'a mut Value,
    what: &str,
) -> SimResult<&'a mut serde_json::Map<String, Value>> {
    value
        .as_object_mut()
        .ok_or_else(|| SimError::Persistence(format!("{what} is not a JSON object")))
}

fn to_value<T: serde::Serialize>(value: &T) -> SimResult<Value> {
    serde_json::to_value(value).map_err(|e| SimError::Persistence(e.to_string()))
}

/// Version 1 → 2.
fn v1_to_v2(world: &mut Value, ctx: &MigrationContext) -> SimResult<()> {
    let seed = ctx.default_seed;
    let w = object(world, "world")?;
    w.entry("meta").or_insert(json!({
        "name": "Migrated world",
        "seed": seed,
        "created_unix": 0
    }));
    w.entry("map_info").or_insert(json!({
        "preset": ctx.default_preset,
        "seed": seed,
        "width": 0,
        "height": 0,
        "fingerprint": ""
    }));
    w.entry("structures")
        .or_insert(to_value(&StructureLayer::default())?);
    w.entry("environment")
        .or_insert(to_value(&EnvironmentState::default())?);
    w.entry("navigation")
        .or_insert(to_value(&NavigationState::default())?);
    let components = w.entry("components").or_insert(json!({}));
    let components = object(components, "world.components")?;
    for store in [
        "positions",
        "movers",
        "characters",
        "items",
        "settlements",
        "factions",
        "buildings",
    ] {
        components.entry(store).or_insert(json!([]));
    }
    Ok(())
}

type Migration = fn(&mut Value, &MigrationContext) -> SimResult<()>;

/// `MIGRATIONS[i]` upgrades version `i + 1` to `i + 2`.
const MIGRATIONS: [Migration; (SAVE_FORMAT_VERSION - 1) as usize] = [v1_to_v2];

/// Upgrades a world value from `version` to [`SAVE_FORMAT_VERSION`].
pub fn migrate(world: &mut Value, version: u32, ctx: &MigrationContext) -> SimResult<()> {
    if version == 0 {
        return Err(SimError::Persistence(
            "save format version 0 does not exist".into(),
        ));
    }
    if version > SAVE_FORMAT_VERSION {
        return Err(SimError::Persistence(format!(
            "save format {version} was written by a newer version of the game (this build reads up to {SAVE_FORMAT_VERSION})"
        )));
    }
    for v in version..SAVE_FORMAT_VERSION {
        MIGRATIONS[(v - 1) as usize](world, ctx)?;
        tracing::info!(from = v, to = v + 1, "migrated save");
    }
    Ok(())
}
