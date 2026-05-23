//! Plate-frame elevation advection. When plates rotate, surface features they carry
//! move with them in the world frame.

use std::collections::BTreeMap;

use glam::{DQuat, DVec3};

use genesis_core::data::{BedrockType, PlateOrigin, WorldData};
use genesis_core::time::WorldYear;
use genesis_core::{HexId, PlateId, unpack_plate_local};

use crate::plate::{PlateRegistry, PlateType};

/// Advects all plate-borne features one step based on current plate rotations.
///
/// For each hex with a `plate_origin`, computes where that feature should now be
/// in the world frame given the plate's `accumulated_rotation_rad`. Transports
/// elevation, bedrock, fertility, and `plate_origin` to the new world-frame hex.
///
/// Collision resolution: lower `plate_id` wins; same plate → higher `age_year` wins.
/// Hexes without transported features get background elevation for their current plate.
pub fn advect_plate_features(
    data: &mut WorldData,
    registry: &PlateRegistry,
    current_year: WorldYear,
) {
    let _ = current_year;
    let n = data.cell_count() as usize;
    let grid = &data.grid;

    #[derive(Clone, Copy)]
    struct Transport {
        source_hex: HexId,
        plate_id: PlateId,
        age_year: i64,
    }

    let mut transports: BTreeMap<HexId, Transport> = BTreeMap::new();

    for i in 0..n {
        let Some(origin) = data.plate_origin[i] else {
            continue;
        };

        let Some(plate) = registry.get(origin.plate) else {
            continue;
        };

        let local_pos = unpack_plate_local(
            origin.plate_local_x,
            origin.plate_local_y,
            origin.plate_local_z,
        );
        let local_v = DVec3::new(local_pos[0], local_pos[1], local_pos[2]).normalize_or_zero();
        if local_v.length_squared() < 0.5 {
            continue;
        }

        let axis = DVec3::new(
            plate.motion_axis[0],
            plate.motion_axis[1],
            plate.motion_axis[2],
        )
        .normalize();
        let q = DQuat::from_axis_angle(axis, plate.accumulated_rotation_rad);
        let world_v = (q * local_v).normalize();
        let dest_hex =
            grid.nearest_hex_direction_from(HexId(i as u32), [world_v.x, world_v.y, world_v.z]);

        let new_transport = Transport {
            source_hex: HexId(i as u32),
            plate_id: origin.plate,
            age_year: origin.age_year,
        };

        match transports.get(&dest_hex) {
            None => {
                transports.insert(dest_hex, new_transport);
            }
            Some(existing) => {
                let new_better = new_transport.plate_id < existing.plate_id
                    || (new_transport.plate_id == existing.plate_id
                        && new_transport.age_year > existing.age_year);
                if new_better {
                    transports.insert(dest_hex, new_transport);
                }
            }
        }
    }

    let mut new_elevation = vec![0.0_f32; n];
    let mut new_relief = vec![0.0_f32; n];
    let mut new_bedrock = vec![BedrockType::Unknown; n];
    let mut new_fertility = vec![0.0_f32; n];
    let mut new_plate_origin: Vec<Option<PlateOrigin>> = vec![None; n];

    for (dest_hex, transport) in &transports {
        let dst_idx = dest_hex.0 as usize;
        let src_idx = transport.source_hex.0 as usize;
        if dst_idx >= n || src_idx >= n {
            continue;
        }

        new_elevation[dst_idx] = data.elevation_mean[src_idx];
        new_relief[dst_idx] = data.elevation_relief[src_idx];
        new_bedrock[dst_idx] = data.bedrock_type[src_idx];
        new_fertility[dst_idx] = data.fertility[src_idx].max(0.0);
        new_plate_origin[dst_idx] = data.plate_origin[src_idx];
    }

    for i in 0..n {
        if new_plate_origin[i].is_some() {
            continue;
        }
        let plate_id = data.plate_id[i];
        if plate_id == PlateId::NONE {
            continue;
        }
        let Some(plate) = registry.get(plate_id) else {
            continue;
        };
        let (background_elev, background_bedrock) = match plate.plate_type {
            PlateType::Continental => (500.0_f32, BedrockType::Igneous),
            PlateType::Oceanic => (-4000.0_f32, BedrockType::OceanicCrust),
        };
        new_elevation[i] = background_elev;
        new_relief[i] = 0.0;
        new_bedrock[i] = background_bedrock;
        new_fertility[i] = 0.0;
    }

    data.elevation_mean = new_elevation;
    data.elevation_relief = new_relief;
    data.bedrock_type = new_bedrock;
    data.fertility = new_fertility;
    data.plate_origin = new_plate_origin;
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::create_world;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{PlateId, pack_plate_local};

    use crate::plate::{Plate, PlateClass, PlateRegistry, PlateType};

    #[test]
    fn zero_rotation_is_identity() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");

        let mut registry = PlateRegistry::new();
        let plate = Plate {
            id: PlateId(0),
            plate_type: PlateType::Continental,
            plate_class: PlateClass::Major,
            seed_hex: HexId(0),
            motion_axis: [0.0, 0.0, 1.0],
            motion_rate_rad_per_year: 0.0,
            age_year: WorldYear::FORMATION,
            target_fraction: 1.0,
            accumulated_rotation_rad: 0.0,
            last_nonempty_year: WorldYear::FORMATION,
        };
        registry.insert(plate);

        let n = world.data.cell_count() as usize;
        for i in 0..n {
            world.data.plate_id[i] = PlateId(0);
            world.data.elevation_mean[i] = 500.0;
            let pos = world.data.grid.cell_center_direction(HexId(i as u32));
            let (px, py, pz) = pack_plate_local(pos);
            world.data.plate_origin[i] = Some(PlateOrigin {
                plate: PlateId(0),
                plate_local_x: px,
                plate_local_y: py,
                plate_local_z: pz,
                age_year: 0,
            });
        }

        let elevation_before = world.data.elevation_mean.clone();
        advect_plate_features(&mut world.data, &registry, WorldYear(1_000_000));
        assert_eq!(world.data.elevation_mean, elevation_before);
    }

    #[test]
    fn rotation_moves_features() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");

        let mut registry = PlateRegistry::new();
        let plate = Plate {
            id: PlateId(0),
            plate_type: PlateType::Continental,
            plate_class: PlateClass::Major,
            seed_hex: HexId(0),
            motion_axis: [0.0, 0.0, 1.0],
            motion_rate_rad_per_year: 0.0,
            age_year: WorldYear::FORMATION,
            target_fraction: 1.0,
            accumulated_rotation_rad: std::f64::consts::PI,
            last_nonempty_year: WorldYear::FORMATION,
        };
        registry.insert(plate);

        let n = world.data.cell_count() as usize;
        let test_hex = HexId(5);
        for i in 0..n {
            world.data.plate_id[i] = PlateId(0);
            world.data.elevation_mean[i] = 0.0;
            world.data.plate_origin[i] = None;
        }

        let pos = world.data.grid.cell_center_direction(test_hex);
        let (px, py, pz) = pack_plate_local(pos);
        world.data.plate_origin[test_hex.0 as usize] = Some(PlateOrigin {
            plate: PlateId(0),
            plate_local_x: px,
            plate_local_y: py,
            plate_local_z: pz,
            age_year: 0,
        });
        world.data.elevation_mean[test_hex.0 as usize] = 8000.0;

        advect_plate_features(&mut world.data, &registry, WorldYear(1_000_000));

        let test_idx = test_hex.0 as usize;
        assert!(
            world.data.elevation_mean[test_idx] < 1000.0,
            "feature should have moved away from origin hex; still at {}m",
            world.data.elevation_mean[test_idx]
        );

        let max_elev = world
            .data
            .elevation_mean
            .iter()
            .copied()
            .fold(f32::MIN, f32::max);
        assert!(
            max_elev > 7000.0,
            "feature should have transported to some hex; max elevation only {max_elev}m"
        );
    }

    #[test]
    #[ignore = "level 7 advection perf budget: cargo test -p genesis_tectonics advection_completes_quickly_at_level_7 -- --ignored --exact"]
    fn advection_completes_quickly_at_level_7() {
        use std::time::Instant;

        use crate::initial_generation::generate_initial_plates_data;
        use crate::initial_terrain::apply_formation_terrain;

        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 7;
        let mut world = create_world(params).expect("world");

        let mut registry = generate_initial_plates_data(&mut world.data, &world.rng);
        apply_formation_terrain(&mut world.data, &registry, &world.rng);

        for plate in registry.plates_mut().values_mut() {
            plate.accumulated_rotation_rad = 1e-4;
        }

        let start = Instant::now();
        advect_plate_features(&mut world.data, &registry, WorldYear(500_000));
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 500,
            "advection took {}ms at level 7; should be under 500ms",
            elapsed.as_millis()
        );
    }
}
