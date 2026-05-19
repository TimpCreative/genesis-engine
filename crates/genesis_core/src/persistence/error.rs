//! Persistence I/O and validation errors.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::branches::BranchError;
use crate::lifecycle::CreateWorldError;
use crate::parameters::ParameterValidationError;

/// Errors from save/load operations.
#[derive(Error, Debug)]
pub enum PersistenceError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("path is not a directory: {0}")]
    NotADirectory(PathBuf),

    #[error("missing file: {0}")]
    MissingFile(PathBuf),

    #[error("TOML parse error: {0}")]
    TomlDeserialize(#[from] toml::de::Error),

    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("JSON parse error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("unsupported save format version: {found} (this version supports {supported})")]
    UnsupportedFormatVersion { found: u32, supported: u32 },

    #[error("loaded parameters failed validation: {0}")]
    InvalidParameters(#[from] ParameterValidationError),

    #[error("branch tree error during load: {0}")]
    BranchTreeError(#[from] BranchError),

    #[error("world creation failed during load: {0}")]
    CreateWorld(#[from] CreateWorldError),
}

impl PersistenceError {
    pub fn missing_file(path: impl AsRef<Path>) -> Self {
        Self::MissingFile(path.as_ref().to_path_buf())
    }
}
