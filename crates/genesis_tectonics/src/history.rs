//! History generation with tectonics registered on the tick coordinator.

use genesis_core::World;
use genesis_core::lifecycle::{GenerationError, GenerationProgress, advance_with_coordinator};
use genesis_core::time::{TickCoordinator, WorldYear};

use crate::erosion::ensure_deposition_buffer;
use crate::events::flush_events_to_branch;
use crate::hotspots::generate_initial_hotspots;
use crate::initial_terrain::apply_formation_terrain;
use crate::layer::TectonicsLayer;
use crate::plate::TectonicsState;

/// Advances simulation to `target_year` with formation and geological tectonics ticks.
pub fn generate_full_history_with_tectonics(
    world: &mut World,
    state: &mut TectonicsState,
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

    let (layer, shared) = TectonicsLayer::attach(state);
    let mut coordinator = TickCoordinator::new();
    coordinator.add_layer(Box::new(layer));

    advance_with_coordinator(world, &mut coordinator, target_year)?;
    drop(coordinator);

    *state = TectonicsLayer::detach_state(shared);
    flush_events_to_branch(world, state);

    progress(GenerationProgress {
        current_year: world.data.current_year,
        target_year,
        recent_events: &[],
        total_events: 0,
    });

    Ok(())
}

/// Runs formation only (year 0 plate seeding) without geological ticks.
pub fn run_formation(world: &mut World, state: &mut TectonicsState) {
    if state.formation_complete {
        return;
    }
    state.registry = crate::generate_initial_plates_data(&mut world.data, &world.rng);
    apply_formation_terrain(&mut world.data, &state.registry, &world.rng);
    state.hotspots = generate_initial_hotspots(&world.data, &world.rng);
    ensure_deposition_buffer(state, world.data.grid.cell_count() as usize);
    state.formation_complete = true;
}
