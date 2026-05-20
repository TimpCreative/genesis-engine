//! Phase 0 bare rendering for Genesis Engine hex grids.

mod color;
mod plugin;
mod polygon;
mod projection;
mod resources;
mod systems;

pub use plugin::GenesisRenderPlugin;
pub use polygon::hex_polygon_vertices;
pub use resources::{CameraState, WorldDirty, WorldResource};
