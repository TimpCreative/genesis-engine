//! World initialization and generation lifecycle.

mod error;
mod progress;

pub use error::{CreateWorldError, GenerationError};
pub use progress::{GenerationProgress, ProgressCallback};

use crate::branches::BranchTree;
use crate::data::WorldData;
use crate::grid::HexGrid;
use crate::parameters::WorldParameters;
use crate::rng::WorldRng;
use crate::time::{TickCoordinator, WorldYear};
use crate::world::World;

/// Creates a new world from the given parameters.
///
/// Validates parameters, constructs the hex grid, initializes the data layer
/// with defaults, creates a root branch tree, and seeds the RNG.
///
/// The returned [`World`] is at year `parameters.core.time.world_start_year`
/// (typically year 0). To populate it with simulated history, call
/// [`generate_full_history`].
pub fn create_world(parameters: WorldParameters) -> Result<World, CreateWorldError> {
    parameters.validate()?;

    let grid = HexGrid::new(
        parameters.core.grid.subdivision_level,
        parameters.core.planet.radius_km,
    )?;

    let rng = WorldRng::from_parameters(&parameters);
    let data = crate::data::WorldData::new(grid, parameters);
    let branch_tree = BranchTree::new();

    Ok(World {
        data,
        branch_tree,
        rng,
    })
}

/// Advances the simulation from the current year to `target_year`.
///
/// Invokes `progress` periodically (at least once per tick interval) so the UI
/// can show progress and stream events. In Phase 0, the tick coordinator has
/// no registered simulation layers, so the year clock advances directly to
/// the target without generating any events.
///
/// Returns immediately if `target_year <= current_year`.
pub fn generate_full_history(
    world: &mut World,
    target_year: WorldYear,
    mut progress: impl FnMut(GenerationProgress<'_>),
) -> Result<(), GenerationError> {
    let current = world.data.current_year;
    if target_year < current {
        return Err(GenerationError::TargetInPast {
            target: target_year.value(),
            current: current.value(),
        });
    }
    if target_year == current {
        return Ok(());
    }

    progress(GenerationProgress {
        current_year: current,
        target_year,
        recent_events: &[],
        total_events: 0,
    });

    let mut coordinator = TickCoordinator::new();
    advance_with_coordinator(world, &mut coordinator, target_year)?;

    progress(GenerationProgress {
        current_year: world.data.current_year,
        target_year,
        recent_events: &[],
        total_events: 0,
    });

    Ok(())
}

/// Advances `world` to `target_year` using a pre-configured [`TickCoordinator`].
///
/// Keeps `genesis_core` independent of domain crates while allowing `genesis_tectonics`
/// (and tests) to register simulation layers.
pub fn advance_with_coordinator(
    world: &mut World,
    coordinator: &mut TickCoordinator,
    target_year: WorldYear,
) -> Result<(), GenerationError> {
    advance_with_coordinator_observed(world, coordinator, target_year, |_| {})
}

/// Like [`advance_with_coordinator`], invoking `on_tick` with the world state
/// after each processed tick (for progress reporting and history buffering).
/// Observational only; simulation output is identical.
pub fn advance_with_coordinator_observed(
    world: &mut World,
    coordinator: &mut TickCoordinator,
    target_year: WorldYear,
    mut on_tick: impl FnMut(&WorldData),
) -> Result<(), GenerationError> {
    let current = world.data.current_year;
    if target_year < current {
        return Err(GenerationError::TargetInPast {
            target: target_year.value(),
            current: current.value(),
        });
    }
    if target_year == current {
        return Ok(());
    }

    let parameters = world.data.parameters.clone();
    coordinator.advance_to_with(
        target_year,
        &mut world.data,
        &world.rng,
        &parameters,
        &mut on_tick,
    );

    if world.data.current_year < target_year {
        world.data.current_year = target_year;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameters::WorldParameters;

    #[test]
    fn create_world_default_succeeds() {
        let _ = create_world(WorldParameters::default()).unwrap();
    }

    #[test]
    fn create_world_grid_subdivision() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 6;
        let world = create_world(params).unwrap();
        assert_eq!(world.data.grid.subdivision_level(), 6);
    }

    #[test]
    fn create_world_single_root_branch() {
        let world = create_world(WorldParameters::default()).unwrap();
        assert_eq!(world.branch_tree.count(), 1);
    }

    #[test]
    fn create_world_rng_deterministic() {
        let params = WorldParameters::default();
        let a = create_world(params.clone()).unwrap();
        let b = create_world(params).unwrap();
        assert_eq!(a.rng.effective_seed(), b.rng.effective_seed());
    }

    #[test]
    fn create_world_invalid_parameters() {
        let mut params = WorldParameters::default();
        params.core.planet.radius_km = -1.0;
        assert!(matches!(
            create_world(params),
            Err(CreateWorldError::InvalidParameters(_))
        ));
    }

    #[test]
    fn create_world_current_year() {
        let mut params = WorldParameters::default();
        params.core.time.world_start_year = WorldYear(500);
        let world = create_world(params).unwrap();
        assert_eq!(world.data.current_year, WorldYear(500));
    }

    #[test]
    fn generate_advances_to_target() {
        let mut world = create_world(WorldParameters::default()).unwrap();
        generate_full_history(&mut world, WorldYear(1000), |_| {}).unwrap();
        assert_eq!(world.data.current_year, WorldYear(1000));
    }

    #[test]
    fn generate_noop_when_already_at_target() {
        let mut world = create_world(WorldParameters::default()).unwrap();
        let year = world.data.current_year;
        generate_full_history(&mut world, year, |_| {}).unwrap();
        assert_eq!(world.data.current_year, year);
    }

    #[test]
    fn generate_target_in_past() {
        let mut world = create_world(WorldParameters::default()).unwrap();
        generate_full_history(&mut world, WorldYear(1000), |_| {}).unwrap();
        let err = generate_full_history(&mut world, WorldYear(500), |_| {}).unwrap_err();
        assert!(matches!(
            err,
            GenerationError::TargetInPast {
                target: 500,
                current: 1000,
            }
        ));
    }

    #[test]
    fn generate_progress_callback_invoked() {
        let mut world = create_world(WorldParameters::default()).unwrap();
        let mut call_count = 0;
        let mut first_fraction = None;
        let mut last_fraction = None;
        generate_full_history(&mut world, WorldYear(1000), |p| {
            call_count += 1;
            let f = p.fraction();
            if first_fraction.is_none() {
                first_fraction = Some(f);
            }
            last_fraction = Some(f);
        })
        .unwrap();
        assert!(call_count >= 2);
        assert!(first_fraction.unwrap() < last_fraction.unwrap());
    }

    #[test]
    fn generate_no_events_phase0() {
        let mut world = create_world(WorldParameters::default()).unwrap();
        let mut final_total = None;
        generate_full_history(&mut world, WorldYear(1000), |p| {
            final_total = Some(p.total_events);
        })
        .unwrap();
        assert_eq!(final_total, Some(0));
    }
}
