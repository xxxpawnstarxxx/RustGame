//! Strongly typed identifiers.
//!
//! Identifiers are plain values that survive serialisation: no identifier is
//! ever derived from a memory address. Entities use a generational index so a
//! stale [`EntityId`] held by some system can never alias a newer entity that
//! reused the same slot.

use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt;

/// Generational identifier of a simulated entity.
///
/// `index` addresses a slot in the [`crate::entities::EntityManager`];
/// `generation` is incremented every time that slot is freed, so ids of
/// destroyed entities are detectably stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EntityId {
    index: u32,
    generation: u32,
}

impl EntityId {
    /// Builds an id from raw parts. Normally only the entity manager does this.
    pub const fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    /// Slot index inside the entity manager.
    pub const fn index(self) -> u32 {
        self.index
    }

    /// Generation of the slot when this id was issued.
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}v{}", self.index, self.generation)
    }
}

macro_rules! sequential_id {
    ($(#[$meta:meta])* $name:ident, $prefix:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            /// Wraps a raw sequence number.
            pub const fn from_raw(raw: u64) -> Self {
                Self(raw)
            }

            /// The raw sequence number.
            pub const fn raw(self) -> u64 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($prefix, "{}"), self.0)
            }
        }
    };
}

sequential_id!(
    /// Identifier of an event, allocated in emission order. Event ids define
    /// the deterministic processing order of the event queue.
    EventId,
    "ev"
);
sequential_id!(
    /// Identifier of a scheduled timer (delayed or recurring action).
    TimerId,
    "timer"
);
sequential_id!(
    /// Identifier of a unit of work performed by a worker.
    JobId,
    "job"
);

/// Monotonic, persistent generator for sequential ids.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdSequence {
    next: u64,
}

impl IdSequence {
    /// A sequence starting at zero.
    pub const fn new() -> Self {
        Self { next: 0 }
    }

    /// Returns the next raw id and advances the sequence.
    pub fn allocate(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        id
    }

    /// The raw id that the next call to [`IdSequence::allocate`] will return.
    pub const fn peek(&self) -> u64 {
        self.next
    }
}

/// Stable name of a simulation system, used for scheduling state, event
/// provenance and debugging. Serialised as a plain string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SystemId(Cow<'static, str>);

impl SystemId {
    /// Creates a system id from a static name.
    pub const fn new(name: &'static str) -> Self {
        Self(Cow::Borrowed(name))
    }

    /// The system name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SystemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_sequence_is_monotonic() {
        let mut seq = IdSequence::new();
        assert_eq!(seq.allocate(), 0);
        assert_eq!(seq.allocate(), 1);
        assert_eq!(seq.peek(), 2);
    }

    #[test]
    fn ids_serialise_transparently() {
        assert_eq!(serde_json::to_string(&EventId::from_raw(7)).unwrap(), "7");
        assert_eq!(
            serde_json::to_string(&SystemId::new("routines")).unwrap(),
            "\"routines\""
        );
        let back: SystemId = serde_json::from_str("\"routines\"").unwrap();
        assert_eq!(back, SystemId::new("routines"));
    }

    #[test]
    fn entity_ids_compare_by_index_then_generation() {
        assert!(EntityId::new(1, 5) < EntityId::new(2, 0));
        assert!(EntityId::new(1, 0) < EntityId::new(1, 1));
        assert_eq!(EntityId::new(3, 2).to_string(), "#3v2");
    }
}
