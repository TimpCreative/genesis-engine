//! Event payload variants.

use serde::{Deserialize, Serialize};

/// Event payload variants. Phase 0 establishes only the type and one
/// placeholder variant; later modules (tectonics, biology, civilization)
/// add their specific event kinds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EventKind {
    /// Used only in tests until simulation modules add real variants.
    Placeholder { description: String },
    // TODO(phase-1+): VolcanicEruption, SeaLevelChange, SpeciesEmergence,
    // SpeciesExtinction, SettlementFounded, NationFormed, Conflict,
    // TechnologyEmergence, ...
}
