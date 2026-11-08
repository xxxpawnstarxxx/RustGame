//! Typed simulation errors.
//!
//! Library code returns [`SimError`]; the application layer wraps it in
//! `anyhow` for top-level reporting.

use crate::core::ids::{EntityId, TimerId};
use std::path::PathBuf;
use thiserror::Error;

/// Result alias used throughout the simulation.
pub type SimResult<T> = Result<T, SimError>;

/// Every recoverable failure the simulation can report.
#[derive(Debug, Error)]
pub enum SimError {
    /// A file could not be read or written.
    #[error("I/O error at '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// A JSON file could not be parsed.
    #[error("failed to parse '{path}': {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    /// Configuration values are inconsistent or out of range.
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    /// A content definition is malformed or references something missing.
    #[error("invalid content in {context}: {message}")]
    InvalidContent { context: String, message: String },

    /// Content validation found one or more problems (all are reported).
    #[error("content validation failed with {} problem(s):\n  {}", .0.len(), .0.join("\n  "))]
    ContentValidation(Vec<String>),

    /// World generation could not satisfy its settings.
    #[error("world generation failed: {0}")]
    WorldGeneration(String),

    /// A definition id was referenced but no such definition is loaded.
    #[error("unknown {category} definition '{id}'")]
    UnknownDefinition { category: &'static str, id: String },

    /// The entity does not exist (never existed, or was destroyed).
    #[error("entity {0} does not exist")]
    NoSuchEntity(EntityId),

    /// The timer does not exist (never scheduled, already fired, or cancelled).
    #[error("timer {0} does not exist")]
    NoSuchTimer(TimerId),

    /// A time value is not valid for the configured calendar.
    #[error("invalid time: {0}")]
    InvalidTime(String),

    /// A command could not be applied.
    #[error("invalid command: {0}")]
    InvalidCommand(String),

    /// A system could not be registered or scheduled.
    #[error("system registration failed: {0}")]
    InvalidSystem(String),

    /// A save file is incompatible or corrupt.
    #[error("persistence error: {0}")]
    Persistence(String),
}

impl SimError {
    /// Convenience constructor for configuration errors.
    pub fn config(message: impl Into<String>) -> Self {
        Self::InvalidConfig(message.into())
    }

    /// Convenience constructor for content errors.
    pub fn content(context: impl Into<String>, message: impl Into<String>) -> Self {
        Self::InvalidContent {
            context: context.into(),
            message: message.into(),
        }
    }
}
