//! Atmospheric circulation cell computation (Doc 07 §6).
//!
//! Derives the number, boundaries, and intensities of atmospheric circulation
//! cells from planetary rotation rate and current pole-to-equator temperature
//! gradient. Recomputed each climate tick (the gradient evolves during Formation
//! and over geological time as continents drift and atmospheric composition shifts).

use std::f64::consts::PI;

use genesis_core::data::WorldData;

use crate::state::{CirculationCell, CirculationCells};

/// Computes the number of circulation cells per hemisphere from rotation period.
///
/// Per Doc 07 §6.2:
/// - Earth-like (24h rotation): 3 cells
/// - Faster rotation: more cells (capped at 6)
/// - Slower rotation: fewer cells (minimum 1)
///
/// Formula: `cells = 3 / sqrt(rotation_hours / 24)`, clamped to [1, 6].
pub fn cells_per_hemisphere(rotation_period_hours: f64) -> u8 {
    if rotation_period_hours <= 0.0 || !rotation_period_hours.is_finite() {
        return 3;
    }
    let ratio = rotation_period_hours / 24.0;
    let cells_f = 3.0 / ratio.sqrt();
    cells_f.round().clamp(1.0, 6.0) as u8
}

/// Computes cell intensity from pole-equator temperature gradient.
///
/// Per Doc 07 §6.5. Higher gradient = stronger circulation.
pub fn cell_intensity(equator_pole_temp_diff_c: f32, rotation_factor: f32) -> f32 {
    let base = (equator_pole_temp_diff_c / 50.0).clamp(0.1, 2.0);
    base * rotation_factor
}

/// Estimates current pole-to-equator temperature gradient from the planet's
/// `global_temperature_c`. During Formation when the planet is uniformly hot,
/// the gradient is small. As the planet cools and atmospheric composition
/// stabilizes, the gradient grows toward Earth-like values (~50°C).
///
/// This is a simplified placeholder until P2-7 implements per-hex temperature
/// with full latitude variation.
///
/// Returns the gradient in °C.
pub fn estimate_equator_pole_gradient_c(global_temperature_c: f32) -> f32 {
    let from_equilibrium = (global_temperature_c - 15.0).abs();
    if from_equilibrium > 200.0 {
        5.0
    } else {
        let t = (200.0 - from_equilibrium) / 200.0;
        5.0 + t * 45.0
    }
}

/// Computes the full circulation cell configuration for the current world state.
///
/// Reads `world.parameters.core.planet.rotation_period_hours` and
/// `world.global_temperature_c`. Callers store the result in `ClimateState`.
pub fn compute_circulation(world: &WorldData) -> CirculationCells {
    let rotation_hours = world.parameters.core.planet.rotation_period_hours;
    let n_cells = cells_per_hemisphere(rotation_hours);

    let gradient_c = estimate_equator_pole_gradient_c(world.global_temperature_c);

    let rotation_factor = if rotation_hours < 24.0 {
        (rotation_hours / 24.0).powf(0.25) as f32
    } else {
        1.0
    };

    let intensity = cell_intensity(gradient_c, rotation_factor);

    let n = n_cells as usize;
    let half_pi = PI / 2.0;
    let mut cells = Vec::with_capacity(n);

    for k in 0..n {
        let lat_low = (k as f64) * half_pi / (n as f64);
        let lat_high = ((k + 1) as f64) * half_pi / (n as f64);
        cells.push(CirculationCell {
            index: k as u8,
            lat_low_rad: lat_low,
            lat_high_rad: lat_high,
            intensity,
        });
    }

    CirculationCells {
        cells_per_hemisphere: n_cells,
        cells,
        equator_pole_temp_diff_c: gradient_c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::create_world;
    use genesis_core::parameters::WorldParameters;

    #[test]
    fn earth_like_rotation_gives_three_cells() {
        assert_eq!(cells_per_hemisphere(24.0), 3);
    }

    #[test]
    fn very_fast_rotation_caps_at_six() {
        assert_eq!(cells_per_hemisphere(1.0), 6);
        assert_eq!(cells_per_hemisphere(6.0), 6);
    }

    #[test]
    fn very_slow_rotation_floors_at_one() {
        assert_eq!(cells_per_hemisphere(300.0), 1);
        assert_eq!(cells_per_hemisphere(10_000.0), 1);
    }

    #[test]
    fn cell_count_decreases_with_slower_rotation() {
        let counts: Vec<u8> = [12.0, 24.0, 48.0, 96.0, 192.0, 384.0]
            .iter()
            .map(|&h| cells_per_hemisphere(h))
            .collect();
        for w in counts.windows(2) {
            assert!(w[0] >= w[1], "cell count not monotonic: {counts:?}");
        }
    }

    #[test]
    fn cell_intensity_scales_with_gradient() {
        let low = cell_intensity(5.0, 1.0);
        let high = cell_intensity(50.0, 1.0);
        assert!(high > low);
    }

    #[test]
    fn cell_intensity_clamped() {
        let extreme = cell_intensity(500.0, 1.0);
        assert!(extreme <= 2.0);
        let tiny = cell_intensity(0.0, 1.0);
        assert!(tiny >= 0.1);
    }

    #[test]
    fn gradient_increases_as_planet_cools() {
        let hot = estimate_equator_pole_gradient_c(1500.0);
        let warm = estimate_equator_pole_gradient_c(50.0);
        let earth = estimate_equator_pole_gradient_c(15.0);
        assert!(warm > hot);
        assert!(earth > warm);
    }

    #[test]
    fn invalid_rotation_falls_back_to_earth_like() {
        assert_eq!(cells_per_hemisphere(0.0), 3);
        assert_eq!(cells_per_hemisphere(-10.0), 3);
        assert_eq!(cells_per_hemisphere(f64::NAN), 3);
    }

    #[test]
    fn cell_boundaries_cover_hemisphere() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let world = create_world(params).expect("world");

        let circulation = compute_circulation(&world.data);
        assert_eq!(circulation.cells_per_hemisphere, 3);

        assert!((circulation.cells[0].lat_low_rad - 0.0).abs() < 1e-9);

        let last = circulation.cells.last().unwrap();
        assert!((last.lat_high_rad - std::f64::consts::FRAC_PI_2).abs() < 1e-9);

        for w in circulation.cells.windows(2) {
            assert!((w[0].lat_high_rad - w[1].lat_low_rad).abs() < 1e-9);
        }
    }

    #[test]
    fn cell_for_latitude_finds_correct_cell() {
        let cells = CirculationCells {
            cells_per_hemisphere: 3,
            cells: vec![
                CirculationCell {
                    index: 0,
                    lat_low_rad: 0.0,
                    lat_high_rad: 0.5,
                    intensity: 1.0,
                },
                CirculationCell {
                    index: 1,
                    lat_low_rad: 0.5,
                    lat_high_rad: 1.0,
                    intensity: 1.0,
                },
                CirculationCell {
                    index: 2,
                    lat_low_rad: 1.0,
                    lat_high_rad: std::f64::consts::FRAC_PI_2,
                    intensity: 1.0,
                },
            ],
            equator_pole_temp_diff_c: 50.0,
        };

        assert_eq!(cells.cell_for_latitude(0.0).map(|c| c.index), Some(0));
        assert_eq!(cells.cell_for_latitude(0.7).map(|c| c.index), Some(1));
        assert_eq!(cells.cell_for_latitude(1.4).map(|c| c.index), Some(2));
        assert_eq!(cells.cell_for_latitude(-0.3).map(|c| c.index), Some(0));
    }
}
