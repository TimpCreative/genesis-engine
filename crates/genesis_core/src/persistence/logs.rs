//! Per-branch JSONL log files.

use std::fs;
use std::path::{Path, PathBuf};

use crate::branches::{Branch, BranchId};
use crate::events::EventLog;
use crate::interventions::InterventionLog;
use crate::persistence::error::PersistenceError;
use crate::persistence::tree::BranchMetadata;

pub fn branch_dir(root: &Path, id: BranchId) -> PathBuf {
    root.join("branches").join(id.0.to_string())
}

pub fn events_path(branch_dir: &Path) -> PathBuf {
    branch_dir.join("events.jsonl")
}

pub fn interventions_path(branch_dir: &Path) -> PathBuf {
    branch_dir.join("interventions.jsonl")
}

pub fn write_branch_logs(branch_dir: &Path, branch: &Branch) -> Result<(), PersistenceError> {
    let events = branch.event_log.to_jsonl()?;
    let interventions = branch.intervention_log.to_jsonl()?;
    fs::write(events_path(branch_dir), events)?;
    fs::write(interventions_path(branch_dir), interventions)?;
    Ok(())
}

pub fn read_branch_logs(
    branch_dir: &Path,
    meta: &BranchMetadata,
) -> Result<(EventLog, InterventionLog), PersistenceError> {
    let events_path = events_path(branch_dir);
    let interventions_path = interventions_path(branch_dir);

    let events_text = fs::read_to_string(&events_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            PersistenceError::missing_file(&events_path)
        } else {
            PersistenceError::Io(e)
        }
    })?;
    let interventions_text = fs::read_to_string(&interventions_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            PersistenceError::missing_file(&interventions_path)
        } else {
            PersistenceError::Io(e)
        }
    })?;

    let event_log = EventLog::from_jsonl(&events_text, meta.parent, meta.divergence_year)?;
    let intervention_log = InterventionLog::from_jsonl(&interventions_text)?;
    Ok((event_log, intervention_log))
}
