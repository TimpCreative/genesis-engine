//! Soil fertility from bedrock, climate, and accumulated marine deposits
//! (Doc 01 §"fertile ancient seabed"; Doc 02 Phase 2 item 6; future Doc 08).
//!
//! Fertility is a consequence, not a declaration: sedimentary bedrock from
//! ancient seas is rich, igneous shield is poor, moisture and temperature
//! gate what the soil can support, rivers deposit alluvium, and the tectonic
//! fertility accumulator (shallow tropical seas, Doc 06 §8.4) marks the
//! Cretaceous-beach lands.

use genesis_core::HexId;
use genesis_core::data::{BedrockType, WorldData};

/// Base soil quality per bedrock type (0..1).
pub fn bedrock_base_fertility(bedrock: BedrockType) -> f32 {
    match bedrock {
        // Ancient seabeds and floodplains: the good stuff.
        BedrockType::Sedimentary => 0.75,
        BedrockType::Limestone => 0.65,
        // Weathered metamorphic supports moderate soil.
        BedrockType::Metamorphic => 0.45,
        // Igneous shield weathers slowly.
        BedrockType::Igneous => 0.30,
        BedrockType::OceanicCrust | BedrockType::Unknown => 0.0,
    }
}

/// Marine-deposit bonus weight applied to the tectonic fertility accumulator.
pub const MARINE_DEPOSIT_WEIGHT: f32 = 0.6;

/// Alluvial bonus for hexes carrying at least ten local-runoffs of discharge.
pub const ALLUVIAL_BONUS: f32 = 0.15;

/// Writes `WorldData.soil_fertility` for every hex (ocean hexes are 0).
///
/// `soil = clamp01((bedrock_base + marine_bonus + alluvial_bonus) × climate)`
/// where the climate factor peaks in warm, moist conditions and collapses in
/// deserts and permafrost. Deterministic and stateless.
pub fn compute_soil_fertility(data: &mut WorldData) {
    let n = data.cell_count() as usize;
    let sea = data.sea_level_m;
    let climate_active = data.precipitation.iter().any(|&p| p > 0.0);

    // Alluvium threshold: ten times the mean local runoff of a land hex.
    let mean_flow = {
        let mut sum = 0.0_f64;
        let mut count = 0_u64;
        for i in 0..n {
            if data.elevation_mean[i] >= sea && data.flow_volume[i] > 0.0 {
                sum += f64::from(data.flow_volume[i]);
                count += 1;
            }
        }
        if count == 0 { 0.0 } else { sum / count as f64 }
    };
    let alluvial_threshold = (mean_flow * 10.0) as f32;

    for i in 0..n {
        let hex = HexId(i as u32);
        let _ = hex;
        if data.elevation_mean[i] < sea {
            data.soil_fertility[i] = 0.0;
            continue;
        }

        let mut fertility = bedrock_base_fertility(data.bedrock_type[i]);
        fertility += data.fertility[i].clamp(0.0, 1.0) * MARINE_DEPOSIT_WEIGHT;
        if alluvial_threshold > 0.0 && data.flow_volume[i] > alluvial_threshold {
            fertility += ALLUVIAL_BONUS;
        }

        let climate_factor = if climate_active {
            moisture_factor(data.precipitation[i]) * temperature_factor(data.temperature_mean[i])
        } else {
            1.0
        };

        data.soil_fertility[i] = (fertility * climate_factor).clamp(0.0, 1.0);
    }
}

/// Moisture gate: deserts can't build soil, ~800 mm/yr is ideal, saturated
/// rainforest leaches somewhat.
fn moisture_factor(precip_mm: f32) -> f32 {
    if precip_mm <= 0.0 {
        return 0.05;
    }
    let p = precip_mm / 800.0;
    if p < 1.0 {
        (0.1 + 0.9 * p).min(1.0)
    } else {
        // Gentle leaching decline: 1.0 at 800 mm, ~0.7 at 4000 mm.
        (1.0 - (p - 1.0) * 0.075).max(0.6)
    }
}

/// Temperature gate: frozen ground locks nutrients; warm is fine.
fn temperature_factor(temp_c: f32) -> f32 {
    if temp_c <= -10.0 {
        0.05
    } else if temp_c < 5.0 {
        0.05 + (temp_c + 10.0) / 15.0 * 0.75
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::create_world;
    use genesis_core::parameters::WorldParameters;

    fn test_world() -> genesis_core::World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        create_world(params).expect("world")
    }

    fn setup_land(world: &mut genesis_core::World) {
        let n = world.data.cell_count() as usize;
        for i in 0..n {
            world.data.elevation_mean[i] = 200.0;
            world.data.bedrock_type[i] = BedrockType::Igneous;
            world.data.precipitation[i] = 800.0;
            world.data.temperature_mean[i] = 15.0;
        }
        world.data.sea_level_m = 0.0;
    }

    #[test]
    fn ancient_seabed_beats_igneous_shield() {
        let mut world = test_world();
        setup_land(&mut world);
        // Hex 1: a Cretaceous beach — sedimentary bedrock with marine deposits.
        world.data.bedrock_type[1] = BedrockType::Sedimentary;
        world.data.fertility[1] = 0.5;

        compute_soil_fertility(&mut world.data);

        assert!(
            world.data.soil_fertility[1] > world.data.soil_fertility[2] + 0.4,
            "ancient seabed ({}) should far exceed igneous shield ({})",
            world.data.soil_fertility[1],
            world.data.soil_fertility[2]
        );
    }

    #[test]
    fn desert_and_permafrost_suppress_soil() {
        let mut world = test_world();
        setup_land(&mut world);
        world.data.bedrock_type[1] = BedrockType::Sedimentary;
        world.data.bedrock_type[2] = BedrockType::Sedimentary;
        world.data.bedrock_type[3] = BedrockType::Sedimentary;
        world.data.precipitation[2] = 50.0; // desert
        world.data.temperature_mean[3] = -25.0; // permafrost

        compute_soil_fertility(&mut world.data);

        assert!(world.data.soil_fertility[2] < world.data.soil_fertility[1] * 0.4);
        assert!(world.data.soil_fertility[3] < world.data.soil_fertility[1] * 0.2);
    }

    #[test]
    fn ocean_has_no_soil_and_output_is_bounded() {
        let mut world = test_world();
        setup_land(&mut world);
        world.data.elevation_mean[0] = -2000.0;
        world.data.fertility[1] = 1.0;
        world.data.bedrock_type[1] = BedrockType::Sedimentary;

        compute_soil_fertility(&mut world.data);

        assert_eq!(world.data.soil_fertility[0], 0.0);
        for i in 0..world.data.cell_count() as usize {
            let s = world.data.soil_fertility[i];
            assert!((0.0..=1.0).contains(&s), "soil {s} out of range at {i}");
        }
    }
}
