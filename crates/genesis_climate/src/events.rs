//! Climate event emission (Doc 07 §15).

use genesis_core::World;
use genesis_core::branches::BranchId;
use genesis_core::data::WorldData;
use genesis_core::events::{Event, EventId, EventKind, EventLocation, Significance};
use genesis_core::time::WorldYear;

use crate::state::{ClimateState, FormationSubPhase};

/// Allocates the next monotonic [`EventId`] from climate state.
pub fn alloc_event_id(state: &mut ClimateState) -> EventId {
    let id = EventId(state.next_event_id);
    state.next_event_id += 1;
    id
}

/// Conditionally emit an event if its significance meets the threshold.
pub fn maybe_emit(state: &mut ClimateState, event: Event, threshold: Significance) {
    if event.significance >= threshold {
        state.pending_events.push(event);
    }
}

/// Emit an event when crossing a Formation sub-phase boundary.
pub fn emit_phase_transition_event(
    state: &mut ClimateState,
    world: &WorldData,
    from: FormationSubPhase,
    to: FormationSubPhase,
    year: WorldYear,
    granularity: Significance,
) {
    let event_id = alloc_event_id(state);
    let kind = match (from, to) {
        (_, FormationSubPhase::Cooling) => Some(EventKind::PlanetaryCoolingMilestone {
            surface_temp_c: world.global_temperature_c,
        }),
        (_, FormationSubPhase::Condensation) => Some(EventKind::OceansBeginForming {
            sea_level_m: world.sea_level_m,
        }),
        (_, FormationSubPhase::Stabilization) => Some(EventKind::OceansStabilized {
            sea_level_m: world.sea_level_m,
        }),
        (_, FormationSubPhase::Complete) => Some(EventKind::FormationComplete {
            final_temperature_c: world.global_temperature_c,
            final_co2_ppm: state.atmospheric_composition.co2_ppm,
        }),
        _ => None,
    };

    if let Some(kind) = kind {
        if let EventKind::PlanetaryCoolingMilestone { surface_temp_c } = kind {
            // Keep milestone tracker in sync so maybe_emit_cooling_milestone does not
            // re-emit on the same tick (e.g. Molten → Cooling at year 50M).
            state.last_cooling_milestone_temp_c = (surface_temp_c / 100.0).floor() * 100.0;
            let significance = Significance::Notable;
            maybe_emit(
                state,
                Event {
                    id: event_id,
                    year,
                    branch_id: BranchId::ROOT,
                    location: EventLocation::Global,
                    significance,
                    kind: EventKind::PlanetaryCoolingMilestone { surface_temp_c },
                },
                granularity,
            );
            return;
        }

        let significance = match &kind {
            EventKind::OceansBeginForming { .. } => Significance::Major,
            EventKind::OceansStabilized { .. } => Significance::Major,
            EventKind::FormationComplete { .. } => Significance::Pivotal,
            _ => Significance::Notable,
        };
        maybe_emit(
            state,
            Event {
                id: event_id,
                year,
                branch_id: BranchId::ROOT,
                location: EventLocation::Global,
                significance,
                kind,
            },
            granularity,
        );
    }
}

/// Emit a cooling milestone if the temperature has crossed a 100°C threshold downward.
pub fn maybe_emit_cooling_milestone(
    state: &mut ClimateState,
    current_temp_c: f32,
    year: WorldYear,
    granularity: Significance,
) {
    let last = state.last_cooling_milestone_temp_c;
    if last == f32::INFINITY {
        state.last_cooling_milestone_temp_c = (current_temp_c / 100.0).ceil() * 100.0;
        return;
    }

    let current_step = (current_temp_c / 100.0).floor() * 100.0;
    if current_step < last - 50.0 {
        let event_id = alloc_event_id(state);
        maybe_emit(
            state,
            Event {
                id: event_id,
                year,
                branch_id: BranchId::ROOT,
                location: EventLocation::Global,
                significance: Significance::Notable,
                kind: EventKind::PlanetaryCoolingMilestone {
                    surface_temp_c: current_temp_c,
                },
            },
            granularity,
        );
        state.last_cooling_milestone_temp_c = current_step;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{WorldYear, create_world};

    #[test]
    fn phase_transition_cooling_milestone_prevents_same_tick_duplicate() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        let mut state = ClimateState::new();
        state.last_cooling_milestone_temp_c = 1100.0;

        let temp_c = 1077.0;
        world.data.global_temperature_c = temp_c;

        emit_phase_transition_event(
            &mut state,
            &world.data,
            FormationSubPhase::Molten,
            FormationSubPhase::Cooling,
            WorldYear(50_000_000),
            Significance::Notable,
        );
        assert_eq!(state.pending_events.len(), 1);
        assert!(matches!(
            state.pending_events[0].kind,
            EventKind::PlanetaryCoolingMilestone { .. }
        ));
        assert_eq!(state.last_cooling_milestone_temp_c, 1000.0);

        maybe_emit_cooling_milestone(
            &mut state,
            temp_c,
            WorldYear(50_000_000),
            Significance::Notable,
        );
        assert_eq!(
            state.pending_events.len(),
            1,
            "cooling milestone should not double-fire on the same tick"
        );
    }
}

/// Pushes [`ClimateState::pending_events`] onto the root branch event log.
pub fn flush_events_to_branch(world: &mut World, state: &mut ClimateState) {
    let root = world
        .branch_tree
        .get_mut(BranchId::ROOT)
        .expect("root branch always exists");
    for event in state.pending_events.drain(..) {
        root.event_log.push(event);
    }
}
