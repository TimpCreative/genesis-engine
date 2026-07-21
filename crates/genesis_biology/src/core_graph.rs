//! The core trait morphospace (Doc 09 §2.9).
//!
//! A coherent starter graph across all §2.2 axes: the microbial progression
//! (chemosynthesis → oxygenation → eukaryote → multicellular), the three
//! Earth-like kingdom basins (producer / animal / fungus), and the reachable
//! sapience line. **The editable source of truth is the data file**
//! `src/data/core_traits.json` (Doc 09 §2.8, §16) — [`core_morphospace`] loads
//! it. [`NODES`] below is the original Rust seed that file was generated from,
//! kept behind `cfg(test)` for regeneration and a parity check.

use crate::morphospace::TraitGraph;

/// Builds the core morphospace graph from the editable data file.
pub fn core_morphospace() -> TraitGraph {
    crate::trait_data::load_core_traits()
}

/// Builds the graph from the Rust seed [`NODES`] (test-only; the parity check in
/// [`crate::trait_data`] asserts it matches the data file).
#[cfg(test)]
pub(crate) fn core_morphospace_from_code() -> TraitGraph {
    TraitGraph::from_raw(NODES)
}

#[cfg(test)]
use crate::morphospace::{RawTraitNode as N, TraitAxis::*, TraitTag::*};

// Convention: `proximity` points *forward* (toward what a trait enables), so the
// biased walk flows outward from a microbial root. Exclusions are symmetrized at
// load, so each pair is listed once.
#[cfg(test)]
pub(crate) const NODES: &[N] = &[
    // ---- Tier 0 — Metabolism ----
    N {
        name: "core:chemosynthesis",
        axis: Metabolism,
        tier: 0,
        prerequisites: &[],
        exclusions: &[],
        proximity: &[
            ("core:anoxygenic_phototrophy", 0.6),
            ("core:heterotrophy", 0.5),
            ("core:unicellular", 0.9),
        ],
        reversal_cost: 3.0,
        base_energy_cost: 1.0,
        tags: &[Autotroph],
    },
    N {
        name: "core:anoxygenic_phototrophy",
        axis: Metabolism,
        tier: 0,
        prerequisites: &["core:chemosynthesis"],
        exclusions: &[],
        proximity: &[("core:oxygenic_photosynthesis", 0.7)],
        reversal_cost: 2.5,
        base_energy_cost: 1.0,
        tags: &[Autotroph],
    },
    N {
        name: "core:oxygenic_photosynthesis",
        axis: Metabolism,
        tier: 0,
        prerequisites: &["core:anoxygenic_phototrophy"],
        exclusions: &[],
        proximity: &[
            ("core:cellulose_wall", 0.6),
            ("core:multicellular", 0.4),
            ("core:colonial", 0.4),
        ],
        reversal_cost: 3.0,
        base_energy_cost: 1.2,
        tags: &[Autotroph, KeyInnovation, ProducerBasin],
    },
    N {
        name: "core:heterotrophy",
        axis: Metabolism,
        tier: 0,
        prerequisites: &[],
        exclusions: &["core:oxygenic_photosynthesis"],
        proximity: &[
            ("core:ciliary", 0.6),
            ("core:chitin", 0.4),
            ("core:diet_generalist", 0.6),
        ],
        reversal_cost: 2.5,
        base_energy_cost: 1.0,
        tags: &[Heterotroph, AnimalBasin],
    },
    N {
        name: "core:absorptive_decomposition",
        axis: Metabolism,
        tier: 0,
        prerequisites: &[],
        exclusions: &["core:oxygenic_photosynthesis"],
        proximity: &[
            ("core:chitin", 0.6),
            ("core:spores", 0.6),
            ("core:detritivore", 0.5),
        ],
        reversal_cost: 2.5,
        base_energy_cost: 0.9,
        tags: &[Decomposer, FungusBasin],
    },
    N {
        name: "core:mixotrophy",
        axis: Metabolism,
        tier: 0,
        prerequisites: &["core:chemosynthesis"],
        exclusions: &[],
        proximity: &[("core:heterotrophy", 0.4)],
        reversal_cost: 1.5,
        base_energy_cost: 1.1,
        tags: &[Autotroph, Heterotroph],
    },
    // ---- Tier 0 — Organization ----
    // Organization is a *progression*, not a contradiction: the walk accumulates
    // the chain (unicellular → eukaryote → colonial → multicellular) as
    // developmental prerequisites, so there is no exclusion here. The reversal
    // mechanic (Doc 09 §2.3) now lets integrity-safe traits be shed, and the
    // presentation collapses superseded grades (`view::visible_traits`), so a
    // multicellular genome reads as its current grade rather than the whole ladder.
    N {
        name: "core:unicellular",
        axis: Organization,
        tier: 0,
        prerequisites: &[],
        exclusions: &[],
        proximity: &[
            ("core:eukaryote", 0.8),
            ("core:binary_fission", 0.7),
            ("core:silica_frustule", 0.4),
        ],
        reversal_cost: 2.0,
        base_energy_cost: 0.5,
        tags: &[],
    },
    // Eukaryogenesis — the differentiated single cell; the gateway to complexity
    // (Doc 09 §3.3 #4). Precedes colonial/multicellular; O₂-gated in the
    // microbial era (P4-3).
    N {
        name: "core:eukaryote",
        axis: Organization,
        tier: 0,
        prerequisites: &["core:unicellular"],
        exclusions: &[],
        proximity: &[("core:colonial", 0.8)],
        reversal_cost: 2.5,
        base_energy_cost: 0.8,
        tags: &[KeyInnovation],
    },
    N {
        name: "core:colonial",
        axis: Organization,
        tier: 0,
        prerequisites: &["core:eukaryote"],
        exclusions: &[],
        proximity: &[("core:multicellular", 0.8)],
        reversal_cost: 1.5,
        base_energy_cost: 0.6,
        tags: &[],
    },
    N {
        name: "core:multicellular",
        axis: Organization,
        tier: 0,
        prerequisites: &["core:colonial"],
        exclusions: &[],
        proximity: &[
            ("core:differentiated_tissue", 0.8),
            ("core:cellulose_wall", 0.5),
            ("core:chitin", 0.5),
            ("core:hydrostatic", 0.5),
            ("core:radial", 0.5),
            ("core:bilateral", 0.5),
            ("core:spores", 0.4),
        ],
        reversal_cost: 3.0,
        base_energy_cost: 1.0,
        tags: &[KeyInnovation],
    },
    N {
        name: "core:differentiated_tissue",
        axis: Organization,
        tier: 0,
        prerequisites: &["core:multicellular"],
        exclusions: &[],
        proximity: &[
            ("core:nerve_net", 0.7),
            ("core:mineral_endoskeleton", 0.5),
            ("core:poikilotherm", 0.5),
            ("core:external_eggs", 0.4),
            ("core:size_large", 0.4),
        ],
        reversal_cost: 2.5,
        base_energy_cost: 1.1,
        tags: &[KeyInnovation],
    },
    // ---- Tier 1 — Structure ----
    N {
        name: "core:cellulose_wall",
        axis: Structure,
        tier: 1,
        prerequisites: &["core:multicellular"],
        exclusions: &[
            "core:chitin",
            "core:mineral_endoskeleton",
            "core:mineral_exoskeleton",
        ],
        proximity: &[
            ("core:bark", 0.6),
            ("core:seed_analog", 0.6),
            ("core:sessile", 0.7),
        ],
        reversal_cost: 2.5,
        base_energy_cost: 0.8,
        tags: &[ProducerBasin],
    },
    N {
        name: "core:chitin",
        axis: Structure,
        tier: 1,
        prerequisites: &["core:multicellular"],
        exclusions: &["core:cellulose_wall"],
        proximity: &[
            ("core:mineral_exoskeleton", 0.6),
            ("core:cuticle", 0.6),
            ("core:segmented", 0.4),
        ],
        reversal_cost: 2.0,
        base_energy_cost: 0.8,
        tags: &[],
    },
    N {
        name: "core:mineral_endoskeleton",
        axis: Structure,
        tier: 1,
        prerequisites: &["core:differentiated_tissue"],
        exclusions: &["core:mineral_exoskeleton", "core:cellulose_wall"],
        proximity: &[
            ("core:limbed_walk", 0.5),
            ("core:scale", 0.5),
            ("core:size_mega", 0.4),
        ],
        reversal_cost: 2.5,
        base_energy_cost: 1.0,
        tags: &[AnimalBasin],
    },
    N {
        name: "core:mineral_exoskeleton",
        axis: Structure,
        tier: 1,
        prerequisites: &["core:chitin"],
        exclusions: &["core:mineral_endoskeleton", "core:cellulose_wall"],
        proximity: &[("core:shell", 0.6), ("core:scale", 0.4)],
        reversal_cost: 2.0,
        base_energy_cost: 1.0,
        tags: &[],
    },
    N {
        name: "core:hydrostatic",
        axis: Structure,
        tier: 1,
        prerequisites: &["core:multicellular"],
        exclusions: &[],
        proximity: &[("core:muscular_crawl", 0.5)],
        reversal_cost: 1.0,
        base_energy_cost: 0.6,
        tags: &[],
    },
    N {
        name: "core:silica_frustule",
        axis: Structure,
        tier: 1,
        prerequisites: &["core:unicellular"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 1.5,
        base_energy_cost: 0.7,
        tags: &[],
    },
    // ---- Tier 1 — Symmetry ----
    N {
        name: "core:radial",
        axis: Symmetry,
        tier: 1,
        prerequisites: &["core:multicellular"],
        exclusions: &["core:bilateral"],
        proximity: &[("core:sessile", 0.5), ("core:filter_feeder", 0.5)],
        reversal_cost: 1.5,
        base_energy_cost: 0.5,
        tags: &[],
    },
    N {
        name: "core:bilateral",
        axis: Symmetry,
        tier: 1,
        prerequisites: &["core:multicellular"],
        exclusions: &["core:radial"],
        proximity: &[
            ("core:muscular_crawl", 0.6),
            ("core:ganglia", 0.5),
            ("core:segmented", 0.5),
        ],
        reversal_cost: 2.0,
        base_energy_cost: 0.5,
        tags: &[AnimalBasin],
    },
    N {
        name: "core:asymmetric",
        axis: Symmetry,
        tier: 1,
        prerequisites: &["core:multicellular"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 0.5,
        base_energy_cost: 0.4,
        tags: &[],
    },
    N {
        name: "core:segmented",
        axis: Symmetry,
        tier: 1,
        prerequisites: &["core:bilateral"],
        exclusions: &[],
        proximity: &[("core:limbed_walk", 0.4)],
        reversal_cost: 1.5,
        base_energy_cost: 0.6,
        tags: &[],
    },
    // ---- Tier 2 — Motility ----
    N {
        name: "core:sessile",
        axis: Motility,
        tier: 2,
        prerequisites: &["core:multicellular"],
        exclusions: &[
            "core:muscular_crawl",
            "core:limbed_walk",
            "core:swim",
            "core:jet",
            "core:powered_flight",
        ],
        proximity: &[("core:filter_feeder", 0.5), ("core:bark", 0.4)],
        reversal_cost: 1.5,
        base_energy_cost: 0.4,
        tags: &[],
    },
    N {
        name: "core:ciliary",
        axis: Motility,
        tier: 2,
        prerequisites: &["core:unicellular"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 1.0,
        base_energy_cost: 0.5,
        tags: &[Motile],
    },
    N {
        name: "core:muscular_crawl",
        axis: Motility,
        tier: 2,
        prerequisites: &["core:nerve_net"],
        exclusions: &[],
        proximity: &[
            ("core:limbed_walk", 0.6),
            ("core:swim", 0.6),
            ("core:jet", 0.4),
        ],
        reversal_cost: 1.5,
        base_energy_cost: 0.9,
        tags: &[Motile],
    },
    N {
        name: "core:limbed_walk",
        axis: Motility,
        tier: 2,
        prerequisites: &["core:muscular_crawl", "core:bilateral"],
        exclusions: &[],
        proximity: &[("core:powered_flight", 0.4), ("core:homeotherm", 0.3)],
        reversal_cost: 1.5,
        base_energy_cost: 1.0,
        tags: &[Motile],
    },
    N {
        name: "core:swim",
        axis: Motility,
        tier: 2,
        prerequisites: &["core:muscular_crawl"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 1.0,
        base_energy_cost: 0.9,
        tags: &[Motile],
    },
    N {
        name: "core:jet",
        axis: Motility,
        tier: 2,
        prerequisites: &["core:muscular_crawl"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 1.0,
        base_energy_cost: 1.0,
        tags: &[Motile],
    },
    N {
        name: "core:powered_flight",
        axis: Motility,
        tier: 2,
        prerequisites: &["core:limbed_walk"],
        exclusions: &["core:size_mega"],
        proximity: &[("core:feather", 0.4)],
        reversal_cost: 2.0,
        base_energy_cost: 1.4,
        tags: &[Motile, KeyInnovation],
    },
    // ---- Tier 2 — Thermoregulation ----
    N {
        name: "core:poikilotherm",
        axis: Thermoregulation,
        tier: 2,
        prerequisites: &["core:differentiated_tissue"],
        exclusions: &["core:homeotherm"],
        proximity: &[],
        reversal_cost: 1.0,
        base_energy_cost: 0.5,
        tags: &[],
    },
    N {
        name: "core:homeotherm",
        axis: Thermoregulation,
        tier: 2,
        prerequisites: &["core:differentiated_tissue"],
        exclusions: &["core:poikilotherm"],
        proximity: &[
            ("core:fur", 0.5),
            ("core:feather", 0.5),
            ("core:internal_gestation", 0.4),
        ],
        reversal_cost: 2.5,
        base_energy_cost: 1.3,
        tags: &[KeyInnovation],
    },
    // ---- Tier 2→5 — Nervous / cognition ----
    N {
        name: "core:nerve_net",
        axis: Nervous,
        tier: 2,
        prerequisites: &["core:multicellular"],
        exclusions: &[],
        proximity: &[
            ("core:ganglia", 0.7),
            ("core:muscular_crawl", 0.5),
            ("core:chemoreception", 0.5),
        ],
        reversal_cost: 2.5,
        base_energy_cost: 0.9,
        tags: &[AnimalBasin],
    },
    N {
        name: "core:ganglia",
        axis: Nervous,
        tier: 2,
        prerequisites: &["core:nerve_net"],
        exclusions: &[],
        proximity: &[
            ("core:centralized_brain", 0.7),
            ("core:photoreception", 0.5),
            ("core:eusocial", 0.4),
        ],
        reversal_cost: 2.5,
        base_energy_cost: 1.0,
        tags: &[],
    },
    N {
        name: "core:centralized_brain",
        axis: Nervous,
        tier: 2,
        prerequisites: &["core:ganglia"],
        exclusions: &[],
        proximity: &[
            ("core:complex_cognition", 0.6),
            ("core:image_eye", 0.5),
            ("core:herd", 0.4),
            ("core:pair_bond", 0.4),
        ],
        reversal_cost: 3.0,
        base_energy_cost: 1.3,
        tags: &[KeyInnovation],
    },
    N {
        name: "core:complex_cognition",
        axis: Nervous,
        tier: 5,
        prerequisites: &["core:centralized_brain"],
        exclusions: &[],
        proximity: &[("core:sapience", 0.4), ("core:eusocial", 0.3)],
        reversal_cost: 3.0,
        base_energy_cost: 1.6,
        tags: &[],
    },
    N {
        name: "core:sapience",
        axis: Nervous,
        tier: 5,
        prerequisites: &["core:complex_cognition"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 4.0,
        base_energy_cost: 2.0,
        tags: &[KeyInnovation],
    },
    // ---- Tier 3 — Sensory ----
    N {
        name: "core:chemoreception",
        axis: Sensory,
        tier: 3,
        prerequisites: &["core:nerve_net"],
        exclusions: &[],
        proximity: &[("core:photoreception", 0.5)],
        reversal_cost: 1.0,
        base_energy_cost: 0.5,
        tags: &[],
    },
    N {
        name: "core:photoreception",
        axis: Sensory,
        tier: 3,
        prerequisites: &["core:nerve_net"],
        exclusions: &[],
        proximity: &[("core:image_eye", 0.6)],
        reversal_cost: 1.5,
        base_energy_cost: 0.6,
        tags: &[],
    },
    N {
        name: "core:image_eye",
        axis: Sensory,
        tier: 3,
        prerequisites: &["core:photoreception", "core:ganglia"],
        exclusions: &[],
        proximity: &[("core:display_coloration", 0.4)],
        reversal_cost: 2.0,
        base_energy_cost: 0.8,
        tags: &[KeyInnovation],
    },
    N {
        name: "core:electroreception",
        axis: Sensory,
        tier: 3,
        prerequisites: &["core:ganglia"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 1.0,
        base_energy_cost: 0.6,
        tags: &[],
    },
    N {
        name: "core:echolocation",
        axis: Sensory,
        tier: 3,
        prerequisites: &["core:centralized_brain"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 1.5,
        base_energy_cost: 0.8,
        tags: &[],
    },
    N {
        name: "core:magnetoreception",
        axis: Sensory,
        tier: 3,
        prerequisites: &["core:ganglia"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 1.0,
        base_energy_cost: 0.5,
        tags: &[],
    },
    // ---- Tier 3 — Reproduction ----
    N {
        name: "core:binary_fission",
        axis: Reproduction,
        tier: 3,
        prerequisites: &["core:unicellular"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 0.5,
        base_energy_cost: 0.3,
        tags: &[],
    },
    N {
        name: "core:spores",
        axis: Reproduction,
        tier: 3,
        prerequisites: &["core:multicellular"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 1.0,
        base_energy_cost: 0.4,
        tags: &[],
    },
    N {
        name: "core:seed_analog",
        axis: Reproduction,
        tier: 3,
        prerequisites: &["core:cellulose_wall"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 1.5,
        base_energy_cost: 0.6,
        tags: &[ProducerBasin],
    },
    N {
        name: "core:external_eggs",
        axis: Reproduction,
        tier: 3,
        prerequisites: &["core:differentiated_tissue"],
        exclusions: &[],
        proximity: &[("core:internal_gestation", 0.4)],
        reversal_cost: 1.0,
        base_energy_cost: 0.5,
        tags: &[],
    },
    N {
        name: "core:internal_gestation",
        axis: Reproduction,
        tier: 3,
        prerequisites: &["core:differentiated_tissue"],
        exclusions: &[],
        proximity: &[("core:pair_bond", 0.3)],
        reversal_cost: 2.0,
        base_energy_cost: 0.9,
        tags: &[],
    },
    // ---- Tier 4 — Integument ----
    N {
        name: "core:naked",
        axis: Integument,
        tier: 4,
        prerequisites: &["core:multicellular"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 0.5,
        base_energy_cost: 0.3,
        tags: &[],
    },
    N {
        name: "core:cuticle",
        axis: Integument,
        tier: 4,
        prerequisites: &["core:chitin"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 1.0,
        base_energy_cost: 0.4,
        tags: &[],
    },
    N {
        name: "core:scale",
        axis: Integument,
        tier: 4,
        prerequisites: &["core:mineral_endoskeleton"],
        exclusions: &[],
        proximity: &[("core:feather", 0.4)],
        reversal_cost: 1.0,
        base_energy_cost: 0.5,
        tags: &[],
    },
    N {
        name: "core:feather",
        axis: Integument,
        tier: 4,
        prerequisites: &["core:homeotherm", "core:scale"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 1.5,
        base_energy_cost: 0.6,
        tags: &[],
    },
    N {
        name: "core:fur",
        axis: Integument,
        tier: 4,
        prerequisites: &["core:homeotherm"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 1.5,
        base_energy_cost: 0.6,
        tags: &[],
    },
    N {
        name: "core:bark",
        axis: Integument,
        tier: 4,
        prerequisites: &["core:cellulose_wall"],
        exclusions: &[],
        proximity: &[("core:size_large", 0.3)],
        reversal_cost: 1.5,
        base_energy_cost: 0.7,
        tags: &[ProducerBasin],
    },
    N {
        name: "core:shell",
        axis: Integument,
        tier: 4,
        prerequisites: &["core:mineral_exoskeleton"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 1.5,
        base_energy_cost: 0.8,
        tags: &[],
    },
    // ---- Tier 4 — Diet specialization ----
    N {
        name: "core:diet_generalist",
        axis: Diet,
        tier: 4,
        prerequisites: &["core:heterotrophy"],
        exclusions: &[],
        proximity: &[("core:folivore", 0.4), ("core:hypercarnivore", 0.4)],
        reversal_cost: 0.5,
        base_energy_cost: 0.4,
        tags: &[Heterotroph],
    },
    N {
        name: "core:folivore",
        axis: Diet,
        tier: 4,
        prerequisites: &["core:heterotrophy"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 1.0,
        base_energy_cost: 0.5,
        tags: &[Heterotroph],
    },
    N {
        name: "core:granivore",
        axis: Diet,
        tier: 4,
        prerequisites: &["core:heterotrophy"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 1.0,
        base_energy_cost: 0.5,
        tags: &[Heterotroph],
    },
    N {
        name: "core:hypercarnivore",
        axis: Diet,
        tier: 4,
        prerequisites: &["core:heterotrophy", "core:muscular_crawl"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 1.5,
        base_energy_cost: 0.8,
        tags: &[Heterotroph],
    },
    N {
        name: "core:filter_feeder",
        axis: Diet,
        tier: 4,
        prerequisites: &["core:heterotrophy"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 1.0,
        base_energy_cost: 0.5,
        tags: &[Heterotroph],
    },
    N {
        name: "core:detritivore",
        axis: Diet,
        tier: 4,
        prerequisites: &["core:heterotrophy"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 0.8,
        base_energy_cost: 0.4,
        tags: &[Heterotroph],
    },
    N {
        name: "core:parasite",
        axis: Diet,
        tier: 4,
        prerequisites: &["core:heterotrophy"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 1.2,
        base_energy_cost: 0.4,
        tags: &[Heterotroph],
    },
    // ---- Tier 5 — Social structure ----
    N {
        name: "core:solitary",
        axis: Social,
        tier: 5,
        prerequisites: &["core:nerve_net"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 0.5,
        base_energy_cost: 0.3,
        tags: &[],
    },
    N {
        name: "core:herd",
        axis: Social,
        tier: 5,
        prerequisites: &["core:centralized_brain"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 1.0,
        base_energy_cost: 0.5,
        tags: &[],
    },
    N {
        name: "core:eusocial",
        axis: Social,
        tier: 5,
        prerequisites: &["core:ganglia"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 2.0,
        base_energy_cost: 0.7,
        tags: &[],
    },
    N {
        name: "core:pair_bond",
        axis: Social,
        tier: 5,
        prerequisites: &["core:centralized_brain"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 1.0,
        base_energy_cost: 0.5,
        tags: &[],
    },
    // ---- Tier 5 — Size class (quantized) ----
    N {
        name: "core:size_micro",
        axis: Size,
        tier: 5,
        prerequisites: &["core:unicellular"],
        exclusions: &[
            "core:size_small",
            "core:size_medium",
            "core:size_large",
            "core:size_mega",
        ],
        proximity: &[],
        reversal_cost: 0.5,
        base_energy_cost: 0.2,
        tags: &[],
    },
    N {
        name: "core:size_small",
        axis: Size,
        tier: 5,
        prerequisites: &["core:multicellular"],
        exclusions: &["core:size_medium", "core:size_large", "core:size_mega"],
        proximity: &[],
        reversal_cost: 0.5,
        base_energy_cost: 0.4,
        tags: &[],
    },
    N {
        name: "core:size_medium",
        axis: Size,
        tier: 5,
        prerequisites: &["core:multicellular"],
        exclusions: &["core:size_large", "core:size_mega"],
        proximity: &[],
        reversal_cost: 0.5,
        base_energy_cost: 0.6,
        tags: &[],
    },
    N {
        name: "core:size_large",
        axis: Size,
        tier: 5,
        prerequisites: &["core:differentiated_tissue"],
        exclusions: &["core:size_mega"],
        proximity: &[],
        reversal_cost: 0.8,
        base_energy_cost: 0.9,
        tags: &[],
    },
    N {
        name: "core:size_mega",
        axis: Size,
        tier: 5,
        prerequisites: &["core:differentiated_tissue", "core:mineral_endoskeleton"],
        exclusions: &["core:powered_flight"],
        proximity: &[],
        reversal_cost: 1.2,
        base_energy_cost: 1.4,
        tags: &[],
    },
    // ---- Tier 6 — Coloration / display ----
    N {
        name: "core:cryptic_coloration",
        axis: Coloration,
        tier: 6,
        prerequisites: &["core:multicellular"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 0.3,
        base_energy_cost: 0.2,
        tags: &[],
    },
    N {
        name: "core:display_coloration",
        axis: Coloration,
        tier: 6,
        prerequisites: &["core:image_eye"],
        exclusions: &[],
        proximity: &[],
        reversal_cost: 0.3,
        base_energy_cost: 0.3,
        tags: &[],
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evolution::{NeutralPayoff, WalkParams, biased_walk_step};
    use crate::morphospace::TraitSet;
    use genesis_rules::FactContext;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    #[test]
    fn graph_builds_and_is_fuller() {
        let g = core_morphospace();
        // All 14 axes represented; a substantial starter graph.
        assert!(g.len() >= 55, "expected a fuller graph, got {}", g.len());
    }

    #[test]
    fn every_reference_resolves() {
        // `from_raw` panics on a dangling ref, so a clean build proves all
        // prerequisite/exclusion/proximity references are valid.
        let g = core_morphospace();
        for node in g.nodes() {
            for p in &node.prerequisites {
                assert!((p.0 as usize) < g.len());
            }
        }
    }

    #[test]
    fn multicellular_needs_the_progression() {
        let g = core_morphospace();
        let multicellular = g.node(g.id_of("core:multicellular").unwrap());
        // Directly gated behind colonial (which is gated behind unicellular).
        assert!(
            multicellular
                .prerequisites
                .contains(&g.id_of("core:colonial").unwrap())
        );
        let brain = g.node(g.id_of("core:centralized_brain").unwrap());
        assert!(
            brain
                .prerequisites
                .contains(&g.id_of("core:ganglia").unwrap())
        );
    }

    /// A greedy walk from a chemosynthetic microbial root reaches multicellular
    /// and a nervous system — the progression Doc 09 §17 #2 expects.
    #[test]
    fn walk_reaches_multicellularity_and_nerves() {
        let g = core_morphospace();
        let mut genome: TraitSet = [
            g.id_of("core:chemosynthesis").unwrap(),
            g.id_of("core:unicellular").unwrap(),
        ]
        .into_iter()
        .collect();
        let env = FactContext::new();
        let params = WalkParams::default();
        let mut rng = SmallRng::seed_from_u64(20260720);

        let multicellular = g.id_of("core:multicellular").unwrap();
        let nerve_net = g.id_of("core:nerve_net").unwrap();
        for _ in 0..400 {
            if genome.contains(multicellular) && genome.contains(nerve_net) {
                break;
            }
            match biased_walk_step(&g, &genome, &env, &params, &NeutralPayoff, &mut rng) {
                Some(step) => {
                    genome.insert(step);
                }
                None => break,
            }
        }
        assert!(
            genome.contains(multicellular),
            "walk should reach multicellularity"
        );
        assert!(
            genome.contains(nerve_net),
            "walk should reach a nervous system"
        );
    }
}
