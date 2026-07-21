//! Bevy resources for world rendering state.

use bevy::prelude::*;
use genesis_core::World;
use genesis_core::biology_view::BiologyView;

use crate::projection::MapProjection;

/// The active map projection (flat equirectangular vs. rotatable globe). Cycled
/// with the `P` key; a change rebuilds the hex mesh topology (the visible hex set
/// differs between projections).
#[derive(Resource, Default)]
pub struct CurrentProjection(pub MapProjection);

/// Wraps the simulation [`World`] as a Bevy resource (read-only for rendering).
#[derive(Resource)]
pub struct WorldResource(pub World);

/// The active biology read-view (Prep-09 §2). Holds a `StubBiologyView` now; a
/// `genesis_biology` adapter at Doc 09. Lives here (not `genesis_ui`) so the
/// recolor systems can read it; inserted by `genesis_ui` at world load.
///
/// **Doc 09 integration checklist** (Prep-09 §13 — the entire shell goes live by
/// doing only this, no screen/overlay/inspector rework):
/// 1. Implement `BiologyView` in a `genesis_biology` adapter over the real
///    ledger + `WorldData` biology arrays.
/// 2. Register it here instead of `StubBiologyView` (the single
///    `ActiveBiologyView(Box::new(StubBiologyView::new(seed)))` line in
///    `genesis_ui::ui` on `GenEvent::InitialWorld`).
/// 3. Fill `HistoryFrame::{biome,biomass,biotic_richness}` from real data so the
///    Biome/Biomass/Diversity layers become scrub-accurate.
/// 4. Emit real biology `EventKind`s so the timeline pips switch from stub
///    `life_events` to real.
/// 5. Map `TraitSet` → the Doc 09 creature renderer's inputs and drop an
///    `ImageNode` into the existing text species cards / tree nodes.
/// 6. Delete the stub and its `// STUB` markers.
#[derive(Resource)]
pub struct ActiveBiologyView(pub Box<dyn BiologyView>);

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

/// True while a full-screen UI overlay (Bestiary / Tree of Life / species detail)
/// has captured the pointer, so map pan/zoom must not also react to the wheel or
/// drag underneath it. Set by the UI layer each frame from its overlay state.
#[derive(Resource, Default)]
pub struct PointerCapturedByUi(pub bool);

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
    /// Unit-sphere direction of every vertex, parallel to the mesh's position
    /// buffer. Lets the globe reproject positions in place each frame as it
    /// rotates, without rebuilding the mesh. A slot's center direction is
    /// `vertex_dirs[first_vertex]` (the fan's center vertex is pushed first).
    pub vertex_dirs: Vec<Vec3>,
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

/// When true (or LOD band changes), river overlay meshes are rebuilt.
#[derive(Resource)]
pub struct RiversDirty {
    pub dirty: bool,
    pub last_lod: Option<genesis_core::data::RiverClass>,
}

impl Default for RiversDirty {
    fn default() -> Self {
        Self {
            dirty: true,
            last_lod: None,
        }
    }
}
