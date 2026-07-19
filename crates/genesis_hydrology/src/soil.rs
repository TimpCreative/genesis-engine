//! Soil depth, class, and fertility (Doc 08 §10).

use genesis_core::data::{
    BedrockType, HydroFlags, SoilClass, WaterBodyId, WaterBodyKind, WorldData,
};

use crate::routing::RoutingSurface;

/// Weathering rate baseline, m/yr (§10.1).
pub const WEATHERING_RATE_M_PER_YEAR: f64 = 1.0e-5;
/// Maximum soil depth, meters.
pub const SOIL_DEPTH_MAX_M: f32 = 50.0;
/// Arid precipitation threshold for Sandy soils, mm/yr.
pub const SANDY_PRECIP_MAX_MM: f32 = 250.0;
/// Cold threshold for Peaty wetlands, °C.
pub const PEATY_TEMP_MAX_C: f32 = 5.0;

/// Updates soil depth, class, wetlands, and fertility (§10).
pub fn update_soil(
    data: &mut WorldData,
    surface: &RoutingSurface,
    alluvium_depth_m: &[f32],
    tick_years: f64,
) {
    let n = data.cell_count() as usize;
    for i in 0..n {
        if data.water_body_id[i] != WaterBodyId::NONE || data.ice_mask[i] {
            data.soil_class[i] = SoilClass::None;
            data.soil_depth_m[i] = 0.0;
            data.soil_fertility[i] = 0.0;
            continue;
        }

        // Depth: weathering + alluvium − |erosion delta| (erosion already in delta).
        let weather = WEATHERING_RATE_M_PER_YEAR
            * bedrock_weather_mult(data.bedrock_type[i])
            * climate_weather_mult(data.precipitation[i], data.temperature_mean[i])
            * tick_years;
        let alluv = f64::from(alluvium_depth_m.get(i).copied().unwrap_or(0.0));
        let eroded = (-data.hydro_elevation_delta_m[i]).max(0.0);
        let mut depth =
            f64::from(data.soil_depth_m[i]) + weather + alluv * 0.01 - f64::from(eroded) * 0.1;
        depth = depth.clamp(0.0, f64::from(SOIL_DEPTH_MAX_M));
        data.soil_depth_m[i] = depth as f32;

        // Class decision tree (§10.1 priority).
        data.soil_class[i] = classify_soil(data, surface, i, alluv);

        // Wetlands (§10.2).
        if is_wetland(data, surface, i) {
            data.hydro_flags[i] |= HydroFlags::WETLAND;
            if matches!(data.soil_class[i], SoilClass::Loamy | SoilClass::Sandy) {
                data.soil_class[i] = SoilClass::Peaty;
            }
        }

        // Fertility blend (§10.3).
        data.soil_fertility[i] = soil_fertility(data, i);
    }
}

fn bedrock_weather_mult(bedrock: BedrockType) -> f64 {
    match bedrock {
        BedrockType::Sedimentary | BedrockType::Limestone => 1.5,
        BedrockType::Igneous => 0.5,
        BedrockType::Metamorphic => 0.7,
        BedrockType::OceanicCrust => 0.0,
        BedrockType::Unknown => 1.0,
    }
}

fn climate_weather_mult(precip_mm: f32, temp_c: f32) -> f64 {
    let p = f64::from(precip_mm / 800.0).clamp(0.2, 2.0);
    let t = if temp_c < 0.0 { 0.3 } else { 1.0 };
    p * t
}

fn classify_soil(data: &WorldData, surface: &RoutingSurface, i: usize, alluv: f64) -> SoilClass {
    // Salt flats / accumulated salt dominate (§10.1 / §5).
    let on_salt_flat = data
        .water_bodies
        .get(&data.water_body_id[i])
        .is_some_and(|b| b.kind == WaterBodyKind::SaltFlat)
        || (data.salt_accumulated[i] > 1.0 && data.water_body_id[i] == WaterBodyId::NONE);
    if on_salt_flat {
        return SoilClass::Saline;
    }
    if data.soil_class[i] == SoilClass::Loess {
        return SoilClass::Loess; // persistent once lofted (§9.2)
    }
    if alluv > 1.0 {
        return SoilClass::Alluvial;
    }
    if data.bedrock_type[i] == BedrockType::Igneous && data.elevation_relief[i] > 500.0 {
        return SoilClass::Volcanic;
    }
    if matches!(
        data.bedrock_type[i],
        BedrockType::Limestone | BedrockType::Sedimentary
    ) || data.fertility[i] > 0.3
    {
        return SoilClass::Calcareous;
    }
    if data.temperature_mean[i] < PEATY_TEMP_MAX_C
        && data.precipitation[i] > 600.0
        && channel_slope(data, surface, i) < 0.001
    {
        return SoilClass::Peaty;
    }
    if data.precipitation[i] < SANDY_PRECIP_MAX_MM && data.soil_depth_m[i] < 2.0 {
        return SoilClass::Sandy;
    }
    SoilClass::Loamy
}

fn channel_slope(_data: &WorldData, surface: &RoutingSurface, i: usize) -> f64 {
    let Some(target) = surface.flow_target[i] else {
        return 0.0;
    };
    let drop = f64::from(surface.filled_m[i]) - f64::from(surface.filled_m[target as usize]);
    drop.max(0.0) / 1.0e5
}

fn is_wetland(data: &WorldData, surface: &RoutingSurface, i: usize) -> bool {
    if channel_slope(data, surface, i) > 0.002 {
        return false;
    }
    if data.water_table_depth_m[i] < 1.0 {
        return true;
    }
    if data.river_discharge_m3_yr[i] > 1.0e9 {
        return true;
    }
    data.grid
        .neighbors(genesis_core::HexId(i as u32))
        .iter()
        .any(|nb| data.water_body_id[nb.0 as usize] != WaterBodyId::NONE)
}

/// §10.3 fertility: class-base blend + marine `fertility` + depth bonus
/// (Doc 08 v0.6). Saline stays zero.
fn soil_fertility(data: &WorldData, i: usize) -> f32 {
    let class = data.soil_class[i];
    if class == SoilClass::Saline {
        // Salt flats stay infertile regardless of marine fertility (§10.3).
        return 0.0;
    }
    let base = match class {
        SoilClass::None => 0.0,
        SoilClass::Saline => 0.0,
        SoilClass::Sandy => 0.25,
        SoilClass::Loamy => 0.55,
        SoilClass::Calcareous => 0.5,
        SoilClass::Volcanic => 0.7,
        SoilClass::Peaty => 0.45,
        SoilClass::Alluvial => 0.75,
        SoilClass::Loess => 0.9,
    };
    let marine = data.fertility[i].clamp(0.0, 1.0) * 0.4;
    let depth_bonus = (data.soil_depth_m[i] / 10.0).clamp(0.0, 0.2);
    (base + marine + depth_bonus).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::RoutingSurface;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{WorldYear, create_world};

    #[test]
    fn loamy_default_on_temperate_land() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        world.data.current_year = WorldYear(1_000_000_000);
        let n = world.data.cell_count() as usize;
        world.data.elevation_mean[0] = -100.0;
        world.data.water_body_id[0] = WaterBodyId(0);
        for i in 1..n {
            world.data.elevation_mean[i] = 200.0;
            world.data.precipitation[i] = 800.0;
            world.data.temperature_mean[i] = 12.0;
            world.data.bedrock_type[i] = BedrockType::Unknown;
        }
        let surface = RoutingSurface::build(&world.data, &[]);
        let alluvium = vec![0.0; n];
        update_soil(&mut world.data, &surface, &alluvium, 500_000.0);
        assert!(world.data.soil_class[1..].iter().any(|c| matches!(
            c,
            SoilClass::Loamy | SoilClass::Calcareous | SoilClass::Sandy
        )));
        assert!(world.data.soil_depth_m.iter().skip(1).any(|&d| d > 0.0));
    }

    #[test]
    fn marine_fertility_boosts_cretaceous_beach() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        world.data.elevation_mean.fill(100.0);
        world.data.precipitation.fill(800.0);
        world.data.temperature_mean.fill(15.0);
        world.data.fertility[10] = 1.0;
        world.data.soil_class[10] = SoilClass::Loamy;
        world.data.soil_depth_m[10] = 5.0;
        let surface = RoutingSurface::build(&world.data, &[]);
        let alluvium = vec![0.0; n];
        update_soil(&mut world.data, &surface, &alluvium, 1.0);
        assert!(world.data.soil_fertility[10] > 0.6);
    }
}
