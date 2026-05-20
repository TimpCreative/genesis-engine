//! Deterministic elevation-based hex coloring (Doc 06 §15 step 10).
//!
//! Colors are derived only from `elevation_m` and `sea_level_m` — no `HexId` hue.
//! Pentagons use the same ramp as hexes; five-sided geometry distinguishes them.

use bevy::prelude::*;

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
}
