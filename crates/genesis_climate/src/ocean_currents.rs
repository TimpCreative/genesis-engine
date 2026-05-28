//! Ocean surface currents (Doc 07 §8).
//!
//! Heuristic gyre model: for each large basin, compute tangential currents
//! relative to the basin's centroid with Coriolis deflection and equatorial
//! trade-wind driven westward bias. NOT a fluid dynamics simulation.

use std::collections::BTreeMap;

use genesis_core::HexId;
use genesis_core::data::{BasinId, WorldData};

use crate::state::{OceanBasin, OceanBasins};

/// Minimum basin size (hex count) to compute gyre currents.
pub const MIN_BASIN_SIZE_FOR_GYRE: u32 = 50;

/// Maximum current speed in m/s.
pub const MAX_CURRENT_SPEED_M_S: f32 = 1.5;

/// Equatorial westward bias strength in m/s.
pub const EQUATORIAL_BIAS_M_S: f32 = 0.5;

/// Latitude range (radians) over which the equatorial bias applies.
pub const EQUATORIAL_BIAS_LAT_HALFWIDTH_RAD: f64 = std::f64::consts::FRAC_PI_6;

/// Computes ocean surface currents for every hex.
///
/// Writes `WorldData.ocean_current_vec` (east, north components in m/s).
pub fn compute_ocean_currents(data: &mut WorldData, basins: &OceanBasins) {
    let n = data.cell_count() as usize;
    let grid = &data.grid;

    for i in 0..n {
        data.ocean_current_vec[i] = [0.0, 0.0];
    }

    let mut basin_by_id: BTreeMap<BasinId, &OceanBasin> = BTreeMap::new();
    for basin in &basins.basins {
        if basin.hex_count >= MIN_BASIN_SIZE_FOR_GYRE && !basin.is_inland {
            basin_by_id.insert(basin.id, basin);
        }
    }

    for i in 0..n {
        let hex = HexId(i as u32);
        let basin_id = data.basin_id[i];

        if basin_id == BasinId::NONE {
            continue;
        }

        let (lat_rad, _lon_rad) = grid.center_lat_lon(hex);

        let gyre_vec = if let Some(basin) = basin_by_id.get(&basin_id) {
            gyre_tangent_at_hex(grid, hex, basin, lat_rad)
        } else {
            [0.0, 0.0]
        };

        let bias_vec = equatorial_bias_vector(lat_rad);

        let combined_east = gyre_vec[0] + bias_vec[0];
        let combined_north = gyre_vec[1] + bias_vec[1];
        let magnitude = (combined_east * combined_east + combined_north * combined_north).sqrt();
        let scale = if magnitude > MAX_CURRENT_SPEED_M_S {
            MAX_CURRENT_SPEED_M_S / magnitude
        } else {
            1.0
        };
        data.ocean_current_vec[i] = [combined_east * scale, combined_north * scale];
        let [e, n] = data.ocean_current_vec[i];
        let speed = (e * e + n * n).sqrt();
        if speed > MAX_CURRENT_SPEED_M_S {
            let fix = MAX_CURRENT_SPEED_M_S / speed;
            data.ocean_current_vec[i] = [e * fix, n * fix];
        }
    }
}

/// Computes coastal temperature adjustments from ocean currents (Doc 07 §4.5, §8.3).
pub fn compute_coastal_temperature_adjustments(data: &WorldData) -> BTreeMap<HexId, f32> {
    let n = data.cell_count() as usize;
    let grid = &data.grid;
    let sea_level = data.sea_level_m;
    let mut adjustments = BTreeMap::new();

    for i in 0..n {
        if data.elevation_mean[i] < sea_level {
            continue;
        }

        let hex = HexId(i as u32);
        let neighbors = grid.neighbors(hex);

        let mut adjustment_sum = 0.0_f32;
        let mut neighbor_count = 0;

        for &neighbor in neighbors {
            let n_idx = neighbor.0 as usize;
            if data.elevation_mean[n_idx] >= sea_level {
                continue;
            }

            let current = data.ocean_current_vec[n_idx];
            let current_strength = (current[0] * current[0] + current[1] * current[1]).sqrt();

            if current_strength < 0.05 {
                continue;
            }

            let Some(upstream_hex) = find_upstream_neighbor(grid, neighbor, current) else {
                continue;
            };
            let upstream_idx = upstream_hex.0 as usize;

            if data.elevation_mean[upstream_idx] >= sea_level {
                continue;
            }

            let upstream_temp = data.temperature_mean[upstream_idx];
            let local_temp = data.temperature_mean[n_idx];
            let anomaly = upstream_temp - local_temp;

            let falloff = (current_strength / MAX_CURRENT_SPEED_M_S).min(1.0);
            adjustment_sum += anomaly * falloff * 0.5;
            neighbor_count += 1;
        }

        if neighbor_count > 0 {
            let avg = adjustment_sum / neighbor_count as f32;
            if avg.abs() > 0.1 {
                adjustments.insert(hex, avg);
            }
        }
    }

    adjustments
}

fn gyre_tangent_at_hex(
    grid: &genesis_core::HexGrid,
    hex: HexId,
    basin: &OceanBasin,
    lat_rad: f64,
) -> [f32; 2] {
    let hex_pos = grid.cell_center_direction(hex);
    let centroid_pos = grid.cell_center_direction(basin.centroid_hex);

    let to_hex = [
        hex_pos[0] - centroid_pos[0],
        hex_pos[1] - centroid_pos[1],
        hex_pos[2] - centroid_pos[2],
    ];

    let dot = to_hex[0] * hex_pos[0] + to_hex[1] * hex_pos[1] + to_hex[2] * hex_pos[2];
    let to_hex_tangent = [
        to_hex[0] - dot * hex_pos[0],
        to_hex[1] - dot * hex_pos[1],
        to_hex[2] - dot * hex_pos[2],
    ];

    let north_pole = [0.0_f64, 0.0, 1.0];
    let east = normalize_cross(north_pole, hex_pos);
    let north = normalize_cross(hex_pos, east);

    let east_component =
        to_hex_tangent[0] * east[0] + to_hex_tangent[1] * east[1] + to_hex_tangent[2] * east[2];
    let north_component =
        to_hex_tangent[0] * north[0] + to_hex_tangent[1] * north[1] + to_hex_tangent[2] * north[2];

    let (tangent_east, tangent_north) = if lat_rad >= 0.0 {
        (north_component, -east_component)
    } else {
        (-north_component, east_component)
    };

    let mag = (tangent_east * tangent_east + tangent_north * tangent_north).sqrt();
    if mag < 1e-9 {
        return [0.0, 0.0];
    }

    let angular_dist = (hex_pos[0] * centroid_pos[0]
        + hex_pos[1] * centroid_pos[1]
        + hex_pos[2] * centroid_pos[2])
        .acos();

    let basin_extent = (basin.lat_max_rad - basin.lat_min_rad) / 2.0;
    let dist_factor = (angular_dist / basin_extent.max(0.1)).clamp(0.0, 1.0) as f32;

    let scale = MAX_CURRENT_SPEED_M_S * 0.7 * dist_factor;

    [
        (tangent_east / mag) as f32 * scale,
        (tangent_north / mag) as f32 * scale,
    ]
}

fn equatorial_bias_vector(lat_rad: f64) -> [f32; 2] {
    let abs_lat = lat_rad.abs();
    if abs_lat > EQUATORIAL_BIAS_LAT_HALFWIDTH_RAD {
        return [0.0, 0.0];
    }
    let strength =
        (abs_lat / EQUATORIAL_BIAS_LAT_HALFWIDTH_RAD * std::f64::consts::FRAC_PI_2).cos();
    let westward = -EQUATORIAL_BIAS_M_S * strength as f32;
    [westward, 0.0]
}

fn find_upstream_neighbor(
    grid: &genesis_core::HexGrid,
    hex: HexId,
    current: [f32; 2],
) -> Option<HexId> {
    let upstream_dir_east = -f64::from(current[0]);
    let upstream_dir_north = -f64::from(current[1]);
    let mag =
        (upstream_dir_east * upstream_dir_east + upstream_dir_north * upstream_dir_north).sqrt();
    if mag < 1e-9 {
        return None;
    }
    let upstream_dir = (upstream_dir_east / mag, upstream_dir_north / mag);

    let hex_pos = grid.cell_center_direction(hex);
    let north_pole = [0.0_f64, 0.0, 1.0];
    let east = normalize_cross(north_pole, hex_pos);
    let north = normalize_cross(hex_pos, east);

    let mut best: Option<HexId> = None;
    let mut best_alignment = -1.0_f64;

    for &neighbor in grid.neighbors(hex) {
        let n_pos = grid.cell_center_direction(neighbor);
        let to_n = [
            n_pos[0] - hex_pos[0],
            n_pos[1] - hex_pos[1],
            n_pos[2] - hex_pos[2],
        ];
        let east_comp = to_n[0] * east[0] + to_n[1] * east[1] + to_n[2] * east[2];
        let north_comp = to_n[0] * north[0] + to_n[1] * north[1] + to_n[2] * north[2];
        let mag2 = (east_comp * east_comp + north_comp * north_comp).sqrt();
        if mag2 < 1e-9 {
            continue;
        }

        let alignment = (east_comp / mag2) * upstream_dir.0 + (north_comp / mag2) * upstream_dir.1;
        if alignment > best_alignment {
            best_alignment = alignment;
            best = Some(neighbor);
        }
    }

    best
}

fn normalize_cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    let cross = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let mag = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
    if mag < 1e-9 {
        [1.0, 0.0, 0.0]
    } else {
        [cross[0] / mag, cross[1] / mag, cross[2] / mag]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::create_world;
    use genesis_core::parameters::WorldParameters;

    fn world_at_level(level: u8) -> genesis_core::World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = level;
        create_world(params).expect("world")
    }

    #[test]
    fn empty_basins_produces_zero_currents() {
        let mut world = world_at_level(5);
        let basins = OceanBasins::default();

        for i in 0..world.data.cell_count() as usize {
            world.data.elevation_mean[i] = -2000.0;
            world.data.basin_id[i] = BasinId::NONE;
        }

        compute_ocean_currents(&mut world.data, &basins);

        for &v in &world.data.ocean_current_vec {
            assert_eq!(v, [0.0, 0.0]);
        }
    }

    #[test]
    fn land_hexes_have_zero_current() {
        let mut world = world_at_level(5);
        let mut basins = OceanBasins::default();
        basins.basins.push(OceanBasin {
            id: BasinId(0),
            centroid_hex: HexId(0),
            hex_count: 1000,
            lat_min_rad: -1.0,
            lat_max_rad: 1.0,
            is_inland: false,
        });

        for i in 0..world.data.cell_count() as usize {
            world.data.elevation_mean[i] = 500.0;
            world.data.basin_id[i] = BasinId::NONE;
        }

        compute_ocean_currents(&mut world.data, &basins);

        for &v in &world.data.ocean_current_vec {
            assert_eq!(v, [0.0, 0.0]);
        }
    }

    #[test]
    fn equatorial_bias_only_in_tropics() {
        let tropical = equatorial_bias_vector(0.1);
        let subtropical = equatorial_bias_vector(0.7);
        let polar = equatorial_bias_vector(1.4);

        assert!(tropical[0] < 0.0, "equator should have westward bias");
        assert_eq!(subtropical, [0.0, 0.0]);
        assert_eq!(polar, [0.0, 0.0]);
    }

    #[test]
    fn equatorial_bias_strongest_at_equator() {
        let at_equator = equatorial_bias_vector(0.0);
        let near_edge = equatorial_bias_vector(EQUATORIAL_BIAS_LAT_HALFWIDTH_RAD * 0.9);

        assert!(at_equator[0].abs() > near_edge[0].abs());
    }

    #[test]
    fn small_basins_get_no_gyre_just_equatorial_bias() {
        let mut world = world_at_level(5);
        let mut basins = OceanBasins::default();
        basins.basins.push(OceanBasin {
            id: BasinId(0),
            centroid_hex: HexId(0),
            hex_count: 10,
            lat_min_rad: 0.0,
            lat_max_rad: 0.2,
            is_inland: false,
        });

        for i in 0..world.data.cell_count() as usize {
            world.data.elevation_mean[i] = -2000.0;
            world.data.basin_id[i] = BasinId(0);
        }

        compute_ocean_currents(&mut world.data, &basins);

        for i in 0..world.data.cell_count() as usize {
            let (lat_rad, _lon_rad) = world.data.grid.center_lat_lon(HexId(i as u32));
            let current = world.data.ocean_current_vec[i];

            if lat_rad.abs() < EQUATORIAL_BIAS_LAT_HALFWIDTH_RAD {
                assert!(
                    current[0] <= 0.0,
                    "tropical hex should have westward or zero current"
                );
                assert_eq!(
                    current[1], 0.0,
                    "small basin should not produce north/south current"
                );
            } else {
                assert_eq!(current, [0.0, 0.0]);
            }
        }
    }

    #[test]
    fn currents_clamped_to_max_speed() {
        let mut world = world_at_level(5);
        let mut basins = OceanBasins::default();
        basins.basins.push(OceanBasin {
            id: BasinId(0),
            centroid_hex: HexId(0),
            hex_count: 1000,
            lat_min_rad: -std::f64::consts::FRAC_PI_2,
            lat_max_rad: std::f64::consts::FRAC_PI_2,
            is_inland: false,
        });

        for i in 0..world.data.cell_count() as usize {
            world.data.elevation_mean[i] = -2000.0;
            world.data.basin_id[i] = BasinId(0);
        }

        compute_ocean_currents(&mut world.data, &basins);

        for &[e, n] in &world.data.ocean_current_vec {
            let magnitude = (e * e + n * n).sqrt();
            assert!(magnitude <= MAX_CURRENT_SPEED_M_S);
        }
    }

    #[test]
    fn performance_at_level_7_under_50ms() {
        use std::time::Instant;

        let mut world = world_at_level(7);
        let mut basins = OceanBasins::default();
        basins.basins.push(OceanBasin {
            id: BasinId(0),
            centroid_hex: HexId(0),
            hex_count: 15_000,
            lat_min_rad: -1.5,
            lat_max_rad: 1.5,
            is_inland: false,
        });

        for i in 0..world.data.cell_count() as usize {
            world.data.elevation_mean[i] = -2000.0;
            world.data.basin_id[i] = BasinId(0);
        }

        let start = Instant::now();
        compute_ocean_currents(&mut world.data, &basins);
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 50,
            "ocean currents took {}ms at level 7; should be under 50ms",
            elapsed.as_millis()
        );
    }

    #[test]
    fn determinism() {
        let mut world_a = world_at_level(5);
        let mut world_b = world_at_level(5);
        let mut basins = OceanBasins::default();
        basins.basins.push(OceanBasin {
            id: BasinId(0),
            centroid_hex: HexId(0),
            hex_count: 1000,
            lat_min_rad: -1.0,
            lat_max_rad: 1.0,
            is_inland: false,
        });

        for i in 0..world_a.data.cell_count() as usize {
            world_a.data.elevation_mean[i] = -2000.0;
            world_a.data.basin_id[i] = BasinId(0);
            world_b.data.elevation_mean[i] = -2000.0;
            world_b.data.basin_id[i] = BasinId(0);
        }

        compute_ocean_currents(&mut world_a.data, &basins);
        compute_ocean_currents(&mut world_b.data, &basins);

        assert_eq!(
            world_a.data.ocean_current_vec,
            world_b.data.ocean_current_vec
        );
    }
}
