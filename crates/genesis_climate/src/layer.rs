//! [`SimulationLayer`] integration for climate.
//!
//! P2-1 wires up the layer infrastructure with no climate logic. Subsequent
//! prompts fill in per-section behavior per Doc 07.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use genesis_core::data::WorldData;
use genesis_core::parameters::WorldParameters;
use genesis_core::rng::WorldRng;
use genesis_core::time::{Era, SimulationLayer, WorldYear};

use crate::state::ClimateState;

/// Default Geological-era climate tick interval (Doc 07 §2.2).
pub const DEFAULT_GEOLOGICAL_CLIMATE_TICK_YEARS: i64 = 500_000;
/// Default Prehistoric-era climate tick interval (Doc 07 §2.2).
pub const DEFAULT_PREHISTORIC_CLIMATE_TICK_YEARS: i64 = 500_000;
/// Default Ancient-era climate tick interval (Doc 07 §2.2).
pub const DEFAULT_ANCIENT_CLIMATE_TICK_YEARS: i64 = 100_000;
/// Default Recent-era climate tick interval (Doc 07 §2.2).
pub const DEFAULT_RECENT_CLIMATE_TICK_YEARS: i64 = 1_000;
/// Default Formation-era climate tick interval (Doc 07 §3.2).
pub const DEFAULT_FORMATION_CLIMATE_TICK_YEARS: i64 = 5_000_000;

/// Climate simulation layer (Doc 07).
pub struct ClimateLayer {
    state: Rc<RefCell<ClimateState>>,
    last_tick_year: Cell<WorldYear>,
}

impl ClimateLayer {
    /// Creates a layer sharing `state` with the caller via `Rc`.
    pub fn attach(state: &mut ClimateState) -> (Self, Rc<RefCell<ClimateState>>) {
        let shared = Rc::new(RefCell::new(std::mem::take(state)));
        let layer = Self {
            state: Rc::clone(&shared),
            last_tick_year: Cell::new(WorldYear::FORMATION),
        };
        (layer, shared)
    }

    /// Recovers owned state from a shared handle after tick simulation.
    pub fn detach_state(shared: Rc<RefCell<ClimateState>>) -> ClimateState {
        Rc::try_unwrap(shared)
            .expect("climate state still borrowed")
            .into_inner()
    }
}

impl SimulationLayer for ClimateLayer {
    fn name(&self) -> &str {
        "climate"
    }

    fn tick_interval(&self, current_time: WorldYear, params: &WorldParameters) -> i64 {
        let era = Era::for_year(current_time, params);
        match era {
            Era::Formation => DEFAULT_FORMATION_CLIMATE_TICK_YEARS,
            Era::Geological => DEFAULT_GEOLOGICAL_CLIMATE_TICK_YEARS,
            Era::Prehistoric => DEFAULT_PREHISTORIC_CLIMATE_TICK_YEARS,
            Era::Ancient => DEFAULT_ANCIENT_CLIMATE_TICK_YEARS,
            Era::Recent => DEFAULT_RECENT_CLIMATE_TICK_YEARS,
        }
    }

    fn advance(&mut self, world: &mut WorldData, _rng: &WorldRng) -> Vec<()> {
        // P2-1: Layer is wired but does no work yet. Subsequent prompts fill
        // in formation (P2-2), temperature (P2-7), etc.
        let _state = self.state.borrow_mut();
        self.last_tick_year.set(world.current_year);
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;

    #[test]
    fn climate_layer_ticks_at_geological_interval_by_default() {
        let state = ClimateState::default();
        let params = WorldParameters::default();

        let mut state_owned = state;
        let (layer, _shared) = ClimateLayer::attach(&mut state_owned);

        let interval = layer.tick_interval(WorldYear(100), &params);
        assert_eq!(interval, DEFAULT_GEOLOGICAL_CLIMATE_TICK_YEARS);
    }
}
