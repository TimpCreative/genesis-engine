//! Formation-era initial elevation and bedrock (Doc 06 §4.3 steps 5–8).

use genesis_core::data::{BedrockType, WorldData};
use genesis_core::rng::WorldRng;
use genesis_core::{HexId, PlateId};
use rand::Rng;

use crate::plate::{PlateRegistry, PlateType};
use crate::plate_surface::SurfaceFeature;
use crate::world_rebuild::rebuild_world_from_plate_surfaces;

/// RNG stream for per-hex initial elevation variation (Doc 06 §4.4).
pub const INITIAL_ELEVATION_NOISE_STREAM: &str = "tectonics.initial_elevation_noise";

/// Mean elevation for continental plates at formation (meters).
pub const CONTINENTAL_BASE_ELEVATION_M: f32 = 800.0;

/// Mean elevation for oceanic plates at formation (meters).
pub const OCEANIC_BASE_ELEVATION_M: f32 = -4000.0;

/// Long-wavelength noise amplitude (m): regional highlands, plateaus, lowlands,
/// and submerged continental margins.
pub const COARSE_NOISE_AMPLITUDE_M: f32 = 900.0;

/// Medium-wavelength noise amplitude (m): hills and basins.
pub const MEDIUM_NOISE_AMPLITUDE_M: f32 = 300.0;

/// Per-hex noise amplitude (m): local texture.
pub const FINE_NOISE_AMPLITUDE_M: f32 = 60.0;

/// Neighbor-averaging passes for the coarse octave AT THE REFERENCE LEVEL.
const COARSE_SMOOTHING_PASSES: u32 = 6;

/// Neighbor-averaging passes for the medium octave AT THE REFERENCE LEVEL.
const MEDIUM_SMOOTHING_PASSES: u32 = 2;

/// Subdivision level the pass counts are calibrated for.
const NOISE_REFERENCE_LEVEL: i32 = 7;

/// Scales smoothing passes so the noise's PHYSICAL wavelength is the same at
/// every subdivision level. Smoothing diffuses ~sqrt(passes) hexes; hex
/// spacing shrinks by sqrt(3) per level (3x cells), so passes must scale by
/// 3x per level. Without this, level-8 worlds get "regional" noise at local
/// wavelengths — continents peppered with small inland seas.
fn smoothing_passes_for_level(base: u32, subdivision_level: u8) -> u32 {
    let delta = subdivision_level as i32 - NOISE_REFERENCE_LEVEL;
    let factor = 3.0_f64.powi(delta);
    ((f64::from(base) * factor).round() as u32).max(1)
}

/// Populates plate surfaces with initial terrain, then rebuilds `WorldData`.
pub fn apply_formation_terrain(data: &mut WorldData, registry: &mut PlateRegistry, rng: &WorldRng) {
    let noise = correlated_elevation_noise(data, rng);

    for (i, &hex_noise) in noise.iter().enumerate() {
        let hex = HexId(i as u32);
        let plate_id = data.plate_id[i];
        if plate_id == PlateId::NONE {
            continue;
        }

        let plate_type = {
            let Some(plate) = registry.get(plate_id) else {
                continue;
            };
            plate.plate_type
        };

        let Some(plate) = registry.plates_mut().get_mut(&plate_id) else {
            continue;
        };

        let (base, bedrock) = match plate_type {
            PlateType::Continental => (CONTINENTAL_BASE_ELEVATION_M, BedrockType::Igneous),
            PlateType::Oceanic => (OCEANIC_BASE_ELEVATION_M, BedrockType::OceanicCrust),
        };

        // At year 0, birth hex == world hex (zero accumulated rotation).
        plate.surface.set(
            hex,
            SurfaceFeature {
                elevation_m: base + hex_noise,
                relief_m: 0.0,
                bedrock,
                fertility: 0.0,
                age_year: 0,
                continental_crust: plate_type == PlateType::Continental,
                root_m: 0.0,
            },
        );
    }

    seed_fossil_basement_belts(data, registry, rng);

    data.sea_level_m = 0.0;
    rebuild_world_from_plate_surfaces(data, registry);
}

/// RNG stream for the fossil basement belts (fixed draw order — seed-determined).
const FOSSIL_BELT_STREAM: &str = "fossil-basement-belts";
/// Fossil-root amplitude on the belt axis, m. Ancient basement highlands
/// stand 500–1500 m on Earth (Guiana shield, Baltic/Canadian shield highs),
/// so the axis banks the middle of that band and calibration maps it into
/// the highland mid-band, well below the active orogenic tail.
const FOSSIL_ROOT_AXIS_M: f32 = 900.0;
/// Neighbor-halo root as a fraction of the axis value.
const FOSSIL_ROOT_HALO_FRACTION: f32 = 0.5;
/// Initial surface stands this factor above the root floor — an eroded
/// ancient belt, not a fresh range.
const FOSSIL_SURFACE_OVER_ROOT: f32 = 1.15;
/// Minimum continental plate size (hexes) that receives fossil belts.
const FOSSIL_BELT_MIN_PLATE_HEXES: usize = 8;

/// Doc 06 §5.2 roots: continents are palimpsests, not blank slates. Every
/// formation continent is seeded with 1–2 ancient basement belts — wandering
/// bands of permanent crustal root (plus matching surface) recording the
/// orogenies that assembled it before year 0. Deterministic per world seed:
/// one dedicated stream, plates visited in ascending id order.
fn seed_fossil_basement_belts(data: &WorldData, registry: &mut PlateRegistry, rng: &WorldRng) {
    let mut belt_rng = rng.stream(FOSSIL_BELT_STREAM);
    for plate_id in registry.plate_ids() {
        let hexes: Vec<u32> = (0..data.plate_id.len() as u32)
            .filter(|&i| data.plate_id[i as usize] == plate_id)
            .collect();
        let Some(plate) = registry.plates_mut().get_mut(&plate_id) else {
            continue;
        };
        if plate.plate_type != PlateType::Continental || hexes.len() < FOSSIL_BELT_MIN_PLATE_HEXES {
            continue;
        }
        // Belt count scales with plate area (Earth basement maps carry a
        // belt every few hundred km): a fragment rifting off a large plate
        // should usually inherit at least one.
        let belts = (hexes.len() / 120).clamp(1, 6) + belt_rng.gen_range(0..=1usize);
        for _ in 0..belts {
            let start = hexes[belt_rng.gen_range(0..hexes.len())];
            let mut dir = belt_rng.gen_range(0..6usize);
            let length = ((hexes.len() as f32).sqrt() as usize).clamp(3, 24);
            let mut current = start;
            for _ in 0..length {
                let axis = FOSSIL_ROOT_AXIS_M * belt_rng.gen_range(0.75..=1.25);
                bank_fossil_root(plate, HexId(current), axis);
                let neighbors = data.grid.neighbors(HexId(current));
                for &neighbor in neighbors {
                    if data
                        .plate_id
                        .get(neighbor.0 as usize)
                        .is_some_and(|&p| p == plate_id)
                    {
                        bank_fossil_root(plate, neighbor, axis * FOSSIL_ROOT_HALO_FRACTION);
                    }
                }
                // Ancient belts curve: wobble the heading now and then.
                if belt_rng.gen_bool(0.35) {
                    dir = (dir + if belt_rng.gen_bool(0.5) { 1 } else { 5 }) % 6;
                }
                let Some(&next) = neighbors.get(dir % neighbors.len().max(1)) else {
                    break;
                };
                if data
                    .plate_id
                    .get(next.0 as usize)
                    .is_none_or(|&p| p != plate_id)
                {
                    break; // the belt ends at the continent edge.
                }
                current = next.0;
            }
        }
    }
}

/// Banks a fossil root on one feature (max, not sum — overlapping halo
/// visits must not stack a plateau into a false Himalaya) and stands the
/// initial surface just above the new floor.
fn bank_fossil_root(plate: &mut crate::plate::Plate, hex: HexId, root_m: f32) {
    let Some(feature) = plate
        .surface
        .features
        .get_mut(hex.0 as usize)
        .and_then(|f| f.as_mut())
    else {
        return;
    };
    if !feature.continental_crust {
        return;
    }
    feature.root_m = feature
        .root_m
        .max(root_m)
        .min(crate::elevation::ROOT_MAX_M);
    let min_surface = CONTINENTAL_BASE_ELEVATION_M + feature.root_m * FOSSIL_SURFACE_OVER_ROOT;
    if feature.elevation_m < min_surface {
        feature.elevation_m = min_surface;
    }
}

/// Multi-octave, spatially correlated elevation noise.
///
/// Uncorrelated per-hex noise reads as salt-and-pepper speckle; summing a
/// heavily smoothed octave (regional highlands/lowlands), a lightly smoothed
/// octave (hills and basins), and raw texture yields natural-looking terrain
/// variation. Deterministic: one dedicated RNG stream, fixed draw order,
/// neighbor sums in ascending `HexId` order.
fn correlated_elevation_noise(data: &WorldData, rng: &WorldRng) -> Vec<f32> {
    let n = data.plate_id.len();
    let mut noise_rng = rng.stream(INITIAL_ELEVATION_NOISE_STREAM);

    let coarse_white: Vec<f32> = (0..n).map(|_| noise_rng.gen_range(-1.0f32..=1.0)).collect();
    let fine_white: Vec<f32> = (0..n).map(|_| noise_rng.gen_range(-1.0f32..=1.0)).collect();

    let level = data.grid.subdivision_level();
    let coarse_passes = smoothing_passes_for_level(COARSE_SMOOTHING_PASSES, level);
    let medium_passes = smoothing_passes_for_level(MEDIUM_SMOOTHING_PASSES, level);
    let coarse = normalized(smoothed(&coarse_white, data, coarse_passes));
    let medium = normalized(smoothed(&fine_white, data, medium_passes));

    let combined: Vec<f32> = (0..n)
        .map(|i| {
            coarse[i] * COARSE_NOISE_AMPLITUDE_M
                + medium[i] * MEDIUM_NOISE_AMPLITUDE_M
                + fine_white[i] * FINE_NOISE_AMPLITUDE_M
        })
        .collect();
    fill_single_hex_pits(&combined, data)
}

/// Elevation drop below the lowest neighbor above which a lone hex counts as
/// a noise pit rather than terrain (m).
const PIT_DEPTH_THRESHOLD_M: f32 = 150.0;

/// Residual depth left when a pit is raised to blend with its surroundings (m).
const PIT_FILL_MARGIN_M: f32 = 50.0;

/// Raises single-hex noise pits to just below their lowest neighbor. Lone
/// deep dips render as near-black one-hex "holes" inside continents; real
/// basins are multi-hex and survive this pass untouched.
fn fill_single_hex_pits(noise: &[f32], data: &WorldData) -> Vec<f32> {
    let grid = &data.grid;
    let n = noise.len();
    let mut out = noise.to_vec();
    for (i, value) in out.iter_mut().enumerate() {
        let hex = HexId(i as u32);
        let mut min_neighbor = f32::MAX;
        for neighbor in grid.neighbors(hex) {
            let j = neighbor.0 as usize;
            if j < n {
                min_neighbor = min_neighbor.min(noise[j]);
            }
        }
        if min_neighbor < f32::MAX && *value < min_neighbor - PIT_DEPTH_THRESHOLD_M {
            *value = min_neighbor - PIT_FILL_MARGIN_M;
        }
    }
    out
}

/// Repeated neighbor averaging: each pass replaces every value with the mean
/// of itself and its neighbors.
fn smoothed(values: &[f32], data: &WorldData, passes: u32) -> Vec<f32> {
    let grid = &data.grid;
    let n = values.len();
    let mut current = values.to_vec();
    let mut next = vec![0.0f32; n];

    for _ in 0..passes {
        for (i, out) in next.iter_mut().enumerate() {
            let hex = HexId(i as u32);
            let mut sum = f64::from(current[i]);
            let mut count = 1u32;
            for neighbor in grid.neighbors(hex) {
                let j = neighbor.0 as usize;
                if j < n {
                    sum += f64::from(current[j]);
                    count += 1;
                }
            }
            *out = (sum / f64::from(count)) as f32;
        }
        std::mem::swap(&mut current, &mut next);
    }
    current
}

/// Rescales so the maximum absolute value is 1 (no-op for an all-zero field).
fn normalized(values: Vec<f32>) -> Vec<f32> {
    let max_abs = values.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if max_abs <= f32::EPSILON {
        return values;
    }
    values.into_iter().map(|v| v / max_abs).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{PlateId, create_world};

    fn formation_world() -> genesis_core::World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        create_world(params).expect("valid world")
    }

    fn mean_elevation_for_type(
        data: &WorldData,
        registry: &PlateRegistry,
        plate_type: PlateType,
    ) -> f32 {
        let mut sum = 0.0_f64;
        let mut count = 0_u64;
        for (i, &plate_id) in data.plate_id.iter().enumerate() {
            if plate_id == PlateId::NONE {
                continue;
            }
            let Some(plate) = registry.get(plate_id) else {
                continue;
            };
            if plate.plate_type != plate_type {
                continue;
            }
            sum += f64::from(data.elevation_mean[i]);
            count += 1;
        }
        if count == 0 {
            0.0
        } else {
            (sum / count as f64) as f32
        }
    }

    #[test]
    fn continental_mean_elevation_above_oceanic() {
        let mut world = formation_world();
        let mut registry = crate::generate_initial_plates_data(&mut world.data, &world.rng);
        apply_formation_terrain(&mut world.data, &mut registry, &world.rng);

        let continental = mean_elevation_for_type(&world.data, &registry, PlateType::Continental);
        let oceanic = mean_elevation_for_type(&world.data, &registry, PlateType::Oceanic);
        assert!(
            continental > oceanic,
            "continental mean {continental} should exceed oceanic mean {oceanic}"
        );
    }

    #[test]
    fn bedrock_matches_plate_type() {
        let mut world = formation_world();
        let mut registry = crate::generate_initial_plates_data(&mut world.data, &world.rng);
        apply_formation_terrain(&mut world.data, &mut registry, &world.rng);

        for (i, &plate_id) in world.data.plate_id.iter().enumerate() {
            if plate_id == PlateId::NONE {
                continue;
            }
            let plate = registry.get(plate_id).expect("plate");
            let expected = match plate.plate_type {
                PlateType::Continental => BedrockType::Igneous,
                PlateType::Oceanic => BedrockType::OceanicCrust,
            };
            assert_eq!(
                world.data.bedrock_type[i], expected,
                "hex {i} plate {:?}",
                plate.plate_type
            );
        }
    }

    #[test]
    fn sea_level_and_fertility_zeroed() {
        let mut world = formation_world();
        let mut registry = crate::generate_initial_plates_data(&mut world.data, &world.rng);
        apply_formation_terrain(&mut world.data, &mut registry, &world.rng);
        assert_eq!(world.data.sea_level_m, 0.0);
        assert!(world.data.fertility.iter().all(|&f| f == 0.0));
    }

    #[test]
    fn formation_populates_plate_surfaces() {
        let mut world = formation_world();
        let mut registry = crate::generate_initial_plates_data(&mut world.data, &world.rng);
        apply_formation_terrain(&mut world.data, &mut registry, &world.rng);

        for (i, &plate_id) in world.data.plate_id.iter().enumerate() {
            if plate_id == PlateId::NONE {
                continue;
            }
            let plate = registry.get(plate_id).expect("plate");
            let hex = HexId(i as u32);
            let feature = plate
                .surface
                .get(hex)
                .expect("surface feature for assigned hex");
            assert_eq!(feature.age_year, 0);
        }
    }

    #[test]
    fn formation_elevation_is_deterministic() {
        let mut world_a = formation_world();
        let mut world_b = formation_world();
        let mut reg_a = crate::generate_initial_plates_data(&mut world_a.data, &world_a.rng);
        let mut reg_b = crate::generate_initial_plates_data(&mut world_b.data, &world_b.rng);
        apply_formation_terrain(&mut world_a.data, &mut reg_a, &world_a.rng);
        apply_formation_terrain(&mut world_b.data, &mut reg_b, &world_b.rng);
        assert_eq!(world_a.data.elevation_mean, world_b.data.elevation_mean);
    }
}
