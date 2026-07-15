//! Köppen-like climate regime classification (Doc 07 §10, P2-12).
//!
//! A decision tree over `(temperature_mean, temperature_range, precipitation)`
//! labels every hex with one of ten regimes. Biology (Phase 4) maps regimes to
//! biomes; rendering exposes them as a map mode.

use genesis_core::HexId;
use genesis_core::data::{ClimateRegimePlaceholder, WorldData};

/// Classifies one hex (Doc 07 §10.2 thresholds).
pub fn classify(temp_mean_c: f32, temp_range_c: f32, precip_mm: f32) -> ClimateRegimePlaceholder {
    use ClimateRegimePlaceholder as R;
    if temp_mean_c < -10.0 {
        return R::Polar;
    }
    if temp_mean_c < 0.0 {
        return if precip_mm < 100.0 {
            R::Tundra
        } else {
            R::Boreal
        };
    }
    if temp_mean_c < 10.0 {
        return if precip_mm < 200.0 {
            R::ColdDesert
        } else {
            R::ContinentalCool
        };
    }
    if temp_mean_c < 20.0 {
        if precip_mm < 250.0 {
            return R::HotDesert;
        }
        if precip_mm < 600.0 && temp_range_c > 20.0 {
            return R::Mediterranean;
        }
        return R::Temperate;
    }
    if temp_mean_c < 25.0 {
        return if precip_mm < 250.0 {
            R::HotDesert
        } else {
            R::Subtropical
        };
    }
    if precip_mm < 250.0 {
        return R::HotDesert;
    }
    R::Tropical
}

/// Writes `WorldData.climate_regime` for every hex. Ocean hexes (below sea
/// level) keep `Unset`: regimes describe land climate for biology.
pub fn classify_regimes(data: &mut WorldData) {
    let n = data.cell_count() as usize;
    for i in 0..n {
        let hex = HexId(i as u32);
        let _ = hex;
        data.climate_regime[i] = if data.elevation_mean[i] < data.sea_level_m {
            ClimateRegimePlaceholder::Unset
        } else {
            classify(
                data.temperature_mean[i],
                data.temperature_range[i],
                data.precipitation[i],
            )
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ClimateRegimePlaceholder as R;

    #[test]
    fn decision_tree_matches_spec_examples() {
        assert_eq!(classify(-20.0, 10.0, 500.0), R::Polar);
        assert_eq!(classify(-5.0, 10.0, 50.0), R::Tundra);
        assert_eq!(classify(-5.0, 10.0, 400.0), R::Boreal);
        assert_eq!(classify(5.0, 15.0, 100.0), R::ColdDesert);
        assert_eq!(classify(5.0, 15.0, 500.0), R::ContinentalCool);
        assert_eq!(classify(15.0, 10.0, 100.0), R::HotDesert);
        assert_eq!(classify(15.0, 25.0, 400.0), R::Mediterranean);
        assert_eq!(classify(15.0, 10.0, 900.0), R::Temperate);
        assert_eq!(classify(22.0, 10.0, 900.0), R::Subtropical);
        assert_eq!(classify(28.0, 5.0, 100.0), R::HotDesert);
        assert_eq!(classify(28.0, 5.0, 2000.0), R::Tropical);
    }

    #[test]
    fn ocean_hexes_stay_unset_and_land_gets_regimes() {
        let mut params = genesis_core::parameters::WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = genesis_core::create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        for i in 0..n {
            world.data.elevation_mean[i] = if i % 2 == 0 { 500.0 } else { -3000.0 };
            world.data.temperature_mean[i] = 22.0;
            world.data.temperature_range[i] = 8.0;
            world.data.precipitation[i] = 900.0;
        }
        world.data.sea_level_m = 0.0;
        classify_regimes(&mut world.data);
        assert_eq!(world.data.climate_regime[0], R::Subtropical);
        assert_eq!(world.data.climate_regime[1], R::Unset);
    }
}
