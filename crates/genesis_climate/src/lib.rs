//! Climate simulation layer for Genesis Engine.
//!
//! Implements Doc 07. Phase 2 builds out the layer in stages:
//! formation sequence, temperature, circulation, ocean currents,
//! precipitation, regimes, atmospheric composition, variability,
//! and climate-tectonics feedback.

pub mod layer;
pub mod state;

pub use layer::{
    ClimateLayer, DEFAULT_ANCIENT_CLIMATE_TICK_YEARS, DEFAULT_FORMATION_CLIMATE_TICK_YEARS,
    DEFAULT_GEOLOGICAL_CLIMATE_TICK_YEARS, DEFAULT_PREHISTORIC_CLIMATE_TICK_YEARS,
    DEFAULT_RECENT_CLIMATE_TICK_YEARS,
};
pub use state::{AtmosphericComposition, ClimateRegime, ClimateState, GlaciationState};
