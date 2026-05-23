//! World bulk data containers for Genesis Engine.
//!
//! [`WorldData`] holds per-hex Struct-of-Arrays fields keyed by [`HexId`](crate::HexId),
//! plus the [`HexGrid`](crate::HexGrid) they align with. Simulation modules (Phase 1+)
//! populate these arrays; Phase 0 initializes them to deterministic defaults.

mod enums;
mod ids;

pub use crate::grid::Direction;
pub use enums::BedrockType;
pub use ids::{BiomeId, HotSpotId, NationId, PlateId, SettlementId, SpeciesId};

use crate::HexGrid;
use crate::parameters::WorldParameters;
use serde::{Deserialize, Serialize};

pub use crate::time::WorldYear;

/// Records which plate owns a hex's surface features and where they originated in that
/// plate's local reference frame. Used to advect surface features when plates rotate.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlateOrigin {
    /// The plate whose surface features this hex carries.
    pub plate: PlateId,
    /// Quantized unit vector in plate-local coordinates. Use [`pack_plate_local`] /
    /// [`unpack_plate_local`] to convert to/from `f64`.
    pub plate_local_x: i16,
    pub plate_local_y: i16,
    pub plate_local_z: i16,
    /// When this feature was created or last significantly updated (tie-breaking).
    pub age_year: i64,
}

/// Scale factor for quantizing plate-local unit vectors to `i16`.
pub const PLATE_LOCAL_QUANTUM: f64 = 32000.0;

/// Packs a unit direction into quantized `i16` components for deterministic storage.
pub fn pack_plate_local(v: [f64; 3]) -> (i16, i16, i16) {
    let x = (v[0].clamp(-1.0, 1.0) * PLATE_LOCAL_QUANTUM).round() as i16;
    let y = (v[1].clamp(-1.0, 1.0) * PLATE_LOCAL_QUANTUM).round() as i16;
    let z = (v[2].clamp(-1.0, 1.0) * PLATE_LOCAL_QUANTUM).round() as i16;
    (x, y, z)
}

/// Unpacks quantized plate-local components to `f64` direction components.
pub fn unpack_plate_local(x: i16, y: i16, z: i16) -> [f64; 3] {
    [
        f64::from(x) / PLATE_LOCAL_QUANTUM,
        f64::from(y) / PLATE_LOCAL_QUANTUM,
        f64::from(z) / PLATE_LOCAL_QUANTUM,
    ]
}

/// Per-hex bulk arrays and global physical state for one world instance.
///
/// Engine-agnostic plain struct (not a Bevy resource). `genesis_app` wraps this as a
/// Bevy resource when the application binary is assembled.
pub struct WorldData {
    // ---- Infrastructure ----
    pub grid: HexGrid,
    /// Immutable recipe for this world. Set at construction; do not mutate in simulation code.
    pub parameters: WorldParameters,
    pub current_year: WorldYear,

    // ---- Physical Layer (Layer 0) ----
    /// Mean elevation in meters, signed (negative = below sea-level baseline).
    pub elevation_mean: Vec<f32>,
    /// Vertical range within the hex, in meters.
    pub elevation_relief: Vec<f32>,
    /// Bedrock composition.
    pub bedrock_type: Vec<BedrockType>,
    /// Tectonic plate assignment.
    pub plate_id: Vec<PlateId>,
    /// Plate-frame origin metadata per hex. `None` means no plate-borne feature; use
    /// background elevation for the current plate's type.
    pub plate_origin: Vec<Option<PlateOrigin>>,
    /// Mean annual temperature in degrees Celsius.
    pub temperature_mean: Vec<f32>,
    /// Annual temperature range (max − min) in degrees Celsius.
    pub temperature_range: Vec<f32>,
    /// Annual precipitation in mm/year.
    pub precipitation: Vec<f32>,
    /// Habitability score from 0.0 to 1.0.
    pub habitability: Vec<f32>,
    /// Primary flow direction toward a neighbor; `None` when there is no flow.
    pub flow_direction: Vec<Option<Direction>>,
    /// Water volume passing through the hex, in m³/year.
    pub flow_volume: Vec<f32>,
    /// Soil fertility from 0.0 to 1.0.
    pub soil_fertility: Vec<f32>,

    // ---- Global Physical State ----
    /// Global sea level in meters relative to baseline.
    pub sea_level_m: f32,
    /// Mean global surface temperature in degrees Celsius.
    pub global_temperature_c: f32,

    // ---- Biological Layer (Layer 1) ----
    /// Biome assignment per hex.
    pub biome: Vec<BiomeId>,
    /// Total biomass in tons per hex.
    pub biomass: Vec<f32>,
    /// Bio-deposit accumulator from shallow tropical seas. Monotonic; never decreases.
    /// Phase 1 tectonics increments this for hexes in shallow tropical conditions.
    /// Phase 4 biology will refine accumulation rate and drive bedrock transitions.
    pub fertility: Vec<f32>,

    // ---- Civilizational Layer (Layer 2) ----
    /// Population count per hex (most hexes are zero).
    pub population: Vec<u64>,
    /// Settlement in this hex, if any.
    pub settlement_id: Vec<Option<SettlementId>>,
    /// Nation controlling this hex, if any.
    pub nation_id: Vec<Option<NationId>>,
}

impl WorldData {
    /// Constructs a new [`WorldData`] backed by the given grid.
    ///
    /// All bulk arrays are sized to `grid.cell_count()` and filled with Phase 0 defaults.
    pub fn new(grid: HexGrid, parameters: WorldParameters) -> Self {
        let n = grid.cell_count() as usize;
        Self {
            grid,
            parameters: parameters.clone(),
            current_year: parameters.core.time.world_start_year,
            elevation_mean: vec![0.0; n],
            elevation_relief: vec![0.0; n],
            bedrock_type: vec![BedrockType::Unknown; n],
            plate_id: vec![PlateId::NONE; n],
            plate_origin: vec![None; n],
            temperature_mean: vec![15.0; n],
            temperature_range: vec![0.0; n],
            precipitation: vec![0.0; n],
            habitability: vec![0.0; n],
            flow_direction: vec![None; n],
            flow_volume: vec![0.0; n],
            soil_fertility: vec![0.0; n],
            sea_level_m: 0.0,
            global_temperature_c: 15.0,
            biome: vec![BiomeId::NONE; n],
            biomass: vec![0.0; n],
            fertility: vec![0.0; n],
            population: vec![0; n],
            settlement_id: vec![None; n],
            nation_id: vec![None; n],
        }
    }

    /// Returns the number of hexes in this world (matches `grid.cell_count()`).
    pub fn cell_count(&self) -> u32 {
        self.grid.cell_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HexGrid;
    use crate::parameters::WorldParameters;

    const EARTH_RADIUS_KM: f64 = 6371.0;

    fn world_at_level(level: u8) -> WorldData {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = level;
        let grid = HexGrid::new(level, EARTH_RADIUS_KM).expect("grid constructs");
        WorldData::new(grid, params)
    }

    #[test]
    fn new_populates_parameters() {
        let params = WorldParameters::default();
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid constructs");
        let world = WorldData::new(grid, params.clone());
        assert_eq!(world.parameters, params);
    }

    #[test]
    fn new_sets_current_year_from_parameters() {
        let mut params = WorldParameters::default();
        params.core.time.world_start_year = WorldYear(1000);
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid constructs");
        let world = WorldData::new(grid, params);
        assert_eq!(world.current_year, WorldYear(1000));
    }

    #[test]
    fn construction_succeeds_level_4() {
        let _ = world_at_level(4);
    }

    #[test]
    fn construction_succeeds_level_8() {
        let _ = world_at_level(8);
    }

    #[test]
    fn bulk_array_lengths_match_cell_count() {
        let world = world_at_level(4);
        let n = world.cell_count() as usize;
        assert_eq!(world.elevation_mean.len(), n);
        assert_eq!(world.elevation_relief.len(), n);
        assert_eq!(world.bedrock_type.len(), n);
        assert_eq!(world.plate_id.len(), n);
        assert_eq!(world.plate_origin.len(), n);
        assert_eq!(world.temperature_mean.len(), n);
        assert_eq!(world.temperature_range.len(), n);
        assert_eq!(world.precipitation.len(), n);
        assert_eq!(world.habitability.len(), n);
        assert_eq!(world.flow_direction.len(), n);
        assert_eq!(world.flow_volume.len(), n);
        assert_eq!(world.soil_fertility.len(), n);
        assert_eq!(world.biome.len(), n);
        assert_eq!(world.biomass.len(), n);
        assert_eq!(world.fertility.len(), n);
        assert_eq!(world.population.len(), n);
        assert_eq!(world.settlement_id.len(), n);
        assert_eq!(world.nation_id.len(), n);
    }

    #[test]
    fn default_values_level_4() {
        let world = world_at_level(4);
        assert!(
            world
                .bedrock_type
                .iter()
                .all(|&b| b == BedrockType::Unknown)
        );
        assert!(world.plate_id.iter().all(|&p| p == PlateId::NONE));
        assert!(world.biome.iter().all(|&b| b == BiomeId::NONE));
        assert!(world.temperature_mean.iter().all(|&t| t == 15.0));
        assert_eq!(world.global_temperature_c, 15.0);
        assert_eq!(world.sea_level_m, 0.0);
        assert!(world.population.iter().all(|&p| p == 0));
        assert!(world.flow_direction.iter().all(|d| d.is_none()));
        assert!(world.settlement_id.iter().all(|s| s.is_none()));
        assert!(world.nation_id.iter().all(|n| n.is_none()));
        assert!(world.fertility.iter().all(|&f| f == 0.0));
    }

    #[test]
    fn cell_count_matches_grid() {
        let world = world_at_level(4);
        assert_eq!(world.cell_count(), world.grid.cell_count());
    }

    #[test]
    fn bedrock_type_default_is_unknown() {
        assert_eq!(BedrockType::default(), BedrockType::Unknown);
    }

    #[test]
    fn sentinel_ids_are_max() {
        assert_eq!(PlateId::NONE.0, u16::MAX);
        assert_eq!(BiomeId::NONE.0, u16::MAX);
    }

    #[test]
    fn mutate_elevation_mean_smoke() {
        let mut world = world_at_level(4);
        world.elevation_mean[5] = 42.0;
        assert_eq!(world.elevation_mean[5], 42.0);
        assert_eq!(world.elevation_mean[0], 0.0);
        assert_eq!(world.elevation_mean[4], 0.0);
        assert_eq!(world.elevation_mean[6], 0.0);
    }

    #[test]
    fn flow_direction_round_trip() {
        let mut world = world_at_level(4);
        world.flow_direction[10] = Some(Direction::D2);
        assert_eq!(world.flow_direction[10], Some(Direction::D2));
    }

    #[test]
    fn plate_origin_defaults_none() {
        let world = world_at_level(4);
        assert!(world.plate_origin.iter().all(|o| o.is_none()));
    }

    #[test]
    fn pack_unpack_plate_local_round_trip() {
        let v = [0.1, -0.5, 0.866];
        let (x, y, z) = pack_plate_local(v);
        let out = unpack_plate_local(x, y, z);
        assert!((out[0] - v[0]).abs() < 0.001);
        assert!((out[1] - v[1]).abs() < 0.001);
        assert!((out[2] - v[2]).abs() < 0.001);
    }
}
