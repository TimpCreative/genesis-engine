//! The flooding solve (Doc 08 §3.4): derives `sea_level_m`, the ocean mask,
//! `water_level_m`, and the water-body registry from the hypsometry and the
//! budget's ocean term. Stateless — rebuilt fresh every tick (§2.2).
//!
//! Per §3.5, sea level is an output, never an input: no hex stores an
//! ocean/land identity, and the solve is the only place the level is set.

use std::collections::{BTreeMap, VecDeque};

use genesis_core::HexId;
use genesis_core::data::{WATER_NONE, WaterBody, WaterBodyId, WaterBodyKind, WorldData};

/// Thermal expansion coefficient of seawater, per °C (§3.5.1).
pub const THERMOSTERIC_BETA_PER_C: f64 = 1.9e-4;
/// Reference temperature for the conservation volume, °C (§3.5.1).
pub const THERMOSTERIC_REFERENCE_C: f64 = 15.0;

/// Outcome of one tick's flooding solve.
#[derive(Clone, Copy, Debug)]
pub struct FloodOutcome {
    /// Derived bathtub level in meters. Always finite: with no standing water
    /// it sits at the lowest cell's elevation (zero wet cells), so one-tick-
    /// lagged readers (tectonics freeboard, coast logic) see a sane value.
    pub sea_level_m: f64,
    /// Number of cells below the derived level this tick.
    pub wet_cell_count: u32,
}

/// Global mean of `data.temperature_mean` (°C), summed in ascending-`HexId`
/// order — §3.5.1's `T_ocean` (the ocean equilibrates within a 500 ky tick).
/// Cells are equal-area (`HexGrid::hex_area_km2` is a uniform constant), so a
/// plain mean is the area-weighted mean.
pub fn global_mean_temperature_c(data: &WorldData) -> f64 {
    let n = data.temperature_mean.len();
    if n == 0 {
        return THERMOSTERIC_REFERENCE_C;
    }
    let sum: f64 = data.temperature_mean.iter().map(|&t| f64::from(t)).sum();
    sum / n as f64
}

/// Thermosteric effective volume (§3.5.1): warm water occupies more volume.
/// Conservation accounts mass (reference-temperature volume); this factor
/// enters only the volume→level mapping.
pub fn thermosteric_effective_volume_m3(ocean_volume_m3: f64, mean_temperature_c: f64) -> f64 {
    ocean_volume_m3
        * (1.0 + THERMOSTERIC_BETA_PER_C * (mean_temperature_c - THERMOSTERIC_REFERENCE_C))
}

/// Exact bathtub level for `effective_volume_m3` over equal-area cells
/// (§3.4 step 1).
///
/// `sorted_elevations_m` must be ascending (ties already broken by `HexId`,
/// which does not affect the level). Returns a finite level in all cases:
/// - `volume ≤ 0` → the lowest cell's elevation (zero wet cells under the
///   strict `elev < L` predicate);
/// - volume past the highest cell → the level that submerges everything.
pub fn bathtub_level_m(
    sorted_elevations_m: &[f64],
    hex_area_m2: f64,
    effective_volume_m3: f64,
) -> f64 {
    let n = sorted_elevations_m.len();
    if n == 0 {
        return 0.0;
    }
    if effective_volume_m3 <= 0.0 {
        return sorted_elevations_m[0];
    }

    // Walk the prefix: at step k the first k cells stand under a common level
    // and we ask whether the water reaches the (k+1)-th cell's elevation.
    let mut prefix_sum = 0.0_f64; // Σ e_i over i < k
    for (k, &elev) in sorted_elevations_m.iter().enumerate() {
        let volume_to_level = hex_area_m2 * (k as f64 * elev - prefix_sum);
        if volume_to_level >= effective_volume_m3 {
            // k ≥ 1 here: k = 0 gives volume_to_level = 0 < volume.
            return (effective_volume_m3 / hex_area_m2 + prefix_sum) / k as f64;
        }
        prefix_sum += elev;
    }
    (effective_volume_m3 / hex_area_m2 + prefix_sum) / n as f64
}

/// One connected component of `{elev < L}` cells.
#[derive(Debug)]
struct BasinComponent {
    /// Lowest `HexId` in the component — the basin's stable `WaterBodyId`.
    lowest_hex: u32,
    /// Member cells (BFS discovery order).
    cells: Vec<u32>,
    /// Component water volume in m³ at the solved level.
    volume_m3: f64,
    /// Assigned by [`adjudicate_candidate_seas`].
    kind: WaterBodyKind,
}

/// §3.4 step 2 seam: the largest-volume component is **the ocean**; every
/// other below-`L` component is a *candidate sea* — ocean-fed or a doomed
/// endorheic basin by its climate. The §5 adjudication (evaporation balance,
/// registry upgrade, and the closed-form `ΔL = returned / ocean_area`
/// correction) is Slice 2; Slice 1 treats every candidate as ocean-fed, so it
/// becomes a [`WaterBodyKind::Sea`] standing at the shared sea level.
fn adjudicate_candidate_seas(components: &mut [BasinComponent]) {
    components.sort_by(|a, b| {
        b.volume_m3
            .total_cmp(&a.volume_m3)
            .then_with(|| a.lowest_hex.cmp(&b.lowest_hex))
    });
    for (rank, component) in components.iter_mut().enumerate() {
        component.kind = if rank == 0 {
            WaterBodyKind::Ocean
        } else {
            WaterBodyKind::Sea
        };
    }
}

/// Runs the §3.4 flooding solve for one tick and writes the derived fields:
/// `sea_level_m`, `water_level_m`, `water_body_id`, and the `water_bodies`
/// registry. Never writes `elevation_mean` (§3.5: water coverage is derived,
/// terrain is tectonics').
pub fn solve_flooding(data: &mut WorldData, ocean_volume_m3: f64) -> FloodOutcome {
    let n = data.cell_count() as usize;
    let hex_area_m2 = data.grid.hex_area_km2(HexId(0)) * 1.0e6;

    // §3.5.1: conservation accounts mass; the level responds to temperature.
    let t_mean = global_mean_temperature_c(data);
    let effective_volume_m3 = thermosteric_effective_volume_m3(ocean_volume_m3, t_mean);

    // Sort cells by elevation ascending, HexId tie-break (§3.4 step 1). The
    // sort is the hot spot; §14's radix/near-sorted re-sort is a later
    // performance pass.
    let mut order: Vec<u32> = (0..n as u32).collect();
    order.sort_by(|&a, &b| {
        data.elevation_mean[a as usize]
            .total_cmp(&data.elevation_mean[b as usize])
            .then_with(|| a.cmp(&b))
    });
    let sorted_elevations: Vec<f64> = order
        .iter()
        .map(|&i| f64::from(data.elevation_mean[i as usize]))
        .collect();

    let sea_level_m = bathtub_level_m(&sorted_elevations, hex_area_m2, effective_volume_m3);

    // §3.4 step 2: connected components of {elev < L}, BFS with
    // ascending-HexId frontiers, scanned in ascending-HexId order.
    let mut component_of = vec![u32::MAX; n];
    let mut components: Vec<BasinComponent> = Vec::new();
    let mut queue: VecDeque<u32> = VecDeque::new();
    for start in 0..n {
        if component_of[start] != u32::MAX || f64::from(data.elevation_mean[start]) >= sea_level_m {
            continue;
        }
        let component_index = components.len() as u32;
        component_of[start] = component_index;
        queue.push_back(start as u32);
        let mut cells: Vec<u32> = Vec::new();
        let mut volume_m3 = 0.0_f64;
        while let Some(cell) = queue.pop_front() {
            cells.push(cell);
            volume_m3 +=
                hex_area_m2 * (sea_level_m - f64::from(data.elevation_mean[cell as usize]));
            let mut neighbors: Vec<HexId> = data.grid.neighbors(HexId(cell)).to_vec();
            neighbors.sort_unstable();
            for neighbor in neighbors {
                let j = neighbor.0 as usize;
                if j < n
                    && component_of[j] == u32::MAX
                    && f64::from(data.elevation_mean[j]) < sea_level_m
                {
                    component_of[j] = component_index;
                    queue.push_back(neighbor.0);
                }
            }
        }
        let lowest_hex = cells.iter().copied().min().unwrap_or(start as u32);
        components.push(BasinComponent {
            lowest_hex,
            cells,
            volume_m3,
            kind: WaterBodyKind::Ocean,
        });
    }

    adjudicate_candidate_seas(&mut components);

    // §3.4 step 3: write the derived level and the ocean mask.
    let sea_level_f32 = sea_level_m as f32;
    data.sea_level_m = sea_level_f32;
    data.water_bodies = BTreeMap::new();
    let hex_area_km2 = data.grid.hex_area_km2(HexId(0));
    let mut wet_cell_count = 0_u32;
    for component in &components {
        let id = WaterBodyId(component.lowest_hex);
        data.water_bodies.insert(
            id,
            WaterBody {
                id,
                kind: component.kind,
                surface_m: sea_level_f32,
                area_km2: component.cells.len() as f64 * hex_area_km2,
                volume_km3: component.volume_m3 / 1.0e9,
                salinity: 0.0,
                outlet: None,
            },
        );
        for &cell in &component.cells {
            data.water_level_m[cell as usize] = sea_level_f32;
            data.water_body_id[cell as usize] = id;
        }
        wet_cell_count += component.cells.len() as u32;
    }

    // Dry cells carry the sentinel; flow/drainage fields are Slice 2's.
    for (i, &component) in component_of.iter().enumerate() {
        if component == u32::MAX {
            data.water_level_m[i] = WATER_NONE;
            data.water_body_id[i] = WaterBodyId::NONE;
        }
    }

    FloodOutcome {
        sea_level_m,
        wet_cell_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{HexGrid, WorldYear};

    const EARTH_RADIUS_KM: f64 = 6371.0;

    fn world_at_level(level: u8) -> WorldData {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = level;
        let grid = HexGrid::new(level, EARTH_RADIUS_KM).expect("grid constructs");
        let mut world = WorldData::new(grid, params);
        world.current_year = WorldYear(300_000_000);
        world
    }

    #[test]
    fn bathtub_level_handles_degenerate_cases() {
        let elevations = [-100.0, -50.0, 0.0, 50.0];
        assert_eq!(bathtub_level_m(&[], 1.0, 100.0), 0.0);
        // Zero and negative volume sit at the lowest cell: nothing is wet.
        assert_eq!(bathtub_level_m(&elevations, 1.0, 0.0), -100.0);
        assert_eq!(bathtub_level_m(&elevations, 1.0, -5.0), -100.0);
    }

    #[test]
    fn bathtub_level_interpolates_exactly() {
        // Unit-area cells at -100, 0, 100: 150 volume units stand the two
        // lowest cells at +25 (depths 125 + 25).
        let elevations = [-100.0, 0.0, 100.0];
        assert_eq!(bathtub_level_m(&elevations, 1.0, 150.0), 25.0);
        // 300 units stand them exactly at +100 (200 + 100); the top cell,
        // level with the water, is not yet wet.
        assert_eq!(bathtub_level_m(&elevations, 1.0, 300.0), 100.0);
        // Past the top: 600 units submerge all three to +200; 900 → +300.
        assert_eq!(bathtub_level_m(&elevations, 1.0, 600.0), 200.0);
        assert_eq!(bathtub_level_m(&elevations, 1.0, 900.0), 300.0);
    }

    /// Gate §15 #2: land fraction responds monotonically to the inventory
    /// dial on a fixed synthetic hypsometry (3-point sweep).
    #[test]
    fn sea_level_dial_is_monotonic_in_inventory() {
        // Synthetic Earth-ish hypsometry: 30% continental plateau at +800 m,
        // 70% ocean floor at -3700 m (unit-area cells).
        let mut elevations: Vec<f64> = Vec::new();
        elevations.extend(std::iter::repeat_n(-3700.0, 70));
        elevations.extend(std::iter::repeat_n(800.0, 30));
        elevations.sort_by(f64::total_cmp);

        let sphere_area_m2 = 4.0 * std::f64::consts::PI * 6.371e6_f64.powi(2);
        let cell_area_m2 = sphere_area_m2 / 100.0;

        let mut prev_land_fraction = f64::INFINITY;
        for gel_m in [1000.0, 2700.0, 5000.0] {
            let level = bathtub_level_m(&elevations, cell_area_m2, gel_m * sphere_area_m2);
            let land = elevations.iter().filter(|&&e| e >= level).count() as f64 / 100.0;
            assert!(
                land <= prev_land_fraction,
                "land fraction must not rise with inventory: {land} > {prev_land_fraction} at {gel_m} GEL"
            );
            prev_land_fraction = land;
        }
        // Sanity: 1000 GEL leaves the plateau fully dry, 5000 GEL drowns it all.
        let level_dry = bathtub_level_m(&elevations, cell_area_m2, 1000.0 * sphere_area_m2);
        let level_wet = bathtub_level_m(&elevations, cell_area_m2, 5000.0 * sphere_area_m2);
        assert!(
            level_dry < 800.0,
            "1000 GEL stays below the plateau: {level_dry}"
        );
        assert!(level_wet > 800.0, "5000 GEL tops the plateau: {level_wet}");
    }

    /// Gate §15 #19: at equal inventory and hypsometry, a warmer ocean stands
    /// higher; the swing lands in the 5–25 m band for a greenhouse step.
    #[test]
    fn thermosteric_warming_raises_the_sea() {
        // 70% ocean floor at -3700 m → Earth-like ~3.7 km mean ocean depth.
        let mut elevations: Vec<f64> = Vec::new();
        elevations.extend(std::iter::repeat_n(-3700.0, 70));
        elevations.extend(std::iter::repeat_n(800.0, 30));
        elevations.sort_by(f64::total_cmp);

        let sphere_area_m2 = 4.0 * std::f64::consts::PI * 6.371e6_f64.powi(2);
        let cell_area_m2 = sphere_area_m2 / 100.0;
        // Inventory that puts the reference-temperature level exactly at 0 m.
        let inventory_m3 = 70.0 * cell_area_m2 * 3700.0;

        let reference = bathtub_level_m(&elevations, cell_area_m2, inventory_m3);
        assert!(
            reference.abs() < 1.0e-6,
            "reference level at 0 m: {reference}"
        );

        for (t_mean, label) in [(5.0, "icehouse"), (25.0, "greenhouse")] {
            let effective = thermosteric_effective_volume_m3(inventory_m3, t_mean);
            let level = bathtub_level_m(&elevations, cell_area_m2, effective);
            let delta = level - reference;
            let expected = THERMOSTERIC_BETA_PER_C * (t_mean - THERMOSTERIC_REFERENCE_C) * 3700.0;
            assert!(
                (delta - expected).abs() < 1.0,
                "{label}: ΔL {delta} should track β·ΔT·depth ≈ {expected}"
            );
        }

        let cold = bathtub_level_m(
            &elevations,
            cell_area_m2,
            thermosteric_effective_volume_m3(inventory_m3, 5.0),
        );
        let warm = bathtub_level_m(
            &elevations,
            cell_area_m2,
            thermosteric_effective_volume_m3(inventory_m3, 25.0),
        );
        let swing = warm - cold;
        assert!(
            (5.0..=25.0).contains(&swing),
            "greenhouse swing {swing} m must land in the 5–25 m band (gate #19)"
        );
    }

    #[test]
    fn solve_with_no_water_leaves_finite_level_and_dry_world() {
        let mut world = world_at_level(4);
        world.elevation_mean.fill(-100.0);
        world.elevation_mean[7] = -500.0;

        let outcome = solve_flooding(&mut world, 0.0);

        assert_eq!(outcome.sea_level_m, -500.0, "level sits at the lowest cell");
        assert_eq!(outcome.wet_cell_count, 0);
        assert!(world.sea_level_m.is_finite());
        assert!(world.water_bodies.is_empty());
        assert!(world.water_level_m.iter().all(|&w| w == WATER_NONE));
        assert!(world.water_body_id.iter().all(|&b| b == WaterBodyId::NONE));
    }

    #[test]
    fn solve_floods_below_level_components_as_ocean_and_sea() {
        let mut world = world_at_level(4);
        let n = world.cell_count() as usize;
        // One world-spanning low region (the ocean) plus hex 100 walled off by
        // its high neighbors (a candidate sea).
        world.elevation_mean.fill(-100.0);
        world.elevation_mean[100] = -50.0;
        let wall: Vec<HexId> = world.grid.neighbors(HexId(100)).to_vec();
        for &neighbor in &wall {
            world.elevation_mean[neighbor.0 as usize] = 500.0;
        }
        // Enough water to stand a little above 0 m: the low cells plus the
        // isolated cell are submerged; the wall stays dry.
        let area_m2 = world.grid.hex_area_km2(HexId(0)) * 1.0e6;
        let low_cells = (n - wall.len() - 1) as f64;
        let volume_m3 = area_m2 * (low_cells * 100.0 + 50.0 + (low_cells + 1.0) * 10.0);

        let outcome = solve_flooding(&mut world, volume_m3);

        assert!(
            outcome.sea_level_m > -50.0,
            "both basins submerged: L = {}",
            outcome.sea_level_m
        );
        assert_eq!(
            outcome.wet_cell_count as usize,
            n - wall.len(),
            "every cell but the wall is wet"
        );
        assert_eq!(world.water_bodies.len(), 2, "ocean + one candidate sea");

        // Ocean = largest-volume component; ids are each basin's lowest hex.
        let ocean = world
            .water_bodies
            .values()
            .find(|b| b.kind == WaterBodyKind::Ocean)
            .expect("an ocean exists");
        let sea = world
            .water_bodies
            .values()
            .find(|b| b.kind == WaterBodyKind::Sea)
            .expect("a candidate sea exists");
        assert_eq!(sea.id, WaterBodyId(100), "sea id is its basin's lowest hex");
        let expected_ocean_lowest = (0..n as u32)
            .filter(|&i| i != 100 && !wall.contains(&HexId(i)))
            .min()
            .expect("ocean cells exist");
        assert_eq!(ocean.id, WaterBodyId(expected_ocean_lowest));
        assert!(ocean.volume_km3 > sea.volume_km3);
        assert_eq!(world.water_body_id[100], WaterBodyId(100));
        assert_eq!(world.water_level_m[100], world.sea_level_m);
        for &w in &wall {
            let i = w.0 as usize;
            assert_eq!(world.water_body_id[i], WaterBodyId::NONE);
            assert_eq!(world.water_level_m[i], WATER_NONE);
        }
    }
}
