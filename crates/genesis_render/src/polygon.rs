//! Hex polygon vertex computation on the unit sphere.

use genesis_core::{HexGrid, HexId};
use glam::DVec3;

/// Computes polygon vertices for a single hex on the unit sphere.
///
/// Vertex `i` is the normalized centroid of the cell center, neighbor `i`, and neighbor `i+1`.
/// Hexes have 6 vertices; pentagons have 5.
pub fn hex_polygon_vertices(grid: &HexGrid, hex: HexId) -> Vec<DVec3> {
    let center = direction_to_dvec3(grid.cell_center_direction(hex));
    let neighbors = grid.neighbors(hex);
    let count = neighbors.len();

    let mut vertices = Vec::with_capacity(count);
    for i in 0..count {
        let n_i = direction_to_dvec3(grid.cell_center_direction(neighbors[i]));
        let n_next = direction_to_dvec3(grid.cell_center_direction(neighbors[(i + 1) % count]));
        let sum = center + n_i + n_next;
        let len = sum.length();
        if len > 0.0 {
            vertices.push(sum / len);
        } else {
            vertices.push(center);
        }
    }
    vertices
}

pub(crate) fn direction_to_dvec3([x, y, z]: [f64; 3]) -> DVec3 {
    DVec3::new(x, y, z)
}

/// Latitude and longitude in radians from a unit-sphere direction.
pub(crate) fn direction_to_lat_lon(v: DVec3) -> (f64, f64) {
    let lat = v.z.clamp(-1.0, 1.0).asin();
    let lon = v.y.atan2(v.x);
    (lat, lon)
}

/// Unwraps `lon` to be within π of `reference_lon` by shifting by integer multiples of 2π.
///
/// Used to make a polygon's vertices contiguous with its center in projected space.
/// When a hex straddles the antimeridian, some of its vertices have longitudes on the
/// opposite side of the ±π discontinuity from its center; this unwraps them to be
/// consistent with the center's longitude.
pub(crate) fn unwrap_lon_relative(lon: f64, reference_lon: f64) -> f64 {
    let mut diff = lon - reference_lon;
    while diff > std::f64::consts::PI {
        diff -= 2.0 * std::f64::consts::PI;
    }
    while diff < -std::f64::consts::PI {
        diff += 2.0 * std::f64::consts::PI;
    }
    reference_lon + diff
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::HexGrid;

    fn test_grid() -> HexGrid {
        HexGrid::new(5, 6371.0).expect("level 5 grid")
    }

    #[test]
    fn hex_has_six_vertices() {
        let grid = test_grid();
        let verts = hex_polygon_vertices(&grid, HexId(12));
        assert_eq!(verts.len(), 6);
    }

    #[test]
    fn pentagon_has_five_vertices() {
        let grid = test_grid();
        let verts = hex_polygon_vertices(&grid, HexId(0));
        assert_eq!(verts.len(), 5);
    }

    #[test]
    fn vertices_are_unit_length() {
        let grid = test_grid();
        for hex in grid.iter() {
            for v in hex_polygon_vertices(&grid, hex) {
                let len = v.length();
                assert!(
                    (len - 1.0).abs() < 1e-6,
                    "hex {:?} vertex length {len}",
                    hex
                );
            }
        }
    }

    #[test]
    fn polygon_center_approximates_cell_center() {
        let grid = test_grid();
        let mut checked = 0u32;
        let mut passed = 0u32;
        for hex in grid.iter() {
            if grid.is_pentagon(hex) {
                continue;
            }
            let verts = hex_polygon_vertices(&grid, hex);
            let mean: DVec3 = verts.iter().copied().sum::<DVec3>() / verts.len() as f64;
            let mean = mean.normalize();
            let center = direction_to_dvec3(grid.cell_center_direction(hex)).normalize();
            let dot = mean.dot(center);
            checked += 1;
            // Vertex-mean is a coarse Voronoi approximation; most hexes align within ~30°.
            if dot > 0.85 {
                passed += 1;
            }
        }
        assert!(checked > 0);
        let ratio = f64::from(passed) / f64::from(checked);
        assert!(
            ratio > 0.9,
            "only {passed}/{checked} hex centroids within tolerance (ratio {ratio})"
        );
    }

    #[test]
    fn unwrap_lon_keeps_close_values() {
        let r = 0.0;
        assert!((unwrap_lon_relative(0.5, r) - 0.5).abs() < 1e-10);
        assert!((unwrap_lon_relative(-0.5, r) - (-0.5)).abs() < 1e-10);
    }

    #[test]
    fn unwrap_lon_shifts_far_values() {
        use std::f64::consts::PI;
        let unwrapped = unwrap_lon_relative(-PI + 0.1, PI - 0.1);
        assert!(unwrapped > PI, "unwrapped value {unwrapped} should be > π");
        assert!((unwrapped - (PI + 0.1)).abs() < 1e-10);
    }

    #[test]
    fn unwrap_lon_handles_reference_at_negative_pi() {
        use std::f64::consts::PI;
        let unwrapped = unwrap_lon_relative(PI - 0.1, -PI + 0.1);
        assert!(unwrapped < -PI);
        assert!((unwrapped - (-PI - 0.1)).abs() < 1e-10);
    }

    #[test]
    fn pentagon_vertices_are_distinct() {
        let grid = test_grid();
        let verts = hex_polygon_vertices(&grid, HexId(0));
        for i in 0..verts.len() {
            for j in (i + 1)..verts.len() {
                assert!(
                    verts[i].distance(verts[j]) > 1e-6,
                    "duplicate vertices at {i} and {j}"
                );
            }
        }
    }
}
