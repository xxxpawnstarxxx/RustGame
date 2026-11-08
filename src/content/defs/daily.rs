//! Activities and daily routines.

use crate::content::ids::{ActivityId, RoutineId};
use crate::content::registry::{Definition, Reference, ValidationEnv};
use crate::core::time::{Calendar, TimeOfDay};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Broad category of a scheduled activity, used by AI and UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ActivityCategory {
    Rest,
    Work,
    Meal,
    Worship,
    Chores,
    Leisure,
    Trade,
}

/// Something a person can be scheduled to do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityDef {
    pub id: ActivityId,
    pub name: String,
    pub category: ActivityCategory,
    #[serde(default)]
    pub description: String,
}

impl Definition for ActivityDef {
    type Id = ActivityId;
    const CATEGORY: &'static str = "activities";
    fn id(&self) -> &ActivityId {
        &self.id
    }
}

/// A routine block: from `start` until the next block's start, do `activity`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutineBlockDef {
    pub start: TimeOfDay,
    pub activity: ActivityId,
}

/// A daily routine: ordered blocks covering the whole day, with optional
/// replacement lists for specific weekdays (e.g. Sunday rest).
///
/// The last block of a day continues past midnight until the first block of
/// the following day (which may come from a different weekday's list).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DailyRoutineDef {
    pub id: RoutineId,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub blocks: Vec<RoutineBlockDef>,
    /// Weekday name → blocks used instead of `blocks` on that weekday.
    #[serde(default)]
    pub weekday_overrides: BTreeMap<String, Vec<RoutineBlockDef>>,
}

impl DailyRoutineDef {
    /// Block list used on a 0-based weekday.
    pub fn blocks_for(&self, calendar: &Calendar, weekday: u32) -> &[RoutineBlockDef] {
        self.weekday_overrides
            .get(calendar.weekday_name(weekday))
            .map_or(self.blocks.as_slice(), Vec::as_slice)
    }

    fn all_blocks(&self) -> impl Iterator<Item = &RoutineBlockDef> {
        self.blocks
            .iter()
            .chain(self.weekday_overrides.values().flatten())
    }
}

impl Definition for DailyRoutineDef {
    type Id = RoutineId;
    const CATEGORY: &'static str = "routines";
    fn id(&self) -> &RoutineId {
        &self.id
    }

    fn references(&self) -> Vec<Reference> {
        self.all_blocks()
            .map(|b| Reference::new("blocks.activity", "activities", &b.activity))
            .collect()
    }

    fn validate(&self, env: &ValidationEnv<'_>) -> Vec<String> {
        let mut errors = Vec::new();
        validate_blocks(&self.blocks, env.calendar, "blocks", &mut errors);
        for (weekday, blocks) in &self.weekday_overrides {
            if env.calendar.weekday_index(weekday).is_none() {
                errors.push(format!(
                    "weekday override '{weekday}' is not a calendar weekday"
                ));
            }
            validate_blocks(
                blocks,
                env.calendar,
                &format!("weekday override '{weekday}'"),
                &mut errors,
            );
        }
        errors
    }
}

fn validate_blocks(
    blocks: &[RoutineBlockDef],
    calendar: &Calendar,
    label: &str,
    errors: &mut Vec<String>,
) {
    if blocks.is_empty() {
        errors.push(format!("{label}: a routine needs at least one block"));
        return;
    }
    let mut previous: Option<u64> = None;
    for block in blocks {
        match calendar.time_of_day_seconds(block.start) {
            Ok(seconds) => {
                if previous.is_some_and(|p| seconds <= p) {
                    errors.push(format!(
                        "{label}: block starting at {} is not after the previous block",
                        block.start
                    ));
                }
                previous = Some(seconds);
            }
            Err(err) => errors.push(format!("{label}: {err}")),
        }
    }
}
