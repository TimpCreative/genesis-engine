//! Hex grid rendering for Genesis Engine (elevation and climate visualization).

mod color;
mod plugin;
mod polygon;
mod projection;
mod render_mode;
mod resources;
mod systems;

pub use color::{
    MAX_ELEVATION_M, MIN_ELEVATION_M, elevation_color, hex_color_for_mode, hex_fill_color,
    precipitation_to_color, temperature_to_color,
};
pub use plugin::GenesisRenderPlugin;
pub use polygon::hex_polygon_vertices;
pub use render_mode::{CurrentRenderMode, RenderMode};
pub use resources::{
    CameraState, ColorsDirty, HexChunk, HexEntityCache, HexMeshIndex, WorldDirty, WorldResource,
};
