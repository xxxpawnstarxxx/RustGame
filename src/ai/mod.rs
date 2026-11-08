//! Decision-making for autonomous characters.
//!
//! Will own: utility-based action selection seeded by the scheduled routine
//! activity (`ScheduledActivityChanged`), needs and obligations; task
//! planning; movement intent. AI never mutates the world directly: it submits
//! commands or performs actions through systems. Implemented in the *AI* stage.
