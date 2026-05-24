//! Per-hex temperature computation (Doc 07 §4).
//!
//! Implements the temperature model: latitude baseline, elevation lapse,
//! continentality, ocean current placeholder, glaciation, and greenhouse forcing.
//! Also computes per-hex seasonal range (`temperature_range`).

use genesis_core::HexId;
use genesis_core::data::WorldData;

use crate::state::{AtmosphericComposition, ClimateState, GlaciationState};

/// Equatorial baseline temperature in °C before atmospheric and elevation adjustments.
pub const T_EQUATOR_BASELINE_C: f32 = 30.0;

/// Atmospheric lapse rate: temperature drop per meter of elevation gain.
pub const LAPSE_RATE_C_PER_M: f32 = 6.5e-3;

/// Continentality offset: continental interiors are slightly cooler on average.
pub const CONTINENTALITY_MAX_OFFSET_C: f32 = 3.0;

/// Distance (km) at which continentality has reached half of its full effect.
pub const CONTINENTALITY_HALF_DISTANCE_KM: f32 = 500.0;

/// Glaciation amplitude: maximum cooling at poles when fully glacial.
pub const GLACIATION_MAX_COOLING_C: f32 = 10.0;

/// Computes per-hex temperature for every hex on the planet.
///
/// Writes `WorldData.temperature_mean` and `WorldData.temperature_range`.
pub fn compute_temperature_field(data: &mut WorldData, climate: &ClimateState) {
    let n = data.cell_count() as usize;
    let grid = &data.grid;

    let axial_tilt_rad = f64::from(data.parameters.core.planet.axial_tilt_degrees).to_radians();
    let solar_luminosity = data.parameters.core.planet.solar_luminosity_relative_to_sol;
    let composition = &climate.atmospheric_composition;
    let glaciation = climate.glaciation;

    let p = latitude_exponent(axial_tilt_rad);

    for i in 0..n {
        let hex = HexId(i as u32);
        let (lat_rad, _lon_rad) = grid.center_lat_lon(hex);
        let elevation_m = data.elevation_mean[i];
        let distance_km = data.distance_to_ocean_km[i];

        let t_baseline = baseline_temperature_c(lat_rad, p, solar_luminosity, composition);
        let t_elevation = elevation_adjustment_c(elevation_m, data.sea_level_m);
        let t_continentality = continentality_adjustment_c(distance_km);
        let t_ocean_current = 0.0_f32;
        let t_glaciation = glaciation_adjustment_c(lat_rad, glaciation);
        let t_greenhouse = composition.greenhouse_forcing;

        let temperature = t_baseline
            + t_elevation
            + t_continentality
            + t_ocean_current
            + t_glaciation
            + t_greenhouse;

        data.temperature_mean[i] = temperature;
        data.temperature_range[i] = seasonal_range_c(lat_rad, distance_km, axial_tilt_rad);
    }

    let coastal_adjustments = crate::ocean_currents::compute_coastal_temperature_adjustments(data);
    for (hex, adjustment) in coastal_adjustments {
        data.temperature_mean[hex.0 as usize] += adjustment;
    }
}

fn latitude_exponent(axial_tilt_rad: f64) -> f32 {
    let earth_tilt = 23.44_f64.to_radians();
    let ratio = axial_tilt_rad / earth_tilt;
    (1.0 / ratio.max(0.1)) as f32
}

fn baseline_temperature_c(
    lat_rad: f64,
    p: f32,
    solar_luminosity: f32,
    _composition: &AtmosphericComposition,
) -> f32 {
    let t_equator = T_EQUATOR_BASELINE_C * solar_luminosity;

    let cos_lat = lat_rad.cos().abs() as f32;
    let factor = cos_lat.powf(p);

    let pole_baseline = -40.0_f32 * solar_luminosity;
    pole_baseline + (t_equator - pole_baseline) * factor
}

fn elevation_adjustment_c(elevation_m: f32, sea_level_m: f32) -> f32 {
    if elevation_m < sea_level_m {
        0.0
    } else {
        -LAPSE_RATE_C_PER_M * (elevation_m - sea_level_m)
    }
}

fn continentality_adjustment_c(distance_km: f32) -> f32 {
    if !distance_km.is_finite() {
        return -CONTINENTALITY_MAX_OFFSET_C;
    }
    let t = distance_km / CONTINENTALITY_HALF_DISTANCE_KM;
    let factor = t / (1.0 + t);
    -CONTINENTALITY_MAX_OFFSET_C * factor
}

fn glaciation_adjustment_c(lat_rad: f64, glaciation: GlaciationState) -> f32 {
    let intensity = match glaciation {
        GlaciationState::Interglacial => 0.0,
        GlaciationState::Transition => 0.3,
        GlaciationState::Glacial => 1.0,
    };
    if intensity == 0.0 {
        return 0.0;
    }
    let lat_factor = (1.0 - lat_rad.cos().abs()) as f32;
    -GLACIATION_MAX_COOLING_C * intensity * lat_factor
}

fn seasonal_range_c(lat_rad: f64, distance_km: f32, axial_tilt_rad: f64) -> f32 {
    let abs_lat_deg = lat_rad.abs().to_degrees() as f32;
    let base_range = if abs_lat_deg < 10.0 {
        5.0
    } else if abs_lat_deg < 30.0 {
        5.0 + (abs_lat_deg - 10.0) * 0.5
    } else if abs_lat_deg < 60.0 {
        15.0 + (abs_lat_deg - 30.0) * (20.0 / 30.0)
    } else {
        35.0 + ((abs_lat_deg - 60.0) / 30.0).clamp(0.0, 1.0) * 5.0
    };

    let continental_factor = if distance_km.is_finite() {
        let t = distance_km / CONTINENTALITY_HALF_DISTANCE_KM;
        1.0 + (t / (1.0 + t)) * 1.5
    } else {
        2.5
    };

    let tilt_factor = (axial_tilt_rad.to_degrees() as f32 / 23.44).max(0.1);

    base_range * continental_factor * tilt_factor
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

    fn earth_like_climate_state() -> ClimateState {
        let mut state = ClimateState::default();
        state.atmospheric_composition = AtmosphericComposition {
            co2_ppm: 280.0,
            water_vapor_index: 0.4,
            oxygen_fraction: 0.21,
            greenhouse_forcing: 0.0,
        };
        state.glaciation = GlaciationState::Interglacial;
        state
    }

    #[test]
    fn equator_warmer_than_poles() {
        let mut world = world_at_level(5);
        let climate = earth_like_climate_state();

        for i in 0..world.data.cell_count() as usize {
            world.data.elevation_mean[i] = 0.0;
            world.data.distance_to_ocean_km[i] = 0.0;
        }
        world.data.sea_level_m = 0.0;

        compute_temperature_field(&mut world.data, &climate);

        let mut equator_temp = None;
        let mut polar_temp = None;
        for i in 0..world.data.cell_count() as usize {
            let (lat, _) = world.data.grid.center_lat_lon(HexId(i as u32));
            let abs_lat_deg = lat.abs().to_degrees();
            if abs_lat_deg < 5.0 && equator_temp.is_none() {
                equator_temp = Some(world.data.temperature_mean[i]);
            }
            if abs_lat_deg > 70.0 && polar_temp.is_none() {
                polar_temp = Some(world.data.temperature_mean[i]);
            }
        }

        let eq = equator_temp.expect("found equator hex");
        let pole = polar_temp.expect("found polar hex");
        assert!(
            eq > pole + 30.0,
            "equator ({eq}°C) should be much warmer than pole ({pole}°C)"
        );
    }

    #[test]
    fn elevation_cools_hex() {
        let mut world = world_at_level(5);
        let climate = earth_like_climate_state();

        for i in 0..world.data.cell_count() as usize {
            world.data.elevation_mean[i] = if i == 0 { 5000.0 } else { 0.0 };
            world.data.distance_to_ocean_km[i] = 0.0;
        }
        world.data.sea_level_m = 0.0;

        compute_temperature_field(&mut world.data, &climate);

        let (lat_0, _) = world.data.grid.center_lat_lon(HexId(0));
        let mut similar_temp = None;
        for i in 1..world.data.cell_count() as usize {
            let (lat_i, _) = world.data.grid.center_lat_lon(HexId(i as u32));
            if (lat_i - lat_0).abs() < 0.1 {
                similar_temp = Some(world.data.temperature_mean[i]);
                break;
            }
        }

        let high_alt_temp = world.data.temperature_mean[0];
        if let Some(low_alt) = similar_temp {
            assert!(
                low_alt - high_alt_temp > 25.0,
                "5000m elevation ({high_alt_temp}°C) should be ~32.5°C cooler than sea level ({low_alt}°C)"
            );
        }
    }

    #[test]
    fn continental_interior_cooler_than_coast() {
        let coast = continentality_adjustment_c(0.0);
        let interior = continentality_adjustment_c(2000.0);
        assert!(interior < coast, "interior should be cooler");
        assert!(coast >= -0.5, "coastal effect should be near zero");
        assert!(interior < -2.0, "interior effect should be substantial");
    }

    #[test]
    fn no_ocean_world_has_max_continentality() {
        let inf = continentality_adjustment_c(f32::INFINITY);
        assert_eq!(inf, -CONTINENTALITY_MAX_OFFSET_C);
    }

    #[test]
    fn glaciation_cools_poles_more_than_equator() {
        let pole_lat = std::f64::consts::FRAC_PI_2 - 0.1;
        let equator_lat = 0.0;

        let pole_cooling = glaciation_adjustment_c(pole_lat, GlaciationState::Glacial);
        let equator_cooling = glaciation_adjustment_c(equator_lat, GlaciationState::Glacial);

        assert!(pole_cooling < equator_cooling);
        assert!(equator_cooling.abs() < 0.5);
    }

    #[test]
    fn interglacial_has_no_cooling() {
        let any_lat = 0.5;
        assert_eq!(
            glaciation_adjustment_c(any_lat, GlaciationState::Interglacial),
            0.0
        );
    }

    #[test]
    fn greenhouse_warms_planet() {
        let mut world = world_at_level(5);
        let mut climate = earth_like_climate_state();

        for i in 0..world.data.cell_count() as usize {
            world.data.elevation_mean[i] = 0.0;
            world.data.distance_to_ocean_km[i] = 0.0;
        }

        compute_temperature_field(&mut world.data, &climate);
        let baseline_temp = world.data.temperature_mean[0];

        climate.atmospheric_composition.greenhouse_forcing = 10.0;
        compute_temperature_field(&mut world.data, &climate);
        let warmed_temp = world.data.temperature_mean[0];

        assert!((warmed_temp - baseline_temp - 10.0).abs() < 0.01);
    }

    #[test]
    fn seasonal_range_increases_toward_poles() {
        let tilt = 23.44_f64.to_radians();

        let equator_range = seasonal_range_c(0.0, 0.0, tilt);
        let mid_range = seasonal_range_c(0.7, 0.0, tilt);
        let polar_range = seasonal_range_c(1.3, 0.0, tilt);

        assert!(equator_range < mid_range);
        assert!(mid_range < polar_range);
    }

    #[test]
    fn seasonal_range_continental_amplification() {
        let tilt = 23.44_f64.to_radians();
        let lat = 0.7;

        let coast = seasonal_range_c(lat, 0.0, tilt);
        let interior = seasonal_range_c(lat, 2000.0, tilt);

        assert!(interior > coast * 1.5);
    }

    #[test]
    fn performance_at_level_7_under_50ms() {
        use std::time::Instant;

        let mut world = world_at_level(7);
        let climate = earth_like_climate_state();

        let start = Instant::now();
        compute_temperature_field(&mut world.data, &climate);
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 50,
            "temperature computation took {}ms at level 7; should be under 50ms",
            elapsed.as_millis()
        );
    }

    #[test]
    fn determinism() {
        let mut world_a = world_at_level(5);
        let mut world_b = world_at_level(5);
        let climate = earth_like_climate_state();

        compute_temperature_field(&mut world_a.data, &climate);
        compute_temperature_field(&mut world_b.data, &climate);

        assert_eq!(world_a.data.temperature_mean, world_b.data.temperature_mean);
        assert_eq!(
            world_a.data.temperature_range,
            world_b.data.temperature_range
        );
    }
}
