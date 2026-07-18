//! Planetary formation sequence (Doc 07 §3).
//!
//! Multi-tick state machine governing the planet's transition from molten
//! surface to habitable Earth-like state across ~500M years.

use crate::state::{
    AtmosphericComposition, CONDENSATION_END_YEAR, COOLING_END_YEAR, COOLING_TAU_YEARS,
    FormationSubPhase, MOLTEN_END_YEAR, STABILIZATION_END_YEAR, T_EQUILIBRIUM_C,
    T_INITIAL_MOLTEN_C,
};

/// Asymptotic cooling curve (Doc 07 §3.3):
///
/// `T(t) = T_inf + (T_initial - T_inf) * exp(-t / tau)`
///
/// Returns global mean surface temperature in °C at the given year.
pub fn cooling_temperature_c(year_value: i64) -> f32 {
    let t_inf = f64::from(T_EQUILIBRIUM_C);
    let t_initial = f64::from(T_INITIAL_MOLTEN_C);
    let elapsed_years = year_value.max(0) as f64;
    let decay = (-elapsed_years / COOLING_TAU_YEARS).exp();
    (t_inf + (t_initial - t_inf) * decay) as f32
}

/// Computes atmospheric composition at a given Formation sub-phase boundary.
/// Used for interpolation within a sub-phase.
fn composition_at_phase_end(phase: FormationSubPhase) -> AtmosphericComposition {
    match phase {
        FormationSubPhase::Molten => AtmosphericComposition {
            co2_ppm: 100_000.0,
            water_vapor_index: 1.0,
            oxygen_fraction: 0.0,
            greenhouse_forcing: 100.0,
        },
        FormationSubPhase::Cooling => AtmosphericComposition {
            co2_ppm: 50_000.0,
            water_vapor_index: 0.8,
            oxygen_fraction: 0.0,
            greenhouse_forcing: 40.0,
        },
        FormationSubPhase::Condensation => AtmosphericComposition {
            co2_ppm: 10_000.0,
            water_vapor_index: 0.5,
            oxygen_fraction: 0.0,
            greenhouse_forcing: 15.0,
        },
        FormationSubPhase::Stabilization => AtmosphericComposition {
            co2_ppm: 3_000.0,
            water_vapor_index: 0.4,
            oxygen_fraction: 0.0,
            greenhouse_forcing: 5.0,
        },
        FormationSubPhase::Complete => AtmosphericComposition {
            co2_ppm: 1_000.0,
            water_vapor_index: 0.4,
            oxygen_fraction: 0.0,
            greenhouse_forcing: 0.0,
        },
    }
}

/// Computes atmospheric composition at the start of a formation sub-phase.
/// (Equivalent to the previous phase's end values.)
fn composition_at_phase_start(phase: FormationSubPhase) -> AtmosphericComposition {
    match phase {
        FormationSubPhase::Molten => AtmosphericComposition {
            co2_ppm: 200_000.0,
            water_vapor_index: 1.0,
            oxygen_fraction: 0.0,
            greenhouse_forcing: 200.0,
        },
        FormationSubPhase::Cooling => composition_at_phase_end(FormationSubPhase::Molten),
        FormationSubPhase::Condensation => composition_at_phase_end(FormationSubPhase::Cooling),
        FormationSubPhase::Stabilization => {
            composition_at_phase_end(FormationSubPhase::Condensation)
        }
        FormationSubPhase::Complete => composition_at_phase_end(FormationSubPhase::Stabilization),
    }
}

/// Computes atmospheric composition at a specific year within Formation, by
/// linear interpolation between the start and end of the current sub-phase.
pub fn composition_at_year(year_value: i64) -> AtmosphericComposition {
    let phase = FormationSubPhase::for_year(year_value);
    let (phase_start, phase_end_year) = match phase {
        FormationSubPhase::Molten => (0, MOLTEN_END_YEAR),
        FormationSubPhase::Cooling => (MOLTEN_END_YEAR, COOLING_END_YEAR),
        FormationSubPhase::Condensation => (COOLING_END_YEAR, CONDENSATION_END_YEAR),
        FormationSubPhase::Stabilization => (CONDENSATION_END_YEAR, STABILIZATION_END_YEAR),
        FormationSubPhase::Complete => {
            return composition_at_phase_end(FormationSubPhase::Complete);
        }
    };

    let phase_start_comp = composition_at_phase_start(phase);
    let phase_end_comp = composition_at_phase_end(phase);

    let progress = if phase_end_year > phase_start {
        (year_value - phase_start).clamp(0, phase_end_year - phase_start) as f64
            / (phase_end_year - phase_start) as f64
    } else {
        1.0
    };
    let progress = progress as f32;

    AtmosphericComposition {
        co2_ppm: lerp(phase_start_comp.co2_ppm, phase_end_comp.co2_ppm, progress),
        water_vapor_index: lerp(
            phase_start_comp.water_vapor_index,
            phase_end_comp.water_vapor_index,
            progress,
        ),
        oxygen_fraction: 0.0,
        greenhouse_forcing: lerp(
            phase_start_comp.greenhouse_forcing,
            phase_end_comp.greenhouse_forcing,
            progress,
        ),
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::*;

    #[test]
    fn cooling_curve_endpoints_match_design() {
        assert!((cooling_temperature_c(0) - T_INITIAL_MOLTEN_C).abs() < 1.0);

        let far_future = cooling_temperature_c(10_000_000_000);
        assert!((far_future - T_EQUILIBRIUM_C).abs() < 0.1);
    }

    #[test]
    fn cooling_curve_monotonically_decreasing() {
        let years = [
            0,
            10_000_000,
            50_000_000,
            100_000_000,
            200_000_000,
            500_000_000,
        ];
        let mut prev = f32::MAX;
        for y in years {
            let t = cooling_temperature_c(y);
            assert!(
                t < prev,
                "expected monotonic decrease at year {y}: {t} >= {prev}"
            );
            prev = t;
        }
    }

    #[test]
    fn cooling_curve_reasonable_at_formation_end() {
        let t = cooling_temperature_c(STABILIZATION_END_YEAR);
        assert!(
            (t - T_EQUILIBRIUM_C).abs() < 10.0,
            "by Formation end, should be near equilibrium; got {t}"
        );
    }

    #[test]
    fn composition_co2_monotonically_decreasing() {
        // Post-formation CO2 is flat at 1000 ppm; only check through stabilization end.
        let years = [0, 50_000_000, 200_000_000, 350_000_000, 500_000_000];
        let mut prev = f32::INFINITY;
        for y in years {
            let comp = composition_at_year(y);
            assert!(
                comp.co2_ppm < prev,
                "CO2 should monotonically decrease; year {y} got {} after {prev}",
                comp.co2_ppm
            );
            prev = comp.co2_ppm;
        }
    }

    #[test]
    fn formation_subphase_boundaries_correct() {
        assert_eq!(FormationSubPhase::for_year(0), FormationSubPhase::Molten);
        assert_eq!(
            FormationSubPhase::for_year(49_999_999),
            FormationSubPhase::Molten
        );
        assert_eq!(
            FormationSubPhase::for_year(50_000_000),
            FormationSubPhase::Cooling
        );
        assert_eq!(
            FormationSubPhase::for_year(199_999_999),
            FormationSubPhase::Cooling
        );
        assert_eq!(
            FormationSubPhase::for_year(200_000_000),
            FormationSubPhase::Condensation
        );
        assert_eq!(
            FormationSubPhase::for_year(350_000_000),
            FormationSubPhase::Stabilization
        );
        assert_eq!(
            FormationSubPhase::for_year(500_000_000),
            FormationSubPhase::Complete
        );
        assert_eq!(
            FormationSubPhase::for_year(1_000_000_000),
            FormationSubPhase::Complete
        );
    }
}
