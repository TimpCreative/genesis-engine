//! ISEA3H coordinate math (Vince/Kristensen A3 scheme).
//!
//! Cells use extended Vince coordinates on the icosahedron per Kristensen (2021), with
//! closed-form topological neighbors. Cell centers are geodesic barycentric blends on the
//! unit sphere (Doc 04 §3.3.2).

use std::collections::{BTreeSet, VecDeque};
use std::f64::consts::PI;
use std::sync::OnceLock;

/// Maximum subdivision level validated by project docs (tests cover 0–9).
pub const MAX_SUBDIVISION_LEVEL: u8 = 9;

/// Internal ISEA3H cell coordinate (Vince/Kristensen extended coordinates).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Isea3hCoord {
    /// Pentagon at an icosahedron vertex.
    Pentagon { vertex: u8 },
    /// Hex on an icosahedron edge (two vertices, two A3 indices).
    Edge {
        v_i: u8,
        v_j: u8,
        h_i: i32,
        h_j: i32,
    },
    /// Hex in a face interior (three vertices, three A3 indices).
    Interior {
        v_i: u8,
        v_j: u8,
        v_k: u8,
        h_i: i32,
        h_j: i32,
        h_k: i32,
    },
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
    matches!(coord, Isea3hCoord::Pentagon { .. })
}

/// Cell center as latitude/longitude in radians.
pub fn cell_center_lat_lon(coord: Isea3hCoord, level: u8) -> LatLon {
    vec3_to_lat_lon(cell_center_vec3(coord, level))
}

/// Cell center as a unit vector on the sphere.
pub fn cell_center_vec3(coord: Isea3hCoord, level: u8) -> Vec3 {
    match coord {
        Isea3hCoord::Pentagon { vertex } => ico_vertices()[vertex as usize],
        Isea3hCoord::Edge { v_i, v_j, h_i, h_j } => {
            let h_hat = h_hat(level) as f64;
            let face = oriented_face_for_edge(v_i, v_j);
            let bary = Barycentric {
                a: h_i as f64 / h_hat,
                b: h_j as f64 / h_hat,
                c: 0.0,
            };
            bary_to_vec3(bary, face)
        }
        Isea3hCoord::Interior {
            v_i,
            v_j,
            v_k,
            h_i,
            h_j,
            h_k,
        } => {
            let h_hat = h_hat(level) as f64;
            let face = [v_i as usize, v_j as usize, v_k as usize];
            let bary = Barycentric {
                a: h_i as f64 / h_hat,
                b: h_j as f64 / h_hat,
                c: h_k as f64 / h_hat,
            };
            bary_to_vec3(bary, face)
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
pub fn pentagon_lat_lon(vertex: u8) -> LatLon {
    vec3_to_lat_lon(ico_vertices()[vertex as usize])
}

/// Topological neighbors at `level` (5 for pentagons, 6 for hexes).
pub(crate) fn coord_neighbors(coord: Isea3hCoord, level: u8) -> Vec<Isea3hCoord> {
    let mut out = match coord {
        Isea3hCoord::Pentagon { vertex } => pentagon_neighbors(vertex, level),
        Isea3hCoord::Edge { v_i, v_j, h_i, h_j } => edge_neighbors(v_i, v_j, h_i, h_j, level),
        Isea3hCoord::Interior {
            v_i,
            v_j,
            v_k,
            h_i,
            h_j,
            h_k,
        } => interior_neighbors(v_i, v_j, v_k, h_i, h_j, h_k, level),
    };
    out.retain(|&n| n != coord);
    out.sort();
    out.dedup();
    out
}

/// All cells at `level` in deterministic canonical order.
pub fn all_cells(level: u8) -> impl Iterator<Item = Isea3hCoord> + Clone {
    all_cells_cached(level).into_iter()
}

/// Maximum observed round-trip angular error (radians) for `level`.
pub fn max_cell_angular_radius(level: u8) -> f64 {
    static CACHE: OnceLock<[f64; (MAX_SUBDIVISION_LEVEL + 1) as usize]> = OnceLock::new();
    CACHE.get_or_init(compute_max_round_trip_angular_errors)[level as usize]
}

// --- Topology (Kristensen Fig. 6) ---

const S_EVEN: [u8; 5] = [10, 1, 2, 3, 11];
const S_ODD: [u8; 5] = [1, 4, 8, 9, 11];

fn vertex_neighbors(v: u8) -> [u8; 5] {
    let s = if v.is_multiple_of(2) { S_EVEN } else { S_ODD };
    let mut out = [0u8; 5];
    for (i, &step) in s.iter().enumerate() {
        out[i] = (v + step) % 12;
    }
    out
}

/// Edge-crossing: return `x'` such that `{x', y, z}` is the face adjacent across edge `{y,z}`.
fn omega(x: u8, y: u8, z: u8) -> u8 {
    let n_y = vertex_neighbors(y);
    let i = n_y.iter().position(|&v| v == z).expect("z adjacent to y");
    let cand_plus = n_y[(i + 1) % 5];
    let cand_minus = n_y[(i + 4) % 5];
    if cand_plus == x {
        cand_minus
    } else {
        cand_plus
    }
}

fn faces_containing_edge(v_i: u8, v_j: u8) -> [usize; 2] {
    let mut found = [None, None];
    let mut count = 0usize;
    for (idx, face) in ico_faces().iter().enumerate() {
        let on = face.contains(&(v_i as usize)) && face.contains(&(v_j as usize));
        if on {
            found[count] = Some(idx);
            count += 1;
            if count == 2 {
                break;
            }
        }
    }
    [
        found[0].expect("edge on face"),
        found[1].expect("edge on two faces"),
    ]
}

fn face_for_edge(v_i: u8, v_j: u8) -> [usize; 3] {
    ico_faces()[faces_containing_edge(v_i, v_j)[0]]
}

/// Face corners ordered `[v_i, v_j, v_k]` so barycentric `h_i`/`h_j` attach to the correct vertices.
fn oriented_face_for_edge(v_i: u8, v_j: u8) -> [usize; 3] {
    let face = face_for_edge(v_i, v_j);
    let vi = v_i as usize;
    let vj = v_j as usize;
    let vk = face
        .iter()
        .copied()
        .find(|&v| v != vi && v != vj)
        .expect("icosahedron face has a third vertex");
    [vi, vj, vk]
}

fn h_hat(level: u8) -> i32 {
    let exp = (level as u32).div_ceil(2);
    3_i32.pow(exp)
}

// --- Coordinate normalization ---

/// Post-process neighbor `(v, h)` like the reference R (zero `v` where `h==0`, sort by `v`).
fn finalize_neighbor(v: [u8; 3], h: [i32; 3]) -> Isea3hCoord {
    let mut pairs = Vec::with_capacity(3);
    for i in 0..3 {
        if h[i] != 0 {
            pairs.push((v[i], h[i]));
        }
    }
    match pairs.len() {
        1 => Isea3hCoord::Pentagon { vertex: pairs[0].0 },
        2 => {
            pairs.sort_by_key(|p| p.0);
            Isea3hCoord::Edge {
                v_i: pairs[0].0,
                v_j: pairs[1].0,
                h_i: pairs[0].1,
                h_j: pairs[1].1,
            }
        }
        3 => {
            pairs.sort_by_key(|p| p.0);
            Isea3hCoord::Interior {
                v_i: pairs[0].0,
                v_j: pairs[1].0,
                v_k: pairs[2].0,
                h_i: pairs[0].1,
                h_j: pairs[1].1,
                h_k: pairs[2].1,
            }
        }
        _ => panic!("invalid neighbor state v={v:?} h={h:?}"),
    }
}

// --- Neighbor rules ---

const EVEN_OFFSETS: [[i32; 3]; 6] = [
    [1, -1, 0],
    [-1, 1, 0],
    [1, 0, -1],
    [-1, 0, 1],
    [0, 1, -1],
    [0, -1, 1],
];

const ODD_OFFSETS: [[i32; 3]; 6] = [
    [2, -1, -1],
    [-2, 1, 1],
    [-1, 2, -1],
    [1, -2, 1],
    [-1, -1, 2],
    [1, 1, -2],
];

fn interior_neighbors(
    v_i: u8,
    v_j: u8,
    v_k: u8,
    h_i: i32,
    h_j: i32,
    h_k: i32,
    level: u8,
) -> Vec<Isea3hCoord> {
    let v = [v_i, v_j, v_k];
    let h = [h_i, h_j, h_k];
    let offsets = if level.is_multiple_of(2) {
        &EVEN_OFFSETS[..]
    } else {
        &ODD_OFFSETS[..]
    };
    let mut out = Vec::with_capacity(6);
    for dh in offsets {
        let mut nv = v;
        let mut nh = [h[0] + dh[0], h[1] + dh[1], h[2] + dh[2]];
        if level % 2 == 1 && nh.iter().any(|&x| x < 0) {
            let lost = nh.iter().position(|&x| x < 0).expect("negative");
            let edge: [u8; 2] = [nv[(lost + 1) % 3], nv[(lost + 2) % 3]];
            let lost_v = nv[lost];
            let opposites = opposite_vertices_on_edge(edge[0], edge[1]);
            nv[lost] = if opposites[0] != lost_v {
                opposites[0]
            } else {
                opposites[1]
            };
            nh = h;
        } else if level.is_multiple_of(2) {
            for k in 0..3 {
                if nh[k] >= 0 {
                    continue;
                }
                let i = (k + 1) % 3;
                let j = (k + 2) % 3;
                if h[k] == 0 {
                    nv[k] = omega(nv[k], nv[i], nv[j]);
                    nh = [h[0] - dh[0], h[1] - dh[1], h[2] - dh[2]];
                    if nh[k] < 0 {
                        nh[k] = -nh[k];
                    }
                } else {
                    nv[k] = omega(nv[k], nv[i], nv[j]);
                    nh = h;
                }
                break;
            }
        }
        out.push(finalize_neighbor(nv, nh));
    }
    out
}

fn opposite_vertices_on_edge(v1: u8, v2: u8) -> [u8; 2] {
    let mut opposites = [0u8; 2];
    let mut count = 0usize;
    for face in ico_faces() {
        let has_v1 = face.contains(&(v1 as usize));
        let has_v2 = face.contains(&(v2 as usize));
        if has_v1 && has_v2 {
            for &vk in face {
                let vk = vk as u8;
                if vk != v1 && vk != v2 {
                    opposites[count] = vk;
                    count += 1;
                }
            }
        }
    }
    assert_eq!(count, 2, "edge {v1},{v2} has two opposite vertices");
    opposites
}

fn pentagon_neighbors(vertex: u8, level: u8) -> Vec<Isea3hCoord> {
    // Level 0 has only 12 pentagon cells; Vince even-rule neighbors would be edge
    // coords that do not exist yet. Use icosahedron vertex adjacency instead.
    if level == 0 {
        return vertex_neighbors(vertex)
            .into_iter()
            .map(|v| Isea3hCoord::Pentagon { vertex: v })
            .collect();
    }
    let m = h_hat(level);
    let n_v = vertex_neighbors(vertex);
    let mut out = Vec::with_capacity(5);
    if level.is_multiple_of(2) {
        for &v2 in &n_v {
            out.push(finalize_neighbor([vertex, v2, 0], [m - 1, 1, 0]));
        }
    } else {
        for i in 0..5 {
            let v2 = n_v[i];
            let v3 = n_v[(i + 1) % 5];
            out.push(finalize_neighbor([vertex, v2, v3], [m - 2, 1, 1]));
        }
    }
    out
}

fn edge_neighbors(v_i: u8, v_j: u8, h_i: i32, h_j: i32, level: u8) -> Vec<Isea3hCoord> {
    let v = [v_i, v_j, 0];
    let h = [h_i, h_j, 0];
    let mut out = Vec::new();
    let v_opposites = opposite_vertices_on_edge(v_i, v_j);

    let edge_ops_even: [[i32; 3]; 2] = [[-1, 0, 1], [0, -1, 1]];
    let edge_ops_odd: [[i32; 3]; 3] = [[-2, 1, 1], [1, -2, 1], [-1, -1, 2]];

    for &v3 in &v_opposites {
        let ops: &[[i32; 3]] = if level.is_multiple_of(2) {
            &edge_ops_even
        } else {
            &edge_ops_odd
        };
        for dh in ops {
            let nh = [h[0] + dh[0], h[1] + dh[1], h[2] + dh[2]];
            let nv = [v[0], v[1], v3];
            out.push(finalize_neighbor(nv, nh));
        }
    }

    if level.is_multiple_of(2) {
        out.push(finalize_neighbor(v, [h[0] + 1, h[1] - 1, h[2]]));
        out.push(finalize_neighbor(v, [h[0] - 1, h[1] + 1, h[2]]));
    }

    out
}

// --- Cell enumeration (neighbor-walk) ---

fn all_cells_cached(level: u8) -> Vec<Isea3hCoord> {
    static CACHE: OnceLock<Vec<Vec<Isea3hCoord>>> = OnceLock::new();
    let table = CACHE.get_or_init(|| {
        (0..=MAX_SUBDIVISION_LEVEL)
            .map(enumerate_cells_at_level)
            .collect()
    });
    table[level as usize].clone()
}

fn enumerate_cells_at_level(level: u8) -> Vec<Isea3hCoord> {
    let mut set = BTreeSet::new();
    let mut queue = VecDeque::new();
    for v in 0..12u8 {
        let pent = Isea3hCoord::Pentagon { vertex: v };
        if set.insert(pent) {
            queue.push_back(pent);
        }
    }
    while let Some(coord) = queue.pop_front() {
        for neighbor in coord_neighbors(coord, level) {
            if set.insert(neighbor) {
                queue.push_back(neighbor);
            }
        }
    }
    let expected = cell_count(level) as usize;
    assert_eq!(
        set.len(),
        expected,
        "neighbor-walk cell count mismatch at level {level}"
    );
    set.into_iter().collect()
}

// --- Nearest cell ---

fn nearest_cell(point: Vec3, level: u8) -> Isea3hCoord {
    let mut best = Isea3hCoord::Pentagon { vertex: 0 };
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

// --- Icosahedron geometry ---

const PHI: f64 = 1.618_033_988_749_895;

fn ico_faces() -> &'static [[usize; 3]; 20] {
    static FACES: OnceLock<[[usize; 3]; 20]> = OnceLock::new();
    FACES.get_or_init(build_ico_faces)
}

fn build_ico_faces() -> [[usize; 3]; 20] {
    let mut faces = Vec::new();
    for v_i in 0..12u8 {
        for &v_j in vertex_neighbors(v_i).iter() {
            if v_j <= v_i {
                continue;
            }
            let n_i = vertex_neighbors(v_i);
            for &v_k in n_i.iter() {
                if v_k <= v_j {
                    continue;
                }
                let n_j = vertex_neighbors(v_j);
                if n_j.contains(&v_k) {
                    faces.push([v_i as usize, v_j as usize, v_k as usize]);
                }
            }
        }
    }
    assert_eq!(faces.len(), 20);
    let mut arr = [[0usize; 3]; 20];
    for (i, f) in faces.iter().enumerate() {
        arr[i] = *f;
    }
    arr
}

fn raw_golden_ratio_vertices() -> [Vec3; 12] {
    [
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
    ]
}

const EDGE_COS: f64 = 0.447_213_595_499_957_9; // 1 / sqrt(5)

fn permutation_matches_kristensen(verts: &[Vec3; 12], perm: &[usize; 12]) -> bool {
    for v in 0..12usize {
        let pv = perm[v];
        let pa = perm[(v + 6) % 12];
        if (verts[pv].dot(verts[pa]) + 1.0).abs() > 1e-9 {
            return false;
        }
        for n in vertex_neighbors(v as u8) {
            let pn = perm[n as usize];
            if (verts[pv].dot(verts[pn]) - EDGE_COS).abs() > 1e-9 {
                return false;
            }
        }
    }
    true
}

fn partial_permutation_valid(
    verts: &[Vec3; 12],
    perm: &[usize; 12],
    assigned_through: usize,
) -> bool {
    for v in 0..=assigned_through {
        let partner = (v + 6) % 12;
        if partner < v {
            let dot = verts[perm[v]].dot(verts[perm[partner]]);
            if (dot + 1.0).abs() > 1e-9 {
                return false;
            }
        }
        for n in vertex_neighbors(v as u8) {
            let n = n as usize;
            if n < v {
                let dot = verts[perm[v]].dot(verts[perm[n]]);
                if (dot - EDGE_COS).abs() > 1e-9 {
                    return false;
                }
            }
        }
    }
    true
}

fn search_kristensen_permutation(
    verts: &[Vec3; 12],
    perm: &mut [usize; 12],
    used: &mut [bool; 12],
    kristensen_index: usize,
) -> bool {
    if kristensen_index == 12 {
        return permutation_matches_kristensen(verts, perm);
    }
    for g in 0..12 {
        if used[g] {
            continue;
        }
        perm[kristensen_index] = g;
        used[g] = true;
        if partial_permutation_valid(verts, perm, kristensen_index)
            && search_kristensen_permutation(verts, perm, used, kristensen_index + 1)
        {
            return true;
        }
        used[g] = false;
    }
    false
}

/// Permute raw golden-ratio vertex indices so `ico_vertices()[k]` matches Kristensen topology.
fn find_kristensen_permutation(verts: &[Vec3; 12]) -> [usize; 12] {
    let mut perm = [0usize; 12];
    let mut used = [false; 12];
    if search_kristensen_permutation(verts, &mut perm, &mut used, 0) {
        return perm;
    }
    panic!("no Kristensen permutation found for golden-ratio vertices");
}

fn ico_vertices() -> &'static [Vec3; 12] {
    static VERTS: OnceLock<[Vec3; 12]> = OnceLock::new();
    VERTS.get_or_init(|| {
        let raw_normalized = raw_golden_ratio_vertices().map(normalize);
        let perm = find_kristensen_permutation(&raw_normalized);

        let mut verts = [Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }; 12];
        for i in 0..12 {
            verts[i] = raw_normalized[perm[i]];
        }

        let target = lat_lon_to_vec3(58.28_f64.to_radians(), 11.25_f64.to_radians());
        rotate_vertex_to(verts, 0, target)
    })
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

// --- Vector / lat-lon helpers ---

impl Vec3 {
    pub fn dot(self, other: Vec3) -> f64 {
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

    #[test]
    fn omega_kristensen_example() {
        // Edge {7,11}: y=7, z=11 -> x'=8 per Kristensen Method 1 example
        assert_eq!(omega(3, 7, 11), 8);
    }

    #[test]
    fn build_twenty_faces() {
        assert_eq!(ico_faces().len(), 20);
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
    fn neighbor_walk_from_pentagon_zero_finds_all_cells() {
        let level = 4u8;
        let start = Isea3hCoord::Pentagon { vertex: 0 };
        let mut set = BTreeSet::new();
        let mut queue = VecDeque::new();
        set.insert(start);
        queue.push_back(start);
        while let Some(c) = queue.pop_front() {
            for n in coord_neighbors(c, level) {
                if set.insert(n) {
                    queue.push_back(n);
                }
            }
        }
        assert_eq!(set.len() as u64, cell_count(level));
    }

    #[test]
    fn exactly_twelve_pentagons_at_each_level() {
        for level in 0..=MAX_SUBDIVISION_LEVEL {
            let pent_count = all_cells(level).filter(|c| is_pentagon(*c)).count();
            assert_eq!(pent_count, 12, "level {level}");
        }
    }

    #[test]
    fn neighbor_counts_and_symmetry_levels_0_to_4() {
        for level in 0..=4u8 {
            let cells: Vec<_> = all_cells(level).collect();
            for &coord in &cells {
                let neighbors = coord_neighbors(coord, level);
                let expected = if is_pentagon(coord) { 5 } else { 6 };
                assert_eq!(neighbors.len(), expected, "level {level} {coord:?}");
                for n in neighbors {
                    assert_ne!(n, coord);
                    if level > 0 {
                        assert!(
                            !is_pentagon(coord) || !is_pentagon(n),
                            "level {level} pent-pent: {coord:?} -> {n:?}"
                        );
                    }
                    let back = coord_neighbors(n, level);
                    assert!(
                        back.contains(&coord),
                        "level {level} {coord:?} -> {n:?} missing reverse"
                    );
                }
            }
        }
    }

    #[test]
    fn pentagon_positions_stable_across_levels() {
        let anchors: Vec<LatLon> = (0..12).map(pentagon_lat_lon).collect();
        for level in 1..=MAX_SUBDIVISION_LEVEL {
            for (i, anchor) in anchors.iter().enumerate() {
                let coord = Isea3hCoord::Pentagon { vertex: i as u8 };
                let center = cell_center_lat_lon(coord, level);
                assert_approx_eq(center.lat_rad, anchor.lat_rad, 1e-12);
                assert_approx_eq(center.lon_rad, anchor.lon_rad, 1e-12);
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
    fn kristensen_permutation_is_deterministic() {
        let raw = raw_golden_ratio_vertices().map(normalize);
        let perm = find_kristensen_permutation(&raw);
        // Golden-ratio index assigned to each Kristensen vertex label.
        assert_eq!(perm, [0, 1, 8, 4, 10, 6, 3, 2, 11, 7, 9, 5]);
    }

    #[test]
    fn icosahedron_vertices_match_kristensen_topology() {
        let verts = ico_vertices();

        for v in 0..12 {
            let antipode = (v + 6) % 12;
            let dot = verts[v].dot(verts[antipode]);
            assert!(
                (dot - (-1.0)).abs() < 1e-9,
                "vertex {v} and {antipode} should be antipodes (dot = {dot})"
            );
        }

        for v in 0..12u8 {
            let neighbors = vertex_neighbors(v);
            for &n in &neighbors {
                let dot = verts[usize::from(v)].dot(verts[usize::from(n)]);
                assert!(
                    (dot - EDGE_COS).abs() < 1e-9,
                    "vertex {v} and {n} should be edge-adjacent (dot = {dot}, expected {EDGE_COS})"
                );
            }
        }
    }
}
