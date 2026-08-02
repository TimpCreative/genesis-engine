//! Hydrology & soil simulation layer for Genesis Engine (Doc 08).
//!
//! Slice 1 (P2-20..P2-22): the conserved planetary water budget (§3.2),
//! Formation condensation (§3.3), and the flooding solve (§3.4) that derives
//! `sea_level_m`, the ocean mask, and the water-body registry from the
//! hypsometry — including the thermosteric term (§3.5.1).
//!
//! Slice 2 (P2-23..P2-26): the drainage network (§4), lake balance (§5),
//! groundwater (§6), and seasonal regime (§7).
//!
//! Slice 3 (P2-27+): erosion/sediment (§8), ice (§9), soil (§10), coastal
//! (§11) — elevation deltas apply through tectonics' plate-surface API.

pub mod budget;
pub mod coastal;
pub mod erosion;
pub mod events;
pub mod groundwater;
pub mod ice;
pub mod lakes;
pub mod layer;
pub mod partition;
pub mod regime;
pub mod rivers;
pub mod routing;
pub mod soil;
pub mod solve;
pub mod state;
pub mod validation;

pub use budget::{
    CONDENSATION_COMPLETE_C, CONDENSATION_ONSET_C, CONSERVATION_TOLERANCE_REL, FORMATION_END_YEAR,
    WaterBudget, condensed_fraction_at_year, formation_cooling_temperature_c, inventory_volume_m3,
};
pub use erosion::{
    CONTINENTAL_FREEBOARD_M, CONTINENTAL_INCISION_ALLOWANCE_M, DEPOSITION_THRESHOLD_M,
    ErosionOutcome, K_CHANNEL_PER_YEAR, apply_erosion, continental_fluvial_floor_m,
};
pub use events::flush_events_to_branch;
pub use groundwater::is_hot_spring;
pub use lakes::{
    SALINE_SOIL_SALT_MIN, SALT_EXPORT_RATE, SALT_LAKE_SALINITY_THRESHOLD, SALT_LOAD_FACTOR,
    export_salt,
};
pub use layer::{
    DEFAULT_ANCIENT_HYDROLOGY_TICK_YEARS, DEFAULT_FORMATION_HYDROLOGY_TICK_YEARS,
    DEFAULT_GEOLOGICAL_HYDROLOGY_TICK_YEARS, DEFAULT_PREHISTORIC_HYDROLOGY_TICK_YEARS,
    DEFAULT_RECENT_HYDROLOGY_TICK_YEARS, HydrologyLayer,
};
pub use regime::{EPHEMERAL_BASEFLOW_MIN_M3_YR, FlowRegime, flood_magnitude_m3_yr};
pub use rivers::{
    MAJOR_CLASS_MIN_M3_YR, Navigability, RIVER_CLASS_MIN_M3_YR, RiverClass, STREAM_CLASS_MIN_M3_YR,
    is_waterfall, navigability, river_class,
};
pub use solve::{CandidateSea, FloodOutcome, solve_flooding, thermosteric_effective_volume_m3};
pub use state::HydrologyState;
