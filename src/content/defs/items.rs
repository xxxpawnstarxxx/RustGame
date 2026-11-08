//! Materials, items and the weapon / armour / food / resource profiles that
//! extend items, plus production recipes.

use super::{default_one, require_non_negative, require_positive, require_unit};
use crate::content::ids::*;
use crate::content::registry::{Definition, Reference, ValidationEnv};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum MaterialCategory {
    Metal,
    Wood,
    Stone,
    Earth,
    Textile,
    Leather,
    Bone,
    Fibre,
    Organic,
    Glass,
}

/// A physical material with its mechanical and economic properties.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialDef {
    pub id: MaterialId,
    pub name: String,
    pub category: MaterialCategory,
    pub density_kg_m3: f32,
    /// Relative hardness, roughly the Mohs scale (0–10).
    pub hardness: f32,
    /// Resistance to fracture, 0 (brittle) – 1 (very tough).
    pub toughness: f32,
    #[serde(default)]
    pub flammable: bool,
    /// Typical value in pence per kilogram.
    pub value_per_kg: f32,
    #[serde(default)]
    pub technology: Option<TechnologyId>,
}

impl Definition for MaterialDef {
    type Id = MaterialId;
    const CATEGORY: &'static str = "materials";
    fn id(&self) -> &MaterialId {
        &self.id
    }
    fn references(&self) -> Vec<Reference> {
        self.technology
            .iter()
            .map(|t| Reference::new("technology", "technologies", t))
            .collect()
    }
    fn validate(&self, env: &ValidationEnv<'_>) -> Vec<String> {
        let mut e = Vec::new();
        require_positive(&mut e, "density_kg_m3", self.density_kg_m3);
        if !(0.0..=10.0).contains(&self.hardness) {
            e.push("hardness must be between 0 and 10".into());
        }
        require_unit(&mut e, "toughness", self.toughness);
        require_non_negative(&mut e, "value_per_kg", self.value_per_kg);
        env.check_technology(&self.technology, &mut e);
        e
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ItemCategory {
    Tool,
    Weapon,
    Armour,
    Clothing,
    Food,
    RawMaterial,
    Container,
    Goods,
}

/// The base definition of every portable object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemDef {
    pub id: ItemDefId,
    pub name: String,
    pub category: ItemCategory,
    pub weight_kg: f32,
    /// Maximum units per stack (1 = never stacks). Default 1.
    #[serde(default = "default_one")]
    pub stack_size: u32,
    /// Typical value of one unit in pence.
    pub base_value_pence: f32,
    #[serde(default)]
    pub materials: Vec<MaterialId>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub technology: Option<TechnologyId>,
    #[serde(default)]
    pub description: String,
}

impl Definition for ItemDef {
    type Id = ItemDefId;
    const CATEGORY: &'static str = "items";
    fn id(&self) -> &ItemDefId {
        &self.id
    }
    fn references(&self) -> Vec<Reference> {
        let mut refs: Vec<_> = self
            .materials
            .iter()
            .map(|m| Reference::new("materials", "materials", m))
            .collect();
        refs.extend(
            self.technology
                .iter()
                .map(|t| Reference::new("technology", "technologies", t)),
        );
        refs
    }
    fn validate(&self, env: &ValidationEnv<'_>) -> Vec<String> {
        let mut e = Vec::new();
        require_positive(&mut e, "weight_kg", self.weight_kg);
        require_non_negative(&mut e, "base_value_pence", self.base_value_pence);
        if self.stack_size == 0 {
            e.push("stack_size must be at least 1".into());
        }
        env.check_technology(&self.technology, &mut e);
        e
    }
}

/// Checks that a referenced item exists and has the expected category.
fn require_item_category(
    env: &ValidationEnv<'_>,
    item: &ItemDefId,
    expected: &[ItemCategory],
    errors: &mut Vec<String>,
) {
    if let Some(def) = env.content.items().get(item)
        && !expected.contains(&def.category)
    {
        errors.push(format!(
            "item '{item}' has category {:?}, expected one of {expected:?}",
            def.category
        ));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DamageKind {
    Cut,
    Pierce,
    Blunt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum WeaponClass {
    Sword,
    Axe,
    Mace,
    Spear,
    Polearm,
    Dagger,
    Bow,
    Crossbow,
    Tool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DamageProfile {
    pub kind: DamageKind,
    /// Relative striking power (1 = a well-made arming sword's cut).
    pub power: f32,
}

/// Combat characteristics of an item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponDef {
    pub id: WeaponId,
    pub item: ItemDefId,
    pub class: WeaponClass,
    pub damage: Vec<DamageProfile>,
    pub reach_m: f32,
    pub hands: u8,
    pub skill: SkillId,
    /// Effective range for missile weapons.
    #[serde(default)]
    pub range_m: Option<f32>,
    #[serde(default)]
    pub technology: Option<TechnologyId>,
}

impl Definition for WeaponDef {
    type Id = WeaponId;
    const CATEGORY: &'static str = "weapons";
    fn id(&self) -> &WeaponId {
        &self.id
    }
    fn references(&self) -> Vec<Reference> {
        let mut refs = vec![
            Reference::new("item", "items", &self.item),
            Reference::new("skill", "skills", &self.skill),
        ];
        refs.extend(
            self.technology
                .iter()
                .map(|t| Reference::new("technology", "technologies", t)),
        );
        refs
    }
    fn validate(&self, env: &ValidationEnv<'_>) -> Vec<String> {
        let mut e = Vec::new();
        if self.damage.is_empty() {
            e.push("damage must list at least one profile".into());
        }
        for d in &self.damage {
            require_positive(&mut e, "damage.power", d.power);
        }
        require_positive(&mut e, "reach_m", self.reach_m);
        if !(1..=2).contains(&self.hands) {
            e.push("hands must be 1 or 2".into());
        }
        if let Some(range) = self.range_m {
            require_positive(&mut e, "range_m", range);
        }
        require_item_category(
            env,
            &self.item,
            &[ItemCategory::Weapon, ItemCategory::Tool],
            &mut e,
        );
        env.check_technology(&self.technology, &mut e);
        e
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ArmourLayer {
    /// Padded or quilted garments worn beneath.
    Padding,
    Mail,
    Plate,
    /// Surcoats, cloaks and the like worn over.
    Over,
}

/// Fractional protection against each damage kind (0 – 1).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Protection {
    pub cut: f32,
    pub pierce: f32,
    pub blunt: f32,
}

/// Protective characteristics of an item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmourDef {
    pub id: ArmourId,
    pub item: ItemDefId,
    pub layer: ArmourLayer,
    pub material: MaterialId,
    /// Body part ids covered; each must exist in at least one body template.
    pub coverage: Vec<BodyPartId>,
    pub protection: Protection,
    #[serde(default)]
    pub technology: Option<TechnologyId>,
}

impl Definition for ArmourDef {
    type Id = ArmourId;
    const CATEGORY: &'static str = "armour";
    fn id(&self) -> &ArmourId {
        &self.id
    }
    fn references(&self) -> Vec<Reference> {
        let mut refs = vec![
            Reference::new("item", "items", &self.item),
            Reference::new("material", "materials", &self.material),
        ];
        refs.extend(
            self.technology
                .iter()
                .map(|t| Reference::new("technology", "technologies", t)),
        );
        refs
    }
    fn validate(&self, env: &ValidationEnv<'_>) -> Vec<String> {
        let mut e = Vec::new();
        if self.coverage.is_empty() {
            e.push("coverage must list at least one body part".into());
        }
        for part in &self.coverage {
            if !env
                .content
                .body_templates()
                .iter()
                .any(|t| t.has_part(part))
            {
                e.push(format!("coverage part '{part}' exists in no body template"));
            }
        }
        require_unit(&mut e, "protection.cut", self.protection.cut);
        require_unit(&mut e, "protection.pierce", self.protection.pierce);
        require_unit(&mut e, "protection.blunt", self.protection.blunt);
        require_item_category(
            env,
            &self.item,
            &[ItemCategory::Armour, ItemCategory::Clothing],
            &mut e,
        );
        env.check_technology(&self.technology, &mut e);
        e
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum FoodGroup {
    Grain,
    Bread,
    Pottage,
    Meat,
    Fish,
    Dairy,
    Vegetable,
    Fruit,
    Drink,
}

/// Nutritional characteristics of an item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoodDef {
    pub id: FoodId,
    pub item: ItemDefId,
    pub group: FoodGroup,
    pub kcal_per_kg: f32,
    /// Days until spoiled; `None` keeps indefinitely.
    #[serde(default)]
    pub spoilage_days: Option<f32>,
    /// Salted, smoked, dried or otherwise preserved.
    #[serde(default)]
    pub preserved: bool,
}

impl Definition for FoodDef {
    type Id = FoodId;
    const CATEGORY: &'static str = "foods";
    fn id(&self) -> &FoodId {
        &self.id
    }
    fn references(&self) -> Vec<Reference> {
        vec![Reference::new("item", "items", &self.item)]
    }
    fn validate(&self, env: &ValidationEnv<'_>) -> Vec<String> {
        let mut e = Vec::new();
        require_positive(&mut e, "kcal_per_kg", self.kcal_per_kg);
        if let Some(days) = self.spoilage_days {
            require_positive(&mut e, "spoilage_days", days);
        }
        require_item_category(env, &self.item, &[ItemCategory::Food], &mut e);
        e
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ResourceKind {
    Ore,
    Stone,
    Clay,
    Sand,
    Peat,
    Salt,
    Fuel,
}

/// A natural deposit placed by world generation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceDef {
    pub id: ResourceId,
    pub name: String,
    pub kind: ResourceKind,
    /// Item obtained by extracting one unit.
    pub item: ItemDefId,
    #[serde(default)]
    pub renewable: bool,
    /// Units regained per year for renewable deposits.
    #[serde(default)]
    pub regeneration_per_year: f32,
    #[serde(default)]
    pub technology: Option<TechnologyId>,
}

impl Definition for ResourceDef {
    type Id = ResourceId;
    const CATEGORY: &'static str = "resources";
    fn id(&self) -> &ResourceId {
        &self.id
    }
    fn references(&self) -> Vec<Reference> {
        let mut refs = vec![Reference::new("item", "items", &self.item)];
        refs.extend(
            self.technology
                .iter()
                .map(|t| Reference::new("technology", "technologies", t)),
        );
        refs
    }
    fn validate(&self, env: &ValidationEnv<'_>) -> Vec<String> {
        let mut e = Vec::new();
        require_non_negative(&mut e, "regeneration_per_year", self.regeneration_per_year);
        if !self.renewable && self.regeneration_per_year > 0.0 {
            e.push("regeneration_per_year is set but the resource is not renewable".into());
        }
        env.check_technology(&self.technology, &mut e);
        e
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemAmount {
    pub item: ItemDefId,
    pub quantity: u32,
}

/// Transformation of input items into output items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeDef {
    pub id: RecipeId,
    pub name: String,
    #[serde(default)]
    pub inputs: Vec<ItemAmount>,
    pub outputs: Vec<ItemAmount>,
    #[serde(default)]
    pub tools: Vec<ItemDefId>,
    #[serde(default)]
    pub building: Option<BuildingId>,
    pub skill: SkillId,
    pub labour_hours: f32,
    #[serde(default)]
    pub technology: Option<TechnologyId>,
}

impl Definition for RecipeDef {
    type Id = RecipeId;
    const CATEGORY: &'static str = "recipes";
    fn id(&self) -> &RecipeId {
        &self.id
    }
    fn references(&self) -> Vec<Reference> {
        let mut refs: Vec<_> = self
            .inputs
            .iter()
            .map(|a| Reference::new("inputs.item", "items", &a.item))
            .chain(
                self.outputs
                    .iter()
                    .map(|a| Reference::new("outputs.item", "items", &a.item)),
            )
            .chain(
                self.tools
                    .iter()
                    .map(|t| Reference::new("tools", "items", t)),
            )
            .collect();
        refs.push(Reference::new("skill", "skills", &self.skill));
        refs.extend(
            self.building
                .iter()
                .map(|b| Reference::new("building", "buildings", b)),
        );
        refs.extend(
            self.technology
                .iter()
                .map(|t| Reference::new("technology", "technologies", t)),
        );
        refs
    }
    fn validate(&self, env: &ValidationEnv<'_>) -> Vec<String> {
        let mut e = Vec::new();
        if self.outputs.is_empty() {
            e.push("outputs must not be empty".into());
        }
        if self
            .inputs
            .iter()
            .chain(&self.outputs)
            .any(|a| a.quantity == 0)
        {
            e.push("quantities must be at least 1".into());
        }
        require_positive(&mut e, "labour_hours", self.labour_hours);
        env.check_technology(&self.technology, &mut e);
        e
    }
}
