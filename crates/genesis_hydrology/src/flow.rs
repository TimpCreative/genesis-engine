//! Surface water flow: directions and accumulated discharge (Doc 08 scope).
//!
//! Every land hex drains toward its steepest-descent neighbor; runoff from
//! precipitation accumulates downstream in elevation order. Hexes with no
//! lower neighbor are endorheic sinks — their accumulated volume marks
//! (future) lakes. Deterministic: fixed iteration orders, no RNG.

use genesis_core::data::WorldData;
use genesis_core::grid::Direction;
use genesis_core::{HexGrid, HexId};

/// Fraction of annual precipitation that becomes surface runoff (the rest
/// evaporates or infiltrates). Earth land average is ~0.35–0.45.
pub const RUNOFF_COEFFICIENT: f64 = 0.4;

/// Runoff assumed when the climate layer has not populated precipitation
/// (tectonics-only worlds), mm/year.
pub const DEFAULT_PRECIPITATION_MM: f32 = 800.0;

/// Mean hex area for this grid (m²).
pub fn hex_area_m2(grid: &HexGrid) -> f64 {
    let radius_m = grid.planet_radius_km() * 1000.0;
    4.0 * std::f64::consts::PI * radius_m * radius_m / f64::from(grid.cell_count())
}

/// Writes `WorldData.flow_direction`: steepest-descent neighbor slot for land
/// hexes, `None` for ocean hexes and endorheic pits. Ties break toward the
/// lowest neighbor `HexId`.
pub fn compute_flow_directions(data: &mut WorldData) {
    let n = data.cell_count() as usize;
    let sea = data.sea_level_m;

    for i in 0..n {
        let hex = HexId(i as u32);
        if data.elevation_mean[i] < sea {
            data.flow_direction[i] = None;
            continue;
        }

        let own_elevation = data.elevation_mean[i];
        let mut best: Option<(Direction, f32, HexId)> = None;
        for (slot, &neighbor) in data.grid.neighbors(hex).iter().enumerate() {
            let j = neighbor.0 as usize;
            if j >= n {
                continue;
            }
            let neighbor_elevation = data.elevation_mean[j];
            if neighbor_elevation >= own_elevation {
                continue;
            }
            let Some(direction) = Direction::from_index(slot) else {
                continue;
            };
            let replace = match &best {
                None => true,
                Some((_, best_elevation, best_id)) => {
                    neighbor_elevation < *best_elevation
                        || (neighbor_elevation == *best_elevation && neighbor < *best_id)
                }
            };
            if replace {
                best = Some((direction, neighbor_elevation, neighbor));
            }
        }
        data.flow_direction[i] = best.map(|(direction, _, _)| direction);
    }
}

/// Writes `WorldData.flow_volume` (m³/year): per-hex runoff accumulated along
/// flow directions. Land hexes are processed in descending elevation order
/// (ties: ascending `HexId`) so upstream volume is complete before it passes
/// downstream. Ocean hexes carry no flow volume.
pub fn compute_flow_accumulation(data: &mut WorldData) {
    let n = data.cell_count() as usize;
    let sea = data.sea_level_m;
    let area_m2 = hex_area_m2(&data.grid);
    let climate_active = data.precipitation.iter().any(|&p| p > 0.0);

    // Seed with local runoff.
    for i in 0..n {
        if data.elevation_mean[i] < sea {
            data.flow_volume[i] = 0.0;
            continue;
        }
        let precip_mm = if climate_active {
            data.precipitation[i]
        } else {
            DEFAULT_PRECIPITATION_MM
        };
        // mm/year × m² × 1e-3 → m³/year.
        data.flow_volume[i] = (f64::from(precip_mm) * area_m2 * 1e-3 * RUNOFF_COEFFICIENT) as f32;
    }

    // Descending elevation, ascending id: upstream-before-downstream.
    let mut order: Vec<usize> = (0..n).filter(|&i| data.elevation_mean[i] >= sea).collect();
    order.sort_by(|&a, &b| {
        data.elevation_mean[b]
            .partial_cmp(&data.elevation_mean[a])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.cmp(&b))
    });

    for &i in &order {
        let Some(direction) = data.flow_direction[i] else {
            continue;
        };
        let hex = HexId(i as u32);
        let neighbors = data.grid.neighbors(hex);
        let Some(&downstream) = neighbors.get(direction.index()) else {
            continue;
        };
        let j = downstream.0 as usize;
        if j >= n || data.elevation_mean[j] < sea {
            // Discharges into the ocean; volume leaves the land system.
            continue;
        }
        data.flow_volume[j] += data.flow_volume[i];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::create_world;
    use genesis_core::parameters::WorldParameters;

    fn test_world() -> genesis_core::World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        create_world(params).expect("world")
    }

    #[test]
    fn flow_directions_point_downhill() {
        let mut world = test_world();
        let n = world.data.cell_count() as usize;
        for i in 0..n {
            world.data.elevation_mean[i] = (i % 100) as f32 * 10.0 + 1.0;
        }
        world.data.sea_level_m = 0.0;
        compute_flow_directions(&mut world.data);

        for i in 0..n {
            let hex = HexId(i as u32);
            if let Some(direction) = world.data.flow_direction[i] {
                let neighbor = world.data.grid.neighbors(hex)[direction.index()];
                assert!(
                    world.data.elevation_mean[neighbor.0 as usize] < world.data.elevation_mean[i],
                    "flow at hex {i} must point downhill"
                );
            }
        }
    }

    #[test]
    fn ocean_hexes_have_no_flow() {
        let mut world = test_world();
        let n = world.data.cell_count() as usize;
        for i in 0..n {
            world.data.elevation_mean[i] = -1000.0;
        }
        world.data.sea_level_m = 0.0;
        compute_flow_directions(&mut world.data);
        compute_flow_accumulation(&mut world.data);
        for i in 0..n {
            assert!(world.data.flow_direction[i].is_none());
            assert_eq!(world.data.flow_volume[i], 0.0);
        }
    }

    #[test]
    fn accumulation_grows_downstream() {
        let mut world = test_world();
        let n = world.data.cell_count() as usize;
        // A single tall peak at hex 100 sloping to its neighbors: neighbors
        // must accumulate the peak's runoff on top of their own.
        for i in 0..n {
            world.data.elevation_mean[i] = 100.0;
        }
        let peak = HexId(100);
        world.data.elevation_mean[peak.0 as usize] = 2000.0;
        let downhill = world.data.grid.neighbors(peak)[0];
        world.data.elevation_mean[downhill.0 as usize] = 50.0;
        world.data.sea_level_m = 0.0;

        compute_flow_directions(&mut world.data);
        compute_flow_accumulation(&mut world.data);

        let base_runoff = (f64::from(DEFAULT_PRECIPITATION_MM)
            * hex_area_m2(&world.data.grid)
            * 1e-3
            * RUNOFF_COEFFICIENT) as f32;
        let downstream_volume = world.data.flow_volume[downhill.0 as usize];
        assert!(
            downstream_volume > base_runoff * 1.5,
            "downstream hex should carry upstream volume: {downstream_volume} vs {base_runoff}"
        );
    }

    #[test]
    fn accumulation_uses_precipitation_when_climate_active() {
        let mut world = test_world();
        let n = world.data.cell_count() as usize;
        for i in 0..n {
            world.data.elevation_mean[i] = 100.0;
            world.data.precipitation[i] = if i == 0 { 2400.0 } else { 800.0 };
        }
        world.data.sea_level_m = 0.0;
        compute_flow_directions(&mut world.data);
        compute_flow_accumulation(&mut world.data);
        assert!(
            world.data.flow_volume[0] > world.data.flow_volume[1] * 2.5,
            "3x precipitation should give ~3x runoff"
        );
    }

    #[test]
    fn flow_is_deterministic() {
        let mut a = test_world();
        let mut b = test_world();
        for world in [&mut a, &mut b] {
            let n = world.data.cell_count() as usize;
            for i in 0..n {
                world.data.elevation_mean[i] = ((i * 37) % 500) as f32 - 100.0;
            }
            world.data.sea_level_m = 0.0;
            compute_flow_directions(&mut world.data);
            compute_flow_accumulation(&mut world.data);
        }
        assert_eq!(a.data.flow_direction, b.data.flow_direction);
        assert_eq!(a.data.flow_volume, b.data.flow_volume);
    }
}
