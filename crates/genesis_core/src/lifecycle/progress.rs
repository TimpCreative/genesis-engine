//! Progress reporting during world generation.

use crate::events::Event;
use crate::time::WorldYear;

/// Progress update emitted periodically during world generation.
///
/// Used by the UI to show progress and stream major events to the user
/// while the simulation runs (Factorio-style preview pattern from Doc 04).
#[derive(Clone, Debug)]
pub struct GenerationProgress<'a> {
    /// Simulation year currently being processed.
    pub current_year: WorldYear,
    /// Year the generation will stop at.
    pub target_year: WorldYear,
    /// Events generated in the most recent step (since last callback).
    /// Phase 0: always empty until real simulation lands.
    pub recent_events: &'a [Event],
    /// Total events generated so far across all branches.
    pub total_events: usize,
}

impl<'a> GenerationProgress<'a> {
    /// Returns generation progress as a fraction in 0.0..=1.0.
    pub fn fraction(&self) -> f64 {
        let cur = self.current_year.value() as f64;
        let tgt = self.target_year.value() as f64;
        if tgt <= 0.0 {
            return 1.0;
        }
        (cur / tgt).clamp(0.0, 1.0)
    }
}

/// Callback invoked periodically during generation.
pub type ProgressCallback<'a> = &'a mut dyn FnMut(GenerationProgress<'_>);
