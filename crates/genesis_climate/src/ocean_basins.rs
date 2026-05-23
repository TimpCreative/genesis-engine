//! Ocean basin identification (Doc 07 §8.1).
//!
//! Connected-component analysis of below-sea-level hexes via BFS flood-fill.
//! Produces per-hex basin assignment and per-basin metadata.

use std::collections::VecDeque;

use genesis_core::HexId;
use genesis_core::data::{BasinId, WorldData};

use crate::state::{OceanBasin, OceanBasins};

/// Identifies all ocean basins on the planet by connected-component flood-fill.
///
/// Writes `WorldData.basin_id` per hex and returns [`OceanBasins`] metadata.
/// Basins are sorted by descending hex count, so `BasinId(0)` is the largest.
pub fn identify_ocean_basins(data: &mut WorldData) -> OceanBasins {
    let n = data.cell_count() as usize;
    let sea_level = data.sea_level_m;
    let grid = &data.grid;

    for i in 0..n {
        data.basin_id[i] = BasinId::NONE;
    }

    let mut provisional_ids = vec![u32::MAX; n];
    let mut basin_hex_counts: Vec<u32> = Vec::new();
    let mut basin_centroid_sums: Vec<[f64; 3]> = Vec::new();
    let mut basin_lat_min: Vec<f64> = Vec::new();
    let mut basin_lat_max: Vec<f64> = Vec::new();
    let mut next_id: u32 = 0;

    for i in 0..n {
        if data.elevation_mean[i] >= sea_level {
            continue;
        }
        if provisional_ids[i] != u32::MAX {
            continue;
        }

        let this_id = next_id;
        next_id += 1;
        let mut queue: VecDeque<HexId> = VecDeque::new();
        let mut hex_count: u32 = 0;
        let mut centroid_sum = [0.0_f64, 0.0_f64, 0.0_f64];
        let mut lat_min = f64::MAX;
        let mut lat_max = f64::MIN;

        queue.push_back(HexId(i as u32));
        provisional_ids[i] = this_id;

        while let Some(hex) = queue.pop_front() {
            hex_count += 1;

            let center = grid.cell_center_direction(hex);
            centroid_sum[0] += center[0];
            centroid_sum[1] += center[1];
            centroid_sum[2] += center[2];

            let (lat_rad, _lon_rad) = grid.center_lat_lon(hex);
            lat_min = lat_min.min(lat_rad);
            lat_max = lat_max.max(lat_rad);

            let mut neighbors: Vec<HexId> = grid.neighbors(hex).to_vec();
            neighbors.sort_by_key(|h| h.0);

            for neighbor in neighbors {
                let n_idx = neighbor.0 as usize;

                if data.elevation_mean[n_idx] >= sea_level {
                    continue;
                }
                if provisional_ids[n_idx] != u32::MAX {
                    continue;
                }

                provisional_ids[n_idx] = this_id;
                queue.push_back(neighbor);
            }
        }

        basin_hex_counts.push(hex_count);
        basin_centroid_sums.push(centroid_sum);
        basin_lat_min.push(lat_min);
        basin_lat_max.push(lat_max);
    }

    if next_id == 0 {
        return OceanBasins { basins: Vec::new() };
    }

    let mut order: Vec<u32> = (0..next_id).collect();
    order.sort_by(|a, b| {
        basin_hex_counts[*b as usize]
            .cmp(&basin_hex_counts[*a as usize])
            .then_with(|| a.cmp(b))
    });

    let mut prov_to_final = vec![BasinId::NONE; next_id as usize];
    for (final_id, &prov_id) in order.iter().enumerate() {
        prov_to_final[prov_id as usize] = BasinId(final_id as u16);
    }

    for i in 0..n {
        if provisional_ids[i] != u32::MAX {
            data.basin_id[i] = prov_to_final[provisional_ids[i] as usize];
        }
    }

    let mut basins = Vec::with_capacity(next_id as usize);
    for (final_id, &prov_id) in order.iter().enumerate() {
        let count = basin_hex_counts[prov_id as usize];
        let centroid_sum = basin_centroid_sums[prov_id as usize];
        let lat_min = basin_lat_min[prov_id as usize];
        let lat_max = basin_lat_max[prov_id as usize];

        let mag = (centroid_sum[0] * centroid_sum[0]
            + centroid_sum[1] * centroid_sum[1]
            + centroid_sum[2] * centroid_sum[2])
            .sqrt();
        let centroid_dir = if mag > 0.0 {
            [
                centroid_sum[0] / mag,
                centroid_sum[1] / mag,
                centroid_sum[2] / mag,
            ]
        } else {
            [1.0, 0.0, 0.0]
        };
        let centroid_hex = grid.nearest_hex_direction(centroid_dir);

        basins.push(OceanBasin {
            id: BasinId(final_id as u16),
            centroid_hex,
            hex_count: count,
            lat_min_rad: lat_min,
            lat_max_rad: lat_max,
        });
    }

    OceanBasins { basins }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::create_world;
    use genesis_core::parameters::WorldParameters;

    fn world_at_level(level: u8) -> genesis_core::World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = level;
        create_world(params).expect("world")
    }

    #[test]
    fn all_land_world_has_no_basins() {
        let mut world = world_at_level(5);
        for i in 0..world.data.cell_count() as usize {
            world.data.elevation_mean[i] = 500.0;
        }
        world.data.sea_level_m = 0.0;

        let basins = identify_ocean_basins(&mut world.data);

        assert_eq!(basins.basins.len(), 0);
        for &b in &world.data.basin_id {
            assert_eq!(b, BasinId::NONE);
        }
    }

    #[test]
    fn all_ocean_world_has_one_basin() {
        let mut world = world_at_level(5);
        for i in 0..world.data.cell_count() as usize {
            world.data.elevation_mean[i] = -2000.0;
        }
        world.data.sea_level_m = 0.0;

        let basins = identify_ocean_basins(&mut world.data);

        assert_eq!(basins.basins.len(), 1);
        assert_eq!(basins.basins[0].id, BasinId(0));
        assert_eq!(basins.basins[0].hex_count, world.data.cell_count());

        for &b in &world.data.basin_id {
            assert_eq!(b, BasinId(0));
        }
    }

    #[test]
    fn land_hexes_have_basin_none() {
        let mut world = world_at_level(5);
        for i in 0..world.data.cell_count() as usize {
            world.data.elevation_mean[i] = if i % 3 == 0 { 500.0 } else { -2000.0 };
        }
        world.data.sea_level_m = 0.0;

        identify_ocean_basins(&mut world.data);

        for (i, &b) in world.data.basin_id.iter().enumerate() {
            if world.data.elevation_mean[i] >= 0.0 {
                assert_eq!(b, BasinId::NONE, "land hex {i} should be NONE");
            } else {
                assert_ne!(b, BasinId::NONE, "ocean hex {i} should have a basin");
            }
        }
    }

    #[test]
    fn basins_sorted_by_size_descending() {
        let mut world = world_at_level(5);
        for i in 0..world.data.cell_count() as usize {
            world.data.elevation_mean[i] = -2000.0;
        }
        world.data.sea_level_m = 0.0;

        let basins = identify_ocean_basins(&mut world.data);

        for i in 1..basins.basins.len() {
            assert!(
                basins.basins[i - 1].hex_count >= basins.basins[i].hex_count,
                "basin {} count {} should be >= basin {} count {}",
                i - 1,
                basins.basins[i - 1].hex_count,
                i,
                basins.basins[i].hex_count
            );
        }
    }

    #[test]
    fn isolated_water_pockets_get_separate_basins() {
        let mut world = world_at_level(5);
        let n = world.data.cell_count() as usize;

        for i in 0..n {
            world.data.elevation_mean[i] = 500.0;
        }
        world.data.sea_level_m = 0.0;

        world.data.elevation_mean[0] = -2000.0;

        let mut distant_hex = 100;
        let dist_0 = world.data.grid.distance_km(HexId(0), HexId(100));
        if dist_0 < 1000.0 {
            for i in 100..n {
                if world.data.grid.distance_km(HexId(0), HexId(i as u32)) > 5000.0 {
                    distant_hex = i;
                    break;
                }
            }
        }
        world.data.elevation_mean[distant_hex] = -2000.0;

        let basins = identify_ocean_basins(&mut world.data);

        assert_eq!(basins.basins.len(), 2, "expected 2 isolated pockets");
        assert_eq!(basins.basins[0].hex_count, 1);
        assert_eq!(basins.basins[1].hex_count, 1);
    }

    #[test]
    fn determinism() {
        let mut world_a = world_at_level(5);
        let mut world_b = world_at_level(5);

        for i in 0..world_a.data.cell_count() as usize {
            let e = if i % 5 == 0 { 500.0 } else { -2000.0 };
            world_a.data.elevation_mean[i] = e;
            world_b.data.elevation_mean[i] = e;
        }
        world_a.data.sea_level_m = 0.0;
        world_b.data.sea_level_m = 0.0;

        let basins_a = identify_ocean_basins(&mut world_a.data);
        let basins_b = identify_ocean_basins(&mut world_b.data);

        assert_eq!(basins_a, basins_b);
        assert_eq!(world_a.data.basin_id, world_b.data.basin_id);
    }

    #[test]
    fn performance_at_level_7_under_50ms() {
        use std::time::Instant;

        let mut world = world_at_level(7);
        for i in 0..world.data.cell_count() as usize {
            world.data.elevation_mean[i] = if i % 3 == 0 { 500.0 } else { -2000.0 };
        }
        world.data.sea_level_m = 0.0;

        let start = Instant::now();
        identify_ocean_basins(&mut world.data);
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 50,
            "basin identification took {}ms at level 7; should be under 50ms",
            elapsed.as_millis()
        );
    }

    #[test]
    fn centroid_hex_is_within_basin() {
        let mut world = world_at_level(5);
        for i in 0..world.data.cell_count() as usize {
            world.data.elevation_mean[i] = -2000.0;
        }
        world.data.sea_level_m = 0.0;

        let basins = identify_ocean_basins(&mut world.data);

        for basin in &basins.basins {
            let centroid_basin_id = world.data.basin_id[basin.centroid_hex.0 as usize];
            assert_eq!(
                centroid_basin_id, basin.id,
                "basin {:?} centroid_hex belongs to basin {:?}, not its own basin",
                basin.id, centroid_basin_id
            );
        }
    }
}
