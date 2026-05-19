//! User intervention log: changes that fork branches.

mod actions;

pub use actions::InterventionAction;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::branches::BranchId;
use crate::time::WorldYear;

/// Unique identifier for a user intervention.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub struct InterventionId(pub u64);

/// Spatial extent of an intervention's effect.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub enum InterventionScope {
    Local,
    Regional,
    Global,
}

/// A single user intervention on a branch.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Intervention {
    pub id: InterventionId,
    pub year: WorldYear,
    pub branch_id: BranchId,
    pub scope: InterventionScope,
    pub action: InterventionAction,
    /// Wall-clock time when the user made this intervention. Metadata only;
    /// does not affect simulation determinism.
    pub created_at_real_time: DateTime<Utc>,
}

/// Per-branch intervention log.
#[derive(Clone, Debug, Default)]
pub struct InterventionLog {
    interventions: Vec<Intervention>,
}

impl InterventionLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, intervention: Intervention) {
        self.interventions.push(intervention);
    }

    pub fn len(&self) -> usize {
        self.interventions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.interventions.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Intervention> {
        self.interventions.iter()
    }

    pub fn iter_up_to(&self, year: WorldYear) -> impl Iterator<Item = &Intervention> {
        self.interventions.iter().filter(move |i| i.year <= year)
    }

    pub fn iter_in_range(
        &self,
        start: WorldYear,
        end: WorldYear,
    ) -> impl Iterator<Item = &Intervention> {
        self.interventions
            .iter()
            .filter(move |i| i.year >= start && i.year <= end)
    }

    pub fn to_jsonl(&self) -> Result<String, serde_json::Error> {
        let mut lines = String::new();
        for intervention in &self.interventions {
            let line = serde_json::to_string(intervention)?;
            if !lines.is_empty() {
                lines.push('\n');
            }
            lines.push_str(&line);
        }
        Ok(lines)
    }

    pub fn from_jsonl(s: &str) -> Result<Self, serde_json::Error> {
        let mut log = Self::new();
        for line in s.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let intervention: Intervention = serde_json::from_str(line)?;
            log.push(intervention);
        }
        Ok(log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_intervention(id: u64, year: i64) -> Intervention {
        Intervention {
            id: InterventionId(id),
            year: WorldYear(year),
            branch_id: BranchId::ROOT,
            scope: InterventionScope::Global,
            action: InterventionAction::Placeholder {
                description: "test".to_string(),
            },
            created_at_real_time: Utc::now(),
        }
    }

    #[test]
    fn intervention_serializes_including_datetime() {
        let intervention = sample_intervention(1, 500);
        let json = serde_json::to_string(&intervention).unwrap();
        let back: Intervention = serde_json::from_str(&json).unwrap();
        assert_eq!(intervention, back);
    }

    #[test]
    fn push_and_iter_round_trip() {
        let mut log = InterventionLog::new();
        log.push(sample_intervention(1, 100));
        log.push(sample_intervention(2, 200));
        assert_eq!(log.len(), 2);
        let ids: Vec<u64> = log.iter().map(|i| i.id.0).collect();
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn iter_in_range_filters_inclusively() {
        let mut log = InterventionLog::new();
        log.push(sample_intervention(1, 100));
        log.push(sample_intervention(2, 200));
        log.push(sample_intervention(3, 300));

        let years: Vec<i64> = log
            .iter_in_range(WorldYear(150), WorldYear(250))
            .map(|i| i.year.value())
            .collect();
        assert_eq!(years, vec![200]);
    }

    #[test]
    fn jsonl_round_trip() {
        let mut log = InterventionLog::new();
        log.push(sample_intervention(1, 100));
        log.push(sample_intervention(2, 200));

        let jsonl = log.to_jsonl().unwrap();
        let back = InterventionLog::from_jsonl(&jsonl).unwrap();
        assert_eq!(log.len(), back.len());
        for (a, b) in log.iter().zip(back.iter()) {
            assert_eq!(a, b);
        }
    }

    #[test]
    fn intervention_scope_serializes() {
        for scope in [
            InterventionScope::Local,
            InterventionScope::Regional,
            InterventionScope::Global,
        ] {
            let json = serde_json::to_string(&scope).unwrap();
            let back: InterventionScope = serde_json::from_str(&json).unwrap();
            assert_eq!(scope, back);
        }
    }
}
