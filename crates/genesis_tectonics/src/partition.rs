//! Voronoi re-partition of hexes to plates by effective seed positions.

use genesis_core::data::WorldData;
use genesis_core::{HexGrid, HexId, PlateId};

use crate::motion::effective_position_direction;
use crate::plate::PlateRegistry;

/// Reassigns every hex to the plate whose effective position is nearest (angular distance).
///
/// Iterates hexes in ascending [`HexId`] order. Ties break on lowest [`PlateId`].
pub fn repartition_hexes(data: &mut WorldData, registry: &PlateRegistry) {
    let grid = &data.grid;
    let n = data.plate_id.len();
    let plate_ids = registry.plate_ids();

    for i in 0..n {
        let hex = HexId(i as u32);
        let owner = nearest_plate(grid, hex, registry, &plate_ids);
        data.plate_id[i] = owner;
    }
}

fn nearest_plate(
    grid: &HexGrid,
    hex: HexId,
    registry: &PlateRegistry,
    plate_ids: &[PlateId],
) -> PlateId {
    let hex_pos = grid.cell_center_direction(hex);
    let hex_v = glam::DVec3::new(hex_pos[0], hex_pos[1], hex_pos[2]);

    let mut best_id = plate_ids[0];
    let mut best_dot = f64::NEG_INFINITY;

    for &id in plate_ids {
        let plate = registry.get(id).expect("plate in registry");
        let eff = effective_position_direction(grid, plate);
        let eff_v = glam::DVec3::new(eff[0], eff[1], eff[2]);
        let dot = hex_v.dot(eff_v);
        if dot > best_dot || (dot == best_dot && id < best_id) {
            best_dot = dot;
            best_id = id;
        }
    }

    best_id
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::time::WorldYear;
    use genesis_core::{HexGrid, HexId, PlateId};

    use crate::plate::{Plate, PlateClass, PlateRegistry, PlateType};

    const EARTH_RADIUS_KM: f64 = 6371.0;

    fn plate_at(id: u16, seed: u32, axis: [f64; 3], rotation: f64) -> Plate {
        Plate {
            id: PlateId(id),
            plate_type: PlateType::Oceanic,
            plate_class: PlateClass::Major,
            seed_hex: HexId(seed),
            motion_axis: axis,
            motion_rate_rad_per_year: 1e-8,
            age_year: WorldYear::FORMATION,
            target_fraction: 0.5,
            accumulated_rotation_rad: rotation,
            last_nonempty_year: WorldYear::FORMATION,
        }
    }

    #[test]
    fn two_plates_assign_by_nearest_effective_position() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let mut registry = PlateRegistry::new();
        registry.insert(plate_at(0, 0, [0.0, 0.0, 1.0], 0.0));
        registry.insert(plate_at(1, 500, [0.0, 0.0, 1.0], 0.0));

        let params = WorldParameters::default();
        let mut data = WorldData::new(grid, params);
        data.plate_id.fill(PlateId::NONE);

        repartition_hexes(&mut data, &registry);

        assert_ne!(data.plate_id[0], PlateId::NONE);
        assert_eq!(data.plate_id[0], data.plate_id[0]);
    }

    #[test]
    fn tie_breaks_to_lowest_plate_id() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let mut registry = PlateRegistry::new();
        let seed_hex = HexId(10);
        registry.insert(plate_at(1, seed_hex.0, [1.0, 0.0, 0.0], 0.0));
        registry.insert(plate_at(0, seed_hex.0, [1.0, 0.0, 0.0], 0.0));

        let params = WorldParameters::default();
        let data = WorldData::new(grid, params);

        let owner = nearest_plate(&data.grid, seed_hex, &registry, &registry.plate_ids());
        assert_eq!(owner, PlateId(0));
    }

    #[test]
    fn rotation_changes_some_assignments() {
        let grid = HexGrid::new(5, EARTH_RADIUS_KM).expect("grid");
        let mut registry = PlateRegistry::new();
        registry.insert(plate_at(0, 0, [0.0, 0.0, 1.0], 0.0));
        registry.insert(plate_at(1, 1000, [1.0, 0.0, 0.0], 0.0));

        let params = WorldParameters::default();
        let mut data = WorldData::new(grid, params);
        repartition_hexes(&mut data, &registry);
        let before = data.plate_id.clone();

        if let Some(p) = registry.plates_mut().get_mut(&PlateId(1)) {
            p.accumulated_rotation_rad = 0.3;
        }
        repartition_hexes(&mut data, &registry);

        let changed = before
            .iter()
            .zip(data.plate_id.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            changed > 0,
            "expected some hexes to change plate after rotation"
        );
    }
}
