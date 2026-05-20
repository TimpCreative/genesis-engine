//! [`GenesisRenderPlugin`] — Bevy plugin for Phase 0 bare hex rendering.

use bevy::prelude::*;

use crate::resources::{CameraState, HexEntityCache, WorldDirty};
use crate::systems::{
    handle_camera_input, handle_quit, handle_regenerate, render_world_if_dirty, setup_camera,
    sync_camera,
};

/// Renders a [`crate::WorldResource`] as colored equirectangular hex polygons.
pub struct GenesisRenderPlugin;

impl Plugin for GenesisRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraState>()
            .init_resource::<WorldDirty>()
            .init_resource::<HexEntityCache>()
            .add_systems(Startup, setup_camera)
            .add_systems(
                Update,
                (
                    handle_camera_input,
                    handle_regenerate,
                    handle_quit,
                    sync_camera,
                    render_world_if_dirty,
                )
                    .chain(),
            );
    }
}
