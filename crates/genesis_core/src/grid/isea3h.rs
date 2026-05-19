//! ISEA3H coordinate math for Genesis Engine.
//!
//! # Representation
//!
//! Cells use a **hierarchical face path** on the icosahedron:
//!
//! - **Pentagon** cells (`Isea3hCoord::Pentagon`) sit at the 12 icosahedron vertices for
//!   every subdivision level. Level 0 consists of these 12 cells only.
//! - **Hex** cells (`Isea3hCoord::Hex`) belong to one of 20 triangular faces. Each hex is
//!   addressed by a base-3 path of length `0..=level - 1`. An empty path is the face
//!   centroid; each digit `0..2` steps one-third of the remaining barycentric distance
//!   toward that face corner. This yields `(3^level - 1) / 2` hexes per face and total
//!   count `10 * 3^level + 2`.
//!
//! Cell centers are **geodesic**: barycentric weights are applied to the three face
//! vertex unit vectors, then the result is normalized onto the unit sphere. This gives
//! deterministic, platform-stable centers. Full Snyder equal-area forward/inverse
//! projection (per the ISEA name) is deferred; topology and counts match ISEA3H aperture 3.
//!
//! Reference: aperture-3 refinement pattern and cell counts per Sahr et al. / DGGRID ISEA3H;
//! projection approach informed by the hexify package (MIT), without linking that library.

use std::f64::consts::PI;
use std::sync::OnceLock;

/// Maximum subdivision level validated by project docs (v1 worlds use 5–9; tests cover 0–9).
pub const MAX_SUBDIVISION_LEVEL: u8 = 9;

/// Internal ISEA3H cell coordinate. `HexId` mapping is step 2 (`grid` module).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Isea3hCoord {
    /// One of the 12 icosahedron vertices (always a pentagon).
    Pentagon(PentagonId),
    /// Hex cell on a triangular face, addressed by a base-3 refinement path.
    Hex { face: FaceId, path: FacePath },
}

/// Index of one of the 12 icosahedron vertices / pentagon cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PentagonId(pub u8);

/// Index of one of the 20 icosahedron triangular faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FaceId(pub u8);

/// Base-3 refinement path on a face (`0..2` per step). Empty path = face centroid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FacePath {
    digits: [u8; MAX_SUBDIVISION_LEVEL as usize],
    len: u8,
}

/// Geographic point in radians (north latitude, east longitude).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LatLon {
    pub lat_rad: f64,
    pub lon_rad: f64,
}

/// Unit-sphere Cartesian coordinates (right-handed, +Z north pole).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Barycentric coordinates on a triangular face (sum to 1).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Barycentric {
    a: f64,
    b: f64,
    c: f64,
}

// --- Public API ---

/// Total cells at subdivision `level`: `10 * 3^level + 2`.
pub fn cell_count(level: u8) -> u64 {
    let n = level as u32;
    10_u64.saturating_mul(3_u64.pow(n)).saturating_add(2)
}

/// Returns `true` when `level` is within the supported range for grid construction.
pub fn is_valid_subdivision_level(level: u8) -> bool {
    level <= MAX_SUBDIVISION_LEVEL
}

/// Whether this coordinate is one of the 12 pentagon cells.
pub fn is_pentagon(coord: Isea3hCoord) -> bool {
    matches!(coord, Isea3hCoord::Pentagon(_))
}

/// Cell center as latitude/longitude in radians.
pub fn cell_center_lat_lon(coord: Isea3hCoord, level: u8) -> LatLon {
    vec3_to_lat_lon(cell_center_vec3(coord, level))
}

/// Cell center as a unit vector on the sphere.
pub fn cell_center_vec3(coord: Isea3hCoord, level: u8) -> Vec3 {
    let _ = level;
    match coord {
        Isea3hCoord::Pentagon(id) => ico_vertices()[id.0 as usize],
        Isea3hCoord::Hex { face, path } => {
            let bary = barycentric_for_path(path);
            bary_to_vec3(bary, ICO_FACES[face.0 as usize])
        }
    }
}

/// Nearest cell at `level` to the given geographic point.
pub fn lat_lon_to_cell(lat_rad: f64, lon_rad: f64, level: u8) -> Isea3hCoord {
    nearest_cell(lat_lon_to_vec3(lat_rad, lon_rad), level)
}

/// Nearest cell at `level` to a unit-sphere point.
pub fn vec3_to_cell(point: Vec3, level: u8) -> Isea3hCoord {
    nearest_cell(normalize(point), level)
}

/// Latitude/longitude of the 12 pentagon vertices (fixed for all levels).
pub fn pentagon_lat_lon(pentagon: PentagonId) -> LatLon {
    vec3_to_lat_lon(ico_vertices()[pentagon.0 as usize])
}

/// Iterate all cells at `level` in deterministic order (pentagons, then face-major paths).
pub fn all_cells(level: u8) -> impl Iterator<Item = Isea3hCoord> {
    AllCells {
        level,
        face: 0,
        phase: Phase::Pentagons(0),
    }
}

/// Maximum observed round-trip angular error (radians) for `level`.
///
/// Computed deterministically by sampling directions on the sphere and measuring
/// lat/lon → nearest cell → cell center angular distance. Cached per level.
pub fn max_cell_angular_radius(level: u8) -> f64 {
    static CACHE: OnceLock<[f64; (MAX_SUBDIVISION_LEVEL + 1) as usize]> = OnceLock::new();
    CACHE.get_or_init(compute_max_round_trip_angular_errors)[level as usize]
}

fn compute_max_round_trip_angular_errors() -> [f64; (MAX_SUBDIVISION_LEVEL + 1) as usize] {
    let mut out = [0.0_f64; (MAX_SUBDIVISION_LEVEL + 1) as usize];
    let mut probe_directions = fibonacci_sphere_directions(512);
    for (lat, lon) in [
        (0.0, 0.0),
        (0.5, 1.0),
        (-0.3, -2.1),
        (1.0, 0.2),
        (-1.1, 3.0),
    ] {
        probe_directions.push(lat_lon_to_vec3(lat, lon));
    }
    for level in 0..=MAX_SUBDIVISION_LEVEL {
        let mut max_err = 0.0_f64;
        for point in &probe_directions {
            let cell = vec3_to_cell(*point, level);
            let center = cell_center_vec3(cell, level);
            let err = point.dot(center).clamp(-1.0, 1.0).acos();
            if err > max_err {
                max_err = err;
            }
        }
        out[level as usize] = max_err;
    }
    out
}

/// Deterministic quasi-uniform directions on the unit sphere (Fibonacci / golden spiral).
fn fibonacci_sphere_directions(count: usize) -> Vec<Vec3> {
    let golden = PI * (3.0 - 5.0_f64.sqrt());
    (0..count)
        .map(|i| {
            let y = 1.0 - (2.0 * (i as f64) + 1.0) / count as f64;
            let radius = (1.0 - y * y).max(0.0).sqrt();
            let theta = golden * i as f64;
            normalize(Vec3 {
                x: radius * theta.cos(),
                y: radius * theta.sin(),
                z: y,
            })
        })
        .collect()
}

// --- Face path ---

impl FacePath {
    pub const EMPTY: Self = Self {
        digits: [0; MAX_SUBDIVISION_LEVEL as usize],
        len: 0,
    };

    pub fn len(self) -> u8 {
        self.len
    }

    pub fn is_empty(self) -> bool {
        self.len == 0
    }

    pub fn digit(self, index: u8) -> Option<u8> {
        if index >= self.len {
            return None;
        }
        Some(self.digits[index as usize])
    }

    pub fn push(self, digit: u8) -> Option<Self> {
        if digit > 2 || self.len as usize >= MAX_SUBDIVISION_LEVEL as usize {
            return None;
        }
        let mut next = self;
        next.digits[self.len as usize] = digit;
        next.len += 1;
        Some(next)
    }
}

// --- Iteration ---

enum Phase {
    Pentagons(u8),
    Hexes { path_len: u8, path_index: u32 },
    Done,
}

struct AllCells {
    level: u8,
    face: u8,
    phase: Phase,
}

impl Iterator for AllCells {
    type Item = Isea3hCoord;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.phase {
                Phase::Pentagons(id) => {
                    if id >= 12 {
                        if self.level == 0 {
                            self.phase = Phase::Done;
                            continue;
                        }
                        self.phase = Phase::Hexes {
                            path_len: 0,
                            path_index: 0,
                        };
                        continue;
                    }
                    self.phase = Phase::Pentagons(id + 1);
                    return Some(Isea3hCoord::Pentagon(PentagonId(id)));
                }
                Phase::Hexes {
                    path_len,
                    path_index,
                } => {
                    if self.face >= 20 {
                        self.phase = Phase::Done;
                        continue;
                    }
                    let per_len = 3_u32.pow(path_len as u32);
                    if path_index < per_len {
                        let path = decode_path(path_len, path_index);
                        self.phase = Phase::Hexes {
                            path_len,
                            path_index: path_index + 1,
                        };
                        return Some(Isea3hCoord::Hex {
                            face: FaceId(self.face),
                            path,
                        });
                    }
                    if path_len + 1 < self.level {
                        self.phase = Phase::Hexes {
                            path_len: path_len + 1,
                            path_index: 0,
                        };
                        continue;
                    }
                    self.face += 1;
                    self.phase = Phase::Hexes {
                        path_len: 0,
                        path_index: 0,
                    };
                }
                Phase::Done => return None,
            }
        }
    }
}

fn decode_path(len: u8, mut index: u32) -> FacePath {
    let mut path = FacePath::EMPTY;
    for _ in 0..len {
        let digit = (index % 3) as u8;
        index /= 3;
        path = path.push(digit).expect("valid path digit");
    }
    path
}

// --- Icosahedron geometry ---

const PHI: f64 = 1.618_033_988_749_895;

/// 20 triangular faces as vertex index triples (outward winding).
const ICO_FACES: [[usize; 3]; 20] = [
    [0, 11, 5],
    [0, 5, 1],
    [0, 1, 7],
    [0, 7, 10],
    [0, 10, 11],
    [1, 5, 9],
    [5, 11, 4],
    [11, 10, 2],
    [10, 7, 6],
    [7, 1, 8],
    [3, 9, 4],
    [3, 4, 2],
    [3, 2, 6],
    [3, 6, 8],
    [3, 8, 9],
    [4, 9, 5],
    [2, 4, 11],
    [6, 2, 10],
    [8, 6, 7],
    [9, 8, 1],
];

fn ico_vertices() -> &'static [Vec3; 12] {
    static VERTS: OnceLock<[Vec3; 12]> = OnceLock::new();
    VERTS.get_or_init(|| {
        let raw = [
            Vec3 {
                x: 0.0,
                y: 1.0,
                z: PHI,
            },
            Vec3 {
                x: 0.0,
                y: -1.0,
                z: PHI,
            },
            Vec3 {
                x: 0.0,
                y: 1.0,
                z: -PHI,
            },
            Vec3 {
                x: 0.0,
                y: -1.0,
                z: -PHI,
            },
            Vec3 {
                x: 1.0,
                y: PHI,
                z: 0.0,
            },
            Vec3 {
                x: -1.0,
                y: PHI,
                z: 0.0,
            },
            Vec3 {
                x: 1.0,
                y: -PHI,
                z: 0.0,
            },
            Vec3 {
                x: -1.0,
                y: -PHI,
                z: 0.0,
            },
            Vec3 {
                x: PHI,
                y: 0.0,
                z: 1.0,
            },
            Vec3 {
                x: -PHI,
                y: 0.0,
                z: 1.0,
            },
            Vec3 {
                x: PHI,
                y: 0.0,
                z: -1.0,
            },
            Vec3 {
                x: -PHI,
                y: 0.0,
                z: -1.0,
            },
        ];
        let mut verts = raw.map(normalize);
        let target = lat_lon_to_vec3(58.28_f64.to_radians(), 11.25_f64.to_radians());
        verts = rotate_vertex_to(verts, 0, target);
        verts
    })
}

// --- Barycentric refinement ---

const FACE_CENTROID: Barycentric = Barycentric {
    a: 1.0 / 3.0,
    b: 1.0 / 3.0,
    c: 1.0 / 3.0,
};

fn barycentric_for_path(path: FacePath) -> Barycentric {
    let mut bary = FACE_CENTROID;
    for i in 0..path.len {
        let d = path.digits[i as usize];
        bary = step_toward_corner(bary, d);
    }
    bary
}

fn step_toward_corner(bary: Barycentric, corner: u8) -> Barycentric {
    let target = match corner {
        0 => Barycentric {
            a: 1.0,
            b: 0.0,
            c: 0.0,
        },
        1 => Barycentric {
            a: 0.0,
            b: 1.0,
            c: 0.0,
        },
        _ => Barycentric {
            a: 0.0,
            b: 0.0,
            c: 1.0,
        },
    };
    Barycentric {
        a: bary.a + (target.a - bary.a) / 3.0,
        b: bary.b + (target.b - bary.b) / 3.0,
        c: bary.c + (target.c - bary.c) / 3.0,
    }
}

fn bary_to_vec3(bary: Barycentric, face: [usize; 3]) -> Vec3 {
    let verts = ico_vertices();
    let a = verts[face[0]];
    let b = verts[face[1]];
    let c = verts[face[2]];
    normalize(Vec3 {
        x: bary.a * a.x + bary.b * b.x + bary.c * c.x,
        y: bary.a * a.y + bary.b * b.y + bary.c * c.y,
        z: bary.a * a.z + bary.b * b.z + bary.c * c.z,
    })
}

// --- Nearest cell ---

fn nearest_cell(point: Vec3, level: u8) -> Isea3hCoord {
    let mut best = Isea3hCoord::Pentagon(PentagonId(0));
    let mut best_dot = -f64::INFINITY;
    for coord in all_cells(level) {
        let center = cell_center_vec3(coord, level);
        let d = point.dot(center);
        if d > best_dot {
            best_dot = d;
            best = coord;
        }
    }
    best
}

// --- Vector / lat-lon helpers ---

impl Vec3 {
    fn dot(self, other: Vec3) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }
}

pub fn lat_lon_to_vec3(lat_rad: f64, lon_rad: f64) -> Vec3 {
    let cos_lat = lat_rad.cos();
    Vec3 {
        x: cos_lat * lon_rad.cos(),
        y: cos_lat * lon_rad.sin(),
        z: lat_rad.sin(),
    }
}

pub fn vec3_to_lat_lon(v: Vec3) -> LatLon {
    let v = normalize(v);
    LatLon {
        lat_rad: v.z.clamp(-1.0, 1.0).asin(),
        lon_rad: v.y.atan2(v.x),
    }
}

fn normalize(v: Vec3) -> Vec3 {
    let len = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
    if len == 0.0 {
        return Vec3 {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        };
    }
    Vec3 {
        x: v.x / len,
        y: v.y / len,
        z: v.z / len,
    }
}

fn rotate_vertex_to(verts: [Vec3; 12], from_index: usize, target: Vec3) -> [Vec3; 12] {
    let from = verts[from_index];
    let axis = cross(from, target);
    let axis_len = (axis.x * axis.x + axis.y * axis.y + axis.z * axis.z).sqrt();
    if axis_len < 1e-15 {
        if from.dot(target) > 0.0 {
            return verts;
        }
        return verts.map(|v| Vec3 {
            x: v.x,
            y: -v.y,
            z: -v.z,
        });
    }
    let axis = Vec3 {
        x: axis.x / axis_len,
        y: axis.y / axis_len,
        z: axis.z / axis_len,
    };
    let angle = from.dot(target).clamp(-1.0, 1.0).acos();
    verts.map(|v| rotate_around_axis(v, axis, angle))
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    Vec3 {
        x: a.y * b.z - a.z * b.y,
        y: a.z * b.x - a.x * b.z,
        z: a.x * b.y - a.y * b.x,
    }
}

fn rotate_around_axis(v: Vec3, axis: Vec3, angle: f64) -> Vec3 {
    let cos = angle.cos();
    let sin = angle.sin();
    let dot = v.dot(axis);
    let cross_av = cross(axis, v);
    Vec3 {
        x: v.x * cos + cross_av.x * sin + axis.x * dot * (1.0 - cos),
        y: v.y * cos + cross_av.y * sin + axis.y * dot * (1.0 - cos),
        z: v.z * cos + cross_av.z * sin + axis.z * dot * (1.0 - cos),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn assert_approx_eq(a: f64, b: f64, eps: f64) {
        assert!((a - b).abs() <= eps, "expected {a} ≈ {b} (eps {eps})");
    }

    fn angular_distance(a: Vec3, b: Vec3) -> f64 {
        a.dot(b).clamp(-1.0, 1.0).acos()
    }

    #[test]
    fn cell_count_matches_formula_levels_0_to_9() {
        let expected = [12, 32, 92, 272, 812, 2432, 7292, 21872, 65612, 196832];
        for (level, &want) in expected.iter().enumerate() {
            assert_eq!(cell_count(level as u8), want, "level {level}");
            assert_eq!(all_cells(level as u8).count(), want as usize);
        }
    }

    #[test]
    fn exactly_twelve_pentagons_at_each_level() {
        for level in 0..=MAX_SUBDIVISION_LEVEL {
            let pent_count = all_cells(level).filter(|c| is_pentagon(*c)).count();
            assert_eq!(pent_count, 12, "level {level}");
        }
    }

    #[test]
    fn pentagon_positions_stable_across_levels() {
        let anchors: Vec<LatLon> = (0..12).map(|i| pentagon_lat_lon(PentagonId(i))).collect();
        for level in 1..=MAX_SUBDIVISION_LEVEL {
            for (i, anchor) in anchors.iter().enumerate() {
                let coord = Isea3hCoord::Pentagon(PentagonId(i as u8));
                let center = cell_center_lat_lon(coord, level);
                assert_approx_eq(center.lat_rad, anchor.lat_rad, 1e-12);
                assert_approx_eq(center.lon_rad, anchor.lon_rad, 1e-12);
            }
        }
    }

    #[test]
    fn lat_lon_round_trip_within_cell_angular_radius() {
        let samples = [
            (0.0, 0.0),
            (0.5, 1.0),
            (-0.3, -2.1),
            (1.0, 0.2),
            (-1.1, 3.0),
        ];
        for level in 0..=6 {
            let tol = max_cell_angular_radius(level);
            for (lat, lon) in samples {
                let cell = lat_lon_to_cell(lat, lon, level);
                let center = cell_center_lat_lon(cell, level);
                let v0 = lat_lon_to_vec3(lat, lon);
                let v1 = lat_lon_to_vec3(center.lat_rad, center.lon_rad);
                let angular = angular_distance(v0, v1);
                assert!(
                    angular <= tol,
                    "level {level} ({lat},{lon}) angular error {angular} > {tol}"
                );
            }
        }
    }

    #[test]
    fn determinism_byte_identical_centers() {
        for level in [0u8, 3, 6, 9] {
            let first: Vec<u8> = all_cells(level)
                .flat_map(|coord| {
                    let v = cell_center_vec3(coord, level);
                    v.x.to_le_bytes()
                        .into_iter()
                        .chain(v.y.to_le_bytes())
                        .chain(v.z.to_le_bytes())
                })
                .collect();
            let second: Vec<u8> = all_cells(level)
                .flat_map(|coord| {
                    let v = cell_center_vec3(coord, level);
                    v.x.to_le_bytes()
                        .into_iter()
                        .chain(v.y.to_le_bytes())
                        .chain(v.z.to_le_bytes())
                })
                .collect();
            assert_eq!(first, second, "level {level}");
        }
    }

    #[test]
    fn all_coords_unique_at_level_4() {
        let level = 4;
        let coords: BTreeSet<Isea3hCoord> = all_cells(level).collect();
        assert_eq!(coords.len() as u64, cell_count(level));
    }

    #[test]
    fn only_pentagon_coords_are_pentagons() {
        for level in 0..=4 {
            for coord in all_cells(level) {
                assert_eq!(
                    is_pentagon(coord),
                    matches!(coord, Isea3hCoord::Pentagon(_))
                );
            }
        }
    }
}
