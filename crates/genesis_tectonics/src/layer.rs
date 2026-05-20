//! [`SimulationLayer`] integration for tectonics.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use genesis_core::data::WorldData;
use genesis_core::parameters::WorldParameters;
use genesis_core::rng::WorldRng;
use genesis_core::time::{Era, SimulationLayer, WorldYear};

use crate::boundary::detect_and_classify_boundaries;
use crate::initial_generation::generate_initial_plates_data;
use crate::motion::advance_plate_motion;
use crate::partition::repartition_hexes;
use crate::plate::TectonicsState;

/// Default Geological-era tick interval (Doc 06 §4.1).
pub const DEFAULT_GEOLOGICAL_TICK_YEARS: i64 = 500_000;

/// Tectonic simulation layer for the [`TickCoordinator`](genesis_core::TickCoordinator).
pub struct TectonicsLayer {
    state: Rc<RefCell<TectonicsState>>,
    last_tick_year: Cell<WorldYear>,
}

impl TectonicsLayer {
    /// Creates a layer sharing `state` with the caller via `Rc`.
    ///
    /// Returns the layer and the shared handle; after the coordinator runs, call
    /// [`detach_state`] to recover ownership.
    pub fn attach(state: &mut TectonicsState) -> (Self, Rc<RefCell<TectonicsState>>) {
        let shared = Rc::new(RefCell::new(std::mem::take(state)));
        let layer = Self {
            state: Rc::clone(&shared),
            last_tick_year: Cell::new(WorldYear::FORMATION),
        };
        (layer, shared)
    }

    /// Recovers owned state from a shared handle after tick simulation.
    pub fn detach_state(shared: Rc<RefCell<TectonicsState>>) -> TectonicsState {
        Rc::try_unwrap(shared)
            .expect("tectonics state still borrowed")
            .into_inner()
    }
}

impl SimulationLayer for TectonicsLayer {
    fn name(&self) -> &str {
        "tectonics"
    }

    fn tick_interval(&self, current_time: WorldYear, params: &WorldParameters) -> i64 {
        let era = Era::for_year(current_time, params);
        match era {
            Era::Formation | Era::Geological => geological_tick_interval(params),
            _ => 0,
        }
    }

    fn advance(&mut self, world: &mut WorldData, rng: &WorldRng) -> Vec<()> {
        let params = &world.parameters;
        let era = Era::for_year(world.current_year, params);
        let mut state = self.state.borrow_mut();

        if !state.formation_complete && era == Era::Formation {
            state.registry = generate_initial_plates_data(world, rng);
            state.formation_complete = true;
            self.last_tick_year.set(world.current_year);
            return Vec::new();
        }

        if era == Era::Geological {
            let interval_years = (world.current_year - self.last_tick_year.get()) as f64;
            self.last_tick_year.set(world.current_year);

            let plate_ids = state.registry.plate_ids();
            for id in plate_ids {
                if let Some(plate) = state.registry.plates_mut().get_mut(&id) {
                    advance_plate_motion(plate, interval_years);
                }
            }

            repartition_hexes(world, &state.registry);
            state.boundaries = detect_and_classify_boundaries(world, &state.registry);
            tracing::debug!(
                year = world.current_year.value(),
                boundary_hex_count = state.boundaries.boundary_hexes.len(),
                "tectonics boundaries classified"
            );
        }

        Vec::new()
    }
}

/// Geological tick interval from parameters or Doc 06 default.
pub fn geological_tick_interval(params: &WorldParameters) -> i64 {
    params
        .core
        .geology
        .tick_interval_overrides_years
        .as_ref()
        .and_then(|m| m.get(&Era::Geological).copied())
        .unwrap_or(DEFAULT_GEOLOGICAL_TICK_YEARS)
}
