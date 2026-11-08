//! Built structures stamped onto the map from building layouts.
//!
//! Each placed building is a `Structure` entity; its cells are recorded
//! here, keyed by tile, so navigation can query walls, doors and gates in
//! constant time. The layer is persistent (it changes during play).

use crate::content::{BuildingDef, StructureCell, TerrainKind};
use crate::core::error::{SimError, SimResult};
use crate::core::ids::EntityId;
use crate::world::coords::TilePos;
use crate::world::map::WorldMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;

/// State of a door or gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DoorState {
    Open,
    Closed,
    Locked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum StructureKind {
    Wall,
    Floor,
    Door(DoorState),
    Gate(DoorState),
    Fence,
}

impl StructureKind {
    /// Whether the tile can never be walked through.
    pub const fn blocks(self) -> bool {
        matches!(
            self,
            Self::Wall
                | Self::Fence
                | Self::Door(DoorState::Locked)
                | Self::Gate(DoorState::Locked)
        )
    }

    /// Seconds spent opening (and closing behind) before passing.
    pub fn opening_delay(self, door_seconds: f32, gate_seconds: f32) -> f32 {
        match self {
            Self::Door(DoorState::Closed) => door_seconds,
            Self::Gate(DoorState::Closed) => gate_seconds,
            _ => 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructureTile {
    pub kind: StructureKind,
    /// The structure entity this cell belongs to.
    pub building: EntityId,
}

/// Structure cells by tile.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StructureLayer {
    tiles: BTreeMap<TilePos, StructureTile>,
}

impl Serialize for StructureLayer {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.tiles.iter())
    }
}

impl<'de> Deserialize<'de> for StructureLayer {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let pairs: Vec<(TilePos, StructureTile)> = Vec::deserialize(deserializer)?;
        Ok(Self {
            tiles: pairs.into_iter().collect(),
        })
    }
}

fn cell_kind(cell: StructureCell) -> StructureKind {
    match cell {
        StructureCell::Wall => StructureKind::Wall,
        StructureCell::Floor => StructureKind::Floor,
        StructureCell::Door => StructureKind::Door(DoorState::Closed),
        StructureCell::Gate => StructureKind::Gate(DoorState::Closed),
        StructureCell::Fence => StructureKind::Fence,
    }
}

impl StructureLayer {
    pub fn get(&self, pos: TilePos) -> Option<&StructureTile> {
        self.tiles.get(&pos)
    }

    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (TilePos, &StructureTile)> {
        self.tiles.iter().map(|(p, t)| (*p, t))
    }

    /// Checks that a building fits at `origin` without overlapping water,
    /// cliffs or other structures.
    pub fn check_placement(
        &self,
        def: &BuildingDef,
        origin: TilePos,
        map: &WorldMap,
    ) -> SimResult<()> {
        for (dx, dy, _) in def.cells() {
            let pos = origin.offset(dx, dy);
            let tile = map.tile(pos).ok_or_else(|| {
                SimError::InvalidCommand(format!(
                    "building '{}' extends off the map at {pos}",
                    def.id
                ))
            })?;
            if tile.terrain.is_water() || tile.terrain == TerrainKind::Cliff {
                return Err(SimError::InvalidCommand(format!(
                    "building '{}' cannot stand on {:?} at {pos}",
                    def.id, tile.terrain
                )));
            }
            if self.tiles.contains_key(&pos) {
                return Err(SimError::InvalidCommand(format!(
                    "building '{}' overlaps an existing structure at {pos}",
                    def.id
                )));
            }
        }
        Ok(())
    }

    /// Stamps a building's cells; returns the tiles changed.
    pub fn place(
        &mut self,
        def: &BuildingDef,
        origin: TilePos,
        building: EntityId,
        map: &WorldMap,
    ) -> SimResult<Vec<TilePos>> {
        self.check_placement(def, origin, map)?;
        let mut changed = Vec::new();
        for (dx, dy, cell) in def.cells() {
            let pos = origin.offset(dx, dy);
            self.tiles.insert(
                pos,
                StructureTile {
                    kind: cell_kind(cell),
                    building,
                },
            );
            changed.push(pos);
        }
        Ok(changed)
    }

    /// Removes every cell of a building; returns the tiles changed.
    pub fn remove_building(&mut self, building: EntityId) -> Vec<TilePos> {
        let tiles: Vec<TilePos> = self
            .tiles
            .iter()
            .filter(|(_, t)| t.building == building)
            .map(|(p, _)| *p)
            .collect();
        for pos in &tiles {
            self.tiles.remove(pos);
        }
        tiles
    }

    /// Sets the state of the door or gate at `pos`.
    pub fn set_door(&mut self, pos: TilePos, state: DoorState) -> SimResult<()> {
        let tile = self
            .tiles
            .get_mut(&pos)
            .ok_or_else(|| SimError::InvalidCommand(format!("no structure at {pos}")))?;
        tile.kind = match tile.kind {
            StructureKind::Door(_) => StructureKind::Door(state),
            StructureKind::Gate(_) => StructureKind::Gate(state),
            other => {
                return Err(SimError::InvalidCommand(format!(
                    "{other:?} at {pos} is not a door or gate"
                )));
            }
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{BuildingCategory, BuildingId};

    fn hut() -> BuildingDef {
        BuildingDef {
            id: BuildingId::new("hut"),
            name: "Hut".into(),
            category: BuildingCategory::Dwelling,
            layout: vec!["###".into(), "#.#".into(), "#D#".into()],
            materials: vec![],
            build_hours: 10.0,
            technology: None,
        }
    }

    #[test]
    fn place_overlap_remove_and_doors() {
        let map = WorldMap::new(32, 32, 16, 5.0);
        let mut layer = StructureLayer::default();
        let id = EntityId::new(0, 0);
        let tiles = layer.place(&hut(), TilePos::new(2, 2), id, &map).unwrap();
        assert_eq!(tiles.len(), 9);
        assert!(layer.get(TilePos::new(2, 2)).unwrap().kind.blocks());
        assert!(!layer.get(TilePos::new(3, 3)).unwrap().kind.blocks());
        assert!(
            layer
                .place(&hut(), TilePos::new(3, 3), EntityId::new(1, 0), &map)
                .is_err()
        );
        assert!(
            layer
                .place(&hut(), TilePos::new(31, 31), EntityId::new(1, 0), &map)
                .is_err()
        );

        let door = TilePos::new(3, 4);
        layer.set_door(door, DoorState::Locked).unwrap();
        assert!(layer.get(door).unwrap().kind.blocks());
        assert!(layer.set_door(TilePos::new(2, 2), DoorState::Open).is_err());

        let json = serde_json::to_string(&layer).unwrap();
        let back: StructureLayer = serde_json::from_str(&json).unwrap();
        assert_eq!(back, layer);

        assert_eq!(layer.remove_building(id).len(), 9);
        assert!(layer.is_empty());
    }
}
