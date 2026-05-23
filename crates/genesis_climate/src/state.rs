//! Climate simulation state (Doc 07 §2.3).
//!
//! Held alongside tectonics state at the app layer. Not serialized with
//! [`WorldData`](genesis_core::data::WorldData); reconstructed from world data snapshots if needed.

use std::collections::BTreeMap;

use genesis_core::HexId;
use genesis_core::events::Event;

/// Per-hex climate regime label (Doc 07 §10).
///
/// Placeholder for P2-1. Filled out properly in P2-12 (regime classification).
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum ClimateRegime {
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

impl Default for ClimateRegime {
    fn default() -> Self {
        Self::Unset
    }
}

/// Global atmospheric composition (Doc 07 §3.4, §11).
///
/// Placeholder for P2-1. Filled out in P2-2 (formation) and P2-13 (drift).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AtmosphericComposition {
    pub co2_ppm: f32,
    pub water_vapor_index: f32,
    pub oxygen_fraction: f32,
    pub greenhouse_forcing: f32,
}

impl Default for AtmosphericComposition {
    fn default() -> Self {
        // Earth pre-industrial baseline, used as Phase 1-compatible default
        // until P2-2 implements Formation properly.
        Self {
            co2_ppm: 280.0,
            water_vapor_index: 0.4,
            oxygen_fraction: 0.21,
            greenhouse_forcing: 0.0,
        }
    }
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
#[derive(Clone, Debug, Default)]
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
}

impl ClimateState {
    pub fn new() -> Self {
        Self::default()
    }
}
