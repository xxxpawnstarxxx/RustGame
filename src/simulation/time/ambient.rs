use super::daylight::{self, DaylightState};
use crate::core::config::Season;
use crate::core::ids::SystemId;
use crate::core::time::{Calendar, GameInstant, TimeUnit};
use crate::simulation::context::SimulationContext;
use crate::simulation::events::SimEvent;
use crate::simulation::system::{System, SystemPhase, UpdateSchedule};
use serde::{Deserialize, Serialize};

/// World-wide, time-dependent conditions read by other systems (and the
/// renderer): light level, day phase and season.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AmbientState {
    pub daylight: DaylightState,
    pub season: Season,
}

impl AmbientState {
    pub fn compute(calendar: &Calendar, t: GameInstant) -> Self {
        Self {
            daylight: daylight::evaluate(calendar, t),
            season: calendar.season_at(t),
        }
    }
}

/// Updates [`AmbientState`] every game minute and emits
/// [`SimEvent::DayPhaseChanged`] / [`SimEvent::SeasonChanged`] on change.
#[derive(Debug, Default)]
pub struct AmbientTimeSystem;

impl AmbientTimeSystem {
    pub const ID: SystemId = SystemId::new("ambient_time");
}

impl System for AmbientTimeSystem {
    fn id(&self) -> SystemId {
        Self::ID
    }

    fn phase(&self) -> SystemPhase {
        SystemPhase::PreUpdate
    }

    fn schedule(&self) -> UpdateSchedule {
        UpdateSchedule::Interval {
            amount: 1,
            unit: TimeUnit::Minute,
        }
    }

    fn update(&mut self, ctx: &mut SimulationContext<'_>) {
        let next = AmbientState::compute(ctx.calendar, ctx.now());
        let previous = std::mem::replace(&mut ctx.world.ambient, next.clone());
        if previous.daylight.phase != next.daylight.phase {
            ctx.emit(SimEvent::DayPhaseChanged {
                previous: previous.daylight.phase,
                current: next.daylight.phase,
            });
        }
        if previous.season != next.season {
            ctx.emit(SimEvent::SeasonChanged {
                previous: previous.season,
                current: next.season,
            });
        }
    }
}
