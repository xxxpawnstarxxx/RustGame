//! [`GameState`]: owns a running simulation.

use crate::content::ContentRegistry;
use crate::core::config::GameConfig;
use crate::core::error::SimResult;
use crate::core::time::Calendar;
use crate::environment::{GroundSystem, VegetationSystem, WeatherSystem, WildlifeSystem};
use crate::navigation::{EncumbranceSystem, MovementSystem, NavigationSystem};
use crate::simulation::commands::Command;
use crate::simulation::context::SimEnv;
use crate::simulation::creation::{NewWorldOptions, create_world, restore_world};
use crate::simulation::events::{EventBus, EventLog};
use crate::simulation::scheduler::{SimulationScheduler, TickReport};
use crate::simulation::system::{System, SystemRegistry};
use crate::simulation::time::{AmbientTimeSystem, RoutineSystem, TimeController};
use crate::simulation::world_state::WorldState;

/// Result of advancing one rendered frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameReport {
    /// Ticks executed this frame.
    pub ticks: u32,
    /// Ticks dropped this frame because of `max_ticks_per_frame`.
    pub dropped_ticks: u64,
}

/// A running simulation.
///
/// Separates the five kinds of state:
///
/// 1. persistent world state — [`WorldState`] (`world`)
/// 2. immutable content — [`ContentRegistry`] (`content`)
/// 3. temporary runtime state — pacing, event history, diagnostics
/// 4. simulation systems — [`SystemRegistry`]
/// 5. rendering/UI state — owned by the application, not here
pub struct GameState {
    world: WorldState,
    content: ContentRegistry,
    config: GameConfig,
    calendar: Calendar,
    systems: SystemRegistry,
    bus: EventBus,
    scheduler: SimulationScheduler,
    time: TimeController,
}

impl GameState {
    /// Starts a new world with the configured default name, seed and preset.
    pub fn new(config: GameConfig, content: ContentRegistry) -> SimResult<Self> {
        let options = NewWorldOptions::from_config(&config);
        Self::new_world(config, content, &options)
    }

    /// Generates and starts a new world.
    pub fn new_world(
        config: GameConfig,
        content: ContentRegistry,
        options: &NewWorldOptions,
    ) -> SimResult<Self> {
        config.validate()?;
        content.validate(&config)?;
        let calendar = Calendar::new(&config.calendar)?;
        let world = create_world(&config, &calendar, &content, options)?;
        Self::assemble(config, calendar, content, world)
    }

    /// Resumes a previously saved world, regenerating its derived state.
    pub fn from_world(
        config: GameConfig,
        content: ContentRegistry,
        mut world: WorldState,
    ) -> SimResult<Self> {
        config.validate()?;
        let calendar = Calendar::new(&config.calendar)?;
        restore_world(&mut world, &config, &calendar, &content)?;
        Self::assemble(config, calendar, content, world)
    }

    fn assemble(
        config: GameConfig,
        calendar: Calendar,
        content: ContentRegistry,
        world: WorldState,
    ) -> SimResult<Self> {
        let mut game = Self {
            bus: EventBus::new(config.simulation.event_history_capacity),
            time: TimeController::new(&config.simulation),
            world,
            content,
            config,
            calendar,
            systems: SystemRegistry::new(),
            scheduler: SimulationScheduler::new(),
        };
        // Registration order is execution order within each phase.
        game.register_system(Box::new(AmbientTimeSystem))?;
        game.register_system(Box::new(WeatherSystem))?;
        game.register_system(Box::new(GroundSystem))?;
        game.register_system(Box::new(VegetationSystem))?;
        game.register_system(Box::new(WildlifeSystem))?;
        game.register_system(Box::new(RoutineSystem))?;
        game.register_system(Box::new(NavigationSystem))?;
        game.register_system(Box::new(MovementSystem))?;
        game.register_system(Box::new(EncumbranceSystem))?;
        Ok(game)
    }

    /// Adds a system. Registration order is execution order within a phase.
    pub fn register_system(&mut self, system: Box<dyn System>) -> SimResult<()> {
        self.systems
            .register(system, &self.calendar, self.world.clock.tick_duration())
    }

    /// Queues a command for the next tick.
    pub fn submit(&mut self, command: Command) {
        self.world.commands.push(command);
    }

    /// Advances real time by `real_dt` seconds, running as many fixed ticks
    /// as the pacing controller allows.
    pub fn advance_frame(&mut self, real_dt: f64) -> FrameReport {
        let dropped_before = self.time.dropped_ticks();
        let ticks = self.time.plan_frame(real_dt);
        for _ in 0..ticks {
            self.step_tick();
        }
        FrameReport {
            ticks,
            dropped_ticks: self.time.dropped_ticks() - dropped_before,
        }
    }

    /// Executes exactly one tick regardless of pause state.
    pub fn step_tick(&mut self) -> TickReport {
        let env = SimEnv {
            content: &self.content,
            config: &self.config,
            calendar: &self.calendar,
        };
        self.scheduler
            .run_tick(&mut self.world, &mut self.systems, &mut self.bus, env)
    }

    /// Executes `n` ticks (headless runs and tests).
    pub fn run_ticks(&mut self, n: u64) {
        for _ in 0..n {
            self.step_tick();
        }
    }

    /// Replaces the world (loading a save). Runtime history is cleared.
    pub fn replace_world(&mut self, mut world: WorldState) -> SimResult<()> {
        restore_world(&mut world, &self.config, &self.calendar, &self.content)?;
        self.world = world;
        self.bus.log_mut().clear();
        Ok(())
    }

    /// Replaces the running world with a newly generated one.
    pub fn regenerate(&mut self, options: &NewWorldOptions) -> SimResult<()> {
        let world = create_world(&self.config, &self.calendar, &self.content, options)?;
        self.world = world;
        self.bus.log_mut().clear();
        Ok(())
    }

    pub fn world(&self) -> &WorldState {
        &self.world
    }

    pub fn content(&self) -> &ContentRegistry {
        &self.content
    }

    pub fn config(&self) -> &GameConfig {
        &self.config
    }

    pub fn calendar(&self) -> &Calendar {
        &self.calendar
    }

    pub fn systems(&self) -> &SystemRegistry {
        &self.systems
    }

    pub fn event_log(&self) -> &EventLog {
        self.bus.log()
    }

    pub fn scheduler(&self) -> &SimulationScheduler {
        &self.scheduler
    }

    pub fn time(&self) -> &TimeController {
        &self.time
    }

    pub fn time_mut(&mut self) -> &mut TimeController {
        &mut self.time
    }
}
