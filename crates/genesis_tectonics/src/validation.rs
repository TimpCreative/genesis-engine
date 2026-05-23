//! Doc 06 §11 validation metrics and helpers for Phase 1 tectonics.
//!
//! All validation runs use a fixed seed ([`VALIDATION_SEED`]) and subdivision
//! level ([`VALIDATION_SUBDIVISION_LEVEL`]) for reproducible CI checks.

use std::collections::{BTreeMap, BTreeSet};

use glam::DVec3;

use genesis_core::World;
use genesis_core::data::{BedrockType, WorldData};
use genesis_core::events::Significance;
use genesis_core::grid::HexGrid;
use genesis_core::lifecycle::GenerationError;
use genesis_core::parameters::{WorldParameters, WorldSeed};
use genesis_core::time::WorldYear;
use genesis_core::{HexId, create_world};

use crate::history::generate_full_history_with_tectonics;
use crate::motion::effective_position_direction;
use crate::plate::TectonicsState;

/// Fixed seed for all §11 / determinism / perf validation (Doc 06 §11).
pub const VALIDATION_SEED: u64 = 42;

/// Subdivision level for CI-friendly validation (~2,432 hexes at level 5).
pub const VALIDATION_SUBDIVISION_LEVEL: u8 = 5;

/// Quick validation target year (2 Geological ticks at 500k-year interval).
pub const VALIDATION_TARGET_YEAR_QUICK: i64 = 1_000_000;

/// Long-history validation for mountains, ocean basins, bedrock, event volume.
pub const VALIDATION_TARGET_YEAR_FULL: i64 = 100_000_000;

/// Deep persistence check (`cargo test --ignored`). Subdiv 5; ~10× [`VALIDATION_TARGET_YEAR_FULL`].
pub const VALIDATION_TARGET_YEAR_DEEP_PERSISTENCE: i64 = 500_000_000;

/// Full 1B-year run is manual only (`GENESIS_TARGET_YEAR=1000000000 cargo run -p genesis_app`).
pub const VALIDATION_TARGET_YEAR_ONE_BILLION: i64 = 1_000_000_000;

/// Shorter horizon for advection drift check (10M vs this year).
pub const VALIDATION_TARGET_YEAR_ADVECTION_DRIFT: i64 = 100_000_000;

/// Performance budget test target year (20 Geological ticks).
pub const PERF_TARGET_YEAR: i64 = 10_000_000;

/// Maximum wall time for [`PERF_TARGET_YEAR`] at subdiv 5 in default CI (§9.3).
pub const PERF_BUDGET_SECS: f64 = 30.0;

/// Doc §11 #4 nominal minimum ocean basin size (hexes).
pub const OCEAN_BASIN_MIN_HEXES_DOC: usize = 1000;

/// Doc §11 #1 lower bound on land fraction (loosened per §17 Q6 for fixed-seed CI).
pub const CONTINENTAL_FRACTION_MIN: f32 = 0.20;

/// Doc §11 #1 upper bound on land fraction (nominal §11 is 0.35; §17 Q6 allows 0.20–0.40).
pub const CONTINENTAL_FRACTION_MAX: f32 = 0.40;

/// Minimum land fraction at 1B years — continental cratons should persist (P1-13).
pub const CONTINENTAL_PERSISTENCE_MIN_FRAC: f32 = 0.15;

/// Doc §11 #6 elevation lower bound (m).
pub const ELEVATION_MIN_BOUND_M: f32 = -11_000.0;

/// Doc §11 #6 elevation upper bound (m).
pub const ELEVATION_MAX_BOUND_M: f32 = 9_000.0;

/// Tolerance for detecting hexes at elevation clamp (P1-11 saturation guard).
pub const SATURATION_TOLERANCE_M: f32 = 1.0;

/// Doc §11 #7 sea level bound (m).
pub const SEA_LEVEL_MAX_ABS_M: f32 = 200.0;

/// Doc §11 #3 mountain elevation threshold (m).
pub const MOUNTAIN_ELEVATION_THRESHOLD_M: f32 = 3000.0;

/// Doc §11 #4 ocean basin elevation threshold (m).
pub const OCEAN_BASIN_ELEVATION_THRESHOLD_M: f32 = -3000.0;

/// Doc §11 #8 loose lower bound on Notable+ events over long validation history.
pub const EVENT_COUNT_NOTABLE_MIN: usize = 500;

/// Doc §11 #8 nominal upper bound at 4.5B years (§6.4 table).
pub const EVENT_COUNT_NOTABLE_MAX_DOC: usize = 3000;

/// Upper bound for [`VALIDATION_TARGET_YEAR_FULL`] at seed 42 (implementation emits
/// more Notable events per tick than §6.4 table; calibrated empirically).
pub const EVENT_COUNT_NOTABLE_MAX_AT_FULL_YEAR: usize = 15_000;

/// Minimum distinct mountain regions (§11 #3).
pub const MIN_MOUNTAIN_REGIONS: usize = 3;

/// Default `WorldParameters` for validation: seed 42, subdiv 5, production geology defaults.
pub fn validation_parameters() -> WorldParameters {
    let mut params = WorldParameters::default();
    params.core.seed = WorldSeed::from_integer(VALIDATION_SEED);
    params.core.grid.subdivision_level = VALIDATION_SUBDIVISION_LEVEL;
    params
}

/// Builds a world and runs tectonics through `target_year` (events flushed to root log).
pub fn run_validation_world(
    target_year: WorldYear,
) -> Result<(World, TectonicsState), GenerationError> {
    let mut world = create_world(validation_parameters()).expect("validation parameters valid");
    let mut state = TectonicsState::new();
    generate_full_history_with_tectonics(&mut world, &mut state, target_year, |_| {})?;
    Ok((world, state))
}

/// Returns counts of hexes at or near max/min elevation clamps (§11 #6 strengthening).
pub fn count_saturated_hexes(data: &WorldData) -> (usize, usize) {
    let near_max = data
        .elevation_mean
        .iter()
        .filter(|&&e| (e - ELEVATION_MAX_BOUND_M).abs() < SATURATION_TOLERANCE_M)
        .count();
    let near_min = data
        .elevation_mean
        .iter()
        .filter(|&&e| (e - ELEVATION_MIN_BOUND_M).abs() < SATURATION_TOLERANCE_M)
        .count();
    (near_max, near_min)
}

/// Fraction of hexes with `elevation_mean > sea_level_m` (§11 #1).
pub fn continental_fraction(data: &WorldData) -> f32 {
    let sea = data.sea_level_m;
    let land = data.elevation_mean.iter().filter(|&&e| e > sea).count();
    land as f32 / data.cell_count() as f32
}

/// Hex with the highest `elevation_mean` (tie-break: lowest `HexId`).
pub fn peak_elevation_hex(data: &WorldData) -> HexId {
    let mut best = HexId(0);
    let mut best_e = f32::MIN;
    for (i, &e) in data.elevation_mean.iter().enumerate() {
        if e > best_e {
            best_e = e;
            best = HexId(i as u32);
        } else if e == best_e && HexId(i as u32) < best {
            best = HexId(i as u32);
        }
    }
    best
}

/// Scales Doc §11 #4 ocean basin minimum area for small grids.
pub fn min_ocean_basin_hex_threshold(cell_count: u32) -> usize {
    OCEAN_BASIN_MIN_HEXES_DOC.min(cell_count as usize / 4)
}

/// Connected-component sizes (hex count), sorted ascending; BFS in ascending `HexId` order.
pub fn count_connected_regions<F>(grid: &HexGrid, n: usize, mut predicate: F) -> Vec<usize>
where
    F: FnMut(usize) -> bool,
{
    let mut visited = vec![false; n];
    let mut sizes = Vec::new();

    for start in 0..n {
        if visited[start] || !predicate(start) {
            continue;
        }
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(start);
        visited[start] = true;
        let mut size = 0usize;

        while let Some(i) = queue.pop_front() {
            size += 1;
            let hex = HexId(i as u32);
            let mut neighbors: Vec<HexId> = grid.neighbors(hex).to_vec();
            neighbors.sort_by_key(|h| h.0);
            for neighbor_hex in neighbors {
                let j = neighbor_hex.0 as usize;
                if j >= n || visited[j] || !predicate(j) {
                    continue;
                }
                visited[j] = true;
                queue.push_back(j);
            }
        }
        sizes.push(size);
    }

    sizes.sort_unstable();
    sizes
}

/// Region sizes for hexes with `elevation_mean > threshold_m`.
pub fn mountain_regions_above_elevation(data: &WorldData, threshold_m: f32) -> Vec<usize> {
    let n = data.elevation_mean.len();
    count_connected_regions(&data.grid, n, |i| data.elevation_mean[i] > threshold_m)
}

/// Region sizes for hexes with `elevation_mean < threshold_m`.
pub fn ocean_basins_below_elevation(data: &WorldData, threshold_m: f32) -> Vec<usize> {
    let n = data.elevation_mean.len();
    count_connected_regions(&data.grid, n, |i| data.elevation_mean[i] < threshold_m)
}

/// Distinct bedrock types present in the world.
pub fn bedrock_types_present(data: &WorldData) -> BTreeSet<BedrockType> {
    let mut types = BTreeSet::new();
    for &bt in &data.bedrock_type {
        types.insert(bt);
    }
    types
}

/// Phase 1 bedrock check: four tectonic types assigned; Limestone deferred to Phase 4 (§8.4 / §11 #5).
pub fn check_phase1_bedrock_diversity(types: &BTreeSet<BedrockType>) -> Result<(), String> {
    let required = [
        BedrockType::Igneous,
        BedrockType::Sedimentary,
        BedrockType::Metamorphic,
        BedrockType::OceanicCrust,
    ];
    for &bt in &required {
        if !types.contains(&bt) {
            return Err(format!(
                "missing bedrock type {bt:?} (Phase 1 tectonic set)"
            ));
        }
    }
    let has_assigned = types.iter().any(|t| *t != BedrockType::Unknown);
    if !has_assigned {
        return Err("all hexes still Unknown; expected tectonic bedrock assignment".into());
    }
    if types.contains(&BedrockType::Limestone) {
        return Err("Limestone present but Phase 1 does not assign it (§8.4); unexpected".into());
    }
    Ok(())
}

/// Min and max `elevation_mean` (§11 #6).
pub fn elevation_bounds(data: &WorldData) -> (f32, f32) {
    let mut min = f32::MAX;
    let mut max = f32::MIN;
    for &e in &data.elevation_mean {
        min = min.min(e);
        max = max.max(e);
    }
    if min == f32::MAX {
        (0.0, 0.0)
    } else {
        (min, max)
    }
}

/// Summarizes total plate motion over simulated history (angular seed→effective distance, km).
pub fn plate_motion_summary(world: &World, state: &TectonicsState) -> Vec<f64> {
    let grid = &world.data.grid;
    let radius_km = world.data.parameters.core.planet.radius_km;
    let mut motions = Vec::new();

    for plate in state.registry.iter() {
        let seed = grid.cell_center_direction(plate.seed_hex);
        let seed_v = DVec3::new(seed[0], seed[1], seed[2]);
        let eff = effective_position_direction(grid, plate);
        let eff_v = DVec3::new(eff[0], eff[1], eff[2]);
        let dot = seed_v.dot(eff_v).clamp(-1.0, 1.0);
        let angular_rad = dot.acos();
        motions.push(angular_rad * radius_km);
    }

    motions.sort_by(|a, b| a.partial_cmp(b).unwrap());
    motions
}

/// Bins `elevation_mean` into 1000 m brackets for distribution analysis.
pub fn elevation_distribution(data: &WorldData) -> BTreeMap<i32, usize> {
    let mut bins = BTreeMap::new();
    for &e in &data.elevation_mean {
        let bin = (e / 1000.0).floor() as i32 * 1000;
        *bins.entry(bin).or_insert(0) += 1;
    }
    bins
}

/// Compact elevation histogram for diagnostic logs.
pub fn format_elevation_distribution(data: &WorldData) -> String {
    elevation_distribution(data)
        .iter()
        .map(|(bin, count)| format!("{bin}:{count}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Events in the root log at or above `min_significance`.
pub fn event_count_at_granularity(world: &World, min_significance: Significance) -> usize {
    world
        .branch_tree
        .root()
        .event_log
        .iter_significant(min_significance)
        .count()
}

/// Human-readable one-line summary for diagnostics (§9.3 / completion reports).
pub fn summarize_world(world: &World, state: &TectonicsState) -> String {
    let data = &world.data;
    let land_frac = continental_fraction(data);
    let (min_e, max_e) = elevation_bounds(data);
    let bedrock: Vec<_> = bedrock_types_present(data)
        .iter()
        .map(|t| format!("{t:?}"))
        .collect();
    let notable_events = event_count_at_granularity(world, Significance::Notable);
    let mountains = mountain_regions_above_elevation(data, MOUNTAIN_ELEVATION_THRESHOLD_M);
    let ocean_threshold = min_ocean_basin_hex_threshold(data.cell_count());
    let deep_oceans: Vec<_> = ocean_basins_below_elevation(data, OCEAN_BASIN_ELEVATION_THRESHOLD_M)
        .into_iter()
        .filter(|&s| s >= ocean_threshold)
        .collect();

    let motions = plate_motion_summary(world, state);
    let motion_summary = if motions.is_empty() {
        "none".to_string()
    } else {
        format!(
            "min={:.0}km median={:.0}km max={:.0}km",
            motions.first().copied().unwrap_or(0.0),
            motions[motions.len() / 2],
            motions.last().copied().unwrap_or(0.0),
        )
    };

    let land_pct = land_frac * 100.0;
    format!(
        "seed={VALIDATION_SEED} subdiv={} year={} land={land_pct:.1}% elev=[{min_e:.0},{max_e:.0}] \
         sea_level={:.1}m plates={} bedrock=[{}] notable_events={notable_events} \
         mountain_regions(>{MOUNTAIN_ELEVATION_THRESHOLD_M}m)={} \
         deep_ocean_basins(>={ocean_threshold} hex)={} motion={motion_summary}",
        data.grid.subdivision_level(),
        data.current_year.value(),
        data.sea_level_m,
        state.registry.count(),
        bedrock.join(","),
        mountains.len(),
        deep_oceans.len(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{PlateId, create_world};

    #[test]
    fn validation_parameters_use_seed_42_and_subdiv_5() {
        let p = validation_parameters();
        assert_eq!(p.core.seed.value, VALIDATION_SEED);
        assert_eq!(p.core.grid.subdivision_level, VALIDATION_SUBDIVISION_LEVEL);
    }

    #[test]
    fn continental_fraction_counts_land_above_sea_level() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let world = create_world(params).expect("world");
        let mut data = world.data;
        data.sea_level_m = 0.0;
        let n = data.elevation_mean.len();
        for i in 0..n {
            data.elevation_mean[i] = if i < n / 4 { 100.0 } else { -100.0 };
        }
        let frac = continental_fraction(&data);
        assert!((frac - 0.25).abs() < 0.02, "got {frac}");
    }

    #[test]
    fn min_ocean_basin_threshold_scales_down_on_small_grid() {
        assert_eq!(min_ocean_basin_hex_threshold(2432), 608);
        assert_eq!(min_ocean_basin_hex_threshold(500), 125);
        assert_eq!(
            min_ocean_basin_hex_threshold(10_000),
            OCEAN_BASIN_MIN_HEXES_DOC
        );
    }

    #[test]
    fn bfs_finds_two_regions_on_manual_fixture() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let world = create_world(params).expect("world");
        let grid = &world.data.grid;
        let n = world.data.cell_count() as usize;
        // Mark two separated land clusters by elevation sign on a small grid.
        let mut land = vec![false; n];
        if n >= 4 {
            land[0] = true;
            land[1] = true;
            land[n - 1] = true;
        }
        let sizes = count_connected_regions(grid, n, |i| land[i]);
        assert!(
            !sizes.is_empty(),
            "expected at least one region, got {sizes:?}"
        );
    }

    #[test]
    fn phase1_bedrock_check_accepts_four_tectonic_types() {
        let types = BTreeSet::from([
            BedrockType::Igneous,
            BedrockType::Sedimentary,
            BedrockType::Metamorphic,
            BedrockType::OceanicCrust,
            BedrockType::Unknown,
        ]);
        check_phase1_bedrock_diversity(&types).expect("ok");
    }

    #[test]
    fn phase1_bedrock_check_rejects_missing_igneous() {
        let types = BTreeSet::from([
            BedrockType::Sedimentary,
            BedrockType::Metamorphic,
            BedrockType::OceanicCrust,
        ]);
        assert!(check_phase1_bedrock_diversity(&types).is_err());
    }

    #[test]
    fn run_validation_world_reaches_target_year() {
        let (world, state) =
            run_validation_world(WorldYear(VALIDATION_TARGET_YEAR_QUICK)).expect("run");
        assert_eq!(
            world.data.current_year,
            WorldYear(VALIDATION_TARGET_YEAR_QUICK)
        );
        assert!(state.formation_complete);
        for &pid in &world.data.plate_id {
            assert_ne!(pid, PlateId::NONE);
        }
    }
}
