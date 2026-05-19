//! Intervention payload variants.

use serde::{Deserialize, Serialize};

/// Intervention payload variants. Phase 0 has only a placeholder;
/// later phases add specific actions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum InterventionAction {
    /// Used only in tests until later phases add real variants.
    Placeholder { description: String },
    // TODO(phase-1+): AdjustHexProperty, AdjustParameter, SpawnSettlement,
    // RenameEntity, PlaceEvent, BoostSetting, ...
}
