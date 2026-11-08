//! Wild animal populations per region.
//!
//! Populations are continuous counts per species and region (individual
//! animals become entities only when characters interact with them). Daily:
//!
//! * **carrying capacity** from habitat quality (forest, open grassland,
//!   wetland, upland weighted by the species' preferences), area, human
//!   disturbance near settlements, hunting wariness, food (grass for
//!   grazers) and, for predators, territory size;
//! * **reproduction** in breeding months, logistic towards capacity;
//!   over-capacity mortality outside them;
//! * **predation** with a saturating (Holling type II) response, shared
//!   across prey by biomass; predators starve and breed less when unfed;
//! * **grazing** removes grass; **browsing** suppresses woodland
//!   regeneration;
//! * **disease** (SIR) breaks out spontaneously with density-dependent
//!   probability, kills, confers temporary immunity, spreads with migrating
//!   animals and fouls water with carcasses;
//! * **migration** moves animals towards regions with spare capacity;
//! * **hunting pressure** decays over time.

use super::ground::GroundGrid;
use super::statics::EnvStatics;
use crate::content::{AnimalDef, AnimalId, ContentRegistry, Diet, WorldGenPresetDef};
use crate::core::config::EnvironmentConfig;
use crate::core::rng::SimRng;
use crate::world::map::NO_INDEX;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Population {
    pub count: f32,
    pub infected: f32,
    pub immune: f32,
    /// Index into the species' disease list, or `u16::MAX` when healthy.
    pub disease: u16,
    pub hunting_pressure: f32,
    /// Predators: share of food demand met yesterday (0 – 1).
    pub fed: f32,
    /// Carrying capacity computed on the last update (diagnostic).
    pub capacity: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WildlifeRegion {
    /// One entry per species in [`WildlifeState::species`].
    pub populations: Vec<Population>,
    /// Browsing pressure on tree regeneration, 0 – 1.
    pub browse_pressure: f32,
    /// Human disturbance from settlements, 0 – 1.
    pub disturbance: f32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WildlifeState {
    pub cols: u32,
    pub rows: u32,
    /// Wild species simulated, in id order.
    pub species: Vec<AnimalId>,
    pub regions: Vec<WildlifeRegion>,
}

/// Outbreak transitions produced by a step.
#[derive(Debug, Default, PartialEq)]
pub struct WildlifeTransitions {
    pub outbreaks_started: Vec<(u32, AnimalId, crate::content::AnimalDiseaseId)>,
    pub outbreaks_ended: Vec<(u32, AnimalId)>,
}

fn healthy() -> Population {
    Population {
        disease: NO_INDEX,
        fed: 1.0,
        ..Population::default()
    }
}

impl WildlifeState {
    pub fn initialize(
        statics: &EnvStatics,
        _preset: &WorldGenPresetDef,
        content: &ContentRegistry,
    ) -> Self {
        let species: Vec<AnimalId> = content
            .animals()
            .iter()
            .filter(|a| !a.domesticated)
            .map(|a| a.id.clone())
            .collect();
        let regions = (0..statics.wildlife.len())
            .map(|_| WildlifeRegion {
                populations: vec![healthy(); species.len()],
                browse_pressure: 0.0,
                disturbance: 0.0,
            })
            .collect();
        Self {
            cols: statics.wildlife_cols,
            rows: statics.wildlife_rows,
            species,
            regions,
        }
    }

    pub fn species_index(&self, id: &AnimalId) -> Option<usize> {
        self.species.iter().position(|s| s == id)
    }

    /// Total population of a species across all regions.
    pub fn total(&self, species: usize) -> f32 {
        self.regions
            .iter()
            .map(|r| r.populations[species].count)
            .sum()
    }
}

/// Habitat measures of a region from the current ground state.
struct Habitat {
    forest: f32,
    open: f32,
    wetland: f32,
    upland: f32,
    grass_kg: f32,
}

fn habitat(
    statics: &EnvStatics,
    grid: &GroundGrid,
    region: usize,
    config: &EnvironmentConfig,
    tile_area_m2: f32,
) -> Habitat {
    let rs = &statics.wildlife[region];
    let (mut forest, mut open, mut grass_kg) = (0f32, 0f32, 0f32);
    for &c in &rs.ground_cells {
        let c = c as usize;
        let cs = &statics.ground[c];
        let land_share = cs.land_tiles as f32 / (cs.land_tiles + cs.water_tiles).max(1) as f32;
        forest += grid.tree_density[c] * land_share;
        open += grid.grass[c] * (1.0 - grid.tree_density[c]) * land_share;
        grass_kg += grid.grass[c] * cs.land_tiles as f32 * tile_area_m2 * config.grass_kg_per_m2;
    }
    let n = rs.ground_cells.len().max(1) as f32;
    Habitat {
        forest: forest / n,
        open: open / n,
        wetland: rs.wetland_fraction,
        upland: rs.upland_fraction,
        grass_kg,
    }
}

fn habitat_score(def: &AnimalDef, h: &Habitat) -> f32 {
    let p = &def.habitat;
    let weight = (p.forest + p.open + p.wetland + p.upland).max(1e-3);
    ((p.forest * h.forest + p.open * h.open + p.wetland * h.wetland + p.upland * h.upland) / weight)
        .clamp(0.0, 1.0)
}

fn capacity(
    def: &AnimalDef,
    rs_land_km2: f32,
    h: &Habitat,
    region: &WildlifeRegion,
    pop: &Population,
) -> f32 {
    let mut k = def.density_per_km2 * rs_land_km2 * habitat_score(def, h);
    k *= 1.0 - 0.6 * region.disturbance;
    k *= 1.0 - 0.3 * pop.hunting_pressure.min(1.0);
    if let Some(territory) = def.territory_km2 {
        k = k.min(rs_land_km2 / territory * (0.5 + 0.5 * pop.fed));
    }
    k.max(0.0)
}

/// Sets initial populations to the preset's fraction of capacity.
pub fn seed_populations(
    state: &mut super::EnvironmentState,
    statics: &EnvStatics,
    preset: &WorldGenPresetDef,
    content: &ContentRegistry,
    config: &EnvironmentConfig,
) {
    let tile_area = (preset.tile_size_m * preset.tile_size_m).max(1.0);
    for seed in &preset.wildlife {
        let Some(s) = state.wildlife.species_index(&seed.animal) else {
            continue;
        };
        let Some(def) = content.animals().get(&seed.animal) else {
            continue;
        };
        for r in 0..state.wildlife.regions.len() {
            let h = habitat(statics, &state.ground, r, config, tile_area);
            let region = &state.wildlife.regions[r];
            let k = capacity(
                def,
                statics.wildlife[r].land_km2,
                &h,
                region,
                &region.populations[s],
            );
            let pop = &mut state.wildlife.regions[r].populations[s];
            pop.count = k * seed.abundance;
            pop.capacity = k;
        }
    }
}

/// Advances wildlife by `days`. `settlement_regions` lists regions with a
/// settlement and its disturbance strength (0 – 1).
#[allow(clippy::too_many_arguments)]
pub fn step(
    state: &mut WildlifeState,
    grid: &mut GroundGrid,
    statics: &EnvStatics,
    content: &ContentRegistry,
    config: &EnvironmentConfig,
    rng: &mut SimRng,
    month: u32,
    tile_size_m: f32,
    settlement_regions: &[(usize, f32)],
    days: f32,
) -> WildlifeTransitions {
    let mut transitions = WildlifeTransitions::default();
    if days <= 0.0 || state.regions.is_empty() {
        return transitions;
    }
    let tile_area = tile_size_m * tile_size_m;
    let defs: Vec<Option<&AnimalDef>> = state
        .species
        .iter()
        .map(|id| content.animals().get(id))
        .collect();
    for region in &mut state.regions {
        region.disturbance = 0.0;
    }
    for &(r, strength) in settlement_regions {
        if let Some(region) = state.regions.get_mut(r) {
            region.disturbance = region.disturbance.max(strength.clamp(0.0, 1.0));
        }
    }

    for r in 0..state.regions.len() {
        let land_km2 = statics.wildlife[r].land_km2;
        let h = habitat(statics, grid, r, config, tile_area);

        // Births, density-dependent mortality and capacity.
        for (s, def) in defs.iter().enumerate() {
            let Some(def) = def else { continue };
            let region = &state.regions[r];
            let k = capacity(def, land_km2, &h, region, &region.populations[s]);
            let k = if def.diet == Diet::Grazer {
                let need =
                    region.populations[s].count * def.mass_kg * config.forage_fraction_per_day;
                if need > 0.0 {
                    k * (h.grass_kg / need).min(1.0).sqrt()
                } else {
                    k
                }
            } else {
                k
            };
            let pop = &mut state.regions[r].populations[s];
            pop.capacity = k;
            let n = pop.count;
            if def.breeding_months.contains(&month) {
                let fed_factor = if def.prey.is_empty() { 1.0 } else { pop.fed };
                let r_day = def.annual_growth_rate / (def.breeding_months.len() as f32 * 30.4);
                if k > 0.0 {
                    pop.count += r_day * fed_factor * n * (1.0 - n / k) * days;
                } else {
                    pop.count -= 0.02 * n * days;
                }
            } else if n > k {
                pop.count -= 0.01 * (n - k) * days;
            }
            pop.count = pop.count.max(0.0);
            pop.hunting_pressure *= 0.98f32.powf(days);
        }

        // Predation.
        for (p, def) in defs.iter().enumerate() {
            let Some(def) = def else { continue };
            if def.prey.is_empty() {
                continue;
            }
            let predators = state.regions[r].populations[p].count;
            let demand = predators * def.food_kg_per_day * days;
            if demand <= 0.0 {
                continue;
            }
            let prey: Vec<(usize, f32)> = def
                .prey
                .iter()
                .filter_map(|id| state.species.iter().position(|s| s == id))
                .filter_map(|q| {
                    defs[q].map(|d| (q, state.regions[r].populations[q].count * d.mass_kg))
                })
                .collect();
            let available: f32 = prey.iter().map(|(_, kg)| kg).sum();
            let killed = if available > 0.0 {
                demand * available / (available + demand)
            } else {
                0.0
            };
            for (q, kg) in &prey {
                if *kg > 0.0
                    && let Some(d) = defs[*q]
                {
                    let share = killed * kg / available;
                    let pop = &mut state.regions[r].populations[*q];
                    pop.count = (pop.count - share / d.mass_kg).max(0.0);
                }
            }
            let fed = (killed / demand).clamp(0.0, 1.0);
            let pop = &mut state.regions[r].populations[p];
            pop.fed = fed;
            if fed < 0.5 {
                pop.count -= 0.01 * (0.5 - fed) * pop.count * days;
            }
        }

        // Grazing and browsing.
        let mut grazed_kg = 0f32;
        let mut browser_kg = 0f32;
        for (s, def) in defs.iter().enumerate() {
            let Some(def) = def else { continue };
            let kg = state.regions[r].populations[s].count * def.mass_kg;
            match def.diet {
                Diet::Grazer => grazed_kg += kg * config.forage_fraction_per_day * days,
                Diet::Browser => browser_kg += kg,
                Diet::Omnivore => {
                    grazed_kg += kg * config.forage_fraction_per_day * days * 0.3;
                    browser_kg += kg * 0.5;
                }
                Diet::Carnivore => {}
            }
        }
        if h.grass_kg > 0.0 && grazed_kg > 0.0 {
            let fraction = (grazed_kg / h.grass_kg).min(0.5);
            for &c in &statics.wildlife[r].ground_cells {
                grid.grass[c as usize] *= 1.0 - fraction;
            }
        }
        let hectares = (land_km2 * 100.0).max(1e-3);
        state.regions[r].browse_pressure = (browser_kg / hectares / 20.0).clamp(0.0, 1.0);

        // Disease.
        for (s, def) in defs.iter().enumerate() {
            let Some(def) = def else { continue };
            if def.diseases.is_empty() {
                continue;
            }
            let pop = &mut state.regions[r].populations[s];
            if pop.disease == NO_INDEX {
                if pop.count > 2.0 && pop.capacity > 0.0 {
                    let crowding = (pop.count / pop.capacity).min(2.0);
                    let index = rng.range_u32(0..def.diseases.len() as u32) as usize;
                    let d = &def.diseases[index];
                    let chance = f64::from(d.outbreak_chance_per_year / 365.0 * days * crowding);
                    if rng.chance(chance) {
                        pop.disease = index as u16;
                        pop.infected = (pop.count * 0.03).max(1.0).min(pop.count);
                        transitions.outbreaks_started.push((
                            r as u32,
                            def.id.clone(),
                            d.id.clone(),
                        ));
                    }
                }
                pop.immune *= 0.995f32.powf(days);
                continue;
            }
            let d = &def.diseases[pop.disease as usize];
            let n = pop.count.max(1e-3);
            let susceptible = (pop.count - pop.infected - pop.immune).max(0.0);
            let new_infections =
                (d.transmission_per_day * susceptible * pop.infected / n * days).min(susceptible);
            let deaths = (d.mortality_per_day * pop.infected * days).min(pop.infected);
            let recoveries = (d.recovery_per_day * pop.infected * days).min(pop.infected - deaths);
            pop.infected += new_infections - deaths - recoveries;
            pop.immune += recoveries;
            pop.count = (pop.count - deaths).max(0.0);
            pop.immune = pop.immune.min(pop.count);
            pop.infected = pop.infected.clamp(0.0, pop.count);
            if deaths > 0.0 {
                let fouling = deaths * def.mass_kg * 1e-4;
                for &c in &statics.wildlife[r].ground_cells {
                    let c = c as usize;
                    if statics.ground[c].has_river || statics.ground[c].water_tiles > 0 {
                        grid.contamination[c] = (grid.contamination[c] + fouling).min(1.0);
                    }
                }
            }
            if pop.infected < 0.05 {
                pop.infected = 0.0;
                pop.disease = NO_INDEX;
                transitions.outbreaks_ended.push((r as u32, def.id.clone()));
            }
        }
    }

    // Migration towards spare capacity (double-buffered for order independence).
    let mut delta: Vec<Vec<(f32, f32)>> =
        vec![vec![(0.0, 0.0); state.species.len()]; state.regions.len()];
    let mut carried_disease: Vec<Vec<u16>> =
        vec![vec![NO_INDEX; state.species.len()]; state.regions.len()];
    for (s, def) in defs.iter().enumerate() {
        let Some(def) = def else { continue };
        for r in 0..state.regions.len() {
            let here = &state.regions[r].populations[s];
            if here.count <= 0.01 {
                continue;
            }
            let crowding = if here.capacity > 0.0 {
                (here.count / here.capacity - 1.0).max(0.0)
            } else {
                1.0
            };
            let mobility = (def.migratory + crowding).min(1.0) * 0.02 * days;
            if mobility <= 0.0 {
                continue;
            }
            let spare_here = if here.capacity > 0.0 {
                1.0 - here.count / here.capacity
            } else {
                -1.0
            };
            for &nb in &statics.wildlife[r].neighbours {
                let there = &state.regions[nb as usize].populations[s];
                let spare_there = if there.capacity > 0.0 {
                    1.0 - there.count / there.capacity
                } else {
                    -1.0
                };
                let pull = (spare_there - spare_here).clamp(0.0, 1.0);
                if pull <= 0.0 {
                    continue;
                }
                let moved = here.count * mobility * pull * 0.25;
                let infected_share = if here.count > 0.0 {
                    here.infected / here.count
                } else {
                    0.0
                };
                delta[r][s].0 -= moved;
                delta[r][s].1 -= moved * infected_share;
                delta[nb as usize][s].0 += moved;
                delta[nb as usize][s].1 += moved * infected_share;
                if moved * infected_share > 0.01 && there.disease == NO_INDEX {
                    carried_disease[nb as usize][s] = here.disease;
                }
            }
        }
    }
    for r in 0..state.regions.len() {
        for s in 0..state.species.len() {
            let pop = &mut state.regions[r].populations[s];
            pop.count = (pop.count + delta[r][s].0).max(0.0);
            pop.infected = (pop.infected + delta[r][s].1).clamp(0.0, pop.count);
            if pop.disease == NO_INDEX && carried_disease[r][s] != NO_INDEX && pop.infected > 0.0 {
                pop.disease = carried_disease[r][s];
                if let (Some(def), Some(disease)) = (
                    defs[s],
                    defs[s].and_then(|d| d.diseases.get(carried_disease[r][s] as usize)),
                ) {
                    transitions.outbreaks_started.push((
                        r as u32,
                        def.id.clone(),
                        disease.id.clone(),
                    ));
                }
            } else if pop.disease == NO_INDEX {
                pop.infected = 0.0;
            }
        }
    }
    transitions
}
