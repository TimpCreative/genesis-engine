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
use crate::frames::rebase_birth_frame;
use crate::motion::{
    effective_position_direction, sample_motion_axis, surface_velocity_m_per_year,
};
use crate::partition::repartition_hexes;
use crate::plate::{Plate, PlateClass, PlateRegistry, PlateType, TectonicsState};
use crate::plate_surface::{PlateSurface, modify_surface_at_world_hex};
use crate::projection::ProjectionCache;

/// Per-tick reorganization probability gate (§4.5).
pub const REORGANIZATION_CHECK_STREAM: &str = "tectonics.reorganization_check";

/// Chooses split / merge / motion-change action (§4.5).
pub const REORGANIZATION_ACTION_STREAM: &str = "tectonics.reorganization_action";

/// Base probability of reorganization per Geological tick at scale 1.0.
/// ~16–30 events per 4.5 B years: the supercontinent cycle paces ~100–300 My.
const REORGANIZATION_PROBABILITY_BASE: f64 = 0.004;

/// Extra reorganization pressure per fully-stalled plate fraction: a welded
/// world (plates creeping at the slab-less drift base) breaks up or redirects
/// sooner than an actively drifting one (Wilson cadence).
const STALL_PRESSURE_FACTOR: f64 = 2.0;

/// A plate creeping below this speed (cm/yr at scale 1.0) counts as stalled.
/// Slab-less plates relax toward the ~1.5 cm/yr drift base
/// ([`crate::motion::DRIFT_BASE_CM_PER_YEAR`]), so welded continents pile up
/// below this line while slab-driven plates sit far above it.
const STALL_CM_PER_YEAR: f64 = 2.0;

/// Stall speed in rad/year for this planet and velocity scaling.
fn stall_threshold_rad_per_year(
    geology: &genesis_core::parameters::GeologyParameters,
    planet: &genesis_core::parameters::PlanetParameters,
) -> f64 {
    let rotation_factor = (24.0 / planet.rotation_period_hours).sqrt();
    let cm_per_year = STALL_CM_PER_YEAR * f64::from(geology.plate_velocity_scale) * rotation_factor;
    (cm_per_year * 1e-5) / planet.radius_km
}

/// Candidate motion axes sampled per split; the most divergent wins (§4.5).
const SPLIT_AXIS_CANDIDATES: usize = 8;

/// Plates empty for at least this many years are purged (§12.1).
const EXTINCT_PLATE_YEARS: i64 = 10_000_000;

/// Minimum fraction of grid cells for a plate to qualify for split.
const LARGE_PLATE_FRACTION: f64 = 0.05;

/// Mild subsidence on continental hexes along the new split boundary (m).
const SPLIT_BOUNDARY_SUBSIDENCE_M: f32 = 50.0;

/// Reorganization probability for this tick: base rate scaled by geology
/// activity and by the stalled-plate fraction (Wilson cadence).
fn reorganization_probability(scale: f64, stalled_fraction: f64) -> f64 {
    REORGANIZATION_PROBABILITY_BASE * scale * (1.0 + STALL_PRESSURE_FACTOR * stalled_fraction)
}

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

    // Wilson cadence: plates creeping below the stall threshold count as
    // stalled (slab-less plates relax to the drift base, so a welded world
    // piles up there); the more of the world is stalled, the sooner a
    // reorganization fires to break it up or redirect it.
    let stall_threshold =
        stall_threshold_rad_per_year(&data.parameters.core.geology, &data.parameters.core.planet);
    let mut stalled = 0usize;
    let mut total = 0usize;
    for plate in state.registry.iter() {
        total += 1;
        if plate.motion_rate_rad_per_year < stall_threshold {
            stalled += 1;
        }
    }
    let stalled_fraction = if total == 0 {
        0.0
    } else {
        stalled as f64 / total as f64
    };
    let probability = reorganization_probability(scale, stalled_fraction);

    let mut check_rng = rng.stream_at(REORGANIZATION_CHECK_STREAM, tick_year.value() as u64);
    let roll: f64 = check_rng.gen_range(0.0..1.0);
    if roll >= probability {
        return false;
    }

    let mut action_rng = rng.stream_at(REORGANIZATION_ACTION_STREAM, tick_year.value() as u64);
    let action_roll: f64 = action_rng.gen_range(0.0..1.0);

    // Finite sphere: keep the plate census in the Earthlike 5–15 band
    // (§11 #2). Past 15 plates, boundaries crowd every coastline and the
    // world fragments into microplates — splits downweight, mergers
    // upweight. At 5 or fewer, the world goes tectonically stagnant —
    // splits upweight, mergers off. Motion changes always keep 40%.
    let plate_count = state.registry.count();
    let split_weight = if plate_count >= 15 {
        0.2
    } else if plate_count <= 5 {
        0.6
    } else {
        0.4
    };

    let result = if action_roll < split_weight {
        apply_split(data, &mut state.registry, tick_year, &mut action_rng)
    } else if action_roll < split_weight + 0.4 {
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

    state.projection = repartition_hexes(data, &mut state.registry).projection;

    if let Some((parent, child)) = split_pair {
        apply_split_boundary_subsidence(
            data,
            &mut state.registry,
            &state.projection,
            parent,
            child,
        );
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

    // Prefer splitting a stalled plate (creeping below the stall threshold):
    // welded supercontinents are where rifts nucleate.
    let stall_threshold =
        stall_threshold_rad_per_year(&data.parameters.core.geology, &data.parameters.core.planet);
    let stalled: Vec<PlateId> = candidates
        .iter()
        .copied()
        .filter(|id| {
            registry
                .get(*id)
                .is_some_and(|p| p.motion_rate_rad_per_year < stall_threshold)
        })
        .collect();
    let pool = if stalled.is_empty() {
        &candidates
    } else {
        &stalled
    };
    if pool.is_empty() {
        return None;
    }
    let parent_id = pool[rng.gen_range(0..pool.len())];

    // Re-anchor the parent so the split operates on current world positions and
    // the child's fresh axis starts from zero accumulated rotation (no teleport).
    {
        let parent_mut = registry.plates_mut().get_mut(&parent_id)?;
        rebase_birth_frame(&data.grid, parent_mut);
    }
    let parent = registry.get(parent_id)?.clone();
    let child_seed = farthest_hex_from_seed(&data.grid, &parent, data)?;
    let child_id = registry.next_id();

    // Divide the parent's footprint along the bisector between the parent seed
    // and the child seed: each feature (anchored at its current world hex after
    // the rebase) goes to whichever anchor is closer.
    let n = parent.surface.features.len();
    let mut child_surface = PlateSurface::new(n);
    let parent_dir = data.grid.cell_center_direction(parent.seed_hex);
    let child_dir = data.grid.cell_center_direction(child_seed);
    let parent_v = DVec3::new(parent_dir[0], parent_dir[1], parent_dir[2]);
    let child_v = DVec3::new(child_dir[0], child_dir[1], child_dir[2]);
    let mut child_hexes: Vec<usize> = Vec::new();
    for (idx, slot) in parent.surface.features.iter().enumerate() {
        if slot.is_none() {
            continue;
        }
        let dir = data.grid.cell_center_direction(HexId(idx as u32));
        let v = DVec3::new(dir[0], dir[1], dir[2]);
        if v.dot(child_v) > v.dot(parent_v) {
            child_hexes.push(idx);
        }
    }
    if child_hexes.is_empty() {
        return None;
    }
    for &idx in &child_hexes {
        child_surface.features[idx] = parent.surface.features[idx].clone();
    }

    // Ocean-opening split: the child gets a freshly sampled rate, and its
    // motion axis is the candidate that maximizes separation speed along the
    // parent→mid→child line, so the new boundary rifts apart into a young
    // ocean instead of shearing or converging (Wilson cycle, §4.5).
    let child_rate = sample_motion_rate_rad_per_year(
        parent.plate_type,
        &data.parameters.core.geology,
        &data.parameters.core.planet,
        rng,
    );
    let parent_center_dir = effective_position_direction(&data.grid, &parent);
    let parent_center_v = DVec3::new(
        parent_center_dir[0],
        parent_center_dir[1],
        parent_center_dir[2],
    );
    let mid_v = (parent_v + child_v).normalize();
    let radius_km = data.parameters.core.planet.radius_km;
    let mut best: Option<(f64, DVec3)> = None;
    for _ in 0..SPLIT_AXIS_CANDIDATES {
        let candidate = sample_motion_axis(child_v, rng);
        let candidate_arr = [candidate.x, candidate.y, candidate.z];
        let score = [parent_v, mid_v, child_v]
            .iter()
            .map(|&p| {
                separation_speed_m_per_year(
                    p,
                    parent_center_v,
                    parent.motion_axis,
                    parent.motion_rate_rad_per_year,
                    child_v,
                    candidate_arr,
                    child_rate,
                    radius_km,
                )
            })
            .sum::<f64>();
        if best.is_none_or(|(s, _)| score > s) {
            best = Some((score, candidate));
        }
    }
    let child_axis = best.map_or(
        DVec3::new(
            parent.motion_axis[0],
            parent.motion_axis[1],
            parent.motion_axis[2],
        ),
        |(_, axis)| axis,
    );

    let child = Plate {
        id: child_id,
        plate_type: parent.plate_type,
        plate_class: PlateClass::Minor,
        seed_hex: child_seed,
        motion_axis: [child_axis.x, child_axis.y, child_axis.z],
        motion_rate_rad_per_year: child_rate,
        age_year: tick_year,
        target_fraction: parent.target_fraction * 0.5,
        accumulated_rotation_rad: 0.0,
        last_nonempty_year: tick_year,
        surface: child_surface,
    };
    registry.insert(child);

    if let Some(parent_mut) = registry.plates_mut().get_mut(&parent_id) {
        for &idx in &child_hexes {
            parent_mut.surface.features[idx] = None;
        }
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
    // Prefer stalled plates (creeping below the stall threshold): stagnant
    // sutured continents get redirected sooner, feeding the Wilson cycle.
    let stall_threshold =
        stall_threshold_rad_per_year(&data.parameters.core.geology, &data.parameters.core.planet);
    let stalled: Vec<PlateId> = plate_ids
        .iter()
        .copied()
        .filter(|id| {
            registry
                .get(*id)
                .is_some_and(|p| p.motion_rate_rad_per_year < stall_threshold)
        })
        .collect();
    let pool = if stalled.is_empty() {
        &plate_ids
    } else {
        &stalled
    };
    let idx = rng.gen_range(0..pool.len());
    let plate_id = pool[idx];

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
    // Re-anchor before swapping the axis: accumulated rotation is only valid
    // around the axis it accumulated on.
    rebase_birth_frame(&data.grid, plate);
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
    Some(weld_plate_pair(data, registry, tick_year, into, absorbed))
}

/// Welds `absorbed` into `into`: hexes reassign, both frames re-anchor to
/// current world positions, surfaces union (newer feature wins overlaps), the
/// absorbed plate leaves the registry, and the combined footprint continues on
/// `into`'s motion. Used by random reorganization mergers and by the suturing
/// weld when a continental collision pair completes
/// [`crate::suture::WELD_AFTER_YEARS`] of sustained contact.
pub(crate) fn weld_plate_pair(
    data: &mut WorldData,
    registry: &mut PlateRegistry,
    tick_year: WorldYear,
    into: PlateId,
    absorbed: PlateId,
) -> (PlateReorgAction, Vec<PlateId>) {
    reassign_plate_hexes(data, absorbed, into);

    // Re-anchor both plates so their birth indices agree on world positions,
    // then merge; the combined footprint continues on `into`'s motion.
    if let Some(p) = registry.plates_mut().get_mut(&absorbed) {
        rebase_birth_frame(&data.grid, p);
    }
    if let Some(p) = registry.plates_mut().get_mut(&into) {
        rebase_birth_frame(&data.grid, p);
    }
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

    (
        PlateReorgAction::Merge { absorbed, into },
        vec![into, absorbed],
    )
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
    cache: &ProjectionCache,
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
                modify_surface_at_world_hex(registry, data, cache, hex, 0, |feature| {
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

/// Signed speed (m/yr) at surface point `p` at which the child plate pulls
/// away from the parent along the tangent-projected parent→child direction.
/// Positive means the plates diverge there; negative means they converge.
#[allow(clippy::too_many_arguments)]
fn separation_speed_m_per_year(
    p: DVec3,
    parent_center: DVec3,
    parent_axis: [f64; 3],
    parent_rate: f64,
    child_center: DVec3,
    child_axis: [f64; 3],
    child_rate: f64,
    radius_km: f64,
) -> f64 {
    let pp = [p.x, p.y, p.z];
    let v_parent = surface_velocity_m_per_year(pp, parent_axis, parent_rate, radius_km);
    let v_child = surface_velocity_m_per_year(pp, child_axis, child_rate, radius_km);
    let d = child_center - parent_center;
    let d_tangent = d - p * d.dot(p);
    if d_tangent.length_squared() < 1e-18 {
        return 0.0;
    }
    (v_child - v_parent).dot(d_tangent.normalize())
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

    #[test]
    fn reorganization_probability_scales_with_stall_pressure() {
        let calm = reorganization_probability(1.0, 0.0);
        let welded = reorganization_probability(1.0, 1.0);
        assert!((calm - 0.004).abs() < 1e-12);
        assert!((welded - 0.012).abs() < 1e-12);
        assert!(reorganization_probability(1.0, 0.5) > calm);
        assert!(reorganization_probability(1.0, 0.5) < welded);
        assert_eq!(reorganization_probability(0.0, 1.0), 0.0);
    }

    #[test]
    fn split_child_diverges_from_parent() {
        let mut world = test_world();
        let mut state = TectonicsState::new();
        run_formation(&mut world, &mut state);

        // Stall the largest plate so the split preferentially targets it:
        // pin every plate just above the stall threshold, then drop the
        // largest plate's rate well below it.
        let counts = hex_counts(&world.data);
        let (&largest_id, _) = counts.iter().max_by_key(|(_, c)| *c).expect("a plate");
        let stall_threshold = stall_threshold_rad_per_year(
            &world.data.parameters.core.geology,
            &world.data.parameters.core.planet,
        );
        for plate in state.registry.iter_mut() {
            plate.motion_rate_rad_per_year = stall_threshold * 2.0;
        }
        state
            .registry
            .plates_mut()
            .get_mut(&largest_id)
            .expect("plate")
            .motion_rate_rad_per_year = stall_threshold * 0.1;

        let year = WorldYear(1_000_000);
        let mut rng = world
            .rng
            .stream_at(REORGANIZATION_ACTION_STREAM, year.value() as u64);
        let result = apply_split(&mut world.data, &mut state.registry, year, &mut rng);

        let Some((PlateReorgAction::Split { parent, child }, _)) = result else {
            panic!("expected a split, got {result:?}");
        };
        assert_eq!(parent, largest_id);

        // The chosen child axis must pull away from the parent at the midpoint
        // of the new boundary: an ocean-opening rift, not a shear or collision.
        let parent_plate = state.registry.get(parent).expect("parent");
        let child_plate = state.registry.get(child).expect("child");
        let parent_dir = world.data.grid.cell_center_direction(parent_plate.seed_hex);
        let child_dir = world.data.grid.cell_center_direction(child_plate.seed_hex);
        let parent_v = DVec3::new(parent_dir[0], parent_dir[1], parent_dir[2]);
        let child_v = DVec3::new(child_dir[0], child_dir[1], child_dir[2]);
        let mid_v = (parent_v + child_v).normalize();
        let radius_km = world.data.parameters.core.planet.radius_km;
        let separation = separation_speed_m_per_year(
            mid_v,
            parent_v,
            parent_plate.motion_axis,
            parent_plate.motion_rate_rad_per_year,
            child_v,
            child_plate.motion_axis,
            child_plate.motion_rate_rad_per_year,
            radius_km,
        );
        assert!(
            separation > 0.0,
            "split child should diverge from parent at the boundary midpoint, got {separation} m/yr"
        );
    }
}
