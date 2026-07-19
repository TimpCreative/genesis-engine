//! Collision locking: converging continents jam each other's motion
//! (Doc 06 §4.6).
//!
//! Continents do not bounce off each other, and they do not pass through
//! each other either: within a few tens of My of first contact the two
//! plates lock kinematically — collision resistance couples them into one
//! drifting unit, so relative convergence dies and crustal shortening
//! ([`crate::partition`]) stops feeding the orogen. The pair keeps its plate
//! identities (a later rift can re-separate near the old suture, reopening
//! the Wilson cycle); what dies is the relative velocity. India–Asia is the
//! canonical case: convergence fell from ~15–18 cm/yr to a few cm/yr within
//! ~20 My of contact as the plates' motions coupled, without either plate
//! ceasing to exist.

use std::collections::{BTreeMap, BTreeSet};

use genesis_core::PlateId;
use genesis_core::data::WorldData;
use glam::DVec3;

use crate::plate::PlateRegistry;

/// Time constant for collision locking (years): convergence at a suture
/// decays over ~10 My (India slowed 18→5 cm/yr within ~20 My of contact).
pub const COLLISION_JAM_RELAX_YEARS: f64 = 10_000_000.0;

/// Relaxes every actively colliding continental pair toward a shared angular
/// velocity — the hex-count-weighted mean of the two plates' current rotation
/// vectors — so the pair drifts as one unit and the suture stops shortening.
/// Applied each tick while the pair collides (last tick's contact set, the
/// same one-tick lag as the slab-pull tallies); once contact ends, the slab-
/// and ridge-force set-points (§2.4) take each plate back over ~25 My.
///
/// A locked pair reads as *stalled* to the reorganization pacemaker (§4.5),
/// which is exactly where rifts nucleate: assembly stalls the machine, and
/// the stall raises the pressure that tears the supercontinent apart again.
///
/// Deterministic: ascending pair order, no RNG.
pub fn apply_collision_jam(
    data: &WorldData,
    registry: &mut PlateRegistry,
    colliding_pairs: &BTreeSet<(PlateId, PlateId)>,
    tick_interval_years: f64,
) {
    if colliding_pairs.is_empty() || tick_interval_years <= 0.0 {
        return;
    }
    let relax = 1.0 - (-tick_interval_years / COLLISION_JAM_RELAX_YEARS).exp();

    // Hex counts weight the shared velocity: the bigger plate's motion wins.
    let mut counts: BTreeMap<PlateId, f64> = BTreeMap::new();
    for &pid in &data.plate_id {
        if pid != PlateId::NONE {
            *counts.entry(pid).or_insert(0.0) += 1.0;
        }
    }

    let omega_of = |registry: &PlateRegistry, id: PlateId| -> Option<DVec3> {
        let plate = registry.get(id)?;
        let axis = DVec3::new(
            plate.motion_axis[0],
            plate.motion_axis[1],
            plate.motion_axis[2],
        )
        .normalize_or(DVec3::Z);
        Some(axis * plate.motion_rate_rad_per_year)
    };

    for &(a, b) in colliding_pairs {
        let (Some(omega_a), Some(omega_b)) = (omega_of(registry, a), omega_of(registry, b)) else {
            continue; // a plate left the registry (merger, purge)
        };
        let count_a = counts.get(&a).copied().unwrap_or(0.0);
        let count_b = counts.get(&b).copied().unwrap_or(0.0);
        let total = count_a + count_b;
        if total <= 0.0 {
            continue;
        }
        let shared = (omega_a * count_a + omega_b * count_b) / total;
        for (id, omega) in [(a, omega_a), (b, omega_b)] {
            let new_omega = omega + (shared - omega) * relax;
            let rate = new_omega.length();
            let Some(plate) = registry.plates_mut().get_mut(&id) else {
                continue;
            };
            if rate > 1e-20 {
                let axis = new_omega / rate;
                plate.motion_axis = [axis.x, axis.y, axis.z];
            }
            // A near-zero shared velocity keeps the old axis; the rate is what
            // locks the pair.
            plate.motion_rate_rad_per_year = rate;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{HexId, create_world};

    use crate::plate::{Plate, PlateClass, PlateType};
    use crate::plate_surface::PlateSurface;

    fn test_world() -> genesis_core::World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        create_world(params).expect("world")
    }

    fn plate(id: u16, axis: [f64; 3], rate: f64, cell_count: usize) -> Plate {
        Plate {
            id: PlateId(id),
            plate_type: PlateType::Continental,
            plate_class: PlateClass::Major,
            seed_hex: HexId(id as u32),
            motion_axis: axis,
            motion_rate_rad_per_year: rate,
            age_year: genesis_core::time::WorldYear::FORMATION,
            target_fraction: 0.5,
            accumulated_rotation_rad: 0.0,
            last_nonempty_year: genesis_core::time::WorldYear::FORMATION,
            surface: PlateSurface::new(cell_count),
            forward_world_hint: Vec::new(),
        }
    }

    fn omega_of(registry: &PlateRegistry, id: PlateId) -> DVec3 {
        let plate = registry.get(id).unwrap();
        DVec3::new(
            plate.motion_axis[0],
            plate.motion_axis[1],
            plate.motion_axis[2],
        )
        .normalize()
            * plate.motion_rate_rad_per_year
    }

    /// Opposed spins of equal magnitude on equal-sized plates: the shared
    /// velocity is zero, so both plates lock to a standstill within a few
    /// relaxation times — the sutured supercontinent parked over its mantle.
    #[test]
    fn opposed_plates_lock_to_zero_relative_motion() {
        let mut world = test_world();
        let n = world.data.cell_count() as usize;
        let mut registry = PlateRegistry::new();
        registry.insert(plate(0, [0.0, 0.0, 1.0], 1e-8, n));
        registry.insert(plate(1, [0.0, 0.0, -1.0], 1e-8, n));
        for (i, pid) in world.data.plate_id.iter_mut().enumerate() {
            *pid = if i < n / 2 { PlateId(0) } else { PlateId(1) };
        }
        let pairs = BTreeSet::from([(PlateId(0), PlateId(1))]);

        for _ in 0..200 {
            apply_collision_jam(&world.data, &mut registry, &pairs, 500_000.0);
        }

        let omega_a = omega_of(&registry, PlateId(0));
        let omega_b = omega_of(&registry, PlateId(1));
        assert!(
            omega_a.length() < 1e-12 && omega_b.length() < 1e-12,
            "equal opposed plates lock to near-zero motion: {omega_a:?} {omega_b:?}"
        );
        assert_eq!(registry.count(), 2, "locking keeps both plate identities");
    }

    /// The shared velocity is hex-count-weighted: the bigger plate's motion
    /// dominates the locked pair's common drift.
    #[test]
    fn shared_velocity_is_weighted_by_plate_size() {
        let mut world = test_world();
        let n = world.data.cell_count() as usize;
        let mut registry = PlateRegistry::new();
        registry.insert(plate(0, [0.0, 0.0, 1.0], 2e-8, n));
        registry.insert(plate(1, [0.0, 0.0, 1.0], 0.0, n));
        // Plate 0 owns 3/4 of the sphere.
        for (i, pid) in world.data.plate_id.iter_mut().enumerate() {
            *pid = if i < 3 * n / 4 {
                PlateId(0)
            } else {
                PlateId(1)
            };
        }
        let pairs = BTreeSet::from([(PlateId(0), PlateId(1))]);

        for _ in 0..200 {
            apply_collision_jam(&world.data, &mut registry, &pairs, 500_000.0);
        }

        let rate_a = registry.get(PlateId(0)).unwrap().motion_rate_rad_per_year;
        let rate_b = registry.get(PlateId(1)).unwrap().motion_rate_rad_per_year;
        let expected = 2e-8 * 0.75;
        assert!(
            (rate_a - expected).abs() < expected * 0.02,
            "locked pair co-drifts at the weighted mean: {rate_a} vs {expected}"
        );
        assert!(
            (rate_b - expected).abs() < expected * 0.02,
            "both plates share the locked rate: {rate_b} vs {expected}"
        );
        assert_eq!(
            registry.get(PlateId(1)).unwrap().motion_axis,
            [0.0, 0.0, 1.0],
            "the dragged plate adopts the shared axis"
        );
    }

    #[test]
    fn missing_plate_and_empty_pairs_are_noops() {
        let world = test_world();
        let n = world.data.cell_count() as usize;
        let mut registry = PlateRegistry::new();
        registry.insert(plate(0, [0.0, 0.0, 1.0], 1e-8, n));
        let pairs = BTreeSet::from([(PlateId(0), PlateId(9))]);
        apply_collision_jam(&world.data, &mut registry, &pairs, 500_000.0);
        assert_eq!(
            registry.get(PlateId(0)).unwrap().motion_rate_rad_per_year,
            1e-8,
            "a pair whose mate is gone must not touch the survivor"
        );
        apply_collision_jam(&world.data, &mut registry, &BTreeSet::new(), 500_000.0);
        assert_eq!(
            registry.get(PlateId(0)).unwrap().motion_rate_rad_per_year,
            1e-8
        );
    }
}
