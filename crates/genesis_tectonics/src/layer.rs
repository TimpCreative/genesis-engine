//! [`SimulationLayer`] integration for tectonics.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use genesis_core::branches::BranchId;
use genesis_core::data::WorldData;
use genesis_core::events::{Event, EventKind, EventLocation, Significance};
use genesis_core::parameters::WorldParameters;
use genesis_core::rng::WorldRng;
use genesis_core::time::{Era, SimulationLayer, WorldYear};

use crate::boundary::detect_and_classify_boundaries;
use crate::boundary_events::emit_boundary_events;
use crate::elevation::{apply_boundary_elevation, clamp_terrain};
use crate::erosion::{apply_erosion_tick, ensure_deposition_buffer};
use crate::events::{alloc_event_id, maybe_emit};
use crate::hotspots::{apply_hotspot_tick, generate_initial_hotspots};
use crate::initial_generation::generate_initial_plates_data;
use crate::initial_terrain::apply_formation_terrain;
use crate::motion::advance_plate_motion;
use crate::partition::repartition_hexes;
use crate::plate::TectonicsState;
use crate::reorganization::maybe_reorganize;
use crate::sea_level::update_sea_level;
use crate::volcanism::apply_boundary_volcanism;

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
            apply_formation_terrain(world, &state.registry, rng);
            state.hotspots = generate_initial_hotspots(world, rng);
            ensure_deposition_buffer(&mut state, world.grid.cell_count() as usize);

            let event_granularity = world.parameters.core.geology.event_granularity;
            let formation_event_id = alloc_event_id(&mut state);
            maybe_emit(
                &mut state,
                Event {
                    id: formation_event_id,
                    year: world.current_year,
                    branch_id: BranchId::ROOT,
                    location: EventLocation::Global,
                    significance: Significance::Pivotal,
                    kind: EventKind::WorldFormation,
                },
                event_granularity,
            );

            state.formation_complete = true;
            self.last_tick_year.set(world.current_year);
            return Vec::new();
        }

        if era == Era::Geological {
            let interval_years = (world.current_year - self.last_tick_year.get()) as f64;
            self.last_tick_year.set(world.current_year);
            let tick_year = world.current_year;
            let volcanism_scale = world.parameters.core.geology.volcanism_scale;
            let event_granularity = world.parameters.core.geology.event_granularity;

            debug_tick_step("motion", || {
                let plate_ids = state.registry.plate_ids();
                for id in plate_ids {
                    if let Some(plate) = state.registry.plates_mut().get_mut(&id) {
                        advance_plate_motion(plate, interval_years);
                    }
                }
            });

            debug_tick_step("partition", || {
                repartition_hexes(world, &state.registry);
            });

            debug_tick_step("boundaries", || {
                state.boundaries = detect_and_classify_boundaries(world, &state.registry);
            });

            state.elevation_at_tick_start = world.elevation_mean.clone();

            debug_tick_step("elevation", || {
                apply_boundary_elevation(world, &state.registry, &state.boundaries, interval_years);
            });

            debug_tick_step("volcanism", || {
                apply_boundary_volcanism(
                    world,
                    &mut state,
                    rng,
                    volcanism_scale,
                    event_granularity,
                    tick_year,
                    BranchId::ROOT,
                );
            });

            debug_tick_step("hotspots", || {
                apply_hotspot_tick(
                    world,
                    &mut state,
                    rng,
                    tick_year,
                    event_granularity,
                    BranchId::ROOT,
                );
            });

            debug_tick_step("erosion", || {
                apply_erosion_tick(world, &mut state, rng, tick_year, interval_years);
            });

            let reorg_fired = debug_tick_step("reorg", || {
                maybe_reorganize(
                    world,
                    &mut state,
                    rng,
                    tick_year,
                    event_granularity,
                    BranchId::ROOT,
                )
            });

            if reorg_fired {
                debug_tick_step("repartition_after_reorg", || {
                    repartition_hexes(world, &state.registry);
                    state.boundaries = detect_and_classify_boundaries(world, &state.registry);
                });
            }

            let boundaries = state.boundaries.clone();
            debug_tick_step("sea_level", || {
                update_sea_level(
                    world,
                    &boundaries,
                    &mut state,
                    rng,
                    tick_year,
                    reorg_fired,
                    event_granularity,
                    BranchId::ROOT,
                );
            });

            debug_tick_step("boundary_events", || {
                emit_boundary_events(
                    world,
                    &boundaries,
                    &mut state,
                    tick_year,
                    event_granularity,
                    BranchId::ROOT,
                );
            });

            debug_tick_step("clamp", || {
                clamp_terrain(world);
            });

            let (min_elev, max_elev) = elevation_min_max(world);
            tracing::debug!(
                year = tick_year.value(),
                min_elevation_m = min_elev,
                max_elevation_m = max_elev,
                "tectonics geological tick complete"
            );
        }

        Vec::new()
    }
}

fn elevation_min_max(world: &WorldData) -> (f32, f32) {
    let mut min = f32::MAX;
    let mut max = f32::MIN;
    for &e in &world.elevation_mean {
        min = min.min(e);
        max = max.max(e);
    }
    if min == f32::MAX {
        (0.0, 0.0)
    } else {
        (min, max)
    }
}

/// Runs `f` and logs elapsed milliseconds when `RUST_LOG=genesis_tectonics=debug` (§9.3).
fn debug_tick_step<T>(step: &'static str, f: impl FnOnce() -> T) -> T {
    let start = tracing::enabled!(tracing::Level::DEBUG).then(std::time::Instant::now);
    let out = f();
    if let Some(t0) = start {
        tracing::debug!(step, elapsed_ms = t0.elapsed().as_millis() as u64);
    }
    out
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
