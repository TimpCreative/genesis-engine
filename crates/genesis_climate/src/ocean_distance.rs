//! Distance-to-ocean field computation (Doc 07 §5).
//!
//! Multi-source BFS from all ocean hexes outward through land hexes.
//! Recomputed each climate tick because continents drift.

use std::collections::VecDeque;

use genesis_core::HexId;
use genesis_core::data::WorldData;

/// Computes `data.distance_to_ocean_km` for every hex.
///
/// Ocean hexes (elevation < sea level) get 0.0; land hexes get the great-circle
/// distance to the nearest ocean hex via BFS through the icosahedral grid.
/// Worlds with no ocean leave all distances at `f32::INFINITY`.
///
/// Complexity: O(n) where n is cell count. At level 7 (~22K hexes), single-digit ms.
pub fn compute_distance_to_ocean(data: &mut WorldData) {
    let n = data.cell_count() as usize;
    let sea_level = data.sea_level_m;

    let mut distances = vec![f32::INFINITY; n];
    let mut queue: VecDeque<HexId> = VecDeque::with_capacity(n);

    for (i, &elevation) in data.elevation_mean.iter().enumerate() {
        if elevation < sea_level {
            distances[i] = 0.0;
            queue.push_back(HexId(i as u32));
        }
    }

    while let Some(hex) = queue.pop_front() {
        let current_dist = distances[hex.0 as usize];

        let mut neighbors: Vec<HexId> = data.grid.neighbors(hex).to_vec();
        neighbors.sort_by_key(|h| h.0);

        for neighbor in neighbors {
            let n_idx = neighbor.0 as usize;

            if data.elevation_mean[n_idx] < sea_level {
                continue;
            }

            let step_km = data.grid.distance_km(hex, neighbor) as f32;
            let new_dist = current_dist + step_km;

            if new_dist < distances[n_idx] {
                distances[n_idx] = new_dist;
                queue.push_back(neighbor);
            }
        }
    }

    data.distance_to_ocean_km = distances;
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
    fn all_ocean_world_has_zero_distances() {
        let mut world = world_at_level(5);
        for i in 0..world.data.cell_count() as usize {
            world.data.elevation_mean[i] = -100.0;
        }
        world.data.sea_level_m = 0.0;

        compute_distance_to_ocean(&mut world.data);

        for &d in &world.data.distance_to_ocean_km {
            assert_eq!(d, 0.0);
        }
    }

    #[test]
    fn all_land_world_has_infinity_distances() {
        let mut world = world_at_level(5);
        for i in 0..world.data.cell_count() as usize {
            world.data.elevation_mean[i] = 500.0;
        }
        world.data.sea_level_m = 0.0;

        compute_distance_to_ocean(&mut world.data);

        for &d in &world.data.distance_to_ocean_km {
            assert!(d.is_infinite(), "expected infinity, got {d}");
        }
    }

    #[test]
    fn ocean_hexes_have_zero_distance() {
        let mut world = world_at_level(5);
        for i in 0..world.data.cell_count() as usize {
            world.data.elevation_mean[i] = if i % 2 == 0 { -1000.0 } else { 500.0 };
        }
        world.data.sea_level_m = 0.0;

        compute_distance_to_ocean(&mut world.data);

        for (i, &d) in world.data.distance_to_ocean_km.iter().enumerate() {
            if world.data.elevation_mean[i] < 0.0 {
                assert_eq!(d, 0.0, "ocean hex {i} should have distance 0");
            }
        }
    }

    #[test]
    fn neighbor_of_ocean_has_short_distance() {
        let mut world = world_at_level(5);
        for i in 0..world.data.cell_count() as usize {
            world.data.elevation_mean[i] = 500.0;
        }
        world.data.elevation_mean[0] = -1000.0;
        world.data.sea_level_m = 0.0;

        compute_distance_to_ocean(&mut world.data);

        assert_eq!(world.data.distance_to_ocean_km[0], 0.0);

        for &neighbor in world.data.grid.neighbors(HexId(0)) {
            let d = world.data.distance_to_ocean_km[neighbor.0 as usize];
            assert!(
                d.is_finite(),
                "neighbor {neighbor:?} of ocean should be finite"
            );
            assert!(d > 0.0, "neighbor should be positive distance");
            assert!(d < 5000.0, "neighbor distance {d} suspiciously large");
        }
    }

    #[test]
    fn distance_is_deterministic() {
        let mut world_a = world_at_level(5);
        let mut world_b = world_at_level(5);

        for i in 0..world_a.data.cell_count() as usize {
            let e = if i % 7 == 0 { -2000.0 } else { 500.0 };
            world_a.data.elevation_mean[i] = e;
            world_b.data.elevation_mean[i] = e;
        }
        world_a.data.sea_level_m = 0.0;
        world_b.data.sea_level_m = 0.0;

        compute_distance_to_ocean(&mut world_a.data);
        compute_distance_to_ocean(&mut world_b.data);

        assert_eq!(
            world_a.data.distance_to_ocean_km,
            world_b.data.distance_to_ocean_km
        );
    }

    #[test]
    fn performance_at_level_7_under_50ms() {
        use std::time::Instant;

        let mut world = world_at_level(7);
        for i in 0..world.data.cell_count() as usize {
            world.data.elevation_mean[i] = if i % 4 == 0 { -2000.0 } else { 500.0 };
        }
        world.data.sea_level_m = 0.0;

        let start = Instant::now();
        compute_distance_to_ocean(&mut world.data);
        let elapsed_ms = start.elapsed().as_millis();

        assert!(
            elapsed_ms < 50,
            "distance-to-ocean took {elapsed_ms}ms at level 7; should be under 50ms"
        );
    }
}
