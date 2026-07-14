//! Boundary-driven subduction arc volcanism (Doc 06 §5.5).

use std::collections::BTreeSet;

use genesis_core::branches::BranchId;
use genesis_core::data::{BedrockType, WorldData};
use genesis_core::events::{Event, EventKind, EventLocation, Significance};
use genesis_core::rng::WorldRng;
use genesis_core::time::WorldYear;
use genesis_core::{HexId, PlateId};
use rand::Rng;

use crate::boundary::{BoundaryClass, BoundaryInfo, ConvergentSubtype};
use crate::elevation::subducting_plate_id;
use crate::events::{alloc_event_id, maybe_emit};
use crate::plate::{Plate, PlateRegistry, PlateType};
use crate::plate_surface::modify_surface_at_world_hex;

/// RNG stream for eruption rolls and magnitude sampling (Doc 06 §4.4).
pub const VOLCANISM_STREAM: &str = "tectonics.volcanism";

/// Per-tick eruption probability per arc hex at `volcanism_scale == 1.0` (§5.5).
pub const ERUPTION_PROBABILITY_BASE: f64 = 0.05;

pub const ELEVATION_CHANGE_MIN_M: f32 = 100.0;
pub const ELEVATION_CHANGE_MAX_M: f32 = 500.0;
pub const RELIEF_CHANGE_MIN_M: f32 = 50.0;
pub const RELIEF_CHANGE_MAX_M: f32 = 200.0;

/// Peak elevation proxy above which eruptions are `Notable` (§6.2).
pub const NOTABLE_PEAK_THRESHOLD_M: f32 = 2000.0;

/// Stochastic eruptions at subduction arc boundary hexes.
///
/// Terrain is always updated when an eruption fires; events are appended to
/// `state.pending_events` only when `significance >= event_granularity`.
pub fn apply_boundary_volcanism(
    data: &mut WorldData,
    state: &mut crate::plate::TectonicsState,
    rng: &WorldRng,
    volcanism_scale: f32,
    event_granularity: Significance,
    tick_year: WorldYear,
    branch_id: BranchId,
) {
    if volcanism_scale <= 0.0 {
        return;
    }

    let probability = ERUPTION_PROBABILITY_BASE * f64::from(volcanism_scale);
    let arc_hexes = collect_arc_hexes(data, &state.registry, &state.boundaries);
    let mut volcanism_rng = rng.stream_at(VOLCANISM_STREAM, tick_year.value() as u64);

    for hex in arc_hexes {
        let roll: f64 = volcanism_rng.gen_range(0.0..1.0);
        if roll >= probability {
            continue;
        }

        let idx = hex.0 as usize;
        if idx >= data.plate_id.len() {
            continue;
        }
        let plate_id = data.plate_id[idx];

        let elev_change: f32 =
            volcanism_rng.gen_range(ELEVATION_CHANGE_MIN_M..=ELEVATION_CHANGE_MAX_M);
        let relief_change: f32 = volcanism_rng.gen_range(RELIEF_CHANGE_MIN_M..=RELIEF_CHANGE_MAX_M);

        modify_surface_at_world_hex(
            &mut state.registry,
            data,
            hex,
            tick_year.value(),
            |feature| {
                feature.elevation_m +=
                    elev_change * crate::elevation::uplift_headroom_factor(feature.elevation_m);
                feature.relief_m += relief_change;
                feature.bedrock = BedrockType::Igneous;
            },
        );

        let peak_proxy = data.elevation_mean[idx] + elev_change + data.elevation_relief[idx];
        let significance = eruption_significance(peak_proxy);

        let event_id = alloc_event_id(state);
        maybe_emit(
            state,
            Event {
                id: event_id,
                year: tick_year,
                branch_id,
                location: EventLocation::Hex(hex),
                significance,
                kind: EventKind::VolcanicEruption {
                    hex,
                    elevation_change_m: elev_change,
                    plate: plate_id,
                },
            },
            event_granularity,
        );
    }
}

/// Collects unique subduction-arc boundary hexes in ascending `HexId` order.
fn collect_arc_hexes(
    data: &WorldData,
    registry: &PlateRegistry,
    boundaries: &BoundaryInfo,
) -> Vec<HexId> {
    let mut set = BTreeSet::new();

    for &hex in &boundaries.boundary_hexes {
        let owner_plate_id = data.plate_id[hex.0 as usize];
        let Some(owner_plate) = registry.get(owner_plate_id) else {
            continue;
        };
        let edges = match boundaries.edges.get(&hex) {
            Some(e) => e,
            None => continue,
        };

        for edge in edges {
            let Some(other_plate) = registry.get(edge.other_plate) else {
                continue;
            };
            if is_arc_hex(owner_plate_id, owner_plate, other_plate, edge.class) {
                set.insert(hex);
            }
        }
    }

    set.into_iter().collect()
}

/// Arc side of a convergent subduction boundary (P1-4 rules).
pub fn is_arc_hex(
    owner_plate_id: PlateId,
    owner_plate: &Plate,
    other_plate: &Plate,
    class: BoundaryClass,
) -> bool {
    match class {
        BoundaryClass::Convergent(ConvergentSubtype::ContinentalOceanic) => {
            owner_plate.plate_type == PlateType::Continental
        }
        BoundaryClass::Convergent(ConvergentSubtype::OceanicOceanic) => {
            let subducting =
                subducting_plate_id(owner_plate_id, other_plate.id, owner_plate, other_plate);
            owner_plate_id != subducting
        }
        _ => false,
    }
}

fn eruption_significance(peak_proxy_m: f32) -> Significance {
    if peak_proxy_m > NOTABLE_PEAK_THRESHOLD_M {
        Significance::Notable
    } else {
        Significance::Minor
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::time::WorldYear;
    use genesis_core::{HexGrid, PlateId};

    use crate::boundary::{BoundaryClass, ClassifiedEdge, ConvergentSubtype};
    use crate::plate::{Plate, PlateRegistry, PlateType};
    use crate::plate_surface::SurfaceFeature;
    use crate::world_rebuild::rebuild_world_from_plate_surfaces;

    const EARTH_RADIUS_KM: f64 = 6371.0;

    fn plate_at(id: u16, plate_type: PlateType, seed: u32, rate: f64) -> Plate {
        Plate::test_plate(id, plate_type, seed, rate, 10_000)
    }

    fn seed_surfaces_from_world(data: &WorldData, registry: &mut PlateRegistry) {
        for hex in data.grid.iter() {
            let idx = hex.0 as usize;
            let plate_id = data.plate_id[idx];
            let Some(plate) = registry.plates_mut().get_mut(&plate_id) else {
                continue;
            };
            plate.surface.set(
                hex,
                SurfaceFeature {
                    elevation_m: data.elevation_mean[idx],
                    relief_m: data.elevation_relief[idx],
                    bedrock: data.bedrock_type[idx],
                    fertility: data.fertility[idx],
                    age_year: 0,
                },
            );
        }
    }

    fn apply_volcanism_and_rebuild(
        data: &mut WorldData,
        state: &mut crate::plate::TectonicsState,
        rng: &genesis_core::rng::WorldRng,
        volcanism_scale: f32,
        granularity: Significance,
        year: WorldYear,
    ) {
        seed_surfaces_from_world(data, &mut state.registry);
        apply_boundary_volcanism(
            data,
            state,
            rng,
            volcanism_scale,
            granularity,
            year,
            BranchId::ROOT,
        );
        rebuild_world_from_plate_surfaces(data, &state.registry);
    }

    #[test]
    fn oc_oceanic_owner_is_not_arc() {
        let oceanic = plate_at(0, PlateType::Oceanic, 0, 1e-8);
        let continental = plate_at(1, PlateType::Continental, 1, 1e-8);
        assert!(!is_arc_hex(
            PlateId(0),
            &oceanic,
            &continental,
            BoundaryClass::Convergent(ConvergentSubtype::ContinentalOceanic),
        ));
    }

    #[test]
    fn oc_continental_owner_is_arc() {
        let continental = plate_at(1, PlateType::Continental, 1, 1e-8);
        let oceanic = plate_at(0, PlateType::Oceanic, 0, 1e-8);
        assert!(is_arc_hex(
            PlateId(1),
            &continental,
            &oceanic,
            BoundaryClass::Convergent(ConvergentSubtype::ContinentalOceanic),
        ));
    }

    #[test]
    fn oo_overriding_plate_is_arc() {
        let fast = plate_at(0, PlateType::Oceanic, 0, 2e-8);
        let slow = plate_at(1, PlateType::Oceanic, 1, 1e-8);
        assert!(is_arc_hex(
            PlateId(1),
            &slow,
            &fast,
            BoundaryClass::Convergent(ConvergentSubtype::OceanicOceanic),
        ));
        assert!(!is_arc_hex(
            PlateId(0),
            &fast,
            &slow,
            BoundaryClass::Convergent(ConvergentSubtype::OceanicOceanic),
        ));
    }

    #[test]
    fn volcanism_scale_zero_skips_changes() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let params = WorldParameters::default();
        let mut data = WorldData::new(grid, params);
        let mut registry = PlateRegistry::new();
        registry.insert(plate_at(0, PlateType::Continental, 0, 1e-8));
        registry.insert(plate_at(1, PlateType::Oceanic, 100, 1e-8));

        let hex = HexId(10);
        data.plate_id[hex.0 as usize] = PlateId(0);
        let before = data.elevation_mean[hex.0 as usize];

        let mut boundaries = BoundaryInfo::default();
        boundaries.boundary_hexes.push(hex);
        boundaries.edges.insert(
            hex,
            vec![ClassifiedEdge {
                neighbor_hex: HexId(11),
                other_plate: PlateId(1),
                class: BoundaryClass::Convergent(ConvergentSubtype::ContinentalOceanic),
                normal_velocity_m_per_year: 0.05,
                tangential_velocity_m_per_year: 0.0,
            }],
        );

        let rng = genesis_core::rng::WorldRng::from_effective_seed(42);
        let mut tectonics_state = crate::plate::TectonicsState {
            registry,
            boundaries,
            ..Default::default()
        };
        apply_boundary_volcanism(
            &mut data,
            &mut tectonics_state,
            &rng,
            0.0,
            Significance::Trace,
            WorldYear(500_000),
            BranchId::ROOT,
        );

        assert_eq!(data.elevation_mean[hex.0 as usize], before);
        assert!(tectonics_state.pending_events.is_empty());
    }

    #[test]
    fn forced_eruption_raises_elevation_and_igneous_bedrock() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let params = WorldParameters::default();
        let mut data = WorldData::new(grid, params);
        let mut registry = PlateRegistry::new();
        registry.insert(plate_at(0, PlateType::Continental, 0, 1e-8));
        registry.insert(plate_at(1, PlateType::Oceanic, 100, 1e-8));

        let hex = HexId(10);
        data.plate_id[hex.0 as usize] = PlateId(0);
        data.elevation_mean[hex.0 as usize] = 500.0;
        data.bedrock_type[hex.0 as usize] = BedrockType::Metamorphic;

        let mut boundaries = BoundaryInfo::default();
        boundaries.boundary_hexes.push(hex);
        boundaries.edges.insert(
            hex,
            vec![ClassifiedEdge {
                neighbor_hex: HexId(11),
                other_plate: PlateId(1),
                class: BoundaryClass::Convergent(ConvergentSubtype::ContinentalOceanic),
                normal_velocity_m_per_year: 0.05,
                tangential_velocity_m_per_year: 0.0,
            }],
        );

        let rng = genesis_core::rng::WorldRng::from_effective_seed(7);
        let mut tectonics_state = crate::plate::TectonicsState {
            registry,
            boundaries,
            ..Default::default()
        };
        apply_volcanism_and_rebuild(
            &mut data,
            &mut tectonics_state,
            &rng,
            20.0,
            Significance::Trace,
            WorldYear(500_000),
        );

        assert!(data.elevation_mean[hex.0 as usize] > 500.0);
        assert_eq!(data.bedrock_type[hex.0 as usize], BedrockType::Igneous);
        assert!(!tectonics_state.pending_events.is_empty());
    }

    #[test]
    fn granularity_filters_minor_from_log_but_applies_terrain() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let params = WorldParameters::default();
        let mut data = WorldData::new(grid, params);
        let mut registry = PlateRegistry::new();
        registry.insert(plate_at(0, PlateType::Continental, 0, 1e-8));
        registry.insert(plate_at(1, PlateType::Oceanic, 100, 1e-8));

        let hex = HexId(10);
        data.plate_id[hex.0 as usize] = PlateId(0);
        data.elevation_mean[hex.0 as usize] = 100.0;
        data.elevation_relief[hex.0 as usize] = 0.0;

        let mut boundaries = BoundaryInfo::default();
        boundaries.boundary_hexes.push(hex);
        boundaries.edges.insert(
            hex,
            vec![ClassifiedEdge {
                neighbor_hex: HexId(11),
                other_plate: PlateId(1),
                class: BoundaryClass::Convergent(ConvergentSubtype::ContinentalOceanic),
                normal_velocity_m_per_year: 0.05,
                tangential_velocity_m_per_year: 0.0,
            }],
        );

        let rng = genesis_core::rng::WorldRng::from_effective_seed(99);
        let mut tectonics_state = crate::plate::TectonicsState {
            registry,
            boundaries,
            ..Default::default()
        };
        apply_volcanism_and_rebuild(
            &mut data,
            &mut tectonics_state,
            &rng,
            20.0,
            Significance::Notable,
            WorldYear(500_000),
        );

        assert!(data.elevation_mean[hex.0 as usize] > 100.0);
        assert!(tectonics_state.pending_events.is_empty());
    }

    #[test]
    fn eruptions_are_deterministic_for_fixed_seed() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let params = WorldParameters::default();
        let mut data_a = WorldData::new(
            HexGrid::new(4, EARTH_RADIUS_KM).expect("grid"),
            params.clone(),
        );
        let mut data_b = WorldData::new(grid, params);

        let mut reg = PlateRegistry::new();
        reg.insert(plate_at(0, PlateType::Continental, 0, 1e-8));
        reg.insert(plate_at(1, PlateType::Oceanic, 100, 1e-8));

        let hex = HexId(10);
        for data in [&mut data_a, &mut data_b] {
            data.plate_id[hex.0 as usize] = PlateId(0);
            data.elevation_mean[hex.0 as usize] = 500.0;
        }

        let mut boundaries = BoundaryInfo::default();
        boundaries.boundary_hexes.push(hex);
        boundaries.edges.insert(
            hex,
            vec![ClassifiedEdge {
                neighbor_hex: HexId(11),
                other_plate: PlateId(1),
                class: BoundaryClass::Convergent(ConvergentSubtype::ContinentalOceanic),
                normal_velocity_m_per_year: 0.05,
                tangential_velocity_m_per_year: 0.0,
            }],
        );

        let rng_a = genesis_core::rng::WorldRng::from_effective_seed(12345);
        let rng_b = genesis_core::rng::WorldRng::from_effective_seed(12345);

        let mut state_a = crate::plate::TectonicsState {
            registry: reg.clone(),
            boundaries: boundaries.clone(),
            ..Default::default()
        };
        let mut state_b = crate::plate::TectonicsState {
            registry: reg,
            boundaries,
            ..Default::default()
        };

        apply_boundary_volcanism(
            &mut data_a,
            &mut state_a,
            &rng_a,
            1.0,
            Significance::Trace,
            WorldYear(1_000_000),
            BranchId::ROOT,
        );
        apply_boundary_volcanism(
            &mut data_b,
            &mut state_b,
            &rng_b,
            1.0,
            Significance::Trace,
            WorldYear(1_000_000),
            BranchId::ROOT,
        );

        assert_eq!(data_a.elevation_mean, data_b.elevation_mean);
        assert_eq!(state_a.pending_events, state_b.pending_events);
    }

    #[test]
    fn eruptions_differ_across_ticks_with_same_arc_fixture() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let params = WorldParameters::default();
        let mut data_a = WorldData::new(
            HexGrid::new(4, EARTH_RADIUS_KM).expect("grid"),
            params.clone(),
        );
        let mut data_b = WorldData::new(grid, params);

        let mut reg = PlateRegistry::new();
        reg.insert(plate_at(0, PlateType::Continental, 0, 1e-8));
        reg.insert(plate_at(1, PlateType::Oceanic, 100, 1e-8));

        let hex = HexId(10);
        for data in [&mut data_a, &mut data_b] {
            data.plate_id[hex.0 as usize] = PlateId(0);
            data.elevation_mean[hex.0 as usize] = 500.0;
            data.elevation_relief[hex.0 as usize] = 0.0;
        }

        let boundaries = {
            let mut b = BoundaryInfo::default();
            b.boundary_hexes.push(hex);
            b.edges.insert(
                hex,
                vec![ClassifiedEdge {
                    neighbor_hex: HexId(11),
                    other_plate: PlateId(1),
                    class: BoundaryClass::Convergent(ConvergentSubtype::ContinentalOceanic),
                    normal_velocity_m_per_year: 0.05,
                    tangential_velocity_m_per_year: 0.0,
                }],
            );
            b
        };

        let rng = genesis_core::rng::WorldRng::from_effective_seed(99);

        let mut state_a = crate::plate::TectonicsState {
            registry: reg.clone(),
            boundaries: boundaries.clone(),
            ..Default::default()
        };
        let mut state_b = crate::plate::TectonicsState {
            registry: reg,
            boundaries,
            ..Default::default()
        };

        apply_volcanism_and_rebuild(
            &mut data_a,
            &mut state_a,
            &rng,
            20.0,
            Significance::Trace,
            WorldYear(500_000),
        );
        apply_volcanism_and_rebuild(
            &mut data_b,
            &mut state_b,
            &rng,
            20.0,
            Significance::Trace,
            WorldYear(1_000_000),
        );

        assert_ne!(
            data_a.elevation_mean, data_b.elevation_mean,
            "tick-scoped volcanism stream should not replay identical rolls"
        );
    }
}
