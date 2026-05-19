//! Simulation event log: chronicle records for the user and export.

mod kinds;

pub use kinds::EventKind;

use serde::{Deserialize, Serialize};

use crate::branches::BranchId;
use crate::grid::HexId;
use crate::time::WorldYear;

/// Unique identifier for a simulation event.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub struct EventId(pub u64);

/// Where an event occurred in the world.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventLocation {
    Hex(HexId),
    Region(Vec<HexId>),
    Global,
    None,
}

/// How noteworthy an event is for chronicle views and filtering.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub enum Significance {
    Trace,
    Minor,
    Notable,
    Major,
    Pivotal,
}

/// A single recorded simulation event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: EventId,
    pub year: WorldYear,
    pub branch_id: BranchId,
    pub location: EventLocation,
    pub significance: Significance,
    pub kind: EventKind,
}

/// Per-branch event log (post-divergence events only on this branch).
#[derive(Clone, Debug, Default)]
pub struct EventLog {
    events: Vec<Event>,
    divergence_year: WorldYear,
    parent_branch: Option<BranchId>,
}

impl EventLog {
    pub fn new(parent_branch: Option<BranchId>, divergence_year: WorldYear) -> Self {
        Self {
            events: Vec::new(),
            divergence_year,
            parent_branch,
        }
    }

    pub fn push(&mut self, event: Event) {
        self.events.push(event);
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Event> {
        self.events.iter()
    }

    pub fn iter_up_to(&self, year: WorldYear) -> impl Iterator<Item = &Event> {
        self.events.iter().filter(move |e| e.year <= year)
    }

    pub fn iter_significant(&self, min: Significance) -> impl Iterator<Item = &Event> {
        self.events.iter().filter(move |e| e.significance >= min)
    }

    pub fn divergence_year(&self) -> WorldYear {
        self.divergence_year
    }

    pub fn parent_branch(&self) -> Option<BranchId> {
        self.parent_branch
    }

    /// Serializes events to JSONL (one Event JSON object per line).
    /// The log's divergence_year and parent_branch are NOT serialized here;
    /// they're persisted separately as branch metadata.
    pub fn to_jsonl(&self) -> Result<String, serde_json::Error> {
        let mut lines = String::new();
        for event in &self.events {
            let line = serde_json::to_string(event)?;
            if !lines.is_empty() {
                lines.push('\n');
            }
            lines.push_str(&line);
        }
        Ok(lines)
    }

    /// Parses a JSONL string back into events. Caller provides divergence
    /// metadata (which lives in branch metadata, not the event stream).
    pub fn from_jsonl(
        s: &str,
        parent_branch: Option<BranchId>,
        divergence_year: WorldYear,
    ) -> Result<Self, serde_json::Error> {
        let mut log = Self::new(parent_branch, divergence_year);
        for line in s.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let event: Event = serde_json::from_str(line)?;
            log.push(event);
        }
        Ok(log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::branches::BranchId;

    fn sample_event(id: u64, year: i64, significance: Significance) -> Event {
        Event {
            id: EventId(id),
            year: WorldYear(year),
            branch_id: BranchId::ROOT,
            location: EventLocation::Hex(HexId(42)),
            significance,
            kind: EventKind::Placeholder {
                description: "test".to_string(),
            },
        }
    }

    #[test]
    fn event_serializes_via_serde_json() {
        let event = sample_event(1, 1000, Significance::Major);
        let json = serde_json::to_string(&event).unwrap();
        let back: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(event, back);
    }

    #[test]
    fn push_and_iter_round_trip() {
        let mut log = EventLog::new(None, WorldYear::FORMATION);
        log.push(sample_event(1, 100, Significance::Minor));
        log.push(sample_event(2, 200, Significance::Major));
        assert_eq!(log.len(), 2);
        let ids: Vec<u64> = log.iter().map(|e| e.id.0).collect();
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn iter_up_to_filters_by_year() {
        let mut log = EventLog::new(None, WorldYear::FORMATION);
        log.push(sample_event(1, 100, Significance::Minor));
        log.push(sample_event(2, 200, Significance::Minor));
        log.push(sample_event(3, 300, Significance::Minor));
        let years: Vec<i64> = log
            .iter_up_to(WorldYear(200))
            .map(|e| e.year.value())
            .collect();
        assert_eq!(years, vec![100, 200]);
    }

    #[test]
    fn iter_significant_filters_by_threshold() {
        let mut log = EventLog::new(None, WorldYear::FORMATION);
        log.push(sample_event(1, 100, Significance::Trace));
        log.push(sample_event(2, 200, Significance::Minor));
        log.push(sample_event(3, 300, Significance::Notable));
        log.push(sample_event(4, 400, Significance::Major));
        log.push(sample_event(5, 500, Significance::Pivotal));

        let sigs: Vec<Significance> = log
            .iter_significant(Significance::Notable)
            .map(|e| e.significance)
            .collect();
        assert_eq!(
            sigs,
            vec![
                Significance::Notable,
                Significance::Major,
                Significance::Pivotal
            ]
        );
    }

    #[test]
    fn significance_ordering() {
        assert!(Significance::Trace < Significance::Minor);
        assert!(Significance::Minor < Significance::Notable);
        assert!(Significance::Notable < Significance::Major);
        assert!(Significance::Major < Significance::Pivotal);
    }

    #[test]
    fn jsonl_round_trip() {
        let mut log = EventLog::new(None, WorldYear::FORMATION);
        log.push(sample_event(1, 100, Significance::Minor));
        log.push(sample_event(2, 200, Significance::Major));
        log.push(sample_event(3, 300, Significance::Pivotal));

        let jsonl = log.to_jsonl().unwrap();
        let back = EventLog::from_jsonl(&jsonl, None, WorldYear::FORMATION).unwrap();
        assert_eq!(log.len(), back.len());
        for (a, b) in log.iter().zip(back.iter()) {
            assert_eq!(a, b);
        }
        assert_eq!(back.parent_branch(), None);
        assert_eq!(back.divergence_year(), WorldYear::FORMATION);
    }

    #[test]
    fn jsonl_each_line_is_valid_json() {
        let mut log = EventLog::new(None, WorldYear::FORMATION);
        log.push(sample_event(1, 100, Significance::Minor));
        log.push(sample_event(2, 200, Significance::Major));

        let jsonl = log.to_jsonl().unwrap();
        for line in jsonl.lines() {
            let _: serde_json::Value = serde_json::from_str(line).unwrap();
        }
    }
}
