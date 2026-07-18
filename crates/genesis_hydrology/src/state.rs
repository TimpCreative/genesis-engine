//! Hydrology simulation state (Doc 08 §2.2).
//!
//! Held alongside tectonics/climate state at the app layer. Not serialized
//! with [`WorldData`](genesis_core::data::WorldData); the per-tick fields it
//! feeds (`sea_level_m`, water arrays, registry) are stateless derivations.

use genesis_core::events::Event;

/// State held by [`HydrologyLayer`](crate::layer::HydrologyLayer) across ticks.
///
/// Persistent accumulators per §2.2 for this slice: the Formation atmosphere
/// reserve, groundwater storage, and the lake/ice budget terms (zero until
/// their systems land in later slices).
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
    /// Water stored in aquifers, m³. Relaxes toward capacity once condensation
    /// begins (§3.3; the aridity-equilibrium target is Slice 2's §6).
    pub groundwater_storage_m3: f64,
    /// Previous tick's summed lake volume, m³. Zero until the lake balance
    /// lands (Slice 2, §5); the flooding solve debits it from the ocean term.
    pub prev_lake_volume_m3: f64,
    /// Budgeted land-ice volume, m³. Zero until ice masses land (Slice 2, §9);
    /// the flooding solve debits it from the ocean term.
    pub ice_volume_m3: f64,
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
            prev_lake_volume_m3: 0.0,
            ice_volume_m3: 0.0,
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
