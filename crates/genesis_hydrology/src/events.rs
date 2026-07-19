//! Hydrology event emission (Doc 08 §13).

use genesis_core::HexId;
use genesis_core::World;
use genesis_core::branches::BranchId;
use genesis_core::data::{HydroFlags, WaterBodyKind, WorldData};
use genesis_core::events::{Event, EventId, EventKind, EventLocation, Significance};
use genesis_core::time::WorldYear;

use crate::rivers::{RiverClass, river_class};
use crate::state::HydrologyState;

/// Sea-level change (m) that upgrades a milestone to Major significance.
pub const SEA_LEVEL_MAJOR_DELTA_M: f32 = 50.0;
/// Minimum Absolute sea-level change to emit any milestone.
pub const SEA_LEVEL_NOTEABLE_DELTA_M: f32 = 5.0;
/// Hexes of path divergence required for RiverCourseShifted.
pub const RIVER_AVULSION_HEXES: usize = 8;
/// Ice SLE drop (m) that qualifies as a glacial maximum narrative.
pub const GLACIAL_MAX_SLE_DROP_M: f32 = 60.0;

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

/// Emits the formation ocean events (Doc 08 §13).
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

/// Sea-level milestone from tick-to-tick change (§13).
pub fn maybe_emit_sea_level_milestone(
    state: &mut HydrologyState,
    sea_level_m: f32,
    year: WorldYear,
    granularity: Significance,
) {
    let prev = state.prev_sea_level_m.replace(sea_level_m);
    let Some(prev) = prev else {
        return;
    };
    let delta = sea_level_m - prev;
    if delta.abs() < SEA_LEVEL_NOTEABLE_DELTA_M {
        return;
    }
    let significance = if delta.abs() >= SEA_LEVEL_MAJOR_DELTA_M {
        Significance::Major
    } else {
        Significance::Notable
    };
    let event_id = alloc_event_id(state);
    maybe_emit(
        state,
        Event {
            id: event_id,
            year,
            branch_id: BranchId::ROOT,
            location: EventLocation::Global,
            significance,
            kind: EventKind::SeaLevelMilestone {
                level_m: sea_level_m,
                delta_m: delta,
            },
        },
        granularity,
    );
}

/// Registry-diff events: lakes, inland seas, salt bodies (§13).
pub fn maybe_emit_registry_events(
    state: &mut HydrologyState,
    data: &WorldData,
    year: WorldYear,
    granularity: Significance,
) {
    let current = &data.water_bodies;
    let prev = state.prev_water_bodies.clone();

    for (id, body) in current {
        let Some(old) = prev.get(id) else {
            match body.kind {
                WaterBodyKind::Lake | WaterBodyKind::SaltLake => {
                    let sig = if body.area_km2 >= 50.0 * hex_area_km2(data) {
                        Significance::Major
                    } else {
                        Significance::Notable
                    };
                    let event_id = alloc_event_id(state);
                    maybe_emit(
                        state,
                        Event {
                            id: event_id,
                            year,
                            branch_id: BranchId::ROOT,
                            location: EventLocation::Hex(HexId(id.0)),
                            significance: sig,
                            kind: EventKind::LakeFormed { body: *id },
                        },
                        granularity,
                    );
                    if body.kind == WaterBodyKind::SaltLake
                        && state.emitted_salt_lakes.insert(HexId(id.0))
                    {
                        let event_id = alloc_event_id(state);
                        maybe_emit(
                            state,
                            Event {
                                id: event_id,
                                year,
                                branch_id: BranchId::ROOT,
                                location: EventLocation::Hex(HexId(id.0)),
                                significance: Significance::Notable,
                                kind: EventKind::SaltLakeFormed {
                                    hex: HexId(id.0),
                                    salinity: body.salinity,
                                },
                            },
                            granularity,
                        );
                    }
                }
                WaterBodyKind::Sea => {
                    // New Sea with no prior Ocean sibling at this id → isolated.
                    let event_id = alloc_event_id(state);
                    maybe_emit(
                        state,
                        Event {
                            id: event_id,
                            year,
                            branch_id: BranchId::ROOT,
                            location: EventLocation::Hex(HexId(id.0)),
                            significance: Significance::Major,
                            kind: EventKind::InlandSeaIsolated { body: *id },
                        },
                        granularity,
                    );
                }
                WaterBodyKind::SaltFlat => {
                    if state.emitted_salt_flats.insert(HexId(id.0)) {
                        let event_id = alloc_event_id(state);
                        maybe_emit(
                            state,
                            Event {
                                id: event_id,
                                year,
                                branch_id: BranchId::ROOT,
                                location: EventLocation::Region(vec![HexId(id.0)]),
                                significance: Significance::Minor,
                                kind: EventKind::SaltFlatFormed {
                                    region: vec![HexId(id.0)],
                                },
                            },
                            granularity,
                        );
                    }
                }
                WaterBodyKind::Ocean => {}
            }
            continue;
        };

        // Kind transitions on a stable id.
        if old.kind == WaterBodyKind::Ocean && body.kind == WaterBodyKind::Sea {
            let event_id = alloc_event_id(state);
            maybe_emit(
                state,
                Event {
                    id: event_id,
                    year,
                    branch_id: BranchId::ROOT,
                    location: EventLocation::Hex(HexId(id.0)),
                    significance: Significance::Major,
                    kind: EventKind::InlandSeaIsolated { body: *id },
                },
                granularity,
            );
        } else if old.kind == WaterBodyKind::Sea && body.kind == WaterBodyKind::Ocean {
            let event_id = alloc_event_id(state);
            maybe_emit(
                state,
                Event {
                    id: event_id,
                    year,
                    branch_id: BranchId::ROOT,
                    location: EventLocation::Hex(HexId(id.0)),
                    significance: Significance::Major,
                    kind: EventKind::InlandSeaReconnected { body: *id },
                },
                granularity,
            );
        } else if matches!(body.kind, WaterBodyKind::SaltLake | WaterBodyKind::SaltFlat)
            && old.kind != body.kind
            && body.kind == WaterBodyKind::SaltLake
            && state.emitted_salt_lakes.insert(HexId(id.0))
        {
            let event_id = alloc_event_id(state);
            maybe_emit(
                state,
                Event {
                    id: event_id,
                    year,
                    branch_id: BranchId::ROOT,
                    location: EventLocation::Hex(HexId(id.0)),
                    significance: Significance::Notable,
                    kind: EventKind::SaltLakeFormed {
                        hex: HexId(id.0),
                        salinity: body.salinity,
                    },
                },
                granularity,
            );
        } else if body.kind == WaterBodyKind::SaltFlat
            && old.kind != WaterBodyKind::SaltFlat
            && state.emitted_salt_flats.insert(HexId(id.0))
        {
            let event_id = alloc_event_id(state);
            maybe_emit(
                state,
                Event {
                    id: event_id,
                    year,
                    branch_id: BranchId::ROOT,
                    location: EventLocation::Region(vec![HexId(id.0)]),
                    significance: Significance::Minor,
                    kind: EventKind::SaltFlatFormed {
                        region: vec![HexId(id.0)],
                    },
                },
                granularity,
            );
        }
    }

    for (id, old) in &prev {
        if current.contains_key(id) {
            continue;
        }
        if matches!(
            old.kind,
            WaterBodyKind::Lake | WaterBodyKind::SaltLake | WaterBodyKind::SaltFlat
        ) {
            let sig = if old.area_km2 >= 50.0 * hex_area_km2(data) {
                Significance::Major
            } else {
                Significance::Notable
            };
            let event_id = alloc_event_id(state);
            maybe_emit(
                state,
                Event {
                    id: event_id,
                    year,
                    branch_id: BranchId::ROOT,
                    location: EventLocation::Hex(HexId(id.0)),
                    significance: sig,
                    kind: EventKind::LakeDried { body: *id },
                },
                granularity,
            );
        }
    }

    state.prev_water_bodies = current.clone();
}

/// Flag-first-appearance events: fjords, oases, great springs (§13).
pub fn maybe_emit_flag_events(
    state: &mut HydrologyState,
    data: &WorldData,
    year: WorldYear,
    granularity: Significance,
) {
    let n = data.cell_count() as usize;
    let mut new_fjords = Vec::new();
    for i in 0..n {
        let hex = HexId(i as u32);
        let flags = data.hydro_flags[i];
        if flags.contains(HydroFlags::FJORD) && state.emitted_fjords.insert(hex) {
            new_fjords.push(hex);
        }
        if flags.contains(HydroFlags::OASIS) && state.emitted_oases.insert(hex) {
            let event_id = alloc_event_id(state);
            maybe_emit(
                state,
                Event {
                    id: event_id,
                    year,
                    branch_id: BranchId::ROOT,
                    location: EventLocation::Hex(hex),
                    significance: Significance::Notable,
                    kind: EventKind::OasisFormed { hex },
                },
                granularity,
            );
        }
        if flags.contains(HydroFlags::SPRING)
            && flags.contains(HydroFlags::KARST)
            && data.river_discharge_m3_yr[i] >= 1.0e9
            && state.emitted_springs.insert(hex)
        {
            let event_id = alloc_event_id(state);
            maybe_emit(
                state,
                Event {
                    id: event_id,
                    year,
                    branch_id: BranchId::ROOT,
                    location: EventLocation::Hex(hex),
                    significance: Significance::Minor,
                    kind: EventKind::GreatSpringEmerges { hex },
                },
                granularity,
            );
        }
    }
    if !new_fjords.is_empty() {
        new_fjords.sort_by_key(|h| h.0);
        let event_id = alloc_event_id(state);
        maybe_emit(
            state,
            Event {
                id: event_id,
                year,
                branch_id: BranchId::ROOT,
                location: EventLocation::Region(new_fjords.clone()),
                significance: Significance::Notable,
                kind: EventKind::FjordsCarved { region: new_fjords },
            },
            granularity,
        );
    }
}

/// Major-river avulsion detection (§13).
pub fn maybe_emit_river_course_shift(
    state: &mut HydrologyState,
    data: &WorldData,
    year: WorldYear,
    granularity: Significance,
) {
    let n = data.cell_count() as usize;
    if state.prev_flow_direction.len() != n {
        state.prev_flow_direction = data.flow_direction.clone();
        return;
    }
    let mut changed = Vec::new();
    for i in 0..n {
        if river_class(f64::from(data.river_discharge_m3_yr[i])) < RiverClass::Major {
            continue;
        }
        if data.flow_direction[i] != state.prev_flow_direction[i] {
            changed.push(HexId(i as u32));
        }
    }
    state.prev_flow_direction = data.flow_direction.clone();
    if changed.len() < RIVER_AVULSION_HEXES {
        return;
    }
    changed.sort_by_key(|h| h.0);
    let event_id = alloc_event_id(state);
    maybe_emit(
        state,
        Event {
            id: event_id,
            year,
            branch_id: BranchId::ROOT,
            location: EventLocation::Region(changed.clone()),
            significance: Significance::Notable,
            kind: EventKind::RiverCourseShifted { region: changed },
        },
        granularity,
    );
}

/// Glacial maximum when ice SLE drawdown first exceeds the gate band (§13).
pub fn maybe_emit_glacial_maximum(
    state: &mut HydrologyState,
    ice_volume_m3: f64,
    planet_area_m2: f64,
    year: WorldYear,
    granularity: Significance,
) {
    if state.glacial_maximum_emitted || planet_area_m2 <= 0.0 {
        return;
    }
    let sle_drop = (ice_volume_m3 / planet_area_m2) as f32;
    if sle_drop > state.peak_ice_sle_drop_m {
        state.peak_ice_sle_drop_m = sle_drop;
    }
    if sle_drop < GLACIAL_MAX_SLE_DROP_M {
        return;
    }
    state.glacial_maximum_emitted = true;
    let event_id = alloc_event_id(state);
    maybe_emit(
        state,
        Event {
            id: event_id,
            year,
            branch_id: BranchId::ROOT,
            location: EventLocation::Global,
            significance: Significance::Pivotal,
            kind: EventKind::GlacialMaximum {
                sea_level_drop_m: sle_drop,
            },
        },
        granularity,
    );
}

fn hex_area_km2(data: &WorldData) -> f64 {
    data.grid.hex_area_km2(HexId(0)).max(1e-9)
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
        maybe_emit_formation_ocean_events(
            &mut state,
            0.35,
            0,
            -1500.0,
            WorldYear(100_000_000),
            Significance::Trace,
        );
        assert!(state.pending_events.is_empty());

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

        maybe_emit_formation_ocean_events(
            &mut state,
            1.0,
            512,
            0.0,
            WorldYear(350_000_000),
            Significance::Trace,
        );
        assert_eq!(state.pending_events.len(), 2);
        assert!(matches!(
            state.pending_events[1].kind,
            EventKind::OceansStabilized { sea_level_m: 0.0 }
        ));
    }

    #[test]
    fn sea_level_milestone_requires_delta() {
        let mut state = HydrologyState::new();
        maybe_emit_sea_level_milestone(&mut state, 0.0, WorldYear(1), Significance::Trace);
        assert!(state.pending_events.is_empty());
        maybe_emit_sea_level_milestone(&mut state, 1.0, WorldYear(2), Significance::Trace);
        assert!(state.pending_events.is_empty());
        maybe_emit_sea_level_milestone(&mut state, 60.0, WorldYear(3), Significance::Trace);
        assert_eq!(state.pending_events.len(), 1);
        assert!(matches!(
            state.pending_events[0].kind,
            EventKind::SeaLevelMilestone {
                level_m: 60.0,
                delta_m: 59.0
            }
        ));
        assert_eq!(state.pending_events[0].significance, Significance::Major);
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
        assert!(state.pending_events.is_empty());
        assert!(state.oceans_begin_emitted && state.oceans_stabilized_emitted);
    }

    #[test]
    fn unused_major_threshold_is_referenced() {
        use crate::rivers::MAJOR_CLASS_MIN_M3_YR;
        assert!(river_class(MAJOR_CLASS_MIN_M3_YR) == RiverClass::Major);
    }
}
