//! Equirectangular projection and polar skip rules.

use bevy::prelude::*;

/// Equirectangular projection: longitude → x, latitude → y (radians).
pub fn project(lat_rad: f64, lon_rad: f64) -> (f32, f32) {
    (lon_rad as f32, lat_rad as f32)
}

/// Inverse of [`project`] for 2D world coordinates.
#[cfg_attr(not(test), allow(dead_code))]
pub fn unproject(x: f32, y: f32) -> (f64, f64) {
    (y as f64, x as f64)
}

/// Latitude threshold (radians) above which a hex is too close to a pole to render
/// correctly with simple equirectangular projection.
///
/// Approximately 70° in both hemispheres. Polar hexes have vertices spanning wide
/// longitude ranges that fan into self-crossing polygons when projected. Proper
/// polar handling (cap triangles or alternative projections) is Phase 3 work.
const POLE_SKIP_LAT_RAD: f64 = 1.22; // ~70 degrees

/// Returns true if a hex should be skipped when drawing on an equirectangular map.
///
/// Skip criterion: the hex's center is within ~20° of a pole. The pole region
/// cannot be represented as a single polygon in equirectangular projection without
/// splitting at the antimeridian or adding pole-cap triangles, both of which are
/// out of scope for Phase 0.
pub fn should_skip_for_equirectangular(center_lat_rad: f64) -> bool {
    center_lat_rad.abs() > POLE_SKIP_LAT_RAD
}

/// Builds a 2D triangle-fan mesh for one hex in projected space.
pub fn hex_mesh_2d(center: (f32, f32), ring: &[(f32, f32)]) -> Mesh {
    let mut positions = Vec::with_capacity(1 + ring.len());
    let mut indices = Vec::with_capacity(ring.len() * 3);

    positions.push([center.0, center.1, 0.0]);
    for &(x, y) in ring {
        positions.push([x, y, 0.0]);
    }

    let n = ring.len();
    for i in 0..n {
        let a = 0u32;
        let b = (i + 1) as u32;
        let c = ((i + 1) % n + 1) as u32;
        indices.extend_from_slice(&[a, b, c]);
    }

    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_indices(bevy::mesh::Indices::U32(indices));
    mesh
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_PI_4;

    use super::*;

    #[test]
    fn project_round_trips_sample_points() {
        let samples = [(0.0, 0.0), (FRAC_PI_4, 1.0), (-FRAC_PI_4, -2.0)];
        for (lat, lon) in samples {
            let (x, y) = project(lat, lon);
            let (lat2, lon2) = unproject(x, y);
            assert!((lat - lat2).abs() < 1e-6);
            assert!((lon - lon2).abs() < 1e-6);
        }
    }

    #[test]
    fn skip_polar_hexes() {
        use std::f64::consts::FRAC_PI_2;

        assert!(!should_skip_for_equirectangular(0.0));
        assert!(!should_skip_for_equirectangular(1.0));
        assert!(!should_skip_for_equirectangular(-1.0));
        assert!(should_skip_for_equirectangular(1.3));
        assert!(should_skip_for_equirectangular(-1.3));
        assert!(should_skip_for_equirectangular(FRAC_PI_2));
        assert!(should_skip_for_equirectangular(-FRAC_PI_2));
    }
}
