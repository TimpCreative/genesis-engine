//! Coastal waters: tides, estuaries, intertidal wetlands (Doc 08 §11).

use genesis_core::data::{HydroFlags, WaterBodyId, WaterBodyKind, WorldData};

use crate::rivers::{MAJOR_CLASS_MIN_M3_YR, RIVER_CLASS_MIN_M3_YR};
use crate::routing::RoutingSurface;

/// Solar-only tidal baseline, meters (§11.1).
pub const TIDAL_BASE_M: f64 = 0.4;
/// Tidal increment per moon (§11.1).
pub const TIDAL_PER_MOON_M: f64 = 1.2;
/// Sediment-load threshold (m of alluvium at mouth) above which a delta wins (§11.2).
pub const DELTA_LOAD_THRESHOLD_M: f32 = 50.0;
/// Minimum tidal range for estuary adjudication, meters.
pub const ESTUARY_TIDAL_MIN_M: f64 = 1.0;
/// Minimum tidal range for intertidal wetlands, meters.
pub const INTERTIDAL_TIDAL_MIN_M: f64 = 1.5;

/// §11.1 static global tidal range from moon count.
pub fn tidal_range_m(moon_count: u8) -> f64 {
    TIDAL_BASE_M + TIDAL_PER_MOON_M * f64::from(moon_count)
}

/// Tags estuaries and intertidal wetlands for one tick (§11.2–§11.3).
pub fn update_coastal(data: &mut WorldData, surface: &RoutingSurface, alluvium_depth_m: &[f32]) {
    let tidal = tidal_range_m(data.parameters.core.planet.moon_count);
    let n = data.cell_count() as usize;

    // Clear estuary flags each tick (re-derived).
    for flags in &mut data.hydro_flags {
        flags.remove(HydroFlags::ESTUARY);
    }

    // §11.2 estuary vs delta at River/Major mouths.
    for i in 0..n {
        if data.water_body_id[i] != WaterBodyId::NONE {
            continue;
        }
        let discharge = f64::from(data.river_discharge_m3_yr[i]);
        if discharge < RIVER_CLASS_MIN_M3_YR {
            continue;
        }
        let Some(target) = surface.flow_target[i] else {
            continue;
        };
        let j = target as usize;
        let into_ocean = data
            .water_bodies
            .get(&data.water_body_id[j])
            .is_some_and(|b| b.kind == WaterBodyKind::Ocean);
        if !into_ocean {
            continue;
        }
        let load = alluvium_depth_m.get(i).copied().unwrap_or(0.0);
        if load >= DELTA_LOAD_THRESHOLD_M {
            continue; // delta wins — sediment deposition already handled in §8.
        }
        if tidal >= ESTUARY_TIDAL_MIN_M || discharge >= MAJOR_CLASS_MIN_M3_YR && tidal >= 0.8 {
            data.hydro_flags[i] |= HydroFlags::ESTUARY;
        }
    }

    // §11.3 intertidal wetlands on low-relief ocean shores.
    if tidal < INTERTIDAL_TIDAL_MIN_M {
        return;
    }
    for i in 0..n {
        if data.water_body_id[i] != WaterBodyId::NONE {
            continue;
        }
        if data.elevation_relief[i] > 50.0 {
            continue;
        }
        let touches_ocean = data
            .grid
            .neighbors(genesis_core::HexId(i as u32))
            .iter()
            .any(|nb| {
                data.water_bodies
                    .get(&data.water_body_id[nb.0 as usize])
                    .is_some_and(|b| b.kind == WaterBodyKind::Ocean)
            });
        if touches_ocean {
            data.hydro_flags[i] |= HydroFlags::WETLAND;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidal_range_scales_with_moons() {
        assert!((tidal_range_m(0) - 0.4).abs() < 1e-9);
        assert!((tidal_range_m(1) - 1.6).abs() < 1e-9);
        assert!((tidal_range_m(2) - 2.8).abs() < 1e-9);
    }
}
