# Development Roadmap

Each stage ends with `cargo fmt`, `cargo check --all-targets`,
`cargo clippy --all-targets` and `cargo test` passing, and with these documents
updated. A stage never reaches into a later stage's domain.

## Phase 1 — Foundation ✅

| Prompt | Scope |
|--------|-------|
| 01 | Design and architecture documents |
| 02 | Cargo project, dependencies, module tree, directories, window + egui |
| 03 | WorldState, GameState, SimulationContext, EntityManager, SystemRegistry, EventBus, CommandQueue, SimulationScheduler, ConfigurationManager, ContentRegistry, typed events |
| 04 | Calendar, clock, day/night, pause/speeds, fixed timestep, timers, per-system intervals, daily routines |

## Phase 2 — Data and world ✅

| Prompt | Scope |
|--------|-------|
| 05 | 23 content categories, generic registry, cross-reference and intrinsic validation, aggregated error reporting, defaults, missing-content handling, versioned manifest and fingerprint, duplicate detection; definitions separate from instances |
| 06 | Save slots with metadata, timestamps, content/mod stamps; atomic writes, checksums, rotating backups, recovery, schema versioning with migration (v1 → v2), semantic validation; exact resumption; map regenerated rather than stored |
| 07 | Chunked tile map; relief, hills, mountains, valleys, plains, woodland, grassland, rivers, lakes, coast, marsh, roads with fords and bridges, soils and fertility, deposits, settlement sites; three presets including a 2048² region |
| 08 | Regional weather (temperature, humidity, cloud, wind, rain, snow), ground water (moisture, runoff, mud, floods, drought, contamination, fertility), vegetation (grass, species canopy, regeneration, felling), wildlife (capacity, breeding, predation, grazing/browsing, disease, migration, hunting); query interfaces for other domains |
| 09 | A* with corner rules, traversal modes (walk, wade, swim, climb), terrain/road/forest/slope/weather costs, walls, doors, gates, fences, bridges, budgeted planning, re-planning on change, occupancy and reservations, waiting, sidestepping, gaits, stamina, fatigue, encumbrance from items, mobility, stumbling and falls |

Also delivered to make Phase 2 usable: a chunk-cached map renderer with
overlays, camera controls, an inspector, and a world/saves window.

## Later phases (provisional ordering)

| Phase | Stage | Modules |
|-------|-------|---------|
| 3 | Characters: households, needs (hunger, warmth, rest), health applying injuries to mobility | `characters` |
| 3 | AI: intent from routines and needs, job selection, travel using navigation | `ai` |
| 4 | Combat and anatomy | `anatomy`, `combat` |
| 4 | Items, inventories, equipment slots | `items`, `inventory` |
| 5 | Crafting, agriculture and production using recipes, crops and growing conditions | `crafting`, `economy` |
| 5 | Economy: markets, prices, coinage, wages, rents | `economy` |
| 6 | Settlements and society: buildings in use, plots, obligations, law | `settlements` |
| 6 | Factions and politics | `factions` |
| 7 | Military | `military` |
| 8 | Modding: Lua runtime, mod packages, content overrides (mod stamps in saves) | `scripting`, `content` |

## Known limitations

* Years have fixed length (no leap years).
* Routine transitions are detected at one-minute resolution.
* The environment is simulated at 4 × 4-tile resolution; individual tiles
  inherit their cell's moisture, snow, mud and flooding.
* Wild animals are population counts, not entities; individual animals
  will be instantiated when characters hunt or herd them.
* Structures are placed instantly; construction labour arrives with the
  settlement stage.
* Path planning is single-level A* with an expansion budget; very long
  journeys on the 2048² preset may need hierarchical planning later.
* Injuries are emitted as events; applying them to bodies and mobility is
  the anatomy stage.
