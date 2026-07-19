//! Erosion and sediment transport (Doc 08 §8).
//!
//! Hillslope denudation, stream-power incision, and one-pass sediment routing.
//! Elevation changes are written to [`WorldData::hydro_elevation_delta_m`] and
//! applied to birth-frame plate surfaces by tectonics on the next geological
//! tick (§8.5 — hydrology never calls `modify_surface_at_world_hex` directly).

use genesis_core::HexId;
use genesis_core::data::{
    BedrockType, HydroFlags, RiverClass, WaterBodyId, WorldData, river_class,
};

use crate::regime::flood_magnitude_m3_yr;
use crate::rivers::STREAM_CLASS_MIN_M3_YR;
use crate::routing::{RoutingSurface, hex_area_m2};

/// Channel incision rate constant, 1/yr (§8.2).
pub const K_CHANNEL_PER_YEAR: f64 = 2.0e-7;
/// Discharge (m³/yr) that normalizes `sqrt(discharge_norm)` to 1.0 at Major scale.
pub const DISCHARGE_NORM_M3_YR: f64 = 1.0e11;
/// Freeboard land erodes toward (m above sea) — live tectonics constant, mirrored.
pub const CONTINENTAL_FREEBOARD_M: f32 = 800.0;
/// Cumulative alluvium before bedrock flips to Sedimentary (§8.3).
pub const DEPOSITION_THRESHOLD_M: f32 = 500.0;
/// Relative mass-conservation tolerance (§8.4).
pub const MASS_CONSERVATION_TOLERANCE_REL: f64 = 1.0e-6;

/// Precipitation baseline for the climate erosion modifier (Doc 06 §8.2).
const EROSION_PRECIPITATION_BASELINE_MM: f32 = 800.0;
const EROSION_CLIMATE_MODIFIER_MIN: f64 = 0.1;
const EROSION_CLIMATE_MODIFIER_MAX: f64 = 3.0;
const EROSION_FROZEN_TEMPERATURE_C: f32 = -10.0;
const EROSION_FROZEN_FACTOR: f64 = 0.25;

fn bedrock_mult(bedrock: BedrockType) -> f64 {
    match bedrock {
        BedrockType::Igneous => 0.08,
        BedrockType::Metamorphic => 0.15,
        BedrockType::Sedimentary => 1.2,
        BedrockType::Limestone => 1.0,
        BedrockType::OceanicCrust => 0.0,
        BedrockType::Unknown => 1.0,
    }
}

fn climate_mod(data: &WorldData, i: usize) -> f64 {
    let mut modifier = f64::from(data.precipitation[i] / EROSION_PRECIPITATION_BASELINE_MM)
        .clamp(EROSION_CLIMATE_MODIFIER_MIN, EROSION_CLIMATE_MODIFIER_MAX);
    if data.temperature_mean[i] < EROSION_FROZEN_TEMPERATURE_C {
        modifier *= EROSION_FROZEN_FACTOR;
    }
    // Mild snowmelt-pulse boost for Nival/high-seasonality channels (§7.2 timing).
    if data.discharge_seasonality[i] >= 2.0 {
        modifier *= 1.15;
    }
    modifier
}

/// Per-tick erosion/sediment outcome (diagnostic totals + side effects on world).
#[derive(Clone, Debug, Default)]
pub struct ErosionOutcome {
    pub eroded_m3: f64,
    pub deposited_m3: f64,
    pub ocean_sink_m3: f64,
}

impl ErosionOutcome {
    /// §8.4 mass conservation: eroded ≈ deposited + ocean sink.
    pub fn is_conserved(&self) -> bool {
        let rhs = self.deposited_m3 + self.ocean_sink_m3;
        if self.eroded_m3 <= 0.0 && rhs <= 0.0 {
            return true;
        }
        let scale = self.eroded_m3.max(rhs).max(1.0);
        (self.eroded_m3 - rhs).abs() <= MASS_CONSERVATION_TOLERANCE_REL * scale
    }
}

/// §8 hillslope + stream-power + sediment routing for one tick.
///
/// `glacial_load_m` is optional extra load from §9.2 (zeros until ice carving).
/// `alluvium_depth_m` is the persistent deposition accumulator (soil input).
pub fn apply_erosion(
    data: &mut WorldData,
    surface: &RoutingSurface,
    alluvium_depth_m: &mut [f32],
    glacial_load_m: &[f64],
    tick_years: f64,
) -> ErosionOutcome {
    let n = data.cell_count() as usize;
    debug_assert_eq!(alluvium_depth_m.len(), n);
    debug_assert_eq!(data.hydro_elevation_delta_m.len(), n);

    let base_rate = data.parameters.core.geology.base_erosion_rate_per_year;
    if base_rate <= 0.0 || tick_years <= 0.0 {
        return ErosionOutcome::default();
    }

    let hex_area = hex_area_m2(&data.grid);
    let sea = data.sea_level_m;
    let mut load_m = vec![0.0_f64; n];
    let mut eroded_m3 = 0.0_f64;

    // §8.1 hillslope + §8.2 stream-power — produce load and elevation deltas.
    #[allow(clippy::needless_range_loop)]
    for i in 0..n {
        if data.water_body_id[i] != WaterBodyId::NONE {
            continue;
        }
        let elev = data.elevation_mean[i];
        let elev_above = elev - sea;
        if elev_above <= 0.0 {
            continue;
        }
        // Continental freeboard target; oceanic islands erode toward sea level.
        let freeboard = if data.continental_crust[i] {
            CONTINENTAL_FREEBOARD_M
        } else {
            0.0
        };
        let erodible = f64::from((elev_above - freeboard).max(0.0));
        if erodible <= 0.0 && f64::from(data.river_discharge_m3_yr[i]) < STREAM_CLASS_MIN_M3_YR {
            continue;
        }

        let mult = bedrock_mult(data.bedrock_type[i]) * climate_mod(data, i);
        let mut hillslope = erodible * base_rate * mult * tick_years;
        hillslope = hillslope.min(erodible);

        let mut incision = 0.0_f64;
        let discharge = f64::from(data.river_discharge_m3_yr[i]);
        if discharge >= STREAM_CLASS_MIN_M3_YR {
            let slope = channel_slope(data, surface, i);
            let discharge_norm = (discharge / DISCHARGE_NORM_M3_YR).sqrt();
            incision = K_CHANNEL_PER_YEAR * mult * discharge_norm * slope * tick_years;
            // Floor at downstream water level.
            if let Some(target) = surface.flow_target[i] {
                let floor = water_floor_m(data, target as usize);
                let max_cut = f64::from(elev) - floor;
                if max_cut > 0.0 {
                    incision = incision.min(max_cut);
                } else {
                    incision = 0.0;
                }
            }
        }

        let glacial = glacial_load_m.get(i).copied().unwrap_or(0.0).max(0.0);
        let total = hillslope + incision + glacial;
        if total <= 0.0 {
            continue;
        }
        load_m[i] += total;
        data.hydro_elevation_delta_m[i] -= total as f32;
        eroded_m3 += total * hex_area;
    }

    // §8.3 route load downstream in descending filled order.
    let mut deposited_m3 = 0.0_f64;
    let mut ocean_sink_m3 = 0.0_f64;
    for &cell in &surface.order_desc {
        let i = cell as usize;
        let mut remaining = load_m[i];
        if remaining <= 0.0 {
            continue;
        }
        let Some(target) = surface.flow_target[i] else {
            // Retained basin / sink: perfect trap (§8.3 lakes).
            deposit(
                data,
                alluvium_depth_m,
                i,
                remaining,
                hex_area,
                &mut deposited_m3,
            );
            load_m[i] = 0.0;
            continue;
        };
        let j = target as usize;
        if data.water_body_id[j] != WaterBodyId::NONE {
            let is_ocean = data
                .water_bodies
                .get(&data.water_body_id[j])
                .is_some_and(|b| b.kind == genesis_core::data::WaterBodyKind::Ocean);
            if is_ocean {
                let discharge = f64::from(data.river_discharge_m3_yr[i]);
                if river_class(discharge) >= RiverClass::Major {
                    // Major mouths prograde: keep a fraction on the land hex as delta.
                    let delta_frac = 0.4;
                    deposit(
                        data,
                        alluvium_depth_m,
                        i,
                        remaining * delta_frac,
                        hex_area,
                        &mut deposited_m3,
                    );
                    ocean_sink_m3 += remaining * (1.0 - delta_frac) * hex_area;
                    data.hydro_flags[i] |= HydroFlags::DELTA;
                } else {
                    ocean_sink_m3 += remaining * hex_area;
                }
            } else {
                deposit(
                    data,
                    alluvium_depth_m,
                    j,
                    remaining,
                    hex_area,
                    &mut deposited_m3,
                );
            }
            load_m[i] = 0.0;
            continue;
        }
        if surface.candidate_of[j] != crate::routing::NONE {
            deposit(
                data,
                alluvium_depth_m,
                j,
                remaining,
                hex_area,
                &mut deposited_m3,
            );
            load_m[i] = 0.0;
            continue;
        }

        let slope = channel_slope(data, surface, i).max(1.0e-6);
        let discharge = f64::from(data.river_discharge_m3_yr[i]).max(1.0);
        let capacity = (discharge / DISCHARGE_NORM_M3_YR) * slope * 50.0; // m of transportable load
        let flood_w = flood_magnitude_m3_yr(data, i as u32) / discharge.max(1.0);
        let deposit_here = if slope < 0.002 {
            // Floodplain: deposit excess weighted by flood magnitude.
            ((remaining - capacity).max(0.0) * flood_w.min(2.0)).min(remaining)
        } else {
            (remaining - capacity).max(0.0)
        };
        if deposit_here > 0.0 {
            deposit(
                data,
                alluvium_depth_m,
                i,
                deposit_here,
                hex_area,
                &mut deposited_m3,
            );
            remaining -= deposit_here;
        }
        load_m[i] = 0.0;
        load_m[j] += remaining;
    }

    // Any load that never left the order (no target chain to ocean) counts as deposited
    // at its last cell — already handled. Residual on water cells → ocean sink.
    #[allow(clippy::needless_range_loop)]
    for i in 0..n {
        if load_m[i] <= 0.0 {
            continue;
        }
        if data.water_body_id[i] != WaterBodyId::NONE {
            ocean_sink_m3 += load_m[i] * hex_area;
        } else {
            deposit(
                data,
                alluvium_depth_m,
                i,
                load_m[i],
                hex_area,
                &mut deposited_m3,
            );
        }
        load_m[i] = 0.0;
    }

    let outcome = ErosionOutcome {
        eroded_m3,
        deposited_m3,
        ocean_sink_m3,
    };
    debug_assert!(
        outcome.is_conserved(),
        "Doc 08 §8.4 mass conservation violated: eroded={} deposited={} ocean={}",
        outcome.eroded_m3,
        outcome.deposited_m3,
        outcome.ocean_sink_m3
    );
    outcome
}

fn water_floor_m(data: &WorldData, i: usize) -> f64 {
    if data.water_body_id[i] != WaterBodyId::NONE {
        f64::from(data.water_level_m[i])
    } else {
        f64::from(data.elevation_mean[i])
    }
}

fn channel_slope(data: &WorldData, surface: &RoutingSurface, i: usize) -> f64 {
    let Some(target) = surface.flow_target[i] else {
        return 0.0;
    };
    let drop = f64::from(surface.filled_m[i]) - f64::from(surface.filled_m[target as usize]);
    let width = hex_area_m2(&data.grid).sqrt().max(1.0);
    (drop / width).max(0.0)
}

fn deposit(
    data: &mut WorldData,
    alluvium_depth_m: &mut [f32],
    i: usize,
    depth_m: f64,
    hex_area: f64,
    deposited_m3: &mut f64,
) {
    if depth_m <= 0.0 {
        return;
    }
    alluvium_depth_m[i] += depth_m as f32;
    data.hydro_elevation_delta_m[i] += depth_m as f32;
    *deposited_m3 += depth_m * hex_area;
    if alluvium_depth_m[i] >= DEPOSITION_THRESHOLD_M
        && matches!(
            data.bedrock_type[i],
            BedrockType::Igneous | BedrockType::Metamorphic | BedrockType::Unknown
        )
    {
        // Soft flip on the bulk array; plate-surface bedrock is updated when
        // tectonics applies the pending delta (§8.3 / §8.5).
        data.bedrock_type[i] = BedrockType::Sedimentary;
    }
    let _ = HexId(i as u32);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::RoutingSurface;
    use genesis_core::data::{SoilClass, WaterBody, WaterBodyKind};
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{WorldYear, create_world};

    fn fixture() -> genesis_core::World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        params.core.geology.base_erosion_rate_per_year = 5.0e-8;
        let mut world = create_world(params).expect("world");
        world.data.current_year = WorldYear(1_000_000_000);
        let n = world.data.cell_count() as usize;
        world.data.elevation_mean[0] = -100.0;
        world.data.sea_level_m = 0.0;
        world.data.water_level_m[0] = 0.0;
        world.data.water_body_id[0] = WaterBodyId(0);
        world.data.water_bodies.insert(
            WaterBodyId(0),
            WaterBody {
                id: WaterBodyId(0),
                kind: WaterBodyKind::Ocean,
                surface_m: 0.0,
                area_km2: 1.0,
                volume_km3: 1.0,
                salinity: 0.0,
                outlet: None,
            },
        );
        for i in 1..n {
            world.data.elevation_mean[i] = 1000.0 - (i as f32) * 0.1;
            world.data.precipitation[i] = 800.0;
            world.data.temperature_mean[i] = 10.0;
            world.data.soil_class[i] = SoilClass::Loamy;
            world.data.river_discharge_m3_yr[i] = if i < 20 { 5.0e10 } else { 0.0 };
        }
        world.data.hydro_elevation_delta_m = vec![0.0; n];
        world
    }

    #[test]
    fn hillslope_and_incision_produce_negative_deltas() {
        let mut world = fixture();
        let surface = RoutingSurface::build(&world.data, &[]);
        let n = world.data.cell_count() as usize;
        let mut alluvium = vec![0.0; n];
        let glacial = vec![0.0; n];
        let outcome = apply_erosion(
            &mut world.data,
            &surface,
            &mut alluvium,
            &glacial,
            500_000.0,
        );
        assert!(outcome.eroded_m3 > 0.0, "should erode highland");
        let lowered = world
            .data
            .hydro_elevation_delta_m
            .iter()
            .filter(|&&d| d < 0.0)
            .count();
        assert!(lowered > 0, "some cells must lose elevation");
        assert!(outcome.is_conserved(), "{outcome:?}");
    }

    #[test]
    fn mass_is_conserved_deterministically() {
        let mut a = fixture();
        let mut b = fixture();
        let sa = RoutingSurface::build(&a.data, &[]);
        let sb = RoutingSurface::build(&b.data, &[]);
        let n = a.data.cell_count() as usize;
        let mut alluv_a = vec![0.0; n];
        let mut alluv_b = vec![0.0; n];
        let glacial = vec![0.0; n];
        let oa = apply_erosion(&mut a.data, &sa, &mut alluv_a, &glacial, 500_000.0);
        let ob = apply_erosion(&mut b.data, &sb, &mut alluv_b, &glacial, 500_000.0);
        assert_eq!(oa.eroded_m3, ob.eroded_m3);
        assert_eq!(oa.deposited_m3, ob.deposited_m3);
        assert_eq!(oa.ocean_sink_m3, ob.ocean_sink_m3);
        assert_eq!(
            a.data.hydro_elevation_delta_m,
            b.data.hydro_elevation_delta_m
        );
        assert!(oa.is_conserved());
    }
}
