//! The microbial era — innovation-gated low-resolution evolution (Doc 09 §3.3).
//!
//! Not simulated at species resolution: a single biosphere genome walks the deep
//! tiers of the morphospace, surfacing key innovations (oxygenic photosynthesis
//! → the Great Oxygenation → eukaryogenesis → multicellularity) as events. The
//! walk is complexity-biased (`complexity_pressure`) and O₂-gated so the
//! Doc 09 §3.3 ordering emerges. Multicellularity ends the microbial era; the
//! macroscopic tree is a later slice.

use genesis_core::data::TraitId;
use genesis_core::data::WorldData;
use genesis_core::events::{EventKind, EventLocation, InnovationKind, Significance};
use genesis_core::rng::WorldRng;
use genesis_rules::FactContext;

use crate::events::emit;
use crate::evolution::{SelectivePayoff, WalkParams, WalkStep, biased_evolution_step};
use crate::state::{BiologyState, Milestone};
use rand::Rng;

/// O₂ accrual per biology tick once photosynthesis exists — slow, so the Great
/// Oxygenation is ~100+ My after photosynthesis, not a few My (limitation 1).
const O2_RISE_PER_TICK: f32 = 0.0005;
const O2_CAP: f32 = 0.21;
const GOE_THRESHOLD: f32 = 0.10;
/// Per-tick probability the microbial biosphere takes an innovation step — most
/// ticks are stasis (punctuated equilibrium, §6.1), stretching the microbial era
/// over hundreds of My instead of ~10 (limitation 1). A generations-vs-ticks
/// model (§5.5) is the fuller fix.
const MICROBIAL_STEP_PROB: f64 = 0.04;
/// How strongly reversal/loss steps compete with gains in the microbial walk
/// (Doc 09 §2.3). Small, so most steps are innovations and loss is the rare
/// `reversal_cost`-weighted minority (e.g. a chemoautotroph shedding chemosynthesis
/// once it has turned phototroph).
const MICROBIAL_LOSS_BIAS: f32 = 0.08;

/// Complexity-biased, O₂-gated payoff for the microbial era (Doc 09 §3.3, §2.5).
///
/// The microbial genome tracks the single lineage *driving complexity*, whose
/// path runs through oxygenic photosynthesis (the oxygenation gateway). So
/// metabolisms that would exclude photosynthesis (heterotrophy, absorptive
/// decomposition) are blocked until oxygenation is achieved — their radiations
/// belong to the macroscopic tree (later) — and the innovation chain is boosted.
/// A world that never oxygenates (a stalled, microbial-only planet, Doc 09 §3.3)
/// is handled earlier in `microbial_step` via `speciation::is_microbial_only`.
struct MicrobialPayoff {
    o2_fraction: f32,
    has_photosynthesis: bool,
    complexity_pressure: f32,
    /// Steps blocked until the atmosphere is oxygen-rich.
    o2_gated: [TraitId; 2],
    /// The innovation chain, boosted by `complexity_pressure`.
    chain: [TraitId; 5],
    /// Metabolisms that would foreclose oxygenation if taken first.
    diverging: [TraitId; 2],
}

impl SelectivePayoff for MicrobialPayoff {
    fn payoff(&self, step: TraitId, _facts: &FactContext) -> f32 {
        if !self.has_photosynthesis && self.diverging.contains(&step) {
            return 0.0;
        }
        if self.o2_gated.contains(&step) && self.o2_fraction < GOE_THRESHOLD {
            return 0.0;
        }
        if self.chain.contains(&step) {
            return 2.0 + self.complexity_pressure.max(0.0);
        }
        1.0
    }
}

/// Advances the microbial biosphere by one innovation step this tick.
pub(crate) fn microbial_step(state: &mut BiologyState, world: &mut WorldData, rng: &WorldRng) {
    if state.milestones.contains(&Milestone::Multicellularity) {
        // Microbial era complete — build the recorded tree of life once, the tick
        // after multicellularity opens the macroscopic radiation (Doc 09 §6).
        if state.ledger.is_empty() {
            let seed = world.parameters.core.seed.value;
            let ledger = crate::speciation::build_radiation(
                &state.graph,
                &state.guilds,
                seed,
                world.current_year,
            );
            state.ledger = ledger;
        }
        return;
    }
    let year = world.current_year;
    // Copy the params we need so the `world` borrow is free to be mutated below
    // (the atmospheric-O₂ field lives on `world`).
    let seed = world.parameters.core.seed.value;
    let complexity_pressure = world.parameters.core.biology.complexity_pressure;
    let novelty_temperature = world.parameters.core.biology.novelty_temperature;

    // "Never oxygenates" stalled worlds (Doc 09 §3.3): some low-`complexity_pressure`
    // worlds remain bacterial mats — life emerged, but the biosphere never crosses
    // the oxygenation gateway, so no O₂ rise, no Great Oxygenation, no eukaryote or
    // multicellular radiation ever fire. Its sparse microbial tree is recorded once
    // and the era holds there. (Default `complexity_pressure` = 1.0 never stalls.)
    if crate::speciation::is_microbial_only(seed, complexity_pressure) {
        if state.ledger.is_empty() {
            state.ledger = crate::speciation::build_microbial_only(&state.graph, seed, year);
        }
        return;
    }

    let id = |name: &str| state.graph.id_of(name).expect("core trait");
    let anoxygenic = id("core:anoxygenic_phototrophy");
    let photosynthesis = id("core:oxygenic_photosynthesis");
    let eukaryote = id("core:eukaryote");
    let colonial = id("core:colonial");
    let multicellular = id("core:multicellular");
    let heterotrophy = id("core:heterotrophy");
    let absorptive = id("core:absorptive_decomposition");

    // Oxygenation feedback: oxygenic photosynthesis accrues **atmospheric O₂ on
    // the shared world state** (Doc 09 §11.1), no longer a biology-private proxy —
    // climate can read it (the temperature/CO₂ side of §11 is later). The biology
    // gates below read this same world field.
    if state.root_genome.contains(photosynthesis) {
        world.atmospheric_oxygen_fraction =
            (world.atmospheric_oxygen_fraction + O2_RISE_PER_TICK).min(O2_CAP);
        // Keep the state accessor in sync for callers reading `o2_fraction()`.
        state.o2_fraction = world.atmospheric_oxygen_fraction;
        if world.atmospheric_oxygen_fraction >= GOE_THRESHOLD
            && state.milestones.insert(Milestone::GreatOxygenation)
        {
            let o2 = world.atmospheric_oxygen_fraction;
            emit(
                state,
                year,
                EventLocation::Global,
                Significance::Pivotal,
                EventKind::GreatOxygenation { o2_fraction: o2 },
            );
        }
    }

    let o2 = world.atmospheric_oxygen_fraction;
    let payoff = MicrobialPayoff {
        o2_fraction: o2,
        has_photosynthesis: state.root_genome.contains(photosynthesis),
        complexity_pressure,
        o2_gated: [eukaryote, multicellular],
        chain: [
            anoxygenic,
            photosynthesis,
            eukaryote,
            colonial,
            multicellular,
        ],
        diverging: [heterotrophy, absorptive],
    };
    let env = FactContext::new().with_scalar("env:o2_fraction", f64::from(o2));
    let params = WalkParams {
        novelty_temperature,
    };
    let mut stream = rng.stream_at("biology.evolution", year.value() as u64);

    // Punctuated equilibrium: most ticks are stasis, so the microbial era spans
    // hundreds of My rather than ~10 (limitation 1).
    if stream.gen_range(0.0..1.0) >= MICROBIAL_STEP_PROB {
        return;
    }
    match biased_evolution_step(
        &state.graph,
        &state.root_genome,
        &env,
        &params,
        &payoff,
        MICROBIAL_LOSS_BIAS,
        &mut stream,
    ) {
        Some(WalkStep::Gain(step)) => {
            state.root_genome.insert(step);
            emit_innovation(state, world, step, photosynthesis, eukaryote, multicellular);
        }
        // Reversal (Doc 09 §2.3): shed a superseded trait. Silent — a low-signal
        // event in the deep microbial era; the milestone chain is gain-driven.
        Some(WalkStep::Loss(step)) => {
            state.root_genome.remove(step);
        }
        None => {}
    }
}

/// Fires the once-only innovation event for a just-acquired trait.
fn emit_innovation(
    state: &mut BiologyState,
    world: &WorldData,
    step: TraitId,
    photosynthesis: TraitId,
    eukaryote: TraitId,
    multicellular: TraitId,
) {
    let year = world.current_year;
    let innovation =
        if step == photosynthesis && state.milestones.insert(Milestone::OxygenicPhotosynthesis) {
            InnovationKind::OxygenicPhotosynthesis
        } else if step == eukaryote && state.milestones.insert(Milestone::Eukaryogenesis) {
            InnovationKind::Eukaryogenesis
        } else if step == multicellular && state.milestones.insert(Milestone::Multicellularity) {
            InnovationKind::Multicellularity
        } else {
            return;
        };
    emit(
        state,
        year,
        EventLocation::Global,
        Significance::Pivotal,
        EventKind::EvolutionaryInnovation { innovation },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biogenesis::try_biogenesis;
    use crate::state::{BiologyState, Milestone};
    use genesis_core::parameters::WorldParameters;
    use genesis_core::rng::WorldRng;
    use genesis_core::time::WorldYear;
    use genesis_core::{create_world, data::WorldData};

    const TICK: i64 = 500_000;

    /// A small world with deep ocean on the even-indexed hexes, life targeted
    /// early so the ramp resolves fast.
    fn deep_ocean_world() -> WorldData {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        params.core.biology.life_emergence_year = WorldYear(5_000_000);
        let mut world = create_world(params).expect("world").data;
        for i in (0..world.cell_count() as usize).step_by(2) {
            world.elevation_mean[i] = -3000.0;
            world.water_level_m[i] = 0.0;
        }
        world
    }

    /// Drives the layer's internal steps until the microbial era completes or the
    /// tick budget runs out. Returns the final state.
    fn run(seed: u64) -> BiologyState {
        let mut world = deep_ocean_world();
        let rng = WorldRng::from_effective_seed(seed);
        let mut state = BiologyState::new();
        let mut year = TICK;
        for _ in 0..400 {
            world.current_year = WorldYear(year);
            if state.origin().is_none() {
                try_biogenesis(&mut state, &mut world, &rng, TICK);
            } else {
                microbial_step(&mut state, &mut world, &rng);
            }
            if state.has_milestone(Milestone::Multicellularity) {
                break;
            }
            year += TICK;
        }
        state
    }

    #[test]
    fn life_emerges_at_a_deep_vent_and_climbs_to_multicellularity() {
        let state = run(20260720);
        let origin = state.origin().expect("life should emerge");
        // Origin is a deep-ocean (even-index) vent hex.
        assert_eq!(origin.hex.0 % 2, 0, "origin must be a deep-ocean vent hex");
        // The full §3.3 innovation ladder fired, in the right dependency order.
        for milestone in [
            Milestone::LifeEmerged,
            Milestone::OxygenicPhotosynthesis,
            Milestone::GreatOxygenation,
            Milestone::Eukaryogenesis,
            Milestone::Multicellularity,
        ] {
            assert!(state.has_milestone(milestone), "missing {milestone:?}");
        }
        // Each milestone emitted a pivotal event for the chronicle.
        assert!(state.pending_event_count() >= 5);
    }

    #[test]
    fn origin_is_deterministic_for_a_seed() {
        let a = run(42).origin().expect("a");
        let b = run(42).origin().expect("b");
        assert_eq!(a, b, "same seed → same origin hex and year");
    }

    #[test]
    fn a_stalled_world_stays_microbial() {
        use crate::speciation::is_microbial_only;
        // Find a seed that stalls at zero complexity pressure.
        let seed = (0..10_000u64)
            .find(|&s| is_microbial_only(s, 0.0))
            .expect("some seed stalls at cp 0");

        let mut world = deep_ocean_world();
        world.parameters.core.seed.value = seed;
        world.parameters.core.biology.complexity_pressure = 0.0;
        let rng = WorldRng::from_effective_seed(seed);
        let mut state = BiologyState::new();
        let mut year = TICK;
        for _ in 0..400 {
            world.current_year = WorldYear(year);
            if state.origin().is_none() {
                try_biogenesis(&mut state, &mut world, &rng, TICK);
            } else {
                microbial_step(&mut state, &mut world, &rng);
            }
            year += TICK;
        }
        // Life emerged, but the oxygenation gateway was never crossed.
        assert!(state.has_milestone(Milestone::LifeEmerged));
        for never in [
            Milestone::GreatOxygenation,
            Milestone::Eukaryogenesis,
            Milestone::Multicellularity,
        ] {
            assert!(
                !state.has_milestone(never),
                "a stalled world must never reach {never:?}"
            );
        }
        // It recorded a sparse microbial tree (not the full radiation).
        assert!(
            (2..=6).contains(&state.ledger().len()),
            "microbial-only tree, got {}",
            state.ledger().len()
        );
    }

    #[test]
    fn oxygenation_writes_shared_atmospheric_o2() {
        // The Great Oxygenation now accrues real atmospheric O₂ on the shared
        // world state (Doc 09 §11.1), not a biology-private proxy.
        let mut world = deep_ocean_world();
        let rng = WorldRng::from_effective_seed(20260720);
        let mut state = BiologyState::new();
        let mut year = TICK;
        for _ in 0..400 {
            world.current_year = WorldYear(year);
            if state.origin().is_none() {
                try_biogenesis(&mut state, &mut world, &rng, TICK);
            } else {
                microbial_step(&mut state, &mut world, &rng);
            }
            if state.has_milestone(Milestone::Multicellularity) {
                break;
            }
            year += TICK;
        }
        assert!(
            world.atmospheric_oxygen_fraction >= GOE_THRESHOLD,
            "atmosphere oxygenated on the world state, got {}",
            world.atmospheric_oxygen_fraction
        );
        // The state accessor mirrors the world authority.
        assert_eq!(state.o2_fraction(), world.atmospheric_oxygen_fraction);
    }

    #[test]
    fn oxygenation_precedes_eukaryogenesis() {
        // Eukaryogenesis is O₂-gated, so the Great Oxygenation must come first.
        let mut world = deep_ocean_world();
        let rng = WorldRng::from_effective_seed(7);
        let mut state = BiologyState::new();
        let mut goe_year: Option<i64> = None;
        let mut euk_year: Option<i64> = None;
        let mut year = TICK;
        for _ in 0..400 {
            world.current_year = WorldYear(year);
            let had_goe = state.has_milestone(Milestone::GreatOxygenation);
            let had_euk = state.has_milestone(Milestone::Eukaryogenesis);
            if state.origin().is_none() {
                try_biogenesis(&mut state, &mut world, &rng, TICK);
            } else {
                microbial_step(&mut state, &mut world, &rng);
            }
            if !had_goe && state.has_milestone(Milestone::GreatOxygenation) {
                goe_year = Some(year);
            }
            if !had_euk && state.has_milestone(Milestone::Eukaryogenesis) {
                euk_year = Some(year);
            }
            if state.has_milestone(Milestone::Multicellularity) {
                break;
            }
            year += TICK;
        }
        let (goe, euk) = (goe_year.expect("GOE"), euk_year.expect("eukaryogenesis"));
        assert!(
            goe <= euk,
            "oxygenation ({goe}) must not follow eukaryogenesis ({euk})"
        );
    }
}
