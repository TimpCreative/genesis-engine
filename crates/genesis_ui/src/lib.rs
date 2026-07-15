//! Interactive UI shell and world-generation orchestration for Genesis Engine.
//!
//! [`GenesisUiPlugin`] provides the main menu, world setup, background
//! generation with progress, and the timeline viewer. [`worldgen`] owns the
//! layer registration order and history-frame buffering; the headless
//! screenshot path in `genesis_app` reuses it directly.

pub mod smoke;
pub mod ui;
pub mod worldgen;

pub use smoke::SmokePlugin;
pub use ui::{AppScreen, GenesisUiPlugin, WorldTimeline};
pub use worldgen::{
    HistoryFrame, MAX_HISTORY_FRAMES, WorldGenConfig, generate_full_history,
    generate_world_with_history, history_stride_years,
};
