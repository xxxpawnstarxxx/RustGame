# Data Architecture

## Directories

| Directory | Contents | Read by |
|-----------|----------|---------|
| `config/` | how the simulation runs: `simulation.json`, `calendar.json`, `period.json`, `movement.json`, `environment.json` | `ConfigurationManager` |
| `data/` | what can exist: `manifest.json` plus one sub-directory per content category holding JSON arrays | `ContentRegistry` |
| `saves/<slot>/` | `world.json`, `meta.json`, `backups/` (git-ignored) | `persistence::SaveManager` |
| `mods/` | future mod packages | modding stage |

The application finds the root by checking `$MEDIEVAL_SIM_ROOT`, the working
directory and its ancestors, the executable's ancestors, then the crate
directory.

## Configuration (`config/`)

Missing files fall back to built-in defaults (identical to the shipped files —
a test enforces this) with a warning. Malformed or inconsistent files are
fatal. Unknown fields are rejected.

| File | Structure | Highlights |
|------|-----------|-----------|
| `simulation.json` | `SimulationConfig` | seed, tick length, speeds, per-tick budgets, default world preset and name, number of save backups |
| `calendar.json` | `CalendarConfig` | time units, months (days, season, daylight hours), weekdays, start date, twilight, night light |
| `period.json` | `PeriodConfig` | period name and year range; **technology keys available** |
| `movement.json` | `MovementConfig` | gaits (speed, stamina, fatigue, stumble), terrain costs, road/bridge/ford multipliers, mud/snow/flood penalties, door delays, tile capacity, path budget, re-plan and give-up waits, hazard chances, encumbrance limits, injury ids for falls and stumbles |
| `environment.json` | `EnvironmentConfig` | weather / ground / wildlife cell sizes; snow, melt, flood, drought, growth, contamination, fertility and forage rates |

## Content (`data/`)

### Package manifest

```json
{ "id": "base", "name": "…", "version": "0.2.0", "format_version": 1, "description": "…" }
```

`format_version` is the package layout version this build understands
(`CONTENT_FORMAT_VERSION`); `version` is the content's own version. The
manifest and every file read contribute to a 64-bit FNV-1a **fingerprint**.
Package id, version and fingerprint form the `ContentStamp` stored in saves.

### Loading and validation

1. Files in each category directory are read in file-name order; each is a
   JSON array. Parse errors are collected (with the file name) and loading
   continues.
2. Duplicate ids within a category are reported with both files.
3. Every definition lists its **cross-references** (`Definition::references`);
   each is checked against the target category.
4. Every definition runs its **intrinsic checks** (`Definition::validate`):
   ranges, units, ordering, calendar months, layout characters, technology
   availability in the period, item categories of referenced items, body
   parts named by armour, and so on.
5. **Global checks**: every period technology has a definition, technology
   prerequisites are acyclic, the default world preset exists, movement
   injury ids exist.

All problems are reported together as `SimError::ContentValidation`, e.g.
`data/weapons/bad.json [weapons 'laser']: item refers to unknown items 'laser_gun'`.

Missing category directories are empty (with a warning); a missing manifest
gives an "unversioned" package (with a warning). Optional fields have
documented serde defaults (e.g. `stack_size` 1, `walking_speed_mps` 1.35,
`carry_capacity_kg` 30, `can_climb` true, `spawn_at_world_creation` true).

### Categories

| Directory | Type | Key fields | References |
|-----------|------|-----------|------------|
| `technologies` | `TechnologyDef` | prerequisites, introduced year | technologies |
| `materials` | `MaterialDef` | category, density, hardness, toughness, value | technology |
| `items` | `ItemDef` | category, weight, stack size, value | materials, technology |
| `weapons` | `WeaponDef` | class, damage profiles, reach, hands, range | item, skill, technology |
| `armour` | `ArmourDef` | layer, coverage (body parts), protection | item, material, technology |
| `foods` | `FoodDef` | group, kcal/kg, spoilage, preserved | item |
| `resources` | `ResourceDef` | kind, renewable, regeneration | item, technology |
| `recipes` | `RecipeDef` | inputs, outputs, tools, labour hours | items, building, skill, technology |
| `body_templates` | `BodyTemplateDef` | mass, parts tree (vital, locomotion, grasping) | — |
| `injuries` | `InjuryDef` | kind, severity, bleeding, pain, healing, infection, mobility | — |
| `skills` | `SkillDef` | category | — |
| `professions` | `ProfessionDef` | social rank | skills, routine, jobs, workplaces |
| `jobs` | `JobDef` | labour hours, seasonal months | skill, building, recipe, tools |
| `characters` | `CharacterTemplateDef` | age range, skill ranges, starting items, walking speed, carry capacity, swim/climb, names | body, profession, skills, items |
| `buildings` | `BuildingDef` | category, layout (`#` wall `.` floor `D` door `G` gate `F` fence), materials, build hours | materials, technology |
| `animals` | `AnimalDef` | diet, domesticated, mass, habitat weights, density, growth, breeding months, territory, prey, food need, migration, diseases | body, prey, products |
| `crops` | `CropDef` | sowing months, growth days, yield, temperatures, water and fertility use | item, technology |
| `trees` | `TreeSpeciesDef` | deciduous, growth, moisture/temperature ranges, max elevation, leaf months | timber item |
| `laws` | `LawDef` | category, penalty | — |
| `factions` | `FactionDef` | kind, parent, laws, initial relations | factions, laws |
| `worldgen` | `WorldGenPresetDef` | size, chunk size, tile size; elevation, hydrology, climate (monthly normals), vegetation, resource rules, settlements, roads, wildlife | resources, animals |
| `activities` | `ActivityDef` | category | — |
| `routines` | `DailyRoutineDef` | blocks, weekday overrides | activities |

Adding content requires only JSON. Adding a *category* requires a struct,
a `Definition` impl and one line in the `content_registry!` macro.

### Definitions versus instances

Definitions never change at runtime. Individual things are entities with
components that reference definitions by typed id:

| Instance component | Refers to |
|--------------------|-----------|
| `CharacterRecord` (template, body, profession, birth, skills, home, faction) | `characters`, `body_templates`, `professions`, `skills` |
| `ItemInstance` (definition, quantity, quality, condition, location) | `items` |
| `FactionRecord` (definition, kind, parent, relations) | `factions` |
| `SettlementRecord` (name, kind, centre, founded, faction) | — |
| `BuildingRecord` (definition, origin) | `buildings` |

## Persistent world state (`WorldState`)

| Field | Notes |
|-------|-------|
| `meta` | world name, seed, real creation time |
| `map_info` | generation preset, seed, size, map fingerprint |
| `clock`, `rng` | tick, game time; RNG seed/stream/position |
| `entities`, `components` | generational slots; one `ComponentStore` per component (names, routines, positions, movers, characters, items, settlements, factions, buildings) |
| `structures` | wall/floor/door/gate/fence cells by tile |
| `environment` | weather cells and regional hydrology, ground grid (structure-of-arrays), wildlife populations, environment RNG |
| `navigation` | change epoch per map chunk (path invalidation) |
| `timers`, `events`, `commands` | pending queues |
| `systems` | each system's scheduling state |
| `ambient` | daylight and season |
| `derived` *(not saved)* | regenerated static map, environment aggregates, occupancy |

### Why the map is not saved

The static map (elevation, terrain, soils, hydrology, roads, deposits,
initial woodland) is a pure function of preset, seed and content. Saves
store `map_info` and the map is regenerated on load (0.7 s for the default
640 × 640 map); its fingerprint must match or loading fails with an
explanation. Everything that changes during play is stored separately and
compactly: the environment at 4 × 4-tile resolution in structure-of-arrays
form, structures sparsely.

## Save files

```text
saves/<slot>/world.json        { "format_version": 2, "world": { … } }
saves/<slot>/meta.json         SaveMetadata
saves/<slot>/backups/world.N.json, meta.N.json
```

`SaveMetadata`: format and game version, slot, world name, seed, preset,
creation and save times (Unix), tick, game time and formatted date, entity
counts by kind, content stamp, mods, and the checksum and size of
`world.json`.

* **Atomic writes**: temp file → `fsync` → rename.
* **Backups**: the previous save rotates to `backups/world.1.json` (older
  ones shift up; the oldest beyond `save_backups` is dropped).
* **Integrity**: checksum and size from `meta.json` must match.
* **Recovery**: on failure (unreadable, checksum, migration, parse or
  validation) the loader tries each backup, newest first, and reports which
  copy it used and why the others failed.
* **Migration**: the untyped JSON is upgraded one version at a time before
  typed parsing. v1 → v2 adds metadata, map info, structures, environment,
  navigation and the new component stores; the environment of a migrated
  world is initialised on restore.
* **Validation**: components must belong to live entities of the right
  kind, content ids must resolve, positions must lie on the map, carried
  items need live holders, structures need live buildings.
* **Compatibility**: a different content package, version or fingerprint,
  or mods no longer loaded, produce warnings.

## Identifier types

| Id | Shape | Allocation |
|----|-------|------------|
| `EntityId` | `{ index: u32, generation: u32 }` | `EntityManager` (LIFO slot reuse) |
| `EventId`, `TimerId`, `JobId` | `u64` | persistent `IdSequence` |
| `SystemId` | string | constant per system |
| content ids (`ItemDefId`, `AnimalId`, …) | string | defined in data |

## Time representation

`GameInstant`/`GameDuration` are `u64` seconds; months, days and weekdays
are 0-based internally; years have fixed length.
