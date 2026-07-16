//! Erosion, sediment routing, and shallow-sea fertility (Doc 06 §8).

use std::collections::BTreeMap;

use genesis_core::HexId;
use genesis_core::data::{BedrockType, WorldData};
use genesis_core::grid::HexGrid;
use genesis_core::rng::WorldRng;
use genesis_core::time::WorldYear;
use rand::Rng;

use crate::plate::{PlateRegistry, TectonicsState};
use crate::plate_surface::{
    continental_crust_at, modify_surface_at_world_hex, surface_elevation_at,
};
use crate::projection::ProjectionCache;

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

/// Isostatic freeboard: land elevation (m above sea level) that continental
/// crust erodes down to and rebounds up toward. Crustal buoyancy offsets
/// denudation on low continents (Earth's mean continental elevation is
/// ~840 m; old cratons sit near 300–600 m). Oceanic-crust land (island arcs,
/// hotspot volcanoes) has no freeboard and erodes to sea level.
pub const CONTINENTAL_FREEBOARD_M: f32 = 550.0;

/// Epeirogenic rebound rate: fraction of the gap to the freeboard closed per
/// year for low or submerged continental crust. At 2e-8/yr a 500k-year tick
/// closes ~1%, so drowned margins re-emerge over ~50–100M years —
/// epicontinental seas are transient, continents are permanent, and enough of
/// the continental crust stands above sea at any moment for an Earthlike
/// (~25–30%) land fraction.
pub const EPEIROGENIC_REBOUND_RATE_PER_YEAR: f64 = 2e-8;

/// Continental crust below this is considered consumed/sutured and does not
/// rebound (m).
pub const EPEIROGENIC_REBOUND_FLOOR_M: f32 = -2000.0;

/// Per-hex multiplicative noise amplitude: factor ∈ [1 - A, 1 + A] (Phase 1).
const EROSION_NOISE_AMPLITUDE: f64 = 0.05;

/// Earth's global mean precipitation (mm/year); modifier 1.0 at this value (§8.2).
pub const EROSION_PRECIPITATION_BASELINE_MM: f32 = 800.0;

/// Clamp range for the precipitation-driven erosion modifier: hyper-arid land
/// still weathers slowly; monsoon belts erode a few times faster, not 15x.
pub const EROSION_CLIMATE_MODIFIER_MIN: f64 = 0.05;
pub const EROSION_CLIMATE_MODIFIER_MAX: f64 = 4.0;

/// Below this mean temperature (°C) land is permanently frozen: little liquid
/// water, mechanical weathering only.
pub const EROSION_FROZEN_TEMPERATURE_C: f32 = -15.0;

/// Erosion multiplier applied to frozen hexes.
pub const EROSION_FROZEN_FACTOR: f64 = 0.3;

/// Whether the climate layer has populated the precipitation field.
/// Tectonics-only runs (validation worlds, Phase 1 tests) leave it all zero
/// and must keep the spec's uniform 1.0 modifier.
pub fn climate_fields_active(data: &WorldData) -> bool {
    data.precipitation.iter().any(|&p| p > 0.0)
}

/// Precipitation- and temperature-driven erosion modifier (Doc 06 §8.2):
/// `precipitation / 800 mm/yr`, clamped, and damped on permanently frozen
/// hexes. Uniform 1.0 while climate is inactive.
pub fn climate_modifier(data: &WorldData, hex: HexId, climate_active: bool) -> f64 {
    if !climate_active {
        return 1.0;
    }
    let i = hex.0 as usize;
    if i >= data.precipitation.len() {
        return 1.0;
    }
    let mut modifier = f64::from(data.precipitation[i] / EROSION_PRECIPITATION_BASELINE_MM)
        .clamp(EROSION_CLIMATE_MODIFIER_MIN, EROSION_CLIMATE_MODIFIER_MAX);
    if data.temperature_mean[i] < EROSION_FROZEN_TEMPERATURE_C {
        modifier *= EROSION_FROZEN_FACTOR;
    }
    modifier
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
    cache: &ProjectionCache,
    tick_interval_years: f64,
    base_rate_per_year: f64,
    noise_factors: &[f64],
    tick_year: WorldYear,
) -> BTreeMap<HexId, f64> {
    let mut eroded = BTreeMap::new();
    let sea = data.sea_level_m;
    let tick_value = tick_year.value();
    let climate_active = climate_fields_active(data);

    for hex in data.grid.iter() {
        let idx = hex.0 as usize;
        let elev = data.elevation_mean[idx];
        let elev_above = elev - sea;
        if elev_above <= 0.0 {
            continue;
        }

        // Net erosion drives continental land toward the isostatic freeboard,
        // not sea level: rebound compensates most denudation on low
        // continents. Oceanic-crust land (arc and hotspot islands) has no
        // buoyant root and erodes all the way to sea level, so abandoned
        // volcanic islands are transient. Without the freeboard,
        // persistent-crust continents grind to sea level within ~100M years.
        let freeboard = if continental_crust_at(data, registry, cache, hex) {
            CONTINENTAL_FREEBOARD_M
        } else {
            0.0
        };
        let erodible = elev_above - freeboard;
        if erodible <= 0.0 {
            continue;
        }

        let bedrock_mult = bedrock_erosion_multiplier(data.bedrock_type[idx]);
        let climate = climate_modifier(data, hex, climate_active);
        let noise = noise_factors.get(idx).copied().unwrap_or(1.0);
        let raw = f64::from(erodible)
            * base_rate_per_year
            * climate
            * tick_interval_years
            * noise
            * bedrock_mult;
        let amount = raw.min(f64::from(erodible));
        if amount <= 0.0 {
            continue;
        }

        let amount_f32 = amount as f32;
        modify_surface_at_world_hex(registry, data, cache, hex, tick_value, |feature| {
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
    cache: &ProjectionCache,
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
            modify_surface_at_world_hex(registry, data, cache, target, tick_value, |feature| {
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
    cache: &ProjectionCache,
    tick_year: WorldYear,
) {
    let sea = data.sea_level_m;
    let lat_limit_rad = TROPICAL_LATITUDE_DEG.to_radians();
    let tick_value = tick_year.value();

    for hex in data.grid.iter() {
        let Some(elevation_m) = surface_elevation_at(data, registry, cache, hex) else {
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

        modify_surface_at_world_hex(registry, data, cache, hex, tick_value, |feature| {
            feature.fertility = (feature.fertility + FERTILITY_INCREMENT_PER_TICK).min(1.0);
        });
    }
}

/// Builds per-hex erosion noise multipliers from `tectonics.erosion_noise` at this tick year.
pub fn erosion_noise_factors(data: &WorldData, rng: &WorldRng, tick_year: WorldYear) -> Vec<f64> {
    let mut noise_rng = rng.stream_at(EROSION_NOISE_STREAM, tick_year.value() as u64);
    let n = data.cell_count() as usize;
    let mut factors = Vec::with_capacity(n);
    for _ in 0..n {
        let u: f64 = noise_rng.gen_range(0.0..1.0);
        factors.push(1.0 + (u * 2.0 - 1.0) * EROSION_NOISE_AMPLITUDE);
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
    let TectonicsState {
        registry,
        cumulative_deposition_m,
        projection,
        ..
    } = state;

    apply_isostasy(registry, data.sea_level_m, tick_interval_years);

    let base_rate = data.parameters.core.geology.base_erosion_rate_per_year;
    if base_rate <= 0.0 {
        increment_shallow_tropical_fertility(data, registry, projection, tick_year);
        return;
    }

    let noise = erosion_noise_factors(data, rng, tick_year);
    let eroded = apply_land_erosion(
        data,
        registry,
        projection,
        tick_interval_years,
        base_rate,
        &noise,
        tick_year,
    );
    route_eroded_mass(
        data,
        cumulative_deposition_m,
        registry,
        projection,
        &eroded,
        tick_year,
    );
    increment_shallow_tropical_fertility(data, registry, projection, tick_year);
}

/// Thermal subsidence rate for submerged oceanic crust (fraction of the
/// remaining gap to [`crate::elevation::OCEAN_FLOOR_BASELINE_M`] per year).
///
/// Young ridge crust cools and sinks toward the abyssal baseline as it ages;
/// at 6e-8/yr a 500k-year tick closes ~3% of the gap, so fresh crust drops
/// below -3000 m within ~6M years and the active ridge line stays narrow
/// instead of walling deep basins apart.
pub const THERMAL_SUBSIDENCE_RATE_PER_YEAR: f64 = 6e-8;

/// Sinks submerged oceanic crust toward the abyssal baseline as it ages, and
/// slowly rebounds low or drowned continental crust toward the isostatic
/// freeboard. Crust type comes from the feature's permanent
/// `continental_crust` flag, so sediment/volcanic bedrock overprints do not
/// confuse it. Deterministic: ascending plate and birth-index order; no RNG.
pub fn apply_isostasy(registry: &mut PlateRegistry, sea_level_m: f32, tick_interval_years: f64) {
    let baseline = crate::elevation::OCEAN_FLOOR_BASELINE_M;
    let sink_fraction = (THERMAL_SUBSIDENCE_RATE_PER_YEAR * tick_interval_years).min(1.0) as f32;
    let rebound_fraction =
        (EPEIROGENIC_REBOUND_RATE_PER_YEAR * tick_interval_years).min(1.0) as f32;
    let freeboard_target = sea_level_m + CONTINENTAL_FREEBOARD_M;

    let plate_ids = registry.plate_ids();
    for plate_id in plate_ids {
        let Some(plate) = registry.plates_mut().get_mut(&plate_id) else {
            continue;
        };
        for slot in plate.surface.features.iter_mut() {
            let Some(feature) = slot else {
                continue;
            };
            if feature.continental_crust {
                // Epeirogenic rebound: buoyant crust rises back toward the
                // freeboard unless it has been consumed into a suture.
                if feature.elevation_m < freeboard_target
                    && feature.elevation_m > EPEIROGENIC_REBOUND_FLOOR_M
                    && rebound_fraction > 0.0
                {
                    feature.elevation_m +=
                        (freeboard_target - feature.elevation_m) * rebound_fraction;
                }
            } else if feature.elevation_m < sea_level_m
                && feature.elevation_m > baseline
                && sink_fraction > 0.0
            {
                // Thermal subsidence: submerged oceanic crust cools and sinks;
                // abandoned volcanic islands erode to sea level first, then
                // subside as guyots.
                feature.elevation_m += (baseline - feature.elevation_m) * sink_fraction;
            }
        }
    }
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

    #[test]
    fn climate_modifier_uniform_when_climate_inactive() {
        let world = small_world();
        assert!(!climate_fields_active(&world.data));
        for hex in world.data.grid.iter().take(20) {
            assert_eq!(climate_modifier(&world.data, hex, false), 1.0);
        }
    }

    #[test]
    fn climate_modifier_scales_with_precipitation_and_freezes() {
        let mut world = small_world();
        let wet = HexId(1);
        let dry = HexId(2);
        let frozen = HexId(3);
        world.data.precipitation[wet.0 as usize] = 1600.0;
        world.data.precipitation[dry.0 as usize] = 80.0;
        world.data.precipitation[frozen.0 as usize] = 800.0;
        world.data.temperature_mean[frozen.0 as usize] = -30.0;
        assert!(climate_fields_active(&world.data));

        let wet_m = climate_modifier(&world.data, wet, true);
        let dry_m = climate_modifier(&world.data, dry, true);
        let frozen_m = climate_modifier(&world.data, frozen, true);

        assert!((wet_m - 2.0).abs() < 1e-6, "1600 mm → 2x, got {wet_m}");
        assert!((dry_m - 0.1).abs() < 1e-6, "80 mm → 0.1x, got {dry_m}");
        assert!(
            (frozen_m - EROSION_FROZEN_FACTOR).abs() < 1e-6,
            "frozen 800 mm hex → frozen factor, got {frozen_m}"
        );
        assert!(wet_m > dry_m);
    }

    #[test]
    fn drowned_continental_crust_rebounds_toward_freeboard() {
        let mut registry = PlateRegistry::new();
        let mut plate = Plate {
            id: PlateId(0),
            plate_type: PlateType::Continental,
            plate_class: crate::plate::PlateClass::Major,
            seed_hex: genesis_core::HexId(0),
            motion_axis: [0.0, 0.0, 1.0],
            motion_rate_rad_per_year: 0.0,
            age_year: WorldYear::FORMATION,
            target_fraction: 0.5,
            accumulated_rotation_rad: 0.0,
            last_nonempty_year: WorldYear::FORMATION,
            surface: crate::plate_surface::PlateSurface::new(4),
        };
        plate.surface.set(
            genesis_core::HexId(0),
            SurfaceFeature {
                elevation_m: -200.0,
                relief_m: 0.0,
                bedrock: genesis_core::data::BedrockType::Igneous,
                fertility: 0.0,
                age_year: 0,
                continental_crust: true,
            },
        );
        registry.insert(plate);

        // 200 ticks of 500k years each: rebound closes ~63% of the gap.
        for _ in 0..200 {
            apply_isostasy(&mut registry, 0.0, 500_000.0);
        }
        let elev = registry
            .get(PlateId(0))
            .unwrap()
            .surface
            .get(genesis_core::HexId(0))
            .unwrap()
            .elevation_m;
        assert!(
            elev > 0.0,
            "drowned continental margin should re-emerge over ~100M years, got {elev}"
        );
        assert!(elev < CONTINENTAL_FREEBOARD_M, "asymptotic, not instant");
    }

    #[test]
    fn abandoned_oceanic_island_subsides_once_submerged() {
        let mut registry = PlateRegistry::new();
        let mut plate = Plate {
            id: PlateId(0),
            plate_type: PlateType::Oceanic,
            plate_class: crate::plate::PlateClass::Major,
            seed_hex: genesis_core::HexId(0),
            motion_axis: [0.0, 0.0, 1.0],
            motion_rate_rad_per_year: 0.0,
            age_year: WorldYear::FORMATION,
            target_fraction: 0.5,
            accumulated_rotation_rad: 0.0,
            last_nonempty_year: WorldYear::FORMATION,
            surface: crate::plate_surface::PlateSurface::new(4),
        };
        // A guyot: volcanic island already eroded just below sea level.
        plate.surface.set(
            genesis_core::HexId(0),
            SurfaceFeature {
                elevation_m: -5.0,
                relief_m: 0.0,
                bedrock: genesis_core::data::BedrockType::Igneous,
                fertility: 0.0,
                age_year: 0,
                continental_crust: false,
            },
        );
        registry.insert(plate);

        for _ in 0..200 {
            apply_isostasy(&mut registry, 0.0, 500_000.0);
        }
        let elev = registry
            .get(PlateId(0))
            .unwrap()
            .surface
            .get(genesis_core::HexId(0))
            .unwrap()
            .elevation_m;
        assert!(
            elev < -2000.0,
            "abandoned volcanic island should subside toward the abyss, got {elev}"
        );
    }

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
                    continental_crust: false,
                },
            );
        }
    }

    fn erode_and_rebuild(
        data: &mut WorldData,
        registry: &mut PlateRegistry,
        interval: f64,
        rate: f64,
    ) -> BTreeMap<HexId, f64> {
        let eroded = apply_land_erosion(
            data,
            registry,
            &ProjectionCache::empty(),
            interval,
            rate,
            &[],
            WorldYear(500_000),
        );
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

        let eroded = erode_and_rebuild(data, &mut registry, 500_000.0, 1e-7);
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

        let eroded = erode_and_rebuild(data, &mut registry, 500_000.0, 0.0);
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

        let a = erode_and_rebuild(&mut world_a.data, &mut reg_a, 500_000.0, 1e-7);
        let b = erode_and_rebuild(&mut world_b.data, &mut reg_b, 500_000.0, 1e-7);
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
                &ProjectionCache::empty(),
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
            &ProjectionCache::empty(),
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
                &ProjectionCache::empty(),
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
                &ProjectionCache::empty(),
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
            increment_shallow_tropical_fertility(
                data,
                &mut registry,
                &ProjectionCache::empty(),
                WorldYear(500_000),
            );
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
        increment_shallow_tropical_fertility(
            data,
            &mut registry,
            &ProjectionCache::empty(),
            WorldYear(500_000),
        );
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
            increment_shallow_tropical_fertility(
                data,
                &mut registry,
                &ProjectionCache::empty(),
                WorldYear(500_000),
            );
            rebuild_world_from_plate_surfaces(data, &registry);
            let after_submerged = data.fertility[idx];
            data.elevation_mean[idx] = 500.0;
            seed_surfaces_from_world(data, &mut registry);
            increment_shallow_tropical_fertility(
                data,
                &mut registry,
                &ProjectionCache::empty(),
                WorldYear(500_000),
            );
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

        erode_and_rebuild(&mut world.data, &mut reg_a, 500_000.0, 1e-7);
        erode_and_rebuild(&mut world_sed.data, &mut reg_b, 500_000.0, 1e-7);

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
