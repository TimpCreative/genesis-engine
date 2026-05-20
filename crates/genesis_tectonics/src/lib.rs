//! Tectonic simulation for Genesis Engine.
//!
//! Phase 1: plate generation, drift, boundaries, and terrain sculpting.

pub mod boundary;
pub mod elevation;
pub mod events;
pub mod history;
pub mod hotspots;
pub mod initial_generation;
pub mod initial_terrain;
pub mod layer;
pub mod motion;
pub mod partition;
pub mod plate;
pub mod volcanism;

pub use boundary::{
    BoundaryClass, BoundaryInfo, ClassifiedEdge, ConvergentSubtype, convergent_subtype,
    detect_and_classify_boundaries,
};
pub use elevation::{
    CC_INLAND_HEXES, CONTINENTAL_RIFT_SUBSIDENCE_FACTOR, MAX_ELEVATION_M, MAX_RELIEF_M,
    MIN_ELEVATION_M, OC_INLAND_HEXES, OROGENY_RATE, SUBDUCTION_RATE, SUBSIDENCE_RATE,
    apply_boundary_elevation, clamp_terrain, subducting_plate_id,
};
pub use events::flush_events_to_branch;
pub use history::{generate_full_history_with_tectonics, run_formation};
pub use hotspots::{
    ACTIVITY_RATE_MAX, ACTIVITY_RATE_MIN, HOTSPOT_ACTIVITY_STREAM, HOTSPOT_ELEVATION_CHANGE_MAX_M,
    HOTSPOT_ELEVATION_CHANGE_MIN_M, HOTSPOT_LOCATIONS_STREAM, LIFESPAN_MAX_YEARS,
    LIFESPAN_MIN_YEARS, NOTABLE_CUMULATIVE_UPLIFT_M, SPAWN_PROBABILITY_PER_TICK,
    apply_hotspot_tick, generate_initial_hotspots, hex_at_anchor,
};
pub use initial_generation::{generate_initial_plates, generate_initial_plates_data};
pub use initial_terrain::{
    CONTINENTAL_BASE_ELEVATION_M, INITIAL_ELEVATION_NOISE_RANGE_M, INITIAL_ELEVATION_NOISE_STREAM,
    OCEANIC_BASE_ELEVATION_M, apply_formation_terrain,
};
pub use layer::{DEFAULT_GEOLOGICAL_TICK_YEARS, TectonicsLayer, geological_tick_interval};
pub use motion::{advance_plate_motion, effective_position_direction, surface_velocity_m_per_year};
pub use partition::repartition_hexes;
pub use plate::{
    HotSpot, HotSpotRegistry, Plate, PlateClass, PlateRegistry, PlateType, TectonicsState,
};
pub use volcanism::{
    ELEVATION_CHANGE_MAX_M, ELEVATION_CHANGE_MIN_M, ERUPTION_PROBABILITY_BASE,
    NOTABLE_PEAK_THRESHOLD_M, RELIEF_CHANGE_MAX_M, RELIEF_CHANGE_MIN_M, VOLCANISM_STREAM,
    apply_boundary_volcanism, is_arc_hex,
};

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
    fn one_geological_tick_changes_some_plate_ids() {
        let mut world = test_world();
        let mut state = TectonicsState::new();
        run_formation(&mut world, &mut state);
        let after_formation = world.data.plate_id.clone();

        generate_full_history_with_tectonics(&mut world, &mut state, WorldYear(500_000), |_| {})
            .expect("one tick");

        let changed = after_formation
            .iter()
            .zip(world.data.plate_id.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            changed > 0,
            "expected repartition after 500k years to change some hex assignments"
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
}
