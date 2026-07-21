//! Hex coloring for elevation, climate, water, and soil visualization.
//!
//! Elevation colors follow Doc 06 §15 / Doc 08 §12.1: wet hexes
//! (`water_level_m > elevation_mean`) render depth-tinted water; ice is white;
//! salt flats pale; land uses the terrain ramp.

use bevy::prelude::*;
use genesis_core::data::{
    HydroFlags, SoilClass, WATER_NONE, WaterBodyId, WaterBodyKind, WorldData,
};

use crate::render_mode::RenderMode;

/// Matches tectonics clamp (Doc 06 §5.7); local to render — no `genesis_tectonics` dependency.
pub const MIN_ELEVATION_M: f32 = -11_000.0;
pub const MAX_ELEVATION_M: f32 = 9_000.0;

/// Matches hydrology/climate Formation end (Doc 07 §3.2 / Doc 08 §3.3).
pub const FORMATION_END_YEAR: i64 = 500_000_000;

struct ColorStop {
    /// Elevation relative to sea level (m): `elev − sea_level_m`.
    elev_m: f32,
    rgb: [f32; 3],
}

/// Piecewise-linear ramp in **sea-relative** elevation. Deep dry basins read as
/// dark rock, the 0 m freeboard datum is the coast cue, and land runs plain →
/// lowland → piedmont → upland → tan → brown → white. Freeboard (~+800 m) lands
/// in the green/olive band regardless of absolute sea level.
fn elevation_stops() -> Vec<ColorStop> {
    vec![
        ColorStop {
            elev_m: MIN_ELEVATION_M,
            rgb: [0.09, 0.08, 0.08],
        },
        ColorStop {
            elev_m: -4_000.0,
            rgb: [0.20, 0.18, 0.16],
        },
        ColorStop {
            elev_m: -1_000.0,
            rgb: [0.36, 0.32, 0.27],
        },
        ColorStop {
            elev_m: 0.0,
            rgb: [0.47, 0.43, 0.35],
        },
        // Coastal plain: bright grass green.
        ColorStop {
            elev_m: 10.0,
            rgb: [0.38, 0.54, 0.24],
        },
        // Lowland: rich green.
        ColorStop {
            elev_m: 200.0,
            rgb: [0.27, 0.48, 0.20],
        },
        // Piedmont: olive.
        ColorStop {
            elev_m: 600.0,
            rgb: [0.48, 0.52, 0.24],
        },
        // Upland: khaki.
        ColorStop {
            elev_m: 1_200.0,
            rgb: [0.62, 0.55, 0.30],
        },
        // High plateau: tan.
        ColorStop {
            elev_m: 2_000.0,
            rgb: [0.66, 0.55, 0.36],
        },
        ColorStop {
            elev_m: 4_000.0,
            rgb: [0.55, 0.48, 0.42],
        },
        ColorStop {
            elev_m: MAX_ELEVATION_M,
            rgb: [0.95, 0.95, 0.98],
        },
    ]
}

/// Barren Formation-era dry ramp: charcoal basins → slate continents → tan
/// highlands → white peaks. No grass greens (life has not arrived).
fn formation_elevation_stops() -> Vec<ColorStop> {
    vec![
        ColorStop {
            elev_m: MIN_ELEVATION_M,
            rgb: [0.07, 0.06, 0.06],
        },
        ColorStop {
            elev_m: -4_000.0,
            rgb: [0.16, 0.14, 0.13],
        },
        ColorStop {
            elev_m: -1_000.0,
            rgb: [0.28, 0.26, 0.24],
        },
        ColorStop {
            elev_m: 0.0,
            rgb: [0.38, 0.36, 0.34],
        },
        ColorStop {
            elev_m: 10.0,
            rgb: [0.42, 0.40, 0.37],
        },
        ColorStop {
            elev_m: 200.0,
            rgb: [0.48, 0.45, 0.41],
        },
        ColorStop {
            elev_m: 600.0,
            rgb: [0.52, 0.48, 0.42],
        },
        ColorStop {
            elev_m: 1_200.0,
            rgb: [0.58, 0.52, 0.44],
        },
        ColorStop {
            elev_m: 2_000.0,
            rgb: [0.62, 0.54, 0.44],
        },
        ColorStop {
            elev_m: 4_000.0,
            rgb: [0.55, 0.50, 0.46],
        },
        ColorStop {
            elev_m: MAX_ELEVATION_M,
            rgb: [0.92, 0.92, 0.94],
        },
    ]
}

fn clamp_elevation(elevation_m: f32) -> f32 {
    elevation_m.clamp(MIN_ELEVATION_M, MAX_ELEVATION_M)
}

fn lerp_rgb(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn sample_stops(stops: &[ColorStop], elevation_m: f32) -> [f32; 3] {
    if stops.is_empty() {
        return [0.5, 0.5, 0.5];
    }
    if elevation_m <= stops[0].elev_m {
        return stops[0].rgb;
    }
    let last = stops.len() - 1;
    if elevation_m >= stops[last].elev_m {
        return stops[last].rgb;
    }
    for window in stops.windows(2) {
        let lo = &window[0];
        let hi = &window[1];
        if elevation_m >= lo.elev_m && elevation_m <= hi.elev_m {
            let span = hi.elev_m - lo.elev_m;
            let t = if span.abs() < f32::EPSILON {
                0.0
            } else {
                (elevation_m - lo.elev_m) / span
            };
            return lerp_rgb(lo.rgb, hi.rgb, t);
        }
    }
    stops[last].rgb
}

/// Pure elevation → color mapping for the modern (post-Formation) terrain ramp.
///
/// Samples the dry ramp at `elevation_m − sea_level_m` so freeboard interiors
/// read as Kansas green/olive when sea sits at deeply negative absolute levels.
pub fn elevation_color(elevation_m: f32, sea_level_m: f32) -> Color {
    let relative = clamp_elevation(elevation_m - sea_level_m);
    let stops = elevation_stops();
    let rgb = sample_stops(&stops, relative);
    Color::srgb(rgb[0], rgb[1], rgb[2])
}

/// Barren Formation-era elevation → color (no grass greens).
///
/// Also sea-relative so early continents above a falling/rising sea still read
/// as continental platforms rather than absolute charcoal.
pub fn formation_elevation_color(elevation_m: f32, sea_level_m: f32) -> Color {
    let relative = clamp_elevation(elevation_m - sea_level_m);
    let stops = formation_elevation_stops();
    let rgb = sample_stops(&stops, relative);
    Color::srgb(rgb[0], rgb[1], rgb[2])
}

/// True while the world is still in Formation and formation was not skipped.
pub fn use_formation_elevation_palette(data: &WorldData) -> bool {
    !data.parameters.core.climate.skip_planetary_formation
        && data.current_year.value() < FORMATION_END_YEAR
}

/// Fill color for a hex or pentagon cell (same ramp; geometry distinguishes pentagons).
pub fn hex_fill_color(elevation_m: f32, sea_level_m: f32, _is_pentagon: bool) -> Color {
    elevation_color(elevation_m, sea_level_m)
}

/// Maps temperature in °C to a color across an indigo → blue → teal → green →
/// yellow → orange → red ramp. Control stops are packed at the warm end so the
/// 30/40/50 °C band (where oceans and the tropics cluster) stays legible instead
/// of saturating to one red. Domain −40…+50 °C (Earth's surface extremes).
pub fn temperature_to_color(temp_c: f32) -> Color {
    const STOPS: [(f32, (f32, f32, f32)); 9] = [
        (-40.0, (0.10, 0.10, 0.45)), // deep cold — indigo
        (-20.0, (0.15, 0.30, 0.80)), // cold — blue
        (-5.0, (0.25, 0.60, 0.90)),  // chilly — sky blue
        (5.0, (0.35, 0.80, 0.75)),   // cool — teal
        (14.0, (0.55, 0.85, 0.45)),  // mild — green
        (22.0, (0.90, 0.88, 0.35)),  // warm — yellow
        (30.0, (0.95, 0.65, 0.25)),  // hot — orange
        (40.0, (0.90, 0.35, 0.18)),  // very hot — red-orange
        (50.0, (0.70, 0.12, 0.12)),  // extreme — deep red
    ];
    let t = temp_c.clamp(STOPS[0].0, STOPS[STOPS.len() - 1].0);
    for pair in STOPS.windows(2) {
        let (t0, c0) = pair[0];
        let (t1, c1) = pair[1];
        if t <= t1 {
            let f = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
            return Color::srgb(
                c0.0 + f * (c1.0 - c0.0),
                c0.1 + f * (c1.1 - c0.1),
                c0.2 + f * (c1.2 - c0.2),
            );
        }
    }
    let last = STOPS[STOPS.len() - 1].1;
    Color::srgb(last.0, last.1, last.2)
}

/// Maps precipitation in mm/year to a color: brown (dry) → tan → green → dark blue-green (very wet).
///
/// Domain 0–2500 mm with midpoint near Earth's ~800 mm land mean (moderate green band).
pub fn precipitation_to_color(precip_mm: f32) -> Color {
    let p = precip_mm.clamp(0.0, 2500.0);

    let (r, g, b) = if p < 200.0 {
        let t = p / 200.0;
        (
            0.5 + t * (0.75 - 0.5),
            0.4 + t * (0.6 - 0.4),
            0.2 + t * (0.35 - 0.2),
        )
    } else if p < 600.0 {
        let t = (p - 200.0) / 400.0;
        (
            0.75 + t * (0.85 - 0.75),
            0.6 + t * (0.8 - 0.6),
            0.35 + t * (0.4 - 0.35),
        )
    } else if p < 1200.0 {
        let t = (p - 600.0) / 600.0;
        (
            0.85 + t * (0.45 - 0.85),
            0.8 + t * (0.75 - 0.8),
            0.4 + t * (0.35 - 0.4),
        )
    } else if p < 2000.0 {
        let t = (p - 1200.0) / 800.0;
        (
            0.45 + t * (0.2 - 0.45),
            0.75 + t * (0.6 - 0.75),
            0.35 + t * (0.4 - 0.35),
        )
    } else {
        let t = (p - 2000.0) / 500.0;
        (
            0.2 + t * (0.1 - 0.2),
            0.6 + t * (0.4 - 0.6),
            0.4 + t * (0.55 - 0.4),
        )
    };

    Color::srgb(r, g, b)
}

/// Resolves fill color for a hex under the active render mode.
pub fn hex_color_for_mode(
    data: &WorldData,
    hex_idx: usize,
    mode: RenderMode,
    is_pentagon: bool,
    biology: Option<&dyn genesis_core::biology_view::BiologyView>,
) -> Color {
    let _elev = data.elevation_mean[hex_idx];
    let _sea_level = data.sea_level_m;

    match mode {
        RenderMode::Elevation => water_aware_elevation_color(data, hex_idx, is_pentagon),
        RenderMode::Temperature => {
            // Ice/permafrost reads as white here (its natural home) instead of on
            // the elevation map. Sea ice and glaciated land both show frozen.
            let flags = data
                .hydro_flags
                .get(hex_idx)
                .copied()
                .unwrap_or(HydroFlags::NONE);
            if data.ice_mask.get(hex_idx).copied().unwrap_or(false)
                || flags.contains(HydroFlags::SEA_ICE)
            {
                Color::srgb(0.95, 0.97, 1.0)
            } else {
                temperature_to_color(data.temperature_mean[hex_idx])
            }
        }
        RenderMode::Precipitation => precipitation_to_color(data.precipitation[hex_idx]),
        RenderMode::ClimateRegime => {
            // Water and ice get ocean/ice colors so the regime view stays
            // continent-vs-ocean legible; the land-regime palette (which the
            // classifier also assigns to ocean cells) only applies on dry land.
            let flags = data
                .hydro_flags
                .get(hex_idx)
                .copied()
                .unwrap_or(HydroFlags::NONE);
            let elev = data.elevation_mean[hex_idx];
            let water = data
                .water_level_m
                .get(hex_idx)
                .copied()
                .unwrap_or(WATER_NONE);
            if data.ice_mask.get(hex_idx).copied().unwrap_or(false)
                || flags.contains(HydroFlags::SEA_ICE)
            {
                Color::srgb(0.95, 0.97, 1.0)
            } else if water.is_finite() && water > elev {
                ocean_regime_color(data.temperature_mean[hex_idx])
            } else {
                regime_to_color(data.climate_regime[hex_idx])
            }
        }
        RenderMode::Soil => soil_to_color(data, hex_idx),
        RenderMode::Biome => {
            let biome = biology
                .map(|v| v.biome_at(data, genesis_core::HexId(hex_idx as u32)))
                .unwrap_or(genesis_core::data::BiomeId::NONE);
            biome_color(biome)
        }
        RenderMode::Biomass => {
            let v = biology
                .map(|b| b.biomass_at(data, genesis_core::HexId(hex_idx as u32)))
                .unwrap_or(0.0);
            heatmap_color(v)
        }
        RenderMode::Diversity => {
            let v = biology
                .map(|b| b.richness_at(data, genesis_core::HexId(hex_idx as u32)))
                .unwrap_or(0.0);
            heatmap_color(v)
        }
        // Civilization placeholder (Doc 10). Oceans still render as water so
        // land/sea stays legible (and rivers overlay for future city sites);
        // land is a flat neutral until civ is simulated.
        RenderMode::Society => {
            let elev = data.elevation_mean[hex_idx];
            let water = data
                .water_level_m
                .get(hex_idx)
                .copied()
                .unwrap_or(WATER_NONE);
            if water.is_finite() && water > elev {
                water_depth_color(water - elev)
            } else {
                Color::srgb(0.32, 0.32, 0.36)
            }
        }
    }
}

/// Categorical color for a stub biome id (Prep-09 §4.1). The index scheme
/// matches `genesis_ui::biology_view::STUB_BIOMES`; `BiomeId::NONE` is ocean.
pub fn biome_color(biome: genesis_core::data::BiomeId) -> Color {
    use genesis_core::data::BiomeId;
    if biome == BiomeId::NONE {
        return Color::srgb(0.12, 0.28, 0.48); // ocean
    }
    let (r, g, b) = match biome.0 {
        0 => (0.10, 0.42, 0.16),  // Tropical rainforest
        1 => (0.72, 0.70, 0.32),  // Tropical savanna
        2 => (0.85, 0.74, 0.45),  // Hot desert
        3 => (0.58, 0.58, 0.26),  // Mediterranean scrub
        4 => (0.22, 0.56, 0.26),  // Temperate forest
        5 => (0.74, 0.76, 0.42),  // Temperate grassland
        6 => (0.16, 0.46, 0.40),  // Boreal forest
        7 => (0.66, 0.68, 0.60),  // Tundra
        8 => (0.86, 0.89, 0.92),  // Polar desert
        9 => (0.24, 0.50, 0.46),  // Wetland
        10 => (0.60, 0.62, 0.66), // Alpine
        11 => (0.35, 0.66, 0.72), // Coastal shallows
        _ => (0.5, 0.5, 0.5),
    };
    Color::srgb(r, g, b)
}

/// Sequential heatmap for a normalized value ∈ [0,1] (biomass, diversity):
/// deep indigo → blue → teal-green → yellow (viridis-like), for latitudinal-
/// gradient legibility.
pub fn heatmap_color(v: f32) -> Color {
    const STOPS: [(f32, f32, f32); 5] = [
        (0.09, 0.05, 0.24),
        (0.15, 0.30, 0.54),
        (0.13, 0.55, 0.55),
        (0.42, 0.72, 0.28),
        (0.95, 0.90, 0.20),
    ];
    let v = v.clamp(0.0, 1.0) * (STOPS.len() - 1) as f32;
    let i = (v.floor() as usize).min(STOPS.len() - 2);
    let f = v - i as f32;
    let a = STOPS[i];
    let b = STOPS[i + 1];
    Color::srgb(
        a.0 + f * (b.0 - a.0),
        a.1 + f * (b.1 - a.1),
        a.2 + f * (b.2 - a.2),
    )
}

/// Doc 08 §12.1 water-aware terrain ramp.
fn water_aware_elevation_color(data: &WorldData, hex_idx: usize, is_pentagon: bool) -> Color {
    let elev = data.elevation_mean[hex_idx];
    let sea_level = data.sea_level_m;
    let flags = data
        .hydro_flags
        .get(hex_idx)
        .copied()
        .unwrap_or(HydroFlags::NONE);
    // Ice is NOT painted on the elevation map — this view is for land height and
    // water, and an ice cap would hide the terrain/ocean beneath it. Ice/permafrost
    // has its own white in the Temperature view.
    //
    // Standing water must win over salt-flat tint. Timeline scrubbing restores
    // `water_level_m` from HistoryFrame; salt on a currently wet hex is residue
    // under the water column, not a salt flat (§5.3 / §12.1).
    let water_level = data
        .water_level_m
        .get(hex_idx)
        .copied()
        .unwrap_or(WATER_NONE);
    if water_level > elev && water_level.is_finite() {
        let depth = (water_level - elev).max(0.0);
        let mut c = water_depth_color(depth);
        if flags.contains(HydroFlags::FJORD) {
            // Narrow deep-incursion cue: darker teal.
            c = Color::srgb(0.08, 0.22, 0.42);
        } else if flags.contains(HydroFlags::ESTUARY) {
            c = Color::srgb(0.25, 0.55, 0.55);
        }
        return c;
    }
    // Elevation tint only for true SaltFlat bodies — mild saline soil stays
    // on the hypsometric ramp and shows in Soil mode instead.
    let body_id = data
        .water_body_id
        .get(hex_idx)
        .copied()
        .unwrap_or(WaterBodyId::NONE);
    if body_id != WaterBodyId::NONE
        && data
            .water_bodies
            .get(&body_id)
            .is_some_and(|b| b.kind == WaterBodyKind::SaltFlat)
    {
        return Color::srgb(0.85, 0.82, 0.72);
    }
    if flags.contains(HydroFlags::OASIS) {
        return Color::srgb(0.25, 0.55, 0.35);
    }
    if use_formation_elevation_palette(data) {
        return formation_elevation_color(elev, sea_level);
    }
    hex_fill_color(elev, sea_level, is_pentagon)
}

fn water_depth_color(depth_m: f32) -> Color {
    // Shelf → abyss blue ramp.
    if depth_m < 50.0 {
        Color::srgb(0.35, 0.65, 0.75)
    } else if depth_m < 200.0 {
        Color::srgb(0.20, 0.45, 0.70)
    } else if depth_m < 2000.0 {
        Color::srgb(0.10, 0.28, 0.55)
    } else {
        Color::srgb(0.05, 0.12, 0.30)
    }
}

fn soil_to_color(data: &WorldData, hex_idx: usize) -> Color {
    if data
        .water_body_id
        .get(hex_idx)
        .copied()
        .unwrap_or(WaterBodyId::NONE)
        != WaterBodyId::NONE
    {
        return water_depth_color(10.0);
    }
    let fertility = data.soil_fertility.get(hex_idx).copied().unwrap_or(0.0);
    let class = data
        .soil_class
        .get(hex_idx)
        .copied()
        .unwrap_or(SoilClass::None);
    soil_class_color(class, fertility)
}

/// Color for a soil class at the given fertility (greener when fertile). Public
/// so the UI legend shows exactly the colors on the map — including the
/// purple-grey barren (`None`) and pink saline classes.
pub fn soil_class_color(class: genesis_core::data::SoilClass, fertility: f32) -> Color {
    use genesis_core::data::SoilClass;
    let (r, g, b) = match class {
        SoilClass::None => (0.35, 0.35, 0.35),
        SoilClass::Sandy => (0.75, 0.70, 0.45),
        SoilClass::Loamy => (0.40, 0.55, 0.25),
        SoilClass::Alluvial => (0.35, 0.50, 0.20),
        SoilClass::Loess => (0.70, 0.60, 0.35),
        SoilClass::Volcanic => (0.35, 0.25, 0.25),
        SoilClass::Calcareous => (0.70, 0.72, 0.55),
        SoilClass::Peaty => (0.30, 0.35, 0.22),
        SoilClass::Saline => (0.85, 0.82, 0.70),
    };
    Color::srgb(r * (1.0 - 0.3 * fertility), g * (0.7 + 0.3 * fertility), b)
}

/// Distinct fill per Köppen-like regime (Doc 07 §10), loosely following the
/// conventional Köppen map palette.
pub fn regime_to_color(regime: genesis_core::data::ClimateRegimePlaceholder) -> Color {
    use genesis_core::data::ClimateRegimePlaceholder as R;
    let (r, g, b) = match regime {
        R::Unset => (0.25, 0.25, 0.25),
        R::Tropical => (0.00, 0.35, 0.85),
        R::Subtropical => (0.25, 0.60, 0.95),
        R::HotDesert => (0.95, 0.35, 0.20),
        R::ColdDesert => (0.95, 0.65, 0.45),
        R::Mediterranean => (0.95, 0.85, 0.20),
        R::Temperate => (0.35, 0.75, 0.30),
        R::ContinentalCool => (0.15, 0.55, 0.35),
        R::Boreal => (0.40, 0.65, 0.75),
        R::Tundra => (0.70, 0.75, 0.80),
        R::Polar => (0.92, 0.94, 0.97),
    };
    Color::srgb(r, g, b)
}

/// Ocean cells in the climate-regime view: a cold-navy → warm-teal ramp so water
/// reads as one coherent, distinctly aquatic band instead of borrowing land-
/// regime colors. Domain ≈ −2…+30 °C sea-surface temperature.
fn ocean_regime_color(sea_surface_c: f32) -> Color {
    let t = ((sea_surface_c + 2.0) / 32.0).clamp(0.0, 1.0);
    Color::srgb(0.04 + t * 0.06, 0.20 + t * 0.45, 0.45 + t * 0.20)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-5;

    fn luminance(rgb: [f32; 3]) -> f32 {
        0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
    }

    fn color_to_rgb(c: Color) -> [f32; 3] {
        let [r, g, b, _] = c.to_srgba().to_f32_array();
        [r, g, b]
    }

    fn colors_approx_equal(a: Color, b: Color) {
        let ra = color_to_rgb(a);
        let rb = color_to_rgb(b);
        for i in 0..3 {
            assert!(
                (ra[i] - rb[i]).abs() < EPS,
                "channel {i}: {} vs {}",
                ra[i],
                rb[i]
            );
        }
    }

    #[test]
    fn elevation_color_is_deterministic() {
        let a = elevation_color(1200.0, 0.0);
        let b = elevation_color(1200.0, 0.0);
        colors_approx_equal(a, b);
    }

    #[test]
    fn deep_basin_darker_than_shallow_basin() {
        let deep = color_to_rgb(elevation_color(-8_000.0, 0.0));
        let shallow = color_to_rgb(elevation_color(-500.0, 0.0));
        assert!(
            luminance(deep) < luminance(shallow),
            "deep lum {} should be < shallow lum {}",
            luminance(deep),
            luminance(shallow)
        );
    }

    #[test]
    fn dry_ramp_has_no_water_blues() {
        // Surface water is gone until Doc 08: no elevation may render blue.
        for e in (-11_000..9_000).step_by(250) {
            let [r, g, b] = color_to_rgb(elevation_color(e as f32, 0.0));
            assert!(
                b <= r + 0.10 || b <= g + 0.05,
                "elevation {e} renders bluish: r={r} g={g} b={b}"
            );
        }
    }

    #[test]
    fn land_greener_than_basin_floor() {
        let basin = color_to_rgb(elevation_color(-200.0, 0.0));
        let land = color_to_rgb(elevation_color(500.0, 0.0));
        assert!(
            land[1] > basin[1],
            "land should be greener: basin {:?} land {:?}",
            basin,
            land
        );
    }

    #[test]
    fn peak_lighter_than_lowland() {
        let sea = 0.0_f32;
        let lowland = color_to_rgb(elevation_color(200.0, sea));
        let peak = color_to_rgb(elevation_color(8_000.0, sea));
        assert!(
            luminance(peak) > luminance(lowland),
            "peak lum {} should exceed lowland lum {}",
            luminance(peak),
            luminance(lowland)
        );
    }

    #[test]
    fn pentagon_uses_same_elevation_ramp() {
        let elev = 1500.0_f32;
        let sea = 12.0_f32;
        let hex = hex_fill_color(elev, sea, false);
        let pent = hex_fill_color(elev, sea, true);
        colors_approx_equal(hex, pent);
    }

    #[test]
    fn clamped_below_min_elevation() {
        let at_min = elevation_color(MIN_ELEVATION_M, 0.0);
        let below = elevation_color(MIN_ELEVATION_M - 5_000.0, 0.0);
        colors_approx_equal(at_min, below);
    }

    #[test]
    fn temperature_color_cold_is_bluer_than_hot() {
        let cold = color_to_rgb(temperature_to_color(-40.0));
        let hot = color_to_rgb(temperature_to_color(35.0));
        assert!(cold[2] > hot[2], "cold should be bluer");
        assert!(hot[0] > cold[0], "hot should be redder");
    }

    #[test]
    fn temperature_color_is_deterministic() {
        colors_approx_equal(temperature_to_color(15.0), temperature_to_color(15.0));
    }

    #[test]
    fn precipitation_color_zero_is_dark_brown() {
        let rgb = color_to_rgb(precipitation_to_color(0.0));
        assert!(rgb[0] > rgb[2], "desert should be browner than blue");
        assert!(rgb[0] >= 0.45 && rgb[0] <= 0.55);
        assert!(rgb[1] >= 0.35 && rgb[1] <= 0.45);
    }

    #[test]
    fn precipitation_color_earth_average_is_moderate_green() {
        let rgb = color_to_rgb(precipitation_to_color(800.0));
        assert!(
            rgb[1] > rgb[0] && rgb[1] > rgb[2],
            "800mm should read as moderate green, got {:?}",
            rgb
        );
        assert!(rgb[1] >= 0.7, "green channel should be strong at 800mm");
    }

    #[test]
    fn precipitation_color_max_is_dark_blue_green() {
        let rgb = color_to_rgb(precipitation_to_color(2500.0));
        assert!(rgb[2] > rgb[0], "rainforest should be bluer than red");
        assert!(rgb[1] >= 0.35 && rgb[1] <= 0.45);
        assert!(rgb[2] >= 0.5);
    }

    #[test]
    fn precipitation_color_progression_dry_to_wet() {
        let dry = color_to_rgb(precipitation_to_color(0.0));
        let semi_arid = color_to_rgb(precipitation_to_color(400.0));
        let moderate = color_to_rgb(precipitation_to_color(800.0));
        let rainforest = color_to_rgb(precipitation_to_color(2500.0));

        assert!(dry[0] > dry[2], "desert should be brown, not blue");
        assert!(
            moderate[1] > moderate[0],
            "Earth-average should be green-dominant"
        );
        assert!(moderate[1] > dry[1], "800mm should be greener than desert");
        assert!(
            semi_arid[0] > moderate[0],
            "semi-arid should be redder than moderate"
        );
        assert!(
            rainforest[2] > moderate[2],
            "rainforest should be bluer than moderate"
        );
        assert!(
            dry[0] > rainforest[0],
            "desert should be redder than rainforest"
        );
    }

    #[test]
    fn precipitation_color_is_deterministic() {
        colors_approx_equal(precipitation_to_color(800.0), precipitation_to_color(800.0));
    }

    #[test]
    fn formation_lowland_is_not_grass_green() {
        use genesis_core::parameters::WorldParameters;
        use genesis_core::{HexGrid, WorldYear};

        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let grid = HexGrid::new(5, 6371.0).expect("grid");
        let mut data = WorldData::new(grid, params);
        data.current_year = WorldYear(100_000_000);
        data.elevation_mean[0] = 200.0;
        data.water_level_m[0] = WATER_NONE;
        data.sea_level_m = -3000.0;

        let formation = color_to_rgb(hex_color_for_mode(
            &data,
            0,
            RenderMode::Elevation,
            false,
            None,
        ));
        let modern = color_to_rgb(elevation_color(200.0, 0.0));
        assert!(
            modern[1] > modern[0] && modern[1] > modern[2],
            "modern +200 m should be grass-green dominant, got {modern:?}"
        );
        assert!(
            !(formation[1] > formation[0] + 0.05 && formation[1] > formation[2] + 0.05),
            "Formation +200 m must not read as grass green, got {formation:?}"
        );

        data.current_year = WorldYear(FORMATION_END_YEAR);
        let post = color_to_rgb(hex_color_for_mode(
            &data,
            0,
            RenderMode::Elevation,
            false,
            None,
        ));
        colors_approx_equal(
            Color::srgb(post[0], post[1], post[2]),
            elevation_color(200.0, data.sea_level_m),
        );
    }

    #[test]
    fn freeboard_above_deep_negative_sea_reads_green() {
        let sea = -2000.0_f32;
        let freeboard = sea + 800.0;
        let rgb = color_to_rgb(elevation_color(freeboard, sea));
        assert!(
            rgb[1] > rgb[0] && rgb[1] > rgb[2] * 0.9,
            "sea-relative freeboard (+800) must read green/olive, got {rgb:?} at sea={sea}"
        );
        let dry_pit = color_to_rgb(elevation_color(sea - 2000.0, sea));
        assert!(
            dry_pit[0] < 0.4 && dry_pit[1] < 0.4,
            "dry pit 2000 m below sea must stay dark, got {dry_pit:?}"
        );
    }

    #[test]
    fn skip_formation_uses_modern_palette_at_year_zero() {
        use genesis_core::parameters::WorldParameters;
        use genesis_core::{HexGrid, WorldYear};

        let mut params = WorldParameters::default();
        params.core.climate.skip_planetary_formation = true;
        params.core.grid.subdivision_level = 5;
        let grid = HexGrid::new(5, 6371.0).expect("grid");
        let mut data = WorldData::new(grid, params);
        data.current_year = WorldYear(0);
        data.elevation_mean[0] = 200.0;
        data.water_level_m[0] = WATER_NONE;

        let rgb = color_to_rgb(hex_color_for_mode(
            &data,
            0,
            RenderMode::Elevation,
            false,
            None,
        ));
        let modern = color_to_rgb(elevation_color(200.0, 0.0));
        for i in 0..3 {
            assert!((rgb[i] - modern[i]).abs() < EPS);
        }
    }

    #[test]
    fn standing_water_beats_salt_flat_tint() {
        use genesis_core::parameters::WorldParameters;
        use genesis_core::{HexGrid, WorldYear};

        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let grid = HexGrid::new(5, 6371.0).expect("grid");
        let mut data = WorldData::new(grid, params);
        data.current_year = WorldYear(864_000_000);
        data.elevation_mean[0] = -2000.0;
        data.water_level_m[0] = -500.0;
        data.salt_accumulated[0] = 5.0;
        data.water_body_id[0] = WaterBodyId::NONE;
        data.sea_level_m = -500.0;

        let color = hex_color_for_mode(&data, 0, RenderMode::Elevation, false, None);
        let rgb = color_to_rgb(color);
        assert!(
            rgb[2] > rgb[0] + 0.05 && rgb[2] > rgb[1],
            "wet+salt with NONE body id must render water blue, got {rgb:?}"
        );
    }

    #[test]
    fn dry_salt_flat_stays_pale_tan() {
        use genesis_core::data::WaterBody;
        use genesis_core::parameters::WorldParameters;
        use genesis_core::{HexGrid, WorldYear};

        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let grid = HexGrid::new(5, 6371.0).expect("grid");
        let mut data = WorldData::new(grid, params);
        data.current_year = WorldYear(864_000_000);
        data.elevation_mean[0] = 100.0;
        data.water_level_m[0] = WATER_NONE;
        data.salt_accumulated[0] = 5.0;
        let id = WaterBodyId(0);
        data.water_body_id[0] = id;
        data.water_bodies.insert(
            id,
            WaterBody {
                id,
                kind: WaterBodyKind::SaltFlat,
                surface_m: 100.0,
                area_km2: 0.0,
                volume_km3: 0.0,
                salinity: 0.0,
                outlet: None,
            },
        );
        data.sea_level_m = 0.0;

        let color = hex_color_for_mode(&data, 0, RenderMode::Elevation, false, None);
        let rgb = color_to_rgb(color);
        assert!(
            (rgb[0] - 0.85).abs() < 0.05 && (rgb[1] - 0.82).abs() < 0.05,
            "SaltFlat body must stay pale tan, got {rgb:?}"
        );
    }

    #[test]
    fn residual_salt_without_flat_uses_terrain_ramp() {
        use genesis_core::parameters::WorldParameters;
        use genesis_core::{HexGrid, WorldYear};

        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let grid = HexGrid::new(5, 6371.0).expect("grid");
        let mut data = WorldData::new(grid, params);
        data.current_year = WorldYear(864_000_000);
        data.elevation_mean[0] = 100.0;
        data.water_level_m[0] = WATER_NONE;
        data.salt_accumulated[0] = 5.0;
        data.water_body_id[0] = WaterBodyId::NONE;
        data.sea_level_m = 0.0;

        let color = hex_color_for_mode(&data, 0, RenderMode::Elevation, false, None);
        let rgb = color_to_rgb(color);
        let ramp = color_to_rgb(elevation_color(100.0, 0.0));
        for i in 0..3 {
            assert!(
                (rgb[i] - ramp[i]).abs() < EPS,
                "residual salt without SaltFlat must use terrain ramp, got {rgb:?} vs {ramp:?}"
            );
        }
    }
}
