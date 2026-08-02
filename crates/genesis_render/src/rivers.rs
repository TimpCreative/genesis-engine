//! River polyline overlay with zoom LOD (Doc 08 §12.2–§12.3).
//!
//! World view draws Major trunks only, skips ice, drops short components
//! (< [`WORLD_MIN_RIVER_HEXES`]), and keeps the top [`WORLD_MAX_RIVER_COMPONENTS`]
//! by peak discharge. Regional/local zoom still reveal Stream/Creek per §12.3;
//! ice is always skipped.

use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use genesis_core::HexId;
use genesis_core::data::{HydroFlags, RiverClass, WorldData, river_class};

use glam::DVec3;

use crate::projection::MapProjection;
use crate::resources::{
    CameraState, ColorsDirty, CurrentProjection, HexEntityCache, RiversDirty, WorldDirty,
    WorldResource,
};
use crate::systems::view_center;

/// Zoom ≥ this shows Stream-class rivers (regional).
pub const ZOOM_REGIONAL: f32 = 2.0;
/// Zoom ≥ this shows Creek-class channels (local; still hex-scale, not Doc 14).
pub const ZOOM_LOCAL: f32 = 8.0;
/// Minimum connected Major hexes to draw at world zoom.
pub const WORLD_MIN_RIVER_HEXES: usize = 5;
/// Maximum river components drawn at world zoom (longest/peak discharge first).
pub const WORLD_MAX_RIVER_COMPONENTS: usize = 10;

#[derive(Component)]
pub(crate) struct RiverOverlay;

fn min_class_for_zoom(zoom: f32) -> RiverClass {
    if zoom >= ZOOM_LOCAL {
        RiverClass::Creek
    } else if zoom >= ZOOM_REGIONAL {
        RiverClass::Stream
    } else {
        // World view prefers Major; select_river_hexes may fall back to River.
        RiverClass::Major
    }
}

/// World-view selection: Major trunks first; if none survive filters, fall back
/// to River-class with the same length / top-N caps so late deep time still
/// shows a few channels when discharge sits below Major.
pub fn select_river_hexes(data: &WorldData, lod: RiverClass) -> Vec<bool> {
    if lod == RiverClass::Major {
        let major = select_river_hexes_inner(data, RiverClass::Major, true);
        if major.iter().any(|&d| d) {
            return major;
        }
        return select_river_hexes_inner(data, RiverClass::River, true);
    }
    select_river_hexes_inner(data, lod, false)
}

fn class_width(class: RiverClass) -> f32 {
    match class {
        RiverClass::Major => 0.004,
        RiverClass::River => 0.0025,
        RiverClass::Stream => 0.0015,
        RiverClass::Creek => 0.0008,
    }
}

fn class_color(class: RiverClass, ephemeral: bool) -> [f32; 4] {
    let (r, g, b, a) = match class {
        RiverClass::Major => (0.15, 0.35, 0.75, 0.95),
        RiverClass::River => (0.20, 0.45, 0.80, 0.85),
        RiverClass::Stream => (0.30, 0.55, 0.85, 0.70),
        RiverClass::Creek => (0.40, 0.65, 0.90, 0.55),
    };
    if ephemeral {
        [r, g, b, a * 0.45]
    } else {
        [r, g, b, a]
    }
}

fn is_ice_hex(data: &WorldData, i: usize) -> bool {
    data.ice_mask.get(i).copied().unwrap_or(false)
        || data
            .hydro_flags
            .get(i)
            .copied()
            .unwrap_or(HydroFlags::NONE)
            .contains(HydroFlags::SEA_ICE)
}

/// Candidate channel hex: meets LOD class, has flow, not ice.
fn is_candidate(data: &WorldData, i: usize, lod: RiverClass) -> bool {
    if is_ice_hex(data, i) {
        return false;
    }
    if data.flow_direction.get(i).copied().flatten().is_none() {
        return false;
    }
    river_class(f64::from(data.river_discharge_m3_yr[i])) >= lod
}

struct ComponentMeta {
    root: usize,
    size: usize,
    peak_discharge: f32,
    min_hex: u32,
}

fn select_river_hexes_inner(
    data: &WorldData,
    lod: RiverClass,
    apply_world_caps: bool,
) -> Vec<bool> {
    let n = data.cell_count() as usize;
    let mut parent: Vec<usize> = (0..n).collect();
    let mut rank = vec![0_u8; n];

    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    fn unite(parent: &mut [usize], rank: &mut [u8], a: usize, b: usize) {
        let mut ra = find(parent, a);
        let mut rb = find(parent, b);
        if ra == rb {
            return;
        }
        if rank[ra] < rank[rb] {
            std::mem::swap(&mut ra, &mut rb);
        }
        parent[rb] = ra;
        if rank[ra] == rank[rb] {
            rank[ra] += 1;
        }
    }

    let mut candidates = vec![false; n];
    #[allow(clippy::needless_range_loop)]
    for i in 0..n {
        candidates[i] = is_candidate(data, i, lod);
    }

    #[allow(clippy::needless_range_loop)]
    for i in 0..n {
        if !candidates[i] {
            continue;
        }
        let Some(dir) = data.flow_direction[i] else {
            continue;
        };
        let hex = HexId(i as u32);
        let Some(&target) = data.grid.neighbors(hex).get(dir.index()) else {
            continue;
        };
        let j = target.0 as usize;
        if j < n && candidates[j] {
            unite(&mut parent, &mut rank, i, j);
        }
    }

    let mut meta: Vec<Option<ComponentMeta>> = (0..n).map(|_| None).collect();
    #[allow(clippy::needless_range_loop)]
    for i in 0..n {
        if !candidates[i] {
            continue;
        }
        let root = find(&mut parent, i);
        let discharge = data.river_discharge_m3_yr[i];
        match &mut meta[root] {
            Some(m) => {
                m.size += 1;
                m.peak_discharge = m.peak_discharge.max(discharge);
                m.min_hex = m.min_hex.min(i as u32);
            }
            None => {
                meta[root] = Some(ComponentMeta {
                    root,
                    size: 1,
                    peak_discharge: discharge,
                    min_hex: i as u32,
                });
            }
        }
    }

    let mut kept_roots = vec![false; n];
    if apply_world_caps {
        let mut comps: Vec<ComponentMeta> = meta.into_iter().flatten().collect();
        comps.retain(|c| c.size >= WORLD_MIN_RIVER_HEXES);
        comps.sort_by(|a, b| {
            b.peak_discharge
                .total_cmp(&a.peak_discharge)
                .then_with(|| a.min_hex.cmp(&b.min_hex))
        });
        for c in comps.into_iter().take(WORLD_MAX_RIVER_COMPONENTS) {
            kept_roots[c.root] = true;
        }
    } else {
        for m in meta.into_iter().flatten() {
            kept_roots[m.root] = true;
        }
    }

    let mut draw = vec![false; n];
    #[allow(clippy::needless_range_loop)]
    for i in 0..n {
        if !candidates[i] {
            continue;
        }
        let root = find(&mut parent, i);
        draw[i] = kept_roots[root];
    }
    draw
}

/// Rebuilds river line meshes when the world, colors (timeline scrub), or
/// zoom LOD band changes.
#[allow(clippy::too_many_arguments)]
pub fn update_river_overlay(
    mut commands: Commands,
    world_res: Option<Res<WorldResource>>,
    camera: Res<CameraState>,
    projection_mode: Res<CurrentProjection>,
    world_dirty: Res<WorldDirty>,
    colors_dirty: Res<ColorsDirty>,
    mut rivers_dirty: ResMut<RiversDirty>,
    mut cache: ResMut<HexEntityCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    overlay_q: Query<Entity, With<RiverOverlay>>,
) {
    let lod = min_class_for_zoom(camera.zoom);
    let lod_changed = rivers_dirty.last_lod != Some(lod);
    let projection = projection_mode.0;
    // On the globe, rivers reproject as it rotates (pan changes CameraState), so
    // rebuild on camera change too. The flat map is world-fixed and never needs a
    // pan-driven rebuild (zoom is covered by `lod_changed`).
    let globe_rotated = projection == MapProjection::Orthographic && camera.is_changed();
    if !world_dirty.0 && !colors_dirty.0 && !rivers_dirty.dirty && !lod_changed && !globe_rotated {
        return;
    }
    let Some(world_res) = world_res else {
        return;
    };

    for entity in overlay_q.iter() {
        commands.entity(entity).despawn();
        cache.entities.retain(|&e| e != entity);
    }

    let data = &world_res.0.data;
    let grid = &data.grid;
    let n = data.cell_count() as usize;
    let draw = select_river_hexes(data, lod);
    let view = view_center(&camera);

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    #[allow(clippy::needless_range_loop)]
    for i in 0..n {
        if !draw[i] {
            continue;
        }
        let class = river_class(f64::from(data.river_discharge_m3_yr[i]));
        let Some(dir) = data.flow_direction[i] else {
            continue;
        };
        let hex = HexId(i as u32);
        let center_dir = DVec3::from(grid.cell_center_direction(hex));
        if !projection.hex_visible(center_dir, view) {
            continue; // polar-skipped (flat) or far hemisphere (globe)
        }
        let Some(&target) = grid.neighbors(hex).get(dir.index()) else {
            continue;
        };
        let target_dir = DVec3::from(grid.cell_center_direction(target));
        // Drop segments whose downstream end is off-map, so a river never draws a
        // stray chord across the globe's limb or the flat map's antimeridian gap.
        if !projection.hex_visible(target_dir, view) {
            continue;
        }
        // Equirectangular unwraps the target's longitude relative to the source
        // hex's center; the globe ignores the reference and uses the view.
        let (x0, y0) = projection.project(center_dir, center_dir, view);
        let (x1, y1) = projection.project(target_dir, center_dir, view);

        let ephemeral = data.hydro_flags[i].contains(HydroFlags::EPHEMERAL);
        let rgba = class_color(class, ephemeral);
        let width = class_width(class);

        let dx = x1 - x0;
        let dy = y1 - y0;
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        let nx = -dy / len * width;
        let ny = dx / len * width;

        let segments = if ephemeral { 3_usize } else { 1 };
        for s in 0..segments {
            if ephemeral && s % 2 == 1 {
                continue;
            }
            let t0 = s as f32 / segments as f32;
            let t1 = (s + 1) as f32 / segments as f32;
            let ax = x0 + dx * t0;
            let ay = y0 + dy * t0;
            let bx = x0 + dx * t1;
            let by = y0 + dy * t1;
            let base = positions.len() as u32;
            positions.push([ax + nx, ay + ny, 0.1]);
            positions.push([ax - nx, ay - ny, 0.1]);
            positions.push([bx + nx, by + ny, 0.1]);
            positions.push([bx - nx, by - ny, 0.1]);
            for _ in 0..4 {
                colors.push(rgba);
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
        }
    }

    if !positions.is_empty() {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        mesh.insert_indices(Indices::U32(indices));
        let handle = meshes.add(mesh);
        let mat = materials.add(ColorMaterial::default());
        let entity = commands
            .spawn((
                RiverOverlay,
                Mesh2d(handle),
                MeshMaterial2d(mat),
                Transform::from_xyz(0.0, 0.0, 0.1),
            ))
            .id();
        cache.entities.push(entity);
    }

    rivers_dirty.dirty = false;
    rivers_dirty.last_lod = Some(lod);
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::data::MAJOR_CLASS_MIN_M3_YR;
    use genesis_core::grid::Direction;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{HexGrid, create_world};

    fn chain_world() -> genesis_core::World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        world.data.river_discharge_m3_yr = vec![0.0; n];
        world.data.flow_direction = vec![None; n];
        world.data.ice_mask = vec![false; n];
        world.data.hydro_flags = vec![HydroFlags::NONE; n];
        world
    }

    /// Build a simple path of `len` hexes along neighbor links starting at `start`.
    fn paint_path(data: &mut WorldData, start: u32, len: usize, discharge: f32) -> Vec<u32> {
        let mut path = vec![start];
        let mut current = start;
        for _ in 1..len {
            let neighbors = data.grid.neighbors_sorted(HexId(current));
            let next = neighbors
                .iter()
                .copied()
                .find(|nb| !path.contains(&nb.0))
                .expect("path can extend")
                .0;
            let dir_idx = data
                .grid
                .neighbors(HexId(current))
                .iter()
                .position(|nb| nb.0 == next)
                .expect("neighbor slot");
            data.flow_direction[current as usize] = Direction::from_index(dir_idx);
            data.river_discharge_m3_yr[current as usize] = discharge;
            path.push(next);
            current = next;
        }
        // Last hex: no outbound needed for membership if previous points to it;
        // still mark discharge so it counts when united from upstream.
        data.river_discharge_m3_yr[current as usize] = discharge;
        // Give last a self-ish flow to a neighbor so it is a candidate.
        if let Some(dir) = data
            .grid
            .neighbors(HexId(current))
            .iter()
            .position(|nb| path.contains(&nb.0))
        {
            data.flow_direction[current as usize] = Direction::from_index(dir);
        }
        path
    }

    #[test]
    fn world_lod_drops_short_major_and_caps_count() {
        let mut world = chain_world();
        let major = MAJOR_CLASS_MIN_M3_YR as f32;
        // Three short stubs (length 3) and two long trunks (length 8).
        let _ = paint_path(&mut world.data, 10, 3, major);
        let _ = paint_path(&mut world.data, 200, 3, major * 1.1);
        let _ = paint_path(&mut world.data, 400, 3, major * 1.2);
        let long_a = paint_path(&mut world.data, 600, 8, major * 2.0);
        let long_b = paint_path(&mut world.data, 800, 8, major * 3.0);

        let draw = select_river_hexes(&world.data, RiverClass::Major);
        let drawn: Vec<u32> = draw
            .iter()
            .enumerate()
            .filter_map(|(i, &d)| d.then_some(i as u32))
            .collect();
        assert!(
            drawn.iter().any(|h| long_a.contains(h)),
            "long trunk A should draw"
        );
        assert!(
            drawn.iter().any(|h| long_b.contains(h)),
            "long trunk B should draw"
        );
        // Short stubs must not appear.
        assert!(
            drawn.len() >= WORLD_MIN_RIVER_HEXES,
            "at least one long component"
        );
        assert!(
            drawn.len() <= WORLD_MAX_RIVER_COMPONENTS * 12,
            "should not draw every major scratch"
        );
        // Explicitly: hexes only in short paths should be false if we can isolate.
        // Count components via draw size relative to long paths only.
        let only_long = drawn
            .iter()
            .filter(|h| long_a.contains(h) || long_b.contains(h))
            .count();
        assert_eq!(drawn.len(), only_long, "short Majors must be filtered out");
    }

    #[test]
    fn ice_masked_major_not_drawn() {
        let mut world = chain_world();
        let major = MAJOR_CLASS_MIN_M3_YR as f32;
        let path = paint_path(&mut world.data, 50, 8, major);
        for &h in &path {
            world.data.ice_mask[h as usize] = true;
        }
        let draw = select_river_hexes(&world.data, RiverClass::Major);
        assert!(draw.iter().all(|&d| !d), "ice Major path must not draw");
    }

    #[test]
    fn regional_lod_keeps_short_streams() {
        let mut world = chain_world();
        // Stream-class short path — world would drop it; regional should keep.
        let stream_discharge = 1.0e9_f32;
        let path = paint_path(&mut world.data, 30, 3, stream_discharge);
        let draw = select_river_hexes(&world.data, RiverClass::Stream);
        assert!(
            path.iter().any(|&h| draw[h as usize]),
            "regional Stream LOD must keep short channels"
        );
    }

    #[allow(dead_code)]
    fn _grid_touch(g: &HexGrid) {
        let _ = g.cell_count();
    }
}
