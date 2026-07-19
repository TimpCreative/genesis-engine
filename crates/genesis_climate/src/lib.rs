//! Climate simulation layer for Genesis Engine.
//!
//! Implements Doc 07. Phase 2 builds out the layer in stages:
//! formation sequence, temperature, circulation, ocean currents,
//! precipitation, regimes, atmospheric composition, variability,
//! and climate-tectonics feedback.

pub mod circulation;
pub mod events;
pub mod formation;
pub mod glaciation;
pub mod hydro_mask;
pub mod layer;
pub mod ocean_basins;
pub mod ocean_currents;
pub mod ocean_distance;
pub mod precipitation;
pub mod regimes;
pub mod state;
pub mod temperature;
pub mod wind;

pub use circulation::compute_circulation;
pub use glaciation::{
    MILANKOVITCH_AMPLITUDE_C, advance_glaciation, advance_orbital_phase,
    glaciation_state_intensity, ice_mask_land_fraction, orbital_temperature_modifier_c,
    write_glaciation_intensity,
};
pub use ocean_basins::identify_ocean_basins;
pub use ocean_currents::compute_ocean_currents;
pub use ocean_distance::compute_distance_to_ocean;
pub use precipitation::compute_precipitation_field;
pub use regimes::{classify, classify_regimes};
pub use temperature::compute_temperature_field;
pub use wind::compute_wind_field;

pub use events::flush_events_to_branch;
pub use formation::{composition_at_year, cooling_temperature_c};
pub use layer::{
    ClimateLayer, DEFAULT_ANCIENT_CLIMATE_TICK_YEARS, DEFAULT_FORMATION_CLIMATE_TICK_YEARS,
    DEFAULT_GEOLOGICAL_CLIMATE_TICK_YEARS, DEFAULT_PREHISTORIC_CLIMATE_TICK_YEARS,
    DEFAULT_RECENT_CLIMATE_TICK_YEARS,
};
pub use state::{
    AtmosphericComposition, CirculationCell, CirculationCells, ClimateRegime, ClimateState,
    FormationSubPhase, GlaciationState, OceanBasin, OceanBasins, STABILIZATION_END_YEAR,
    T_EQUILIBRIUM_C, T_INITIAL_MOLTEN_C, formation_period_active,
};
