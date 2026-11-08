//! Immutable, data-driven content definitions.
//!
//! Content describes *what can exist* — materials, items, weapons, armour,
//! foods, resources, recipes, bodies, injuries, skills, professions, jobs,
//! character templates, buildings, animals, crops, trees, technologies, laws,
//! factions, world generation presets, activities and routines. It is loaded
//! once from JSON under `data/`, validated as a whole, and then shared
//! read-only by every system through the [`ContentRegistry`].
//!
//! Definitions are never mutated at runtime. Individual runtime instances
//! (a particular villager, a particular sword) are entities whose components
//! refer to definitions by typed id. New content needs only new JSON.

pub mod defs;
mod ids;
mod registry;

pub use defs::*;
pub use ids::*;
pub use registry::{
    CONTENT_FORMAT_VERSION, ContentIssue, ContentManifest, ContentRegistry, ContentStamp, DefTable,
    Definition, Reference, Registered, ValidationEnv,
};
