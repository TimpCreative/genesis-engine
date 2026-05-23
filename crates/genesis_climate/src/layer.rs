//! [`SimulationLayer`] integration for climate.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use genesis_core::data::WorldData;
use genesis_core::parameters::WorldParameters;
use genesis_core::rng::WorldRng;
use genesis_core::time::{Era, SimulationLayer, WorldYear};

use crate::events::{emit_phase_transition_event, maybe_emit_cooling_milestone};
use crate::formation::{composition_at_year, cooling_temperature_c, sea_level_at_year};
use crate::state::{ClimateState, FormationSubPhase, formation_period_active};

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
        if formation_period_active(current_time.value(), params) {
            return DEFAULT_FORMATION_CLIMATE_TICK_YEARS;
        }

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
        let mut state = self.state.borrow_mut();
        let params = &world.parameters;
        let current_year_value = world.current_year.value();

        if params.core.climate.skip_planetary_formation {
            if !state.formation_complete {
                state.formation_complete = true;
                state.formation_sub_phase = FormationSubPhase::Complete;
            }
        } else if formation_period_active(current_year_value, params) && !state.formation_complete {
            let new_phase = FormationSubPhase::for_year(current_year_value);
            let prev_phase = state.formation_sub_phase;

            world.global_temperature_c = cooling_temperature_c(current_year_value);
            world.sea_level_m = sea_level_at_year(current_year_value);
            state.atmospheric_composition = composition_at_year(current_year_value);

            if new_phase != prev_phase {
                emit_phase_transition_event(
                    &mut state,
                    world,
                    prev_phase,
                    new_phase,
                    world.current_year,
                    params.core.climate.event_granularity,
                );
                state.formation_sub_phase = new_phase;
            }

            if new_phase == FormationSubPhase::Complete {
                state.formation_complete = true;
            }

            maybe_emit_cooling_milestone(
                &mut state,
                world.global_temperature_c,
                world.current_year,
                params.core.climate.event_granularity,
            );
        }

        self.last_tick_year.set(world.current_year);
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::time::TickCoordinator;
    use genesis_core::{WorldYear, create_world};

    #[test]
    fn climate_layer_ticks_at_formation_interval_during_formation_period() {
        let state = ClimateState::default();
        let params = WorldParameters::default();

        let mut state_owned = state;
        let (layer, _shared) = ClimateLayer::attach(&mut state_owned);

        let interval = layer.tick_interval(WorldYear(100), &params);
        assert_eq!(interval, DEFAULT_FORMATION_CLIMATE_TICK_YEARS);
    }

    #[test]
    fn climate_layer_ticks_at_geological_interval_after_formation() {
        let state = ClimateState::default();
        let params = WorldParameters::default();

        let mut state_owned = state;
        let (layer, _shared) = ClimateLayer::attach(&mut state_owned);

        let interval = layer.tick_interval(WorldYear(600_000_000), &params);
        assert_eq!(interval, DEFAULT_GEOLOGICAL_CLIMATE_TICK_YEARS);
    }

    #[test]
    fn formation_completes_by_end_of_formation_era() {
        // Climate-only coordinator: fast, no tectonic ticks to 500M.
        let params = WorldParameters::default();
        let mut world = create_world(params).expect("world");
        let mut climate = ClimateState::new();

        let (layer, shared) = ClimateLayer::attach(&mut climate);
        let mut coordinator = TickCoordinator::new();
        coordinator.add_layer(Box::new(layer));

        let params = world.data.parameters.clone();
        coordinator.advance_to(WorldYear(500_000_000), &mut world.data, &world.rng, &params);
        drop(coordinator);

        let climate = ClimateLayer::detach_state(shared);

        assert!(climate.formation_complete);
        assert_eq!(climate.formation_sub_phase, FormationSubPhase::Complete);
        assert!(
            (world.data.sea_level_m - 0.0).abs() < 50.0,
            "sea level should be near modern; got {}",
            world.data.sea_level_m
        );
        assert!(
            (world.data.global_temperature_c - 15.0).abs() < 20.0,
            "temperature should be near equilibrium; got {}",
            world.data.global_temperature_c
        );
    }
}
