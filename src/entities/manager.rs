use crate::core::error::{SimError, SimResult};
use crate::core::ids::EntityId;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Broad category of an entity. Determines which domain systems take an
/// interest in it; detailed state lives in components.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EntityKind {
    Character,
    Animal,
    Item,
    Structure,
    Settlement,
    Faction,
}

impl EntityKind {
    pub const ALL: [Self; 6] = [
        Self::Character,
        Self::Animal,
        Self::Item,
        Self::Structure,
        Self::Settlement,
        Self::Faction,
    ];
}

impl fmt::Display for EntityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Slot {
    generation: u32,
    /// `Some` while the slot holds a live entity.
    kind: Option<EntityKind>,
}

/// Allocates generational entity ids and tracks which are alive.
///
/// Freed slots are reused in LIFO order, which is deterministic. A slot whose
/// generation would overflow is retired permanently instead of being reused.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EntityManager {
    slots: Vec<Slot>,
    free: Vec<u32>,
    alive: u32,
}

impl EntityManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new live entity.
    pub fn create(&mut self, kind: EntityKind) -> EntityId {
        self.alive += 1;
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.kind = Some(kind);
            return EntityId::new(index, slot.generation);
        }
        let index = self.slots.len() as u32;
        self.slots.push(Slot {
            generation: 0,
            kind: Some(kind),
        });
        EntityId::new(index, 0)
    }

    /// Destroys a live entity, returning its kind.
    pub fn destroy(&mut self, id: EntityId) -> SimResult<EntityKind> {
        let kind = self.kind(id).ok_or(SimError::NoSuchEntity(id))?;
        let slot = &mut self.slots[id.index() as usize];
        slot.kind = None;
        if slot.generation < u32::MAX {
            slot.generation += 1;
            self.free.push(id.index());
        }
        self.alive -= 1;
        Ok(kind)
    }

    /// Whether `id` refers to a live entity (stale generations are not alive).
    pub fn is_alive(&self, id: EntityId) -> bool {
        self.kind(id).is_some()
    }

    /// Kind of a live entity.
    pub fn kind(&self, id: EntityId) -> Option<EntityKind> {
        self.slots
            .get(id.index() as usize)
            .filter(|slot| slot.generation == id.generation())
            .and_then(|slot| slot.kind)
    }

    /// Returns an error unless `id` is alive.
    pub fn require(&self, id: EntityId) -> SimResult<EntityKind> {
        self.kind(id).ok_or(SimError::NoSuchEntity(id))
    }

    /// Number of live entities.
    pub fn len(&self) -> usize {
        self.alive as usize
    }

    pub fn is_empty(&self) -> bool {
        self.alive == 0
    }

    /// Live entities in ascending slot order.
    pub fn iter(&self) -> impl Iterator<Item = (EntityId, EntityKind)> + '_ {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            slot.kind
                .map(|kind| (EntityId::new(index as u32, slot.generation), kind))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_ids_are_rejected_after_reuse() {
        let mut em = EntityManager::new();
        let a = em.create(EntityKind::Character);
        em.destroy(a).unwrap();
        let b = em.create(EntityKind::Item);
        assert_eq!(a.index(), b.index());
        assert_ne!(a, b);
        assert!(!em.is_alive(a));
        assert!(em.is_alive(b));
        assert!(matches!(em.destroy(a), Err(SimError::NoSuchEntity(_))));
        assert_eq!(em.kind(b), Some(EntityKind::Item));
    }

    #[test]
    fn iteration_is_ordered_and_counts_are_tracked() {
        let mut em = EntityManager::new();
        let ids: Vec<_> = (0..4).map(|_| em.create(EntityKind::Animal)).collect();
        em.destroy(ids[1]).unwrap();
        assert_eq!(em.len(), 3);
        let listed: Vec<_> = em.iter().map(|(id, _)| id).collect();
        assert_eq!(listed, vec![ids[0], ids[2], ids[3]]);
    }

    #[test]
    fn round_trips_through_json() {
        let mut em = EntityManager::new();
        let a = em.create(EntityKind::Settlement);
        em.create(EntityKind::Faction);
        em.destroy(a).unwrap();
        let json = serde_json::to_string(&em).unwrap();
        let mut back: EntityManager = serde_json::from_str(&json).unwrap();
        assert_eq!(back, em);
        assert_eq!(back.create(EntityKind::Item), em.create(EntityKind::Item));
    }
}
