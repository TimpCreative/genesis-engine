//! Render mode selection for hex coloring.

use bevy::prelude::Resource;

/// Determines how each hex is colored.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum RenderMode {
    /// Color by elevation (current behavior).
    #[default]
    Elevation,
    /// Color by mean annual temperature.
    Temperature,
    /// Color by annual precipitation.
    Precipitation,
    /// Color by Köppen-like climate regime (Doc 07 §10).
    ClimateRegime,
}

impl RenderMode {
    pub fn cycle_next(self) -> Self {
        match self {
            Self::Elevation => Self::Temperature,
            Self::Temperature => Self::Precipitation,
            Self::Precipitation => Self::ClimateRegime,
            Self::ClimateRegime => Self::Elevation,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Elevation => "Elevation",
            Self::Temperature => "Temperature",
            Self::Precipitation => "Precipitation",
            Self::ClimateRegime => "Climate Regime",
        }
    }
}

/// Active hex coloring mode (cycle with M key).
#[derive(Default, Resource)]
pub struct CurrentRenderMode(pub RenderMode);
