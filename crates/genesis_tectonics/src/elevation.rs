//! Per-tick boundary elevation and bedrock updates (Doc 06 §5).

use std::collections::{BTreeMap, VecDeque};

use genesis_core::data::{BedrockType, WorldData};
use genesis_core::{HexId, PlateId};

use crate::boundary::{BoundaryClass, BoundaryInfo, ClassifiedEdge, ConvergentSubtype};
use crate::plate::{Plate, PlateRegistry, PlateType};

/// Minimum elevation (Mariana Trench depth), meters (§5.7).
pub const MIN_ELEVATION_M: f32 = -11_000.0;

/// Maximum elevation, meters (§5.7).
pub const MAX_ELEVATION_M: f32 = 9_000.0;

/// Maximum vertical relief within a hex, meters (§5.7).
pub const MAX_RELIEF_M: f32 = 5_000.0;

/// Divergent subsidence: m per (cm/year × year) (§5.1).
pub const SUBSIDENCE_RATE: f64 = 0.001;

/// Continental–continental orogeny rate (§5.2).
pub const OROGENY_RATE: f64 = 0.005;

/// Subduction trench deepening rate (§5.3–§5.4).
pub const SUBDUCTION_RATE: f64 = 0.02;

/// Continental rifting subsidence multiplier (§5.1 heuristic).
pub const CONTINENTAL_RIFT_SUBSIDENCE_FACTOR: f64 = 0.3;

/// Inland orogeny spread depth for continental–continental (§5.2).
pub const CC_INLAND_HEXES: u32 = 3;

/// Inland uplift spread for oceanic–continental (§5.3).
pub const OC_INLAND_HEXES: u32 = 3;

/// Coastal uplift fraction of orogeny delta on continental boundary hex.
const OC_COASTAL_UPLIFT_FACTOR: f64 = 0.5;

/// Island-arc uplift fraction of subduction delta on overriding oceanic hex.
const OO_ARC_UPLIFT_FACTOR: f64 = 0.5;

const INLAND_FALLOFF: [f64; 3] = [1.0, 0.67, 0.33];

#[derive(Default)]
struct HexDeltas {
    elev: f64,
    relief: f64,
    bedrock: Option<BedrockType>,
}

/// Applies boundary-driven elevation and bedrock changes, then clamps.
pub fn apply_boundary_elevation(
    data: &mut WorldData,
    registry: &PlateRegistry,
    boundaries: &BoundaryInfo,
    tick_interval_years: f64,
) {
    let mut deltas: BTreeMap<HexId, HexDeltas> = BTreeMap::new();

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
            apply_edge(
                data,
                &mut deltas,
                hex,
                edge,
                owner_plate_id,
                owner_plate,
                other_plate,
                tick_interval_years,
            );
        }
    }

    apply_deltas(data, &deltas);
    clamp_terrain(data);
}

/// Clamps elevation and relief to physically plausible ranges (§5.7).
pub fn clamp_terrain(data: &mut WorldData) {
    for i in 0..data.elevation_mean.len() {
        data.elevation_mean[i] = data.elevation_mean[i].clamp(MIN_ELEVATION_M, MAX_ELEVATION_M);
        data.elevation_relief[i] = data.elevation_relief[i].clamp(0.0, MAX_RELIEF_M);
    }
}

/// Oceanic–oceanic: faster plate subducts; tie → lower `PlateId`.
pub fn subducting_plate_id(
    owner_plate_id: PlateId,
    other_plate_id: PlateId,
    owner_plate: &Plate,
    other_plate: &Plate,
) -> PlateId {
    let owner_rate = owner_plate.motion_rate_rad_per_year;
    let other_rate = other_plate.motion_rate_rad_per_year;
    if owner_rate > other_rate {
        owner_plate_id
    } else if other_rate > owner_rate {
        other_plate_id
    } else if owner_plate_id < other_plate_id {
        owner_plate_id
    } else {
        other_plate_id
    }
}

fn velocity_cm_per_year(edge: &ClassifiedEdge) -> f64 {
    edge.normal_velocity_m_per_year.abs() * 100.0
}

#[allow(clippy::too_many_arguments)]
fn apply_edge(
    data: &WorldData,
    deltas: &mut BTreeMap<HexId, HexDeltas>,
    owner_hex: HexId,
    edge: &ClassifiedEdge,
    owner_plate_id: PlateId,
    owner_plate: &Plate,
    other_plate: &Plate,
    tick_interval_years: f64,
) {
    let v_cm = velocity_cm_per_year(edge);

    match edge.class {
        BoundaryClass::Divergent => {
            apply_divergent(deltas, owner_hex, owner_plate, v_cm, tick_interval_years);
        }
        BoundaryClass::Convergent(ConvergentSubtype::ContinentalContinental) => {
            apply_continental_continental(
                data,
                deltas,
                owner_hex,
                owner_plate_id,
                owner_plate,
                v_cm,
                tick_interval_years,
            );
        }
        BoundaryClass::Convergent(ConvergentSubtype::ContinentalOceanic) => {
            apply_continental_oceanic(
                data,
                deltas,
                owner_hex,
                owner_plate_id,
                owner_plate,
                v_cm,
                tick_interval_years,
            );
        }
        BoundaryClass::Convergent(ConvergentSubtype::OceanicOceanic) => {
            apply_oceanic_oceanic(
                deltas,
                owner_hex,
                owner_plate_id,
                edge.other_plate,
                owner_plate,
                other_plate,
                v_cm,
                tick_interval_years,
            );
        }
        BoundaryClass::Transform => {
            let entry = deltas.entry(owner_hex).or_default();
            entry.bedrock = Some(BedrockType::Metamorphic);
        }
    }
}

fn apply_divergent(
    deltas: &mut BTreeMap<HexId, HexDeltas>,
    hex: HexId,
    owner_plate: &Plate,
    velocity_cm_per_year: f64,
    tick_interval_years: f64,
) {
    let mut delta = velocity_cm_per_year * tick_interval_years * SUBSIDENCE_RATE;
    let mut bedrock = BedrockType::OceanicCrust;

    if owner_plate.plate_type == PlateType::Continental {
        delta *= CONTINENTAL_RIFT_SUBSIDENCE_FACTOR;
        bedrock = BedrockType::Igneous;
    }

    let entry = deltas.entry(hex).or_default();
    entry.elev -= delta;
    entry.bedrock = Some(bedrock);
}

fn apply_continental_continental(
    data: &WorldData,
    deltas: &mut BTreeMap<HexId, HexDeltas>,
    owner_hex: HexId,
    owner_plate_id: PlateId,
    owner_plate: &Plate,
    velocity_cm_per_year: f64,
    tick_interval_years: f64,
) {
    if owner_plate.plate_type != PlateType::Continental {
        return;
    }

    let orogeny = velocity_cm_per_year * tick_interval_years * OROGENY_RATE;
    let relief = orogeny * 0.3;

    let entry = deltas.entry(owner_hex).or_default();
    entry.elev += orogeny;
    entry.relief += relief;
    entry.bedrock = Some(BedrockType::Metamorphic);

    spread_inland(
        data,
        deltas,
        owner_hex,
        owner_plate_id,
        CC_INLAND_HEXES,
        |d, falloff| {
            d.elev += orogeny * falloff;
            d.relief += relief * falloff;
        },
    );
}

fn apply_continental_oceanic(
    data: &WorldData,
    deltas: &mut BTreeMap<HexId, HexDeltas>,
    owner_hex: HexId,
    owner_plate_id: PlateId,
    owner_plate: &Plate,
    velocity_cm_per_year: f64,
    tick_interval_years: f64,
) {
    match owner_plate.plate_type {
        PlateType::Oceanic => {
            let trench = velocity_cm_per_year * tick_interval_years * SUBDUCTION_RATE;
            let entry = deltas.entry(owner_hex).or_default();
            entry.elev -= trench;
            entry.bedrock = Some(BedrockType::OceanicCrust);
        }
        PlateType::Continental => {
            let uplift = velocity_cm_per_year
                * tick_interval_years
                * OROGENY_RATE
                * OC_COASTAL_UPLIFT_FACTOR;
            let entry = deltas.entry(owner_hex).or_default();
            entry.elev += uplift;
            entry.bedrock = Some(BedrockType::Igneous);

            spread_inland(
                data,
                deltas,
                owner_hex,
                owner_plate_id,
                OC_INLAND_HEXES,
                |d, falloff| {
                    d.elev += uplift * falloff;
                },
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_oceanic_oceanic(
    deltas: &mut BTreeMap<HexId, HexDeltas>,
    owner_hex: HexId,
    owner_plate_id: PlateId,
    other_plate_id: PlateId,
    owner_plate: &Plate,
    other_plate: &Plate,
    velocity_cm_per_year: f64,
    tick_interval_years: f64,
) {
    let subducting = subducting_plate_id(owner_plate_id, other_plate_id, owner_plate, other_plate);
    let trench = velocity_cm_per_year * tick_interval_years * SUBDUCTION_RATE;

    if owner_plate_id == subducting {
        let entry = deltas.entry(owner_hex).or_default();
        entry.elev -= trench;
        entry.bedrock = Some(BedrockType::OceanicCrust);
    } else {
        let uplift = trench * OO_ARC_UPLIFT_FACTOR;
        let entry = deltas.entry(owner_hex).or_default();
        entry.elev += uplift;
        entry.bedrock = Some(BedrockType::Igneous);
    }
}

fn spread_inland(
    data: &WorldData,
    deltas: &mut BTreeMap<HexId, HexDeltas>,
    start: HexId,
    plate_id: PlateId,
    max_depth: u32,
    mut apply: impl FnMut(&mut HexDeltas, f64),
) {
    let grid = &data.grid;
    let n = data.plate_id.len();
    let mut visited = BTreeMap::<HexId, u32>::new();
    visited.insert(start, 0);

    let mut queue: VecDeque<HexId> = VecDeque::new();
    queue.push_back(start);

    while let Some(current) = queue.pop_front() {
        let depth = *visited.get(&current).unwrap_or(&0);
        if depth >= max_depth {
            continue;
        }

        let mut neighbors: Vec<HexId> = grid.neighbors(current).to_vec();
        neighbors.sort_by_key(|h| h.0);

        for neighbor in neighbors {
            if visited.contains_key(&neighbor) {
                continue;
            }
            let idx = neighbor.0 as usize;
            if idx >= n || data.plate_id[idx] != plate_id {
                continue;
            }
            let next_depth = depth + 1;
            if next_depth > max_depth {
                continue;
            }
            visited.insert(neighbor, next_depth);
            queue.push_back(neighbor);

            let falloff = INLAND_FALLOFF
                .get(next_depth as usize - 1)
                .copied()
                .unwrap_or(0.0);
            if falloff > 0.0 {
                let entry = deltas.entry(neighbor).or_default();
                apply(entry, falloff);
            }
        }
    }
}

fn apply_deltas(data: &mut WorldData, deltas: &BTreeMap<HexId, HexDeltas>) {
    for (&hex, delta) in deltas {
        let i = hex.0 as usize;
        if i >= data.elevation_mean.len() {
            continue;
        }
        data.elevation_mean[i] += delta.elev as f32;
        data.elevation_relief[i] += delta.relief as f32;
        if let Some(bedrock) = delta.bedrock {
            data.bedrock_type[i] = bedrock;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::time::WorldYear;
    use genesis_core::{HexGrid, PlateId};

    use crate::boundary::{BoundaryClass, BoundaryInfo, ClassifiedEdge, ConvergentSubtype};
    use crate::plate::{Plate, PlateClass, PlateRegistry, PlateType};

    const EARTH_RADIUS_KM: f64 = 6371.0;

    fn plate_at(id: u16, plate_type: PlateType, seed: u32, rate: f64) -> Plate {
        Plate {
            id: PlateId(id),
            plate_type,
            plate_class: PlateClass::Major,
            seed_hex: HexId(seed),
            motion_axis: [0.0, 0.0, 1.0],
            motion_rate_rad_per_year: rate,
            age_year: WorldYear::FORMATION,
            target_fraction: 0.5,
            accumulated_rotation_rad: 0.0,
        }
    }

    fn world_with_plates(level: u8) -> (WorldData, PlateRegistry) {
        let grid = HexGrid::new(level, EARTH_RADIUS_KM).expect("grid");
        let params = WorldParameters::default();
        let data = WorldData::new(grid, params);
        let mut registry = PlateRegistry::new();
        registry.insert(plate_at(0, PlateType::Oceanic, 0, 1e-8));
        registry.insert(plate_at(1, PlateType::Oceanic, 500, 5e-9));
        (data, registry)
    }

    fn set_half_half(data: &mut WorldData) {
        let n = data.plate_id.len();
        for (i, pid) in data.plate_id.iter_mut().enumerate() {
            *pid = if i < n / 2 { PlateId(0) } else { PlateId(1) };
        }
        for elev in &mut data.elevation_mean {
            *elev = 0.0;
        }
    }

    #[test]
    fn divergent_edge_lowers_elevation() {
        let (mut data, registry) = world_with_plates(4);
        set_half_half(&mut data);
        let hex = HexId(10);
        let mut boundaries = BoundaryInfo::default();
        boundaries.boundary_hexes.push(hex);
        boundaries.edges.insert(
            hex,
            vec![ClassifiedEdge {
                neighbor_hex: HexId(11),
                other_plate: PlateId(1),
                class: BoundaryClass::Divergent,
                normal_velocity_m_per_year: -0.05,
                tangential_velocity_m_per_year: 0.0,
            }],
        );

        apply_boundary_elevation(&mut data, &registry, &boundaries, 500_000.0);
        assert!(data.elevation_mean[hex.0 as usize] < 0.0);
        assert_eq!(data.bedrock_type[hex.0 as usize], BedrockType::OceanicCrust);
    }

    #[test]
    fn convergent_cc_raises_elevation() {
        let (mut data, mut registry) = world_with_plates(4);
        registry.insert(plate_at(2, PlateType::Continental, 200, 1e-8));
        registry.insert(plate_at(3, PlateType::Continental, 800, 1e-8));
        let n = data.plate_id.len();
        for (i, pid) in data.plate_id.iter_mut().enumerate() {
            *pid = if i < n / 2 { PlateId(2) } else { PlateId(3) };
        }
        for elev in &mut data.elevation_mean {
            *elev = 100.0;
        }

        let hex = HexId(10);
        let mut boundaries = BoundaryInfo::default();
        boundaries.boundary_hexes.push(hex);
        boundaries.edges.insert(
            hex,
            vec![ClassifiedEdge {
                neighbor_hex: HexId(11),
                other_plate: PlateId(3),
                class: BoundaryClass::Convergent(ConvergentSubtype::ContinentalContinental),
                normal_velocity_m_per_year: 0.05,
                tangential_velocity_m_per_year: 0.0,
            }],
        );

        apply_boundary_elevation(&mut data, &registry, &boundaries, 500_000.0);
        assert!(data.elevation_mean[hex.0 as usize] > 100.0);
        assert_eq!(data.bedrock_type[hex.0 as usize], BedrockType::Metamorphic);
    }

    #[test]
    fn convergent_oc_trench_on_oceanic_owner() {
        let (mut data, mut registry) = world_with_plates(4);
        registry.insert(plate_at(2, PlateType::Oceanic, 100, 1e-8));
        registry.insert(plate_at(3, PlateType::Continental, 900, 1e-8));

        let hex = HexId(50);
        data.plate_id[hex.0 as usize] = PlateId(2);
        let neighbor = data.grid.neighbors(hex)[0];
        data.plate_id[neighbor.0 as usize] = PlateId(3);
        data.elevation_mean[hex.0 as usize] = 0.0;

        let mut boundaries = BoundaryInfo::default();
        boundaries.boundary_hexes.push(hex);
        boundaries.edges.insert(
            hex,
            vec![ClassifiedEdge {
                neighbor_hex: neighbor,
                other_plate: PlateId(3),
                class: BoundaryClass::Convergent(ConvergentSubtype::ContinentalOceanic),
                normal_velocity_m_per_year: 0.05,
                tangential_velocity_m_per_year: 0.0,
            }],
        );

        apply_boundary_elevation(&mut data, &registry, &boundaries, 500_000.0);
        assert!(data.elevation_mean[hex.0 as usize] < 0.0);
    }

    #[test]
    fn clamping_caps_extreme_elevation() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let params = WorldParameters::default();
        let mut data = WorldData::new(grid, params);
        data.elevation_mean[0] = 50_000.0;

        let mut registry = PlateRegistry::new();
        registry.insert(plate_at(0, PlateType::Continental, 0, 1e-8));
        registry.insert(plate_at(1, PlateType::Continental, 500, 1e-8));
        data.plate_id[0] = PlateId(0);
        data.plate_id[1] = PlateId(1);

        let hex = HexId(0);
        let mut boundaries = BoundaryInfo::default();
        boundaries.boundary_hexes.push(hex);
        boundaries.edges.insert(
            hex,
            vec![ClassifiedEdge {
                neighbor_hex: HexId(1),
                other_plate: PlateId(1),
                class: BoundaryClass::Convergent(ConvergentSubtype::ContinentalContinental),
                normal_velocity_m_per_year: 1.0,
                tangential_velocity_m_per_year: 0.0,
            }],
        );

        apply_boundary_elevation(&mut data, &registry, &boundaries, 500_000.0);
        assert!(data.elevation_mean[0] <= MAX_ELEVATION_M);
    }

    #[test]
    fn triple_junction_accumulates_without_panic() {
        let (mut data, mut registry) = world_with_plates(4);
        registry.insert(plate_at(2, PlateType::Oceanic, 200, 1e-8));

        let hex = HexId(100);
        let neighbors: Vec<HexId> = data.grid.neighbors(hex).to_vec();
        data.plate_id[hex.0 as usize] = PlateId(0);
        data.plate_id[neighbors[0].0 as usize] = PlateId(1);
        data.plate_id[neighbors[1].0 as usize] = PlateId(2);

        let mut boundaries = BoundaryInfo::default();
        boundaries.boundary_hexes.push(hex);
        boundaries.edges.insert(
            hex,
            vec![
                ClassifiedEdge {
                    neighbor_hex: neighbors[0],
                    other_plate: PlateId(1),
                    class: BoundaryClass::Divergent,
                    normal_velocity_m_per_year: -0.05,
                    tangential_velocity_m_per_year: 0.0,
                },
                ClassifiedEdge {
                    neighbor_hex: neighbors[1],
                    other_plate: PlateId(2),
                    class: BoundaryClass::Transform,
                    normal_velocity_m_per_year: 0.0,
                    tangential_velocity_m_per_year: 0.1,
                },
            ],
        );

        apply_boundary_elevation(&mut data, &registry, &boundaries, 500_000.0);
        assert_eq!(data.bedrock_type[hex.0 as usize], BedrockType::Metamorphic);
    }

    #[test]
    fn subducting_plate_faster_rate() {
        let fast = plate_at(0, PlateType::Oceanic, 0, 2e-8);
        let slow = plate_at(1, PlateType::Oceanic, 1, 1e-8);
        assert_eq!(
            subducting_plate_id(PlateId(0), PlateId(1), &fast, &slow),
            PlateId(0)
        );
    }

    #[test]
    fn subducting_plate_tie_breaks_lower_id() {
        let a = plate_at(0, PlateType::Oceanic, 0, 1e-8);
        let b = plate_at(1, PlateType::Oceanic, 1, 1e-8);
        assert_eq!(
            subducting_plate_id(PlateId(0), PlateId(1), &a, &b),
            PlateId(0)
        );
    }
}
