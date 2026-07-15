//! Full-world history generation with tectonics and climate layers.

use genesis_climate::{ClimateLayer, ClimateState, flush_events_to_branch as flush_climate_events};
use genesis_core::World;
use genesis_core::lifecycle::{
    GenerationError, GenerationProgress, advance_with_coordinator_observed,
};
use genesis_core::time::{TickCoordinator, WorldYear};
use genesis_hydrology::HydrologyLayer;
use genesis_tectonics::{
    TectonicsLayer, TectonicsState, flush_events_to_branch as flush_tectonic_events,
};

/// Advances simulation to `target_year` with tectonics, climate, and hydrology
/// registered on the coordinator.
///
/// Tectonics registers first; climate second (Doc 07 §13) so climate sees
/// updated terrain each tick; hydrology third so flow reflects both.
pub fn generate_full_history(
    world: &mut World,
    tectonics: &mut TectonicsState,
    climate: &mut ClimateState,
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

    let (tectonics_layer, tectonics_shared) = TectonicsLayer::attach(tectonics);
    let (climate_layer, climate_shared) = ClimateLayer::attach(climate);
    let mut coordinator = TickCoordinator::new();
    coordinator.add_layer(Box::new(tectonics_layer));
    coordinator.add_layer(Box::new(climate_layer));
    coordinator.add_layer(Box::new(HydrologyLayer));

    advance_with_coordinator_observed(world, &mut coordinator, target_year, |year| {
        progress(GenerationProgress {
            current_year: year,
            target_year,
            recent_events: &[],
            total_events: 0,
        });
    })?;
    drop(coordinator);

    *tectonics = TectonicsLayer::detach_state(tectonics_shared);
    *climate = ClimateLayer::detach_state(climate_shared);
    flush_tectonic_events(world, tectonics);
    flush_climate_events(world, climate);

    progress(GenerationProgress {
        current_year: world.data.current_year,
        target_year,
        recent_events: &[],
        total_events: 0,
    });

    Ok(())
}
