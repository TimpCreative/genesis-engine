//! Per-hex wind field computation (Doc 07 §7).
//!
//! Derives per-hex prevailing surface wind direction and speed from atmospheric
//! circulation cells. Reads `ClimateState.circulation_cells` (P2-4) and elevation
//! from `WorldData`.

use genesis_core::HexId;
use genesis_core::data::WorldData;

use crate::state::{CirculationCell, CirculationCells};

/// Base surface wind speed in m/s for a cell with intensity 1.0.
pub const WIND_BASE_SPEED_M_S: f32 = 8.0;

/// Elevation above which winds begin to slow due to surface friction effects.
pub const WIND_ELEVATION_DAMPING_START_M: f32 = 1000.0;

/// Elevation at which winds reach minimum damping (half of base speed).
pub const WIND_ELEVATION_DAMPING_FULL_M: f32 = 6000.0;

/// Computes the prevailing wind for every hex on the planet.
///
/// Per Doc 07 §7. Writes `WorldData.wind_direction_rad` and `WorldData.wind_speed_m_s`.
/// Writes `(0, 0)` if circulation cells aren't initialized yet.
pub fn compute_wind_field(data: &mut WorldData, cells: &CirculationCells) {
    let n = data.cell_count() as usize;

    if cells.cells.is_empty() {
        for i in 0..n {
            data.wind_direction_rad[i] = 0.0;
            data.wind_speed_m_s[i] = 0.0;
        }
        return;
    }

    let grid = &data.grid;

    for i in 0..n {
        let hex = HexId(i as u32);
        let (lat_rad, _lon_rad) = grid.center_lat_lon(hex);

        let Some(cell) = cells.cell_for_latitude(lat_rad) else {
            data.wind_direction_rad[i] = 0.0;
            data.wind_speed_m_s[i] = 0.0;
            continue;
        };

        let direction = cell_wind_direction_at_latitude(cell, lat_rad);
        let mut speed = WIND_BASE_SPEED_M_S * cell.intensity;

        let elevation = data.elevation_mean[i];
        speed *= elevation_damping_factor(elevation);

        data.wind_direction_rad[i] = direction;
        data.wind_speed_m_s[i] = speed;
    }
}

/// Returns the prevailing surface wind direction in radians (0=N, π/2=E,
/// π=S, 3π/2=W) for a given cell at a given latitude.
///
/// Directions are where the wind is blowing TOWARD (for downstream advection).
fn cell_wind_direction_at_latitude(cell: &CirculationCell, lat_rad: f64) -> f32 {
    use std::f64::consts::PI;

    let is_easterly_cell = cell.index.is_multiple_of(2);
    let is_northern = lat_rad >= 0.0;

    let direction = match (is_easterly_cell, is_northern) {
        (true, true) => PI + PI / 4.0,
        (false, true) => PI / 4.0,
        (true, false) => PI + 3.0 * PI / 4.0,
        (false, false) => 3.0 * PI / 4.0,
    };

    direction as f32
}

/// Returns a damping factor [0.5, 1.0] applied to wind speed based on elevation.
pub fn elevation_damping_factor(elevation_m: f32) -> f32 {
    if elevation_m <= WIND_ELEVATION_DAMPING_START_M {
        1.0
    } else if elevation_m >= WIND_ELEVATION_DAMPING_FULL_M {
        0.5
    } else {
        let t = (elevation_m - WIND_ELEVATION_DAMPING_START_M)
            / (WIND_ELEVATION_DAMPING_FULL_M - WIND_ELEVATION_DAMPING_START_M);
        1.0 - 0.5 * t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::create_world;
    use genesis_core::parameters::WorldParameters;

    fn world_at_level(level: u8) -> genesis_core::World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = level;
        create_world(params).expect("world")
    }

    fn earth_like_cells() -> CirculationCells {
        CirculationCells {
            cells_per_hemisphere: 3,
            cells: vec![
                CirculationCell {
                    index: 0,
                    lat_low_rad: 0.0,
                    lat_high_rad: std::f64::consts::FRAC_PI_6,
                    intensity: 1.0,
                },
                CirculationCell {
                    index: 1,
                    lat_low_rad: std::f64::consts::FRAC_PI_6,
                    lat_high_rad: std::f64::consts::FRAC_PI_3,
                    intensity: 1.0,
                },
                CirculationCell {
                    index: 2,
                    lat_low_rad: std::f64::consts::FRAC_PI_3,
                    lat_high_rad: std::f64::consts::FRAC_PI_2,
                    intensity: 1.0,
                },
            ],
            equator_pole_temp_diff_c: 50.0,
        }
    }

    #[test]
    fn empty_circulation_produces_zero_wind() {
        let mut world = world_at_level(5);
        let cells = CirculationCells::default();

        compute_wind_field(&mut world.data, &cells);

        for i in 0..world.data.cell_count() as usize {
            assert_eq!(world.data.wind_direction_rad[i], 0.0);
            assert_eq!(world.data.wind_speed_m_s[i], 0.0);
        }
    }

    #[test]
    fn wind_speed_scales_with_intensity() {
        let mut world = world_at_level(5);
        let mut cells = earth_like_cells();

        for i in 0..world.data.cell_count() as usize {
            world.data.elevation_mean[i] = 0.0;
        }

        compute_wind_field(&mut world.data, &cells);
        let speed_at_full_intensity: Vec<f32> = world.data.wind_speed_m_s.to_vec();

        for cell in cells.cells.iter_mut() {
            cell.intensity = 0.5;
        }
        compute_wind_field(&mut world.data, &cells);

        for (i, (full, half)) in speed_at_full_intensity
            .iter()
            .zip(world.data.wind_speed_m_s.iter())
            .enumerate()
        {
            if *full > 0.0 {
                let ratio = half / full;
                assert!(
                    (ratio - 0.5).abs() < 0.01,
                    "hex {i}: speed ratio {ratio} not 0.5"
                );
            }
        }
    }

    #[test]
    fn wind_speed_damped_at_high_elevation() {
        let mut world = world_at_level(5);
        let cells = earth_like_cells();

        for i in 0..world.data.cell_count() as usize {
            world.data.elevation_mean[i] = 6000.0;
        }
        compute_wind_field(&mut world.data, &cells);
        let high_alt_speed = world.data.wind_speed_m_s.clone();

        for i in 0..world.data.cell_count() as usize {
            world.data.elevation_mean[i] = 0.0;
        }
        compute_wind_field(&mut world.data, &cells);
        let sea_level_speed = world.data.wind_speed_m_s.clone();

        for (i, (high, low)) in high_alt_speed
            .iter()
            .zip(sea_level_speed.iter())
            .enumerate()
        {
            if *low > 0.0 {
                let ratio = high / low;
                assert!(
                    (ratio - 0.5).abs() < 0.01,
                    "hex {i}: damped ratio {ratio} expected ~0.5"
                );
            }
        }
    }

    #[test]
    fn northern_hadley_blows_toward_southwest() {
        let cell = CirculationCell {
            index: 0,
            lat_low_rad: 0.0,
            lat_high_rad: 0.5,
            intensity: 1.0,
        };
        let lat_rad = 0.2;
        let direction = cell_wind_direction_at_latitude(&cell, lat_rad);
        let expected = (std::f64::consts::PI + std::f64::consts::PI / 4.0) as f32;
        assert!((direction - expected).abs() < 0.001);
    }

    #[test]
    fn northern_ferrel_blows_toward_northeast() {
        let cell = CirculationCell {
            index: 1,
            lat_low_rad: 0.5,
            lat_high_rad: 1.0,
            intensity: 1.0,
        };
        let lat_rad = 0.7;
        let direction = cell_wind_direction_at_latitude(&cell, lat_rad);
        let expected = (std::f64::consts::PI / 4.0) as f32;
        assert!((direction - expected).abs() < 0.001);
    }

    #[test]
    fn southern_hemisphere_winds_mirror_north() {
        let cell = CirculationCell {
            index: 0,
            lat_low_rad: 0.0,
            lat_high_rad: 0.5,
            intensity: 1.0,
        };
        let lat_rad = -0.2;
        let direction = cell_wind_direction_at_latitude(&cell, lat_rad);
        let expected = (std::f64::consts::PI + 3.0 * std::f64::consts::PI / 4.0) as f32;
        assert!((direction - expected).abs() < 0.001);
    }

    #[test]
    fn elevation_damping_clamps() {
        assert_eq!(elevation_damping_factor(-1000.0), 1.0);
        assert_eq!(elevation_damping_factor(0.0), 1.0);
        assert_eq!(elevation_damping_factor(1000.0), 1.0);
        assert!((elevation_damping_factor(3500.0) - 0.75).abs() < 0.01);
        assert_eq!(elevation_damping_factor(6000.0), 0.5);
        assert_eq!(elevation_damping_factor(10000.0), 0.5);
    }

    #[test]
    fn performance_at_level_7_under_50ms() {
        use std::time::Instant;

        let mut world = world_at_level(7);
        let cells = earth_like_cells();

        let start = Instant::now();
        compute_wind_field(&mut world.data, &cells);
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 50,
            "wind computation took {}ms at level 7; should be under 50ms",
            elapsed.as_millis()
        );
    }

    #[test]
    fn determinism() {
        let mut world_a = world_at_level(5);
        let mut world_b = world_at_level(5);
        let cells = earth_like_cells();

        compute_wind_field(&mut world_a.data, &cells);
        compute_wind_field(&mut world_b.data, &cells);

        assert_eq!(
            world_a.data.wind_direction_rad,
            world_b.data.wind_direction_rad
        );
        assert_eq!(world_a.data.wind_speed_m_s, world_b.data.wind_speed_m_s);
    }
}
