//! Flush tectonic events into the root branch event log.

use crate::plate::TectonicsState;
use genesis_core::World;
use genesis_core::branches::BranchId;

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
    use genesis_core::events::{Event, EventId, EventKind, EventLocation, Significance};
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
}
