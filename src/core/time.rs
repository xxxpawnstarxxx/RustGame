//! Calendar arithmetic and time primitives.
//!
//! * [`GameInstant`] — an absolute point in game time: whole seconds since the
//!   calendar epoch (year 0, first day of the first month, midnight).
//! * [`GameDuration`] — a span of game seconds.
//! * [`TimeOfDay`] — an hour/minute/second within a day, written `"HH:MM"` or
//!   `"HH:MM:SS"` in data files.
//! * [`Calendar`] — converts between instants and calendar dates using the
//!   configurable units, months and weekdays of [`CalendarConfig`].
//!
//! Internally months, days of month and weekdays are **0-based**. Only the
//! human-facing configuration (`calendar.start`) and formatted output are
//! 1-based. Years have a fixed length (the sum of the month lengths); leap
//! years are not modelled.

use crate::core::config::{CalendarConfig, Season};
use crate::core::error::{SimError, SimResult};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::ops::{Add, AddAssign, Mul};

/// Absolute game time in seconds since the calendar epoch.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct GameInstant(u64);

impl GameInstant {
    /// The calendar epoch.
    pub const EPOCH: Self = Self(0);

    pub const fn from_seconds(seconds: u64) -> Self {
        Self(seconds)
    }

    pub const fn seconds(self) -> u64 {
        self.0
    }

    /// Time elapsed since `earlier` (zero if `earlier` is later).
    pub const fn duration_since(self, earlier: Self) -> GameDuration {
        GameDuration(self.0.saturating_sub(earlier.0))
    }
}

impl Add<GameDuration> for GameInstant {
    type Output = Self;
    fn add(self, rhs: GameDuration) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }
}

impl AddAssign<GameDuration> for GameInstant {
    fn add_assign(&mut self, rhs: GameDuration) {
        *self = *self + rhs;
    }
}

/// A span of game time in seconds.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct GameDuration(u64);

impl GameDuration {
    pub const ZERO: Self = Self(0);

    pub const fn from_seconds(seconds: u64) -> Self {
        Self(seconds)
    }

    pub const fn seconds(self) -> u64 {
        self.0
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }
}

impl Add for GameDuration {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }
}

impl Mul<u64> for GameDuration {
    type Output = Self;
    fn mul(self, rhs: u64) -> Self {
        Self(self.0.saturating_mul(rhs))
    }
}

/// Calendar unit for expressing durations. Months are deliberately absent:
/// their length varies, so month-based timing uses calendar recurrences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum TimeUnit {
    Second,
    Minute,
    Hour,
    Day,
    Week,
    Year,
}

/// A time within a day. Validity depends on the calendar's units and is
/// checked by [`Calendar::time_of_day_seconds`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct TimeOfDay {
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl TimeOfDay {
    pub const MIDNIGHT: Self = Self::new(0, 0, 0);

    pub const fn new(hour: u32, minute: u32, second: u32) -> Self {
        Self {
            hour,
            minute,
            second,
        }
    }

    /// Hour and minute, zero seconds.
    pub const fn hm(hour: u32, minute: u32) -> Self {
        Self::new(hour, minute, 0)
    }

    /// Parses `"HH:MM"` or `"HH:MM:SS"`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let parts: Vec<&str> = text.trim().split(':').collect();
        if !(2..=3).contains(&parts.len()) {
            return Err(format!("expected HH:MM or HH:MM:SS, got '{text}'"));
        }
        let number = |part: &str| {
            part.parse::<u32>()
                .map_err(|_| format!("invalid number '{part}' in time '{text}'"))
        };
        Ok(Self {
            hour: number(parts[0])?,
            minute: number(parts[1])?,
            second: if parts.len() == 3 {
                number(parts[2])?
            } else {
                0
            },
        })
    }
}

impl fmt::Display for TimeOfDay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.second == 0 {
            write!(f, "{:02}:{:02}", self.hour, self.minute)
        } else {
            write!(f, "{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
        }
    }
}

impl Serialize for TimeOfDay {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for TimeOfDay {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// A fully decomposed calendar date. Month, day and weekday are 0-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalendarDate {
    pub year: u32,
    pub month: u32,
    pub day: u32,
    pub day_of_year: u32,
    pub weekday: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl CalendarDate {
    pub const fn time_of_day(&self) -> TimeOfDay {
        TimeOfDay::new(self.hour, self.minute, self.second)
    }
}

/// Calendar built from a validated [`CalendarConfig`].
#[derive(Debug, Clone)]
pub struct Calendar {
    config: CalendarConfig,
    seconds_per_minute: u64,
    seconds_per_hour: u64,
    seconds_per_day: u64,
    days_per_year: u64,
    month_start_day: Vec<u64>,
    weekday_offset: u64,
}

impl Calendar {
    /// Validates the configuration and precomputes month offsets and the
    /// weekday anchor.
    pub fn new(config: &CalendarConfig) -> SimResult<Self> {
        config.validate()?;
        let mut month_start_day = Vec::with_capacity(config.months.len());
        let mut days = 0u64;
        for month in &config.months {
            month_start_day.push(days);
            days += u64::from(month.days);
        }
        let mut calendar = Self {
            config: config.clone(),
            seconds_per_minute: u64::from(config.seconds_per_minute),
            seconds_per_hour: config.seconds_per_hour(),
            seconds_per_day: config.seconds_per_day(),
            days_per_year: days,
            month_start_day,
            weekday_offset: 0,
        };
        let start_weekday = calendar
            .weekday_index(&config.start.weekday)
            .ok_or_else(|| SimError::config("calendar.start.weekday is not a weekday"))?;
        let start_day = calendar.days_since_epoch(calendar.start_instant()?);
        let week = calendar.days_per_week();
        calendar.weekday_offset = (u64::from(start_weekday) + week - start_day % week) % week;
        Ok(calendar)
    }

    pub fn config(&self) -> &CalendarConfig {
        &self.config
    }

    pub fn seconds_per_minute(&self) -> u64 {
        self.seconds_per_minute
    }

    pub fn seconds_per_hour(&self) -> u64 {
        self.seconds_per_hour
    }

    pub fn seconds_per_day(&self) -> u64 {
        self.seconds_per_day
    }

    pub fn hours_per_day(&self) -> u32 {
        self.config.hours_per_day
    }

    pub fn days_per_year(&self) -> u64 {
        self.days_per_year
    }

    pub fn days_per_week(&self) -> u64 {
        self.config.weekdays.len() as u64
    }

    pub fn month_count(&self) -> u32 {
        self.config.months.len() as u32
    }

    /// Length of a 0-based month in days (0 for an invalid month).
    pub fn month_length(&self, month: u32) -> u32 {
        self.config.months.get(month as usize).map_or(0, |m| m.days)
    }

    /// Longest month length; bounds valid day-of-month values.
    pub fn longest_month(&self) -> u32 {
        self.config.months.iter().map(|m| m.days).max().unwrap_or(0)
    }

    pub fn month_name(&self, month: u32) -> &str {
        self.config
            .months
            .get(month as usize)
            .map_or("?", |m| m.name.as_str())
    }

    pub fn season_of_month(&self, month: u32) -> Season {
        self.config
            .months
            .get(month as usize)
            .map_or(Season::Winter, |m| m.season)
    }

    /// First day of a 0-based month, counted from the start of the year.
    pub fn month_start_day(&self, month: u32) -> u64 {
        self.month_start_day
            .get(month as usize)
            .copied()
            .unwrap_or(0)
    }

    pub fn weekday_name(&self, weekday: u32) -> &str {
        self.config
            .weekdays
            .get(weekday as usize)
            .map_or("?", String::as_str)
    }

    pub fn weekday_index(&self, name: &str) -> Option<u32> {
        self.config
            .weekdays
            .iter()
            .position(|w| w == name)
            .map(|i| i as u32)
    }

    /// Converts an amount of calendar units into a duration.
    pub fn duration(&self, amount: u64, unit: TimeUnit) -> GameDuration {
        let unit_seconds = match unit {
            TimeUnit::Second => 1,
            TimeUnit::Minute => self.seconds_per_minute,
            TimeUnit::Hour => self.seconds_per_hour,
            TimeUnit::Day => self.seconds_per_day,
            TimeUnit::Week => self.seconds_per_day * self.days_per_week(),
            TimeUnit::Year => self.seconds_per_day * self.days_per_year,
        };
        GameDuration::from_seconds(amount.saturating_mul(unit_seconds))
    }

    /// Seconds after midnight for a time of day, validating it against the
    /// calendar's units.
    pub fn time_of_day_seconds(&self, time: TimeOfDay) -> SimResult<u64> {
        if time.hour >= self.config.hours_per_day
            || time.minute >= self.config.minutes_per_hour
            || time.second >= self.config.seconds_per_minute
        {
            return Err(SimError::InvalidTime(format!(
                "{time} is not a valid time of day for this calendar"
            )));
        }
        Ok(u64::from(time.hour) * self.seconds_per_hour
            + u64::from(time.minute) * self.seconds_per_minute
            + u64::from(time.second))
    }

    /// Whole days elapsed since the epoch.
    pub fn days_since_epoch(&self, t: GameInstant) -> u64 {
        t.seconds() / self.seconds_per_day
    }

    /// Seconds elapsed since the most recent midnight.
    pub fn seconds_into_day(&self, t: GameInstant) -> u64 {
        t.seconds() % self.seconds_per_day
    }

    /// Midnight at the start of the day containing `t`.
    pub fn start_of_day(&self, t: GameInstant) -> GameInstant {
        GameInstant::from_seconds(self.days_since_epoch(t) * self.seconds_per_day)
    }

    /// Midnight at the start of an absolute day number.
    pub fn instant_of_day(&self, day: u64) -> GameInstant {
        GameInstant::from_seconds(day.saturating_mul(self.seconds_per_day))
    }

    /// 0-based weekday of an absolute day number.
    pub fn weekday_of_day(&self, day: u64) -> u32 {
        ((day + self.weekday_offset) % self.days_per_week()) as u32
    }

    /// Splits an instant into calendar fields.
    pub fn decompose(&self, t: GameInstant) -> CalendarDate {
        let days = self.days_since_epoch(t);
        let year = days / self.days_per_year;
        let day_of_year = days % self.days_per_year;
        let month = self
            .month_start_day
            .partition_point(|&start| start <= day_of_year)
            .saturating_sub(1);
        let day = day_of_year - self.month_start_day[month];
        let seconds = self.seconds_into_day(t);
        CalendarDate {
            year: year as u32,
            month: month as u32,
            day: day as u32,
            day_of_year: day_of_year as u32,
            weekday: self.weekday_of_day(days),
            hour: (seconds / self.seconds_per_hour) as u32,
            minute: (seconds % self.seconds_per_hour / self.seconds_per_minute) as u32,
            second: (seconds % self.seconds_per_minute) as u32,
        }
    }

    /// Builds an instant from a year, 0-based month, 0-based day and time.
    pub fn compose(
        &self,
        year: u32,
        month: u32,
        day: u32,
        time: TimeOfDay,
    ) -> SimResult<GameInstant> {
        if month >= self.month_count() {
            return Err(SimError::InvalidTime(format!(
                "month index {month} is out of range"
            )));
        }
        if day >= self.month_length(month) {
            return Err(SimError::InvalidTime(format!(
                "day index {day} is out of range for {}",
                self.month_name(month)
            )));
        }
        let seconds_of_day = self.time_of_day_seconds(time)?;
        let days = u64::from(year) * self.days_per_year
            + self.month_start_day[month as usize]
            + u64::from(day);
        Ok(GameInstant::from_seconds(
            days * self.seconds_per_day + seconds_of_day,
        ))
    }

    /// The configured start instant of a new world.
    pub fn start_instant(&self) -> SimResult<GameInstant> {
        let start = &self.config.start;
        self.compose(
            start.year,
            start.month.saturating_sub(1),
            start.day.saturating_sub(1),
            TimeOfDay::hm(start.hour, start.minute),
        )
    }

    /// Season at an instant.
    pub fn season_at(&self, t: GameInstant) -> Season {
        self.season_of_month(self.decompose(t).month)
    }

    /// e.g. `"Wednesday 1 March 1340"`.
    pub fn format_date(&self, t: GameInstant) -> String {
        let d = self.decompose(t);
        format!(
            "{} {} {} {}",
            self.weekday_name(d.weekday),
            d.day + 1,
            self.month_name(d.month),
            d.year
        )
    }

    /// e.g. `"06:00"`.
    pub fn format_time(&self, t: GameInstant) -> String {
        let d = self.decompose(t);
        format!("{:02}:{:02}", d.hour, d.minute)
    }

    /// e.g. `"Wednesday 1 March 1340, 06:00"`.
    pub fn format(&self, t: GameInstant) -> String {
        format!("{}, {}", self.format_date(t), self.format_time(t))
    }

    /// Human-readable duration, e.g. `"2d 3h 15m"`.
    pub fn format_duration(&self, d: GameDuration) -> String {
        let s = d.seconds();
        let days = s / self.seconds_per_day;
        let hours = s % self.seconds_per_day / self.seconds_per_hour;
        let minutes = s % self.seconds_per_hour / self.seconds_per_minute;
        let seconds = s % self.seconds_per_minute;
        match (days, hours, minutes) {
            (0, 0, 0) => format!("{seconds}s"),
            (0, 0, _) => format!("{minutes}m {seconds}s"),
            (0, _, _) => format!("{hours}h {minutes}m"),
            _ => format!("{days}d {hours}h {minutes}m"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn calendar() -> Calendar {
        Calendar::new(&CalendarConfig::default()).unwrap()
    }

    #[test]
    fn start_instant_matches_configuration() {
        let cal = calendar();
        let date = cal.decompose(cal.start_instant().unwrap());
        assert_eq!(date.year, 1340);
        assert_eq!(cal.month_name(date.month), "March");
        assert_eq!(date.day, 0);
        assert_eq!(date.hour, 6);
        assert_eq!(cal.weekday_name(date.weekday), "Wednesday");
        assert_eq!(
            cal.format(cal.start_instant().unwrap()),
            "Wednesday 1 March 1340, 06:00"
        );
    }

    #[test]
    fn weekdays_advance_daily_and_wrap() {
        let cal = calendar();
        let start = cal.start_instant().unwrap();
        let next = cal.decompose(start + cal.duration(1, TimeUnit::Day));
        assert_eq!(cal.weekday_name(next.weekday), "Thursday");
        let week = cal.decompose(start + cal.duration(1, TimeUnit::Week));
        assert_eq!(cal.weekday_name(week.weekday), "Wednesday");
    }

    #[test]
    fn year_rolls_over_after_december() {
        let cal = calendar();
        let eve = cal
            .compose(1340, 11, 30, TimeOfDay::new(23, 59, 59))
            .unwrap();
        let next = cal.decompose(eve + GameDuration::from_seconds(1));
        assert_eq!(
            (next.year, next.month, next.day, next.hour),
            (1341, 0, 0, 0)
        );
    }

    #[test]
    fn compose_rejects_invalid_fields() {
        let cal = calendar();
        assert!(cal.compose(1340, 12, 0, TimeOfDay::MIDNIGHT).is_err());
        assert!(cal.compose(1340, 1, 28, TimeOfDay::MIDNIGHT).is_err());
        assert!(cal.compose(1340, 0, 0, TimeOfDay::hm(24, 0)).is_err());
    }

    #[test]
    fn custom_units_are_respected() {
        let mut config = CalendarConfig {
            hours_per_day: 10,
            minutes_per_hour: 100,
            seconds_per_minute: 100,
            ..CalendarConfig::default()
        };
        config.start.hour = 3;
        for month in &mut config.months {
            month.daylight_hours = 5.0;
        }
        let cal = Calendar::new(&config).unwrap();
        assert_eq!(cal.seconds_per_day(), 100_000);
        let d = cal.decompose(cal.start_instant().unwrap() + GameDuration::from_seconds(10_050));
        assert_eq!((d.hour, d.minute, d.second), (4, 0, 50));
    }

    #[test]
    fn time_of_day_parses_and_serialises() {
        assert_eq!(TimeOfDay::parse("05:30").unwrap(), TimeOfDay::hm(5, 30));
        assert_eq!(
            TimeOfDay::parse("23:59:01").unwrap(),
            TimeOfDay::new(23, 59, 1)
        );
        assert!(TimeOfDay::parse("noon").is_err());
        assert!(TimeOfDay::parse("1:2:3:4").is_err());
        let json = serde_json::to_string(&TimeOfDay::hm(7, 5)).unwrap();
        assert_eq!(json, "\"07:05\"");
        let back: TimeOfDay = serde_json::from_str(&json).unwrap();
        assert_eq!(back, TimeOfDay::hm(7, 5));
    }

    proptest! {
        #[test]
        fn decompose_compose_round_trip(seconds in 0u64..(3000u64 * 365 * 86_400)) {
            let cal = calendar();
            let t = GameInstant::from_seconds(seconds);
            let d = cal.decompose(t);
            let back = cal.compose(d.year, d.month, d.day, d.time_of_day()).unwrap();
            prop_assert_eq!(back, t);
        }
    }
}
