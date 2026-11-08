# Game Design

## Vision

A grounded medieval world simulation viewed top-down on a 2D tile map. The
player watches, guides and eventually takes part in a living world in which
every person is individually simulated: they keep daily routines, work,
trade, fall ill, are injured, marry, quarrel, fight and die. Settlements grow
or wither according to their harvests, markets and lords. Stories come from
the interaction of systems, not from scripts.

## Pillars

1. **Systemic depth over content breadth.** A few deeply modelled systems that
   interact (time → routines → labour → production → markets → wealth →
   politics) beat many shallow ones.
2. **Historical plausibility.** No magic, supernatural phenomena, fantasy
   creatures or fictional materials. Technology, materials, occupations,
   architecture and social structures follow the configured historical period.
3. **Individuals matter.** Every character is a simulated entity with a body,
   needs, skills, a household and relationships — not a statistic.
4. **Emergence.** Systems communicate through events; outcomes such as famine,
   a guild dispute or a peasant revolt arise from state, not from triggers.
5. **Determinism and transparency.** A world seed plus the player's commands
   reproduce a world exactly. Debug tools expose why things happened.
6. **Moddable by data.** Content is JSON; behaviour extensions come via Lua.

## Setting

The default period is configured in `config/period.json`:
*fourteenth-century lowland England* (1300–1400). That means open-field
manorial agriculture, villeins and freemen, chartered market towns, craft
guilds, parish churches and monasteries, timber-framed and stone buildings,
water and post windmills, longbows, crossbows, mail and early plate armour.

The period is data, not code. Swapping the period file (and the content that
references its technology keys) moves the setting, e.g. to 13th-century
France, without code changes. Content stages validate every definition that
names a technology against `period.technologies`.

## Time

* 1 tick = 1 game second by default (`simulation.game_seconds_per_tick`).
* At 1× speed, 30 ticks per real second (30 game seconds per real second; one
  game day ≈ 48 real minutes). Speeds 1×, 3×, 10×, 30×, 60× are configurable.
* A Julian-style calendar with 12 months, 7-day weeks, configurable lengths,
  and per-month daylight hours (≈52°N); seasons are assigned per month.
* Day and night change light, and later visibility, work hours, safety and
  crop growth.

## The player (planned)

The initial interaction model is an observer/steward: inspect anyone and
anything, pause and speed time, and issue high-level directions through the
command interface (e.g. found a settlement, assign a household to a plot).
Direct control of a single character is a later option built on the same
command layer, so it never bypasses the simulation rules.

## Core loops (planned, by stage)

| Loop | Systems involved |
|------|------------------|
| Daily life | time, routines, needs, AI, movement |
| Subsistence | agriculture, environment, food, households |
| Production | crafting, items, workshops, labour |
| Exchange | markets, prices, coinage, trade routes |
| Society | settlements, ranks, obligations, law |
| Power | factions, relations, military, combat, anatomy |

## Implemented so far

### Phase 1 — Foundation ✅

Simulation core, event and command architecture, deterministic RNG,
configuration, the content registry, calendar/clock/pacing, timers, day/night
and seasons, data-driven daily routines (peasant labourer, town craftsman,
Benedictine monk with the canonical hours), persistence of the world state, a
Macroquad window with a programmatic day dial, and egui time controls and debug
inspectors.

### Phase 2 — Data and world ✅

* **Data** — 23 content categories (technologies, materials, items, weapons,
  armour, foods, resources, recipes, body templates, injuries, trees, crops,
  animals, buildings, professions, jobs, skills, factions, laws, routines,
  activities, characters, world-generation presets), generic registry with
  cross-reference and intrinsic validation, aggregated error reporting,
  versioned package manifest and fingerprint, and defaults for missing content.
* **World generation** — chunked tile map; warped fBm relief and ridged
  mountain chains; sea level by quantile; orographic precipitation and rain
  shadow; priority-flood hydrology, D8 flow routing, catchment-fed rivers,
  lakes, floodplains and wetness; slope, climate normals, landforms, soils and
  fertility; woodland species and coverage; clustered deposits; scored
  settlement sites with land clearing; road networks (MST plus extra links)
  with A* routing, fords and bridges. The default preset is 640² tiles; presets
  ship in `data/worldgen/presets.json`, including a 2048² region.
* **Environment** — regional weather with synoptic persistence, orographic
  rain, diurnal and seasonal cycles, snow; ground water with infiltration,
  evapotranspiration, runoff, mud, river stage and flooding, drought,
  contamination and fertility recovery; vegetation with grass, canopy species,
  growth, dormancy, drought mortality, colonisation and felling; regional
  wildlife with carrying capacity, breeding, predation, grazing and browsing,
  disease outbreaks, migration and hunting.
* **Navigation and movement** — per-tile traversal (walk, wade, swim, climb),
  structures, doors, gates, fences and bridges; terrain, road, woodland, slope,
  mud, snow and flood costs; budgeted A* without corner-cutting; re-planning on
  world change; occupancy and next-tile reservations; waiting and sidestepping;
  gaits, stamina, fatigue, encumbrance and injury-scaled mobility; stumbling
  and falls.
* **Persistence** — versioned save slots with metadata and content stamps,
  atomic writes, checksums, rotating backups, recovery, schema migration
  (v1 → v2) and semantic validation; the map is regenerated from its seed
  rather than stored, and a load resumes exactly.
* **Interface** — chunk-cached map renderer with nine overlays, pan/zoom
  camera, night wash, structures, settlements, characters and their routes; a
  tile/character inspector with actions; a world-and-saves window that creates
  worlds from presets; and debug tools (F3) for events, systems, timers,
  entities and quick save/load.

**Playable now:** an observer/steward prototype — explore a generated medieval
landscape, watch the weather, seasons and daylight, inspect any tile or person,
spawn characters with daily routines, place buildings, set doors, fell trees,
hunt wildlife, and save, load or generate worlds.

**Not yet implemented:** needs and households, AI intent and job selection,
economy and markets, crafting and agriculture production, living settlements
and society, factions and politics, military, combat and anatomy application,
items and inventories, and Lua modding.
