//! Boundary detection and classification (Doc 06 §3).

use std::collections::{BTreeMap, BTreeSet};

use genesis_core::data::WorldData;
use genesis_core::{HexGrid, HexId, PlateId};
use glam::DVec3;

use crate::motion::surface_velocity_m_per_year;
use crate::plate::{Plate, PlateRegistry, PlateType};

/// High-level boundary classification between two plates at a hex edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundaryClass {
    Divergent,
    Convergent(ConvergentSubtype),
    Transform,
}

/// Convergent boundary subtype from plate types (Doc 06 §3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConvergentSubtype {
    ContinentalContinental,
    OceanicOceanic,
    ContinentalOceanic,
}

/// One classified cross-plate edge from owner hex `h` toward neighbor `n`.
#[derive(Clone, Debug, PartialEq)]
pub struct ClassifiedEdge {
    pub neighbor_hex: HexId,
    pub other_plate: PlateId,
    pub class: BoundaryClass,
    pub normal_velocity_m_per_year: f64,
    pub tangential_velocity_m_per_year: f64,
}

/// Derived boundary state recomputed each Geological tick (Doc 06 §3.1).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoundaryInfo {
    pub boundary_hexes: Vec<HexId>,
    pub plate_contacts: BTreeMap<HexId, BTreeSet<PlateId>>,
    pub edges: BTreeMap<HexId, Vec<ClassifiedEdge>>,
}

/// Maps two plate types to a convergent subtype (order-independent).
pub fn convergent_subtype(a: PlateType, b: PlateType) -> ConvergentSubtype {
    match (a, b) {
        (PlateType::Continental, PlateType::Continental) => {
            ConvergentSubtype::ContinentalContinental
        }
        (PlateType::Oceanic, PlateType::Oceanic) => ConvergentSubtype::OceanicOceanic,
        _ => ConvergentSubtype::ContinentalOceanic,
    }
}

/// Scans all hexes and classifies cross-plate neighbor edges.
///
/// Hexes are visited in ascending [`HexId`] order; edges per hex are sorted by
/// `neighbor_hex`. Directed owner-centric edges `h → n` are stored once per hex.
pub fn detect_and_classify_boundaries(data: &WorldData, registry: &PlateRegistry) -> BoundaryInfo {
    let grid = &data.grid;
    let planet_radius_km = data.parameters.core.planet.radius_km;
    let n = data.plate_id.len();
    let mut info = BoundaryInfo::default();

    for i in 0..n {
        let hex = HexId(i as u32);
        let owner_plate = data.plate_id[i];
        if owner_plate == PlateId::NONE {
            continue;
        }
        let owner = match registry.get(owner_plate) {
            Some(p) => p,
            None => continue,
        };

        let mut neighbor_ids: Vec<HexId> = grid.neighbors(hex).to_vec();
        neighbor_ids.sort_by_key(|id| id.0);

        let mut edges = Vec::new();
        let mut contacts = BTreeSet::new();

        for neighbor_hex in neighbor_ids {
            let neighbor_idx = neighbor_hex.0 as usize;
            if neighbor_idx >= n {
                continue;
            }
            let other_plate = data.plate_id[neighbor_idx];
            if other_plate == PlateId::NONE || other_plate == owner_plate {
                continue;
            }
            let other = match registry.get(other_plate) {
                Some(p) => p,
                None => continue,
            };

            let edge = classify_edge(grid, hex, neighbor_hex, owner, other, planet_radius_km);
            contacts.insert(other_plate);
            edges.push(edge);
        }

        if edges.is_empty() {
            continue;
        }

        info.boundary_hexes.push(hex);
        info.plate_contacts.insert(hex, contacts);
        info.edges.insert(hex, edges);
    }

    info
}

fn classify_edge(
    grid: &HexGrid,
    owner_hex: HexId,
    neighbor_hex: HexId,
    owner: &Plate,
    other: &Plate,
    planet_radius_km: f64,
) -> ClassifiedEdge {
    let p_h = grid.cell_center_direction(owner_hex);
    let p_n = grid.cell_center_direction(neighbor_hex);

    let v_owner = surface_velocity_m_per_year(
        p_h,
        owner.motion_axis,
        owner.motion_rate_rad_per_year,
        planet_radius_km,
    );
    let v_other = surface_velocity_m_per_year(
        p_h,
        other.motion_axis,
        other.motion_rate_rad_per_year,
        planet_radius_km,
    );
    let v_rel = v_owner - v_other;

    let (n_hat, t_hat) = edge_frame(p_h, p_n);
    let normal_velocity = v_rel.dot(n_hat);
    let tangential_velocity = v_rel.dot(t_hat);

    let class = classify_from_velocities(normal_velocity, tangential_velocity, owner, other);

    ClassifiedEdge {
        neighbor_hex,
        other_plate: other.id,
        class,
        normal_velocity_m_per_year: normal_velocity,
        tangential_velocity_m_per_year: tangential_velocity,
    }
}

/// Local boundary frame at hex `h` toward neighbor `n`.
/// Normal points from owner toward neighbor; tangent is along the boundary trace.
fn edge_frame(p_h: [f64; 3], p_n: [f64; 3]) -> (DVec3, DVec3) {
    let p = DVec3::new(p_h[0], p_h[1], p_h[2]);
    let n = DVec3::new(p_n[0], p_n[1], p_n[2]);
    let toward = n - p * n.dot(p);
    let n_hat = toward.normalize_or_zero();
    let t_hat = p.cross(n_hat).normalize_or_zero();
    (n_hat, t_hat)
}

fn classify_from_velocities(
    normal_velocity: f64,
    tangential_velocity: f64,
    owner: &Plate,
    other: &Plate,
) -> BoundaryClass {
    if normal_velocity.abs() < 0.3 * tangential_velocity.abs() {
        BoundaryClass::Transform
    } else if normal_velocity < 0.0 {
        BoundaryClass::Divergent
    } else {
        BoundaryClass::Convergent(convergent_subtype(owner.plate_type, other.plate_type))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::time::WorldYear;
    use genesis_core::{HexGrid, PlateId};

    use crate::plate::{Plate, PlateClass, PlateRegistry};

    const EARTH_RADIUS_KM: f64 = 6371.0;

    #[test]
    fn convergent_subtype_continental_continental() {
        assert_eq!(
            convergent_subtype(PlateType::Continental, PlateType::Continental),
            ConvergentSubtype::ContinentalContinental
        );
    }

    #[test]
    fn convergent_subtype_oceanic_oceanic() {
        assert_eq!(
            convergent_subtype(PlateType::Oceanic, PlateType::Oceanic),
            ConvergentSubtype::OceanicOceanic
        );
    }

    #[test]
    fn convergent_subtype_continental_oceanic() {
        assert_eq!(
            convergent_subtype(PlateType::Continental, PlateType::Oceanic),
            ConvergentSubtype::ContinentalOceanic
        );
        assert_eq!(
            convergent_subtype(PlateType::Oceanic, PlateType::Continental),
            ConvergentSubtype::ContinentalOceanic
        );
    }

    fn plate_with_motion(
        id: u16,
        plate_type: PlateType,
        seed: u32,
        axis: [f64; 3],
        rate: f64,
    ) -> Plate {
        Plate {
            id: PlateId(id),
            plate_type,
            plate_class: PlateClass::Major,
            seed_hex: HexId(seed),
            motion_axis: axis,
            motion_rate_rad_per_year: rate,
            age_year: WorldYear::FORMATION,
            target_fraction: 0.5,
            accumulated_rotation_rad: 0.0,
            last_nonempty_year: WorldYear::FORMATION,
            surface: crate::plate_surface::PlateSurface::new(10_000),
        }
    }

    #[test]
    fn divergent_when_normal_velocity_negative() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let hex = HexId(10);
        let neighbor = grid.neighbors(hex)[0];

        // Plate A rotates about x-axis; plate B stationary.
        let owner = plate_with_motion(0, PlateType::Oceanic, 0, [1.0, 0.0, 0.0], 1e-6);
        let other = plate_with_motion(1, PlateType::Oceanic, 100, [0.0, 0.0, 1.0], 0.0);

        let edge = classify_edge(&grid, hex, neighbor, &owner, &other, EARTH_RADIUS_KM);
        assert!(
            matches!(edge.class, BoundaryClass::Divergent),
            "expected divergent (negative normal), got {:?} v_n={}",
            edge.class,
            edge.normal_velocity_m_per_year
        );
        assert!(
            edge.normal_velocity_m_per_year < 0.0,
            "divergent requires negative normal velocity"
        );
    }

    #[test]
    fn convergent_when_normal_velocity_positive() {
        let owner = plate_with_motion(0, PlateType::Continental, 0, [0.0, 0.0, 1.0], 1e-6);
        let other = plate_with_motion(1, PlateType::Continental, 1, [0.0, 0.0, 1.0], 0.0);
        let class = classify_from_velocities(1.0, 0.1, &owner, &other);
        assert!(matches!(
            class,
            BoundaryClass::Convergent(ConvergentSubtype::ContinentalContinental)
        ));
    }

    #[test]
    fn transform_when_tangential_dominates() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let hex = HexId(50);
        let neighbor = grid.neighbors(hex)[0];
        let p_h = grid.cell_center_direction(hex);
        let p_n = grid.cell_center_direction(neighbor);
        let (n_hat, _t_hat) = edge_frame(p_h, p_n);
        let axis = [n_hat.x, n_hat.y, n_hat.z];

        // Both plates rotate about the edge normal → slip mostly tangential (§3.2).
        let owner = plate_with_motion(0, PlateType::Oceanic, 0, axis, 1e-6);
        let other = plate_with_motion(1, PlateType::Oceanic, 100, axis, 5e-7);

        let edge = classify_edge(&grid, hex, neighbor, &owner, &other, EARTH_RADIUS_KM);
        assert!(
            matches!(edge.class, BoundaryClass::Transform),
            "expected transform, got {:?} v_n={} v_t={}",
            edge.class,
            edge.normal_velocity_m_per_year,
            edge.tangential_velocity_m_per_year
        );
        assert!(
            edge.normal_velocity_m_per_year.abs() < 0.3 * edge.tangential_velocity_m_per_year.abs()
        );
    }

    #[test]
    fn classify_transform_at_threshold() {
        let owner = plate_with_motion(0, PlateType::Oceanic, 0, [0.0, 0.0, 1.0], 1e-6);
        let other = plate_with_motion(1, PlateType::Oceanic, 1, [0.0, 0.0, 1.0], 1e-6);
        let class = classify_from_velocities(0.2, 1.0, &owner, &other);
        assert!(matches!(class, BoundaryClass::Transform));
    }

    #[test]
    fn two_plate_manual_assignment_has_boundary_hexes() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let n = grid.cell_count() as usize;
        let params = WorldParameters::default();
        let mut data = WorldData::new(grid, params);

        let mut registry = PlateRegistry::new();
        registry.insert(plate_with_motion(
            0,
            PlateType::Oceanic,
            0,
            [0.0, 0.0, 1.0],
            1e-8,
        ));
        registry.insert(plate_with_motion(
            1,
            PlateType::Oceanic,
            500,
            [1.0, 0.0, 0.0],
            1e-8,
        ));

        let mid = n / 2;
        for (i, pid) in data.plate_id.iter_mut().enumerate() {
            *pid = if i < mid { PlateId(0) } else { PlateId(1) };
        }

        let info = detect_and_classify_boundaries(&data, &registry);
        assert!(
            !info.boundary_hexes.is_empty(),
            "expected boundary hexes along plate contact"
        );
        for &hex in &info.boundary_hexes {
            assert!(info.plate_contacts.contains_key(&hex));
            assert!(info.edges.contains_key(&hex));
        }
    }

    #[test]
    fn triple_junction_has_multiple_plate_contacts() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let params = WorldParameters::default();
        let mut data = WorldData::new(grid, params);

        let mut registry = PlateRegistry::new();
        for (id, seed) in [(0u16, 0u32), (1, 200), (2, 400)] {
            registry.insert(plate_with_motion(
                id,
                PlateType::Oceanic,
                seed,
                [0.0, 0.0, 1.0],
                1e-8,
            ));
        }

        let junction = HexId(100);
        let neighbors: Vec<HexId> = data.grid.neighbors(junction).to_vec();
        assert!(
            neighbors.len() >= 3,
            "need at least 3 neighbors for triple junction fixture"
        );

        data.plate_id[junction.0 as usize] = PlateId(0);
        data.plate_id[neighbors[0].0 as usize] = PlateId(1);
        data.plate_id[neighbors[1].0 as usize] = PlateId(2);
        if neighbors.len() > 2 {
            data.plate_id[neighbors[2].0 as usize] = PlateId(1);
        }

        let info = detect_and_classify_boundaries(&data, &registry);
        let contacts = info
            .plate_contacts
            .get(&junction)
            .expect("junction hex should be a boundary");
        assert!(
            contacts.len() >= 2,
            "triple junction should contact at least 2 foreign plates, got {contacts:?}"
        );
    }

    #[test]
    fn plate_contacts_match_neighbor_scan() {
        let grid = HexGrid::new(5, EARTH_RADIUS_KM).expect("grid");
        let params = WorldParameters::default();
        let mut data = WorldData::new(grid, params);

        let mut registry = PlateRegistry::new();
        registry.insert(plate_with_motion(
            0,
            PlateType::Oceanic,
            0,
            [0.0, 0.0, 1.0],
            1e-8,
        ));
        registry.insert(plate_with_motion(
            1,
            PlateType::Oceanic,
            800,
            [1.0, 0.0, 0.0],
            1e-8,
        ));

        let n = data.plate_id.len();
        for (i, pid) in data.plate_id.iter_mut().enumerate() {
            *pid = if i < n / 2 { PlateId(0) } else { PlateId(1) };
        }

        let info = detect_and_classify_boundaries(&data, &registry);
        for &hex in &info.boundary_hexes {
            let owner = data.plate_id[hex.0 as usize];
            let mut expected: BTreeSet<PlateId> = BTreeSet::new();
            for &neighbor in data.grid.neighbors(hex) {
                let other = data.plate_id[neighbor.0 as usize];
                if other != PlateId::NONE && other != owner {
                    expected.insert(other);
                }
            }
            assert_eq!(info.plate_contacts.get(&hex), Some(&expected));
        }
    }
}
