//! Lua 5.4 scripting and modding via `mlua`.
//!
//! Will own: a sandboxed Lua runtime, read-only world queries, command
//! submission and event subscriptions exposed to scripts, and loading of mods
//! from `mods/`. Scripts interact with the simulation only through commands
//! and events, never by direct mutation. Implemented in the *Modding* stage.
