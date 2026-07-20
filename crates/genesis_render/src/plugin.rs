//! [`GenesisRenderPlugin`] — Bevy plugin for Phase 0 bare hex rendering.

use bevy::prelude::*;

use crate::outline::{SelectedHex, sync_selection_outline};
use crate::render_mode::CurrentRenderMode;
use crate::resources::{
    CameraState, ColorsDirty, HexEntityCache, HexMeshIndex, RiversDirty, WorldDirty,
};
use crate::WorldResource;
use crate::resources::CurrentProjection;
use crate::rivers::update_river_overlay;
use crate::systems::{
    CameraDragState, cycle_projection_on_keypress, cycle_render_mode_on_keypress,
    handle_camera_input, refresh_projected_positions, render_world_if_dirty, setup_camera,
    sync_camera, update_hex_colors, update_window_title,
};

/// Renders a [`crate::WorldResource`] as colored equirectangular hex polygons.
pub struct GenesisRenderPlugin;

impl Plugin for GenesisRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraState>()
            .init_resource::<CameraDragState>()
            .init_resource::<SelectedHex>()
            .init_resource::<WorldDirty>()
            .init_resource::<ColorsDirty>()
            .init_resource::<RiversDirty>()
            .init_resource::<HexEntityCache>()
            .init_resource::<HexMeshIndex>()
            .init_resource::<CurrentRenderMode>()
            .init_resource::<CurrentProjection>()
            .add_systems(Startup, setup_camera)
            .add_systems(
                Update,
                (
                    // Map input (M-cycle, P-projection, mouse pan/zoom) only when a
                    // world is loaded — so keys don't fire in the menus (e.g. while
                    // typing a seed). Menus never hold a WorldResource.
                    (
                        handle_camera_input,
                        cycle_render_mode_on_keypress,
                        cycle_projection_on_keypress,
                    )
                        .run_if(resource_exists::<WorldResource>),
                    update_window_title,
                    sync_camera,
                    // Globe rotation reprojects positions in place; must run before
                    // render (which skips when clean) and after camera/input.
                    refresh_projected_positions,
                    render_world_if_dirty,
                    update_hex_colors,
                    update_river_overlay,
                    sync_selection_outline,
                )
                    .chain(),
            );
    }
}
