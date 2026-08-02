//! World generation entry point and history buffering for the interactive app.
//!
//! Owns the layer registration order (tectonics → climate → hydrology, Doc 08
//! §2.1) and captures lightweight [`HistoryFrame`]s during generation so the
//! viewer can scrub the timeline without re-simulating. Frames hold only the
//! renderable per-hex fields (~0.5 MB at subdivision 7), not the grid.

use genesis_biology::{BiologyLayer, BiologyState, flush_events_to_branch as flush_biology_events};
use genesis_civilization::{CivilizationLayer, CivilizationState};
use genesis_climate::{ClimateLayer, ClimateState, flush_events_to_branch as flush_climate_events};
use genesis_core::World;
use genesis_core::data::{
    BiomeId, ClimateRegimePlaceholder, Direction, HydroFlags, SoilClass, WaterBodyId, WorldData,
};
use genesis_core::lifecycle::{GenerationError, advance_with_coordinator_observed};
use genesis_core::parameters::{WorldParameters, WorldSeed};
use genesis_core::time::{TickCoordinator, WorldYear};
use genesis_hydrology::{
    HydrologyLayer, HydrologyState, flush_events_to_branch as flush_hydrology_events,
};
use genesis_tectonics::{
    TectonicsLayer, TectonicsState, flush_events_to_branch as flush_tectonic_events,
};

/// Memory budget for buffered history frames (Doc 05 §A). Advisory only —
/// scrub cadence is fixed at [`HISTORY_STRIDE_YEARS`], so long high-resolution
/// runs can exceed this budget.
pub const FRAME_MEMORY_BUDGET_BYTES: usize = 256 << 20;

/// Approximate bytes per cell in a [`HistoryFrame`] (Doc 08 §12.4 + polish fields).
const FRAME_BYTES_PER_CELL: usize = 40;

/// Fixed timeline scrub cadence: one history frame every 10 My.
///
/// Scrubbing and playback step by frame index; a constant stride keeps the
/// year jump identical at 1 By and 4.5 By (unlike the old
/// `target_year / max_frames` thinning).
pub const HISTORY_STRIDE_YEARS: i64 = 10_000_000;

/// Soft frame cap for a grid size from the memory budget (advisory / tests).
pub fn max_history_frames(cell_count: u32) -> usize {
    (FRAME_MEMORY_BUDGET_BYTES / (cell_count as usize * FRAME_BYTES_PER_CELL).max(1)).clamp(16, 256)
}

/// User-adjustable world configuration (the "recipe" surface of the setup menu).
#[derive(Clone, Debug)]
pub struct WorldGenConfig {
    /// Free-text world seed (hex/string); hashed to the engine seed via
    /// [`WorldSeed::from_string`], so any two distinct strings give distinct
    /// worlds.
    pub seed_text: String,
    pub subdivision_level: u8,
    pub target_year: i64,
    pub major_plates: u8,
    pub minor_plates: u8,
    /// Target continental crust coverage at formation (fraction of the
    /// sphere's area; ~0.29 is present-day Earth, 0.22 a Hadean world). Under
    /// the Doc 06-CAL calibration this seeds *where* continents form; the *amount*
    /// of dry land is set by `land_fraction`.
    pub continental_fraction: f32,
    /// Planetary water inventory in global-equivalent-layer meters (Doc 08 §3.1).
    pub water_inventory_gel_m: f32,
    /// Doc 06-CAL land coverage target (fraction of the sphere above sea level).
    /// Solved for exactly each tick.
    pub land_fraction: f32,
    /// Doc 06-CAL mountain intensity (height/fatness of the orogenic tail).
    pub orogeny_intensity: f32,
    /// Doc 06-CAL ocean-island seeding density.
    pub island_density: f32,
}

impl Default for WorldGenConfig {
    fn default() -> Self {
        let defaults = WorldParameters::default();
        Self {
            seed_text: "1a2b3c4d".to_string(),
            // The game runs at subdivision 8 (production resolution, Doc 04 §3.1).
            subdivision_level: 8,
            target_year: 1_000_000_000,
            major_plates: defaults.core.geology.initial_major_plate_count,
            minor_plates: defaults.core.geology.initial_minor_plate_count,
            continental_fraction: defaults.core.geology.initial_continental_fraction,
            water_inventory_gel_m: defaults.core.hydrology.water_inventory_gel_m,
            land_fraction: defaults.core.terrain.land_fraction,
            orogeny_intensity: defaults.core.terrain.orogeny_intensity,
            island_density: defaults.core.terrain.island_density,
        }
    }
}

impl WorldGenConfig {
    /// Builds validated engine parameters from the menu configuration.
    pub fn to_parameters(&self) -> WorldParameters {
        let mut params = WorldParameters::default();
        params.core.seed = WorldSeed::from_string(&self.seed_text);
        params.core.grid.subdivision_level = self.subdivision_level;
        params.core.geology.initial_major_plate_count = self.major_plates;
        params.core.geology.initial_minor_plate_count = self.minor_plates;
        params.core.geology.initial_continental_fraction = self.continental_fraction;
        params.core.hydrology.water_inventory_gel_m = self.water_inventory_gel_m;
        params.core.terrain.land_fraction = self.land_fraction;
        params.core.terrain.orogeny_intensity = self.orogeny_intensity;
        params.core.terrain.island_density = self.island_density;
        params
    }
}

/// Renderable per-hex fields captured at one simulated year.
#[derive(Clone, Debug)]
pub struct HistoryFrame {
    pub year: i64,
    pub sea_level_m: f32,
    pub elevation_mean: Vec<f32>,
    pub temperature_mean: Vec<f32>,
    pub precipitation: Vec<f32>,
    pub climate_regime: Vec<ClimateRegimePlaceholder>,
    /// Doc 08 §12.4 water fields.
    pub water_level_m: Vec<f32>,
    pub water_body_id: Vec<WaterBodyId>,
    pub river_discharge_m3_yr: Vec<f32>,
    pub hydro_flags: Vec<HydroFlags>,
    pub ice_mask: Vec<bool>,
    pub soil_fertility: Vec<f32>,
    pub soil_class: Vec<SoilClass>,
    pub flow_direction: Vec<Option<Direction>>,
    pub salt_accumulated: Vec<f32>,
    /// Prep-09 §10: biology render fields — kept **empty** (`len 0`) until Doc 09
    /// fills them, so the frame schema is Doc-09-ready without the memory cost of
    /// full zeroed vectors. `apply` is length-guarded like the water fields.
    pub biome: Vec<BiomeId>,
    pub biomass: Vec<f32>,
    pub biotic_richness: Vec<f32>,
}

impl HistoryFrame {
    pub fn capture(data: &WorldData) -> Self {
        // Display-only morphological de-speckle of the frame's render buffers
        // (never fed back into the live simulation): removes the single-hex
        // land/ocean spray so the scrubbed timeline shows coherent continents.
        let mut elevation_mean = data.elevation_mean.clone();
        let mut water_level_m = data.water_level_m.clone();
        genesis_tectonics::coast_cleanup::despeckle_display(
            &mut elevation_mean,
            &mut water_level_m,
            &data.grid,
            data.sea_level_m,
        );
        Self {
            year: data.current_year.value(),
            sea_level_m: data.sea_level_m,
            elevation_mean,
            temperature_mean: data.temperature_mean.clone(),
            precipitation: data.precipitation.clone(),
            climate_regime: data.climate_regime.clone(),
            water_level_m,
            water_body_id: data.water_body_id.clone(),
            river_discharge_m3_yr: data.river_discharge_m3_yr.clone(),
            hydro_flags: data.hydro_flags.clone(),
            ice_mask: data.ice_mask.clone(),
            soil_fertility: data.soil_fertility.clone(),
            soil_class: data.soil_class.clone(),
            flow_direction: data.flow_direction.clone(),
            salt_accumulated: data.salt_accumulated.clone(),
            // Doc 09 biology render fields (filled by the biology layer).
            biome: data.biome.clone(),
            biomass: data.biomass.clone(),
            biotic_richness: data.biotic_richness.clone(),
        }
    }

    /// Copies this frame's fields onto `data` for display. The grid and all
    /// non-rendered simulation fields are untouched.
    pub fn apply(&self, data: &mut WorldData) {
        data.current_year = WorldYear(self.year);
        data.sea_level_m = self.sea_level_m;
        data.elevation_mean.copy_from_slice(&self.elevation_mean);
        data.temperature_mean
            .copy_from_slice(&self.temperature_mean);
        data.precipitation.copy_from_slice(&self.precipitation);
        data.climate_regime.copy_from_slice(&self.climate_regime);
        if data.water_level_m.len() == self.water_level_m.len() {
            data.water_level_m.copy_from_slice(&self.water_level_m);
            if data.water_body_id.len() == self.water_body_id.len() {
                data.water_body_id.copy_from_slice(&self.water_body_id);
            }
            data.river_discharge_m3_yr
                .copy_from_slice(&self.river_discharge_m3_yr);
            data.hydro_flags.copy_from_slice(&self.hydro_flags);
            data.ice_mask.copy_from_slice(&self.ice_mask);
            data.soil_fertility.copy_from_slice(&self.soil_fertility);
            data.soil_class.copy_from_slice(&self.soil_class);
            data.flow_direction.copy_from_slice(&self.flow_direction);
            data.salt_accumulated
                .copy_from_slice(&self.salt_accumulated);
            // Biology fields are length-guarded (empty until Doc 09 fills them).
            if data.biome.len() == self.biome.len() {
                data.biome.copy_from_slice(&self.biome);
            }
            if data.biomass.len() == self.biomass.len() {
                data.biomass.copy_from_slice(&self.biomass);
            }
            if data.biotic_richness.len() == self.biotic_richness.len() {
                data.biotic_richness.copy_from_slice(&self.biotic_richness);
            }
        }
    }

}

/// Snapshot stride for a run: always [`HISTORY_STRIDE_YEARS`].
///
/// `target_year` and `cell_count` are kept for call-site compatibility; they
/// no longer change the cadence.
pub fn history_stride_years(_target_year: i64, _cell_count: u32) -> i64 {
    HISTORY_STRIDE_YEARS
}

/// Advances simulation to `target_year` with tectonics, climate, and hydrology
/// registered on the coordinator. `progress` fires after every tick with the
/// current world state.
///
/// Tectonics registers first, climate second (Doc 07 §13), hydrology third
/// (Doc 08 §2.1): climate sees updated terrain each tick, and hydrology sees
/// this tick's climate fields while tectonics reads the derived sea level one
/// tick lagged (Doc 08 §17.1).
pub fn generate_full_history(
    world: &mut World,
    tectonics: &mut TectonicsState,
    climate: &mut ClimateState,
    hydrology: &mut HydrologyState,
    target_year: WorldYear,
    mut progress: impl FnMut(&WorldData),
) -> Result<genesis_biology::Ledger, GenerationError> {
    let current = world.data.current_year;
    if target_year < current {
        return Err(GenerationError::TargetInPast {
            target: target_year.value(),
            current: current.value(),
        });
    }
    if target_year == current {
        return Ok(genesis_biology::Ledger::default());
    }

    let (tectonics_layer, tectonics_shared) = TectonicsLayer::attach(tectonics);
    let (climate_layer, climate_shared) = ClimateLayer::attach(climate);
    let (hydrology_layer, hydrology_shared) = HydrologyLayer::attach(hydrology);
    // Biology (Layer 1) is registered here — always-on, but dormant until P4-3
    // gives it a non-zero tick interval and real `advance` work (Doc 09 §3).
    // State is local for now; P4-3 plumbs it through to the caller for
    // branch-scoped persistence and event flushing.
    let mut biology = BiologyState::new();
    let (biology_layer, biology_shared) = BiologyLayer::attach(&mut biology);
    // Civilization (Doc 10) — registered last (reads finished geography/biology,
    // gates nothing). Dormant until sapience, then ticks at a human-scale cadence
    // (the multi-rate seam). State is local; the scaffold `advance` is inert.
    let mut civilization = CivilizationState::new();
    let (civilization_layer, civilization_shared) =
        CivilizationLayer::attach(&mut civilization);
    let mut coordinator = TickCoordinator::new();
    coordinator.add_layer(Box::new(tectonics_layer));
    coordinator.add_layer(Box::new(climate_layer));
    coordinator.add_layer(Box::new(hydrology_layer));
    coordinator.add_layer(Box::new(biology_layer));
    coordinator.add_layer(Box::new(civilization_layer));

    advance_with_coordinator_observed(world, &mut coordinator, target_year, |data| {
        progress(data);
    })?;
    drop(coordinator);

    *tectonics = TectonicsLayer::detach_state(tectonics_shared);
    *climate = ClimateLayer::detach_state(climate_shared);
    *hydrology = HydrologyLayer::detach_state(hydrology_shared);
    let _civilization = CivilizationLayer::detach_state(civilization_shared);
    // Recover biology state and flush its events to the branch log, like the
    // physical layers below. (State is local; caller-owned persistence is a
    // later slice.)
    let mut biology = BiologyLayer::detach_state(biology_shared);
    flush_biology_events(world, &mut biology);
    flush_tectonic_events(world, tectonics);
    flush_climate_events(world, climate);
    flush_hydrology_events(world, hydrology);

    // Final reconciliation (legacy path only): the last hydrology tick can dry a
    // deep endorheic basin to a salt flat at its tectonically-deepened bottom
    // with no following tectonic tick to fill it. Under the Doc 06-CAL calibration
    // this raw rebuild would overwrite the calibrated terrain with the raw
    // structure, so skip it — the last calibrated tick is authoritative.
    if !world.data.parameters.core.terrain.enabled {
        genesis_tectonics::basin_infill::finalize_dry_basins(
            &mut world.data,
            &mut tectonics.registry,
            &tectonics.projection,
        );
    }

    Ok(biology.into_ledger())
}

/// Events streamed from the generation thread to the UI (Doc 05 §A).
pub enum GenEvent {
    /// Coarse phase before per-tick progress exists (grid build, formation).
    Stage(&'static str),
    /// Per-tick progress (throttled).
    Progress { year: i64, target: i64 },
    /// Display copy of the freshly created world (year 0); the generation
    /// thread keeps the original and continues simulating.
    InitialWorld(Box<World>),
    /// A buffered history frame, in strictly increasing year order.
    Frame(Box<HistoryFrame>),
    /// The recorded tree of life, sent once generation completes so the viewer
    /// can build the real `BiologyView` adapter over it (Doc 09 §8.1).
    BiologyLedger(Box<genesis_biology::Ledger>),
    /// The real biology chronicle (life emerged, oxygenation, innovations) from
    /// the branch event log — for the timeline pips, with true years.
    LifeEvents(Vec<genesis_core::biology_view::LifeEventPip>),
    /// Generation finished; the last emitted frame is the final state.
    Done { final_year: i64 },
    /// Generation failed.
    Failed(String),
}

/// Converts the branch's biology events into timeline pips (Doc 09 §13).
fn life_event_pips(world: &World) -> Vec<genesis_core::biology_view::LifeEventPip> {
    use genesis_core::biology_view::{LifeEventCategory, LifeEventPip};
    use genesis_core::events::{EventKind, InnovationKind};
    world
        .branch_tree
        .root()
        .event_log
        .iter()
        .filter_map(|e| {
            let (label, category) = match &e.kind {
                EventKind::LifeEmerged { .. } => {
                    ("Life emerges".to_string(), LifeEventCategory::Origin)
                }
                EventKind::GreatOxygenation { .. } => (
                    "Great Oxygenation".to_string(),
                    LifeEventCategory::Milestone,
                ),
                EventKind::EvolutionaryInnovation { innovation } => {
                    let name = match innovation {
                        InnovationKind::OxygenicPhotosynthesis => "Oxygenic photosynthesis",
                        InnovationKind::Eukaryogenesis => "Eukaryogenesis",
                        InnovationKind::Multicellularity => "Multicellularity",
                        InnovationKind::LandColonization => "Land colonization",
                        InnovationKind::Flight => "Flight",
                        InnovationKind::Endothermy => "Endothermy",
                    };
                    (name.to_string(), LifeEventCategory::Innovation)
                }
                _ => return None,
            };
            Some(LifeEventPip {
                year: e.year.value(),
                label,
                category,
            })
        })
        .collect()
}

/// Runs a full generation from `config`, streaming [`GenEvent`]s as the world
/// is built: stage markers, the initial display world, history frames at the
/// stride from [`history_stride_years`], throttled progress, and completion.
/// The viewer can open on the first frame and buffer the rest like a video.
pub fn generate_world_streaming(config: &WorldGenConfig, mut emit: impl FnMut(GenEvent)) {
    let params = config.to_parameters();

    emit(GenEvent::Stage("building hex grid..."));
    let mut world = match genesis_core::create_world(params) {
        Ok(world) => world,
        Err(e) => {
            emit(GenEvent::Failed(format!("{e:?}")));
            return;
        }
    };
    emit(GenEvent::InitialWorld(Box::new(world.clone())));
    emit(GenEvent::Stage("running planetary formation..."));

    let mut tectonics = TectonicsState::new();
    let mut climate = ClimateState::new();
    let mut hydrology = HydrologyState::new();

    let target = config.target_year.max(1);
    let stride = history_stride_years(target, world.data.cell_count());
    let mut next_capture_year = 0_i64;
    let mut last_frame_year = -1_i64;
    let mut last_progress_year = -1_i64;
    let progress_step = (target / 200).max(1);

    let result = generate_full_history(
        &mut world,
        &mut tectonics,
        &mut climate,
        &mut hydrology,
        WorldYear(target),
        |data| {
            let year = data.current_year.value();
            if year >= next_capture_year {
                emit(GenEvent::Frame(Box::new(HistoryFrame::capture(data))));
                last_frame_year = year;
                next_capture_year = year + stride;
            }
            if year - last_progress_year >= progress_step {
                last_progress_year = year;
                emit(GenEvent::Progress { year, target });
            }
        },
    );

    match result {
        Ok(ledger) => {
            let final_year = world.data.current_year.value();
            if last_frame_year != final_year {
                emit(GenEvent::Frame(Box::new(HistoryFrame::capture(
                    &world.data,
                ))));
            }
            emit(GenEvent::LifeEvents(life_event_pips(&world)));
            emit(GenEvent::BiologyLedger(Box::new(ledger)));
            emit(GenEvent::Done { final_year });
        }
        Err(e) => emit(GenEvent::Failed(format!("{e:?}"))),
    }
}

/// A command sent from the UI to the persistent [`run_live_simulation`] worker.
pub enum SimCommand {
    /// Advance the live simulation to this absolute year and emit one real
    /// captured frame at the new state. Years at or below the current year are
    /// ignored (time only moves forward; backward scrubbing reads the buffer).
    AdvanceTo(i64),
}

/// The persistent simulation worker (Doc 05 live-stepping model). Unlike
/// [`generate_world_streaming`], which runs the whole history and drops the
/// simulation, this **keeps the tick coordinator and all four layer states
/// resident for the session** so the UI can hand-crank real ticks forward at
/// any granularity — no interpolation, every displayed state is one the engine
/// genuinely computed.
///
/// Lifecycle: build the world → attach the four layers into **one** coordinator
/// (built once, never rebuilt, so each layer's `last_tick_year` bookkeeping
/// stays valid) → run the initial advance to `config.target_year`, streaming
/// stride frames and progress exactly like the old generator → then block on
/// `commands`, advancing the *same* coordinator with
/// [`TickCoordinator::advance_resumable`] per [`SimCommand::AdvanceTo`] and
/// emitting one real frame each. The worker ends when the command channel
/// closes (the UI dropped its sender on teardown).
pub fn run_live_simulation(
    config: &WorldGenConfig,
    commands: std::sync::mpsc::Receiver<SimCommand>,
    mut emit: impl FnMut(GenEvent),
) {
    let params = config.to_parameters();

    emit(GenEvent::Stage("building hex grid..."));
    let mut world = match genesis_core::create_world(params) {
        Ok(world) => world,
        Err(e) => {
            emit(GenEvent::Failed(format!("{e:?}")));
            return;
        }
    };
    emit(GenEvent::InitialWorld(Box::new(world.clone())));
    emit(GenEvent::Stage("running planetary formation..."));

    // Attach every layer into one long-lived coordinator. The shared handles
    // let us read the biology ledger (for the tree/bestiary) and flush events
    // without detaching — detaching would reset the layers' timestep clocks.
    let mut tectonics = TectonicsState::new();
    let mut climate = ClimateState::new();
    let mut hydrology = HydrologyState::new();
    let mut biology = BiologyState::new();
    let mut civilization = CivilizationState::new();
    let (tectonics_layer, tectonics_shared) = TectonicsLayer::attach(&mut tectonics);
    let (climate_layer, climate_shared) = ClimateLayer::attach(&mut climate);
    let (hydrology_layer, hydrology_shared) = HydrologyLayer::attach(&mut hydrology);
    let (biology_layer, biology_shared) = BiologyLayer::attach(&mut biology);
    // Civ layer stays attached for the session like the others (its shared
    // handle would feed the UI once civ produces state); dormant until sapience.
    let (civilization_layer, _civilization_shared) =
        CivilizationLayer::attach(&mut civilization);
    let mut coordinator = TickCoordinator::new();
    coordinator.add_layer(Box::new(tectonics_layer));
    coordinator.add_layer(Box::new(climate_layer));
    coordinator.add_layer(Box::new(hydrology_layer));
    coordinator.add_layer(Box::new(biology_layer));
    coordinator.add_layer(Box::new(civilization_layer));

    let params = world.data.parameters.clone();
    let stride = history_stride_years(config.target_year, world.data.cell_count());
    let initial_target = config.target_year.max(1);

    // Initial run to the target year: same stride-frame + progress stream as the
    // legacy generator, but on the resumable coordinator so stepping continues
    // seamlessly afterward.
    {
        let mut next_capture_year = 0_i64;
        let mut last_progress_year = -1_i64;
        let progress_step = (initial_target / 200).max(1);
        let mut on_tick = |data: &WorldData| {
            let year = data.current_year.value();
            if year >= next_capture_year {
                emit(GenEvent::Frame(Box::new(HistoryFrame::capture(data))));
                next_capture_year = year + stride;
            }
            if year - last_progress_year >= progress_step {
                last_progress_year = year;
                emit(GenEvent::Progress {
                    year,
                    target: initial_target,
                });
            }
        };
        coordinator.advance_resumable_with(
            WorldYear(initial_target),
            &mut world.data,
            &world.rng,
            &params,
            &mut on_tick,
        );
    }

    // Final frame of the initial run + biology view + completion, then go live.
    emit(GenEvent::Frame(Box::new(HistoryFrame::capture(&world.data))));
    flush_live_events(
        &mut world,
        &tectonics_shared,
        &climate_shared,
        &hydrology_shared,
        &biology_shared,
    );
    emit(GenEvent::LifeEvents(life_event_pips(&world)));
    emit(GenEvent::BiologyLedger(Box::new(
        biology_shared.borrow().ledger().clone(),
    )));
    emit(GenEvent::Done {
        final_year: world.data.current_year.value(),
    });

    // Live stepping: advance the resident coordinator on demand. Each command
    // yields exactly the ticks a single run to that year would (proven by
    // `resumable_stepping_equals_a_single_run`), and one real captured frame.
    while let Ok(command) = commands.recv() {
        match command {
            SimCommand::AdvanceTo(year) => {
                if year <= world.data.current_year.value() {
                    continue;
                }
                coordinator.advance_resumable(
                    WorldYear(year),
                    &mut world.data,
                    &world.rng,
                    &params,
                );
                flush_live_events(
                    &mut world,
                    &tectonics_shared,
                    &climate_shared,
                    &hydrology_shared,
                    &biology_shared,
                );
                emit(GenEvent::Frame(Box::new(HistoryFrame::capture(&world.data))));
                emit(GenEvent::LifeEvents(life_event_pips(&world)));
                emit(GenEvent::BiologyLedger(Box::new(
                    biology_shared.borrow().ledger().clone(),
                )));
            }
        }
    }
}

/// Flushes each layer's pending events into the world's branch log, reading the
/// live states through their shared handles (no detach). Keeps the emitted
/// life-event pips and branch history current as the sim steps.
fn flush_live_events(
    world: &mut World,
    tectonics: &std::rc::Rc<std::cell::RefCell<TectonicsState>>,
    climate: &std::rc::Rc<std::cell::RefCell<ClimateState>>,
    hydrology: &std::rc::Rc<std::cell::RefCell<HydrologyState>>,
    biology: &std::rc::Rc<std::cell::RefCell<BiologyState>>,
) {
    flush_tectonic_events(world, &mut tectonics.borrow_mut());
    flush_climate_events(world, &mut climate.borrow_mut());
    flush_hydrology_events(world, &mut hydrology.borrow_mut());
    flush_biology_events(world, &mut biology.borrow_mut());
}

/// Blocking wrapper collecting the stream into `(final world equivalent, frames)`.
/// Kept for the headless path and tests; the final display state equals the
/// last frame applied onto the initial world.
pub fn generate_world_with_history(
    config: &WorldGenConfig,
    mut on_progress: impl FnMut(i64, i64),
) -> Result<(World, Vec<HistoryFrame>), String> {
    let mut initial: Option<Box<World>> = None;
    let mut frames: Vec<HistoryFrame> = Vec::new();
    let mut failure: Option<String> = None;

    generate_world_streaming(config, |event| match event {
        GenEvent::InitialWorld(world) => initial = Some(world),
        GenEvent::Frame(frame) => frames.push(*frame),
        GenEvent::Progress { year, target } => on_progress(year, target),
        GenEvent::Failed(e) => failure = Some(e),
        GenEvent::Stage(_)
        | GenEvent::Done { .. }
        | GenEvent::BiologyLedger(_)
        | GenEvent::LifeEvents(_) => {}
    });

    if let Some(e) = failure {
        return Err(e);
    }
    let mut world = *initial.ok_or_else(|| "generation produced no world".to_string())?;
    if let Some(last) = frames.last() {
        last.apply(&mut world.data);
    }
    Ok((world, frames))
}

#[cfg(test)]
mod live_tests {
    use super::*;

    fn small_config(target_year: i64) -> WorldGenConfig {
        let mut config = WorldGenConfig::default();
        config.seed_text = "live-step-test".to_string();
        config.subdivision_level = 5;
        config.target_year = target_year;
        config
    }

    /// The core live-stepping guarantee: driving the worker to the initial
    /// target and then commanding it forward tick-by-tick produces **exactly**
    /// the state a single generation to the final year produces. No
    /// interpolation, fully deterministic — the displayed states are real.
    #[test]
    fn live_stepping_matches_one_shot_generation() {
        // Worker: generate to 20 My, then step to 30 My and 40 My via commands.
        let (tx, rx) = std::sync::mpsc::channel::<SimCommand>();
        tx.send(SimCommand::AdvanceTo(30_000_000)).unwrap();
        tx.send(SimCommand::AdvanceTo(40_000_000)).unwrap();
        drop(tx); // closes the channel so the worker exits after both commands

        let mut frames: Vec<HistoryFrame> = Vec::new();
        run_live_simulation(&small_config(20_000_000), rx, |event| {
            if let GenEvent::Frame(frame) = event {
                frames.push(*frame);
            }
        });
        let live_final = frames.last().expect("worker emitted frames").clone();
        assert_eq!(live_final.year, 40_000_000, "stepped to the commanded year");

        // One-shot: a single generation straight to 40 My, same seed.
        let (_world, oneshot_frames) =
            generate_world_with_history(&small_config(40_000_000), |_, _| {}).expect("gen");
        let oneshot_final = oneshot_frames.last().expect("one-shot frames");
        assert_eq!(oneshot_final.year, 40_000_000);

        // Every real per-hex field must match bit-for-bit.
        assert_eq!(
            live_final.elevation_mean, oneshot_final.elevation_mean,
            "stepped terrain must equal the one-shot run's terrain"
        );
        assert_eq!(live_final.sea_level_m, oneshot_final.sea_level_m);
        assert_eq!(live_final.water_body_id, oneshot_final.water_body_id);
        assert_eq!(live_final.temperature_mean, oneshot_final.temperature_mean);
    }

    /// Commands that do not move time forward are ignored (backward scrubbing is
    /// the buffer's job); the worker still terminates when the channel closes.
    #[test]
    fn backward_commands_are_ignored() {
        let (tx, rx) = std::sync::mpsc::channel::<SimCommand>();
        tx.send(SimCommand::AdvanceTo(10_000_000)).unwrap(); // below the target
        tx.send(SimCommand::AdvanceTo(25_000_000)).unwrap(); // forward
        drop(tx);

        let mut years: Vec<i64> = Vec::new();
        run_live_simulation(&small_config(20_000_000), rx, |event| {
            if let GenEvent::Frame(frame) = event {
                years.push(frame.year);
            }
        });
        assert_eq!(*years.last().unwrap(), 25_000_000, "only the forward step lands");
        assert!(
            years.iter().all(|&y| y <= 25_000_000),
            "no frame past the last real command"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffered_generation_produces_bounded_ordered_frames() {
        let config = WorldGenConfig {
            seed_text: "42".to_string(),
            subdivision_level: 5,
            target_year: 50_000_000,
            ..WorldGenConfig::default()
        };
        let mut reports = 0;
        let (world, frames) =
            generate_world_with_history(&config, |_, _| reports += 1).expect("generation");

        assert!(reports > 0, "progress must fire");
        assert!(!frames.is_empty());
        assert!(
            frames.windows(2).all(|w| w[0].year < w[1].year),
            "frames strictly ordered"
        );
        for window in frames.windows(2) {
            let gap = window[1].year - window[0].year;
            assert!(
                gap <= HISTORY_STRIDE_YEARS,
                "scrub gap {gap} exceeds {} My stride",
                HISTORY_STRIDE_YEARS / 1_000_000
            );
        }
        assert_eq!(
            frames.last().unwrap().year,
            world.data.current_year.value(),
            "last frame is the final state"
        );
        let n = world.data.cell_count() as usize;
        assert_eq!(frames[0].elevation_mean.len(), n);
    }


    fn history_stride_is_constant_across_targets_and_grid_sizes() {
        assert_eq!(
            history_stride_years(1_000_000_000, 10_242),
            HISTORY_STRIDE_YEARS
        );
        assert_eq!(
            history_stride_years(4_500_000_000, 10_242),
            HISTORY_STRIDE_YEARS
        );
        assert_eq!(
            history_stride_years(4_500_000_000, 655_362),
            HISTORY_STRIDE_YEARS
        );
    }

    #[test]
    fn frame_apply_round_trips_render_fields() {
        let config = WorldGenConfig {
            seed_text: "7".to_string(),
            subdivision_level: 5,
            target_year: 2_000_000,
            ..WorldGenConfig::default()
        };
        let (mut world, frames) =
            generate_world_with_history(&config, |_, _| {}).expect("generation");
        let first = frames.first().unwrap().clone();
        let last = frames.last().unwrap().clone();

        first.apply(&mut world.data);
        assert_eq!(world.data.current_year.value(), first.year);
        assert_eq!(world.data.elevation_mean, first.elevation_mean);

        last.apply(&mut world.data);
        assert_eq!(world.data.elevation_mean, last.elevation_mean);
        assert_eq!(world.data.water_body_id, last.water_body_id);
        assert_eq!(world.data.water_level_m, last.water_level_m);
    }

    /// Scrubbing must not paint oceans as salt flats when body ids are unset.
    #[test]
    fn scrubbed_frame_keeps_wet_hexes_blue_under_salt() {
        use bevy::prelude::ColorToComponents;
        use genesis_core::data::WaterBodyId;
        use genesis_render::{RenderMode, hex_color_for_mode};

        let config = WorldGenConfig {
            seed_text: "0".to_string(),
            subdivision_level: 5,
            // Past condensation onset (~215 My); oceans must be present for the scrub check.
            target_year: 300_000_000,
            ..WorldGenConfig::default()
        };
        let (mut world, frames) =
            generate_world_with_history(&config, |_, _| {}).expect("generation");
        let frame = frames.last().expect("frame");
        // Simulate pre-fix scrub: hydro fields restored, body ids left NONE.
        frame.apply(&mut world.data);
        for id in &mut world.data.water_body_id {
            *id = WaterBodyId::NONE;
        }

        let n = world.data.cell_count() as usize;
        let mut wet = 0u32;
        let mut water_colored = 0u32;
        for i in 0..n {
            let elev = world.data.elevation_mean[i];
            let water = world.data.water_level_m[i];
            if !(water.is_finite() && water > elev) {
                continue;
            }
            wet += 1;
            let rgb = {
                let c = hex_color_for_mode(&world.data, i, RenderMode::Elevation, false, None);
                c.to_srgba().to_f32_array_no_alpha()
            };
            // The scrub bug rendered wet hexes as tan dry land (red-dominant).
            // Water reads blue or, on the calibrated shallow shelf, cyan
            // (blue ≈ green) — either way blue clearly beats red. Guard against
            // the tan bug, not against shallow-water cyan.
            if rgb[2] > rgb[0] + 0.05 && rgb[2] >= rgb[1] - 0.02 {
                water_colored += 1;
            }
        }
        assert!(wet > 0, "expected standing water at 300M");
        assert_eq!(
            water_colored, wet,
            "every wet hex must render as water (blue/cyan), not tan, after scrub (wet={wet} water_colored={water_colored})"
        );
    }

    /// Manual diagnostic: HistoryFrame wetting across the timeline (ocean-tan bug).
    #[test]
    #[ignore = "manual ocean-tan HistoryFrame wetting diagnostic"]
    fn diagnose_history_frame_wetting() {
        let config = WorldGenConfig {
            seed_text: "0".to_string(),
            subdivision_level: 5,
            target_year: 1_000_000_000,
            ..WorldGenConfig::default()
        };
        let (_, frames) = generate_world_with_history(&config, |_, _| {}).expect("generation");
        println!("frames={}", frames.len());
        for frame in &frames {
            let n = frame.elevation_mean.len().max(1);
            let mut land = 0u32;
            let mut wet = 0u32;
            let mut wet_with_salt = 0u32;
            for i in 0..n {
                let elev = frame.elevation_mean[i];
                let water = frame.water_level_m[i];
                if elev > frame.sea_level_m {
                    land += 1;
                }
                let is_wet = water.is_finite() && water > elev;
                if is_wet {
                    wet += 1;
                    if frame.salt_accumulated[i] > 0.0 {
                        wet_with_salt += 1;
                    }
                }
            }
            if frame.year == 0 || frame.year % 200_000_000 == 0 || frame.year == 864_000_000 {
                println!(
                    "year={} land={:.1}% wet={:.1}% wet_with_salt={:.1}%",
                    frame.year,
                    100.0 * land as f64 / n as f64,
                    100.0 * wet as f64 / n as f64,
                    100.0 * wet_with_salt as f64 / n as f64
                );
            }
        }
    }

    #[test]
    fn config_respects_seed() {
        let a = WorldGenConfig {
            seed_text: "1".to_string(),
            subdivision_level: 5,
            target_year: 1_000_000,
            ..WorldGenConfig::default()
        };
        let b = WorldGenConfig {
            seed_text: "2".to_string(),
            ..a.clone()
        };
        let (world_a, _) = generate_world_with_history(&a, |_, _| {}).expect("a");
        let (world_a2, _) = generate_world_with_history(&a, |_, _| {}).expect("a2");
        let (world_b, _) = generate_world_with_history(&b, |_, _| {}).expect("b");

        assert_eq!(
            world_a.data.elevation_mean, world_a2.data.elevation_mean,
            "same seed, same world"
        );
        assert_ne!(
            world_a.data.elevation_mean, world_b.data.elevation_mean,
            "different seed, different world"
        );
    }

    /// End-to-end: life emerges and climbs the innovation ladder in a full run
    /// (Doc 09 §3). Uses `generate_full_history` directly because the streaming
    /// wrapper returns the year-0 clone + render frames, whose event log is
    /// empty — the chronicle lives on the simulated world (surfacing biology
    /// events to the viewer is separate event-stream plumbing). Ignored (a 1-By
    /// generation); run:
    /// `cargo test -p genesis_ui life_emerges_in_a_full_run -- --ignored --nocapture`.
    #[test]
    #[ignore = "full 1-By generation; run explicitly to watch life emerge"]
    fn life_emerges_in_a_full_run() {
        use genesis_climate::ClimateState;
        use genesis_core::create_world;
        use genesis_core::events::EventKind;
        use genesis_hydrology::HydrologyState;
        use genesis_tectonics::TectonicsState;

        let config = WorldGenConfig {
            seed_text: "genesis".to_string(),
            subdivision_level: 5,
            target_year: 1_000_000_000,
            ..WorldGenConfig::default()
        };
        let mut world = create_world(config.to_parameters()).expect("world");
        let mut tectonics = TectonicsState::new();
        let mut climate = ClimateState::new();
        let mut hydrology = HydrologyState::new();
        generate_full_history(
            &mut world,
            &mut tectonics,
            &mut climate,
            &mut hydrology,
            WorldYear(1_000_000_000),
            |_| {},
        )
        .expect("generation");

        let log = &world.branch_tree.root().event_log;
        let life = log
            .iter()
            .find(|e| matches!(e.kind, EventKind::LifeEmerged { .. }))
            .expect("life should emerge within 1 By");
        println!(
            "LIFE EMERGED at {} My ({:?})",
            life.year.value() / 1_000_000,
            life.location
        );
        for e in log.iter() {
            match &e.kind {
                EventKind::GreatOxygenation { o2_fraction } => println!(
                    "  Great Oxygenation at {} My (O2 {o2_fraction:.2})",
                    e.year.value() / 1_000_000
                ),
                EventKind::EvolutionaryInnovation { innovation } => {
                    println!("  {innovation:?} at {} My", e.year.value() / 1_000_000)
                }
                _ => {}
            }
        }
        let innovations = log
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    EventKind::EvolutionaryInnovation { .. } | EventKind::GreatOxygenation { .. }
                )
            })
            .count();
        assert!(
            innovations >= 1,
            "at least one innovation should fire by 1 By"
        );

        // Timeline pips now use the *real* chronicle years (limitations 2 + 15).
        let pips = life_event_pips(&world);
        println!("real life-event pips: {}", pips.len());
        assert!(
            pips.iter()
                .any(|p| p.label == "Life emerges" && p.year == life.year.value()),
            "the 'Life emerges' pip must carry the true simulated emergence year"
        );
    }

    /// End-to-end: after a full run, the biology fields are populated and the
    /// real `GeneratedBiologyView` adapter reads them into biomes / biomass /
    /// species / a tree. Run:
    /// `cargo test -p genesis_ui biology_on_the_map -- --ignored --nocapture`.
    #[test]
    #[ignore = "full 1-By generation; shows what the viewer will render"]
    fn biology_on_the_map() {
        use genesis_biology::GeneratedBiologyView;
        use genesis_climate::ClimateState;
        use genesis_core::HexId;
        use genesis_core::biology_view::BiologyView;
        use genesis_core::create_world;
        use genesis_core::data::BiomeId;
        use genesis_hydrology::HydrologyState;
        use genesis_tectonics::TectonicsState;

        let config = WorldGenConfig {
            seed_text: "genesis".to_string(),
            subdivision_level: 6,
            target_year: 1_000_000_000,
            ..WorldGenConfig::default()
        };
        let mut world = create_world(config.to_parameters()).expect("world");
        let seed = world.data.parameters.core.seed.value;
        let (mut t, mut c, mut h) = (
            TectonicsState::new(),
            ClimateState::new(),
            HydrologyState::new(),
        );
        let ledger = generate_full_history(
            &mut world,
            &mut t,
            &mut c,
            &mut h,
            WorldYear(1_000_000_000),
            |_| {},
        )
        .expect("gen");
        println!("ledger lineages: {}", ledger.len());

        let d = &world.data;
        let n = d.cell_count() as usize;
        // Biome distribution.
        let mut counts: std::collections::BTreeMap<u16, usize> = std::collections::BTreeMap::new();
        let (mut life_hexes, mut max_biomass, mut max_r) = (0, 0.0f32, 0.0f32);
        for i in 0..n {
            *counts.entry(d.biome[i].0).or_default() += 1;
            if d.biome[i] != BiomeId::NONE && d.biome[i].0 != 12 {
                life_hexes += 1;
            }
            max_biomass = max_biomass.max(d.biomass[i]);
            max_r = max_r.max(d.biotic_richness[i]);
        }
        let view = GeneratedBiologyView::with_ledger(seed, ledger);
        println!(
            "biomes present: {} kinds; life-bearing hexes: {life_hexes}/{n}",
            counts.len()
        );
        for (b, ct) in &counts {
            println!("  {} : {ct}", view.biome_name(BiomeId(*b)));
        }
        println!("max biomass {max_biomass:.0}, max richness {max_r:.2}");

        // A sample assemblage from the richest hex.
        let richest = (0..n)
            .max_by(|&a, &b| d.biotic_richness[a].total_cmp(&d.biotic_richness[b]))
            .unwrap();
        let a = view.assemblage(d, HexId(richest as u32));
        println!(
            "richest hex {richest}: {} (R {:.2}), {} guilds, {} species e.g. {:?}",
            a.biome_name,
            a.richness,
            a.occupied_guilds,
            a.species.len(),
            a.species.first().map(|s| &s.name)
        );
        let tree = view.tree_snapshot(WorldYear(1_000_000_000));
        println!("tree nodes at 1 By: {}", tree.nodes.len());

        assert!(life_hexes > 0, "some hexes should be life-bearing");
        assert!(
            max_biomass > 0.0 && max_r > 0.0,
            "biomass/richness fields populated"
        );
        assert!(!a.species.is_empty(), "richest hex should generate species");
        assert!(tree.nodes.len() > 1, "tree should have grown");
    }
}
