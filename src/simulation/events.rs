//! Typed simulation events, the persistent event queue, the dispatching
//! [`EventBus`] and the debug [`EventLog`].
//!
//! Systems never call each other. They emit [`SimEvent`]s into the
//! [`EventQueue`]; the bus delivers each event, in id order, to every system
//! subscribed to its [`EventKind`], in system registration order. Events
//! emitted while handling are appended to the back of the queue and delivered
//! in the same dispatch pass, so cascades are fully deterministic.

use crate::content::{ActivityId, AnimalDiseaseId, AnimalId, BuildingId, InjuryId, ItemDefId};
use crate::core::config::Season;
use crate::core::ids::{EntityId, EventId, IdSequence, JobId, SystemId, TimerId};
use crate::core::time::{GameDuration, GameInstant};
use crate::entities::EntityKind;
use crate::simulation::context::{SimEnv, SimulationContext};
use crate::simulation::system::SystemRegistry;
use crate::simulation::time::DayPhase;
use crate::simulation::world_state::WorldState;
use crate::world::{DoorState, TilePos};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::fmt;

pub use crate::content::InjurySeverity;

/// Declares [`SimEvent`], its field-less discriminant [`EventKind`],
/// `EventKind::ALL` and `SimEvent::kind` from one list, so they cannot drift.
macro_rules! sim_events {
    ($( $(#[$meta:meta])* $name:ident { $( $field:ident : $ty:ty ),* $(,)? } ),* $(,)?) => {
        /// Every event that systems can exchange.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        pub enum SimEvent {
            $( $(#[$meta])* $name { $( $field: $ty ),* }, )*
        }

        /// Field-less discriminant of [`SimEvent`], used for subscriptions,
        /// filters and statistics.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        pub enum EventKind {
            $( $name, )*
        }

        impl EventKind {
            pub const ALL: &'static [EventKind] = &[ $( EventKind::$name, )* ];
        }

        impl SimEvent {
            pub fn kind(&self) -> EventKind {
                match self {
                    $( Self::$name { .. } => EventKind::$name, )*
                }
            }
        }
    };
}

sim_events! {
    // --- Entity lifecycle ---------------------------------------------------
    EntityCreated { entity: EntityId, kind: EntityKind },
    EntityDestroyed { entity: EntityId, kind: EntityKind },

    // --- Characters ---------------------------------------------------------
    CharacterInjured { character: EntityId, severity: InjurySeverity, injury: Option<InjuryId>, cause: Option<EntityId> },
    CharacterDied { character: EntityId, cause: Option<EntityId> },

    // --- Items --------------------------------------------------------------
    ItemCreated { item: EntityId, definition: ItemDefId },
    /// `from`/`to` are the holders (`None` = on the ground).
    ItemTransferred { item: EntityId, from: Option<EntityId>, to: Option<EntityId> },

    // --- Work ---------------------------------------------------------------
    JobStarted { job: JobId, worker: EntityId },
    JobCompleted { job: JobId, worker: EntityId },

    // --- Society ------------------------------------------------------------
    SettlementFounded { settlement: EntityId, founder: Option<EntityId> },
    FactionRelationChanged { faction: EntityId, other: EntityId, previous: i32, current: i32 },

    // --- Structures ---------------------------------------------------------
    StructureBuilt { structure: EntityId, building: BuildingId },
    StructureRemoved { structure: EntityId },
    DoorChanged { tile: TilePos, state: DoorState },

    // --- Calendar boundaries (emitted by the clock) ---------------------------
    HourStarted { hour: u32 },
    DayStarted { year: u32, month: u32, day: u32, weekday: u32 },
    WeekStarted { year: u32 },
    MonthStarted { year: u32, month: u32 },
    YearStarted { year: u32 },

    // --- Ambient time -------------------------------------------------------
    SeasonChanged { previous: Season, current: Season },
    DayPhaseChanged { previous: DayPhase, current: DayPhase },

    // --- Routines -----------------------------------------------------------
    ScheduledActivityChanged { entity: EntityId, previous: Option<ActivityId>, current: ActivityId },

    // --- Environment (region = weather cell index) ---------------------------
    PrecipitationStarted { region: u32, snow: bool },
    PrecipitationStopped { region: u32 },
    DroughtStarted { region: u32 },
    DroughtEnded { region: u32 },
    FloodStarted { region: u32 },
    FloodEnded { region: u32 },
    TreesFelled { tile: TilePos, canopy_removed: f32 },

    // --- Wildlife (region = wildlife region index) ---------------------------
    WildlifeOutbreakStarted { region: u32, animal: AnimalId, disease: AnimalDiseaseId },
    WildlifeOutbreakEnded { region: u32, animal: AnimalId },
    WildlifeHunted { region: u32, animal: AnimalId, count: u32 },

    // --- Movement -----------------------------------------------------------
    DestinationReached { entity: EntityId, tile: TilePos },
    PathFailed { entity: EntityId, goal: TilePos },
    CharacterStumbled { entity: EntityId, tile: TilePos },
    CharacterFell { entity: EntityId, tile: TilePos, height_m: f32 },
}

impl fmt::Display for EventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// Who emitted an event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventSource {
    /// Calendar boundary detection in the clock.
    Clock,
    /// A scheduled timer fired.
    Timer(TimerId),
    /// A command was applied.
    Command,
    /// A system's update or event handler.
    System(SystemId),
}

impl fmt::Display for EventSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Clock => f.write_str("clock"),
            Self::Timer(id) => write!(f, "{id}"),
            Self::Command => f.write_str("command"),
            Self::System(id) => write!(f, "{id}"),
        }
    }
}

/// An event plus its provenance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub id: EventId,
    pub tick: u64,
    pub time: GameInstant,
    pub source: EventSource,
    pub event: SimEvent,
}

/// Persistent FIFO of events awaiting dispatch. Lives in the world state so a
/// save taken with deferred events restores them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EventQueue {
    pending: VecDeque<EventEnvelope>,
    ids: IdSequence,
}

impl EventQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends an event, assigning the next sequential id.
    pub fn push(
        &mut self,
        tick: u64,
        time: GameInstant,
        source: EventSource,
        event: SimEvent,
    ) -> EventId {
        let id = EventId::from_raw(self.ids.allocate());
        self.pending.push_back(EventEnvelope {
            id,
            tick,
            time,
            source,
            event,
        });
        id
    }

    pub fn pop(&mut self) -> Option<EventEnvelope> {
        self.pending.pop_front()
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &EventEnvelope> {
        self.pending.iter()
    }

    /// Id the next pushed event will receive.
    pub fn next_id(&self) -> EventId {
        EventId::from_raw(self.ids.peek())
    }
}

/// A dispatched event with the systems that handled it.
#[derive(Debug, Clone, PartialEq)]
pub struct LoggedEvent {
    pub envelope: EventEnvelope,
    pub handled_by: Vec<SystemId>,
}

/// Bounded history of dispatched events plus counters, for debugging.
/// Runtime-only: not persisted and never read by simulation logic.
#[derive(Debug, Clone)]
pub struct EventLog {
    capacity: usize,
    entries: VecDeque<LoggedEvent>,
    counts: BTreeMap<EventKind, u64>,
    total: u64,
    deferral_ticks: u64,
}

impl EventLog {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: VecDeque::with_capacity(capacity.min(4096)),
            counts: BTreeMap::new(),
            total: 0,
            deferral_ticks: 0,
        }
    }

    pub fn record(&mut self, envelope: EventEnvelope, handled_by: Vec<SystemId>) {
        *self.counts.entry(envelope.event.kind()).or_default() += 1;
        self.total += 1;
        if self.capacity == 0 {
            return;
        }
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(LoggedEvent {
            envelope,
            handled_by,
        });
    }

    /// Notes a tick that ended with events deferred by the per-tick budget.
    pub fn note_deferral(&mut self) {
        self.deferral_ticks += 1;
    }

    /// Recorded events, newest first.
    pub fn recent(&self) -> impl Iterator<Item = &LoggedEvent> {
        self.entries.iter().rev()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Dispatch count per event kind since start (or last clear).
    pub fn counts(&self) -> &BTreeMap<EventKind, u64> {
        &self.counts
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn deferral_ticks(&self) -> u64 {
        self.deferral_ticks
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.counts.clear();
        self.total = 0;
        self.deferral_ticks = 0;
    }
}

/// Delivers queued events to subscribed systems and records them.
#[derive(Debug, Clone)]
pub struct EventBus {
    log: EventLog,
}

impl EventBus {
    pub fn new(history_capacity: usize) -> Self {
        Self {
            log: EventLog::new(history_capacity),
        }
    }

    pub fn log(&self) -> &EventLog {
        &self.log
    }

    pub fn log_mut(&mut self) -> &mut EventLog {
        &mut self.log
    }

    /// Dispatches queued events in FIFO order until the queue is empty or
    /// `budget` is exhausted. Returns the number of events dispatched.
    ///
    /// Delivering an event to an event-driven system also marks it for an
    /// update run (see [`crate::simulation::UpdateSchedule`]).
    pub fn dispatch(
        &mut self,
        budget: &mut u32,
        world: &mut WorldState,
        registry: &mut SystemRegistry,
        env: SimEnv<'_>,
    ) -> u32 {
        let mut dispatched = 0;
        while *budget > 0 {
            let Some(envelope) = world.events.pop() else {
                break;
            };
            *budget -= 1;
            dispatched += 1;
            let kind = envelope.event.kind();
            let mut handled_by = Vec::new();
            for n in 0..registry.subscriber_count(kind) {
                let index = registry.subscriber(kind, n);
                let entry = registry.entry_mut(index);
                let id = entry.id().clone();
                if entry.schedule().wakes_on_events() {
                    world.systems.entry(id.clone()).or_default().wake_pending = true;
                }
                let mut ctx = SimulationContext::new(
                    world,
                    env,
                    EventSource::System(id.clone()),
                    GameDuration::ZERO,
                );
                entry.handle_event(&envelope, &mut ctx);
                handled_by.push(id);
            }
            self.log.record(envelope, handled_by);
        }
        dispatched
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_is_fifo_with_sequential_ids() {
        let mut q = EventQueue::new();
        let a = q.push(
            1,
            GameInstant::EPOCH,
            EventSource::Clock,
            SimEvent::HourStarted { hour: 1 },
        );
        let b = q.push(
            1,
            GameInstant::EPOCH,
            EventSource::Clock,
            SimEvent::HourStarted { hour: 2 },
        );
        assert!(a < b);
        assert_eq!(q.pop().unwrap().id, a);
        assert_eq!(q.pop().unwrap().id, b);
        assert!(q.pop().is_none());
        assert_eq!(q.next_id(), EventId::from_raw(2));
    }

    #[test]
    fn every_kind_is_listed_once() {
        let mut kinds = EventKind::ALL.to_vec();
        kinds.sort();
        kinds.dedup();
        assert_eq!(kinds.len(), EventKind::ALL.len());
    }

    #[test]
    fn log_is_bounded_and_counts_everything() {
        let mut log = EventLog::new(2);
        for hour in 0..5 {
            let env = EventEnvelope {
                id: EventId::from_raw(u64::from(hour)),
                tick: 0,
                time: GameInstant::EPOCH,
                source: EventSource::Clock,
                event: SimEvent::HourStarted { hour },
            };
            log.record(env, vec![]);
        }
        assert_eq!(log.len(), 2);
        assert_eq!(log.total(), 5);
        assert_eq!(log.counts()[&EventKind::HourStarted], 5);
        assert_eq!(
            log.recent().next().unwrap().envelope.id,
            EventId::from_raw(4)
        );
    }
}
