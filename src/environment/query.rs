//! Read-only queries that other systems use to react to the environment.
//!
//! These are the defined interfaces through which environmental conditions
//! influence the rest of the simulation:
//!
//! | Consumer | Query |
//! |----------|-------|
//! | movement / navigation | [`EnvironmentQuery::movement_penalty`] (mud, snow, flood) |
//! | characters' needs and health | [`EnvironmentQuery::conditions_at`] (temperature, wind chill, precipitation, water quality) |
//! | agriculture | [`EnvironmentQuery::growing_conditions`] |
//! | buildings | [`EnvironmentQuery::building_stress`] |
//! | economy, hunting, forestry | [`EnvironmentQuery::region_resources`] |

use crate::content::AnimalId;
use crate::core::config::MovementConfig;
use crate::simulation::WorldState;
use crate::world::TilePos;

/// Conditions a person standing on a tile experiences.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalConditions {
    pub temperature_c: f32,
    pub wind_chill_c: f32,
    pub wind_mps: f32,
    pub wind_from_deg: f32,
    pub humidity: f32,
    pub cloud: f32,
    pub precipitation_mm_h: f32,
    pub snowing: bool,
    pub snow_cm: f32,
    pub mud: f32,
    pub flood_depth_m: f32,
    pub soil_moisture: f32,
    pub surface_water_mm: f32,
    /// 0 (clean) – 1 (foul).
    pub water_contamination: f32,
    pub grass: f32,
    /// Canopy density of this tile, 0 – 1.
    pub tree_density: f32,
    pub in_drought: bool,
    pub river_stage_m: f32,
}

/// Conditions relevant to crops on a tile.
#[derive(Debug, Clone, PartialEq)]
pub struct GrowingConditions {
    pub smoothed_temp_c: f32,
    pub soil_moisture: f32,
    pub fertility: f32,
    pub frost: bool,
    pub snow_cover: bool,
    pub flooded: bool,
    pub drought: bool,
}

/// Loads acting on a building on a tile.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildingStress {
    /// Fresh snow ≈ 1 kg/m² per cm.
    pub snow_load_kg_m2: f32,
    pub flood_depth_m: f32,
    pub wind_mps: f32,
    pub ground_moisture: f32,
}

/// Natural resources of a wildlife region, for hunting, forestry and trade.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionResources {
    pub game: Vec<(AnimalId, f32)>,
    pub mean_canopy: f32,
    pub grass_kg: f32,
}

/// Borrowing view over the world for environment queries.
pub struct EnvironmentQuery<'a> {
    world: &'a WorldState,
}

impl<'a> EnvironmentQuery<'a> {
    pub fn new(world: &'a WorldState) -> Self {
        Self { world }
    }

    fn cells(&self, pos: TilePos) -> Option<(usize, usize)> {
        let statics = &self.world.derived.statics;
        let g = statics.ground_cell(pos)?;
        let w = statics.ground[g].weather_cell as usize;
        let env = &self.world.environment;
        (g < env.ground.len() && w < env.weather.cells.len()).then_some((g, w))
    }

    /// Tree density of an individual tile: the tile's initial canopy scaled
    /// by how its cell has changed since generation.
    pub fn tile_tree_density(&self, pos: TilePos) -> f32 {
        let Some((g, _)) = self.cells(pos) else {
            return 0.0;
        };
        let current = self.world.environment.ground.tree_density[g];
        let initial = self.world.derived.statics.ground[g].initial_tree_density;
        let tile = self
            .world
            .derived
            .map
            .tile(pos)
            .map_or(0.0, |t| t.tree_density);
        if initial > 0.01 {
            (tile * current / initial).clamp(0.0, 1.0)
        } else {
            current
        }
    }

    pub fn conditions_at(&self, pos: TilePos) -> Option<LocalConditions> {
        let (g, w) = self.cells(pos)?;
        let env = &self.world.environment;
        let cell = &env.weather.cells[w];
        let ground = &env.ground;
        let region = &env.weather.regions[w];
        let v = cell.wind_mps * 3.6; // km/h
        let wind_chill = if cell.temperature_c <= 10.0 && v > 4.8 {
            13.12 + 0.6215 * cell.temperature_c - 11.37 * v.powf(0.16)
                + 0.3965 * cell.temperature_c * v.powf(0.16)
        } else {
            cell.temperature_c
        };
        Some(LocalConditions {
            temperature_c: cell.temperature_c,
            wind_chill_c: wind_chill,
            wind_mps: cell.wind_mps,
            wind_from_deg: cell.wind_from_deg,
            humidity: cell.humidity,
            cloud: cell.cloud,
            precipitation_mm_h: cell.precipitation_mm_h,
            snowing: cell.snowing,
            snow_cm: ground.snow_cm[g],
            mud: ground.mud[g],
            flood_depth_m: ground.flood_m[g],
            soil_moisture: ground.soil_moisture[g],
            surface_water_mm: ground.surface_water_mm[g],
            water_contamination: ground.contamination[g],
            grass: ground.grass[g],
            tree_density: self.tile_tree_density(pos),
            in_drought: region.in_drought,
            river_stage_m: region.stage_m,
        })
    }

    /// Additional walking cost from ground conditions (0 on dry, clear ground).
    pub fn movement_penalty(&self, pos: TilePos, config: &MovementConfig) -> f32 {
        let Some((g, _)) = self.cells(pos) else {
            return 0.0;
        };
        let ground = &self.world.environment.ground;
        ground.mud[g] * config.mud_cost
            + ground.snow_cm[g] / 10.0 * config.snow_cost_per_10cm
            + ground.flood_m[g] * config.flood_cost_per_m
    }

    /// Flood depth on a tile.
    pub fn flood_depth(&self, pos: TilePos) -> f32 {
        self.cells(pos)
            .map_or(0.0, |(g, _)| self.world.environment.ground.flood_m[g])
    }

    pub fn growing_conditions(&self, pos: TilePos) -> Option<GrowingConditions> {
        let (g, w) = self.cells(pos)?;
        let env = &self.world.environment;
        let cell = &env.weather.cells[w];
        Some(GrowingConditions {
            smoothed_temp_c: cell.smoothed_temp_c,
            soil_moisture: env.ground.soil_moisture[g],
            fertility: env.ground.fertility[g],
            frost: cell.temperature_c < 0.0,
            snow_cover: env.ground.snow_cm[g] > 2.0,
            flooded: env.ground.flood_m[g] > 0.05,
            drought: env.weather.regions[w].in_drought,
        })
    }

    pub fn building_stress(&self, pos: TilePos) -> Option<BuildingStress> {
        let (g, w) = self.cells(pos)?;
        let env = &self.world.environment;
        Some(BuildingStress {
            snow_load_kg_m2: env.ground.snow_cm[g],
            flood_depth_m: env.ground.flood_m[g],
            wind_mps: env.weather.cells[w].wind_mps,
            ground_moisture: env.ground.soil_moisture[g],
        })
    }

    pub fn region_resources(&self, region: usize) -> Option<RegionResources> {
        let env = &self.world.environment;
        let statics = &self.world.derived.statics;
        let wr = env.wildlife.regions.get(region)?;
        let rs = statics.wildlife.get(region)?;
        let game = env
            .wildlife
            .species
            .iter()
            .zip(&wr.populations)
            .map(|(id, p)| (id.clone(), p.count))
            .collect();
        let cells = rs.ground_cells.len().max(1) as f32;
        let canopy = rs
            .ground_cells
            .iter()
            .map(|&c| env.ground.tree_density[c as usize])
            .sum::<f32>()
            / cells;
        Some(RegionResources {
            game,
            mean_canopy: canopy,
            grass_kg: 0.0,
        })
    }
}
