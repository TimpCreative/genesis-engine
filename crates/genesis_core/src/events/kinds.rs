//! Event payload variants.

use serde::{Deserialize, Serialize};

use crate::data::{HotSpotId, PlateId, WaterBodyId};
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
    /// Surface temperature crossed a cooling threshold during Formation (Doc 07 §15).
    PlanetaryCoolingMilestone { surface_temp_c: f32 },
    /// Condensation produced the first standing water; oceans start forming
    /// (Doc 08 §13; emitted by hydrology since Doc 08 superseded Doc 07 §3.5).
    OceansBeginForming { sea_level_m: f32 },
    /// Condensation ended; the inventory is fully surface-water (Doc 08 §13).
    OceansStabilized { sea_level_m: f32 },
    /// Formation sequence complete (Doc 07 §15).
    FormationComplete {
        final_temperature_c: f32,
        final_co2_ppm: f32,
    },
    /// The world entered a full glacial (Doc 07 §12.4).
    GlaciationBegan { global_temperature_c: f32 },
    /// A glacial ended and an interglacial returned (Doc 07 §12.4).
    GlaciationEnded { duration_years: i64 },
    /// An endorheic salt lake first appears (Doc 08 §13).
    SaltLakeFormed { hex: HexId, salinity: f32 },
    /// A salt flat first appears (Doc 08 §13).
    SaltFlatFormed { region: Vec<HexId> },
    /// A fjord is first flagged after glacial retreat (Doc 08 §13).
    FjordsCarved { region: Vec<HexId> },
    /// Sea-level milestone crossed (Doc 08 §13).
    SeaLevelMilestone { level_m: f32, delta_m: f32 },
    /// A lake body appears in the registry (Doc 08 §13).
    LakeFormed { body: WaterBodyId },
    /// A lake body disappears from the registry (Doc 08 §13).
    LakeDried { body: WaterBodyId },
    /// An inland sea loses ocean connectivity (Doc 08 §13).
    InlandSeaIsolated { body: WaterBodyId },
    /// An inland sea reconnects to the ocean (Doc 08 §13).
    InlandSeaReconnected { body: WaterBodyId },
    /// A Major river path shifts by many hexes (Doc 08 §13).
    RiverCourseShifted { region: Vec<HexId> },
    /// Ice volume peaks (Doc 08 §13).
    GlacialMaximum { sea_level_drop_m: f32 },
    /// An oasis flag appears (Doc 08 §13).
    OasisFormed { hex: HexId },
    /// A karst spring exceeds the discharge threshold (Doc 08 §13).
    GreatSpringEmerges { hex: HexId },
}
