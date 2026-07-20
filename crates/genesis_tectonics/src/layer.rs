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
use crate::coast_cleanup::cleanup_coast_artifacts;
use crate::elevation::clamp_terrain;
use crate::erosion::{apply_erosion_tick, ensure_deposition_buffer};
use crate::events::{alloc_event_id, maybe_emit};
use crate::hotspots::{apply_hotspot_tick, generate_initial_hotspots};
use crate::initial_generation::generate_initial_plates_data;
use crate::initial_terrain::apply_formation_terrain;
use crate::motion::advance_plate_motion;
use crate::partition::repartition_hexes;
use crate::plate::TectonicsState;
use crate::reorganization::maybe_reorganize;
use crate::volcanism::apply_boundary_volcanism;
use crate::world_rebuild::rebuild_world_from_plate_surfaces_cached;

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
            Era::Prehistoric => 2_000_000,
            Era::Ancient => 10_000_000,
            Era::Recent => 0,
        }
    }

    fn advance(&mut self, world: &mut WorldData, rng: &WorldRng) -> Vec<()> {
        let params = &world.parameters;
        let era = Era::for_year(world.current_year, params);
        let mut state = self.state.borrow_mut();

        if !state.formation_complete && era == Era::Formation {
            state.registry = generate_initial_plates_data(world, rng);
            apply_formation_terrain(world, &mut state.registry, rng);
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

            // Calibrate the formation world so downstream layers see the target
            // hypsometry and pinned datum from year 0 (Doc 10). Seeds the
            // temporal ranking EMA (interval 0).
            {
                let targets = world.parameters.core.terrain;
                crate::calibration::apply_hypsometry_transfer(
                    world,
                    &targets,
                    &mut state.calibration_rank_ema,
                    0.0,
                );
            }

            state.formation_complete = true;
            self.last_tick_year.set(world.current_year);
            return Vec::new();
        }

        if matches!(era, Era::Geological | Era::Prehistoric | Era::Ancient) {
            let interval_years = (world.current_year - self.last_tick_year.get()) as f64;
            self.last_tick_year.set(world.current_year);
            let tick_year = world.current_year;
            let volcanism_scale = world.parameters.core.geology.volcanism_scale;
            let event_granularity = world.parameters.core.geology.event_granularity;

            timed_tick_step("motion", tick_year, || {
                // Slab pull: relax rates toward boundary-force targets using
                // last tick's tallies (a 1-tick lag is geologically nothing),
                // then lock colliding continental pairs into a shared drift
                // (§4.6), then advance rotation with the relaxed rates.
                {
                    let geology = &world.parameters.core.geology;
                    let planet = &world.parameters.core.planet;
                    let TectonicsState {
                        registry,
                        boundary_tallies,
                        ..
                    } = &mut *state;
                    crate::motion::relax_motion_rates_toward_targets(
                        registry,
                        boundary_tallies,
                        geology,
                        planet,
                        interval_years,
                    );
                }
                let TectonicsState {
                    registry,
                    colliding_pairs,
                    ..
                } = &mut *state;
                crate::collision_jam::apply_collision_jam(
                    world,
                    registry,
                    colliding_pairs,
                    interval_years,
                );
                let plate_ids = state.registry.plate_ids();
                for id in plate_ids {
                    if let Some(plate) = state.registry.plates_mut().get_mut(&id) {
                        advance_plate_motion(plate, interval_years);
                    }
                }
            });

            let outcome = timed_tick_step_value("partition", tick_year, || {
                repartition_hexes(world, &mut state.registry)
            });
            state.projection = outcome.projection;
            state.colliding_pairs = outcome.colliding_pairs;

            timed_tick_step("rebuild_world", tick_year, || {
                rebuild_world_from_plate_surfaces_cached(world, &state.registry, &state.projection);
            });

            timed_tick_step("boundaries", tick_year, || {
                state.boundaries =
                    detect_and_classify_boundaries(world, &state.registry, &state.projection);
                state.boundary_tallies = crate::boundary::plate_boundary_tallies(
                    world,
                    &state.registry,
                    &state.projection,
                    &state.boundaries,
                );
            });

            capture_elevation_at_tick_start(&mut state, &world.elevation_mean);

            // One water-realm labeling per tick, shared by the elevation pass
            // (trench enclosure) and the accretion pass (trapped basins):
            // data.elevation_mean only changes at world rebuilds, so the
            // labeling is exact for every step between rebuilds.
            let water = crate::accretion::label_water_components(world);
            let open_ocean = water.open_ocean_mask();
            timed_tick_step("elevation", tick_year, || {
                let TectonicsState {
                    registry,
                    projection,
                    boundaries,
                    ..
                } = &mut *state;
                crate::elevation::apply_boundary_elevation_with_mask(
                    world,
                    registry,
                    projection,
                    boundaries,
                    &open_ocean,
                    interval_years,
                    tick_year,
                );
            });

            timed_tick_step("volcanism", tick_year, || {
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

            timed_tick_step("hotspots", tick_year, || {
                apply_hotspot_tick(
                    world,
                    &mut state,
                    rng,
                    tick_year,
                    event_granularity,
                    BranchId::ROOT,
                );
            });

            // Suture accretion: oceanic crust trapped inside continents by
            // closing basins is consumed before erosion/isostasy, so rebound
            // starts lifting it this same tick.
            timed_tick_step("accretion", tick_year, || {
                let TectonicsState {
                    registry,
                    projection,
                    boundaries,
                    ..
                } = &mut *state;
                crate::accretion::accrete_trapped_oceanic_crust(
                    world,
                    registry,
                    projection,
                    &water,
                    boundaries,
                    rng,
                    tick_year.value(),
                    interval_years,
                );
            });

            // Crust balance counter-flow: erosive margins consume forearc
            // rims, bounding the continental fraction over deep time.
            timed_tick_step("subduction_erosion", tick_year, || {
                let s = &mut *state;
                crate::accretion::apply_subduction_erosion(
                    world,
                    &mut s.registry,
                    &s.projection,
                    &s.boundaries,
                    rng,
                    tick_year.value(),
                    interval_years,
                );
            });

            timed_tick_step("erosion", tick_year, || {
                apply_erosion_tick(world, &mut state, rng, tick_year, interval_years);
            });

            // Gravitational collapse (§8.5): relief beyond rock strength
            // spreads under its own weight — no water required.
            timed_tick_step("collapse", tick_year, || {
                let TectonicsState {
                    registry,
                    projection,
                    ..
                } = &mut *state;
                crate::collapse::apply_gravitational_collapse(
                    world,
                    registry,
                    projection,
                    interval_years,
                    tick_year,
                );
            });

            let reorg_fired = timed_tick_step_value("reorg", tick_year, || {
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
                state.reorg_count += 1;
                timed_tick_step("repartition_after_reorg", tick_year, || {
                    let s = &mut *state;
                    s.projection = repartition_hexes(world, &mut s.registry).projection;
                    s.boundaries =
                        detect_and_classify_boundaries(world, &s.registry, &s.projection);
                    s.boundary_tallies = crate::boundary::plate_boundary_tallies(
                        world,
                        &s.registry,
                        &s.projection,
                        &s.boundaries,
                    );
                });
            }

            let boundaries = std::mem::take(&mut state.boundaries);
            timed_tick_step("boundary_events", tick_year, || {
                emit_boundary_events(
                    world,
                    &boundaries,
                    &mut state,
                    tick_year,
                    event_granularity,
                    BranchId::ROOT,
                );
            });

            timed_tick_step("rebuild_world_final", tick_year, || {
                rebuild_world_from_plate_surfaces_cached(world, &state.registry, &state.projection);
            });

            // These passes clean the raw *structure* the calibration then maps:
            // coast de-speckle removes coastal spray, continental heal + closed-
            // depression infill fill the multi-hex accreted-oceanic interior pits
            // that calibration's smoothed ranking does NOT dissolve on its own
            // (it lifts isolated 1-hex lows, not whole basins). Empirically,
            // dropping them under calibration doubled the dry sub-sea perforation
            // (129 -> 269 @ subdiv 7, 1B), so they earn their place as structure
            // conditioning — Doc 10 §9's "delete" is retracted for these.
            timed_tick_step("coast_cleanup", tick_year, || {
                let s = &mut *state;
                cleanup_coast_artifacts(
                    world,
                    &mut s.registry,
                    &s.projection,
                    &boundaries,
                    tick_year.value(),
                );
                rebuild_world_from_plate_surfaces_cached(world, &s.registry, &s.projection);
                crate::continental_heal::heal_continental_surface(
                    world,
                    &mut s.registry,
                    &s.projection,
                    tick_year.value(),
                );
                rebuild_world_from_plate_surfaces_cached(world, &s.registry, &s.projection);
            });

            // Closed-depression sediment infill: oceanic/accreted interior
            // pits heal skipped. Recompute the open-ocean mask after heal
            // raised continental crust (tick-start mask at label_water is stale).
            timed_tick_step("basin_infill", tick_year, || {
                let s = &mut *state;
                let water = crate::accretion::label_water_components(world);
                let open_ocean = water.open_ocean_mask();
                crate::basin_infill::fill_closed_depressions(
                    world,
                    &mut s.registry,
                    &s.projection,
                    &open_ocean,
                    tick_year.value(),
                    interval_years,
                );
                rebuild_world_from_plate_surfaces_cached(world, &s.registry, &s.projection);
            });
            state.boundaries = boundaries;

            timed_tick_step("clamp", tick_year, || {
                clamp_terrain(world);
            });

            // Solve-to-target calibration (Doc 10): map the structure field onto
            // the target hypsometric curve and pin the datum to 0. Final word on
            // absolute height; the raw structure is rebuilt from plate surfaces
            // next tick, so this never feeds back into the sim.
            timed_tick_step("calibration", tick_year, || {
                let targets = world.parameters.core.terrain;
                crate::calibration::apply_hypsometry_transfer(
                    world,
                    &targets,
                    &mut state.calibration_rank_ema,
                    interval_years,
                );
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

/// Copy `elevation_mean` into the reusable tick-start buffer (no per-tick alloc).
fn capture_elevation_at_tick_start(state: &mut TectonicsState, elevation_mean: &[f32]) {
    if state.elevation_at_tick_start.len() != elevation_mean.len() {
        state.elevation_at_tick_start = elevation_mean.to_vec();
    } else {
        state
            .elevation_at_tick_start
            .copy_from_slice(elevation_mean);
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

const SLOW_TICK_STEP_MS: u128 = 100;

/// Slow-step log threshold in ms; `GENESIS_SLOW_TICK_STEP_MS` overrides it
/// for profiling runs (e.g. `GENESIS_SLOW_TICK_STEP_MS=10` to see the per-step
/// cost profile at subdivision level 7).
fn slow_tick_step_threshold_ms() -> u128 {
    static THRESHOLD: std::sync::OnceLock<u128> = std::sync::OnceLock::new();
    *THRESHOLD.get_or_init(|| {
        std::env::var("GENESIS_SLOW_TICK_STEP_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(SLOW_TICK_STEP_MS)
    })
}

fn timed_tick_step(step: &'static str, tick_year: WorldYear, f: impl FnOnce()) {
    let step_start = std::time::Instant::now();
    f();
    log_slow_tick_step(step, tick_year, step_start.elapsed());
}

fn timed_tick_step_value<T>(step: &'static str, tick_year: WorldYear, f: impl FnOnce() -> T) -> T {
    let step_start = std::time::Instant::now();
    let out = f();
    log_slow_tick_step(step, tick_year, step_start.elapsed());
    out
}

fn log_slow_tick_step(step: &'static str, tick_year: WorldYear, elapsed: std::time::Duration) {
    if elapsed.as_millis() > slow_tick_step_threshold_ms() {
        eprintln!(
            "[tectonics] {} tick at year {} took {}ms",
            step,
            tick_year.value(),
            elapsed.as_millis()
        );
        let _ = std::io::Write::flush(&mut std::io::stderr());
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
