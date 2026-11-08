//! Runtime instances of content definitions.
//!
//! Definitions (in [`crate::content`]) say what a *kind* of thing is; these
//! components describe one particular thing in one particular world, and
//! refer back to their definition by id. They are persistent.

use crate::content::{
    BodyTemplateId, BuildingId, CharacterTemplateId, FactionDefId, FactionKind, ItemDefId,
    ProfessionId, SkillId,
};
use crate::core::ids::EntityId;
use crate::core::time::GameInstant;
use crate::world::{SettlementKind, TilePos};
use serde::{Deserialize, Serialize};

/// Where an entity is on the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub tile: TilePos,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillLevel {
    pub skill: SkillId,
    /// 0 – 100.
    pub level: u8,
}

/// One individual person.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharacterRecord {
    pub template: CharacterTemplateId,
    pub body: BodyTemplateId,
    pub profession: Option<ProfessionId>,
    pub birth: GameInstant,
    pub skills: Vec<SkillLevel>,
    /// Settlement the character belongs to.
    pub home: Option<EntityId>,
    pub faction: Option<EntityId>,
}

/// Where an item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemLocation {
    Ground(TilePos),
    Carried(EntityId),
}

/// One individual item or stack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemInstance {
    pub def: ItemDefId,
    pub quantity: u32,
    /// Workmanship, 0 – 1.
    pub quality: f32,
    /// Wear, 1 = as new.
    pub condition: f32,
    pub location: ItemLocation,
}

/// One settlement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettlementRecord {
    pub name: String,
    pub kind: SettlementKind,
    pub centre: TilePos,
    pub founded: GameInstant,
    /// Lordship or corporation holding the settlement.
    pub faction: Option<EntityId>,
    /// Created by world generation rather than founded in play.
    pub generated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactionRelation {
    pub other: EntityId,
    pub value: i32,
}

/// One faction in this world.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactionRecord {
    pub def: FactionDefId,
    pub name: String,
    pub kind: FactionKind,
    pub parent: Option<EntityId>,
    /// Sorted by `other`.
    pub relations: Vec<FactionRelation>,
}

impl FactionRecord {
    pub fn relation(&self, other: EntityId) -> i32 {
        self.relations
            .iter()
            .find(|r| r.other == other)
            .map_or(0, |r| r.value)
    }

    /// Sets a relation, returning the previous value.
    pub fn set_relation(&mut self, other: EntityId, value: i32) -> i32 {
        match self.relations.binary_search_by(|r| r.other.cmp(&other)) {
            Ok(i) => std::mem::replace(&mut self.relations[i].value, value),
            Err(i) => {
                self.relations.insert(i, FactionRelation { other, value });
                0
            }
        }
    }
}

/// One placed building.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildingRecord {
    pub def: BuildingId,
    pub origin: TilePos,
}
