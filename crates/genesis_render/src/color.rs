//! Hex coloring for elevation and climate visualization.
//!
//! Elevation colors follow Doc 06 §15 step 10. Temperature and precipitation
//! ramps support multi-mode rendering (Doc 07 fields).

use bevy::prelude::*;
use genesis_core::data::WorldData;

use crate::render_mode::RenderMode;

/// Uniform ocean color for climate modes (keeps land patterns readable).
pub const OCEAN_BASELINE_COLOR: Color = Color::srgb(0.1, 0.3, 0.5);

/// Matches tectonics clamp (Doc 06 §5.7); local to render — no `genesis_tectonics` dependency.
pub const MIN_ELEVATION_M: f32 = -11_000.0;
pub const MAX_ELEVATION_M: f32 = 9_000.0;

struct ColorStop {
    /// Absolute elevation in meters (may equal `sea_level_m` for coast stops).
    elev_m: f32,
    rgb: [f32; 3],
}

/// Piecewise-linear ramp in absolute elevation; `sea_level_m` anchors shallow water and coast.
fn elevation_stops(sea_level_m: f32) -> Vec<ColorStop> {
    vec![
        ColorStop {
            elev_m: MIN_ELEVATION_M,
            rgb: [0.02, 0.05, 0.28],
        },
        ColorStop {
            elev_m: -4_000.0,
            rgb: [0.05, 0.12, 0.42],
        },
        ColorStop {
            elev_m: sea_level_m - 50.0,
            rgb: [0.12, 0.35, 0.55],
        },
        ColorStop {
            elev_m: sea_level_m,
            rgb: [0.22, 0.48, 0.62],
        },
        ColorStop {
            elev_m: sea_level_m + 10.0,
            rgb: [0.25, 0.45, 0.20],
        },
        ColorStop {
            elev_m: sea_level_m + 1_500.0,
            rgb: [0.45, 0.42, 0.28],
        },
        ColorStop {
            elev_m: sea_level_m + 4_000.0,
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
pub fn elevation_color(elevation_m: f32, sea_level_m: f32) -> Color {
    let elev = clamp_elevation(elevation_m);
    let stops = elevation_stops(sea_level_m);
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
    let elev = data.elevation_mean[hex_idx];
    let sea_level = data.sea_level_m;

    match mode {
        RenderMode::Elevation => hex_fill_color(elev, sea_level, is_pentagon),
        RenderMode::Temperature => {
            if elev < sea_level {
                OCEAN_BASELINE_COLOR
            } else {
                temperature_to_color(data.temperature_mean[hex_idx])
            }
        }
        RenderMode::Precipitation => {
            if elev < sea_level {
                OCEAN_BASELINE_COLOR
            } else {
                precipitation_to_color(data.precipitation[hex_idx])
            }
        }
        RenderMode::ClimateRegime => {
            if elev < sea_level {
                OCEAN_BASELINE_COLOR
            } else {
                regime_to_color(data.climate_regime[hex_idx])
            }
        }
        RenderMode::Rivers => {
            if elev < sea_level {
                OCEAN_BASELINE_COLOR
            } else {
                flow_volume_to_color(data.flow_volume[hex_idx])
            }
        }
    }
}

/// Land fill by accumulated discharge: pale ground through deepening blue as
/// log10(flow m³/year) rises. Major rivers (Mississippi-scale, ~1e11 m³/yr)
/// read as saturated blue channels.
pub fn flow_volume_to_color(flow_m3_per_year: f32) -> Color {
    let ground = (0.82, 0.78, 0.68);
    if flow_m3_per_year <= 0.0 {
        return Color::srgb(ground.0, ground.1, ground.2);
    }
    // Typical single-hex runoff is ~1e9–1e10 m³/yr at level 7; treat volumes a
    // decade above local runoff as river-bearing and saturate two decades up.
    let log = flow_m3_per_year.max(1.0).log10();
    let t = ((log - 10.0) / 2.0).clamp(0.0, 1.0);
    let river = (0.05, 0.25, 0.75);
    Color::srgb(
        ground.0 + (river.0 - ground.0) * t,
        ground.1 + (river.1 - ground.1) * t,
        ground.2 + (river.2 - ground.2) * t,
    )
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
    fn deep_ocean_darker_than_shallow_submerged() {
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
    fn land_greener_than_ocean_at_same_sea_level() {
        let sea = 0.0_f32;
        let ocean = color_to_rgb(elevation_color(-200.0, sea));
        let land = color_to_rgb(elevation_color(500.0, sea));
        assert!(
            land[1] > ocean[1],
            "land should be greener: ocean {:?} land {:?}",
            ocean,
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
