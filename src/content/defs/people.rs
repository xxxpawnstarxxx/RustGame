//! Skills, professions, jobs and character templates.

use super::{ItemAmount, default_true, require_months, require_positive};
use crate::content::ids::*;
use crate::content::registry::{Definition, Reference, ValidationEnv};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SkillCategory {
    Agriculture,
    Husbandry,
    Craft,
    Building,
    Combat,
    Physical,
    Social,
    Learning,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillDef {
    pub id: SkillId,
    pub name: String,
    pub category: SkillCategory,
    #[serde(default)]
    pub description: String,
}

impl Definition for SkillDef {
    type Id = SkillId;
    const CATEGORY: &'static str = "skills";
    fn id(&self) -> &SkillId {
        &self.id
    }
}

/// Legal and social standing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SocialRank {
    Villein,
    Cottar,
    Freeman,
    Burgess,
    Clergy,
    Gentry,
    Nobility,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfessionDef {
    pub id: ProfessionId,
    pub name: String,
    pub social_rank: SocialRank,
    #[serde(default)]
    pub skills: Vec<SkillId>,
    #[serde(default)]
    pub routine: Option<RoutineId>,
    #[serde(default)]
    pub jobs: Vec<JobDefId>,
    #[serde(default)]
    pub workplaces: Vec<BuildingId>,
}

impl Definition for ProfessionDef {
    type Id = ProfessionId;
    const CATEGORY: &'static str = "professions";
    fn id(&self) -> &ProfessionId {
        &self.id
    }
    fn references(&self) -> Vec<Reference> {
        let mut refs: Vec<_> = self
            .skills
            .iter()
            .map(|s| Reference::new("skills", "skills", s))
            .collect();
        refs.extend(
            self.routine
                .iter()
                .map(|r| Reference::new("routine", "routines", r)),
        );
        refs.extend(self.jobs.iter().map(|j| Reference::new("jobs", "jobs", j)));
        refs.extend(
            self.workplaces
                .iter()
                .map(|b| Reference::new("workplaces", "buildings", b)),
        );
        refs
    }
}

/// A unit of work a person can be assigned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobDef {
    pub id: JobDefId,
    pub name: String,
    pub skill: SkillId,
    pub labour_hours: f32,
    #[serde(default)]
    pub building: Option<BuildingId>,
    #[serde(default)]
    pub recipe: Option<RecipeId>,
    #[serde(default)]
    pub tools: Vec<ItemDefId>,
    /// 1-based months in which the job is seasonal; empty = all year.
    #[serde(default)]
    pub months: Vec<u32>,
}

impl Definition for JobDef {
    type Id = JobDefId;
    const CATEGORY: &'static str = "jobs";
    fn id(&self) -> &JobDefId {
        &self.id
    }
    fn references(&self) -> Vec<Reference> {
        let mut refs = vec![Reference::new("skill", "skills", &self.skill)];
        refs.extend(
            self.building
                .iter()
                .map(|b| Reference::new("building", "buildings", b)),
        );
        refs.extend(
            self.recipe
                .iter()
                .map(|r| Reference::new("recipe", "recipes", r)),
        );
        refs.extend(
            self.tools
                .iter()
                .map(|t| Reference::new("tools", "items", t)),
        );
        refs
    }
    fn validate(&self, env: &ValidationEnv<'_>) -> Vec<String> {
        let mut e = Vec::new();
        require_positive(&mut e, "labour_hours", self.labour_hours);
        require_months(&mut e, "months", &self.months, env.calendar);
        e
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillRange {
    pub skill: SkillId,
    pub min: u8,
    pub max: u8,
}

fn default_walk_speed() -> f32 {
    1.35
}

fn default_carry() -> f32 {
    30.0
}

/// A template from which individual characters are created.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterTemplateDef {
    pub id: CharacterTemplateId,
    pub name: String,
    pub body: BodyTemplateId,
    #[serde(default)]
    pub profession: Option<ProfessionId>,
    /// Inclusive age range in years.
    pub age_years: [u32; 2],
    #[serde(default)]
    pub skills: Vec<SkillRange>,
    #[serde(default)]
    pub starting_items: Vec<ItemAmount>,
    /// Comfortable walking speed. Default 1.35 m/s.
    #[serde(default = "default_walk_speed")]
    pub walking_speed_mps: f32,
    /// Comfortable carrying load. Default 30 kg.
    #[serde(default = "default_carry")]
    pub carry_capacity_kg: f32,
    #[serde(default)]
    pub can_swim: bool,
    #[serde(default = "default_true")]
    pub can_climb: bool,
    /// Names drawn at random for new characters.
    #[serde(default)]
    pub given_names: Vec<String>,
}

impl Definition for CharacterTemplateDef {
    type Id = CharacterTemplateId;
    const CATEGORY: &'static str = "characters";
    fn id(&self) -> &CharacterTemplateId {
        &self.id
    }
    fn references(&self) -> Vec<Reference> {
        let mut refs = vec![Reference::new("body", "body_templates", &self.body)];
        refs.extend(
            self.profession
                .iter()
                .map(|p| Reference::new("profession", "professions", p)),
        );
        refs.extend(
            self.skills
                .iter()
                .map(|s| Reference::new("skills.skill", "skills", &s.skill)),
        );
        refs.extend(
            self.starting_items
                .iter()
                .map(|i| Reference::new("starting_items.item", "items", &i.item)),
        );
        refs
    }
    fn validate(&self, _env: &ValidationEnv<'_>) -> Vec<String> {
        let mut e = Vec::new();
        if self.age_years[0] > self.age_years[1] {
            e.push("age_years must be [min, max]".into());
        }
        if self.skills.iter().any(|s| s.min > s.max || s.max > 100) {
            e.push("skill ranges must satisfy min <= max <= 100".into());
        }
        if self.starting_items.iter().any(|i| i.quantity == 0) {
            e.push("starting item quantities must be at least 1".into());
        }
        require_positive(&mut e, "walking_speed_mps", self.walking_speed_mps);
        if self.walking_speed_mps > 2.5 {
            e.push("walking_speed_mps above 2.5 m/s is not a walking pace".into());
        }
        require_positive(&mut e, "carry_capacity_kg", self.carry_capacity_kg);
        e
    }
}
