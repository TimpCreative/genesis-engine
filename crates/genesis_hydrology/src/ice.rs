//! Ice masses, GIA, and glacial carving (Doc 08 §9).

use genesis_core::HexId;
use genesis_core::data::{HydroFlags, SoilClass, WaterBodyId, WaterBodyKind, WorldData};

use crate::routing::{RoutingSurface, hex_area_m2};

/// Ice-sheet temperature threshold, °C (§9.1).
pub const ICE_SHEET_TEMP_C: f32 = -12.0;
/// Alpine glacier temperature threshold, °C (§9.1).
pub const ALPINE_ICE_TEMP_C: f32 = -4.0;
/// Sea-ice temperature threshold, °C (§9.1).
pub const SEA_ICE_TEMP_C: f32 = -2.0;
/// Full-glacial sea-level equivalent, meters (§9.1).
pub const ICE_VOLUME_MAX_SLE_M: f64 = 120.0;
/// Crustal ice-load depression proxy, meters (§9.1 GIA).
pub const ICE_LOAD_DEPRESSION_M: f32 = 250.0;
/// Glacial erosion multiplier vs hillslope (§9.2).
pub const GLACIAL_EROSION_FACTOR: f64 = 2.5;
/// Maximum glacial overdeepening below the fluvial floor, meters (§9.2).
pub const OVERDEEPEN_MAX_M: f32 = 400.0;
/// Loess loft range in hex hops (§9.2).
pub const LOESS_RANGE: usize = 3;
/// Fraction of carved load lofted as loess.
pub const LOESS_FRACTION: f64 = 0.15;
/// Moraine dam fraction of carved load deposited at the terminus.
pub const MORAINE_FRACTION: f64 = 0.2;

/// Updates `ice_mask`, sea-ice flags, budgeted `ice_volume_m3`, GIA deltas,
/// and glacial sediment load for §8. Returns per-hex glacial load (meters).
pub fn update_ice(
    data: &mut WorldData,
    surface: &RoutingSurface,
    prev_ice_mask: &mut Vec<bool>,
    base_erosion_rate: f64,
    tick_years: f64,
) -> (f64, Vec<f64>) {
    let n = data.cell_count() as usize;
    if prev_ice_mask.len() != n {
        *prev_ice_mask = vec![false; n];
    }
    let mut glacial_load = vec![0.0_f64; n];

    for i in 0..n {
        let is_water = data.water_body_id[i] != WaterBodyId::NONE;
        if is_water {
            data.ice_mask[i] = false;
            if data.temperature_mean[i] < SEA_ICE_TEMP_C {
                data.hydro_flags[i] |= HydroFlags::SEA_ICE;
            } else {
                data.hydro_flags[i].remove(HydroFlags::SEA_ICE);
            }
            continue;
        }
        data.hydro_flags[i].remove(HydroFlags::SEA_ICE);
        let t = data.temperature_mean[i];
        let sheet = t < ICE_SHEET_TEMP_C;
        let alpine = !sheet && t < ALPINE_ICE_TEMP_C && data.elevation_relief[i] > 200.0;
        data.ice_mask[i] = sheet || alpine;
    }

    let intensity = f64::from(data.glaciation_intensity).clamp(0.0, 1.0);
    // SLE volume ≈ climate glaciation intensity × 120 m × planetary surface area.
    let planet_area_m2 = hex_area_m2(&data.grid) * n as f64;
    let ice_volume_m3 = intensity * ICE_VOLUME_MAX_SLE_M * planet_area_m2;

    // GIA load target for tectonics isostasy (Doc 08 §9.1) — no direct elev hack.
    for i in 0..n {
        data.ice_load_m[i] = if data.ice_mask[i] {
            ICE_LOAD_DEPRESSION_M
        } else {
            0.0
        };
    }

    // §9.2 glacial carving load + retreat products.
    let hillslope_scale = base_erosion_rate.max(0.0) * tick_years.max(0.0);
    #[allow(clippy::needless_range_loop)]
    for i in 0..n {
        if !data.ice_mask[i] {
            continue;
        }
        let elev_above = (data.elevation_mean[i] - data.sea_level_m).max(0.0);
        let carved = f64::from(elev_above) * hillslope_scale * GLACIAL_EROSION_FACTOR;
        if carved <= 0.0 {
            continue;
        }
        // Overdeepening allowance below the fluvial floor.
        let floor = surface.flow_target[i]
            .map(|t| {
                let j = t as usize;
                if data.water_body_id[j] != WaterBodyId::NONE {
                    data.water_level_m[j] - OVERDEEPEN_MAX_M
                } else {
                    data.elevation_mean[j] - OVERDEEPEN_MAX_M
                }
            })
            .unwrap_or(data.elevation_mean[i] - OVERDEEPEN_MAX_M);
        let max_cut = f64::from(data.elevation_mean[i] - floor).max(0.0);
        let carved = carved.min(max_cut);
        glacial_load[i] += carved;
        data.hydro_elevation_delta_m[i] -= carved as f32;
        data.hydro_flags[i] |= HydroFlags::CARVED_TROUGH;

        // Moraine at terminus (downstream neighbor or self).
        if let Some(target) = surface.flow_target[i] {
            let j = target as usize;
            let moraine = carved * MORAINE_FRACTION;
            data.hydro_elevation_delta_m[j] += moraine as f32;
            glacial_load[i] -= moraine;
        }
    }

    // Retreat: fjords + loess.
    #[allow(clippy::needless_range_loop)]
    for i in 0..n {
        if prev_ice_mask[i] && !data.ice_mask[i] {
            // Newly ice-free.
            let is_ocean = data
                .water_bodies
                .get(&data.water_body_id[i])
                .is_some_and(|b| b.kind == WaterBodyKind::Ocean);
            if is_ocean
                && (data.elevation_relief[i] > 400.0
                    || data.hydro_flags[i].contains(HydroFlags::CARVED_TROUGH))
            {
                data.hydro_flags[i] |= HydroFlags::FJORD;
            }
            // Loft loess downwind using prevailing wind.
            loft_loess(data, i, LOESS_RANGE);
        }
    }

    prev_ice_mask.copy_from_slice(&data.ice_mask);
    (ice_volume_m3, glacial_load)
}

fn loft_loess(data: &mut WorldData, origin: usize, range: usize) {
    // Deterministic downwind walk along `wind_direction_rad`, preferring the
    // neighbor slot aligned with the wind and stronger wind speed for longer
    // effective loft (Doc 08 §9.2).
    let mut current = origin as u32;
    let speed = data.wind_speed_m_s[origin].max(0.5);
    let hops = ((range as f32) * (speed / 5.0).clamp(0.5, 1.5)).round() as usize;
    for _ in 0..hops.max(1) {
        let wind = data.wind_direction_rad[current as usize];
        let sector = ((wind / (std::f32::consts::TAU / 6.0)).round() as i32).rem_euclid(6) as usize;
        let neighbors = data.grid.neighbors_sorted(HexId(current));
        if neighbors.is_empty() {
            break;
        }
        let preferred = neighbors.get(sector % neighbors.len()).copied();
        let next = preferred.unwrap_or_else(|| *neighbors.iter().min().expect("non-empty"));
        let j = next.0 as usize;
        if data.water_body_id[j] == WaterBodyId::NONE && !data.ice_mask[j] {
            data.soil_class[j] = SoilClass::Loess;
            data.soil_depth_m[j] = data.soil_depth_m[j].max(5.0);
            data.soil_fertility[j] = data.soil_fertility[j].max(0.85);
        }
        current = next.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{WorldYear, create_world};

    #[test]
    fn cold_land_sets_ice_mask_and_volume() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        world.data.current_year = WorldYear(1_000_000_000);
        let n = world.data.cell_count() as usize;
        world.data.elevation_mean[0] = -100.0;
        world.data.sea_level_m = 0.0;
        world.data.water_body_id[0] = WaterBodyId(0);
        world.data.temperature_mean.fill(-20.0);
        for i in 1..n {
            world.data.elevation_mean[i] = 500.0;
            world.data.elevation_relief[i] = 300.0;
        }
        world.data.hydro_elevation_delta_m = vec![0.0; n];
        world.data.glaciation_intensity = 1.0;
        world.data.ice_load_m = vec![0.0; n];
        let surface = RoutingSurface::build(&world.data, &[]);
        let mut prev = vec![false; n];
        let (vol, load) = update_ice(&mut world.data, &surface, &mut prev, 5e-8, 500_000.0);
        assert!(world.data.ice_mask.iter().skip(1).any(|&i| i));
        assert!(vol > 0.0);
        assert!(world.data.ice_load_m.iter().any(|&l| l > 0.0));
        assert_eq!(load.len(), n);
    }
}
