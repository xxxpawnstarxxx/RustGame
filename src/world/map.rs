//! The generated, static world map, stored in chunks.
//!
//! The map holds what world generation decided and nothing that changes
//! during play: elevation, terrain, landform, soil, hydrology, climate
//! normals, roads, natural deposits and the *initial* woodland. Everything
//! that changes (moisture, snow, vegetation, floods, structures) lives in
//! separate layers, so the static map can be regenerated deterministically
//! from its seed and preset instead of being written into every save.

use crate::content::{ResourceId, TerrainKind, TreeSpeciesId, WorldPresetId};
use crate::core::hash::{Fnv64, hex64};
use crate::world::coords::{ChunkPos, DIRS8, TilePos};
use serde::{Deserialize, Serialize};

/// Large-scale shape of the land.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum Landform {
    #[default]
    Plain,
    Valley,
    Hill,
    Mountain,
}

/// Soil type, which sets water holding, drainage, mud and base fertility.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum SoilKind {
    /// Under water.
    #[default]
    None,
    Alluvial,
    Loam,
    Clay,
    Sand,
    Peat,
    Stony,
}

impl SoilKind {
    /// Plant-available water at field capacity, in mm.
    pub const fn field_capacity_mm(self) -> f32 {
        match self {
            Self::None => 1.0,
            Self::Alluvial => 220.0,
            Self::Loam => 200.0,
            Self::Clay => 250.0,
            Self::Sand => 110.0,
            Self::Peat => 400.0,
            Self::Stony => 80.0,
        }
    }

    /// Fraction of rain that soaks in rather than running off.
    pub const fn infiltration(self) -> f32 {
        match self {
            Self::None => 0.0,
            Self::Alluvial => 0.8,
            Self::Loam => 0.8,
            Self::Clay => 0.45,
            Self::Sand => 0.95,
            Self::Peat => 0.6,
            Self::Stony => 0.7,
        }
    }

    /// Fraction of moisture above field capacity drained per hour.
    pub const fn drainage_per_hour(self) -> f32 {
        match self {
            Self::None => 0.0,
            Self::Alluvial => 0.02,
            Self::Loam => 0.02,
            Self::Clay => 0.006,
            Self::Sand => 0.06,
            Self::Peat => 0.003,
            Self::Stony => 0.05,
        }
    }

    /// How readily the ground turns to mud when wet.
    pub const fn mud_factor(self) -> f32 {
        match self {
            Self::None => 0.0,
            Self::Alluvial => 1.1,
            Self::Loam => 1.0,
            Self::Clay => 1.3,
            Self::Sand => 0.35,
            Self::Peat => 1.2,
            Self::Stony => 0.25,
        }
    }

    /// Natural fertility before slope and local variation.
    pub const fn base_fertility(self) -> f32 {
        match self {
            Self::None => 0.0,
            Self::Alluvial => 0.92,
            Self::Loam => 0.75,
            Self::Clay => 0.6,
            Self::Sand => 0.35,
            Self::Peat => 0.3,
            Self::Stony => 0.15,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum RoadKind {
    #[default]
    None,
    /// Unmade track between hamlets and fields.
    Track,
    /// Maintained road between villages and towns.
    Road,
}

/// How a road crosses a river.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum Crossing {
    #[default]
    None,
    Ford,
    Bridge,
}

/// Settlement size class decided by world generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SettlementKind {
    Hamlet,
    Village,
    Town,
}

/// Sentinel for "no palette entry".
pub const NO_INDEX: u16 = u16::MAX;
/// Sentinel for "no downstream neighbour".
pub const NO_FLOW: u8 = 8;

/// Static data of one tile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tile {
    pub elevation_m: f32,
    pub terrain: TerrainKind,
    pub landform: Landform,
    pub soil: SoilKind,
    /// Natural fertility, 0 – 1.
    pub fertility: f32,
    pub slope_deg: f32,
    /// Height above the nearest drainage (river, lake or sea) along the flow path.
    pub drainage_height_m: f32,
    pub catchment_km2: f32,
    /// Mean river discharge (0 off-river).
    pub river_flow_m3s: f32,
    /// Index into [`DIRS8`] of the downstream neighbour, or [`NO_FLOW`].
    pub flow_dir: u8,
    /// Wetness index, 0 – 1.
    pub moisture: f32,
    pub mean_temp_c: f32,
    pub road: RoadKind,
    pub crossing: Crossing,
    /// Index into [`WorldMap::resource_palette`], or [`NO_INDEX`].
    pub resource: u16,
    pub resource_amount: u32,
    /// Index into [`WorldMap::tree_palette`], or [`NO_INDEX`].
    pub tree_species: u16,
    /// Initial canopy density, 0 – 1.
    pub tree_density: f32,
}

impl Default for Tile {
    fn default() -> Self {
        Self {
            elevation_m: 0.0,
            terrain: TerrainKind::Lowland,
            landform: Landform::Plain,
            soil: SoilKind::Loam,
            fertility: 0.0,
            slope_deg: 0.0,
            drainage_height_m: 0.0,
            catchment_km2: 0.0,
            river_flow_m3s: 0.0,
            flow_dir: NO_FLOW,
            moisture: 0.0,
            mean_temp_c: 0.0,
            road: RoadKind::None,
            crossing: Crossing::None,
            resource: NO_INDEX,
            resource_amount: 0,
            tree_species: NO_INDEX,
            tree_density: 0.0,
        }
    }
}

impl Tile {
    /// Downstream neighbour position, if any.
    pub fn downstream(&self, pos: TilePos) -> Option<TilePos> {
        DIRS8
            .get(self.flow_dir as usize)
            .map(|(dx, dy)| pos.offset(*dx, *dy))
    }
}

/// A square block of tiles, row-major.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Chunk {
    pub tiles: Vec<Tile>,
}

/// Identifies the generated map a world uses; persisted in saves so the map
/// can be regenerated and verified on load.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapInfo {
    pub preset: WorldPresetId,
    pub seed: u64,
    pub width: u32,
    pub height: u32,
    /// Hex fingerprint of the generated tiles; empty when unknown (migrated saves).
    pub fingerprint: String,
}

/// The static world map.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WorldMap {
    width: u32,
    height: u32,
    chunk_size: u32,
    chunks_x: u32,
    chunks_y: u32,
    tile_size_m: f32,
    chunks: Vec<Chunk>,
    pub resource_palette: Vec<ResourceId>,
    pub tree_palette: Vec<TreeSpeciesId>,
}

impl WorldMap {
    /// An all-default map. `width`/`height` must be multiples of `chunk_size`.
    pub fn new(width: u32, height: u32, chunk_size: u32, tile_size_m: f32) -> Self {
        let chunks_x = width.div_ceil(chunk_size);
        let chunks_y = height.div_ceil(chunk_size);
        let per_chunk = (chunk_size * chunk_size) as usize;
        Self {
            width,
            height,
            chunk_size,
            chunks_x,
            chunks_y,
            tile_size_m,
            chunks: (0..chunks_x * chunks_y)
                .map(|_| Chunk {
                    tiles: vec![Tile::default(); per_chunk],
                })
                .collect(),
            resource_palette: Vec::new(),
            tree_palette: Vec::new(),
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn chunk_size(&self) -> u32 {
        self.chunk_size
    }

    pub fn chunks_x(&self) -> u32 {
        self.chunks_x
    }

    pub fn chunks_y(&self) -> u32 {
        self.chunks_y
    }

    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    pub fn tile_size_m(&self) -> f32 {
        self.tile_size_m
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    pub fn tile_count(&self) -> usize {
        (self.width * self.height) as usize
    }

    pub fn in_bounds(&self, pos: TilePos) -> bool {
        pos.x >= 0 && pos.y >= 0 && (pos.x as u32) < self.width && (pos.y as u32) < self.height
    }

    fn locate(&self, pos: TilePos) -> Option<(usize, usize)> {
        if !self.in_bounds(pos) {
            return None;
        }
        let (x, y) = (pos.x as u32, pos.y as u32);
        let cs = self.chunk_size;
        let chunk = (y / cs) * self.chunks_x + x / cs;
        let local = (y % cs) * cs + x % cs;
        Some((chunk as usize, local as usize))
    }

    pub fn tile(&self, pos: TilePos) -> Option<&Tile> {
        self.locate(pos).map(|(c, l)| &self.chunks[c].tiles[l])
    }

    pub fn tile_mut(&mut self, pos: TilePos) -> Option<&mut Tile> {
        self.locate(pos).map(|(c, l)| &mut self.chunks[c].tiles[l])
    }

    pub fn chunk_of(&self, pos: TilePos) -> ChunkPos {
        let cs = self.chunk_size as i32;
        ChunkPos::new(pos.x.div_euclid(cs), pos.y.div_euclid(cs))
    }

    /// Linear index of a chunk, if inside the map.
    pub fn chunk_index(&self, chunk: ChunkPos) -> Option<usize> {
        (chunk.x >= 0
            && chunk.y >= 0
            && (chunk.x as u32) < self.chunks_x
            && (chunk.y as u32) < self.chunks_y)
            .then(|| (chunk.y as u32 * self.chunks_x + chunk.x as u32) as usize)
    }

    pub fn chunk(&self, chunk: ChunkPos) -> Option<&Chunk> {
        self.chunk_index(chunk).map(|i| &self.chunks[i])
    }

    /// Top-left tile of a chunk.
    pub fn chunk_origin(&self, chunk: ChunkPos) -> TilePos {
        TilePos::new(
            chunk.x * self.chunk_size as i32,
            chunk.y * self.chunk_size as i32,
        )
    }

    /// All tile positions, row-major.
    pub fn positions(&self) -> impl Iterator<Item = TilePos> + '_ {
        (0..self.height as i32)
            .flat_map(move |y| (0..self.width as i32).map(move |x| TilePos::new(x, y)))
    }

    /// Resource deposit on a tile.
    pub fn resource_at(&self, pos: TilePos) -> Option<(&ResourceId, u32)> {
        let tile = self.tile(pos)?;
        self.resource_palette
            .get(tile.resource as usize)
            .map(|id| (id, tile.resource_amount))
    }

    /// Stable fingerprint of every tile's static data.
    pub fn fingerprint(&self) -> String {
        let mut h = Fnv64::new();
        h.write_u32(self.width);
        h.write_u32(self.height);
        h.write_u32(self.chunk_size);
        h.write_f32(self.tile_size_m);
        for id in &self.resource_palette {
            h.write_str(id.as_str());
        }
        for id in &self.tree_palette {
            h.write_str(id.as_str());
        }
        for chunk in &self.chunks {
            for t in &chunk.tiles {
                h.write_f32(t.elevation_m);
                h.write_u8(t.terrain as u8);
                h.write_u8(t.landform as u8);
                h.write_u8(t.soil as u8);
                h.write_f32(t.fertility);
                h.write_f32(t.slope_deg);
                h.write_f32(t.drainage_height_m);
                h.write_f32(t.catchment_km2);
                h.write_f32(t.river_flow_m3s);
                h.write_u8(t.flow_dir);
                h.write_f32(t.moisture);
                h.write_f32(t.mean_temp_c);
                h.write_u8(t.road as u8);
                h.write_u8(t.crossing as u8);
                h.write_u32(u32::from(t.resource));
                h.write_u32(t.resource_amount);
                h.write_u32(u32::from(t.tree_species));
                h.write_f32(t.tree_density);
            }
        }
        hex64(h.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunked_addressing_round_trips() {
        let mut map = WorldMap::new(64, 32, 16, 5.0);
        assert_eq!(map.chunk_count(), 8);
        let pos = TilePos::new(37, 21);
        map.tile_mut(pos).unwrap().elevation_m = 12.5;
        assert_eq!(map.tile(pos).unwrap().elevation_m, 12.5);
        assert_eq!(map.chunk_of(pos), ChunkPos::new(2, 1));
        assert_eq!(map.chunk_index(ChunkPos::new(2, 1)), Some(6));
        assert!(map.tile(TilePos::new(64, 0)).is_none());
        assert!(map.tile(TilePos::new(-1, 0)).is_none());
        assert_eq!(map.positions().count(), 64 * 32);
    }

    #[test]
    fn fingerprint_changes_with_data() {
        let mut map = WorldMap::new(32, 32, 16, 5.0);
        let before = map.fingerprint();
        map.tile_mut(TilePos::new(1, 1)).unwrap().fertility = 0.5;
        assert_ne!(before, map.fingerprint());
    }
}
