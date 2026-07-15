//! World generation entry point and history buffering for the interactive app.
//!
//! Owns the layer registration order (tectonics → climate → hydrology, Doc 07
//! §13) and captures lightweight [`HistoryFrame`]s during generation so the
//! viewer can scrub the timeline without re-simulating. Frames hold only the
//! renderable per-hex fields (~0.5 MB at subdivision 7), not the grid.

use genesis_climate::{ClimateLayer, ClimateState, flush_events_to_branch as flush_climate_events};
use genesis_core::World;
use genesis_core::data::{ClimateRegimePlaceholder, WorldData};
use genesis_core::lifecycle::{GenerationError, advance_with_coordinator_observed};
use genesis_core::parameters::{WorldParameters, WorldSeed};
use genesis_core::time::{TickCoordinator, WorldYear};
use genesis_hydrology::HydrologyLayer;
use genesis_tectonics::{
    TectonicsLayer, TectonicsState, flush_events_to_branch as flush_tectonic_events,
};

/// Maximum buffered history frames per generation. Bounds viewer memory at
/// ~32 MB regardless of target year (Doc 05 snapshot-interval decision).
pub const MAX_HISTORY_FRAMES: usize = 64;

/// User-adjustable world configuration (the "recipe" surface of the setup menu).
#[derive(Clone, Debug)]
pub struct WorldGenConfig {
    pub seed: u64,
    pub subdivision_level: u8,
    pub target_year: i64,
    pub major_plates: u8,
    pub minor_plates: u8,
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
    pub flow_volume: Vec<f32>,
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
            flow_volume: data.flow_volume.clone(),
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
        data.flow_volume.copy_from_slice(&self.flow_volume);
    }
}

/// Snapshot stride for a run: capped frame count, at least one Geological tick.
pub fn history_stride_years(target_year: i64) -> i64 {
    (target_year / MAX_HISTORY_FRAMES as i64).max(500_000)
}

/// Advances simulation to `target_year` with tectonics, climate, and hydrology
/// registered on the coordinator. `progress` fires after every tick with the
/// current world state.
///
/// Tectonics registers first; climate second (Doc 07 §13) so climate sees
/// updated terrain each tick; hydrology third so flow reflects both.
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
    coordinator.add_layer(Box::new(HydrologyLayer));

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

/// Runs a full generation from `config`, buffering history frames at the
/// stride from [`history_stride_years`]. Returns the finished world and its
/// timeline (first frame at the first tick, last frame at `target_year`).
pub fn generate_world_with_history(
    config: &WorldGenConfig,
    mut on_progress: impl FnMut(i64, i64),
) -> Result<(World, Vec<HistoryFrame>), String> {
    let params = config.to_parameters();
    let mut world = genesis_core::create_world(params).map_err(|e| format!("{e:?}"))?;
    let mut tectonics = TectonicsState::new();
    let mut climate = ClimateState::new();

    let target = config.target_year.max(1);
    let stride = history_stride_years(target);
    let mut frames: Vec<HistoryFrame> = Vec::new();
    let mut next_capture_year = 0_i64;

    generate_full_history(
        &mut world,
        &mut tectonics,
        &mut climate,
        WorldYear(target),
        |data| {
            let year = data.current_year.value();
            if year >= next_capture_year {
                frames.push(HistoryFrame::capture(data));
                next_capture_year = year + stride;
            }
            on_progress(year, target);
        },
    )
    .map_err(|e| format!("{e:?}"))?;

    match frames.last() {
        Some(last) if last.year == world.data.current_year.value() => {}
        _ => frames.push(HistoryFrame::capture(&world.data)),
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
        assert!(frames.len() <= MAX_HISTORY_FRAMES + 2, "{}", frames.len());
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
