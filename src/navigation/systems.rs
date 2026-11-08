//! Navigation (path planning), movement and encumbrance systems.

use super::mover::{Locomotion, Mover, NavStatus, Step};
use super::occupancy::Occupancy;
use super::pathfinder::{PathResult, find_path};
use super::traversal::{TraversalMode, drop_m, traversal};
use crate::content::{InjuryId, InjurySeverity};
use crate::core::config::MovementConfig;
use crate::core::ids::{EntityId, SystemId};
use crate::core::rng::SimRng;
use crate::simulation::instances::ItemLocation;
use crate::simulation::{
    EventEnvelope, EventKind, SimEvent, SimulationContext, System, SystemPhase, UpdateSchedule,
    WorldState,
};
use crate::world::TilePos;

/// Plans routes for movers waiting on one, up to a per-tick budget.
#[derive(Debug, Default)]
pub struct NavigationSystem;

impl NavigationSystem {
    pub const ID: SystemId = SystemId::new("navigation");
}

impl System for NavigationSystem {
    fn id(&self) -> SystemId {
        Self::ID
    }
    fn phase(&self) -> SystemPhase {
        SystemPhase::Update
    }
    fn schedule(&self) -> UpdateSchedule {
        UpdateSchedule::EveryTick
    }
    fn update(&mut self, ctx: &mut SimulationContext<'_>) {
        let config = &ctx.config.movement;
        let mut budget = config.max_path_requests_per_tick;
        let candidates: Vec<EntityId> = ctx
            .world
            .components
            .movers
            .iter()
            .filter(|(_, m)| {
                m.step.is_none()
                    && (m.status == NavStatus::Planning
                        || (m.status == NavStatus::Moving && m.repath))
            })
            .map(|(id, _)| id)
            .collect();
        for id in candidates {
            if budget == 0 {
                break;
            }
            budget -= 1;
            let world: &WorldState = ctx.world;
            let (Some(mover), Some(pos)) = (
                world.components.movers.get(id),
                world.components.positions.get(id),
            ) else {
                continue;
            };
            let Some(goal) = mover.goal else {
                if let Some(m) = ctx.world.components.movers.get_mut(id) {
                    m.status = NavStatus::Idle;
                }
                continue;
            };
            let abilities = mover.capability.abilities(config);
            let result = find_path(
                world,
                config,
                &world.derived.occupancy,
                id,
                abilities,
                pos.tile,
                goal,
            );
            let epoch = world.navigation.epoch;
            let Some(m) = ctx.world.components.movers.get_mut(id) else {
                continue;
            };
            m.repath = false;
            m.wait_s = 0.0;
            match result {
                PathResult::Found { path, .. } => {
                    m.path = path.into();
                    m.path_epoch = epoch;
                    m.status = NavStatus::Moving;
                }
                PathResult::Unreachable | PathResult::TooFar => {
                    m.path.clear();
                    m.status = NavStatus::Failed;
                    m.goal = None;
                    m.locomotion = Locomotion::Standing;
                    ctx.emit(SimEvent::PathFailed { entity: id, goal });
                }
            }
        }
    }
}

/// Advances every mover by one tick: reservations, steps, hazards, effort.
#[derive(Debug, Default)]
pub struct MovementSystem;

impl MovementSystem {
    pub const ID: SystemId = SystemId::new("movement");
}

impl System for MovementSystem {
    fn id(&self) -> SystemId {
        Self::ID
    }
    fn phase(&self) -> SystemPhase {
        SystemPhase::Update
    }
    fn schedule(&self) -> UpdateSchedule {
        UpdateSchedule::EveryTick
    }
    fn update(&mut self, ctx: &mut SimulationContext<'_>) {
        if ctx.world.derived.map.is_empty() {
            return;
        }
        let dt = ctx.elapsed().seconds() as f32;
        let config = &ctx.config.movement;
        let mut occupancy = std::mem::take(&mut ctx.world.derived.occupancy);
        let mut rng = std::mem::replace(&mut ctx.world.rng, SimRng::from_seed(0));
        let ids: Vec<EntityId> = ctx.world.components.movers.entities().collect();
        let mut events = Vec::new();
        for id in ids {
            let Some(pos) = ctx.world.components.positions.get(id).map(|p| p.tile) else {
                continue;
            };
            let Some(mut mover) = ctx.world.components.movers.remove(id) else {
                continue;
            };
            let new_pos = advance(
                &mut mover,
                id,
                pos,
                ctx.world,
                &mut occupancy,
                config,
                &mut rng,
                dt,
                &mut events,
            );
            ctx.world.components.movers.insert(id, mover);
            if new_pos != pos
                && let Some(p) = ctx.world.components.positions.get_mut(id)
            {
                p.tile = new_pos;
            }
        }
        ctx.world.rng = rng;
        ctx.world.derived.occupancy = occupancy;
        for event in events {
            ctx.emit(event);
        }
    }
}

fn corner_blocked(
    world: &WorldState,
    config: &MovementConfig,
    from: TilePos,
    to: TilePos,
    mover: &Mover,
) -> bool {
    if !from.is_diagonal_to(to) {
        return false;
    }
    let abilities = mover.capability.abilities(config);
    traversal(world, config, TilePos::new(to.x, from.y), abilities).is_none()
        || traversal(world, config, TilePos::new(from.x, to.y), abilities).is_none()
}

fn path_is_stale(world: &WorldState, mover: &Mover) -> bool {
    let map = &world.derived.map;
    let epochs = &world.navigation.chunk_epochs;
    mover.path.iter().any(|p| {
        map.chunk_index(map.chunk_of(*p))
            .and_then(|c| epochs.get(c))
            .is_some_and(|e| *e > mover.path_epoch)
    })
}

/// A free neighbouring tile that does not lose ground towards the goal.
fn sidestep(
    world: &WorldState,
    config: &MovementConfig,
    occupancy: &Occupancy,
    id: EntityId,
    mover: &Mover,
    pos: TilePos,
    blocked: TilePos,
) -> Option<TilePos> {
    let goal = mover.goal?;
    let here = pos.octile(goal);
    let abilities = mover.capability.abilities(config);
    pos.neighbours8()
        .into_iter()
        .filter(|n| *n != blocked)
        .filter(|n| n.octile(goal) <= here + 0.5)
        .filter(|n| occupancy.has_room(*n, id, config.tile_capacity))
        .filter(|n| traversal(world, config, *n, abilities).is_some())
        .filter(|n| !corner_blocked(world, config, pos, *n, mover))
        .min_by(|a, b| a.octile(goal).total_cmp(&b.octile(goal)).then(a.cmp(b)))
}

fn fall_severity(height: f32, config: &MovementConfig) -> Option<(InjurySeverity, &str)> {
    if height < 1.0 {
        None
    } else if height < config.severe_fall_m * 0.5 {
        Some((InjurySeverity::Minor, config.fall_minor_injury.as_str()))
    } else if height < config.severe_fall_m {
        Some((InjurySeverity::Moderate, config.fall_minor_injury.as_str()))
    } else if height < config.severe_fall_m * 2.5 {
        Some((InjurySeverity::Severe, config.fall_severe_injury.as_str()))
    } else {
        Some((InjurySeverity::Critical, config.fall_severe_injury.as_str()))
    }
}

/// Advances one mover; returns its (possibly new) tile.
#[allow(clippy::too_many_arguments)]
fn advance(
    m: &mut Mover,
    id: EntityId,
    mut pos: TilePos,
    world: &WorldState,
    occupancy: &mut Occupancy,
    config: &MovementConfig,
    rng: &mut SimRng,
    dt: f32,
    events: &mut Vec<SimEvent>,
) -> TilePos {
    let active_mode = m.step.as_ref().filter(|_| m.delay_s <= 0.0).map(|s| s.mode);
    m.capability
        .exert(m.effective_gait, active_mode, dt, config);
    m.effective_gait = m.capability.allowed_gait(m.gait, config);

    if m.delay_s > 0.0 {
        m.delay_s = (m.delay_s - dt).max(0.0);
        return pos;
    }
    if m.status != NavStatus::Moving {
        if m.step.is_none() {
            m.locomotion = Locomotion::Standing;
            return pos;
        }
    } else if m.step.is_none() {
        if m.repath {
            return pos;
        }
        let Some(&next) = m.path.front() else {
            m.status = NavStatus::Arrived;
            m.locomotion = Locomotion::Standing;
            if let Some(goal) = m.goal.take() {
                events.push(SimEvent::DestinationReached {
                    entity: id,
                    tile: goal,
                });
            }
            return pos;
        };
        let Some(t) = traversal(world, config, next, m.capability.abilities(config)) else {
            m.repath = true;
            return pos;
        };
        if !next.is_adjacent(pos) || corner_blocked(world, config, pos, next, m) {
            m.repath = true;
            return pos;
        }
        if !occupancy.has_room(next, id, config.tile_capacity) {
            m.locomotion = Locomotion::Waiting;
            let before = m.wait_s;
            m.wait_s += dt;
            if m.wait_s >= config.give_up_wait_seconds {
                m.status = NavStatus::Failed;
                m.path.clear();
                if let Some(goal) = m.goal.take() {
                    events.push(SimEvent::PathFailed { entity: id, goal });
                }
            } else if before < config.repath_wait_seconds && m.wait_s >= config.repath_wait_seconds
            {
                if let Some(side) = sidestep(world, config, occupancy, id, m, pos, next) {
                    m.path.push_front(side);
                    m.repath_after_step = true;
                } else {
                    m.repath = true;
                }
            }
            return pos;
        }
        if m.capability.speed_mps(m.effective_gait, t.mode, config) <= 0.0 {
            m.locomotion = Locomotion::Standing;
            return pos;
        }
        m.wait_s = 0.0;
        occupancy.add(next, id);
        m.path.pop_front();
        m.step = Some(Step {
            to: next,
            progress: 0.0,
            mode: t.mode,
            cost: t.cost,
            diagonal: pos.is_diagonal_to(next),
        });
        if t.delay_s > 0.0 {
            m.delay_s = t.delay_s;
            m.locomotion = Locomotion::OpeningDoor;
        }

        // Hazards at the start of a step.
        let mobility = m.capability.mobility.max(0.2);
        let fatigue = 1.0 + 2.0 * m.capability.fatigue;
        let overload =
            1.0 + 2.0 * (m.capability.encumbrance() - config.encumbrance_soft_limit).max(0.0);
        match t.mode {
            TraversalMode::Walk | TraversalMode::Wade => {
                let gait = m.effective_gait.config(config);
                let p = config.stumble_chance
                    * t.roughness
                    * gait.stumble_multiplier
                    * fatigue
                    * overload
                    / mobility;
                if rng.chance(f64::from(p)) {
                    m.delay_s += config.stumble_seconds;
                    m.capability.stamina = (m.capability.stamina - 0.03).max(0.0);
                    m.locomotion = Locomotion::Stumbling;
                    events.push(SimEvent::CharacterStumbled {
                        entity: id,
                        tile: pos,
                    });
                    if m.effective_gait >= super::mover::Gait::Run
                        && rng.chance(f64::from(config.stumble_injury_chance))
                    {
                        events.push(SimEvent::CharacterInjured {
                            character: id,
                            severity: InjurySeverity::Minor,
                            injury: Some(InjuryId::new(config.stumble_injury.clone())),
                            cause: None,
                        });
                    }
                }
            }
            TraversalMode::Climb => {
                let p = config.climb_fall_chance * fatigue * overload / mobility;
                if rng.chance(f64::from(p)) {
                    let tile_height = world.derived.map.tile(next).map_or(0.0, |t| {
                        t.slope_deg.to_radians().tan() * world.derived.map.tile_size_m()
                    });
                    let height = drop_m(world, pos, next).abs().max(tile_height);
                    // Land on the lower of the two tiles.
                    let lower = if drop_m(world, pos, next) > 0.0 {
                        next
                    } else {
                        pos
                    };
                    if lower == next {
                        occupancy.remove(pos, id);
                        pos = next;
                    } else {
                        occupancy.remove(next, id);
                    }
                    m.step = None;
                    m.repath = true;
                    m.delay_s = 5.0 + 3.0 * height;
                    m.locomotion = Locomotion::Fallen;
                    events.push(SimEvent::CharacterFell {
                        entity: id,
                        tile: pos,
                        height_m: height,
                    });
                    if let Some((severity, injury)) = fall_severity(height, config) {
                        events.push(SimEvent::CharacterInjured {
                            character: id,
                            severity,
                            injury: Some(InjuryId::new(injury)),
                            cause: None,
                        });
                    }
                    return pos;
                }
            }
            TraversalMode::Swim => {}
        }
        if m.delay_s > 0.0 {
            return pos;
        }
    }

    let Some(step) = m.step.as_mut() else {
        return pos;
    };
    let speed = m.capability.speed_mps(m.effective_gait, step.mode, config);
    let distance = world.derived.map.tile_size_m()
        * if step.diagonal {
            std::f32::consts::SQRT_2
        } else {
            1.0
        };
    step.progress += speed * dt / (distance * step.cost.max(0.05));
    m.locomotion = match step.mode {
        TraversalMode::Walk => Locomotion::Walking,
        TraversalMode::Wade => Locomotion::Wading,
        TraversalMode::Swim => Locomotion::Swimming,
        TraversalMode::Climb => Locomotion::Climbing,
    };
    if step.progress >= 1.0 {
        let to = step.to;
        occupancy.remove(pos, id);
        pos = to;
        m.step = None;
        m.distance_m += distance;
        if m.repath_after_step {
            m.repath_after_step = false;
            m.repath = true;
        } else if path_is_stale(world, m) {
            m.repath = true;
        }
    }
    pos
}

/// Recomputes carried weight whenever items move.
#[derive(Debug, Default)]
pub struct EncumbranceSystem;

impl EncumbranceSystem {
    pub const ID: SystemId = SystemId::new("encumbrance");
}

/// Sums the weight of items carried by `holder`.
pub fn carried_weight(
    world: &WorldState,
    content: &crate::content::ContentRegistry,
    holder: EntityId,
) -> f32 {
    world
        .components
        .items
        .iter()
        .filter(|(_, i)| i.location == ItemLocation::Carried(holder))
        .map(|(_, i)| content.items().get(&i.def).map_or(0.0, |d| d.weight_kg) * i.quantity as f32)
        .sum()
}

impl System for EncumbranceSystem {
    fn id(&self) -> SystemId {
        Self::ID
    }
    fn phase(&self) -> SystemPhase {
        SystemPhase::PostUpdate
    }
    fn schedule(&self) -> UpdateSchedule {
        UpdateSchedule::EventDriven
    }
    fn subscriptions(&self) -> &[EventKind] {
        &[EventKind::ItemCreated, EventKind::ItemTransferred]
    }
    fn update(&mut self, _ctx: &mut SimulationContext<'_>) {}
    fn handle_event(&mut self, event: &EventEnvelope, ctx: &mut SimulationContext<'_>) {
        let holders: Vec<EntityId> = match &event.event {
            SimEvent::ItemTransferred { from, to, .. } => {
                from.iter().chain(to.iter()).copied().collect()
            }
            SimEvent::ItemCreated { item, .. } => {
                match ctx.world.components.items.get(*item).map(|i| i.location) {
                    Some(ItemLocation::Carried(holder)) => vec![holder],
                    _ => Vec::new(),
                }
            }
            _ => Vec::new(),
        };
        for holder in holders {
            let weight = carried_weight(ctx.world, ctx.content, holder);
            if let Some(m) = ctx.world.components.movers.get_mut(holder) {
                m.capability.carried_kg = weight;
            }
        }
    }
}
