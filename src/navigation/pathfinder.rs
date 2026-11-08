//! Grid A* path planning.
//!
//! Eight-connected search over [`traversal`] costs. Diagonal steps may not
//! cut the corner of an impassable tile. Tiles near the start that other
//! characters currently occupy are penalised so that crowds route around
//! each other instead of queuing. The search is bounded by
//! `max_path_expansions`.

use super::occupancy::Occupancy;
use super::traversal::{Abilities, minimum_cost, traversal};
use crate::core::config::MovementConfig;
use crate::core::ids::EntityId;
use crate::simulation::WorldState;
use crate::world::TilePos;
use pathfinding::prelude::astar;
use std::cell::Cell;

/// Radius around the start within which occupancy is considered.
const CROWD_RADIUS: i32 = 10;

/// Outcome of a path search.
#[derive(Debug, Clone, PartialEq)]
pub enum PathResult {
    /// Tiles to visit after the start, ending at the goal.
    Found {
        path: Vec<TilePos>,
        cost: u32,
    },
    Unreachable,
    /// The expansion budget ran out.
    TooFar,
}

/// Plans a route for `mover` from `start` to `goal`.
pub fn find_path(
    world: &WorldState,
    config: &MovementConfig,
    occupancy: &Occupancy,
    mover: EntityId,
    abilities: Abilities,
    start: TilePos,
    goal: TilePos,
) -> PathResult {
    if start == goal {
        return PathResult::Found {
            path: Vec::new(),
            cost: 0,
        };
    }
    if traversal(world, config, goal, abilities).is_none() {
        return PathResult::Unreachable;
    }
    let tile_cm = world.derived.map.tile_size_m() * 100.0;
    let min_cost = minimum_cost(config);
    let expansions = Cell::new(0u32);
    let exhausted = Cell::new(false);
    let passable = |p: TilePos| traversal(world, config, p, abilities);

    let result = astar(
        &start,
        |p: &TilePos| {
            let n = expansions.get() + 1;
            expansions.set(n);
            if n > config.max_path_expansions {
                exhausted.set(true);
                return Vec::new();
            }
            let mut next = Vec::with_capacity(8);
            for q in p.neighbours8() {
                let Some(t) = passable(q) else { continue };
                let diagonal = p.is_diagonal_to(q);
                if diagonal
                    && (passable(TilePos::new(q.x, p.y)).is_none()
                        || passable(TilePos::new(p.x, q.y)).is_none())
                {
                    continue;
                }
                let distance = if diagonal {
                    std::f32::consts::SQRT_2
                } else {
                    1.0
                };
                let mut cost = t.cost * distance * tile_cm + t.delay_s * 135.0;
                if q.chebyshev(start) <= CROWD_RADIUS && occupancy.occupied_by_others(q, mover) {
                    cost *= config.occupied_tile_cost;
                }
                next.push((q, cost.round().max(1.0) as u32));
            }
            next
        },
        |p: &TilePos| (p.octile(goal) * tile_cm * min_cost) as u32,
        |p: &TilePos| *p == goal,
    );
    match result {
        Some((mut path, cost)) => {
            path.remove(0);
            PathResult::Found { path, cost }
        }
        None if exhausted.get() => PathResult::TooFar,
        None => PathResult::Unreachable,
    }
}
