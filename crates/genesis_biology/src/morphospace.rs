//! The trait morphospace — the "physics of life" (Doc 09 §2).
//!
//! A graph of trait nodes on functional **axes** and evolutionary **tiers**, with
//! four edge kinds (prerequisite, exclusion, proximity, reversal asymmetry). A
//! genome ([`TraitSet`]) is a subset of the graph; evolution is a biased walk
//! over it ([`crate::evolution`]).
//!
//! Nodes are authored by namespaced **string** content id and resolved to dense
//! [`TraitId`] indices at load (Doc 09 §2.1) — save files reference strings, the
//! runtime uses the index.

use std::collections::{BTreeMap, BTreeSet};

use genesis_core::data::TraitId;

/// Functional dimension a trait contributes to (Doc 09 §2.2). Governs which role
/// a trait fills; distinct from `tier`, which governs taxonomic rank (Doc 09 §9).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TraitAxis {
    Metabolism,
    Organization,
    Structure,
    Symmetry,
    Motility,
    Thermoregulation,
    Nervous,
    Sensory,
    Reproduction,
    Integument,
    Diet,
    Social,
    Size,
    Coloration,
}

/// A tag consumed by the rule engine and by morphology/description generation
/// (Doc 09 §2.1). Kept a small, extensible set for the foundation.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TraitTag {
    /// An autotrophic metabolism (makes its own food).
    Autotroph,
    /// A heterotrophic metabolism (consumes others).
    Heterotroph,
    /// A decomposer / absorptive metabolism.
    Decomposer,
    /// Confers active movement.
    Motile,
    /// A key innovation that gates a major grade (Doc 09 §3.3).
    KeyInnovation,
    /// Part of the producer (plant-analog) basin.
    ProducerBasin,
    /// Part of the animal-analog basin.
    AnimalBasin,
    /// Part of the fungus-analog basin.
    FungusBasin,
}

/// A resolved trait node (string refs turned into dense [`TraitId`]s).
#[derive(Clone, Debug)]
pub struct TraitNode {
    pub id: TraitId,
    /// Namespaced content id string, e.g. `"core:image_eye"`.
    pub name: String,
    /// Human-readable name, e.g. "camera eye" (from the data file; falls back to
    /// the de-prefixed `name` for code-authored graphs).
    pub display: String,
    /// One-line player-facing definition of the trait (from the data file; empty
    /// for code-authored graphs).
    pub description: String,
    pub axis: TraitAxis,
    /// 0 = deep/fundamental … 6 = shallow/leaf (drives taxonomy, Doc 09 §9).
    pub tier: u8,
    /// Directed hard gate: unreachable until every prerequisite is present.
    pub prerequisites: Vec<TraitId>,
    /// Hard contradictions that cannot coexist (symmetrized at load).
    pub exclusions: Vec<TraitId>,
    /// Weighted soft neighbors — co-occurrence / reachability affinity.
    pub proximity: Vec<(TraitId, f32)>,
    /// How hard this trait is to LOSE once gained (directional asymmetry).
    pub reversal_cost: f32,
    /// Metabolic overhead debit; payoff is contextual (Doc 09 §2.5).
    pub base_energy_cost: f32,
    pub tags: Vec<TraitTag>,
}

/// Author-facing node: string refs, resolved by [`TraitGraph::from_raw`].
pub struct RawTraitNode {
    pub name: &'static str,
    pub axis: TraitAxis,
    pub tier: u8,
    pub prerequisites: &'static [&'static str],
    pub exclusions: &'static [&'static str],
    pub proximity: &'static [(&'static str, f32)],
    pub reversal_cost: f32,
    pub base_energy_cost: f32,
    pub tags: &'static [TraitTag],
}

/// The **editable data-file** form of a trait node: owned strings, (de)serialized
/// from `src/data/core_traits.json` (Doc 09 §2.8). This is the moddable content
/// form — add or edit traits here rather than in Rust.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TraitNodeData {
    pub name: String,
    /// Human-readable name; falls back to the de-prefixed `name` when empty.
    #[serde(default)]
    pub display: String,
    /// One-line player-facing definition.
    #[serde(default)]
    pub description: String,
    pub axis: TraitAxis,
    pub tier: u8,
    #[serde(default)]
    pub prerequisites: Vec<String>,
    #[serde(default)]
    pub exclusions: Vec<String>,
    #[serde(default)]
    pub proximity: Vec<(String, f32)>,
    pub reversal_cost: f32,
    pub base_energy_cost: f32,
    #[serde(default)]
    pub tags: Vec<TraitTag>,
}

/// The de-prefixed, spaced human name for a content id (`core:limbed_walk` →
/// "limbed walk"), the display fallback when the data file leaves it blank.
pub fn derive_display(name: &str) -> String {
    name.trim_start_matches("core:").replace('_', " ")
}

/// Converts code-authored [`RawTraitNode`]s into the editable data form — used to
/// generate the initial `core_traits.json` and by [`TraitGraph::from_raw`].
pub fn raw_to_data(raw: &[RawTraitNode]) -> Vec<TraitNodeData> {
    raw.iter()
        .map(|r| TraitNodeData {
            name: r.name.to_string(),
            display: derive_display(r.name),
            description: String::new(),
            axis: r.axis,
            tier: r.tier,
            prerequisites: r.prerequisites.iter().map(|s| s.to_string()).collect(),
            exclusions: r.exclusions.iter().map(|s| s.to_string()).collect(),
            proximity: r
                .proximity
                .iter()
                .map(|(n, w)| (n.to_string(), *w))
                .collect(),
            reversal_cost: r.reversal_cost,
            base_energy_cost: r.base_energy_cost,
            tags: r.tags.to_vec(),
        })
        .collect()
}

/// A genome — a subset of the morphospace (Doc 09 §2). Ordered for determinism.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TraitSet(BTreeSet<TraitId>);

impl TraitSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn contains(&self, id: TraitId) -> bool {
        self.0.contains(&id)
    }

    pub fn insert(&mut self, id: TraitId) -> bool {
        self.0.insert(id)
    }

    /// Removes a trait (the reversal/loss step, Doc 09 §2.3). Returns whether it
    /// was present.
    pub fn remove(&mut self, id: TraitId) -> bool {
        self.0.remove(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = TraitId> + '_ {
        self.0.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl FromIterator<TraitId> for TraitSet {
    fn from_iter<I: IntoIterator<Item = TraitId>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

/// The loaded trait morphospace: a dense node table plus a name index. Static
/// content, built once, cache-resident (Doc 09 §2.9).
#[derive(Clone, Debug)]
pub struct TraitGraph {
    nodes: Vec<TraitNode>,
    by_name: BTreeMap<String, TraitId>,
}

impl TraitGraph {
    /// Resolves author-facing raw (Rust) nodes into the dense graph. A thin
    /// adapter over [`Self::from_data`]; used by tests and any code-authored graph.
    pub fn from_raw(raw: &[RawTraitNode]) -> Self {
        Self::from_data(&raw_to_data(raw))
    }

    /// Resolves editable data-file nodes into the dense graph. `TraitId`s are
    /// assigned in authored order (deterministic). Exclusions are **symmetrized**
    /// (if A excludes B, B excludes A). Panics on an unknown reference — the core
    /// graph is engine content, so a dangling ref is a content bug.
    pub fn from_data(data: &[TraitNodeData]) -> Self {
        // Pass 1: assign ids by authored order.
        let mut by_name = BTreeMap::new();
        for (i, r) in data.iter().enumerate() {
            if by_name.insert(r.name.clone(), TraitId(i as u32)).is_some() {
                panic!("duplicate trait id: {}", r.name);
            }
        }
        let resolve = |name: &str| -> TraitId {
            *by_name
                .get(name)
                .unwrap_or_else(|| panic!("unknown trait reference: {name}"))
        };

        // Pass 2: resolve refs.
        let mut nodes: Vec<TraitNode> = data
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let id = TraitId(i as u32);
                let prerequisites: Vec<TraitId> =
                    r.prerequisites.iter().map(|n| resolve(n)).collect();
                let exclusions: Vec<TraitId> = r.exclusions.iter().map(|n| resolve(n)).collect();
                let proximity: Vec<(TraitId, f32)> =
                    r.proximity.iter().map(|(n, w)| (resolve(n), *w)).collect();
                assert!(
                    !prerequisites.contains(&id) && !exclusions.contains(&id),
                    "trait {} references itself",
                    r.name
                );
                let display = if r.display.is_empty() {
                    derive_display(&r.name)
                } else {
                    r.display.clone()
                };
                TraitNode {
                    id,
                    name: r.name.clone(),
                    display,
                    description: r.description.clone(),
                    axis: r.axis,
                    tier: r.tier,
                    prerequisites,
                    exclusions,
                    proximity,
                    reversal_cost: r.reversal_cost,
                    base_energy_cost: r.base_energy_cost,
                    tags: r.tags.clone(),
                }
            })
            .collect();

        // Symmetrize exclusions so a candidate check against the genome suffices.
        let pairs: Vec<(TraitId, TraitId)> = nodes
            .iter()
            .flat_map(|n| n.exclusions.iter().map(move |&e| (n.id, e)))
            .collect();
        for (a, b) in pairs {
            let node = &mut nodes[b.0 as usize];
            if !node.exclusions.contains(&a) {
                node.exclusions.push(a);
            }
        }

        Self { nodes, by_name }
    }

    /// Node by id.
    pub fn node(&self, id: TraitId) -> &TraitNode {
        &self.nodes[id.0 as usize]
    }

    /// Id for a namespaced content-id string.
    pub fn id_of(&self, name: &str) -> Option<TraitId> {
        self.by_name.get(name).copied()
    }

    /// Number of trait nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// All nodes, in id order.
    pub fn nodes(&self) -> &[TraitNode] {
        &self.nodes
    }

    /// The candidate next-steps from a genome: proximity neighbors of any genome
    /// member that are not already present, with the **strongest** affinity edge
    /// weight to the genome. Legality (prereqs/exclusions) is filtered by the
    /// walk via the rule engine (Doc 09 §2.4). Returned in ascending `TraitId`
    /// order for determinism.
    pub fn candidate_steps(&self, genome: &TraitSet) -> Vec<(TraitId, f32)> {
        let mut best: BTreeMap<TraitId, f32> = BTreeMap::new();
        for member in genome.iter() {
            for &(neighbor, weight) in &self.node(member).proximity {
                if genome.contains(neighbor) {
                    continue;
                }
                best.entry(neighbor)
                    .and_modify(|w| {
                        if weight > *w {
                            *w = weight;
                        }
                    })
                    .or_insert(weight);
            }
        }
        best.into_iter().collect()
    }
}
