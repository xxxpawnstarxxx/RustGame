# medieval_sim

A grounded, top-down, tile-based medieval world simulation in Rust:
autonomous individually simulated people, settlements, economies, crafting,
combat and politics — no magic, no fantasy, no image assets.

Built with Macroquad (rendering/input), egui (interface), serde/JSON (data and
saves) and mlua (Lua 5.4 modding; scripting lands in a later stage). See
`docs/` for design and architecture.

**Current state:** Phase 2 complete — the world, environment, movement and
persistence layers are playable through an observer/steward interface. The
character-needs, AI, economy, crafting, society and combat stages are next.

## Requirements

* Rust 1.88+ (developed on 1.91, MSVC toolchain on Windows 11)
* A C compiler for the vendored Lua build (Visual Studio C++ build tools on Windows)

## Running

```bash
cargo run
```

The game finds `config/` and `data/` from the working directory (or set
`MEDIEVAL_SIM_ROOT`). Set `RUST_LOG=debug` for verbose logging.

### Controls

#### Map

| Input | Action |
|-------|--------|
| `W` `A` `S` `D` / arrow keys | pan the camera |
| mouse wheel | zoom in / out around the cursor (0.25×–48×) |
| middle mouse drag | pan the camera |
| left click | select a tile or the character under the cursor |
| right click | order the selected character to move to that tile |

The map ignores pointer and keyboard input while an egui window or text field
has focus.

#### Time and interface

| Key | Action |
|-----|--------|
| `Space` | pause / resume |
| `1`–`5` | select speed (1×, 3×, 10×, 30×, 60× by default; configurable) |
| `+` / `-` | faster / slower |
| `.` | step one tick while paused |
| `F3` | toggle debug tools |

The top bar also exposes pause/resume, all speed multipliers, step-tick and
step-hour while paused, the map overlay (natural, elevation, soil moisture,
fertility, temperature, snow, flood, contamination, resources), the night wash,
the world-and-saves window, debug tools and quit.

The inspector shows a tile's terrain, soil, hydrology, deposits, live weather
and regional wildlife, with actions to spawn a character, place a building, set
doors, fell trees and hunt. Selecting a character shows its record, movement
state, carried load and items. The world window creates new worlds from the
shipped presets (including a 2048² region), and saves, lists, loads and deletes
slots. Quick save / quick load to `saves/quicksave.json` is available in the
debug tools.

## Development

```bash
cargo fmt
cargo check --all-targets
cargo clippy --all-targets
cargo test
```

## Layout

| Path | Contents |
|------|----------|
| `src/` | library (`lib.rs`) and binary (`main.rs`); see `docs/TECHNICAL_ARCHITECTURE.md` |
| `config/` | simulation, calendar and historical period configuration |
| `data/` | content definitions (activities, daily routines) |
| `saves/` | save games (git-ignored) |
| `mods/` | future mods |
| `tests/` | integration tests |
| `docs/` | design, architecture, systems, data and roadmap documents |
