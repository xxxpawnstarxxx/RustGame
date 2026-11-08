//! Grass and woodland growth, regeneration, die-back and felling.
//!
//! Daily, per ground cell:
//!
//! * **grass** grows logistically when the smoothed temperature exceeds 5 °C
//!   and the soil is moist, under a ceiling reduced by canopy shade; it dies
//!   back in frost and drought and is eaten by grazing animals (see
//!   [`super::wildlife`]);
//! * **trees** thicken at their species' growth rate scaled by climatic
//!   suitability, suffer mortality in severe drought, and spread into
//!   neighbouring open cells (seed dispersal), held back by deer browsing;
//! * **felling** removes canopy immediately (see [`fell`]).

use super::ground::GroundGrid;
use super::statics::EnvStatics;
use super::weather::WeatherState;
use crate::content::{ContentRegistry, TreeSpeciesDef};
use crate::core::config::EnvironmentConfig;
use crate::world::map::{NO_INDEX, WorldMap};

fn smoothstep(lo: f32, hi: f32, x: f32) -> f32 {
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn species<'a>(
    map: &WorldMap,
    content: &'a ContentRegistry,
    index: u16,
) -> Option<&'a TreeSpeciesDef> {
    map.tree_palette
        .get(index as usize)
        .and_then(|id| content.trees().get(id))
}

/// Advances vegetation by `days`. `browse_pressure` is indexed by wildlife region (0 – 1).
#[allow(clippy::too_many_arguments)]
pub fn step(
    grid: &mut GroundGrid,
    weather: &WeatherState,
    statics: &EnvStatics,
    map: &WorldMap,
    content: &ContentRegistry,
    config: &EnvironmentConfig,
    browse_pressure: &[f32],
    days: f32,
) {
    if days <= 0.0 || grid.is_empty() {
        return;
    }
    let previous_density = grid.tree_density.clone();
    let previous_species = grid.tree_species.clone();
    for (i, cs) in statics.ground.iter().enumerate() {
        if cs.land_tiles == 0 {
            continue;
        }
        let w = &weather.cells[cs.weather_cell as usize];
        let m = grid.soil_moisture[i];
        let snow_covered = grid.snow_cm[i] > 2.0;
        let g_temp = ((w.smoothed_temp_c - 5.0) / 15.0).clamp(0.0, 1.0);
        let g_water = smoothstep(0.12, 0.45, m);
        let growth = if snow_covered { 0.0 } else { g_temp * g_water };

        // Grass.
        let ceiling = (1.0 - 0.75 * grid.tree_density[i]).max(0.05)
            * (cs.land_tiles as f32 / (cs.land_tiles + cs.water_tiles) as f32);
        let mut grass = grid.grass[i].max(0.02);
        grass += config.grass_growth_per_day * growth * grass * (1.0 - grass / ceiling) * days;
        // Turnover of old growth, faster once the grass is dormant in the cold.
        let dormant = w.smoothed_temp_c < 5.0 || snow_covered;
        grass -= (0.003 + if dormant { 0.01 } else { 0.0 }) * days * grass;
        if w.smoothed_temp_c < 0.0 {
            grass -= 0.01 * days * grass;
        }
        if m < 0.08 {
            grass -= 0.02 * days * grass;
        }
        if grid.flood_m[i] > 0.3 {
            grass -= 0.05 * days * grass;
        }
        grid.grass[i] = grass.clamp(0.0, 1.0);

        // Trees.
        let d = grid.tree_density[i];
        if d > 0.0 {
            if let Some(sp) = species(map, content, grid.tree_species[i]) {
                let suit = sp.suitability(cs.mean_moisture, cs.mean_temp_c, cs.mean_elevation_m);
                let rate = sp.growth_rate / 365.0 * days * suit * (0.3 + 0.7 * growth);
                let mut nd = d + rate * d * (1.0 - d);
                if m < 0.05 {
                    nd -= 0.0005 * days;
                }
                grid.tree_density[i] = nd.clamp(0.0, 1.0);
                if grid.tree_density[i] < 0.005 {
                    grid.tree_density[i] = 0.0;
                    grid.tree_species[i] = NO_INDEX;
                }
            }
        } else if growth > 0.0 {
            // Colonisation from the densest neighbouring woodland.
            let best = statics
                .ground_neighbours(i)
                .filter(|&j| previous_species[j] != NO_INDEX)
                .max_by(|&a, &b| {
                    previous_density[a]
                        .total_cmp(&previous_density[b])
                        .then(b.cmp(&a))
                });
            if let Some(j) = best
                && previous_density[j] > 0.4
                && let Some(sp) = species(map, content, previous_species[j])
            {
                let suit = sp.suitability(cs.mean_moisture, cs.mean_temp_c, cs.mean_elevation_m);
                let browse = browse_pressure
                    .get(cs.wildlife_region as usize)
                    .copied()
                    .unwrap_or(0.0);
                let colonise = config.tree_regeneration_per_day
                    * days
                    * previous_density[j]
                    * suit
                    * (1.0 - browse)
                    * growth;
                if colonise > 0.0 {
                    grid.tree_density[i] = colonise.min(0.05);
                    grid.tree_species[i] = previous_species[j];
                }
            }
        }
    }
}

/// Removes up to `fraction` of the canopy of a cell; returns density removed.
pub fn fell(grid: &mut GroundGrid, cell: usize, fraction: f32) -> f32 {
    let Some(d) = grid.tree_density.get_mut(cell) else {
        return 0.0;
    };
    let removed = *d * fraction.clamp(0.0, 1.0);
    *d -= removed;
    if *d < 0.005 {
        *d = 0.0;
        grid.tree_species[cell] = NO_INDEX;
    }
    removed
}
