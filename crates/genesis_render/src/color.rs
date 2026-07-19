//! Hex coloring for elevation, climate, water, and soil visualization.
//!
//! Elevation colors follow Doc 06 §15 / Doc 08 §12.1: wet hexes
//! (`water_level_m > elevation_mean`) render depth-tinted water; ice is white;
//! salt flats pale; land uses the terrain ramp.

use bevy::prelude::*;
use genesis_core::data::{HydroFlags, SoilClass, WATER_NONE, WaterBodyId, WorldData};

use crate::render_mode::RenderMode;

/// Matches tectonics clamp (Doc 06 §5.7); local to render — no `genesis_tectonics` dependency.
pub const MIN_ELEVATION_M: f32 = -11_000.0;
pub const MAX_ELEVATION_M: f32 = 9_000.0;

struct ColorStop {
    /// Absolute elevation in meters.
    elev_m: f32,
    rgb: [f32; 3],
}

/// Piecewise-linear ramp in absolute elevation. The whole planet renders as
/// terrain: deep basins read as dark rock, the 0 m datum is a subtle coast
/// cue, and land runs plain → lowland → piedmont → upland → tan → brown →
/// white. No water colors anywhere (Doc 08 will reintroduce them properly).
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

/// Pure elevation → color mapping for terrain visualization.
///
/// `_sea_level_m` is retained for the Doc 08 water re-introduction; the dry
/// ramp does not use it.
pub fn elevation_color(elevation_m: f32, _sea_level_m: f32) -> Color {
    let elev = clamp_elevation(elevation_m);
    let stops = elevation_stops();
    let rgb = sample_stops(&stops, elev);
    Color::srgb(rgb[0], rgb[1], rgb[2])
}

/// Fill color for a hex or pentagon cell (same ramp; geometry distinguishes pentagons).
pub fn hex_fill_color(elevation_m: f32, sea_level_m: f32, _is_pentagon: bool) -> Color {
    elevation_color(elevation_m, sea_level_m)
}

/// Maps temperature in °C to a color: blue (cold) → green (mild) → yellow (warm) → red (hot).
pub fn temperature_to_color(temp_c: f32) -> Color {
    let normalized = ((temp_c + 40.0) / 75.0).clamp(0.0, 1.0);

    let (r, g, b) = if normalized < 0.25 {
        let t = normalized / 0.25;
        (
            0.1 + t * (0.4 - 0.1),
            0.1 + t * (0.6 - 0.1),
            0.6 + t * (0.95 - 0.6),
        )
    } else if normalized < 0.5 {
        let t = (normalized - 0.25) / 0.25;
        (0.4, 0.6 + t * (0.85 - 0.6), 0.95 + t * (0.4 - 0.95))
    } else if normalized < 0.75 {
        let t = (normalized - 0.5) / 0.25;
        (
            0.4 + t * (0.95 - 0.4),
            0.85 + t * (0.9 - 0.85),
            0.4 + t * (0.2 - 0.4),
        )
    } else {
        let t = (normalized - 0.75) / 0.25;
        (
            0.95 + t * (0.85 - 0.95),
            0.9 + t * (0.2 - 0.9),
            0.2 + t * (0.15 - 0.2),
        )
    };

    Color::srgb(r, g, b)
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
) -> Color {
    let _elev = data.elevation_mean[hex_idx];
    let _sea_level = data.sea_level_m;

    match mode {
        RenderMode::Elevation => water_aware_elevation_color(data, hex_idx, is_pentagon),
        RenderMode::Temperature => temperature_to_color(data.temperature_mean[hex_idx]),
        RenderMode::Precipitation => precipitation_to_color(data.precipitation[hex_idx]),
        RenderMode::ClimateRegime => regime_to_color(data.climate_regime[hex_idx]),
        RenderMode::Soil => soil_to_color(data, hex_idx),
    }
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
    if data.ice_mask.get(hex_idx).copied().unwrap_or(false) || flags.contains(HydroFlags::SEA_ICE) {
        return Color::srgb(0.92, 0.95, 0.98);
    }
    if data.salt_accumulated.get(hex_idx).copied().unwrap_or(0.0) > 0.0
        && data
            .water_body_id
            .get(hex_idx)
            .copied()
            .unwrap_or(WaterBodyId::NONE)
            == WaterBodyId::NONE
    {
        return Color::srgb(0.85, 0.82, 0.72);
    }
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
    if flags.contains(HydroFlags::OASIS) {
        return Color::srgb(0.25, 0.55, 0.35);
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
    // Tint toward fertility (greener when fertile).
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
}
