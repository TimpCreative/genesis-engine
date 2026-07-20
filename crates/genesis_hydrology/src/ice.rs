//! Ice masses, GIA, and glacial carving (Doc 08 §9).

use genesis_core::HexId;
use genesis_core::data::{HydroFlags, SoilClass, WaterBodyId, WaterBodyKind, WorldData};

use crate::erosion::continental_fluvial_floor_m;
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
/// Hex rings from ocean within which continental glacial overdeepening (fjords)
/// may ignore the freeboard floor.
pub const FJORD_OCEAN_RING_HEXES: u32 = 2;
/// Loess loft range in hex hops (§9.2).
pub const LOESS_RANGE: usize = 3;
/// Fraction of carved load lofted as loess.
pub const LOESS_FRACTION: f64 = 0.15;
/// Moraine dam fraction of carved load deposited at the terminus.
pub const MORAINE_FRACTION: f64 = 0.2;

/// True if hex is ocean/sea wet, or dry land below sea (ocean-side bathymetry).
fn is_ocean_side(data: &WorldData, i: usize) -> bool {
    if data.elevation_mean[i] < data.sea_level_m {
        return true;
    }
    let id = data.water_body_id[i];
    if id == WaterBodyId::NONE {
        return false;
    }
    data.water_bodies
        .get(&id)
        .is_some_and(|b| matches!(b.kind, WaterBodyKind::Ocean | WaterBodyKind::Sea))
}

/// True if any hex within `rings` hops is ocean-side (fjord coastal exception).
fn within_ocean_rings(data: &WorldData, start: usize, rings: u32) -> bool {
    if rings == 0 {
        return is_ocean_side(data, start);
    }
    let n = data.cell_count() as usize;
    let mut visited = vec![false; n];
    let mut frontier = vec![start];
    visited[start] = true;
    for _ in 0..=rings {
        let mut next = Vec::new();
        for &i in &frontier {
            if is_ocean_side(data, i) {
                return true;
            }
            for nb in data.grid.neighbors(HexId(i as u32)) {
                let j = nb.0 as usize;
                if j < n && !visited[j] {
                    visited[j] = true;
                    next.push(j);
                }
            }
        }
        frontier = next;
        if frontier.is_empty() {
            break;
        }
    }
    false
}

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
        let mut max_cut = f64::from(data.elevation_mean[i] - floor).max(0.0);
        // Continental interiors respect the freeboard floor; coastal rings
        // keep §9.2 fjord overdeepening (Doc 08 morphology deviation).
        if let Some(cont_floor) = continental_fluvial_floor_m(data, i)
            && !within_ocean_rings(data, i, FJORD_OCEAN_RING_HEXES)
        {
            max_cut = max_cut.min((f64::from(data.elevation_mean[i]) - cont_floor).max(0.0));
        }
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
    use crate::erosion::{CONTINENTAL_FREEBOARD_M, CONTINENTAL_INCISION_ALLOWANCE_M};
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

    #[test]
    fn interior_continental_ice_respects_freeboard_floor() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        params.core.geology.base_erosion_rate_per_year = 5.0e-8;
        let mut world = create_world(params).expect("world");
        world.data.current_year = WorldYear(1_000_000_000);
        let n = world.data.cell_count() as usize;
        let sea = 0.0_f32;
        world.data.sea_level_m = sea;
        // Ocean only at hex 0; pick a far interior hex for ice.
        world.data.elevation_mean[0] = -100.0;
        world.data.water_level_m[0] = 0.0;
        world.data.water_body_id[0] = WaterBodyId(0);
        world.data.water_bodies.insert(
            WaterBodyId(0),
            genesis_core::data::WaterBody {
                id: WaterBodyId(0),
                kind: WaterBodyKind::Ocean,
                surface_m: 0.0,
                area_km2: 1.0,
                volume_km3: 1.0,
                salinity: 0.0,
                outlet: None,
            },
        );
        world.data.continental_crust = vec![true; n];
        world.data.continental_crust[0] = false;
        for i in 1..n {
            world.data.elevation_mean[i] = sea + CONTINENTAL_FREEBOARD_M;
            world.data.elevation_relief[i] = 50.0;
            world.data.temperature_mean[i] = -20.0;
        }
        // Find a hex more than 2 rings from ocean hex 0.
        let interior = (1..n)
            .find(|&i| !within_ocean_rings(&world.data, i, FJORD_OCEAN_RING_HEXES))
            .expect("grid should have an interior hex beyond fjord rings");
        world.data.hydro_elevation_delta_m = vec![0.0; n];
        world.data.glaciation_intensity = 1.0;
        world.data.ice_load_m = vec![0.0; n];
        let surface = RoutingSurface::build(&world.data, &[]);
        let mut prev = vec![false; n];
        let (_vol, _load) = update_ice(&mut world.data, &surface, &mut prev, 5e-8, 50_000_000.0);
        let floor = sea + CONTINENTAL_FREEBOARD_M - CONTINENTAL_INCISION_ALLOWANCE_M;
        let elev_after =
            world.data.elevation_mean[interior] + world.data.hydro_elevation_delta_m[interior];
        assert!(
            elev_after >= floor - 1e-3,
            "interior glacial carve must not go below freeboard floor; hex={interior} elev={elev_after}"
        );
    }

    #[test]
    fn coastal_ice_may_overdeepen_below_freeboard() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        params.core.geology.base_erosion_rate_per_year = 5.0e-8;
        let mut world = create_world(params).expect("world");
        world.data.current_year = WorldYear(1_000_000_000);
        let n = world.data.cell_count() as usize;
        let sea = 0.0_f32;
        world.data.sea_level_m = sea;
        world.data.elevation_mean[0] = -100.0;
        world.data.water_level_m[0] = 0.0;
        world.data.water_body_id[0] = WaterBodyId(0);
        world.data.water_bodies.insert(
            WaterBodyId(0),
            genesis_core::data::WaterBody {
                id: WaterBodyId(0),
                kind: WaterBodyKind::Ocean,
                surface_m: 0.0,
                area_km2: 1.0,
                volume_km3: 1.0,
                salinity: 0.0,
                outlet: None,
            },
        );
        world.data.continental_crust = vec![true; n];
        world.data.continental_crust[0] = false;
        for i in 1..n {
            world.data.elevation_mean[i] = sea + CONTINENTAL_FREEBOARD_M;
            world.data.elevation_relief[i] = 500.0;
            world.data.temperature_mean[i] = -20.0;
        }
        let coastal = world
            .data
            .grid
            .neighbors(HexId(0))
            .first()
            .copied()
            .expect("ocean has a neighbor")
            .0 as usize;
        world.data.hydro_elevation_delta_m = vec![0.0; n];
        world.data.glaciation_intensity = 1.0;
        world.data.ice_load_m = vec![0.0; n];
        assert!(
            within_ocean_rings(&world.data, coastal, FJORD_OCEAN_RING_HEXES),
            "neighbor of ocean must be in fjord rings"
        );
        let surface = RoutingSurface::build(&world.data, &[]);
        let mut prev = vec![false; n];
        let (_vol, _load) = update_ice(&mut world.data, &surface, &mut prev, 5e-8, 50_000_000.0);
        let floor = sea + CONTINENTAL_FREEBOARD_M - CONTINENTAL_INCISION_ALLOWANCE_M;
        let elev_after =
            world.data.elevation_mean[coastal] + world.data.hydro_elevation_delta_m[coastal];
        // Fjord path may cut below the continental freeboard floor.
        assert!(
            elev_after < floor,
            "coastal glacial overdeepen should be allowed below freeboard floor; got {elev_after}"
        );
    }
}
