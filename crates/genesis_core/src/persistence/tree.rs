//! Branch tree metadata serialization (`branch_tree.toml`).

use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::branches::{Branch, BranchId, BranchTree};
use crate::persistence::error::PersistenceError;
use crate::time::WorldYear;

/// On-disk branch tree (metadata only; logs live in subdirectories).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BranchTreeFile {
    pub branches: Vec<BranchMetadata>,
}

/// Per-branch metadata persisted in `branch_tree.toml`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BranchMetadata {
    pub id: BranchId,
    pub parent: Option<BranchId>,
    pub divergence_year: WorldYear,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

pub fn branch_metadata_from(branch: &Branch) -> BranchMetadata {
    BranchMetadata {
        id: branch.id,
        parent: branch.parent,
        divergence_year: branch.divergence_year,
        name: branch.name.clone(),
        created_at: branch.created_at_real_time,
    }
}

pub fn write_branch_tree_toml(path: &Path, tree: &BranchTree) -> Result<(), PersistenceError> {
    let mut branches: Vec<BranchMetadata> = tree.all_branches().map(branch_metadata_from).collect();
    branches.sort_by_key(|b| b.id);
    let file = BranchTreeFile { branches };
    let text = toml::to_string_pretty(&file)?;
    fs::write(path, text)?;
    Ok(())
}

pub fn read_branch_tree_toml(path: &Path) -> Result<BranchTreeFile, PersistenceError> {
    let text = fs::read_to_string(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            PersistenceError::missing_file(path)
        } else {
            PersistenceError::Io(e)
        }
    })?;
    let file: BranchTreeFile = toml::from_str(&text)?;
    Ok(file)
}
