use std::f64::consts::PI;

use crate::grid::ids::HexId;
use crate::grid::isea3h::{self, Vec3};

/// Center of a hex as (latitude_radians, longitude_radians).
pub fn center_lat_lon(center: Vec3) -> (f64, f64) {
    let lat_lon = isea3h::vec3_to_lat_lon(center);
    (lat_lon.lat_rad, lat_lon.lon_rad)
}

/// Great-circle distance on the unit sphere in radians.
pub fn angular_distance(a: Vec3, b: Vec3) -> f64 {
    a.dot(b).clamp(-1.0, 1.0).acos()
}

/// Surface (great-circle) distance between two points on a sphere, in kilometers.
pub fn distance_km(a: Vec3, b: Vec3, planet_radius_km: f64) -> f64 {
    planet_radius_km * angular_distance(a, b)
}

/// Uniform approximate cell area: total surface area divided by cell count.
pub fn uniform_hex_area_km2(planet_radius_km: f64, cell_count: u32) -> f64 {
    let r = planet_radius_km;
    4.0 * PI * r * r / f64::from(cell_count)
}

/// Bearing (radians) of `neighbor` as seen from `origin` on the tangent plane at `origin`.
/// `0` = north, increasing clockwise when viewed from outside along `origin`.
pub fn bearing_rad(origin: Vec3, neighbor: Vec3) -> f64 {
    let north_pole = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    };
    let east = normalize(cross(north_pole, origin));
    let north = normalize(cross(origin, east));
    let to_neighbor = normalize(neighbor);
    let east_component = to_neighbor.dot(east);
    let north_component = to_neighbor.dot(north);
    east_component.atan2(north_component)
}

/// Sort `neighbors` by bearing from `origin` ascending (clockwise from north).
pub fn sort_neighbors_by_bearing(origin: Vec3, centers: &[Vec3], neighbors: &mut [HexId]) {
    neighbors.sort_by(|a, b| {
        let bearing_a = bearing_rad(origin, centers[a.0 as usize]);
        let bearing_b = bearing_rad(origin, centers[b.0 as usize]);
        bearing_a
            .partial_cmp(&bearing_b)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
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

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    Vec3 {
        x: a.y * b.z - a.z * b.y,
        y: a.z * b.x - a.x * b.z,
        z: a.x * b.y - a.y * b.x,
    }
}
