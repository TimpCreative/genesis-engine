//! Event emission and flush to the branch log (mirrors the physical layers).

use genesis_core::World;
use genesis_core::branches::BranchId;
use genesis_core::events::{Event, EventId, EventKind, EventLocation, Significance};
use genesis_core::time::WorldYear;

use crate::state::BiologyState;

/// Allocates the next monotonic [`EventId`].
pub(crate) fn alloc_event_id(state: &mut BiologyState) -> EventId {
    let id = EventId(state.next_event_id);
    state.next_event_id += 1;
    id
}

/// Buffers a biology event for the branch log. Biology's P4-3 events are all
/// pivotal (life, oxygenation, key innovations), so they are emitted directly;
/// granularity gating for frequent speciation events arrives with the ledger
/// (Doc 09 §13).
pub(crate) fn emit(
    state: &mut BiologyState,
    year: WorldYear,
    location: EventLocation,
    significance: Significance,
    kind: EventKind,
) {
    let id = alloc_event_id(state);
    state.pending_events.push(Event {
        id,
        year,
        branch_id: BranchId::ROOT,
        location,
        significance,
        kind,
    });
}

/// Drains buffered biology events onto the root branch's event log.
pub fn flush_events_to_branch(world: &mut World, state: &mut BiologyState) {
    let root = world
        .branch_tree
        .get_mut(BranchId::ROOT)
        .expect("root branch always exists");
    for event in state.pending_events.drain(..) {
        root.event_log.push(event);
    }
}
