//! Hydrology layer for Genesis Engine (Doc 08 scope, Phase 2/3).
//!
//! Surface water flow derived from terrain and precipitation: flow directions,
//! accumulated discharge (rivers), and endorheic sinks (future lakes).

pub mod flow;
pub mod layer;
pub mod soil;

pub use flow::{
    DEFAULT_PRECIPITATION_MM, RUNOFF_COEFFICIENT, compute_flow_accumulation,
    compute_flow_directions, hex_area_m2,
};
pub use layer::HydrologyLayer;
pub use soil::compute_soil_fertility;
