//! Interactive UI shell and world-generation orchestration for Genesis Engine.
//!
//! [`GenesisUiPlugin`] provides the main menu, world setup, background
//! generation with progress, and the timeline viewer. [`worldgen`] owns the
//! layer registration order and history-frame buffering; the headless
//! screenshot path in `genesis_app` reuses it directly.

pub mod hex_inspect;
pub mod hydro_validation;
pub mod smoke;
pub mod ui;
pub mod worldgen;

pub use smoke::SmokePlugin;
pub use ui::{AppScreen, GenesisUiPlugin, WorldTimeline};
pub use worldgen::{
    GenEvent, HISTORY_STRIDE_YEARS, HistoryFrame, WorldGenConfig, generate_full_history,
    generate_world_streaming, generate_world_with_history, history_stride_years,
    max_history_frames,
};
