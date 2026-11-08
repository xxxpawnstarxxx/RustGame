//! The [`System`] trait and the [`SystemRegistry`].
//!
//! A system is a unit of simulation behaviour. It declares *when* it wants to
//! run ([`UpdateSchedule`]), *which phase* of the tick it belongs to
//! ([`SystemPhase`]) and *which events* it listens to. Persistent state a
//! system needs belongs in [`crate::simulation::WorldState`] (components or a
//! dedicated resource), never in the system struct itself, so saving the
//! world captures everything.

use crate::core::error::{SimError, SimResult};
use crate::core::ids::SystemId;
use crate::core::time::{Calendar, GameDuration, GameInstant, TimeUnit};
use crate::simulation::context::SimulationContext;
use crate::simulation::events::{EventEnvelope, EventKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::time::Instant;

/// Ordered phases of a tick. All systems of one phase run before any system
/// of the next; queued events are dispatched between phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SystemPhase {
    /// World-wide state that other systems read (ambient light, season).
    PreUpdate,
    /// Main behaviour (routines, movement, needs, production…).
    Update,
    /// Consequences and bookkeeping (statistics, cleanup).
    PostUpdate,
}

impl SystemPhase {
    pub const ALL: [Self; 3] = [Self::PreUpdate, Self::Update, Self::PostUpdate];
}

/// When a system's `update` runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateSchedule {
    /// Every simulation tick (e.g. movement).
    EveryTick,
    /// Once per elapsed interval of game time (e.g. needs every 10 minutes,
    /// agriculture every hour). The first run happens on the first tick.
    Interval { amount: u64, unit: TimeUnit },
    /// Only on ticks after a subscribed event was delivered.
    EventDriven,
    /// On the interval *and* whenever a subscribed event was delivered
    /// (e.g. markets: daily, plus whenever trade happens).
    IntervalOrEvent { amount: u64, unit: TimeUnit },
}

impl UpdateSchedule {
    /// Whether event delivery wakes the system for an update.
    pub const fn wakes_on_events(self) -> bool {
        matches!(self, Self::EventDriven | Self::IntervalOrEvent { .. })
    }

    fn interval(self) -> Option<(u64, TimeUnit)> {
        match self {
            Self::Interval { amount, unit } | Self::IntervalOrEvent { amount, unit } => {
                Some((amount, unit))
            }
            Self::EveryTick | Self::EventDriven => None,
        }
    }
}

impl fmt::Display for UpdateSchedule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EveryTick => f.write_str("every tick"),
            Self::Interval { amount, unit } => write!(f, "every {amount} {unit:?}"),
            Self::EventDriven => f.write_str("on event"),
            Self::IntervalOrEvent { amount, unit } => {
                write!(f, "every {amount} {unit:?} or on event")
            }
        }
    }
}

/// A unit of simulation behaviour.
pub trait System {
    /// Stable, unique name.
    fn id(&self) -> SystemId;

    /// Tick phase this system runs in.
    fn phase(&self) -> SystemPhase {
        SystemPhase::Update
    }

    /// When `update` should run.
    fn schedule(&self) -> UpdateSchedule;

    /// Event kinds delivered to [`System::handle_event`].
    fn subscriptions(&self) -> &[EventKind] {
        &[]
    }

    /// Periodic update.
    fn update(&mut self, ctx: &mut SimulationContext<'_>);

    /// Reacts to a subscribed event. Called during dispatch, in event order.
    fn handle_event(&mut self, _event: &EventEnvelope, _ctx: &mut SimulationContext<'_>) {}
}

/// Persistent scheduling state of one system, stored in the world state so
/// update cadence survives save/load exactly.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SystemRunState {
    pub last_run_tick: Option<u64>,
    pub last_run_time: Option<GameInstant>,
    pub wake_pending: bool,
    pub run_count: u64,
}

/// Runtime performance counters of one system (not persisted).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SystemStats {
    pub last_update_micros: u64,
    pub total_update_micros: u64,
    pub updates: u64,
    pub events_handled: u64,
}

/// A registered system with its resolved schedule.
pub struct SystemEntry {
    system: Box<dyn System>,
    id: SystemId,
    phase: SystemPhase,
    schedule: UpdateSchedule,
    interval_ticks: Option<u64>,
    subscriptions: Vec<EventKind>,
    stats: SystemStats,
}

impl SystemEntry {
    pub fn id(&self) -> &SystemId {
        &self.id
    }

    pub fn phase(&self) -> SystemPhase {
        self.phase
    }

    pub fn schedule(&self) -> UpdateSchedule {
        self.schedule
    }

    /// Interval resolved to ticks, if the schedule has one.
    pub fn interval_ticks(&self) -> Option<u64> {
        self.interval_ticks
    }

    pub fn subscriptions(&self) -> &[EventKind] {
        &self.subscriptions
    }

    pub fn stats(&self) -> &SystemStats {
        &self.stats
    }

    /// Whether `update` should run on `tick` given the persistent run state.
    pub fn is_due(&self, state: &SystemRunState, tick: u64) -> bool {
        let interval_due = || match (self.interval_ticks, state.last_run_tick) {
            (Some(_), None) => true,
            (Some(n), Some(last)) => tick.saturating_sub(last) >= n,
            (None, _) => false,
        };
        match self.schedule {
            UpdateSchedule::EveryTick => true,
            UpdateSchedule::Interval { .. } => interval_due(),
            UpdateSchedule::EventDriven => state.wake_pending,
            UpdateSchedule::IntervalOrEvent { .. } => state.wake_pending || interval_due(),
        }
    }

    pub(crate) fn run_update(&mut self, ctx: &mut SimulationContext<'_>) {
        let started = Instant::now();
        self.system.update(ctx);
        let micros = started.elapsed().as_micros() as u64;
        self.stats.last_update_micros = micros;
        self.stats.total_update_micros += micros;
        self.stats.updates += 1;
    }

    pub(crate) fn handle_event(&mut self, event: &EventEnvelope, ctx: &mut SimulationContext<'_>) {
        self.system.handle_event(event, ctx);
        self.stats.events_handled += 1;
    }
}

/// Ordered collection of systems plus the event subscription table.
///
/// Systems are ordered by phase, then by registration order. That order is the
/// update order within a tick and the handler order for every event.
#[derive(Default)]
pub struct SystemRegistry {
    entries: Vec<SystemEntry>,
    subscribers: BTreeMap<EventKind, Vec<usize>>,
}

impl SystemRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a system. Intervals are converted to ticks using the
    /// calendar and the tick length (rounded up, minimum one tick).
    pub fn register(
        &mut self,
        system: Box<dyn System>,
        calendar: &Calendar,
        tick_duration: GameDuration,
    ) -> SimResult<()> {
        let id = system.id();
        if self.entries.iter().any(|e| e.id == id) {
            return Err(SimError::InvalidSystem(format!(
                "duplicate system id '{id}'"
            )));
        }
        let schedule = system.schedule();
        let interval_ticks = match schedule.interval() {
            Some((0, _)) => {
                return Err(SimError::InvalidSystem(format!(
                    "system '{id}' has a zero-length update interval"
                )));
            }
            Some((amount, unit)) => {
                let seconds = calendar.duration(amount, unit).seconds();
                let per_tick = tick_duration.seconds().max(1);
                Some(seconds.div_ceil(per_tick).max(1))
            }
            None => None,
        };
        let mut subscriptions = system.subscriptions().to_vec();
        subscriptions.sort();
        subscriptions.dedup();
        let phase = system.phase();
        let position = self
            .entries
            .iter()
            .position(|e| e.phase > phase)
            .unwrap_or(self.entries.len());
        self.entries.insert(
            position,
            SystemEntry {
                system,
                id,
                phase,
                schedule,
                interval_ticks,
                subscriptions,
                stats: SystemStats::default(),
            },
        );
        self.rebuild_subscribers();
        Ok(())
    }

    fn rebuild_subscribers(&mut self) {
        self.subscribers.clear();
        for (index, entry) in self.entries.iter().enumerate() {
            for kind in &entry.subscriptions {
                self.subscribers.entry(*kind).or_default().push(index);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Entries in execution order.
    pub fn iter(&self) -> impl Iterator<Item = &SystemEntry> {
        self.entries.iter()
    }

    pub fn entry(&self, index: usize) -> &SystemEntry {
        &self.entries[index]
    }

    pub(crate) fn entry_mut(&mut self, index: usize) -> &mut SystemEntry {
        &mut self.entries[index]
    }

    /// Number of systems subscribed to `kind`.
    pub fn subscriber_count(&self, kind: EventKind) -> usize {
        self.subscribers.get(&kind).map_or(0, Vec::len)
    }

    /// Entry index of the `n`-th subscriber of `kind`.
    pub fn subscriber(&self, kind: EventKind, n: usize) -> usize {
        self.subscribers[&kind][n]
    }
}
