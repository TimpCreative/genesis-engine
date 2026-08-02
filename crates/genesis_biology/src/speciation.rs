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

use genesis_core::data::{GuildId, WorldData};
use genesis_core::time::WorldYear;
use rand::SeedableRng;
use rand::rngs::SmallRng;

use crate::evolution::{EnvProfile, EnvironmentPayoff, WalkParams, WalkStep, biased_evolution_step};
use crate::guild::{GuildRoster, fills_guild, rule_specificity};
use crate::ledger::{Ledger, LineageRecord};
use crate::morphospace::{TraitGraph, TraitSet};
use crate::province::{ProvinceRegistry, Realm};
use genesis_rules::FactContext;

const MAX_DEPTH: usize = 6;
/// Depth from which a lineage is specialized enough to fill a guild (a "species").
const LEAF_DEPTH: usize = 4;
#[allow(dead_code)]
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

/// One environment profile per biogeographic region (Doc 09B), aligned with
/// [`crate::view::geo_region`]. `neutral()` reproduces the seed-only radiation.
pub struct RegionProfiles(pub [EnvProfile; BIOGEOGRAPHIC_REGIONS as usize]);

impl RegionProfiles {
    pub fn neutral() -> Self {
        Self([EnvProfile::neutral(); BIOGEOGRAPHIC_REGIONS as usize])
    }
}

/// Builds the recorded tree of life with a neutral (environment-agnostic)
/// radiation — a pure function of the seed. The sim path uses
/// [`build_radiation_with`] to make regions adapt to their climate.
pub fn build_radiation(
    graph: &TraitGraph,
    roster: &GuildRoster,
    seed: u64,
    base_year: WorldYear,
) -> Ledger {
    build_radiation_with(graph, roster, seed, base_year, &RegionProfiles::neutral())
}

/// Builds the recorded tree of life for a world (Doc 09 §6). `base_year` is the
/// multicellular radiation year; branches arise progressively after it. Each
/// region's clades are biased toward its `EnvProfile` (Doc 09B), so cold regions
/// evolve insulation, arid regions water-conservation, oceans swimmers, etc.
pub fn build_radiation_with(
    graph: &TraitGraph,
    roster: &GuildRoster,
    seed: u64,
    base_year: WorldYear,
    profiles: &RegionProfiles,
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
            let payoff = EnvironmentPayoff::new(graph, profiles.0[region as usize]);
            let Some(step) = biased_evolution_step(
                graph,
                &kgenome,
                &env,
                &WalkParams::default(),
                &payoff,
                0.0, // kingdom founders only gain (establishing basin traits)
                &mut rng,
            ) else {
                continue;
            };
            // Kingdom founders always gain — loss makes no sense at founding.
            let tid = match step {
                WalkStep::Gain(tid) => tid,
                WalkStep::Loss(_) => continue,
            };
            let mut rgenome = kgenome.clone();
            rgenome.insert(tid);
            let founder = ledger.push(LineageRecord {
                id: genesis_core::data::LineageId::NONE,
                parent: Some(kid),
                origin_year: WorldYear(base_year.value() + GENERATION_SPAN_YEARS),
                extinction_year: None,
                trait_set: rgenome.clone(),
                trait_delta: Some(tid),
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
                &payoff,
            );
        }
    }

    // Extinction is now applied per-tick (selective, environment-driven) rather
    // than as a flat hazard at build time. Lineages start extant; the layer's
    // heavy-stride block calls `selective_extinction`.
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
    payoff: &dyn crate::evolution::SelectivePayoff,
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
        // Small loss bias — most steps are still gains (descent with modification),
        // but a lineage can occasionally shed a superseded trait (Doc 09 §2.3).
        const RADIATION_LOSS_BIAS: f32 = 0.04;
        let Some(step) = biased_evolution_step(
            graph, genome, &env, &WalkParams::default(), payoff,
            RADIATION_LOSS_BIAS, rng,
        )
        else {
            break;
        };
        let mut child = genome.clone();
        let delta = match step {
            WalkStep::Gain(tid) => {
                child.insert(tid);
                tid
            }
            WalkStep::Loss(tid) => {
                child.remove(tid);
                tid
            }
        };
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
            trait_delta: Some(delta),
            guild,
            region: Some(region),
            name_seed: mix(mix(seed, parent.0), delta.0 as u64),
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
            payoff,
        );
    }
}

/// Flat-hazard fallback for microbial-only worlds (no provinces or climate data).
#[allow(dead_code)]
fn mark_extinctions(ledger: &mut Ledger) {
    for record in &mut ledger.lineages {
        if record.guild != GuildId::NONE
            && record.parent.is_some()
            && record.region.is_some()
            && mix(record.name_seed, 0xEED) % 100 < EXTINCT_PERCENT
        {
            let lifespan = 30_000_000 + (record.name_seed % 120) as i64 * 1_000_000;
            record.extinction_year = Some(WorldYear(record.origin_year.value() + lifespan));
        }
    }
}

/// Environment-driven selective extinction (Phase 1). Called each heavy stride
/// after provinces are recomputed. Lineage hazard is a deterministic function of
/// background rate, climate stability, and competition — the world kills species
/// that lose their niche.
///
/// Returns the number of lineages newly marked extinct this stride.
pub fn selective_extinction(
    ledger: &mut Ledger,
    roster: &GuildRoster,
    provinces: &ProvinceRegistry,
    prior_temps: &[f32],
    prior_precips: &[f32],
    world: &WorldData,
    year: WorldYear,
    seed: u64,
) -> usize {
    const BASE_HAZARD: u64 = 10; // 10% per heavy stride (~35 My mean lifespan in stable times)
    const CLIMATE_SHOCK_THRESHOLD_C: f32 = 3.0;
    const CLIMATE_SHOCK_THRESHOLD_P: f32 = 0.30;
    const CLIMATE_SHOCK_MAX_HAZARD: u64 = 20; // +20% additive for severe climate shock
    const COMPETITION_HAZARD_MULTIPLIER: u64 = 2; // 2× for the weaker competitor

    // Compute current per-province mean temperature and precipitation.
    let n_provinces = provinces.len().max(1);
    let mut cur_temps = vec![0f32; n_provinces];
    let mut cur_precips = vec![0f32; n_provinces];
    let mut cur_counts = vec![0usize; n_provinces];
    for i in 0..world.cell_count() as usize {
        let pid = world.province_id.get(i).map(|p| p.0 as usize).unwrap_or(0);
        if pid < n_provinces {
            cur_temps[pid] += world.temperature_mean[i];
            cur_precips[pid] += world.precipitation[i].max(0.0);
            cur_counts[pid] += 1;
        }
    }
    for p in 0..n_provinces {
        if cur_counts[p] > 0 {
            let n = cur_counts[p] as f32;
            cur_temps[p] /= n;
            cur_precips[p] /= n;
        }
    }

    // Build a per-(region, guild) index of extant lineages for competition.
    // Also pre-compute guild specificity scores to avoid borrowing ledger
    // immutably while we iterate it mutably below.
    let mut guild_region: std::collections::BTreeMap<(u16, u16), Vec<(usize, u64, u32)>> =
        std::collections::BTreeMap::new();
    for (idx, rec) in ledger.lineages.iter().enumerate() {
        if rec.extinction_year.is_none()
            && rec.guild != GuildId::NONE
            && rec.region.is_some()
        {
            let spec = roster
                .iter()
                .find(|g| g.id == rec.guild)
                .map(|g| rule_specificity(&g.membership))
                .unwrap_or(1);
            let key = (rec.region.unwrap(), rec.guild.0);
            guild_region
                .entry(key)
                .or_default()
                .push((idx, rec.name_seed, spec));
        }
    }

    let mut newly_extinct = 0usize;

    for (idx, record) in ledger.lineages.iter_mut().enumerate() {
        if record.extinction_year.is_some()
            || record.guild == GuildId::NONE
            || record.region.is_none()
        {
            continue; // already extinct, not guild-bearing, or cosmopolitan (immortal)
        }
        let region = record.region.unwrap();

        // --- base hazard ---
        let mut hazard = BASE_HAZARD;

        // --- climate shock ---
        let pidx = region as usize;
        if pidx < prior_temps.len() && pidx < cur_temps.len() && cur_counts.get(pidx).copied().unwrap_or(0) > 0 {
            let dt = (cur_temps[pidx] - prior_temps[pidx]).abs();
            let dp = if prior_precips[pidx] > 0.0 {
                ((cur_precips[pidx] - prior_precips[pidx]) / prior_precips[pidx]).abs()
            } else {
                0.0
            };
            let temp_shock = (dt / CLIMATE_SHOCK_THRESHOLD_C).min(1.0);
            let precip_shock = (dp / CLIMATE_SHOCK_THRESHOLD_P).min(1.0);
            let shock = temp_shock.max(precip_shock);
            hazard += (CLIMATE_SHOCK_MAX_HAZARD as f32 * shock) as u64;
        }

        // --- competition ---
        if let Some(competitors) = guild_region.get(&(region, record.guild.0)) {
            if competitors.len() > 1 {
                let my_spec = competitors
                    .iter()
                    .find(|(ci, _, _)| *ci == idx)
                    .map(|(_, _, s)| *s)
                    .unwrap_or(1);
                let better = competitors
                    .iter()
                    .any(|(_, _, s)| *s > my_spec);
                if better {
                    hazard *= COMPETITION_HAZARD_MULTIPLIER;
                }
            }
        }

        // --- deterministic roll ---
        hazard = hazard.min(95); // cap at 95% — always a chance to survive
        let roll = mix(record.name_seed ^ seed, year.value() as u64) % 100;
        if roll < hazard {
            record.extinction_year = Some(year);
            newly_extinct += 1;
        }
    }

    newly_extinct
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
        // Descent with modification: a walk-generated child differs from its
        // parent by exactly one trait — either one gain (inherits all parent traits
        // + 1 new) or one loss (inherits all but one parent trait, Doc 09 §2.3).
        // Kingdom founders are scaffolded — they add 4 fixed traits at once
        // (ktid + eukaryote + colonial + multicellular) as direct children of LUCA.
        for l in ledger.iter() {
            if let Some(p) = l.parent {
                let parent = ledger.get(p).unwrap();
                let inherited = parent.trait_set.iter()
                    .filter(|t| l.trait_set.contains(*t))
                    .count();
                let lost = parent.trait_set.len() - inherited;
                let gained = l.trait_set.len() - inherited;
                // Scaffolded kingdom founders: child of LUCA (2 traits) with 6 traits
                // (ktid + 3 scaffold + LUCA).
                let is_scaffolded =
                    l.trait_delta.is_some() && parent.trait_set.len() == 2 && l.trait_set.len() >= 6;
                if !is_scaffolded {
                    assert!(
                        lost + gained == 1 && lost <= 1 && gained <= 1,
                        "walk child must differ by exactly 1 trait: lost={lost} gained={gained} parent_len={} child_len={}",
                        parent.trait_set.len(),
                        l.trait_set.len(),
                    );
                } else {
                    // Scaffolded founders must still inherit all LUCA traits.
                    assert_eq!(lost, 0, "scaffolded founder must inherit all parent traits");
                }
            }
        }
        // Some leaves specialized into guilds. Extinction is now applied
        // per-heavy-stride (selective_extinction), not at build time.
        assert!(ledger.iter().any(|l| l.guild != GuildId::NONE));
        // All lineages start extant — extinction happens later, driven by the world.
        assert!(ledger.iter().all(|l| l.extinction_year.is_none()));
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
    fn basins_stay_coherent_no_photosynthesizing_animals() {
        // Trait coherence (Doc 09 §2.3): the metabolism basins don't mix, so there
        // are no photosynthesizing predators. Nerves/bones require heterotrophy;
        // cellulose walls require photosynthesis; the metabolisms are exclusive.
        let graph = core_morphospace();
        let roster = core_guilds(&graph);
        let ledger = build_radiation(&graph, &roster, 42, WorldYear(400_000_000));
        let id = |n: &str| graph.id_of(n).unwrap();
        let (photo, hetero) = (id("core:oxygenic_photosynthesis"), id("core:heterotrophy"));
        let (nerve, endo, cellulose) = (
            id("core:nerve_net"),
            id("core:mineral_endoskeleton"),
            id("core:cellulose_wall"),
        );
        for l in ledger.iter() {
            let g = &l.trait_set;
            assert!(
                !(g.contains(photo) && g.contains(hetero)),
                "metabolisms not exclusive"
            );
            assert!(
                !(g.contains(photo) && g.contains(nerve)),
                "photosynthesizing animal (nerves)"
            );
            assert!(
                !(g.contains(photo) && g.contains(endo)),
                "photosynthesizing animal (bones)"
            );
            assert!(
                !(g.contains(hetero) && g.contains(cellulose)),
                "animal with plant cell walls"
            );
        }
    }

    #[test]
    fn environment_shapes_the_radiation_deterministically() {
        let graph = core_morphospace();
        let roster = core_guilds(&graph);
        let y = WorldYear(400_000_000);
        // Neutral profiles reproduce the seed-only radiation exactly (determinism
        // + endemism tests stay valid).
        assert_eq!(
            build_radiation(&graph, &roster, 42, y),
            build_radiation_with(&graph, &roster, 42, y, &RegionProfiles::neutral())
        );
        // A cold and a warm world evolve different trait sets from the same seed.
        let profile = |t: f32, marine: f32| EnvProfile {
            temperature_c: t,
            aridity: 0.4,
            marine_fraction: marine,
            nutrient: 0.5,
            disturbance: 0.0,
            co2_ppm: 280.0,
            neutral: false,
        };
        let cold = RegionProfiles([profile(-25.0, 0.0); BIOGEOGRAPHIC_REGIONS as usize]);
        let warm = RegionProfiles([profile(30.0, 1.0); BIOGEOGRAPHIC_REGIONS as usize]);
        let cold_l = build_radiation_with(&graph, &roster, 42, y, &cold);
        let warm_l = build_radiation_with(&graph, &roster, 42, y, &warm);
        assert_ne!(
            cold_l, warm_l,
            "environment should change which traits evolve"
        );
        // Same environment → identical ledger (deterministic).
        assert_eq!(cold_l, build_radiation_with(&graph, &roster, 42, y, &cold));
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

    // --- selective extinction tests ---

    /// Builds a minimal world + radiation for testing extinction.
    fn extinction_fixture() -> (WorldData, GuildRoster, ProvinceRegistry, Ledger) {
        use genesis_core::create_world;
        let mut params = genesis_core::parameters::WorldParameters::default();
        params.core.grid.subdivision_level = 5; // small grid for speed
        let mut world = create_world(params).expect("world").data;
        // Give it deep ocean and varied terrain so provinces form.
        for i in 0..world.cell_count() as usize {
            if i % 3 == 0 {
                world.elevation_mean[i] = -3000.0;
                world.water_level_m[i] = 0.0;
            } else {
                world.elevation_mean[i] = 500.0;
                world.water_level_m[i] = 0.0;
            }
            world.temperature_mean[i] = 20.0;
            world.precipitation[i] = 500.0;
        }
        let provinces = crate::province::label_provinces(&mut world);
        let graph = core_morphospace();
        let roster = core_guilds(&graph);
        let ledger = build_radiation(&graph, &roster, 42, WorldYear(400_000_000));
        (world, roster, provinces, ledger)
    }

    #[test]
    fn background_extinction_is_deterministic() {
        let (world, roster, provinces, mut ledger) = extinction_fixture();
        let n = provinces.len().max(1);
        let temps = vec![20.0; n];
        let precips = vec![500.0; n];

        let a = selective_extinction(
            &mut ledger, &roster, &provinces, &temps, &precips,
            &world, WorldYear(410_000_000), 42,
        );
        // Same inputs → same result.
        let graph = core_morphospace();
        let roster2 = core_guilds(&graph);
        let mut ledger2 = build_radiation(&graph, &roster2, 42, WorldYear(400_000_000));
        let b = selective_extinction(
            &mut ledger2, &roster2, &provinces, &temps, &precips,
            &world, WorldYear(410_000_000), 42,
        );
        assert_eq!(a, b, "selective extinction must be deterministic");
    }

    #[test]
    fn climate_shock_elevates_extinction() {
        let (world, roster, provinces, mut ledger) = extinction_fixture();
        let n = provinces.len().max(1);

        // Stable: prior ≈ current → low extinction.
        let stable_temps = vec![20.0; n];
        let stable_precips = vec![500.0; n];
        let graph = core_morphospace();
        let r2 = core_guilds(&graph);
        let mut stable_ledger = build_radiation(&graph, &r2, 42, WorldYear(400_000_000));
        let stable_dead = selective_extinction(
            &mut stable_ledger, &r2, &provinces, &stable_temps, &stable_precips,
            &world, WorldYear(410_000_000), 100,
        );

        // Shocked: prior very different from current → higher extinction.
        let shock_temps = vec![30.0; n]; // 10°C warmer than current
        let shock_precips = vec![100.0; n]; // much drier than current
        let shock_dead = selective_extinction(
            &mut ledger, &roster, &provinces, &shock_temps, &shock_precips,
            &world, WorldYear(410_000_000), 100,
        );

        assert!(
            shock_dead >= stable_dead,
            "climate shock should not reduce extinction: shock={shock_dead} stable={stable_dead}"
        );
    }

    #[test]
    fn cosmopolitan_lineages_are_immortal() {
        let (world, roster, provinces, mut ledger) = extinction_fixture();
        let n = provinces.len().max(1);
        let temps = vec![20.0; n];
        let precips = vec![500.0; n];

        let cosm_count_before = ledger.iter().filter(|l| l.region.is_none()).count();
        assert!(cosm_count_before > 0, "need cosmopolitan lineages");

        selective_extinction(
            &mut ledger, &roster, &provinces, &temps, &precips,
            &world, WorldYear(410_000_000), 42,
        );

        for l in ledger.iter() {
            if l.region.is_none() {
                assert!(l.extinction_year.is_none(), "cosmopolitan lineages are immortal");
            }
        }
    }
}
