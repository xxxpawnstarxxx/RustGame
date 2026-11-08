//! What it costs (and whether it is possible) to cross a tile on foot.
//!
//! Costs are dimensionless multipliers of the time needed to walk the same
//! distance over dry, open lowland. They combine terrain, roads, crossings,
//! structures, woodland, slope and current ground conditions (mud, snow,
//! flood), and depend on what the mover can do (swim, climb).

use crate::content::TerrainKind;
use crate::core::config::MovementConfig;
use crate::environment::EnvironmentQuery;
use crate::simulation::WorldState;
use crate::world::{Crossing, RoadKind, StructureKind, TilePos};
use serde::{Deserialize, Serialize};

/// How a tile is crossed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TraversalMode {
    Walk,
    Wade,
    Swim,
    Climb,
}

/// Abilities that open up otherwise impassable tiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Abilities {
    pub swim: bool,
    pub climb: bool,
}

/// Result of evaluating a tile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Traversal {
    pub cost: f32,
    pub mode: TraversalMode,
    /// Seconds spent opening a closed door or gate.
    pub delay_s: f32,
    /// Roughness for stumbling, 0 (smooth) – 2 (very rough).
    pub roughness: f32,
}

/// Flood water deeper than this cannot be waded.
const MAX_WADE_FLOOD_M: f32 = 1.2;

/// Evaluates crossing `pos`, or `None` if it is impassable for these abilities.
pub fn traversal(
    world: &WorldState,
    config: &MovementConfig,
    pos: TilePos,
    abilities: Abilities,
) -> Option<Traversal> {
    let tile = world.derived.map.tile(pos)?;
    let tc = &config.terrain_cost;
    let env = EnvironmentQuery::new(world);
    let mut delay = 0.0;
    if let Some(structure) = world.structures.get(pos) {
        if structure.kind.blocks() {
            return None;
        }
        delay = structure
            .kind
            .opening_delay(config.door_open_seconds, config.gate_open_seconds);
        if matches!(
            structure.kind,
            StructureKind::Floor | StructureKind::Door(_) | StructureKind::Gate(_)
        ) {
            // Built floors, thresholds and gateways are firm, level ground.
            return Some(Traversal {
                cost: tc.lowland,
                mode: TraversalMode::Walk,
                delay_s: delay,
                roughness: 0.1,
            });
        }
    }

    let (mut cost, mode, mut rough) = match tile.terrain {
        TerrainKind::Ocean | TerrainKind::Lake | TerrainKind::River => match tile.crossing {
            Crossing::Bridge => (
                tc.lowland * config.bridge_multiplier,
                TraversalMode::Walk,
                0.1,
            ),
            Crossing::Ford => (config.ford_cost, TraversalMode::Wade, 1.0),
            Crossing::None if abilities.swim => (tc.swim, TraversalMode::Swim, 0.0),
            Crossing::None => return None,
        },
        TerrainKind::Shallows => (tc.shallows, TraversalMode::Wade, 0.9),
        TerrainKind::Cliff if abilities.climb => (tc.cliff, TraversalMode::Climb, 0.0),
        TerrainKind::Cliff => return None,
        TerrainKind::Marsh => (tc.marsh, TraversalMode::Walk, 0.9),
        TerrainKind::Beach => (tc.beach, TraversalMode::Walk, 0.4),
        TerrainKind::Mountain => (tc.mountain, TraversalMode::Walk, 1.1),
        TerrainKind::Hills => (tc.hills, TraversalMode::Walk, 0.6),
        TerrainKind::Lowland => (tc.lowland, TraversalMode::Walk, 0.3),
    };

    if mode == TraversalMode::Walk || mode == TraversalMode::Wade {
        let flood = env.flood_depth(pos);
        if flood > MAX_WADE_FLOOD_M && !abilities.swim {
            return None;
        }
        let on_road = tile.road != RoadKind::None;
        if !on_road {
            let trees = env.tile_tree_density(pos);
            cost += config.forest_cost * trees;
            rough += 0.5 * trees;
        }
        if mode == TraversalMode::Walk && tile.crossing != Crossing::Bridge {
            cost += config.slope_cost_per_degree * tile.slope_deg;
        }
        let penalty = env.movement_penalty(pos, config);
        cost += penalty;
        rough += 0.5 * penalty.min(2.0);
        cost *= match tile.road {
            RoadKind::Road => config.road_multiplier,
            RoadKind::Track => config.track_multiplier,
            RoadKind::None => 1.0,
        };
    }
    Some(Traversal {
        cost,
        mode,
        delay_s: delay,
        roughness: rough.min(2.0),
    })
}

/// Lowest cost any step can have; keeps the A* heuristic admissible.
pub fn minimum_cost(config: &MovementConfig) -> f32 {
    let t = &config.terrain_cost;
    let cheapest = [
        t.lowland,
        t.hills,
        t.mountain,
        t.beach,
        t.marsh,
        t.shallows,
        t.cliff,
        t.swim,
        config.ford_cost,
    ]
    .into_iter()
    .fold(f32::MAX, f32::min);
    let multiplier = config
        .road_multiplier
        .min(config.bridge_multiplier)
        .min(1.0);
    (cheapest.min(t.lowland * config.bridge_multiplier) * multiplier).max(0.05)
}

/// Height lost stepping from `from` to `to` (positive when descending).
pub fn drop_m(world: &WorldState, from: TilePos, to: TilePos) -> f32 {
    let map = &world.derived.map;
    match (map.tile(from), map.tile(to)) {
        (Some(a), Some(b)) => a.elevation_m.max(0.0) - b.elevation_m.max(0.0),
        _ => 0.0,
    }
}
