//! Snapshot cache stubs (binary snapshots deferred to a later phase).

use std::fs;
use std::path::{Path, PathBuf};

use crate::branches::BranchId;
use crate::persistence::error::PersistenceError;
use crate::time::WorldYear;

/// Path to the per-branch snapshot cache directory.
pub fn snapshots_dir(branch_dir: &Path) -> PathBuf {
    branch_dir.join("snapshots")
}

/// A snapshot entry at a specific year (payload stubbed).
#[allow(dead_code)]
pub(crate) struct SnapshotEntry {
    pub year: WorldYear,
    // TODO(future): snapshot data
}

/// Stub snapshot I/O until real snapshot format is implemented.
#[allow(dead_code)]
pub struct SnapshotStub;

#[allow(dead_code)]
impl SnapshotStub {
    /// Stub: writes nothing. Real implementation in future phase.
    pub fn save(_dir: &Path, _branch: BranchId, _year: WorldYear) -> Result<(), PersistenceError> {
        Ok(())
    }

    /// Stub: returns empty Vec. Real implementation in future phase.
    pub fn load_all(
        _dir: &Path,
        _branch: BranchId,
    ) -> Result<Vec<SnapshotEntry>, PersistenceError> {
        Ok(Vec::new())
    }
}

pub fn ensure_snapshots_dir(branch_dir: &Path) -> Result<(), PersistenceError> {
    fs::create_dir_all(snapshots_dir(branch_dir))?;
    Ok(())
}
