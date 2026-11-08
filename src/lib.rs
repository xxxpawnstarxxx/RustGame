//! # medieval_sim
//!
//! A grounded, top-down, tile-based medieval world simulation.
//!
//! The crate is split into a library (all simulation, content, rendering and
//! interface code) and a thin binary (`main.rs`) that opens the window. Keeping
//! the logic in the library lets integration tests drive the simulation
//! headlessly, without a window.
//!
//! Layering (lower layers never depend on higher ones):
//!
//! ```text
//!   app ─────────────► ui, debugging, rendering   (presentation; read state, submit commands)
//!                         │
//!                         ▼
//!   simulation  (GameState, WorldState, scheduler, events, commands, time systems)
//!                         │
//!                         ▼
//!   content, entities     (immutable definitions; entity/component storage)
//!                         │
//!                         ▼
//!   core                  (ids, errors, deterministic RNG, configuration, calendar)
//! ```
//!
//! Implemented domain modules: `world` (map and generation), `environment`
//! (weather, ground, vegetation, wildlife) and `navigation` (path planning and
//! movement). Remaining domain modules (`characters`, `anatomy`, `combat`, `items`,
//! `inventory`, `ai`, `economy`, `crafting`, `settlements`, `factions`,
//! `military`, `scripting`) are declared now so that module
//! boundaries are fixed; each documents the responsibility it will own and is
//! filled in by its own development stage (see `docs/DEVELOPMENT_ROADMAP.md`).

pub mod ai;
pub mod anatomy;
pub mod app;
pub mod characters;
pub mod combat;
pub mod content;
pub mod core;
pub mod crafting;
pub mod debugging;
pub mod economy;
pub mod entities;
pub mod environment;
pub mod factions;
pub mod inventory;
pub mod items;
pub mod military;
pub mod navigation;
pub mod persistence;
pub mod rendering;
pub mod scripting;
pub mod settlements;
pub mod simulation;
pub mod ui;
pub mod world;
