//! Hex grid rendering for Genesis Engine (elevation-colored terrain).

mod color;
mod plugin;
mod polygon;
mod projection;
mod resources;
mod systems;

pub use color::{MAX_ELEVATION_M, MIN_ELEVATION_M, elevation_color, hex_fill_color};
pub use plugin::GenesisRenderPlugin;
pub use polygon::hex_polygon_vertices;
pub use resources::{CameraState, WorldDirty, WorldResource};
