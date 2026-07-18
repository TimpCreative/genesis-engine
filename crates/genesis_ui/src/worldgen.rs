//! World generation entry point and history buffering for the interactive app.
//!
//! Owns the layer registration order (tectonics → climate, Doc 07 §13) and
//! captures lightweight [`HistoryFrame`]s during generation so the viewer can
//! scrub the timeline without re-simulating. Frames hold only the renderable
//! per-hex fields (~0.5 MB at subdivision 7), not the grid. Surface water
//! (hydrology) is out of the game until Doc 08.

use genesis_climate::{ClimateLayer, ClimateState, flush_events_to_branch as flush_climate_events};
use genesis_core::World;
use genesis_core::data::{ClimateRegimePlaceholder, WorldData};
use genesis_core::lifecycle::{GenerationError, advance_with_coordinator_observed};
use genesis_core::parameters::{WorldParameters, WorldSeed};
use genesis_core::time::{TickCoordinator, WorldYear};
use genesis_tectonics::{
    TectonicsLayer, TectonicsState, flush_events_to_branch as flush_tectonic_events,
};

/// Memory budget for buffered history frames (Doc 05 §A).
pub const FRAME_MEMORY_BUDGET_BYTES: usize = 256 << 20;

/// Approximate bytes per cell in a [`HistoryFrame`]: 3 × f32 fields + regime.
const FRAME_BYTES_PER_CELL: usize = 13;

/// Frame cap for a grid size, from the memory budget.
pub fn max_history_frames(cell_count: u32) -> usize {
    (FRAME_MEMORY_BUDGET_BYTES / (cell_count as usize * FRAME_BYTES_PER_CELL).max(1)).clamp(16, 256)
}

/// User-adjustable world configuration (the "recipe" surface of the setup menu).
#[derive(Clone, Debug)]
pub struct WorldGenConfig {
    pub seed: u64,
    pub subdivision_level: u8,
    pub target_year: i64,
    pub major_plates: u8,
    pub minor_plates: u8,
    /// Target continental crust coverage at formation (fraction of the
    /// sphere's area; ~0.29 is present-day Earth, 0.22 a Hadean world).
    pub continental_fraction: f32,
}

impl Default for WorldGenConfig {
    fn default() -> Self {
        let defaults = WorldParameters::default();
        Self {
            seed: defaults.core.seed.value,
            subdivision_level: 6,
            target_year: 1_000_000_000,
            major_plates: defaults.core.geology.initial_major_plate_count,
            minor_plates: defaults.core.geology.initial_minor_plate_count,
            continental_fraction: defaults.core.geology.initial_continental_fraction,
        }
    }
}

impl WorldGenConfig {
    /// Builds validated engine parameters from the menu configuration.
    pub fn to_parameters(&self) -> WorldParameters {
        let mut params = WorldParameters::default();
        params.core.seed = WorldSeed::from_integer(self.seed);
        params.core.grid.subdivision_level = self.subdivision_level;
        params.core.geology.initial_major_plate_count = self.major_plates;
        params.core.geology.initial_minor_plate_count = self.minor_plates;
        params.core.geology.initial_continental_fraction = self.continental_fraction;
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
}

impl HistoryFrame {
    pub fn capture(data: &WorldData) -> Self {
        Self {
            year: data.current_year.value(),
            sea_level_m: data.sea_level_m,
            elevation_mean: data.elevation_mean.clone(),
            temperature_mean: data.temperature_mean.clone(),
            precipitation: data.precipitation.clone(),
            climate_regime: data.climate_regime.clone(),
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
    }
}

/// Snapshot stride for a run: budgeted frame count, at least one Geological tick.
pub fn history_stride_years(target_year: i64, cell_count: u32) -> i64 {
    (target_year / max_history_frames(cell_count) as i64).max(500_000)
}

/// Advances simulation to `target_year` with tectonics and climate registered
/// on the coordinator. `progress` fires after every tick with the current world
/// state. (Hydrology is removed until Doc 08.)
///
/// Tectonics registers first; climate second (Doc 07 §13) so climate sees
/// updated terrain each tick.
pub fn generate_full_history(
    world: &mut World,
    tectonics: &mut TectonicsState,
    climate: &mut ClimateState,
    target_year: WorldYear,
    mut progress: impl FnMut(&WorldData),
) -> Result<(), GenerationError> {
    let current = world.data.current_year;
    if target_year < current {
        return Err(GenerationError::TargetInPast {
            target: target_year.value(),
            current: current.value(),
        });
    }
    if target_year == current {
        return Ok(());
    }

    let (tectonics_layer, tectonics_shared) = TectonicsLayer::attach(tectonics);
    let (climate_layer, climate_shared) = ClimateLayer::attach(climate);
    let mut coordinator = TickCoordinator::new();
    coordinator.add_layer(Box::new(tectonics_layer));
    coordinator.add_layer(Box::new(climate_layer));

    advance_with_coordinator_observed(world, &mut coordinator, target_year, |data| {
        progress(data);
    })?;
    drop(coordinator);

    *tectonics = TectonicsLayer::detach_state(tectonics_shared);
    *climate = ClimateLayer::detach_state(climate_shared);
    flush_tectonic_events(world, tectonics);
    flush_climate_events(world, climate);

    Ok(())
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
    /// Generation finished; the last emitted frame is the final state.
    Done { final_year: i64 },
    /// Generation failed.
    Failed(String),
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
        Ok(()) => {
            let final_year = world.data.current_year.value();
            if last_frame_year != final_year {
                emit(GenEvent::Frame(Box::new(HistoryFrame::capture(
                    &world.data,
                ))));
            }
            emit(GenEvent::Done { final_year });
        }
        Err(e) => emit(GenEvent::Failed(format!("{e:?}"))),
    }
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
        GenEvent::Stage(_) | GenEvent::Done { .. } => {}
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
mod tests {
    use super::*;

    #[test]
    fn buffered_generation_produces_bounded_ordered_frames() {
        let config = WorldGenConfig {
            seed: 42,
            subdivision_level: 5,
            target_year: 50_000_000,
            ..WorldGenConfig::default()
        };
        let mut reports = 0;
        let (world, frames) =
            generate_world_with_history(&config, |_, _| reports += 1).expect("generation");

        assert!(reports > 0, "progress must fire");
        assert!(!frames.is_empty());
        let cap = max_history_frames(world.data.cell_count());
        assert!(frames.len() <= cap + 2, "{} > {cap}", frames.len());
        assert!(
            frames.windows(2).all(|w| w[0].year < w[1].year),
            "frames strictly ordered"
        );
        assert_eq!(
            frames.last().unwrap().year,
            world.data.current_year.value(),
            "last frame is the final state"
        );
        let n = world.data.cell_count() as usize;
        assert_eq!(frames[0].elevation_mean.len(), n);
    }

    #[test]
    fn frame_apply_round_trips_render_fields() {
        let config = WorldGenConfig {
            seed: 7,
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
    }

    #[test]
    fn config_respects_seed() {
        let a = WorldGenConfig {
            seed: 1,
            subdivision_level: 5,
            target_year: 1_000_000,
            ..WorldGenConfig::default()
        };
        let b = WorldGenConfig {
            seed: 2,
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
}
