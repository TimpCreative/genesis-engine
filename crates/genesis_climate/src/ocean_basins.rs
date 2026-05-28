//! Ocean basin identification (Doc 07 §8.1).
//!
//! Connected-component analysis of below-sea-level hexes with sill-height
//! permeability through low land bridges. Produces per-hex basin assignment
//! and per-basin metadata.

use std::collections::VecDeque;

use genesis_core::HexId;
use genesis_core::data::{BasinId, WorldData};

use crate::state::{OceanBasin, OceanBasins};

/// Ocean hex count at or above this in a non-main component is a marginal sea, not inland.
const MIN_OPEN_OCEAN_HEXES: u32 = 500;

/// Identifies all ocean basins on the planet.
///
/// Writes `WorldData.basin_id` per hex and returns [`OceanBasins`] metadata.
/// Basins are sorted by descending hex_count, so `BasinId(0)` is the largest.
pub fn identify_ocean_basins(data: &mut WorldData) -> OceanBasins {
    let n = data.cell_count() as usize;
    let sea_level = data.sea_level_m;
    let sill_height = data.parameters.core.climate.ocean_basin_sill_height_m;
    let grid = &data.grid;

    for i in 0..n {
        data.basin_id[i] = BasinId::NONE;
    }

    let mut connectivity = vec![u32::MAX; n];
    let mut next_component: u32 = 0;

    for i in 0..n {
        if connectivity[i] != u32::MAX {
            continue;
        }
        if !is_ocean(data, i) {
            continue;
        }
        flood_connectivity_component(
            data,
            grid,
            i,
            next_component,
            sea_level,
            sill_height,
            &mut connectivity,
        );
        next_component += 1;
    }

    if next_component == 0 {
        return OceanBasins { basins: Vec::new() };
    }

    let mut ocean_hexes_per_component: Vec<Vec<usize>> = vec![Vec::new(); next_component as usize];
    for i in 0..n {
        if !is_ocean(data, i) {
            continue;
        }
        let cid = connectivity[i] as usize;
        ocean_hexes_per_component[cid].push(i);
    }

    let main_component = ocean_hexes_per_component
        .iter()
        .enumerate()
        .max_by_key(|(_, hexes)| hexes.len())
        .map(|(id, _)| id)
        .unwrap_or(0);

    let world_ocean_reachable = flood_world_ocean_reachability(
        data,
        grid,
        main_component as u32,
        sill_height,
        &connectivity,
    );

    let mut provisional_meta: Vec<(u32, u32, [f64; 3], f64, f64, bool)> = Vec::new();

    for (component_id, ocean_indices) in ocean_hexes_per_component.iter().enumerate() {
        if ocean_indices.is_empty() {
            continue;
        }

        let is_inland = is_inland_component(
            ocean_indices,
            &world_ocean_reachable,
            &connectivity,
            component_id as u32,
            data,
            grid,
        );

        let mut hex_count: u32 = 0;
        let mut centroid_sum = [0.0_f64, 0.0_f64, 0.0_f64];
        let mut lat_min = f64::MAX;
        let mut lat_max = f64::MIN;

        for &idx in ocean_indices {
            hex_count += 1;
            let hex = HexId(idx as u32);
            let center = grid.cell_center_direction(hex);
            centroid_sum[0] += center[0];
            centroid_sum[1] += center[1];
            centroid_sum[2] += center[2];
            let (lat_rad, _lon_rad) = grid.center_lat_lon(hex);
            lat_min = lat_min.min(lat_rad);
            lat_max = lat_max.max(lat_rad);
        }

        provisional_meta.push((
            component_id as u32,
            hex_count,
            centroid_sum,
            lat_min,
            lat_max,
            is_inland,
        ));
    }

    provisional_meta.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let mut component_to_final: Vec<BasinId> = vec![BasinId::NONE; next_component as usize];
    for (final_id, meta) in provisional_meta.iter().enumerate() {
        component_to_final[meta.0 as usize] = BasinId(final_id as u16);
    }

    for i in 0..n {
        if is_ocean(data, i) {
            data.basin_id[i] = component_to_final[connectivity[i] as usize];
        }
    }

    let mut basins = Vec::with_capacity(provisional_meta.len());
    for (final_id, meta) in provisional_meta.iter().enumerate() {
        let mag = (meta.2[0] * meta.2[0] + meta.2[1] * meta.2[1] + meta.2[2] * meta.2[2]).sqrt();
        let centroid_dir = if mag > 0.0 {
            [meta.2[0] / mag, meta.2[1] / mag, meta.2[2] / mag]
        } else {
            [1.0, 0.0, 0.0]
        };
        let centroid_hex = grid.nearest_hex_direction(centroid_dir);

        basins.push(OceanBasin {
            id: BasinId(final_id as u16),
            centroid_hex,
            hex_count: meta.1,
            lat_min_rad: meta.3,
            lat_max_rad: meta.4,
            is_inland: meta.5,
        });
    }

    OceanBasins { basins }
}

fn is_ocean(data: &WorldData, idx: usize) -> bool {
    data.elevation_mean[idx] < data.sea_level_m
}

fn is_permeable_land(data: &WorldData, idx: usize, sill_height: f32) -> bool {
    let sea = data.sea_level_m;
    let elev = data.elevation_mean[idx];
    elev >= sea && elev <= sea + sill_height
}

fn can_traverse(data: &WorldData, idx: usize, sill_height: f32) -> bool {
    is_ocean(data, idx) || is_permeable_land(data, idx, sill_height)
}

fn flood_connectivity_component(
    data: &WorldData,
    grid: &genesis_core::HexGrid,
    start: usize,
    component_id: u32,
    sea_level: f32,
    sill_height: f32,
    connectivity: &mut [u32],
) {
    let n = connectivity.len();
    let mut queue = VecDeque::new();
    queue.push_back(start);
    connectivity[start] = component_id;

    while let Some(i) = queue.pop_front() {
        let hex = HexId(i as u32);
        let mut neighbors: Vec<HexId> = grid.neighbors(hex).to_vec();
        neighbors.sort_by_key(|h| h.0);

        for neighbor in neighbors {
            let j = neighbor.0 as usize;
            if j >= n || connectivity[j] != u32::MAX {
                continue;
            }
            if !can_traverse(data, j, sill_height) {
                continue;
            }
            connectivity[j] = component_id;
            queue.push_back(j);
        }
    }

    let _ = sea_level;
}

/// BFS from the main ocean component through ocean hexes and permeable land (sill height).
fn flood_world_ocean_reachability(
    data: &WorldData,
    grid: &genesis_core::HexGrid,
    main_component: u32,
    sill_height: f32,
    connectivity: &[u32],
) -> Vec<bool> {
    let n = connectivity.len();
    let mut reachable = vec![false; n];
    let mut queue = VecDeque::new();

    for i in 0..n {
        if is_ocean(data, i) && connectivity[i] == main_component {
            reachable[i] = true;
            queue.push_back(i);
        }
    }

    while let Some(i) = queue.pop_front() {
        let hex = HexId(i as u32);
        let mut neighbors: Vec<HexId> = grid.neighbors(hex).to_vec();
        neighbors.sort_by_key(|h| h.0);

        for neighbor in neighbors {
            let j = neighbor.0 as usize;
            if j >= n || reachable[j] || !can_traverse(data, j, sill_height) {
                continue;
            }
            reachable[j] = true;
            queue.push_back(j);
        }
    }

    reachable
}

/// Inland seas are enclosed or not connected to the world ocean via sill-permeable paths.
fn is_inland_component(
    ocean_indices: &[usize],
    world_ocean_reachable: &[bool],
    connectivity: &[u32],
    component_id: u32,
    data: &WorldData,
    grid: &genesis_core::HexGrid,
) -> bool {
    if ocean_indices.len() as u32 >= MIN_OPEN_OCEAN_HEXES {
        return false;
    }
    if fully_enclosed_ocean_component(ocean_indices, connectivity, component_id, data, grid) {
        return true;
    }
    !ocean_indices
        .iter()
        .any(|&idx| world_ocean_reachable.get(idx).copied().unwrap_or(false))
}

fn fully_enclosed_ocean_component(
    ocean_indices: &[usize],
    connectivity: &[u32],
    component_id: u32,
    data: &WorldData,
    grid: &genesis_core::HexGrid,
) -> bool {
    for &idx in ocean_indices {
        let hex = HexId(idx as u32);
        let mut neighbors: Vec<HexId> = grid.neighbors(hex).to_vec();
        neighbors.sort_by_key(|h| h.0);
        for neighbor in neighbors {
            let j = neighbor.0 as usize;
            if is_ocean(data, j) && connectivity[j] != component_id {
                return false;
            }
        }
    }
    true
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
        assert!(!basins.basins[0].is_inland);

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
    fn low_land_bridge_connects_oceans_with_sill() {
        let mut world = world_at_level(5);
        let n = world.data.cell_count() as usize;
        let grid = &world.data.grid;

        for i in 0..n {
            world.data.elevation_mean[i] = -2000.0;
        }
        world.data.sea_level_m = 0.0;
        world.data.parameters.core.climate.ocean_basin_sill_height_m = 50.0;

        let mut land_bridge = None;
        for i in 1..n - 1 {
            let hex = HexId(i as u32);
            let prev = HexId((i - 1) as u32);
            let next = HexId((i + 1) as u32);
            if grid.distance_km(prev, hex) < 500.0 && grid.distance_km(hex, next) < 500.0 {
                land_bridge = Some(i);
                break;
            }
        }
        let bridge = land_bridge.expect("bridge candidate");
        world.data.elevation_mean[bridge] = 10.0;

        let basins = identify_ocean_basins(&mut world.data);
        assert_eq!(
            basins.basins.len(),
            1,
            "low sill should connect oceans into one basin"
        );
    }

    #[test]
    fn mountain_isthmus_separates_oceans() {
        let mut world = world_at_level(5);
        let n = world.data.cell_count() as usize;

        for i in 0..n {
            world.data.elevation_mean[i] = 500.0;
        }
        world.data.sea_level_m = 0.0;
        world.data.parameters.core.climate.ocean_basin_sill_height_m = 50.0;

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
        assert!(
            basins.basins.len() >= 2,
            "land between distant oceans should yield separate basins"
        );
    }

    #[test]
    fn enclosed_deep_inland_sea_is_inland() {
        let mut world = world_at_level(5);
        let n = world.data.cell_count() as usize;

        for i in 0..n {
            world.data.elevation_mean[i] = 500.0;
        }
        world.data.sea_level_m = 0.0;

        let pocket = HexId(20);
        world.data.elevation_mean[pocket.0 as usize] = -800.0;

        let basins = identify_ocean_basins(&mut world.data);
        assert_eq!(basins.basins.len(), 1);
        assert!(
            basins.basins[0].is_inland,
            "enclosed deep water should be an inland basin"
        );
    }

    #[test]
    fn isolated_water_pockets_get_separate_basins() {
        let mut world = world_at_level(5);
        let n = world.data.cell_count() as usize;

        for i in 0..n {
            world.data.elevation_mean[i] = 500.0;
        }
        world.data.sea_level_m = 0.0;
        world.data.parameters.core.climate.ocean_basin_sill_height_m = 0.0;

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
        assert!(
            basins.basins.iter().filter(|b| b.is_inland).count() >= 1,
            "isolated pockets should be inland basins"
        );
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
