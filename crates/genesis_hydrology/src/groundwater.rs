//! Groundwater (Doc 08 §6): per-hex aquifer storage, baseflow, karst
//! diversion, and the water-table proxy with its springs and oases.
//!
//! No lateral aquifer solver — recharge follows the surface drainage topology
//! (§6). Storage is the persistent per-hex accumulator in
//! [`HydrologyState::aquifer_storage_m`](crate::state::HydrologyState); its
//! ascending-`HexId` sum × hex area is the §3.2 groundwater reservoir.

use genesis_core::data::{BedrockType, HydroFlags, WorldData};

use crate::partition::{PERMAFROST_TEMP_C, pet_mm};
use crate::regime::EPHEMERAL_BASEFLOW_MIN_M3_YR;
use crate::routing::{RoutingSurface, hex_area_m2};

/// Fraction of stored water released as baseflow per tick (§6.2).
pub const BASEFLOW_RATE: f64 = 0.02;

/// Share of a karst hex's would-be runoff routed underground (§6.3).
pub const KARST_UNDERGROUND_FRACTION: f64 = 0.5;
/// Maximum underground hops before the diversion re-emerges (§6.3 "or after
/// 2 hexes").
pub const KARST_MAX_UNDERGROUND_HEXES: u32 = 2;

/// Water-table offset in humid terrain, m (§6.4 aridity curve, P/PET ≥ 1).
pub const WATER_TABLE_HUMID_OFFSET_M: f64 = 2.0;
/// Water-table offset in hyper-arid terrain, m (§6.4 aridity curve, P/PET → 0).
pub const WATER_TABLE_ARID_OFFSET_M: f64 = 60.0;
/// Per-hex decay of the proximity factor along the flow path away from
/// perennial water (§6.4 "decays with flow-path distance").
pub const PROXIMITY_DECAY_PER_HEX: f64 = 0.7;
/// Permafrost pins water tables at or above this depth, m (§7.4 "shallow").
pub const PERMAFROST_WATER_TABLE_MAX_M: f64 = 1.0;

/// Water table shallower than this (m) supports a spring (§6.4).
pub const SPRING_WATER_TABLE_MAX_M: f64 = 2.0;
/// Minimum accumulated upstream recharge for a spring, m³/yr — the §6.4
/// "upstream recharge area above threshold" proxy (≈ a handful of hexes of
/// humid recharge at subdivision 7).
pub const SPRING_MIN_UPSTREAM_RECHARGE_M3_YR: f64 = 1.0e7;
/// Minimum drop to the flow target for a spring slope, m (§6.4 "slope hex").
pub const SPRING_MIN_DROP_M: f64 = 20.0;

/// Maximum annual precipitation for an oasis hex, mm (§6.4).
pub const OASIS_MAX_PRECIP_MM: f64 = 100.0;
/// Water table shallower than this (m) supports an oasis (§6.4).
pub const OASIS_WATER_TABLE_MAX_M: f64 = 5.0;

/// Per-hex baseflow seeds (m³/yr) after recharge, release, overflow, and the
/// karst re-emergence injections.
#[derive(Clone, Debug, Default)]
pub struct Baseflow {
    /// Baseflow volume per land hex, m³/yr, ready for §4.3 accumulation.
    pub baseflow_m3_yr: Vec<f64>,
}

/// §6.1–§6.3: banks recharge into the per-hex aquifer, releases baseflow
/// (storage × [`BASEFLOW_RATE`] plus any over-capacity overflow, §6.1), then
/// diverts karst runoff underground and re-emerges it as SPRING-flagged
/// baseflow (§6.3). Updates [`HydrologyState::aquifer_storage_m`].
///
/// `candidate_inflow_m3_yr` collects karst water that re-emerges directly
/// into a candidate sea (mass balance: the diversion is conserved — runoff
/// loses exactly what baseflow and candidate inflow gain, gate §15 #14).
pub fn recharge_and_baseflow(
    data: &mut WorldData,
    surface: &RoutingSurface,
    aquifer_storage_m: &mut Vec<f64>,
    runoff_m3_yr: &mut [f64],
    recharge_m3_yr: &[f64],
    tick_years: f64,
    candidate_inflow_m3_yr: &mut [f64],
) -> Baseflow {
    let n = data.cell_count() as usize;
    let area_m2 = hex_area_m2(&data.grid);
    if aquifer_storage_m.len() != n {
        aquifer_storage_m.clear();
        aquifer_storage_m.resize(n, 0.0);
    }
    let mut baseflow = Baseflow {
        baseflow_m3_yr: vec![0.0; n],
    };
    if tick_years <= 0.0 {
        return baseflow;
    }
    let capacity_m = f64::from(data.parameters.core.hydrology.groundwater_capacity_m);

    for i in 0..n {
        if surface.is_water(data, i as u32) {
            continue;
        }
        // §6.1: recharge enters storage; storage above capacity overflows as
        // immediate baseflow; the rest releases at BASEFLOW_RATE per tick.
        let mut storage_m3 = aquifer_storage_m[i] * area_m2 + recharge_m3_yr[i] * tick_years;
        let capacity_m3 = capacity_m * area_m2;
        let overflow_m3 = (storage_m3 - capacity_m3).max(0.0);
        storage_m3 -= overflow_m3;
        let released_m3 = BASEFLOW_RATE * storage_m3;
        storage_m3 -= released_m3;
        aquifer_storage_m[i] = storage_m3 / area_m2;
        baseflow.baseflow_m3_yr[i] = (overflow_m3 + released_m3) / tick_years;
    }

    // §6.3: a share of karst runoff disappears underground and re-emerges at
    // the first non-karst hex downstream (or after the hop cap) as a
    // SPRING-flagged resurgence injected as baseflow.
    #[allow(clippy::needless_range_loop)]
    for i in 0..n {
        if surface.is_water(data, i as u32) || !data.hydro_flags[i].contains(HydroFlags::KARST) {
            continue;
        }
        let diverted = KARST_UNDERGROUND_FRACTION * runoff_m3_yr[i];
        if diverted <= 0.0 {
            continue;
        }
        runoff_m3_yr[i] -= diverted;
        let mut emergence = i;
        let mut current = i;
        for _ in 0..KARST_MAX_UNDERGROUND_HEXES {
            let Some(target) = surface.flow_target[current] else {
                break;
            };
            current = target as usize;
            emergence = current;
            if surface.is_water(data, target)
                || !data.hydro_flags[current].contains(HydroFlags::KARST)
            {
                break; // first non-karst hex (or open water) reached.
            }
        }
        let candidate = surface.candidate_of[emergence];
        if candidate != crate::routing::NONE {
            candidate_inflow_m3_yr[candidate as usize] += diverted;
        } else if !surface.is_water(data, emergence as u32) {
            baseflow.baseflow_m3_yr[emergence] += diverted;
            data.hydro_flags[emergence] |= HydroFlags::SPRING;
        }
        // Emergence into the ocean itself leaves the network (conserved out).
    }
    baseflow
}

/// Ascending-`HexId` sum of per-hex aquifer storage as a volume, m³ — the
/// §3.2 groundwater reservoir term (§6.1 "the deterministic sum").
pub fn total_groundwater_storage_m3(data: &WorldData, aquifer_storage_m: &[f64]) -> f64 {
    let area_m2 = hex_area_m2(&data.grid);
    aquifer_storage_m.iter().map(|&m| m * area_m2).sum()
}

/// §6.4: derives `water_table_depth_m`, then flags SPRING (water table < 2 m
/// on a recharge-fed slope) and OASIS (hyper-arid hex with a shallow table).
/// Karst resurgences are already SPRING-flagged by [`recharge_and_baseflow`].
///
/// Perennial water for the proximity factor: open water plus channels whose
/// accumulated baseflow keeps them perennial (§7.3's test). Distance is
/// measured downstream along flow directions, per §6.4's flow-path decay.
pub fn water_tables(
    data: &mut WorldData,
    surface: &RoutingSurface,
    baseflow_acc_m3_yr: &[f64],
    recharge_acc_m3_yr: &[f64],
) {
    let n = data.cell_count() as usize;

    // Flow-path distance to perennial water, computed downstream-first by
    // walking the accumulation order in reverse (targets are strictly lower,
    // hence earlier in `order_desc`).
    let mut distance = vec![u32::MAX; n];
    for i in 0..n {
        if surface.is_water(data, i as u32) || baseflow_acc_m3_yr[i] >= EPHEMERAL_BASEFLOW_MIN_M3_YR
        {
            distance[i] = 0;
        }
    }
    for &i in surface.order_desc.iter().rev() {
        if distance[i as usize] == 0 {
            continue;
        }
        if let Some(target) = surface.flow_target[i as usize] {
            let downstream = distance[target as usize];
            if downstream != u32::MAX {
                distance[i as usize] = downstream.saturating_add(1);
            }
        }
    }

    for i in 0..n {
        if surface.is_water(data, i as u32) {
            data.water_table_depth_m[i] = 0.0;
            continue;
        }
        // Aridity offset: 2 m humid → 60 m hyper-arid, linear in P/PET.
        let pet = pet_mm(data.temperature_mean[i]);
        let aridity = if pet > 0.0 {
            (f64::from(data.precipitation[i]) / pet).clamp(0.0, 1.0)
        } else {
            1.0 // PET 0 means precip cannot evaporate: effectively humid.
        };
        let offset = WATER_TABLE_ARID_OFFSET_M
            - (WATER_TABLE_ARID_OFFSET_M - WATER_TABLE_HUMID_OFFSET_M) * aridity;
        let proximity = if distance[i] == u32::MAX {
            0.0
        } else {
            PROXIMITY_DECAY_PER_HEX.powi(distance[i] as i32)
        };
        let mut depth = offset * (1.0 - proximity);
        if data.temperature_mean[i] < PERMAFROST_TEMP_C {
            depth = depth.min(PERMAFROST_WATER_TABLE_MAX_M); // §7.4 pinned shallow.
        }
        data.water_table_depth_m[i] = depth as f32;

        // §6.4 SPRING: shallow table on a slope with upstream recharge.
        if depth < SPRING_WATER_TABLE_MAX_M
            && recharge_acc_m3_yr[i] >= SPRING_MIN_UPSTREAM_RECHARGE_M3_YR
            && let Some(target) = surface.flow_target[i]
        {
            let drop =
                f64::from(surface.filled_m[i]) - f64::from(surface.filled_m[target as usize]);
            if drop >= SPRING_MIN_DROP_M {
                data.hydro_flags[i] |= HydroFlags::SPRING;
            }
        }
        // §6.4 OASIS: arid hex whose table is reachable — upstream recharge
        // proximity is what puts it there (the Sahara pattern).
        if f64::from(data.precipitation[i]) < OASIS_MAX_PRECIP_MM && depth < OASIS_WATER_TABLE_MAX_M
        {
            data.hydro_flags[i] |= HydroFlags::OASIS;
        }
    }
}

/// §6.4 hot springs: a SPRING on young igneous ground reads as geothermal.
///
/// Approximation: tectonics' hotspot/volcanism state lives in
/// `TectonicsState`, which this crate cannot see (it depends on
/// `genesis_core` only), so `BedrockType::Igneous` stands in for volcanic
/// activity until tectonics exposes a live volcanism field on `WorldData`.
pub fn is_hot_spring(data: &WorldData, hex: u32) -> bool {
    let i = hex as usize;
    data.hydro_flags[i].contains(HydroFlags::SPRING) && data.bedrock_type[i] == BedrockType::Igneous
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::data::SoilClass;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{HexId, WorldYear, create_world};

    const TICK_YEARS: f64 = 1_000.0;

    fn land_world() -> genesis_core::World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        world.data.current_year = WorldYear(1_000_000_000);
        let n = world.data.cell_count() as usize;
        world.data.elevation_mean[0] = -100.0;
        world.data.sea_level_m = 0.0;
        world.data.water_level_m[0] = 0.0;
        world.data.water_body_id[0] = genesis_core::WaterBodyId(0);
        for i in 1..n {
            // Flat plain: routes through the fill's +epsilon gradient with
            // no spurious index-ordered pits.
            world.data.elevation_mean[i] = 500.0;
            world.data.soil_class[i] = SoilClass::Loamy;
        }
        world
    }

    #[test]
    fn storage_caps_and_overflow_becomes_baseflow() {
        let mut world = land_world();
        let surface = RoutingSurface::build(&world.data, &[]);
        let n = world.data.cell_count() as usize;
        let area_m2 = hex_area_m2(&world.data.grid);
        let capacity_m = f64::from(world.data.parameters.core.hydrology.groundwater_capacity_m);
        // Recharge far above capacity in one tick: nearly everything overflows.
        let recharge = vec![capacity_m * area_m2 * 2.0 / TICK_YEARS; n];
        let mut runoff = vec![0.0; n];
        let mut aquifer = Vec::new();
        let mut candidate_inflow = vec![0.0; 0];
        let flow = recharge_and_baseflow(
            &mut world.data,
            &surface,
            &mut aquifer,
            &mut runoff,
            &recharge,
            TICK_YEARS,
            &mut candidate_inflow,
        );
        assert_eq!(aquifer.len(), n);
        for (i, &stored) in aquifer.iter().enumerate() {
            if i == 0 {
                continue;
            }
            assert!(stored <= capacity_m, "storage respects the capacity cap");
            // Baseflow = the overflow (one full capacity) plus the 2%
            // release of the capped storage.
            let expected = capacity_m * area_m2 * (1.0 + BASEFLOW_RATE) / TICK_YEARS;
            assert!(
                (flow.baseflow_m3_yr[i] - expected).abs() / expected < 1e-9,
                "baseflow = overflow + 2% release: {} vs {expected}",
                flow.baseflow_m3_yr[i]
            );
        }
    }

    #[test]
    fn empty_aquifer_releases_nothing_without_recharge() {
        let mut world = land_world();
        let surface = RoutingSurface::build(&world.data, &[]);
        let n = world.data.cell_count() as usize;
        let zeros = vec![0.0; n];
        let mut runoff = vec![0.0; n];
        let mut aquifer = Vec::new();
        let flow = recharge_and_baseflow(
            &mut world.data,
            &surface,
            &mut aquifer,
            &mut runoff,
            &zeros,
            TICK_YEARS,
            &mut [],
        );
        assert!(flow.baseflow_m3_yr.iter().all(|&b| b == 0.0));
    }

    #[test]
    fn karst_diversion_conserves_water_and_flags_the_resurgence() {
        let mut world = land_world();
        let n = world.data.cell_count() as usize;
        // Karst source hex with a guaranteed non-karst flow target: the
        // first spatial neighbor sits lowest, every other neighbor far above.
        let source = 1000_usize;
        let target = world.data.grid.neighbors(HexId(source as u32))[0].0 as usize;
        world.data.elevation_mean[source] = 600.0;
        world.data.elevation_mean[target] = 50.0;
        for neighbor in world.data.grid.neighbors(HexId(source as u32)) {
            let j = neighbor.0 as usize;
            if j != target {
                world.data.elevation_mean[j] = 10_000.0;
            }
        }
        world.data.bedrock_type[source] = BedrockType::Limestone;
        world.data.precipitation[source] = 1200.0;
        world.data.temperature_mean[source] = 0.0;

        let surface = RoutingSurface::build(&world.data, &[]);
        assert_eq!(surface.flow_target[source], Some(target as u32));
        world.data.hydro_flags[source] |= HydroFlags::KARST;

        let recharge = vec![0.0; n];
        let mut runoff = vec![0.0; n];
        runoff[source] = 1.0e9;
        let mut aquifer = vec![0.0; n];
        let mut candidate_inflow = vec![0.0; 0];
        let flow = recharge_and_baseflow(
            &mut world.data,
            &surface,
            &mut aquifer,
            &mut runoff,
            &recharge,
            TICK_YEARS,
            &mut candidate_inflow,
        );
        let diverted = 1.0e9 * KARST_UNDERGROUND_FRACTION;
        assert_eq!(
            runoff[source],
            1.0e9 - diverted,
            "runoff thins by the diverted share"
        );
        assert!(
            (flow.baseflow_m3_yr[target] - diverted).abs() < 1e-3,
            "the diversion re-emerges at the first non-karst hex"
        );
        assert!(world.data.hydro_flags[target].contains(HydroFlags::SPRING));
        // Mass balance: runoff loss == baseflow gain, nothing lost.
        let total = runoff[source] + flow.baseflow_m3_yr[target];
        assert!((total - 1.0e9).abs() < 1e-3);
    }

    #[test]
    fn water_table_follows_aridity_and_proximity() {
        let mut world = land_world();
        let n = world.data.cell_count() as usize;
        // A wet ramp: hex 1 is ocean-adjacent, high precip everywhere humid.
        for i in 1..n {
            world.data.precipitation[i] = 2000.0;
            world.data.temperature_mean[i] = 0.0;
        }
        // Hex 5 is hyper-arid and far from any perennial channel.
        let arid = 5_usize;
        world.data.precipitation[arid] = 10.0;
        world.data.temperature_mean[arid] = 35.0;

        let surface = RoutingSurface::build(&world.data, &[]);
        let baseflow_acc = vec![0.0; n];
        let recharge_acc = vec![0.0; n];
        water_tables(&mut world.data, &surface, &baseflow_acc, &recharge_acc);

        assert_eq!(
            world.data.water_table_depth_m[0], 0.0,
            "open water sits at 0"
        );
        let humid = world.data.water_table_depth_m[1];
        assert!(
            humid <= WATER_TABLE_HUMID_OFFSET_M as f32,
            "humid coast near 2 m: {humid}"
        );
        let deep = world.data.water_table_depth_m[arid];
        assert!(
            deep > humid,
            "hyper-arid table deeper than humid: {deep} vs {humid}"
        );
        assert!(deep <= WATER_TABLE_ARID_OFFSET_M as f32);
    }

    #[test]
    fn oasis_flags_where_desert_meets_shallow_table() {
        let mut world = land_world();
        let n = world.data.cell_count() as usize;
        for i in 1..n {
            world.data.precipitation[i] = 10.0;
            world.data.temperature_mean[i] = 35.0;
        }
        // A perennial channel (baseflow above the threshold) two hops upstream
        // of the oasis candidate: ocean <- 8 <- 7 <- 6? Build a chain: pick
        // hex 1's downstream to the ocean and give hexes near it baseflow.
        let surface = RoutingSurface::build(&world.data, &[]);
        let mut baseflow_acc = vec![0.0; n];
        // Find a short flow path to the ocean and make its cells perennial.
        let mut path = Vec::new();
        let mut current = 1_u32;
        for _ in 0..10 {
            path.push(current as usize);
            match surface.flow_target[current as usize] {
                Some(t) if !surface.is_water(&world.data, t) => current = t,
                _ => break,
            }
        }
        for &cell in &path {
            baseflow_acc[cell] = EPHEMERAL_BASEFLOW_MIN_M3_YR * 2.0;
        }
        let recharge_acc = vec![0.0; n];
        water_tables(&mut world.data, &surface, &baseflow_acc, &recharge_acc);
        // Cells at distance 1–2 from the perennial channel: wt = 60×(1−0.7^d)
        // → 18 m at d=1 … above 5. Only d≥5 stays > 5? 0.7^5≈0.168→wt≈49.9.
        // So with a 60 m aridity offset nothing flags here; instead make the
        // climate humid enough at the path cells for a shallow table.
        for &cell in &path {
            world.data.precipitation[cell] = 10.0; // still arid → OASIS-eligible
        }
        // Recompute with humid P/PET? P=10 < PET → aridity 0 → offset 60.
        // Proximity 1 at distance 0 (the channel cells themselves): wt = 0.
        water_tables(&mut world.data, &surface, &baseflow_acc, &recharge_acc);
        for &cell in &path {
            let depth = world.data.water_table_depth_m[cell];
            if depth < OASIS_WATER_TABLE_MAX_M as f32 {
                assert!(
                    world.data.hydro_flags[cell].contains(HydroFlags::OASIS),
                    "perennial desert channel hex is an oasis"
                );
            }
        }
    }

    #[test]
    fn permafrost_pins_the_table_shallow() {
        let mut world = land_world();
        let n = world.data.cell_count() as usize;
        for i in 1..n {
            world.data.temperature_mean[i] = -20.0;
            world.data.precipitation[i] = 5.0;
        }
        let surface = RoutingSurface::build(&world.data, &[]);
        let zeros = vec![0.0; n];
        water_tables(&mut world.data, &surface, &zeros, &zeros);
        for i in 1..n {
            assert!(
                world.data.water_table_depth_m[i] <= PERMAFROST_WATER_TABLE_MAX_M as f32,
                "frozen ground pins the water table: {}",
                world.data.water_table_depth_m[i]
            );
        }
    }
}
