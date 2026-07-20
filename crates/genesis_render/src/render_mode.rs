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
    /// Categorical biome fill (Prep-09 §4; via `BiologyView::biome_at`).
    Biome,
    /// Living-biomass heatmap (Prep-09 §4; via `BiologyView::biomass_at`).
    Biomass,
    /// Biotic-richness / diversity heatmap (Prep-09 §4; `richness_at`).
    Diversity,
    /// Civilization placeholder, reserved for Doc 10.
    Society,
}

impl RenderMode {
    /// All modes in top-bar / M-cycle order.
    pub const ALL: [RenderMode; 9] = [
        Self::Elevation,
        Self::Temperature,
        Self::Precipitation,
        Self::ClimateRegime,
        Self::Soil,
        Self::Biome,
        Self::Biomass,
        Self::Diversity,
        Self::Society,
    ];

    pub fn cycle_next(self) -> Self {
        let i = Self::ALL.iter().position(|&m| m == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Elevation => "Elevation",
            Self::Temperature => "Temperature",
            Self::Precipitation => "Precipitation",
            Self::ClimateRegime => "Climate Regime",
            Self::Soil => "Soil",
            Self::Biome => "Biome",
            Self::Biomass => "Biomass",
            Self::Diversity => "Diversity",
            Self::Society => "Society",
        }
    }

    /// True for modes that read through the `BiologyView` (stub in Prep-09).
    pub fn is_biology(self) -> bool {
        matches!(self, Self::Biome | Self::Biomass | Self::Diversity)
    }
}

/// Active hex coloring mode (cycle with M key).
#[derive(Default, Resource)]
pub struct CurrentRenderMode(pub RenderMode);
