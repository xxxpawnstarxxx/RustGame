//! Simulation systems that advance the environment.

use super::ground::{self, PollutionSource};
use super::{vegetation, weather, wildlife};
use crate::core::ids::SystemId;
use crate::core::time::TimeUnit;
use crate::simulation::{SimEvent, SimulationContext, System, SystemPhase, UpdateSchedule};
use crate::world::SettlementKind;

fn hours(ctx: &SimulationContext<'_>) -> f32 {
    ctx.elapsed().seconds() as f32 / ctx.calendar.seconds_per_hour() as f32
}

fn days(ctx: &SimulationContext<'_>) -> f32 {
    ctx.elapsed().seconds() as f32 / ctx.calendar.seconds_per_day() as f32
}

fn ready(ctx: &SimulationContext<'_>) -> bool {
    ctx.world.environment.initialized && !ctx.world.derived.statics.ground.is_empty()
}

/// Hourly weather.
#[derive(Debug, Default)]
pub struct WeatherSystem;

impl WeatherSystem {
    pub const ID: SystemId = SystemId::new("weather");
}

impl System for WeatherSystem {
    fn id(&self) -> SystemId {
        Self::ID
    }
    fn phase(&self) -> SystemPhase {
        SystemPhase::PreUpdate
    }
    fn schedule(&self) -> UpdateSchedule {
        UpdateSchedule::Interval {
            amount: 1,
            unit: TimeUnit::Hour,
        }
    }
    fn update(&mut self, ctx: &mut SimulationContext<'_>) {
        if !ready(ctx) {
            return;
        }
        let Some(preset) = ctx.content.worldgen().get(&ctx.world.map_info.preset) else {
            return;
        };
        let h = hours(ctx);
        let now = ctx.now();
        let before: Vec<bool> = ctx
            .world
            .environment
            .weather
            .cells
            .iter()
            .map(|c| c.is_precipitating())
            .collect();
        let world = &mut *ctx.world;
        let tile = world.derived.map.tile_size_m();
        weather::step(
            &mut world.environment,
            &world.derived.statics,
            preset,
            &ctx.config.environment,
            ctx.calendar,
            now,
            h,
            tile,
        );
        let changes: Vec<SimEvent> = ctx
            .world
            .environment
            .weather
            .cells
            .iter()
            .enumerate()
            .filter(|(i, c)| c.is_precipitating() != before[*i])
            .map(|(i, c)| {
                if c.is_precipitating() {
                    SimEvent::PrecipitationStarted {
                        region: i as u32,
                        snow: c.snowing,
                    }
                } else {
                    SimEvent::PrecipitationStopped { region: i as u32 }
                }
            })
            .collect();
        for event in changes {
            ctx.emit(event);
        }
    }
}

/// Hourly ground water, snow, floods, mud, contamination and drought.
#[derive(Debug, Default)]
pub struct GroundSystem;

impl GroundSystem {
    pub const ID: SystemId = SystemId::new("ground");
}

impl System for GroundSystem {
    fn id(&self) -> SystemId {
        Self::ID
    }
    fn schedule(&self) -> UpdateSchedule {
        UpdateSchedule::Interval {
            amount: 1,
            unit: TimeUnit::Hour,
        }
    }
    fn update(&mut self, ctx: &mut SimulationContext<'_>) {
        if !ready(ctx) {
            return;
        }
        let h = hours(ctx);
        let world = &mut *ctx.world;
        let sources: Vec<PollutionSource> = world
            .components
            .settlements
            .iter()
            .filter_map(|(_, s)| {
                world
                    .derived
                    .statics
                    .ground_cell(s.centre)
                    .map(|cell| PollutionSource {
                        cell,
                        strength: match s.kind {
                            SettlementKind::Town => 3.0,
                            SettlementKind::Village => 1.5,
                            SettlementKind::Hamlet => 0.5,
                        },
                    })
            })
            .collect();
        let env = &mut world.environment;
        let t = ground::step(
            &mut env.ground,
            &mut env.weather,
            &world.derived.statics,
            &ctx.config.environment,
            &sources,
            h,
        );
        let flood_regions: Vec<u32> = t
            .flood_started
            .iter()
            .chain(&t.flood_ended)
            .copied()
            .collect();
        if !flood_regions.is_empty() {
            world.invalidate_weather_regions(&flood_regions);
        }
        for region in t.drought_started {
            ctx.emit(SimEvent::DroughtStarted { region });
        }
        for region in t.drought_ended {
            ctx.emit(SimEvent::DroughtEnded { region });
        }
        for region in t.flood_started {
            ctx.emit(SimEvent::FloodStarted { region });
        }
        for region in t.flood_ended {
            ctx.emit(SimEvent::FloodEnded { region });
        }
    }
}

/// Daily grass and woodland dynamics.
#[derive(Debug, Default)]
pub struct VegetationSystem;

impl VegetationSystem {
    pub const ID: SystemId = SystemId::new("vegetation");
}

impl System for VegetationSystem {
    fn id(&self) -> SystemId {
        Self::ID
    }
    fn schedule(&self) -> UpdateSchedule {
        UpdateSchedule::Interval {
            amount: 1,
            unit: TimeUnit::Day,
        }
    }
    fn update(&mut self, ctx: &mut SimulationContext<'_>) {
        if !ready(ctx) {
            return;
        }
        let d = days(ctx);
        let world = &mut *ctx.world;
        let browse: Vec<f32> = world
            .environment
            .wildlife
            .regions
            .iter()
            .map(|r| r.browse_pressure)
            .collect();
        let env = &mut world.environment;
        vegetation::step(
            &mut env.ground,
            &env.weather,
            &world.derived.statics,
            &world.derived.map,
            ctx.content,
            &ctx.config.environment,
            &browse,
            d,
        );
    }
}

/// Daily wildlife population dynamics.
#[derive(Debug, Default)]
pub struct WildlifeSystem;

impl WildlifeSystem {
    pub const ID: SystemId = SystemId::new("wildlife");
}

impl System for WildlifeSystem {
    fn id(&self) -> SystemId {
        Self::ID
    }
    fn schedule(&self) -> UpdateSchedule {
        UpdateSchedule::Interval {
            amount: 1,
            unit: TimeUnit::Day,
        }
    }
    fn update(&mut self, ctx: &mut SimulationContext<'_>) {
        if !ready(ctx) {
            return;
        }
        let d = days(ctx);
        let month = ctx.calendar.decompose(ctx.now()).month + 1;
        let world = &mut *ctx.world;
        let settlement_regions: Vec<(usize, f32)> = world
            .components
            .settlements
            .iter()
            .filter_map(|(_, s)| {
                world.derived.statics.wildlife_region(s.centre).map(|r| {
                    (
                        r,
                        match s.kind {
                            SettlementKind::Town => 1.0,
                            SettlementKind::Village => 0.7,
                            SettlementKind::Hamlet => 0.4,
                        },
                    )
                })
            })
            .collect();
        let tile = world.derived.map.tile_size_m();
        let env = &mut world.environment;
        let t = wildlife::step(
            &mut env.wildlife,
            &mut env.ground,
            &world.derived.statics,
            ctx.content,
            &ctx.config.environment,
            &mut env.rng,
            month,
            tile,
            &settlement_regions,
            d,
        );
        for (region, animal, disease) in t.outbreaks_started {
            ctx.emit(SimEvent::WildlifeOutbreakStarted {
                region,
                animal,
                disease,
            });
        }
        for (region, animal) in t.outbreaks_ended {
            ctx.emit(SimEvent::WildlifeOutbreakEnded { region, animal });
        }
    }
}
