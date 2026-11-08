//! The simulation core: state containers, systems, events, commands,
//! scheduling, time, world creation and runtime instances.
//!
//! This module never touches rendering or UI. The application drives it via
//! [`GameState::advance_frame`] / [`GameState::step_tick`] and mutates it only
//! by submitting [`Command`]s.

pub mod commands;
pub mod context;
pub mod creation;
pub mod events;
pub mod game_state;
pub mod instances;
pub mod scheduler;
pub mod system;
pub mod time;
pub mod world_state;

pub use commands::{Command, CommandQueue};
pub use context::{SimEnv, SimulationContext};
pub use creation::{NewWorldOptions, create_world, restore_world};
pub use events::{
    EventBus, EventEnvelope, EventKind, EventLog, EventQueue, EventSource, InjurySeverity,
    LoggedEvent, SimEvent,
};
pub use game_state::{FrameReport, GameState};
pub use instances::{
    BuildingRecord, CharacterRecord, FactionRecord, FactionRelation, ItemInstance, ItemLocation,
    Position, SettlementRecord, SkillLevel,
};
pub use scheduler::{RejectedCommand, SimulationScheduler, TickReport};
pub use system::{
    System, SystemEntry, SystemPhase, SystemRegistry, SystemRunState, SystemStats, UpdateSchedule,
};
pub use world_state::{Components, DerivedWorld, WorldMeta, WorldState};
