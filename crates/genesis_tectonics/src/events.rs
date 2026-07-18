//! Event emission and flush to branch event log.

use crate::plate::TectonicsState;
use genesis_core::World;
use genesis_core::branches::BranchId;
use genesis_core::events::{Event, EventId, Significance};

/// Allocates the next monotonic [`EventId`] from tectonics state.
pub fn alloc_event_id(state: &mut TectonicsState) -> EventId {
    let id = EventId(state.next_event_id);
    state.next_event_id += 1;
    id
}

/// Records `event` when its significance meets the user granularity threshold.
///
/// Terrain and world-state updates happen regardless; only chronicle logging is gated.
pub fn maybe_emit(state: &mut TectonicsState, event: Event, granularity: Significance) {
    if event.significance >= granularity {
        state.pending_events.push(event);
    }
}

/// Pushes [`TectonicsState::pending_events`] onto the root branch [`EventLog`](genesis_core::events::EventLog).
pub fn flush_events_to_branch(world: &mut World, state: &mut TectonicsState) {
    let root = world
        .branch_tree
        .get_mut(BranchId::ROOT)
        .expect("root branch always exists");
    for event in state.pending_events.drain(..) {
        root.event_log.push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::events::{EventKind, EventLocation};
    use genesis_core::parameters::WorldParameters;
    use genesis_core::time::WorldYear;
    use genesis_core::{HexId, PlateId, create_world};

    #[test]
    fn flush_pushes_events_to_root_log() {
        let mut world = create_world(WorldParameters::default()).expect("world");
        let mut state = TectonicsState::new();
        state.pending_events.push(Event {
            id: EventId(1),
            year: WorldYear(500_000),
            branch_id: BranchId::ROOT,
            location: EventLocation::Hex(HexId(10)),
            significance: Significance::Minor,
            kind: EventKind::VolcanicEruption {
                hex: HexId(10),
                elevation_change_m: 200.0,
                plate: PlateId(0),
            },
        });

        flush_events_to_branch(&mut world, &mut state);
        assert!(state.pending_events.is_empty());
        assert_eq!(world.branch_tree.root().event_log.len(), 1);
        let event = world.branch_tree.root().event_log.iter().next().unwrap();
        assert!(matches!(event.kind, EventKind::VolcanicEruption { .. }));
    }

    #[test]
    fn maybe_emit_respects_granularity() {
        let mut state = TectonicsState::new();
        let event = Event {
            id: alloc_event_id(&mut state),
            year: WorldYear(500_000),
            branch_id: BranchId::ROOT,
            location: EventLocation::Global,
            significance: Significance::Trace,
            kind: EventKind::BoundaryTransition {
                hex: HexId(10),
                from: genesis_core::events::BoundaryType::Divergent,
                to: genesis_core::events::BoundaryType::Transform,
            },
        };
        maybe_emit(&mut state, event, Significance::Pivotal);
        assert!(state.pending_events.is_empty());

        let event2 = Event {
            id: alloc_event_id(&mut state),
            year: WorldYear(500_000),
            branch_id: BranchId::ROOT,
            location: EventLocation::Global,
            significance: Significance::Pivotal,
            kind: EventKind::WorldFormation,
        };
        maybe_emit(&mut state, event2, Significance::Notable);
        assert_eq!(state.pending_events.len(), 1);
    }
}
