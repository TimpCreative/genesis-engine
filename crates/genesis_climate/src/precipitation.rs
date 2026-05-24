//! Per-hex precipitation computation (Doc 07 §9).

use genesis_core::HexId;
use genesis_core::data::WorldData;

use crate::state::{ClimateState, GlaciationState};

/// Earth-like global precipitation baseline at sea level under average conditions.
pub const PRECIPITATION_BASELINE_MM: f32 = 800.0;

/// Maximum precipitation any hex can receive in a year. Cherrapunji-level extreme.
pub const PRECIPITATION_MAX_MM: f32 = 12000.0;

/// Minimum precipitation (extreme desert floor).
pub const PRECIPITATION_MIN_MM: f32 = 0.0;

/// Hexes within this distance of ocean can experience monsoonal precipitation.
pub const MONSOON_DISTANCE_KM: f32 = 200.0;

/// Maximum monsoon bonus in mm/year (added to base precipitation in eligible regions).
pub const MONSOON_MAX_BONUS_MM: f32 = 1200.0;

/// Glaciation reduces global precipitation by this factor at maximum intensity.
pub const GLACIATION_PRECIPITATION_REDUCTION: f32 = 0.2;

/// Computes per-hex precipitation for every hex on the planet.
///
/// Writes `WorldData.precipitation` (mm per year).
pub fn compute_precipitation_field(data: &mut WorldData, climate: &ClimateState) {
    let n = data.cell_count() as usize;
    let grid = &data.grid;
    let glaciation_factor = match climate.glaciation {
        GlaciationState::Glacial => 1.0 - GLACIATION_PRECIPITATION_REDUCTION,
        GlaciationState::Transition => 1.0 - GLACIATION_PRECIPITATION_REDUCTION * 0.5,
        GlaciationState::Interglacial => 1.0,
    };

    for i in 0..n {
        let hex = HexId(i as u32);
        let (lat_rad, _lon_rad) = grid.center_lat_lon(hex);
        let temp_c = data.temperature_mean[i];
        let distance_km = data.distance_to_ocean_km[i];

        let base = base_precipitation_mm(temp_c, lat_rad);
        let orographic = orographic_precipitation_mm(data, hex, i);
        let monsoon = monsoon_modifier_mm(lat_rad, distance_km);
        let coastal = coastal_current_modifier_mm(data, hex, i);

        let combined = (base + orographic + monsoon + coastal) * glaciation_factor;
        let clamped = combined.clamp(PRECIPITATION_MIN_MM, PRECIPITATION_MAX_MM);

        data.precipitation[i] = clamped;
    }
}

/// Base precipitation from temperature and latitude (circulation cell bands).
fn base_precipitation_mm(temp_c: f32, lat_rad: f64) -> f32 {
    // Warmth factor: warm air holds more moisture; cold air holds less.
    // -20°C → 0.15; 0°C → 0.7; 25°C → 1.5; 30°C+ → 1.6
    let warmth_factor = if temp_c < -20.0 {
        0.15
    } else if temp_c > 30.0 {
        1.6
    } else {
        0.15 + (temp_c + 20.0) / 50.0 * 1.35
    };

    let abs_lat = lat_rad.abs();
    let lat_factor = latitude_precipitation_factor(abs_lat);

    PRECIPITATION_BASELINE_MM * warmth_factor * lat_factor as f32
}

/// Latitude factor modeling Earth-like wet/dry bands.
fn latitude_precipitation_factor(abs_lat_rad: f64) -> f64 {
    // Wet bands at equator (lat 0) and ~60° (lat ≈ π/3); dry at ~30° (lat ≈ π/6) and poles.
    let term1 = (abs_lat_rad * 6.0).cos();
    let term2 = 0.3 * (abs_lat_rad * 2.0).cos();
    let combined = 0.5 + 0.35 * term1 + 0.15 * term2;
    combined.clamp(0.2, 1.5)
}

/// Orographic precipitation: wind hitting elevation creates rain on windward side,
/// rain shadow on leeward side.
fn orographic_precipitation_mm(data: &WorldData, hex: HexId, idx: usize) -> f32 {
    use std::f32::consts::PI;

    let wind_dir = data.wind_direction_rad[idx];
    let wind_speed = data.wind_speed_m_s[idx];

    if wind_speed < 1.0 {
        return 0.0;
    }

    let elev_here = data.elevation_mean[idx];

    let upwind_neighbor = find_neighbor_in_direction(data, hex, wind_dir + PI);

    let elev_upwind = match upwind_neighbor {
        Some(n) => data.elevation_mean[n.0 as usize],
        None => elev_here,
    };

    if elev_here > elev_upwind + 200.0 {
        let rise = (elev_here - elev_upwind).min(3000.0);
        let enhancement = (rise / 1000.0) * 600.0 * (wind_speed / 8.0);
        return enhancement.clamp(0.0, 3000.0);
    }

    if elev_upwind > elev_here + 500.0 {
        let shadow_factor = ((elev_upwind - elev_here) / 1000.0).min(2.0);
        let suppression = -400.0 * shadow_factor * (wind_speed / 8.0);
        return suppression.clamp(-700.0, 0.0);
    }

    0.0
}

/// Finds the neighbor of `hex` closest to the given bearing (radians from north, clockwise).
fn find_neighbor_in_direction(data: &WorldData, hex: HexId, bearing_rad: f32) -> Option<HexId> {
    let grid = &data.grid;
    let hex_pos = grid.cell_center_direction(hex);

    let north_pole = [0.0_f64, 0.0, 1.0];
    let east = normalize_cross(north_pole, hex_pos);
    let north = normalize_cross(hex_pos, east);

    let target_east = bearing_rad.sin();
    let target_north = bearing_rad.cos();

    let mut best: Option<HexId> = None;
    let mut best_alignment = -1.0_f64;

    for &neighbor in grid.neighbors(hex) {
        let n_pos = grid.cell_center_direction(neighbor);
        let to_n = [
            n_pos[0] - hex_pos[0],
            n_pos[1] - hex_pos[1],
            n_pos[2] - hex_pos[2],
        ];
        let east_comp = to_n[0] * east[0] + to_n[1] * east[1] + to_n[2] * east[2];
        let north_comp = to_n[0] * north[0] + to_n[1] * north[1] + to_n[2] * north[2];
        let mag = (east_comp * east_comp + north_comp * north_comp).sqrt();
        if mag < 1e-9 {
            continue;
        }

        let alignment = (east_comp / mag) * f64::from(target_east)
            + (north_comp / mag) * f64::from(target_north);
        if alignment > best_alignment {
            best_alignment = alignment;
            best = Some(neighbor);
        }
    }

    best
}

fn normalize_cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    let cross = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let mag = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
    if mag < 1e-9 {
        [1.0, 0.0, 0.0]
    } else {
        [cross[0] / mag, cross[1] / mag, cross[2] / mag]
    }
}

/// Monsoon bonus for tropical/subtropical coastal land hexes.
fn monsoon_modifier_mm(lat_rad: f64, distance_to_ocean_km: f32) -> f32 {
    let abs_lat_deg = lat_rad.abs().to_degrees() as f32;
    if abs_lat_deg > 30.0 {
        return 0.0;
    }
    if !distance_to_ocean_km.is_finite() || distance_to_ocean_km > MONSOON_DISTANCE_KM {
        return 0.0;
    }
    if distance_to_ocean_km < 1.0 {
        return 0.0;
    }

    let coastal_factor = 1.0 - (distance_to_ocean_km / MONSOON_DISTANCE_KM);
    let latitude_factor = 1.0 - (abs_lat_deg / 30.0);
    MONSOON_MAX_BONUS_MM * coastal_factor * latitude_factor
}

/// Coastal precipitation modifier from adjacent ocean currents.
fn coastal_current_modifier_mm(data: &WorldData, hex: HexId, idx: usize) -> f32 {
    let sea_level = data.sea_level_m;
    let elevation = data.elevation_mean[idx];

    if elevation < sea_level {
        return 0.0;
    }
    if !data.distance_to_ocean_km[idx].is_finite() || data.distance_to_ocean_km[idx] > 100.0 {
        return 0.0;
    }

    let local_temp = data.temperature_mean[idx];
    let mut anomaly_sum = 0.0_f32;
    let mut neighbor_count = 0;

    for &neighbor in data.grid.neighbors(hex) {
        let n_idx = neighbor.0 as usize;
        if data.elevation_mean[n_idx] >= sea_level {
            continue;
        }
        let neighbor_temp = data.temperature_mean[n_idx];
        let current = data.ocean_current_vec[n_idx];
        let strength = (current[0] * current[0] + current[1] * current[1]).sqrt();

        if strength < 0.05 {
            continue;
        }

        anomaly_sum += (neighbor_temp - local_temp) * strength;
        neighbor_count += 1;
    }

    if neighbor_count == 0 {
        return 0.0;
    }

    let avg_anomaly = anomaly_sum / neighbor_count as f32;
    (avg_anomaly * 30.0).clamp(-300.0, 400.0)
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
        state.atmospheric_composition.greenhouse_forcing = 0.0;
        state.glaciation = GlaciationState::Interglacial;
        state
    }

    #[test]
    fn warm_climate_gets_more_precipitation_than_cold() {
        let tropical = base_precipitation_mm(28.0, 0.05);
        let polar = base_precipitation_mm(-20.0, 1.4);
        assert!(tropical > polar * 3.0);
    }

    #[test]
    fn latitude_factor_has_equator_peak() {
        let equator = latitude_precipitation_factor(0.0);
        let subtropical = latitude_precipitation_factor(std::f64::consts::FRAC_PI_6);
        assert!(equator > subtropical);
    }

    #[test]
    fn precipitation_clamped() {
        let mut world = world_at_level(5);
        let climate = earth_like_climate_state();
        compute_precipitation_field(&mut world.data, &climate);
        for &p in &world.data.precipitation {
            assert!(p >= 0.0);
            assert!(p <= PRECIPITATION_MAX_MM);
        }
    }

    #[test]
    fn glaciation_reduces_precipitation() {
        let mut world = world_at_level(5);
        let mut climate = earth_like_climate_state();
        compute_precipitation_field(&mut world.data, &climate);
        let baseline: Vec<f32> = world.data.precipitation.clone();

        climate.glaciation = GlaciationState::Glacial;
        compute_precipitation_field(&mut world.data, &climate);

        let mut total_baseline = 0.0_f32;
        let mut total_glacial = 0.0_f32;
        for (i, &p) in world.data.precipitation.iter().enumerate() {
            total_baseline += baseline[i];
            total_glacial += p;
        }
        assert!(total_glacial < total_baseline);
    }

    #[test]
    fn no_monsoon_at_high_latitude() {
        let m = monsoon_modifier_mm(1.0, 50.0);
        assert_eq!(m, 0.0);
    }

    #[test]
    fn no_monsoon_far_from_ocean() {
        let m = monsoon_modifier_mm(0.1, 1000.0);
        assert_eq!(m, 0.0);
    }

    #[test]
    fn monsoon_active_in_tropical_coast() {
        let m = monsoon_modifier_mm(0.2, 50.0);
        assert!(m > 0.0);
        assert!(m <= MONSOON_MAX_BONUS_MM);
    }

    #[test]
    fn performance_at_level_7_under_100ms() {
        use std::time::Instant;

        let mut world = world_at_level(7);
        let climate = earth_like_climate_state();

        let start = Instant::now();
        compute_precipitation_field(&mut world.data, &climate);
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 100,
            "precipitation took {}ms at level 7; should be under 100ms",
            elapsed.as_millis()
        );
    }

    #[test]
    fn determinism() {
        let mut world_a = world_at_level(5);
        let mut world_b = world_at_level(5);
        let climate = earth_like_climate_state();

        compute_precipitation_field(&mut world_a.data, &climate);
        compute_precipitation_field(&mut world_b.data, &climate);

        assert_eq!(world_a.data.precipitation, world_b.data.precipitation);
    }
}
