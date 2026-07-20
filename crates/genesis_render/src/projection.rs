//! Map projections: equirectangular (flat) and orthographic (globe), plus the
//! polar-skip rule the flat map needs.
//!
//! The renderer, picker, and overlays all route lat/lon (or unit-sphere
//! directions) through a [`MapProjection`] so a single toggle swaps the whole
//! view between the flat map and the rotatable globe.

use glam::DVec3;

use crate::polygon::{direction_to_lat_lon, unwrap_lon_relative};

/// Equirectangular projection: longitude → x, latitude → y (radians).
pub fn project(lat_rad: f64, lon_rad: f64) -> (f32, f32) {
    (lon_rad as f32, lat_rad as f32)
}

/// Inverse of [`project`] for 2D world coordinates.
pub fn unproject(x: f32, y: f32) -> (f64, f64) {
    (y as f64, x as f64)
}

/// Latitude threshold (radians) above which a hex is too close to a pole to render
/// correctly with simple equirectangular projection.
///
/// Approximately 70° in both hemispheres. Polar hexes have vertices spanning wide
/// longitude ranges that fan into self-crossing polygons when projected. The globe
/// (orthographic) projection has no such limitation and renders the poles cleanly.
const POLE_SKIP_LAT_RAD: f64 = 1.22; // ~70 degrees

/// Returns true if a hex should be skipped when drawing on an equirectangular map.
///
/// Skip criterion: the hex's center is within ~20° of a pole. The pole region
/// cannot be represented as a single polygon in equirectangular projection without
/// splitting at the antimeridian or adding pole-cap triangles.
pub fn should_skip_for_equirectangular(center_lat_rad: f64) -> bool {
    center_lat_rad.abs() > POLE_SKIP_LAT_RAD
}

/// Unit-sphere direction for a geographic lat/lon (radians). Inverse of
/// [`direction_to_lat_lon`].
pub fn lat_lon_to_dir(lat_rad: f64, lon_rad: f64) -> DVec3 {
    let (clat, slat) = (lat_rad.cos(), lat_rad.sin());
    let (clon, slon) = (lon_rad.cos(), lon_rad.sin());
    DVec3::new(clat * clon, clat * slon, slat)
}

/// The sub-viewer point of an azimuthal projection — the spot on the globe that
/// faces the camera. Panning the globe moves this point; the flat map ignores it
/// for projection (the camera translates instead).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewCenter {
    pub lat_rad: f64,
    pub lon_rad: f64,
}

impl ViewCenter {
    /// Unit direction of the view center (points from the globe's core toward the
    /// camera).
    pub fn axis(self) -> DVec3 {
        lat_lon_to_dir(self.lat_rad, self.lon_rad)
    }

    /// North-up orthonormal screen basis `(east, north, axis)` for the
    /// orthographic projection. `east` is screen +x, `north` is screen +y, `axis`
    /// is depth toward the viewer. Degenerate at the exact pole (axis ∥ world up);
    /// falls back to a fixed east there.
    fn basis(self) -> (DVec3, DVec3, DVec3) {
        let axis = self.axis();
        let east = DVec3::Z.cross(axis);
        let east = if east.length_squared() < 1.0e-12 {
            DVec3::Y
        } else {
            east.normalize()
        };
        let north = axis.cross(east); // unit: axis ⟂ east, both unit
        (east, north, axis)
    }
}

/// Selectable map projection. [`Equirectangular`](Self::Equirectangular) is the
/// flat 2:1 map; [`Orthographic`](Self::Orthographic) is the rotatable globe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MapProjection {
    #[default]
    Equirectangular,
    Orthographic,
}

impl MapProjection {
    /// All projections in cycle order.
    pub const ALL: [MapProjection; 2] = [Self::Equirectangular, Self::Orthographic];

    /// Short human-readable name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Equirectangular => "Flat map",
            Self::Orthographic => "Globe",
        }
    }

    /// Next projection in [`ALL`](Self::ALL), wrapping around.
    pub fn cycle_next(self) -> Self {
        let idx = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    /// Whether a hex whose center points in `center_dir` is drawable in this
    /// projection at the given `view`. The flat map drops the self-crossing polar
    /// caps; the globe shows only the near hemisphere.
    pub fn hex_visible(self, center_dir: DVec3, view: ViewCenter) -> bool {
        match self {
            Self::Equirectangular => {
                let (lat, _) = direction_to_lat_lon(center_dir);
                !should_skip_for_equirectangular(lat)
            }
            // Front hemisphere only. Culling on the center leaves a clean (if
            // slightly jagged) limb; near-limb half-hexes are dropped whole.
            Self::Orthographic => center_dir.dot(view.axis()) > 0.0,
        }
    }

    /// Project a unit-sphere direction to 2D world coordinates.
    ///
    /// `hex_center_dir` is the projected hex's own center direction. The flat map
    /// uses it to unwrap each vertex's longitude across the ±π seam so the polygon
    /// stays contiguous; the globe ignores it. World units: the flat map returns
    /// radians (lon, lat); the globe returns points on the unit disc (radius 1).
    pub fn project(self, dir: DVec3, hex_center_dir: DVec3, view: ViewCenter) -> (f32, f32) {
        match self {
            Self::Equirectangular => {
                let (lat, lon) = direction_to_lat_lon(dir);
                let (_, center_lon) = direction_to_lat_lon(hex_center_dir);
                let unwrapped = unwrap_lon_relative(lon, center_lon);
                project(lat, unwrapped)
            }
            Self::Orthographic => {
                let (east, north, _axis) = view.basis();
                ((dir.dot(east)) as f32, (dir.dot(north)) as f32)
            }
        }
    }

    /// Inverse of [`project`](Self::project): 2D world coordinates back to a
    /// geographic `(lat, lon)` in radians. Returns `None` when the point is
    /// outside the projection's domain (the globe's unit disc).
    pub fn unproject(self, x: f32, y: f32, view: ViewCenter) -> Option<(f64, f64)> {
        match self {
            Self::Equirectangular => Some(unproject(x, y)),
            Self::Orthographic => {
                let (x, y) = (x as f64, y as f64);
                let r2 = x * x + y * y;
                if r2 > 1.0 {
                    return None; // outside the globe's disc
                }
                let depth = (1.0 - r2).max(0.0).sqrt();
                let (east, north, axis) = view.basis();
                let p = east * x + north * y + axis * depth;
                Some(direction_to_lat_lon(p))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

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
        assert!(!should_skip_for_equirectangular(0.0));
        assert!(!should_skip_for_equirectangular(1.0));
        assert!(!should_skip_for_equirectangular(-1.0));
        assert!(should_skip_for_equirectangular(1.3));
        assert!(should_skip_for_equirectangular(-1.3));
        assert!(should_skip_for_equirectangular(FRAC_PI_2));
        assert!(should_skip_for_equirectangular(-FRAC_PI_2));
    }

    #[test]
    fn lat_lon_to_dir_round_trips() {
        let samples = [
            (0.0, 0.0),
            (FRAC_PI_4, 1.0),
            (-FRAC_PI_4, -2.0),
            (1.2, PI - 0.1),
        ];
        for (lat, lon) in samples {
            let dir = lat_lon_to_dir(lat, lon);
            assert!((dir.length() - 1.0).abs() < 1e-12, "not unit length");
            let (lat2, lon2) = direction_to_lat_lon(dir);
            assert!((lat - lat2).abs() < 1e-9, "lat {lat} vs {lat2}");
            assert!((lon - lon2).abs() < 1e-9, "lon {lon} vs {lon2}");
        }
    }

    #[test]
    fn projection_cycles_through_all() {
        let mut p = MapProjection::default();
        assert_eq!(p, MapProjection::Equirectangular);
        p = p.cycle_next();
        assert_eq!(p, MapProjection::Orthographic);
        p = p.cycle_next();
        assert_eq!(p, MapProjection::Equirectangular, "wraps around");
    }

    #[test]
    fn ortho_center_projects_to_origin() {
        // The view center itself sits at the middle of the disc, facing the camera.
        for (lat, lon) in [(0.0, 0.0), (0.5, 1.0), (-0.8, -2.5)] {
            let view = ViewCenter {
                lat_rad: lat,
                lon_rad: lon,
            };
            let center = view.axis();
            assert!(MapProjection::Orthographic.hex_visible(center, view));
            let (x, y) = MapProjection::Orthographic.project(center, center, view);
            assert!(x.abs() < 1e-6 && y.abs() < 1e-6, "center at ({x},{y})");
        }
    }

    #[test]
    fn ortho_is_north_up_and_east_right() {
        // Equatorial view down the +X axis: north (+Z) is up, east (+Y) is right.
        let view = ViewCenter {
            lat_rad: 0.0,
            lon_rad: 0.0,
        };
        let north_of_center = lat_lon_to_dir(0.1, 0.0);
        let (_, ny) = MapProjection::Orthographic.project(north_of_center, north_of_center, view);
        assert!(ny > 0.0, "north should map to +y, got {ny}");

        let east_of_center = lat_lon_to_dir(0.0, 0.1);
        let (ex, _) = MapProjection::Orthographic.project(east_of_center, east_of_center, view);
        assert!(ex > 0.0, "east should map to +x, got {ex}");
    }

    #[test]
    fn ortho_culls_the_far_hemisphere() {
        let view = ViewCenter {
            lat_rad: 0.0,
            lon_rad: 0.0,
        };
        // Antipode of the view center faces away.
        let antipode = lat_lon_to_dir(0.0, PI);
        assert!(!MapProjection::Orthographic.hex_visible(antipode, view));
        // A point 45° away is on the near side.
        let near = lat_lon_to_dir(0.0, FRAC_PI_4);
        assert!(MapProjection::Orthographic.hex_visible(near, view));
    }

    #[test]
    fn ortho_project_unproject_round_trips_near_side() {
        let view = ViewCenter {
            lat_rad: 0.3,
            lon_rad: -0.7,
        };
        for (lat, lon) in [(0.3, -0.7), (0.5, -0.4), (0.1, -1.0), (-0.2, -0.9)] {
            let dir = lat_lon_to_dir(lat, lon);
            assert!(MapProjection::Orthographic.hex_visible(dir, view));
            let (x, y) = MapProjection::Orthographic.project(dir, dir, view);
            let (lat2, lon2) = MapProjection::Orthographic
                .unproject(x, y, view)
                .expect("near-side point is inside the disc");
            assert!((lat - lat2).abs() < 1e-6, "lat {lat} vs {lat2}");
            assert!((lon - lon2).abs() < 1e-6, "lon {lon} vs {lon2}");
        }
    }

    #[test]
    fn ortho_unproject_outside_disc_is_none() {
        let view = ViewCenter {
            lat_rad: 0.0,
            lon_rad: 0.0,
        };
        assert!(MapProjection::Orthographic.unproject(1.5, 0.0, view).is_none());
        assert!(MapProjection::Orthographic.unproject(0.8, 0.8, view).is_none());
        // On the disc: valid.
        assert!(MapProjection::Orthographic.unproject(0.5, 0.5, view).is_some());
    }

    #[test]
    fn ortho_points_stay_within_unit_disc() {
        let view = ViewCenter {
            lat_rad: 0.2,
            lon_rad: 0.4,
        };
        // Every front-facing direction projects inside the unit disc.
        for lat_i in -8..=8 {
            for lon_i in -8..=8 {
                let lat = f64::from(lat_i) * (FRAC_PI_2 / 8.0);
                let lon = f64::from(lon_i) * (PI / 8.0);
                let dir = lat_lon_to_dir(lat, lon);
                if !MapProjection::Orthographic.hex_visible(dir, view) {
                    continue;
                }
                let (x, y) = MapProjection::Orthographic.project(dir, dir, view);
                let r = ((x * x + y * y) as f64).sqrt();
                assert!(r <= 1.0 + 1e-6, "point outside disc: r={r}");
            }
        }
    }

    #[test]
    fn equirect_projection_matches_free_functions() {
        // The enum's Equirectangular arm must agree with the standalone project().
        let dir = lat_lon_to_dir(0.4, 1.1);
        let (x, y) = MapProjection::Equirectangular.project(dir, dir, ViewCenter {
            lat_rad: 0.0,
            lon_rad: 0.0,
        });
        let (fx, fy) = project(0.4, 1.1);
        assert!((x - fx).abs() < 1e-6 && (y - fy).abs() < 1e-6);
    }
}
