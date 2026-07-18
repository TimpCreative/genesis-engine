//! Plate motion: accumulated rotation and effective seed positions.

use glam::{DQuat, DVec3};
use rand::Rng;

use genesis_core::HexGrid;

use crate::plate::Plate;

/// Doc 06 §2.1 — sample a motion axis uniform on the sphere with centroid constraints.
pub fn sample_motion_axis(centroid: DVec3, rng: &mut rand::rngs::SmallRng) -> DVec3 {
    use std::f64::consts::PI;

    let z_axis = DVec3::Z;
    let centroid = centroid.normalize();

    for _attempt in 0..100 {
        let u: f64 = rng.gen_range(0.0..1.0);
        let v: f64 = rng.gen_range(0.0..1.0);
        let theta = 2.0 * PI * u;
        let phi = (2.0_f64 * v - 1.0).acos();
        let axis = DVec3::new(phi.sin() * theta.cos(), phi.sin() * theta.sin(), phi.cos());

        if axis.dot(z_axis).abs() > 0.95 {
            continue;
        }

        let to_centroid = axis.dot(centroid).clamp(-1.0, 1.0).acos();
        if !(PI * 30.0 / 180.0..=PI * 150.0 / 180.0).contains(&to_centroid) {
            continue;
        }

        return axis;
    }

    if centroid.dot(z_axis).abs() < 0.99 {
        centroid.cross(z_axis).normalize()
    } else {
        centroid.cross(DVec3::X).normalize()
    }
}

/// Rotates the seed hex center direction about `plate.motion_axis` by
/// `plate.accumulated_rotation_rad`, returning a unit direction.
pub fn effective_position_direction(grid: &HexGrid, plate: &Plate) -> [f64; 3] {
    let seed = grid.cell_center_direction(plate.seed_hex);
    let v = DVec3::new(seed[0], seed[1], seed[2]);
    let axis = DVec3::new(
        plate.motion_axis[0],
        plate.motion_axis[1],
        plate.motion_axis[2],
    )
    .normalize();
    let rotated = rotate_vector(v, axis, plate.accumulated_rotation_rad);
    let out = rotated.normalize();
    [out.x, out.y, out.z]
}

/// Increments `accumulated_rotation_rad` for one tick interval.
pub fn advance_plate_motion(plate: &mut Plate, tick_interval_years: f64) {
    plate.accumulated_rotation_rad += plate.motion_rate_rad_per_year * tick_interval_years;
}

/// Surface velocity at a point on the sphere in meters per year (Doc 06 §3.4).
///
/// `ω = axis_unit * rate_rad_per_year`, `v = ω × p`, scaled by planet radius.
pub fn surface_velocity_m_per_year(
    center_dir: [f64; 3],
    motion_axis: [f64; 3],
    motion_rate_rad_per_year: f64,
    planet_radius_km: f64,
) -> DVec3 {
    let p = DVec3::new(center_dir[0], center_dir[1], center_dir[2]);
    let axis = DVec3::new(motion_axis[0], motion_axis[1], motion_axis[2]).normalize();
    let omega = axis * motion_rate_rad_per_year;
    let radius_m = planet_radius_km * 1000.0;
    omega.cross(p) * radius_m
}

/// Drift speed of a plate with no driving boundary forces (cm/yr): the slow
/// creep of slab-less plates (Africa, Eurasia, Antarctica run 1–3 cm/yr).
pub const DRIFT_BASE_CM_PER_YEAR: f64 = 1.5;

/// Extra speed for a plate whose entire rim is pulling slab (cm/yr). Slab
/// pull is ~90% of Earth's plate-driving force budget.
pub const SLAB_PULL_CM_PER_YEAR: f64 = 10.0;

/// Extra speed for a plate whose entire rim is live ridge (cm/yr); ridge
/// push is a minor force next to slab pull.
pub const RIDGE_PUSH_CM_PER_YEAR: f64 = 2.0;

/// Sustained speed ceiling (cm/yr): nothing on Earth sustains more than
/// ~15–18 cm/yr (India right before the Asia collision).
pub const MAX_PLATE_CM_PER_YEAR: f64 = 15.0;

/// Boundary-force relaxation time (years): plates take ~25 My to spin up or
/// slow down as slabs founder or choke (India slowed 18→5 cm/yr over ~20 My).
pub const MOTION_RELAX_YEARS: f64 = 25_000_000.0;

/// Relaxes every plate's motion rate toward its boundary-force set-point
/// (Doc 06 §2.2): slab-less plates drift slowly, plates rimmed by pulling
/// slabs run fast, and suturing slows a plate over ~[`MOTION_RELAX_YEARS`] as
/// the arriving continent chokes its trench. Speed is emergent from boundary
/// geometry — no sampled base rate, no damping/floor machinery — so plates
/// neither freeze in place nor cross the planet in a screenshot gap.
pub fn relax_motion_rates_toward_targets(
    registry: &mut crate::plate::PlateRegistry,
    tallies: &std::collections::BTreeMap<genesis_core::PlateId, crate::boundary::BoundaryTally>,
    geology: &genesis_core::parameters::GeologyParameters,
    planet: &genesis_core::parameters::PlanetParameters,
    interval_years: f64,
) {
    let rotation_factor = (24.0 / planet.rotation_period_hours).sqrt();
    let scale = f64::from(geology.plate_velocity_scale) * rotation_factor;
    let relax = 1.0 - (-interval_years / MOTION_RELAX_YEARS).exp();
    let plate_ids = registry.plate_ids();
    for id in plate_ids {
        let Some(plate) = registry.plates_mut().get_mut(&id) else {
            continue;
        };
        let tally = tallies.get(&id).copied().unwrap_or_default();
        let per_edge = if tally.total_edges == 0 {
            0.0
        } else {
            1.0 / f64::from(tally.total_edges)
        };
        let target_cm_per_year = (DRIFT_BASE_CM_PER_YEAR
            + SLAB_PULL_CM_PER_YEAR * f64::from(tally.slab_edges) * per_edge
            + RIDGE_PUSH_CM_PER_YEAR * f64::from(tally.ridge_edges) * per_edge)
            .min(MAX_PLATE_CM_PER_YEAR)
            * scale;
        // Same cm/yr → rad/yr conversion as the reorganization rate sampler.
        let target_rad_per_year = (target_cm_per_year * 1e-5) / planet.radius_km;
        plate.motion_rate_rad_per_year +=
            (target_rad_per_year - plate.motion_rate_rad_per_year) * relax;
    }
}

fn rotate_vector(vec: DVec3, axis: DVec3, angle_rad: f64) -> DVec3 {
    let q = DQuat::from_axis_angle(axis, angle_rad);
    q * vec
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::time::WorldYear;
    use genesis_core::{HexGrid, HexId, PlateId};

    use crate::plate::{Plate, PlateClass, PlateType};

    const EARTH_RADIUS_KM: f64 = 6371.0;

    fn sample_plate() -> Plate {
        Plate {
            id: PlateId(0),
            plate_type: PlateType::Oceanic,
            plate_class: PlateClass::Major,
            seed_hex: HexId(100),
            motion_axis: [0.0, 0.0, 1.0],
            motion_rate_rad_per_year: 1e-8,
            age_year: WorldYear::FORMATION,
            target_fraction: 0.1,
            accumulated_rotation_rad: 0.0,
            last_nonempty_year: WorldYear::FORMATION,
            surface: crate::plate_surface::PlateSurface::new(10_000),
        }
    }

    #[test]
    fn zero_rotation_matches_seed_direction() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let plate = sample_plate();
        let seed = grid.cell_center_direction(plate.seed_hex);
        let effective = effective_position_direction(&grid, &plate);
        let seed_v = DVec3::new(seed[0], seed[1], seed[2]);
        let eff_v = DVec3::new(effective[0], effective[1], effective[2]);
        let dot = seed_v.dot(eff_v).clamp(-1.0, 1.0);
        assert!((dot - 1.0).abs() < 1e-9, "dot = {dot}");
    }

    #[test]
    fn known_rotation_changes_effective_position() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let mut plate = sample_plate();
        plate.motion_axis = [1.0, 0.0, 0.0];
        plate.accumulated_rotation_rad = std::f64::consts::FRAC_PI_2;

        let seed = grid.cell_center_direction(plate.seed_hex);
        let seed_v = DVec3::new(seed[0], seed[1], seed[2]).normalize();
        let effective = effective_position_direction(&grid, &plate);
        let eff_v = DVec3::new(effective[0], effective[1], effective[2]).normalize();

        let dot = seed_v.dot(eff_v).clamp(-1.0, 1.0);
        assert!(
            (dot - 1.0).abs() > 0.01,
            "rotation should move effective position away from seed (dot = {dot})"
        );
    }

    #[test]
    fn effective_position_is_unit_length() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let mut plate = sample_plate();
        plate.accumulated_rotation_rad = 0.5;
        plate.motion_axis = [0.1, 0.7, 0.71];
        let effective = effective_position_direction(&grid, &plate);
        let v = DVec3::new(effective[0], effective[1], effective[2]);
        assert!((v.length() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn advance_plate_motion_accumulates() {
        let mut plate = sample_plate();
        plate.motion_rate_rad_per_year = 2e-8;
        advance_plate_motion(&mut plate, 500_000.0);
        assert!((plate.accumulated_rotation_rad - 2e-8 * 500_000.0).abs() < 1e-20);
    }

    #[test]
    fn surface_velocity_scales_with_radius() {
        let p = [0.0, 0.0, 1.0];
        let axis = [1.0, 0.0, 0.0];
        let rate = 1e-8;
        let v = surface_velocity_m_per_year(p, axis, rate, EARTH_RADIUS_KM);
        let expected_mag = rate * EARTH_RADIUS_KM * 1000.0;
        assert!((v.length() - expected_mag).abs() < 1e-6);
    }

    #[test]
    fn surface_velocity_zero_when_rate_zero() {
        let p = [0.0, 1.0, 0.0];
        let v = surface_velocity_m_per_year(p, [0.0, 0.0, 1.0], 0.0, EARTH_RADIUS_KM);
        assert!(v.length() < 1e-12);
    }

    fn cm_per_year(rad_per_year: f64, radius_km: f64) -> f64 {
        rad_per_year * radius_km * 1e5
    }

    fn relax_registry(id: u16, rate: f64) -> crate::plate::PlateRegistry {
        let mut registry = crate::plate::PlateRegistry::new();
        let mut plate = sample_plate();
        plate.id = PlateId(id);
        plate.motion_rate_rad_per_year = rate;
        registry.insert(plate);
        registry
    }

    fn relax_n(
        registry: &mut crate::plate::PlateRegistry,
        tallies: &std::collections::BTreeMap<PlateId, crate::boundary::BoundaryTally>,
        ticks: usize,
        interval_years: f64,
    ) {
        let params = genesis_core::parameters::WorldParameters::default();
        for _ in 0..ticks {
            relax_motion_rates_toward_targets(
                registry,
                tallies,
                &params.core.geology,
                &params.core.planet,
                interval_years,
            );
        }
    }

    fn tally(slab: u32, ridge: u32, total: u32) -> crate::boundary::BoundaryTally {
        crate::boundary::BoundaryTally {
            slab_edges: slab,
            ridge_edges: ridge,
            total_edges: total,
        }
    }

    #[test]
    fn slab_rimmed_plate_spins_up_toward_slab_pull_speed() {
        let mut registry = relax_registry(0, 0.0);
        let tallies = std::collections::BTreeMap::from([(PlateId(0), tally(5, 0, 10))]);
        relax_n(&mut registry, &tallies, 200, 1_000_000.0);
        let rate = registry.get(PlateId(0)).unwrap().motion_rate_rad_per_year;
        let cm = cm_per_year(rate, EARTH_RADIUS_KM);
        // Target: 1.5 drift + 10 slab × 0.5 rim = 6.5 cm/yr.
        assert!(
            (cm - 6.5).abs() < 0.1,
            "slab-rimmed plate should converge to 6.5 cm/yr, got {cm}"
        );
    }

    #[test]
    fn slab_free_plate_slows_from_fast_seed_to_drift_speed() {
        // Seeded absurdly fast (63.7 cm/yr) as a reorg might; with only a
        // ridge quarter-rim it must decay to 1.5 + 2×0.25 = 2.0 cm/yr.
        let mut registry = relax_registry(0, 1e-7);
        let tallies = std::collections::BTreeMap::from([(PlateId(0), tally(0, 1, 4))]);
        relax_n(&mut registry, &tallies, 200, 1_000_000.0);
        let rate = registry.get(PlateId(0)).unwrap().motion_rate_rad_per_year;
        let cm = cm_per_year(rate, EARTH_RADIUS_KM);
        assert!(
            (cm - 2.0).abs() < 0.1,
            "slab-free plate should decay to 2.0 cm/yr, got {cm}"
        );
    }

    #[test]
    fn relax_never_stops_a_plate_and_never_exceeds_earth_ceiling() {
        let params = genesis_core::parameters::WorldParameters::default();
        // Fully slab-girdled AND fully ridged (impossible geometry, tests bounds).
        let tallies = std::collections::BTreeMap::from([(PlateId(0), tally(10, 10, 10))]);
        let mut registry = relax_registry(0, 0.0);
        for _ in 0..400 {
            relax_motion_rates_toward_targets(
                &mut registry,
                &tallies,
                &params.core.geology,
                &params.core.planet,
                1_000_000.0,
            );
            let rate = registry.get(PlateId(0)).unwrap().motion_rate_rad_per_year;
            assert!(rate > 0.0, "a plate must never freeze: {rate}");
            let cm = cm_per_year(rate, EARTH_RADIUS_KM);
            assert!(
                cm <= MAX_PLATE_CM_PER_YEAR + 1e-9,
                "no plate sustains above the ceiling: {cm}"
            );
        }
    }

    #[test]
    fn relax_is_tick_interval_independent() {
        let params = genesis_core::parameters::WorldParameters::default();
        let tallies = std::collections::BTreeMap::from([(PlateId(0), tally(3, 1, 8))]);

        let mut coarse = relax_registry(0, 5e-8);
        relax_motion_rates_toward_targets(
            &mut coarse,
            &tallies,
            &params.core.geology,
            &params.core.planet,
            2_000_000.0,
        );
        let coarse_rate = coarse.get(PlateId(0)).unwrap().motion_rate_rad_per_year;

        let mut fine = relax_registry(0, 5e-8);
        relax_n(&mut fine, &tallies, 2, 1_000_000.0);
        let fine_rate = fine.get(PlateId(0)).unwrap().motion_rate_rad_per_year;

        let diff = (coarse_rate - fine_rate).abs() / coarse_rate.abs().max(1e-30);
        assert!(
            diff < 1e-9,
            "one 2 My step must equal two 1 My steps (relative diff {diff})"
        );
    }
}
