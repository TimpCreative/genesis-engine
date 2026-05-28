//! Plate-local ↔ world-frame hex conversion (Doc 06 destination-driven model).

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

/// Converts a world-frame hex to a plate-local [`HexId`] for the given plate.
///
/// Inverse-rotates the world position by the plate's accumulated rotation, then finds
/// the nearest grid hex using `world_hex` as the search hint.
pub fn world_to_plate_local(grid: &HexGrid, world_hex: HexId, plate: &Plate) -> HexId {
    let world_pos = grid.cell_center_direction(world_hex);
    let world_v = DVec3::new(world_pos[0], world_pos[1], world_pos[2]);

    let q_inverse =
        DQuat::from_axis_angle(plate_rotation_axis(plate), -plate.accumulated_rotation_rad);
    let local_v = (q_inverse * world_v).normalize();

    grid.nearest_hex_direction_from(world_hex, [local_v.x, local_v.y, local_v.z])
}

/// Finds the world-frame [`HexId`] that currently displays a plate-local feature.
///
/// Scans all hexes for a deterministic inverse of [`world_to_plate_local`]. `world_hint`
/// is returned when no hex maps to `plate_local_hex` (should not occur in valid worlds).
pub fn plate_local_to_world(
    grid: &HexGrid,
    plate_local_hex: HexId,
    plate: &Plate,
    world_hint: HexId,
) -> HexId {
    grid.iter()
        .filter(|&world_hex| world_to_plate_local(grid, world_hex, plate) == plate_local_hex)
        .min()
        .unwrap_or(world_hint)
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
                world_to_plate_local(grid, hex, &plate),
                hex,
                "zero rotation should map {hex:?} to itself"
            );
        }
    }

    #[test]
    fn round_trip_world_to_local_to_world() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let world = create_world(params).expect("world");
        let grid = &world.data.grid;
        let plate = test_plate(0.3);

        for hex in grid.iter().take(100) {
            let local = world_to_plate_local(grid, hex, &plate);
            let back = plate_local_to_world(grid, local, &plate, hex);
            assert_eq!(
                back, hex,
                "round-trip failed for world hex {hex:?} (local {local:?})"
            );
        }
    }

    #[test]
    fn rotation_moves_local_positions() {
        let grid = genesis_core::HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let mut plate = test_plate(std::f64::consts::FRAC_PI_2);
        plate.surface = PlateSurface::new(grid.cell_count() as usize);

        let world_hex = HexId(10);
        let local = world_to_plate_local(&grid, world_hex, &plate);
        let back_at_zero = {
            let mut zero = plate.clone();
            zero.accumulated_rotation_rad = 0.0;
            world_to_plate_local(&grid, world_hex, &zero)
        };

        assert_ne!(
            local, back_at_zero,
            "90° rotation should change plate-local index for hex {world_hex:?}"
        );
    }
}
