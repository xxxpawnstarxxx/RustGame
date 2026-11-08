//! Content definition types, grouped by domain.
//!
//! Definitions are immutable templates. Runtime *instances* (a particular
//! axe, a particular villager) are entities whose components refer back to
//! these definitions by id; see `simulation::instances`.

mod body;
mod daily;
mod items;
mod people;
mod society;
mod world;

pub use body::*;
pub use daily::*;
pub use items::*;
pub use people::*;
pub use society::*;
pub use world::*;

use crate::core::time::Calendar;

/// Appends an error unless `value` is finite and strictly positive.
pub(crate) fn require_positive(errors: &mut Vec<String>, field: &str, value: f32) {
    if !(value.is_finite() && value > 0.0) {
        errors.push(format!("{field} must be a positive number (got {value})"));
    }
}

/// Appends an error unless `value` is finite and not negative.
pub(crate) fn require_non_negative(errors: &mut Vec<String>, field: &str, value: f32) {
    if !(value.is_finite() && value >= 0.0) {
        errors.push(format!("{field} must not be negative (got {value})"));
    }
}

/// Appends an error unless `value` lies in `[0, 1]`.
pub(crate) fn require_unit(errors: &mut Vec<String>, field: &str, value: f32) {
    if !(value.is_finite() && (0.0..=1.0).contains(&value)) {
        errors.push(format!("{field} must be between 0 and 1 (got {value})"));
    }
}

/// Appends an error for each 1-based month number outside the calendar.
pub(crate) fn require_months(
    errors: &mut Vec<String>,
    field: &str,
    months: &[u32],
    calendar: &Calendar,
) {
    for month in months {
        if *month == 0 || *month > calendar.month_count() {
            errors.push(format!(
                "{field} contains month {month}, but the calendar has {} months",
                calendar.month_count()
            ));
        }
    }
}

/// Appends an error if `[low, high]` is not an ordered range.
pub(crate) fn require_range(errors: &mut Vec<String>, field: &str, range: [f32; 2]) {
    if !(range[0].is_finite() && range[1].is_finite() && range[0] <= range[1]) {
        errors.push(format!("{field} must be an ordered [low, high] range"));
    }
}

pub(crate) fn default_one() -> u32 {
    1
}

pub(crate) fn default_true() -> bool {
    true
}
