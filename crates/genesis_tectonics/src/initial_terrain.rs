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
            },
        );
    }

    data.sea_level_m = 0.0;
    rebuild_world_from_plate_surfaces(data, registry);
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
