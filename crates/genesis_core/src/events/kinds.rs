//! Event payload variants.

use serde::{Deserialize, Serialize};

use crate::data::{HotSpotId, PlateId};
use crate::grid::HexId;

/// Serializable boundary classification for events (Doc 06 §6.1).
///
/// Maps from `genesis_tectonics::boundary::BoundaryClass` in the tectonics crate.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub enum BoundaryType {
    Divergent,
    Transform,
    ConvergentContinentalContinental,
    ConvergentOceanicOceanic,
    ConvergentContinentalOceanic,
}

/// Plate reorganization action payload (Doc 06 §4.5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PlateReorgAction {
    Split {
        parent: PlateId,
        child: PlateId,
    },
    Merge {
        absorbed: PlateId,
        into: PlateId,
    },
    MotionChange {
        plate: PlateId,
        new_axis: [f64; 3],
        new_rate: f64,
    },
}

/// Event payload variants for simulation chronicle records.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum EventKind {
    /// Used only in tests until simulation modules add real variants.
    Placeholder { description: String },
    /// World created at Formation (Doc 06 §4.3).
    WorldFormation,
    /// Plate split, merge, or motion change (Doc 06 §4.5).
    PlateReorganization {
        action: PlateReorgAction,
        affected_plates: Vec<PlateId>,
    },
    /// Continental collision mountain building (Doc 06 §6.1).
    MountainRangeFormed {
        boundary_hexes: Vec<HexId>,
        plates: (PlateId, PlateId),
        peak_elevation_m: f32,
    },
    /// New or widening ocean basin at divergent boundary (Doc 06 §6.1).
    OceanBasinOpened {
        boundary_hexes: Vec<HexId>,
        plates: (PlateId, PlateId),
    },
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
    /// Boundary class changed on a directed edge (Doc 06 §6.1).
    BoundaryTransition {
        hex: HexId,
        from: BoundaryType,
        to: BoundaryType,
    },
    /// Global sea level adjustment (Doc 06 §4.6).
    SeaLevelChange { delta_m: f32, new_sea_level_m: f32 },
}
