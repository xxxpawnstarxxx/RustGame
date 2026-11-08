//! The world map: coordinates, the static generated map, procedural
//! generation and the structure layer (walls, doors, gates, fences).
//!
//! World data is independent of its graphical representation; the renderer
//! derives colours from it but nothing here knows about drawing.

pub mod coords;
pub mod generation;
pub mod map;
pub mod noise;
pub mod structures;

pub use coords::{ChunkPos, DIRS4, DIRS8, TilePos};
pub use generation::{GeneratedWorld, SettlementSite, generate};
pub use map::{Crossing, Landform, MapInfo, RoadKind, SettlementKind, SoilKind, Tile, WorldMap};
pub use structures::{DoorState, StructureKind, StructureLayer, StructureTile};
