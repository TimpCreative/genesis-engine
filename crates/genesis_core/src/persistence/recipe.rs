//! Recipe serialization (`world.toml`).

use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::parameters::WorldParameters;
use crate::persistence::error::PersistenceError;

/// Top-level world recipe file: metadata plus parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldRecipe {
    pub meta: SaveMeta,
    pub parameters: WorldParameters,
}

/// Save-file metadata (engine version, format version, creation time).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SaveMeta {
    pub genesis_engine_version: String,
    pub save_format_version: u32,
    pub created_at: DateTime<Utc>,
}

impl SaveMeta {
    pub const CURRENT_SAVE_FORMAT_VERSION: u32 = 1;

    pub fn current() -> Self {
        Self {
            genesis_engine_version: env!("CARGO_PKG_VERSION").to_string(),
            save_format_version: Self::CURRENT_SAVE_FORMAT_VERSION,
            created_at: Utc::now(),
        }
    }
}

pub fn write_world_toml(path: &Path, parameters: &WorldParameters) -> Result<(), PersistenceError> {
    let recipe = WorldRecipe {
        meta: SaveMeta::current(),
        parameters: parameters.clone(),
    };
    let text = toml::to_string_pretty(&recipe)?;
    fs::write(path, text)?;
    Ok(())
}

pub fn read_world_toml(path: &Path) -> Result<(SaveMeta, WorldParameters), PersistenceError> {
    let text = fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            PersistenceError::missing_file(path)
        } else {
            PersistenceError::Io(e)
        }
    })?;
    let recipe: WorldRecipe = toml::from_str(&text)?;
    if recipe.meta.save_format_version != SaveMeta::CURRENT_SAVE_FORMAT_VERSION {
        return Err(PersistenceError::UnsupportedFormatVersion {
            found: recipe.meta.save_format_version,
            supported: SaveMeta::CURRENT_SAVE_FORMAT_VERSION,
        });
    }
    recipe.parameters.validate()?;
    Ok((recipe.meta, recipe.parameters))
}
