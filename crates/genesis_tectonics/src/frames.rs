//! Plate birth-frame ↔ world-frame conversion (Doc 06 destination-driven model, P1-16).
//!
//! Features are stored indexed by their BIRTH world-HexId (the hex they occupied at
//! year 0 or when created). To find where a feature currently appears, rotate its birth
//! position FORWARD by the plate's accumulated rotation. We always rotate from the fixed
//! birth position, so quantization error does not compound over time.

use glam::{DQuat, DVec3};

use genesis_core::{HexGrid, HexId};

use crate::plate::Plate;

fn plate_rotation_axis(plate: &Plate) -> DVec3 {
    DVec3::new(
        plate.motion_axis[0],
        plate.motion_axis[1],
        plate.motion_axis[2],
    )
    .normalize()
}

/// Rotates a birth-frame world hex FORWARD to its current world position, returns the
/// current world HexId. This is the primary lookup used by world_rebuild.
pub fn birth_hex_to_current_world(grid: &HexGrid, birth_hex: HexId, plate: &Plate) -> HexId {
    let birth_pos = grid.cell_center_direction(birth_hex);
    let birth_v = DVec3::new(birth_pos[0], birth_pos[1], birth_pos[2]);
    let q = DQuat::from_axis_angle(plate_rotation_axis(plate), plate.accumulated_rotation_rad);
    let current_v = (q * birth_v).normalize();
    grid.nearest_hex_direction_from(birth_hex, [current_v.x, current_v.y, current_v.z])
}

/// Inverse-rotates a CURRENT world hex back to its birth-frame world HexId. Used at WRITE
/// time when a terrain event occurs at a current world hex and we need its birth index.
/// Done once per write; result is stored as a fixed index, so no compounding.
pub fn current_world_to_birth_hex(
    grid: &HexGrid,
    current_world_hex: HexId,
    plate: &Plate,
) -> HexId {
    let world_pos = grid.cell_center_direction(current_world_hex);
    let world_v = DVec3::new(world_pos[0], world_pos[1], world_pos[2]);
    let q_inv = DQuat::from_axis_angle(plate_rotation_axis(plate), -plate.accumulated_rotation_rad);
    let birth_v = (q_inv * world_v).normalize();
    grid.nearest_hex_direction_from(current_world_hex, [birth_v.x, birth_v.y, birth_v.z])
}

/// Re-anchors a plate's birth frame to the present: every feature is re-stored
/// at its CURRENT world hex and `accumulated_rotation_rad` resets to zero.
///
/// Must be called before changing a plate's `motion_axis` — the accumulated
/// rotation is only meaningful around the axis it accumulated on, so swapping
/// the axis without re-anchoring teleports every feature. Collisions from
/// quantization keep the higher-elevation (then newer, then lower-index)
/// feature, mirroring `world_rebuild`'s display priority.
pub fn rebase_birth_frame(grid: &HexGrid, plate: &mut Plate) {
    let n = plate.surface.features.len();
    let mut rebased: Vec<Option<crate::plate_surface::SurfaceFeature>> = vec![None; n];

    for (birth_idx, slot) in plate.surface.features.iter().enumerate() {
        let Some(feature) = slot else {
            continue;
        };
        let birth_hex = HexId(birth_idx as u32);
        let current = birth_hex_to_current_world(grid, birth_hex, plate);
        let w = current.0 as usize;
        if w >= n {
            continue;
        }
        let replace = match &rebased[w] {
            None => true,
            Some(existing) => {
                feature.elevation_m > existing.elevation_m
                    || (feature.elevation_m == existing.elevation_m
                        && feature.age_year > existing.age_year)
            }
        };
        if replace {
            rebased[w] = Some(feature.clone());
        }
    }

    plate.surface.features = rebased;
    plate.accumulated_rotation_rad = 0.0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::time::WorldYear;
    use genesis_core::{PlateId, create_world};

    use crate::plate::{PlateClass, PlateType};
    use crate::plate_surface::PlateSurface;

    const EARTH_RADIUS_KM: f64 = 6371.0;

    fn test_plate(rotation_rad: f64) -> Plate {
        Plate {
            id: PlateId(0),
            plate_type: PlateType::Continental,
            plate_class: PlateClass::Major,
            seed_hex: HexId(0),
            motion_axis: [0.0, 0.0, 1.0],
            motion_rate_rad_per_year: 1e-8,
            age_year: WorldYear::FORMATION,
            target_fraction: 0.5,
            accumulated_rotation_rad: rotation_rad,
            last_nonempty_year: WorldYear::FORMATION,
            surface: PlateSurface::new(100),
        }
    }

    #[test]
    fn zero_rotation_is_identity() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let world = create_world(params).expect("world");
        let grid = &world.data.grid;
        let plate = test_plate(0.0);

        for hex in grid.iter().take(50) {
            assert_eq!(
                birth_hex_to_current_world(grid, hex, &plate),
                hex,
                "zero rotation should map {hex:?} to itself"
            );
            assert_eq!(
                current_world_to_birth_hex(grid, hex, &plate),
                hex,
                "zero rotation should map {hex:?} to itself"
            );
        }
    }

    #[test]
    fn forward_then_inverse_stays_near_birth_hex() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let world = create_world(params).expect("world");
        let grid = &world.data.grid;
        let plate = test_plate(0.3);

        for hex in grid.iter().take(100) {
            let current = birth_hex_to_current_world(grid, hex, &plate);
            let recovered = current_world_to_birth_hex(grid, current, &plate);
            let is_self_or_neighbor =
                recovered == hex || grid.neighbors(hex).iter().copied().any(|n| n == recovered);
            assert!(
                is_self_or_neighbor,
                "inverse(forward({hex:?})) should be {hex:?} or neighbor, got {recovered:?}"
            );
        }
    }

    #[test]
    fn rebase_preserves_feature_world_positions() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let world = create_world(params).expect("world");
        let grid = &world.data.grid;
        let n = grid.cell_count() as usize;

        let mut plate = test_plate(0.4);
        plate.surface = PlateSurface::new(n);
        let birth_hex = HexId(30);
        plate.surface.set(
            birth_hex,
            crate::plate_surface::SurfaceFeature {
                elevation_m: 1234.0,
                relief_m: 10.0,
                bedrock: genesis_core::data::BedrockType::Igneous,
                fertility: 0.0,
                age_year: 5,
            },
        );

        let world_pos_before = birth_hex_to_current_world(grid, birth_hex, &plate);
        rebase_birth_frame(grid, &mut plate);

        assert_eq!(plate.accumulated_rotation_rad, 0.0);
        // After rebase, the feature lives AT its world position (identity mapping).
        let feature = plate
            .surface
            .get(world_pos_before)
            .expect("feature stored at current world hex");
        assert_eq!(feature.elevation_m, 1234.0);
        assert_eq!(
            birth_hex_to_current_world(grid, world_pos_before, &plate),
            world_pos_before,
            "zero rotation after rebase keeps the feature in place"
        );
    }

    #[test]
    fn rotation_moves_current_world_position() {
        let grid = genesis_core::HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let mut plate = test_plate(std::f64::consts::FRAC_PI_2);
        plate.surface = PlateSurface::new(grid.cell_count() as usize);

        let birth_hex = HexId(10);
        let rotated_world = birth_hex_to_current_world(&grid, birth_hex, &plate);
        let world_at_zero = {
            let mut zero = plate.clone();
            zero.accumulated_rotation_rad = 0.0;
            birth_hex_to_current_world(&grid, birth_hex, &zero)
        };

        assert_ne!(
            rotated_world, world_at_zero,
            "90° rotation should change current world position for birth hex {birth_hex:?}"
        );
    }
}
