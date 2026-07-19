//! Boundary detection and classification (Doc 06 §3).

use std::collections::{BTreeMap, BTreeSet};

use genesis_core::data::WorldData;
use genesis_core::{HexGrid, HexId, PlateId};
use glam::DVec3;

use crate::motion::surface_velocity_m_per_year;
use crate::plate::{Plate, PlateRegistry, PlateType};
use crate::projection::ProjectionCache;

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

/// Whether the crust AT this hex is oceanic. Plates carry mixed crust (a
/// continental plate accretes oceanic floor at rifts, like the South American
/// Plate), so boundary behavior keys on the per-hex crust flag, not the owning
/// plate's type.
pub fn hex_crust_is_oceanic(
    data: &WorldData,
    registry: &PlateRegistry,
    cache: &ProjectionCache,
    hex: HexId,
) -> bool {
    !crate::plate_surface::continental_crust_at(data, registry, cache, hex)
}

/// Maps two per-hex crust kinds to a convergent subtype (order-independent).
fn convergent_subtype_from_crust(owner_oceanic: bool, other_oceanic: bool) -> ConvergentSubtype {
    match (owner_oceanic, other_oceanic) {
        (false, false) => ConvergentSubtype::ContinentalContinental,
        (true, true) => ConvergentSubtype::OceanicOceanic,
        _ => ConvergentSubtype::ContinentalOceanic,
    }
}

/// Scans all hexes and classifies cross-plate neighbor edges.
///
/// Hexes are visited in ascending [`HexId`] order; edges per hex are sorted by
/// `neighbor_hex`. Directed owner-centric edges `h → n` are stored once per hex.
pub fn detect_and_classify_boundaries(
    data: &WorldData,
    registry: &PlateRegistry,
    cache: &ProjectionCache,
) -> BoundaryInfo {
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

        let neighbor_ids = grid.neighbors_sorted(hex);

        let mut edges = Vec::new();
        let mut contacts = BTreeSet::new();

        for &neighbor_hex in neighbor_ids {
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

            let edge = classify_edge(
                grid,
                hex,
                neighbor_hex,
                owner,
                other,
                planet_radius_km,
                hex_crust_is_oceanic(data, registry, cache, hex),
                hex_crust_is_oceanic(data, registry, cache, neighbor_hex),
            );
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

/// Per-plate boundary-force tallies driving slab-pull motion (§2.2).
///
/// Counts are of directed owner-centric edges from [`BoundaryInfo`]; each
/// physical plate contact appears once per side, which is exactly what the
/// force fractions need: a continental plate's side of a continent–ocean
/// margin is NOT slab pull even though the oceanic side of the same contact
/// is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoundaryTally {
    /// Live convergent edges where this plate's oceanic crust is the
    /// downgoing slab. Slab pull is the dominant plate-driving force on
    /// Earth (~90%); plates with long subducting rims run 6–15 cm/yr.
    pub slab_edges: u32,
    /// Live divergent edges (ridge push, a minor driving force).
    pub ridge_edges: u32,
    /// All classified edges; normalizer for the force fractions.
    pub total_edges: u32,
}

/// Tallies each plate's slab and ridge edges from the latest boundary scan.
///
/// An edge only exerts force while it is genuinely active: convergent edges
/// must close faster than [`crate::partition::CONVERGENCE_THRESHOLD_M_PER_YEAR`]
/// (a stalled suture is not a pulling slab) and divergent edges must open at
/// the same pace. Which side subducts follows the elevation model: at
/// continent–ocean margins the oceanic-crust side sinks; at ocean–ocean
/// margins the faster plate does ([`crate::elevation::subducting_plate_id`]);
/// continent–continent collisions have no slab.
pub fn plate_boundary_tallies(
    data: &WorldData,
    registry: &PlateRegistry,
    cache: &ProjectionCache,
    info: &BoundaryInfo,
) -> BTreeMap<PlateId, BoundaryTally> {
    let mut tallies: BTreeMap<PlateId, BoundaryTally> = BTreeMap::new();
    for (&hex, edges) in &info.edges {
        let owner_plate_id = data.plate_id[hex.0 as usize];
        if owner_plate_id == PlateId::NONE {
            continue;
        }
        let owner_oceanic = hex_crust_is_oceanic(data, registry, cache, hex);
        let tally = tallies.entry(owner_plate_id).or_default();
        for edge in edges {
            tally.total_edges += 1;
            match edge.class {
                BoundaryClass::Divergent => {
                    if edge.normal_velocity_m_per_year
                        < -crate::partition::CONVERGENCE_THRESHOLD_M_PER_YEAR
                    {
                        tally.ridge_edges += 1;
                    }
                }
                BoundaryClass::Convergent(subtype) => {
                    if edge.normal_velocity_m_per_year
                        <= crate::partition::CONVERGENCE_THRESHOLD_M_PER_YEAR
                    {
                        continue;
                    }
                    let subducts = match subtype {
                        ConvergentSubtype::ContinentalContinental => false,
                        ConvergentSubtype::ContinentalOceanic => owner_oceanic,
                        ConvergentSubtype::OceanicOceanic => {
                            let (Some(owner), Some(other)) =
                                (registry.get(owner_plate_id), registry.get(edge.other_plate))
                            else {
                                continue;
                            };
                            crate::elevation::subducting_plate_id(
                                owner_plate_id,
                                edge.other_plate,
                                owner,
                                other,
                            ) == owner_plate_id
                        }
                    };
                    if subducts {
                        tally.slab_edges += 1;
                    }
                }
                BoundaryClass::Transform => {}
            }
        }
    }
    tallies
}

#[allow(clippy::too_many_arguments)]
fn classify_edge(
    grid: &HexGrid,
    owner_hex: HexId,
    neighbor_hex: HexId,
    owner: &Plate,
    other: &Plate,
    planet_radius_km: f64,
    owner_crust_oceanic: bool,
    other_crust_oceanic: bool,
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

    let class = classify_from_velocities(
        normal_velocity,
        tangential_velocity,
        owner_crust_oceanic,
        other_crust_oceanic,
    );

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
    owner_crust_oceanic: bool,
    other_crust_oceanic: bool,
) -> BoundaryClass {
    if normal_velocity.abs() < 0.3 * tangential_velocity.abs() {
        BoundaryClass::Transform
    } else if normal_velocity < 0.0 {
        BoundaryClass::Divergent
    } else {
        BoundaryClass::Convergent(convergent_subtype_from_crust(
            owner_crust_oceanic,
            other_crust_oceanic,
        ))
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
            forward_world_hint: Vec::new(),
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

        let edge = classify_edge(
            &grid,
            hex,
            neighbor,
            &owner,
            &other,
            EARTH_RADIUS_KM,
            true,
            true,
        );
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
        // Continental crust on both sides.
        let class = classify_from_velocities(1.0, 0.1, false, false);
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

        let edge = classify_edge(
            &grid,
            hex,
            neighbor,
            &owner,
            &other,
            EARTH_RADIUS_KM,
            true,
            true,
        );
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
        let class = classify_from_velocities(0.2, 1.0, true, true);
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

        let info = detect_and_classify_boundaries(&data, &registry, &ProjectionCache::empty());
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

        let info = detect_and_classify_boundaries(&data, &registry, &ProjectionCache::empty());
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

        let info = detect_and_classify_boundaries(&data, &registry, &ProjectionCache::empty());
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

    /// Two-hex world for tally tests: hex 10 on plate 0, hex 11 on plate 1.
    /// Crust comes from the bedrock/elevation fallback (empty surfaces).
    fn tally_fixture(
        oceanic_owner: bool,
        oceanic_other: bool,
    ) -> (WorldData, PlateRegistry, ProjectionCache) {
        use genesis_core::data::BedrockType;

        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let params = WorldParameters::default();
        let mut data = WorldData::new(grid, params);

        let mut registry = PlateRegistry::new();
        registry.insert(plate_with_motion(
            0,
            PlateType::Oceanic,
            0,
            [0.0, 0.0, 1.0],
            2e-8,
        ));
        registry.insert(plate_with_motion(
            1,
            PlateType::Oceanic,
            100,
            [0.0, 0.0, 1.0],
            1e-8,
        ));

        data.plate_id[10] = PlateId(0);
        data.plate_id[11] = PlateId(1);
        for (idx, oceanic) in [(10usize, oceanic_owner), (11usize, oceanic_other)] {
            if oceanic {
                data.bedrock_type[idx] = BedrockType::OceanicCrust;
                data.elevation_mean[idx] = -4000.0;
            } else {
                data.bedrock_type[idx] = BedrockType::Igneous;
                data.elevation_mean[idx] = 500.0;
            }
        }
        (data, registry, ProjectionCache::empty())
    }

    fn tally_info(edges: &[(u32, ClassifiedEdge)]) -> BoundaryInfo {
        let mut info = BoundaryInfo::default();
        for &(hex, ref edge) in edges {
            info.edges.entry(HexId(hex)).or_default().push(edge.clone());
        }
        info
    }

    fn edge_to(
        neighbor: u32,
        other_plate: u16,
        class: BoundaryClass,
        v_n: f64,
    ) -> (u32, ClassifiedEdge) {
        (
            0, // placeholder, replaced by caller key
            ClassifiedEdge {
                neighbor_hex: HexId(neighbor),
                other_plate: PlateId(other_plate),
                class,
                normal_velocity_m_per_year: v_n,
                tangential_velocity_m_per_year: 0.0,
            },
        )
    }

    #[test]
    fn tally_continent_ocean_margin_pulls_only_oceanic_side() {
        let (data, registry, cache) = tally_fixture(true, false);
        let (_, co_edge) = edge_to(
            11,
            1,
            BoundaryClass::Convergent(ConvergentSubtype::ContinentalOceanic),
            0.02,
        );
        let (_, co_edge_back) = edge_to(
            10,
            0,
            BoundaryClass::Convergent(ConvergentSubtype::ContinentalOceanic),
            0.02,
        );
        let info = tally_info(&[(10, co_edge), (11, co_edge_back)]);

        let tallies = plate_boundary_tallies(&data, &registry, &cache, &info);
        let oceanic = tallies[&PlateId(0)];
        let continental = tallies[&PlateId(1)];
        assert_eq!(oceanic.slab_edges, 1, "oceanic side subducts: {oceanic:?}");
        assert_eq!(
            continental.slab_edges, 0,
            "overriding side has no slab: {continental:?}"
        );
        assert_eq!(oceanic.total_edges, 1);
        assert_eq!(continental.total_edges, 1);
    }

    #[test]
    fn tally_ocean_ocean_margin_pulls_faster_plate() {
        let (data, registry, cache) = tally_fixture(true, true);
        let (_, oo_edge) = edge_to(
            11,
            1,
            BoundaryClass::Convergent(ConvergentSubtype::OceanicOceanic),
            0.02,
        );
        let (_, oo_edge_back) = edge_to(
            10,
            0,
            BoundaryClass::Convergent(ConvergentSubtype::OceanicOceanic),
            0.02,
        );
        let info = tally_info(&[(10, oo_edge), (11, oo_edge_back)]);

        let tallies = plate_boundary_tallies(&data, &registry, &cache, &info);
        assert_eq!(tallies[&PlateId(0)].slab_edges, 1, "faster plate subducts");
        assert_eq!(tallies[&PlateId(1)].slab_edges, 0);
    }

    #[test]
    fn tally_stalled_convergence_has_no_slab() {
        let (data, registry, cache) = tally_fixture(true, false);
        let (_, slow_edge) = edge_to(
            11,
            1,
            BoundaryClass::Convergent(ConvergentSubtype::ContinentalOceanic),
            0.001,
        );
        let info = tally_info(&[(10, slow_edge)]);

        let tallies = plate_boundary_tallies(&data, &registry, &cache, &info);
        let tally = tallies[&PlateId(0)];
        assert_eq!(tally.slab_edges, 0, "stalled margin is not a pulling slab");
        assert_eq!(tally.total_edges, 1);
    }

    #[test]
    fn tally_continental_collision_has_no_slab() {
        let (data, registry, cache) = tally_fixture(false, false);
        let (_, cc_edge) = edge_to(
            11,
            1,
            BoundaryClass::Convergent(ConvergentSubtype::ContinentalContinental),
            0.02,
        );
        let info = tally_info(&[(10, cc_edge)]);

        let tallies = plate_boundary_tallies(&data, &registry, &cache, &info);
        assert_eq!(tallies[&PlateId(0)].slab_edges, 0, "sutures have no slab");
    }

    #[test]
    fn tally_live_ridge_counts_and_stalled_ridge_does_not() {
        let (data, registry, cache) = tally_fixture(true, true);
        let (_, ridge) = edge_to(11, 1, BoundaryClass::Divergent, -0.02);
        let (_, stalled_ridge) = edge_to(11, 1, BoundaryClass::Divergent, -0.001);
        let info = tally_info(&[(10, ridge), (11, stalled_ridge)]);

        let tallies = plate_boundary_tallies(&data, &registry, &cache, &info);
        assert_eq!(tallies[&PlateId(0)].ridge_edges, 1);
        assert_eq!(
            tallies[&PlateId(1)].ridge_edges,
            0,
            "below-threshold opening is not ridge push"
        );
    }
}
