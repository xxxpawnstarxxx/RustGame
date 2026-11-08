//! Body templates and injury types.

use super::{require_non_negative, require_positive, require_unit};
use crate::content::ids::*;
use crate::content::registry::{Definition, ValidationEnv};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// One part of a body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyPartDef {
    pub id: BodyPartId,
    pub name: String,
    /// The part this one is attached to; exactly one part has none.
    #[serde(default)]
    pub parent: Option<BodyPartId>,
    /// Share of body surface, used for hit location (relative weights).
    pub relative_size: f32,
    /// Destruction is fatal.
    #[serde(default)]
    pub vital: bool,
    /// Needed for walking.
    #[serde(default)]
    pub locomotion: bool,
    /// Needed for gripping tools and weapons.
    #[serde(default)]
    pub grasping: bool,
}

/// The anatomical layout shared by a kind of creature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyTemplateDef {
    pub id: BodyTemplateId,
    pub name: String,
    /// Typical adult mass.
    pub mass_kg: f32,
    pub parts: Vec<BodyPartDef>,
}

impl BodyTemplateDef {
    pub fn has_part(&self, part: &BodyPartId) -> bool {
        self.parts.iter().any(|p| &p.id == part)
    }

    pub fn part(&self, part: &BodyPartId) -> Option<&BodyPartDef> {
        self.parts.iter().find(|p| &p.id == part)
    }

    /// Share of locomotion parts by relative size.
    pub fn locomotion_parts(&self) -> impl Iterator<Item = &BodyPartDef> {
        self.parts.iter().filter(|p| p.locomotion)
    }
}

impl Definition for BodyTemplateDef {
    type Id = BodyTemplateId;
    const CATEGORY: &'static str = "body_templates";
    fn id(&self) -> &BodyTemplateId {
        &self.id
    }
    fn validate(&self, _env: &ValidationEnv<'_>) -> Vec<String> {
        let mut e = Vec::new();
        require_positive(&mut e, "mass_kg", self.mass_kg);
        let mut ids = BTreeSet::new();
        for part in &self.parts {
            if !ids.insert(part.id.as_str()) {
                e.push(format!("duplicate body part '{}'", part.id));
            }
            require_positive(&mut e, "relative_size", part.relative_size);
        }
        let roots = self.parts.iter().filter(|p| p.parent.is_none()).count();
        if roots != 1 {
            e.push(format!("exactly one root part is required, found {roots}"));
        }
        for part in &self.parts {
            if let Some(parent) = &part.parent
                && !ids.contains(parent.as_str())
            {
                e.push(format!("part '{}' has unknown parent '{parent}'", part.id));
            }
            // Walk the parent chain; a chain longer than the part count is a cycle.
            let mut current = part.parent.as_ref();
            let mut steps = 0;
            while let Some(p) = current {
                steps += 1;
                if steps > self.parts.len() {
                    e.push(format!("part '{}' is on a parent cycle", part.id));
                    break;
                }
                current = self.part(p).and_then(|d| d.parent.as_ref());
            }
        }
        if !self.parts.iter().any(|p| p.vital) {
            e.push("at least one part must be vital".into());
        }
        e
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum InjuryKind {
    Bruise,
    Cut,
    Puncture,
    Fracture,
    Sprain,
    Burn,
    Crush,
}

/// Severity class of an injury.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum InjurySeverity {
    Minor,
    Moderate,
    Severe,
    Critical,
}

/// A kind of wound and its clinical course.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InjuryDef {
    pub id: InjuryId,
    pub name: String,
    pub kind: InjuryKind,
    pub severity: InjurySeverity,
    pub bleeding_ml_per_min: f32,
    /// 0 – 1.
    pub pain: f32,
    pub healing_days: f32,
    /// Chance of infection without treatment, 0 – 1.
    pub infection_risk: f32,
    /// Reduction of mobility when on a locomotion part, 0 – 1.
    pub mobility_penalty: f32,
    #[serde(default)]
    pub permanent: bool,
}

impl Definition for InjuryDef {
    type Id = InjuryId;
    const CATEGORY: &'static str = "injuries";
    fn id(&self) -> &InjuryId {
        &self.id
    }
    fn validate(&self, _env: &ValidationEnv<'_>) -> Vec<String> {
        let mut e = Vec::new();
        require_non_negative(&mut e, "bleeding_ml_per_min", self.bleeding_ml_per_min);
        require_unit(&mut e, "pain", self.pain);
        require_positive(&mut e, "healing_days", self.healing_days);
        require_unit(&mut e, "infection_risk", self.infection_risk);
        require_unit(&mut e, "mobility_penalty", self.mobility_penalty);
        e
    }
}
