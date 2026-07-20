//! [`GenesisRenderPlugin`] — Bevy plugin for Phase 0 bare hex rendering.

use bevy::prelude::*;

use crate::outline::{SelectedHex, sync_selection_outline};
use crate::render_mode::CurrentRenderMode;
use crate::resources::{
    CameraState, ColorsDirty, HexEntityCache, HexMeshIndex, RiversDirty, WorldDirty,
};
use crate::rivers::update_river_overlay;
use crate::systems::{
    CameraDragState, cycle_render_mode_on_keypress, handle_camera_input, render_world_if_dirty,
    setup_camera, sync_camera, update_hex_colors, update_window_title,
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
            .add_systems(Startup, setup_camera)
            .add_systems(
                Update,
                (
                    handle_camera_input,
                    cycle_render_mode_on_keypress,
                    update_window_title,
                    sync_camera,
                    render_world_if_dirty,
                    update_hex_colors,
                    update_river_overlay,
                    sync_selection_outline,
                )
                    .chain(),
            );
    }
}
