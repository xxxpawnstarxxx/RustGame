use crate::core::time::{Calendar, GameDuration, GameInstant};
use crate::simulation::events::SimEvent;
use serde::{Deserialize, Serialize};

/// The fixed-step simulation clock.
///
/// Each tick advances game time by exactly `seconds_per_tick`; real time only
/// determines *how many* ticks run per frame, never how long a tick is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimClock {
    tick: u64,
    now: GameInstant,
    seconds_per_tick: u32,
}

impl SimClock {
    pub fn new(start: GameInstant, seconds_per_tick: u32) -> Self {
        Self {
            tick: 0,
            now: start,
            seconds_per_tick: seconds_per_tick.max(1),
        }
    }

    /// Number of ticks executed since the world began.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// Current game time.
    pub fn now(&self) -> GameInstant {
        self.now
    }

    /// Game time per tick.
    pub fn tick_duration(&self) -> GameDuration {
        GameDuration::from_seconds(u64::from(self.seconds_per_tick))
    }

    /// Ticks needed to cover `duration` (rounded up, minimum one).
    pub fn ticks_for(&self, duration: GameDuration) -> u64 {
        duration
            .seconds()
            .div_ceil(u64::from(self.seconds_per_tick))
            .max(1)
    }

    /// Advances one tick and returns the previous time.
    pub fn advance(&mut self) -> GameInstant {
        let previous = self.now;
        self.tick += 1;
        self.now += self.tick_duration();
        previous
    }
}

/// Calendar boundary events crossed when time moves from `previous` to
/// `current`, coarsest first (year, month, week, day, hour).
///
/// Configuration guarantees a tick never exceeds one hour, so at most one
/// boundary of each kind can be crossed per tick.
pub fn calendar_boundary_events(
    calendar: &Calendar,
    previous: GameInstant,
    current: GameInstant,
) -> Vec<SimEvent> {
    let mut events = Vec::new();
    if current <= previous {
        return events;
    }
    let before = calendar.decompose(previous);
    let after = calendar.decompose(current);
    if calendar.days_since_epoch(previous) != calendar.days_since_epoch(current) {
        if before.year != after.year {
            events.push(SimEvent::YearStarted { year: after.year });
        }
        if before.year != after.year || before.month != after.month {
            events.push(SimEvent::MonthStarted {
                year: after.year,
                month: after.month,
            });
        }
        if after.weekday == 0 {
            events.push(SimEvent::WeekStarted { year: after.year });
        }
        events.push(SimEvent::DayStarted {
            year: after.year,
            month: after.month,
            day: after.day,
            weekday: after.weekday,
        });
    }
    let hour_len = calendar.seconds_per_hour();
    if previous.seconds() / hour_len != current.seconds() / hour_len {
        events.push(SimEvent::HourStarted { hour: after.hour });
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::CalendarConfig;
    use crate::core::time::TimeOfDay;
    use crate::simulation::events::EventKind;

    #[test]
    fn advancing_moves_time_by_tick_length() {
        let mut clock = SimClock::new(GameInstant::from_seconds(100), 5);
        assert_eq!(clock.advance(), GameInstant::from_seconds(100));
        assert_eq!(clock.now(), GameInstant::from_seconds(105));
        assert_eq!(clock.tick(), 1);
        assert_eq!(clock.ticks_for(GameDuration::from_seconds(11)), 3);
        assert_eq!(clock.ticks_for(GameDuration::ZERO), 1);
    }

    #[test]
    fn detects_new_year_boundaries() {
        let cal = Calendar::new(&CalendarConfig::default()).unwrap();
        let before = cal
            .compose(1340, 11, 30, TimeOfDay::new(23, 59, 59))
            .unwrap();
        let after = before + GameDuration::from_seconds(1);
        let kinds: Vec<_> = calendar_boundary_events(&cal, before, after)
            .iter()
            .map(SimEvent::kind)
            .collect();
        assert!(kinds.starts_with(&[EventKind::YearStarted, EventKind::MonthStarted]));
        assert!(kinds.contains(&EventKind::DayStarted));
        assert_eq!(kinds.last(), Some(&EventKind::HourStarted));
    }

    #[test]
    fn no_events_within_an_hour() {
        let cal = Calendar::new(&CalendarConfig::default()).unwrap();
        let t = cal.compose(1340, 2, 0, TimeOfDay::hm(10, 15)).unwrap();
        assert!(calendar_boundary_events(&cal, t, t + GameDuration::from_seconds(60)).is_empty());
    }
}
