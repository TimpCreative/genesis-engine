//! Hydrology & soil simulation layer for Genesis Engine (Doc 08).
//!
//! Slice 1 (P2-20..P2-22): the conserved planetary water budget (§3.2),
//! Formation condensation (§3.3), and the flooding solve (§3.4) that derives
//! `sea_level_m`, the ocean mask, and the water-body registry from the
//! hypsometry — including the thermosteric term (§3.5.1). Drainage, lakes,
//! groundwater detail, ice, erosion, and soil arrive in later slices.

pub mod budget;
pub mod events;
pub mod layer;
pub mod solve;
pub mod state;

pub use budget::{
    CONSERVATION_TOLERANCE_REL, WaterBudget, condensed_fraction_at_year, inventory_volume_m3,
};
pub use events::flush_events_to_branch;
pub use layer::{
    DEFAULT_ANCIENT_HYDROLOGY_TICK_YEARS, DEFAULT_FORMATION_HYDROLOGY_TICK_YEARS,
    DEFAULT_GEOLOGICAL_HYDROLOGY_TICK_YEARS, DEFAULT_PREHISTORIC_HYDROLOGY_TICK_YEARS,
    DEFAULT_RECENT_HYDROLOGY_TICK_YEARS, HydrologyLayer,
};
pub use solve::{FloodOutcome, solve_flooding, thermosteric_effective_volume_m3};
pub use state::HydrologyState;
