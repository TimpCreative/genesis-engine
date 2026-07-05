//! Climate simulation state (Doc 07 §2.3).
//!
//! Held alongside tectonics state at the app layer. Not serialized with
//! [`WorldData`](genesis_core::data::WorldData); reconstructed from world data snapshots if needed.

use std::collections::BTreeMap;

use genesis_core::HexId;
use genesis_core::data::BasinId;
use genesis_core::events::Event;
use genesis_core::parameters::WorldParameters;

/// Per-hex climate regime label (Doc 07 §10).
///
/// Placeholder for P2-1. Filled out properly in P2-12 (regime classification).
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
#[derive(Default)]
pub enum ClimateRegime {
    #[default]
    Unset = 0,
    Tropical = 1,
    Subtropical = 2,
    HotDesert = 3,
    ColdDesert = 4,
    Mediterranean = 5,
    Temperate = 6,
    ContinentalCool = 7,
    Boreal = 8,
    Tundra = 9,
    Polar = 10,
}

/// Global atmospheric composition (Doc 07 §3.4, §11).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AtmosphericComposition {
    pub co2_ppm: f32,
    pub water_vapor_index: f32,
    pub oxygen_fraction: f32,
    pub greenhouse_forcing: f32,
}

impl Default for AtmosphericComposition {
    fn default() -> Self {
        // Earth pre-industrial baseline; overwritten on first formation tick.
        Self {
            co2_ppm: 280.0,
            water_vapor_index: 0.4,
            oxygen_fraction: 0.21,
            greenhouse_forcing: 0.0,
        }
    }
}

/// Planetary formation sub-phase (Doc 07 §3.2).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FormationSubPhase {
    /// Pre-formation initial state. Surface molten, atmosphere is dense steam.
    #[default]
    Molten,
    /// Surface cooling toward 100°C threshold.
    Cooling,
    /// Water vapor condensing; oceans forming.
    Condensation,
    /// Approaching modern equilibrium; final settling.
    Stabilization,
    /// Formation complete; Geological era can begin.
    Complete,
}

/// Year boundaries between formation sub-phases (Doc 07 §3.2).
/// All values in years since world start.
pub const MOLTEN_END_YEAR: i64 = 50_000_000;
pub const COOLING_END_YEAR: i64 = 200_000_000;
pub const CONDENSATION_END_YEAR: i64 = 350_000_000;
pub const STABILIZATION_END_YEAR: i64 = 500_000_000;

/// Initial molten surface temperature in °C (Doc 07 §3.3).
pub const T_INITIAL_MOLTEN_C: f32 = 2000.0;

/// Equilibrium target temperature in °C after Formation completes (Doc 07 §3.3).
pub const T_EQUILIBRIUM_C: f32 = 15.0;

/// Cooling time constant in years (Doc 07 §3.3). Tuned so most cooling
/// occurs over ~500M years.
pub const COOLING_TAU_YEARS: f64 = 80_000_000.0;

impl FormationSubPhase {
    /// Returns the sub-phase appropriate for the given year, assuming the
    /// default Formation timeline. Used at world reload to reconstruct state.
    pub fn for_year(year_value: i64) -> Self {
        if year_value < MOLTEN_END_YEAR {
            Self::Molten
        } else if year_value < COOLING_END_YEAR {
            Self::Cooling
        } else if year_value < CONDENSATION_END_YEAR {
            Self::Condensation
        } else if year_value < STABILIZATION_END_YEAR {
            Self::Stabilization
        } else {
            Self::Complete
        }
    }
}

/// Returns true when the climate formation period is active (Doc 07 §3).
///
/// Inclusive of `STABILIZATION_END_YEAR` so the coordinator schedules a final
/// formation tick at year 500M.
pub fn formation_period_active(year: i64, params: &WorldParameters) -> bool {
    !params.core.climate.skip_planetary_formation && year <= STABILIZATION_END_YEAR
}

/// A single circulation cell, spanning a latitude band in one hemisphere.
///
/// Per Doc 07 §6.3, cells alternate in circulation direction: cell 0 (equator-most)
/// is Hadley-like, cell 1 is Ferrel-like, cell 2 is polar-like, etc.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CirculationCell {
    /// Zero-indexed from equator outward (0 = equator-most cell, N-1 = polar cell).
    pub index: u8,
    /// Latitude (radians, absolute value) of the cell's equator-side boundary.
    pub lat_low_rad: f64,
    /// Latitude (radians, absolute value) of the cell's pole-side boundary.
    pub lat_high_rad: f64,
    /// Cell's circulation intensity (dimensionless, 0.1-2.0 range typical).
    /// Scales with pole-equator temperature gradient.
    pub intensity: f32,
}

/// Atmospheric circulation cells for the planet (Doc 07 §6).
///
/// Cells are symmetric: each cell description applies to both hemispheres. The
/// `cells_per_hemisphere` field is the count for one hemisphere; total cells
/// in the atmosphere is `2 * cells_per_hemisphere`.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CirculationCells {
    /// Number of cells per hemisphere. 1-6 range.
    pub cells_per_hemisphere: u8,
    /// Cell descriptions, ordered from equator outward.
    /// `cells.len() == cells_per_hemisphere`.
    pub cells: Vec<CirculationCell>,
    /// Pole-to-equator temperature gradient (°C) used to compute intensity this tick.
    pub equator_pole_temp_diff_c: f32,
}

impl CirculationCells {
    /// Returns the cell that contains the given latitude (radians, signed).
    /// Returns `None` if the cells haven't been computed yet (empty cells list).
    pub fn cell_for_latitude(&self, lat_rad: f64) -> Option<&CirculationCell> {
        let abs_lat = lat_rad.abs();
        self.cells
            .iter()
            .find(|c| abs_lat >= c.lat_low_rad && abs_lat <= c.lat_high_rad)
    }
}

/// Metadata for a single ocean basin (Doc 07 §8.1).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OceanBasin {
    /// Unique identifier within the world's basin set.
    pub id: BasinId,
    /// Hex closest to the basin's geographic centroid (used by P2-8 for gyre seed).
    pub centroid_hex: HexId,
    /// Number of hexes in this basin.
    pub hex_count: u32,
    /// Minimum (southernmost) latitude in radians.
    pub lat_min_rad: f64,
    /// Maximum (northernmost) latitude in radians.
    pub lat_max_rad: f64,
    /// True when this basin is an enclosed inland sea (not a marginal sea of the world ocean).
    pub is_inland: bool,
}

/// Set of ocean basins on the planet. Recomputed each climate tick.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OceanBasins {
    /// All identified basins, sorted by descending hex_count (largest = `BasinId(0)`).
    pub basins: Vec<OceanBasin>,
}

/// Glaciation state (Doc 07 §12.2).
#[derive(Copy, Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum GlaciationState {
    #[default]
    Interglacial,
    Transition,
    Glacial,
}

/// State held by [`ClimateLayer`](crate::layer::ClimateLayer) across ticks.
#[derive(Clone, Debug)]
pub struct ClimateState {
    /// Events queued for emission this tick (cleared on flush).
    pub pending_events: Vec<Event>,
    /// Monotonic event ID counter for this layer's events.
    pub next_event_id: u64,
    /// Current global atmospheric composition.
    pub atmospheric_composition: AtmosphericComposition,
    /// Cumulative orbital cycle phase (Milankovitch-like). Years since formation.
    pub cumulative_orbital_phase_rad: f64,
    /// Glaciation state.
    pub glaciation: GlaciationState,
    /// Previous regime per hex for regime-shift event emission (P2-12+).
    pub previous_regime: BTreeMap<HexId, ClimateRegime>,
    /// Current formation sub-phase (Doc 07 §3.2).
    pub formation_sub_phase: FormationSubPhase,
    /// True once Formation is complete and Geological era can begin.
    pub formation_complete: bool,
    /// Last temperature at which a cooling milestone was emitted.
    /// `INFINITY` until first tick (no emissions before then).
    pub last_cooling_milestone_temp_c: f32,
    /// Atmospheric circulation cell configuration. Recomputed each climate tick.
    pub circulation_cells: CirculationCells,
    /// True after the one-time circulation diagnostic has been logged to stderr.
    pub circulation_logged_once: bool,
    /// Ocean basin configuration. Recomputed each climate tick.
    pub ocean_basins: OceanBasins,
}

impl Default for ClimateState {
    fn default() -> Self {
        Self {
            pending_events: Vec::new(),
            next_event_id: 0,
            atmospheric_composition: AtmosphericComposition::default(),
            cumulative_orbital_phase_rad: 0.0,
            glaciation: GlaciationState::default(),
            previous_regime: BTreeMap::new(),
            formation_sub_phase: FormationSubPhase::Molten,
            formation_complete: false,
            last_cooling_milestone_temp_c: f32::INFINITY,
            circulation_cells: CirculationCells::default(),
            circulation_logged_once: false,
            ocean_basins: OceanBasins::default(),
        }
    }
}

impl ClimateState {
    pub fn new() -> Self {
        Self::default()
    }
}
