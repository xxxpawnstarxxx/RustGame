//! Simulation time: the fixed-step clock, real-time pacing, timers,
//! day/night, and time-based daily routines.
//!
//! * [`SimClock`] — tick counter and current [`GameInstant`]; detects calendar
//!   boundaries ([`calendar_boundary_events`]).
//! * [`TimeController`] — converts real frame time into a number of fixed
//!   ticks (pause, speed multipliers, single-stepping, spiral-of-death cap).
//! * [`TimerQueue`] — scheduled, delayed and recurring actions.
//! * [`daylight`] — sunrise/sunset, twilight and light level per season.
//! * [`AmbientTimeSystem`] — keeps [`AmbientState`] (light, day phase,
//!   season) current and emits change events.
//! * [`RoutineSystem`] — drives entities' daily routines from content data.
//!
//! [`GameInstant`]: crate::core::time::GameInstant

mod ambient;
mod clock;
mod controller;
pub mod daylight;
mod routine;
mod timers;

pub use ambient::{AmbientState, AmbientTimeSystem};
pub use clock::{SimClock, calendar_boundary_events};
pub use controller::TimeController;
pub use daylight::{DayPhase, DaylightState};
pub use routine::{RoutineAssignment, RoutineSegment, RoutineSystem, resolve_routine};
pub use timers::{Recurrence, Timer, TimerAction, TimerQueue};
