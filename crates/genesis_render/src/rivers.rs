//! River polyline overlay with zoom LOD (Doc 08 §12.2–§12.3).

use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use genesis_core::HexId;
use genesis_core::data::{HydroFlags, RiverClass, river_class};

use crate::polygon::unwrap_lon_relative;
use crate::projection::{project, should_skip_for_equirectangular};
use crate::resources::{
    CameraState, ColorsDirty, HexEntityCache, RiversDirty, WorldDirty, WorldResource,
};

/// Zoom ≥ this shows Stream-class rivers (regional).
pub const ZOOM_REGIONAL: f32 = 2.0;
/// Zoom ≥ this shows Creek-class channels (local; still hex-scale, not Doc 14).
pub const ZOOM_LOCAL: f32 = 8.0;

#[derive(Component)]
pub(crate) struct RiverOverlay;

fn min_class_for_zoom(zoom: f32) -> RiverClass {
    if zoom >= ZOOM_LOCAL {
        RiverClass::Creek
    } else if zoom >= ZOOM_REGIONAL {
        RiverClass::Stream
    } else {
        RiverClass::River
    }
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

/// Rebuilds river line meshes when the world, colors (timeline scrub), or
/// zoom LOD band changes.
#[allow(clippy::too_many_arguments)]
pub fn update_river_overlay(
    mut commands: Commands,
    world_res: Option<Res<WorldResource>>,
    camera: Res<CameraState>,
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
    if !world_dirty.0 && !colors_dirty.0 && !rivers_dirty.dirty && !lod_changed {
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

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for i in 0..n {
        let class = river_class(f64::from(data.river_discharge_m3_yr[i]));
        if class < lod {
            continue;
        }
        let Some(dir) = data.flow_direction[i] else {
            continue;
        };
        let hex = HexId(i as u32);
        let (clat, clon) = grid.center_lat_lon(hex);
        if should_skip_for_equirectangular(clat) {
            continue;
        }
        let Some(&target) = grid.neighbors(hex).get(dir.index()) else {
            continue;
        };
        let (tlat, tlon) = grid.center_lat_lon(target);
        let (x0, y0) = project(clat, clon);
        let unwrapped = unwrap_lon_relative(tlon, clon);
        let (x1, y1) = project(tlat, unwrapped);

        let ephemeral = data.hydro_flags[i].contains(HydroFlags::EPHEMERAL);
        let rgba = class_color(class, ephemeral);
        let width = class_width(class);

        // Quad strip along the segment (two triangles) for visible width.
        let dx = x1 - x0;
        let dy = y1 - y0;
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        let nx = -dy / len * width;
        let ny = dx / len * width;

        let segments = if ephemeral { 3_usize } else { 1 };
        for s in 0..segments {
            // Ephemeral: dashed — draw odd thirds only.
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
