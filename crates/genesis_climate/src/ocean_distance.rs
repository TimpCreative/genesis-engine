//! Distance-to-water field computation (Doc 07 §5, Doc 08 §17.2 lake effect).
//!
//! Multi-source BFS from ocean and large inland water bodies outward through
//! land hexes. Recomputed each climate tick because continents drift and
//! hydrology's standing-water mask changes (one-tick lag is inherent: climate
//! runs before hydrology in the coordinator).

use std::collections::VecDeque;

use genesis_core::HexId;
use genesis_core::data::WorldData;

use crate::hydro_mask::{is_hydro_ocean, is_large_inland_water};

/// Minimum hex count for an inland body to act as a climate moisture source
/// (Doc 08 §2.3 / §17.2 — Caspian-scale lake effect).
pub const LAKE_CLIMATE_MIN_HEXES: usize = 8;

/// Computes `data.distance_to_ocean_km` for every hex as distance-to-*water*:
/// ocean cells and inland bodies of at least [`LAKE_CLIMATE_MIN_HEXES`] are
/// sources at 0 km.
///
/// Prefers hydrology's `water_body_id` / registry when present; falls back to
/// `elevation < sea_level` while hydrology has not yet written an ocean.
pub fn compute_distance_to_ocean(data: &mut WorldData) {
    let n = data.cell_count() as usize;

    let mut is_source = vec![false; n];
    for (i, source) in is_source.iter_mut().enumerate() {
        *source = is_hydro_ocean(data, i) || is_large_inland_water(data, i);
    }

    let mut distances = vec![f32::INFINITY; n];
    let mut queue: VecDeque<HexId> = VecDeque::with_capacity(n);

    for (i, &source) in is_source.iter().enumerate() {
        if source {
            distances[i] = 0.0;
            queue.push_back(HexId(i as u32));
        }
    }

    while let Some(hex) = queue.pop_front() {
        let current_dist = distances[hex.0 as usize];

        let neighbors = data.grid.neighbors_sorted(hex);

        for &neighbor in neighbors {
            let n_idx = neighbor.0 as usize;
            if is_source[n_idx] {
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
    use genesis_core::data::{WaterBody, WaterBodyId, WaterBodyKind};
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
    fn hydro_ocean_mask_is_preferred_over_elevation() {
        let mut world = world_at_level(5);
        let n = world.data.cell_count() as usize;
        // All land by elevation, but hex 0 marked as ocean via hydrology.
        world.data.elevation_mean.fill(100.0);
        world.data.sea_level_m = 0.0;
        world.data.water_body_id[0] = WaterBodyId(0);
        world.data.water_bodies.insert(
            WaterBodyId(0),
            WaterBody {
                id: WaterBodyId(0),
                kind: WaterBodyKind::Ocean,
                surface_m: 0.0,
                area_km2: 1.0e6,
                volume_km3: 1.0e6,
                salinity: 0.0,
                outlet: None,
            },
        );
        compute_distance_to_ocean(&mut world.data);
        assert_eq!(world.data.distance_to_ocean_km[0], 0.0);
        assert!(
            world.data.distance_to_ocean_km[1..n]
                .iter()
                .any(|&d| d > 0.0)
        );
    }
}
