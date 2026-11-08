//! Technologies, laws and factions.

use crate::content::ids::*;
use crate::content::registry::{Definition, Reference, ValidationEnv};
use serde::{Deserialize, Serialize};

/// A technique or invention. The active period lists which are available.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechnologyDef {
    pub id: TechnologyId,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub prerequisites: Vec<TechnologyId>,
    /// Approximate year of adoption in the region (informational).
    #[serde(default)]
    pub introduced_year: Option<u32>,
}

impl Definition for TechnologyDef {
    type Id = TechnologyId;
    const CATEGORY: &'static str = "technologies";
    fn id(&self) -> &TechnologyId {
        &self.id
    }
    fn references(&self) -> Vec<Reference> {
        self.prerequisites
            .iter()
            .map(|p| Reference::new("prerequisites", "technologies", p))
            .collect()
    }
    fn validate(&self, _env: &ValidationEnv<'_>) -> Vec<String> {
        if self.prerequisites.contains(&self.id) {
            vec!["a technology cannot require itself".into()]
        } else {
            Vec::new()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum LawCategory {
    Property,
    Criminal,
    Inheritance,
    Labour,
    Trade,
    Forest,
    Religious,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum PenaltyKind {
    /// Amercement: a money penalty.
    Fine,
    Stocks,
    Pillory,
    Imprisonment,
    Penance,
    Outlawry,
    Hanging,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PenaltyDef {
    pub kind: PenaltyKind,
    #[serde(default)]
    pub fine_pence: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LawDef {
    pub id: LawId,
    pub name: String,
    pub category: LawCategory,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub penalty: Option<PenaltyDef>,
}

impl Definition for LawDef {
    type Id = LawId;
    const CATEGORY: &'static str = "laws";
    fn id(&self) -> &LawId {
        &self.id
    }
    fn validate(&self, _env: &ValidationEnv<'_>) -> Vec<String> {
        match &self.penalty {
            Some(p) if p.kind == PenaltyKind::Fine && p.fine_pence == 0 => {
                vec!["a fine penalty needs fine_pence".into()]
            }
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FactionKind {
    Crown,
    Barony,
    Manor,
    Church,
    Monastery,
    Guild,
    Town,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactionRelationDef {
    pub faction: FactionDefId,
    /// -100 (open hostility) to 100 (close alliance).
    pub value: i32,
}

/// A lordship, institution or corporate body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactionDef {
    pub id: FactionDefId,
    pub name: String,
    pub kind: FactionKind,
    /// Overlord or mother institution.
    #[serde(default)]
    pub parent: Option<FactionDefId>,
    #[serde(default)]
    pub laws: Vec<LawId>,
    /// Initial relations towards other factions.
    #[serde(default)]
    pub relations: Vec<FactionRelationDef>,
    /// Whether new worlds create this faction. Default true.
    #[serde(default = "super::default_true")]
    pub spawn_at_world_creation: bool,
}

impl Definition for FactionDef {
    type Id = FactionDefId;
    const CATEGORY: &'static str = "factions";
    fn id(&self) -> &FactionDefId {
        &self.id
    }
    fn references(&self) -> Vec<Reference> {
        let mut refs: Vec<_> = self
            .parent
            .iter()
            .map(|p| Reference::new("parent", "factions", p))
            .collect();
        refs.extend(self.laws.iter().map(|l| Reference::new("laws", "laws", l)));
        refs.extend(
            self.relations
                .iter()
                .map(|r| Reference::new("relations.faction", "factions", &r.faction)),
        );
        refs
    }
    fn validate(&self, _env: &ValidationEnv<'_>) -> Vec<String> {
        let mut e = Vec::new();
        if self.parent.as_ref() == Some(&self.id) {
            e.push("a faction cannot be its own parent".into());
        }
        for r in &self.relations {
            if r.faction == self.id {
                e.push("a faction cannot have a relation with itself".into());
            }
            if !(-100..=100).contains(&r.value) {
                e.push(format!(
                    "relation with '{}' must be between -100 and 100",
                    r.faction
                ));
            }
        }
        e
    }
}
