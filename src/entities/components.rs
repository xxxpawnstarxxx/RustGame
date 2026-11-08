use crate::core::ids::EntityId;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;

/// Storage for one component type, keyed by entity.
///
/// Backed by a `BTreeMap`, so iteration order is always ascending entity id —
/// a requirement for deterministic simulation. Serialised as a list of
/// `[entity, component]` pairs, because JSON object keys must be strings.
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentStore<T> {
    data: BTreeMap<EntityId, T>,
}

impl<T> Default for ComponentStore<T> {
    fn default() -> Self {
        Self {
            data: BTreeMap::new(),
        }
    }
}

impl<T> ComponentStore<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts or replaces a component, returning the previous value.
    pub fn insert(&mut self, entity: EntityId, component: T) -> Option<T> {
        self.data.insert(entity, component)
    }

    pub fn remove(&mut self, entity: EntityId) -> Option<T> {
        self.data.remove(&entity)
    }

    pub fn get(&self, entity: EntityId) -> Option<&T> {
        self.data.get(&entity)
    }

    pub fn get_mut(&mut self, entity: EntityId) -> Option<&mut T> {
        self.data.get_mut(&entity)
    }

    pub fn contains(&self, entity: EntityId) -> bool {
        self.data.contains_key(&entity)
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Components in ascending entity order.
    pub fn iter(&self) -> impl Iterator<Item = (EntityId, &T)> {
        self.data.iter().map(|(id, c)| (*id, c))
    }

    /// Mutable components in ascending entity order.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (EntityId, &mut T)> {
        self.data.iter_mut().map(|(id, c)| (*id, c))
    }

    /// Entity ids in ascending order.
    pub fn entities(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.data.keys().copied()
    }
}

impl<T: Serialize> Serialize for ComponentStore<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.data.iter())
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for ComponentStore<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let pairs: Vec<(EntityId, T)> = Vec::deserialize(deserializer)?;
        Ok(Self {
            data: pairs.into_iter().collect(),
        })
    }
}

/// Display name of an entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Name(pub String);

impl Name {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iterates_in_entity_order_and_round_trips() {
        let mut store = ComponentStore::new();
        store.insert(EntityId::new(5, 0), Name::new("Alice"));
        store.insert(EntityId::new(1, 2), Name::new("Bertram"));
        let order: Vec<_> = store.entities().collect();
        assert_eq!(order, vec![EntityId::new(1, 2), EntityId::new(5, 0)]);

        let json = serde_json::to_string(&store).unwrap();
        let back: ComponentStore<Name> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, store);
    }
}
