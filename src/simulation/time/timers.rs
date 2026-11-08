//! Scheduled events, delayed actions and recurring events.
//!
//! A [`Timer`] fires its [`TimerAction`] (emit an event, or queue a command)
//! at `fire_at`, then — if its [`Recurrence`] yields a next occurrence and its
//! repeat limit allows — is rescheduled. Recurrences are computed from the
//! *scheduled* firing time, not the tick it actually fired on, so recurring
//! timers never drift and missed occurrences are caught up in order.

use crate::core::error::{SimError, SimResult};
use crate::core::ids::{IdSequence, TimerId};
use crate::core::time::{Calendar, GameDuration, GameInstant, TimeOfDay};
use crate::simulation::commands::Command;
use crate::simulation::events::SimEvent;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What a timer does when it fires.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TimerAction {
    /// Emits an event (source: the timer).
    Emit(SimEvent),
    /// Queues a command, applied in the same tick.
    Command(Command),
}

/// When a timer repeats. Months, days and weekdays are 0-based.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Recurrence {
    /// Fires once.
    Once,
    /// Fixed period after each scheduled firing.
    Every(GameDuration),
    /// Every day at a time of day.
    Daily { at: TimeOfDay },
    /// Every week on a weekday at a time of day.
    Weekly { weekday: u32, at: TimeOfDay },
    /// Every month on a day (clamped to the month's length).
    Monthly { day: u32, at: TimeOfDay },
    /// Every year on a month and day (day clamped to the month's length).
    Yearly { month: u32, day: u32, at: TimeOfDay },
}

impl Recurrence {
    /// Checks the recurrence is meaningful for this calendar.
    pub fn validate(&self, calendar: &Calendar) -> SimResult<()> {
        let invalid = |message: String| Err(SimError::InvalidTime(message));
        match *self {
            Self::Once => Ok(()),
            Self::Every(period) if period.is_zero() => {
                invalid("recurrence period must be positive".into())
            }
            Self::Every(_) => Ok(()),
            Self::Daily { at } => calendar.time_of_day_seconds(at).map(drop),
            Self::Weekly { weekday, at } => {
                if u64::from(weekday) >= calendar.days_per_week() {
                    return invalid(format!("weekday index {weekday} is out of range"));
                }
                calendar.time_of_day_seconds(at).map(drop)
            }
            Self::Monthly { day, at } => {
                if day >= calendar.longest_month() {
                    return invalid(format!("day index {day} exceeds every month"));
                }
                calendar.time_of_day_seconds(at).map(drop)
            }
            Self::Yearly { month, day, at } => {
                if month >= calendar.month_count() {
                    return invalid(format!("month index {month} is out of range"));
                }
                if day >= calendar.month_length(month) {
                    return invalid(format!("day index {day} is out of range for its month"));
                }
                calendar.time_of_day_seconds(at).map(drop)
            }
        }
    }

    /// The first occurrence strictly after `after`, or `None` for `Once`.
    pub fn next_after(
        &self,
        calendar: &Calendar,
        after: GameInstant,
    ) -> SimResult<Option<GameInstant>> {
        self.validate(calendar)?;
        let day_len = calendar.seconds_per_day();
        let next = match *self {
            Self::Once => return Ok(None),
            Self::Every(period) => after + period,
            Self::Daily { at } => {
                let candidate = calendar.start_of_day(after)
                    + GameDuration::from_seconds(calendar.time_of_day_seconds(at)?);
                if candidate > after {
                    candidate
                } else {
                    candidate + GameDuration::from_seconds(day_len)
                }
            }
            Self::Weekly { weekday, at } => {
                let week = calendar.days_per_week();
                let today = calendar.days_since_epoch(after);
                let delta =
                    (u64::from(weekday) + week - u64::from(calendar.weekday_of_day(today))) % week;
                let offset = GameDuration::from_seconds(calendar.time_of_day_seconds(at)?);
                let candidate = calendar.instant_of_day(today + delta) + offset;
                if candidate > after {
                    candidate
                } else {
                    calendar.instant_of_day(today + delta + week) + offset
                }
            }
            Self::Monthly { day, at } => {
                let date = calendar.decompose(after);
                let (mut year, mut month) = (date.year, date.month);
                loop {
                    let d = day.min(calendar.month_length(month) - 1);
                    let candidate = calendar.compose(year, month, d, at)?;
                    if candidate > after {
                        break candidate;
                    }
                    month += 1;
                    if month == calendar.month_count() {
                        month = 0;
                        year += 1;
                    }
                }
            }
            Self::Yearly { month, day, at } => {
                let year = calendar.decompose(after).year;
                let candidate = calendar.compose(year, month, day, at)?;
                if candidate > after {
                    candidate
                } else {
                    calendar.compose(year + 1, month, day, at)?
                }
            }
        };
        Ok(Some(next))
    }
}

/// A scheduled action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timer {
    pub id: TimerId,
    pub label: String,
    pub fire_at: GameInstant,
    pub recurrence: Recurrence,
    pub action: TimerAction,
    /// Remaining firings, or `None` for unlimited.
    pub remaining_firings: Option<u32>,
    pub times_fired: u64,
}

/// Pending timers ordered by `(fire_at, id)` — a total, deterministic order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "TimerQueueData", into = "TimerQueueData")]
pub struct TimerQueue {
    queue: BTreeMap<(GameInstant, TimerId), Timer>,
    index: BTreeMap<TimerId, GameInstant>,
    ids: IdSequence,
}

/// Serialised form: JSON maps need string keys, so timers are stored as a list.
#[derive(Serialize, Deserialize)]
struct TimerQueueData {
    ids: IdSequence,
    timers: Vec<Timer>,
}

impl From<TimerQueue> for TimerQueueData {
    fn from(queue: TimerQueue) -> Self {
        Self {
            ids: queue.ids,
            timers: queue.queue.into_values().collect(),
        }
    }
}

impl From<TimerQueueData> for TimerQueue {
    fn from(data: TimerQueueData) -> Self {
        let mut queue = Self {
            ids: data.ids,
            ..Self::default()
        };
        for timer in data.timers {
            queue.insert(timer);
        }
        queue
    }
}

impl TimerQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Schedules a new timer and returns its id.
    pub fn schedule(
        &mut self,
        label: impl Into<String>,
        fire_at: GameInstant,
        recurrence: Recurrence,
        action: TimerAction,
        remaining_firings: Option<u32>,
    ) -> TimerId {
        let id = TimerId::from_raw(self.ids.allocate());
        self.insert(Timer {
            id,
            label: label.into(),
            fire_at,
            recurrence,
            action,
            remaining_firings,
            times_fired: 0,
        });
        id
    }

    fn insert(&mut self, timer: Timer) {
        self.index.insert(timer.id, timer.fire_at);
        self.queue.insert((timer.fire_at, timer.id), timer);
    }

    /// Puts a fired recurring timer back (keeping its id).
    pub fn reschedule(&mut self, timer: Timer) {
        self.insert(timer);
    }

    /// Removes a pending timer.
    pub fn cancel(&mut self, id: TimerId) -> Option<Timer> {
        let fire_at = self.index.remove(&id)?;
        self.queue.remove(&(fire_at, id))
    }

    /// Removes and returns the earliest timer due at or before `now`.
    pub fn pop_due(&mut self, now: GameInstant) -> Option<Timer> {
        let (&key, _) = self.queue.first_key_value()?;
        if key.0 > now {
            return None;
        }
        self.index.remove(&key.1);
        self.queue.remove(&key)
    }

    /// When the next timer is due.
    pub fn next_due(&self) -> Option<GameInstant> {
        self.queue.first_key_value().map(|(key, _)| key.0)
    }

    pub fn get(&self, id: TimerId) -> Option<&Timer> {
        let fire_at = self.index.get(&id)?;
        self.queue.get(&(*fire_at, id))
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Pending timers in firing order.
    pub fn iter(&self) -> impl Iterator<Item = &Timer> {
        self.queue.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::CalendarConfig;
    use crate::core::time::TimeUnit;
    use proptest::prelude::*;

    fn calendar() -> Calendar {
        Calendar::new(&CalendarConfig::default()).unwrap()
    }

    fn emit() -> TimerAction {
        TimerAction::Emit(SimEvent::HourStarted { hour: 0 })
    }

    #[test]
    fn queue_orders_by_time_then_id() {
        let mut q = TimerQueue::new();
        let late = q.schedule(
            "late",
            GameInstant::from_seconds(20),
            Recurrence::Once,
            emit(),
            None,
        );
        let early_a = q.schedule(
            "a",
            GameInstant::from_seconds(10),
            Recurrence::Once,
            emit(),
            None,
        );
        let early_b = q.schedule(
            "b",
            GameInstant::from_seconds(10),
            Recurrence::Once,
            emit(),
            None,
        );
        assert_eq!(q.next_due(), Some(GameInstant::from_seconds(10)));
        assert!(q.pop_due(GameInstant::from_seconds(5)).is_none());
        let now = GameInstant::from_seconds(30);
        assert_eq!(q.pop_due(now).unwrap().id, early_a);
        assert_eq!(q.pop_due(now).unwrap().id, early_b);
        assert_eq!(q.pop_due(now).unwrap().id, late);
        assert!(q.is_empty());
    }

    #[test]
    fn cancel_and_round_trip() {
        let mut q = TimerQueue::new();
        let a = q.schedule(
            "a",
            GameInstant::from_seconds(1),
            Recurrence::Once,
            emit(),
            None,
        );
        let b = q.schedule(
            "b",
            GameInstant::from_seconds(2),
            Recurrence::Daily {
                at: TimeOfDay::hm(6, 0),
            },
            emit(),
            Some(3),
        );
        assert!(q.cancel(a).is_some());
        assert!(q.cancel(a).is_none());
        let json = serde_json::to_string(&q).unwrap();
        let mut back: TimerQueue = serde_json::from_str(&json).unwrap();
        assert_eq!(back, q);
        assert_eq!(back.get(b).unwrap().label, "b");
        let next = back.schedule("c", GameInstant::EPOCH, Recurrence::Once, emit(), None);
        assert_eq!(next, TimerId::from_raw(2));
    }

    #[test]
    fn calendar_recurrences() {
        let cal = calendar();
        // Wednesday 1 March 1340, 06:00.
        let start = cal.start_instant().unwrap();

        let daily = Recurrence::Daily {
            at: TimeOfDay::hm(5, 0),
        };
        let next = cal.decompose(daily.next_after(&cal, start).unwrap().unwrap());
        assert_eq!((next.day, next.hour), (1, 5));

        let same_day = Recurrence::Daily {
            at: TimeOfDay::hm(7, 0),
        };
        let next = cal.decompose(same_day.next_after(&cal, start).unwrap().unwrap());
        assert_eq!((next.day, next.hour), (0, 7));

        let sunday = cal.weekday_index("Sunday").unwrap();
        let weekly = Recurrence::Weekly {
            weekday: sunday,
            at: TimeOfDay::hm(9, 0),
        };
        let next = cal.decompose(weekly.next_after(&cal, start).unwrap().unwrap());
        assert_eq!((cal.weekday_name(next.weekday), next.day), ("Sunday", 4));

        let wednesday = cal.weekday_index("Wednesday").unwrap();
        let weekly = Recurrence::Weekly {
            weekday: wednesday,
            at: TimeOfDay::hm(6, 0),
        };
        let next = cal.decompose(weekly.next_after(&cal, start).unwrap().unwrap());
        assert_eq!(next.day, 7);

        // Day index 30 clamps to the last day of April (index 29).
        let monthly = Recurrence::Monthly {
            day: 30,
            at: TimeOfDay::MIDNIGHT,
        };
        let first = monthly.next_after(&cal, start).unwrap().unwrap();
        let d = cal.decompose(first);
        assert_eq!((cal.month_name(d.month), d.day), ("March", 30));
        let d = cal.decompose(monthly.next_after(&cal, first).unwrap().unwrap());
        assert_eq!((cal.month_name(d.month), d.day), ("April", 29));

        let lady_day = Recurrence::Yearly {
            month: 2,
            day: 24,
            at: TimeOfDay::hm(12, 0),
        };
        let d = cal.decompose(lady_day.next_after(&cal, start).unwrap().unwrap());
        assert_eq!((d.year, d.month, d.day), (1340, 2, 24));

        let new_year = Recurrence::Yearly {
            month: 0,
            day: 0,
            at: TimeOfDay::MIDNIGHT,
        };
        let d = cal.decompose(new_year.next_after(&cal, start).unwrap().unwrap());
        assert_eq!(d.year, 1341);

        assert_eq!(Recurrence::Once.next_after(&cal, start).unwrap(), None);
    }

    #[test]
    fn invalid_recurrences_are_rejected() {
        let cal = calendar();
        assert!(
            Recurrence::Every(GameDuration::ZERO)
                .validate(&cal)
                .is_err()
        );
        assert!(
            Recurrence::Weekly {
                weekday: 7,
                at: TimeOfDay::MIDNIGHT
            }
            .validate(&cal)
            .is_err()
        );
        assert!(
            Recurrence::Monthly {
                day: 31,
                at: TimeOfDay::MIDNIGHT
            }
            .validate(&cal)
            .is_err()
        );
        assert!(
            Recurrence::Yearly {
                month: 1,
                day: 28,
                at: TimeOfDay::MIDNIGHT
            }
            .validate(&cal)
            .is_err()
        );
        assert!(
            Recurrence::Daily {
                at: TimeOfDay::hm(25, 0)
            }
            .validate(&cal)
            .is_err()
        );
        assert!(
            Recurrence::Every(cal.duration(1, TimeUnit::Hour))
                .validate(&cal)
                .is_ok()
        );
    }

    proptest! {
        #[test]
        fn next_occurrence_is_strictly_later(
            seconds in 0u64..(2000u64 * 365 * 86_400),
            hour in 0u32..24,
            weekday in 0u32..7,
            day in 0u32..31,
            month in 0u32..12,
        ) {
            let cal = calendar();
            let after = GameInstant::from_seconds(seconds);
            let at = TimeOfDay::hm(hour, 0);
            let yearly_day = day.min(cal.month_length(month) - 1);
            for r in [
                Recurrence::Daily { at },
                Recurrence::Weekly { weekday, at },
                Recurrence::Monthly { day, at },
                Recurrence::Yearly { month, day: yearly_day, at },
            ] {
                let next = r.next_after(&cal, after).unwrap().unwrap();
                prop_assert!(next > after);
                prop_assert!(next.seconds() - after.seconds() <= 366 * 86_400);
            }
        }
    }
}
