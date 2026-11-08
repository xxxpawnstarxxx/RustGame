//! Procedural world generation.
//!
//! Generation is a pure function of `(preset, seed, content)`: the same
//! inputs always produce the same map, which is what allows saves to store
//! only the seed and a fingerprint instead of every tile.
//!
//! Pipeline:
//!
//! 1. **Relief** — warped fractal noise plus ridged mountain chains masked
//!    into ranges, tilted towards the configured coast; sea level set by
//!    quantile so the requested fraction of the map is sea.
//! 2. **Precipitation** — orographic enhancement on windward slopes, rain
//!    shadow in the lee.
//! 3. **Hydrology** — priority-flood depression filling; deep depressions
//!    become lakes; D8 flow routing; catchment accumulation weighted by
//!    rainfall; rivers above a catchment threshold, widened by discharge;
//!    height above nearest drainage for flood risk and wetness.
//! 4. **Slope, climate, terrain classes, landforms and soils.**
//! 5. **Woodland** — species by climatic suitability, extent by coverage.
//! 6. **Resources** — deposits per terrain/elevation rule, clustered.
//! 7. **Settlements** — water, fertility, flatness, flood safety, spacing.
//! 8. **Roads** — MST plus extra links, A* over terrain cost, with fords and
//!    bridges at river crossings.

mod placement;
mod terrain;

use crate::content::{ContentRegistry, TerrainKind, WorldGenPresetDef};
use crate::core::error::{SimError, SimResult};
use crate::world::coords::{DIRS8, TilePos};
use crate::world::map::{
    Crossing, Landform, MapInfo, NO_FLOW, NO_INDEX, RoadKind, SettlementKind, SoilKind, WorldMap,
};
use serde::{Deserialize, Serialize};

/// A settlement location chosen by generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettlementSite {
    pub name: String,
    pub kind: SettlementKind,
    pub centre: TilePos,
    pub score: f32,
}

/// Output of world generation.
#[derive(Debug, Clone)]
pub struct GeneratedWorld {
    pub map: WorldMap,
    pub sites: Vec<SettlementSite>,
    pub info: MapInfo,
}

/// Working fields, one value per tile, row-major.
pub(crate) struct Fields {
    pub w: usize,
    pub h: usize,
    pub elevation: Vec<f32>,
    pub filled: Vec<f32>,
    pub precip: Vec<f32>,
    pub flow_dir: Vec<u8>,
    pub catchment: Vec<f32>,
    pub flow: Vec<f32>,
    pub hand: Vec<f32>,
    pub slope: Vec<f32>,
    pub moisture: Vec<f32>,
    pub temp: Vec<f32>,
    pub terrain: Vec<TerrainKind>,
    pub landform: Vec<Landform>,
    pub soil: Vec<SoilKind>,
    pub fertility: Vec<f32>,
    pub tree_species: Vec<u16>,
    pub tree_density: Vec<f32>,
    pub resource: Vec<u16>,
    pub resource_amount: Vec<u32>,
    pub road: Vec<RoadKind>,
    pub crossing: Vec<Crossing>,
}

impl Fields {
    fn new(w: usize, h: usize) -> Self {
        let n = w * h;
        Self {
            w,
            h,
            elevation: vec![0.0; n],
            filled: vec![0.0; n],
            precip: vec![1.0; n],
            flow_dir: vec![NO_FLOW; n],
            catchment: vec![0.0; n],
            flow: vec![0.0; n],
            hand: vec![0.0; n],
            slope: vec![0.0; n],
            moisture: vec![0.0; n],
            temp: vec![0.0; n],
            terrain: vec![TerrainKind::Lowland; n],
            landform: vec![Landform::Plain; n],
            soil: vec![SoilKind::Loam; n],
            fertility: vec![0.0; n],
            tree_species: vec![NO_INDEX; n],
            tree_density: vec![0.0; n],
            resource: vec![NO_INDEX; n],
            resource_amount: vec![0; n],
            road: vec![RoadKind::None; n],
            crossing: vec![Crossing::None; n],
        }
    }

    pub fn len(&self) -> usize {
        self.w * self.h
    }

    pub fn idx(&self, x: usize, y: usize) -> usize {
        y * self.w + x
    }

    pub fn xy(&self, i: usize) -> (usize, usize) {
        (i % self.w, i / self.w)
    }

    pub fn index(&self, x: i32, y: i32) -> Option<usize> {
        (x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h)
            .then(|| self.idx(x as usize, y as usize))
    }

    /// In-bounds neighbours with step distance (1 or √2).
    pub fn neighbours(&self, i: usize) -> impl Iterator<Item = (usize, f32)> + '_ {
        self.neighbours_dir(i).map(|(_, n)| n)
    }

    /// In-bounds neighbours with their [`DIRS8`] index.
    pub fn neighbours_dir(&self, i: usize) -> impl Iterator<Item = (u8, (usize, f32))> + '_ {
        let (x, y) = self.xy(i);
        DIRS8.iter().enumerate().filter_map(move |(d, (dx, dy))| {
            let j = self.index(x as i32 + dx, y as i32 + dy)?;
            let dist = if *dx != 0 && *dy != 0 {
                std::f32::consts::SQRT_2
            } else {
                1.0
            };
            Some((d as u8, (j, dist)))
        })
    }

    pub fn downstream(&self, i: usize) -> Option<usize> {
        let (dx, dy) = DIRS8.get(self.flow_dir[i] as usize)?;
        let (x, y) = self.xy(i);
        self.index(x as i32 + dx, y as i32 + dy)
    }
}

/// Generates a world map and settlement sites.
pub fn generate(
    preset: &WorldGenPresetDef,
    seed: u64,
    content: &ContentRegistry,
) -> SimResult<GeneratedWorld> {
    let (w, h) = (preset.width as usize, preset.height as usize);
    if w == 0 || h == 0 || preset.chunk_size == 0 {
        return Err(SimError::WorldGeneration(
            "map dimensions must be positive".into(),
        ));
    }
    let started = std::time::Instant::now();
    let mut f = Fields::new(w, h);
    let tile_area_km2 = (preset.tile_size_m * preset.tile_size_m) / 1e6;

    terrain::elevation(&mut f, preset, seed);
    terrain::precipitation(&mut f, preset, seed);
    terrain::hydrology(&mut f, preset, tile_area_km2);
    terrain::slopes(&mut f, preset.tile_size_m);
    terrain::climate(&mut f, preset, seed);
    terrain::classify(&mut f, preset);
    terrain::soils(&mut f, seed);
    terrain::woodland(&mut f, preset, content, seed);
    let resource_palette = placement::resources(&mut f, preset, content, seed);
    let sites = placement::settlements(&mut f, preset, seed);
    placement::clear_around_settlements(&mut f, &sites);
    placement::roads(&mut f, preset, &sites);

    let mut map = WorldMap::new(
        preset.width,
        preset.height,
        preset.chunk_size,
        preset.tile_size_m,
    );
    map.resource_palette = resource_palette;
    map.tree_palette = content.trees().ids().cloned().collect();
    for y in 0..h {
        for x in 0..w {
            let i = f.idx(x, y);
            let tile = map
                .tile_mut(TilePos::new(x as i32, y as i32))
                .expect("position inside generated bounds");
            tile.elevation_m = f.elevation[i];
            tile.terrain = f.terrain[i];
            tile.landform = f.landform[i];
            tile.soil = f.soil[i];
            tile.fertility = f.fertility[i];
            tile.slope_deg = f.slope[i];
            tile.drainage_height_m = f.hand[i];
            tile.catchment_km2 = f.catchment[i];
            tile.river_flow_m3s = f.flow[i];
            tile.flow_dir = f.flow_dir[i];
            tile.moisture = f.moisture[i];
            tile.mean_temp_c = f.temp[i];
            tile.road = f.road[i];
            tile.crossing = f.crossing[i];
            tile.resource = f.resource[i];
            tile.resource_amount = f.resource_amount[i];
            tile.tree_species = f.tree_species[i];
            tile.tree_density = f.tree_density[i];
        }
    }
    let info = MapInfo {
        preset: preset.id.clone(),
        seed,
        width: preset.width,
        height: preset.height,
        fingerprint: map.fingerprint(),
    };
    tracing::info!(
        preset = %preset.id,
        seed,
        width = w,
        height = h,
        settlements = sites.len(),
        millis = started.elapsed().as_millis() as u64,
        "world generated"
    );
    Ok(GeneratedWorld { map, sites, info })
}
