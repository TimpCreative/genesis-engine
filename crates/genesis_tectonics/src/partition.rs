//! Voronoi re-partition of hexes to plates by effective seed positions.

use genesis_core::data::WorldData;
use genesis_core::{HexGrid, HexId, PlateId};

use crate::frames::current_world_to_birth_hex;
use crate::motion::effective_position_direction;
use crate::plate::PlateRegistry;
use crate::plate_surface::SurfaceFeature;

/// Reassigns every hex to the plate whose effective position is nearest (angular distance).
///
/// Iterates hexes in ascending [`HexId`] order. Ties break on lowest [`PlateId`].
///
/// When a hex changes owner, the displayed terrain (`elevation_mean`, relief, bedrock,
/// fertility) is copied onto the new plate's surface at the correct birth index so
/// [`crate::world_rebuild::rebuild_world_from_plate_surfaces`] does not fall back to type
/// baselines at stale indices.
pub fn repartition_hexes(data: &mut WorldData, registry: &mut PlateRegistry) {
    let grid = &data.grid;
    let n = data.plate_id.len();
    let plate_ids = registry.plate_ids();
    let tick_year = data.current_year.value();

    let mut new_owners = Vec::with_capacity(n);
    for i in 0..n {
        let hex = HexId(i as u32);
        new_owners.push(nearest_plate(grid, hex, registry, &plate_ids));
    }

    for (i, &new_owner) in new_owners.iter().enumerate() {
        let old_owner = data.plate_id[i];
        if old_owner == new_owner {
            continue;
        }

        let feature = SurfaceFeature {
            elevation_m: data.elevation_mean[i],
            relief_m: data.elevation_relief[i],
            bedrock: data.bedrock_type[i],
            fertility: data.fertility[i],
            age_year: tick_year,
        };

        if new_owner == PlateId::NONE {
            continue;
        }

        let Some(new_plate) = registry.get(new_owner) else {
            continue;
        };
        let world_hex = HexId(i as u32);
        let birth_hex = current_world_to_birth_hex(grid, world_hex, new_plate);

        if let Some(plate) = registry.plates_mut().get_mut(&new_owner) {
            plate.surface.set(birth_hex, feature);
        }
    }

    data.plate_id.copy_from_slice(&new_owners);
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
    use genesis_core::data::BedrockType;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::time::WorldYear;
    use genesis_core::{HexGrid, HexId, PlateId};

    use crate::frames::current_world_to_birth_hex;
    use crate::plate::{Plate, PlateClass, PlateRegistry, PlateType};
    use crate::plate_surface::{PlateSurface, SurfaceFeature};
    use crate::world_rebuild::rebuild_world_from_plate_surfaces;

    const EARTH_RADIUS_KM: f64 = 6371.0;

    fn plate_at(id: u16, seed: u32, axis: [f64; 3], rotation: f64, plate_type: PlateType) -> Plate {
        Plate {
            id: PlateId(id),
            plate_type,
            plate_class: PlateClass::Major,
            seed_hex: HexId(seed),
            motion_axis: axis,
            motion_rate_rad_per_year: 1e-8,
            age_year: WorldYear::FORMATION,
            target_fraction: 0.5,
            accumulated_rotation_rad: rotation,
            last_nonempty_year: WorldYear::FORMATION,
            surface: PlateSurface::new(10_000),
        }
    }

    #[test]
    fn two_plates_assign_by_nearest_effective_position() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let mut registry = PlateRegistry::new();
        registry.insert(plate_at(0, 0, [0.0, 0.0, 1.0], 0.0, PlateType::Oceanic));
        registry.insert(plate_at(1, 500, [0.0, 0.0, 1.0], 0.0, PlateType::Oceanic));

        let params = WorldParameters::default();
        let mut data = WorldData::new(grid, params);
        data.plate_id.fill(PlateId::NONE);

        repartition_hexes(&mut data, &mut registry);

        assert_ne!(data.plate_id[0], PlateId::NONE);
        assert_eq!(data.plate_id[0], data.plate_id[0]);
    }

    #[test]
    fn tie_breaks_to_lowest_plate_id() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let mut registry = PlateRegistry::new();
        let seed_hex = HexId(10);
        registry.insert(plate_at(
            1,
            seed_hex.0,
            [1.0, 0.0, 0.0],
            0.0,
            PlateType::Oceanic,
        ));
        registry.insert(plate_at(
            0,
            seed_hex.0,
            [1.0, 0.0, 0.0],
            0.0,
            PlateType::Oceanic,
        ));

        let params = WorldParameters::default();
        let data = WorldData::new(grid, params);

        let owner = nearest_plate(&data.grid, seed_hex, &registry, &registry.plate_ids());
        assert_eq!(owner, PlateId(0));
    }

    #[test]
    fn rotation_changes_some_assignments() {
        let grid = HexGrid::new(5, EARTH_RADIUS_KM).expect("grid");
        let mut registry = PlateRegistry::new();
        registry.insert(plate_at(0, 0, [0.0, 0.0, 1.0], 0.0, PlateType::Oceanic));
        registry.insert(plate_at(1, 1000, [1.0, 0.0, 0.0], 0.0, PlateType::Oceanic));

        let params = WorldParameters::default();
        let mut data = WorldData::new(grid, params);
        repartition_hexes(&mut data, &mut registry);
        let before = data.plate_id.clone();

        if let Some(p) = registry.plates_mut().get_mut(&PlateId(1)) {
            p.accumulated_rotation_rad = 0.3;
        }
        repartition_hexes(&mut data, &mut registry);

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

    #[test]
    fn ownership_change_migrates_terrain_to_new_plate_surface() {
        let grid = HexGrid::new(5, EARTH_RADIUS_KM).expect("grid");
        let cell_count = grid.cell_count() as usize;
        let mut registry = PlateRegistry::new();
        registry.insert(plate_at(0, 0, [0.0, 0.0, 1.0], 0.0, PlateType::Continental));
        registry.insert(plate_at(1, 1000, [1.0, 0.0, 0.0], 0.0, PlateType::Oceanic));

        let params = WorldParameters::default();
        let mut data = WorldData::new(grid, params);
        repartition_hexes(&mut data, &mut registry);

        let target_hex = HexId(50);
        let idx = target_hex.0 as usize;
        assert!(idx < cell_count);

        let old_owner = data.plate_id[idx];
        let new_owner = if old_owner == PlateId(0) {
            PlateId(1)
        } else {
            PlateId(0)
        };

        data.elevation_mean[idx] = 1234.0;
        data.elevation_relief[idx] = 50.0;
        data.bedrock_type[idx] = BedrockType::Metamorphic;
        data.fertility[idx] = 0.25;
        data.plate_id[idx] = old_owner;

        data.plate_id[idx] = new_owner;
        let feature = SurfaceFeature {
            elevation_m: data.elevation_mean[idx],
            relief_m: data.elevation_relief[idx],
            bedrock: data.bedrock_type[idx],
            fertility: data.fertility[idx],
            age_year: 0,
        };
        let new_plate = registry.get(new_owner).expect("plate");
        let birth_hex = current_world_to_birth_hex(&data.grid, target_hex, new_plate);
        registry
            .plates_mut()
            .get_mut(&new_owner)
            .expect("plate")
            .surface
            .set(birth_hex, feature);

        rebuild_world_from_plate_surfaces(&mut data, &registry);

        assert!(
            (data.elevation_mean[idx] - 1234.0).abs() < 1e-3,
            "expected migrated elevation on new plate, got {}",
            data.elevation_mean[idx]
        );
        assert_eq!(data.bedrock_type[idx], BedrockType::Metamorphic);
    }
}
