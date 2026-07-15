//! Tectonic simulation for Genesis Engine.
//!
//! Phase 1: plate generation, drift, boundaries, and terrain sculpting.

pub mod boundary;
pub mod boundary_events;
pub mod coast_cleanup;
pub mod elevation;
pub mod erosion;
pub mod events;
pub mod frames;
pub mod history;
pub mod hotspots;
pub mod initial_generation;
pub mod initial_terrain;
pub mod layer;
pub mod motion;
pub mod partition;
pub mod plate;
pub mod plate_surface;
pub mod reorganization;
pub mod sea_level;
pub mod validation;
pub mod volcanism;
pub mod world_rebuild;

pub use boundary::{
    BoundaryClass, BoundaryInfo, ClassifiedEdge, ConvergentSubtype, convergent_subtype,
    detect_and_classify_boundaries,
};
pub use boundary_events::{boundary_type_from_class, emit_boundary_events};
pub use elevation::{
    CC_INLAND_HEXES, COASTAL_SHELF_HEXES, CONTINENTAL_RIFT_SUBSIDENCE_FACTOR, MAX_ELEVATION_M,
    MAX_RELIEF_M, MIN_ELEVATION_M, MOUNTAIN_EQUILIBRIUM_M, OC_INLAND_HEXES, OCEAN_FLOOR_BASELINE_M,
    OROGENY_RATE, SUBDUCTION_RATE, SUBSIDENCE_RATE, TRENCH_EQUILIBRIUM_M, apply_boundary_elevation,
    clamp_terrain, subducting_plate_id,
};
pub use erosion::{
    DEPOSITION_THRESHOLD_M, EROSION_NOISE_STREAM, FERTILITY_INCREMENT_PER_TICK,
    SHALLOW_SEA_DEPTH_M, TROPICAL_LATITUDE_DEG, apply_erosion_tick, apply_land_erosion,
    climate_fields_active, climate_modifier, ensure_deposition_buffer,
    increment_shallow_tropical_fertility, lowest_elevation_neighbor, route_eroded_mass,
};
pub use events::flush_events_to_branch;
pub use frames::{birth_hex_to_current_world, current_world_to_birth_hex};
pub use history::{generate_full_history_with_tectonics, run_formation};
pub use hotspots::{
    ACTIVITY_RATE_MAX, ACTIVITY_RATE_MIN, HOTSPOT_ACTIVITY_STREAM, HOTSPOT_ELEVATION_CHANGE_MAX_M,
    HOTSPOT_ELEVATION_CHANGE_MIN_M, HOTSPOT_LOCATIONS_STREAM, LIFESPAN_MAX_YEARS,
    LIFESPAN_MIN_YEARS, NOTABLE_CUMULATIVE_UPLIFT_M, SPAWN_PROBABILITY_PER_TICK,
    apply_hotspot_tick, generate_initial_hotspots, hex_at_anchor,
};
pub use initial_generation::{generate_initial_plates, generate_initial_plates_data};
pub use initial_terrain::{
    COARSE_NOISE_AMPLITUDE_M, CONTINENTAL_BASE_ELEVATION_M, FINE_NOISE_AMPLITUDE_M,
    INITIAL_ELEVATION_NOISE_STREAM, MEDIUM_NOISE_AMPLITUDE_M, OCEANIC_BASE_ELEVATION_M,
    apply_formation_terrain,
};
pub use layer::{DEFAULT_GEOLOGICAL_TICK_YEARS, TectonicsLayer, geological_tick_interval};
pub use motion::{advance_plate_motion, effective_position_direction, surface_velocity_m_per_year};
pub use partition::repartition_hexes;
pub use plate::{
    HotSpot, HotSpotRegistry, Plate, PlateClass, PlateRegistry, PlateType, TectonicsState,
};
pub use plate_surface::{PlateSurface, SurfaceFeature, baseline_feature, type_baseline};
pub use reorganization::{
    REORGANIZATION_ACTION_STREAM, REORGANIZATION_CHECK_STREAM, maybe_reorganize,
    purge_extinct_plates, update_last_nonempty_years,
};
pub use sea_level::{total_divergent_boundary_length_km, update_sea_level};
pub use validation::{
    CONTINENTAL_FRACTION_MAX, CONTINENTAL_FRACTION_MIN, CONTINENTAL_PERSISTENCE_MIN_FRAC,
    ELEVATION_MAX_BOUND_M, ELEVATION_MIN_BOUND_M, EVENT_COUNT_NOTABLE_MAX_AT_FULL_YEAR,
    EVENT_COUNT_NOTABLE_MAX_DOC, EVENT_COUNT_NOTABLE_MIN, MOUNTAIN_ELEVATION_THRESHOLD_M,
    OCEAN_BASIN_ELEVATION_THRESHOLD_M, PERF_BUDGET_SECS, PERF_TARGET_YEAR, SATURATION_TOLERANCE_M,
    SEA_LEVEL_MAX_ABS_M, VALIDATION_SEED, VALIDATION_SUBDIVISION_LEVEL,
    VALIDATION_TARGET_YEAR_ADVECTION_DRIFT, VALIDATION_TARGET_YEAR_DEEP_PERSISTENCE,
    VALIDATION_TARGET_YEAR_FULL, VALIDATION_TARGET_YEAR_ONE_BILLION, VALIDATION_TARGET_YEAR_QUICK,
    bedrock_types_present, check_phase1_bedrock_diversity, continental_fraction,
    count_saturated_hexes, elevation_bounds, elevation_distribution, event_count_at_granularity,
    format_elevation_distribution, min_ocean_basin_hex_threshold, mountain_regions_above_elevation,
    ocean_basins_below_elevation, peak_elevation_hex, plate_motion_summary, run_validation_world,
    summarize_world, validation_parameters,
};
pub use volcanism::{
    ELEVATION_CHANGE_MAX_M, ELEVATION_CHANGE_MIN_M, ERUPTION_PROBABILITY_BASE,
    NOTABLE_PEAK_THRESHOLD_M, RELIEF_CHANGE_MAX_M, RELIEF_CHANGE_MIN_M, VOLCANISM_STREAM,
    apply_boundary_volcanism, is_arc_hex,
};
pub use world_rebuild::rebuild_world_from_plate_surfaces;

#[cfg(test)]
mod integration_tests {
    use super::*;
    use genesis_core::events::{EventKind, Significance};
    use genesis_core::parameters::WorldParameters;
    use genesis_core::time::WorldYear;
    use genesis_core::{PlateId, create_world};

    use crate::plate::PlateType;

    fn test_world() -> genesis_core::World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        create_world(params).expect("valid world")
    }

    #[test]
    fn formation_populates_all_hexes() {
        let mut world = test_world();
        let mut state = TectonicsState::new();
        run_formation(&mut world, &mut state);
        assert!(state.formation_complete);
        assert!(state.registry.count() >= 13);
        for &plate_id in &world.data.plate_id {
            assert_ne!(plate_id, PlateId::NONE);
            assert!(state.registry.get(plate_id).is_some());
        }
    }

    fn mean_elevation_for_plate_type(
        data: &genesis_core::data::WorldData,
        registry: &PlateRegistry,
        plate_type: PlateType,
    ) -> f32 {
        let mut sum = 0.0_f64;
        let mut count = 0_u64;
        for (i, &plate_id) in data.plate_id.iter().enumerate() {
            if plate_id == PlateId::NONE {
                continue;
            }
            let Some(plate) = registry.get(plate_id) else {
                continue;
            };
            if plate.plate_type != plate_type {
                continue;
            }
            sum += f64::from(data.elevation_mean[i]);
            count += 1;
        }
        if count == 0 {
            0.0
        } else {
            (sum / count as f64) as f32
        }
    }

    #[test]
    fn formation_continental_elevation_above_oceanic() {
        let mut world = test_world();
        let mut state = TectonicsState::new();
        run_formation(&mut world, &mut state);
        let continental =
            mean_elevation_for_plate_type(&world.data, &state.registry, PlateType::Continental);
        let oceanic =
            mean_elevation_for_plate_type(&world.data, &state.registry, PlateType::Oceanic);
        assert!(continental > oceanic, "{continental} vs {oceanic}");
    }

    #[test]
    fn terrain_elevation_deterministic_at_one_million_years() {
        let mut world_a = test_world();
        let mut world_b = test_world();
        let mut state_a = TectonicsState::new();
        let mut state_b = TectonicsState::new();

        generate_full_history_with_tectonics(
            &mut world_a,
            &mut state_a,
            WorldYear(1_000_000),
            |_| {},
        )
        .expect("history a");
        generate_full_history_with_tectonics(
            &mut world_b,
            &mut state_b,
            WorldYear(1_000_000),
            |_| {},
        )
        .expect("history b");

        assert_eq!(world_a.data.elevation_mean, world_b.data.elevation_mean);
        assert_eq!(world_a.data.elevation_relief, world_b.data.elevation_relief);
        assert_eq!(world_a.data.bedrock_type, world_b.data.bedrock_type);
        assert_eq!(world_a.data.fertility, world_b.data.fertility);
    }

    #[test]
    fn terrain_elevation_sanity_at_one_million_years() {
        let mut world = test_world();
        let mut state = TectonicsState::new();
        generate_full_history_with_tectonics(&mut world, &mut state, WorldYear(1_000_000), |_| {})
            .expect("history");

        let min = world
            .data
            .elevation_mean
            .iter()
            .copied()
            .fold(f32::INFINITY, f32::min);
        let max = world
            .data
            .elevation_mean
            .iter()
            .copied()
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(min < -1000.0, "min elevation {min}");
        assert!(max > 0.0, "max elevation {max}");
    }

    #[test]
    fn history_reaches_past_life_emergence_without_stalling() {
        let mut world = test_world();
        let mut state = TectonicsState::new();
        let target = WorldYear(600_000_000);
        generate_full_history_with_tectonics(&mut world, &mut state, target, |_| {})
            .expect("history past life emergence");
        assert_eq!(world.data.current_year, target);
    }

    #[test]
    fn geological_ticks_are_deterministic() {
        let mut world_a = test_world();
        let mut world_b = test_world();
        let mut state_a = TectonicsState::new();
        let mut state_b = TectonicsState::new();

        generate_full_history_with_tectonics(
            &mut world_a,
            &mut state_a,
            WorldYear(1_000_000),
            |_| {},
        )
        .expect("history a");
        generate_full_history_with_tectonics(
            &mut world_b,
            &mut state_b,
            WorldYear(1_000_000),
            |_| {},
        )
        .expect("history b");

        assert_eq!(world_a.data.plate_id, world_b.data.plate_id);
    }

    #[test]
    fn accumulated_geological_ticks_change_some_plate_ids() {
        let mut world = test_world();
        let mut state = TectonicsState::new();
        run_formation(&mut world, &mut state);
        let after_formation = world.data.plate_id.clone();

        // Material footprints move sub-hex per 500k-year tick (~25 km vs ~330 km
        // hexes at subdiv 5); give drift enough ticks to cross hex boundaries.
        generate_full_history_with_tectonics(&mut world, &mut state, WorldYear(20_000_000), |_| {})
            .expect("forty ticks");

        let changed = after_formation
            .iter()
            .zip(world.data.plate_id.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            changed > 0,
            "expected accumulated drift over 20M years to change some hex assignments"
        );
    }

    #[test]
    fn history_advances_current_year() {
        let mut world = test_world();
        let mut state = TectonicsState::new();
        generate_full_history_with_tectonics(&mut world, &mut state, WorldYear(1_000_000), |_| {})
            .expect("history");
        assert_eq!(world.data.current_year, WorldYear(1_000_000));
    }

    #[test]
    fn one_geological_tick_populates_boundaries() {
        let mut world = test_world();
        let mut state = TectonicsState::new();
        run_formation(&mut world, &mut state);
        generate_full_history_with_tectonics(&mut world, &mut state, WorldYear(500_000), |_| {})
            .expect("one tick");
        assert!(
            !state.boundaries.boundary_hexes.is_empty(),
            "expected boundary hexes after one geological tick"
        );
    }

    #[test]
    fn boundaries_are_deterministic_at_one_million_years() {
        let mut world_a = test_world();
        let mut world_b = test_world();
        let mut state_a = TectonicsState::new();
        let mut state_b = TectonicsState::new();

        generate_full_history_with_tectonics(
            &mut world_a,
            &mut state_a,
            WorldYear(1_000_000),
            |_| {},
        )
        .expect("history a");
        generate_full_history_with_tectonics(
            &mut world_b,
            &mut state_b,
            WorldYear(1_000_000),
            |_| {},
        )
        .expect("history b");

        assert_eq!(
            state_a.boundaries.boundary_hexes,
            state_b.boundaries.boundary_hexes
        );
        assert_eq!(
            state_a.boundaries.plate_contacts,
            state_b.boundaries.plate_contacts
        );

        for hex in state_a.boundaries.boundary_hexes {
            let edges_a = state_a.boundaries.edges.get(&hex).expect("edges");
            let edges_b = state_b.boundaries.edges.get(&hex).expect("edges");
            assert_eq!(edges_a.len(), edges_b.len());
            for (a, b) in edges_a.iter().zip(edges_b.iter()) {
                assert_eq!(a.neighbor_hex, b.neighbor_hex);
                assert_eq!(a.other_plate, b.other_plate);
                assert_eq!(a.class, b.class);
            }
        }
    }

    fn mean_land_elevation_m(data: &genesis_core::data::WorldData) -> f32 {
        let sea = data.sea_level_m;
        let mut sum = 0.0_f64;
        let mut count = 0_u64;
        for &elev in &data.elevation_mean {
            if elev > sea {
                sum += f64::from(elev);
                count += 1;
            }
        }
        if count == 0 {
            0.0
        } else {
            (sum / count as f64) as f32
        }
    }

    #[test]
    fn erosion_lowers_mean_land_elevation_over_one_million_years() {
        let mut world_eroding = test_world();
        let mut state_eroding = TectonicsState::new();
        run_formation(&mut world_eroding, &mut state_eroding);
        let mean_after_formation = mean_land_elevation_m(&world_eroding.data);

        generate_full_history_with_tectonics(
            &mut world_eroding,
            &mut state_eroding,
            WorldYear(1_000_000),
            |_| {},
        )
        .expect("history with erosion");
        let mean_after_history = mean_land_elevation_m(&world_eroding.data);

        let mut world_no_erosion = test_world();
        let mut state_no_erosion = TectonicsState::new();
        world_no_erosion
            .data
            .parameters
            .core
            .geology
            .base_erosion_rate_per_year = 0.0;
        run_formation(&mut world_no_erosion, &mut state_no_erosion);
        generate_full_history_with_tectonics(
            &mut world_no_erosion,
            &mut state_no_erosion,
            WorldYear(1_000_000),
            |_| {},
        )
        .expect("history without erosion");

        assert!(
            mean_after_history < mean_after_formation
                || mean_after_history < mean_land_elevation_m(&world_no_erosion.data),
            "erosion should lower land elevations over 1M years (with={mean_after_history}, formation={mean_after_formation}, without={})",
            mean_land_elevation_m(&world_no_erosion.data)
        );
    }

    use crate::erosion::TROPICAL_LATITUDE_DEG;
    use crate::frames::current_world_to_birth_hex;
    use crate::plate_surface::SurfaceFeature;

    fn seed_shallow_tropical_hex(world: &mut genesis_core::World, state: &mut TectonicsState) {
        run_formation(world, state);
        let sea = world.data.sea_level_m;
        let lat_limit = TROPICAL_LATITUDE_DEG.to_radians();
        let shallow_elev = sea - 50.0;

        let hexes: Vec<_> = world.data.grid.iter().collect();
        for hex in hexes {
            let (lat, _) = world.data.grid.center_lat_lon(hex);
            if lat.abs() >= lat_limit {
                continue;
            }
            // A lone shallow pocket enclosed by land is an artifact that coast
            // cleanup rightly fills; seed a geologically justified shelf hex
            // instead — one adjacent to open ocean.
            let touches_open_ocean = world.data.grid.neighbors(hex).iter().any(|n| {
                let j = n.0 as usize;
                j < world.data.elevation_mean.len() && world.data.elevation_mean[j] < sea - 500.0
            });
            if !touches_open_ocean {
                continue;
            }
            let idx = hex.0 as usize;
            let plate_id = world.data.plate_id[idx];
            let birth_hex = {
                let Some(plate) = state.registry.get(plate_id) else {
                    continue;
                };
                current_world_to_birth_hex(&world.data.grid, hex, plate)
            };

            let mut feature = {
                let Some(plate) = state.registry.get(plate_id) else {
                    continue;
                };
                plate
                    .surface
                    .get(birth_hex)
                    .cloned()
                    .unwrap_or(SurfaceFeature {
                        elevation_m: shallow_elev,
                        relief_m: 0.0,
                        bedrock: genesis_core::data::BedrockType::OceanicCrust,
                        fertility: 0.0,
                        age_year: 0,
                        continental_crust: false,
                    })
            };
            feature.elevation_m = shallow_elev;

            if let Some(plate) = state.registry.plates_mut().get_mut(&plate_id) {
                plate.surface.set(birth_hex, feature);
            } else {
                continue;
            }
            rebuild_world_from_plate_surfaces(&mut world.data, &state.registry);
            return;
        }
        panic!("test grid should include a tropical hex");
    }

    #[test]
    fn shallow_tropical_fertility_accumulates_by_one_million_years() {
        let mut world = test_world();
        let mut state = TectonicsState::new();
        seed_shallow_tropical_hex(&mut world, &mut state);
        generate_full_history_with_tectonics(&mut world, &mut state, WorldYear(1_000_000), |_| {})
            .expect("history");

        let max_fertility = world.data.fertility.iter().copied().fold(0.0_f32, f32::max);
        let fertile_count = world.data.fertility.iter().filter(|&&f| f > 0.0).count();
        assert!(
            max_fertility > 0.0 && fertile_count > 0,
            "expected some shallow tropical fertility (max={max_fertility}, count={fertile_count})"
        );
    }

    #[test]
    fn formation_populates_hotspots() {
        let mut world = test_world();
        let mut state = TectonicsState::new();
        run_formation(&mut world, &mut state);
        assert!(
            state.hotspots.count() > 0,
            "Formation should seed mantle hot spots"
        );
    }

    #[test]
    fn history_records_hotspot_activity_with_trace_granularity() {
        let mut world = test_world();
        world.data.parameters.core.geology.event_granularity = Significance::Trace;
        let mut state = TectonicsState::new();
        generate_full_history_with_tectonics(&mut world, &mut state, WorldYear(1_000_000), |_| {})
            .expect("history");

        let count = world
            .branch_tree
            .root()
            .event_log
            .iter()
            .filter(|e| matches!(e.kind, EventKind::HotSpotActivity { .. }))
            .count();
        assert!(
            count > 0,
            "expected at least one HotSpotActivity in root log (got {count})"
        );
    }

    #[test]
    fn history_records_volcanic_eruptions_with_trace_granularity() {
        let mut world = test_world();
        world.data.parameters.core.geology.event_granularity = Significance::Trace;
        world.data.parameters.core.geology.volcanism_scale = 3.0;
        let mut state = TectonicsState::new();
        generate_full_history_with_tectonics(&mut world, &mut state, WorldYear(1_000_000), |_| {})
            .expect("history");

        let eruption_count = world
            .branch_tree
            .root()
            .event_log
            .iter()
            .filter(|e| matches!(e.kind, EventKind::VolcanicEruption { .. }))
            .count();
        assert!(
            eruption_count > 0,
            "expected at least one VolcanicEruption in root log (got {eruption_count})"
        );
    }

    #[test]
    fn formation_emits_world_formation_event() {
        let mut world = test_world();
        let mut state = TectonicsState::new();
        run_formation(&mut world, &mut state);
        flush_events_to_branch(&mut world, &mut state);
        assert!(
            world
                .branch_tree
                .root()
                .event_log
                .iter()
                .any(|e| matches!(e.kind, EventKind::WorldFormation)),
            "expected WorldFormation in root log after formation"
        );
    }

    #[test]
    fn sea_level_bounded_after_formation() {
        // Tectonic sea-level drift is suppressed during 0–500M (climate owns it).
        let mut world = test_world();
        world.data.parameters.core.geology.event_granularity = Significance::Trace;
        let mut state = TectonicsState::new();
        generate_full_history_with_tectonics(
            &mut world,
            &mut state,
            WorldYear(600_000_000),
            |_| {},
        )
        .expect("history");

        assert!(
            world.data.sea_level_m.abs() <= 200.0,
            "post-formation tectonic sea level should stay within ±200 m (got {})",
            world.data.sea_level_m
        );

        let sea_level_events = world
            .branch_tree
            .root()
            .event_log
            .iter()
            .filter(|e| matches!(e.kind, EventKind::SeaLevelChange { .. }))
            .count();
        assert!(
            sea_level_events > 0,
            "expected SeaLevelChange events at Trace granularity (got {sea_level_events})"
        );
    }

    #[test]
    fn plate_count_reasonable_at_one_million_years() {
        let mut world = test_world();
        let mut state = TectonicsState::new();
        generate_full_history_with_tectonics(&mut world, &mut state, WorldYear(1_000_000), |_| {})
            .expect("history");
        let count = state.registry.count();
        assert!(
            (5..=15).contains(&count),
            "plate count at 1M years should be in [5, 15], got {count}"
        );
    }

    #[test]
    fn event_log_deterministic_at_one_million_years() {
        let mut world_a = test_world();
        let mut world_b = test_world();
        let mut state_a = TectonicsState::new();
        let mut state_b = TectonicsState::new();

        world_a.data.parameters.core.geology.event_granularity = Significance::Trace;
        world_b.data.parameters.core.geology.event_granularity = Significance::Trace;

        generate_full_history_with_tectonics(
            &mut world_a,
            &mut state_a,
            WorldYear(1_000_000),
            |_| {},
        )
        .expect("history a");
        generate_full_history_with_tectonics(
            &mut world_b,
            &mut state_b,
            WorldYear(1_000_000),
            |_| {},
        )
        .expect("history b");

        let log_a: Vec<_> = world_a
            .branch_tree
            .root()
            .event_log
            .iter()
            .map(|e| (e.id, e.year, format!("{:?}", e.kind)))
            .collect();
        let log_b: Vec<_> = world_b
            .branch_tree
            .root()
            .event_log
            .iter()
            .map(|e| (e.id, e.year, format!("{:?}", e.kind)))
            .collect();
        assert_eq!(log_a, log_b);
    }

    #[test]
    fn full_history_has_boundary_hexes_at_one_million_years() {
        let mut world = test_world();
        let mut state = TectonicsState::new();
        generate_full_history_with_tectonics(&mut world, &mut state, WorldYear(1_000_000), |_| {})
            .expect("history");
        let boundary_count = state.boundaries.boundary_hexes.len();
        let total = world.data.plate_id.len();
        assert!(
            boundary_count > 0,
            "default seed should produce plate boundaries at 1M years"
        );
        assert!(
            boundary_count < total,
            "boundary hexes should not cover entire grid (got {boundary_count}/{total})"
        );
    }

    // --- P1-9 validation suite (seed 42, Doc 06 §11) ---

    #[test]
    fn validation_quick_suite_passes() {
        let (world, state) = run_validation_world(WorldYear(VALIDATION_TARGET_YEAR_QUICK))
            .expect("validation quick run");

        let land_frac = continental_fraction(&world.data);
        assert!(
            (CONTINENTAL_FRACTION_MIN..=CONTINENTAL_FRACTION_MAX).contains(&land_frac),
            "§11 #1 continental fraction: expected [{CONTINENTAL_FRACTION_MIN},{CONTINENTAL_FRACTION_MAX}], got {land_frac}"
        );

        let plate_count = state.registry.count();
        assert!(
            (5..=15).contains(&plate_count),
            "§11 #2 plate count: expected [5,15], got {plate_count}"
        );

        let (min_e, max_e) = elevation_bounds(&world.data);
        assert!(
            min_e >= ELEVATION_MIN_BOUND_M && max_e <= ELEVATION_MAX_BOUND_M,
            "§11 #6 elevation bounds: min={min_e} max={max_e} (clamp allows inclusive endpoints)"
        );

        assert!(
            world.data.sea_level_m.abs() <= SEA_LEVEL_MAX_ABS_M,
            "§11 #7 sea level: got {} m",
            world.data.sea_level_m
        );

        let notable_events = event_count_at_granularity(&world, Significance::Notable);
        assert!(
            notable_events > 0,
            "§11 #8 quick proxy: expected >0 Notable events at 1M years, got {notable_events}"
        );
    }

    #[test]
    #[ignore = "long history: §11 criteria 3–5 and event volume (run with cargo test -p genesis_tectonics -- --ignored)"]
    fn validation_full_suite_passes() {
        let (world, state) = run_validation_world(WorldYear(VALIDATION_TARGET_YEAR_FULL))
            .expect("validation full run");

        let mountains =
            mountain_regions_above_elevation(&world.data, MOUNTAIN_ELEVATION_THRESHOLD_M);
        assert!(
            mountains.len() >= 3,
            "§11 #3 mountain regions: expected >=3 above {MOUNTAIN_ELEVATION_THRESHOLD_M}m, got {} regions (sizes {:?})",
            mountains.len(),
            mountains
        );

        let ocean_min = min_ocean_basin_hex_threshold(world.data.cell_count());
        let deep_oceans: Vec<_> =
            ocean_basins_below_elevation(&world.data, OCEAN_BASIN_ELEVATION_THRESHOLD_M)
                .into_iter()
                .filter(|&s| s >= ocean_min)
                .collect();
        assert!(
            !deep_oceans.is_empty(),
            "§11 #4 ocean basin: expected at least one region >= {ocean_min} hexes below {OCEAN_BASIN_ELEVATION_THRESHOLD_M}m, got sizes {:?}",
            ocean_basins_below_elevation(&world.data, OCEAN_BASIN_ELEVATION_THRESHOLD_M)
        );

        let bedrock = bedrock_types_present(&world.data);
        check_phase1_bedrock_diversity(&bedrock)
            .unwrap_or_else(|e| panic!("§11 #5 bedrock: {e} (types: {bedrock:?})"));

        let notable_events = event_count_at_granularity(&world, Significance::Notable);
        assert!(
            notable_events >= EVENT_COUNT_NOTABLE_MIN,
            "§11 #8 event count at Notable: expected >= {EVENT_COUNT_NOTABLE_MIN}, got {notable_events}"
        );
        assert!(
            notable_events <= EVENT_COUNT_NOTABLE_MAX_AT_FULL_YEAR,
            "§11 #8 event count at Notable: expected <= {EVENT_COUNT_NOTABLE_MAX_AT_FULL_YEAR} at \
             {VALIDATION_TARGET_YEAR_FULL} years (doc nominal {EVENT_COUNT_NOTABLE_MAX_DOC} at 4.5B), got {notable_events}"
        );

        let (saturated_max, saturated_min) = count_saturated_hexes(&world.data);
        eprintln!(
            "validation full: {} (saturated_max={saturated_max} saturated_min={saturated_min})",
            summarize_world(&world, &state)
        );
    }

    #[test]
    #[ignore = "long history: §11 saturation guard at 100M years (run with cargo test -p genesis_tectonics -- --ignored)"]
    fn long_validation_does_not_saturate_elevation() {
        let (world, state) = run_validation_world(WorldYear(VALIDATION_TARGET_YEAR_FULL))
            .expect("validation world runs");
        let (saturated_max, saturated_min) = count_saturated_hexes(&world.data);
        let total = world.data.cell_count() as usize;
        let max_allowed = total / 200;

        assert!(
            saturated_max <= max_allowed,
            "{saturated_max}/{total} hexes saturated to MAX_ELEVATION_M; orogeny rate may be too aggressive"
        );
        assert!(
            saturated_min <= max_allowed,
            "{saturated_min}/{total} hexes saturated to MIN_ELEVATION_M; subduction rate may be too aggressive"
        );

        let motions = plate_motion_summary(&world, &state);
        let any_rotation = state
            .registry
            .iter()
            .any(|p| p.accumulated_rotation_rad > 0.0);
        eprintln!(
            "saturation at 100M years: saturated_max={saturated_max} saturated_min={saturated_min} (limit {max_allowed})"
        );
        eprintln!(
            "elevation_distribution: {}",
            format_elevation_distribution(&world.data)
        );
        eprintln!(
            "plate_motion: min={:.0}km median={:.0}km max={:.0}km any_rotation={any_rotation}",
            motions.first().copied().unwrap_or(0.0),
            motions.get(motions.len() / 2).copied().unwrap_or(0.0),
            motions.last().copied().unwrap_or(0.0),
        );
    }

    #[test]
    fn validation_summary_logged_quick() {
        let (world, state) =
            run_validation_world(WorldYear(VALIDATION_TARGET_YEAR_QUICK)).expect("quick run");
        let (saturated_max, saturated_min) = count_saturated_hexes(&world.data);
        eprintln!(
            "validation quick: {} (saturated_max={saturated_max} saturated_min={saturated_min})",
            summarize_world(&world, &state)
        );
    }

    #[test]
    fn world_data_identical_after_validation_run() {
        let (world_a, _) =
            run_validation_world(WorldYear(VALIDATION_TARGET_YEAR_QUICK)).expect("run a");
        let (world_b, _) =
            run_validation_world(WorldYear(VALIDATION_TARGET_YEAR_QUICK)).expect("run b");

        assert_eq!(world_a.data.elevation_mean, world_b.data.elevation_mean);
        assert_eq!(world_a.data.elevation_relief, world_b.data.elevation_relief);
        assert_eq!(world_a.data.bedrock_type, world_b.data.bedrock_type);
        assert_eq!(world_a.data.plate_id, world_b.data.plate_id);
        assert_eq!(world_a.data.fertility, world_b.data.fertility);
        assert_eq!(world_a.data.sea_level_m, world_b.data.sea_level_m);
    }

    #[test]
    fn continental_cratons_persist_at_100m_years() {
        let (world, _) =
            run_validation_world(WorldYear(VALIDATION_TARGET_YEAR_FULL)).expect("validation run");
        let land_fraction = continental_fraction(&world.data);
        assert!(
            land_fraction >= CONTINENTAL_PERSISTENCE_MIN_FRAC,
            "land fraction {land_fraction} at {VALIDATION_TARGET_YEAR_FULL} years too low"
        );
    }

    #[test]
    fn plate_features_advect_between_10m_and_100m() {
        let (world_short, _) = run_validation_world(WorldYear(10_000_000)).expect("validation run");
        let (world_long, _) =
            run_validation_world(WorldYear(VALIDATION_TARGET_YEAR_ADVECTION_DRIFT))
                .expect("validation run");

        let peak_short = peak_elevation_hex(&world_short.data);
        let peak_long = peak_elevation_hex(&world_long.data);

        assert_ne!(
            peak_short, peak_long,
            "peak elevation unchanged between 10M and 100M years — advection broken?"
        );
    }

    #[test]
    #[ignore = "diagnostic: compares direct forward mapping at 1B and 4.5B"]
    fn forward_rotation_does_not_compound_error() {
        fn projected_feature_matches_world(
            world: &genesis_core::World,
            state: &TectonicsState,
        ) -> (PlateId, genesis_core::HexId, genesis_core::HexId) {
            for (plate_id, plate) in state.registry.iter_sorted() {
                let mut candidates: Vec<(genesis_core::HexId, &SurfaceFeature)> = plate
                    .surface
                    .features
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, slot)| {
                        slot.as_ref().map(|f| (genesis_core::HexId(idx as u32), f))
                    })
                    .collect();
                candidates.sort_by(|a, b| {
                    b.1.elevation_m
                        .partial_cmp(&a.1.elevation_m)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });

                for (birth_hex, feature) in candidates {
                    let expected_world =
                        birth_hex_to_current_world(&world.data.grid, birth_hex, plate);
                    let w = expected_world.0 as usize;
                    if world.data.plate_id[w] != plate_id {
                        continue;
                    }
                    if (world.data.elevation_mean[w] - feature.elevation_m).abs() < 1e-3 {
                        return (plate_id, birth_hex, expected_world);
                    }
                }
            }
            panic!("no projected feature matched world reconstruction");
        }

        let (world_1b, state_1b) =
            run_validation_world(WorldYear(VALIDATION_TARGET_YEAR_ONE_BILLION))
                .expect("1B validation run");
        let (world_45b, state_45b) = run_validation_world(WorldYear(VALIDATION_TARGET_YEAR_FULL))
            .expect("4.5B validation run");

        let (_plate_1b, birth_1b, expected_1b) =
            projected_feature_matches_world(&world_1b, &state_1b);
        let (_plate_45b, birth_45b, expected_45b) =
            projected_feature_matches_world(&world_45b, &state_45b);

        let recovered_1b = {
            let plate = state_1b
                .registry
                .get(world_1b.data.plate_id[expected_1b.0 as usize])
                .unwrap();
            current_world_to_birth_hex(&world_1b.data.grid, expected_1b, plate)
        };
        let recovered_45b = {
            let plate = state_45b
                .registry
                .get(world_45b.data.plate_id[expected_45b.0 as usize])
                .unwrap();
            current_world_to_birth_hex(&world_45b.data.grid, expected_45b, plate)
        };

        let near_1b = recovered_1b == birth_1b
            || world_1b
                .data
                .grid
                .neighbors(birth_1b)
                .iter()
                .copied()
                .any(|n| n == recovered_1b);
        let near_45b = recovered_45b == birth_45b
            || world_45b
                .data
                .grid
                .neighbors(birth_45b)
                .iter()
                .copied()
                .any(|n| n == recovered_45b);

        assert!(
            near_1b,
            "1B projection should recover birth hex or neighbor"
        );
        assert!(
            near_45b,
            "4.5B projection should recover birth hex or neighbor"
        );
    }

    #[test]
    #[ignore = "deep history: continental persistence at 500M years (cargo test -p genesis_tectonics -- --ignored)"]
    fn continents_persist_past_deep_history() {
        use crate::validation::VALIDATION_TARGET_YEAR_DEEP_PERSISTENCE;

        let (world, _state) =
            run_validation_world(WorldYear(VALIDATION_TARGET_YEAR_DEEP_PERSISTENCE))
                .expect("validation world runs");
        let land_fraction = continental_fraction(&world.data);
        assert!(
            land_fraction >= CONTINENTAL_PERSISTENCE_MIN_FRAC,
            "land fraction {land_fraction} at {VALIDATION_TARGET_YEAR_DEEP_PERSISTENCE} years too low"
        );
    }

    #[test]
    #[ignore = "deep time: Earthlike metrics at 1B years (cargo test -p genesis_tectonics -- --ignored)"]
    fn deep_time_metrics_stay_earthlike() {
        use crate::validation::{
            OCEAN_BASIN_ELEVATION_THRESHOLD_M, min_ocean_basin_hex_threshold,
            ocean_basins_below_elevation,
        };

        // Every deep-time regression found during P1-17/P1-18 development —
        // sea level integrating unbounded, continents grinding to sea level,
        // margin-minting ballooning land past 50% — was invisible at the 100M
        // years the §11 suite covers and only surfaced by 1B. This gate runs
        // the validation world 10x deeper and pins the Earthlike envelope.
        let (world, _state) =
            run_validation_world(WorldYear(1_000_000_000)).expect("validation world runs");

        let land_fraction = continental_fraction(&world.data);
        assert!(
            (0.15..=0.45).contains(&land_fraction),
            "land fraction {land_fraction} at 1B years outside Earthlike envelope [0.15, 0.45]"
        );

        let sea = world.data.sea_level_m;
        assert!(
            sea.abs() < 200.0,
            "sea level {sea} m at 1B years should stay bounded (|sea| < 200 m)"
        );

        let threshold = min_ocean_basin_hex_threshold(world.data.cell_count());
        let basins = ocean_basins_below_elevation(&world.data, OCEAN_BASIN_ELEVATION_THRESHOLD_M);
        let largest = basins.iter().copied().max().unwrap_or(0);
        assert!(
            largest >= threshold,
            "largest deep ocean basin at 1B years is {largest} hexes; expected >= {threshold} \
             (a connected world ocean)"
        );

        let (saturated_max, saturated_min) = crate::validation::count_saturated_hexes(&world.data);
        let max_allowed = world.data.cell_count() as usize / 200;
        assert!(
            saturated_max <= max_allowed && saturated_min <= max_allowed,
            "elevation clamp saturation at 1B years: {saturated_max} at MAX, {saturated_min} at MIN \
             (allowed {max_allowed})"
        );
    }

    #[test]
    fn event_granularity_pivotal_logs_only_pivotal_events() {
        let mut params = validation_parameters();
        params.core.geology.event_granularity = Significance::Pivotal;

        let mut world = create_world(params.clone()).expect("world");
        let mut state = TectonicsState::new();
        generate_full_history_with_tectonics(
            &mut world,
            &mut state,
            WorldYear(VALIDATION_TARGET_YEAR_QUICK),
            |_| {},
        )
        .expect("history pivotal");

        for event in world.branch_tree.root().event_log.iter() {
            assert_eq!(
                event.significance,
                Significance::Pivotal,
                "§12.5: expected only Pivotal events, got {:?} ({:?})",
                event.significance,
                event.kind
            );
        }

        let mut control = create_world(validation_parameters()).expect("control world");
        let mut control_state = TectonicsState::new();
        run_formation(&mut control, &mut control_state);
        let formation_elev = control.data.elevation_mean.clone();

        generate_full_history_with_tectonics(
            &mut control,
            &mut control_state,
            WorldYear(VALIDATION_TARGET_YEAR_QUICK),
            |_| {},
        )
        .expect("history notable");

        let changed = formation_elev
            .iter()
            .zip(control.data.elevation_mean.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            changed > 0,
            "§12.5: simulation must change terrain even when only Pivotal events are logged"
        );
    }

    #[test]
    fn tectonics_full_history_completes_within_budget() {
        let start = std::time::Instant::now();
        run_validation_world(WorldYear(PERF_TARGET_YEAR)).expect("perf run");
        let elapsed = start.elapsed();
        eprintln!(
            "tectonics perf: {:?} for {} years at subdiv {}",
            elapsed, PERF_TARGET_YEAR, VALIDATION_SUBDIVISION_LEVEL
        );
        assert!(
            elapsed.as_secs_f64() < PERF_BUDGET_SECS,
            "§9.3 perf budget: {:.2}s >= {PERF_BUDGET_SECS}s for {PERF_TARGET_YEAR} years",
            elapsed.as_secs_f64()
        );
    }

    #[test]
    #[ignore = "subdiv 7 smoke (~3min at 1M years): cargo test -p genesis_tectonics -- --ignored --exact tectonics_full_history_subdiv_seven"]
    fn tectonics_full_history_subdiv_seven_within_budget() {
        const SUBDIV7_SMOKE_YEAR: i64 = 1_000_000;
        const SUBDIV7_SMOKE_BUDGET_SECS: f64 = 240.0;

        let mut params = validation_parameters();
        params.core.grid.subdivision_level = 7;
        let start = std::time::Instant::now();
        let mut world = create_world(params).expect("world");
        let mut state = TectonicsState::new();
        generate_full_history_with_tectonics(
            &mut world,
            &mut state,
            WorldYear(SUBDIV7_SMOKE_YEAR),
            |_| {},
        )
        .expect("perf subdiv 7");
        let elapsed = start.elapsed();
        eprintln!(
            "tectonics perf subdiv 7: {:?} for {SUBDIV7_SMOKE_YEAR} years ({} hexes)",
            elapsed,
            world.data.grid.cell_count()
        );
        assert!(
            elapsed.as_secs_f64() < SUBDIV7_SMOKE_BUDGET_SECS,
            "subdiv 7 should complete {SUBDIV7_SMOKE_YEAR} years in under {SUBDIV7_SMOKE_BUDGET_SECS}s, took {:.2}s",
            elapsed.as_secs_f64()
        );
    }
}
