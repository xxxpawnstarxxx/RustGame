//! Typed string identifiers of content definitions.
//!
//! Each definition category has its own id type, so an `ItemDefId` can never
//! be passed where a `MaterialId` is expected. All ids serialise as plain
//! strings and can be looked up by `&str` (they implement `Borrow<str>`).

use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::fmt;

/// Common behaviour of definition ids.
pub trait DefKey: Ord + Clone + fmt::Display + fmt::Debug + Borrow<str> {
    fn as_str(&self) -> &str;
}

macro_rules! define_def_ids {
    ($( $(#[$meta:meta])* $name:ident ),* $(,)?) => {
        $(
            $(#[$meta])*
            #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
            #[serde(transparent)]
            pub struct $name(String);

            impl $name {
                pub fn new(id: impl Into<String>) -> Self {
                    Self(id.into())
                }

                pub fn as_str(&self) -> &str {
                    &self.0
                }
            }

            impl DefKey for $name {
                fn as_str(&self) -> &str {
                    &self.0
                }
            }

            impl Borrow<str> for $name {
                fn borrow(&self) -> &str {
                    &self.0
                }
            }

            impl fmt::Display for $name {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    f.write_str(&self.0)
                }
            }
        )*
    };
}

define_def_ids!(
    /// Scheduled activity, e.g. `"field_work"`.
    ActivityId,
    /// Daily routine, e.g. `"peasant_labourer"`.
    RoutineId,
    /// Material, e.g. `"wrought_iron"`.
    MaterialId,
    /// Item, e.g. `"iron_axe"`.
    ItemDefId,
    /// Weapon profile of an item.
    WeaponId,
    /// Armour profile of an item.
    ArmourId,
    /// Body template, e.g. `"human"`.
    BodyTemplateId,
    /// Body part within a template, e.g. `"left_leg"`.
    BodyPartId,
    /// Injury type, e.g. `"fracture"`.
    InjuryId,
    /// Skill, e.g. `"ploughing"`.
    SkillId,
    /// Profession, e.g. `"ploughman"`.
    ProfessionId,
    /// Job (unit of work), e.g. `"plough_field"`.
    JobDefId,
    /// Building, e.g. `"cruck_cottage"`.
    BuildingId,
    /// Natural resource, e.g. `"iron_ore"`.
    ResourceId,
    /// Animal species, e.g. `"roe_deer"`.
    AnimalId,
    /// Animal disease, unique within its species.
    AnimalDiseaseId,
    /// Crop, e.g. `"wheat"`.
    CropId,
    /// Recipe, e.g. `"bake_bread"`.
    RecipeId,
    /// Food profile of an item.
    FoodId,
    /// Faction definition, e.g. `"crown"`.
    FactionDefId,
    /// Law, e.g. `"forest_law"`.
    LawId,
    /// Technology, e.g. `"water_mill"`.
    TechnologyId,
    /// Character template, e.g. `"villein_ploughman"`.
    CharacterTemplateId,
    /// Tree species, e.g. `"oak"`.
    TreeSpeciesId,
    /// World generation preset, e.g. `"lowland_england"`.
    WorldPresetId,
);
