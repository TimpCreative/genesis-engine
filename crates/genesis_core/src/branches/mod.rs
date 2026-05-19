//! Branch tree: alternate timelines forked by interventions.

use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::events::{Event, EventLog};
use crate::interventions::{Intervention, InterventionLog};
use crate::time::WorldYear;

/// Identifier for a simulation branch (timeline).
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub struct BranchId(pub u32);

impl BranchId {
    /// The root branch always has ID 0.
    pub const ROOT: BranchId = BranchId(0);
}

/// A single branch in the world's timeline tree.
#[derive(Clone, Debug)]
pub struct Branch {
    pub id: BranchId,
    pub parent: Option<BranchId>,
    pub divergence_year: WorldYear,
    pub name: String,
    /// Wall-clock time when this branch was created. Metadata only; does not
    /// affect simulation determinism.
    pub created_at_real_time: DateTime<Utc>,
    pub intervention_log: InterventionLog,
    pub event_log: EventLog,
}

/// All branches in a world, keyed by [`BranchId`].
#[derive(Clone, Debug)]
pub struct BranchTree {
    branches: BTreeMap<BranchId, Branch>,
    next_id: u32,
}

impl BranchTree {
    /// Constructs a new tree with just a root branch.
    pub fn new() -> Self {
        let root = Branch {
            id: BranchId::ROOT,
            parent: None,
            divergence_year: WorldYear::FORMATION,
            name: "Root".to_string(),
            created_at_real_time: Utc::now(),
            intervention_log: InterventionLog::new(),
            event_log: EventLog::new(None, WorldYear::FORMATION),
        };
        let mut branches = BTreeMap::new();
        branches.insert(BranchId::ROOT, root);
        Self {
            branches,
            next_id: 1,
        }
    }

    pub fn root(&self) -> &Branch {
        self.branches
            .get(&BranchId::ROOT)
            .expect("root branch always exists")
    }

    pub fn root_id(&self) -> BranchId {
        BranchId::ROOT
    }

    pub fn get(&self, id: BranchId) -> Option<&Branch> {
        self.branches.get(&id)
    }

    pub fn get_mut(&mut self, id: BranchId) -> Option<&mut Branch> {
        self.branches.get_mut(&id)
    }

    /// Returns immediate children of `parent`, in [`BranchId`] order.
    pub fn children_of(&self, parent: BranchId) -> Vec<BranchId> {
        let mut children: Vec<BranchId> = self
            .branches
            .values()
            .filter(|b| b.parent == Some(parent))
            .map(|b| b.id)
            .collect();
        children.sort();
        children
    }

    /// Returns all ancestors of `branch` (parents up to root), starting with
    /// the immediate parent and ending at the root.
    pub fn ancestors_of(&self, branch: BranchId) -> Vec<BranchId> {
        let mut ancestors = Vec::new();
        let mut current = branch;
        while let Some(b) = self.branches.get(&current) {
            if let Some(parent_id) = b.parent {
                ancestors.push(parent_id);
                current = parent_id;
            } else {
                break;
            }
        }
        ancestors
    }

    /// Creates a new branch forking from `parent` at `divergence_year`.
    /// Returns the new [`BranchId`]. Errors if `parent` doesn't exist.
    pub fn create_branch(
        &mut self,
        parent: BranchId,
        divergence_year: WorldYear,
        name: String,
    ) -> Result<BranchId, BranchError> {
        if !self.branches.contains_key(&parent) {
            return Err(BranchError::UnknownBranch(parent));
        }
        let id = BranchId(self.next_id);
        self.next_id += 1;
        let branch = Branch {
            id,
            parent: Some(parent),
            divergence_year,
            name,
            created_at_real_time: Utc::now(),
            intervention_log: InterventionLog::new(),
            event_log: EventLog::new(Some(parent), divergence_year),
        };
        self.branches.insert(id, branch);
        Ok(id)
    }

    pub fn all_branches(&self) -> impl Iterator<Item = &Branch> {
        self.branches.values()
    }

    pub fn count(&self) -> usize {
        self.branches.len()
    }

    /// Reconstructs a branch tree from a list of loaded branches.
    /// Used by persistence; not for normal branching operations.
    ///
    /// Errors if branches reference unknown parents or if the ROOT branch is missing.
    pub fn from_loaded_branches(branches: Vec<Branch>) -> Result<Self, BranchError> {
        let ids: HashSet<BranchId> = branches.iter().map(|b| b.id).collect();
        if !ids.contains(&BranchId::ROOT) {
            return Err(BranchError::MissingRoot);
        }
        for branch in &branches {
            if let Some(parent) = branch.parent
                && !ids.contains(&parent)
            {
                return Err(BranchError::InvalidParentReference {
                    child: branch.id,
                    parent,
                });
            }
        }
        let next_id = branches
            .iter()
            .map(|b| b.id.0)
            .max()
            .map(|max| max + 1)
            .unwrap_or(1);
        let branches: BTreeMap<BranchId, Branch> =
            branches.into_iter().map(|b| (b.id, b)).collect();
        Ok(Self { branches, next_id })
    }

    /// Returns all events visible on `branch` at or before `year`, walking up
    /// the parent chain. Each parent's events are included only up to the
    /// child's divergence point.
    pub fn events_visible_on(&self, branch: BranchId, year: WorldYear) -> Vec<&Event> {
        let mut collected = Vec::new();
        let mut current = branch;
        let mut cap = year;
        while let Some(b) = self.branches.get(&current) {
            collected.extend(b.event_log.iter_up_to(cap));
            if let Some(parent_id) = b.parent {
                cap = b.divergence_year;
                current = parent_id;
            } else {
                break;
            }
        }
        collected.sort_by_key(|a| (a.year, a.id));
        collected
    }

    /// Same as [`events_visible_on`](Self::events_visible_on) but for interventions.
    pub fn interventions_visible_on(
        &self,
        branch: BranchId,
        year: WorldYear,
    ) -> Vec<&Intervention> {
        let mut collected = Vec::new();
        let mut current = branch;
        let mut cap = year;
        while let Some(b) = self.branches.get(&current) {
            collected.extend(b.intervention_log.iter_up_to(cap));
            if let Some(parent_id) = b.parent {
                cap = b.divergence_year;
                current = parent_id;
            } else {
                break;
            }
        }
        collected.sort_by_key(|a| (a.year, a.id));
        collected
    }
}

impl Default for BranchTree {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors from branch tree operations.
#[derive(Error, Debug)]
pub enum BranchError {
    #[error("branch {0:?} does not exist")]
    UnknownBranch(BranchId),

    #[error("loaded branch tree missing required ROOT branch")]
    MissingRoot,

    #[error("loaded branch {child:?} references unknown parent {parent:?}")]
    InvalidParentReference { child: BranchId, parent: BranchId },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{Event, EventId, EventKind, EventLocation, Significance};
    use crate::interventions::{
        Intervention, InterventionAction, InterventionId, InterventionScope,
    };
    use chrono::Utc;

    fn test_event(id: u64, year: i64, branch: BranchId, significance: Significance) -> Event {
        Event {
            id: EventId(id),
            year: WorldYear(year),
            branch_id: branch,
            location: EventLocation::None,
            significance,
            kind: EventKind::Placeholder {
                description: format!("event {id}"),
            },
        }
    }

    fn test_intervention(id: u64, year: i64, branch: BranchId) -> Intervention {
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

    #[test]
    fn new_creates_single_root() {
        let tree = BranchTree::new();
        assert_eq!(tree.count(), 1);
        assert_eq!(tree.root_id(), BranchId::ROOT);
        assert_eq!(tree.root().id, BranchId::ROOT);
        assert_eq!(tree.root().parent, None);
        assert!(tree.root().intervention_log.is_empty());
        assert!(tree.root().event_log.is_empty());
    }

    #[test]
    fn create_branch_sets_parent() {
        let mut tree = BranchTree::new();
        let child = tree
            .create_branch(BranchId::ROOT, WorldYear(200), "Alt".to_string())
            .unwrap();
        assert_eq!(child, BranchId(1));
        assert_eq!(tree.get(child).unwrap().parent, Some(BranchId::ROOT));
        assert_eq!(tree.get(child).unwrap().divergence_year, WorldYear(200));
    }

    #[test]
    fn create_branch_unknown_parent_errors() {
        let mut tree = BranchTree::new();
        let err = tree
            .create_branch(BranchId(99), WorldYear(100), "X".to_string())
            .unwrap_err();
        assert!(matches!(err, BranchError::UnknownBranch(BranchId(99))));
    }

    #[test]
    fn children_of_returns_created_children() {
        let mut tree = BranchTree::new();
        let c1 = tree
            .create_branch(BranchId::ROOT, WorldYear(100), "A".to_string())
            .unwrap();
        let c2 = tree
            .create_branch(BranchId::ROOT, WorldYear(200), "B".to_string())
            .unwrap();
        assert_eq!(tree.children_of(BranchId::ROOT), vec![c1, c2]);
    }

    #[test]
    fn ancestors_of_child_returns_parent_chain() {
        let mut tree = BranchTree::new();
        let child = tree
            .create_branch(BranchId::ROOT, WorldYear(100), "Child".to_string())
            .unwrap();
        assert_eq!(tree.ancestors_of(child), vec![BranchId::ROOT]);
    }

    #[test]
    fn ancestors_of_grandchild_returns_full_chain() {
        let mut tree = BranchTree::new();
        let child = tree
            .create_branch(BranchId::ROOT, WorldYear(100), "Child".to_string())
            .unwrap();
        let grandchild = tree
            .create_branch(child, WorldYear(150), "Grandchild".to_string())
            .unwrap();
        assert_eq!(tree.ancestors_of(grandchild), vec![child, BranchId::ROOT]);
    }

    #[test]
    fn events_visible_on_respects_divergence() {
        let mut tree = BranchTree::new();
        tree.get_mut(BranchId::ROOT)
            .unwrap()
            .event_log
            .push(test_event(1, 100, BranchId::ROOT, Significance::Minor));
        tree.get_mut(BranchId::ROOT)
            .unwrap()
            .event_log
            .push(test_event(2, 200, BranchId::ROOT, Significance::Minor));
        tree.get_mut(BranchId::ROOT)
            .unwrap()
            .event_log
            .push(test_event(3, 300, BranchId::ROOT, Significance::Minor));

        let child = tree
            .create_branch(BranchId::ROOT, WorldYear(200), "Alt".to_string())
            .unwrap();
        tree.get_mut(child)
            .unwrap()
            .event_log
            .push(test_event(4, 250, child, Significance::Minor));

        let visible = tree.events_visible_on(child, WorldYear(300));
        let years: Vec<i64> = visible.iter().map(|e| e.year.value()).collect();
        assert_eq!(years, vec![100, 200, 250]);
    }

    #[test]
    fn events_visible_on_chronological_order() {
        let mut tree = BranchTree::new();
        tree.get_mut(BranchId::ROOT)
            .unwrap()
            .event_log
            .push(test_event(3, 300, BranchId::ROOT, Significance::Minor));
        tree.get_mut(BranchId::ROOT)
            .unwrap()
            .event_log
            .push(test_event(1, 100, BranchId::ROOT, Significance::Minor));

        let child = tree
            .create_branch(BranchId::ROOT, WorldYear(500), "Alt".to_string())
            .unwrap();
        tree.get_mut(child)
            .unwrap()
            .event_log
            .push(test_event(2, 200, child, Significance::Minor));

        let visible = tree.events_visible_on(child, WorldYear(500));
        let ids: Vec<u64> = visible.iter().map(|e| e.id.0).collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn interventions_visible_on_respects_divergence() {
        let mut tree = BranchTree::new();
        tree.get_mut(BranchId::ROOT)
            .unwrap()
            .intervention_log
            .push(test_intervention(1, 100, BranchId::ROOT));
        tree.get_mut(BranchId::ROOT)
            .unwrap()
            .intervention_log
            .push(test_intervention(2, 200, BranchId::ROOT));
        tree.get_mut(BranchId::ROOT)
            .unwrap()
            .intervention_log
            .push(test_intervention(3, 300, BranchId::ROOT));

        let child = tree
            .create_branch(BranchId::ROOT, WorldYear(200), "Alt".to_string())
            .unwrap();
        tree.get_mut(child)
            .unwrap()
            .intervention_log
            .push(test_intervention(4, 250, child));

        let visible = tree.interventions_visible_on(child, WorldYear(300));
        let years: Vec<i64> = visible.iter().map(|i| i.year.value()).collect();
        assert_eq!(years, vec![100, 200, 250]);
    }

    fn loaded_branch(id: u32, parent: Option<BranchId>, name: &str) -> Branch {
        Branch {
            id: BranchId(id),
            parent,
            divergence_year: WorldYear(100),
            name: name.to_string(),
            created_at_real_time: Utc::now(),
            intervention_log: InterventionLog::new(),
            event_log: EventLog::new(parent, WorldYear(100)),
        }
    }

    #[test]
    fn from_loaded_branches_rejects_missing_root() {
        let branches = vec![loaded_branch(1, Some(BranchId(99)), "orphan")];
        let err = BranchTree::from_loaded_branches(branches).unwrap_err();
        assert!(matches!(err, BranchError::MissingRoot));
    }

    #[test]
    fn from_loaded_branches_rejects_invalid_parent() {
        let branches = vec![
            loaded_branch(0, None, "Root"),
            loaded_branch(1, Some(BranchId(99)), "Child"),
        ];
        let err = BranchTree::from_loaded_branches(branches).unwrap_err();
        assert!(matches!(
            err,
            BranchError::InvalidParentReference {
                child: BranchId(1),
                parent: BranchId(99),
            }
        ));
    }

    #[test]
    fn from_loaded_branches_sets_next_id() {
        let branches = vec![
            loaded_branch(0, None, "Root"),
            loaded_branch(5, Some(BranchId::ROOT), "High"),
        ];
        let mut tree = BranchTree::from_loaded_branches(branches).unwrap();
        let next = tree
            .create_branch(BranchId::ROOT, WorldYear(200), "New".to_string())
            .unwrap();
        assert_eq!(next, BranchId(6));
    }
}
