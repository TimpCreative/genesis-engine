//! World bulk data containers for Genesis Engine.
//!
//! [`WorldData`] holds per-hex Struct-of-Arrays fields keyed by [`HexId`](crate::HexId),
//! plus the [`HexGrid`](crate::HexGrid) they align with. Simulation modules (Phase 1+)
//! populate these arrays; Phase 0 initializes them to deterministic defaults.

mod enums;
mod ids;

pub use crate::grid::Direction;
pub use enums::BedrockType;
pub use ids::{BiomeId, NationId, PlateId, SettlementId, SpeciesId};

use crate::HexGrid;
use crate::parameters::WorldParameters;

pub use crate::time::WorldYear;

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
}
