//! Rebuilds [`WorldData`] per-hex fields from plate surfaces.

use genesis_core::data::{BedrockType, WorldData};
use genesis_core::{HexId, PlateId};

use crate::frames::world_to_plate_local;
use crate::plate::PlateRegistry;
use crate::plate_surface::type_baseline;

/// Rebuilds `elevation_mean`, `elevation_relief`, `bedrock_type`, and `fertility` from
/// each plate's surface storage. Called after motion/repartition and again after surface
/// mutations each Geological tick.
pub fn rebuild_world_from_plate_surfaces(data: &mut WorldData, registry: &PlateRegistry) {
    let n = data.cell_count() as usize;
    let grid = &data.grid;

    for i in 0..n {
        let world_hex = HexId(i as u32);
        let plate_id = data.plate_id[i];

        if plate_id == PlateId::NONE {
            data.elevation_mean[i] = 0.0;
            data.elevation_relief[i] = 0.0;
            data.bedrock_type[i] = BedrockType::Unknown;
            data.fertility[i] = 0.0;
            continue;
        }

        let Some(plate) = registry.get(plate_id) else {
            data.elevation_mean[i] = 0.0;
            data.elevation_relief[i] = 0.0;
            data.bedrock_type[i] = BedrockType::Unknown;
            data.fertility[i] = 0.0;
            continue;
        };

        let plate_local_hex = world_to_plate_local(grid, world_hex, plate);

        match plate.surface.get(plate_local_hex) {
            Some(feature) => {
                data.elevation_mean[i] = feature.elevation_m;
                data.elevation_relief[i] = feature.relief_m;
                data.bedrock_type[i] = feature.bedrock;
                data.fertility[i] = feature.fertility;
            }
            None => {
                let (elev, bedrock) = type_baseline(plate.plate_type);
                data.elevation_mean[i] = elev;
                data.elevation_relief[i] = 0.0;
                data.bedrock_type[i] = bedrock;
                data.fertility[i] = 0.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::time::WorldYear;
    use genesis_core::{PlateId, create_world};

    use crate::frames::plate_local_to_world;
    use crate::initial_terrain::CONTINENTAL_BASE_ELEVATION_M;
    use crate::plate::{Plate, PlateClass, PlateRegistry, PlateType};
    use crate::plate_surface::{PlateSurface, SurfaceFeature};

    fn test_registry(cell_count: usize) -> PlateRegistry {
        let mut registry = PlateRegistry::new();
        registry.insert(Plate {
            id: PlateId(0),
            plate_type: PlateType::Continental,
            plate_class: PlateClass::Major,
            seed_hex: HexId(0),
            motion_axis: [0.0, 0.0, 1.0],
            motion_rate_rad_per_year: 1e-8,
            age_year: WorldYear::FORMATION,
            target_fraction: 0.5,
            accumulated_rotation_rad: 0.0,
            last_nonempty_year: WorldYear::FORMATION,
            surface: PlateSurface::new(cell_count),
        });
        registry.insert(Plate {
            id: PlateId(1),
            plate_type: PlateType::Oceanic,
            plate_class: PlateClass::Major,
            seed_hex: HexId(100),
            motion_axis: [0.0, 1.0, 0.0],
            motion_rate_rad_per_year: 5e-9,
            age_year: WorldYear::FORMATION,
            target_fraction: 0.3,
            accumulated_rotation_rad: 0.0,
            last_nonempty_year: WorldYear::FORMATION,
            surface: PlateSurface::new(cell_count),
        });
        registry
    }

    #[test]
    fn empty_surfaces_produce_baseline_elevations() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        let registry = test_registry(n);

        for (i, pid) in world.data.plate_id.iter_mut().enumerate() {
            *pid = if i < n / 2 { PlateId(0) } else { PlateId(1) };
        }

        rebuild_world_from_plate_surfaces(&mut world.data, &registry);

        assert!(
            world.data.elevation_mean[0] > 0.0,
            "continental baseline should be positive"
        );
        assert!(
            world.data.elevation_mean[n - 1] < 0.0,
            "oceanic baseline should be negative"
        );
    }

    #[test]
    fn populated_surfaces_produce_correct_elevations() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        let mut registry = test_registry(n);

        for i in 0..n {
            world.data.plate_id[i] = PlateId(0);
        }

        {
            let plate = registry.plates_mut().get_mut(&PlateId(0)).unwrap();
            plate.surface.set(
                HexId(5),
                SurfaceFeature {
                    elevation_m: 3000.0,
                    relief_m: 500.0,
                    bedrock: BedrockType::Metamorphic,
                    fertility: 0.1,
                    age_year: 0,
                },
            );
        }

        rebuild_world_from_plate_surfaces(&mut world.data, &registry);

        assert_eq!(world.data.elevation_mean[5], 3000.0);
        assert_eq!(world.data.elevation_relief[5], 500.0);
        assert_eq!(world.data.bedrock_type[5], BedrockType::Metamorphic);
        assert!((world.data.fertility[5] - 0.1).abs() < 1e-6);
    }

    #[test]
    fn rotation_moves_features_in_world_view() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        let mut registry = test_registry(n);

        for i in 0..n {
            world.data.plate_id[i] = PlateId(0);
        }

        let plate_local = HexId(50);
        let peak_elev = 4000.0_f32;

        {
            let plate = registry.plates_mut().get_mut(&PlateId(0)).unwrap();
            plate.surface.set(
                plate_local,
                SurfaceFeature {
                    elevation_m: peak_elev,
                    relief_m: 800.0,
                    bedrock: BedrockType::Igneous,
                    fertility: 0.0,
                    age_year: 0,
                },
            );
        }

        rebuild_world_from_plate_surfaces(&mut world.data, &registry);
        let world_at_zero = plate_local_to_world(
            &world.data.grid,
            plate_local,
            registry.get(PlateId(0)).unwrap(),
            plate_local,
        );
        assert_eq!(
            world.data.elevation_mean[world_at_zero.0 as usize],
            peak_elev
        );

        {
            let plate = registry.plates_mut().get_mut(&PlateId(0)).unwrap();
            plate.accumulated_rotation_rad = 0.5;
        }

        rebuild_world_from_plate_surfaces(&mut world.data, &registry);
        let world_after_rotation = plate_local_to_world(
            &world.data.grid,
            plate_local,
            registry.get(PlateId(0)).unwrap(),
            world_at_zero,
        );

        assert_ne!(world_at_zero, world_after_rotation);
        assert_eq!(
            world.data.elevation_mean[world_after_rotation.0 as usize],
            peak_elev
        );
        assert!(
            world.data.elevation_mean[world_at_zero.0 as usize] < peak_elev - 100.0
                || (world.data.elevation_mean[world_at_zero.0 as usize]
                    - CONTINENTAL_BASE_ELEVATION_M)
                    .abs()
                    < 100.0,
            "old world position should no longer carry the peak after rotation"
        );
    }
}
