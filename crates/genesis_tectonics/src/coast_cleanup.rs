//! Removes geologically unjustified coast artifacts (Doc 06 coast cleanup).
//!
//! Runs after [`crate::world_rebuild::rebuild_world_from_plate_surfaces`] each
//! Geological tick. Mutates plate surfaces via [`crate::plate_surface::modify_surface_at_world_hex`].

use std::collections::{BTreeSet, VecDeque};

use genesis_core::data::{BedrockType, WATER_NONE, WorldData};
use genesis_core::{HexGrid, HexId, PlateId};

use crate::boundary::BoundaryInfo;
use crate::plate::{PlateRegistry, PlateType};
use crate::plate_surface::modify_surface_at_world_hex;
use crate::projection::ProjectionCache;

/// Depth below sea level applied when submerging ephemeral islands (m).
pub const SUBMERGE_DEPTH_M: f32 = 10.0;

/// Elevation above sea level when filling artifact inland puddles (m).
pub const FILL_ABOVE_SEA_M: f32 = 1.0;

/// Largest ocean-surrounded land component the display de-speckle submerges.
/// Real islands are multi-hex clusters; the salt-and-pepper is 1–3 hex spray.
/// Display-only, so removing spray costs no simulated land fraction.
pub const DESPECKLE_OPEN_MAX_HEXES: usize = 3;
/// Largest land-surrounded ocean pocket the display de-speckle fills.
pub const DESPECKLE_CLOSE_MAX_HEXES: usize = 3;
/// A land-surrounded ocean pocket deeper than this (below sea) is kept as a
/// real basin; shallower multi-hex pockets are filled. Single-hex pockets are
/// filled regardless of depth (a lone below-sea hex ringed by land is a
/// projection/incision artifact, not a basin).
pub const DESPECKLE_CLOSE_MAX_DEPTH_M: f32 = 300.0;

/// **Display-only** morphological open/close on the land/ocean mask.
///
/// The salt-and-pepper single-hex islands and pockets are **adopted
/// projection-hole hexes**: a plate owns the world hex by BFS adoption but
/// stores no birth feature there, so the surface-modify cleanup
/// ([`submerge_ephemeral_islands`]) is a no-op on them. This pass edits an
/// elevation buffer directly: it submerges ocean-surrounded land components of
/// `<= DESPECKLE_OPEN_MAX_HEXES` and fills small land-surrounded ocean pockets,
/// leaving multi-hex islands and deep basins.
///
/// It is applied ONLY to display copies (headless render buffer, history
/// frames), never to the live simulation `WorldData`: the deep-time land
/// fraction sits close to the §11 gate floor and is chaotically sensitive, so
/// perturbing the simulated elevation each tick tipped it below the gate. The
/// simulation is left untouched; only what the user sees is de-speckled.
///
/// Updates `elevation` and `water_level` together so the render stays
/// consistent: a submerged island joins the ocean (blue), a filled pocket
/// becomes dry land. Deterministic: `neighbors_sorted` BFS, ascending `HexId`,
/// both passes evaluated on the entry snapshot so nothing cascades.
pub fn despeckle_display(elevation: &mut [f32], water_level: &mut [f32], grid: &HexGrid, sea: f32) {
    let n = elevation.len();
    let snap: Vec<f32> = elevation.to_vec();
    let is_land = |e: f32| e > sea;
    let is_ocean = |e: f32| e < sea;

    // Opening: submerge isolated small land components. A connected land
    // component of size <= K is, by construction, ringed by non-land (any land
    // neighbor would be in the component), i.e. an ocean-surrounded island.
    let mut visited = vec![false; n];
    for start in 0..n {
        if visited[start] || !is_land(snap[start]) {
            continue;
        }
        let mut queue = VecDeque::from([start]);
        visited[start] = true;
        let mut cells = vec![start];
        while let Some(i) = queue.pop_front() {
            for &nb in grid.neighbors_sorted(HexId(i as u32)) {
                let j = nb.0 as usize;
                if j < n && !visited[j] && is_land(snap[j]) {
                    visited[j] = true;
                    queue.push_back(j);
                    cells.push(j);
                }
            }
        }
        if cells.len() <= DESPECKLE_OPEN_MAX_HEXES {
            for &c in &cells {
                elevation[c] = sea - SUBMERGE_DEPTH_M;
                water_level[c] = sea; // joins the ocean → renders as water
            }
        }
    }

    // Closing: fill isolated small ocean pockets ringed by land.
    let mut ovisited = vec![false; n];
    for start in 0..n {
        if ovisited[start] || !is_ocean(snap[start]) {
            continue;
        }
        let mut queue = VecDeque::from([start]);
        ovisited[start] = true;
        let mut cells = vec![start];
        let mut min_elev = snap[start];
        while let Some(i) = queue.pop_front() {
            for &nb in grid.neighbors_sorted(HexId(i as u32)) {
                let j = nb.0 as usize;
                if j < n && !ovisited[j] && is_ocean(snap[j]) {
                    ovisited[j] = true;
                    queue.push_back(j);
                    cells.push(j);
                    min_elev = min_elev.min(snap[j]);
                }
            }
        }
        let shallow = min_elev > sea - DESPECKLE_CLOSE_MAX_DEPTH_M;
        if cells.len() <= DESPECKLE_CLOSE_MAX_HEXES && (cells.len() == 1 || shallow) {
            for &c in &cells {
                elevation[c] = sea + FILL_ABOVE_SEA_M;
                water_level[c] = WATER_NONE; // becomes dry land
            }
        }
    }
}

/// Removes sub-grid islands and enclosed shallow puddles that lack tectonic justification.
pub fn cleanup_coast_artifacts(
    data: &mut WorldData,
    registry: &mut PlateRegistry,
    cache: &ProjectionCache,
    boundaries: &BoundaryInfo,
    tick_year: i64,
) {
    let geo = data.parameters.core.geology.clone();
    let boundary_hexes: BTreeSet<HexId> = boundaries.boundary_hexes.iter().copied().collect();

    submerge_ephemeral_islands(
        data,
        registry,
        cache,
        &boundary_hexes,
        tick_year,
        geo.max_ephemeral_island_hexes,
        geo.max_ephemeral_island_height_m,
        geo.max_ephemeral_island_relief_m,
    );

    fill_artifact_inland_lakes(
        data,
        registry,
        cache,
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

#[allow(clippy::too_many_arguments)]
fn submerge_ephemeral_islands(
    data: &mut WorldData,
    registry: &mut PlateRegistry,
    cache: &ProjectionCache,
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
        // Tiny ocean-surrounded speckles are always artifacts, even on a
        // plate boundary (island-arc noise). Larger candidates still respect
        // the boundary guard so real arcs survive.
        let tiny = component.len() as u32 <= max_hexes / 2;
        if on_boundary && !tiny {
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
            modify_surface_at_world_hex(registry, data, cache, hex, tick_year, |feature| {
                feature.elevation_m = target;
                feature.relief_m = 0.0;
            });
        }
    }
}

fn fill_artifact_inland_lakes(
    data: &mut WorldData,
    registry: &mut PlateRegistry,
    cache: &ProjectionCache,
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
            if plate_id != PlateId::NONE
                && let Some(plate) = registry.get(plate_id)
                && plate.plate_type != PlateType::Continental
            {
                all_continental = false;
            }

            let neighbors = grid.neighbors_sorted(hex);
            for &neighbor in neighbors {
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
            modify_surface_at_world_hex(registry, data, cache, hex, tick_year, |feature| {
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
        let neighbors = grid.neighbors_sorted(hex);
        for &neighbor in neighbors {
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
        let neighbors = grid.neighbors_sorted(hex);
        for &neighbor in neighbors {
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
        let neighbors = grid.neighbors_sorted(hex);
        for &neighbor in neighbors {
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
            forward_world_hint: Vec::new(),
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
                        continental_crust: false,
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
                    continental_crust: false,
                },
            );

        world.data.sea_level_m = 0.0;
        cleanup_coast_artifacts(
            &mut world.data,
            &mut registry,
            &ProjectionCache::empty(),
            &BoundaryInfo::default(),
            0,
        );
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
                        continental_crust: false,
                    },
                );
        }

        let center = HexId(10);
        let center_idx = center.0 as usize;
        world.data.elevation_mean[center_idx] = 15.0;
        world.data.elevation_relief[center_idx] = 900.0;
        registry
            .plates_mut()
            .get_mut(&PlateId(0))
            .unwrap()
            .surface
            .set(
                center,
                SurfaceFeature {
                    elevation_m: 15.0,
                    relief_m: 900.0,
                    bedrock: BedrockType::Igneous,
                    fertility: 0.0,
                    age_year: 0,
                    continental_crust: false,
                },
            );

        world.data.sea_level_m = 0.0;
        cleanup_coast_artifacts(
            &mut world.data,
            &mut registry,
            &ProjectionCache::empty(),
            &BoundaryInfo::default(),
            0,
        );
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
            forward_world_hint: Vec::new(),
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
                        continental_crust: false,
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
                    continental_crust: false,
                },
            );

        world.data.sea_level_m = 0.0;
        cleanup_coast_artifacts(
            &mut world.data,
            &mut registry,
            &ProjectionCache::empty(),
            &BoundaryInfo::default(),
            0,
        );
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
            forward_world_hint: Vec::new(),
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
                        continental_crust: false,
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
                    continental_crust: false,
                },
            );

        world.data.sea_level_m = 0.0;
        cleanup_coast_artifacts(
            &mut world.data,
            &mut registry,
            &ProjectionCache::empty(),
            &BoundaryInfo::default(),
            0,
        );
        rebuild_world_from_plate_surfaces(&mut world.data, &registry);

        assert!(
            world.data.elevation_mean[pocket_idx] < 0.0,
            "deep enclosed lake should remain"
        );
    }

    #[test]
    fn despeckle_display_removes_single_hex_island_and_pocket_and_keeps_continent() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let world = create_world(params).expect("world");
        let grid = &world.data.grid;
        let n = world.data.cell_count() as usize;
        let sea = 0.0_f32;

        // An all-ocean world with one lone land island.
        let mut elev = vec![-1000.0_f32; n];
        let mut water = vec![sea; n];
        let island = 100usize;
        elev[island] = 800.0;
        water[island] = WATER_NONE;

        despeckle_display(&mut elev, &mut water, grid, sea);
        assert!(elev[island] < sea, "lone island must be submerged");
        assert!(
            water[island].is_finite() && water[island] >= sea,
            "submerged island must join the ocean (render as water)"
        );

        // An all-land world with one lone ocean pocket.
        let mut elev = vec![800.0_f32; n];
        let mut water = vec![WATER_NONE; n];
        let pocket = 100usize;
        elev[pocket] = -50.0;
        water[pocket] = sea;

        despeckle_display(&mut elev, &mut water, grid, sea);
        assert!(elev[pocket] > sea, "lone pocket must be filled to land");
        assert!(
            !(water[pocket].is_finite() && water[pocket] > elev[pocket]),
            "filled pocket must render as dry land"
        );

        // A large contiguous continent is untouched.
        let mut elev = vec![-1000.0_f32; n];
        let mut water = vec![sea; n];
        for (i, e) in elev.iter_mut().enumerate() {
            if i < n / 2 {
                *e = 800.0;
                water[i] = WATER_NONE;
            }
        }
        let before = elev.clone();
        despeckle_display(&mut elev, &mut water, grid, sea);
        // Interior continent hexes (far from any coast) keep their elevation.
        let interior = 0usize;
        assert_eq!(
            elev[interior], before[interior],
            "solid continent interior must be untouched by de-speckle"
        );
    }
}
