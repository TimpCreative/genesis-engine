//! Removes geologically unjustified coast artifacts (Doc 06 coast cleanup).
//!
//! Runs after [`crate::world_rebuild::rebuild_world_from_plate_surfaces`] each
//! Geological tick. Mutates plate surfaces via [`crate::plate_surface::modify_surface_at_world_hex`].

use std::collections::{BTreeSet, VecDeque};

use genesis_core::data::{BedrockType, WorldData};
use genesis_core::{HexId, PlateId};

use crate::boundary::BoundaryInfo;
use crate::plate::{PlateRegistry, PlateType};
use crate::plate_surface::modify_surface_at_world_hex;

/// Depth below sea level applied when submerging ephemeral islands (m).
pub const SUBMERGE_DEPTH_M: f32 = 10.0;

/// Elevation above sea level when filling artifact inland puddles (m).
pub const FILL_ABOVE_SEA_M: f32 = 1.0;

/// Removes sub-grid islands and enclosed shallow puddles that lack tectonic justification.
pub fn cleanup_coast_artifacts(
    data: &mut WorldData,
    registry: &mut PlateRegistry,
    boundaries: &BoundaryInfo,
    tick_year: i64,
) {
    let geo = data.parameters.core.geology.clone();
    let boundary_hexes: BTreeSet<HexId> = boundaries.boundary_hexes.iter().copied().collect();

    submerge_ephemeral_islands(
        data,
        registry,
        &boundary_hexes,
        tick_year,
        geo.max_ephemeral_island_hexes,
        geo.max_ephemeral_island_height_m,
        geo.max_ephemeral_island_relief_m,
    );

    fill_artifact_inland_lakes(
        data,
        registry,
        tick_year,
        geo.max_artifact_lake_hexes,
        geo.min_geologic_lake_depth_m,
    );
}

fn is_land(data: &WorldData, idx: usize) -> bool {
    data.elevation_mean[idx] > data.sea_level_m
}

fn is_ocean(data: &WorldData, idx: usize) -> bool {
    data.elevation_mean[idx] < data.sea_level_m
}

fn submerge_ephemeral_islands(
    data: &mut WorldData,
    registry: &mut PlateRegistry,
    boundary_hexes: &BTreeSet<HexId>,
    tick_year: i64,
    max_hexes: u32,
    max_height_m: f32,
    max_relief_m: f32,
) {
    let sea = data.sea_level_m;
    let grid = &data.grid;
    let n = data.cell_count() as usize;
    let mut visited = vec![false; n];

    for start in 0..n {
        if visited[start] || !is_land(data, start) {
            continue;
        }

        let component = collect_land_component(data, start, &mut visited);
        if component.len() as u32 > max_hexes {
            continue;
        }

        let mut max_elev = f32::MIN;
        let mut max_relief = f32::MIN;
        let mut on_boundary = false;
        let component_set: BTreeSet<usize> = component.iter().copied().collect();

        if !ocean_surrounded_land_component(&component_set, data, grid, n) {
            continue;
        }

        for &idx in &component {
            let hex = HexId(idx as u32);
            if boundary_hexes.contains(&hex) {
                on_boundary = true;
            }
            max_elev = max_elev.max(data.elevation_mean[idx]);
            max_relief = max_relief.max(data.elevation_relief[idx]);
        }
        if on_boundary {
            continue;
        }
        if max_elev > sea + max_height_m {
            continue;
        }
        if max_relief > max_relief_m {
            continue;
        }

        let target = sea - SUBMERGE_DEPTH_M;
        for idx in component {
            let hex = HexId(idx as u32);
            modify_surface_at_world_hex(registry, data, hex, tick_year, |feature| {
                feature.elevation_m = target;
                feature.relief_m = 0.0;
            });
        }
    }
}

fn fill_artifact_inland_lakes(
    data: &mut WorldData,
    registry: &mut PlateRegistry,
    tick_year: i64,
    max_hexes: u32,
    min_geologic_lake_depth_m: f32,
) {
    let sea = data.sea_level_m;
    let grid = &data.grid;
    let n = data.cell_count() as usize;
    let mut visited = vec![false; n];

    for start in 0..n {
        if visited[start] || !is_ocean(data, start) {
            continue;
        }

        let component = collect_ocean_component(data, start, &mut visited);
        if component.len() as u32 > max_hexes {
            continue;
        }

        let mut min_elev = f32::MAX;
        let mut fully_enclosed = true;
        let mut all_continental = true;

        for &idx in &component {
            min_elev = min_elev.min(data.elevation_mean[idx]);
            let hex = HexId(idx as u32);
            let plate_id = data.plate_id[idx];
            if plate_id != PlateId::NONE {
                if let Some(plate) = registry.get(plate_id) {
                    if plate.plate_type != PlateType::Continental {
                        all_continental = false;
                    }
                }
            }

            let mut neighbors: Vec<HexId> = grid.neighbors(hex).to_vec();
            neighbors.sort_by_key(|h| h.0);
            for neighbor in neighbors {
                let j = neighbor.0 as usize;
                if j < n && is_ocean(data, j) {
                    fully_enclosed = false;
                }
            }
        }

        if !fully_enclosed {
            continue;
        }
        if min_elev <= sea - min_geologic_lake_depth_m {
            continue;
        }
        if !all_continental && data.bedrock_type[component[0]] == BedrockType::OceanicCrust {
            continue;
        }

        let target = sea + FILL_ABOVE_SEA_M;
        for idx in component {
            let hex = HexId(idx as u32);
            modify_surface_at_world_hex(registry, data, hex, tick_year, |feature| {
                feature.elevation_m = target;
                feature.relief_m = 0.0;
            });
        }
    }
}

fn ocean_surrounded_land_component(
    component: &BTreeSet<usize>,
    data: &WorldData,
    grid: &genesis_core::HexGrid,
    n: usize,
) -> bool {
    for &idx in component {
        let hex = HexId(idx as u32);
        let mut neighbors: Vec<HexId> = grid.neighbors(hex).to_vec();
        neighbors.sort_by_key(|h| h.0);
        for neighbor in neighbors {
            let j = neighbor.0 as usize;
            if j < n && is_land(data, j) && !component.contains(&j) {
                return false;
            }
        }
    }
    true
}

fn collect_land_component(data: &WorldData, start: usize, visited: &mut [bool]) -> Vec<usize> {
    let grid = &data.grid;
    let n = visited.len();
    let mut queue = VecDeque::new();
    let mut component = Vec::new();

    queue.push_back(start);
    visited[start] = true;

    while let Some(i) = queue.pop_front() {
        component.push(i);
        let hex = HexId(i as u32);
        let mut neighbors: Vec<HexId> = grid.neighbors(hex).to_vec();
        neighbors.sort_by_key(|h| h.0);
        for neighbor in neighbors {
            let j = neighbor.0 as usize;
            if j >= n || visited[j] || !is_land(data, j) {
                continue;
            }
            visited[j] = true;
            queue.push_back(j);
        }
    }

    component
}

fn collect_ocean_component(data: &WorldData, start: usize, visited: &mut [bool]) -> Vec<usize> {
    let grid = &data.grid;
    let n = visited.len();
    let mut queue = VecDeque::new();
    let mut component = Vec::new();

    queue.push_back(start);
    visited[start] = true;

    while let Some(i) = queue.pop_front() {
        component.push(i);
        let hex = HexId(i as u32);
        let mut neighbors: Vec<HexId> = grid.neighbors(hex).to_vec();
        neighbors.sort_by_key(|h| h.0);
        for neighbor in neighbors {
            let j = neighbor.0 as usize;
            if j >= n || visited[j] || !is_ocean(data, j) {
                continue;
            }
            visited[j] = true;
            queue.push_back(j);
        }
    }

    component
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::time::WorldYear;
    use genesis_core::{PlateId, create_world};

    use crate::boundary::BoundaryInfo;
    use crate::plate::{Plate, PlateClass, PlateRegistry, PlateType};
    use crate::plate_surface::{PlateSurface, SurfaceFeature};
    use crate::world_rebuild::rebuild_world_from_plate_surfaces;

    fn test_registry(cell_count: usize) -> PlateRegistry {
        let mut registry = PlateRegistry::new();
        registry.insert(Plate {
            id: PlateId(0),
            plate_type: PlateType::Oceanic,
            plate_class: PlateClass::Major,
            seed_hex: HexId(0),
            motion_axis: [0.0, 0.0, 1.0],
            motion_rate_rad_per_year: 0.0,
            age_year: WorldYear::FORMATION,
            target_fraction: 0.5,
            accumulated_rotation_rad: 0.0,
            last_nonempty_year: WorldYear::FORMATION,
            surface: PlateSurface::new(cell_count),
        });
        registry
    }

    #[test]
    fn submerges_ocean_surrounded_low_island() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        let mut registry = test_registry(n);

        for i in 0..n {
            world.data.plate_id[i] = PlateId(0);
            world.data.elevation_mean[i] = -100.0;
            world.data.elevation_relief[i] = 0.0;
            registry
                .plates_mut()
                .get_mut(&PlateId(0))
                .unwrap()
                .surface
                .set(
                    HexId(i as u32),
                    SurfaceFeature {
                        elevation_m: -100.0,
                        relief_m: 0.0,
                        bedrock: BedrockType::OceanicCrust,
                        fertility: 0.0,
                        age_year: 0,
                    },
                );
        }

        let center = HexId(10);
        let center_idx = center.0 as usize;
        world.data.elevation_mean[center_idx] = 15.0;
        world.data.elevation_relief[center_idx] = 0.0;
        registry
            .plates_mut()
            .get_mut(&PlateId(0))
            .unwrap()
            .surface
            .set(
                center,
                SurfaceFeature {
                    elevation_m: 15.0,
                    relief_m: 0.0,
                    bedrock: BedrockType::Igneous,
                    fertility: 0.0,
                    age_year: 0,
                },
            );

        world.data.sea_level_m = 0.0;
        cleanup_coast_artifacts(&mut world.data, &mut registry, &BoundaryInfo::default(), 0);
        rebuild_world_from_plate_surfaces(&mut world.data, &registry);

        assert!(
            world.data.elevation_mean[center_idx] < 0.0,
            "ephemeral island should be submerged"
        );
    }

    #[test]
    fn keeps_island_with_high_relief() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        let mut registry = test_registry(n);

        for i in 0..n {
            world.data.plate_id[i] = PlateId(0);
            world.data.elevation_mean[i] = -100.0;
            registry
                .plates_mut()
                .get_mut(&PlateId(0))
                .unwrap()
                .surface
                .set(
                    HexId(i as u32),
                    SurfaceFeature {
                        elevation_m: -100.0,
                        relief_m: 0.0,
                        bedrock: BedrockType::OceanicCrust,
                        fertility: 0.0,
                        age_year: 0,
                    },
                );
        }

        let center = HexId(10);
        let center_idx = center.0 as usize;
        world.data.elevation_mean[center_idx] = 15.0;
        world.data.elevation_relief[center_idx] = 500.0;
        registry
            .plates_mut()
            .get_mut(&PlateId(0))
            .unwrap()
            .surface
            .set(
                center,
                SurfaceFeature {
                    elevation_m: 15.0,
                    relief_m: 500.0,
                    bedrock: BedrockType::Igneous,
                    fertility: 0.0,
                    age_year: 0,
                },
            );

        world.data.sea_level_m = 0.0;
        cleanup_coast_artifacts(&mut world.data, &mut registry, &BoundaryInfo::default(), 0);
        rebuild_world_from_plate_surfaces(&mut world.data, &registry);

        assert!(
            world.data.elevation_mean[center_idx] > 0.0,
            "volcanic relief island should remain"
        );
    }

    #[test]
    fn fills_enclosed_shallow_pocket_on_continental_plate() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        let mut registry = PlateRegistry::new();
        registry.insert(Plate {
            id: PlateId(0),
            plate_type: PlateType::Continental,
            plate_class: PlateClass::Major,
            seed_hex: HexId(0),
            motion_axis: [0.0, 0.0, 1.0],
            motion_rate_rad_per_year: 0.0,
            age_year: WorldYear::FORMATION,
            target_fraction: 0.5,
            accumulated_rotation_rad: 0.0,
            last_nonempty_year: WorldYear::FORMATION,
            surface: PlateSurface::new(n),
        });

        for i in 0..n {
            world.data.plate_id[i] = PlateId(0);
            world.data.elevation_mean[i] = 500.0;
            registry
                .plates_mut()
                .get_mut(&PlateId(0))
                .unwrap()
                .surface
                .set(
                    HexId(i as u32),
                    SurfaceFeature {
                        elevation_m: 500.0,
                        relief_m: 0.0,
                        bedrock: BedrockType::Igneous,
                        fertility: 0.0,
                        age_year: 0,
                    },
                );
        }

        let pocket = HexId(20);
        let pocket_idx = pocket.0 as usize;
        world.data.elevation_mean[pocket_idx] = -50.0;
        registry
            .plates_mut()
            .get_mut(&PlateId(0))
            .unwrap()
            .surface
            .set(
                pocket,
                SurfaceFeature {
                    elevation_m: -50.0,
                    relief_m: 0.0,
                    bedrock: BedrockType::Igneous,
                    fertility: 0.0,
                    age_year: 0,
                },
            );

        world.data.sea_level_m = 0.0;
        cleanup_coast_artifacts(&mut world.data, &mut registry, &BoundaryInfo::default(), 0);
        rebuild_world_from_plate_surfaces(&mut world.data, &registry);

        assert!(
            world.data.elevation_mean[pocket_idx] > 0.0,
            "shallow enclosed pocket should be filled"
        );
    }

    #[test]
    fn keeps_deep_enclosed_lake() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        let mut registry = PlateRegistry::new();
        registry.insert(Plate {
            id: PlateId(0),
            plate_type: PlateType::Continental,
            plate_class: PlateClass::Major,
            seed_hex: HexId(0),
            motion_axis: [0.0, 0.0, 1.0],
            motion_rate_rad_per_year: 0.0,
            age_year: WorldYear::FORMATION,
            target_fraction: 0.5,
            accumulated_rotation_rad: 0.0,
            last_nonempty_year: WorldYear::FORMATION,
            surface: PlateSurface::new(n),
        });

        for i in 0..n {
            world.data.plate_id[i] = PlateId(0);
            world.data.elevation_mean[i] = 500.0;
            registry
                .plates_mut()
                .get_mut(&PlateId(0))
                .unwrap()
                .surface
                .set(
                    HexId(i as u32),
                    SurfaceFeature {
                        elevation_m: 500.0,
                        relief_m: 0.0,
                        bedrock: BedrockType::Igneous,
                        fertility: 0.0,
                        age_year: 0,
                    },
                );
        }

        let pocket = HexId(20);
        let pocket_idx = pocket.0 as usize;
        world.data.elevation_mean[pocket_idx] = -800.0;
        registry
            .plates_mut()
            .get_mut(&PlateId(0))
            .unwrap()
            .surface
            .set(
                pocket,
                SurfaceFeature {
                    elevation_m: -800.0,
                    relief_m: 0.0,
                    bedrock: BedrockType::Igneous,
                    fertility: 0.0,
                    age_year: 0,
                },
            );

        world.data.sea_level_m = 0.0;
        cleanup_coast_artifacts(&mut world.data, &mut registry, &BoundaryInfo::default(), 0);
        rebuild_world_from_plate_surfaces(&mut world.data, &registry);

        assert!(
            world.data.elevation_mean[pocket_idx] < 0.0,
            "deep enclosed lake should remain"
        );
    }
}
