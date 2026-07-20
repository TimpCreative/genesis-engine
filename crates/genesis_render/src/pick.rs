//! Screen-space hex picking for both the flat map and the globe.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use genesis_core::{HexGrid, HexId};

use crate::projection::{MapProjection, project, should_skip_for_equirectangular};
use crate::resources::CameraState;
use crate::systems::{globe_viewport_world_size, view_center, viewport_world_size};

/// Convert a window cursor position to a world hex, or `None` if the cursor is
/// outside the window, over a polar-skipped region, or off the globe's disc.
pub fn screen_to_hex(
    window: &Window,
    camera: &CameraState,
    projection: MapProjection,
    cursor: Vec2,
    grid: &HexGrid,
) -> Option<HexId> {
    let (lat, lon) = screen_to_lat_lon(window, camera, projection, cursor)?;
    // The flat map can't represent the polar caps; reject picks there. The globe
    // already rejects off-disc picks via `unproject` returning `None`.
    if projection == MapProjection::Equirectangular && should_skip_for_equirectangular(lat) {
        return None;
    }
    Some(grid.nearest_hex(lat, lon))
}

/// Cursor → geographic lat/lon using the same viewport math as [`sync_camera`].
pub fn screen_to_lat_lon(
    window: &Window,
    camera: &CameraState,
    projection: MapProjection,
    cursor: Vec2,
) -> Option<(f64, f64)> {
    let width = window.width();
    let height = window.height();
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let aspect = width / height;

    // Bevy window cursor: origin top-left, +y down. Ortho world: +y up.
    let ndc_x = (cursor.x / width) * 2.0 - 1.0;
    let ndc_y = 1.0 - (cursor.y / height) * 2.0;

    match projection {
        MapProjection::Equirectangular => {
            let (viewport_width, viewport_height) = viewport_world_size(aspect, camera.zoom);
            let (cx, cy) = project(camera.center_lat_rad, camera.center_lon_rad);
            let world_x = cx + ndc_x * (viewport_width * 0.5);
            let world_y = cy + ndc_y * (viewport_height * 0.5);
            Some(projection.unproject(world_x, world_y, view_center(camera))?)
        }
        MapProjection::Orthographic => {
            // Globe is centered at the origin; no camera translation.
            let (viewport_width, viewport_height) = globe_viewport_world_size(aspect, camera.zoom);
            let world_x = ndc_x * (viewport_width * 0.5);
            let world_y = ndc_y * (viewport_height * 0.5);
            projection.unproject(world_x, world_y, view_center(camera))
        }
    }
}

/// Convenience: pick under the primary window cursor.
pub fn cursor_hex(
    window_query: &Query<&Window, With<PrimaryWindow>>,
    camera: &CameraState,
    projection: MapProjection,
    grid: &HexGrid,
) -> Option<HexId> {
    let window = window_query.single().ok()?;
    let cursor = window.cursor_position()?;
    screen_to_hex(window, camera, projection, cursor, grid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::HexGrid;

    #[test]
    fn lat_lon_center_of_viewport_matches_camera() {
        let camera = CameraState {
            center_lat_rad: 0.25,
            center_lon_rad: -0.5,
            zoom: 2.0,
        };
        // Synthetic window geometry — only aspect + size matter for NDC.
        let mut window = Window::default();
        window.resolution.set(800.0, 600.0);
        let cursor = Vec2::new(400.0, 300.0);
        let (lat, lon) = screen_to_lat_lon(&window, &camera, MapProjection::Equirectangular, cursor)
            .expect("center");
        assert!((lat - camera.center_lat_rad).abs() < 1e-4);
        assert!((lon - camera.center_lon_rad).abs() < 1e-4);
    }

    #[test]
    fn screen_to_hex_returns_valid_id() {
        let grid = HexGrid::new(5, 6371.0).expect("grid");
        let camera = CameraState::default();
        let mut window = Window::default();
        window.resolution.set(800.0, 600.0);
        let hex = screen_to_hex(
            &window,
            &camera,
            MapProjection::Equirectangular,
            Vec2::new(400.0, 300.0),
            &grid,
        )
        .expect("equator pick");
        assert!(hex.0 < grid.cell_count());
    }

    #[test]
    fn globe_center_pick_matches_view_center() {
        // Cursor at the middle of the disc → the sub-viewer point.
        let grid = HexGrid::new(5, 6371.0).expect("grid");
        let camera = CameraState {
            center_lat_rad: 0.2,
            center_lon_rad: -0.6,
            zoom: 1.0,
        };
        let mut window = Window::default();
        window.resolution.set(800.0, 800.0);
        let (lat, lon) =
            screen_to_lat_lon(&window, &camera, MapProjection::Orthographic, Vec2::new(400.0, 400.0))
                .expect("disc center");
        assert!((lat - camera.center_lat_rad).abs() < 1e-4, "lat {lat}");
        assert!((lon - camera.center_lon_rad).abs() < 1e-4, "lon {lon}");
        // And a corner (outside the inscribed disc) rejects.
        assert!(
            screen_to_hex(
                &window,
                &camera,
                MapProjection::Orthographic,
                Vec2::new(5.0, 5.0),
                &grid,
            )
            .is_none()
        );
    }
}
