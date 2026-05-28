//! Plate reorganization: split, merge, motion change (Doc 06 §4.5).

use std::collections::{BTreeMap, BTreeSet};

use genesis_core::branches::BranchId;
use genesis_core::data::WorldData;
use genesis_core::events::{Event, EventKind, EventLocation, PlateReorgAction, Significance};
use genesis_core::rng::WorldRng;
use genesis_core::time::WorldYear;
use genesis_core::{HexGrid, HexId, PlateId};
use glam::DVec3;
use rand::Rng;

use crate::events::{alloc_event_id, maybe_emit};
use crate::motion::sample_motion_axis;
use crate::partition::repartition_hexes;
use crate::plate::{Plate, PlateClass, PlateRegistry, PlateType, TectonicsState};
use crate::plate_surface::modify_surface_at_world_hex;

/// Per-tick reorganization probability gate (§4.5).
pub const REORGANIZATION_CHECK_STREAM: &str = "tectonics.reorganization_check";

/// Chooses split / merge / motion-change action (§4.5).
pub const REORGANIZATION_ACTION_STREAM: &str = "tectonics.reorganization_action";

/// Base probability of reorganization per Geological tick at scale 1.0.
const REORGANIZATION_PROBABILITY_BASE: f64 = 0.001;

/// Plates empty for at least this many years are purged (§12.1).
const EXTINCT_PLATE_YEARS: i64 = 10_000_000;

/// Minimum fraction of grid cells for a plate to qualify for split.
const LARGE_PLATE_FRACTION: f64 = 0.05;

/// Mild subsidence on continental hexes along the new split boundary (m).
const SPLIT_BOUNDARY_SUBSIDENCE_M: f32 = 50.0;

/// Attempts a plate reorganization; returns whether one fired.
pub fn maybe_reorganize(
    data: &mut WorldData,
    state: &mut TectonicsState,
    rng: &WorldRng,
    tick_year: WorldYear,
    event_granularity: Significance,
    branch_id: BranchId,
) -> bool {
    let scale = f64::from(data.parameters.core.geology.geology_activity_scale);
    if scale <= 0.0 {
        return false;
    }

    let mut check_rng = rng.stream_at(REORGANIZATION_CHECK_STREAM, tick_year.value() as u64);
    let roll: f64 = check_rng.gen_range(0.0..1.0);
    if roll >= REORGANIZATION_PROBABILITY_BASE * scale {
        return false;
    }

    let mut action_rng = rng.stream_at(REORGANIZATION_ACTION_STREAM, tick_year.value() as u64);
    let action_roll: f64 = action_rng.gen_range(0.0..1.0);

    let result = if action_roll < 0.4 {
        apply_split(data, &mut state.registry, tick_year, &mut action_rng)
    } else if action_roll < 0.8 {
        apply_motion_change(data, &mut state.registry, tick_year, &mut action_rng)
    } else {
        apply_merge(data, &mut state.registry, tick_year, &mut action_rng)
    };

    let Some((action, affected_plates)) = result else {
        return false;
    };

    let split_pair = match action {
        PlateReorgAction::Split { parent, child } => Some((parent, child)),
        _ => None,
    };

    repartition_hexes(data, &mut state.registry);

    if let Some((parent, child)) = split_pair {
        apply_split_boundary_subsidence(data, &mut state.registry, parent, child);
    }
    update_last_nonempty_years(data, &mut state.registry, tick_year);
    purge_extinct_plates(&mut state.registry, data, tick_year);

    let event_id = alloc_event_id(state);
    maybe_emit(
        state,
        Event {
            id: event_id,
            year: tick_year,
            branch_id,
            location: EventLocation::Global,
            significance: Significance::Pivotal,
            kind: EventKind::PlateReorganization {
                action,
                affected_plates,
            },
        },
        event_granularity,
    );

    true
}

fn apply_split(
    data: &mut WorldData,
    registry: &mut PlateRegistry,
    tick_year: WorldYear,
    rng: &mut rand::rngs::SmallRng,
) -> Option<(PlateReorgAction, Vec<PlateId>)> {
    let counts = hex_counts(data);
    let cell_count = data.plate_id.len();
    let min_hexes = ((cell_count as f64) * LARGE_PLATE_FRACTION).ceil() as usize;
    let min_hexes = min_hexes.max(1);

    let mut candidates: Vec<PlateId> = counts
        .iter()
        .filter(|(_, c)| **c >= min_hexes)
        .map(|(&id, _)| id)
        .collect();
    candidates.sort_by_key(|id| id.0);
    let parent_id = *candidates.first()?;

    let parent = registry.get(parent_id)?.clone();
    let child_seed = farthest_hex_from_seed(&data.grid, &parent, data)?;
    let child_id = registry.next_id();

    let perturb = rng.gen_range(-0.15..=0.15);
    let parent_axis = DVec3::new(
        parent.motion_axis[0],
        parent.motion_axis[1],
        parent.motion_axis[2],
    );
    let child_axis = (parent_axis + DVec3::new(perturb, perturb * 0.5, -perturb)).normalize();

    let child = Plate {
        id: child_id,
        plate_type: parent.plate_type,
        plate_class: PlateClass::Minor,
        seed_hex: child_seed,
        motion_axis: [child_axis.x, child_axis.y, child_axis.z],
        motion_rate_rad_per_year: parent.motion_rate_rad_per_year,
        age_year: tick_year,
        target_fraction: parent.target_fraction * 0.5,
        accumulated_rotation_rad: parent.accumulated_rotation_rad,
        last_nonempty_year: tick_year,
        surface: parent.surface.clone(),
    };
    registry.insert(child);

    if let Some(parent_mut) = registry.plates_mut().get_mut(&parent_id) {
        parent_mut.age_year = tick_year;
        parent_mut.last_nonempty_year = tick_year;
    }

    Some((
        PlateReorgAction::Split {
            parent: parent_id,
            child: child_id,
        },
        vec![parent_id, child_id],
    ))
}

fn apply_motion_change(
    data: &WorldData,
    registry: &mut PlateRegistry,
    tick_year: WorldYear,
    rng: &mut rand::rngs::SmallRng,
) -> Option<(PlateReorgAction, Vec<PlateId>)> {
    let counts = hex_counts(data);
    let mut plate_ids: Vec<PlateId> = counts.keys().copied().collect();
    plate_ids.sort_by_key(|id| id.0);
    if plate_ids.is_empty() {
        return None;
    }
    let idx = rng.gen_range(0..plate_ids.len());
    let plate_id = plate_ids[idx];

    let plate_type = registry.get(plate_id)?.plate_type;
    let centroid = plate_centroid(&data.grid, plate_id, data);
    let axis = sample_motion_axis(centroid, rng);
    let rate = sample_motion_rate_rad_per_year(
        plate_type,
        &data.parameters.core.geology,
        &data.parameters.core.planet,
        rng,
    );

    let plate = registry.plates_mut().get_mut(&plate_id)?;
    plate.motion_axis = [axis.x, axis.y, axis.z];
    plate.motion_rate_rad_per_year = rate;
    plate.age_year = tick_year;
    plate.last_nonempty_year = tick_year;
    let new_axis = plate.motion_axis;
    let new_rate = plate.motion_rate_rad_per_year;

    Some((
        PlateReorgAction::MotionChange {
            plate: plate_id,
            new_axis,
            new_rate,
        },
        vec![plate_id],
    ))
}

fn apply_merge(
    data: &mut WorldData,
    registry: &mut PlateRegistry,
    tick_year: WorldYear,
    rng: &mut rand::rngs::SmallRng,
) -> Option<(PlateReorgAction, Vec<PlateId>)> {
    let pairs = adjacent_plate_pairs(data);
    if pairs.is_empty() {
        return None;
    }
    let idx = rng.gen_range(0..pairs.len());
    let (into, absorbed) = pairs[idx];

    reassign_plate_hexes(data, absorbed, into);

    if let Some(absorbed_plate) = registry.get(absorbed) {
        let absorbed_surface = absorbed_plate.surface.clone();
        if let Some(into_plate) = registry.plates_mut().get_mut(&into) {
            into_plate.surface.merge_from(&absorbed_surface);
        }
    }

    registry.remove(absorbed);

    if let Some(plate) = registry.plates_mut().get_mut(&into) {
        plate.age_year = tick_year;
        plate.last_nonempty_year = tick_year;
    }

    Some((
        PlateReorgAction::Merge { absorbed, into },
        vec![into, absorbed],
    ))
}

fn adjacent_plate_pairs(data: &WorldData) -> Vec<(PlateId, PlateId)> {
    let grid = &data.grid;
    let n = data.plate_id.len();
    let mut pairs = BTreeSet::new();

    for i in 0..n {
        let hex = HexId(i as u32);
        let owner = data.plate_id[i];
        if owner == PlateId::NONE {
            continue;
        }
        let mut neighbors: Vec<HexId> = grid.neighbors(hex).to_vec();
        neighbors.sort_by_key(|h| h.0);
        for neighbor_hex in neighbors {
            let j = neighbor_hex.0 as usize;
            if j >= n {
                continue;
            }
            let other = data.plate_id[j];
            if other == PlateId::NONE || other == owner {
                continue;
            }
            let (min_id, max_id) = if owner < other {
                (owner, other)
            } else {
                (other, owner)
            };
            pairs.insert((min_id, max_id));
        }
    }

    pairs.into_iter().collect()
}

fn reassign_plate_hexes(data: &mut WorldData, from: PlateId, to: PlateId) {
    for pid in data.plate_id.iter_mut() {
        if *pid == from {
            *pid = to;
        }
    }
}

fn apply_split_boundary_subsidence(
    data: &WorldData,
    registry: &mut PlateRegistry,
    parent: PlateId,
    child: PlateId,
) {
    let grid = &data.grid;
    let n = data.plate_id.len();
    for i in 0..n {
        if data.plate_id[i] != parent && data.plate_id[i] != child {
            continue;
        }
        let owner = data.plate_id[i];
        let is_continental = registry
            .get(owner)
            .is_some_and(|p| p.plate_type == PlateType::Continental);
        if !is_continental {
            continue;
        }
        let hex = HexId(i as u32);
        let owner = data.plate_id[i];
        let mut neighbors: Vec<HexId> = grid.neighbors(hex).to_vec();
        neighbors.sort_by_key(|h| h.0);
        for neighbor_hex in neighbors {
            let j = neighbor_hex.0 as usize;
            if j >= n {
                continue;
            }
            let other_plate = data.plate_id[j];
            if (owner == parent && other_plate == child)
                || (owner == child && other_plate == parent)
            {
                modify_surface_at_world_hex(registry, data, hex, 0, |feature| {
                    feature.elevation_m -= SPLIT_BOUNDARY_SUBSIDENCE_M;
                });
            }
        }
    }
}

/// Updates `last_nonempty_year` for every plate that owns at least one hex.
pub fn update_last_nonempty_years(
    data: &WorldData,
    registry: &mut PlateRegistry,
    tick_year: WorldYear,
) {
    let counts = hex_counts(data);
    for (id, &count) in &counts {
        if count > 0
            && let Some(plate) = registry.plates_mut().get_mut(id)
        {
            plate.last_nonempty_year = tick_year;
        }
    }
}

/// Removes plates that have been empty for at least [`EXTINCT_PLATE_YEARS`].
pub fn purge_extinct_plates(registry: &mut PlateRegistry, data: &WorldData, tick_year: WorldYear) {
    let counts = hex_counts(data);
    let mut to_remove = Vec::new();
    for plate in registry.iter() {
        if counts.get(&plate.id).copied().unwrap_or(0) > 0 {
            continue;
        }
        let empty_years = tick_year.value() - plate.last_nonempty_year.value();
        if empty_years >= EXTINCT_PLATE_YEARS {
            to_remove.push(plate.id);
        }
    }
    to_remove.sort_by_key(|id| id.0);
    for id in to_remove {
        registry.remove(id);
    }
}

fn hex_counts(data: &WorldData) -> BTreeMap<PlateId, usize> {
    let mut counts = BTreeMap::new();
    for &pid in &data.plate_id {
        if pid != PlateId::NONE {
            *counts.entry(pid).or_insert(0) += 1;
        }
    }
    counts
}

fn farthest_hex_from_seed(grid: &HexGrid, plate: &Plate, data: &WorldData) -> Option<HexId> {
    let seed_dir = grid.cell_center_direction(plate.seed_hex);
    let seed_v = DVec3::new(seed_dir[0], seed_dir[1], seed_dir[2]);
    let mut best_hex = None;
    let mut best_dist = f64::NEG_INFINITY;

    for (i, &pid) in data.plate_id.iter().enumerate() {
        if pid != plate.id {
            continue;
        }
        let hex = HexId(i as u32);
        let dir = grid.cell_center_direction(hex);
        let v = DVec3::new(dir[0], dir[1], dir[2]);
        let dist = seed_v.dot(v).clamp(-1.0, 1.0).acos();
        if dist > best_dist {
            best_dist = dist;
            best_hex = Some(hex);
        }
    }
    best_hex
}

fn plate_centroid(grid: &HexGrid, plate_id: PlateId, data: &WorldData) -> DVec3 {
    let mut sum = DVec3::ZERO;
    let mut n = 0u32;
    for (i, &pid) in data.plate_id.iter().enumerate() {
        if pid != plate_id {
            continue;
        }
        let pos = grid.cell_center_direction(HexId(i as u32));
        sum += DVec3::new(pos[0], pos[1], pos[2]);
        n += 1;
    }
    if n == 0 { DVec3::Z } else { sum.normalize() }
}

fn sample_motion_rate_rad_per_year(
    plate_type: PlateType,
    geology: &genesis_core::parameters::GeologyParameters,
    planet: &genesis_core::parameters::PlanetParameters,
    rng: &mut rand::rngs::SmallRng,
) -> f64 {
    use std::f64::consts::PI;

    let rotation_factor = (24.0 / planet.rotation_period_hours).sqrt();
    let effective_scale = geology.plate_velocity_scale as f64 * rotation_factor;
    let median_cm_per_year = 5.0 * effective_scale;
    let sigma = 0.6;

    let u1: f64 = rng.gen_range(1e-10..1.0);
    let u2: f64 = rng.gen_range(0.0..1.0);
    let z = (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos();
    let mut rate_cm_per_year = median_cm_per_year * (z * sigma).exp();
    if plate_type == PlateType::Continental {
        rate_cm_per_year *= 0.7;
    }
    (rate_cm_per_year * 1e-5) / planet.radius_km
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::create_world;
    use genesis_core::parameters::WorldParameters;

    use crate::history::run_formation;
    use crate::plate::TectonicsState;

    fn test_world() -> genesis_core::World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        create_world(params).expect("world")
    }

    #[test]
    fn forced_reorganization_is_deterministic() {
        let mut world_a = test_world();
        let mut world_b = test_world();
        let mut state_a = TectonicsState::new();
        let mut state_b = TectonicsState::new();
        run_formation(&mut world_a, &mut state_a);
        run_formation(&mut world_b, &mut state_b);

        world_a.data.parameters.core.geology.geology_activity_scale = 10.0;
        world_b.data.parameters.core.geology.geology_activity_scale = 10.0;

        let year = WorldYear(500_000);
        let fired_a = maybe_reorganize(
            &mut world_a.data,
            &mut state_a,
            &world_a.rng,
            year,
            Significance::Trace,
            BranchId::ROOT,
        );
        let fired_b = maybe_reorganize(
            &mut world_b.data,
            &mut state_b,
            &world_b.rng,
            year,
            Significance::Trace,
            BranchId::ROOT,
        );

        assert_eq!(fired_a, fired_b);
        assert_eq!(world_a.data.plate_id, world_b.data.plate_id);
    }

    #[test]
    fn all_hexes_remain_assigned_after_reorganization_attempts() {
        let mut world = test_world();
        let mut state = TectonicsState::new();
        run_formation(&mut world, &mut state);
        world.data.parameters.core.geology.geology_activity_scale = 10.0;

        for tick in 1..=20 {
            let year = WorldYear(tick * 500_000);
            let _ = maybe_reorganize(
                &mut world.data,
                &mut state,
                &world.rng,
                year,
                Significance::Trace,
                BranchId::ROOT,
            );
            repartition_hexes(&mut world.data, &mut state.registry);
            update_last_nonempty_years(&world.data, &mut state.registry, year);
        }

        for &pid in &world.data.plate_id {
            assert_ne!(pid, PlateId::NONE);
            assert!(state.registry.get(pid).is_some());
        }
    }
}
