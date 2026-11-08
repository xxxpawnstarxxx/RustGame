//! Day/night model.
//!
//! Daylight length is configured per month (hours of daylight at mid-month)
//! and interpolated linearly between month midpoints, so day length changes
//! smoothly through the year. Solar noon is at the middle of the day. Dawn
//! and dusk twilight ramp the light level smoothly between the configured
//! night level and full daylight.

use crate::core::time::{Calendar, GameInstant};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Coarse part of the day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DayPhase {
    Night,
    Dawn,
    Day,
    Dusk,
}

impl fmt::Display for DayPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// Daylight at one instant.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DaylightState {
    pub phase: DayPhase,
    /// 0 = darkness, 1 = full daylight.
    pub light_level: f32,
    /// Hours between sunrise and sunset today.
    pub daylight_hours: f32,
    /// Seconds after midnight.
    pub sunrise_seconds: u64,
    /// Seconds after midnight.
    pub sunset_seconds: u64,
}

/// Interpolated daylight hours for a 0-based day of the year.
pub fn daylight_hours(calendar: &Calendar, day_of_year: u32) -> f32 {
    let months = &calendar.config().months;
    let year = calendar.days_per_year() as f64;
    let midpoint =
        |i: usize| calendar.month_start_day(i as u32) as f64 + f64::from(months[i].days) / 2.0;
    let d = f64::from(day_of_year) + 0.5;
    let last = months.len() - 1;
    let next_index = (0..months.len()).find(|&i| midpoint(i) > d);
    let ((prev_mid, prev_val), (next_mid, next_val)) = match next_index {
        Some(0) => (
            (midpoint(last) - year, months[last].daylight_hours),
            (midpoint(0), months[0].daylight_hours),
        ),
        Some(i) => (
            (midpoint(i - 1), months[i - 1].daylight_hours),
            (midpoint(i), months[i].daylight_hours),
        ),
        None => (
            (midpoint(last), months[last].daylight_hours),
            (midpoint(0) + year, months[0].daylight_hours),
        ),
    };
    let span = next_mid - prev_mid;
    let t = if span > 0.0 {
        (d - prev_mid) / span
    } else {
        0.0
    };
    (f64::from(prev_val) + (f64::from(next_val) - f64::from(prev_val)) * t) as f32
}

fn smoothstep(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Evaluates daylight at an instant.
pub fn evaluate(calendar: &Calendar, t: GameInstant) -> DaylightState {
    let config = calendar.config();
    let date = calendar.decompose(t);
    let hours = daylight_hours(calendar, date.day_of_year);
    let day_len = calendar.seconds_per_day() as f64;
    let noon = day_len / 2.0;
    let half = (f64::from(hours) * calendar.seconds_per_hour() as f64 / 2.0).clamp(0.0, noon);
    let sunrise = noon - half;
    let sunset = noon + half;
    let twilight = f64::from(config.twilight_minutes) * calendar.seconds_per_minute() as f64;
    let night = f64::from(config.night_light_level);
    let s = calendar.seconds_into_day(t) as f64;
    let ramp = |f: f64| night + (1.0 - night) * smoothstep(f);

    let (phase, light) = if s >= sunrise && s < sunset {
        (DayPhase::Day, 1.0)
    } else if twilight > 0.0 && s >= sunrise - twilight && s < sunrise {
        (DayPhase::Dawn, ramp((s - (sunrise - twilight)) / twilight))
    } else if twilight > 0.0 && s >= sunset && s < sunset + twilight {
        (DayPhase::Dusk, ramp(1.0 - (s - sunset) / twilight))
    } else {
        (DayPhase::Night, night)
    };

    DaylightState {
        phase,
        light_level: light as f32,
        daylight_hours: hours,
        sunrise_seconds: sunrise.round() as u64,
        sunset_seconds: sunset.round() as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::CalendarConfig;
    use crate::core::time::TimeOfDay;

    fn calendar() -> Calendar {
        Calendar::new(&CalendarConfig::default()).unwrap()
    }

    #[test]
    fn summer_days_are_longer_than_winter_days() {
        let cal = calendar();
        let june = cal.compose(1340, 5, 15, TimeOfDay::hm(12, 0)).unwrap();
        let december = cal.compose(1340, 11, 15, TimeOfDay::hm(12, 0)).unwrap();
        let summer = evaluate(&cal, june);
        let winter = evaluate(&cal, december);
        assert!(summer.daylight_hours > 16.0);
        assert!(winter.daylight_hours < 8.0);
        assert!(summer.sunrise_seconds < winter.sunrise_seconds);
    }

    #[test]
    fn interpolation_is_continuous_across_year_end() {
        let cal = calendar();
        let last = daylight_hours(&cal, cal.days_per_year() as u32 - 1);
        let first = daylight_hours(&cal, 0);
        assert!((last - first).abs() < 0.1);
    }

    #[test]
    fn phases_follow_the_sun() {
        let cal = calendar();
        let at = |h, m| evaluate(&cal, cal.compose(1340, 2, 10, TimeOfDay::hm(h, m)).unwrap());
        let midnight = at(0, 0);
        assert_eq!(midnight.phase, DayPhase::Night);
        assert!((midnight.light_level - cal.config().night_light_level).abs() < 1e-6);
        assert_eq!(at(12, 0).phase, DayPhase::Day);
        assert_eq!(at(12, 0).light_level, 1.0);

        let noon = at(12, 0);
        let sunrise = noon.sunrise_seconds;
        let dawn_time = GameInstant::from_seconds(
            cal.start_of_day(cal.compose(1340, 2, 10, TimeOfDay::MIDNIGHT).unwrap())
                .seconds()
                + sunrise
                - 600,
        );
        let dawn = evaluate(&cal, dawn_time);
        assert_eq!(dawn.phase, DayPhase::Dawn);
        assert!(dawn.light_level > midnight.light_level && dawn.light_level < 1.0);
    }
}
