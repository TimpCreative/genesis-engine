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

/// When true, existing hex materials are recolored from [`WorldResource`]
/// without rebuilding meshes. Set by timeline scrubbing: the grid never
/// changes within a run, so recoloring is all a year change needs.
#[derive(Resource, Default)]
pub struct ColorsDirty(pub bool);

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

/// Spawned map entities (chunk meshes), despawned on world rebuild.
#[derive(Resource, Default)]
pub struct HexEntityCache {
    pub entities: Vec<Entity>,
}

/// One combined mesh carrying a batch of hexes with per-vertex colors.
pub struct HexChunk {
    pub mesh: Handle<Mesh>,
    /// `(hex, first_vertex, vertex_count)` — the hex's fan vertices within
    /// this chunk's buffers (7 for hexagons, 6 for pentagons).
    pub slots: Vec<(genesis_core::HexId, u32, u8)>,
}

/// Chunk lookup for in-place recoloring. Hexes are merged into a few large
/// meshes (one draw call each) instead of one entity per hex; recoloring
/// rewrites the color attribute buffers rather than touching materials.
#[derive(Resource, Default)]
pub struct HexMeshIndex {
    pub chunks: Vec<HexChunk>,
}

impl HexMeshIndex {
    pub fn clear(&mut self) {
        self.chunks.clear();
    }
}
