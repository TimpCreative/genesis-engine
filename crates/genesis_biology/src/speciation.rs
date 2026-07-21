//! Speciation and extinction — building the recorded tree of life (Doc 09 §6–§7).
//!
//! P4-8/9, simplified but real: at the multicellular threshold the biosphere
//! radiates. From LUCA it branches into kingdom clades (by metabolism), then each
//! clade radiates by the **biased walk** (§2.4) — every child gains one trait,
//! so descent-with-modification is real and clades resemble their ancestors.
//! Leaf lineages specialize into guilds; a deterministic hazard marks some
//! extinct (greyed in the tree). The whole forest is a pure function of the
//! world seed, recorded once in the [`Ledger`].
//!
//! Not yet the full §6/§7: no per-tick speciation triggers (allopatry, niche
//! divergence) tied to geography, and extinction is a flat hazard, not a
//! selective mass extinction. But the *shape* — a branching family tree from
//! LUCA with coherent inheritance and extinct lines — is honest.

use genesis_core::data::GuildId;
use genesis_core::time::WorldYear;
use rand::SeedableRng;
use rand::rngs::SmallRng;

use crate::evolution::{NeutralPayoff, WalkParams, biased_walk_step};
use crate::guild::{GuildRoster, fills_guild, rule_specificity};
use crate::ledger::{Ledger, LineageRecord};
use crate::morphospace::{TraitGraph, TraitSet};
use crate::province::Realm;
use genesis_rules::FactContext;

const MAX_DEPTH: usize = 6;
/// Depth from which a lineage is specialized enough to fill a guild (a "species").
const LEAF_DEPTH: usize = 4;
const EXTINCT_PERCENT: u64 = 35;
/// Years between successive branch generations (deeper = later, for time-aware
/// tree growth).
const GENERATION_SPAN_YEARS: i64 = 40_000_000;
/// Biogeographic regions the world is divided into, giving each area its own
/// endemic radiations (Doc 09 §6.4); the adapter maps each hex to a region by
/// geography.
pub const BIOGEOGRAPHIC_REGIONS: u16 = 12;
/// Lineages per (kingdom, region) subtree — bounds each region's subtree so the
/// budget is shared fairly rather than the first region eating it all.
const REGION_BUDGET: usize = 11;
/// The largest fraction of worlds that stall microbial at the lowest
/// `complexity_pressure` (Doc 09 §3.3).
const MAX_STALL_FRACTION: f32 = 0.6;
/// How long the microbial biosphere (LUCA → basal life) predates the
/// multicellular radiation, so LUCA originates at biogenesis, not world start.
const MICROBIAL_ERA_SPAN: i64 = 200_000_000;

fn mix(a: u64, b: u64) -> u64 {
    let mut z = a
        .wrapping_add(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(b.rotate_left(31));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Builds the recorded tree of life for a world (Doc 09 §6). `base_year` is the
/// multicellular radiation year; branches arise progressively after it.
pub fn build_radiation(
    graph: &TraitGraph,
    roster: &GuildRoster,
    seed: u64,
    base_year: WorldYear,
) -> Ledger {
    let mut ledger = Ledger::default();
    let id_of = |name: &str| graph.id_of(name).expect("core trait");

    // LUCA and the basal biosphere emerge at biogenesis — the start of the
    // microbial era, `MICROBIAL_ERA_SPAN` before the multicellular radiation
    // (`base_year`) — not at world start. So the Tree of Life shows nothing until
    // life has actually emerged (no LUCA hovering over a lifeless young planet).
    let emergence_year = WorldYear(base_year.value().saturating_sub(MICROBIAL_ERA_SPAN).max(0));

    // LUCA.
    let root_genome: TraitSet = [id_of("core:chemosynthesis"), id_of("core:unicellular")]
        .into_iter()
        .collect();
    let root = ledger.push(LineageRecord {
        id: genesis_core::data::LineageId::NONE,
        parent: None,
        origin_year: emergence_year,
        extinction_year: None,
        trait_set: root_genome.clone(),
        trait_delta: None,
        guild: GuildId::NONE,
        region: None,
        name_seed: mix(seed, 0),
    });

    // Cosmopolitan basal life (Doc 09 §3.4): a producer and a decomposer in BOTH
    // realms, tagged `region: None` so they occur in every biogeographic region.
    // This guarantees basal life blankets every habitable hex — land and ocean —
    // underneath the endemic radiations. Without it, every radiated leaf collapses
    // onto the terrestrial `producer` guild and all four marine guilds stay empty,
    // leaving ~all ocean hexes lifeless. Immortal (never marked extinct).
    let gid = |name: &str| {
        roster
            .iter()
            .find(|g| g.name == name)
            .map(|g| g.id)
            .unwrap_or(GuildId::NONE)
    };
    let photo = id_of("core:oxygenic_photosynthesis");
    let decomp = id_of("core:absorptive_decomposition");
    for (defining, guild_name) in [
        (photo, "producer"),
        (decomp, "decomposer"),
        (photo, "phytoplankton"),
        (decomp, "marine_decomposer"),
    ] {
        let mut g = root_genome.clone();
        g.insert(defining);
        let g_id = gid(guild_name);
        ledger.push(LineageRecord {
            id: genesis_core::data::LineageId::NONE,
            parent: Some(root),
            origin_year: emergence_year, // the early microbial biosphere
            extinction_year: None,
            trait_set: g,
            trait_delta: Some(defining),
            guild: g_id,
            region: None,
            name_seed: mix(mix(seed, 0x6A5A1), g_id.0 as u64),
        });
    }

    let mut rng = SmallRng::seed_from_u64(seed ^ 0xB1_0109);
    let kingdoms = [
        "core:oxygenic_photosynthesis",
        "core:heterotrophy",
        "core:absorptive_decomposition",
    ];
    let scaffold = ["core:eukaryote", "core:colonial", "core:multicellular"];
    for kt in kingdoms {
        let ktid = id_of(kt);
        let mut kgenome = root_genome.clone();
        kgenome.insert(ktid);
        for s in scaffold {
            kgenome.insert(id_of(s));
        }
        let kid = ledger.push(LineageRecord {
            id: genesis_core::data::LineageId::NONE,
            parent: Some(root),
            origin_year: base_year,
            extinction_year: None,
            trait_set: kgenome.clone(),
            trait_delta: Some(ktid),
            guild: GuildId::NONE,
            region: None,
            name_seed: mix(seed, ktid.0 as u64),
        });
        // Each kingdom radiates *independently in each biogeographic region*, so
        // regions get distinct endemic clades (Doc 09 §6.4).
        for region in 0..BIOGEOGRAPHIC_REGIONS {
            let env = FactContext::new();
            let Some(step) = biased_walk_step(
                graph,
                &kgenome,
                &env,
                &WalkParams::default(),
                &NeutralPayoff,
                &mut rng,
            ) else {
                continue;
            };
            let mut rgenome = kgenome.clone();
            rgenome.insert(step);
            let founder = ledger.push(LineageRecord {
                id: genesis_core::data::LineageId::NONE,
                parent: Some(kid),
                origin_year: WorldYear(base_year.value() + GENERATION_SPAN_YEARS),
                extinction_year: None,
                trait_set: rgenome.clone(),
                trait_delta: Some(step),
                guild: GuildId::NONE,
                region: Some(region),
                name_seed: mix(mix(seed, kid.0), region as u64),
            });
            let mut remaining = REGION_BUDGET;
            radiate(
                &mut ledger,
                graph,
                roster,
                founder,
                &rgenome,
                region,
                2,
                &mut rng,
                seed,
                base_year,
                &mut remaining,
            );
        }
    }

    mark_extinctions(&mut ledger);
    ledger
}

/// Whether a world **stalls microbial** — life emerges but never crosses the
/// oxygenation gateway, so no eukaryote/multicellular radiation ever occurs
/// (Doc 09 §3.3, "never oxygenates"). Tied to `complexity_pressure`: at the
/// default `1.0` **no** world stalls (every existing run is unchanged); lowering
/// it makes a deterministic, increasing seed-fraction of worlds remain bacterial
/// mats. A pure function of `(seed, complexity_pressure)` so the sim and any
/// consumer agree.
pub fn is_microbial_only(seed: u64, complexity_pressure: f32) -> bool {
    let shortfall = (1.0 - complexity_pressure).clamp(0.0, 1.0);
    if shortfall <= 0.0 {
        return false;
    }
    let roll = (mix(seed, 0x57A11) % 1000) as f32 / 1000.0;
    roll < shortfall * MAX_STALL_FRACTION
}

/// Builds the sparse tree of a **stalled, microbial-only world** (Doc 09 §3.3):
/// LUCA plus a handful of divergent microbial metabolism lineages (anoxygenic
/// phototrophy, heterotrophy, absorptive decomposition) — no oxygenic
/// photosynthesis, so it never oxygenates, and no eukaryote/multicellular
/// radiation. The lineages carry no macroscopic guild, so the Bestiary is empty
/// by design (a bacterial-mat planet); the Tree of Life shows the microbial
/// divergence rather than only LUCA.
pub fn build_microbial_only(graph: &TraitGraph, seed: u64, base_year: WorldYear) -> Ledger {
    let mut ledger = Ledger::default();
    let id_of = |name: &str| graph.id_of(name).expect("core trait");
    let root_genome: TraitSet = [id_of("core:chemosynthesis"), id_of("core:unicellular")]
        .into_iter()
        .collect();
    let root = ledger.push(LineageRecord {
        id: genesis_core::data::LineageId::NONE,
        parent: None,
        origin_year: WorldYear(0),
        extinction_year: None,
        trait_set: root_genome.clone(),
        trait_delta: None,
        guild: GuildId::NONE,
        region: None,
        name_seed: mix(seed, 0),
    });
    let microbes = [
        "core:anoxygenic_phototrophy",
        "core:heterotrophy",
        "core:absorptive_decomposition",
    ];
    for (k, mt) in microbes.iter().enumerate() {
        let mtid = id_of(mt);
        let mut g = root_genome.clone();
        g.insert(mtid);
        ledger.push(LineageRecord {
            id: genesis_core::data::LineageId::NONE,
            parent: Some(root),
            origin_year: WorldYear(base_year.value() + k as i64 * GENERATION_SPAN_YEARS),
            extinction_year: None,
            trait_set: g,
            trait_delta: Some(mtid),
            guild: GuildId::NONE,
            region: None,
            name_seed: mix(seed, mtid.0 as u64),
        });
    }
    ledger
}

/// The ecological realm a lineage belongs to, inferred from its traits: aquatic
/// locomotion/feeding (`swim`, `filter_feeder`) ⇒ marine, else terrestrial.
fn infer_realm(graph: &TraitGraph, genome: &TraitSet) -> Realm {
    let marine = ["core:swim", "core:filter_feeder"]
        .iter()
        .filter_map(|n| graph.id_of(n))
        .any(|id| genome.contains(id));
    if marine {
        Realm::Marine
    } else {
        Realm::Terrestrial
    }
}

/// Which guild a specialized trait set fills (`NONE` if none). Picks the
/// **most-specific** guild *of the lineage's realm* — so a marine predator fills
/// `nekton_predator`, not the loose terrestrial `producer` that every genome
/// still matches via the `chemosynthesis` inherited from LUCA. Without the
/// realm + specificity filter, every leaf collapses onto `producer` (roster
/// index 0) and all marine / higher-trophic guilds stay empty.
fn leaf_guild(graph: &TraitGraph, roster: &GuildRoster, genome: &TraitSet) -> GuildId {
    let realm = infer_realm(graph, genome);
    roster
        .iter()
        .filter(|g| g.realm == realm && fills_guild(g, genome))
        .max_by_key(|g| (rule_specificity(&g.membership), std::cmp::Reverse(g.id.0)))
        .map(|g| g.id)
        .unwrap_or(GuildId::NONE)
}

#[allow(clippy::too_many_arguments)]
fn radiate(
    ledger: &mut Ledger,
    graph: &TraitGraph,
    roster: &GuildRoster,
    parent: genesis_core::data::LineageId,
    genome: &TraitSet,
    region: u16,
    depth: usize,
    rng: &mut SmallRng,
    seed: u64,
    base_year: WorldYear,
    remaining: &mut usize,
) {
    if depth > MAX_DEPTH || *remaining == 0 {
        return;
    }
    let branches = 3usize.saturating_sub(depth / 3); // 3 shallow, fewer deep
    let env = FactContext::new();
    for _ in 0..branches {
        if *remaining == 0 {
            break;
        }
        let Some(step) = biased_walk_step(
            graph,
            genome,
            &env,
            &WalkParams::default(),
            &NeutralPayoff,
            rng,
        ) else {
            break;
        };
        let mut child = genome.clone();
        child.insert(step);
        let guild = if depth >= LEAF_DEPTH {
            leaf_guild(graph, roster, &child)
        } else {
            GuildId::NONE
        };
        let origin = WorldYear(base_year.value() + depth as i64 * GENERATION_SPAN_YEARS);
        let cid = ledger.push(LineageRecord {
            id: genesis_core::data::LineageId::NONE,
            parent: Some(parent),
            origin_year: origin,
            extinction_year: None,
            trait_set: child.clone(),
            trait_delta: Some(step),
            guild,
            region: Some(region),
            name_seed: mix(mix(seed, parent.0), step.0 as u64),
        });
        *remaining -= 1;
        radiate(
            ledger,
            graph,
            roster,
            cid,
            &child,
            region,
            depth + 1,
            rng,
            seed,
            base_year,
            remaining,
        );
    }
}

/// Marks a deterministic fraction of leaf (guild-bearing) lineages extinct
/// (Doc 09 §7, simplified flat hazard) — the greyed lines in the tree.
fn mark_extinctions(ledger: &mut Ledger) {
    for record in &mut ledger.lineages {
        if record.guild != GuildId::NONE
            && record.parent.is_some()
            && record.region.is_some() // cosmopolitan basal seeds are immortal
            && mix(record.name_seed, 0xEED) % 100 < EXTINCT_PERCENT
        {
            let lifespan = 30_000_000 + (record.name_seed % 120) as i64 * 1_000_000;
            record.extinction_year = Some(WorldYear(record.origin_year.value() + lifespan));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core_graph::core_morphospace;
    use crate::guild::core_guilds;

    #[test]
    fn radiation_builds_a_rooted_tree_with_descent() {
        let graph = core_morphospace();
        let roster = core_guilds(&graph);
        let ledger = build_radiation(&graph, &roster, 42, WorldYear(300_000_000));
        assert!(ledger.len() > 20, "should radiate many lineages");
        // Exactly one root (LUCA), everything else has a parent.
        assert_eq!(ledger.iter().filter(|l| l.parent.is_none()).count(), 1);
        // Descent with modification: a child's genome is a superset of its parent's.
        for l in ledger.iter() {
            if let Some(p) = l.parent {
                let parent = ledger.get(p).unwrap();
                assert!(
                    parent.trait_set.iter().all(|t| l.trait_set.contains(t)),
                    "child must inherit all parent traits"
                );
                assert!(l.trait_set.len() >= parent.trait_set.len());
            }
        }
        // Some leaves specialized into guilds; some went extinct.
        assert!(ledger.iter().any(|l| l.guild != GuildId::NONE));
        assert!(ledger.iter().any(|l| l.extinction_year.is_some()));
    }

    #[test]
    fn basal_life_covers_both_realms_and_guilds_diversify() {
        let graph = core_morphospace();
        let roster = core_guilds(&graph);
        let ledger = build_radiation(&graph, &roster, 42, WorldYear(300_000_000));
        let gid = |name: &str| roster.iter().find(|g| g.name == name).unwrap().id;
        // An immortal, cosmopolitan (region-None) basal lineage exists for a
        // producer + decomposer in BOTH realms.
        for name in [
            "producer",
            "decomposer",
            "phytoplankton",
            "marine_decomposer",
        ] {
            let g = gid(name);
            assert!(
                ledger
                    .iter()
                    .any(|l| l.guild == g && l.region.is_none() && l.extinction_year.is_none()),
                "missing immortal cosmopolitan basal lineage for {name}"
            );
        }
        // The radiation no longer collapses onto a single guild — at least the 4
        // basal guilds are populated (food web has structure, not all 'producer').
        let guilds: std::collections::BTreeSet<u16> = ledger
            .iter()
            .filter(|l| l.guild != GuildId::NONE)
            .map(|l| l.guild.0)
            .collect();
        assert!(guilds.len() >= 4, "expected diverse guilds, got {guilds:?}");
    }

    #[test]
    fn radiation_is_deterministic() {
        let graph = core_morphospace();
        let roster = core_guilds(&graph);
        let a = build_radiation(&graph, &roster, 7, WorldYear(300_000_000));
        let b = build_radiation(&graph, &roster, 7, WorldYear(300_000_000));
        assert_eq!(a, b);
        let c = build_radiation(&graph, &roster, 8, WorldYear(300_000_000));
        assert_ne!(a.len(), 0);
        assert_ne!(a, c);
    }

    #[test]
    fn default_complexity_never_stalls_but_low_pressure_does() {
        // At the default complexity_pressure (1.0) no world is microbial-only, so
        // every existing run is unchanged.
        for seed in 0..500u64 {
            assert!(
                !is_microbial_only(seed, 1.0),
                "seed {seed} stalled at cp 1.0"
            );
        }
        // Dropping the pressure makes a real, deterministic fraction stall.
        let stalled = (0..1000u64).filter(|&s| is_microbial_only(s, 0.0)).count();
        assert!(
            (300..=900).contains(&stalled),
            "≈60% of worlds should stall at cp 0.0, got {stalled}/1000"
        );
        // Deterministic.
        assert_eq!(is_microbial_only(42, 0.0), is_microbial_only(42, 0.0));
    }

    #[test]
    fn microbial_only_tree_is_sparse_with_no_guilds() {
        let graph = core_morphospace();
        let ledger = build_microbial_only(&graph, 42, WorldYear(300_000_000));
        // LUCA + a few microbial metabolism lineages, and nothing more.
        assert!(
            (2..=6).contains(&ledger.len()),
            "a stalled world is sparse, got {}",
            ledger.len()
        );
        assert_eq!(ledger.iter().filter(|l| l.parent.is_none()).count(), 1);
        // No macroscopic guild anywhere (empty Bestiary by design), and no
        // multicellular grade in any genome (never oxygenated).
        let multicellular = graph.id_of("core:multicellular").unwrap();
        for l in ledger.iter() {
            assert_eq!(l.guild, GuildId::NONE);
            assert!(!l.trait_set.contains(multicellular));
        }
    }
}
