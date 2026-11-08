//! Ground state per ground cell and its hourly water balance.
//!
//! Per hour and cell:
//!
//! 1. precipitation falls as snow (accumulates) or rain; snow melts by a
//!    degree-hour factor;
//! 2. water infiltrates according to soil type, the rest runs off;
//! 3. evapotranspiration (temperature, wind, humidity, vegetation) dries the
//!    soil; excess above field capacity drains, saturated soil overflows to
//!    surface water, which drains to streams;
//! 4. runoff feeds the region's catchment surplus; the river stage rises with
//!    it and floods cells whose height above drainage is below the stage
//!    (scaled by the size of the nearby river); floods leave silt;
//! 5. mud forms from wet soil and standing water (less on sand, none frozen);
//! 6. contamination decays, spreads on flood water and is added by
//!    settlements; fertility recovers towards its natural level;
//! 7. regional drought accumulates while mean soil moisture stays low.

use super::statics::EnvStatics;
use super::weather::WeatherState;
use crate::core::config::EnvironmentConfig;
use crate::world::TilePos;
use crate::world::map::{NO_INDEX, WorldMap};
use serde::{Deserialize, Serialize};

/// Dynamic ground and vegetation state, structure-of-arrays by ground cell.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GroundGrid {
    pub cols: u32,
    pub rows: u32,
    /// Fraction of field capacity (0 – 1).
    pub soil_moisture: Vec<f32>,
    pub surface_water_mm: Vec<f32>,
    pub snow_cm: Vec<f32>,
    pub flood_m: Vec<f32>,
    /// 0 – 1.
    pub mud: Vec<f32>,
    /// 0 – 1.
    pub fertility: Vec<f32>,
    /// 0 – 1 (water fouling).
    pub contamination: Vec<f32>,
    /// Grass biomass as a fraction of its maximum, 0 – 1.
    pub grass: Vec<f32>,
    /// Canopy density, 0 – 1.
    pub tree_density: Vec<f32>,
    /// Index into the map's tree palette, or `u16::MAX`.
    pub tree_species: Vec<u16>,
}

impl GroundGrid {
    pub fn initialize(map: &WorldMap, statics: &EnvStatics, _config: &EnvironmentConfig) -> Self {
        let n = statics.ground.len();
        let mut grid = Self {
            cols: statics.ground_cols,
            rows: statics.ground_rows,
            soil_moisture: vec![0.0; n],
            surface_water_mm: vec![0.0; n],
            snow_cm: vec![0.0; n],
            flood_m: vec![0.0; n],
            mud: vec![0.0; n],
            fertility: vec![0.0; n],
            contamination: vec![0.0; n],
            grass: vec![0.0; n],
            tree_density: vec![0.0; n],
            tree_species: vec![NO_INDEX; n],
        };
        for (i, cell) in statics.ground.iter().enumerate() {
            let land = cell.land_tiles > 0;
            grid.soil_moisture[i] = if land {
                (0.35 + 0.5 * cell.mean_moisture).min(0.95)
            } else {
                1.0
            };
            grid.fertility[i] = cell.base_fertility;
            grid.tree_density[i] = cell.initial_tree_density;
            grid.grass[i] = if land {
                (0.6 * (1.0 - 0.75 * cell.initial_tree_density)).max(0.05)
            } else {
                0.0
            };
            // Majority species of the cell's tiles.
            let (x0, y0, x1, y1) = statics.ground_cell_bounds(i);
            let mut counts: Vec<(u16, u32)> = Vec::new();
            for y in y0..y1 {
                for x in x0..x1 {
                    if let Some(t) = map.tile(TilePos::new(x, y))
                        && t.tree_species != NO_INDEX
                    {
                        match counts.iter_mut().find(|(s, _)| *s == t.tree_species) {
                            Some(entry) => entry.1 += 1,
                            None => counts.push((t.tree_species, 1)),
                        }
                    }
                }
            }
            grid.tree_species[i] = counts
                .iter()
                .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
                .map_or(NO_INDEX, |(s, _)| *s);
        }
        grid
    }

    pub fn len(&self) -> usize {
        self.soil_moisture.len()
    }

    pub fn is_empty(&self) -> bool {
        self.soil_moisture.is_empty()
    }
}

/// Region-level changes produced by one ground step.
#[derive(Debug, Default, PartialEq)]
pub struct GroundTransitions {
    pub drought_started: Vec<u32>,
    pub drought_ended: Vec<u32>,
    pub flood_started: Vec<u32>,
    pub flood_ended: Vec<u32>,
}

/// A settlement's effect on the ground cell at its centre.
pub struct PollutionSource {
    pub cell: usize,
    pub strength: f32,
}

/// Advances the ground water balance by `hours`.
// Cells are updated from several parallel arrays by index.
#[allow(clippy::needless_range_loop)]
pub fn step(
    grid: &mut GroundGrid,
    weather: &mut WeatherState,
    statics: &EnvStatics,
    config: &EnvironmentConfig,
    sources: &[PollutionSource],
    hours: f32,
) -> GroundTransitions {
    let mut transitions = GroundTransitions::default();
    if hours <= 0.0 || grid.is_empty() {
        return transitions;
    }
    let decay_surface = 0.85f32.powf(hours);
    let mut region_runoff = vec![0f32; weather.regions.len()];
    let mut region_cells = vec![0u32; weather.regions.len()];

    for (i, cs) in statics.ground.iter().enumerate() {
        let w = &weather.cells[cs.weather_cell as usize];
        if cs.land_tiles == 0 {
            grid.soil_moisture[i] = 1.0;
            grid.snow_cm[i] = 0.0;
            continue;
        }
        let soil = cs.dominant_soil;
        let capacity = soil.field_capacity_mm();
        let precip = w.precipitation_mm_h * hours;
        let mut water_in = 0.0;
        if precip > 0.0 && w.snowing {
            grid.snow_cm[i] += precip; // 1 mm water ≈ 1 cm fresh snow
        } else {
            water_in += precip;
        }
        if w.temperature_c > 0.0 && grid.snow_cm[i] > 0.0 {
            let melt =
                (config.melt_mm_per_degree_hour * w.temperature_c * hours).min(grid.snow_cm[i]);
            grid.snow_cm[i] -= melt;
            water_in += melt;
        }
        let infiltrated = water_in * soil.infiltration();
        let mut runoff = water_in - infiltrated;
        let mut m = grid.soil_moisture[i] + infiltrated / capacity;

        // Evapotranspiration.
        let frozen = w.temperature_c < -1.0 || grid.snow_cm[i] > 1.0;
        let pet = (0.012 * (w.temperature_c + 5.0)).max(0.0)
            * (1.0 + 0.04 * w.wind_mps)
            * (1.0 - w.humidity)
            * 2.0;
        let cover = 0.5 + 0.5 * (grid.grass[i] + grid.tree_density[i]).min(1.0);
        let et = pet * hours * (m / 0.5).min(1.0) * cover * if frozen { 0.2 } else { 1.0 };
        m -= et / capacity;

        // Drainage and saturation overflow.
        if m > 0.6 {
            m -= soil.drainage_per_hour() * (m - 0.6) * hours;
        }
        if m > 1.0 {
            runoff += (m - 1.0) * capacity;
            m = 1.0;
        }
        m = m.max(0.0);
        // Standing water soaks in as capacity allows, otherwise drains away.
        let mut surface = grid.surface_water_mm[i] + runoff;
        let soak = ((1.0 - m) * capacity).min(surface * 0.1 * hours).max(0.0);
        m += soak / capacity;
        surface = (surface - soak) * decay_surface;
        grid.soil_moisture[i] = m.min(1.0);
        grid.surface_water_mm[i] = surface;

        let r = cs.weather_cell as usize;
        region_runoff[r] += runoff;
        region_cells[r] += 1;

        // Mud.
        let wet = ((m - 0.55) * 2.2 + surface / 30.0) * soil.mud_factor();
        grid.mud[i] = if w.temperature_c < -2.0 && grid.snow_cm[i] < 1.0 {
            (wet * 0.1).clamp(0.0, 1.0)
        } else {
            wet.clamp(0.0, 1.0)
        };

        // Contamination decay and fertility recovery.
        grid.contamination[i] *= (1.0 - config.contamination_decay_per_hour).powf(hours);
        let recovery = config.fertility_recovery_per_year / 8766.0 * hours;
        grid.fertility[i] += (cs.base_fertility - grid.fertility[i]) * recovery.min(1.0);
    }
    for source in sources {
        if let Some(c) = grid.contamination.get_mut(source.cell) {
            *c = (*c + config.settlement_contamination_per_hour * source.strength * hours).min(1.0);
        }
    }

    // Regional river stage, floods and drought.
    let surplus_decay = (-hours / 72.0).exp();
    for (r, region) in weather.regions.iter_mut().enumerate() {
        let mean_runoff = region_runoff[r] / region_cells[r].max(1) as f32;
        region.surplus_mm = region.surplus_mm * surplus_decay + mean_runoff;
        region.stage_m =
            (region.surplus_mm * config.flood_rise_m_per_mm).min(config.max_flood_stage_m);
    }
    let mut region_flooded = vec![false; weather.regions.len()];
    let mut region_moisture = vec![0f32; weather.regions.len()];
    let previous_flood = grid.flood_m.clone();
    for (i, cs) in statics.ground.iter().enumerate() {
        let r = cs.weather_cell as usize;
        if cs.land_tiles > 0 {
            region_moisture[r] += grid.soil_moisture[i];
        }
        let river_factor = (1.0 + (1.0 + cs.nearby_river_flow_m3s).ln()) / 3.0;
        let depth = if cs.nearby_river_flow_m3s > 0.0 && cs.land_tiles > 0 {
            (weather.regions[r].stage_m * river_factor - cs.min_drainage_height_m).max(0.0)
        } else {
            0.0
        };
        grid.flood_m[i] = depth;
        if depth > 0.05 {
            region_flooded[r] = true;
            grid.fertility[i] = (grid.fertility[i] + 0.0005 * hours * depth.min(1.0)).min(1.0);
            grid.surface_water_mm[i] = grid.surface_water_mm[i].max(depth * 1000.0);
            grid.soil_moisture[i] = 1.0;
        }
    }
    // Flood water carries contamination between neighbouring flooded cells.
    for i in 0..grid.len() {
        if grid.flood_m[i] > 0.05 && previous_flood[i] > 0.05 {
            let spread = statics
                .ground_neighbours(i)
                .filter(|&j| grid.flood_m[j] > 0.05)
                .map(|j| grid.contamination[j])
                .fold(0f32, f32::max);
            grid.contamination[i] = grid.contamination[i].max(spread * 0.5);
        }
    }
    for (r, region) in weather.regions.iter_mut().enumerate() {
        region.mean_soil_moisture = region_moisture[r] / region_cells[r].max(1) as f32;
        if region.mean_soil_moisture < config.drought_moisture {
            region.drought_days += hours / 24.0;
        } else {
            region.drought_days = (region.drought_days - 2.0 * hours / 24.0).max(0.0);
        }
        if !region.in_drought && region.drought_days >= config.drought_days {
            region.in_drought = true;
            transitions.drought_started.push(r as u32);
        } else if region.in_drought && region.mean_soil_moisture > config.drought_moisture * 1.5 {
            region.in_drought = false;
            region.drought_days = 0.0;
            transitions.drought_ended.push(r as u32);
        }
        if region_flooded[r] != region.flooding {
            region.flooding = region_flooded[r];
            if region.flooding {
                transitions.flood_started.push(r as u32);
            } else {
                transitions.flood_ended.push(r as u32);
            }
        }
    }
    transitions
}
