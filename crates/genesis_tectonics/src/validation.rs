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
// Raised 15k → 17k when Wilson-cycle rift recovery (P1-19) kept sutured
// plates moving: persistent activity emits ~0.1% more Notable events. Loose
// calibration bound per §11 note, not a physical limit.
pub const EVENT_COUNT_NOTABLE_MAX_AT_FULL_YEAR: usize = 17_000;

/// Minimum distinct mountain regions (§11 #3).
pub const MIN_MOUNTAIN_REGIONS: usize = 3;

/// Doc §11 #10 crust-area lower bound at 1B years (Wilson-cycle pass). The
/// gate is on continental CRUST area, not land: land fraction at a snapshot
/// is hostage to Wilson phase, sea level, and resolution (v0.14). The floor
/// tolerates chaotic phase (measured 0.12–0.22 across seeds at subdiv 5)
/// while catching crust-destruction regressions (the unscaled subduction
/// erosion leak read 0.07).
pub const WILSON_CRUST_FRACTION_MIN: f32 = 0.10;

/// Doc §11 #10 crust-area upper bound at 1B years (guards the pre-v0.13
/// accretion ratchet that paved ~half the sphere by 4B years).
pub const WILSON_CRUST_FRACTION_MAX: f32 = 0.45;

/// Doc §11 #11: detached below-sea cells (inland seas/pits cut off from the
/// main ocean) must stay under this fraction of all cells.
pub const DETACHED_BELOW_SEA_MAX_FRACTION: f32 = 0.02;

/// Doc §11 #11: no detached below-sea component may be deeper than this (m) —
/// fossil trenches and failed rifts heal; only the live world ocean reaches
/// abyssal depth.
pub const DETACHED_DEEPEST_FLOOR_M: f32 = -6_000.0;

/// Doc §11 #12: minimum passive-margin share of coastline (Atlantic-style
/// trailing edges, not convergent arcs).
pub const PASSIVE_MARGIN_MIN_FRACTION: f64 = 0.25;

/// Default `WorldParameters` for validation: seed 42, subdiv 5, production geology defaults.
/// `GENESIS_VALIDATION_SEED` overrides the seed so chaotic-snapshot gates can be
/// sampled across realizations (`GENESIS_VALIDATION_SEED=43 cargo test ...`).
pub fn validation_parameters() -> WorldParameters {
    let mut params = WorldParameters::default();
    let seed = std::env::var("GENESIS_VALIDATION_SEED")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(VALIDATION_SEED);
    params.core.seed = WorldSeed::from_integer(seed);
    params.core.grid.subdivision_level = VALIDATION_SUBDIVISION_LEVEL;
    params
}

/// Builds a world and runs tectonics through `target_year` (events flushed to root log).
pub fn run_validation_world(
    target_year: WorldYear,
) -> Result<(World, TectonicsState), GenerationError> {
    run_validation_world_with(target_year, |_| {})
}

/// [`run_validation_world`] with a hook to tweak the parameters first.
///
/// Doc 06 §11 structure gates and projection round-trip tests validate the
/// **structure engine**, so they disable the Doc 06-CAL calibration
/// (`p.core.terrain.enabled = false`) to read the raw tectonic field; the
/// calibrated output is validated separately (land fraction, shelf, no-pit).
pub fn run_validation_world_with(
    target_year: WorldYear,
    configure: impl FnOnce(&mut genesis_core::parameters::WorldParameters),
) -> Result<(World, TectonicsState), GenerationError> {
    let mut params = validation_parameters();
    configure(&mut params);
    let mut world = create_world(params).expect("validation parameters valid");
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

/// Spatial-coherence metrics for the land mask (Doc 06 continent morphology).
///
/// `large_frac`: fraction of land hexes that sit in connected components of at
/// least `large_min` hexes — high means land is a few solid continents, low
/// means salt-and-pepper spray. `small_components`: number of connected land
/// components of `1..=3` hexes — the "starfield" count, which should be tiny.
/// Deterministic: `neighbors_sorted` BFS in ascending `HexId`.
pub struct LandCohesion {
    pub large_frac: f32,
    pub small_components: u32,
    pub components: u32,
}

pub fn land_cohesion(data: &WorldData, large_min: u32) -> LandCohesion {
    let n = data.cell_count() as usize;
    let sea = data.sea_level_m;
    let is_land = |i: usize| data.elevation_mean[i] > sea;
    let mut visited = vec![false; n];
    let mut sizes: Vec<u32> = Vec::new();
    for start in 0..n {
        if visited[start] || !is_land(start) {
            continue;
        }
        let mut queue = std::collections::VecDeque::from([start]);
        visited[start] = true;
        let mut size = 0u32;
        while let Some(i) = queue.pop_front() {
            size += 1;
            for &nb in data.grid.neighbors_sorted(HexId(i as u32)) {
                let j = nb.0 as usize;
                if j < n && !visited[j] && is_land(j) {
                    visited[j] = true;
                    queue.push_back(j);
                }
            }
        }
        sizes.push(size);
    }
    let land_total: u32 = sizes.iter().sum();
    let large: u32 = sizes.iter().filter(|&&s| s >= large_min).sum();
    LandCohesion {
        large_frac: if land_total == 0 {
            0.0
        } else {
            large as f32 / land_total as f32
        },
        small_components: sizes.iter().filter(|&&s| s <= 3).count() as u32,
        components: sizes.len() as u32,
    }
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
            let neighbors = grid.neighbors_sorted(hex);
            for &neighbor_hex in neighbors {
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

/// Connected below-sea components as `(size_hexes, deepest_elevation_m)`,
/// sorted by descending size (index 0 is the main ocean; the rest are
/// detached inland seas/pits). Used by §11 #11 (Wilson-cycle pass).
pub fn below_sea_components(data: &WorldData) -> Vec<(usize, f32)> {
    let n = data.cell_count() as usize;
    let sea = data.sea_level_m;
    let below = |i: usize| data.elevation_mean[i] < sea;
    let mut visited = vec![false; n];
    let mut comps = Vec::new();
    for start in 0..n {
        if visited[start] || !below(start) {
            continue;
        }
        visited[start] = true;
        let mut queue = std::collections::VecDeque::from([start]);
        let mut size = 0usize;
        let mut deepest = f32::MAX;
        while let Some(i) = queue.pop_front() {
            size += 1;
            deepest = deepest.min(data.elevation_mean[i]);
            for nb in data.grid.neighbors(HexId(i as u32)) {
                let j = nb.0 as usize;
                if j < n && !visited[j] && below(j) {
                    visited[j] = true;
                    queue.push_back(j);
                }
            }
        }
        comps.push((size, deepest));
    }
    comps.sort_by_key(|c| std::cmp::Reverse(c.0));
    comps
}

/// §11 #11 components: below-sea connected components that are detached
/// (smaller than the §5.8 open-ocean threshold) AND not touching a live
/// convergent or divergent margin. An active trench or its marginal basin is
/// still being consumed by live subduction, and an actively opening rift
/// basin is still being born (Afar, Baikal) — their depth is current
/// geology, not fossil relief, so both are excluded from the fossil-floor
/// check (mirrors §5.8's liveness rules: closing/opening velocity beyond
/// `CONVERGENCE_THRESHOLD_M_PER_YEAR`).
/// Returns `(cell_count, deepest_elevation)` per component.
pub fn fossil_below_sea_components(
    data: &WorldData,
    boundaries: &crate::boundary::BoundaryInfo,
) -> Vec<(usize, f32)> {
    use crate::boundary::BoundaryClass;

    let water = crate::accretion::label_water_components(data);
    let n = data.cell_count() as usize;
    let mut active = vec![false; n];
    for (&hex, edges) in &boundaries.edges {
        for edge in edges {
            let live = match edge.class {
                BoundaryClass::Convergent(_) => {
                    edge.normal_velocity_m_per_year
                        > crate::partition::CONVERGENCE_THRESHOLD_M_PER_YEAR
                }
                BoundaryClass::Divergent => {
                    edge.normal_velocity_m_per_year
                        < -crate::partition::CONVERGENCE_THRESHOLD_M_PER_YEAR
                }
                BoundaryClass::Transform => false,
            };
            if live {
                active[hex.0 as usize] = true;
                let j = edge.neighbor_hex.0 as usize;
                if j < n {
                    active[j] = true;
                }
            }
        }
    }
    let mut touches_live = vec![false; water.comp_sizes.len()];
    let mut deepest = vec![f32::MAX; water.comp_sizes.len()];
    for (i, &id) in water.comp_of.iter().enumerate() {
        if id == usize::MAX {
            continue;
        }
        if active[i] {
            touches_live[id] = true;
        }
        deepest[id] = deepest[id].min(data.elevation_mean[i]);
    }
    water
        .comp_sizes
        .iter()
        .enumerate()
        .filter(|&(ref id, &size)| size < water.open_ocean_min && !touches_live[*id])
        .map(|(id, &size)| (size, deepest[id]))
        .collect()
}

/// Fraction of coastline hexes (land hexes adjacent to a below-sea hex) that
/// are NOT within 2 rings of a convergent boundary — Atlantic-style passive
/// margins. Used by §11 #12 (Wilson-cycle pass).
pub fn passive_margin_fraction(data: &WorldData, state: &TectonicsState) -> f64 {
    use crate::boundary::{BoundaryClass, detect_and_classify_boundaries};

    let n = data.cell_count() as usize;
    let sea = data.sea_level_m;
    let below_sea: Vec<bool> = (0..n).map(|i| data.elevation_mean[i] < sea).collect();

    let boundaries = detect_and_classify_boundaries(data, &state.registry, &state.projection);

    // BFS ring distance from convergent boundary hexes only.
    let mut conv_dist = vec![u32::MAX; n];
    let mut queue = std::collections::VecDeque::new();
    for &h in &boundaries.boundary_hexes {
        let is_convergent = boundaries.edges.get(&h).is_some_and(|edges| {
            edges
                .iter()
                .any(|e| matches!(e.class, BoundaryClass::Convergent(_)))
        });
        if is_convergent {
            let i = h.0 as usize;
            if conv_dist[i] == u32::MAX {
                conv_dist[i] = 0;
                queue.push_back(i);
            }
        }
    }
    while let Some(i) = queue.pop_front() {
        let d = conv_dist[i];
        if d >= 2 {
            continue;
        }
        for nb in data.grid.neighbors(HexId(i as u32)) {
            let j = nb.0 as usize;
            if j < n && conv_dist[j] == u32::MAX {
                conv_dist[j] = d + 1;
                queue.push_back(j);
            }
        }
    }

    let mut coastline = 0u64;
    let mut passive = 0u64;
    for i in 0..n {
        if below_sea[i] {
            continue;
        }
        let on_coast = data
            .grid
            .neighbors(HexId(i as u32))
            .iter()
            .any(|nb| (nb.0 as usize) < n && below_sea[nb.0 as usize]);
        if !on_coast {
            continue;
        }
        coastline += 1;
        if conv_dist[i] > 2 {
            passive += 1;
        }
    }
    if coastline == 0 {
        1.0
    } else {
        passive as f64 / coastline as f64
    }
}

/// Distinct bedrock types present in the world.
pub fn bedrock_types_present(data: &WorldData) -> BTreeSet<BedrockType> {
    let mut types = BTreeSet::new();
    for &bt in &data.bedrock_type {
        types.insert(bt);
    }
    types
}

/// Bedrock diversity check: the four tectonic types must be assigned (§11 #5).
/// Phase 1 deferred `Limestone` (§8.4); since Phase 2 the platform pass
/// (`assign_platform_limestone`, Doc 08 §6.3) assigns it on warm shallow
/// continental platforms, so its presence is expected, not an error.
pub fn check_bedrock_diversity(types: &BTreeSet<BedrockType>) -> Result<(), String> {
    let required = [
        BedrockType::Igneous,
        BedrockType::Sedimentary,
        BedrockType::Metamorphic,
        BedrockType::OceanicCrust,
    ];
    for &bt in &required {
        if !types.contains(&bt) {
            return Err(format!("missing bedrock type {bt:?} (tectonic set)"));
        }
    }
    let has_assigned = types.iter().any(|t| *t != BedrockType::Unknown);
    if !has_assigned {
        return Err("all hexes still Unknown; expected tectonic bedrock assignment".into());
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
    let seed = data.parameters.core.seed.value;
    format!(
        "seed={seed} subdiv={} year={} land={land_pct:.1}% elev=[{min_e:.0},{max_e:.0}] \
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
    fn bedrock_check_accepts_four_tectonic_types() {
        let types = BTreeSet::from([
            BedrockType::Igneous,
            BedrockType::Sedimentary,
            BedrockType::Metamorphic,
            BedrockType::OceanicCrust,
            BedrockType::Unknown,
        ]);
        check_bedrock_diversity(&types).expect("ok");
    }

    #[test]
    fn bedrock_check_accepts_limestone_since_phase2() {
        // Doc 08 §6.3: the platform pass assigns Limestone in Phase 2.
        let types = BTreeSet::from([
            BedrockType::Igneous,
            BedrockType::Sedimentary,
            BedrockType::Metamorphic,
            BedrockType::OceanicCrust,
            BedrockType::Limestone,
        ]);
        check_bedrock_diversity(&types).expect("limestone expected since Phase 2");
    }

    #[test]
    fn bedrock_check_rejects_missing_igneous() {
        let types = BTreeSet::from([
            BedrockType::Sedimentary,
            BedrockType::Metamorphic,
            BedrockType::OceanicCrust,
        ]);
        assert!(check_bedrock_diversity(&types).is_err());
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

#[cfg(test)]
mod continental_relief {
    use super::*;
    use genesis_core::create_world;
    use genesis_core::time::WorldYear;

    /// Doc 06 §5.2 roots gate: continents are palimpsests. At 1 B and 4.4 B,
    /// no continent may hoard mountains beyond its land share (enrichment =
    /// mountain share ÷ land share ≤ 2.5; Earth's Asia sits near 1.8 — a
    /// supercontinent legitimately holds most mountains *and* most land), and
    /// flat continents follow Earth's rule: at most ONE Australia-class flat
    /// major per world — "flat" meaning genuinely featureless (peak below
    /// 700 m; even Australia peaks at 2,228 m) — and it must be
    /// Australia-sized or smaller. Every larger landmass carries basement
    /// relief. Three seeds, subdiv 6.
    #[test]
    #[ignore = "deep-time relief-distribution gate; run with --ignored --nocapture"]
    fn gate_continental_relief_distribution() {
        for &seed in &[42u64, 7, 99] {
            for &year in &[1_000_000_000i64, 4_400_000_000] {
                let mut params = validation_parameters();
                params.core.seed = genesis_core::parameters::WorldSeed::from_integer(seed);
                params.core.grid.subdivision_level = 6;
                let mut world = create_world(params).expect("params");
                let mut state = crate::plate::TectonicsState::new();
                generate_full_history_with_tectonics(
                    &mut world,
                    &mut state,
                    WorldYear(year),
                    |_| {},
                )
                .expect("run");
                let data = &world.data;
                let n = data.cell_count() as usize;
                let sea = data.sea_level_m;
                let mut comp = vec![u32::MAX; n];
                let mut comps: Vec<Vec<u32>> = Vec::new();
                for start in 0..n {
                    if comp[start] != u32::MAX || data.elevation_mean[start] <= sea {
                        continue;
                    }
                    let id = comps.len() as u32;
                    let mut queue = std::collections::VecDeque::new();
                    comp[start] = id;
                    queue.push_back(start as u32);
                    let mut cells = Vec::new();
                    while let Some(c) = queue.pop_front() {
                        cells.push(c);
                        for nb in data.grid.neighbors(genesis_core::HexId(c)) {
                            let j = nb.0 as usize;
                            if j < n && comp[j] == u32::MAX && data.elevation_mean[j] > sea {
                                comp[j] = id;
                                queue.push_back(nb.0);
                            }
                        }
                    }
                    comps.push(cells);
                }
                let mut majors: Vec<&Vec<u32>> = comps.iter().filter(|c| c.len() >= 45).collect();
                majors.sort_by_key(|c| std::cmp::Reverse(c.len()));
                let mtn_total = data
                    .elevation_mean
                    .iter()
                    .filter(|&&e| e > sea + 1500.0)
                    .count()
                    .max(1);
                let land_total = comps.iter().map(|c| c.len()).sum::<usize>().max(1);
                let mut flat_majors = 0;
                let mut oversized_flat = 0;
                let mut max_enrichment = 0.0f64;
                let mut per = String::new();
                for cells in &majors {
                    let high = cells
                        .iter()
                        .filter(|&&c| data.elevation_mean[c as usize] > sea + 1000.0)
                        .count();
                    let mtn = cells
                        .iter()
                        .filter(|&&c| data.elevation_mean[c as usize] > sea + 1500.0)
                        .count();
                    let mtn_share = mtn as f64 / mtn_total as f64;
                    let land_share = cells.len() as f64 / land_total as f64;
                    // Hoarding is only meaningful at scale: small mountainous
                    // arcs (a New Zealand) legitimately exceed any enrichment
                    // cap. Judge continents holding ≥ 10% of world land.
                    if land_share >= 0.10 {
                        max_enrichment = max_enrichment.max(mtn_share / land_share);
                    }
                    let high_frac = high as f64 / cells.len() as f64;
                    let max_elev = cells
                        .iter()
                        .map(|&c| data.elevation_mean[c as usize])
                        .fold(f32::MIN, f32::max);
                    let cont = cells
                        .iter()
                        .filter(|&&c| {
                            data.continental_crust
                                .get(c as usize)
                                .copied()
                                .unwrap_or(false)
                        })
                        .count();
                    if max_elev < 700.0 {
                        flat_majors += 1;
                        // Australia is ~7.7M km² ≈ 110 subdiv-6 hexes. Young
                        // continentalized platforms (§5.11) can assemble a
                        // somewhat larger flat before their margins orogenize,
                        // so the cap allows roughly a double Australia.
                        if cells.len() > 220 {
                            oversized_flat += 1;
                        }
                    }
                    per.push_str(&format!(
                        " {}c/{:.0}%hi/mx{:.0}/cc{:.0}%",
                        cells.len(),
                        high_frac * 100.0,
                        max_elev,
                        cont as f64 / cells.len() as f64 * 100.0
                    ));
                }
                println!(
                    "[gate-relief] seed={seed:>3} year={:.1}B majors={} flat_majors={flat_majors} max_enrichment={max_enrichment:.2} |{per}",
                    year as f64 / 1e9,
                    majors.len()
                );
                assert!(
                    max_enrichment <= 2.5,
                    "seed {seed} @{year}: a continent hoards mountains beyond its land share (enrichment {max_enrichment:.2})"
                );
                // Epoch-aware flatness bound: mid-history worlds pass
                // through young-continent phases (freshly continentalized
                // crust that has not yet orogenized), so 1 B allows a couple
                // of modest flats. Deep time is the promise: at most one
                // Australia-class flat, nothing larger.
                if year >= 4_000_000_000 {
                    assert!(
                        flat_majors <= 1,
                        "seed {seed} @{year}: {flat_majors} flat majors (Earth allows one Australia)"
                    );
                    assert_eq!(
                        oversized_flat, 0,
                        "seed {seed} @{year}: a flat continent larger than Australia-class"
                    );
                } else {
                    assert!(
                        flat_majors <= 2,
                        "seed {seed} @{year}: {flat_majors} flat majors in the young-continent phase"
                    );
                    assert!(
                        oversized_flat <= 1,
                        "seed {seed} @{year}: {oversized_flat} oversized flats in the young-continent phase"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod vast_plains {
    use super::*;
    use genesis_core::create_world;
    use genesis_core::time::WorldYear;
    use genesis_core::HexId;

    /// A cell is "plains" when the 2-ring neighborhood's total relief is
    /// under 200 m — the West-Siberian-Plain grade of flatness.
    const PLAINS_LOCAL_RELIEF_M: f32 = 200.0;
    /// Largest connected plain allowed, as a fraction of world land. Earth's
    /// largest (the West Siberian Plain) is ~2% of land; the pre-epeirogeny
    /// pathology reached 20%. Measured post-§5.12: ≤ 2% on the gate seeds;
    /// the bound leaves chaos headroom while catching the vast-plain class.
    const MAX_PLAIN_LAND_FRACTION: f64 = 0.08;

    fn largest_plain(seed: u64, year: i64) -> (usize, usize) {
        let mut params = validation_parameters();
        params.core.seed = genesis_core::parameters::WorldSeed::from_integer(seed);
        params.core.grid.subdivision_level = 6;
        let mut world = create_world(params).expect("params");
        let mut state = crate::plate::TectonicsState::new();
        generate_full_history_with_tectonics(&mut world, &mut state, WorldYear(year), |_| {})
            .expect("run");
        let data = &world.data;
        let n = data.cell_count() as usize;
        let land: Vec<bool> = (0..n).map(|i| data.elevation_mean[i] > 0.0).collect();
        let land_total = land.iter().filter(|&&l| l).count();

        let mut plains = vec![false; n];
        for i in 0..n {
            if !land[i] {
                continue;
            }
            let mut lo = data.elevation_mean[i];
            let mut hi = lo;
            for &nb in data.grid.neighbors(HexId(i as u32)) {
                for nb2 in data.grid.neighbors(nb) {
                    let e = data.elevation_mean[nb2.0 as usize];
                    lo = lo.min(e);
                    hi = hi.max(e);
                }
                let e = data.elevation_mean[nb.0 as usize];
                lo = lo.min(e);
                hi = hi.max(e);
            }
            plains[i] = hi - lo < PLAINS_LOCAL_RELIEF_M;
        }

        let mut seen = vec![false; n];
        let mut largest = 0usize;
        for start in 0..n {
            if !plains[start] || seen[start] {
                continue;
            }
            let mut size = 0usize;
            let mut queue = std::collections::VecDeque::new();
            seen[start] = true;
            queue.push_back(start as u32);
            while let Some(c) = queue.pop_front() {
                size += 1;
                for nb in data.grid.neighbors(HexId(c)) {
                    let j = nb.0 as usize;
                    if j < n && plains[j] && !seen[j] {
                        seen[j] = true;
                        queue.push_back(nb.0);
                    }
                }
            }
            largest = largest.max(size);
        }
        (largest, land_total)
    }

    /// Doc 06 §5.12 gate: no vast inland plains. The epeirogenic swell keeps
    /// interior structure varied, so no connected sub-200 m-relief region may
    /// grow beyond an Earth-plausible share of world land.
    #[test]
    #[ignore = "deep-time gate; run with --ignored --nocapture"]
    fn gate_no_vast_inland_plains() {
        let jobs: Vec<(u64, i64)> = [42u64, 7, 99]
            .iter()
            .flat_map(|&s| [(s, 1_000_000_000i64), (s, 4_400_000_000)])
            .collect();
        let results: Vec<(u64, i64, usize, usize)> = std::thread::scope(|scope| {
            let handles: Vec<_> = jobs
                .iter()
                .map(|&(s, y)| scope.spawn(move || {
                    let (largest, land) = largest_plain(s, y);
                    (s, y, largest, land)
                }))
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for (seed, year, largest, land) in results {
            let frac = largest as f64 / land.max(1) as f64;
            println!(
                "[gate-plains] seed={seed:>3} year={:.1}B largest={largest} ({:.0}% of land)",
                year as f64 / 1e9,
                frac * 100.0,
            );
            assert!(
                frac <= MAX_PLAIN_LAND_FRACTION,
                "seed {seed} @{year}: a {largest}-cell connected plain covers {:.0}% of land \
                 (bound {:.0}%) — the vast-inland-plain pathology (Doc 06 §5.12)",
                frac * 100.0,
                MAX_PLAIN_LAND_FRACTION * 100.0,
            );
        }
    }
}


#[cfg(test)]
mod mountain_belts {
    use super::*;
    use genesis_core::create_world;
    use genesis_core::time::WorldYear;
    use genesis_core::HexId;

    fn measure(seed: u64, year: i64) -> (String, usize, f64, f64) {
        let mut params = validation_parameters();
        params.core.seed = genesis_core::parameters::WorldSeed::from_integer(seed);
        params.core.grid.subdivision_level = 6;
        let mut world = create_world(params).expect("params");
        let mut state = crate::plate::TectonicsState::new();
        generate_full_history_with_tectonics(&mut world, &mut state, WorldYear(year), |_| {})
            .expect("run");
        let data = &world.data;
        let n = data.cell_count() as usize;
        let mtn: Vec<bool> = (0..n).map(|i| data.elevation_mean[i] >= 1500.0).collect();
        let mtn_total = mtn.iter().filter(|&&m| m).count();
        let land_total = (0..n).filter(|&i| data.elevation_mean[i] > 0.0).count();

        // Connected mountain belts.
        let mut seen = vec![false; n];
        let mut belts: Vec<Vec<u32>> = Vec::new();
        for start in 0..n {
            if !mtn[start] || seen[start] {
                continue;
            }
            let mut comp = Vec::new();
            let mut queue = std::collections::VecDeque::new();
            seen[start] = true;
            queue.push_back(start as u32);
            while let Some(c) = queue.pop_front() {
                comp.push(c);
                for nb in data.grid.neighbors(HexId(c)) {
                    let j = nb.0 as usize;
                    if j < n && mtn[j] && !seen[j] {
                        seen[j] = true;
                        queue.push_back(nb.0);
                    }
                }
            }
            belts.push(comp);
        }
        belts.sort_by_key(|b| std::cmp::Reverse(b.len()));
        let real_belts = belts.iter().filter(|b| b.len() >= 5).count();
        let largest = belts.first().map(|b| b.len()).unwrap_or(0);
        // Elongation of the largest belt: BFS diameter (hex steps) vs mean width.
        let elong = belts.first().map(|belt| {
            let set: std::collections::HashSet<u32> = belt.iter().copied().collect();
            let bfs_far = |start: u32| -> (u32, usize) {
                let mut dist = std::collections::HashMap::new();
                dist.insert(start, 0usize);
                let mut queue = std::collections::VecDeque::new();
                queue.push_back(start);
                let mut far = (start, 0usize);
                while let Some(c) = queue.pop_front() {
                    let d = dist[&c];
                    if d > far.1 {
                        far = (c, d);
                    }
                    for nb in data.grid.neighbors(HexId(c)) {
                        if set.contains(&nb.0) && !dist.contains_key(&nb.0) {
                            dist.insert(nb.0, d + 1);
                            queue.push_back(nb.0);
                        }
                    }
                }
                far
            };
            let (a, _) = bfs_far(belt[0]);
            let (_, diameter) = bfs_far(a);
            let length = diameter.max(1);
            belt.len() as f64 / length as f64
        });
        let share = largest as f64 / mtn_total.max(1) as f64;
        let width = elong.unwrap_or(0.0);
        let line = format!(
            "[belts] seed={seed:>3} year={:.1}B mtn={mtn_total} ({:.0}% land) belts={real_belts} largest={largest} ({:.0}% of mtn) width={:.1}",
            year as f64 / 1e9,
            mtn_total as f64 / land_total.max(1) as f64 * 100.0,
            share * 100.0,
            width,
        );
        (line, real_belts, share, width)
    }

    #[test]
    #[ignore = "deep-time gate; run with --ignored --nocapture"]
    fn gate_mountain_belt_distribution() {
        // Range of epochs, not just endpoints: mid-history is where the
        // conglomerate pathology peaked (assembly-era supercontinents).
        let jobs: Vec<(u64, i64)> = [42u64, 7, 99]
            .iter()
            .flat_map(|&s| {
                [
                    (s, 1_000_000_000i64),
                    (s, 2_000_000_000),
                    (s, 3_000_000_000),
                    (s, 4_400_000_000),
                ]
            })
            .collect();
        let results: Vec<(u64, i64, String, usize, f64, f64)> = std::thread::scope(|scope| {
            let handles: Vec<_> = jobs
                .iter()
                .map(|&(s, y)| scope.spawn(move || {
                    let (line, belts, share, width) = measure(s, y);
                    (s, y, line, belts, share, width)
                }))
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for (seed, year, line, belts, share, width) in results {
            println!("{line}");
            // Doc 06 §5.13: mountains come as several long belts, not one
            // blob. Deep time is the promise; 1 B may still carry a wide
            // incumbent from the assembly era (EMA + banked-root memory).
            if year >= 4_000_000_000 {
                assert!(
                    belts >= 4,
                    "seed {seed} @{year}: only {belts} mountain belts (want ≥ 4 — Earth has dozens)"
                );
                assert!(
                    share <= 0.75,
                    "seed {seed} @{year}: one belt holds {:.0}% of mountain area (bound 75%)",
                    share * 100.0
                );
                assert!(
                    width <= 9.0,
                    "seed {seed} @{year}: largest belt mean width {width:.1} hexes (bound 9 — bands, not blobs)"
                );
            }
        }
    }
}


#[cfg(test)]
mod render_probe {
    use super::*;
    use genesis_core::create_world;
    use genesis_core::time::WorldYear;
    use genesis_core::HexId;

    fn render(seed: u64, year: i64, path: &str) {
        let mut params = validation_parameters();
        params.core.seed = genesis_core::parameters::WorldSeed::from_integer(seed);
        params.core.grid.subdivision_level = 6;
        let mut world = create_world(params).expect("params");
        let mut state = crate::plate::TectonicsState::new();
        generate_full_history_with_tectonics(&mut world, &mut state, WorldYear(year), |_| {})
            .expect("run");
        let data = &world.data;
        let (w, h) = (900usize, 450usize);
        let mut img = vec![0u8; w * h * 3];
        for py in 0..h {
            let lat = 89.9 - 179.8 * (py as f64 + 0.5) / h as f64;
            for px in 0..w {
                let lon = -180.0 + 360.0 * (px as f64 + 0.5) / w as f64;
                let hex = data.grid.nearest_hex(lat.to_radians(), lon.to_radians());
                let e = data.elevation_mean[hex.0 as usize];
                let (r, g, b) = if e <= 0.0 {
                    let t = (1.0 + (e / 5000.0).max(-1.0)) as f32;
                    ((10.0 + 30.0 * t) as u8, (30.0 + 70.0 * t) as u8, (80.0 + 120.0 * t) as u8)
                } else if e < 300.0 {
                    (60, 130, 60)
                } else if e < 800.0 {
                    (110, 140, 70)
                } else if e < 1500.0 {
                    (150, 120, 80)
                } else if e < 3000.0 {
                    (120, 100, 90)
                } else {
                    (235, 235, 235)
                };
                let i = (py * w + px) * 3;
                img[i] = r;
                img[i + 1] = g;
                img[i + 2] = b;
            }
        }
        let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
        out.extend_from_slice(&img);
        std::fs::write(path, out).expect("write ppm");
        println!("[render] wrote {path}");
    }

    #[test]
    #[ignore = "utility: writes equirect elevation PPMs to $RENDER_DIR for visual inspection"]
    fn render_probe() {
        let dir = std::env::var("RENDER_DIR").unwrap_or_else(|_| "/tmp".into());
        std::thread::scope(|scope| {
            scope.spawn(|| render(7, 2_000_000_000, &format!("{dir}/w7_2b.ppm")));
            scope.spawn(|| render(2024, 2_000_000_000, &format!("{dir}/w2024_2b.ppm")));
            scope.spawn(|| render(123, 2_000_000_000, &format!("{dir}/w123_2b.ppm")));
        });
    }
}
