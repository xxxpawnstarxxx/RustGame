//! Configuration structures and the [`ConfigurationManager`].
//!
//! Configuration is *how the simulation runs* (seed, tick length, speeds,
//! calendar, historical period). It is distinct from content definitions
//! (*what exists*: activities, routines, later items and buildings), which
//! live in [`crate::content`].
//!
//! Configuration is loaded from JSON files in the `config/` directory:
//!
//! | File               | Structure            |
//! |--------------------|----------------------|
//! | `simulation.json`  | [`SimulationConfig`] |
//! | `calendar.json`    | [`CalendarConfig`]   |
//! | `period.json`      | [`PeriodConfig`]     |
//! | `movement.json`    | [`MovementConfig`]   |
//! | `environment.json` | [`EnvironmentConfig`] |
//!
//! A missing file falls back to the built-in defaults (which match the shipped
//! files) and logs a warning; a malformed file is an error. Every loaded
//! configuration is validated before use.

use crate::core::error::{SimError, SimResult};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

/// Meteorological season, assigned per month by the calendar configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl fmt::Display for Season {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Spring => "Spring",
            Self::Summer => "Summer",
            Self::Autumn => "Autumn",
            Self::Winter => "Winter",
        })
    }
}

/// Simulation pacing and determinism settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimulationConfig {
    /// World seed for all simulation randomness.
    pub seed: u64,
    /// In-game seconds that elapse per fixed simulation tick.
    pub game_seconds_per_tick: u32,
    /// Simulation ticks per real second at speed multiplier 1.
    pub base_ticks_per_real_second: f64,
    /// Selectable speed multipliers, slowest first.
    pub speed_multipliers: Vec<f64>,
    /// Index into `speed_multipliers` used at start-up.
    pub default_speed_index: usize,
    /// Upper bound on ticks executed in one rendered frame. Excess ticks are
    /// dropped (the simulation slows down instead of spiralling).
    pub max_ticks_per_frame: u32,
    /// Real frame deltas are clamped to this many seconds (window drags,
    /// breakpoints) before being converted to ticks.
    pub max_frame_delta_seconds: f64,
    /// Upper bound on events dispatched in one tick; the remainder is deferred.
    pub max_events_per_tick: u32,
    /// Upper bound on timer firings in one tick; the remainder fires next tick.
    pub max_timer_firings_per_tick: u32,
    /// Number of processed events retained for the debug inspector.
    pub event_history_capacity: usize,
    /// Whether the simulation starts paused.
    pub start_paused: bool,
    /// World generation preset used for new worlds unless one is chosen.
    pub default_world_preset: String,
    /// Name given to new worlds unless one is chosen.
    pub default_world_name: String,
    /// Number of rotated backups kept per save slot.
    pub save_backups: u32,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            default_world_preset: "lowland_england".into(),
            default_world_name: "Ashworth Vale".into(),
            save_backups: 3,
            seed: 1340,
            game_seconds_per_tick: 1,
            base_ticks_per_real_second: 30.0,
            speed_multipliers: vec![1.0, 3.0, 10.0, 30.0, 60.0],
            default_speed_index: 0,
            max_ticks_per_frame: 1000,
            max_frame_delta_seconds: 0.25,
            max_events_per_tick: 20_000,
            max_timer_firings_per_tick: 1_000,
            event_history_capacity: 2_000,
            start_paused: false,
        }
    }
}

impl SimulationConfig {
    /// Checks value ranges.
    pub fn validate(&self) -> SimResult<()> {
        ensure(self.game_seconds_per_tick >= 1, || {
            "simulation.game_seconds_per_tick must be at least 1".into()
        })?;
        ensure(
            self.base_ticks_per_real_second.is_finite() && self.base_ticks_per_real_second > 0.0,
            || "simulation.base_ticks_per_real_second must be a positive number".into(),
        )?;
        ensure(!self.speed_multipliers.is_empty(), || {
            "simulation.speed_multipliers must not be empty".into()
        })?;
        ensure(
            self.speed_multipliers
                .iter()
                .all(|m| m.is_finite() && *m > 0.0),
            || "simulation.speed_multipliers must all be positive numbers".into(),
        )?;
        ensure(
            self.default_speed_index < self.speed_multipliers.len(),
            || "simulation.default_speed_index is out of range".into(),
        )?;
        ensure(self.max_ticks_per_frame >= 1, || {
            "simulation.max_ticks_per_frame must be at least 1".into()
        })?;
        ensure(
            self.max_frame_delta_seconds.is_finite() && self.max_frame_delta_seconds > 0.0,
            || "simulation.max_frame_delta_seconds must be a positive number".into(),
        )?;
        ensure(self.max_events_per_tick >= 1, || {
            "simulation.max_events_per_tick must be at least 1".into()
        })?;
        ensure(self.max_timer_firings_per_tick >= 1, || {
            "simulation.max_timer_firings_per_tick must be at least 1".into()
        })
    }
}

/// One month of the configured calendar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonthConfig {
    pub name: String,
    pub days: u32,
    pub season: Season,
    /// Hours of daylight (sunrise to sunset) at the middle of the month.
    pub daylight_hours: f32,
}

/// The date and time at which a new world starts. Month and day are 1-based.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartDateConfig {
    pub year: u32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    /// Name of the weekday the start date falls on; anchors the week cycle.
    pub weekday: String,
}

/// Calendar definition: time units, months, weekdays and daylight.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarConfig {
    pub seconds_per_minute: u32,
    pub minutes_per_hour: u32,
    pub hours_per_day: u32,
    pub weekdays: Vec<String>,
    pub months: Vec<MonthConfig>,
    pub start: StartDateConfig,
    /// Length of dawn and dusk twilight, in minutes.
    pub twilight_minutes: u32,
    /// Ambient light level at night (0 = black, 1 = full daylight).
    pub night_light_level: f32,
}

impl Default for CalendarConfig {
    fn default() -> Self {
        // Julian-style months; daylight approximates southern England (~52°N).
        let months = [
            ("January", 31, Season::Winter, 8.0),
            ("February", 28, Season::Winter, 9.8),
            ("March", 31, Season::Spring, 11.8),
            ("April", 30, Season::Spring, 13.9),
            ("May", 31, Season::Spring, 15.7),
            ("June", 30, Season::Summer, 16.6),
            ("July", 31, Season::Summer, 16.2),
            ("August", 31, Season::Summer, 14.6),
            ("September", 30, Season::Autumn, 12.6),
            ("October", 31, Season::Autumn, 10.6),
            ("November", 30, Season::Autumn, 8.7),
            ("December", 31, Season::Winter, 7.7),
        ]
        .into_iter()
        .map(|(name, days, season, daylight_hours)| MonthConfig {
            name: name.into(),
            days,
            season,
            daylight_hours,
        })
        .collect();
        Self {
            seconds_per_minute: 60,
            minutes_per_hour: 60,
            hours_per_day: 24,
            weekdays: [
                "Sunday",
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            months,
            start: StartDateConfig {
                year: 1340,
                month: 3,
                day: 1,
                hour: 6,
                minute: 0,
                weekday: "Wednesday".into(),
            },
            twilight_minutes: 40,
            night_light_level: 0.12,
        }
    }
}

impl CalendarConfig {
    /// Seconds in one day under this calendar.
    pub fn seconds_per_day(&self) -> u64 {
        u64::from(self.seconds_per_minute)
            * u64::from(self.minutes_per_hour)
            * u64::from(self.hours_per_day)
    }

    /// Seconds in one hour under this calendar.
    pub fn seconds_per_hour(&self) -> u64 {
        u64::from(self.seconds_per_minute) * u64::from(self.minutes_per_hour)
    }

    /// Checks internal consistency.
    pub fn validate(&self) -> SimResult<()> {
        ensure(
            self.seconds_per_minute >= 1 && self.minutes_per_hour >= 1 && self.hours_per_day >= 1,
            || "calendar time units must all be at least 1".into(),
        )?;
        ensure(!self.weekdays.is_empty(), || {
            "calendar.weekdays must not be empty".into()
        })?;
        ensure_unique(
            self.weekdays.iter().map(String::as_str),
            "calendar.weekdays",
        )?;
        ensure(!self.months.is_empty(), || {
            "calendar.months must not be empty".into()
        })?;
        ensure_unique(
            self.months.iter().map(|m| m.name.as_str()),
            "calendar.months",
        )?;
        let hours = self.hours_per_day as f32;
        for month in &self.months {
            ensure(!month.name.trim().is_empty(), || {
                "calendar month names must not be empty".into()
            })?;
            ensure(month.days >= 1, || {
                format!("calendar month '{}' must have at least one day", month.name)
            })?;
            ensure(
                month.daylight_hours.is_finite() && (0.0..=hours).contains(&month.daylight_hours),
                || {
                    format!(
                        "calendar month '{}' daylight_hours must be between 0 and {}",
                        month.name, self.hours_per_day
                    )
                },
            )?;
        }
        let start = &self.start;
        ensure(
            (1..=self.months.len() as u32).contains(&start.month),
            || "calendar.start.month is out of range".into(),
        )?;
        let month_days = self.months[(start.month - 1) as usize].days;
        ensure((1..=month_days).contains(&start.day), || {
            "calendar.start.day is out of range for its month".into()
        })?;
        ensure(start.hour < self.hours_per_day, || {
            "calendar.start.hour is out of range".into()
        })?;
        ensure(start.minute < self.minutes_per_hour, || {
            "calendar.start.minute is out of range".into()
        })?;
        ensure(self.weekdays.contains(&start.weekday), || {
            format!(
                "calendar.start.weekday '{}' is not a configured weekday",
                start.weekday
            )
        })?;
        let twilight = u64::from(self.twilight_minutes) * u64::from(self.seconds_per_minute);
        ensure(twilight * 2 < self.seconds_per_day(), || {
            "calendar.twilight_minutes must be shorter than half a day".into()
        })?;
        ensure(
            self.night_light_level.is_finite() && (0.0..=1.0).contains(&self.night_light_level),
            || "calendar.night_light_level must be between 0 and 1".into(),
        )
    }
}

/// Historical period and technology assumptions.
///
/// Content stages validate their definitions (tools, buildings, equipment)
/// against `technologies`, keeping the setting consistent and swappable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeriodConfig {
    pub name: String,
    pub region: String,
    pub description: String,
    pub earliest_year: u32,
    pub latest_year: u32,
    /// Technology keys considered available in this period.
    pub technologies: Vec<String>,
}

impl Default for PeriodConfig {
    fn default() -> Self {
        Self {
            name: "Fourteenth-century England".into(),
            region: "Lowland England".into(),
            description: "Manorial agriculture, chartered market towns, guild crafts, \
                          parish churches and monasteries, feudal lordship and royal justice."
                .into(),
            earliest_year: 1300,
            latest_year: 1400,
            technologies: [
                "heavy_plough",
                "open_field_rotation",
                "horse_collar",
                "water_mill",
                "post_windmill",
                "fulling_mill",
                "spinning_wheel",
                "treadle_loom",
                "bloomery_iron",
                "charcoal_burning",
                "timber_framing",
                "stone_masonry",
                "mechanical_clock",
                "longbow",
                "crossbow",
                "mail_armour",
                "plate_armour",
                "early_gunpowder_artillery",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        }
    }
}

impl PeriodConfig {
    /// Checks internal consistency.
    pub fn validate(&self) -> SimResult<()> {
        ensure(!self.name.trim().is_empty(), || {
            "period.name must not be empty".into()
        })?;
        ensure(self.earliest_year <= self.latest_year, || {
            "period.earliest_year must not be after period.latest_year".into()
        })?;
        ensure(
            self.technologies.iter().all(|t| !t.trim().is_empty()),
            || "period.technologies must not contain empty keys".into(),
        )?;
        ensure_unique(
            self.technologies.iter().map(String::as_str),
            "period.technologies",
        )
    }

    /// Whether a technology key is available in this period.
    pub fn has_technology(&self, key: &str) -> bool {
        self.technologies.iter().any(|t| t == key)
    }
}

/// Speed, effort and hazard multipliers of one gait.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GaitConfig {
    /// Multiple of the character's walking speed.
    pub speed_multiplier: f32,
    /// Stamina change per second (negative drains).
    pub stamina_per_second: f32,
    /// Fatigue change per second.
    pub fatigue_per_second: f32,
    /// Multiple of the base stumble chance.
    pub stumble_multiplier: f32,
}

/// Time cost multipliers of crossing each terrain on foot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerrainCostConfig {
    pub lowland: f32,
    pub hills: f32,
    pub mountain: f32,
    pub beach: f32,
    pub marsh: f32,
    /// Wading through shallow water.
    pub shallows: f32,
    /// Climbing a cliff (requires the ability to climb).
    pub cliff: f32,
    /// Swimming in deep water (requires the ability to swim).
    pub swim: f32,
}

/// Navigation and locomotion tuning.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MovementConfig {
    pub walk: GaitConfig,
    pub run: GaitConfig,
    pub sprint: GaitConfig,
    pub crouch: GaitConfig,
    pub swim_speed_mps: f32,
    pub swim_stamina_per_second: f32,
    pub climb_speed_mps: f32,
    pub climb_stamina_per_second: f32,
    pub rest_stamina_per_second: f32,
    pub rest_fatigue_per_second: f32,
    /// Below this stamina a character drops to a slower gait.
    pub exhausted_stamina: f32,
    pub terrain_cost: TerrainCostConfig,
    pub road_multiplier: f32,
    pub track_multiplier: f32,
    pub bridge_multiplier: f32,
    pub ford_cost: f32,
    /// Extra cost per unit of tree density (undergrowth, roots).
    pub forest_cost: f32,
    pub slope_cost_per_degree: f32,
    pub mud_cost: f32,
    /// Extra cost per 10 cm of snow.
    pub snow_cost_per_10cm: f32,
    pub flood_cost_per_m: f32,
    pub door_open_seconds: f32,
    pub gate_open_seconds: f32,
    /// Characters that may share a tile.
    pub tile_capacity: u8,
    /// Path cost multiplier of a tile currently occupied by others.
    pub occupied_tile_cost: f32,
    pub max_path_expansions: u32,
    pub max_path_requests_per_tick: u32,
    pub repath_wait_seconds: f32,
    pub give_up_wait_seconds: f32,
    /// Chance of stumbling per step on rough ground at walking pace.
    pub stumble_chance: f32,
    pub stumble_seconds: f32,
    /// Chance of falling per climbing step, before modifiers.
    pub climb_fall_chance: f32,
    /// Load fraction above which speed starts to drop.
    pub encumbrance_soft_limit: f32,
    /// Load fraction at which a character cannot move.
    pub encumbrance_hard_limit: f32,
    /// Heaviest load a swimmer can carry.
    pub max_swim_load_kg: f32,
    /// Injury definition for falls under `severe_fall_m`.
    pub fall_minor_injury: String,
    /// Injury definition for falls of `severe_fall_m` or more.
    pub fall_severe_injury: String,
    pub severe_fall_m: f32,
    pub stumble_injury: String,
    /// Chance that a stumble at a run or sprint causes an injury.
    pub stumble_injury_chance: f32,
}

impl Default for MovementConfig {
    fn default() -> Self {
        let gait = |speed_multiplier,
                    stamina_per_second,
                    fatigue_per_second,
                    stumble_multiplier| GaitConfig {
            speed_multiplier,
            stamina_per_second,
            fatigue_per_second,
            stumble_multiplier,
        };
        Self {
            walk: gait(1.0, 0.002, 0.000_02, 1.0),
            run: gait(2.2, -0.012, 0.000_06, 2.5),
            sprint: gait(4.0, -0.05, 0.000_15, 5.0),
            crouch: gait(0.45, 0.001, 0.000_03, 0.5),
            swim_speed_mps: 0.7,
            swim_stamina_per_second: -0.01,
            climb_speed_mps: 0.25,
            climb_stamina_per_second: -0.02,
            rest_stamina_per_second: 0.02,
            rest_fatigue_per_second: -0.000_01,
            exhausted_stamina: 0.05,
            terrain_cost: TerrainCostConfig {
                lowland: 1.0,
                hills: 1.3,
                mountain: 2.2,
                beach: 1.4,
                marsh: 2.8,
                shallows: 3.5,
                cliff: 6.0,
                swim: 4.0,
            },
            road_multiplier: 0.75,
            track_multiplier: 0.85,
            bridge_multiplier: 0.8,
            ford_cost: 2.5,
            forest_cost: 0.8,
            slope_cost_per_degree: 0.02,
            mud_cost: 1.2,
            snow_cost_per_10cm: 0.35,
            flood_cost_per_m: 4.0,
            door_open_seconds: 2.0,
            gate_open_seconds: 4.0,
            tile_capacity: 2,
            occupied_tile_cost: 4.0,
            max_path_expansions: 60_000,
            max_path_requests_per_tick: 8,
            repath_wait_seconds: 3.0,
            give_up_wait_seconds: 60.0,
            stumble_chance: 0.002,
            stumble_seconds: 2.0,
            climb_fall_chance: 0.01,
            encumbrance_soft_limit: 0.5,
            encumbrance_hard_limit: 1.5,
            max_swim_load_kg: 15.0,
            fall_minor_injury: "bruise".into(),
            fall_severe_injury: "fracture".into(),
            severe_fall_m: 4.0,
            stumble_injury: "sprain".into(),
            stumble_injury_chance: 0.05,
        }
    }
}

impl MovementConfig {
    pub fn validate(&self) -> SimResult<()> {
        for (name, g) in [
            ("walk", &self.walk),
            ("run", &self.run),
            ("sprint", &self.sprint),
            ("crouch", &self.crouch),
        ] {
            ensure(
                g.speed_multiplier.is_finite() && g.speed_multiplier > 0.0,
                || format!("movement.{name}.speed_multiplier must be positive"),
            )?;
        }
        let t = &self.terrain_cost;
        ensure(
            [
                t.lowland, t.hills, t.mountain, t.beach, t.marsh, t.shallows, t.cliff, t.swim,
            ]
            .iter()
            .all(|c| c.is_finite() && *c >= 0.5),
            || "movement.terrain_cost values must be at least 0.5".into(),
        )?;
        // Path heuristics assume no step is cheaper than a road over lowland.
        ensure(
            self.road_multiplier > 0.0
                && self.road_multiplier <= self.track_multiplier
                && self.track_multiplier <= 1.0
                && self.bridge_multiplier > 0.0,
            || "movement road/track/bridge multipliers must satisfy 0 < road <= track <= 1".into(),
        )?;
        ensure(self.tile_capacity >= 1, || {
            "movement.tile_capacity must be at least 1".into()
        })?;
        ensure(self.max_path_expansions >= 100, || {
            "movement.max_path_expansions must be at least 100".into()
        })?;
        ensure(self.max_path_requests_per_tick >= 1, || {
            "movement.max_path_requests_per_tick must be at least 1".into()
        })?;
        ensure(
            self.encumbrance_soft_limit > 0.0
                && self.encumbrance_soft_limit < self.encumbrance_hard_limit,
            || "movement encumbrance limits must satisfy 0 < soft < hard".into(),
        )?;
        ensure(
            (0.0..=1.0).contains(&self.stumble_chance)
                && (0.0..=1.0).contains(&self.climb_fall_chance)
                && (0.0..=1.0).contains(&self.stumble_injury_chance),
            || "movement chances must be between 0 and 1".into(),
        )
    }
}

/// Environment simulation resolution and rates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentConfig {
    /// Tiles per side of a weather cell.
    pub weather_cell_tiles: u32,
    /// Tiles per side of a ground/vegetation cell.
    pub ground_cell_tiles: u32,
    /// Tiles per side of a wildlife region.
    pub wildlife_region_tiles: u32,
    /// Precipitation falls as snow below this temperature.
    pub snow_threshold_c: f32,
    pub melt_mm_per_degree_hour: f32,
    /// River stage rise per mm of catchment surplus (last ~3 days).
    pub flood_rise_m_per_mm: f32,
    pub max_flood_stage_m: f32,
    /// Regional soil moisture below which drought days accumulate.
    pub drought_moisture: f32,
    pub drought_days: f32,
    pub grass_growth_per_day: f32,
    pub grass_kg_per_m2: f32,
    pub tree_regeneration_per_day: f32,
    pub contamination_decay_per_hour: f32,
    pub settlement_contamination_per_hour: f32,
    pub fertility_recovery_per_year: f32,
    /// Daily forage eaten by herbivores, as a fraction of body mass.
    pub forage_fraction_per_day: f32,
}

impl Default for EnvironmentConfig {
    fn default() -> Self {
        Self {
            weather_cell_tiles: 32,
            ground_cell_tiles: 4,
            wildlife_region_tiles: 64,
            snow_threshold_c: 0.5,
            melt_mm_per_degree_hour: 0.15,
            flood_rise_m_per_mm: 0.02,
            max_flood_stage_m: 3.0,
            drought_moisture: 0.18,
            drought_days: 14.0,
            grass_growth_per_day: 0.08,
            grass_kg_per_m2: 0.4,
            tree_regeneration_per_day: 0.0004,
            contamination_decay_per_hour: 0.02,
            settlement_contamination_per_hour: 0.004,
            fertility_recovery_per_year: 0.05,
            forage_fraction_per_day: 0.025,
        }
    }
}

impl EnvironmentConfig {
    pub fn validate(&self) -> SimResult<()> {
        ensure(
            self.ground_cell_tiles >= 1
                && self.weather_cell_tiles >= self.ground_cell_tiles
                && self
                    .weather_cell_tiles
                    .is_multiple_of(self.ground_cell_tiles),
            || {
                "environment cell sizes: weather_cell_tiles must be a multiple of ground_cell_tiles"
                    .into()
            },
        )?;
        ensure(self.wildlife_region_tiles >= self.ground_cell_tiles, || {
            "environment.wildlife_region_tiles must be at least ground_cell_tiles".into()
        })?;
        ensure(
            [
                self.melt_mm_per_degree_hour,
                self.flood_rise_m_per_mm,
                self.max_flood_stage_m,
                self.drought_days,
                self.grass_growth_per_day,
                self.grass_kg_per_m2,
                self.tree_regeneration_per_day,
                self.contamination_decay_per_hour,
                self.settlement_contamination_per_hour,
                self.fertility_recovery_per_year,
                self.forage_fraction_per_day,
            ]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0),
            || "environment rates must not be negative".into(),
        )?;
        ensure((0.0..=1.0).contains(&self.drought_moisture), || {
            "environment.drought_moisture must be between 0 and 1".into()
        })
    }
}

/// The complete, validated game configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameConfig {
    pub simulation: SimulationConfig,
    pub calendar: CalendarConfig,
    pub period: PeriodConfig,
    pub movement: MovementConfig,
    pub environment: EnvironmentConfig,
}

impl GameConfig {
    /// Validates every section and the cross-section constraints.
    pub fn validate(&self) -> SimResult<()> {
        self.simulation.validate()?;
        self.calendar.validate()?;
        self.period.validate()?;
        self.movement.validate()?;
        self.environment.validate()?;
        ensure(
            (self.period.earliest_year..=self.period.latest_year)
                .contains(&self.calendar.start.year),
            || {
                format!(
                    "calendar.start.year {} lies outside the configured period {}–{}",
                    self.calendar.start.year, self.period.earliest_year, self.period.latest_year
                )
            },
        )?;
        ensure(
            u64::from(self.simulation.game_seconds_per_tick) <= self.calendar.seconds_per_hour(),
            || "simulation.game_seconds_per_tick must not exceed one in-game hour".into(),
        )
    }
}

/// Loads, validates and owns the active [`GameConfig`].
#[derive(Debug, Clone)]
pub struct ConfigurationManager {
    directory: Option<PathBuf>,
    config: GameConfig,
}

impl ConfigurationManager {
    pub const SIMULATION_FILE: &'static str = "simulation.json";
    pub const CALENDAR_FILE: &'static str = "calendar.json";
    pub const PERIOD_FILE: &'static str = "period.json";
    pub const MOVEMENT_FILE: &'static str = "movement.json";
    pub const ENVIRONMENT_FILE: &'static str = "environment.json";

    /// Loads every configuration file from `directory` and validates the result.
    pub fn load(directory: &Path) -> SimResult<Self> {
        let config = GameConfig {
            simulation: load_or_default(&directory.join(Self::SIMULATION_FILE))?,
            calendar: load_or_default(&directory.join(Self::CALENDAR_FILE))?,
            period: load_or_default(&directory.join(Self::PERIOD_FILE))?,
            movement: load_or_default(&directory.join(Self::MOVEMENT_FILE))?,
            environment: load_or_default(&directory.join(Self::ENVIRONMENT_FILE))?,
        };
        config.validate()?;
        Ok(Self {
            directory: Some(directory.to_path_buf()),
            config,
        })
    }

    /// Wraps an in-memory configuration (tests, tools) after validating it.
    pub fn from_config(config: GameConfig) -> SimResult<Self> {
        config.validate()?;
        Ok(Self {
            directory: None,
            config,
        })
    }

    /// The active configuration.
    pub fn config(&self) -> &GameConfig {
        &self.config
    }

    /// The directory the configuration was loaded from, if any.
    pub fn directory(&self) -> Option<&Path> {
        self.directory.as_deref()
    }
}

fn load_or_default<T: DeserializeOwned + Default>(path: &Path) -> SimResult<T> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|source| SimError::Parse {
            path: path.to_path_buf(),
            source,
        }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            tracing::warn!(path = %path.display(), "configuration file missing; using built-in defaults");
            Ok(T::default())
        }
        Err(source) => Err(SimError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn ensure(condition: bool, message: impl FnOnce() -> String) -> SimResult<()> {
    if condition {
        Ok(())
    } else {
        Err(SimError::InvalidConfig(message()))
    }
}

fn ensure_unique<'a>(values: impl Iterator<Item = &'a str>, field: &str) -> SimResult<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value) {
            return Err(SimError::config(format!(
                "{field} contains duplicate entry '{value}'"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        GameConfig::default().validate().unwrap();
    }

    #[test]
    fn rejects_start_year_outside_period() {
        let mut config = GameConfig::default();
        config.calendar.start.year = 1500;
        assert!(matches!(config.validate(), Err(SimError::InvalidConfig(_))));
    }

    #[test]
    fn rejects_bad_calendar() {
        let mut config = CalendarConfig::default();
        config.months[0].days = 0;
        assert!(config.validate().is_err());

        let mut config = CalendarConfig::default();
        config.start.weekday = "Caturday".into();
        assert!(config.validate().is_err());

        let mut config = CalendarConfig::default();
        config.weekdays.push("Monday".into());
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_bad_speeds() {
        let mut config = SimulationConfig {
            speed_multipliers: vec![1.0, -2.0],
            ..SimulationConfig::default()
        };
        assert!(config.validate().is_err());
        config.speed_multipliers.clear();
        assert!(config.validate().is_err());
    }

    #[test]
    fn missing_files_fall_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ConfigurationManager::load(dir.path()).unwrap();
        assert_eq!(manager.config(), &GameConfig::default());
    }

    #[test]
    fn malformed_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(ConfigurationManager::SIMULATION_FILE),
            "{ nope",
        )
        .unwrap();
        assert!(matches!(
            ConfigurationManager::load(dir.path()),
            Err(SimError::Parse { .. })
        ));
    }
}
