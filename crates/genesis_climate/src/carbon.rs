//! Post-Formation carbon cycle (Doc 07 §11).
//!
//! During Formation the atmosphere follows the scripted cooling curve
//! (Doc 07 §3); afterwards CO2 drifts under volcanic outgassing (source)
//! and continental weathering (sink). The sink scales with CO2, so the
//! cycle self-regulates: high CO2 weathers faster, pulling CO2 back down
//! (§11.1). Greenhouse forcing derives from CO2 and water vapor (§11.3)
//! and feeds the temperature field each tick.

use genesis_core::data::{WaterBodyId, WorldData};

use crate::state::ClimateState;

/// CO2 source from volcanic outgassing, ppm per year (Doc 07 §11.1).
///
/// Constant: §11.1 ties outgassing to tectonic activity, but no clean
/// activity field exists on `WorldData`, so the loose treatment §11.2
/// allows is used. 4 ppm/My equilibrates against weathering over
/// ~150–200 My — fast enough to track Wilson-cycle drift, slow enough
/// that single ticks are noise.
pub const OUTGASSING_PPM_PER_YEAR: f64 = 4.0e-6;

/// Weathering sink strength, ppm per year at reference conditions (Doc 07
/// §11.1): a fully continental world at [`WEATHERING_P_REF_MM`] mean land
/// precipitation and 280 ppm CO2. 3x [`OUTGASSING_PPM_PER_YEAR`] so the
/// equilibrium lands near 300–500 ppm on an Earthlike (~30% land, ~800 mm)
/// continental surface — and below pre-industrial on land-dominated worlds
/// (more rock surface weathers more CO2; §11.1's self-regulation).
pub const WEATHERING_K_PPM_PER_YEAR: f64 = 12.0e-6;

/// Reference mean land precipitation for the weathering factor, mm/yr
/// (Doc 07 §11.1); Earth's ~800 mm global mean (Doc 06 §8.2 shares it).
pub const WEATHERING_P_REF_MM: f64 = 800.0;

/// Pre-industrial CO2 baseline for the forcing logarithm, ppm (Doc 07 §11.3).
pub const CO2_BASELINE_PPM: f64 = 280.0;

/// Greenhouse sensitivity: °C per CO2 doubling (Doc 07 §11.3).
pub const GREENHOUSE_C_PER_DOUBLING: f64 = 4.0;

/// Water-vapor forcing: °C per unit `water_vapor_index` (Doc 07 §11.3).
pub const WATER_VAPOR_FORCING_C: f64 = 10.0;

/// CO2 bounds (ppm): keep the logarithm finite and the cycle bounded.
pub const CO2_MIN_PPM: f64 = 50.0;
pub const CO2_MAX_PPM: f64 = 10_000.0;

/// Advances the post-Formation CO2 cycle by one tick (Doc 07 §11.1) and
/// rederives greenhouse forcing (§11.3). Deterministic: ascending-HexId
/// accumulation, f64 throughout, no RNG. Forcing is rederived even on a
/// zero-length tick so it always matches the stored CO2.
pub fn update_carbon_cycle(data: &WorldData, state: &mut ClimateState, tick_years: f64) {
    let n = data.cell_count() as usize;
    let mut land_count = 0_u64;
    let mut land_precip_sum_mm = 0.0_f64;
    for i in 0..n {
        // Wet predicate mirrors hydrology: standing water carries a body id.
        if data.water_body_id[i] == WaterBodyId::NONE {
            land_count += 1;
            land_precip_sum_mm += f64::from(data.precipitation[i]);
        }
    }
    let land_fraction = if n == 0 {
        0.0
    } else {
        land_count as f64 / n as f64
    };
    let mean_land_precip_mm = if land_count == 0 {
        0.0
    } else {
        land_precip_sum_mm / land_count as f64
    };

    let co2_ppm = f64::from(state.atmospheric_composition.co2_ppm);
    let outgassing_ppm = OUTGASSING_PPM_PER_YEAR * tick_years;
    let weathering_ppm = WEATHERING_K_PPM_PER_YEAR
        * land_fraction
        * (mean_land_precip_mm / WEATHERING_P_REF_MM)
        * (co2_ppm / CO2_BASELINE_PPM)
        * tick_years;
    let next_ppm = (co2_ppm + outgassing_ppm - weathering_ppm).clamp(CO2_MIN_PPM, CO2_MAX_PPM);

    let composition = &mut state.atmospheric_composition;
    composition.co2_ppm = next_ppm as f32;
    composition.greenhouse_forcing =
        greenhouse_forcing_c(next_ppm, f64::from(composition.water_vapor_index)) as f32;
}

/// Greenhouse forcing in °C (Doc 07 §11.3): logarithmic in CO2 — 4 °C per
/// doubling vs 280 ppm — plus a linear water-vapor term.
pub fn greenhouse_forcing_c(co2_ppm: f64, water_vapor_index: f64) -> f64 {
    GREENHOUSE_C_PER_DOUBLING * (co2_ppm / CO2_BASELINE_PPM).log2()
        + water_vapor_index * WATER_VAPOR_FORCING_C
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::create_world;
    use genesis_core::parameters::WorldParameters;

    fn test_world() -> WorldData {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        create_world(params).expect("world").data
    }

    /// Marks every third hex wet → 2/3 land fraction.
    fn set_land_fraction_two_thirds(data: &mut WorldData) {
        for (i, id) in data.water_body_id.iter_mut().enumerate() {
            if i % 3 == 0 {
                *id = WaterBodyId(0);
            }
        }
    }

    #[test]
    fn forcing_matches_spec_formula() {
        // 280 ppm adds nothing over the water-vapor term; each doubling +4 °C.
        assert_eq!(greenhouse_forcing_c(280.0, 0.4), 4.0);
        assert_eq!(greenhouse_forcing_c(560.0, 0.4), 8.0);
        assert_eq!(greenhouse_forcing_c(1120.0, 0.0), 8.0);
    }

    #[test]
    fn outgassing_alone_raises_co2_when_nothing_is_land() {
        let mut data = test_world();
        for id in data.water_body_id.iter_mut() {
            *id = WaterBodyId(0);
        }
        let mut state = ClimateState::new();
        state.atmospheric_composition.co2_ppm = 400.0;
        update_carbon_cycle(&data, &mut state, 500_000.0);
        let expected = 400.0 + OUTGASSING_PPM_PER_YEAR * 500_000.0;
        assert!(
            (f64::from(state.atmospheric_composition.co2_ppm) - expected).abs() < 1e-6,
            "no land → no weathering; expected {expected}, got {}",
            state.atmospheric_composition.co2_ppm
        );
    }

    #[test]
    fn weathering_self_limits_high_co2() {
        let mut data = test_world();
        set_land_fraction_two_thirds(&mut data);
        data.precipitation.fill(800.0);

        let mut state = ClimateState::new();
        state.atmospheric_composition.co2_ppm = 8_000.0;
        update_carbon_cycle(&data, &mut state, 500_000.0);
        assert!(
            state.atmospheric_composition.co2_ppm < 8_000.0,
            "weathering scales with CO2; 8000 ppm should shrink, got {}",
            state.atmospheric_composition.co2_ppm
        );

        state.atmospheric_composition.co2_ppm = 100.0;
        update_carbon_cycle(&data, &mut state, 500_000.0);
        assert!(
            state.atmospheric_composition.co2_ppm > 100.0,
            "below equilibrium outgassing wins; 100 ppm should grow, got {}",
            state.atmospheric_composition.co2_ppm
        );
    }

    #[test]
    fn co2_equilibrates_where_outgassing_balances_weathering() {
        let mut data = test_world();
        set_land_fraction_two_thirds(&mut data);
        // 400 mm → precip factor 0.5; equilibrium = 280 * (O/K) / (f * r)
        // = 280 * (1/3) / (2/3 * 0.5) = 280 ppm, exactly the baseline.
        data.precipitation.fill(400.0);

        let mut state = ClimateState::new();
        state.atmospheric_composition.co2_ppm = 1_000.0;
        for _ in 0..1_500 {
            update_carbon_cycle(&data, &mut state, 500_000.0);
        }
        let co2 = f64::from(state.atmospheric_composition.co2_ppm);
        assert!(
            (co2 - 280.0).abs() < 10.0,
            "should converge on the 280 ppm balance, got {co2}"
        );
        let forcing = f64::from(state.atmospheric_composition.greenhouse_forcing);
        assert!(
            (forcing - 4.0).abs() < 0.3,
            "280 ppm at water-vapor 0.4 → +4 °C, got {forcing}"
        );
    }

    #[test]
    fn co2_clamps_to_bounds() {
        let mut data = test_world();
        // Monsoon-drenched all-land world weathers faster than outgassing
        // even at the floor.
        data.precipitation.fill(10_000.0);
        let mut state = ClimateState::new();
        state.atmospheric_composition.co2_ppm = 51.0;
        update_carbon_cycle(&data, &mut state, 500_000.0);
        assert_eq!(state.atmospheric_composition.co2_ppm, CO2_MIN_PPM as f32);

        // Barren ocean world: nothing weathers, CO2 piles up to the ceiling.
        for id in data.water_body_id.iter_mut() {
            *id = WaterBodyId(0);
        }
        state.atmospheric_composition.co2_ppm = 9_999.0;
        update_carbon_cycle(&data, &mut state, 500_000.0);
        assert_eq!(state.atmospheric_composition.co2_ppm, CO2_MAX_PPM as f32);
    }

    #[test]
    fn cycle_is_deterministic() {
        let mut data = test_world();
        set_land_fraction_two_thirds(&mut data);
        data.precipitation.fill(650.0);

        let run = || {
            let mut state = ClimateState::new();
            state.atmospheric_composition.co2_ppm = 900.0;
            for _ in 0..50 {
                update_carbon_cycle(&data, &mut state, 500_000.0);
            }
            (
                state.atmospheric_composition.co2_ppm,
                state.atmospheric_composition.greenhouse_forcing,
            )
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn zero_length_tick_keeps_co2_but_rederives_forcing() {
        let mut data = test_world();
        set_land_fraction_two_thirds(&mut data);
        data.precipitation.fill(800.0);
        let mut state = ClimateState::new();
        state.atmospheric_composition.co2_ppm = 560.0;
        state.atmospheric_composition.greenhouse_forcing = 0.0;
        update_carbon_cycle(&data, &mut state, 0.0);
        assert_eq!(state.atmospheric_composition.co2_ppm, 560.0);
        assert!(
            (state.atmospheric_composition.greenhouse_forcing - 8.0).abs() < 1e-6,
            "560 ppm at water-vapor 0.4 → +8 °C, got {}",
            state.atmospheric_composition.greenhouse_forcing
        );
    }
}
