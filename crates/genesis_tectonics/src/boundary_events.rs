//! Boundary-derived tectonic events (Doc 06 §6.1).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use genesis_core::branches::BranchId;
use genesis_core::data::{BedrockType, WorldData};
use genesis_core::events::{BoundaryType, Event, EventKind, EventLocation, Significance};
use genesis_core::time::WorldYear;
use genesis_core::{HexId, PlateId};

use crate::boundary::{BoundaryClass, BoundaryInfo, ConvergentSubtype};
use crate::events::{alloc_event_id, maybe_emit};
use crate::plate::{PlateRegistry, TectonicsState};

/// Uplift this tick (m) to qualify a CC boundary hex for mountain-range grouping.
const MOUNTAIN_UPLIFT_THRESHOLD_M: f32 = 200.0;

/// Elevation (m) to qualify a CC boundary hex for mountain-range grouping.
const MOUNTAIN_ELEVATION_THRESHOLD_M: f32 = 3000.0;

/// Uplift this tick (m) at divergent hexes to qualify for ocean-basin opening.
const OCEAN_BASIN_UPLIFT_THRESHOLD_M: f32 = -100.0;

/// Maps tectonics [`BoundaryClass`] to core [`BoundaryType`] for events.
pub fn boundary_type_from_class(class: BoundaryClass) -> BoundaryType {
    match class {
        BoundaryClass::Divergent => BoundaryType::Divergent,
        BoundaryClass::Transform => BoundaryType::Transform,
        BoundaryClass::Convergent(ConvergentSubtype::ContinentalContinental) => {
            BoundaryType::ConvergentContinentalContinental
        }
        BoundaryClass::Convergent(ConvergentSubtype::OceanicOceanic) => {
            BoundaryType::ConvergentOceanicOceanic
        }
        BoundaryClass::Convergent(ConvergentSubtype::ContinentalOceanic) => {
            BoundaryType::ConvergentContinentalOceanic
        }
    }
}

struct PendingEvent {
    location: EventLocation,
    significance: Significance,
    kind: EventKind,
}

/// Emits mountain-range, ocean-basin, and boundary-transition events.
pub fn emit_boundary_events(
    data: &WorldData,
    boundaries: &BoundaryInfo,
    state: &mut TectonicsState,
    tick_year: WorldYear,
    event_granularity: Significance,
    branch_id: BranchId,
) {
    let snapshot = state.elevation_at_tick_start.clone();
    let registry = &state.registry;

    let mut pending = Vec::new();
    pending.extend(collect_mountain_events(
        data, registry, boundaries, &snapshot,
    ));
    pending.extend(collect_ocean_basin_events(data, boundaries, &snapshot));
    pending.extend(collect_boundary_transitions(
        boundaries,
        &state.previous_edge_class,
    ));

    for pending_event in pending {
        let event_id = alloc_event_id(state);
        maybe_emit(
            state,
            Event {
                id: event_id,
                year: tick_year,
                branch_id,
                location: pending_event.location,
                significance: pending_event.significance,
                kind: pending_event.kind,
            },
            event_granularity,
        );
    }

    state.previous_edge_class = current_edge_classes(boundaries);
}

fn collect_mountain_events(
    data: &WorldData,
    registry: &PlateRegistry,
    boundaries: &BoundaryInfo,
    snapshot: &[f32],
) -> Vec<PendingEvent> {
    if snapshot.len() != data.elevation_mean.len() {
        return Vec::new();
    }

    let mut pending = Vec::new();
    let mut qualifying: BTreeSet<HexId> = BTreeSet::new();
    for (&hex, edges) in &boundaries.edges {
        for edge in edges {
            if !matches!(
                edge.class,
                BoundaryClass::Convergent(ConvergentSubtype::ContinentalContinental)
            ) {
                continue;
            }
            let idx = hex.0 as usize;
            if idx >= data.elevation_mean.len() {
                continue;
            }
            let uplift = data.elevation_mean[idx] - snapshot[idx];
            if uplift >= MOUNTAIN_UPLIFT_THRESHOLD_M
                || data.elevation_mean[idx] >= MOUNTAIN_ELEVATION_THRESHOLD_M
            {
                qualifying.insert(hex);
            }
        }
    }

    let mut visited: BTreeSet<HexId> = BTreeSet::new();
    for &start in qualifying.iter() {
        if visited.contains(&start) {
            continue;
        }
        let run = bfs_cc_run(start, &qualifying, boundaries);
        if run.is_empty() {
            continue;
        }
        for &h in &run {
            visited.insert(h);
        }

        let peak = run
            .iter()
            .map(|h| data.elevation_mean[h.0 as usize])
            .fold(f32::NEG_INFINITY, f32::max);
        let plates = plate_pair_for_hex(data, start, registry);
        let Some(plates) = plates else {
            continue;
        };

        pending.push(PendingEvent {
            location: EventLocation::Region(run.clone()),
            significance: Significance::Major,
            kind: EventKind::MountainRangeFormed {
                boundary_hexes: run,
                plates,
                peak_elevation_m: peak,
            },
        });
    }

    pending
}

fn bfs_cc_run(start: HexId, qualifying: &BTreeSet<HexId>, boundaries: &BoundaryInfo) -> Vec<HexId> {
    let mut run = Vec::new();
    let mut queue = VecDeque::new();
    let mut seen = BTreeSet::new();
    queue.push_back(start);
    seen.insert(start);

    while let Some(hex) = queue.pop_front() {
        if !qualifying.contains(&hex) {
            continue;
        }
        run.push(hex);
        let Some(edges) = boundaries.edges.get(&hex) else {
            continue;
        };
        for edge in edges {
            if !matches!(
                edge.class,
                BoundaryClass::Convergent(ConvergentSubtype::ContinentalContinental)
            ) {
                continue;
            }
            let neighbor = edge.neighbor_hex;
            if qualifying.contains(&neighbor) && seen.insert(neighbor) {
                queue.push_back(neighbor);
            }
        }
    }

    run.sort_by_key(|h| h.0);
    run
}

fn collect_ocean_basin_events(
    data: &WorldData,
    boundaries: &BoundaryInfo,
    snapshot: &[f32],
) -> Vec<PendingEvent> {
    if snapshot.len() != data.elevation_mean.len() {
        return Vec::new();
    }

    let mut pending = Vec::new();
    let mut emitted_pairs: BTreeSet<(PlateId, PlateId)> = BTreeSet::new();

    for (&hex, edges) in &boundaries.edges {
        let idx = hex.0 as usize;
        if idx >= data.elevation_mean.len() {
            continue;
        }
        let uplift = data.elevation_mean[idx] - snapshot[idx];
        let owner_plate = data.plate_id[idx];

        for edge in edges {
            if edge.class != BoundaryClass::Divergent {
                continue;
            }
            let qualifies = uplift <= OCEAN_BASIN_UPLIFT_THRESHOLD_M
                || data.bedrock_type[idx] == BedrockType::OceanicCrust;
            if !qualifies {
                continue;
            }
            let pair = canonical_plate_pair(owner_plate, edge.other_plate);
            if !emitted_pairs.insert(pair) {
                continue;
            }

            pending.push(PendingEvent {
                location: EventLocation::Hex(hex),
                significance: Significance::Major,
                kind: EventKind::OceanBasinOpened {
                    boundary_hexes: vec![hex],
                    plates: pair,
                },
            });
        }
    }

    pending
}

fn current_edge_classes(boundaries: &BoundaryInfo) -> BTreeMap<(HexId, HexId), BoundaryType> {
    let mut current_edges: BTreeMap<(HexId, HexId), BoundaryType> = BTreeMap::new();

    for (&hex, edges) in &boundaries.edges {
        for edge in edges {
            let key = canonical_hex_edge(hex, edge.neighbor_hex);
            let class = boundary_type_from_class(edge.class);
            current_edges.insert(key, class);
        }
    }

    current_edges
}

fn collect_boundary_transitions(
    boundaries: &BoundaryInfo,
    previous: &BTreeMap<(HexId, HexId), BoundaryType>,
) -> Vec<PendingEvent> {
    let mut pending = Vec::new();

    for (&hex, edges) in &boundaries.edges {
        for edge in edges {
            let key = canonical_hex_edge(hex, edge.neighbor_hex);
            let class = boundary_type_from_class(edge.class);
            if let Some(&from) = previous.get(&key)
                && from != class
            {
                pending.push(PendingEvent {
                    location: EventLocation::Hex(hex),
                    significance: Significance::Trace,
                    kind: EventKind::BoundaryTransition {
                        hex,
                        from,
                        to: class,
                    },
                });
            }
        }
    }

    pending
}

fn canonical_hex_edge(a: HexId, b: HexId) -> (HexId, HexId) {
    if a < b { (a, b) } else { (b, a) }
}

fn canonical_plate_pair(a: PlateId, b: PlateId) -> (PlateId, PlateId) {
    if a < b { (a, b) } else { (b, a) }
}

fn plate_pair_for_hex(
    data: &WorldData,
    hex: HexId,
    registry: &PlateRegistry,
) -> Option<(PlateId, PlateId)> {
    let idx = hex.0 as usize;
    let owner = data.plate_id.get(idx)?;
    if *owner == PlateId::NONE {
        return None;
    }
    let grid = &data.grid;
    let mut neighbors: Vec<HexId> = grid.neighbors(hex).to_vec();
    neighbors.sort_by_key(|h| h.0);
    for neighbor_hex in neighbors {
        let j = neighbor_hex.0 as usize;
        let other = data.plate_id.get(j)?;
        if *other != PlateId::NONE && *other != *owner {
            let _ = registry.get(*other)?;
            return Some(canonical_plate_pair(*owner, *other));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::create_world;
    use genesis_core::parameters::WorldParameters;

    use crate::boundary::{BoundaryClass, BoundaryInfo, ClassifiedEdge};
    use crate::history::run_formation;
    use crate::plate::TectonicsState;

    fn test_world() -> genesis_core::World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        create_world(params).expect("world")
    }

    #[test]
    fn boundary_transition_emitted_on_class_change() {
        let mut world = test_world();
        let mut state = TectonicsState::new();
        run_formation(&mut world, &mut state);

        let hex = HexId(0);
        let neighbor = HexId(1);
        let key = canonical_hex_edge(hex, neighbor);
        state
            .previous_edge_class
            .insert(key, BoundaryType::Divergent);

        let mut boundaries = BoundaryInfo::default();
        boundaries.edges.insert(
            hex,
            vec![ClassifiedEdge {
                neighbor_hex: neighbor,
                other_plate: PlateId(1),
                class: BoundaryClass::Transform,
                normal_velocity_m_per_year: 0.0,
                tangential_velocity_m_per_year: 0.01,
            }],
        );
        state.elevation_at_tick_start = world.data.elevation_mean.clone();

        emit_boundary_events(
            &world.data,
            &boundaries,
            &mut state,
            WorldYear(500_000),
            Significance::Trace,
            BranchId::ROOT,
        );

        assert!(
            state.pending_events.iter().any(|e| {
                matches!(
                    e.kind,
                    EventKind::BoundaryTransition {
                        from: BoundaryType::Divergent,
                        to: BoundaryType::Transform,
                        ..
                    }
                )
            }),
            "expected BoundaryTransition event"
        );
    }
}
