//! Lakes and inland seas (Doc 08 §5): the evaporation balance over the
//! depression tree, the candidate-sea adjudication, and salt banking.
//!
//! Equilibrium applies instantly each tick (§5.1) — at 500 ky ticks real
//! lakes equilibrate thousands of times over. Glacials cutting evaporation
//! and shifting precipitation therefore swell arid-basin lakes into pluvial
//! lakes and shrink them back to salt lakes and flats with no special code.

use genesis_core::data::{WaterBody, WaterBodyId, WaterBodyKind, WorldData};

use crate::routing::{FlowAccumulation, RoutingSurface, hex_area_m2};
use crate::solve::CandidateSea;

/// Open-water evaporation intercept, mm/yr (§5.1's `800 + 45 × T`).
pub const LAKE_EVAP_BASE_MM: f64 = 800.0;
/// Open-water evaporation slope per °C (§5.1).
pub const LAKE_EVAP_PER_C_MM: f64 = 45.0;
/// Fixed bisection iteration count for `E(level) = I` (§5.1 — monotone,
/// deterministic, sub-meter exact).
pub const LAKE_BISECTION_ITERATIONS: u32 = 24;

/// Salt banked per m³ of endorheic inflow, arbitrary units (§5.3). Set to
/// an Earth-river dissolved load (~0.1 kg/m³); salinity thresholds read in
/// the same units (ocean ≈ 35).
pub const SALT_LOAD_FACTOR: f64 = 1.0e-4;
/// Salinity at which an endorheic Lake becomes a SaltLake (§5.3, units of
/// [`SALT_LOAD_FACTOR`] — ≈ brackish-to-saline).
pub const SALT_LAKE_SALINITY_THRESHOLD: f64 = 10.0;

/// §5.1 open-water evaporation rate, mm/yr.
pub fn open_water_evap_mm(temperature_mean_c: f64, open_water_evap_factor: f64) -> f64 {
    (LAKE_EVAP_BASE_MM + LAKE_EVAP_PER_C_MM * temperature_mean_c).max(0.0) * open_water_evap_factor
}

/// Area (m²) of a depression's cells standing below `level_m`.
fn area_below_m2(cells: &[u32], elevations: &[f32], level_m: f64, hex_area_m2: f64) -> f64 {
    cells
        .iter()
        .filter(|&&c| f64::from(elevations[c as usize]) < level_m)
        .count() as f64
        * hex_area_m2
}

/// Volume (m³) of a depression's cells standing below `level_m`, ascending
/// cell order (deterministic f64 sum).
fn volume_below_m3(cells: &[u32], elevations: &[f32], level_m: f64, hex_area_m2: f64) -> f64 {
    cells
        .iter()
        .map(|&c| (level_m - f64::from(elevations[c as usize])).max(0.0) * hex_area_m2)
        .sum()
}

/// Mean temperature (°C) over a cell set, ascending order.
fn mean_temperature_c(cells: &[u32], temperatures: &[f32]) -> f64 {
    if cells.is_empty() {
        return 0.0;
    }
    let sum: f64 = cells
        .iter()
        .map(|&c| f64::from(temperatures[c as usize]))
        .sum();
    sum / cells.len() as f64
}

/// Mean precipitation (mm/yr) over a cell set, ascending order.
fn mean_precipitation_mm(cells: &[u32], precipitations: &[f32]) -> f64 {
    if cells.is_empty() {
        return 0.0;
    }
    let sum: f64 = cells
        .iter()
        .map(|&c| f64::from(precipitations[c as usize]))
        .sum();
    sum / cells.len() as f64
}

/// Solves `E(level) = I` by fixed 24-iteration bisection between
/// `bottom_m` and `spill_m` (§5.1). `E` is monotone non-decreasing in level;
/// `E(bottom) = 0 ≤ I < E(spill)` brackets the root. Returns the level.
fn solve_endorheic_level_m(
    cells: &[u32],
    elevations: &[f32],
    bottom_m: f64,
    spill_m: f64,
    inflow_m3_yr: f64,
    evap_mm_yr: f64,
    hex_area_m2: f64,
) -> f64 {
    let mut low = bottom_m;
    let mut high = spill_m.max(bottom_m);
    for _ in 0..LAKE_BISECTION_ITERATIONS {
        let mid = 0.5 * (low + high);
        let evaporation = area_below_m2(cells, elevations, mid, hex_area_m2) * evap_mm_yr * 1.0e-3;
        if evaporation < inflow_m3_yr {
            low = mid;
        } else {
            high = mid;
        }
    }
    0.5 * (low + high)
}

/// Banked salt (units of [`SALT_LOAD_FACTOR`]) over a cell set, ascending
/// order — the numerator of the §5.3 salinity.
fn banked_salt(data: &WorldData, cells: &[u32]) -> f64 {
    cells
        .iter()
        .map(|&c| f64::from(data.salt_accumulated[c as usize]))
        .sum()
}

/// §5.3: banks `delta_salt` equally over the floor cells (ascending order).
/// Endorheic banking is monotonic, like `fertility`.
fn bank_salt(data: &mut WorldData, floor_cells: &[u32], delta_salt: f64) {
    if floor_cells.is_empty() || delta_salt <= 0.0 {
        return;
    }
    let share = (delta_salt / floor_cells.len() as f64) as f32;
    for &cell in floor_cells {
        data.salt_accumulated[cell as usize] += share;
    }
}

/// Writes a standing body into the registry and its cells' water fields.
#[allow(clippy::too_many_arguments)]
fn write_body(
    data: &mut WorldData,
    id: WaterBodyId,
    kind: WaterBodyKind,
    surface_m: f32,
    cells: &[u32],
    volume_m3: f64,
    salinity: f32,
    outlet: Option<genesis_core::HexId>,
    hex_area_m2: f64,
) {
    let area_km2 = cells.len() as f64 * hex_area_m2 / 1.0e6;
    data.water_bodies.insert(
        id,
        WaterBody {
            id,
            kind,
            surface_m,
            area_km2,
            volume_km3: volume_m3 / 1.0e9,
            salinity,
            outlet,
        },
    );
    for &cell in cells {
        data.water_level_m[cell as usize] = surface_m;
        data.water_body_id[cell as usize] = id;
    }
}

/// Outcome of the §5 step: how much candidate-sea volume returns to the
/// ocean term (the §3.4 `ΔL = returned / ocean_area` correction input).
#[derive(Clone, Copy, Debug, Default)]
pub struct LakeStepOutcome {
    /// Candidate-sea volume handed back to the ocean, m³.
    pub returned_to_ocean_m3: f64,
    /// Candidate-sea volume kept standing (sustained seas plus drawn-down
    /// balances), m³ — with the ocean registry volume this must account the
    /// solve's effective ocean volume exactly (§3.4 partition check).
    pub candidate_kept_m3: f64,
}

/// §5.1–§5.3: adjudicates every retained depression bottom-up (descending
/// spill level — children before parents — tie: ascending bottom `HexId`),
/// then every candidate sea (§5.2), banking salt on endorheic floors.
///
/// `acc.discharge_m3_yr` is updated where exorheic surplus rides the channel
/// downstream of a spill; inflow credits to downstream depressions and
/// candidate seas land in the matching `acc` inflow vectors.
pub fn adjudicate_lakes(
    data: &mut WorldData,
    surface: &RoutingSurface,
    acc: &mut FlowAccumulation,
    candidates: &[CandidateSea],
    tick_years: f64,
) -> LakeStepOutcome {
    let hex_area_m2 = hex_area_m2(&data.grid);
    let evap_factor = f64::from(data.parameters.core.hydrology.open_water_evap_factor);
    // Elevations are a read-only input (§3.5: never written here); clone so
    // the registry/salt writes below don't fight the borrow checker.
    let elevations = data.elevation_mean.clone();
    let elevations = elevations.as_slice();

    // Bottom-up order: descending spill level, tie ascending bottom HexId.
    // Children spill into parents from above, so children adjudicate first
    // and their surplus credits the parent's inflow before it is read.
    let mut order: Vec<usize> = (0..surface.depressions.len()).collect();
    order.sort_by(|&a, &b| {
        surface.depressions[b]
            .spill_level_m
            .total_cmp(&surface.depressions[a].spill_level_m)
            .then_with(|| {
                surface.depressions[a]
                    .bottom
                    .cmp(&surface.depressions[b].bottom)
            })
    });

    for &index in &order {
        let depression = &surface.depressions[index];
        let cells = &depression.cells;
        let bottom_m = f64::from(elevations[depression.bottom as usize]);
        let spill_m = f64::from(depression.spill_level_m);
        let evap_mm = open_water_evap_mm(
            mean_temperature_c(cells, &data.temperature_mean),
            evap_factor,
        );

        // §5.1 I: entering discharge (incl. baseflow, via the bottom's
        // accumulation) plus precipitation on the lake surface. The surface
        // area is taken at spill — the maximum — a documented approximation
        // that keeps I constant during the endorheic bisection.
        let area_spill_m2 = area_below_m2(cells, elevations, spill_m, hex_area_m2);
        let inflow = acc.depression_inflow_m3_yr[index]
            + mean_precipitation_mm(cells, &data.precipitation) * area_spill_m2 * 1.0e-3;
        let evaporation_at_spill = area_spill_m2 * evap_mm * 1.0e-3;

        let id = WaterBodyId(depression.bottom);
        if inflow >= evaporation_at_spill && evaporation_at_spill > 0.0 {
            // Exorheic: stands at spill; surplus continues downstream.
            let level_m = spill_m;
            let wet_cells: Vec<u32> = cells
                .iter()
                .copied()
                .filter(|&c| f64::from(elevations[c as usize]) < level_m)
                .collect();
            let volume_m3 = volume_below_m3(cells, elevations, level_m, hex_area_m2);
            write_body(
                data,
                id,
                WaterBodyKind::Lake,
                level_m as f32,
                &wet_cells,
                volume_m3,
                0.0,
                Some(genesis_core::HexId(depression.spill_hex)),
                hex_area_m2,
            );
            let surplus = inflow - evaporation_at_spill;
            route_spill_surplus(data, surface, acc, depression.spill_hex, surplus);
            continue;
        }

        // Endorheic: solve E(level) = I; the lake stands where evaporation
        // exactly disposes of the inflow.
        let level_m = solve_endorheic_level_m(
            cells,
            elevations,
            bottom_m,
            spill_m,
            inflow,
            evap_mm,
            hex_area_m2,
        );
        let volume_m3 = volume_below_m3(cells, elevations, level_m, hex_area_m2);
        let wet_cells: Vec<u32> = cells
            .iter()
            .copied()
            .filter(|&c| f64::from(elevations[c as usize]) < level_m)
            .collect();
        let evaporation_at_level = wet_cells.len() as f64 * hex_area_m2 * evap_mm * 1.0e-3;
        let cell_evap = hex_area_m2 * evap_mm * 1.0e-3;
        // Flat-floor quantization: if the first wet step already overshoots
        // I by more than one cell's evaporation, no sustainable standing
        // water exists → SaltFlat (graded floors stay within one cell).
        let unsustainable = evaporation_at_level > inflow + cell_evap;

        // §5.3: endorheic inflow banks salt on the basin floor (the wet
        // floor; the bottom cell alone when the lake has dried).
        let floor_cells: Vec<u32> = if wet_cells.is_empty() || unsustainable {
            vec![depression.bottom]
        } else {
            wet_cells.clone()
        };
        bank_salt(data, &floor_cells, inflow * SALT_LOAD_FACTOR * tick_years);

        if inflow <= 0.0 || volume_m3 <= 0.0 || unsustainable {
            // Total drying: a SaltFlat body and its soil penalty (Slice 3).
            // (Zero inflow bisects to the basin floor with only bisection
            // dust standing — that is a flat, not a lake.)
            data.water_bodies.insert(
                id,
                WaterBody {
                    id,
                    kind: WaterBodyKind::SaltFlat,
                    surface_m: level_m as f32,
                    area_km2: 0.0,
                    volume_km3: 0.0,
                    salinity: 0.0,
                    outlet: None,
                },
            );
            continue;
        }
        let salinity = banked_salt(data, cells) / volume_m3;
        let kind = if salinity > SALT_LAKE_SALINITY_THRESHOLD {
            WaterBodyKind::SaltLake
        } else {
            WaterBodyKind::Lake
        };
        write_body(
            data,
            id,
            kind,
            level_m as f32,
            &wet_cells,
            volume_m3,
            salinity as f32,
            None,
            hex_area_m2,
        );
    }

    // §5.2 candidate seas: the same balance seeded with the bathtub volume.
    let mut outcome = LakeStepOutcome::default();
    for (index, candidate) in candidates.iter().enumerate() {
        let cells = &candidate.cells;
        let sea_level = data.sea_level_m as f64;
        let evap_mm = open_water_evap_mm(
            mean_temperature_c(cells, &data.temperature_mean),
            evap_factor,
        );
        let area_m2 = cells.len() as f64 * hex_area_m2;
        let inflow = acc
            .candidate_inflow_m3_yr
            .get(index)
            .copied()
            .unwrap_or(0.0)
            + mean_precipitation_mm(cells, &data.precipitation) * area_m2 * 1.0e-3;
        let evaporation_at_surface = area_m2 * evap_mm * 1.0e-3;
        let id = WaterBodyId(candidate.lowest_hex);

        if inflow >= evaporation_at_surface {
            // Sustained: an isolated Sea standing at its bathtub level (an
            // ocean-fed spill keeps it full — the Caspian analog).
            write_body(
                data,
                id,
                WaterBodyKind::Sea,
                sea_level as f32,
                cells,
                candidate.bathtub_volume_m3,
                0.0,
                None,
                hex_area_m2,
            );
            outcome.candidate_kept_m3 += candidate.bathtub_volume_m3;
            continue;
        }

        // Unsustainable: draw down to the evaporation balance, bank the
        // stranded salt, and return the surplus to the ocean term.
        let level_m = solve_endorheic_level_m(
            cells,
            elevations,
            candidate.bottom_elevation_m,
            sea_level,
            inflow,
            evap_mm,
            hex_area_m2,
        );
        let kept_volume_m3 = volume_below_m3(cells, elevations, level_m, hex_area_m2);
        let wet_cells: Vec<u32> = cells
            .iter()
            .copied()
            .filter(|&c| f64::from(elevations[c as usize]) < level_m)
            .collect();
        let floor_cells: Vec<u32> = if wet_cells.is_empty() {
            vec![candidate.lowest_hex]
        } else {
            wet_cells.clone()
        };
        bank_salt(data, &floor_cells, inflow * SALT_LOAD_FACTOR * tick_years);
        outcome.returned_to_ocean_m3 += candidate.bathtub_volume_m3 - kept_volume_m3;
        outcome.candidate_kept_m3 += kept_volume_m3;

        if inflow <= 0.0 || kept_volume_m3 <= 0.0 {
            data.water_bodies.insert(
                id,
                WaterBody {
                    id,
                    kind: WaterBodyKind::SaltFlat,
                    surface_m: level_m as f32,
                    area_km2: 0.0,
                    volume_km3: 0.0,
                    salinity: 0.0,
                    outlet: None,
                },
            );
            continue;
        }
        let salinity = banked_salt(data, cells) / kept_volume_m3;
        let kind = if salinity > SALT_LAKE_SALINITY_THRESHOLD {
            WaterBodyKind::SaltLake
        } else {
            WaterBodyKind::Lake
        };
        write_body(
            data,
            id,
            kind,
            level_m as f32,
            &wet_cells,
            kept_volume_m3,
            salinity as f32,
            None,
            hex_area_m2,
        );
    }
    outcome
}

/// §5.1: an exorheic lake's surplus (`I − E` at spill) continues downstream
/// from the spill hex. It rides channel cells (raising their discharge),
/// credits the first downstream depression's inflow or candidate sea, and
/// leaves the network at the ocean. Deterministic: the filled surface
/// descends monotonically, so the trace terminates.
fn route_spill_surplus(
    data: &WorldData,
    surface: &RoutingSurface,
    acc: &mut FlowAccumulation,
    spill_hex: u32,
    surplus_m3_yr: f64,
) {
    if surplus_m3_yr <= 0.0 {
        return;
    }
    let mut current = spill_hex;
    for _ in 0..surface.flow_target.len() {
        let candidate = surface.candidate_of[current as usize];
        if candidate != crate::routing::NONE {
            acc.candidate_inflow_m3_yr[candidate as usize] += surplus_m3_yr;
            return;
        }
        if data.water_body_id[current as usize] != WaterBodyId::NONE {
            return; // reached the ocean or a written lake.
        }
        let depression = surface.depression_of[current as usize];
        if depression != crate::routing::NONE {
            acc.depression_inflow_m3_yr[depression as usize] += surplus_m3_yr;
            return;
        }
        acc.discharge_m3_yr[current as usize] += surplus_m3_yr;
        let Some(target) = surface.flow_target[current as usize] else {
            return;
        };
        current = target;
    }
}

/// Applies the §3.4 closed-form correction: candidate-sea surplus returns to
/// the ocean as `ΔL = returned / ocean_area`. Bumps `sea_level_m`, the ocean
/// cells' standing level, and the ocean registry volume. Fringe cells
/// between the old and new level flood at next tick's solve — the residual
/// folds into the next tick (§3.4).
pub fn apply_returned_surplus(data: &mut WorldData, returned_m3: f64) {
    if returned_m3 <= 0.0 {
        return;
    }
    let ocean_id = data
        .water_bodies
        .values()
        .find(|body| body.kind == WaterBodyKind::Ocean)
        .map(|body| body.id);
    let Some(ocean_id) = ocean_id else {
        return;
    };
    let ocean_area_m2 = data.water_bodies[&ocean_id].area_km2 * 1.0e6;
    if ocean_area_m2 <= 0.0 {
        return;
    }
    let delta_l = (returned_m3 / ocean_area_m2) as f32;
    data.sea_level_m += delta_l;
    for i in 0..data.cell_count() as usize {
        if data.water_body_id[i] == ocean_id {
            data.water_level_m[i] = data.sea_level_m;
        }
    }
    let ocean = data
        .water_bodies
        .get_mut(&ocean_id)
        .expect("ocean body exists");
    ocean.surface_m = data.sea_level_m;
    ocean.volume_km3 += returned_m3 / 1.0e9;
}

/// Summed non-ocean registry volume, m³, in `BTreeMap` (ascending-id)
/// order — next tick's §3.4 lake debit (§3.2's `Σ lake_volumes`).
pub fn registry_lake_volume_m3(data: &WorldData) -> f64 {
    data.water_bodies
        .values()
        .filter(|body| body.kind != WaterBodyKind::Ocean)
        .map(|body| body.volume_km3 * 1.0e9)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::data::SoilClass;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{HexId, WorldYear, create_world};

    const TICK_YEARS: f64 = 500_000.0;

    /// A world with a one-cell ocean at hex 0 and a ramp of land.
    fn base_world() -> genesis_core::World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        world.data.current_year = WorldYear(1_000_000_000);
        let n = world.data.cell_count() as usize;
        world.data.elevation_mean[0] = -100.0;
        world.data.sea_level_m = 0.0;
        world.data.water_level_m[0] = 0.0;
        world.data.water_body_id[0] = WaterBodyId(0);
        world.data.water_bodies.insert(
            WaterBodyId(0),
            WaterBody {
                id: WaterBodyId(0),
                kind: WaterBodyKind::Ocean,
                surface_m: 0.0,
                area_km2: world.data.grid.hex_area_km2(HexId(0)),
                volume_km3: 1.0e6,
                salinity: 0.0,
                outlet: None,
            },
        );
        for i in 1..n {
            // Flat plain: routes through the fill's +epsilon gradient with
            // no spurious index-ordered pits; tests sink their own basins.
            world.data.elevation_mean[i] = 500.0;
            world.data.soil_class[i] = SoilClass::Loamy;
        }
        world
    }

    /// Sinks a connected `size`-cell blob rooted at `root`. Each cell is
    /// lowered by `depth_m + k×5` (BFS order) so the floor is graded — the
    /// endorheic bisection can stand at a partial-basin level instead of
    /// jumping from dry to all-cells-wet on a flat floor.
    fn sink_basin(
        world: &mut genesis_core::World,
        root: u32,
        size: usize,
        depth_m: f32,
    ) -> Vec<u32> {
        let n = world.data.cell_count() as usize;
        let mut in_basin = vec![false; n];
        let mut basin = vec![root];
        in_basin[root as usize] = true;
        let mut cursor = 0;
        while basin.len() < size && cursor < basin.len() {
            let cell = basin[cursor];
            for neighbor in world.data.grid.neighbors(HexId(cell)) {
                let j = neighbor.0 as usize;
                if !in_basin[j] && neighbor.0 != 0 {
                    in_basin[j] = true;
                    basin.push(neighbor.0);
                    if basin.len() >= size {
                        break;
                    }
                }
            }
            cursor += 1;
        }
        for (k, &cell) in basin.iter().enumerate() {
            world.data.elevation_mean[cell as usize] -= depth_m + k as f32 * 5.0;
        }
        basin.sort_unstable();
        basin
    }

    fn accumulate(world: &WorldData, surface: &RoutingSurface, runoff: &[f64]) -> FlowAccumulation {
        let n = world.cell_count() as usize;
        let zeros = vec![0.0; n];
        FlowAccumulation::accumulate(world, surface, runoff, &zeros, &zeros)
    }

    #[test]
    fn humid_depression_overflows_as_an_exorheic_lake() {
        let mut world = base_world();
        sink_basin(&mut world, 2000, 20, 1000.0);
        // Cold and very wet: inflow dwarfs evaporation at spill.
        for i in 0..world.data.cell_count() as usize {
            world.data.precipitation[i] = 4000.0;
            world.data.temperature_mean[i] = -5.0;
        }
        let surface = RoutingSurface::build(&world.data, &[]);
        assert_eq!(surface.depressions.len(), 1);
        let runoff = vec![1.0e9; world.data.cell_count() as usize];
        let mut acc = accumulate(&world.data, &surface, &runoff);
        adjudicate_lakes(&mut world.data, &surface, &mut acc, &[], TICK_YEARS);

        let depression = &surface.depressions[0];
        let body = &world.data.water_bodies[&WaterBodyId(depression.bottom)];
        assert_eq!(body.kind, WaterBodyKind::Lake);
        assert_eq!(body.surface_m, depression.spill_level_m);
        assert_eq!(body.outlet, Some(HexId(depression.spill_hex)));
        assert!(body.volume_km3 > 0.0);
        // Wet cells carry the body's level and id.
        for &cell in &depression.cells {
            let i = cell as usize;
            if world.data.elevation_mean[i] < body.surface_m {
                assert_eq!(world.data.water_level_m[i], body.surface_m);
                assert_eq!(world.data.water_body_id[i], body.id);
            }
        }
    }

    #[test]
    fn arid_depression_goes_endorheic_and_banks_salt() {
        let mut world = base_world();
        let basin = sink_basin(&mut world, 2000, 20, 1000.0);
        // Hot and dry: evaporation at spill exceeds any inflow.
        for i in 0..world.data.cell_count() as usize {
            world.data.precipitation[i] = 50.0;
            world.data.temperature_mean[i] = 30.0;
        }
        let surface = RoutingSurface::build(&world.data, &[]);
        let runoff = vec![1.0e7; world.data.cell_count() as usize];
        let mut acc = accumulate(&world.data, &surface, &runoff);
        adjudicate_lakes(&mut world.data, &surface, &mut acc, &[], TICK_YEARS);

        let depression = &surface.depressions[0];
        let body = &world.data.water_bodies[&WaterBodyId(depression.bottom)];
        assert!(
            matches!(
                body.kind,
                WaterBodyKind::Lake | WaterBodyKind::SaltLake | WaterBodyKind::SaltFlat
            ),
            "endorheic body kind: {:?}",
            body.kind
        );
        assert!(
            body.surface_m < depression.spill_level_m,
            "endorheic level below spill"
        );
        // Salt banked monotonically on basin-floor hexes (§5.3).
        let salt: f32 = basin
            .iter()
            .map(|&c| world.data.salt_accumulated[c as usize])
            .sum();
        assert!(salt > 0.0, "endorheic inflow banks salt");
        if body.kind != WaterBodyKind::SaltFlat {
            assert!(body.salinity > 0.0, "salinity = salt/volume");
        }
    }

    #[test]
    fn exorheic_surplus_continues_downstream_of_the_spill() {
        let mut world = base_world();
        sink_basin(&mut world, 2000, 20, 1000.0);
        // Cold and very wet: the lake stands at spill with surplus to spare.
        for i in 0..world.data.cell_count() as usize {
            world.data.precipitation[i] = 4000.0;
            world.data.temperature_mean[i] = -5.0;
        }
        let surface = RoutingSurface::build(&world.data, &[]);
        assert_eq!(surface.depressions.len(), 1);
        let runoff = vec![5.0e9; world.data.cell_count() as usize];
        let mut acc = accumulate(&world.data, &surface, &runoff);
        let depression = &surface.depressions[0];
        let spill = depression.spill_hex as usize;
        let spill_discharge_before = acc.discharge_m3_yr[spill];
        // Snapshot inflow before adjudication — route_spill_surplus may
        // credit the same depression if the spill path re-enters it.
        let depression_inflow = acc.depression_inflow_m3_yr[0];
        let elevations = world.data.elevation_mean.clone();
        let spill_m = f64::from(depression.spill_level_m);
        let hex_area = hex_area_m2(&world.data.grid);
        let area_spill = super::area_below_m2(&depression.cells, &elevations, spill_m, hex_area);
        let precip = super::mean_precipitation_mm(&depression.cells, &world.data.precipitation);
        let temp = super::mean_temperature_c(&depression.cells, &world.data.temperature_mean);
        let evap_factor = f64::from(world.data.parameters.core.hydrology.open_water_evap_factor);
        let expected_surplus = depression_inflow + precip * area_spill * 1.0e-3
            - area_spill * open_water_evap_mm(temp, evap_factor) * 1.0e-3;

        adjudicate_lakes(&mut world.data, &surface, &mut acc, &[], TICK_YEARS);

        // §5.1: I − E at spill rides the channel from the spill hex onward.
        let body = &world.data.water_bodies[&WaterBodyId(depression.bottom)];
        assert_eq!(body.kind, WaterBodyKind::Lake);
        assert!(
            acc.discharge_m3_yr[spill] > spill_discharge_before,
            "surplus augments the spill channel: {} -> {}",
            spill_discharge_before,
            acc.discharge_m3_yr[spill]
        );
        let gained = acc.discharge_m3_yr[spill] - spill_discharge_before;
        assert!(
            (gained - expected_surplus).abs() / expected_surplus.max(1.0) < 1e-9,
            "surplus = I − E(spill): {gained} vs {expected_surplus}"
        );
    }

    #[test]
    fn sustained_candidate_becomes_a_sea() {
        let mut world = base_world();
        let cells = sink_basin(&mut world, 2000, 15, 3000.0);
        // The candidate sits below sea level: flood it to the surface.
        for &cell in &cells {
            world.data.elevation_mean[cell as usize] = -2000.0;
        }
        // Wet, cold climate: evaporation is tiny, inflow large.
        for i in 0..world.data.cell_count() as usize {
            world.data.precipitation[i] = 3000.0;
            world.data.temperature_mean[i] = -10.0;
        }
        let hex_area_m2 = hex_area_m2(&world.data.grid);
        let candidate = CandidateSea {
            lowest_hex: cells.iter().copied().min().unwrap(),
            cells: cells.clone(),
            bathtub_volume_m3: cells.len() as f64 * hex_area_m2 * 2000.0,
            bottom_elevation_m: -2000.0,
        };
        let surface = RoutingSurface::build(&world.data, std::slice::from_ref(&candidate));
        let mut acc = accumulate(
            &world.data,
            &surface,
            &vec![1.0e9; world.data.cell_count() as usize],
        );
        let outcome = adjudicate_lakes(
            &mut world.data,
            &surface,
            &mut acc,
            std::slice::from_ref(&candidate),
            TICK_YEARS,
        );
        assert_eq!(
            outcome.returned_to_ocean_m3, 0.0,
            "nothing returns when sustained"
        );
        let body = &world.data.water_bodies[&WaterBodyId(cells.iter().copied().min().unwrap())];
        assert_eq!(body.kind, WaterBodyKind::Sea);
        assert_eq!(body.surface_m, world.data.sea_level_m);
        for &cell in &cells {
            assert_eq!(world.data.water_body_id[cell as usize], body.id);
        }
    }

    #[test]
    fn unsustained_candidate_draws_down_and_returns_surplus() {
        let mut world = base_world();
        let cells = sink_basin(&mut world, 2000, 15, 3000.0);
        for &cell in &cells {
            world.data.elevation_mean[cell as usize] = -2000.0;
        }
        // Hyper-arid hot basin: evaporation vastly exceeds inflow.
        for i in 0..world.data.cell_count() as usize {
            world.data.precipitation[i] = 0.0;
            world.data.temperature_mean[i] = 40.0;
        }
        let hex_area_m2 = hex_area_m2(&world.data.grid);
        let bathtub = cells.len() as f64 * hex_area_m2 * 2000.0;
        let candidate = CandidateSea {
            lowest_hex: cells.iter().copied().min().unwrap(),
            cells: cells.clone(),
            bathtub_volume_m3: bathtub,
            bottom_elevation_m: -2000.0,
        };
        let surface = RoutingSurface::build(&world.data, std::slice::from_ref(&candidate));
        let mut acc = accumulate(
            &world.data,
            &surface,
            &vec![0.0; world.data.cell_count() as usize],
        );
        let sea_level_before = world.data.sea_level_m;
        let outcome = adjudicate_lakes(
            &mut world.data,
            &surface,
            &mut acc,
            std::slice::from_ref(&candidate),
            TICK_YEARS,
        );
        assert!(
            outcome.returned_to_ocean_m3 > 0.0,
            "surplus returns to the ocean term"
        );
        let id = WaterBodyId(cells.iter().copied().min().unwrap());
        let body = &world.data.water_bodies[&id];
        assert!(
            matches!(body.kind, WaterBodyKind::SaltLake | WaterBodyKind::SaltFlat),
            "drawdown leaves salt: {:?}",
            body.kind
        );
        assert!(
            body.surface_m < sea_level_before,
            "drawn down below the ocean"
        );
        // The closed-form ΔL correction raises the written sea level.
        apply_returned_surplus(&mut world.data, outcome.returned_to_ocean_m3);
        assert!(world.data.sea_level_m > sea_level_before);
        assert_eq!(world.data.water_level_m[0], world.data.sea_level_m);
    }

    #[test]
    fn endorheic_bisection_balances_evaporation() {
        // A 20-cell graded basin in a warm, dry-ish climate: E(spill) exceeds
        // the modest inflow, so the lake stands where E(level) = I.
        let mut world = base_world();
        sink_basin(&mut world, 2000, 20, 1000.0);
        for i in 0..world.data.cell_count() as usize {
            world.data.precipitation[i] = 200.0;
            world.data.temperature_mean[i] = 20.0;
        }
        let surface = RoutingSurface::build(&world.data, &[]);
        let runoff = vec![2.0e7; world.data.cell_count() as usize];
        let mut acc = accumulate(&world.data, &surface, &runoff);
        let depression_inflow = acc.depression_inflow_m3_yr[0];
        let depression = surface.depressions[0].clone();
        let elevations = world.data.elevation_mean.clone();
        let spill_m = f64::from(depression.spill_level_m);
        let hex_area = hex_area_m2(&world.data.grid);
        let area_spill = super::area_below_m2(&depression.cells, &elevations, spill_m, hex_area);
        let precip = super::mean_precipitation_mm(&depression.cells, &world.data.precipitation);
        let expected_inflow = depression_inflow + precip * area_spill * 1.0e-3;

        adjudicate_lakes(&mut world.data, &surface, &mut acc, &[], TICK_YEARS);
        let body = &world.data.water_bodies[&WaterBodyId(depression.bottom)];
        assert_ne!(body.kind, WaterBodyKind::SaltFlat, "inflow keeps it wet");
        assert!(
            body.surface_m < depression.spill_level_m,
            "endorheic: stands below spill"
        );
        // E(solved) = I, up to one cell-quantum of lake area (the bisection
        // root sits at the area step where evaporation crosses the inflow).
        let evap_mm = open_water_evap_mm(20.0, 1.2);
        let evaporation = body.area_km2 * 1.0e6 * evap_mm * 1.0e-3;
        let cell_evap = hex_area * evap_mm * 1.0e-3;
        assert!(
            (evaporation - expected_inflow).abs() <= cell_evap,
            "E(level) ≈ I within one cell of area: E={evaporation} I={expected_inflow}"
        );
    }
}
