//! Bevy resources for world rendering state.

use bevy::prelude::*;
use genesis_core::World;

/// Wraps the simulation [`World`] as a Bevy resource (read-only for rendering).
#[derive(Resource)]
pub struct WorldResource(pub World);

/// When true, hex mesh entities are rebuilt from [`WorldResource`].
#[derive(Resource)]
pub struct WorldDirty(pub bool);

impl Default for WorldDirty {
    fn default() -> Self {
        Self(true)
    }
}

/// Pan/zoom state for the 2D equirectangular view.
#[derive(Resource)]
pub struct CameraState {
    pub center_lon_rad: f64,
    pub center_lat_rad: f64,
    /// 1.0 = whole world visible in the viewport.
    pub zoom: f32,
}

impl Default for CameraState {
    fn default() -> Self {
        Self {
            center_lon_rad: 0.0,
            center_lat_rad: 0.0,
            zoom: 1.0,
        }
    }
}

/// Spawned hex entities, despawned on world rebuild.
#[derive(Resource, Default)]
pub struct HexEntityCache {
    pub entities: Vec<Entity>,
}
