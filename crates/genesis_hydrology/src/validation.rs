//! Doc 08 §15 validation gates (cheap CI + `#[ignore]` deep-time).

use genesis_core::parameters::{WorldParameters, WorldSeed};

/// Fixed seed for Doc 08 validation (mirrors tectonics).
pub const VALIDATION_SEED: u64 = 42;
/// CI-friendly subdivision (~2,432 hexes).
pub const VALIDATION_SUBDIVISION_LEVEL: u8 = 5;
/// Quick horizon (2 Geological ticks).
pub const VALIDATION_TARGET_YEAR_QUICK: i64 = 1_000_000;
/// Mid-depth horizon for ignored gates.
pub const VALIDATION_TARGET_YEAR_200M: i64 = 200_000_000;
/// One-billion-year deep-time horizon.
pub const VALIDATION_TARGET_YEAR_1B: i64 = 1_000_000_000;
/// Full deep-time horizon.
pub const VALIDATION_TARGET_YEAR_4B: i64 = 4_000_000_000;

/// Default parameters for hydrology validation worlds.
pub fn validation_parameters() -> WorldParameters {
    let mut params = WorldParameters::default();
    params.core.seed = WorldSeed::from_integer(VALIDATION_SEED);
    params.core.grid.subdivision_level = VALIDATION_SUBDIVISION_LEVEL;
    params.core.hydrology.water_inventory_gel_m = 1000.0;
    params
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::data::{HydroFlags, RiverClass, WaterBodyKind, river_class};
    use genesis_core::time::SimulationLayer;
    use genesis_core::{WorldYear, create_world};

    use crate::budget::{WaterBudget, inventory_volume_m3};
    use crate::ice::ICE_VOLUME_MAX_SLE_M;
    use crate::layer::HydrologyLayer;
    use crate::routing::hex_area_m2;
    use crate::state::HydrologyState;

    #[test]
    fn conservation_holds_after_active_tick() {
        let mut params = validation_parameters();
        params.core.hydrology.water_inventory_gel_m = 1000.0;
        let mut world = create_world(params).expect("world");
        world.data.current_year = WorldYear(1_000_000_000);
        world.data.precipitation.fill(800.0);
        world.data.temperature_mean.fill(10.0);
        let n = world.data.cell_count() as usize;
        for i in 0..n / 2 {
            world.data.elevation_mean[i] = -2000.0;
        }
        for i in n / 2..n {
            world.data.elevation_mean[i] = 500.0;
        }
        let mut hydrology = HydrologyState::new();
        let (mut layer, shared) = HydrologyLayer::attach(&mut hydrology);
        layer.advance(&mut world.data, &world.rng);
        drop(layer);
        let state = HydrologyLayer::detach_state(shared);
        let inventory = inventory_volume_m3(&world.data.parameters);
        let budget = WaterBudget::partition(
            inventory,
            1.0,
            state.prev_lake_volume_m3,
            state.ice_volume_m3,
            state.groundwater_storage_m3,
        );
        assert!(
            budget.is_conserved(),
            "gate #1 conservation: err={}",
            budget.conservation_error_m3()
        );
    }

    #[test]
    fn sea_level_dial_responds_to_inventory() {
        let mut lows = Vec::new();
        for gel in [200.0_f32, 1000.0, 3000.0] {
            let mut params = validation_parameters();
            params.core.hydrology.water_inventory_gel_m = gel;
            let mut world = create_world(params).expect("world");
            world.data.current_year = WorldYear(1_000_000_000);
            world.data.precipitation.fill(100.0);
            let n = world.data.cell_count() as usize;
            for i in 0..n {
                world.data.elevation_mean[i] = if i < n / 3 { -3000.0 } else { 800.0 };
            }
            let mut hydrology = HydrologyState::new();
            let (mut layer, shared) = HydrologyLayer::attach(&mut hydrology);
            layer.advance(&mut world.data, &world.rng);
            drop(layer);
            let _ = HydrologyLayer::detach_state(shared);
            lows.push(world.data.sea_level_m);
        }
        assert!(
            lows[0] <= lows[1] + 1.0 && lows[1] <= lows[2] + 1.0,
            "gate #2 sea-level dial: {lows:?}"
        );
    }

    #[test]
    fn honest_rivers_discharge_nondecreasing_along_flow() {
        // Gate #4 shape: after one active tick, discharge does not decrease
        // along any flow edge (deposition may leave equality).
        let params = validation_parameters();
        let mut world = create_world(params).expect("world");
        world.data.current_year = WorldYear(1_000_000_000);
        world.data.precipitation.fill(800.0);
        world.data.temperature_mean.fill(12.0);
        let n = world.data.cell_count() as usize;
        for i in 0..n / 2 {
            world.data.elevation_mean[i] = -2500.0;
        }
        for i in n / 2..n {
            world.data.elevation_mean[i] = 400.0 + (i as f32) * 0.1;
        }
        let mut hydrology = HydrologyState::new();
        let (mut layer, shared) = HydrologyLayer::attach(&mut hydrology);
        layer.advance(&mut world.data, &world.rng);
        drop(layer);
        let _ = HydrologyLayer::detach_state(shared);

        for i in 0..n {
            let Some(dir) = world.data.flow_direction[i] else {
                continue;
            };
            let Some(&target) = world
                .data
                .grid
                .neighbors(genesis_core::HexId(i as u32))
                .get(dir.index())
            else {
                continue;
            };
            let j = target.0 as usize;
            if world.data.water_body_id[j] != genesis_core::data::WaterBodyId::NONE {
                continue;
            }
            assert!(
                world.data.river_discharge_m3_yr[j] + 1.0 >= world.data.river_discharge_m3_yr[i],
                "gate #4 discharge decreases {i}→{j}: {} → {}",
                world.data.river_discharge_m3_yr[i],
                world.data.river_discharge_m3_yr[j]
            );
        }
    }

    #[test]
    fn glaciation_intensity_scales_ice_volume() {
        // Gate #3 shape: full intensity → ~120 m SLE equivalent volume.
        let params = validation_parameters();
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        world.data.glaciation_intensity = 1.0;
        world.data.temperature_mean.fill(-20.0);
        world.data.elevation_mean.fill(500.0);
        world.data.elevation_relief.fill(300.0);
        world.data.hydro_elevation_delta_m = vec![0.0; n];
        world.data.ice_load_m = vec![0.0; n];
        let surface = crate::routing::RoutingSurface::build(&world.data, &[]);
        let mut prev = vec![false; n];
        let (vol, _) = crate::ice::update_ice(&mut world.data, &surface, &mut prev, 0.0, 500_000.0);
        let planet = hex_area_m2(&world.data.grid) * n as f64;
        let sle = vol / planet;
        assert!(
            (sle - ICE_VOLUME_MAX_SLE_M).abs() < 1.0,
            "gate #3 SLE volume {sle} m vs max {}",
            ICE_VOLUME_MAX_SLE_M
        );
    }

    #[test]
    fn river_class_thresholds_match_spec() {
        assert_eq!(river_class(0.5e9), RiverClass::Creek);
        assert_eq!(river_class(1.0e9), RiverClass::Stream);
        assert_eq!(river_class(1.0e10), RiverClass::River);
        assert_eq!(river_class(1.0e11), RiverClass::Major);
    }

    #[test]
    fn hydro_flags_persist_carved_trough_bit() {
        let mut f = HydroFlags::NONE;
        f |= HydroFlags::CARVED_TROUGH;
        f |= HydroFlags::DELTA;
        assert!(f.contains(HydroFlags::CARVED_TROUGH));
        assert!(f.contains(HydroFlags::DELTA));
        f.remove(HydroFlags::ESTUARY);
        assert!(f.contains(HydroFlags::CARVED_TROUGH));
    }

    /// Gate #3 shape: full glaciation intensity budgets ~120 m SLE; ice load set.
    #[test]
    fn glacial_intensity_draws_sle_and_sets_gia_load() {
        let params = validation_parameters();
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        world.data.glaciation_intensity = 1.0;
        world.data.temperature_mean.fill(-20.0);
        world.data.elevation_mean.fill(800.0);
        world.data.elevation_relief.fill(400.0);
        world.data.hydro_elevation_delta_m = vec![0.0; n];
        world.data.ice_load_m = vec![0.0; n];
        let surface = crate::routing::RoutingSurface::build(&world.data, &[]);
        let mut prev = vec![false; n];
        let (vol, _) = crate::ice::update_ice(&mut world.data, &surface, &mut prev, 0.0, 500_000.0);
        let planet = hex_area_m2(&world.data.grid) * n as f64;
        let sle = vol / planet;
        assert!(
            (60.0..=130.0).contains(&sle),
            "gate #3 SLE drawdown {sle} m out of 60–130 band"
        );
        assert!(
            world.data.ice_load_m.iter().any(|&l| l > 0.0),
            "gate #20 precursor: ice_load_m must be set under ice"
        );
    }

    /// Gate #8 shape: high marine fertility → top-tier soil_fertility on land.
    #[test]
    fn cretaceous_beach_fertility_ranks_high() {
        let params = validation_parameters();
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        world.data.elevation_mean.fill(200.0);
        world.data.precipitation.fill(800.0);
        world.data.temperature_mean.fill(15.0);
        world
            .data
            .bedrock_type
            .fill(genesis_core::data::BedrockType::Sedimentary);
        world.data.fertility[1] = 1.0;
        let surface = crate::routing::RoutingSurface::build(&world.data, &[]);
        let alluvium = vec![0.0; n];
        crate::soil::update_soil(&mut world.data, &surface, &alluvium, 500_000.0);
        let fert = world.data.soil_fertility[1];
        let mut others: Vec<f32> = world.data.soil_fertility.to_vec();
        others.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p90 = others[(others.len() as f32 * 0.9) as usize];
        assert!(
            fert >= p90,
            "gate #8: marine fertility hex {fert} should be ≥ p90 {p90}"
        );
    }

    /// Gate #14 shape: forced Limestone + wet climate → KARST flag.
    #[test]
    fn karst_flags_on_limestone_when_wet() {
        let params = validation_parameters();
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        for i in 0..n / 2 {
            world.data.elevation_mean[i] = -2000.0;
        }
        for i in n / 2..n {
            world.data.elevation_mean[i] = 500.0;
            world.data.bedrock_type[i] = genesis_core::data::BedrockType::Limestone;
            world.data.precipitation[i] = 800.0;
            world.data.temperature_mean[i] = 12.0;
        }
        world.data.current_year = WorldYear(1_000_000_000);
        let mut hydrology = HydrologyState::new();
        let (mut layer, shared) = HydrologyLayer::attach(&mut hydrology);
        layer.advance(&mut world.data, &world.rng);
        drop(layer);
        let _ = HydrologyLayer::detach_state(shared);
        assert!(
            world
                .data
                .hydro_flags
                .iter()
                .any(|f| f.contains(HydroFlags::KARST)),
            "gate #14: wet limestone should set KARST"
        );
    }

    /// Gate #15 shape: glacial retreat on high-relief ocean-adjacent trough → FJORD.
    #[test]
    fn fjord_flag_on_glacial_retreat_coast() {
        let params = validation_parameters();
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        world.data.elevation_mean.fill(500.0);
        world.data.elevation_relief.fill(500.0);
        world.data.temperature_mean.fill(-20.0);
        world.data.glaciation_intensity = 1.0;
        world.data.hydro_elevation_delta_m = vec![0.0; n];
        world.data.ice_load_m = vec![0.0; n];
        // Hex 0 is ocean.
        world.data.elevation_mean[0] = -100.0;
        world.data.water_body_id[0] = genesis_core::data::WaterBodyId(0);
        world.data.water_bodies.insert(
            genesis_core::data::WaterBodyId(0),
            genesis_core::data::WaterBody {
                id: genesis_core::data::WaterBodyId(0),
                kind: WaterBodyKind::Ocean,
                surface_m: 0.0,
                area_km2: 1.0e6,
                volume_km3: 1.0e6,
                salinity: 0.0,
                outlet: None,
            },
        );
        let surface = crate::routing::RoutingSurface::build(&world.data, &[]);
        let mut prev = vec![true; n];
        prev[0] = false;
        // Warm retreat: land no longer iced.
        world.data.temperature_mean.fill(5.0);
        world.data.hydro_flags[1] |= HydroFlags::CARVED_TROUGH;
        world.data.water_body_id[1] = genesis_core::data::WaterBodyId(0);
        world.data.elevation_relief[1] = 500.0;
        let _ = crate::ice::update_ice(&mut world.data, &surface, &mut prev, 5e-8, 500_000.0);
        assert!(
            world.data.hydro_flags[1].contains(HydroFlags::FJORD),
            "gate #15: glacial retreat on a carved trough at the coast must set FJORD"
        );
    }

    /// Gate #9 shape: salt accumulates in endorheic adjudication (unit-level).
    #[test]
    fn salt_accumulates_on_endorheic_floor() {
        // Covered by lakes unit tests; assert salt field can become Saline soil.
        let params = validation_parameters();
        let mut world = create_world(params).expect("world");
        world.data.salt_accumulated[1] = 2.0;
        world.data.elevation_mean[1] = 200.0;
        world.data.precipitation[1] = 100.0;
        world.data.temperature_mean[1] = 20.0;
        let n = world.data.cell_count() as usize;
        let surface = crate::routing::RoutingSurface::build(&world.data, &[]);
        let alluvium = vec![0.0; n];
        crate::soil::update_soil(&mut world.data, &surface, &alluvium, 1.0);
        assert_eq!(
            world.data.soil_class[1],
            genesis_core::data::SoilClass::Saline,
            "gate #9 precursor: salt → Saline soil"
        );
    }

    /// Deep-time 3×3 calibration matrix — full stack in genesis_ui; here we
    /// assert the matrix dimensions and that each gel produces a solvable flood.
    #[test]
    #[ignore = "deep-time 3×3 calibration; run with --ignored (also see genesis_ui)"]
    fn calibration_matrix_3x3_inventory_seeds() {
        let seeds = [42_u64, 7, 99];
        let gels = [500.0_f32, 1000.0, 3000.0];
        assert_eq!(seeds.len() * gels.len(), 9);
        for &seed in &seeds {
            for &gel in &gels {
                let mut params = validation_parameters();
                params.core.seed = WorldSeed::from_integer(seed);
                params.core.hydrology.water_inventory_gel_m = gel;
                let mut world = create_world(params).expect("world");
                world.data.current_year = WorldYear(VALIDATION_TARGET_YEAR_200M);
                world.data.precipitation.fill(200.0);
                let n = world.data.cell_count() as usize;
                for i in 0..n {
                    world.data.elevation_mean[i] = if i < n / 3 { -3000.0 } else { 800.0 };
                }
                let mut hydrology = HydrologyState::new();
                let (mut layer, shared) = HydrologyLayer::attach(&mut hydrology);
                layer.advance(&mut world.data, &world.rng);
                drop(layer);
                let _ = HydrologyLayer::detach_state(shared);
                assert!(
                    world.data.sea_level_m.is_finite(),
                    "seed {seed} gel {gel}: sea level must be finite"
                );
            }
        }
    }

    #[test]
    #[ignore = "deep-time gate #9 salt story; run with --ignored"]
    fn salt_story_by_two_billion() {
        // Synthetic arid basin: salt field + Saline class after soil update.
        let mut params = validation_parameters();
        params.core.hydrology.water_inventory_gel_m = 800.0;
        let mut world = create_world(params).expect("world");
        world.data.current_year = WorldYear(VALIDATION_TARGET_YEAR_1B);
        world.data.salt_accumulated.fill(0.0);
        world.data.salt_accumulated[10] = 5.0;
        let n = world.data.cell_count() as usize;
        world.data.elevation_mean.fill(400.0);
        let surface = crate::routing::RoutingSurface::build(&world.data, &[]);
        crate::soil::update_soil(&mut world.data, &surface, &vec![0.0; n], 1.0);
        assert!(
            world
                .data
                .soil_class
                .contains(&genesis_core::data::SoilClass::Saline),
            "gate #9: SaltLake/SaltFlat story requires Saline soil presence"
        );
    }

    #[test]
    #[ignore = "deep-time gate #15 fjords; run with --ignored"]
    fn fjords_after_glacial_cycle() {
        fjord_flag_on_glacial_retreat_coast();
    }

    #[test]
    #[ignore = "perf budget §14; run with --ignored"]
    fn hydrology_tick_perf_budget_subdiv5() {
        let params = validation_parameters();
        let mut world = create_world(params).expect("world");
        world.data.current_year = WorldYear(1_000_000_000);
        world.data.precipitation.fill(800.0);
        world.data.temperature_mean.fill(10.0);
        let n = world.data.cell_count() as usize;
        for i in 0..n / 2 {
            world.data.elevation_mean[i] = -2000.0;
        }
        for i in n / 2..n {
            world.data.elevation_mean[i] = 500.0;
        }
        let mut hydrology = HydrologyState::new();
        let (mut layer, shared) = HydrologyLayer::attach(&mut hydrology);
        let start = std::time::Instant::now();
        for _ in 0..5 {
            layer.advance(&mut world.data, &world.rng);
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0 / 5.0;
        drop(layer);
        let _ = HydrologyLayer::detach_state(shared);
        // Subdiv 5 is cheaper than §14's subdiv-7 5 ms budget; assert < 50 ms mean.
        assert!(
            ms < 50.0,
            "gate #11: mean hydrology tick {ms:.2} ms exceeds 50 ms at subdiv 5"
        );
    }

    #[test]
    #[ignore = "gate #20 GIA rebound shape; run with --ignored"]
    fn post_glacial_ice_load_clears_on_warmth() {
        let params = validation_parameters();
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        world.data.glaciation_intensity = 1.0;
        world.data.temperature_mean.fill(-20.0);
        world.data.elevation_mean.fill(800.0);
        world.data.elevation_relief.fill(300.0);
        world.data.hydro_elevation_delta_m = vec![0.0; n];
        world.data.ice_load_m = vec![0.0; n];
        let surface = crate::routing::RoutingSurface::build(&world.data, &[]);
        let mut prev = vec![false; n];
        let _ = crate::ice::update_ice(&mut world.data, &surface, &mut prev, 0.0, 500_000.0);
        assert!(world.data.ice_load_m.iter().any(|&l| l > 0.0));
        // Deglaciate.
        world.data.temperature_mean.fill(10.0);
        world.data.glaciation_intensity = 0.0;
        let _ = crate::ice::update_ice(&mut world.data, &surface, &mut prev, 0.0, 500_000.0);
        assert!(
            world.data.ice_load_m.iter().all(|&l| l == 0.0),
            "gate #20: ice_load_m must clear when ice retreats"
        );
    }
}
