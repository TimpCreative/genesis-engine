//! Loading the trait morphospace from an editable data file (Doc 09 §2.8, §16).
//!
//! `src/data/core_traits.json` is the **moddable source of truth** for the core
//! trait graph — edit it to add, remove, or re-tune traits (names, tiers, axes,
//! prerequisites, proximity, and player-facing descriptions) without touching
//! Rust. [`core_graph::NODES`](crate::core_graph) is only the seed the file was
//! generated from, kept behind `cfg(test)` for regeneration and a parity check.

use crate::morphospace::{TraitGraph, TraitNodeData};

/// The whole trait graph as editable data — the shape of `core_traits.json`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TraitGraphData {
    pub traits: Vec<TraitNodeData>,
}

/// Loads the core trait graph from the embedded editable data file.
pub fn load_core_traits() -> TraitGraph {
    const JSON: &str = include_str!("data/core_traits.json");
    let data: TraitGraphData =
        serde_json::from_str(JSON).expect("src/data/core_traits.json is malformed content");
    TraitGraph::from_data(&data.traits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::morphospace::raw_to_data;

    /// Bootstrap/regeneration: writes `src/data/core_traits.json` from the Rust
    /// seed (`core_graph::NODES`), preserving any descriptions already present in
    /// the file (merged by name). Run with:
    /// `cargo test -p genesis_biology regenerate_core_traits_json -- --ignored`.
    #[test]
    #[ignore]
    fn regenerate_core_traits_json() {
        use std::collections::BTreeMap;
        let path = "src/data/core_traits.json";
        // Preserve existing descriptions/displays keyed by trait name.
        let existing: BTreeMap<String, (String, String)> = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<TraitGraphData>(&s).ok())
            .map(|d| {
                d.traits
                    .into_iter()
                    .map(|t| (t.name, (t.display, t.description)))
                    .collect()
            })
            .unwrap_or_default();
        let mut traits = raw_to_data(crate::core_graph::NODES);
        for t in &mut traits {
            if let Some((display, description)) = existing.get(&t.name) {
                if !display.is_empty() {
                    t.display = display.clone();
                }
                if !description.is_empty() {
                    t.description = description.clone();
                }
            }
        }
        let json = serde_json::to_string_pretty(&TraitGraphData { traits }).unwrap();
        std::fs::write(path, json).unwrap();
    }

    /// Sanity: `core_traits.json` loads to a coherent graph — a good chunk of
    /// nodes, the microbial roots present, and every prerequisite/proximity
    /// reference resolvable (`from_data` panics on a dangling ref). This is
    /// **not** a parity check against the Rust seed, so the data file can be
    /// edited freely (add traits, retune prereqs) without a test fighting you.
    #[test]
    fn core_traits_json_loads_and_is_coherent() {
        let g = load_core_traits();
        assert!(g.len() >= 50, "core graph is sparse: {} nodes", g.len());
        for core in [
            "core:chemosynthesis",
            "core:oxygenic_photosynthesis",
            "core:heterotrophy",
            "core:multicellular",
        ] {
            assert!(g.id_of(core).is_some(), "missing core trait {core}");
        }
        // Every node has a non-empty display (fallback derives it from the name).
        assert!(g.nodes().iter().all(|n| !n.display.is_empty()));
    }
}
