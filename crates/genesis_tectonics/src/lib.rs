//! Tectonic simulation for Genesis Engine.
//!
//! Phase 1: initial plate generation, motion, and Voronoi re-partition per Geological-era ticks.

pub mod history;
pub mod initial_generation;
pub mod layer;
pub mod motion;
pub mod partition;
pub mod plate;

pub use history::{generate_full_history_with_tectonics, run_formation};
pub use initial_generation::{generate_initial_plates, generate_initial_plates_data};
pub use layer::{DEFAULT_GEOLOGICAL_TICK_YEARS, TectonicsLayer, geological_tick_interval};
pub use motion::{advance_plate_motion, effective_position_direction};
pub use partition::repartition_hexes;
pub use plate::{Plate, PlateClass, PlateRegistry, PlateType, TectonicsState};

#[cfg(test)]
mod integration_tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::time::WorldYear;
    use genesis_core::{PlateId, create_world};

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
}
