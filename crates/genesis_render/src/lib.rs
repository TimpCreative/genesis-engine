//! Hex grid rendering for Genesis Engine (elevation and climate visualization).

mod color;
mod outline;
mod pick;
mod plugin;
mod polygon;
mod projection;
mod render_mode;
mod resources;
mod rivers;
mod systems;

pub use color::{
    MAX_ELEVATION_M, MIN_ELEVATION_M, biome_color, elevation_color, heatmap_color,
    hex_color_for_mode, hex_fill_color, precipitation_to_color, regime_to_color, soil_class_color,
    temperature_to_color,
};
pub use outline::SelectedHex;
pub use pick::{cursor_hex, screen_to_hex, screen_to_lat_lon};
pub use plugin::GenesisRenderPlugin;
pub use polygon::hex_polygon_vertices;
pub use projection::{MapProjection, ViewCenter, lat_lon_to_dir};
pub use render_mode::{CurrentRenderMode, RenderMode};
pub use resources::{
    ActiveBiologyView, CameraState, ColorsDirty, CurrentProjection, HexChunk, HexEntityCache,
    HexMeshIndex, PointerCapturedByUi, RiversDirty, WorldDirty, WorldResource,
};
pub use systems::{CameraDragState, MAP_DRAG_THRESHOLD_PX};
