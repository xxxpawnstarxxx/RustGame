//! Core foundation shared by every other module.
//!
//! Nothing in `core` depends on any other module of this crate. It provides:
//!
//! * [`ids`] — strongly typed identifiers ([`ids::EntityId`], [`ids::EventId`], …).
//! * [`error`] — the typed [`SimError`] used across the simulation.
//! * [`hash`] — stable FNV-1a hashing for fingerprints and checksums.
//! * [`rng`] — the seeded, serialisable, deterministic [`rng::SimRng`].
//! * [`config`] — configuration structures and the [`config::ConfigurationManager`].
//! * [`time`] — calendar arithmetic and time primitives ([`time::Calendar`],
//!   [`time::GameInstant`], [`time::GameDuration`], [`time::TimeOfDay`]).

pub mod config;
pub mod error;
pub mod hash;
pub mod ids;
pub mod rng;
pub mod time;

pub use error::{SimError, SimResult};
