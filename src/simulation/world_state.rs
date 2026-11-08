//! [`WorldState`]: the complete persistent state of a simulated world.

use crate::core::config::GameConfig;
use crate::core::error::SimResult;
use crate::core::ids::{EntityId, SystemId};
use crate::core::rng::SimRng;
use crate::core::time::Calendar;
use crate::entities::{ComponentStore, EntityManager, Name};
use crate::environment::{EnvStatics, EnvironmentState};
use crate::navigation::{Mover, NavigationState, Occupancy};
use crate::simulation::commands::CommandQueue;
use crate::simulation::events::EventQueue;
use crate::simulation::instances::{
    BuildingRecord, CharacterRecord, FactionRecord, ItemInstance, Position, SettlementRecord,
};
use crate::simulation::system::SystemRunState;
use crate::simulation::time::{AmbientState, RoutineAssignment, SimClock, TimerQueue};
use crate::world::{MapInfo, StructureLayer, TilePos, WorldMap};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One store per component type. Every component type gets a field here
/// and a line in [`Components::remove_all`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Components {
    pub names: ComponentStore<Name>,
    pub routines: ComponentStore<RoutineAssignment>,
    pub positions: ComponentStore<Position>,
    pub movers: ComponentStore<Mover>,
    pub characters: ComponentStore<CharacterRecord>,
    pub items: ComponentStore<ItemInstance>,
    pub settlements: ComponentStore<SettlementRecord>,
    pub factions: ComponentStore<FactionRecord>,
    pub buildings: ComponentStore<BuildingRecord>,
}

impl Components {
    /// Removes every component of a destroyed entity.
    pub fn remove_all(&mut self, entity: EntityId) {
        self.names.remove(entity);
        self.routines.remove(entity);
        self.positions.remove(entity);
        self.movers.remove(entity);
        self.characters.remove(entity);
        self.items.remove(entity);
        self.settlements.remove(entity);
        self.factions.remove(entity);
        self.buildings.remove(entity);
    }
}

/// Descriptive metadata of a world.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldMeta {
    pub name: String,
    pub seed: u64,
    /// Real-world creation time (Unix seconds); never read by the simulation.
    pub created_unix: u64,
}

/// State derived deterministically from persistent state and content;
/// rebuilt after loading and never saved.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DerivedWorld {
    /// The static map, regenerated from [`WorldState::map_info`].
    pub map: WorldMap,
    pub statics: EnvStatics,
    pub occupancy: Occupancy,
}

/// Everything that must be saved to reproduce a world exactly.
///
/// Serialising this struct *is* saving the game: clock, RNG position,
/// entities and components, structures, environment, navigation epochs,
/// pending timers, events and commands, and each system's scheduling state.
/// Nothing outside it may influence simulation outcomes. The static map is
/// not stored; it is regenerated from `map_info` and verified by fingerprint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldState {
    pub meta: WorldMeta,
    pub map_info: MapInfo,
    pub clock: SimClock,
    pub rng: SimRng,
    pub entities: EntityManager,
    pub components: Components,
    pub structures: StructureLayer,
    pub environment: EnvironmentState,
    pub navigation: NavigationState,
    pub timers: TimerQueue,
    pub events: EventQueue,
    pub commands: CommandQueue,
    pub systems: BTreeMap<SystemId, SystemRunState>,
    pub ambient: AmbientState,
    #[serde(skip)]
    pub derived: DerivedWorld,
}

impl WorldState {
    /// An empty world (no map yet) at the configured start date.
    pub fn new(
        config: &GameConfig,
        calendar: &Calendar,
        meta: WorldMeta,
        map_info: MapInfo,
    ) -> SimResult<Self> {
        let start = calendar.start_instant()?;
        Ok(Self {
            clock: SimClock::new(start, config.simulation.game_seconds_per_tick),
            rng: SimRng::from_seed(meta.seed),
            meta,
            map_info,
            entities: EntityManager::new(),
            components: Components::default(),
            structures: StructureLayer::default(),
            environment: EnvironmentState::default(),
            navigation: NavigationState::default(),
            timers: TimerQueue::new(),
            events: EventQueue::new(),
            commands: CommandQueue::new(),
            systems: BTreeMap::new(),
            ambient: AmbientState::compute(calendar, start),
            derived: DerivedWorld::default(),
        })
    }

    /// Display name of an entity, if it has one.
    pub fn name_of(&self, entity: EntityId) -> Option<&str> {
        self.components.names.get(entity).map(Name::as_str)
    }

    /// Rebuilds occupancy from movers' positions and current steps.
    pub fn rebuild_occupancy(&mut self) {
        let mut occupancy = Occupancy::default();
        for (id, mover) in self.components.movers.iter() {
            if let Some(pos) = self.components.positions.get(id) {
                occupancy.add(pos.tile, id);
            }
            if let Some(step) = &mover.step {
                occupancy.add(step.to, id);
            }
        }
        self.derived.occupancy = occupancy;
    }

    /// Records a navigation-relevant change at the given tiles.
    pub fn invalidate_tiles(&mut self, tiles: &[TilePos]) {
        if tiles.is_empty() {
            return;
        }
        self.navigation.epoch += 1;
        let epoch = self.navigation.epoch;
        let map = &self.derived.map;
        if self.navigation.chunk_epochs.len() != map.chunk_count() {
            self.navigation.chunk_epochs.resize(map.chunk_count(), 0);
        }
        for pos in tiles {
            if let Some(c) = map.chunk_index(map.chunk_of(*pos)) {
                self.navigation.chunk_epochs[c] = epoch;
            }
        }
    }

    /// Records a navigation-relevant change across whole weather regions
    /// (floods rising or falling).
    pub fn invalidate_weather_regions(&mut self, regions: &[u32]) {
        let statics = &self.derived.statics;
        let wc = statics.weather_cell_tiles as i32;
        let mut tiles = Vec::new();
        for &r in regions {
            if statics.weather_cols == 0 {
                break;
            }
            let x0 = (r % statics.weather_cols) as i32 * wc;
            let y0 = (r / statics.weather_cols) as i32 * wc;
            let cs = self.derived.map.chunk_size().max(1) as i32;
            for y in (y0..y0 + wc).step_by(cs as usize) {
                for x in (x0..x0 + wc).step_by(cs as usize) {
                    tiles.push(TilePos::new(x, y));
                }
            }
        }
        self.invalidate_tiles(&tiles);
    }
}
