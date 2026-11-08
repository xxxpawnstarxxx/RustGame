//! Commands: the only way for code outside the tick (UI, debug tools, timers,
//! later AI planners and scripts) to request changes to world state.
//!
//! Commands are queued in the persistent [`CommandQueue`] and applied in FIFO
//! order at a fixed point of the next tick, so outcomes are independent of
//! frame timing. Each command is validated first; a rejected command leaves
//! the world unchanged.

use crate::content::{
    AnimalId, BuildingId, CharacterTemplateId, ItemDefId, RoutineId, StructureCell,
};
use crate::core::error::{SimError, SimResult};
use crate::core::ids::{EntityId, TimerId};
use crate::core::time::{GameDuration, GameInstant, TimeUnit};
use crate::entities::{EntityKind, Name};
use crate::environment::vegetation;
use crate::navigation::{Gait, MovementCapability, Mover, carried_weight, traversal};
use crate::simulation::context::SimulationContext;
use crate::simulation::events::SimEvent;
use crate::simulation::instances::{
    BuildingRecord, CharacterRecord, ItemInstance, ItemLocation, Position, SkillLevel,
};
use crate::simulation::time::{Recurrence, RoutineAssignment, TimerAction};
use crate::world::{DoorState, TilePos};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// A requested change to world state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Command {
    /// Creates a bare entity with optional name and daily routine.
    SpawnEntity {
        kind: EntityKind,
        name: Option<String>,
        routine: Option<RoutineId>,
    },
    /// Creates a character from a template at a tile.
    SpawnCharacter {
        template: CharacterTemplateId,
        name: Option<String>,
        tile: TilePos,
    },
    /// Creates an item stack on the ground or carried by someone.
    SpawnItem {
        item: ItemDefId,
        quantity: u32,
        location: ItemLocation,
    },
    /// Moves an item to the ground or to a holder.
    TransferItem {
        item: EntityId,
        to: ItemLocation,
    },
    /// Destroys an entity and all of its components.
    DestroyEntity {
        entity: EntityId,
    },
    AssignRoutine {
        entity: EntityId,
        routine: RoutineId,
    },
    ClearRoutine {
        entity: EntityId,
    },
    /// Sends a mover to a tile.
    MoveTo {
        entity: EntityId,
        goal: TilePos,
        gait: Option<Gait>,
    },
    StopMoving {
        entity: EntityId,
    },
    SetGait {
        entity: EntityId,
        gait: Gait,
    },
    /// Places a building with its layout's top-left corner at `origin`.
    PlaceBuilding {
        building: BuildingId,
        origin: TilePos,
    },
    SetDoor {
        tile: TilePos,
        state: DoorState,
    },
    /// Removes a fraction of the canopy around a tile.
    FellTrees {
        tile: TilePos,
        fraction: f32,
    },
    /// Removes animals from the wildlife region containing `tile`.
    HuntWildlife {
        tile: TilePos,
        animal: AnimalId,
        count: u32,
    },
    SetFactionRelation {
        faction: EntityId,
        other: EntityId,
        value: i32,
    },
    /// Schedules a delayed and/or recurring action. With `delay`, the first
    /// firing is `delay` after now; otherwise the recurrence's next occurrence.
    ScheduleTimer {
        label: String,
        delay: Option<GameDuration>,
        recurrence: Recurrence,
        action: Box<TimerAction>,
        repeat_limit: Option<u32>,
    },
    CancelTimer {
        timer: TimerId,
    },
}

/// Persistent FIFO of commands awaiting application.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CommandQueue {
    pending: VecDeque<Command>,
}

impl CommandQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, command: Command) {
        self.pending.push_back(command);
    }

    /// Removes and returns every queued command in order.
    pub fn drain(&mut self) -> Vec<Command> {
        self.pending.drain(..).collect()
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Command> {
        self.pending.iter()
    }
}

fn invalid(message: impl Into<String>) -> SimError {
    SimError::InvalidCommand(message.into())
}

fn require_kind(ctx: &SimulationContext<'_>, entity: EntityId, kind: EntityKind) -> SimResult<()> {
    let actual = ctx.world.entities.require(entity)?;
    if actual == kind {
        Ok(())
    } else {
        Err(invalid(format!("{entity} is a {actual}, not a {kind}")))
    }
}

/// Applies one command. On error the world is left unchanged.
pub fn apply(command: Command, ctx: &mut SimulationContext<'_>) -> SimResult<()> {
    match command {
        Command::SpawnEntity {
            kind,
            name,
            routine,
        } => {
            if let Some(routine) = &routine {
                ctx.content.routines().require(routine)?;
            }
            let entity = ctx.world.entities.create(kind);
            if let Some(name) = name {
                ctx.world.components.names.insert(entity, Name::new(name));
            }
            if let Some(routine) = routine {
                let now = ctx.now();
                ctx.world
                    .components
                    .routines
                    .insert(entity, RoutineAssignment::new(routine, now));
            }
            ctx.emit(SimEvent::EntityCreated { entity, kind });
        }
        Command::SpawnCharacter {
            template,
            name,
            tile,
        } => spawn_character(ctx, &template, name, tile)?,
        Command::SpawnItem {
            item,
            quantity,
            location,
        } => {
            spawn_item(ctx, &item, quantity, location)?;
        }
        Command::TransferItem { item, to } => transfer_item(ctx, item, to)?,
        Command::DestroyEntity { entity } => destroy(ctx, entity)?,
        Command::AssignRoutine { entity, routine } => {
            ctx.world.entities.require(entity)?;
            ctx.content.routines().require(&routine)?;
            let now = ctx.now();
            ctx.world
                .components
                .routines
                .insert(entity, RoutineAssignment::new(routine, now));
        }
        Command::ClearRoutine { entity } => {
            ctx.world.entities.require(entity)?;
            ctx.world.components.routines.remove(entity);
        }
        Command::MoveTo { entity, goal, gait } => {
            if !ctx.world.derived.map.in_bounds(goal) {
                return Err(invalid(format!("goal {goal} is outside the map")));
            }
            let mover = ctx
                .world
                .components
                .movers
                .get_mut(entity)
                .ok_or_else(|| invalid(format!("{entity} cannot move itself")))?;
            if let Some(gait) = gait {
                mover.gait = gait;
            }
            mover.travel_to(goal);
        }
        Command::StopMoving { entity } => {
            ctx.world
                .components
                .movers
                .get_mut(entity)
                .ok_or_else(|| invalid(format!("{entity} cannot move itself")))?
                .stop();
        }
        Command::SetGait { entity, gait } => {
            ctx.world
                .components
                .movers
                .get_mut(entity)
                .ok_or_else(|| invalid(format!("{entity} cannot move itself")))?
                .gait = gait;
        }
        Command::PlaceBuilding { building, origin } => {
            let def = ctx.content.buildings().require(&building)?;
            ctx.world
                .structures
                .check_placement(def, origin, &ctx.world.derived.map)?;
            // Walls and fences cannot be raised on top of people.
            for (dx, dy, cell) in def.cells() {
                let pos = origin.offset(dx, dy);
                if matches!(cell, StructureCell::Wall | StructureCell::Fence)
                    && !ctx.world.derived.occupancy.at(pos).is_empty()
                {
                    return Err(invalid(format!("someone is standing at {pos}")));
                }
            }
            let entity = ctx.world.entities.create(EntityKind::Structure);
            let world = &mut *ctx.world;
            let changed = world
                .structures
                .place(def, origin, entity, &world.derived.map)?;
            world
                .components
                .names
                .insert(entity, Name::new(def.name.clone()));
            world
                .components
                .positions
                .insert(entity, Position { tile: origin });
            world.components.buildings.insert(
                entity,
                BuildingRecord {
                    def: building.clone(),
                    origin,
                },
            );
            world.invalidate_tiles(&changed);
            ctx.emit(SimEvent::EntityCreated {
                entity,
                kind: EntityKind::Structure,
            });
            ctx.emit(SimEvent::StructureBuilt {
                structure: entity,
                building,
            });
        }
        Command::SetDoor { tile, state } => {
            ctx.world.structures.set_door(tile, state)?;
            ctx.world.invalidate_tiles(&[tile]);
            ctx.emit(SimEvent::DoorChanged { tile, state });
        }
        Command::FellTrees { tile, fraction } => {
            if !(0.0..=1.0).contains(&fraction) {
                return Err(invalid("fraction must be between 0 and 1"));
            }
            let cell = ctx
                .world
                .derived
                .statics
                .ground_cell(tile)
                .ok_or_else(|| invalid(format!("{tile} is outside the map")))?;
            let removed = vegetation::fell(&mut ctx.world.environment.ground, cell, fraction);
            if removed <= 0.0 {
                return Err(invalid(format!("there are no trees at {tile}")));
            }
            ctx.world.invalidate_tiles(&[tile]);
            ctx.emit(SimEvent::TreesFelled {
                tile,
                canopy_removed: removed,
            });
        }
        Command::HuntWildlife {
            tile,
            animal,
            count,
        } => {
            let region = ctx
                .world
                .derived
                .statics
                .wildlife_region(tile)
                .ok_or_else(|| invalid(format!("{tile} is outside the map")))?;
            let wildlife = &mut ctx.world.environment.wildlife;
            let species = wildlife
                .species_index(&animal)
                .ok_or_else(|| invalid(format!("'{animal}' is not wild game here")))?;
            let pop = &mut wildlife.regions[region].populations[species];
            let taken = (count as f32).min(pop.count.floor());
            if taken < 1.0 {
                return Err(invalid(format!("no {animal} left to hunt in this region")));
            }
            let infected_share = if pop.count > 0.0 {
                pop.infected / pop.count
            } else {
                0.0
            };
            pop.count -= taken;
            pop.infected = (pop.infected - taken * infected_share).max(0.0);
            pop.hunting_pressure += taken / (pop.capacity + 1.0);
            ctx.emit(SimEvent::WildlifeHunted {
                region: region as u32,
                animal,
                count: taken as u32,
            });
        }
        Command::SetFactionRelation {
            faction,
            other,
            value,
        } => {
            require_kind(ctx, faction, EntityKind::Faction)?;
            require_kind(ctx, other, EntityKind::Faction)?;
            if faction == other || !(-100..=100).contains(&value) {
                return Err(invalid(
                    "relations need two different factions and a value in -100..=100",
                ));
            }
            let record = ctx
                .world
                .components
                .factions
                .get_mut(faction)
                .ok_or_else(|| invalid(format!("{faction} has no faction record")))?;
            let previous = record.set_relation(other, value);
            if previous != value {
                ctx.emit(SimEvent::FactionRelationChanged {
                    faction,
                    other,
                    previous,
                    current: value,
                });
            }
        }
        Command::ScheduleTimer {
            label,
            delay,
            recurrence,
            action,
            repeat_limit,
        } => {
            recurrence.validate(ctx.calendar)?;
            if repeat_limit == Some(0) {
                return Err(invalid("repeat_limit must be at least 1"));
            }
            let now = ctx.now();
            let first = match delay {
                Some(delay) => now + delay,
                None => recurrence
                    .next_after(ctx.calendar, now)?
                    .ok_or_else(|| invalid("a one-shot timer needs a delay"))?,
            };
            ctx.world
                .timers
                .schedule(label, first, recurrence, *action, repeat_limit);
        }
        Command::CancelTimer { timer } => {
            ctx.world
                .timers
                .cancel(timer)
                .ok_or(SimError::NoSuchTimer(timer))?;
        }
    }
    Ok(())
}

fn spawn_character(
    ctx: &mut SimulationContext<'_>,
    template: &CharacterTemplateId,
    name: Option<String>,
    tile: TilePos,
) -> SimResult<()> {
    let def = ctx.content.characters().require(template)?;
    let capability = MovementCapability {
        walk_speed_mps: def.walking_speed_mps,
        carry_capacity_kg: def.carry_capacity_kg,
        can_swim: def.can_swim,
        can_climb: def.can_climb,
        mobility: 1.0,
        stamina: 1.0,
        fatigue: 0.0,
        carried_kg: 0.0,
    };
    let walkable = traversal(
        ctx.world,
        &ctx.config.movement,
        tile,
        capability.abilities(&ctx.config.movement),
    )
    .is_some_and(|t| t.mode == crate::navigation::TraversalMode::Walk);
    if !walkable {
        return Err(invalid(format!("a character cannot stand at {tile}")));
    }
    if !ctx.world.derived.occupancy.has_room(
        tile,
        EntityId::new(u32::MAX, u32::MAX),
        ctx.config.movement.tile_capacity,
    ) {
        return Err(invalid(format!("{tile} is crowded")));
    }
    for item in &def.starting_items {
        ctx.content.items().require(&item.item)?;
    }

    let rng = &mut ctx.world.rng;
    let name = name
        .or_else(|| rng.pick(&def.given_names).cloned())
        .unwrap_or_else(|| def.name.clone());
    let age_years = rng.range_u32(def.age_years[0]..def.age_years[1] + 1);
    let skills: Vec<SkillLevel> = def
        .skills
        .iter()
        .map(|s| SkillLevel {
            skill: s.skill.clone(),
            level: rng.range_u32(u32::from(s.min)..u32::from(s.max) + 1) as u8,
        })
        .collect();
    let year = ctx.calendar.duration(1, TimeUnit::Year).seconds();
    let now = ctx.now();
    let birth =
        GameInstant::from_seconds(now.seconds().saturating_sub(u64::from(age_years) * year));

    // Belongs to the nearest settlement and its lord.
    let home = ctx
        .world
        .components
        .settlements
        .iter()
        .min_by(|a, b| {
            a.1.centre
                .euclidean(tile)
                .total_cmp(&b.1.centre.euclidean(tile))
                .then(a.0.cmp(&b.0))
        })
        .map(|(id, s)| (id, s.faction));
    let routine = def
        .profession
        .as_ref()
        .and_then(|p| ctx.content.professions().get(p))
        .and_then(|p| p.routine.clone());

    let entity = ctx.world.entities.create(EntityKind::Character);
    let world = &mut *ctx.world;
    world.components.names.insert(entity, Name::new(name));
    world.components.characters.insert(
        entity,
        CharacterRecord {
            template: template.clone(),
            body: def.body.clone(),
            profession: def.profession.clone(),
            birth,
            skills,
            home: home.map(|h| h.0),
            faction: home.and_then(|h| h.1),
        },
    );
    world.components.positions.insert(entity, Position { tile });
    world
        .components
        .movers
        .insert(entity, Mover::new(capability));
    world.derived.occupancy.add(tile, entity);
    if let Some(routine) = routine {
        world
            .components
            .routines
            .insert(entity, RoutineAssignment::new(routine, now));
    }
    ctx.emit(SimEvent::EntityCreated {
        entity,
        kind: EntityKind::Character,
    });
    for item in &def.starting_items {
        spawn_item(
            ctx,
            &item.item,
            item.quantity,
            ItemLocation::Carried(entity),
        )?;
    }
    Ok(())
}

fn spawn_item(
    ctx: &mut SimulationContext<'_>,
    item: &ItemDefId,
    quantity: u32,
    location: ItemLocation,
) -> SimResult<EntityId> {
    let def = ctx.content.items().require(item)?;
    if quantity == 0 || quantity > def.stack_size {
        return Err(invalid(format!(
            "quantity {quantity} is outside 1..={} for '{item}'",
            def.stack_size
        )));
    }
    match location {
        ItemLocation::Ground(tile) if !ctx.world.derived.map.in_bounds(tile) => {
            return Err(invalid(format!("{tile} is outside the map")));
        }
        ItemLocation::Carried(holder) => {
            ctx.world.entities.require(holder)?;
        }
        ItemLocation::Ground(_) => {}
    }
    let entity = ctx.world.entities.create(EntityKind::Item);
    ctx.world.components.items.insert(
        entity,
        ItemInstance {
            def: item.clone(),
            quantity,
            quality: 0.5,
            condition: 1.0,
            location,
        },
    );
    if let ItemLocation::Ground(tile) = location {
        ctx.world
            .components
            .positions
            .insert(entity, Position { tile });
    }
    ctx.emit(SimEvent::EntityCreated {
        entity,
        kind: EntityKind::Item,
    });
    ctx.emit(SimEvent::ItemCreated {
        item: entity,
        definition: item.clone(),
    });
    Ok(entity)
}

fn holder_of(location: ItemLocation) -> Option<EntityId> {
    match location {
        ItemLocation::Carried(holder) => Some(holder),
        ItemLocation::Ground(_) => None,
    }
}

fn transfer_item(
    ctx: &mut SimulationContext<'_>,
    item: EntityId,
    to: ItemLocation,
) -> SimResult<()> {
    require_kind(ctx, item, EntityKind::Item)?;
    match to {
        ItemLocation::Carried(holder) => {
            ctx.world.entities.require(holder)?;
            if holder == item {
                return Err(invalid("an item cannot carry itself"));
            }
        }
        ItemLocation::Ground(tile) if !ctx.world.derived.map.in_bounds(tile) => {
            return Err(invalid(format!("{tile} is outside the map")));
        }
        ItemLocation::Ground(_) => {}
    }
    let instance = ctx
        .world
        .components
        .items
        .get_mut(item)
        .ok_or_else(|| invalid(format!("{item} has no item record")))?;
    let from = holder_of(instance.location);
    instance.location = to;
    match to {
        ItemLocation::Ground(tile) => {
            ctx.world
                .components
                .positions
                .insert(item, Position { tile });
        }
        ItemLocation::Carried(_) => {
            ctx.world.components.positions.remove(item);
        }
    }
    ctx.emit(SimEvent::ItemTransferred {
        item,
        from,
        to: holder_of(to),
    });
    Ok(())
}

fn destroy(ctx: &mut SimulationContext<'_>, entity: EntityId) -> SimResult<()> {
    let kind = ctx.world.entities.require(entity)?;
    // Carried items fall where their holder stood.
    let carried: Vec<EntityId> = ctx
        .world
        .components
        .items
        .iter()
        .filter(|(_, i)| i.location == ItemLocation::Carried(entity))
        .map(|(id, _)| id)
        .collect();
    if !carried.is_empty() {
        let drop_at = ctx.world.components.positions.get(entity).map(|p| p.tile);
        for item in carried {
            match drop_at {
                Some(tile) => transfer_item(ctx, item, ItemLocation::Ground(tile))?,
                None => destroy(ctx, item)?,
            }
        }
    }
    if let Some(ItemLocation::Carried(holder)) =
        ctx.world.components.items.get(entity).map(|i| i.location)
    {
        let content = ctx.content;
        let weight = carried_weight(ctx.world, content, holder)
            - ctx.world.components.items.get(entity).map_or(0.0, |i| {
                content.items().get(&i.def).map_or(0.0, |d| d.weight_kg) * i.quantity as f32
            });
        if let Some(m) = ctx.world.components.movers.get_mut(holder) {
            m.capability.carried_kg = weight.max(0.0);
        }
    }
    if let Some(mover) = ctx.world.components.movers.get(entity) {
        let step = mover.step.as_ref().map(|s| s.to);
        if let Some(pos) = ctx.world.components.positions.get(entity).map(|p| p.tile) {
            ctx.world.derived.occupancy.remove(pos, entity);
        }
        if let Some(to) = step {
            ctx.world.derived.occupancy.remove(to, entity);
        }
    }
    if kind == EntityKind::Structure {
        let changed = ctx.world.structures.remove_building(entity);
        ctx.world.invalidate_tiles(&changed);
        ctx.emit(SimEvent::StructureRemoved { structure: entity });
    }
    ctx.world.entities.destroy(entity)?;
    ctx.world.components.remove_all(entity);
    ctx.emit(SimEvent::EntityDestroyed { entity, kind });
    Ok(())
}
