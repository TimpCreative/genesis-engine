//! Milankovitch-like orbital cycles and the glaciation state machine (Doc 07 §12).
//!
//! Two superimposed orbital cycles drive a global temperature modifier. When
//! the modifier swings cold the world steps Interglacial → Transition →
//! Glacial (and back when it swings warm); temperature and precipitation
//! already consume [`GlaciationState`], so ice ages cool high latitudes and
//! dry the world without further wiring.

use genesis_core::branches::BranchId;
use genesis_core::data::WorldData;
use genesis_core::events::{Event, EventKind, EventLocation, Significance};
use genesis_core::time::WorldYear;

use crate::events::{alloc_event_id, maybe_emit};
use crate::state::{ClimateState, GlaciationState};

/// Peak-to-mean amplitude of the combined orbital temperature swing (°C).
pub const MILANKOVITCH_AMPLITUDE_C: f32 = 3.0;

/// Short orbital cycle period (years). Real Milankovitch cycles (~100k years)
/// are sub-tick at the 500k-year Geological cadence — sampling them per tick
/// produces anti-correlated noise, not a cycle — so the model uses stretched
/// "Milankovitch-like" periods the tick rate can resolve. Deliberately NOT a
/// multiple of the tick interval (that would freeze the sampled phase).
pub const SHORT_CYCLE_YEARS: f64 = 2_300_000.0;

/// The long cycle runs at a quarter of the short cycle's rate (Doc 07 §12.1).
pub const LONG_CYCLE_PHASE_FACTOR: f64 = 0.25;

/// Orbital modifier that begins a cold excursion (Interglacial → Transition),
/// °C. Deliberately deep: both cycles must align cold, so glacials are
/// episodic (Earth spends most of deep time ice-free in greenhouse climates).
pub const GLACIAL_ONSET_THRESHOLD_C: f32 = -2.2;

/// Modifier below which an in-progress cold excursion continues into a full
/// glacial (Transition → Glacial), °C. Softer than onset: once triggered, the
/// ice-albedo feedback sustains cooling.
pub const GLACIAL_CONTINUE_THRESHOLD_C: f32 = -0.8;

/// Modifier above which the world steps back toward Interglacial (°C).
/// Shallow: any warm swing ends an ice age, keeping the duty cycle low.
pub const INTERGLACIAL_STEP_THRESHOLD_C: f32 = 0.3;

/// Accumulates orbital phase for one tick.
pub fn advance_orbital_phase(state: &mut ClimateState, tick_interval_years: f64) {
    state.cumulative_orbital_phase_rad +=
        std::f64::consts::TAU * tick_interval_years / SHORT_CYCLE_YEARS;
}

/// Combined short + long cycle global temperature modifier (Doc 07 §12.1).
pub fn orbital_temperature_modifier_c(state: &ClimateState) -> f32 {
    let phase = state.cumulative_orbital_phase_rad;
    let short = phase.sin();
    let long = (phase * LONG_CYCLE_PHASE_FACTOR).sin();
    ((short * 0.5 + long * 0.5) as f32) * MILANKOVITCH_AMPLITUDE_C
}

/// Steps the glaciation state machine and emits Pivotal events on entering or
/// leaving a full glacial. Hysteresis: the modifier must swing past opposite
/// thresholds to reverse, and each swing moves one stage per tick.
pub fn advance_glaciation(
    data: &WorldData,
    state: &mut ClimateState,
    tick_year: WorldYear,
    event_granularity: Significance,
    branch_id: BranchId,
) {
    let modifier = orbital_temperature_modifier_c(state);

    let next = match state.glaciation {
        GlaciationState::Interglacial if modifier < GLACIAL_ONSET_THRESHOLD_C => {
            GlaciationState::Transition
        }
        GlaciationState::Transition if modifier < GLACIAL_CONTINUE_THRESHOLD_C => {
            GlaciationState::Glacial
        }
        GlaciationState::Transition if modifier > INTERGLACIAL_STEP_THRESHOLD_C => {
            GlaciationState::Interglacial
        }
        GlaciationState::Glacial if modifier > INTERGLACIAL_STEP_THRESHOLD_C => {
            GlaciationState::Transition
        }
        other => other,
    };

    let entering_glacial = !matches!(state.glaciation, GlaciationState::Glacial)
        && matches!(next, GlaciationState::Glacial);
    let leaving_glacial_era =
        state.glacial_started_year.is_some() && matches!(next, GlaciationState::Interglacial);

    state.glaciation = next;

    if entering_glacial {
        state.glacial_started_year = Some(tick_year.value());
        let event_id = alloc_event_id(state);
        maybe_emit(
            state,
            Event {
                id: event_id,
                year: tick_year,
                branch_id,
                location: EventLocation::Global,
                significance: Significance::Pivotal,
                kind: EventKind::GlaciationBegan {
                    global_temperature_c: data.global_temperature_c + modifier,
                },
            },
            event_granularity,
        );
    } else if leaving_glacial_era {
        let started = state
            .glacial_started_year
            .take()
            .unwrap_or(tick_year.value());
        let event_id = alloc_event_id(state);
        maybe_emit(
            state,
            Event {
                id: event_id,
                year: tick_year,
                branch_id,
                location: EventLocation::Global,
                significance: Significance::Pivotal,
                kind: EventKind::GlaciationEnded {
                    duration_years: tick_year.value() - started,
                },
            },
            event_granularity,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::create_world;
    use genesis_core::parameters::WorldParameters;

    fn test_data() -> genesis_core::data::WorldData {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        create_world(params).expect("world").data
    }

    #[test]
    fn orbital_modifier_stays_within_amplitude() {
        let mut state = ClimateState::default();
        for _ in 0..500 {
            advance_orbital_phase(&mut state, 500_000.0);
            let m = orbital_temperature_modifier_c(&state);
            assert!(
                m.abs() <= MILANKOVITCH_AMPLITUDE_C + 1e-3,
                "modifier {m} exceeds amplitude"
            );
        }
    }

    #[test]
    fn orbital_phase_is_not_frozen_by_tick_aliasing() {
        let mut state = ClimateState::default();
        let mut values = Vec::new();
        for _ in 0..50 {
            advance_orbital_phase(&mut state, 500_000.0);
            values.push(orbital_temperature_modifier_c(&state));
        }
        let min = values.iter().copied().fold(f32::MAX, f32::min);
        let max = values.iter().copied().fold(f32::MIN, f32::max);
        assert!(
            max - min > MILANKOVITCH_AMPLITUDE_C,
            "orbital modifier barely moves across ticks (min={min}, max={max}); \
             the cycle period aliases against the tick interval"
        );
    }

    #[test]
    fn glaciation_cycles_within_a_billion_years() {
        let data = test_data();
        let mut state = ClimateState::default();
        let mut began = 0;
        let mut ended = 0;
        // 2000 Geological ticks = 1B years.
        for tick in 1..=2000 {
            advance_orbital_phase(&mut state, 500_000.0);
            let before = state.pending_events.len();
            advance_glaciation(
                &data,
                &mut state,
                WorldYear(tick * 500_000),
                Significance::Trace,
                BranchId::ROOT,
            );
            for event in &state.pending_events[before..] {
                match event.kind {
                    EventKind::GlaciationBegan { .. } => began += 1,
                    EventKind::GlaciationEnded { .. } => ended += 1,
                    _ => {}
                }
            }
        }
        assert!(
            began >= 1 && ended >= 1,
            "expected at least one full glaciation cycle in 1B years (began={began}, ended={ended})"
        );
    }

    #[test]
    fn glaciation_is_deterministic() {
        let data = test_data();
        let mut a = ClimateState::default();
        let mut b = ClimateState::default();
        for tick in 1..=200 {
            for s in [&mut a, &mut b] {
                advance_orbital_phase(s, 500_000.0);
                advance_glaciation(
                    &data,
                    s,
                    WorldYear(tick * 500_000),
                    Significance::Trace,
                    BranchId::ROOT,
                );
            }
        }
        assert_eq!(
            a.cumulative_orbital_phase_rad,
            b.cumulative_orbital_phase_rad
        );
        assert_eq!(a.pending_events.len(), b.pending_events.len());
    }
}
