//! Hydrology event emission (Doc 08 §13).
//!
//! The formation ocean events moved here from climate (§17.2): hydrology owns
//! condensation and the flooding solve, so it owns the moment standing water
//! first exists and the moment the inventory is fully condensed.

use genesis_core::World;
use genesis_core::branches::BranchId;
use genesis_core::events::{Event, EventId, EventKind, EventLocation, Significance};
use genesis_core::time::WorldYear;

use crate::state::HydrologyState;

/// Allocates the next monotonic [`EventId`] from hydrology state.
pub fn alloc_event_id(state: &mut HydrologyState) -> EventId {
    let id = EventId(state.next_event_id);
    state.next_event_id += 1;
    id
}

/// Conditionally emit an event if its significance meets the threshold.
pub fn maybe_emit(state: &mut HydrologyState, event: Event, threshold: Significance) {
    if event.significance >= threshold {
        state.pending_events.push(event);
    }
}

/// Emits the formation ocean events (Doc 08 §13, payload-compatible with the
/// retired Doc 07 §15 emissions):
///
/// - [`EventKind::OceansBeginForming`] — Major, the first tick the flooding
///   solve produces standing water (condensed fraction crossing into seas).
/// - [`EventKind::OceansStabilized`] — Major, the first tick the inventory is
///   fully condensed (§3.3).
pub fn maybe_emit_formation_ocean_events(
    state: &mut HydrologyState,
    condensed_fraction: f64,
    wet_cell_count: u32,
    sea_level_m: f32,
    year: WorldYear,
    granularity: Significance,
) {
    if !state.oceans_begin_emitted && wet_cell_count > 0 {
        state.oceans_begin_emitted = true;
        let event_id = alloc_event_id(state);
        maybe_emit(
            state,
            Event {
                id: event_id,
                year,
                branch_id: BranchId::ROOT,
                location: EventLocation::Global,
                significance: Significance::Major,
                kind: EventKind::OceansBeginForming { sea_level_m },
            },
            granularity,
        );
    }

    if !state.oceans_stabilized_emitted && condensed_fraction >= 1.0 {
        state.oceans_stabilized_emitted = true;
        let event_id = alloc_event_id(state);
        maybe_emit(
            state,
            Event {
                id: event_id,
                year,
                branch_id: BranchId::ROOT,
                location: EventLocation::Global,
                significance: Significance::Major,
                kind: EventKind::OceansStabilized { sea_level_m },
            },
            granularity,
        );
    }
}

/// Pushes [`HydrologyState::pending_events`] onto the root branch event log.
pub fn flush_events_to_branch(world: &mut World, state: &mut HydrologyState) {
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

    #[test]
    fn formation_ocean_events_fire_once_in_order() {
        let mut state = HydrologyState::new();
        // No standing water yet: nothing fires.
        maybe_emit_formation_ocean_events(
            &mut state,
            0.35,
            0,
            -1500.0,
            WorldYear(100_000_000),
            Significance::Trace,
        );
        assert!(state.pending_events.is_empty());

        // First standing water: OceansBeginForming fires once.
        maybe_emit_formation_ocean_events(
            &mut state,
            0.90,
            128,
            -400.0,
            WorldYear(250_000_000),
            Significance::Trace,
        );
        maybe_emit_formation_ocean_events(
            &mut state,
            0.90,
            256,
            -350.0,
            WorldYear(255_000_000),
            Significance::Trace,
        );
        assert_eq!(state.pending_events.len(), 1);
        assert!(matches!(
            state.pending_events[0].kind,
            EventKind::OceansBeginForming {
                sea_level_m: -400.0
            }
        ));

        // Full condensation: OceansStabilized fires once.
        maybe_emit_formation_ocean_events(
            &mut state,
            1.0,
            512,
            0.0,
            WorldYear(350_000_000),
            Significance::Trace,
        );
        maybe_emit_formation_ocean_events(
            &mut state,
            1.0,
            512,
            0.0,
            WorldYear(355_000_000),
            Significance::Trace,
        );
        assert_eq!(state.pending_events.len(), 2);
        assert!(matches!(
            state.pending_events[1].kind,
            EventKind::OceansStabilized { sea_level_m: 0.0 }
        ));
        assert_eq!(state.pending_events[0].year, WorldYear(250_000_000));
        assert_eq!(state.pending_events[1].year, WorldYear(350_000_000));
    }

    #[test]
    fn maybe_emit_respects_granularity() {
        let mut state = HydrologyState::new();
        maybe_emit_formation_ocean_events(
            &mut state,
            1.0,
            512,
            0.0,
            WorldYear(350_000_000),
            Significance::Pivotal,
        );
        assert!(
            state.pending_events.is_empty(),
            "Major events stay below a Pivotal threshold"
        );
        assert!(state.oceans_begin_emitted && state.oceans_stabilized_emitted);
    }
}
