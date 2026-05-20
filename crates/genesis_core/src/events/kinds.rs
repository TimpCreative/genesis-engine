//! Event payload variants.

use serde::{Deserialize, Serialize};

use crate::data::PlateId;
use crate::grid::HexId;

/// Event payload variants. Phase 0 establishes only the type and one
/// placeholder variant; later modules (tectonics, biology, civilization)
/// add their specific event kinds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EventKind {
    /// Used only in tests until simulation modules add real variants.
    Placeholder { description: String },
    /// Boundary-driven volcanic eruption at a subduction arc hex (Doc 06 §5.5).
    VolcanicEruption {
        hex: HexId,
        elevation_change_m: f32,
        plate: PlateId,
    },
}
