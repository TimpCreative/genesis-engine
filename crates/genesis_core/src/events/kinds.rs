//! Event payload variants.

use serde::{Deserialize, Serialize};

use crate::data::{HotSpotId, PlateId};
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
    /// Mantle hot spot eruption at the hex under the anchor (Doc 06 §7).
    HotSpotActivity {
        hex: HexId,
        hot_spot_id: HotSpotId,
        elevation_change_m: f32,
    },
}
