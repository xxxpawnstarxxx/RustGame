//! Weather: regional temperature, humidity, cloud, precipitation and wind.
//!
//! Each hour, every weather cell's weather is built from:
//!
//! * **monthly climate normals** of the preset, interpolated through the year;
//! * **regional differences** — north–south gradient, elevation lapse rate,
//!   coastal damping of the daily temperature swing;
//! * a **synoptic state** shared by the whole map (temperature and wetness
//!   anomalies, wind veer) evolving as a first-order autoregressive process
//!   with the preset's persistence time, so spells of weather last;
//! * a **spatial weather field** that drifts downwind, so showers and fronts
//!   cross the map rather than flickering in place;
//! * **orography** — windward slopes are wetter, lee slopes drier;
//! * the **diurnal cycle** (coolest before dawn, warmest mid-afternoon).
//!
//! Precipitation falls as snow below the configured threshold. A rain-day
//! frequency matching the monthly normal is obtained by thresholding the
//! wetness signal at the corresponding quantile.

use super::EnvironmentState;
use super::statics::EnvStatics;
use crate::content::{MonthClimate, WorldGenPresetDef};
use crate::core::config::EnvironmentConfig;
use crate::core::time::{Calendar, GameInstant};
use crate::world::noise::Noise2;
use serde::{Deserialize, Serialize};

/// Weather in one cell.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WeatherCell {
    pub temperature_c: f32,
    /// Exponentially smoothed temperature (≈ 24 h), used for growth and frost.
    pub smoothed_temp_c: f32,
    /// Relative humidity, 0 – 1.
    pub humidity: f32,
    /// Cloud cover, 0 – 1.
    pub cloud: f32,
    /// Liquid-equivalent precipitation rate.
    pub precipitation_mm_h: f32,
    pub snowing: bool,
    pub wind_mps: f32,
    /// Direction the wind blows from, degrees.
    pub wind_from_deg: f32,
}

impl WeatherCell {
    pub fn is_precipitating(&self) -> bool {
        self.precipitation_mm_h > 0.0
    }
}

/// Map-wide synoptic state.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Synoptic {
    pub temperature_anomaly_c: f32,
    pub wetness_anomaly: f32,
    pub wind_veer_rad: f32,
    /// Displacement of the spatial weather field, in map fractions.
    pub field_offset_u: f32,
    pub field_offset_v: f32,
}

/// Hydrological status of a weather cell's region.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RegionHydrology {
    /// Exponentially weighted runoff over the last few days, mm.
    pub surplus_mm: f32,
    /// River stage above normal, m.
    pub stage_m: f32,
    pub drought_days: f32,
    pub in_drought: bool,
    pub flooding: bool,
    /// Mean soil moisture of the region's land.
    pub mean_soil_moisture: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WeatherState {
    pub cols: u32,
    pub rows: u32,
    pub cells: Vec<WeatherCell>,
    pub regions: Vec<RegionHydrology>,
    pub synoptic: Synoptic,
    /// Seed of the spatial weather field (fixed per world).
    pub field_seed: u64,
}

impl WeatherState {
    pub fn initialize(statics: &EnvStatics) -> Self {
        let n = (statics.weather_cols * statics.weather_rows) as usize;
        Self {
            cols: statics.weather_cols,
            rows: statics.weather_rows,
            cells: vec![WeatherCell::default(); n],
            regions: vec![
                RegionHydrology {
                    mean_soil_moisture: 0.6,
                    ..RegionHydrology::default()
                };
                n
            ],
            synoptic: Synoptic::default(),
            field_seed: 0,
        }
    }
}

/// Climate normals at an instant, interpolated between month midpoints.
pub fn normals_at(preset: &WorldGenPresetDef, calendar: &Calendar, t: GameInstant) -> MonthClimate {
    let months = &preset.climate.months;
    let date = calendar.decompose(t);
    let len = calendar.month_length(date.month).max(1) as f32;
    let pos = (date.day as f32 + 0.5) / len - 0.5; // -0.5 .. 0.5 around mid-month
    let m = date.month as usize % months.len().max(1);
    let (a, b, frac) = if pos >= 0.0 {
        (m, (m + 1) % months.len(), pos)
    } else {
        ((m + months.len() - 1) % months.len(), m, 1.0 + pos)
    };
    let (ma, mb) = (&months[a], &months[b]);
    let lerp = |x: f32, y: f32| x + (y - x) * frac;
    MonthClimate {
        mean_temp_c: lerp(ma.mean_temp_c, mb.mean_temp_c),
        diurnal_range_c: lerp(ma.diurnal_range_c, mb.diurnal_range_c),
        precipitation_mm: lerp(ma.precipitation_mm, mb.precipitation_mm),
        rain_days: lerp(ma.rain_days, mb.rain_days),
        wind_mps: lerp(ma.wind_mps, mb.wind_mps),
    }
}

/// Standard normal deviate via Box–Muller.
pub(crate) fn gaussian(rng: &mut crate::core::rng::SimRng) -> f32 {
    let u1 = rng.unit().max(1e-12);
    let u2 = rng.unit();
    ((-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()) as f32
}

/// Approximate inverse standard normal CDF (logistic approximation).
fn inverse_normal(q: f32) -> f32 {
    let q = q.clamp(1e-4, 1.0 - 1e-4);
    0.5513 * (q / (1.0 - q)).ln()
}

/// Advances weather by `hours` and recomputes every cell for time `now`.
/// `hours == 0` only evaluates (used at world creation).
#[allow(clippy::too_many_arguments)]
pub fn step(
    state: &mut EnvironmentState,
    statics: &EnvStatics,
    preset: &WorldGenPresetDef,
    config: &EnvironmentConfig,
    calendar: &Calendar,
    now: GameInstant,
    hours: f32,
    tile_size_m: f32,
) {
    let climate = &preset.climate;
    let normals = normals_at(preset, calendar, now);
    let date = calendar.decompose(now);
    let hour = date.hour as f32 + date.minute as f32 / 60.0;
    let hours_per_day = calendar.hours_per_day() as f32;

    if state.weather.field_seed == 0 {
        state.weather.field_seed =
            crate::core::hash::mix64(state.rng.range_u32(1..u32::MAX) as u64) | 1;
    }
    let syn = &mut state.weather.synoptic;
    if hours > 0.0 {
        let phi = (-hours / climate.weather_persistence_hours).exp();
        let shock = (1.0 - phi * phi).sqrt();
        syn.temperature_anomaly_c = phi * syn.temperature_anomaly_c
            + shock * climate.temperature_variability_c * gaussian(&mut state.rng);
        syn.wetness_anomaly = phi * syn.wetness_anomaly + shock * 0.6 * gaussian(&mut state.rng);
        syn.wind_veer_rad = phi * syn.wind_veer_rad + shock * 0.6 * gaussian(&mut state.rng);
        // The weather field drifts downwind.
        let wind_from = climate.prevailing_wind_from_deg.to_radians() + syn.wind_veer_rad;
        let travel_m = normals.wind_mps * 3600.0 * hours * 0.4;
        let map_m = (statics.weather_cols.max(statics.weather_rows) * statics.weather_cell_tiles)
            as f32
            * tile_size_m;
        let toward = (-wind_from.sin(), wind_from.cos());
        syn.field_offset_u += toward.0 * travel_m / map_m.max(1.0);
        syn.field_offset_v += toward.1 * travel_m / map_m.max(1.0);
    }
    let syn = state.weather.synoptic.clone();
    let field_t = Noise2::new(state.weather.field_seed, 1);
    let field_w = Noise2::new(state.weather.field_seed, 2);

    let p_rain = (normals.rain_days / 30.4).clamp(0.01, 0.95);
    let threshold = 0.9 * inverse_normal(1.0 - p_rain);
    let rain_hours = (normals.rain_days.max(1.0) * 6.0).max(1.0);
    let base_intensity = normals.precipitation_mm / rain_hours;
    let wind_from_deg =
        (climate.prevailing_wind_from_deg + syn.wind_veer_rad.to_degrees()).rem_euclid(360.0);
    let wind_rad = wind_from_deg.to_radians();
    let upwind = (wind_rad.sin(), -wind_rad.cos());
    let smoothing = if hours > 0.0 {
        1.0 - (-hours / 24.0).exp()
    } else {
        1.0
    };

    for (i, cs) in statics.weather.iter().enumerate() {
        let u = f64::from(cs.centre_u - syn.field_offset_u) * 3.0;
        let v = f64::from(cs.centre_v - syn.field_offset_v) * 3.0;
        let ft = field_t.fbm(u, v, 2, 0.5, 2.0) as f32;
        let fw = field_w.fbm(u, v, 3, 0.5, 2.0) as f32;

        // Orographic lift from the upwind neighbour cell.
        let (cx, cy) = (
            (i as u32 % statics.weather_cols) as i32,
            (i as u32 / statics.weather_cols) as i32,
        );
        let (ux, uy) = (cx + upwind.0.round() as i32, cy + upwind.1.round() as i32);
        let upwind_elev = if ux >= 0
            && uy >= 0
            && ux < statics.weather_cols as i32
            && uy < statics.weather_rows as i32
        {
            statics.weather[(uy as u32 * statics.weather_cols + ux as u32) as usize]
                .mean_elevation_m
        } else {
            cs.mean_elevation_m
        };
        let rise = cs.mean_elevation_m - upwind_elev;
        let oro = if rise > 0.0 {
            climate.orographic_factor * rise / 100.0
        } else {
            climate.orographic_factor * rise / 200.0
        };

        let damping = 1.0 - climate.coastal_moderation * cs.coastal_fraction;
        let diurnal = 0.5
            * normals.diurnal_range_c
            * damping
            * (std::f32::consts::TAU * (hour - 15.0) / hours_per_day).cos();
        let temperature = normals.mean_temp_c
            + climate.north_south_gradient_c * (cs.latitude_fraction - 0.5)
            - climate.lapse_rate_c_per_km * cs.mean_elevation_m / 1000.0
            + syn.temperature_anomaly_c
            + diurnal
            + 0.8 * ft;
        let wet = syn.wetness_anomaly + 0.7 * fw + 0.5 * oro;
        let raining = wet > threshold;
        let intensity = if raining {
            base_intensity * (1.0 + (wet - threshold)).clamp(0.3, 3.0) * (1.0 + oro).clamp(0.5, 2.0)
        } else {
            0.0
        };
        let cell = &mut state.weather.cells[i];
        cell.temperature_c = temperature;
        cell.smoothed_temp_c = if hours > 0.0 {
            cell.smoothed_temp_c + (temperature - diurnal - cell.smoothed_temp_c) * smoothing
        } else {
            temperature - diurnal
        };
        cell.precipitation_mm_h = intensity;
        cell.snowing = raining && temperature < config.snow_threshold_c;
        cell.humidity = if raining {
            0.97
        } else {
            (0.78 + 0.12 * wet - 0.01 * (temperature - 10.0)).clamp(0.3, 0.97)
        };
        cell.cloud = if raining {
            1.0
        } else {
            ((wet - threshold + 1.4) / 1.6).clamp(0.0, 1.0)
        };
        cell.wind_mps = (normals.wind_mps * (1.0 + 0.3 * syn.wetness_anomaly.abs() + 0.15 * fw)
            + cs.mean_elevation_m * 0.004)
            .max(0.0);
        cell.wind_from_deg = wind_from_deg;
    }
}
