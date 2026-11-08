//! Tile occupancy and movement reservations.
//!
//! Each moving character counts against the tile it stands on and, while
//! stepping, against the tile it is entering (its reservation). A tile holds
//! at most `tile_capacity` characters, so two people can pass on a path but
//! a crowd cannot pile onto one tile.
//!
//! Occupancy is derived from the movers' persistent state and rebuilt after
//! loading, so it is never saved.

use crate::core::ids::EntityId;
use crate::world::TilePos;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Occupancy {
    tiles: BTreeMap<TilePos, Vec<EntityId>>,
}

impl Occupancy {
    pub fn clear(&mut self) {
        self.tiles.clear();
    }

    pub fn add(&mut self, pos: TilePos, entity: EntityId) {
        let list = self.tiles.entry(pos).or_default();
        if !list.contains(&entity) {
            list.push(entity);
        }
    }

    pub fn remove(&mut self, pos: TilePos, entity: EntityId) {
        if let Some(list) = self.tiles.get_mut(&pos) {
            list.retain(|e| *e != entity);
            if list.is_empty() {
                self.tiles.remove(&pos);
            }
        }
    }

    /// Characters on or entering `pos`.
    pub fn at(&self, pos: TilePos) -> &[EntityId] {
        self.tiles.get(&pos).map_or(&[], Vec::as_slice)
    }

    /// Whether `entity` may enter `pos` under the capacity limit.
    pub fn has_room(&self, pos: TilePos, entity: EntityId, capacity: u8) -> bool {
        let others = self.at(pos).iter().filter(|e| **e != entity).count();
        others < capacity as usize
    }

    pub fn occupied_by_others(&self, pos: TilePos, entity: EntityId) -> bool {
        self.at(pos).iter().any(|e| *e != entity)
    }

    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_counts_other_entities_only() {
        let mut occ = Occupancy::default();
        let p = TilePos::new(1, 1);
        let (a, b, c) = (
            EntityId::new(0, 0),
            EntityId::new(1, 0),
            EntityId::new(2, 0),
        );
        occ.add(p, a);
        assert!(occ.has_room(p, a, 1));
        assert!(!occ.has_room(p, b, 1));
        assert!(occ.has_room(p, b, 2));
        occ.add(p, b);
        assert!(!occ.has_room(p, c, 2));
        occ.remove(p, a);
        occ.remove(p, b);
        assert!(occ.is_empty());
    }
}
