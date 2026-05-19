//! On-disk save format for Genesis Engine worlds (`.gen` directories).

mod error;
mod logs;
mod recipe;
mod snapshots;
mod tree;

pub use error::PersistenceError;
pub use recipe::{SaveMeta, WorldRecipe};
pub use tree::{BranchMetadata, BranchTreeFile};

use std::fs;
use std::path::Path;

use crate::branches::{Branch, BranchTree};
use crate::parameters::WorldParameters;

/// What [`load_world`] returns.
#[derive(Clone, Debug)]
pub struct LoadedWorld {
    pub meta: SaveMeta,
    pub parameters: WorldParameters,
    pub branch_tree: BranchTree,
}

/// Saves a complete world to a `.gen` directory.
///
/// Creates the directory if it doesn't exist. Overwrites existing contents.
/// The given path should have `.gen` suffix by convention (not enforced).
pub fn save_world(
    parameters: &WorldParameters,
    tree: &BranchTree,
    path: &Path,
) -> Result<(), PersistenceError> {
    fs::create_dir_all(path)?;

    recipe::write_world_toml(&path.join("world.toml"), parameters)?;
    tree::write_branch_tree_toml(&path.join("branch_tree.toml"), tree)?;

    let branches_root = path.join("branches");
    if branches_root.exists() {
        fs::remove_dir_all(&branches_root)?;
    }

    for branch in tree.all_branches() {
        let branch_dir = logs::branch_dir(path, branch.id);
        fs::create_dir_all(&branch_dir)?;
        logs::write_branch_logs(&branch_dir, branch)?;
        snapshots::ensure_snapshots_dir(&branch_dir)?;
    }

    Ok(())
}

/// Loads a complete world from a `.gen` directory.
///
/// Returns the parameters and branch tree. Snapshots are loaded but stubbed.
pub fn load_world(path: &Path) -> Result<LoadedWorld, PersistenceError> {
    if !path.is_dir() {
        return Err(PersistenceError::NotADirectory(path.to_path_buf()));
    }

    let (meta, parameters) = recipe::read_world_toml(&path.join("world.toml"))?;
    let tree_file = tree::read_branch_tree_toml(&path.join("branch_tree.toml"))?;

    let mut metadata = tree_file.branches;
    metadata.sort_by_key(|b| b.id);

    let mut branches = Vec::with_capacity(metadata.len());
    for meta in metadata {
        let branch_dir = logs::branch_dir(path, meta.id);
        let (event_log, intervention_log) = logs::read_branch_logs(&branch_dir, &meta)?;
        branches.push(Branch {
            id: meta.id,
            parent: meta.parent,
            divergence_year: meta.divergence_year,
            name: meta.name,
            created_at_real_time: meta.created_at,
            intervention_log,
            event_log,
        });
    }

    let branch_tree = BranchTree::from_loaded_branches(branches)?;

    Ok(LoadedWorld {
        meta,
        parameters,
        branch_tree,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::branches::{BranchError, BranchId};
    use crate::events::{Event, EventId, EventKind, EventLocation, Significance};
    use crate::interventions::{
        Intervention, InterventionAction, InterventionId, InterventionScope,
    };
    use crate::time::WorldYear;
    use chrono::Utc;
    use tempfile::tempdir;

    fn sample_event(id: u64, year: i64, branch: BranchId) -> Event {
        Event {
            id: EventId(id),
            year: WorldYear(year),
            branch_id: branch,
            location: EventLocation::None,
            significance: Significance::Minor,
            kind: EventKind::Placeholder {
                description: format!("event {id}"),
            },
        }
    }

    fn sample_intervention(id: u64, year: i64, branch: BranchId) -> Intervention {
        Intervention {
            id: InterventionId(id),
            year: WorldYear(year),
            branch_id: branch,
            scope: InterventionScope::Local,
            action: InterventionAction::Placeholder {
                description: format!("intervention {id}"),
            },
            created_at_real_time: Utc::now(),
        }
    }

    fn trees_equal(a: &BranchTree, b: &BranchTree) {
        assert_eq!(a.count(), b.count());
        for branch_a in a.all_branches() {
            let branch_b = b.get(branch_a.id).expect("branch id present");
            assert_eq!(branch_a.parent, branch_b.parent);
            assert_eq!(branch_a.divergence_year, branch_b.divergence_year);
            assert_eq!(branch_a.name, branch_b.name);
            assert_eq!(branch_a.event_log.len(), branch_b.event_log.len());
            for (ea, eb) in branch_a.event_log.iter().zip(branch_b.event_log.iter()) {
                assert_eq!(ea, eb);
            }
            assert_eq!(
                branch_a.intervention_log.len(),
                branch_b.intervention_log.len()
            );
            for (ia, ib) in branch_a
                .intervention_log
                .iter()
                .zip(branch_b.intervention_log.iter())
            {
                assert_eq!(ia, ib);
            }
        }
    }

    fn tree_with_branches_and_logs() -> BranchTree {
        let mut tree = BranchTree::new();
        tree.get_mut(BranchId::ROOT)
            .unwrap()
            .event_log
            .push(sample_event(1, 100, BranchId::ROOT));
        tree.get_mut(BranchId::ROOT)
            .unwrap()
            .intervention_log
            .push(sample_intervention(1, 50, BranchId::ROOT));

        let c1 = tree
            .create_branch(BranchId::ROOT, WorldYear(1_000), "Alt A".to_string())
            .unwrap();
        tree.get_mut(c1)
            .unwrap()
            .event_log
            .push(sample_event(2, 1100, c1));
        tree.get_mut(c1)
            .unwrap()
            .intervention_log
            .push(sample_intervention(2, 1050, c1));

        let c2 = tree
            .create_branch(BranchId::ROOT, WorldYear(2_000), "Alt B".to_string())
            .unwrap();
        tree.get_mut(c2)
            .unwrap()
            .event_log
            .push(sample_event(3, 2100, c2));

        let gc = tree
            .create_branch(c1, WorldYear(1_500), "Grandchild".to_string())
            .unwrap();
        tree.get_mut(gc)
            .unwrap()
            .event_log
            .push(sample_event(4, 1600, gc));
        tree.get_mut(gc)
            .unwrap()
            .intervention_log
            .push(sample_intervention(3, 1550, gc));

        tree
    }

    #[test]
    fn round_trip_empty_world() {
        let params = WorldParameters::default();
        let tree = BranchTree::new();
        let dir = tempdir().unwrap();
        save_world(&params, &tree, dir.path()).unwrap();
        let loaded = load_world(dir.path()).unwrap();
        assert_eq!(loaded.parameters, params);
        trees_equal(&tree, &loaded.branch_tree);
    }

    #[test]
    fn round_trip_multi_branch_with_logs() {
        let params = WorldParameters::default();
        let tree = tree_with_branches_and_logs();
        let dir = tempdir().unwrap();
        save_world(&params, &tree, dir.path()).unwrap();
        let loaded = load_world(dir.path()).unwrap();
        assert_eq!(loaded.parameters, params);
        trees_equal(&tree, &loaded.branch_tree);
    }

    #[test]
    fn save_creates_expected_directory_structure() {
        let params = WorldParameters::default();
        let tree = tree_with_branches_and_logs();
        let dir = tempdir().unwrap();
        let path = dir.path();
        save_world(&params, &tree, path).unwrap();

        assert!(path.join("world.toml").is_file());
        assert!(path.join("branch_tree.toml").is_file());

        for branch in tree.all_branches() {
            let branch_path = path.join("branches").join(branch.id.0.to_string());
            assert!(branch_path.join("events.jsonl").is_file());
            assert!(branch_path.join("interventions.jsonl").is_file());
            assert!(branch_path.join("snapshots").is_dir());
        }
    }

    #[test]
    fn save_overwrites_existing_directory() {
        let params = WorldParameters::default();
        let dir = tempdir().unwrap();
        let path = dir.path();

        let mut tree_v1 = BranchTree::new();
        tree_v1
            .get_mut(BranchId::ROOT)
            .unwrap()
            .event_log
            .push(sample_event(99, 999, BranchId::ROOT));
        save_world(&params, &tree_v1, path).unwrap();

        let tree_v2 = tree_with_branches_and_logs();
        save_world(&params, &tree_v2, path).unwrap();

        let loaded = load_world(path).unwrap();
        trees_equal(&tree_v2, &loaded.branch_tree);
        assert_eq!(loaded.branch_tree.count(), 4);
    }

    #[test]
    fn load_rejects_non_directory() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("not_a_dir");
        std::fs::write(&file_path, b"x").unwrap();
        let err = load_world(&file_path).unwrap_err();
        assert!(matches!(err, PersistenceError::NotADirectory(_)));
    }

    #[test]
    fn load_rejects_missing_world_toml() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path()).unwrap();
        let err = load_world(dir.path()).unwrap_err();
        assert!(matches!(err, PersistenceError::MissingFile(_)));
    }

    #[test]
    fn load_rejects_missing_branch_tree_toml() {
        let params = WorldParameters::default();
        let tree = BranchTree::new();
        let dir = tempdir().unwrap();
        let path = dir.path();
        save_world(&params, &tree, path).unwrap();
        fs::remove_file(path.join("branch_tree.toml")).unwrap();

        let err = load_world(path).unwrap_err();
        assert!(matches!(err, PersistenceError::MissingFile(_)));
    }

    #[test]
    fn load_rejects_unsupported_format_version() {
        let dir = tempdir().unwrap();
        let path = dir.path();
        fs::create_dir_all(path).unwrap();

        let recipe = WorldRecipe {
            meta: SaveMeta {
                genesis_engine_version: "0.1.0".to_string(),
                save_format_version: 999,
                created_at: Utc::now(),
            },
            parameters: WorldParameters::default(),
        };
        let text = toml::to_string_pretty(&recipe).unwrap();
        fs::write(path.join("world.toml"), text).unwrap();
        fs::write(path.join("branch_tree.toml"), "branches = []\n").unwrap();

        let err = load_world(path).unwrap_err();
        assert!(matches!(
            err,
            PersistenceError::UnsupportedFormatVersion {
                found: 999,
                supported: 1,
            }
        ));
    }

    #[test]
    fn load_rejects_invalid_parameters() {
        let params = WorldParameters::default();
        let tree = BranchTree::new();
        let dir = tempdir().unwrap();
        let path = dir.path();
        save_world(&params, &tree, path).unwrap();

        let world_path = path.join("world.toml");
        let mut text = fs::read_to_string(&world_path).unwrap();
        text = text.replace("radius_km = 6371.0", "radius_km = -1.0");
        fs::write(&world_path, text).unwrap();

        let err = load_world(path).unwrap_err();
        assert!(matches!(err, PersistenceError::InvalidParameters(_)));
    }

    #[test]
    fn empty_event_log_round_trips() {
        let params = WorldParameters::default();
        let tree = BranchTree::new();
        assert!(tree.root().event_log.is_empty());
        let dir = tempdir().unwrap();
        save_world(&params, &tree, dir.path()).unwrap();

        let events_path = dir.path().join("branches/0/events.jsonl");
        assert!(events_path.is_file());
        let content = fs::read_to_string(&events_path).unwrap();
        assert!(content.is_empty());

        let loaded = load_world(dir.path()).unwrap();
        assert!(loaded.branch_tree.root().event_log.is_empty());
    }

    #[test]
    fn next_id_restored_after_load() {
        use crate::branches::Branch;
        use crate::events::EventLog;
        use crate::interventions::InterventionLog;

        let branches = vec![
            Branch {
                id: BranchId::ROOT,
                parent: None,
                divergence_year: WorldYear::FORMATION,
                name: "Root".to_string(),
                created_at_real_time: Utc::now(),
                intervention_log: InterventionLog::new(),
                event_log: EventLog::new(None, WorldYear::FORMATION),
            },
            Branch {
                id: BranchId(5),
                parent: Some(BranchId::ROOT),
                divergence_year: WorldYear(100),
                name: "High".to_string(),
                created_at_real_time: Utc::now(),
                intervention_log: InterventionLog::new(),
                event_log: EventLog::new(Some(BranchId::ROOT), WorldYear(100)),
            },
        ];

        let params = WorldParameters::default();
        let tree = BranchTree::from_loaded_branches(branches).unwrap();
        let dir = tempdir().unwrap();
        save_world(&params, &tree, dir.path()).unwrap();

        let mut loaded = load_world(dir.path()).unwrap().branch_tree;
        let next = loaded
            .create_branch(BranchId::ROOT, WorldYear(200), "New".to_string())
            .unwrap();
        assert_eq!(next, BranchId(6));
    }

    #[test]
    fn meta_version_matches_crate_version_on_save() {
        let params = WorldParameters::default();
        let tree = BranchTree::new();
        let dir = tempdir().unwrap();
        save_world(&params, &tree, dir.path()).unwrap();
        let loaded = load_world(dir.path()).unwrap();
        assert_eq!(
            loaded.meta.genesis_engine_version,
            env!("CARGO_PKG_VERSION")
        );
    }

    #[test]
    fn from_loaded_branches_missing_root_via_persistence_types() {
        use crate::branches::BranchTree;

        let err = BranchTree::from_loaded_branches(vec![]).unwrap_err();
        assert!(matches!(err, BranchError::MissingRoot));
    }
}
