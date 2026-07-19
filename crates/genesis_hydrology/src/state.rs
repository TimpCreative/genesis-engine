//! Hydrology simulation state (Doc 08 §2.2).
//!
//! Held alongside tectonics/climate state at the app layer. Not serialized
//! with [`WorldData`](genesis_core::data::WorldData); the per-tick fields it
//! feeds (`sea_level_m`, water arrays, registry) are stateless derivations.

use std::collections::{BTreeMap, BTreeSet};

use genesis_core::HexId;
use genesis_core::data::{Direction, WaterBody, WaterBodyId};
use genesis_core::events::Event;

/// State held by [`HydrologyLayer`](crate::layer::HydrologyLayer) across ticks.
///
/// Persistent accumulators per §2.2: the Formation atmosphere reserve, the
/// per-hex aquifer storage (whose deterministic sum is the §3.2 groundwater
/// reservoir), and the lake/ice budget terms.
#[derive(Clone, Debug)]
pub struct HydrologyState {
    /// Events queued for emission this tick (cleared on flush).
    pub pending_events: Vec<Event>,
    /// Monotonic event ID counter for this layer's events.
    pub next_event_id: u64,
    /// Uncondensed inventory still in the atmosphere, m³ (§3.3). Nonzero only
    /// during Formation; informational — recomputed from the condensed
    /// fraction every tick.
    pub atmosphere_reserve_m3: f64,
    /// Water stored in aquifers, m³ — the §3.2 reservoir. Deterministic
    /// ascending-`HexId` sum of [`Self::aquifer_storage_m`] × hex area,
    /// recomputed every active tick (§6.1).
    pub groundwater_storage_m3: f64,
    /// Per-hex aquifer storage in meters of water (§6.1). Sized to the grid
    /// lazily on the first active tick; multiplied by hex area for volumes.
    /// Persistent across ticks (a §2.2 accumulator).
    pub aquifer_storage_m: Vec<f64>,
    /// Previous tick's summed lake volume, m³ — the registry's non-ocean
    /// total (`water_bodies`, §5); the flooding solve debits it from the
    /// ocean term.
    pub prev_lake_volume_m3: f64,
    /// Budgeted land-ice volume, m³. Zero until ice masses land (Slice 3, §9);
    /// the flooding solve debits it from the ocean term.
    pub ice_volume_m3: f64,
    /// Persistent alluvium depth (m) per hex — soil's deposition input (§8.3 / §10).
    pub alluvium_depth_m: Vec<f32>,
    /// Previous-tick ice mask for glacial retreat diffs (§9.2).
    pub prev_ice_mask: Vec<bool>,
    /// Previous-tick sea level for milestone events (§13).
    pub prev_sea_level_m: Option<f32>,
    /// Previous-tick water-body registry snapshot for lake/sea diffs (§13).
    pub prev_water_bodies: BTreeMap<WaterBodyId, WaterBody>,
    /// Previous-tick flow directions for Major-river avulsion detection (§13).
    pub prev_flow_direction: Vec<Option<Direction>>,
    /// Peak ice SLE drawdown seen so far (for GlacialMaximum once).
    pub peak_ice_sle_drop_m: f32,
    /// Hexes that have already emitted SaltLakeFormed.
    pub emitted_salt_lakes: BTreeSet<HexId>,
    /// Hexes that have already emitted SaltFlatFormed membership.
    pub emitted_salt_flats: BTreeSet<HexId>,
    /// Hexes that have already emitted FjordsCarved membership.
    pub emitted_fjords: BTreeSet<HexId>,
    /// Hexes that have already emitted OasisFormed.
    pub emitted_oases: BTreeSet<HexId>,
    /// Hexes that have already emitted GreatSpringEmerges.
    pub emitted_springs: BTreeSet<HexId>,
    /// True once a GlacialMaximum event has been emitted.
    pub glacial_maximum_emitted: bool,
    /// True once the formation event flags have been initialized (used to
    /// suppress the ocean narrative on `skip_planetary_formation` worlds).
    pub formation_events_initialized: bool,
    /// True once [`EventKind::OceansBeginForming`](genesis_core::events::EventKind)
    /// has been emitted.
    pub oceans_begin_emitted: bool,
    /// True once [`EventKind::OceansStabilized`](genesis_core::events::EventKind)
    /// has been emitted.
    pub oceans_stabilized_emitted: bool,
}

impl Default for HydrologyState {
    fn default() -> Self {
        Self {
            pending_events: Vec::new(),
            next_event_id: 0,
            atmosphere_reserve_m3: 0.0,
            groundwater_storage_m3: 0.0,
            aquifer_storage_m: Vec::new(),
            prev_lake_volume_m3: 0.0,
            ice_volume_m3: 0.0,
            alluvium_depth_m: Vec::new(),
            prev_ice_mask: Vec::new(),
            prev_sea_level_m: None,
            prev_water_bodies: BTreeMap::new(),
            prev_flow_direction: Vec::new(),
            peak_ice_sle_drop_m: 0.0,
            emitted_salt_lakes: BTreeSet::new(),
            emitted_salt_flats: BTreeSet::new(),
            emitted_fjords: BTreeSet::new(),
            emitted_oases: BTreeSet::new(),
            emitted_springs: BTreeSet::new(),
            glacial_maximum_emitted: false,
            formation_events_initialized: false,
            oceans_begin_emitted: false,
            oceans_stabilized_emitted: false,
        }
    }
}

impl HydrologyState {
    pub fn new() -> Self {
        Self::default()
    }
}
