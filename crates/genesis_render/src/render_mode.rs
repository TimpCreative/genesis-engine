//! Render mode selection for hex coloring.

use bevy::prelude::Resource;

/// Determines how each hex is colored.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum RenderMode {
    /// Color by elevation (water-aware when hydrology has written levels).
    #[default]
    Elevation,
    /// Color by mean annual temperature.
    Temperature,
    /// Color by annual precipitation.
    Precipitation,
    /// Color by Köppen-like climate regime (Doc 07 §10).
    ClimateRegime,
    /// Color by soil fertility / class (Doc 08 §12).
    Soil,
}

impl RenderMode {
    pub fn cycle_next(self) -> Self {
        match self {
            Self::Elevation => Self::Temperature,
            Self::Temperature => Self::Precipitation,
            Self::Precipitation => Self::ClimateRegime,
            Self::ClimateRegime => Self::Soil,
            Self::Soil => Self::Elevation,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Elevation => "Elevation",
            Self::Temperature => "Temperature",
            Self::Precipitation => "Precipitation",
            Self::ClimateRegime => "Climate Regime",
            Self::Soil => "Soil",
        }
    }
}

/// Active hex coloring mode (cycle with M key).
#[derive(Default, Resource)]
pub struct CurrentRenderMode(pub RenderMode);
