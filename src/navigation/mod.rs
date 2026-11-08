//! Character navigation and movement.
//!
//! * [`traversal`] — per-tile passability and cost for walking, wading,
//!   swimming and climbing, combining terrain, roads, bridges, fords,
//!   structures (walls, fences, doors, gates), woodland, slope and current
//!   ground conditions.
//! * [`pathfinder`] — bounded 8-connected A* that avoids corner-cutting and
//!   routes around crowds.
//! * [`occupancy`] — tile occupancy and step reservations with a per-tile
//!   capacity, so characters never pile onto the same tile.
//! * [`mover`] — the `Mover` component: gaits (crouch, walk, run, sprint),
//!   stamina, fatigue, encumbrance and mobility.
//! * [`systems`] — `NavigationSystem` (budgeted planning and re-planning),
//!   `MovementSystem` (stepping, waiting, sidestepping, giving up, stumbling,
//!   falling) and `EncumbranceSystem` (carried weight from items).
//!
//! Re-planning happens when the next tile turns impassable (a door locked, a
//! wall built, a flood), when the map changes in a chunk the remaining path
//! crosses (tracked by per-chunk navigation epochs), after sidestepping, or
//! after a fall.

pub mod mover;
pub mod occupancy;
pub mod pathfinder;
pub mod systems;
pub mod traversal;

pub use mover::{Gait, Locomotion, MovementCapability, Mover, NavStatus, Step};
pub use occupancy::Occupancy;
pub use pathfinder::{PathResult, find_path};
pub use systems::{EncumbranceSystem, MovementSystem, NavigationSystem, carried_weight};
pub use traversal::{Abilities, Traversal, TraversalMode, traversal};

use serde::{Deserialize, Serialize};

/// Persistent change counters for path invalidation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NavigationState {
    /// Incremented on every navigation-relevant change.
    pub epoch: u64,
    /// Epoch of the latest change per map chunk.
    pub chunk_epochs: Vec<u64>,
}
