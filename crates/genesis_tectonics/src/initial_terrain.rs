//! Formation-era initial elevation and bedrock (Doc 06 §4.3 steps 5–8).

use genesis_core::data::{BedrockType, WorldData};
use genesis_core::rng::WorldRng;
use rand::Rng;

use crate::plate::{PlateRegistry, PlateType};

/// RNG stream for per-hex initial elevation variation (Doc 06 §4.4).
pub const INITIAL_ELEVATION_NOISE_STREAM: &str = "tectonics.initial_elevation_noise";

/// Mean elevation for continental plates at formation (meters).
pub const CONTINENTAL_BASE_ELEVATION_M: f32 = 500.0;

/// Mean elevation for oceanic plates at formation (meters).
pub const OCEANIC_BASE_ELEVATION_M: f32 = -3500.0;

/// Uniform noise half-range added to base elevation (±meters).
pub const INITIAL_ELEVATION_NOISE_RANGE_M: f32 = 200.0;

/// Sets per-hex elevation, bedrock, sea level, and fertility after plate generation.
pub fn apply_formation_terrain(data: &mut WorldData, registry: &PlateRegistry, rng: &WorldRng) {
    let mut noise_rng = rng.stream(INITIAL_ELEVATION_NOISE_STREAM);
    let n = data.plate_id.len();

    for i in 0..n {
        let plate_id = data.plate_id[i];
        let base = match registry.get(plate_id) {
            Some(plate) => match plate.plate_type {
                PlateType::Continental => CONTINENTAL_BASE_ELEVATION_M,
                PlateType::Oceanic => OCEANIC_BASE_ELEVATION_M,
            },
            None => OCEANIC_BASE_ELEVATION_M,
        };

        let noise: f32 =
            noise_rng.gen_range(-INITIAL_ELEVATION_NOISE_RANGE_M..=INITIAL_ELEVATION_NOISE_RANGE_M);
        data.elevation_mean[i] = base + noise;
        data.elevation_relief[i] = 0.0;

        data.bedrock_type[i] = match registry.get(plate_id).map(|p| p.plate_type) {
            Some(PlateType::Continental) => BedrockType::Igneous,
            Some(PlateType::Oceanic) => BedrockType::OceanicCrust,
            None => BedrockType::Unknown,
        };
        data.fertility[i] = 0.0;
    }

    data.sea_level_m = 0.0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::{PlateId, create_world};
    use genesis_core::parameters::WorldParameters;

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
        let registry = crate::generate_initial_plates_data(&mut world.data, &world.rng);
        apply_formation_terrain(&mut world.data, &registry, &world.rng);

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
        let registry = crate::generate_initial_plates_data(&mut world.data, &world.rng);
        apply_formation_terrain(&mut world.data, &registry, &world.rng);

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
        let registry = crate::generate_initial_plates_data(&mut world.data, &world.rng);
        apply_formation_terrain(&mut world.data, &registry, &world.rng);
        assert_eq!(world.data.sea_level_m, 0.0);
        assert!(world.data.fertility.iter().all(|&f| f == 0.0));
    }

    #[test]
    fn formation_elevation_is_deterministic() {
        let mut world_a = formation_world();
        let mut world_b = formation_world();
        let reg_a = crate::generate_initial_plates_data(&mut world_a.data, &world_a.rng);
        let reg_b = crate::generate_initial_plates_data(&mut world_b.data, &world_b.rng);
        apply_formation_terrain(&mut world_a.data, &reg_a, &world_a.rng);
        apply_formation_terrain(&mut world_b.data, &reg_b, &world_b.rng);
        assert_eq!(world_a.data.elevation_mean, world_b.data.elevation_mean);
    }
}
