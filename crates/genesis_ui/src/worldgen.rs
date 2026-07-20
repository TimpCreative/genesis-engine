//! World generation entry point and history buffering for the interactive app.
//!
//! Owns the layer registration order (tectonics → climate → hydrology, Doc 08
//! §2.1) and captures lightweight [`HistoryFrame`]s during generation so the
//! viewer can scrub the timeline without re-simulating. Frames hold only the
//! renderable per-hex fields (~0.5 MB at subdivision 7), not the grid.

use genesis_climate::{ClimateLayer, ClimateState, flush_events_to_branch as flush_climate_events};
use genesis_core::World;
use genesis_core::data::{
    ClimateRegimePlaceholder, Direction, HydroFlags, SoilClass, WaterBodyId, WorldData,
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
    pub seed: u64,
    pub subdivision_level: u8,
    pub target_year: i64,
    pub major_plates: u8,
    pub minor_plates: u8,
    /// Target continental crust coverage at formation (fraction of the
    /// sphere's area; ~0.29 is present-day Earth, 0.22 a Hadean world). Under
    /// the Doc 10 calibration this seeds *where* continents form; the *amount*
    /// of dry land is set by `land_fraction`.
    pub continental_fraction: f32,
    /// Planetary water inventory in global-equivalent-layer meters (Doc 08 §3.1).
    pub water_inventory_gel_m: f32,
    /// Doc 10 land coverage target (fraction of the sphere above sea level).
    /// Solved for exactly each tick.
    pub land_fraction: f32,
    /// Doc 10 mountain intensity (height/fatness of the orogenic tail).
    pub orogeny_intensity: f32,
    /// Doc 10 ocean-island seeding density.
    pub island_density: f32,
}

impl Default for WorldGenConfig {
    fn default() -> Self {
        let defaults = WorldParameters::default();
        Self {
            seed: defaults.core.seed.value,
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
        params.core.seed = WorldSeed::from_integer(self.seed);
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
    let (hydrology_layer, hydrology_shared) = HydrologyLayer::attach(hydrology);
    let mut coordinator = TickCoordinator::new();
    coordinator.add_layer(Box::new(tectonics_layer));
    coordinator.add_layer(Box::new(climate_layer));
    coordinator.add_layer(Box::new(hydrology_layer));

    advance_with_coordinator_observed(world, &mut coordinator, target_year, |data| {
        progress(data);
    })?;
    drop(coordinator);

    *tectonics = TectonicsLayer::detach_state(tectonics_shared);
    *climate = ClimateLayer::detach_state(climate_shared);
    *hydrology = HydrologyLayer::detach_state(hydrology_shared);
    flush_tectonic_events(world, tectonics);
    flush_climate_events(world, climate);
    flush_hydrology_events(world, hydrology);

    // Final reconciliation (legacy path only): the last hydrology tick can dry a
    // deep endorheic basin to a salt flat at its tectonically-deepened bottom
    // with no following tectonic tick to fill it. Under the Doc 10 calibration
    // this raw rebuild would overwrite the calibrated terrain with the raw
    // structure, so skip it — the last calibrated tick is authoritative.
    if !world.data.parameters.core.terrain.enabled {
        genesis_tectonics::basin_infill::finalize_dry_basins(
            &mut world.data,
            &mut tectonics.registry,
            &tectonics.projection,
        );
    }

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

    #[test]
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
            seed: 0,
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
                let c = hex_color_for_mode(&world.data, i, RenderMode::Elevation, false);
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
            seed: 0,
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
