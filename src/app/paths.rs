//! Locating the game's data, configuration and save directories.

use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

/// Environment variable that overrides the project root.
pub const ROOT_ENV: &str = "MEDIEVAL_SIM_ROOT";

/// Directories the application reads and writes.
#[derive(Debug, Clone)]
pub struct GamePaths {
    pub root: PathBuf,
    pub config: PathBuf,
    pub data: PathBuf,
    pub saves: PathBuf,
    pub mods: PathBuf,
}

impl GamePaths {
    pub fn from_root(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            config: root.join("config"),
            data: root.join("data"),
            saves: root.join("saves"),
            mods: root.join("mods"),
        }
    }

    /// Finds the root containing `config/` and `data/`, checking in order:
    /// `$MEDIEVAL_SIM_ROOT`, the working directory and its ancestors, the
    /// executable's directory and its ancestors, then the crate directory.
    pub fn locate() -> Result<Self> {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(root) = std::env::var_os(ROOT_ENV) {
            candidates.push(PathBuf::from(root));
        }
        if let Ok(cwd) = std::env::current_dir() {
            candidates.extend(cwd.ancestors().map(Path::to_path_buf));
        }
        if let Ok(exe) = std::env::current_exe() {
            candidates.extend(exe.ancestors().skip(1).map(Path::to_path_buf));
        }
        candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));

        for candidate in candidates {
            if candidate.join("config").is_dir() && candidate.join("data").is_dir() {
                return Ok(Self::from_root(&candidate));
            }
        }
        bail!(
            "could not find the game directory (one containing 'config' and 'data'); \
             set {ROOT_ENV} to point at it"
        )
    }
}
