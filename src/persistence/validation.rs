//! Semantic validation of a loaded world.
//!
//! Deserialisation proves a save is well-formed JSON of the right shape;
//! this proves it is *consistent*: components belong to live entities of the
//! right kind, references to content definitions resolve, positions lie on
//! the map, and carried items have live holders.

use crate::content::ContentRegistry;
use crate::core::ids::EntityId;
use crate::entities::EntityKind;
use crate::simulation::{ItemLocation, WorldState};

/// Returns every inconsistency found (empty = valid).
pub fn validate_world(world: &WorldState, content: &ContentRegistry) -> Vec<String> {
    let mut problems = Vec::new();
    let entities = &world.entities;
    let mut check_kind =
        |store: &str, id: EntityId, expected: Option<EntityKind>| match entities.kind(id) {
            None => problems.push(format!("{store} component on missing entity {id}")),
            Some(kind) if expected.is_some_and(|e| e != kind) => {
                problems.push(format!("{store} component on {id}, which is a {kind}"));
            }
            Some(_) => {}
        };
    let c = &world.components;
    c.names
        .entities()
        .for_each(|id| check_kind("name", id, None));
    c.routines
        .entities()
        .for_each(|id| check_kind("routine", id, None));
    c.positions
        .entities()
        .for_each(|id| check_kind("position", id, None));
    c.movers
        .entities()
        .for_each(|id| check_kind("mover", id, None));
    c.characters
        .entities()
        .for_each(|id| check_kind("character", id, Some(EntityKind::Character)));
    c.items
        .entities()
        .for_each(|id| check_kind("item", id, Some(EntityKind::Item)));
    c.settlements
        .entities()
        .for_each(|id| check_kind("settlement", id, Some(EntityKind::Settlement)));
    c.factions
        .entities()
        .for_each(|id| check_kind("faction", id, Some(EntityKind::Faction)));
    c.buildings
        .entities()
        .for_each(|id| check_kind("building", id, Some(EntityKind::Structure)));

    let mut missing = |category: &str, id: &str, owner: EntityId| {
        problems.push(format!("{owner} refers to unknown {category} '{id}'"));
    };
    for (id, r) in c.routines.iter() {
        if !content.routines().contains(&r.routine) {
            missing("routine", r.routine.as_str(), id);
        }
    }
    for (id, ch) in c.characters.iter() {
        if !content.characters().contains(&ch.template) {
            missing("character template", ch.template.as_str(), id);
        }
        if !content.body_templates().contains(&ch.body) {
            missing("body template", ch.body.as_str(), id);
        }
        if let Some(p) = &ch.profession
            && !content.professions().contains(p)
        {
            missing("profession", p.as_str(), id);
        }
    }
    for (id, item) in c.items.iter() {
        if !content.items().contains(&item.def) {
            missing("item", item.def.as_str(), id);
        }
    }
    for (id, f) in c.factions.iter() {
        if !content.factions().contains(&f.def) {
            missing("faction", f.def.as_str(), id);
        }
    }
    for (id, b) in c.buildings.iter() {
        if !content.buildings().contains(&b.def) {
            missing("building", b.def.as_str(), id);
        }
    }
    if !content.worldgen().contains(&world.map_info.preset) {
        problems.push(format!(
            "world uses unknown generation preset '{}'",
            world.map_info.preset
        ));
    }

    for (id, item) in c.items.iter() {
        if let ItemLocation::Carried(holder) = item.location
            && !world.entities.is_alive(holder)
        {
            problems.push(format!("item {id} is carried by missing entity {holder}"));
        }
    }
    let (w, h) = (world.map_info.width as i32, world.map_info.height as i32);
    if w > 0 && h > 0 {
        for (id, p) in c.positions.iter() {
            if p.tile.x < 0 || p.tile.y < 0 || p.tile.x >= w || p.tile.y >= h {
                problems.push(format!("{id} is positioned off the map at {}", p.tile));
            }
        }
    }
    for (pos, tile) in world.structures.iter() {
        if !world.entities.is_alive(tile.building) {
            problems.push(format!(
                "structure at {pos} belongs to missing entity {}",
                tile.building
            ));
        }
    }
    problems
}
