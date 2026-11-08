//! Time-based daily routines (NPC schedules).
//!
//! A [`RoutineAssignment`] component links an entity to a
//! [`DailyRoutineDef`] from content. The [`RoutineSystem`] checks, once per
//! game minute, only those entities whose next transition time has passed,
//! and emits [`SimEvent::ScheduledActivityChanged`] when the scheduled
//! activity changes. Later AI stages treat the scheduled activity as the
//! default intention, which needs and events can override.

use crate::content::{ActivityId, DailyRoutineDef, RoutineId};
use crate::core::error::SimResult;
use crate::core::ids::{EntityId, SystemId};
use crate::core::time::{Calendar, GameDuration, GameInstant, TimeUnit};
use crate::simulation::context::SimulationContext;
use crate::simulation::events::SimEvent;
use crate::simulation::system::{System, SystemPhase, UpdateSchedule};
use serde::{Deserialize, Serialize};

/// Component: an entity follows a daily routine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutineAssignment {
    pub routine: RoutineId,
    /// Activity currently scheduled (`None` until first evaluated).
    pub current: Option<ActivityId>,
    /// When the scheduled activity next changes.
    pub next_transition: GameInstant,
}

impl RoutineAssignment {
    /// A new assignment, evaluated on the routine system's next run.
    pub fn new(routine: RoutineId, now: GameInstant) -> Self {
        Self {
            routine,
            current: None,
            next_transition: now,
        }
    }
}

/// The scheduled activity at an instant and when it ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutineSegment {
    pub activity: ActivityId,
    pub next_transition: GameInstant,
}

fn block_starts<'a>(
    def: &'a DailyRoutineDef,
    calendar: &Calendar,
    weekday: u32,
) -> SimResult<Vec<(u64, &'a ActivityId)>> {
    def.blocks_for(calendar, weekday)
        .iter()
        .map(|b| Ok((calendar.time_of_day_seconds(b.start)?, &b.activity)))
        .collect()
}

/// Resolves which activity a routine schedules at `t`.
///
/// Before the first block of a day, the last block of the previous day
/// (which may use a different weekday's blocks) still applies.
pub fn resolve_routine(
    def: &DailyRoutineDef,
    calendar: &Calendar,
    t: GameInstant,
) -> SimResult<RoutineSegment> {
    let day = calendar.days_since_epoch(t);
    let seconds = calendar.seconds_into_day(t);
    let today = block_starts(def, calendar, calendar.weekday_of_day(day))?;
    let position = today.partition_point(|(start, _)| *start <= seconds);

    let activity = if position == 0 {
        let yesterday = block_starts(
            def,
            calendar,
            calendar.weekday_of_day(day.saturating_sub(1)),
        )?;
        yesterday.last().or(today.last()).map(|(_, a)| (*a).clone())
    } else {
        Some(today[position - 1].1.clone())
    };
    let next_transition = if position < today.len() {
        calendar.instant_of_day(day) + GameDuration::from_seconds(today[position].0)
    } else {
        let tomorrow = block_starts(def, calendar, calendar.weekday_of_day(day + 1))?;
        let first = tomorrow.first().map_or(0, |(s, _)| *s);
        calendar.instant_of_day(day + 1) + GameDuration::from_seconds(first)
    };
    let activity = activity.ok_or_else(|| {
        crate::core::error::SimError::content(
            format!("routine '{}'", def.id),
            "routine has no blocks",
        )
    })?;
    Ok(RoutineSegment {
        activity,
        next_transition,
    })
}

/// Advances entities' scheduled activities.
#[derive(Debug, Default)]
pub struct RoutineSystem;

impl RoutineSystem {
    pub const ID: SystemId = SystemId::new("routines");
}

impl System for RoutineSystem {
    fn id(&self) -> SystemId {
        Self::ID
    }

    fn phase(&self) -> SystemPhase {
        SystemPhase::Update
    }

    fn schedule(&self) -> UpdateSchedule {
        UpdateSchedule::Interval {
            amount: 1,
            unit: TimeUnit::Minute,
        }
    }

    fn update(&mut self, ctx: &mut SimulationContext<'_>) {
        let now = ctx.now();
        let due: Vec<EntityId> = ctx
            .world
            .components
            .routines
            .iter()
            .filter(|(_, a)| a.current.is_none() || a.next_transition <= now)
            .map(|(id, _)| id)
            .collect();
        for entity in due {
            let Some(assignment) = ctx.world.components.routines.get(entity) else {
                continue;
            };
            let Some(def) = ctx.content.routines().get(&assignment.routine) else {
                tracing::warn!(%entity, routine = %assignment.routine, "routine definition missing; assignment removed");
                ctx.world.components.routines.remove(entity);
                continue;
            };
            let segment = match resolve_routine(def, ctx.calendar, now) {
                Ok(segment) => segment,
                Err(err) => {
                    tracing::warn!(%entity, %err, "routine could not be resolved; assignment removed");
                    ctx.world.components.routines.remove(entity);
                    continue;
                }
            };
            let previous = assignment.current.clone();
            if previous.as_ref() != Some(&segment.activity) {
                ctx.emit(SimEvent::ScheduledActivityChanged {
                    entity,
                    previous,
                    current: segment.activity.clone(),
                });
            }
            if let Some(assignment) = ctx.world.components.routines.get_mut(entity) {
                assignment.current = Some(segment.activity);
                assignment.next_transition = segment.next_transition;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::RoutineBlockDef;
    use crate::core::config::CalendarConfig;
    use crate::core::time::TimeOfDay;
    use std::collections::BTreeMap;

    fn blocks(spec: &[(u32, &str)]) -> Vec<RoutineBlockDef> {
        spec.iter()
            .map(|(h, a)| RoutineBlockDef {
                start: TimeOfDay::hm(*h, 0),
                activity: ActivityId::new(*a),
            })
            .collect()
    }

    fn routine() -> DailyRoutineDef {
        let mut overrides = BTreeMap::new();
        overrides.insert(
            "Sunday".to_string(),
            blocks(&[(7, "mass"), (10, "leisure"), (22, "sleep")]),
        );
        DailyRoutineDef {
            id: RoutineId::new("test"),
            name: "Test".into(),
            description: String::new(),
            blocks: blocks(&[(5, "work"), (12, "dinner"), (13, "work"), (21, "sleep")]),
            weekday_overrides: overrides,
        }
    }

    #[test]
    fn resolves_blocks_and_transitions() {
        let cal = Calendar::new(&CalendarConfig::default()).unwrap();
        let def = routine();
        // Wednesday 1 March 1340.
        let at = |h, m| cal.compose(1340, 2, 0, TimeOfDay::hm(h, m)).unwrap();
        let seg = resolve_routine(&def, &cal, at(6, 0)).unwrap();
        assert_eq!(seg.activity.as_str(), "work");
        assert_eq!(seg.next_transition, at(12, 0));

        let seg = resolve_routine(&def, &cal, at(12, 0)).unwrap();
        assert_eq!(seg.activity.as_str(), "dinner");

        // Before the first block: still asleep from the previous evening.
        let seg = resolve_routine(&def, &cal, at(3, 0)).unwrap();
        assert_eq!(seg.activity.as_str(), "sleep");
        assert_eq!(seg.next_transition, at(5, 0));

        // After the last block, the next transition is tomorrow's first block.
        let seg = resolve_routine(&def, &cal, at(23, 0)).unwrap();
        assert_eq!(seg.activity.as_str(), "sleep");
        assert_eq!(
            seg.next_transition,
            cal.compose(1340, 2, 1, TimeOfDay::hm(5, 0)).unwrap()
        );
    }

    #[test]
    fn weekday_overrides_apply() {
        let cal = Calendar::new(&CalendarConfig::default()).unwrap();
        let def = routine();
        // Saturday 4 March → Sunday 5 March 1340.
        let saturday_night = cal.compose(1340, 2, 3, TimeOfDay::hm(23, 0)).unwrap();
        let seg = resolve_routine(&def, &cal, saturday_night).unwrap();
        assert_eq!(
            seg.next_transition,
            cal.compose(1340, 2, 4, TimeOfDay::hm(7, 0)).unwrap()
        );
        let sunday = cal.compose(1340, 2, 4, TimeOfDay::hm(8, 0)).unwrap();
        assert_eq!(
            resolve_routine(&def, &cal, sunday)
                .unwrap()
                .activity
                .as_str(),
            "mass"
        );
        // Monday before dawn: Sunday's last block (22:00 sleep) carries over.
        let monday = cal.compose(1340, 2, 5, TimeOfDay::hm(1, 0)).unwrap();
        assert_eq!(
            resolve_routine(&def, &cal, monday)
                .unwrap()
                .activity
                .as_str(),
            "sleep"
        );
    }
}
