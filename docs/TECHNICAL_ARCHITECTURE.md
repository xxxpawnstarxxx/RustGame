# Technical Architecture

## Technology

| Concern | Choice |
|---------|--------|
| Language | Rust 2024 edition, `unsafe_code = "forbid"` for this crate |
| Window, input, 2D drawing | Macroquad 0.4 (primitives and text only; no image assets) |
| Interface | egui 0.31 via egui-macroquad 0.17 |
| Data | serde + serde_json |
| Scripting (planned) | mlua with vendored Lua 5.4 |
| Randomness | rand 0.10 + rand_chacha (ChaCha8, seeded, serialisable) |
| Path finding (planned) | pathfinding (A*, Dijkstra) |
| Errors | thiserror (`SimError`) in the library, anyhow at the application edge |
| Logging | tracing + tracing-subscriber (`RUST_LOG` overrides) |

No game engine framework (no Bevy). The architecture is a small hand-rolled
entity/component/system design that favours determinism and persistence.

## Crate layout

The package is a library (`src/lib.rs`) plus a thin binary (`src/main.rs`).
All logic lives in the library so integration tests drive the simulation
headlessly.

```text
src/
  main.rs         window configuration; calls app::run
  lib.rs          module declarations
  app/            start-up, path discovery, the frame loop
  core/           ids, errors, RNG, configuration, calendar  (depends on nothing)
  entities/       EntityManager, ComponentStore, Name
  content/        ContentRegistry (macro-generated tables), 23 definition categories
  simulation/     GameState, WorldState, events, commands, systems, scheduler,
                  world creation/restoration, runtime instance components
    time/         clock, pacing, timers, daylight, ambient system, routines
  world/          coordinates, chunked static map, noise, generation, structures
  environment/    weather, ground water, vegetation, wildlife, queries, systems
  navigation/     traversal, A*, occupancy, movers, navigation/movement systems
  persistence/    save slots, metadata, checksums, backups, migration, validation
  rendering/      chunk-cached map textures, overlays, camera (reads state only)
  ui/             egui interface: time bar, inspector, world & saves window
  debugging/      egui inspectors and debug actions
  characters/ anatomy/ combat/ items/ inventory/ ai/ economy/ crafting/
  settlements/ factions/ military/ scripting/
                  domain modules: boundaries documented, filled by later stages
```

## Layering rules

```text
app ──► ui, debugging, rendering ──► simulation ──► content, entities ──► core
```

* Lower layers never import higher ones. `core` imports nothing from the crate.
* `simulation` never imports Macroquad or egui. Rendering and UI only *read*
  simulation state and *write* through `Command`s or the `TimeController`.
* Domain modules (characters, economy, …) will each contribute components,
  systems, events and content types, and communicate only through events and
  commands — never by calling each other's systems.

## Kinds of state

| # | Kind | Where | Persisted | Mutated by |
|---|------|-------|-----------|------------|
| 1 | Persistent world state | `WorldState` | yes (save file) | systems and commands, during ticks |
| 2 | Immutable content | `ContentRegistry` | no (reloaded from `data/`) | nobody after load |
| 3 | Temporary runtime state | `TimeController`, `EventLog`, `SystemStats`, `SimulationScheduler` diagnostics | no | runtime |
| 4 | Simulation systems | `SystemRegistry` (boxed `System`s) | no (re-registered at start-up) | — |
| 5 | Rendering/interface state | `UiState`, `DebugTools`, egui memory | no | UI |

The invariant: *nothing outside `WorldState` may influence simulation
outcomes.* Systems therefore keep any persistent state in components or world
resources, never in their own struct. Each system's scheduling state
(`SystemRunState`) is in the world too, so cadence survives save/load.

## Tick pipeline

`GameState::advance_frame(real_dt)` asks the `TimeController` how many fixed
ticks this frame needs (accumulator × speed, capped by `max_ticks_per_frame`),
then runs each tick through `SimulationScheduler::run_tick`:

```text
1. clock.advance()                 → YearStarted/MonthStarted/WeekStarted/DayStarted/HourStarted
2. fire due timers                 → emit event, or queue command (applied in step 3)
3. apply queued commands (FIFO)    → events
4. dispatch events
5. for phase in [PreUpdate, Update, PostUpdate]:
       for each system in phase (registration order): if due → update(ctx)
       dispatch events
```

Event dispatch delivers each queued event, in id order, to every subscribed
system in registry order. Handlers can emit more events; they are appended and
delivered in the same pass. A per-tick budget (`max_events_per_tick`) bounds
runaway cascades: remaining events stay in the persistent queue and go first
next tick (and the deferral is logged and counted).

## Determinism

* Fixed timestep: every tick advances exactly `game_seconds_per_tick`.
* Real time, frame rate, pause and speed affect only *how many* ticks run per
  frame, never what a tick does.
* All collections that are iterated are ordered (`BTreeMap`, `Vec`,
  `VecDeque`); no `HashMap` iteration in simulation code.
* All randomness comes from `WorldState::rng` (`SimRng`), whose seed, stream
  and word position are serialised. `SimRng::derive_stream` gives subsystems
  independent reproducible streams.
* Ids are values (generational `EntityId`, sequential `EventId`/`TimerId`),
  never addresses.
* Integration tests assert that two identical runs, and a run interrupted by
  save/load, produce byte-identical world JSON.

## Error handling

Library functions return `SimResult<T>`. A rejected command leaves the world
unchanged, is logged with `tracing::warn!`, and is kept in the scheduler's
rejection history for the debug UI. Configuration and content errors are fatal
at start-up with an explanatory message.

## Testing

* Unit tests sit beside the code (`#[cfg(test)]`), including proptest
  properties for calendar round-trips and recurrence ordering.
* `tests/simulation_core.rs` covers commands, events, cascades, scheduling,
  timers, routines, pacing, determinism and save/load continuity.
* Every stage ends with `cargo fmt`, `cargo check --all-targets`,
  `cargo clippy --all-targets`, `cargo test`.
