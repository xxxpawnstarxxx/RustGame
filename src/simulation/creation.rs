//! Creating new worlds and restoring loaded ones.

use crate::content::{ContentRegistry, FactionKind, WorldPresetId};
use crate::core::config::GameConfig;
use crate::core::error::{SimError, SimResult};
use crate::core::ids::EntityId;
use crate::core::time::Calendar;
use crate::entities::{EntityKind, Name};
use crate::environment::{EnvStatics, EnvironmentState};
use crate::simulation::events::{EventSource, SimEvent};
use crate::simulation::instances::{FactionRecord, Position, SettlementRecord};
use crate::simulation::world_state::{WorldMeta, WorldState};
use crate::world::{SettlementKind, generate};
use std::collections::BTreeMap;

/// Parameters of a new world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewWorldOptions {
    pub name: String,
    pub seed: u64,
    pub preset: WorldPresetId,
    /// Real-world creation time recorded in the world metadata (Unix seconds).
    pub created_unix: u64,
}

impl NewWorldOptions {
    /// The configured defaults.
    pub fn from_config(config: &GameConfig) -> Self {
        Self {
            name: config.simulation.default_world_name.clone(),
            seed: config.simulation.seed,
            preset: WorldPresetId::new(config.simulation.default_world_preset.clone()),
            created_unix: unix_now(),
        }
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Generates the map, initialises the environment and creates the factions
/// and settlements of a new world.
pub fn create_world(
    config: &GameConfig,
    calendar: &Calendar,
    content: &ContentRegistry,
    options: &NewWorldOptions,
) -> SimResult<WorldState> {
    let preset = content.worldgen().require(&options.preset)?;
    let generated = generate(preset, options.seed, content)?;
    let meta = WorldMeta {
        name: options.name.clone(),
        seed: options.seed,
        created_unix: options.created_unix,
    };
    let mut world = WorldState::new(config, calendar, meta, generated.info.clone())?;
    world.derived.statics = EnvStatics::build(&generated.map, &config.environment);
    world.navigation.chunk_epochs = vec![0; generated.map.chunk_count()];
    let now = world.clock.now();
    world.environment = EnvironmentState::initialize(
        &generated.map,
        &world.derived.statics,
        preset,
        content,
        config,
        calendar,
        options.seed,
        now,
    );
    world.derived.map = generated.map;

    let tick = world.clock.tick();
    let push = |world: &mut WorldState, event: SimEvent| {
        world.events.push(tick, now, EventSource::Command, event);
    };

    // Factions, in id order; parents and relations once all exist.
    let mut factions: BTreeMap<String, EntityId> = BTreeMap::new();
    for def in content
        .factions()
        .iter()
        .filter(|f| f.spawn_at_world_creation)
    {
        let entity = world.entities.create(EntityKind::Faction);
        world
            .components
            .names
            .insert(entity, Name::new(def.name.clone()));
        world.components.factions.insert(
            entity,
            FactionRecord {
                def: def.id.clone(),
                name: def.name.clone(),
                kind: def.kind,
                parent: None,
                relations: Vec::new(),
            },
        );
        factions.insert(def.id.to_string(), entity);
        push(
            &mut world,
            SimEvent::EntityCreated {
                entity,
                kind: EntityKind::Faction,
            },
        );
    }
    for def in content
        .factions()
        .iter()
        .filter(|f| f.spawn_at_world_creation)
    {
        let entity = factions[def.id.as_str()];
        let parent = def
            .parent
            .as_ref()
            .and_then(|p| factions.get(p.as_str()).copied());
        let relations: Vec<(EntityId, i32)> = def
            .relations
            .iter()
            .filter_map(|r| factions.get(r.faction.as_str()).map(|e| (*e, r.value)))
            .collect();
        if let Some(record) = world.components.factions.get_mut(entity) {
            record.parent = parent;
            for (other, value) in relations {
                record.set_relation(other, value);
            }
        }
    }

    // Settlements, held by a fitting faction.
    let of_kind = |kind: FactionKind| -> Vec<EntityId> {
        content
            .factions()
            .iter()
            .filter(|f| f.kind == kind)
            .filter_map(|f| factions.get(f.id.as_str()).copied())
            .collect()
    };
    let towns = of_kind(FactionKind::Town);
    let manors = of_kind(FactionKind::Manor);
    let mut next_manor = 0usize;
    for site in &generated.sites {
        let faction = match site.kind {
            SettlementKind::Town => towns.first().copied().or_else(|| manors.first().copied()),
            _ if manors.is_empty() => None,
            _ => {
                let f = manors[next_manor % manors.len()];
                next_manor += 1;
                Some(f)
            }
        };
        let entity = world.entities.create(EntityKind::Settlement);
        world
            .components
            .names
            .insert(entity, Name::new(site.name.clone()));
        world
            .components
            .positions
            .insert(entity, Position { tile: site.centre });
        world.components.settlements.insert(
            entity,
            SettlementRecord {
                name: site.name.clone(),
                kind: site.kind,
                centre: site.centre,
                founded: now,
                faction,
                generated: true,
            },
        );
        push(
            &mut world,
            SimEvent::EntityCreated {
                entity,
                kind: EntityKind::Settlement,
            },
        );
        push(
            &mut world,
            SimEvent::SettlementFounded {
                settlement: entity,
                founder: faction,
            },
        );
    }
    Ok(world)
}

/// Rebuilds everything a loaded world does not store: regenerates the static
/// map from its seed and preset (verifying the fingerprint), rebuilds the
/// environment aggregates and occupancy, and initialises the environment of
/// worlds migrated from saves that predate it.
pub fn restore_world(
    world: &mut WorldState,
    config: &GameConfig,
    calendar: &Calendar,
    content: &ContentRegistry,
) -> SimResult<()> {
    let preset = content.worldgen().require(&world.map_info.preset)?;
    let generated = generate(preset, world.map_info.seed, content)?;
    if !world.map_info.fingerprint.is_empty()
        && world.map_info.fingerprint != generated.info.fingerprint
    {
        return Err(SimError::Persistence(format!(
            "the regenerated map does not match the saved world (fingerprint {} vs {}); \
             the world generator or its content has changed",
            generated.info.fingerprint, world.map_info.fingerprint
        )));
    }
    world.map_info = generated.info;
    world.derived.statics = EnvStatics::build(&generated.map, &config.environment);
    if world.navigation.chunk_epochs.len() != generated.map.chunk_count() {
        world
            .navigation
            .chunk_epochs
            .resize(generated.map.chunk_count(), 0);
    }
    if !world.environment.initialized {
        world.environment = EnvironmentState::initialize(
            &generated.map,
            &world.derived.statics,
            preset,
            content,
            config,
            calendar,
            world.map_info.seed,
            world.clock.now(),
        );
    } else if world.environment.ground.len() != world.derived.statics.ground.len()
        || world.environment.weather.cells.len() != world.derived.statics.weather.len()
        || world.environment.wildlife.regions.len() != world.derived.statics.wildlife.len()
    {
        return Err(SimError::Persistence(
            "environment grid sizes do not match the configured cell sizes".into(),
        ));
    }
    world.derived.map = generated.map;
    world.rebuild_occupancy();
    Ok(())
}
