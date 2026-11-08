//! The [`SimulationContext`] handed to systems.

use crate::content::ContentRegistry;
use crate::core::config::GameConfig;
use crate::core::ids::EventId;
use crate::core::rng::SimRng;
use crate::core::time::{Calendar, GameDuration, GameInstant};
use crate::simulation::commands::Command;
use crate::simulation::events::{EventSource, SimEvent};
use crate::simulation::world_state::WorldState;

/// Read-only environment shared by every system invocation: content,
/// configuration and calendar.
#[derive(Debug, Clone, Copy)]
pub struct SimEnv<'a> {
    pub content: &'a ContentRegistry,
    pub config: &'a GameConfig,
    pub calendar: &'a Calendar,
}

/// Everything a system may touch while running.
///
/// * `world` — the persistent, mutable world state.
/// * `content`, `config`, `calendar` — immutable.
///
/// Events emitted through [`SimulationContext::emit`] are stamped with the
/// current tick, time and the invoking system as their source.
pub struct SimulationContext<'a> {
    pub world: &'a mut WorldState,
    pub content: &'a ContentRegistry,
    pub config: &'a GameConfig,
    pub calendar: &'a Calendar,
    source: EventSource,
    elapsed: GameDuration,
}

impl<'a> SimulationContext<'a> {
    pub fn new(
        world: &'a mut WorldState,
        env: SimEnv<'a>,
        source: EventSource,
        elapsed: GameDuration,
    ) -> Self {
        Self {
            world,
            content: env.content,
            config: env.config,
            calendar: env.calendar,
            source,
            elapsed,
        }
    }

    /// Current tick number.
    pub fn tick(&self) -> u64 {
        self.world.clock.tick()
    }

    /// Current game time.
    pub fn now(&self) -> GameInstant {
        self.world.clock.now()
    }

    /// Game time elapsed since this system's previous update (one tick for
    /// the first run; zero inside event handlers). Interval systems integrate
    /// over this so their results do not depend on the chosen interval.
    pub fn elapsed(&self) -> GameDuration {
        self.elapsed
    }

    /// Who events emitted through this context are attributed to.
    pub fn source(&self) -> &EventSource {
        &self.source
    }

    /// Queues an event for dispatch.
    pub fn emit(&mut self, event: SimEvent) -> EventId {
        let tick = self.world.clock.tick();
        let now = self.world.clock.now();
        self.world
            .events
            .push(tick, now, self.source.clone(), event)
    }

    /// Queues a command for application at the next tick.
    pub fn submit(&mut self, command: Command) {
        self.world.commands.push(command);
    }

    /// The world's deterministic random number generator.
    pub fn rng(&mut self) -> &mut SimRng {
        &mut self.world.rng
    }
}
