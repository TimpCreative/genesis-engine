//! Bevy systems for camera, input, and hex mesh rendering.

use std::f64::consts::PI;

use bevy::camera::{OrthographicProjection, ScalingMode};
use bevy::input::mouse::{AccumulatedMouseMotion, MouseWheel};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use genesis_core::HexId;

use crate::color::hex_color_for_mode;
use crate::polygon::{direction_to_lat_lon, hex_polygon_vertices, unwrap_lon_relative};
use crate::projection::{project, should_skip_for_equirectangular};
use crate::render_mode::CurrentRenderMode;
use crate::resources::{
    ActiveBiologyView, CameraState, ColorsDirty, HexChunk, HexEntityCache, HexMeshIndex, WorldDirty,
    WorldResource,
};

const MIN_ZOOM: f32 = 0.1;
const MAX_ZOOM: f32 = 50.0;
const ZOOM_SENSITIVITY: f32 = 0.1;

/// World width/height in projected radians (equirectangular).
const WORLD_WIDTH: f32 = (2.0 * PI) as f32;
const WORLD_HEIGHT: f32 = PI as f32;
const WORLD_ASPECT: f32 = WORLD_WIDTH / WORLD_HEIGHT;

/// 2D camera sits above meshes at z = 0 (Bevy looks down -Z).
const CAMERA_Z: f32 = 999.0;

#[derive(Component)]
pub(crate) struct MainCamera;

/// Visible world size in radians for the current window aspect and zoom.
///
/// At `zoom = 1.0` the **whole** map is visible (contain, not fill): the
/// limiting axis fits exactly and the other gets margin, so the map's edges are
/// always on screen regardless of window aspect. Zoom > 1 crops in.
pub(crate) fn viewport_world_size(window_aspect: f32, zoom: f32) -> (f32, f32) {
    if window_aspect > WORLD_ASPECT {
        // Window wider than the map: height is the limiting axis.
        let height = WORLD_HEIGHT / zoom;
        (height * window_aspect, height)
    } else {
        // Window narrower than the map: width is the limiting axis.
        let width = WORLD_WIDTH / zoom;
        (width, width / window_aspect)
    }
}

pub fn setup_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        MainCamera,
        Transform::from_xyz(0.0, 0.0, CAMERA_Z),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: WORLD_HEIGHT,
            },
            scale: 1.0,
            ..OrthographicProjection::default_2d()
        }),
    ));
}

pub fn sync_camera(
    camera_state: Res<CameraState>,
    mut camera_query: Query<(&mut Transform, &mut Projection), With<MainCamera>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
) {
    let Ok((mut transform, mut projection)) = camera_query.single_mut() else {
        return;
    };

    let (cx, cy) = project(camera_state.center_lat_rad, camera_state.center_lon_rad);
    transform.translation = Vec3::new(cx, cy, CAMERA_Z);

    let aspect = window_query
        .single()
        .map(|w| w.width() / w.height().max(1.0))
        .unwrap_or(16.0 / 9.0);

    let (viewport_width, viewport_height) = viewport_world_size(aspect, camera_state.zoom);

    if let Projection::Orthographic(ref mut ortho) = *projection {
        ortho.scale = 1.0;
        ortho.scaling_mode = if aspect > WORLD_ASPECT {
            ScalingMode::FixedHorizontal { viewport_width }
        } else {
            ScalingMode::FixedVertical { viewport_height }
        };
    }
}

/// Pixel distance before an LMB press becomes a pan (below = click).
pub const MAP_DRAG_THRESHOLD_PX: f32 = 5.0;

/// Tracks LMB press so short clicks select hexes and longer drags pan.
#[derive(Resource, Default)]
pub struct CameraDragState {
    press_cursor: Option<Vec2>,
    dragging: bool,
    /// Set for one frame when LMB releases without exceeding the drag threshold.
    pub just_clicked_map: bool,
}

pub fn handle_camera_input(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mut mouse_wheel: MessageReader<MouseWheel>,
    mut camera_state: ResMut<CameraState>,
    mut drag: ResMut<CameraDragState>,
    window_query: Query<&Window, With<PrimaryWindow>>,
) {
    drag.just_clicked_map = false;
    let Ok(window) = window_query.single() else {
        return;
    };
    let cursor = window.cursor_position();

    if mouse_buttons.just_pressed(MouseButton::Left) {
        drag.press_cursor = cursor;
        drag.dragging = false;
    }

    if mouse_buttons.pressed(MouseButton::Left) {
        if let (Some(origin), Some(pos)) = (drag.press_cursor, cursor)
            && !drag.dragging
            && origin.distance(pos) > MAP_DRAG_THRESHOLD_PX
        {
            drag.dragging = true;
        }
        if drag.dragging {
            let delta = mouse_motion.delta;
            if delta != Vec2::ZERO {
                let aspect = window.width() / window.height().max(1.0);
                let (viewport_width, viewport_height) =
                    viewport_world_size(aspect, camera_state.zoom);

                let lon_per_pixel = f64::from(viewport_width) / f64::from(window.width());
                let lat_per_pixel = f64::from(viewport_height) / f64::from(window.height());

                camera_state.center_lon_rad -= f64::from(delta.x) * lon_per_pixel;
                camera_state.center_lat_rad += f64::from(delta.y) * lat_per_pixel;
                camera_state.center_lat_rad = camera_state
                    .center_lat_rad
                    .clamp(-PI / 2.0 + 0.01, PI / 2.0 - 0.01);
            }
        }
    }

    if mouse_buttons.just_released(MouseButton::Left) {
        if drag.press_cursor.is_some() && !drag.dragging {
            drag.just_clicked_map = true;
        }
        drag.press_cursor = None;
        drag.dragging = false;
    }

    for event in mouse_wheel.read() {
        let factor = if event.y > 0.0 {
            1.0 + ZOOM_SENSITIVITY
        } else {
            1.0 - ZOOM_SENSITIVITY
        };
        camera_state.zoom = (camera_state.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
    }
}

pub fn cycle_render_mode_on_keypress(
    keys: Res<ButtonInput<KeyCode>>,
    mut render_mode: ResMut<CurrentRenderMode>,
) {
    if keys.just_pressed(KeyCode::KeyM) {
        render_mode.0 = render_mode.0.cycle_next();
        eprintln!("[render] mode: {}", render_mode.0.label());
    }
}

pub fn update_window_title(
    render_mode: Res<CurrentRenderMode>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if !render_mode.is_changed() {
        return;
    }
    if let Ok(mut window) = windows.single_mut() {
        window.title = format!(
            "Genesis Engine — {} (press M to cycle)",
            render_mode.0.label()
        );
    }
}

pub fn update_hex_colors(
    world_res: Option<Res<WorldResource>>,
    render_mode: Res<CurrentRenderMode>,
    biology: Option<Res<ActiveBiologyView>>,
    mut colors_dirty: ResMut<ColorsDirty>,
    index: Res<HexMeshIndex>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    // Re-tint when the mode changes, the colors are flagged dirty, or the biology
    // view was (re)registered (so biology layers fill in once the world loads).
    if !render_mode.is_changed()
        && !colors_dirty.0
        && !biology.as_ref().is_some_and(|b| b.is_changed())
    {
        return;
    }
    let Some(world_res) = world_res else {
        colors_dirty.0 = false;
        return;
    };
    if index.chunks.is_empty() {
        // Rebuild hasn't run yet this frame; keep the flag so we retry.
        return;
    }
    colors_dirty.0 = false;

    let data = &world_res.0.data;
    let grid = &data.grid;
    let n = data.cell_count() as usize;
    let bio = biology.as_ref().map(|b| b.0.as_ref());

    for chunk in &index.chunks {
        let Some(mesh) = meshes.get_mut(&chunk.mesh) else {
            continue;
        };
        let Some(VertexAttributeValues::Float32x4(colors)) =
            mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR)
        else {
            continue;
        };
        for &(hex, base, count) in &chunk.slots {
            let idx = hex.0 as usize;
            if idx >= n {
                continue;
            }
            let color = hex_color_for_mode(data, idx, render_mode.0, grid.is_pentagon(hex), bio)
                .to_linear()
                .to_f32_array();
            let start = base as usize;
            for v in &mut colors[start..start + count as usize] {
                *v = color;
            }
        }
    }
}

/// Hexes per combined mesh. Bounds the unit of GPU re-upload on recolor and
/// gives per-chunk frustum culling; level 8 (65,612 hexes) yields ~17 chunks.
const HEXES_PER_CHUNK: usize = 4096;

#[allow(clippy::too_many_arguments)]
pub fn render_world_if_dirty(
    mut commands: Commands,
    world_res: Option<Res<WorldResource>>,
    render_mode: Res<CurrentRenderMode>,
    biology: Option<Res<ActiveBiologyView>>,
    mut world_dirty: ResMut<WorldDirty>,
    mut cache: ResMut<HexEntityCache>,
    mut index: ResMut<HexMeshIndex>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    if !world_dirty.0 {
        return;
    }
    let Some(world_res) = world_res else {
        return;
    };

    for entity in cache.entities.drain(..) {
        commands.entity(entity).despawn();
    }
    index.clear();

    let data = &world_res.0.data;
    let grid = &data.grid;
    let bio = biology.as_ref().map(|b| b.0.as_ref());
    // White base material shared by every chunk: the shader multiplies it by
    // per-vertex colors, so the vertex color IS the hex color.
    let shared_material = materials.add(ColorMaterial::default());

    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut slots: Vec<(HexId, u32, u8)> = Vec::new();
    let mut spawned = 0u32;
    let mut skipped = 0u32;

    let flush = |commands: &mut Commands,
                 meshes: &mut Assets<Mesh>,
                 cache: &mut HexEntityCache,
                 index: &mut HexMeshIndex,
                 positions: &mut Vec<[f32; 3]>,
                 colors: &mut Vec<[f32; 4]>,
                 indices: &mut Vec<u32>,
                 slots: &mut Vec<(HexId, u32, u8)>| {
        if slots.is_empty() {
            return;
        }
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            bevy::asset::RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, std::mem::take(positions));
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, std::mem::take(colors));
        mesh.insert_indices(Indices::U32(std::mem::take(indices)));
        let handle = meshes.add(mesh);
        let entity = commands
            .spawn((
                Mesh2d(handle.clone()),
                MeshMaterial2d(shared_material.clone()),
                Transform::from_xyz(0.0, 0.0, 0.0),
            ))
            .id();
        cache.entities.push(entity);
        index.chunks.push(HexChunk {
            mesh: handle,
            slots: std::mem::take(slots),
        });
    };

    for hex in grid.iter() {
        let (center_lat, center_lon) = grid.center_lat_lon(hex);

        if should_skip_for_equirectangular(center_lat) {
            skipped += 1;
            continue;
        }

        let vertices = hex_polygon_vertices(grid, hex);
        let center_2d = project(center_lat, center_lon);
        let ring: Vec<(f32, f32)> = vertices
            .iter()
            .map(|v| {
                let (lat, lon) = direction_to_lat_lon(*v);
                // Unwrap vertex longitude to be contiguous with center longitude.
                // This fixes the antimeridian seam: a hex straddling the ±π discontinuity
                // gets vertex longitudes unwrapped to its center's reference frame, so
                // the polygon draws as a contiguous shape that may extend slightly past
                // ±π on one side but is no longer torn in half.
                let unwrapped_lon = unwrap_lon_relative(lon, center_lon);
                project(lat, unwrapped_lon)
            })
            .collect();

        let idx = hex.0 as usize;
        let color = hex_color_for_mode(data, idx, render_mode.0, grid.is_pentagon(hex), bio)
            .to_linear()
            .to_f32_array();

        // Triangle fan: center vertex + ring, offset into the chunk buffers.
        let base = positions.len() as u32;
        positions.push([center_2d.0, center_2d.1, 0.0]);
        colors.push(color);
        for &(x, y) in &ring {
            positions.push([x, y, 0.0]);
            colors.push(color);
        }
        let ring_len = ring.len() as u32;
        for i in 0..ring_len {
            indices.extend_from_slice(&[base, base + 1 + i, base + 1 + (i + 1) % ring_len]);
        }
        slots.push((hex, base, (1 + ring_len) as u8));
        spawned += 1;

        if slots.len() >= HEXES_PER_CHUNK {
            flush(
                &mut commands,
                &mut meshes,
                &mut cache,
                &mut index,
                &mut positions,
                &mut colors,
                &mut indices,
                &mut slots,
            );
        }
    }
    flush(
        &mut commands,
        &mut meshes,
        &mut cache,
        &mut index,
        &mut positions,
        &mut colors,
        &mut indices,
        &mut slots,
    );

    info!(
        "Rendered {spawned} hexes in {} chunks ({skipped} skipped near poles)",
        index.chunks.len()
    );
    world_dirty.0 = false;
}
