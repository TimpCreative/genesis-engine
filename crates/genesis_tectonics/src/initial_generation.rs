//! Initial plate generation at world formation (year 0).

use std::collections::{BTreeMap, BTreeSet};

use genesis_core::data::WorldData;
use genesis_core::rng::WorldRng;
use genesis_core::time::WorldYear;
use genesis_core::{HexGrid, HexId, PlateId, World};
use glam::DVec3;
use rand::Rng;
use rand::seq::SliceRandom;

use crate::plate::{Plate, PlateClass, PlateRegistry, PlateType};
use crate::plate_surface::PlateSurface;

/// Doc 06 §4.4 — seed placement, growth plate selection, growth neighbor choice.
const PLATE_SEEDS_STREAM: &str = "tectonics.plate_seeds";
/// Doc 06 §4.4 — motion axis sampling.
const PLATE_AXES_STREAM: &str = "tectonics.plate_axes";
/// Doc 06 §4.4 — log-normal motion rate sampling.
const PLATE_RATES_STREAM: &str = "tectonics.plate_rates";

/// Performs initial plate generation. Mutates `data.plate_id` for every hex and returns
/// the `PlateRegistry`. Should be called exactly once per world, at year 0.
pub fn generate_initial_plates_data(data: &mut WorldData, rng: &WorldRng) -> PlateRegistry {
    let params = &data.parameters.core.geology;
    let major_count = params.initial_major_plate_count as usize;
    let minor_count = params.initial_minor_plate_count as usize;
    let total_cells = data.grid.cell_count() as usize;

    let mut registry = PlateRegistry::new();
    let mut seeds_rng = rng.stream(PLATE_SEEDS_STREAM);

    let major_seeds = place_seeds_poisson_disk(data, major_count, None, &mut seeds_rng);

    let major_target_fractions = sample_major_target_fractions(major_count, 0.50, &mut seeds_rng);

    let mut plate_id_for_hex: Vec<Option<PlateId>> = vec![None; total_cells];
    let mut major_plate_ids = Vec::with_capacity(major_count);

    for (i, &seed_hex) in major_seeds.iter().enumerate() {
        let id = registry.next_id();
        plate_id_for_hex[seed_hex.0 as usize] = Some(id);
        major_plate_ids.push(id);

        let plate = Plate {
            id,
            plate_type: PlateType::Continental,
            plate_class: PlateClass::Major,
            seed_hex,
            motion_axis: [0.0, 0.0, 1.0],
            motion_rate_rad_per_year: 0.0,
            age_year: WorldYear::FORMATION,
            target_fraction: major_target_fractions[i],
            accumulated_rotation_rad: 0.0,
            last_nonempty_year: WorldYear::FORMATION,
            surface: PlateSurface::new(total_cells),
        };
        registry.insert(plate);
    }

    let major_growth_target = (total_cells as f64 * 0.50) as usize;
    grow_plates_to_coverage(
        data,
        &registry,
        &mut plate_id_for_hex,
        &major_plate_ids,
        major_growth_target,
        &mut seeds_rng,
        true,
    );

    let unowned_hexes: Vec<HexId> = (0..total_cells)
        .filter(|&i| plate_id_for_hex[i].is_none())
        .map(|i| HexId(i as u32))
        .collect();

    let minor_seeds = place_seeds_in_pool(data, &unowned_hexes, minor_count, &mut seeds_rng);

    let minor_target_fractions = sample_minor_target_fractions(minor_count, &mut seeds_rng);

    let mut minor_plate_ids = Vec::with_capacity(minor_count);
    for (i, &seed_hex) in minor_seeds.iter().enumerate() {
        let id = registry.next_id();
        plate_id_for_hex[seed_hex.0 as usize] = Some(id);
        minor_plate_ids.push(id);

        let plate = Plate {
            id,
            plate_type: PlateType::Continental,
            plate_class: PlateClass::Minor,
            seed_hex,
            motion_axis: [0.0, 0.0, 1.0],
            motion_rate_rad_per_year: 0.0,
            age_year: WorldYear::FORMATION,
            target_fraction: minor_target_fractions[i],
            accumulated_rotation_rad: 0.0,
            last_nonempty_year: WorldYear::FORMATION,
            surface: PlateSurface::new(total_cells),
        };
        registry.insert(plate);
    }

    let all_plate_ids: Vec<PlateId> = major_plate_ids
        .iter()
        .chain(minor_plate_ids.iter())
        .copied()
        .collect();
    grow_plates_to_coverage(
        data,
        &registry,
        &mut plate_id_for_hex,
        &all_plate_ids,
        total_cells,
        &mut seeds_rng,
        false,
    );

    debug_assert!(plate_id_for_hex.iter().all(|p| p.is_some()));

    assign_plate_types(data, &mut registry);
    assign_plate_motion(data, rng, &mut registry, &plate_id_for_hex);

    for (i, plate_opt) in plate_id_for_hex.iter().enumerate() {
        data.plate_id[i] = plate_opt.expect("all hexes have plates after growth");
    }

    tracing::info!(
        plate_count = registry.count(),
        major = major_count,
        minor = minor_count,
        "WorldFormation: initial plate generation complete"
    );

    registry
}

/// Convenience wrapper using a full [`World`].
pub fn generate_initial_plates(world: &mut World) -> PlateRegistry {
    generate_initial_plates_data(&mut world.data, &world.rng)
}

/// Poisson-disk-like seed placement with relaxed spacing fallback, then deterministic
/// `HexId`-ordered fill. Never inserts duplicate seeds.
fn place_seeds_poisson_disk(
    data: &WorldData,
    count: usize,
    exclude: Option<&BTreeSet<HexId>>,
    rng: &mut rand::rngs::SmallRng,
) -> Vec<HexId> {
    let total = data.grid.cell_count() as usize;
    let grid = &data.grid;

    let mut min_dist_rad = (1.0_f64 - 2.0 / count as f64).acos() * 0.6;
    let mut seeds: Vec<HexId> = Vec::with_capacity(count);
    let max_attempts = count as u32 * 100;

    while seeds.len() < count {
        let mut attempts = 0u32;
        let seeds_before = seeds.len();

        while seeds.len() < count && attempts < max_attempts {
            let candidate_idx = rng.gen_range(0..total);
            let candidate = HexId(candidate_idx as u32);
            attempts += 1;

            if exclude.is_some_and(|ex| ex.contains(&candidate)) || seeds.contains(&candidate) {
                continue;
            }

            if !too_close_to_seeds(grid, candidate, &seeds, min_dist_rad) {
                seeds.push(candidate);
            }
        }

        if seeds.len() == count {
            break;
        }

        if seeds.len() == seeds_before {
            fill_seeds_deterministic(grid, count, exclude, &mut seeds, min_dist_rad);
            if seeds.len() < count {
                min_dist_rad *= 0.85;
            }
        } else {
            min_dist_rad *= 0.85;
        }
    }

    seeds
}

fn place_seeds_in_pool(
    data: &WorldData,
    pool: &[HexId],
    count: usize,
    rng: &mut rand::rngs::SmallRng,
) -> Vec<HexId> {
    let grid = &data.grid;
    let mut available: Vec<HexId> = pool.to_vec();
    available.shuffle(rng);

    let mut min_dist_rad = (1.0_f64 - 2.0 / count as f64).acos() * 0.4;
    let mut seeds: Vec<HexId> = Vec::with_capacity(count);

    while seeds.len() < count {
        let seeds_before = seeds.len();

        for &candidate in &available {
            if seeds.len() == count {
                break;
            }
            if seeds.contains(&candidate) {
                continue;
            }
            if !too_close_to_seeds(grid, candidate, &seeds, min_dist_rad) {
                seeds.push(candidate);
            }
        }

        if seeds.len() == count {
            break;
        }

        if seeds.len() == seeds_before {
            let exclude: BTreeSet<HexId> = seeds.iter().copied().collect();
            fill_seeds_deterministic(grid, count, Some(&exclude), &mut seeds, min_dist_rad);
            if seeds.len() < count {
                min_dist_rad *= 0.85;
            }
        } else {
            min_dist_rad *= 0.85;
        }
    }

    seeds
}

/// Deterministic fill: walk pool hexes in ascending `HexId` order, respect spacing.
fn fill_seeds_deterministic(
    grid: &HexGrid,
    count: usize,
    exclude: Option<&BTreeSet<HexId>>,
    seeds: &mut Vec<HexId>,
    min_dist_rad: f64,
) {
    let total = grid.cell_count() as usize;
    for i in 0..total {
        if seeds.len() >= count {
            break;
        }
        let candidate = HexId(i as u32);
        if exclude.is_some_and(|ex| ex.contains(&candidate)) || seeds.contains(&candidate) {
            continue;
        }
        if !too_close_to_seeds(grid, candidate, seeds, min_dist_rad) {
            seeds.push(candidate);
        }
    }
}

fn too_close_to_seeds(
    grid: &HexGrid,
    candidate: HexId,
    seeds: &[HexId],
    min_dist_rad: f64,
) -> bool {
    let cand_pos = grid.cell_center_direction(candidate);
    let cand_vec = DVec3::new(cand_pos[0], cand_pos[1], cand_pos[2]);

    seeds.iter().any(|&existing| {
        let existing_pos = grid.cell_center_direction(existing);
        let existing_vec = DVec3::new(existing_pos[0], existing_pos[1], existing_pos[2]);
        let dot = cand_vec.dot(existing_vec).clamp(-1.0, 1.0);
        dot.acos() < min_dist_rad
    })
}

fn sample_major_target_fractions(
    count: usize,
    total_target: f32,
    rng: &mut rand::rngs::SmallRng,
) -> Vec<f32> {
    let mean = total_target / count as f32;

    let mut fractions: Vec<f32> = (0..count)
        .map(|_| {
            let variation: f32 = rng.gen_range(0.5..1.5);
            mean * variation
        })
        .collect();

    let sum: f32 = fractions.iter().sum();
    if sum > 0.0 {
        for f in &mut fractions {
            *f *= total_target / sum;
        }
    }

    fractions
}

/// Doc 06 §2.2 — each minor plate ~0.03–0.07 of the sphere (not a shared 0.50 budget).
fn sample_minor_target_fractions(count: usize, rng: &mut rand::rngs::SmallRng) -> Vec<f32> {
    (0..count).map(|_| rng.gen_range(0.03..=0.07)).collect()
}

fn grow_plates_to_coverage(
    data: &WorldData,
    registry: &PlateRegistry,
    plate_id_for_hex: &mut [Option<PlateId>],
    active_plate_ids: &[PlateId],
    target_coverage: usize,
    rng: &mut rand::rngs::SmallRng,
    enforce_per_plate_budget: bool,
) {
    let grid = &data.grid;
    let total_cells = plate_id_for_hex.len();

    let mut owned_hexes: BTreeMap<PlateId, Vec<HexId>> = active_plate_ids
        .iter()
        .map(|&id| (id, Vec::new()))
        .collect();
    let mut current_size: BTreeMap<PlateId, usize> =
        active_plate_ids.iter().map(|&id| (id, 0usize)).collect();

    for (i, plate_opt) in plate_id_for_hex.iter().enumerate() {
        if let Some(id) = plate_opt {
            owned_hexes
                .get_mut(id)
                .expect("plate tracked")
                .push(HexId(i as u32));
            *current_size.get_mut(id).expect("plate tracked") += 1;
        }
    }

    let mut total_owned = current_size.values().sum::<usize>();

    while total_owned < target_coverage {
        let Some(plate_id) = ({
            let growth_ctx = GrowthContext {
                registry,
                grid,
                active_plate_ids,
                owned_hexes: &owned_hexes,
                plate_id_for_hex,
                current_size: &current_size,
                total_cells,
                enforce_per_plate_budget,
            };
            pick_next_plate_to_grow(&growth_ctx, rng)
        }) else {
            break;
        };

        let owned = owned_hexes.get(&plate_id).expect("plate tracked");
        let hex = find_growth_candidate(grid, owned, plate_id_for_hex, rng)
            .expect("picked plate must have a growth candidate");
        plate_id_for_hex[hex.0 as usize] = Some(plate_id);
        owned_hexes
            .get_mut(&plate_id)
            .expect("plate tracked")
            .push(hex);
        *current_size.get_mut(&plate_id).expect("plate id valid") += 1;
        total_owned += 1;
    }
}

struct GrowthContext<'a> {
    registry: &'a PlateRegistry,
    grid: &'a HexGrid,
    active_plate_ids: &'a [PlateId],
    owned_hexes: &'a BTreeMap<PlateId, Vec<HexId>>,
    plate_id_for_hex: &'a [Option<PlateId>],
    current_size: &'a BTreeMap<PlateId, usize>,
    total_cells: usize,
    enforce_per_plate_budget: bool,
}

fn pick_next_plate_to_grow(
    ctx: &GrowthContext<'_>,
    rng: &mut rand::rngs::SmallRng,
) -> Option<PlateId> {
    let mut eligible: Vec<PlateId> = ctx
        .active_plate_ids
        .iter()
        .copied()
        .filter(|&id| {
            if ctx.enforce_per_plate_budget {
                let budget = ctx
                    .registry
                    .get(id)
                    .map(|p| (p.target_fraction * ctx.total_cells as f32) as usize)
                    .unwrap_or(0);
                if ctx.current_size.get(&id).copied().unwrap_or(0) >= budget {
                    return false;
                }
            }
            let owned = ctx.owned_hexes.get(&id).map(Vec::as_slice).unwrap_or(&[]);
            find_growth_candidate(ctx.grid, owned, ctx.plate_id_for_hex, rng).is_some()
        })
        .collect();
    eligible.sort_by_key(|id| id.0);

    if eligible.is_empty() {
        return None;
    }

    let total_hexes: usize = ctx.current_size.values().sum::<usize>().max(1);

    let mut weights: Vec<(PlateId, f64)> = eligible
        .iter()
        .map(|&id| {
            let target = ctx
                .registry
                .get(id)
                .map(|p| p.target_fraction as f64)
                .unwrap_or(0.0);
            let current =
                ctx.current_size.get(&id).copied().unwrap_or(0) as f64 / total_hexes as f64;
            let neediness = (target - current).max(0.0);
            (id, neediness)
        })
        .collect();
    weights.sort_by_key(|(id, _)| id.0);

    let total_weight: f64 = weights.iter().map(|(_, w)| w).sum();

    if total_weight <= 0.0 {
        return Some(eligible[rng.gen_range(0..eligible.len())]);
    }

    let mut roll = rng.gen_range(0.0..total_weight);
    for (id, w) in &weights {
        roll -= *w;
        if roll <= 0.0 {
            return Some(*id);
        }
    }

    Some(weights.last().expect("non-empty").0)
}

/// Doc 06 §2.2 — random unowned frontier neighbor; candidates sorted by `HexId` before draw.
fn find_growth_candidate(
    grid: &HexGrid,
    owned: &[HexId],
    plate_id_for_hex: &[Option<PlateId>],
    rng: &mut rand::rngs::SmallRng,
) -> Option<HexId> {
    let mut candidates: Vec<HexId> = Vec::new();

    for &hex in owned {
        for &neighbor in grid.neighbors(hex) {
            if plate_id_for_hex[neighbor.0 as usize].is_none() {
                candidates.push(neighbor);
            }
        }
    }

    candidates.sort_by_key(|h| h.0);
    candidates.dedup();

    if candidates.is_empty() {
        return None;
    }

    Some(candidates[rng.gen_range(0..candidates.len())])
}

fn assign_plate_types(data: &WorldData, registry: &mut PlateRegistry) {
    let continental_fraction = data.parameters.core.geology.initial_continental_fraction;
    let total = registry.count();
    let num_continental = ((total as f32) * continental_fraction).round() as usize;

    let mut plate_ids: Vec<PlateId> = registry.iter().map(|p| p.id).collect();
    plate_ids.sort_by_key(|id| {
        let p = registry.get(*id).unwrap();
        (
            match p.plate_class {
                PlateClass::Major => 0,
                PlateClass::Minor => 1,
            },
            id.0,
        )
    });

    let mut updates = Vec::new();
    for (i, &id) in plate_ids.iter().enumerate() {
        let plate_type = if i < num_continental {
            PlateType::Continental
        } else {
            PlateType::Oceanic
        };
        updates.push((id, plate_type));
    }

    for (id, plate_type) in updates {
        if let Some(plate) = registry.plates_mut().get_mut(&id) {
            plate.plate_type = plate_type;
        }
    }
}

fn assign_plate_motion(
    data: &WorldData,
    rng: &WorldRng,
    registry: &mut PlateRegistry,
    plate_id_for_hex: &[Option<PlateId>],
) {
    let mut axes_rng = rng.stream(PLATE_AXES_STREAM);
    let mut rates_rng = rng.stream(PLATE_RATES_STREAM);
    let params = &data.parameters.core.geology;
    let planet_params = &data.parameters.core.planet;
    let grid = &data.grid;

    let rotation_factor = (24.0 / planet_params.rotation_period_hours).sqrt();
    let effective_scale = params.plate_velocity_scale as f64 * rotation_factor;

    let median_cm_per_year = 5.0 * effective_scale;
    let sigma: f64 = 0.6;

    let mut plate_ids: Vec<PlateId> = registry.iter().map(|p| p.id).collect();
    plate_ids.sort_by_key(|id| id.0);

    for id in plate_ids {
        let centroid = compute_plate_centroid(grid, id, plate_id_for_hex);
        let axis = crate::motion::sample_motion_axis(centroid, &mut axes_rng);

        let log_sample: f64 = sample_log_normal(&mut rates_rng, sigma);
        let mut rate_cm_per_year = median_cm_per_year * log_sample;

        if let Some(plate) = registry.get(id)
            && plate.plate_type == PlateType::Continental
        {
            rate_cm_per_year *= 0.7;
        }

        let rate_rad_per_year = (rate_cm_per_year * 1e-5) / planet_params.radius_km;

        if let Some(plate) = registry.plates_mut().get_mut(&id) {
            plate.motion_axis = [axis.x, axis.y, axis.z];
            plate.motion_rate_rad_per_year = rate_rad_per_year;
        }
    }
}

/// Mean of unit center directions for all hexes owned by `plate_id`.
fn compute_plate_centroid(
    grid: &HexGrid,
    plate_id: PlateId,
    plate_id_for_hex: &[Option<PlateId>],
) -> DVec3 {
    let mut sum = DVec3::ZERO;
    let mut n = 0u32;

    for (i, plate_opt) in plate_id_for_hex.iter().enumerate() {
        if *plate_opt == Some(plate_id) {
            let pos = grid.cell_center_direction(HexId(i as u32));
            sum += DVec3::new(pos[0], pos[1], pos[2]);
            n += 1;
        }
    }

    debug_assert!(n > 0, "plate must own at least one hex");
    if n == 0 {
        return DVec3::Z;
    }
    sum.normalize()
}

/// Whether `axis` satisfies §2.1 angular-distance constraints relative to `centroid`.
#[cfg_attr(not(test), allow(dead_code))]
fn axis_satisfies_centroid_constraint(axis: DVec3, centroid: DVec3) -> bool {
    use std::f64::consts::PI;

    let axis = axis.normalize();
    let centroid = centroid.normalize();
    if axis.dot(DVec3::Z).abs() > 0.95 {
        return false;
    }
    let dist = axis.dot(centroid).clamp(-1.0, 1.0).acos();
    (PI * 30.0 / 180.0..=PI * 150.0 / 180.0).contains(&dist)
}

fn sample_log_normal(rng: &mut rand::rngs::SmallRng, sigma: f64) -> f64 {
    use std::f64::consts::PI;
    let u1: f64 = rng.gen_range(1e-10..1.0);
    let u2: f64 = rng.gen_range(0.0..1.0);
    let z = (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos();
    (z * sigma).exp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::{WorldParameters, WorldSeed, create_world};

    fn build_test_world() -> World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        create_world(params).expect("default world valid")
    }

    #[test]
    fn generates_expected_plate_count() {
        let mut world = build_test_world();
        let registry = generate_initial_plates(&mut world);
        let expected_total = (world.data.parameters.core.geology.initial_major_plate_count
            + world.data.parameters.core.geology.initial_minor_plate_count)
            as usize;
        assert_eq!(registry.count(), expected_total);
    }

    #[test]
    fn every_hex_has_a_plate() {
        let mut world = build_test_world();
        let _registry = generate_initial_plates(&mut world);
        for plate_id in &world.data.plate_id {
            assert_ne!(*plate_id, PlateId::NONE);
        }
    }

    #[test]
    fn plate_ids_are_valid() {
        let mut world = build_test_world();
        let registry = generate_initial_plates(&mut world);
        for plate_id in &world.data.plate_id {
            assert!(registry.get(*plate_id).is_some());
        }
    }

    #[test]
    fn major_minor_split_correct() {
        let mut world = build_test_world();
        let registry = generate_initial_plates(&mut world);
        let expected_major = world.data.parameters.core.geology.initial_major_plate_count as usize;
        let expected_minor = world.data.parameters.core.geology.initial_minor_plate_count as usize;
        let major_count = registry
            .iter()
            .filter(|p| p.plate_class == PlateClass::Major)
            .count();
        let minor_count = registry
            .iter()
            .filter(|p| p.plate_class == PlateClass::Minor)
            .count();
        assert_eq!(major_count, expected_major);
        assert_eq!(minor_count, expected_minor);
    }

    #[test]
    fn minor_target_fractions_in_doc_range() {
        let mut world = build_test_world();
        let registry = generate_initial_plates(&mut world);
        for plate in registry
            .iter()
            .filter(|p| p.plate_class == PlateClass::Minor)
        {
            assert!(
                (0.03..=0.07).contains(&plate.target_fraction),
                "minor plate {:?} target_fraction {} outside 0.03..=0.07",
                plate.id,
                plate.target_fraction
            );
        }
    }

    #[test]
    fn continental_fraction_approximate() {
        let mut world = build_test_world();
        let registry = generate_initial_plates(&mut world);
        let target_fraction = world
            .data
            .parameters
            .core
            .geology
            .initial_continental_fraction;
        let total = registry.count() as f32;
        let continental = registry
            .iter()
            .filter(|p| p.plate_type == PlateType::Continental)
            .count() as f32;
        let actual_fraction = continental / total;
        let tolerance = 1.0 / total;
        assert!(
            (actual_fraction - target_fraction).abs() <= tolerance,
            "expected ~{target_fraction}, got {actual_fraction}"
        );
    }

    #[test]
    fn determinism_same_seed_same_result() {
        let mut world_a = build_test_world();
        let mut world_b = build_test_world();
        let registry_a = generate_initial_plates(&mut world_a);
        let registry_b = generate_initial_plates(&mut world_b);

        assert_eq!(registry_a.count(), registry_b.count());
        for hex_idx in 0..world_a.data.plate_id.len() {
            assert_eq!(
                world_a.data.plate_id[hex_idx],
                world_b.data.plate_id[hex_idx]
            );
        }
    }

    #[test]
    fn different_seeds_produce_different_results() {
        let mut params_a = WorldParameters::default();
        params_a.core.grid.subdivision_level = 5;
        params_a.core.seed = WorldSeed::from_integer(1);

        let mut params_b = WorldParameters::default();
        params_b.core.grid.subdivision_level = 5;
        params_b.core.seed = WorldSeed::from_integer(2);

        let mut world_a = create_world(params_a).unwrap();
        let mut world_b = create_world(params_b).unwrap();

        generate_initial_plates(&mut world_a);
        generate_initial_plates(&mut world_b);

        let mut differences = 0;
        for hex_idx in 0..world_a.data.plate_id.len() {
            if world_a.data.plate_id[hex_idx] != world_b.data.plate_id[hex_idx] {
                differences += 1;
            }
        }

        let threshold = world_a.data.plate_id.len() / 2;
        assert!(
            differences > threshold,
            "expected substantial differences between seeds, got {differences}/{}",
            world_a.data.plate_id.len()
        );
    }

    #[test]
    fn motion_axes_are_unit_length() {
        let mut world = build_test_world();
        let registry = generate_initial_plates(&mut world);
        for plate in registry.iter() {
            let axis = DVec3::new(
                plate.motion_axis[0],
                plate.motion_axis[1],
                plate.motion_axis[2],
            );
            let len = axis.length();
            assert!(
                (len - 1.0).abs() < 1e-6,
                "plate {:?} motion axis length {len}",
                plate.id
            );
        }
    }

    #[test]
    fn motion_axis_constraint_uses_centroid_not_seed() {
        let seed_vec = DVec3::new(1.0, 0.0, 0.0);
        let centroid = DVec3::new(0.0, 1.0, 0.0);
        // Axis ~20° from +X (seed): too close to seed for §2.1, but ~70° from +Y centroid: valid.
        let axis = DVec3::new(0.94, 0.34, 0.0).normalize();

        assert!(!axis_satisfies_centroid_constraint(axis, seed_vec));
        assert!(axis_satisfies_centroid_constraint(axis, centroid));

        let mut world = build_test_world();
        let registry = generate_initial_plates(&mut world);
        let plate_id_for_hex: Vec<Option<PlateId>> =
            world.data.plate_id.iter().map(|&id| Some(id)).collect();
        let grid = &world.data.grid;

        for plate in registry.iter() {
            let centroid = compute_plate_centroid(grid, plate.id, &plate_id_for_hex);
            let axis = DVec3::new(
                plate.motion_axis[0],
                plate.motion_axis[1],
                plate.motion_axis[2],
            );
            assert!(
                axis_satisfies_centroid_constraint(axis, centroid),
                "plate {:?} axis must satisfy centroid constraint",
                plate.id
            );
        }

        let has_offset_centroid = registry.iter().any(|p| {
            let seed_pos = grid.cell_center_direction(p.seed_hex);
            let seed = DVec3::new(seed_pos[0], seed_pos[1], seed_pos[2]);
            let centroid = compute_plate_centroid(grid, p.id, &plate_id_for_hex);
            seed.dot(centroid).abs() < 0.99
        });
        assert!(
            has_offset_centroid,
            "expected at least one plate whose centroid differs from seed direction"
        );
    }

    #[test]
    fn motion_rates_are_reasonable() {
        let mut world = build_test_world();
        let registry = generate_initial_plates(&mut world);
        for plate in registry.iter() {
            assert!(plate.motion_rate_rad_per_year > 0.0);
            let planet_radius = world.data.parameters.core.planet.radius_km;
            let cm_per_year = plate.motion_rate_rad_per_year * planet_radius / 1e-5;
            assert!(
                cm_per_year > 0.05 && cm_per_year < 50.0,
                "plate {:?} velocity {} cm/year out of range",
                plate.id,
                cm_per_year
            );
        }
    }

    #[test]
    fn no_pentagon_special_handling_needed() {
        let mut world = build_test_world();
        let _registry = generate_initial_plates(&mut world);
        for pentagon_id in 0..12u32 {
            assert_ne!(world.data.plate_id[pentagon_id as usize], PlateId::NONE);
        }
    }

    #[test]
    fn poisson_seed_placement_completes_without_duplicates() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        params.core.geology.initial_major_plate_count = 9;
        params.core.geology.initial_minor_plate_count = 10;
        let world = create_world(params).expect("valid");
        let mut rng = world.rng.stream(PLATE_SEEDS_STREAM);

        let seeds = place_seeds_poisson_disk(&world.data, 9, None, &mut rng);
        assert_eq!(seeds.len(), 9);
        let unique: BTreeSet<HexId> = seeds.iter().copied().collect();
        assert_eq!(unique.len(), 9, "seeds must be unique");

        let grid = &world.data.grid;
        let min_dist = (1.0_f64 - 2.0 / 9.0).acos() * 0.6 * 0.85_f64.powi(3);
        for i in 0..seeds.len() {
            for j in (i + 1)..seeds.len() {
                let a = grid.cell_center_direction(seeds[i]);
                let b = grid.cell_center_direction(seeds[j]);
                let av = DVec3::new(a[0], a[1], a[2]);
                let bv = DVec3::new(b[0], b[1], b[2]);
                let dist = av.dot(bv).clamp(-1.0, 1.0).acos();
                assert!(
                    dist >= min_dist * 0.95,
                    "seeds {:?} and {:?} closer than relaxed minimum ({dist} < {min_dist})",
                    seeds[i],
                    seeds[j]
                );
            }
        }
    }

    #[test]
    fn poisson_pool_placement_completes_on_constrained_grid() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let world = create_world(params).expect("valid");
        let pool: Vec<HexId> = (100..200).map(HexId).collect();
        let mut rng = world.rng.stream(PLATE_SEEDS_STREAM);
        let seeds = place_seeds_in_pool(&world.data, &pool, 8, &mut rng);
        assert_eq!(seeds.len(), 8);
        assert!(seeds.iter().all(|h| pool.contains(h)));
        let unique: BTreeSet<_> = seeds.iter().copied().collect();
        assert_eq!(unique.len(), 8);
    }
}
