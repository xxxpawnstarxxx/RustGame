//! Entity identity and component storage.
//!
//! * [`EntityManager`] allocates and retires generational [`EntityId`]s.
//! * [`ComponentStore`] is the generic, deterministic, serialisable storage for
//!   one component type, keyed by entity.
//!
//! Domain modules define their own component types (e.g. routines in
//! `simulation::time`, later anatomy, inventories, skills) and the aggregate
//! [`crate::simulation::Components`] collects one store per type.
//!
//! [`EntityId`]: crate::core::ids::EntityId

mod components;
mod manager;

pub use components::{ComponentStore, Name};
pub use manager::{EntityKind, EntityManager};
