//! Bevy systems for camera, input, and hex mesh rendering.

use std::f64::consts::PI;

use bevy::camera::{OrthographicProjection, ScalingMode};
use bevy::input::mouse::{AccumulatedMouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use genesis_core::HexId;

use crate::color::hex_color_for_mode;
use crate::polygon::{direction_to_lat_lon, hex_polygon_vertices, unwrap_lon_relative};
use crate::projection::{hex_mesh_2d, project, should_skip_for_equirectangular};
use crate::render_mode::CurrentRenderMode;
use crate::resources::{CameraState, ColorsDirty, HexEntityCache, WorldDirty, WorldResource};

const MIN_ZOOM: f32 = 0.1;
const MAX_ZOOM: f32 = 50.0;
const ZOOM_SENSITIVITY: f32 = 0.1;

/// Maps a spawned mesh entity back to its hex for color updates.
#[derive(Component)]
pub(crate) struct HexCell(pub HexId);

/// World width/height in projected radians (equirectangular).
const WORLD_WIDTH: f32 = (2.0 * PI) as f32;
const WORLD_HEIGHT: f32 = PI as f32;
const WORLD_ASPECT: f32 = WORLD_WIDTH / WORLD_HEIGHT;

/// 2D camera sits above meshes at z = 0 (Bevy looks down -Z).
const CAMERA_Z: f32 = 999.0;

#[derive(Component)]
pub(crate) struct MainCamera;

/// Visible world size in radians for the current window aspect and zoom.
fn viewport_world_size(window_aspect: f32, zoom: f32) -> (f32, f32) {
    if window_aspect > WORLD_ASPECT {
        let width = WORLD_WIDTH / zoom;
        (width, width / window_aspect)
    } else {
        let height = WORLD_HEIGHT / zoom;
        (height * window_aspect, height)
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

pub fn handle_camera_input(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mut mouse_wheel: MessageReader<MouseWheel>,
    mut camera_state: ResMut<CameraState>,
    window_query: Query<&Window, With<PrimaryWindow>>,
) {
    if mouse_buttons.pressed(MouseButton::Left) {
        let delta = mouse_motion.delta;
        if delta != Vec2::ZERO {
            let Some(window) = window_query.single().ok() else {
                return;
            };
            let aspect = window.width() / window.height().max(1.0);
            let (viewport_width, viewport_height) = viewport_world_size(aspect, camera_state.zoom);

            let lon_per_pixel = f64::from(viewport_width) / f64::from(window.width());
            let lat_per_pixel = f64::from(viewport_height) / f64::from(window.height());

            camera_state.center_lon_rad -= f64::from(delta.x) * lon_per_pixel;
            camera_state.center_lat_rad += f64::from(delta.y) * lat_per_pixel;
            camera_state.center_lat_rad = camera_state
                .center_lat_rad
                .clamp(-PI / 2.0 + 0.01, PI / 2.0 - 0.01);
        }
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
    mut colors_dirty: ResMut<ColorsDirty>,
    mut materials: ResMut<Assets<ColorMaterial>>,
    query: Query<(&HexCell, &MeshMaterial2d<ColorMaterial>)>,
) {
    if !render_mode.is_changed() && !colors_dirty.0 {
        return;
    }
    colors_dirty.0 = false;
    let Some(world_res) = world_res else {
        return;
    };

    let data = &world_res.0.data;
    let grid = &data.grid;

    for (hex_cell, mat_handle) in &query {
        let idx = hex_cell.0.0 as usize;
        if idx >= data.cell_count() as usize {
            continue;
        }
        let is_pentagon = grid.is_pentagon(hex_cell.0);
        let color = hex_color_for_mode(data, idx, render_mode.0, is_pentagon);
        if let Some(material) = materials.get_mut(&mat_handle.0) {
            material.color = color;
        }
    }
}

pub fn render_world_if_dirty(
    mut commands: Commands,
    world_res: Option<Res<WorldResource>>,
    render_mode: Res<CurrentRenderMode>,
    mut world_dirty: ResMut<WorldDirty>,
    mut cache: ResMut<HexEntityCache>,
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

    let grid = &world_res.0.data.grid;
    let mut spawned = 0u32;
    let mut skipped = 0u32;

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
        let mesh = hex_mesh_2d(center_2d, &ring);

        let mesh_handle = meshes.add(mesh);
        let idx = hex.0 as usize;
        let is_pentagon = grid.is_pentagon(hex);
        let fill = hex_color_for_mode(&world_res.0.data, idx, render_mode.0, is_pentagon);
        let material = materials.add(ColorMaterial::from_color(fill));

        let entity = commands
            .spawn((
                HexCell(hex),
                Mesh2d(mesh_handle),
                MeshMaterial2d(material),
                Transform::from_xyz(0.0, 0.0, 0.0),
            ))
            .id();
        cache.entities.push(entity);
        spawned += 1;
    }

    info!("Rendered {spawned} hexes ({skipped} skipped near poles)");
    world_dirty.0 = false;
}
