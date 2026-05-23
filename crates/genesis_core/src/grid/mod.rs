//! Hex grid infrastructure for Genesis Engine.
//!
//! `isea3h` provides pure coordinate math. This module exposes the public [`HexGrid`] API
//! with dense [`HexId`] values, precomputed neighbors, and geographic queries.

mod error;
mod geography;
mod ids;
mod neighbors;

pub mod isea3h;

pub use error::GridError;
pub use ids::{Direction, HexId};

use std::collections::BTreeMap;

use isea3h::{Isea3hCoord, Vec3};

/// The complete hex grid at a specific subdivision level for a planet of a given radius.
///
/// # Invariants
///
/// - [`HexId`] assignment: `HexId(0..12)` are the twelve [`Isea3hCoord::Pentagon`] cells in
///   ascending vertex order (`vertex` 0..11). `HexId(12..N-1)` are the remaining cells in
///   canonical [`isea3h::all_cells`] order (pentagons first, then edges, then interiors;
///   lexicographic within each variant).
/// - [`HexGrid::is_pentagon`] is `hex.0 < 12`.
/// - Neighbor slices have length 5 for pentagons and 6 for hexes.
pub struct HexGrid {
    subdivision_level: u8,
    planet_radius_km: f64,
    cell_count: u32,
    uniform_hex_area_km2: f64,
    #[allow(dead_code)] // used in step 3+ for coordinate ↔ id resolution
    coords: Vec<Isea3hCoord>,
    #[allow(dead_code)]
    coord_to_id: BTreeMap<Isea3hCoord, HexId>,
    centers: Vec<Vec3>,
    neighbors: Vec<Vec<HexId>>,
}

impl HexGrid {
    /// Constructs the grid at the given subdivision level for a planet of the given radius.
    /// Pre-computes the neighbor table.
    pub fn new(subdivision_level: u8, planet_radius_km: f64) -> Result<Self, GridError> {
        if !isea3h::is_valid_subdivision_level(subdivision_level) {
            return Err(GridError::InvalidSubdivisionLevel(subdivision_level));
        }
        if !planet_radius_km.is_finite() || planet_radius_km <= 0.0 {
            return Err(GridError::InvalidPlanetRadius(planet_radius_km));
        }

        let mut coords = Vec::new();
        let mut coord_to_id = BTreeMap::new();
        let mut centers = Vec::new();

        for (index, coord) in isea3h::all_cells(subdivision_level).enumerate() {
            let id = HexId(index as u32);
            centers.push(isea3h::cell_center_vec3(coord, subdivision_level));
            coord_to_id.insert(coord, id);
            coords.push(coord);
        }

        let cell_count = coords.len() as u32;
        let neighbor_table = neighbors::build_neighbor_table(&coords, &centers, subdivision_level)?;
        let uniform_hex_area_km2 = geography::uniform_hex_area_km2(planet_radius_km, cell_count);

        Ok(Self {
            subdivision_level,
            planet_radius_km,
            cell_count,
            uniform_hex_area_km2,
            coords,
            coord_to_id,
            centers,
            neighbors: neighbor_table,
        })
    }

    /// Total number of cells in the grid.
    pub fn cell_count(&self) -> u32 {
        self.cell_count
    }

    /// Returns the subdivision level this grid was built at.
    pub fn subdivision_level(&self) -> u8 {
        self.subdivision_level
    }

    /// Returns the planet radius (km) this grid was built for.
    pub fn planet_radius_km(&self) -> f64 {
        self.planet_radius_km
    }

    /// True if the given hex is a pentagon (5 neighbors instead of 6).
    pub fn is_pentagon(&self, hex: HexId) -> bool {
        hex.0 < 12
    }

    /// Returns the neighbors of a hex. Slice has 5 entries for pentagons, 6 for hexes.
    pub fn neighbors(&self, hex: HexId) -> &[HexId] {
        &self.neighbors[hex.0 as usize]
    }

    /// Returns the neighbor in a specific direction, if one exists at that index for this hex.
    /// Returns `None` for the missing direction on pentagons (`D5`).
    pub fn neighbor_in_direction(&self, hex: HexId, direction: Direction) -> Option<HexId> {
        let neighbors = self.neighbors(hex);
        neighbors.get(direction.index()).copied()
    }

    /// Center of the hex as `(latitude_radians, longitude_radians)`.
    pub fn center_lat_lon(&self, hex: HexId) -> (f64, f64) {
        geography::center_lat_lon(self.centers[hex.0 as usize])
    }

    /// Unit-sphere center direction `(x, y, z)`; length ≈ 1.
    pub fn cell_center_direction(&self, hex: HexId) -> [f64; 3] {
        let v = self.centers[hex.0 as usize];
        [v.x, v.y, v.z]
    }

    /// Area of the hex on the planet's surface, in square kilometers.
    pub fn hex_area_km2(&self, hex: HexId) -> f64 {
        let _ = hex; // All hexes share uniform area in v1; per-hex variation deferred (Doc 04 §3.3.2).
        self.uniform_hex_area_km2
    }

    /// Surface (great-circle) distance between two hexes, in kilometers.
    pub fn distance_km(&self, a: HexId, b: HexId) -> f64 {
        if a == b {
            return 0.0;
        }
        geography::distance_km(
            self.centers[a.0 as usize],
            self.centers[b.0 as usize],
            self.planet_radius_km,
        )
    }

    /// Returns the hex nearest to the given geographic point.
    pub fn nearest_hex(&self, lat_rad: f64, lon_rad: f64) -> HexId {
        let point = isea3h::lat_lon_to_vec3(lat_rad, lon_rad);
        self.nearest_hex_direction([point.x, point.y, point.z])
    }

    /// Returns the hex whose center direction has the largest dot product with `direction`.
    ///
    /// Tie-break: lower [`HexId`] wins (deterministic).
    pub fn nearest_hex_direction(&self, direction: [f64; 3]) -> HexId {
        let mut best = HexId(0);
        let mut best_dot = f64::NEG_INFINITY;
        for (index, center) in self.centers.iter().enumerate() {
            let d = direction[0] * f64::from(center.x)
                + direction[1] * f64::from(center.y)
                + direction[2] * f64::from(center.z);
            let hex = HexId(index as u32);
            if d > best_dot {
                best_dot = d;
                best = hex;
            } else if d == best_dot && hex < best {
                best = hex;
            }
        }
        best
    }

    /// Returns the hex whose center has the largest dot product with `direction`,
    /// searching locally starting from `hint`.
    ///
    /// O(1) amortized when `hint` is close to the target hex. Uses the icosahedral
    /// grid's local convexity: hill-climbing through neighbors reliably converges
    /// to the global maximum. Use this when you have a reasonable starting hint
    /// (e.g., the previous position of a moving feature).
    ///
    /// For arbitrary directions with no good hint, use [`Self::nearest_hex_direction`]
    /// instead (O(n) but no hint required).
    ///
    /// Uses bounded hill-climbing capped at `2 * cell_count()` iterations. In
    /// practice converges in 1-5 steps. If the cap is hit (pathological case),
    /// falls back to global search.
    ///
    /// Tie-break: lower [`HexId`] wins, matching [`Self::nearest_hex_direction`].
    pub fn nearest_hex_direction_from(&self, hint: HexId, direction: [f64; 3]) -> HexId {
        let max_iterations = (self.cell_count() as usize) * 2;
        let mut current = hint;
        let mut current_dot = self.dot_with_direction(current, direction);

        for _ in 0..max_iterations {
            let neighbors = self.neighbors(current);
            let mut best_neighbor = current;
            let mut best_neighbor_dot = current_dot;

            for &neighbor in neighbors {
                let n_dot = self.dot_with_direction(neighbor, direction);
                if n_dot > best_neighbor_dot {
                    best_neighbor_dot = n_dot;
                    best_neighbor = neighbor;
                } else if n_dot == best_neighbor_dot && neighbor < best_neighbor {
                    best_neighbor = neighbor;
                }
            }

            if best_neighbor == current {
                return current;
            }

            current = best_neighbor;
            current_dot = best_neighbor_dot;
        }

        self.nearest_hex_direction(direction)
    }

    /// Dot product of `hex`'s center direction with `direction`.
    fn dot_with_direction(&self, hex: HexId, direction: [f64; 3]) -> f64 {
        let center = self.centers[hex.0 as usize];
        direction[0] * f64::from(center.x)
            + direction[1] * f64::from(center.y)
            + direction[2] * f64::from(center.z)
    }

    /// Iterate all valid [`HexId`] values in this grid (`0..cell_count`).
    pub fn iter(&self) -> impl Iterator<Item = HexId> + '_ {
        (0..self.cell_count).map(HexId)
    }

    /// Canonical byte serialization of neighbor tables for determinism tests.
    #[cfg(test)]
    pub(crate) fn neighbors_canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for neighbor_row in &self.neighbors {
            for neighbor in neighbor_row {
                bytes.extend_from_slice(&neighbor.0.to_le_bytes());
            }
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;
    use std::time::Instant;

    const EARTH_RADIUS_KM: f64 = 6371.0;

    fn assert_approx_eq(a: f64, b: f64, eps: f64) {
        assert!((a - b).abs() <= eps, "expected {a} ≈ {b} (eps {eps})");
    }

    #[test]
    fn construction_succeeds_levels_0_through_9() {
        for level in 0..=isea3h::MAX_SUBDIVISION_LEVEL {
            let grid = HexGrid::new(level, EARTH_RADIUS_KM).expect("grid constructs");
            assert_eq!(grid.subdivision_level(), level);
            assert_eq!(grid.planet_radius_km(), EARTH_RADIUS_KM);
        }
    }

    #[test]
    fn cell_count_matches_isea3h() {
        for level in 0..=isea3h::MAX_SUBDIVISION_LEVEL {
            assert_eq!(
                HexGrid::new(level, EARTH_RADIUS_KM).unwrap().cell_count(),
                isea3h::cell_count(level) as u32,
                "level {level}"
            );
        }
    }

    #[test]
    fn iter_covers_all_ids_without_gaps() {
        for level in 0..=4 {
            let grid = HexGrid::new(level, EARTH_RADIUS_KM).unwrap();
            let ids: Vec<HexId> = grid.iter().collect();
            assert_eq!(ids.len(), grid.cell_count() as usize);
            for (i, id) in ids.iter().enumerate() {
                assert_eq!(id.0, i as u32);
            }
        }
    }

    #[test]
    fn first_twelve_ids_are_pentagons() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).unwrap();
        for i in 0..12 {
            assert!(grid.is_pentagon(HexId(i)));
        }
        assert!(!grid.is_pentagon(HexId(12)));
    }

    #[test]
    fn exactly_twelve_pentagons_at_each_level() {
        for level in 0..=6 {
            let grid = HexGrid::new(level, EARTH_RADIUS_KM).unwrap();
            let count = grid.iter().filter(|id| grid.is_pentagon(*id)).count();
            assert_eq!(count, 12, "level {level}");
        }
    }

    #[test]
    fn neighbor_counts_pentagon_five_hex_six() {
        for level in 0..=4 {
            let grid = HexGrid::new(level, EARTH_RADIUS_KM).unwrap();
            for hex in grid.iter() {
                let n = grid.neighbors(hex).len();
                if grid.is_pentagon(hex) {
                    assert_eq!(n, 5, "pentagon {:?}", hex);
                } else {
                    assert_eq!(n, 6, "hex {:?}", hex);
                }
            }
        }
    }

    #[test]
    fn neighbor_symmetry() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).unwrap();
        for hex in grid.iter() {
            for &neighbor in grid.neighbors(hex) {
                assert!(
                    grid.neighbors(neighbor).contains(&hex),
                    "{hex:?} -> {neighbor:?} but not reverse"
                );
            }
        }
    }

    #[test]
    fn no_self_neighbors() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).unwrap();
        for hex in grid.iter() {
            assert!(!grid.neighbors(hex).contains(&hex));
        }
    }

    #[test]
    fn neighbor_in_direction_matches_neighbors_slice() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).unwrap();
        for hex in grid.iter() {
            let neighbors = grid.neighbors(hex);
            let max_dir = if grid.is_pentagon(hex) { 5 } else { 6 };
            for (d, &neighbor) in neighbors.iter().enumerate().take(max_dir) {
                let direction = Direction::from_index(d).unwrap();
                assert_eq!(grid.neighbor_in_direction(hex, direction), Some(neighbor));
            }
            if grid.is_pentagon(hex) {
                assert_eq!(grid.neighbor_in_direction(hex, Direction::D5), None);
            }
        }
    }

    #[test]
    fn center_lat_lon_matches_isea3h() {
        let level = 4;
        let grid = HexGrid::new(level, EARTH_RADIUS_KM).unwrap();
        let coords: Vec<_> = isea3h::all_cells(level).collect();
        for hex in grid.iter() {
            let (lat, lon) = grid.center_lat_lon(hex);
            let expected = isea3h::cell_center_lat_lon(coords[hex.0 as usize], level);
            assert_approx_eq(lat, expected.lat_rad, 1e-12);
            assert_approx_eq(lon, expected.lon_rad, 1e-12);
        }
    }

    #[test]
    fn hex_area_sum_approximates_sphere_surface() {
        let grid = HexGrid::new(6, EARTH_RADIUS_KM).unwrap();
        let r = EARTH_RADIUS_KM;
        let expected = 4.0 * PI * r * r;
        let sum: f64 = grid.iter().map(|h| grid.hex_area_km2(h)).sum();
        assert_approx_eq(sum, expected, expected * 1e-6);
        for hex in grid.iter() {
            assert!(grid.hex_area_km2(hex) > 0.0);
        }
    }

    #[test]
    fn distance_symmetric_and_self_zero() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).unwrap();
        for a in grid.iter() {
            assert_eq!(grid.distance_km(a, a), 0.0);
        }
        for a in grid.iter() {
            for &b in grid.neighbors(a) {
                assert_approx_eq(grid.distance_km(a, b), grid.distance_km(b, a), 1e-6);
            }
        }
    }

    #[test]
    fn nearest_hex_round_trip_within_cell_radius() {
        let samples = [
            (0.0, 0.0),
            (0.5, 1.0),
            (-0.3, -2.1),
            (1.0, 0.2),
            (-1.1, 3.0),
        ];
        for level in 0..=6 {
            let grid = HexGrid::new(level, EARTH_RADIUS_KM).unwrap();
            let tol = isea3h::max_cell_angular_radius(level);
            for (lat, lon) in samples {
                let hex = grid.nearest_hex(lat, lon);
                let (lat2, lon2) = grid.center_lat_lon(hex);
                let v0 = isea3h::lat_lon_to_vec3(lat, lon);
                let v1 = isea3h::lat_lon_to_vec3(lat2, lon2);
                let angular = v0.dot(v1).clamp(-1.0, 1.0).acos();
                assert!(
                    angular <= tol + 1e-10,
                    "level {level} ({lat},{lon}) error {angular} > {tol}"
                );
            }
        }
    }

    #[test]
    fn determinism_identical_neighbor_tables() {
        for level in [0u8, 3, 6] {
            let a = HexGrid::new(level, EARTH_RADIUS_KM).unwrap();
            let b = HexGrid::new(level, EARTH_RADIUS_KM).unwrap();
            assert_eq!(
                a.neighbors_canonical_bytes(),
                b.neighbors_canonical_bytes(),
                "level {level}"
            );
        }
    }

    #[test]
    fn grid_construction_level_8_timing() {
        let start = Instant::now();
        let grid = HexGrid::new(8, EARTH_RADIUS_KM).expect("level 8 constructs");
        let elapsed = start.elapsed();
        assert_eq!(grid.cell_count(), 65_612);
        eprintln!(
            "level 8 HexGrid construction: {:.3}s (rescope target < 5s release)",
            elapsed.as_secs_f64()
        );
        assert!(
            elapsed.as_secs_f64() < 5.0,
            "level 8 construction took {:.3}s (expected < 5s)",
            elapsed.as_secs_f64()
        );
    }

    #[test]
    fn invalid_subdivision_level_errors() {
        assert!(matches!(
            HexGrid::new(10, EARTH_RADIUS_KM),
            Err(GridError::InvalidSubdivisionLevel(10))
        ));
    }

    #[test]
    fn invalid_planet_radius_errors() {
        assert!(matches!(
            HexGrid::new(4, 0.0),
            Err(GridError::InvalidPlanetRadius(r)) if r == 0.0
        ));
    }

    #[test]
    fn all_cell_centers_are_unique() {
        use std::collections::BTreeSet;

        let grid = HexGrid::new(5, EARTH_RADIUS_KM).unwrap();
        let mut seen: BTreeSet<(i64, i64, i64)> = BTreeSet::new();

        for hex in grid.iter() {
            let center = grid.cell_center_direction(hex);
            let key = (
                (center[0] * 1e9).round() as i64,
                (center[1] * 1e9).round() as i64,
                (center[2] * 1e9).round() as i64,
            );
            assert!(
                seen.insert(key),
                "duplicate cell center for {:?} at level 5",
                hex
            );
        }
    }

    #[test]
    fn cell_centers_unique_at_multiple_levels() {
        use std::collections::BTreeSet;

        for level in [3u8, 5, 7] {
            let grid = HexGrid::new(level, EARTH_RADIUS_KM).unwrap();
            let mut seen: BTreeSet<(i64, i64, i64)> = BTreeSet::new();

            for hex in grid.iter() {
                let center = grid.cell_center_direction(hex);
                let key = (
                    (center[0] * 1e9).round() as i64,
                    (center[1] * 1e9).round() as i64,
                    (center[2] * 1e9).round() as i64,
                );
                assert!(
                    seen.insert(key),
                    "level {level}: duplicate cell center for {:?}",
                    hex
                );
            }
        }
    }

    #[test]
    fn nearest_hex_direction_from_matches_global_for_each_hex_target() {
        let grid = HexGrid::new(5, EARTH_RADIUS_KM).unwrap();
        let n = grid.cell_count() as usize;

        for query_idx in 0..n {
            let query_hex = HexId(query_idx as u32);
            let target = grid.cell_center_direction(query_hex);
            let global_result = grid.nearest_hex_direction(target);

            for hint_idx in [0, n / 4, n / 2, (n * 3) / 4, n - 1] {
                let hint = HexId(hint_idx as u32);
                let local_result = grid.nearest_hex_direction_from(hint, target);
                assert_eq!(
                    local_result, global_result,
                    "from hint {hint:?} querying {query_hex:?}'s center, got {local_result:?} but global says {global_result:?}"
                );
            }
        }
    }

    #[test]
    fn nearest_hex_direction_from_matches_global_with_random_directions() {
        let grid = HexGrid::new(5, EARTH_RADIUS_KM).unwrap();
        let n = grid.cell_count() as usize;

        let mut state: u64 = 0x517c_c1b7_2722_0a95;
        let mut next_rand = || -> f64 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state as f64) / (u64::MAX as f64)
        };

        for trial in 0..100 {
            let u = next_rand();
            let v = next_rand();
            let theta = 2.0 * PI * u;
            let phi = (2.0 * v - 1.0).acos();
            let target = [phi.sin() * theta.cos(), phi.sin() * theta.sin(), phi.cos()];

            let global_result = grid.nearest_hex_direction(target);

            for hint_idx in [0, n / 8, n / 4, n / 2, n - 1] {
                let hint = HexId(hint_idx as u32);
                let local_result = grid.nearest_hex_direction_from(hint, target);
                assert_eq!(
                    local_result, global_result,
                    "trial {trial}: local search from hint {hint:?} disagrees with global"
                );
            }
        }
    }

    #[test]
    fn nearest_hex_direction_from_returns_self_or_neighbor_for_small_drift() {
        let grid = HexGrid::new(5, EARTH_RADIUS_KM).unwrap();

        for hex_idx in [0u32, 12, 100, 500, 1000, 2000] {
            let hex = HexId(hex_idx);
            if hex.0 as usize >= grid.cell_count() as usize {
                continue;
            }
            let center = grid.cell_center_direction(hex);

            let p = [center[0] + 0.01, center[1] - 0.01, center[2] + 0.005];
            let mag = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
            let perturbed = [p[0] / mag, p[1] / mag, p[2] / mag];

            let result = grid.nearest_hex_direction_from(hex, perturbed);

            let mut allowed: Vec<HexId> = vec![hex];
            allowed.extend(grid.neighbors(hex).iter().copied());
            assert!(
                allowed.contains(&result),
                "small perturbation of {hex:?} produced unexpected hex {result:?}; allowed: {allowed:?}"
            );
        }
    }
}
