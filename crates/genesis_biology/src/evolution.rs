//! The biased walk — one speciation step through the morphospace (Doc 09 §2.4).
//!
//! Candidate steps are the reachable proximity-neighbors of the current genome;
//! each is scored `proximity × prerequisite_gate × selective_payoff ×
//! novelty_factor × directional_factor` and one is drawn from the normalized
//! weights via a deterministic stream. **Legality** (the prerequisite/exclusion
//! gate) is evaluated by the rule engine (`genesis_rules`, Doc 11); **scoring**
//! stays here (Doc 09 §1.2 boundary). Steps may **gain** a trait or, via
//! [`biased_evolution_step`], **lose** one (the reversal mechanic, Doc 09 §2.3,
//! weighted by `reversal_cost`). The ecological payoff arrives with guilds (P4-5).

use genesis_core::data::TraitId;
use genesis_rules::{FactContext, evaluate, trait_gate};
use rand::Rng;
use rand::rngs::SmallRng;

use crate::morphospace::{TraitGraph, TraitSet};

/// Tunables for the walk.
#[derive(Clone, Copy, Debug)]
pub struct WalkParams {
    /// Exploration temperature: 0 = Earth-like clustering, 1.0 = alien (default),
    /// ~2.0 = weird (Doc 09 §2.6).
    pub novelty_temperature: f32,
}

impl Default for WalkParams {
    fn default() -> Self {
        Self {
            novelty_temperature: 1.0,
        }
    }
}

/// The coupling to ecology (Doc 09 §2.5): how strongly selection favors a step.
/// Neutral until guilds exist (P4-5).
pub trait SelectivePayoff {
    /// Multiplicative payoff for adding `step` given the current facts (≥ 0).
    fn payoff(&self, step: TraitId, facts: &FactContext) -> f32;
}

/// Favors nothing — every legal step scored purely by graph structure + novelty.
/// The P4-2 default; P4-5 replaces it with guild/niche coupling.
pub struct NeutralPayoff;

impl SelectivePayoff for NeutralPayoff {
    fn payoff(&self, _step: TraitId, _facts: &FactContext) -> f32 {
        1.0
    }
}

/// The fact context a walk evaluates against: `env` scalars plus the genome's
/// traits.
fn walk_facts(genome: &TraitSet, env: &FactContext) -> FactContext {
    let mut facts = env.clone();
    for id in genome.iter() {
        facts.insert_trait(id);
    }
    facts
}

/// `proximity ^ (1 − clamp(temperature, 0, 2))` (Doc 09 §2.6): low temperature
/// suppresses low-proximity steps; high temperature boosts them.
fn novelty_factor(proximity: f32, temperature: f32) -> f32 {
    let exponent = 1.0 - temperature.clamp(0.0, 2.0);
    proximity.max(f32::MIN_POSITIVE).powf(exponent)
}

/// Scores each **legal** candidate step from `genome`, in ascending `TraitId`
/// order. Illegal steps (unmet prerequisite or present exclusion) are dropped —
/// that is the `prerequisite_gate` term. `directional_factor` = 1 in P4-2.
pub fn candidate_weights(
    graph: &TraitGraph,
    genome: &TraitSet,
    env: &FactContext,
    params: &WalkParams,
    payoff: &dyn SelectivePayoff,
) -> Vec<(TraitId, f32)> {
    let facts = walk_facts(genome, env);
    graph
        .candidate_steps(genome)
        .into_iter()
        .filter_map(|(candidate, proximity)| {
            let node = graph.node(candidate);
            let gate = trait_gate(node.prerequisites.clone(), node.exclusions.clone());
            if !evaluate(&gate, &facts) {
                return None; // prerequisite_gate = 0
            }
            let weight = proximity
                * payoff.payoff(candidate, &facts)
                * novelty_factor(proximity, params.novelty_temperature);
            (weight > 0.0).then_some((candidate, weight))
        })
        .collect()
}

/// One step of the walk: gain a new trait, or **lose** an existing one (the
/// reversal/loss mechanic, Doc 09 §2.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WalkStep {
    Gain(TraitId),
    Loss(TraitId),
}

/// The traits eligible to be **lost** from `genome`, each with a loss weight
/// ∝ `1 / (1 + reversal_cost)` — a high `reversal_cost` trait is rarely shed
/// (Doc 09 §2.3). A trait is eligible only if its removal keeps the genome
/// legal: no *other* present trait still lists it as a prerequisite (so we never
/// strand a dependent), and the genome never empties. Ascending `TraitId` order.
pub fn loss_candidates(graph: &TraitGraph, genome: &TraitSet) -> Vec<(TraitId, f32)> {
    if genome.len() <= 1 {
        return Vec::new();
    }
    genome
        .iter()
        .filter(|&t| {
            // Integrity: nothing still present depends on `t`.
            !genome
                .iter()
                .any(|other| other != t && graph.node(other).prerequisites.contains(&t))
        })
        .map(|t| (t, 1.0 / (1.0 + graph.node(t).reversal_cost.max(0.0))))
        .collect()
}

/// Takes one biased walk step from `genome`: draws a legal trait to gain,
/// weighted per Doc 09 §2.4, from the deterministic `rng` stream. `None` when no
/// legal candidate exists (the caller handles stasis / triggers, Doc 09 §6).
pub fn biased_walk_step(
    graph: &TraitGraph,
    genome: &TraitSet,
    env: &FactContext,
    params: &WalkParams,
    payoff: &dyn SelectivePayoff,
    rng: &mut SmallRng,
) -> Option<TraitId> {
    let weights = candidate_weights(graph, genome, env, params, payoff);
    draw_weighted(&weights, rng).copied()
}

/// One evolution step that may **gain or lose** a trait (Doc 09 §2.3–§2.4).
/// Gains are scored as in [`biased_walk_step`]; losses are the [`loss_candidates`]
/// scaled by `loss_bias` (0 ⇒ gain-only, recovering the plain walk). Both sets
/// are drawn together from one normalized distribution, so most steps are gains
/// and reversal is the rare, `reversal_cost`-weighted minority.
pub fn biased_evolution_step(
    graph: &TraitGraph,
    genome: &TraitSet,
    env: &FactContext,
    params: &WalkParams,
    payoff: &dyn SelectivePayoff,
    loss_bias: f32,
    rng: &mut SmallRng,
) -> Option<WalkStep> {
    let mut steps: Vec<(WalkStep, f32)> = candidate_weights(graph, genome, env, params, payoff)
        .into_iter()
        .map(|(id, w)| (WalkStep::Gain(id), w))
        .collect();
    if loss_bias > 0.0 {
        for (id, w) in loss_candidates(graph, genome) {
            steps.push((WalkStep::Loss(id), w * loss_bias));
        }
    }
    draw_weighted(&steps, rng).copied()
}

/// Draws one item from `(item, weight)` pairs proportional to weight, using the
/// deterministic stream. `None` if the total weight is non-positive.
fn draw_weighted<'a, T>(weights: &'a [(T, f32)], rng: &mut SmallRng) -> Option<&'a T> {
    let total: f32 = weights.iter().map(|(_, w)| *w).sum();
    if total <= 0.0 {
        return None;
    }
    let roll: f32 = rng.gen_range(0.0..total);
    let mut cumulative = 0.0;
    for (item, weight) in weights {
        cumulative += *weight;
        if roll < cumulative {
            return Some(item);
        }
    }
    weights.last().map(|(item, _)| item) // floating-point guard
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::morphospace::{RawTraitNode, TraitAxis, TraitGraph, TraitSet};
    use rand::SeedableRng;

    // A tiny graph: root → {a (near), b (far)}; c requires a.
    fn test_graph() -> TraitGraph {
        TraitGraph::from_raw(&[
            RawTraitNode {
                name: "root",
                axis: TraitAxis::Metabolism,
                tier: 0,
                prerequisites: &[],
                exclusions: &[],
                proximity: &[("a", 0.8), ("b", 0.2)],
                reversal_cost: 0.0,
                base_energy_cost: 0.0,
                tags: &[],
            },
            RawTraitNode {
                name: "a",
                axis: TraitAxis::Organization,
                tier: 0,
                prerequisites: &[],
                exclusions: &[],
                proximity: &[("c", 0.5)],
                reversal_cost: 0.0,
                base_energy_cost: 0.0,
                tags: &[],
            },
            RawTraitNode {
                name: "b",
                axis: TraitAxis::Structure,
                tier: 1,
                prerequisites: &[],
                exclusions: &[],
                proximity: &[],
                reversal_cost: 0.0,
                base_energy_cost: 0.0,
                tags: &[],
            },
            RawTraitNode {
                name: "c",
                axis: TraitAxis::Motility,
                tier: 2,
                prerequisites: &["a"],
                exclusions: &[],
                proximity: &[],
                reversal_cost: 0.0,
                base_energy_cost: 0.0,
                tags: &[],
            },
        ])
    }

    fn root_genome(g: &TraitGraph) -> TraitSet {
        [g.id_of("root").unwrap()].into_iter().collect()
    }

    #[test]
    fn candidates_are_reachable_neighbors_only() {
        let g = test_graph();
        let genome = root_genome(&g);
        let w = candidate_weights(
            &g,
            &genome,
            &FactContext::new(),
            &WalkParams::default(),
            &NeutralPayoff,
        );
        let ids: Vec<_> = w.iter().map(|(id, _)| *id).collect();
        // a and b are proximity neighbors of root and legal; c is not a neighbor
        // of the genome yet (needs `a` first), so it is not a candidate.
        assert!(ids.contains(&g.id_of("a").unwrap()));
        assert!(ids.contains(&g.id_of("b").unwrap()));
        assert!(!ids.contains(&g.id_of("c").unwrap()));
    }

    #[test]
    fn prerequisite_gate_blocks_illegal_steps() {
        let g = test_graph();
        // Genome has root + a: now c (requires a) is a legal neighbor of a.
        let genome: TraitSet = [g.id_of("root").unwrap(), g.id_of("a").unwrap()]
            .into_iter()
            .collect();
        let w = candidate_weights(
            &g,
            &genome,
            &FactContext::new(),
            &WalkParams::default(),
            &NeutralPayoff,
        );
        let ids: Vec<_> = w.iter().map(|(id, _)| *id).collect();
        assert!(
            ids.contains(&g.id_of("c").unwrap()),
            "c reachable once a present"
        );
    }

    #[test]
    fn step_is_deterministic_for_a_seed() {
        let g = test_graph();
        let genome = root_genome(&g);
        let step = |seed: u64| {
            let mut rng = SmallRng::seed_from_u64(seed);
            biased_walk_step(
                &g,
                &genome,
                &FactContext::new(),
                &WalkParams::default(),
                &NeutralPayoff,
                &mut rng,
            )
        };
        assert_eq!(step(7), step(7), "same seed → same step");
        assert!(step(7).is_some());
    }

    #[test]
    fn novelty_low_temp_favors_high_proximity() {
        // Earth-like (temp 0): the near neighbor should dominate the far one more
        // than at alien temp (1.0).
        let g = test_graph();
        let genome = root_genome(&g);
        let a = g.id_of("a").unwrap();
        let b = g.id_of("b").unwrap();
        let weight_of = |temp: f32, id: TraitId| {
            candidate_weights(
                &g,
                &genome,
                &FactContext::new(),
                &WalkParams {
                    novelty_temperature: temp,
                },
                &NeutralPayoff,
            )
            .into_iter()
            .find(|(c, _)| *c == id)
            .unwrap()
            .1
        };
        let ratio_cold = weight_of(0.0, a) / weight_of(0.0, b);
        let ratio_alien = weight_of(1.0, a) / weight_of(1.0, b);
        assert!(
            ratio_cold > ratio_alien,
            "low temperature must favor the near neighbor more (cold {ratio_cold} vs alien {ratio_alien})"
        );
    }

    #[test]
    fn loss_candidates_respect_prerequisite_integrity() {
        let g = test_graph();
        // Genome root + a + c, where c requires a.
        let genome: TraitSet = [
            g.id_of("root").unwrap(),
            g.id_of("a").unwrap(),
            g.id_of("c").unwrap(),
        ]
        .into_iter()
        .collect();
        let ids: Vec<_> = loss_candidates(&g, &genome)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        // `a` is load-bearing (c depends on it) → not losable; root and c are.
        assert!(
            !ids.contains(&g.id_of("a").unwrap()),
            "cannot strand c's prereq"
        );
        assert!(ids.contains(&g.id_of("c").unwrap()));
        assert!(ids.contains(&g.id_of("root").unwrap()));
        // A single-trait genome can never lose its last trait.
        let solo: TraitSet = [g.id_of("root").unwrap()].into_iter().collect();
        assert!(loss_candidates(&g, &solo).is_empty());
    }

    #[test]
    fn loss_weight_falls_with_reversal_cost() {
        // Two sibling leaves off root, one sticky (high reversal_cost).
        let g = TraitGraph::from_raw(&[
            RawTraitNode {
                name: "root",
                axis: TraitAxis::Metabolism,
                tier: 0,
                prerequisites: &[],
                exclusions: &[],
                proximity: &[("cheap", 0.5), ("sticky", 0.5)],
                reversal_cost: 0.0,
                base_energy_cost: 0.0,
                tags: &[],
            },
            RawTraitNode {
                name: "cheap",
                axis: TraitAxis::Size,
                tier: 1,
                prerequisites: &[],
                exclusions: &[],
                proximity: &[],
                reversal_cost: 0.0,
                base_energy_cost: 0.0,
                tags: &[],
            },
            RawTraitNode {
                name: "sticky",
                axis: TraitAxis::Size,
                tier: 1,
                prerequisites: &[],
                exclusions: &[],
                proximity: &[],
                reversal_cost: 9.0,
                base_energy_cost: 0.0,
                tags: &[],
            },
        ]);
        let genome: TraitSet = [
            g.id_of("root").unwrap(),
            g.id_of("cheap").unwrap(),
            g.id_of("sticky").unwrap(),
        ]
        .into_iter()
        .collect();
        let w: std::collections::BTreeMap<_, _> =
            loss_candidates(&g, &genome).into_iter().collect();
        assert!(
            w[&g.id_of("cheap").unwrap()] > w[&g.id_of("sticky").unwrap()],
            "a sticky (high reversal_cost) trait is less likely to be shed"
        );
    }

    #[test]
    fn evolution_step_is_gain_only_without_loss_bias() {
        let g = test_graph();
        let genome: TraitSet = [g.id_of("root").unwrap(), g.id_of("a").unwrap()]
            .into_iter()
            .collect();
        // With loss_bias 0, every draw is a Gain (never a Loss), for many seeds.
        for seed in 0..64u64 {
            let mut rng = SmallRng::seed_from_u64(seed);
            if let Some(step) = biased_evolution_step(
                &g,
                &genome,
                &FactContext::new(),
                &WalkParams::default(),
                &NeutralPayoff,
                0.0,
                &mut rng,
            ) {
                assert!(
                    matches!(step, WalkStep::Gain(_)),
                    "no loss without loss_bias"
                );
            }
        }
        // With a large loss_bias, losses do occur across seeds.
        let saw_loss = (0..64u64).any(|seed| {
            let mut rng = SmallRng::seed_from_u64(seed);
            matches!(
                biased_evolution_step(
                    &g,
                    &genome,
                    &FactContext::new(),
                    &WalkParams::default(),
                    &NeutralPayoff,
                    5.0,
                    &mut rng,
                ),
                Some(WalkStep::Loss(_))
            )
        });
        assert!(saw_loss, "a strong loss_bias should produce reversal steps");
    }

    #[test]
    fn novelty_factor_formula() {
        // temp 0: exponent 1 → high proximity favored. temp 2: exponent −1 → low
        // proximity boosted.
        assert!(novelty_factor(0.8, 0.0) > novelty_factor(0.2, 0.0));
        assert!(novelty_factor(0.2, 2.0) > novelty_factor(0.2, 0.0));
    }
}
