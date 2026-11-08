# Systems Overview

## How systems communicate

Systems never call each other and never hold references to each other. They
interact only through:

1. **World state** — reading and writing components and world resources.
2. **Events** (`SimEvent`) — "something happened". Emitted into the persistent
   `EventQueue`, delivered to subscribers in deterministic order.
3. **Commands** (`Command`) — "please change something". Used by the UI, debug
   tools, timers and (later) AI and scripts. Applied at a fixed point of the
   next tick, validated, and rejected without side effects if invalid.
4. **Queries** — read-only views such as `EnvironmentQuery` and
   `navigation::traversal`, the defined interfaces through which one domain
   reads another's state.

A system declares its id, phase (`PreUpdate`, `Update`, `PostUpdate`),
update schedule (`EveryTick`, `Interval`, `EventDriven`, `IntervalOrEvent`)
and event subscriptions. Interval systems integrate over `ctx.elapsed()`.

## Registered systems (execution order)

| System | Phase | Schedule | Writes / emits |
|--------|-------|----------|----------------|
| `ambient_time` | Pre | 1 min | `ambient`; `DayPhaseChanged`, `SeasonChanged` |
| `weather` | Pre | 1 h | weather cells, synoptic state; `PrecipitationStarted/Stopped` |
| `ground` | Update | 1 h | ground grid, regional hydrology; `DroughtStarted/Ended`, `FloodStarted/Ended`; invalidates navigation in flooded regions |
| `vegetation` | Update | 1 day | grass, canopy, species |
| `wildlife` | Update | 1 day | populations, grazing on grass, browse pressure, carcass contamination; `WildlifeOutbreakStarted/Ended` |
| `routines` | Update | 1 min | routine assignments; `ScheduledActivityChanged` |
| `navigation` | Update | every tick | plans paths (budgeted); `PathFailed` |
| `movement` | Update | every tick | positions, steps, stamina, fatigue, occupancy; `DestinationReached`, `PathFailed`, `CharacterStumbled`, `CharacterFell`, `CharacterInjured` |
| `encumbrance` | Post | on `ItemCreated` / `ItemTransferred` | movers' carried weight |

Engine-level producers: the clock (calendar boundaries), command
application (entity, item, structure, door, felling, hunting and faction
events) and timers.

## World generation (`world::generation`)

A pure function of preset, seed and content:

1. **Relief** — warped fBm plus ridged chains masked into ranges, tilted
   towards the coast side; sea level by quantile.
2. **Precipitation** — orographic lift on windward slopes, rain shadow in
   the lee.
3. **Hydrology** — priority-flood depression filling; deep depressions
   become lakes; D8 flow routing; rainfall-weighted catchment accumulation;
   rivers above a catchment threshold, widened by discharge; height above
   nearest drainage (flood risk, wetness).
4. **Slope, climate normals, terrain classes, landforms (plain, valley,
   hill, mountain), soils and fertility.**
5. **Woodland** — species chosen by climatic suitability, extent by
   coverage target.
6. **Resources** — per-rule terrain and elevation eligibility, clustered.
7. **Settlements** — scored on fertility, fresh water, flatness, flood
   safety, coast, deposits and woodland, at a minimum spacing; sized by
   rank into towns, villages and hamlets; land cleared around them.
8. **Roads** — MST plus extra links, A* over terrain cost; fords and
   bridges at river crossings.

Geography then drives play: terrain and roads set movement costs; soils,
moisture and floods set fertility and growing conditions; habitat sets
wildlife capacity; deposits mark where extraction will happen.

## Environment (`environment`)

* **Weather** (per 32 × 32-tile cell, hourly): monthly normals interpolated
  through the year; north–south gradient, lapse rate, coastal damping;
  AR(1) synoptic temperature, wetness and wind anomalies with the preset's
  persistence; a spatial weather field drifting downwind; orographic rain;
  diurnal cycle; snow below a threshold; rain frequency matched to the
  monthly rain days.
* **Ground** (per 4 × 4-tile cell, hourly): snow accumulation and
  degree-hour melt; infiltration by soil; evapotranspiration; drainage and
  saturation runoff; standing water; regional catchment surplus → river
  stage → flooding of cells below the stage (scaled by nearby discharge)
  with silt deposition and spreading contamination; mud; contamination
  decay and settlement sources; fertility recovery; regional drought.
* **Vegetation** (daily): logistic grass growth under canopy shade with
  dormancy, frost, drought and flood die-back; canopy growth by species
  suitability; drought mortality; colonisation of open cells from
  neighbouring woodland, suppressed by browsing; felling.
* **Wildlife** (per 64 × 64-tile region, daily): carrying capacity from
  habitat, area, settlement disturbance, hunting wariness, grass (grazers)
  and territory (predators); seasonal breeding; type-II predation shared by
  prey biomass with predator starvation; grazing and browsing feedback on
  vegetation; SIR disease outbreaks with immunity, carcass fouling and
  spread by migration; migration towards spare capacity; hunting pressure.

`EnvironmentQuery` exposes local conditions (temperature, wind chill,
precipitation, snow, mud, flood, moisture, water quality, canopy, drought),
movement penalties, growing conditions, building stress and regional
resources. Movement uses it now; needs, agriculture, buildings and the
economy will.

Wolves are deliberately absent: they were extinct in England by the
fourteenth century. Predators are fox and wildcat.

## Navigation and movement (`navigation`)

* **Traversal** per tile and ability set: walk, wade (shallows, fords),
  swim (deep water, if able and lightly loaded), climb (cliffs, if able);
  structures (walls and fences block; closed doors and gates cost an
  opening delay; locked ones block; floors are firm); road, track and
  bridge multipliers; woodland, slope, mud, snow and flood penalties.
* **A\*** over eight neighbours without corner-cutting, admissible octile
  heuristic, crowd penalty near the start, expansion budget.
* **Re-planning** when the next tile becomes impassable, when a chunk on
  the remaining path changed after planning (per-chunk epochs bumped by
  buildings, doors, felling and floods), after sidestepping and after falls.
* **Occupancy and reservations**: a character occupies its tile and
  reserves the next before stepping; tiles hold at most `tile_capacity`.
  Blocked characters wait, then sidestep to a free tile that keeps their
  progress, then re-plan treating occupied tiles as costly, and finally
  give up.
* **Locomotion**: crouch, walk, run, sprint; swimming and climbing speeds;
  stamina drain and recovery per gait, fatigue, automatic slowing when
  exhausted; encumbrance from carried items slows, forbids running above
  capacity and swimming above the swim limit, and stops movement at the
  hard limit; mobility (injury) scales everything.
* **Hazards**: stumbling (roughness × gait × fatigue × load ÷ mobility)
  costs time and stamina and, at speed, may sprain; climbing can end in a
  fall whose height sets the injury.

## Event catalogue

| Group | Events |
|-------|--------|
| Entities | `EntityCreated`, `EntityDestroyed` |
| Characters | `CharacterInjured`, `CharacterDied` |
| Items | `ItemCreated`, `ItemTransferred` |
| Work | `JobStarted`, `JobCompleted` |
| Society | `SettlementFounded`, `FactionRelationChanged` |
| Structures | `StructureBuilt`, `StructureRemoved`, `DoorChanged` |
| Calendar | `HourStarted`, `DayStarted`, `WeekStarted`, `MonthStarted`, `YearStarted` |
| Ambient | `SeasonChanged`, `DayPhaseChanged` |
| Routines | `ScheduledActivityChanged` |
| Environment | `PrecipitationStarted/Stopped`, `DroughtStarted/Ended`, `FloodStarted/Ended`, `TreesFelled` |
| Wildlife | `WildlifeOutbreakStarted/Ended`, `WildlifeHunted` |
| Movement | `DestinationReached`, `PathFailed`, `CharacterStumbled`, `CharacterFell` |

## Commands

`SpawnEntity`, `SpawnCharacter`, `SpawnItem`, `TransferItem`,
`DestroyEntity` (drops carried items, frees occupancy, removes structures),
`AssignRoutine`, `ClearRoutine`, `MoveTo`, `StopMoving`, `SetGait`,
`PlaceBuilding`, `SetDoor`, `FellTrees`, `HuntWildlife`,
`SetFactionRelation`, `ScheduleTimer`, `CancelTimer`.

## Interface and debugging

The map view draws chunk textures computed from data (overlays: natural,
elevation, soil moisture, fertility, temperature, snow, flooding, water
fouling, resources), structures, settlements, characters with their routes
and a night wash. The inspector shows a tile's terrain, soil, hydrology,
deposit, live conditions and regional wildlife, plus actions (spawn a
character, place a building, set doors, fell trees, hunt), and the selected
entity's record, movement state, load and items. The world window saves,
lists, loads and deletes slots and generates new worlds. Debug tools (F3)
show events, systems, timers and entities, and quick-save/quick-load.
