//! Scientific-name generation (Doc 09 §9) from an editable morpheme file.
//!
//! A binomial is built from the organism's **own traits**: its most-specific
//! trait picks a genus root, the next picks a species-epithet root, and a
//! grammatical suffix is chosen deterministically from the lineage's `name_seed`.
//! So an aquatic hypercarnivore reads *Nataops raptinus*, not a random syllable
//! salad. Morphemes live in `src/data/naming.json` and are moddable (add an alien
//! scheme, or edit the Latin roots). The scheme is chosen by [`NamingScheme`].

use std::collections::BTreeMap;

use genesis_core::data::TraitId;

use crate::morphospace::{TraitGraph, TraitSet};

/// Which naming scheme to draw from (a key into `naming.json`'s `schemes`).
/// Selectable per world; moddable by adding schemes to the data file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum NamingScheme {
    /// Latin/Greek-rooted binomials keyed to traits (the default).
    #[default]
    Latin,
}

impl NamingScheme {
    fn key(self) -> &'static str {
        match self {
            NamingScheme::Latin => "latin",
        }
    }
}

/// One scheme's morpheme tables (the shape of a `schemes.*` entry).
#[derive(Clone, Debug, serde::Deserialize)]
struct SchemeData {
    axis_roots: BTreeMap<String, Vec<String>>,
    trait_roots: BTreeMap<String, Vec<String>>,
    genus_suffixes: Vec<String>,
    epithet_suffixes: Vec<String>,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct NamingFile {
    schemes: BTreeMap<String, SchemeData>,
}

/// A loaded naming scheme ready to generate names.
#[derive(Clone, Debug)]
pub struct Namer {
    scheme: SchemeData,
}

impl Namer {
    /// Loads a scheme from the embedded editable morpheme file.
    pub fn load(scheme: NamingScheme) -> Self {
        const JSON: &str = include_str!("data/naming.json");
        let file: NamingFile =
            serde_json::from_str(JSON).expect("src/data/naming.json is malformed content");
        let scheme = file
            .schemes
            .get(scheme.key())
            .cloned()
            .expect("naming.json missing the requested scheme");
        Self { scheme }
    }

    /// Picks a Latin root for a trait: its specific override, else its axis root,
    /// else a fallback from the trait's display name — chosen by `seed`.
    fn root(&self, graph: &TraitGraph, trait_id: TraitId, seed: u64) -> String {
        let node = graph.node(trait_id);
        let pool = self
            .scheme
            .trait_roots
            .get(&node.name)
            .or_else(|| self.scheme.axis_roots.get(axis_key(node.axis)));
        match pool.filter(|p| !p.is_empty()) {
            Some(p) => p[(seed % p.len() as u64) as usize].clone(),
            None => node
                .display
                .chars()
                .take(4)
                .collect::<String>()
                .to_lowercase(),
        }
    }

    /// The two most-namable (shallowest-tier, then lowest-id) traits of a genome —
    /// the genus driver and the epithet driver.
    fn signature_traits(&self, graph: &TraitGraph, genome: &TraitSet) -> (TraitId, TraitId) {
        let mut ts: Vec<TraitId> = genome.iter().collect();
        ts.sort_by_key(|&id| {
            let n = graph.node(id);
            (std::cmp::Reverse(n.tier), id.0)
        });
        let primary = ts.first().copied().unwrap_or(TraitId(0));
        let secondary = ts.get(1).copied().unwrap_or(primary);
        (primary, secondary)
    }

    /// A capitalized genus name for a clade/lineage, from its defining traits.
    pub fn genus(&self, graph: &TraitGraph, genome: &TraitSet, seed: u64) -> String {
        let (primary, _) = self.signature_traits(graph, genome);
        let root = self.root(graph, primary, seed);
        let suffix = pick(&self.scheme.genus_suffixes, seed.rotate_left(7));
        capitalize(&join(&root, &suffix))
    }

    /// A full `(Genus, epithet)` binomial for a species, from its traits + seed.
    pub fn binomial(&self, graph: &TraitGraph, genome: &TraitSet, seed: u64) -> (String, String) {
        let (primary, secondary) = self.signature_traits(graph, genome);
        let genus = capitalize(&join(
            &self.root(graph, primary, seed),
            &pick(&self.scheme.genus_suffixes, seed.rotate_left(7)),
        ));
        let epithet = join(
            &self.root(graph, secondary, seed.rotate_left(23)),
            &pick(&self.scheme.epithet_suffixes, seed.rotate_left(41)),
        );
        (genus, epithet.to_lowercase())
    }
}

fn pick(pool: &[String], seed: u64) -> String {
    if pool.is_empty() {
        return String::new();
    }
    pool[(seed % pool.len() as u64) as usize].clone()
}

/// Joins a root and a suffix with vowel elision — drops the root's trailing vowel
/// when the suffix also starts with one, so `phyto`+`osus` reads `phytosus`, not
/// `phytoosus`.
fn join(root: &str, suffix: &str) -> String {
    let is_vowel = |c: char| "aeiou".contains(c);
    let root_ends_vowel = root.chars().last().is_some_and(is_vowel);
    let suffix_starts_vowel = suffix.chars().next().is_some_and(is_vowel);
    if root_ends_vowel && suffix_starts_vowel {
        format!("{}{}", &root[..root.len() - 1], suffix)
    } else {
        format!("{root}{suffix}")
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn axis_key(axis: crate::morphospace::TraitAxis) -> &'static str {
    use crate::morphospace::TraitAxis::*;
    match axis {
        Metabolism => "Metabolism",
        Organization => "Organization",
        Structure => "Structure",
        Symmetry => "Symmetry",
        Motility => "Motility",
        Thermoregulation => "Thermoregulation",
        Nervous => "Nervous",
        Sensory => "Sensory",
        Reproduction => "Reproduction",
        Integument => "Integument",
        Diet => "Diet",
        Social => "Social",
        Size => "Size",
        Coloration => "Coloration",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core_graph::core_morphospace;

    fn genome(graph: &TraitGraph, names: &[&str]) -> TraitSet {
        names.iter().map(|n| graph.id_of(n).unwrap()).collect()
    }

    #[test]
    fn binomials_are_latin_deterministic_and_trait_keyed() {
        let graph = core_morphospace();
        let namer = Namer::load(NamingScheme::Latin);
        let swimmer = genome(
            &graph,
            &["core:multicellular", "core:swim", "core:hypercarnivore"],
        );
        let (g1, e1) = namer.binomial(&graph, &swimmer, 12345);
        let (g2, e2) = namer.binomial(&graph, &swimmer, 12345);
        assert_eq!((&g1, &e1), (&g2, &e2), "same inputs → same name");
        // Capitalized genus, lowercase epithet, both non-empty.
        assert!(g1.chars().next().unwrap().is_uppercase());
        assert_eq!(e1, e1.to_lowercase());
        assert!(!g1.is_empty() && !e1.is_empty());
        // Trait-keyed: an aquatic predator draws from swim/predator roots.
        let all = format!("{g1} {e1}");
        assert!(
            ["nat", "nekt", "thalass", "rapt", "pred", "carn"]
                .iter()
                .any(|r| all.to_lowercase().contains(r)),
            "expected an aquatic/predator root in {all}"
        );
        // A different organism gets a different name.
        let plant = genome(
            &graph,
            &["core:multicellular", "core:oxygenic_photosynthesis"],
        );
        let (gp, ep) = namer.binomial(&graph, &plant, 12345);
        assert_ne!((g1, e1), (gp, ep));
    }
}

#[cfg(test)]
mod sample {
    use super::*;
    use crate::core_graph::core_morphospace;
    use crate::guild::core_guilds;
    use crate::speciation::build_radiation;
    use genesis_core::data::GuildId;
    use genesis_core::time::WorldYear;

    #[test]
    #[ignore]
    fn print_sample_names() {
        let graph = core_morphospace();
        let roster = core_guilds(&graph);
        let ledger = build_radiation(&graph, &roster, 42, WorldYear(400_000_000));
        let namer = Namer::load(NamingScheme::Latin);
        let mut n = 0;
        for l in ledger.iter() {
            if l.guild != GuildId::NONE && l.parent.is_some() {
                let (g, e) = namer.binomial(&graph, &l.trait_set, l.name_seed);
                println!("NAME: {g} {e}");
                n += 1;
                if n >= 16 {
                    break;
                }
            }
        }
    }
}
