//! Saving and loading worlds.
//!
//! # Layout
//!
//! ```text
//! saves/<slot>/
//!     world.json          { "format_version": N, "world": { …WorldState… } }
//!     meta.json           SaveMetadata (name, seed, dates, counts, content, checksum)
//!     backups/
//!         world.1.json    previous save (newest backup)
//!         meta.1.json
//!         world.2.json …  up to `save_backups`
//! ```
//!
//! # Guarantees
//!
//! * **Atomic writes** — every file is written to a temporary sibling,
//!   flushed to disk and renamed over the target.
//! * **Backups** — before overwriting, the previous save rotates into
//!   `backups/` (oldest dropped).
//! * **Integrity** — `meta.json` records the FNV-1a checksum and size of
//!   `world.json`; a mismatch marks the save corrupt.
//! * **Recovery** — loading tries the main save, then each backup from the
//!   newest; it reports which one it recovered from and why the others
//!   failed.
//! * **Versioning and migration** — the world file carries its format
//!   version; older versions are migrated (see [`migration`]); newer ones are
//!   refused.
//! * **Validation** — loaded worlds are checked for consistency and against
//!   the current content (see [`validation`]).
//! * **Compatibility** — the content package, version and fingerprint (and
//!   active mods, once modding exists) are recorded and compared on load.
//!
//! Only [`WorldState`] is saved: rendering state, event history, pacing and
//! derived caches (the regenerable map, environment aggregates, occupancy)
//! are not.

pub mod migration;
pub mod validation;

pub use migration::{MigrationContext, SAVE_FORMAT_VERSION, migrate};
pub use validation::validate_world;

use crate::content::{ContentRegistry, ContentStamp};
use crate::core::config::GameConfig;
use crate::core::error::{SimError, SimResult};
use crate::core::hash::{fnv1a64, hex64};
use crate::core::time::{Calendar, GameInstant};
use crate::entities::EntityKind;
use crate::simulation::WorldState;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

const WORLD_FILE: &str = "world.json";
const META_FILE: &str = "meta.json";
const BACKUP_DIR: &str = "backups";

/// A mod active when the save was written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModStamp {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityCounts {
    pub total: usize,
    pub characters: usize,
    pub items: usize,
    pub settlements: usize,
    pub factions: usize,
    pub structures: usize,
}

/// Summary of a save, readable without loading the world.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SaveMetadata {
    pub format_version: u32,
    pub game_version: String,
    pub slot: String,
    pub world_name: String,
    pub seed: u64,
    pub preset: String,
    /// Real time the world was created (Unix seconds).
    pub created_unix: u64,
    /// Real time of this save (Unix seconds).
    pub saved_unix: u64,
    pub tick: u64,
    pub game_time: GameInstant,
    pub game_date: String,
    pub counts: EntityCounts,
    pub content: ContentStamp,
    pub mods: Vec<ModStamp>,
    /// Hex FNV-1a of `world.json`.
    pub world_checksum: String,
    pub world_bytes: u64,
}

/// State of a slot on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotStatus {
    Ok,
    MissingMetadata,
    CorruptMetadata(String),
    MissingWorld,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SaveSlotInfo {
    pub slot: String,
    pub status: SlotStatus,
    pub metadata: Option<SaveMetadata>,
    pub backups: usize,
}

/// Result of a successful load.
#[derive(Debug, Clone)]
pub struct LoadedSave {
    pub world: WorldState,
    pub metadata: Option<SaveMetadata>,
    /// Set when the main save was unusable and a backup was loaded.
    pub recovered_from: Option<PathBuf>,
    /// Format version the save was migrated from, if any.
    pub migrated_from: Option<u32>,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
struct WorldFileRef<'a> {
    format_version: u32,
    world: &'a WorldState,
}

fn io_error(path: &Path) -> impl FnOnce(std::io::Error) -> SimError + '_ {
    move |source| SimError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Writes `bytes` to `path` atomically: temp file, fsync, rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> SimResult<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(io_error(dir))?;
    let temp = path.with_extension("tmp");
    {
        let mut file = std::fs::File::create(&temp).map_err(io_error(&temp))?;
        file.write_all(bytes).map_err(io_error(&temp))?;
        file.sync_all().map_err(io_error(&temp))?;
    }
    std::fs::rename(&temp, path).map_err(io_error(path))
}

/// Serialises a world into a versioned world file.
pub fn world_to_json(world: &WorldState) -> SimResult<String> {
    serde_json::to_string(&WorldFileRef {
        format_version: SAVE_FORMAT_VERSION,
        world,
    })
    .map_err(|e| SimError::Persistence(format!("could not serialise world: {e}")))
}

/// Parses a world file, migrating older formats. Returns the world and the
/// version it was migrated from (if any).
pub fn world_from_json(json: &str, ctx: &MigrationContext) -> SimResult<(WorldState, Option<u32>)> {
    let mut file: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| SimError::Persistence(format!("could not parse save: {e}")))?;
    let version = file
        .get("format_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| SimError::Persistence("save has no format_version".into()))?
        as u32;
    let mut world = file
        .get_mut("world")
        .map(serde_json::Value::take)
        .ok_or_else(|| SimError::Persistence("save has no world".into()))?;
    migrate(&mut world, version, ctx)?;
    let world: WorldState = serde_json::from_value(world)
        .map_err(|e| SimError::Persistence(format!("save does not match the world format: {e}")))?;
    Ok((world, (version != SAVE_FORMAT_VERSION).then_some(version)))
}

/// Manages save slots under a directory.
#[derive(Debug, Clone)]
pub struct SaveManager {
    root: PathBuf,
    backups: u32,
}

impl SaveManager {
    pub fn new(root: impl Into<PathBuf>, backups: u32) -> Self {
        Self {
            root: root.into(),
            backups,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Validates a slot name: 1–64 characters of letters, digits, `-`, `_`.
    pub fn validate_slot(slot: &str) -> SimResult<()> {
        if slot.is_empty()
            || slot.len() > 64
            || !slot
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(SimError::Persistence(format!(
                "invalid slot name '{slot}': use 1–64 letters, digits, '-' or '_'"
            )));
        }
        Ok(())
    }

    fn slot_dir(&self, slot: &str) -> SimResult<PathBuf> {
        Self::validate_slot(slot)?;
        Ok(self.root.join(slot))
    }

    pub fn exists(&self, slot: &str) -> bool {
        self.slot_dir(slot)
            .is_ok_and(|d| d.join(WORLD_FILE).is_file())
    }

    /// Lists every slot directory with its status, sorted by name.
    pub fn list(&self) -> Vec<SaveSlotInfo> {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut slots: Vec<SaveSlotInfo> = entries
            .filter_map(Result::ok)
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().to_str().map(str::to_owned))
            .filter(|name| Self::validate_slot(name).is_ok())
            .map(|slot| {
                let dir = self.root.join(&slot);
                let backups = (1..=self.backups.max(1) + 8)
                    .filter(|i| {
                        dir.join(BACKUP_DIR)
                            .join(format!("world.{i}.json"))
                            .is_file()
                    })
                    .count();
                let (status, metadata) = if !dir.join(WORLD_FILE).is_file() {
                    (SlotStatus::MissingWorld, None)
                } else {
                    match std::fs::read(dir.join(META_FILE)) {
                        Err(_) => (SlotStatus::MissingMetadata, None),
                        Ok(bytes) => match serde_json::from_slice::<SaveMetadata>(&bytes) {
                            Ok(meta) => (SlotStatus::Ok, Some(meta)),
                            Err(e) => (SlotStatus::CorruptMetadata(e.to_string()), None),
                        },
                    }
                };
                SaveSlotInfo {
                    slot,
                    status,
                    metadata,
                    backups,
                }
            })
            .collect();
        slots.sort_by(|a, b| a.slot.cmp(&b.slot));
        slots
    }

    /// Saves a world into a slot, rotating the previous save into backups.
    pub fn save(
        &self,
        slot: &str,
        world: &WorldState,
        content: &ContentRegistry,
        calendar: &Calendar,
    ) -> SimResult<SaveMetadata> {
        let dir = self.slot_dir(slot)?;
        std::fs::create_dir_all(&dir).map_err(io_error(&dir))?;
        let json = world_to_json(world)?;
        let bytes = json.as_bytes();

        let count = |kind: EntityKind| world.entities.iter().filter(|(_, k)| *k == kind).count();
        let metadata = SaveMetadata {
            format_version: SAVE_FORMAT_VERSION,
            game_version: env!("CARGO_PKG_VERSION").to_owned(),
            slot: slot.to_owned(),
            world_name: world.meta.name.clone(),
            seed: world.meta.seed,
            preset: world.map_info.preset.to_string(),
            created_unix: world.meta.created_unix,
            saved_unix: unix_now(),
            tick: world.clock.tick(),
            game_time: world.clock.now(),
            game_date: calendar.format(world.clock.now()),
            counts: EntityCounts {
                total: world.entities.len(),
                characters: count(EntityKind::Character),
                items: count(EntityKind::Item),
                settlements: count(EntityKind::Settlement),
                factions: count(EntityKind::Faction),
                structures: count(EntityKind::Structure),
            },
            content: content.stamp(),
            mods: Vec::new(),
            world_checksum: hex64(fnv1a64(bytes)),
            world_bytes: bytes.len() as u64,
        };
        let meta_json = serde_json::to_vec_pretty(&metadata)
            .map_err(|e| SimError::Persistence(e.to_string()))?;

        self.rotate_backups(&dir)?;
        write_atomic(&dir.join(WORLD_FILE), bytes)?;
        write_atomic(&dir.join(META_FILE), &meta_json)?;
        tracing::info!(
            slot,
            tick = metadata.tick,
            bytes = metadata.world_bytes,
            "world saved"
        );
        Ok(metadata)
    }

    fn rotate_backups(&self, dir: &Path) -> SimResult<()> {
        if self.backups == 0 || !dir.join(WORLD_FILE).is_file() {
            return Ok(());
        }
        let backups = dir.join(BACKUP_DIR);
        std::fs::create_dir_all(&backups).map_err(io_error(&backups))?;
        for name in ["world", "meta"] {
            let oldest = backups.join(format!("{name}.{}.json", self.backups));
            if oldest.exists() {
                std::fs::remove_file(&oldest).map_err(io_error(&oldest))?;
            }
            for i in (1..self.backups).rev() {
                let from = backups.join(format!("{name}.{i}.json"));
                if from.exists() {
                    let to = backups.join(format!("{name}.{}.json", i + 1));
                    std::fs::rename(&from, &to).map_err(io_error(&from))?;
                }
            }
            let current = dir.join(format!("{name}.json"));
            if current.exists() {
                let to = backups.join(format!("{name}.1.json"));
                std::fs::copy(&current, &to).map_err(io_error(&current))?;
            }
        }
        Ok(())
    }

    /// Loads a slot, falling back to backups if the main save is unusable.
    pub fn load(
        &self,
        slot: &str,
        content: &ContentRegistry,
        config: &GameConfig,
    ) -> SimResult<LoadedSave> {
        let dir = self.slot_dir(slot)?;
        if !dir.is_dir() {
            return Err(SimError::Persistence(format!(
                "save slot '{slot}' does not exist"
            )));
        }
        let ctx = MigrationContext {
            default_preset: config.simulation.default_world_preset.clone(),
            default_seed: config.simulation.seed,
        };
        let mut candidates = vec![(dir.join(WORLD_FILE), dir.join(META_FILE))];
        for i in 1..=self.backups.max(1) + 8 {
            let world = dir.join(BACKUP_DIR).join(format!("world.{i}.json"));
            if world.is_file() {
                candidates.push((world, dir.join(BACKUP_DIR).join(format!("meta.{i}.json"))));
            }
        }
        let mut failures = Vec::new();
        for (index, (world_path, meta_path)) in candidates.iter().enumerate() {
            match Self::load_file(world_path, meta_path, content, &ctx) {
                Ok(mut loaded) => {
                    if index > 0 {
                        loaded.recovered_from = Some(world_path.clone());
                        loaded.warnings.extend(failures.iter().cloned());
                        tracing::warn!(slot, from = %world_path.display(), "main save unusable; recovered from backup");
                    }
                    return Ok(loaded);
                }
                Err(err) => failures.push(format!("{}: {err}", world_path.display())),
            }
        }
        Err(SimError::Persistence(format!(
            "no usable save in slot '{slot}':\n  {}",
            failures.join("\n  ")
        )))
    }

    fn load_file(
        world_path: &Path,
        meta_path: &Path,
        content: &ContentRegistry,
        ctx: &MigrationContext,
    ) -> SimResult<LoadedSave> {
        let bytes = std::fs::read(world_path).map_err(io_error(world_path))?;
        let mut warnings = Vec::new();
        let metadata = match std::fs::read(meta_path) {
            Ok(meta) => match serde_json::from_slice::<SaveMetadata>(&meta) {
                Ok(meta) => Some(meta),
                Err(e) => {
                    warnings.push(format!("metadata unreadable ({e}); integrity not verified"));
                    None
                }
            },
            Err(_) => {
                warnings.push("metadata missing; integrity not verified".into());
                None
            }
        };
        if let Some(meta) = &metadata {
            let checksum = hex64(fnv1a64(&bytes));
            if checksum != meta.world_checksum || bytes.len() as u64 != meta.world_bytes {
                return Err(SimError::Persistence(format!(
                    "checksum mismatch (file {checksum}, {} bytes; recorded {}, {} bytes): the save is corrupt or incomplete",
                    bytes.len(),
                    meta.world_checksum,
                    meta.world_bytes
                )));
            }
            let current = content.stamp();
            if meta.content.package != current.package {
                warnings.push(format!(
                    "saved with content package '{}', running '{}'",
                    meta.content.package, current.package
                ));
            } else if meta.content.version != current.version {
                warnings.push(format!(
                    "saved with content version {}, running {}",
                    meta.content.version, current.version
                ));
            } else if meta.content.fingerprint != current.fingerprint {
                warnings.push("content files have changed since this save was written".into());
            }
            for m in &meta.mods {
                warnings.push(format!(
                    "save used mod '{}' {}, which is not loaded",
                    m.id, m.version
                ));
            }
        }
        let text = std::str::from_utf8(&bytes)
            .map_err(|e| SimError::Persistence(format!("save is not UTF-8: {e}")))?;
        let (world, migrated_from) = world_from_json(text, ctx)?;
        let problems = validate_world(&world, content);
        if !problems.is_empty() {
            return Err(SimError::Persistence(format!(
                "save failed validation:\n    {}",
                problems.join("\n    ")
            )));
        }
        Ok(LoadedSave {
            world,
            metadata,
            recovered_from: None,
            migrated_from,
            warnings,
        })
    }

    /// Deletes a slot and its backups.
    pub fn delete(&self, slot: &str) -> SimResult<()> {
        let dir = self.slot_dir(slot)?;
        if dir.is_dir() {
            std::fs::remove_dir_all(&dir).map_err(io_error(&dir))?;
        }
        Ok(())
    }
}
