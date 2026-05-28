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
}
