//! Erosion, sediment routing, and shallow-sea fertility (Doc 06 §8).

use std::collections::BTreeMap;

use genesis_core::HexId;
use genesis_core::data::{BedrockType, WorldData};
use genesis_core::grid::HexGrid;
use genesis_core::rng::WorldRng;
use genesis_core::time::WorldYear;
use rand::Rng;

use crate::plate::{PlateRegistry, TectonicsState};
use crate::plate_surface::{modify_surface_at_world_hex, surface_elevation_at};

/// Per-tick erosion variation stream (§4.4).
pub const EROSION_NOISE_STREAM: &str = "tectonics.erosion_noise";

/// Cumulative deposited material before bedrock becomes `Sedimentary` (§8.3).
pub const DEPOSITION_THRESHOLD_M: f32 = 500.0;

/// Fertility increment per Geological tick for qualifying shallow tropical seas (§8.4).
pub const FERTILITY_INCREMENT_PER_TICK: f32 = 0.001;

/// |latitude| below this (degrees) counts as tropical/subtropical (§8.4).
pub const TROPICAL_LATITUDE_DEG: f64 = 30.0;

/// Maximum water depth for shallow-shelf fertility proxy (§8.4).
pub const SHALLOW_SEA_DEPTH_M: f32 = 200.0;

/// Per-hex multiplicative noise amplitude: factor ∈ [1 - A, 1 + A] (Phase 1).
const EROSION_NOISE_AMPLITUDE: f64 = 0.05;

/// Phase 1 stub: uniform modifier until climate drives precipitation (§8.2).
pub fn climate_modifier_phase1(_data: &WorldData, _hex: HexId) -> f64 {
    1.0
}

/// Erosion rate multiplier per bedrock type. Continental cratons resist erosion;
/// sedimentary basins erode faster. Oceanic crust is underwater (no surface erosion).
fn bedrock_erosion_multiplier(bedrock: BedrockType) -> f64 {
    match bedrock {
        // Continental cratons (Igneous) erode slowly over billion-year scales.
        BedrockType::Igneous => 0.10,
        BedrockType::Metamorphic => 0.25,
        BedrockType::Sedimentary => 1.2,
        BedrockType::Limestone => 1.0,
        BedrockType::OceanicCrust => 0.0,
        BedrockType::Unknown => 1.0,
    }
}

/// Ensures `TectonicsState::cumulative_deposition_m` matches grid cell count.
pub fn ensure_deposition_buffer(state: &mut TectonicsState, cell_count: usize) {
    if state.cumulative_deposition_m.len() != cell_count {
        state.cumulative_deposition_m = vec![0.0; cell_count];
    }
}

/// Erodes land hexes above sea level; returns eroded mass (m) per source hex in `HexId` order.
///
/// Relief is scaled by the remaining elevation fraction above sea level so peaks flatten
/// proportionally. Erosion does not drive land below `sea_level_m`.
pub fn apply_land_erosion(
    data: &WorldData,
    registry: &mut PlateRegistry,
    tick_interval_years: f64,
    base_rate_per_year: f64,
    noise_factors: &BTreeMap<HexId, f64>,
    tick_year: WorldYear,
) -> BTreeMap<HexId, f64> {
    let mut eroded = BTreeMap::new();
    let sea = data.sea_level_m;
    let tick_value = tick_year.value();

    for hex in data.grid.iter() {
        let idx = hex.0 as usize;
        let elev = data.elevation_mean[idx];
        let elev_above = elev - sea;
        if elev_above <= 0.0 {
            continue;
        }

        let bedrock_mult = bedrock_erosion_multiplier(data.bedrock_type[idx]);
        let climate = climate_modifier_phase1(data, hex);
        let noise = noise_factors.get(&hex).copied().unwrap_or(1.0);
        let raw = f64::from(elev_above)
            * base_rate_per_year
            * climate
            * tick_interval_years
            * noise
            * bedrock_mult;
        let amount = raw.min(f64::from(elev_above));
        if amount <= 0.0 {
            continue;
        }

        let amount_f32 = amount as f32;
        modify_surface_at_world_hex(registry, data, hex, tick_value, |feature| {
            feature.elevation_m -= amount_f32;
            let remaining_above = elev_above - amount_f32;
            if elev_above > 0.0 {
                feature.relief_m *= remaining_above / elev_above;
            }
        });

        eroded.insert(hex, amount);
    }

    eroded
}

/// Picks the lowest-elevation neighbor; tie-break lowest `HexId`.
pub fn lowest_elevation_neighbor(grid: &HexGrid, data: &WorldData, hex: HexId) -> Option<HexId> {
    let neighbors = grid.neighbors(hex);
    if neighbors.is_empty() {
        return None;
    }

    let mut best = neighbors[0];
    let mut best_elev = data.elevation_mean[best.0 as usize];

    for &neighbor in &neighbors[1..] {
        let elev = data.elevation_mean[neighbor.0 as usize];
        if elev < best_elev - f32::EPSILON {
            best_elev = elev;
            best = neighbor;
        } else if (elev - best_elev).abs() <= f32::EPSILON && neighbor < best {
            best = neighbor;
        }
    }

    Some(best)
}

/// Routes eroded mass to lowest neighbors and updates deposition / bedrock (§8.2–§8.3).
pub fn route_eroded_mass(
    data: &WorldData,
    cumulative_deposition_m: &mut [f32],
    registry: &mut PlateRegistry,
    eroded_per_hex: &BTreeMap<HexId, f64>,
    tick_year: WorldYear,
) {
    let tick_value = tick_year.value();
    for (&source, &mass) in eroded_per_hex {
        let Some(target) = lowest_elevation_neighbor(&data.grid, data, source) else {
            continue;
        };
        if mass <= 0.0 {
            continue;
        }

        let idx = target.0 as usize;
        cumulative_deposition_m[idx] += mass as f32;

        if cumulative_deposition_m[idx] > DEPOSITION_THRESHOLD_M {
            modify_surface_at_world_hex(registry, data, target, tick_value, |feature| {
                if matches!(
                    feature.bedrock,
                    BedrockType::Igneous | BedrockType::Metamorphic
                ) {
                    feature.bedrock = BedrockType::Sedimentary;
                }
            });
        }
    }
}

/// Increments `fertility` for shallow tropical submerged hexes (§8.4); never decreases.
pub fn increment_shallow_tropical_fertility(
    data: &WorldData,
    registry: &mut PlateRegistry,
    tick_year: WorldYear,
) {
    let sea = data.sea_level_m;
    let lat_limit_rad = TROPICAL_LATITUDE_DEG.to_radians();
    let tick_value = tick_year.value();

    for hex in data.grid.iter() {
        let Some(elevation_m) = surface_elevation_at(data, registry, hex) else {
            continue;
        };
        if elevation_m >= sea {
            continue;
        }

        let (lat_rad, _) = data.grid.center_lat_lon(hex);
        if lat_rad.abs() >= lat_limit_rad {
            continue;
        }

        let depth_m = sea - elevation_m;
        if depth_m >= SHALLOW_SEA_DEPTH_M {
            continue;
        }

        modify_surface_at_world_hex(registry, data, hex, tick_value, |feature| {
            feature.fertility = (feature.fertility + FERTILITY_INCREMENT_PER_TICK).min(1.0);
        });
    }
}

/// Builds per-hex erosion noise multipliers from `tectonics.erosion_noise` at this tick year.
pub fn erosion_noise_factors(
    data: &WorldData,
    rng: &WorldRng,
    tick_year: WorldYear,
) -> BTreeMap<HexId, f64> {
    let mut noise_rng = rng.stream_at(EROSION_NOISE_STREAM, tick_year.value() as u64);
    let mut factors = BTreeMap::new();
    for hex in data.grid.iter() {
        let u: f64 = noise_rng.gen_range(0.0..1.0);
        let factor = 1.0 + (u * 2.0 - 1.0) * EROSION_NOISE_AMPLITUDE;
        factors.insert(hex, factor);
    }
    factors
}

/// Geological-tick erosion, sediment routing, and fertility; ends with `clamp_terrain`.
pub fn apply_erosion_tick(
    data: &WorldData,
    state: &mut TectonicsState,
    rng: &WorldRng,
    tick_year: WorldYear,
    tick_interval_years: f64,
) {
    ensure_deposition_buffer(state, data.grid.cell_count() as usize);

    let base_rate = data.parameters.core.geology.base_erosion_rate_per_year;
    if base_rate <= 0.0 {
        increment_shallow_tropical_fertility(data, &mut state.registry, tick_year);
        return;
    }

    let noise = erosion_noise_factors(data, rng, tick_year);
    let eroded = apply_land_erosion(
        data,
        &mut state.registry,
        tick_interval_years,
        base_rate,
        &noise,
        tick_year,
    );
    route_eroded_mass(
        data,
        &mut state.cumulative_deposition_m,
        &mut state.registry,
        &eroded,
        tick_year,
    );
    increment_shallow_tropical_fertility(data, &mut state.registry, tick_year);
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::time::WorldYear;
    use genesis_core::{HexGrid, PlateId, create_world};

    use crate::plate::{Plate, PlateRegistry, PlateType, TectonicsState};
    use crate::plate_surface::SurfaceFeature;
    use crate::world_rebuild::rebuild_world_from_plate_surfaces;

    const EARTH_RADIUS_KM: f64 = 6371.0;

    fn small_world() -> genesis_core::World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        create_world(params).expect("world")
    }

    fn assign_single_plate(data: &mut WorldData, registry: &mut PlateRegistry) {
        let cell_count = data.cell_count() as usize;
        if registry.count() == 0 {
            registry.insert(Plate::test_plate(
                0,
                PlateType::Continental,
                0,
                1e-8,
                cell_count,
            ));
        }
        for pid in &mut data.plate_id {
            *pid = PlateId(0);
        }
        seed_surfaces_from_world(data, registry);
    }

    fn seed_surfaces_from_world(data: &WorldData, registry: &mut PlateRegistry) {
        for hex in data.grid.iter() {
            let idx = hex.0 as usize;
            let plate_id = data.plate_id[idx];
            let Some(plate) = registry.plates_mut().get_mut(&plate_id) else {
                continue;
            };
            plate.surface.set(
                hex,
                SurfaceFeature {
                    elevation_m: data.elevation_mean[idx],
                    relief_m: data.elevation_relief[idx],
                    bedrock: data.bedrock_type[idx],
                    fertility: data.fertility[idx],
                    age_year: 0,
                },
            );
        }
    }

    fn erode_and_rebuild(
        data: &mut WorldData,
        registry: &mut PlateRegistry,
        interval: f64,
        rate: f64,
        factors: &BTreeMap<HexId, f64>,
    ) -> BTreeMap<HexId, f64> {
        let eroded =
            apply_land_erosion(data, registry, interval, rate, factors, WorldYear(500_000));
        rebuild_world_from_plate_surfaces(data, registry);
        eroded
    }

    #[test]
    fn land_above_sea_erodes_submerged_does_not() {
        let mut world = small_world();
        let data = &mut world.data;
        let mut registry = PlateRegistry::new();
        assign_single_plate(data, &mut registry);
        let hex_land = HexId(50);
        let hex_sea = HexId(51);
        data.sea_level_m = 0.0;
        data.elevation_mean[hex_land.0 as usize] = 1000.0;
        data.elevation_mean[hex_sea.0 as usize] = -100.0;
        data.elevation_relief[hex_land.0 as usize] = 200.0;
        seed_surfaces_from_world(data, &mut registry);

        let mut factors = BTreeMap::new();
        factors.insert(hex_land, 1.0);

        let eroded = erode_and_rebuild(data, &mut registry, 500_000.0, 1e-7, &factors);
        assert!(eroded.get(&hex_land).copied().unwrap_or(0.0) > 0.0);
        assert!(data.elevation_mean[hex_land.0 as usize] < 1000.0);
        assert_eq!(data.elevation_mean[hex_sea.0 as usize], -100.0);
        assert!(!eroded.contains_key(&hex_sea));
    }

    #[test]
    fn zero_base_rate_no_erosion() {
        let mut world = small_world();
        let data = &mut world.data;
        let mut registry = PlateRegistry::new();
        assign_single_plate(data, &mut registry);
        let hex = HexId(10);
        data.sea_level_m = 0.0;
        data.elevation_mean[hex.0 as usize] = 500.0;
        seed_surfaces_from_world(data, &mut registry);

        let eroded = erode_and_rebuild(data, &mut registry, 500_000.0, 0.0, &BTreeMap::new());
        assert!(eroded.is_empty());
        assert_eq!(data.elevation_mean[hex.0 as usize], 500.0);
    }

    #[test]
    fn erosion_map_is_deterministic() {
        let mut world_a = small_world();
        let mut world_b = small_world();
        let hex = HexId(20);
        let mut reg_a = PlateRegistry::new();
        let mut reg_b = PlateRegistry::new();
        assign_single_plate(&mut world_a.data, &mut reg_a);
        assign_single_plate(&mut world_b.data, &mut reg_b);
        for data in [&mut world_a.data, &mut world_b.data] {
            data.sea_level_m = 0.0;
            data.elevation_mean[hex.0 as usize] = 3000.0;
        }
        seed_surfaces_from_world(&world_a.data, &mut reg_a);
        seed_surfaces_from_world(&world_b.data, &mut reg_b);

        let factors = BTreeMap::from([(hex, 1.0)]);
        let a = erode_and_rebuild(&mut world_a.data, &mut reg_a, 500_000.0, 1e-7, &factors);
        let b = erode_and_rebuild(&mut world_b.data, &mut reg_b, 500_000.0, 1e-7, &factors);
        assert_eq!(a, b);
        assert_eq!(
            world_a.data.elevation_mean[hex.0 as usize],
            world_b.data.elevation_mean[hex.0 as usize]
        );
    }

    #[test]
    fn routing_deposits_on_lowest_neighbor() {
        let mut world = small_world();
        let data = &mut world.data;
        let mut registry = PlateRegistry::new();
        assign_single_plate(data, &mut registry);
        let high = HexId(30);
        let low = HexId(31);
        data.sea_level_m = 0.0;
        data.elevation_mean[high.0 as usize] = 500.0;
        data.elevation_mean[low.0 as usize] = 100.0;
        seed_surfaces_from_world(data, &mut registry);

        let grid = &data.grid;
        let neighbors: Vec<_> = grid.neighbors(high).to_vec();
        if !neighbors.contains(&low) {
            let target = lowest_elevation_neighbor(grid, data, high).expect("neighbor");
            let mut state = TectonicsState::new();
            state.registry = registry;
            ensure_deposition_buffer(&mut state, data.grid.cell_count() as usize);
            let eroded = BTreeMap::from([(high, 10.0)]);
            route_eroded_mass(
                data,
                &mut state.cumulative_deposition_m,
                &mut state.registry,
                &eroded,
                WorldYear(500_000),
            );
            assert!(state.cumulative_deposition_m[target.0 as usize] > 0.0);
            return;
        }

        let mut state = TectonicsState::new();
        state.registry = registry;
        ensure_deposition_buffer(&mut state, data.grid.cell_count() as usize);
        route_eroded_mass(
            data,
            &mut state.cumulative_deposition_m,
            &mut state.registry,
            &BTreeMap::from([(high, 25.0)]),
            WorldYear(500_000),
        );
        assert_eq!(state.cumulative_deposition_m[low.0 as usize], 25.0);
    }

    #[test]
    fn tie_break_picks_lowest_hex_id_among_equal_neighbors() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let params = WorldParameters::default();
        let mut data = WorldData::new(grid, params);
        let mut registry = PlateRegistry::new();
        assign_single_plate(&mut data, &mut registry);
        data.sea_level_m = 0.0;

        let cell_count = data.grid.cell_count() as usize;
        for high in data.grid.iter().collect::<Vec<_>>() {
            let neighbors: Vec<_> = data.grid.neighbors(high).to_vec();
            if neighbors.len() < 2 {
                continue;
            }
            data.elevation_mean[high.0 as usize] = 500.0;
            for &n in &neighbors {
                data.elevation_mean[n.0 as usize] = 100.0;
            }
            seed_surfaces_from_world(&data, &mut registry);
            let expected = *neighbors.iter().min().expect("neighbors");
            assert_eq!(
                lowest_elevation_neighbor(&data.grid, &data, high),
                Some(expected)
            );

            let mut state = TectonicsState::new();
            state.registry = registry;
            ensure_deposition_buffer(&mut state, cell_count);
            route_eroded_mass(
                &data,
                &mut state.cumulative_deposition_m,
                &mut state.registry,
                &BTreeMap::from([(high, 5.0)]),
                WorldYear(500_000),
            );
            assert_eq!(state.cumulative_deposition_m[expected.0 as usize], 5.0);
            return;
        }
        panic!("no hex with two neighbors found");
    }

    #[test]
    fn deposition_threshold_sets_sedimentary_bedrock() {
        let mut world = small_world();
        let data = &mut world.data;
        let mut registry = PlateRegistry::new();
        assign_single_plate(data, &mut registry);
        data.sea_level_m = 0.0;

        let mut state = TectonicsState::new();
        state.registry = registry;
        ensure_deposition_buffer(&mut state, data.grid.cell_count() as usize);

        for high in data.grid.iter().collect::<Vec<_>>() {
            let neighbors: Vec<_> = data.grid.neighbors(high).to_vec();
            if neighbors.is_empty() {
                continue;
            }
            let low = *neighbors.iter().min().expect("neighbor");
            data.elevation_mean[high.0 as usize] = 800.0;
            for &n in &neighbors {
                data.elevation_mean[n.0 as usize] = if n == low { 0.0 } else { 10_000.0 };
            }
            data.bedrock_type[low.0 as usize] = BedrockType::Igneous;
            seed_surfaces_from_world(data, &mut state.registry);
            state.cumulative_deposition_m[low.0 as usize] = DEPOSITION_THRESHOLD_M - 1.0;
            route_eroded_mass(
                data,
                &mut state.cumulative_deposition_m,
                &mut state.registry,
                &BTreeMap::from([(high, 2.0)]),
                WorldYear(500_000),
            );
            rebuild_world_from_plate_surfaces(data, &state.registry);
            assert_eq!(lowest_elevation_neighbor(&data.grid, data, high), Some(low));
            assert_eq!(data.bedrock_type[low.0 as usize], BedrockType::Sedimentary);
            return;
        }
        panic!("no adjacent hex pair");
    }

    #[test]
    fn tropical_shallow_sea_gains_fertility() {
        let mut world = small_world();
        let data = &mut world.data;
        let mut registry = PlateRegistry::new();
        assign_single_plate(data, &mut registry);
        data.sea_level_m = 0.0;

        let mut found = false;
        for hex in data.grid.iter().collect::<Vec<_>>() {
            let (lat, _) = data.grid.center_lat_lon(hex);
            if lat.abs() >= TROPICAL_LATITUDE_DEG.to_radians() {
                continue;
            }
            let idx = hex.0 as usize;
            data.elevation_mean[idx] = -50.0;
            seed_surfaces_from_world(data, &mut registry);
            let before = data.fertility[idx];
            increment_shallow_tropical_fertility(data, &mut registry, WorldYear(500_000));
            rebuild_world_from_plate_surfaces(data, &registry);
            assert!(
                data.fertility[idx] > before,
                "tropical shallow hex {hex:?} should gain fertility"
            );
            found = true;
            break;
        }
        assert!(found, "grid should have a tropical hex");
    }

    #[test]
    fn land_hex_does_not_gain_fertility() {
        let mut world = small_world();
        let data = &mut world.data;
        let mut registry = PlateRegistry::new();
        assign_single_plate(data, &mut registry);
        let hex = HexId(5);
        data.sea_level_m = 0.0;
        data.elevation_mean[hex.0 as usize] = 100.0;
        seed_surfaces_from_world(data, &mut registry);
        let before = data.fertility[hex.0 as usize];
        increment_shallow_tropical_fertility(data, &mut registry, WorldYear(500_000));
        rebuild_world_from_plate_surfaces(data, &registry);
        assert_eq!(data.fertility[hex.0 as usize], before);
    }

    #[test]
    fn fertility_never_decreases_when_hex_rises() {
        let mut world = small_world();
        let data = &mut world.data;
        let mut registry = PlateRegistry::new();
        assign_single_plate(data, &mut registry);
        data.sea_level_m = 0.0;

        for hex in data.grid.iter().collect::<Vec<_>>() {
            let (lat, _) = data.grid.center_lat_lon(hex);
            if lat.abs() >= TROPICAL_LATITUDE_DEG.to_radians() {
                continue;
            }
            let idx = hex.0 as usize;
            data.elevation_mean[idx] = -50.0;
            seed_surfaces_from_world(data, &mut registry);
            increment_shallow_tropical_fertility(data, &mut registry, WorldYear(500_000));
            rebuild_world_from_plate_surfaces(data, &registry);
            let after_submerged = data.fertility[idx];
            data.elevation_mean[idx] = 500.0;
            seed_surfaces_from_world(data, &mut registry);
            increment_shallow_tropical_fertility(data, &mut registry, WorldYear(500_000));
            rebuild_world_from_plate_surfaces(data, &registry);
            assert_eq!(data.fertility[idx], after_submerged);
            return;
        }
        panic!("no tropical hex for test");
    }

    #[test]
    fn igneous_erodes_less_than_sedimentary() {
        let mut world = small_world();
        let mut world_sed = small_world();
        let hex = HexId(25);
        let mut reg_a = PlateRegistry::new();
        let mut reg_b = PlateRegistry::new();
        assign_single_plate(&mut world.data, &mut reg_a);
        assign_single_plate(&mut world_sed.data, &mut reg_b);
        for data in [&mut world.data, &mut world_sed.data] {
            data.sea_level_m = 0.0;
            data.elevation_mean[hex.0 as usize] = 2000.0;
        }
        world.data.bedrock_type[hex.0 as usize] = BedrockType::Igneous;
        world_sed.data.bedrock_type[hex.0 as usize] = BedrockType::Sedimentary;
        seed_surfaces_from_world(&world.data, &mut reg_a);
        seed_surfaces_from_world(&world_sed.data, &mut reg_b);

        let factors = BTreeMap::from([(hex, 1.0)]);
        erode_and_rebuild(&mut world.data, &mut reg_a, 500_000.0, 1e-7, &factors);
        erode_and_rebuild(&mut world_sed.data, &mut reg_b, 500_000.0, 1e-7, &factors);

        let igneous_elev = world.data.elevation_mean[hex.0 as usize];
        let sedimentary_elev = world_sed.data.elevation_mean[hex.0 as usize];
        assert!(
            igneous_elev > sedimentary_elev,
            "igneous {igneous_elev} should erode less than sedimentary {sedimentary_elev}"
        );
    }

    #[test]
    fn apply_erosion_tick_end_to_end() {
        let mut world = small_world();
        let mut state = TectonicsState::new();
        assign_single_plate(&mut world.data, &mut state.registry);
        let rng = genesis_core::rng::WorldRng::from_effective_seed(42);
        let hex = HexId(25);
        world.data.sea_level_m = 0.0;
        world.data.elevation_mean[hex.0 as usize] = 4000.0;
        seed_surfaces_from_world(&world.data, &mut state.registry);

        apply_erosion_tick(&world.data, &mut state, &rng, WorldYear(500_000), 500_000.0);
        rebuild_world_from_plate_surfaces(&mut world.data, &state.registry);

        assert!(world.data.elevation_mean[hex.0 as usize] < 4000.0);
        assert_eq!(
            state.cumulative_deposition_m.len(),
            world.data.grid.cell_count() as usize
        );
    }
}
