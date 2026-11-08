//! Buildings, animals, crops, tree species and world generation presets.

use super::{require_months, require_non_negative, require_positive, require_range, require_unit};
use crate::content::ids::*;
use crate::content::registry::{Definition, Reference, ValidationEnv};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Base terrain class of a map tile.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum TerrainKind {
    Ocean,
    Shallows,
    Lake,
    River,
    Beach,
    Marsh,
    #[default]
    Lowland,
    Hills,
    Mountain,
    Cliff,
}

impl TerrainKind {
    pub const ALL: [Self; 10] = [
        Self::Ocean,
        Self::Shallows,
        Self::Lake,
        Self::River,
        Self::Beach,
        Self::Marsh,
        Self::Lowland,
        Self::Hills,
        Self::Mountain,
        Self::Cliff,
    ];

    pub const fn is_water(self) -> bool {
        matches!(
            self,
            Self::Ocean | Self::Shallows | Self::Lake | Self::River
        )
    }

    /// Water too deep to wade.
    pub const fn is_deep_water(self) -> bool {
        matches!(self, Self::Ocean | Self::Lake | Self::River)
    }
}

/// One cell of a building layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum StructureCell {
    Wall,
    Floor,
    Door,
    Gate,
    Fence,
}

impl StructureCell {
    fn from_char(c: char) -> Result<Option<Self>, char> {
        Ok(Some(match c {
            '#' => Self::Wall,
            '.' => Self::Floor,
            'D' => Self::Door,
            'G' => Self::Gate,
            'F' => Self::Fence,
            ' ' => return Ok(None),
            other => return Err(other),
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum BuildingCategory {
    Dwelling,
    Workshop,
    Agricultural,
    Religious,
    Defensive,
    Storage,
    Enclosure,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialAmount {
    pub material: MaterialId,
    pub kg: f32,
}

/// A building or enclosure with a tile layout.
///
/// `layout` rows use `#` wall, `.` floor, `D` door, `G` gate, `F` fence and
/// space for untouched ground. Row 0 is the northern edge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildingDef {
    pub id: BuildingId,
    pub name: String,
    pub category: BuildingCategory,
    pub layout: Vec<String>,
    #[serde(default)]
    pub materials: Vec<MaterialAmount>,
    pub build_hours: f32,
    #[serde(default)]
    pub technology: Option<TechnologyId>,
}

impl BuildingDef {
    /// Layout cells as `(dx, dy, cell)`; invalid characters are skipped
    /// (validation rejects them at load).
    pub fn cells(&self) -> Vec<(i32, i32, StructureCell)> {
        let mut cells = Vec::new();
        for (dy, row) in self.layout.iter().enumerate() {
            for (dx, c) in row.chars().enumerate() {
                if let Ok(Some(cell)) = StructureCell::from_char(c) {
                    cells.push((dx as i32, dy as i32, cell));
                }
            }
        }
        cells
    }

    /// Bounding size `(width, height)` in tiles.
    pub fn footprint(&self) -> (u32, u32) {
        let w = self
            .layout
            .iter()
            .map(|r| r.chars().count())
            .max()
            .unwrap_or(0);
        (w as u32, self.layout.len() as u32)
    }
}

impl Definition for BuildingDef {
    type Id = BuildingId;
    const CATEGORY: &'static str = "buildings";
    fn id(&self) -> &BuildingId {
        &self.id
    }
    fn references(&self) -> Vec<Reference> {
        let mut refs: Vec<_> = self
            .materials
            .iter()
            .map(|m| Reference::new("materials.material", "materials", &m.material))
            .collect();
        refs.extend(
            self.technology
                .iter()
                .map(|t| Reference::new("technology", "technologies", t)),
        );
        refs
    }
    fn validate(&self, env: &ValidationEnv<'_>) -> Vec<String> {
        let mut e = Vec::new();
        if self.layout.is_empty() {
            e.push("layout must have at least one row".into());
        }
        for row in &self.layout {
            for c in row.chars() {
                if let Err(bad) = StructureCell::from_char(c) {
                    e.push(format!("layout contains unknown character '{bad}'"));
                }
            }
        }
        let cells = self.cells();
        let enclosed = cells
            .iter()
            .any(|(_, _, c)| matches!(c, StructureCell::Wall | StructureCell::Fence));
        let entrance = cells
            .iter()
            .any(|(_, _, c)| matches!(c, StructureCell::Door | StructureCell::Gate));
        if enclosed && !entrance {
            e.push("a walled or fenced layout needs at least one door or gate".into());
        }
        for m in &self.materials {
            require_positive(&mut e, "materials.kg", m.kg);
        }
        require_positive(&mut e, "build_hours", self.build_hours);
        env.check_technology(&self.technology, &mut e);
        e
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Diet {
    /// Eats grass.
    Grazer,
    /// Eats shoots, leaves and mast in woodland.
    Browser,
    Omnivore,
    Carnivore,
}

/// Habitat preference weights, each 0 – 1.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HabitatPreference {
    pub forest: f32,
    pub open: f32,
    pub wetland: f32,
    pub upland: f32,
}

/// A disease circulating in a wild population (SIR dynamics).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimalDiseaseDef {
    pub id: AnimalDiseaseId,
    pub name: String,
    /// Transmission rate per day at full contact.
    pub transmission_per_day: f32,
    /// Daily mortality of infected animals.
    pub mortality_per_day: f32,
    /// Daily recovery rate of infected animals.
    pub recovery_per_day: f32,
    /// Chance per year of a spontaneous outbreak at carrying capacity.
    pub outbreak_chance_per_year: f32,
}

/// An animal species.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimalDef {
    pub id: AnimalId,
    pub name: String,
    pub body: BodyTemplateId,
    pub diet: Diet,
    /// Domestic stock is kept by people, not simulated as wildlife.
    #[serde(default)]
    pub domesticated: bool,
    pub mass_kg: f32,
    pub speed_mps: f32,
    pub habitat: HabitatPreference,
    /// Population density at ideal habitat, per km².
    pub density_per_km2: f32,
    /// Intrinsic annual population growth rate (r).
    pub annual_growth_rate: f32,
    /// 1-based months in which young are born.
    pub breeding_months: Vec<u32>,
    /// Home range of one animal; limits predator density.
    #[serde(default)]
    pub territory_km2: Option<f32>,
    #[serde(default)]
    pub prey: Vec<AnimalId>,
    /// Food required per day, for predators.
    #[serde(default)]
    pub food_kg_per_day: f32,
    /// Tendency to wander between regions, 0 – 1.
    #[serde(default)]
    pub migratory: f32,
    #[serde(default)]
    pub products: Vec<ItemDefId>,
    #[serde(default)]
    pub diseases: Vec<AnimalDiseaseDef>,
}

impl Definition for AnimalDef {
    type Id = AnimalId;
    const CATEGORY: &'static str = "animals";
    fn id(&self) -> &AnimalId {
        &self.id
    }
    fn references(&self) -> Vec<Reference> {
        let mut refs = vec![Reference::new("body", "body_templates", &self.body)];
        refs.extend(
            self.prey
                .iter()
                .map(|p| Reference::new("prey", "animals", p)),
        );
        refs.extend(
            self.products
                .iter()
                .map(|p| Reference::new("products", "items", p)),
        );
        refs
    }
    fn validate(&self, env: &ValidationEnv<'_>) -> Vec<String> {
        let mut e = Vec::new();
        require_positive(&mut e, "mass_kg", self.mass_kg);
        require_positive(&mut e, "speed_mps", self.speed_mps);
        require_unit(&mut e, "habitat.forest", self.habitat.forest);
        require_unit(&mut e, "habitat.open", self.habitat.open);
        require_unit(&mut e, "habitat.wetland", self.habitat.wetland);
        require_unit(&mut e, "habitat.upland", self.habitat.upland);
        require_non_negative(&mut e, "density_per_km2", self.density_per_km2);
        require_non_negative(&mut e, "annual_growth_rate", self.annual_growth_rate);
        require_months(
            &mut e,
            "breeding_months",
            &self.breeding_months,
            env.calendar,
        );
        if self.breeding_months.is_empty() {
            e.push("breeding_months must not be empty".into());
        }
        if let Some(t) = self.territory_km2 {
            require_positive(&mut e, "territory_km2", t);
        }
        require_unit(&mut e, "migratory", self.migratory);
        if self.prey.contains(&self.id) {
            e.push("an animal cannot prey on itself".into());
        }
        if !self.prey.is_empty() {
            require_positive(&mut e, "food_kg_per_day", self.food_kg_per_day);
        }
        let mut disease_ids = BTreeSet::new();
        for d in &self.diseases {
            if !disease_ids.insert(d.id.as_str()) {
                e.push(format!("duplicate disease '{}'", d.id));
            }
            require_non_negative(&mut e, "transmission_per_day", d.transmission_per_day);
            require_unit(&mut e, "mortality_per_day", d.mortality_per_day);
            require_unit(&mut e, "recovery_per_day", d.recovery_per_day);
            require_unit(
                &mut e,
                "outbreak_chance_per_year",
                d.outbreak_chance_per_year,
            );
        }
        e
    }
}

/// A field crop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CropDef {
    pub id: CropId,
    pub name: String,
    /// Harvested item.
    pub item: ItemDefId,
    /// 1-based sowing months.
    pub sow_months: Vec<u32>,
    pub growth_days: f32,
    pub yield_kg_per_acre: f32,
    pub min_temperature_c: f32,
    pub optimal_temperature_c: f32,
    /// Water requirement, 0 – 1.
    pub water_need: f32,
    /// Fertility drawn from the soil per harvest, 0 – 1.
    pub fertility_use: f32,
    #[serde(default)]
    pub frost_tolerant: bool,
    #[serde(default)]
    pub technology: Option<TechnologyId>,
}

impl Definition for CropDef {
    type Id = CropId;
    const CATEGORY: &'static str = "crops";
    fn id(&self) -> &CropId {
        &self.id
    }
    fn references(&self) -> Vec<Reference> {
        let mut refs = vec![Reference::new("item", "items", &self.item)];
        refs.extend(
            self.technology
                .iter()
                .map(|t| Reference::new("technology", "technologies", t)),
        );
        refs
    }
    fn validate(&self, env: &ValidationEnv<'_>) -> Vec<String> {
        let mut e = Vec::new();
        if self.sow_months.is_empty() {
            e.push("sow_months must not be empty".into());
        }
        require_months(&mut e, "sow_months", &self.sow_months, env.calendar);
        require_positive(&mut e, "growth_days", self.growth_days);
        require_positive(&mut e, "yield_kg_per_acre", self.yield_kg_per_acre);
        if self.optimal_temperature_c < self.min_temperature_c {
            e.push("optimal_temperature_c must not be below min_temperature_c".into());
        }
        require_unit(&mut e, "water_need", self.water_need);
        require_unit(&mut e, "fertility_use", self.fertility_use);
        env.check_technology(&self.technology, &mut e);
        e
    }
}

/// A tree species of the natural woodland.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeSpeciesDef {
    pub id: TreeSpeciesId,
    pub name: String,
    /// Item yielded when felled.
    pub timber: ItemDefId,
    #[serde(default)]
    pub deciduous: bool,
    /// Logistic canopy growth rate per year.
    pub growth_rate: f32,
    /// Preferred soil moisture range (0 – 1).
    pub moisture: [f32; 2],
    /// Preferred mean annual temperature range (°C).
    pub temperature_c: [f32; 2],
    pub max_elevation_m: f32,
    /// 1-based months in which leaves open and fall (deciduous only).
    #[serde(default)]
    pub leaf_out_month: u32,
    #[serde(default)]
    pub leaf_fall_month: u32,
}

impl TreeSpeciesDef {
    /// Suitability 0 – 1 for given conditions.
    pub fn suitability(&self, moisture: f32, mean_temp_c: f32, elevation_m: f32) -> f32 {
        fn band(v: f32, [lo, hi]: [f32; 2], soft: f32) -> f32 {
            if v < lo {
                (1.0 - (lo - v) / soft).max(0.0)
            } else if v > hi {
                (1.0 - (v - hi) / soft).max(0.0)
            } else {
                1.0
            }
        }
        if elevation_m > self.max_elevation_m {
            return 0.0;
        }
        band(moisture, self.moisture, 0.25) * band(mean_temp_c, self.temperature_c, 3.0)
    }

    /// Whether the canopy is in leaf in a 1-based month.
    pub fn in_leaf(&self, month: u32) -> bool {
        if !self.deciduous {
            return true;
        }
        if self.leaf_out_month <= self.leaf_fall_month {
            (self.leaf_out_month..self.leaf_fall_month).contains(&month)
        } else {
            month >= self.leaf_out_month || month < self.leaf_fall_month
        }
    }
}

impl Definition for TreeSpeciesDef {
    type Id = TreeSpeciesId;
    const CATEGORY: &'static str = "trees";
    fn id(&self) -> &TreeSpeciesId {
        &self.id
    }
    fn references(&self) -> Vec<Reference> {
        vec![Reference::new("timber", "items", &self.timber)]
    }
    fn validate(&self, env: &ValidationEnv<'_>) -> Vec<String> {
        let mut e = Vec::new();
        require_positive(&mut e, "growth_rate", self.growth_rate);
        require_range(&mut e, "moisture", self.moisture);
        require_range(&mut e, "temperature_c", self.temperature_c);
        require_positive(&mut e, "max_elevation_m", self.max_elevation_m);
        if self.deciduous {
            require_months(
                &mut e,
                "leaf months",
                &[self.leaf_out_month, self.leaf_fall_month],
                env.calendar,
            );
        }
        e
    }
}

/// Which map edge faces the sea.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CoastSide {
    None,
    North,
    South,
    East,
    West,
    Island,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ElevationSettings {
    pub max_m: f32,
    pub sea_depth_m: f32,
    /// Noise cycles across the map width for the base relief.
    pub base_frequency: f32,
    pub octaves: u32,
    pub persistence: f32,
    pub lacunarity: f32,
    /// Weight of ridged noise forming hill and mountain chains.
    pub ridge_strength: f32,
    pub ridge_frequency: f32,
    /// Domain warping, in map-width fractions.
    pub warp_strength: f32,
    /// Exponent shaping relief: >1 broadens lowlands.
    pub height_exponent: f32,
    /// Target fraction of the map below sea level.
    pub sea_coverage: f32,
    pub coast: CoastSide,
    /// How strongly land rises away from the coast side.
    pub coast_strength: f32,
    /// Elevation above which land counts as hills.
    pub hill_m: f32,
    /// Elevation above which land counts as mountain.
    pub mountain_m: f32,
    /// Slope above which a tile is a cliff.
    pub cliff_slope_deg: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HydrologySettings {
    pub annual_rainfall_mm: f32,
    /// Catchment area at which a stream becomes a mapped river.
    pub river_threshold_km2: f32,
    /// Depressions deeper than this become lakes.
    pub lake_min_depth_m: f32,
    /// Fraction of rainfall reaching streams.
    pub runoff_coefficient: f32,
    /// Rivers above this discharge widen to several tiles.
    pub wide_river_m3s: f32,
    /// Roads may ford rivers below this discharge; above, they bridge.
    pub ford_max_m3s: f32,
}

/// Monthly climate normals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonthClimate {
    pub mean_temp_c: f32,
    pub diurnal_range_c: f32,
    pub precipitation_mm: f32,
    pub rain_days: f32,
    pub wind_mps: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateSettings {
    /// One entry per calendar month.
    pub months: Vec<MonthClimate>,
    /// Direction the prevailing wind blows *from*, degrees (270 = west).
    pub prevailing_wind_from_deg: f32,
    pub lapse_rate_c_per_km: f32,
    /// Mean temperature difference from the southern to the northern edge.
    pub north_south_gradient_c: f32,
    /// Damping of diurnal range near the sea, 0 – 1.
    pub coastal_moderation: f32,
    /// Standard deviation of synoptic temperature anomalies.
    pub temperature_variability_c: f32,
    /// Typical lifetime of a weather pattern.
    pub weather_persistence_hours: f32,
    /// Extra rainfall on windward slopes per 100 m of rise.
    pub orographic_factor: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VegetationSettings {
    /// Target fraction of suitable land under woodland.
    pub forest_coverage: f32,
    pub forest_frequency: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceDistribution {
    pub resource: ResourceId,
    pub terrains: Vec<TerrainKind>,
    #[serde(default)]
    pub min_elevation_m: f32,
    #[serde(default = "f32_max")]
    pub max_elevation_m: f32,
    /// Fraction of eligible tiles holding a deposit.
    pub abundance: f32,
    /// Noise cycles across the map controlling clustering.
    pub cluster_frequency: f32,
    /// Units per deposit tile, inclusive range.
    pub amount: [u32; 2],
}

fn f32_max() -> f32 {
    f32::MAX
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettlementSettings {
    pub count: u32,
    pub town_count: u32,
    pub min_spacing_tiles: u32,
    pub water_radius_tiles: u32,
    pub names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoadSettings {
    /// Connections beyond the minimum spanning tree.
    pub extra_connections: u32,
    pub slope_cost: f32,
    pub forest_cost: f32,
    pub marsh_cost: f32,
    pub river_crossing_cost: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WildlifeSeed {
    pub animal: AnimalId,
    /// Initial population as a fraction of carrying capacity.
    pub abundance: f32,
}

/// Complete settings for generating a world.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldGenPresetDef {
    pub id: WorldPresetId,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub width: u32,
    pub height: u32,
    pub chunk_size: u32,
    pub tile_size_m: f32,
    pub elevation: ElevationSettings,
    pub hydrology: HydrologySettings,
    pub climate: ClimateSettings,
    pub vegetation: VegetationSettings,
    #[serde(default)]
    pub resources: Vec<ResourceDistribution>,
    pub settlements: SettlementSettings,
    pub roads: RoadSettings,
    #[serde(default)]
    pub wildlife: Vec<WildlifeSeed>,
}

impl Definition for WorldGenPresetDef {
    type Id = WorldPresetId;
    const CATEGORY: &'static str = "worldgen";
    fn id(&self) -> &WorldPresetId {
        &self.id
    }
    fn references(&self) -> Vec<Reference> {
        let mut refs: Vec<_> = self
            .resources
            .iter()
            .map(|r| Reference::new("resources.resource", "resources", &r.resource))
            .collect();
        refs.extend(
            self.wildlife
                .iter()
                .map(|w| Reference::new("wildlife.animal", "animals", &w.animal)),
        );
        refs
    }
    fn validate(&self, env: &ValidationEnv<'_>) -> Vec<String> {
        let mut e = Vec::new();
        if !(8..=128).contains(&self.chunk_size) {
            e.push("chunk_size must be between 8 and 128".into());
        } else if !self.width.is_multiple_of(self.chunk_size)
            || !self.height.is_multiple_of(self.chunk_size)
        {
            e.push("width and height must be multiples of chunk_size".into());
        }
        if !(32..=8192).contains(&self.width) || !(32..=8192).contains(&self.height) {
            e.push("width and height must be between 32 and 8192 tiles".into());
        }
        require_positive(&mut e, "tile_size_m", self.tile_size_m);
        let el = &self.elevation;
        require_positive(&mut e, "elevation.max_m", el.max_m);
        require_positive(&mut e, "elevation.base_frequency", el.base_frequency);
        require_positive(&mut e, "elevation.height_exponent", el.height_exponent);
        if el.octaves == 0 || el.octaves > 12 {
            e.push("elevation.octaves must be between 1 and 12".into());
        }
        if !(0.0..0.9).contains(&el.sea_coverage) {
            e.push("elevation.sea_coverage must be in [0, 0.9)".into());
        }
        if el.coast == CoastSide::None && el.sea_coverage > 0.0 {
            e.push("elevation.sea_coverage needs a coast side".into());
        }
        if !(el.hill_m < el.mountain_m && el.mountain_m <= el.max_m) {
            e.push("elevation thresholds must satisfy hill_m < mountain_m <= max_m".into());
        }
        let hy = &self.hydrology;
        require_positive(
            &mut e,
            "hydrology.annual_rainfall_mm",
            hy.annual_rainfall_mm,
        );
        require_positive(
            &mut e,
            "hydrology.river_threshold_km2",
            hy.river_threshold_km2,
        );
        require_unit(
            &mut e,
            "hydrology.runoff_coefficient",
            hy.runoff_coefficient,
        );
        if self.climate.months.len() != env.calendar.month_count() as usize {
            e.push(format!(
                "climate.months has {} entries but the calendar has {} months",
                self.climate.months.len(),
                env.calendar.month_count()
            ));
        }
        for m in &self.climate.months {
            require_non_negative(&mut e, "climate.precipitation_mm", m.precipitation_mm);
            require_non_negative(&mut e, "climate.diurnal_range_c", m.diurnal_range_c);
            require_non_negative(&mut e, "climate.wind_mps", m.wind_mps);
            if !(0.0..=31.0).contains(&m.rain_days) {
                e.push("climate.rain_days must be between 0 and 31".into());
            }
        }
        require_positive(
            &mut e,
            "climate.weather_persistence_hours",
            self.climate.weather_persistence_hours,
        );
        require_unit(
            &mut e,
            "climate.coastal_moderation",
            self.climate.coastal_moderation,
        );
        require_unit(
            &mut e,
            "vegetation.forest_coverage",
            self.vegetation.forest_coverage,
        );
        for r in &self.resources {
            require_unit(&mut e, "resources.abundance", r.abundance);
            if r.amount[0] > r.amount[1] || r.amount[1] == 0 {
                e.push(format!(
                    "resource '{}' amount must be a non-empty [min, max] range",
                    r.resource
                ));
            }
            if r.terrains.is_empty() {
                e.push(format!("resource '{}' lists no terrains", r.resource));
            }
        }
        let s = &self.settlements;
        if s.town_count > s.count {
            e.push("settlements.town_count exceeds settlements.count".into());
        }
        if (s.names.len() as u32) < s.count {
            e.push("settlements.names must provide at least `count` names".into());
        }
        for w in &self.wildlife {
            require_unit(&mut e, "wildlife.abundance", w.abundance);
            if let Some(animal) = env.content.animals().get(&w.animal)
                && animal.domesticated
            {
                e.push(format!("wildlife '{}' is a domesticated animal", w.animal));
            }
        }
        e
    }
}
