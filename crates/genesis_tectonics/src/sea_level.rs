//! Sea level drift from divergent boundary length (Doc 06 §4.6).

use genesis_core::branches::BranchId;
use genesis_core::data::WorldData;
use genesis_core::events::{Event, EventKind, EventLocation, Significance};
use genesis_core::rng::WorldRng;
use genesis_core::time::WorldYear;
use glam::DVec3;
use rand::Rng;

use crate::boundary::{BoundaryClass, BoundaryInfo};
use crate::events::{alloc_event_id, maybe_emit};
use crate::plate::TectonicsState;
use crate::reorganization::REORGANIZATION_ACTION_STREAM;

/// Scaling from divergent-length excess to sea level change (m per km per tick).
const DIVERGENT_LENGTH_SCALE: f64 = 1e-6;

/// Equilibrium damping toward zero sea level (per tick).
const SEA_LEVEL_EQUILIBRIUM_K: f64 = 1e-6;

/// Reorganization-driven sea level excursion magnitude (m).
const REORG_SEA_LEVEL_EXCURSION_M: f64 = 100.0;

/// |delta_m| above this records `Notable` significance.
const NOTABLE_DELTA_THRESHOLD_M: f32 = 50.0;

/// Sum of great-circle edge lengths (km) over directed divergent boundary edges.
pub fn total_divergent_boundary_length_km(data: &WorldData, boundaries: &BoundaryInfo) -> f64 {
    let grid = &data.grid;
    let radius_km = data.parameters.core.planet.radius_km;
    let mut total = 0.0_f64;

    for (&hex, edges) in &boundaries.edges {
        let owner_dir = grid.cell_center_direction(hex);
        let owner_v = DVec3::new(owner_dir[0], owner_dir[1], owner_dir[2]);
        for edge in edges {
            if edge.class != BoundaryClass::Divergent {
                continue;
            }
            let neighbor_dir = grid.cell_center_direction(edge.neighbor_hex);
            let neighbor_v = DVec3::new(neighbor_dir[0], neighbor_dir[1], neighbor_dir[2]);
            let dot = owner_v.dot(neighbor_v).clamp(-1.0, 1.0);
            let angle_rad = dot.acos();
            total += angle_rad * radius_km;
        }
    }

    total
}

/// Updates global `sea_level_m` and may emit [`EventKind::SeaLevelChange`].
pub fn update_sea_level(
    data: &mut WorldData,
    boundaries: &BoundaryInfo,
    state: &mut TectonicsState,
    rng: &WorldRng,
    tick_year: WorldYear,
    reorg_fired: bool,
    event_granularity: Significance,
    branch_id: BranchId,
) {
    // Climate layer owns sea level during Formation period (Doc 07 §3.5).
    const FORMATION_END_YEAR: i64 = 500_000_000;
    if !data.parameters.core.climate.skip_planetary_formation
        && tick_year.value() <= FORMATION_END_YEAR
    {
        return;
    }

    let current_div_km = total_divergent_boundary_length_km(data, boundaries);

    if state.baseline_divergent_length_km.is_none() {
        state.baseline_divergent_length_km = Some(current_div_km);
    }
    let baseline = state.baseline_divergent_length_km.unwrap_or(current_div_km);

    let mut delta_m = (current_div_km - baseline) * DIVERGENT_LENGTH_SCALE
        - f64::from(data.sea_level_m) * SEA_LEVEL_EQUILIBRIUM_K;

    if reorg_fired {
        let mut action_rng = rng.stream_at(REORGANIZATION_ACTION_STREAM, tick_year.value() as u64);
        let excursion: f64 =
            action_rng.gen_range(-REORG_SEA_LEVEL_EXCURSION_M..=REORG_SEA_LEVEL_EXCURSION_M);
        delta_m += excursion;
    }

    let delta_f = delta_m as f32;
    data.sea_level_m += delta_f;
    let new_sea_level = data.sea_level_m;

    let significance = if delta_f.abs() > NOTABLE_DELTA_THRESHOLD_M {
        Significance::Notable
    } else {
        Significance::Trace
    };

    let event_id = alloc_event_id(state);
    maybe_emit(
        state,
        Event {
            id: event_id,
            year: tick_year,
            branch_id,
            location: EventLocation::Global,
            significance,
            kind: EventKind::SeaLevelChange {
                delta_m: delta_f,
                new_sea_level_m: new_sea_level,
            },
        },
        event_granularity,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{PlateId, create_world};

    use crate::boundary::{BoundaryClass, BoundaryInfo, ClassifiedEdge};
    use crate::history::run_formation;
    use crate::plate::TectonicsState;

    fn test_world() -> genesis_core::World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        create_world(params).expect("world")
    }

    #[test]
    fn more_divergent_length_raises_sea_level_vs_baseline() {
        let mut world = test_world();
        let mut state = TectonicsState::new();
        run_formation(&mut world, &mut state);

        state.baseline_divergent_length_km = Some(0.0);
        let mut boundaries = BoundaryInfo::default();
        boundaries.edges.insert(
            genesis_core::HexId(0),
            vec![ClassifiedEdge {
                neighbor_hex: genesis_core::HexId(1),
                other_plate: PlateId(1),
                class: BoundaryClass::Divergent,
                normal_velocity_m_per_year: -0.01,
                tangential_velocity_m_per_year: 0.0,
            }],
        );

        let before = world.data.sea_level_m;
        // Post-formation year: climate owns sea level during 0–500M (Doc 07 §3.5).
        update_sea_level(
            &mut world.data,
            &boundaries,
            &mut state,
            &world.rng,
            genesis_core::time::WorldYear(600_000_000),
            false,
            Significance::Trace,
            genesis_core::branches::BranchId::ROOT,
        );
        assert!(world.data.sea_level_m > before);
    }

    #[test]
    fn damping_keeps_sea_level_bounded_over_many_ticks() {
        let mut world = test_world();
        let mut state = TectonicsState::new();
        run_formation(&mut world, &mut state);
        state.baseline_divergent_length_km = Some(1000.0);

        let boundaries = state.boundaries.clone();
        for tick in 1..=200 {
            update_sea_level(
                &mut world.data,
                &boundaries,
                &mut state,
                &world.rng,
                genesis_core::time::WorldYear(600_000_000 + tick * 500_000),
                false,
                Significance::Trace,
                genesis_core::branches::BranchId::ROOT,
            );
        }
        assert!(
            world.data.sea_level_m.abs() < 500.0,
            "sea level should remain bounded with damping, got {}",
            world.data.sea_level_m
        );
    }
}
