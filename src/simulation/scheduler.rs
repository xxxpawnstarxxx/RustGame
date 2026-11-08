//! [`SimulationScheduler`]: the fixed pipeline executed once per tick.
//!
//! ```text
//! 1. advance clock by one tick          → calendar boundary events
//! 2. fire due timers                    → events / commands
//! 3. apply queued commands (FIFO)       → events
//! 4. dispatch events
//! 5. for each phase (Pre, Update, Post):
//!        run every due system, in registration order
//!        dispatch events
//! ```
//!
//! The per-tick event budget is shared by all dispatch passes; events beyond
//! it stay queued (persistently) and are dispatched first next tick.

use crate::core::time::GameDuration;
use crate::simulation::commands;
use crate::simulation::context::{SimEnv, SimulationContext};
use crate::simulation::events::{EventBus, EventSource};
use crate::simulation::system::{SystemPhase, SystemRegistry};
use crate::simulation::time::{TimerAction, calendar_boundary_events};
use crate::simulation::world_state::WorldState;
use std::collections::VecDeque;

/// What happened during one tick.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TickReport {
    pub tick: u64,
    pub timers_fired: u32,
    pub commands_applied: u32,
    pub commands_rejected: u32,
    pub systems_run: u32,
    pub events_dispatched: u32,
    pub events_deferred: usize,
}

/// A command that failed to apply, kept for the debug UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedCommand {
    pub tick: u64,
    pub command: String,
    pub reason: String,
}

const REJECTION_HISTORY: usize = 64;

/// Runs ticks. Holds only runtime diagnostics; all scheduling state that
/// affects outcomes lives in [`WorldState`].
#[derive(Debug, Clone, Default)]
pub struct SimulationScheduler {
    last_report: TickReport,
    rejected: VecDeque<RejectedCommand>,
}

impl SimulationScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn last_report(&self) -> &TickReport {
        &self.last_report
    }

    /// Most recent rejected commands, newest first.
    pub fn rejected_commands(&self) -> impl Iterator<Item = &RejectedCommand> {
        self.rejected.iter().rev()
    }

    /// Executes exactly one tick.
    pub fn run_tick(
        &mut self,
        world: &mut WorldState,
        registry: &mut SystemRegistry,
        bus: &mut EventBus,
        env: SimEnv<'_>,
    ) -> TickReport {
        let previous = world.clock.advance();
        let now = world.clock.now();
        let tick = world.clock.tick();
        let mut report = TickReport {
            tick,
            ..TickReport::default()
        };

        // 1. Calendar boundaries.
        for event in calendar_boundary_events(env.calendar, previous, now) {
            world.events.push(tick, now, EventSource::Clock, event);
        }

        // 2. Timers.
        report.timers_fired = Self::fire_timers(world, env);

        // 3. Commands.
        for command in world.commands.drain() {
            let description = format!("{command:?}");
            let mut ctx =
                SimulationContext::new(world, env, EventSource::Command, GameDuration::ZERO);
            match commands::apply(command, &mut ctx) {
                Ok(()) => report.commands_applied += 1,
                Err(err) => {
                    report.commands_rejected += 1;
                    tracing::warn!(tick, %err, "command rejected");
                    if self.rejected.len() == REJECTION_HISTORY {
                        self.rejected.pop_front();
                    }
                    self.rejected.push_back(RejectedCommand {
                        tick,
                        command: description,
                        reason: err.to_string(),
                    });
                }
            }
        }

        // 4. Dispatch.
        let mut budget = env.config.simulation.max_events_per_tick;
        report.events_dispatched += bus.dispatch(&mut budget, world, registry, env);

        // 5. Phased system updates.
        for phase in SystemPhase::ALL {
            for index in 0..registry.len() {
                if registry.entry(index).phase() != phase {
                    continue;
                }
                let entry = registry.entry_mut(index);
                let state = world.systems.entry(entry.id().clone()).or_default();
                if !entry.is_due(state, tick) {
                    continue;
                }
                let elapsed = state
                    .last_run_time
                    .map_or(world.clock.tick_duration(), |last| now.duration_since(last));
                state.wake_pending = false;
                state.last_run_tick = Some(tick);
                state.last_run_time = Some(now);
                state.run_count += 1;
                let source = EventSource::System(entry.id().clone());
                let mut ctx = SimulationContext::new(world, env, source, elapsed);
                entry.run_update(&mut ctx);
                report.systems_run += 1;
            }
            report.events_dispatched += bus.dispatch(&mut budget, world, registry, env);
        }

        report.events_deferred = world.events.len();
        if report.events_deferred > 0 {
            bus.log_mut().note_deferral();
            tracing::warn!(
                tick,
                deferred = report.events_deferred,
                "event budget exhausted; remaining events deferred to next tick"
            );
        }
        self.last_report = report.clone();
        report
    }

    fn fire_timers(world: &mut WorldState, env: SimEnv<'_>) -> u32 {
        let now = world.clock.now();
        let tick = world.clock.tick();
        let mut fired = 0;
        while fired < env.config.simulation.max_timer_firings_per_tick {
            let Some(mut timer) = world.timers.pop_due(now) else {
                break;
            };
            fired += 1;
            match &timer.action {
                TimerAction::Emit(event) => {
                    world
                        .events
                        .push(tick, now, EventSource::Timer(timer.id), event.clone());
                }
                TimerAction::Command(command) => world.commands.push(command.clone()),
            }
            timer.times_fired += 1;
            if let Some(remaining) = timer.remaining_firings.as_mut() {
                *remaining = remaining.saturating_sub(1);
                if *remaining == 0 {
                    continue;
                }
            }
            match timer.recurrence.next_after(env.calendar, timer.fire_at) {
                Ok(Some(next)) => {
                    timer.fire_at = next;
                    world.timers.reschedule(timer);
                }
                Ok(None) => {}
                Err(err) => tracing::warn!(timer = %timer.id, %err, "recurring timer dropped"),
            }
        }
        fired
    }
}
