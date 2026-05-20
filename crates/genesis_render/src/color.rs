//! Deterministic hex coloring for the Phase 0 smoke test.

use bevy::prelude::*;
use genesis_core::HexId;

/// Flat color for a hex: bright red for pentagons, golden-angle HSL for hexes.
pub fn hex_color(hex: HexId, is_pentagon: bool) -> Color {
    if is_pentagon {
        return Color::srgb(1.0, 0.1, 0.1);
    }
    let hue = (hex.0 as f32 * 137.508) % 360.0;
    Color::hsl(hue, 0.6, 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pentagon_is_bright_red() {
        let c = hex_color(HexId(0), true);
        let [r, g, b, _] = c.to_srgba().to_f32_array();
        assert!(r > 0.9 && g < 0.2 && b < 0.2);
    }

    #[test]
    fn hex_is_not_red() {
        let c = hex_color(HexId(12), false);
        let [r, g, b, _] = c.to_srgba().to_f32_array();
        assert!(g > 0.1 || b > 0.1 || r < 0.9);
    }
}
