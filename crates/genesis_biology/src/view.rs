//! The real `BiologyView` adapter (Doc 09 §8, the P4-11 stub swap).
//!
//! Spatial answers (biome / richness / biomass / occupied guilds) come from the
//! simulated `WorldData` arrays (carried in `HistoryFrame`s, so scrubbing shows
//! real data). The **tree of life and species** come from the recorded lineage
//! [`Ledger`](crate::ledger) — a real family tree from LUCA with
//! descent-with-modification and extinct lines. The ledger is a pure function of
//! the seed, so the adapter **builds it at construction** (available immediately,
//! not only after generation finishes); the exact simulated ledger refines the
//! origin years if it arrives. Everything is deterministic (Doc 09 §8.3).

use std::sync::OnceLock;

use genesis_core::biology_view::{
    Assemblage, BiologyView, FoodWeb, GuildSummary, LifeEventCategory, LifeEventPip, SpeciesDetail,
    SpeciesPeek, TreeNodePeek, TreePeek,
};
use genesis_core::data::{BiomeId, GuildId, TraitId, WATER_NONE, WorldData};
use genesis_core::grid::HexId;
use genesis_core::time::WorldYear;

use crate::biome::{BARREN_ID, biome_name};
use crate::core_graph::core_morphospace;
use crate::guild::{GuildRoster, core_guilds};
use crate::ledger::{Ledger, LineageRecord, rank_for_tier};
use crate::morphospace::{TraitAxis, TraitGraph, TraitSet};
use crate::naming::{Namer, NamingScheme};
use crate::population::cascade_order;
use crate::province::Realm;
use crate::richness::{occupied_guild_count, species_in_guild};
use crate::speciation::BIOGEOGRAPHIC_REGIONS;

const BIOMASS_FULL: f32 = 1100.0; // matches population::TROPHIC_TOTAL_SCALE
const DISPLAY_SPECIES_PER_GUILD: u32 = 6;
/// Max prey/predator/competitor species shown in a food-web panel.
const FOOD_WEB_LIMIT: usize = 8;
/// Max related species (siblings/cousins) shown in the detail panel.
const RELATIVES_LIMIT: usize = 8;
const SPECIES_S_MAX: u32 = 400;
const SPECIES_K: f32 = 0.35;
/// Fallback multicellular-radiation year used when the adapter builds its own
/// ledger at load (the real simulated year, if it ever arrives, refines this).
const DEFAULT_RADIATION_YEAR: i64 = 400_000_000;

/// The Doc 09 biology read-view: real fields + a recorded (or generated) tree.
pub struct GeneratedBiologyView {
    seed: u64,
    graph: TraitGraph,
    roster: GuildRoster,
    namer: Namer,
    ledger: Option<Ledger>,
    /// Lazily-built spatial range model (Doc 09 §6.4 redesign). A pure function
    /// of `(seed, ledger, first-queried WorldData)`, primed once on first spatial
    /// query — so it stays deterministic while living behind `&self`.
    range: OnceLock<RangeIndex>,
}

impl GeneratedBiologyView {
    /// Builds the adapter, immediately deriving the tree of life from the seed so
    /// it is available the moment the world loads — the ledger is a pure function
    /// of the seed (Doc 09 §8.3), so it does not need to wait for generation to
    /// finish. `with_ledger` later refines it with the exact simulated one.
    pub fn new(seed: u64) -> Self {
        let graph = core_morphospace();
        let roster = core_guilds(&graph);
        let ledger = crate::speciation::build_radiation(
            &graph,
            &roster,
            seed,
            WorldYear(DEFAULT_RADIATION_YEAR),
        );
        Self {
            seed,
            graph,
            roster,
            namer: Namer::load(NamingScheme::Latin),
            ledger: Some(ledger),
            range: OnceLock::new(),
        }
    }

    /// Builds the adapter over the exact simulated tree of life (accurate origin
    /// years).
    pub fn with_ledger(seed: u64, ledger: Ledger) -> Self {
        let mut v = Self::new(seed);
        v.ledger = Some(ledger);
        v
    }

    fn realm_at(&self, data: &WorldData, i: usize) -> Realm {
        realm_at_hex(data, i)
    }

    /// A lineage's presence field at hex `i` ∈ [0,1] — the heart of the range
    /// model (Doc 09 §6.4 redesign). Cosmopolitan lineages (`region == None`,
    /// e.g. the basal seeds) blanket every habitable hex, graded only by
    /// habitability. An endemic lineage's presence is `proximity × climate ×
    /// habitability`, where proximity is a great-circle falloff from its frozen
    /// birthplace (so ranges wrap the globe and feather at the edges) and climate
    /// is a Gaussian match of the hex's temperature/moisture to the birthplace's
    /// (so ranges bend along isotherms/isohyets, live, as the world changes).
    fn presence(&self, idx: &RangeIndex, data: &WorldData, i: usize, l: &LineageRecord) -> f32 {
        let hab = data.biotic_richness.get(i).copied().unwrap_or(0.0);
        if hab <= 0.0 || data.biome.get(i).map(|b| b.0) == Some(BARREN_ID) {
            return 0.0;
        }
        if l.region.is_none() {
            return hab; // cosmopolitan blanket
        }
        let lid = l.id.0 as usize;
        let o = idx.origin_dir[lid];
        let d = data.grid.cell_center_direction(HexId(i as u32));
        // Cosine of the great-circle angle between hex and birthplace (wraps the
        // sphere and the poles with no seam); 1 - cos ≈ θ²/2, so `prox` is
        // Gaussian in angular distance.
        let c = o[0] * d[0] as f32 + o[1] * d[1] as f32 + o[2] * d[2] as f32;
        let prox = (-RANGE_K_PROX * (1.0 - c)).exp();
        let dt = (data.temperature_mean[i] - idx.t0[lid]) / RANGE_SIGMA_T;
        let dm = (moisture(data.precipitation[i]) - idx.m0[lid]) / RANGE_SIGMA_M;
        let clim = (-(dt * dt + dm * dm)).exp();
        prox * clim * hab
    }

    /// The single shared predicate: every lineage whose range covers hex `i`,
    /// grouped by trophic guild in cascade order and passed through the exact same
    /// occupancy contingency the old `occupied_guild_names` used. All three
    /// spatial consumers (occupied guilds, the Bestiary assemblage, and the map
    /// paint) route through this, so the map provably matches the species list.
    fn present_lineages_at(&self, data: &WorldData, i: usize) -> Vec<PresentLineage<'_>> {
        let Some(ledger) = self.ledger.as_ref() else {
            return Vec::new();
        };
        let r = data.biotic_richness.get(i).copied().unwrap_or(0.0);
        if r <= 0.0 || data.biome.get(i).map(|b| b.0) == Some(BARREN_ID) {
            return Vec::new();
        }
        let roster = &self.roster;
        let idx = self
            .range
            .get_or_init(|| RangeIndex::build(ledger, data, roster));
        let year = data.current_year;
        let realm = realm_at_hex(data, i);
        let order = cascade_order(realm);
        let mut out: Vec<PresentLineage<'_>> = Vec::new();
        let mut occupied: Vec<&'static str> = Vec::new();
        for (k, &gname) in order.iter().enumerate() {
            let Some(gid) = roster.iter().find(|g| g.name == gname).map(|g| g.id) else {
                continue;
            };
            let Some(bucket) = idx.by_guild.get(&gid) else {
                continue;
            };
            let mut hits: Vec<PresentLineage<'_>> = Vec::new();
            for &lid in bucket {
                let l = &ledger.lineages[lid as usize];
                if !l.alive_at(year) {
                    continue;
                }
                let s = self.presence(idx, data, i, l);
                // Cosmopolitan seeds are present on any habitable hex (preserving
                // the old blanket); endemics must clear the soft range threshold.
                let present = if l.region.is_none() {
                    s > 0.0
                } else {
                    s >= RANGE_TAU
                };
                if present {
                    hits.push(PresentLineage {
                        guild_name: gname,
                        lineage: l,
                        strength: s,
                    });
                }
            }
            if hits.is_empty() {
                continue;
            }
            // Identical trophic contingency to the old occupancy cascade: producer
            // + decomposer are guaranteed; herbivores need producers and enough R;
            // predators need herbivores and more R (see the old view.rs:141-148).
            let allowed = match k {
                0 | 1 => true,
                2 => r > 0.15 && occupied.contains(&order[0]),
                _ => r > 0.35 && occupied.contains(&order[2]),
            };
            if !allowed {
                continue;
            }
            occupied.push(gname);
            // Place-specific endemics lead (the interesting, distinct-per-area
            // species), then strongest presence first; the cosmopolitan basal
            // seeds sort last as filler so every hex still lists *something*
            // without every list leading with the same generic organism.
            hits.sort_by(|a, b| {
                a.lineage
                    .region
                    .is_none()
                    .cmp(&b.lineage.region.is_none())
                    .then_with(|| {
                        b.strength
                            .partial_cmp(&a.strength)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .then(a.lineage.name_seed.cmp(&b.lineage.name_seed))
            });
            out.extend(hits);
        }
        out
    }

    fn occupied_guild_names(&self, data: &WorldData, i: usize) -> Vec<&'static str> {
        let r = data.biotic_richness.get(i).copied().unwrap_or(0.0);
        if r <= 0.0 || data.biome.get(i).map(|b| b.0) == Some(BARREN_ID) {
            return Vec::new();
        }
        if self.ledger.is_none() {
            // Pre-ledger fallback: a plain richness cascade.
            let order = cascade_order(self.realm_at(data, i));
            let budget = (occupied_guild_count(r) as usize).max(1);
            return order
                .iter()
                .take(budget)
                .copied()
                .filter(|name| self.roster.iter().any(|g| g.name == *name))
                .collect();
        }
        // Real spatial occupancy (Doc 09 §4.3 + §6.4 redesign): the distinct
        // guilds whose range actually covers this hex, via the shared predicate —
        // so this list matches the Bestiary assemblage and the map paint exactly.
        let mut names: Vec<&'static str> = Vec::new();
        for p in self.present_lineages_at(data, i) {
            if !names.contains(&p.guild_name) {
                names.push(p.guild_name);
            }
        }
        names
    }

    /// A lineage's display name: "Root (LUCA)", a Latin binomial for a species
    /// (keyed to its traits), or a single genus name for an internal clade.
    fn lineage_name(&self, l: &LineageRecord) -> String {
        if l.parent.is_none() {
            return "Root (LUCA)".to_string();
        }
        if l.guild != GuildId::NONE {
            let (genus, epithet) = self.namer.binomial(&self.graph, &l.trait_set, l.name_seed);
            format!("{genus} {epithet}")
        } else {
            self.namer.genus(&self.graph, &l.trait_set, l.name_seed)
        }
    }

    /// The family name — the genus stem of a family-rank ancestor plus "-idae".
    fn family_name(&self, ledger: &Ledger, l: &LineageRecord) -> String {
        let mut cur = l.parent;
        while let Some(p) = cur {
            let rec = ledger.get(p).expect("parent");
            if let Some(t) = rec.trait_delta
                && self.graph.node(t).tier == 4
            {
                return family_from_genus(&self.namer.genus(
                    &self.graph,
                    &rec.trait_set,
                    rec.name_seed,
                ));
            }
            cur = rec.parent;
        }
        family_from_genus(
            &self
                .namer
                .genus(&self.graph, &l.trait_set, l.name_seed ^ 0xFA),
        )
    }

    /// Builds a `SpeciesPeek` for a lineage (shared by the assemblage, the global
    /// catalog, and the food web).
    fn peek_from_lineage(
        &self,
        ledger: &Ledger,
        l: &LineageRecord,
        guild_name: &str,
        description: String,
    ) -> SpeciesPeek {
        SpeciesPeek {
            species_id: l.name_seed,
            name: self.lineage_name(l),
            guild: guild_name.to_string(),
            family: self.family_name(ledger, l),
            trait_chips: chips_from_traits(&self.graph, &l.trait_set),
            description,
        }
    }

    fn tree_from_ledger(&self, ledger: &Ledger, year: WorldYear) -> TreePeek {
        let y = year.value();
        let depth = depth_map(ledger);
        let nodes = ledger
            .iter()
            .filter(|l| l.origin_year.value() <= y)
            .map(|l| {
                let d = depth.get(&l.id.0).copied().unwrap_or(0);
                // Rank by nesting depth (monotonic), not trait tier: LUCA → life,
                // then kingdom → phylum → …; a guild-bearing leaf is a "species".
                let rank = if l.parent.is_none() {
                    "life".to_string()
                } else if l.guild != GuildId::NONE {
                    "species".to_string()
                } else {
                    rank_for_tier((d.saturating_sub(1)).min(u8::MAX as u32) as u8).to_string()
                };
                let defining_trait = l
                    .trait_delta
                    .map(|t| self.graph.node(t).display.clone())
                    .unwrap_or_else(|| "the origin of life".to_string());
                TreeNodePeek {
                    id: l.id.0,
                    parent: l.parent.map(|p| p.0),
                    name: self.lineage_name(l),
                    rank,
                    depth: d,
                    species_id: l.name_seed,
                    defining_trait,
                    origin_year: l.origin_year.value(),
                    // Only grey lines already extinct at the viewed year.
                    extinction_year: l.extinction_year.map(|e| e.value()).filter(|&e| e <= y),
                }
            })
            .collect();
        TreePeek { nodes }
    }

    fn assemblage_from_ledger(&self, ledger: &Ledger, data: &WorldData, hex: HexId) -> Assemblage {
        let i = hex.0 as usize;
        let r = self.richness_at(data, hex);
        let biome = self.biome_name(self.biome_at(data, hex));
        // The species whose ranges actually cover this hex (strongest first per
        // guild), via the shared predicate — so the Bestiary here is exactly what
        // the map paints. Capped per guild for a readable list.
        let present = self.present_lineages_at(data, i);
        let total = species_in_guild(r, SPECIES_S_MAX, SPECIES_K);
        let mut species = Vec::new();
        let mut guild_names: Vec<&'static str> = Vec::new();
        let mut per_guild: std::collections::BTreeMap<&'static str, usize> = Default::default();
        for p in &present {
            let shown = per_guild.entry(p.guild_name).or_insert(0);
            if *shown >= DISPLAY_SPECIES_PER_GUILD as usize {
                continue;
            }
            *shown += 1;
            if !guild_names.contains(&p.guild_name) {
                guild_names.push(p.guild_name);
            }
            let gname = p.guild_name;
            let family = self.family_name(ledger, p.lineage);
            let description =
                format!("A {biome} {gname} (family {family}); ~{total} species share its guild here.");
            species.push(self.peek_from_lineage(ledger, p.lineage, gname, description));
        }
        Assemblage {
            biome_name: biome,
            richness: r,
            occupied_guilds: guild_names.len() as u32,
            guild_capacity: cascade_order(self.realm_at(data, i)).len() as u32,
            species,
        }
    }
}

fn mix(a: u64, b: u64) -> u64 {
    let mut z = a
        .wrapping_add(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(b.rotate_left(31));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

// ---------------------------------------------------------------------------
// Range model (Doc 09 §6.4 redesign): Origin-Envelope Presence Fields.
//
// Replaces the old lat/lon-box endemism — where a range was a rectangle that
// couldn't wrap the globe and had hard straight edges — with a smooth field: a
// lineage is born at a deterministic birthplace inside its region, and its range
// fades outward by great-circle distance × climate match × habitability. This
// wraps the sphere with no date-line seam and feathers naturally at the edges,
// while the coarse `region` field is kept purely to seed distinct clades per area.
// ---------------------------------------------------------------------------

/// Range-shape tuning. `K_PROX` sets how fast a range fades with great-circle
/// distance from its birthplace (bigger ⇒ tighter); `SIGMA_*` set the climate
/// tolerance; `TAU` is the presence a hex needs to count as within an endemic's
/// range. Global (not yet per-lineage) — see limitations.
const RANGE_K_PROX: f32 = 6.0;
const RANGE_SIGMA_T: f32 = 12.0;
const RANGE_SIGMA_M: f32 = 0.30;
const RANGE_TAU: f32 = 0.05;

/// Saturating moisture ∈ [0,1) from annual precipitation — the same 800 mm shape
/// as `richness::moisture_factor`, so the range envelope speaks the same climate
/// language the richness model does.
fn moisture(precip_mm: f32) -> f32 {
    1.0 - (-precip_mm.max(0.0) / 800.0).exp()
}

/// The ecological realm of a hex — free-standing so the range index can gate
/// birthplaces by realm without a view in hand (mirrors `realm_at`).
fn realm_at_hex(data: &WorldData, i: usize) -> Realm {
    let water = data.water_level_m.get(i).copied().unwrap_or(WATER_NONE);
    if !(water.is_finite() && water > data.elevation_mean[i]) {
        return Realm::Terrestrial;
    }
    let freshwater = data
        .water_body_id
        .get(i)
        .copied()
        .and_then(|id| data.water_bodies.get(&id))
        .map(|b| {
            use genesis_core::data::WaterBodyKind::{Lake, SaltLake};
            matches!(b.kind, Lake | SaltLake)
        })
        .unwrap_or(false);
    if freshwater {
        Realm::Freshwater
    } else {
        Realm::Marine
    }
}

/// A hex supports life at all (not barren, positive richness).
fn hex_habitable(data: &WorldData, i: usize) -> bool {
    data.biotic_richness.get(i).copied().unwrap_or(0.0) > 0.0
        && data.biome.get(i).map(|b| b.0) != Some(BARREN_ID)
}

/// Deterministic uniform in (0,1) from a mixed 64-bit hash (top 53 bits → f64).
fn uniform01(z: u64) -> f64 {
    ((z >> 11) as f64 + 1.0) / ((1u64 << 53) as f64 + 1.0)
}

/// One lineage present at a hex, with the strength of its presence there.
struct PresentLineage<'a> {
    guild_name: &'static str,
    lineage: &'a LineageRecord,
    strength: f32,
}

/// The precomputed spatial range model: per-lineage birthplace direction + frozen
/// climate niche, guild buckets, and an Euler tour for O(1) subtree tests. Built
/// once, then read-only — a pure function of `(ledger, WorldData)` (the seed
/// enters through the ledger's `name_seed`s).
#[derive(PartialEq)]
struct RangeIndex {
    /// Unit direction of each lineage's birthplace hex (`[0;3]` where unused).
    origin_dir: Vec<[f32; 3]>,
    /// Temperature (°C) and moisture at the birthplace — the preferred niche.
    t0: Vec<f32>,
    m0: Vec<f32>,
    /// Guild-bearing lineages (endemic *and* cosmopolitan) bucketed by guild.
    by_guild: std::collections::BTreeMap<GuildId, Vec<u32>>,
    /// Euler-tour entry/exit times, for `in_subtree` interval tests.
    tin: Vec<u32>,
    tout: Vec<u32>,
}

impl RangeIndex {
    /// Builds the index against `data`. Deterministic: every loop iterates in
    /// LineageId / HexId-ascending order and uses `BTreeMap`/`BTreeSet` (never a
    /// `HashMap`), so two builds on the same `(ledger, data)` are byte-identical.
    fn build(ledger: &Ledger, data: &WorldData, roster: &GuildRoster) -> RangeIndex {
        let n = ledger.len();
        let mut origin_dir = vec![[0.0f32; 3]; n];
        let mut t0 = vec![0.0f32; n];
        let mut m0 = vec![0.0f32; n];

        // --- Euler tour (parents precede children in push order) ---
        let mut children: Vec<Vec<u32>> = vec![Vec::new(); n];
        let mut roots: Vec<u32> = Vec::new();
        for l in ledger.iter() {
            match l.parent {
                Some(p) => children[p.0 as usize].push(l.id.0 as u32),
                None => roots.push(l.id.0 as u32),
            }
        }
        let mut tin = vec![0u32; n];
        let mut tout = vec![0u32; n];
        let mut timer = 0u32;
        enum Visit {
            Enter(u32),
            Exit(u32),
        }
        let mut stack: Vec<Visit> = roots.iter().rev().map(|&r| Visit::Enter(r)).collect();
        while let Some(v) = stack.pop() {
            match v {
                Visit::Enter(x) => {
                    tin[x as usize] = timer;
                    timer += 1;
                    stack.push(Visit::Exit(x));
                    for &c in children[x as usize].iter().rev() {
                        stack.push(Visit::Enter(c));
                    }
                }
                Visit::Exit(x) => tout[x as usize] = timer,
            }
        }

        // --- Guild buckets (ascending id) + which guilds are marine ---
        let mut by_guild: std::collections::BTreeMap<GuildId, Vec<u32>> = Default::default();
        for l in ledger.iter() {
            if l.guild != GuildId::NONE {
                by_guild.entry(l.guild).or_default().push(l.id.0 as u32);
            }
        }
        let marine_guilds: std::collections::BTreeSet<GuildId> = cascade_order(Realm::Marine)
            .iter()
            .filter_map(|name| roster.iter().find(|g| g.name == *name).map(|g| g.id))
            .collect();

        // --- Region → member hexes (ascending), one pass ---
        let ncells = data.cell_count() as usize;
        let mut region_hexes: Vec<Vec<u32>> = vec![Vec::new(); BIOGEOGRAPHIC_REGIONS as usize];
        for i in 0..ncells {
            let (lat, lon) = data.grid.center_lat_lon(HexId(i as u32));
            let reg = geo_region(lat, lon) as usize;
            if reg < region_hexes.len() {
                region_hexes[reg].push(i as u32);
            }
        }

        // --- Birthplace per endemic, guild-bearing lineage ---
        for l in ledger.iter() {
            let (Some(region), true) = (l.region, l.guild != GuildId::NONE) else {
                continue;
            };
            let want_marine = marine_guilds.contains(&l.guild);
            let hexes = &region_hexes[region as usize];
            // Efraimidis–Spirakis weighted reservoir (weight = richness): keep the
            // hex with the largest key ln(u)/w, iterating in HexId order — so the
            // birthplace lands in the region's most habitable spot, deterministically.
            let mut best_key = f64::NEG_INFINITY;
            let mut best: Option<u32> = None;
            for &h in hexes {
                let hi = h as usize;
                if !hex_habitable(data, hi) {
                    continue;
                }
                if (realm_at_hex(data, hi) == Realm::Marine) != want_marine {
                    continue;
                }
                let w = data.biotic_richness[hi].max(1e-4) as f64;
                let key = uniform01(mix(l.name_seed, h as u64)).ln() / w;
                if key > best_key {
                    best_key = key;
                    best = Some(h);
                }
            }
            // Fallback (region has no realm-matched habitable hex): any hex in the
            // box — the clade then paints faintly, which is acceptable.
            let origin = best.or_else(|| hexes.first().copied()).unwrap_or(0);
            let lid = l.id.0 as usize;
            let d = data.grid.cell_center_direction(HexId(origin));
            origin_dir[lid] = [d[0] as f32, d[1] as f32, d[2] as f32];
            let oi = origin as usize;
            t0[lid] = data.temperature_mean.get(oi).copied().unwrap_or(0.0);
            m0[lid] = moisture(data.precipitation.get(oi).copied().unwrap_or(0.0));
        }

        RangeIndex {
            origin_dir,
            t0,
            m0,
            by_guild,
            tin,
            tout,
        }
    }

    /// Whether lineage `l` lies in the subtree rooted at `root` — an O(1)
    /// Euler-tour interval test (replaces the old per-hex subtree rebuild).
    fn in_subtree(&self, root: u64, l: u64) -> bool {
        let (ri, li) = (root as usize, l as usize);
        if ri >= self.tin.len() || li >= self.tin.len() {
            return false;
        }
        self.tin[li] >= self.tin[ri] && self.tin[li] < self.tout[ri]
    }
}

/// Depth from the root for every lineage (LUCA = 0). Relies on the ledger being
/// in push order (parents before children).
fn depth_map(ledger: &Ledger) -> std::collections::BTreeMap<u64, u32> {
    let mut depth = std::collections::BTreeMap::new();
    for l in ledger.iter() {
        let d = match l.parent {
            None => 0,
            Some(p) => depth.get(&p.0).copied().unwrap_or(0) + 1,
        };
        depth.insert(l.id.0, d);
    }
    depth
}

/// A one-line "how it makes a living" story for a guild — so descriptions lead
/// with the organism's actual ecology (a producer makes its own food; it isn't a
/// hungry predator), keeping the text coherent with its guild.
fn guild_story(guild: &str) -> &'static str {
    match guild {
        "producer" => "a producer — it makes its own food from sunlight or chemistry",
        "phytoplankton" => "a drifting marine producer — it photosynthesizes in the sunlit water",
        "herbivore" => "a herbivore — it grazes on producers",
        "apex_predator" => "an apex predator — it hunts other animals",
        "nekton_predator" => "a marine predator — it hunts other swimmers",
        "filter_feeder" => "a filter feeder — it strains small food from the water",
        "decomposer" | "marine_decomposer" => "a decomposer — it breaks down dead matter",
        _ => "a living thing",
    }
}

/// The guilds one trophic level below (prey) and above (predators) a guild
/// (Doc 09 §5.3). Decomposers feed on detritus, so they have no living prey/predator.
fn trophic_neighbors(guild: &str) -> (Option<&'static str>, Option<&'static str>) {
    match guild {
        "producer" => (None, Some("herbivore")),
        "herbivore" => (Some("producer"), Some("apex_predator")),
        "apex_predator" => (Some("herbivore"), None),
        "phytoplankton" => (None, Some("filter_feeder")),
        "filter_feeder" => (Some("phytoplankton"), Some("nekton_predator")),
        "nekton_predator" => (Some("filter_feeder"), None),
        _ => (None, None),
    }
}

/// A "notability" score so the global Bestiary surfaces the charismatic species
/// first (the user's "pay attention to these"): a bigger genome, plus marquee
/// traits (sapience, big brains, apex predation, flight, giant size).
fn notable_prominence(graph: &TraitGraph, l: &LineageRecord) -> f32 {
    let mut score = l.trait_set.len() as f32;
    let bonus = [
        ("core:sapience", 30.0),
        ("core:complex_cognition", 14.0),
        ("core:hypercarnivore", 8.0),
        ("core:powered_flight", 7.0),
        ("core:size_mega", 7.0),
        ("core:size_large", 3.0),
        ("core:eusocial", 4.0),
    ];
    for (name, pts) in bonus {
        if let Some(id) = graph.id_of(name)
            && l.trait_set.contains(id)
        {
            score += pts;
        }
    }
    score
}

/// The biogeographic region a hex falls in (0..`BIOGEOGRAPHIC_REGIONS`): a coarse
/// latitude-band × longitude-sector grid, so endemic clades vary across the world
/// yet stay coherent within a region (Doc 09 §6.4).
pub(crate) fn geo_region(lat_rad: f64, lon_rad: f64) -> u16 {
    let band: u16 = if lat_rad < -0.35 {
        0
    } else if lat_rad < 0.35 {
        1
    } else {
        2
    };
    let t = lon_rad.rem_euclid(std::f64::consts::TAU) / std::f64::consts::TAU;
    let sector = ((t * 4.0) as u16).min(3);
    (band * 4 + sector).min(BIOGEOGRAPHIC_REGIONS - 1)
}

/// Turns a genus name into a family name (genus stem + "-idae"), e.g. "Nataops"
/// → "Nataopsidae" — the Linnaean family convention.
fn family_from_genus(genus: &str) -> String {
    let stem = genus.trim_end_matches(['a', 'e', 'i', 'o', 'u', 's']);
    let stem = if stem.is_empty() { genus } else { stem };
    format!("{stem}idae")
}

/// Whether an axis is a **progression** whose grades supersede one another
/// (Doc 09 §2.3) — Organization runs unicellular → eukaryote → colonial →
/// multicellular, so a multicellular species should not also display
/// "unicellular". Additive axes (Structure, Sensory, …) keep every trait.
fn is_progression_axis(axis: TraitAxis) -> bool {
    matches!(axis, TraitAxis::Organization)
}

/// Whether `u` (transitively) depends on `t` through its prerequisite chain.
fn depends_on(graph: &TraitGraph, u: TraitId, t: TraitId) -> bool {
    graph.node(u).prerequisites.iter().any(|&p| {
        // The chain is short; the graph is a DAG, so this terminates.
        p == t || depends_on(graph, p, t)
    })
}

/// The traits of a genome worth displaying: on a progression axis, an ancestral
/// grade is hidden once a later grade that supersedes it (depends on it) is also
/// present, so genomes read as their current grade rather than the whole ladder
/// (Doc 09 §2.3; limitation 4). Additive-axis traits are all kept.
fn visible_traits(graph: &TraitGraph, trait_set: &TraitSet) -> Vec<(u8, String)> {
    trait_set
        .iter()
        .filter(|&t| {
            let axis = graph.node(t).axis;
            if !is_progression_axis(axis) {
                return true;
            }
            // Hidden if a present, same-axis trait supersedes it.
            !trait_set
                .iter()
                .any(|u| u != t && graph.node(u).axis == axis && depends_on(graph, u, t))
        })
        .map(|id| {
            let node = graph.node(id);
            (node.tier, node.display.clone())
        })
        .collect()
}

/// The most-specific (shallowest-tier) traits of a genome, as chips.
fn chips_from_traits(graph: &TraitGraph, trait_set: &TraitSet) -> Vec<String> {
    let mut ts = visible_traits(graph, trait_set);
    ts.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    ts.into_iter().take(4).map(|(_, n)| n).collect()
}

/// A species' full genome as chips (specific traits first), for the detail
/// panel — the same ordering as [`chips_from_traits`] but without the 4-chip cap.
fn detail_chips(graph: &TraitGraph, trait_set: &TraitSet) -> Vec<String> {
    let mut ts = visible_traits(graph, trait_set);
    ts.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    ts.into_iter().take(12).map(|(_, n)| n).collect()
}

/// The visible traits as `(display, description)` pairs, for hover tooltips.
fn detail_trait_info(graph: &TraitGraph, trait_set: &TraitSet) -> Vec<(String, String)> {
    let mut ts: Vec<(u8, String, String)> = trait_set
        .iter()
        .filter(|&t| {
            let axis = graph.node(t).axis;
            !is_progression_axis(axis)
                || !trait_set
                    .iter()
                    .any(|u| u != t && graph.node(u).axis == axis && depends_on(graph, u, t))
        })
        .map(|id| {
            let n = graph.node(id);
            (n.tier, n.display.clone(), n.description.clone())
        })
        .collect();
    ts.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    ts.into_iter()
        .take(12)
        .map(|(_, name, desc)| (name, desc))
        .collect()
}

impl BiologyView for GeneratedBiologyView {
    fn biome_at(&self, data: &WorldData, hex: HexId) -> BiomeId {
        data.biome
            .get(hex.0 as usize)
            .copied()
            .unwrap_or(BiomeId::NONE)
    }

    fn biome_name(&self, biome: BiomeId) -> String {
        biome_name(biome).to_string()
    }

    fn richness_at(&self, data: &WorldData, hex: HexId) -> f32 {
        data.biotic_richness
            .get(hex.0 as usize)
            .copied()
            .unwrap_or(0.0)
    }

    fn biomass_at(&self, data: &WorldData, hex: HexId) -> f32 {
        (data.biomass.get(hex.0 as usize).copied().unwrap_or(0.0) / BIOMASS_FULL).clamp(0.0, 1.0)
    }

    fn occupied_guilds(&self, data: &WorldData, hex: HexId) -> Vec<GuildSummary> {
        let occupied = self.occupied_guild_names(data, hex.0 as usize);
        let realm = self.realm_at(data, hex.0 as usize);
        cascade_order(realm)
            .iter()
            .map(|name| GuildSummary {
                name: (*name).to_string(),
                occupied: occupied.contains(name),
            })
            .collect()
    }

    fn assemblage(&self, data: &WorldData, hex: HexId) -> Assemblage {
        if let Some(ledger) = &self.ledger {
            return self.assemblage_from_ledger(ledger, data, hex);
        }
        // Pre-ledger fallback: guild names only, no species yet.
        let i = hex.0 as usize;
        Assemblage {
            biome_name: self.biome_name(self.biome_at(data, hex)),
            richness: self.richness_at(data, hex),
            occupied_guilds: self.occupied_guild_names(data, i).len() as u32,
            guild_capacity: cascade_order(self.realm_at(data, i)).len() as u32,
            species: Vec::new(),
        }
    }

    fn tree_snapshot(&self, year: WorldYear) -> TreePeek {
        if let Some(ledger) = &self.ledger {
            return self.tree_from_ledger(ledger, year);
        }
        // Pre-ledger fallback: just LUCA.
        TreePeek {
            nodes: vec![TreeNodePeek {
                id: 0,
                parent: None,
                name: "Root (LUCA)".to_string(),
                rank: "life".to_string(),
                depth: 0,
                species_id: 0,
                defining_trait: "the origin of life".to_string(),
                origin_year: 0,
                extinction_year: None,
            }],
        }
    }

    fn life_events(&self, from: WorldYear, to: WorldYear) -> Vec<LifeEventPip> {
        // Generated milestone pips — the real event log reaching the viewer is a
        // later plumbing step, so years won't match the true emergence.
        let j = (self.seed % 60) as i64 * 1_000_000;
        [
            (280_000_000 + j, "Life emerges", LifeEventCategory::Origin),
            (
                320_000_000 + j,
                "Great Oxygenation",
                LifeEventCategory::Milestone,
            ),
            (
                600_000_000 + j,
                "Cambrian-style radiation",
                LifeEventCategory::Innovation,
            ),
        ]
        .into_iter()
        .filter(|(yr, _, _)| *yr >= from.value() && *yr <= to.value())
        .map(|(yr, label, category)| LifeEventPip {
            year: yr,
            label: label.to_string(),
            category,
        })
        .collect()
    }

    fn species_detail(&self, species_id: u64) -> Option<SpeciesDetail> {
        let ledger = self.ledger.as_ref()?;
        // Species are keyed by `name_seed` (the `SpeciesPeek::species_id` the
        // Bestiary/Life-tab hand back on click).
        let lineage = ledger.iter().find(|l| l.name_seed == species_id)?;

        // Classification ladder — the *separate* Linnaean view (Doc 09 §9.2):
        // walk root → this lineage and label each ancestor by its **nesting
        // depth** (Kingdom, Phylum, Class, …), so the ladder is always a properly
        // nested hierarchy ending at this species. (The Tree of Life shows the
        // same phylogeny; this presents it as ranks.)
        let mut chain: Vec<&LineageRecord> = Vec::new();
        let mut cur = Some(lineage.id);
        while let Some(id) = cur {
            let rec = ledger.get(id).expect("ancestor in ledger");
            chain.push(rec);
            cur = rec.parent;
        }
        chain.reverse(); // root first, species last
        let ancestors: Vec<&LineageRecord> = chain.into_iter().skip(1).collect(); // drop LUCA root
        let last = ancestors.len().saturating_sub(1);
        let classification: Vec<(String, String)> = ancestors
            .iter()
            .enumerate()
            .map(|(depth, rec)| {
                // Deepest node is always the species; ancestors take the standard
                // ranks by depth (kingdom → phylum → … → genus).
                let rank = if depth == last {
                    "species".to_string()
                } else {
                    rank_for_tier(depth as u8).to_string()
                };
                (rank, self.lineage_name(rec))
            })
            .collect();

        let guild = self
            .roster
            .iter()
            .find(|g| g.id == lineage.guild)
            .map(|g| g.name.to_string())
            .unwrap_or_else(|| "—".to_string());
        let family = self.family_name(ledger, lineage);
        let defining = lineage.trait_delta.map(|t| self.graph.node(t));
        let defining_name = defining
            .map(|n| n.display.clone())
            .unwrap_or_else(|| "a primitive form".to_string());
        // Pull the trait's own definition into the blurb so descriptions say what
        // the trait *is* ("radial symmetry — a body arranged around a central
        // axis…"), not just its terse name.
        let defining_detail = defining
            .filter(|n| !n.description.is_empty())
            .map(|n| format!(" {}", n.description))
            .unwrap_or_default();
        let status = if lineage.extinction_year.is_some() {
            "an extinct"
        } else {
            "a living"
        };
        // Lead with the ecological role (guild story), so the blurb reads coherently
        // — a producer sounds like a producer, then its distinguishing trait.
        let description = format!(
            "This is {status} member of family {family} — {}. It is distinguished by {defining_name}.{defining_detail} Its genome carries {} traits.",
            guild_story(&guild),
            lineage.trait_set.len()
        );

        Some(SpeciesDetail {
            name: self.lineage_name(lineage),
            guild,
            family,
            description,
            trait_chips: detail_chips(&self.graph, &lineage.trait_set),
            trait_details: detail_trait_info(&self.graph, &lineage.trait_set),
            classification,
        })
    }

    fn food_web(&self, species_id: u64, year: WorldYear) -> FoodWeb {
        let Some(ledger) = &self.ledger else {
            return FoodWeb::default();
        };
        let Some(lineage) = ledger.iter().find(|l| l.name_seed == species_id) else {
            return FoodWeb::default();
        };
        // An extinct species is out of the pool — no living web (Doc 09 §5.3).
        if !lineage.alive_at(year) {
            return FoodWeb::default();
        }
        // The web is regional; the cosmopolitan basal seeds have no home region.
        let Some(region) = lineage.region else {
            return FoodWeb::default();
        };
        let Some(guild_name) = self
            .roster
            .iter()
            .find(|g| g.id == lineage.guild)
            .map(|g| g.name)
        else {
            return FoodWeb::default();
        };
        let (prey_g, pred_g) = trophic_neighbors(guild_name);

        let collect = |gname: Option<&'static str>, note: &str| -> Vec<SpeciesPeek> {
            let Some(gname) = gname else {
                return Vec::new();
            };
            let Some(gid) = self.roster.iter().find(|g| g.name == gname).map(|g| g.id) else {
                return Vec::new();
            };
            ledger
                .extant_in_guild_region(gid, region, year)
                .take(FOOD_WEB_LIMIT)
                .map(|l| self.peek_from_lineage(ledger, l, gname, note.to_string()))
                .collect()
        };

        FoodWeb {
            prey: collect(prey_g, "prey — what it eats"),
            predators: collect(pred_g, "predator — what eats it"),
            competitors: ledger
                .extant_in_guild_region(lineage.guild, region, year)
                // Exclude the species itself and the cosmopolitan basal seeds — a
                // seed matches every region, so it would list as everyone's
                // competitor ("everything competes with Ventus"). Seeds still show
                // up as prey (they are a real food source).
                .filter(|l| l.name_seed != species_id && l.region.is_some())
                .take(FOOD_WEB_LIMIT)
                .map(|l| {
                    self.peek_from_lineage(
                        ledger,
                        l,
                        guild_name,
                        "competitor — shares its niche".to_string(),
                    )
                })
                .collect(),
        }
    }

    fn species_catalog(&self, year: WorldYear) -> Vec<SpeciesPeek> {
        let Some(ledger) = &self.ledger else {
            return Vec::new();
        };
        let mut scored: Vec<(f32, SpeciesPeek)> = ledger
            .iter()
            .filter(|l| l.guild != GuildId::NONE && l.alive_at(year))
            .map(|l| {
                let gname = self
                    .roster
                    .iter()
                    .find(|g| g.id == l.guild)
                    .map(|g| g.name)
                    .unwrap_or("life");
                let peek = self.peek_from_lineage(ledger, l, gname, format!("A {gname}."));
                (notable_prominence(&self.graph, l), peek)
            })
            .collect();
        // Most-notable first, then alphabetical (the UI can re-sort A–Z).
        scored.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.1.name.cmp(&b.1.name))
        });
        scored.into_iter().map(|(_, p)| p).collect()
    }

    fn dominant_clade(&self, year: WorldYear) -> Option<String> {
        let ledger = self.ledger.as_ref()?;
        let depth = depth_map(ledger);
        // Tally living species under each major clade (a fixed ancestor depth) and
        // name the era after the biggest — "Age of the …".
        const CLADE_DEPTH: u32 = 3;
        let mut tally: std::collections::BTreeMap<u64, u32> = std::collections::BTreeMap::new();
        for l in ledger.iter() {
            if l.guild == GuildId::NONE || !l.alive_at(year) {
                continue;
            }
            let mut cur = l;
            while depth.get(&cur.id.0).copied().unwrap_or(0) > CLADE_DEPTH {
                match cur.parent.and_then(|p| ledger.get(p)) {
                    Some(p) => cur = p,
                    None => break,
                }
            }
            *tally.entry(cur.id.0).or_insert(0) += 1;
        }
        let (best_id, _) = tally.into_iter().max_by_key(|(_, c)| *c)?;
        let clade = ledger.get(genesis_core::data::LineageId(best_id))?;
        Some(format!("Age of the {}", self.lineage_name(clade)))
    }

    fn relatives(&self, species_id: u64, year: WorldYear) -> Vec<SpeciesPeek> {
        let Some(ledger) = &self.ledger else {
            return Vec::new();
        };
        let Some(lineage) = ledger.iter().find(|l| l.name_seed == species_id) else {
            return Vec::new();
        };
        let Some(parent) = lineage.parent else {
            return Vec::new();
        };
        let guild_name = |l: &LineageRecord| {
            self.roster
                .iter()
                .find(|g| g.id == l.guild)
                .map(|g| g.name)
                .unwrap_or("life")
        };
        let mut out: Vec<SpeciesPeek> = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let mut add = |l: &LineageRecord, note: &str, out: &mut Vec<SpeciesPeek>| {
            if l.name_seed != species_id
                && l.guild != GuildId::NONE
                && l.alive_at(year)
                && seen.insert(l.name_seed)
            {
                out.push(self.peek_from_lineage(ledger, l, guild_name(l), note.to_string()));
            }
        };
        // Siblings first (same parent), then cousins (grandparent's grandchildren).
        for l in ledger.iter().filter(|l| l.parent == Some(parent)) {
            add(l, "sibling — same immediate ancestor", &mut out);
        }
        if out.len() < RELATIVES_LIMIT
            && let Some(gp) = ledger.get(parent).and_then(|p| p.parent)
        {
            let uncles: std::collections::BTreeSet<u64> = ledger
                .iter()
                .filter(|l| l.parent == Some(gp))
                .map(|l| l.id.0)
                .collect();
            for l in ledger
                .iter()
                .filter(|l| l.parent.is_some_and(|p| uncles.contains(&p.0)))
            {
                if out.len() >= RELATIVES_LIMIT {
                    break;
                }
                add(l, "cousin — shares a deeper ancestor", &mut out);
            }
        }
        out.truncate(RELATIVES_LIMIT);
        out
    }

    fn clade_concentration(
        &self,
        data: &WorldData,
        hex: HexId,
        target: genesis_core::biology_view::LineageSelector,
    ) -> f32 {
        use genesis_core::biology_view::LineageSelector::{Clade, Species};
        let Some(ledger) = &self.ledger else {
            return 0.0;
        };
        // Resolve the selection to the id of its subtree root.
        let root = match target {
            Clade(id) => id,
            Species(seed) => match ledger.iter().find(|l| l.name_seed == seed) {
                Some(l) => l.id.0,
                None => return 0.0,
            },
        };
        let i = hex.0 as usize;
        // The same present-lineages the Bestiary lists (and this primes the range
        // index). The concentration is the strongest presence of any species in
        // the selected subtree — a smoothly graded field, so the heatmap fades
        // instead of stepping, and it is > 0 here iff a subtree species is listed.
        let present = self.present_lineages_at(data, i);
        if present.is_empty() {
            return 0.0;
        }
        let Some(idx) = self.range.get() else {
            return 0.0;
        };
        let mut best = 0.0f32;
        for p in &present {
            if idx.in_subtree(root, p.lineage.id.0) {
                best = best.max(p.strength);
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::speciation::build_radiation;
    use genesis_core::data::BiomeId;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{HexId, create_world};

    fn rich_world() -> WorldData {
        let mut world = create_world(WorldParameters::default())
            .expect("world")
            .data;
        world.current_year = WorldYear(1_000_000_000);
        for i in 0..world.cell_count() as usize {
            world.biome[i] = BiomeId(11); // tropical forest
            world.biotic_richness[i] = 0.8;
            world.biomass[i] = 900.0;
            world.water_level_m[i] = genesis_core::data::WATER_NONE;
            world.elevation_mean[i] = 200.0;
        }
        world
    }

    #[test]
    fn ledger_backed_tree_and_species() {
        let graph = core_morphospace();
        let roster = core_guilds(&graph);
        let ledger = build_radiation(&graph, &roster, 42, WorldYear(300_000_000));
        let view = GeneratedBiologyView::with_ledger(42, ledger);
        let world = rich_world();

        let tree = view.tree_snapshot(WorldYear(1_000_000_000));
        assert!(tree.nodes.len() > 20, "real tree has many lineages");
        assert_eq!(tree.nodes[0].name, "Root (LUCA)");
        // Descent: every non-root node has a parent that is also in the tree.
        let ids: std::collections::BTreeSet<u64> = tree.nodes.iter().map(|n| n.id).collect();
        for n in tree.nodes.iter().filter(|n| n.parent.is_some()) {
            assert!(ids.contains(&n.parent.unwrap()));
        }
        let a = view.assemblage(&world, HexId(0));
        assert!(!a.species.is_empty(), "ledger species on a rich land hex");
    }

    #[test]
    fn new_has_a_populated_tree_without_waiting_for_generation() {
        // Regression: the adapter must show the real tree immediately, not only
        // after the ledger arrives at generation end.
        let view = GeneratedBiologyView::new(42);
        let tree = view.tree_snapshot(WorldYear(2_000_000_000));
        assert!(
            tree.nodes.len() > 20,
            "tree available at load, got {}",
            tree.nodes.len()
        );
        let a = view.assemblage(&rich_world(), HexId(0));
        assert!(!a.species.is_empty(), "species available at load");
    }

    #[test]
    fn regions_have_distinct_endemic_species() {
        // rich_world makes every hex identical *except position*, so any variety
        // in the assemblages comes purely from biogeographic region (endemism).
        let view = GeneratedBiologyView::new(42);
        let world = rich_world();
        let mut distinct = std::collections::BTreeSet::new();
        for h in (0..world.cell_count()).step_by(37) {
            let names: Vec<String> = view
                .assemblage(&world, HexId(h))
                .species
                .iter()
                .map(|s| s.name.clone())
                .collect();
            if !names.is_empty() {
                distinct.insert(names);
            }
        }
        assert!(
            distinct.len() > 1,
            "endemism: different regions should differ, got {} distinct assemblages",
            distinct.len()
        );
    }

    #[test]
    fn catalog_food_web_and_era() {
        let view = GeneratedBiologyView::new(42);
        let year = WorldYear(1_000_000_000);

        // Global catalog: many species, no hex needed, prominence-sorted.
        let catalog = view.species_catalog(year);
        assert!(
            catalog.len() > 10,
            "global catalog should list many species, got {}",
            catalog.len()
        );

        // Food web: at least one species has trophic neighbors (prey/predators/
        // competitors) in its region.
        let web_found = catalog.iter().take(60).any(|sp| {
            let w = view.food_web(sp.species_id, year);
            !w.prey.is_empty() || !w.predators.is_empty() || !w.competitors.is_empty()
        });
        assert!(web_found, "some species should have a regional food web");

        // Era named for the dominant clade.
        let era = view.dominant_clade(year).expect("an era should be named");
        assert!(era.starts_with("Age of the"), "unexpected era: {era}");
    }

    #[test]
    fn tree_nodes_carry_species_id_and_depth() {
        let view = GeneratedBiologyView::new(42);
        let tree = view.tree_snapshot(WorldYear(1_000_000_000));
        let root = tree.nodes.iter().find(|n| n.parent.is_none()).unwrap();
        assert_eq!(root.depth, 0);
        assert_eq!(root.rank, "life");
        // A leaf species carries the species_id its detail is keyed by.
        let leaf = tree.nodes.iter().find(|n| n.rank == "species");
        if let Some(leaf) = leaf {
            assert!(view.species_detail(leaf.species_id).is_some());
        }
    }

    #[test]
    fn tree_is_empty_before_life_emerges() {
        // Regression: LUCA must not hover over a lifeless young planet — the tree
        // is empty until biogenesis (DEFAULT_RADIATION_YEAR 400My → emergence 200My).
        let view = GeneratedBiologyView::new(42);
        assert!(
            view.tree_snapshot(WorldYear(100_000_000)).nodes.is_empty(),
            "no lineages before life emerges"
        );
        assert!(
            !view.tree_snapshot(WorldYear(300_000_000)).nodes.is_empty(),
            "life present after emergence"
        );
    }

    #[test]
    fn ocean_hexes_carry_basal_marine_life() {
        // Regression: oceans were ~100% lifeless because every lineage collapsed
        // onto the terrestrial 'producer' guild. Now a warm ocean hex carries
        // basal marine life from the cosmopolitan seeds.
        let view = GeneratedBiologyView::new(42);
        let mut world = rich_world(); // rich_world makes every hex land
        world.water_level_m[0] = 0.0;
        world.elevation_mean[0] = -50.0; // shallow warm sea
        let a = view.assemblage(&world, HexId(0));
        assert!(
            !a.species.is_empty(),
            "ocean hex must carry basal marine life"
        );
        assert!(
            a.species
                .iter()
                .any(|s| s.guild == "phytoplankton" || s.guild == "marine_decomposer"),
            "expected a marine guild, got {:?}",
            a.species
                .iter()
                .map(|s| s.guild.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn occupancy_is_contingent_on_richness() {
        let view = GeneratedBiologyView::new(42);
        let rich = rich_world(); // R = 0.8
        let mut poor = rich_world();
        for i in 0..poor.cell_count() as usize {
            poor.biotic_richness[i] = 0.1; // too low for herbivores/predators
        }
        let rich_n = view.assemblage(&rich, HexId(0)).occupied_guilds;
        let poor_n = view.assemblage(&poor, HexId(0)).occupied_guilds;
        assert!(
            poor_n <= 2,
            "low R should occupy only guaranteed guilds, got {poor_n}"
        );
        assert!(
            rich_n >= poor_n,
            "richer hex occupies at least as many guilds"
        );
    }

    #[test]
    fn species_detail_has_a_classification_ladder() {
        // A clicked species resolves to its full detail: a classification ladder
        // that starts at a kingdom and ends at this species, plus trait chips.
        let view = GeneratedBiologyView::new(7);
        let world = rich_world();
        let sp = view
            .assemblage(&world, HexId(0))
            .species
            .into_iter()
            .next()
            .expect("a species on a rich hex");

        let detail = view
            .species_detail(sp.species_id)
            .expect("clicked species has detail");
        assert_eq!(detail.name, sp.name, "detail matches the clicked card");
        assert!(
            !detail.classification.is_empty(),
            "classification ladder is populated"
        );
        // The ladder ends at the species itself.
        let (last_rank, last_clade) = detail.classification.last().unwrap();
        assert_eq!(last_rank, "species");
        assert_eq!(last_clade, &detail.name);
        // And it descends from a kingdom-rank ancestor.
        assert_eq!(detail.classification[0].0, "kingdom");
        assert!(!detail.trait_chips.is_empty(), "genome chips present");

        // An unknown id yields no detail.
        assert!(view.species_detail(u64::MAX).is_none());
    }

    #[test]
    fn superseded_organization_grades_are_hidden_from_chips() {
        // A multicellular genome should read as "multicellular", not also list the
        // ancestral unicellular/eukaryote/colonial grades it developed through
        // (Doc 09 §2.3; limitation 4).
        let graph = core_morphospace();
        let id = |n: &str| graph.id_of(n).expect("core trait");
        let genome: TraitSet = [
            id("core:chemosynthesis"),
            id("core:unicellular"),
            id("core:eukaryote"),
            id("core:colonial"),
            id("core:multicellular"),
        ]
        .into_iter()
        .collect();
        let chips = detail_chips(&graph, &genome);
        assert!(
            chips.iter().any(|c| c == "multicellular"),
            "current grade shown: {chips:?}"
        );
        for shed in ["unicellular", "eukaryote", "colonial"] {
            assert!(
                !chips.iter().any(|c| c == shed),
                "superseded grade {shed} should be hidden: {chips:?}"
            );
        }
        // A non-progression trait (metabolism) is still kept.
        assert!(chips.iter().any(|c| c == "chemosynthesis"));
    }

    #[test]
    fn tree_grows_with_time() {
        let graph = core_morphospace();
        let roster = core_guilds(&graph);
        let ledger = build_radiation(&graph, &roster, 1, WorldYear(300_000_000));
        let view = GeneratedBiologyView::with_ledger(1, ledger);
        let early = view.tree_snapshot(WorldYear(310_000_000)).nodes.len();
        let late = view.tree_snapshot(WorldYear(1_000_000_000)).nodes.len();
        assert!(early < late, "more branches have arisen by the later year");
    }

    // ---- Range model (Doc 09 §6.4 redesign) ----

    /// An alive, guild-bearing, endemic (`region = Some`) species' id — the kind
    /// whose range the map paints as a bounded blob.
    fn an_endemic_species(view: &GeneratedBiologyView, year: WorldYear) -> u64 {
        view.ledger
            .as_ref()
            .unwrap()
            .iter()
            .find(|l| l.alive_at(year) && l.region.is_some() && l.guild != GuildId::NONE)
            .expect("a living endemic species")
            .name_seed
    }

    #[test]
    fn range_index_build_is_deterministic() {
        // Two builds on the same (ledger, world) must be byte-identical — guards
        // against any HashMap / float-order leak in the index.
        let graph = core_morphospace();
        let roster = core_guilds(&graph);
        let ledger = build_radiation(&graph, &roster, 42, WorldYear(300_000_000));
        let world = rich_world();
        let a = RangeIndex::build(&ledger, &world, &roster);
        let b = RangeIndex::build(&ledger, &world, &roster);
        assert!(a == b, "RangeIndex::build must be deterministic");
    }

    #[test]
    fn endemic_range_crosses_region_boxes_and_is_graded() {
        // The whole point of the redesign: a range is no longer a lat/lon box.
        // A single endemic species' painted range must span more than one of the
        // 12 geo_region boxes (so it is not walled at a box edge) and take a
        // spread of concentration values (a soft gradient, not a 1/0 step).
        use genesis_core::biology_view::LineageSelector::Species;
        let view = GeneratedBiologyView::new(42);
        let world = rich_world();
        let year = world.current_year;
        let id = an_endemic_species(&view, year);

        let mut regions = std::collections::BTreeSet::new();
        let mut positive = 0usize;
        let mut peak = 0.0f32;
        let mut min_pos = f32::INFINITY;
        for h in 0..world.cell_count() {
            let c = view.clade_concentration(&world, HexId(h), Species(id));
            if c > 0.0 {
                positive += 1;
                peak = peak.max(c);
                min_pos = min_pos.min(c);
                let (lat, lon) = world.grid.center_lat_lon(HexId(h));
                regions.insert(geo_region(lat, lon));
            }
        }
        assert!(positive > 0, "the species must paint somewhere");
        assert!(
            regions.len() > 1,
            "an endemic range must cross region-box boundaries, spanned {} box(es)",
            regions.len()
        );
        assert!(
            positive < world.cell_count() as usize,
            "an endemic range must stay bounded, not blanket the globe"
        );
        assert!(
            peak - min_pos > 0.1,
            "concentration must be graded (soft gradient), got peak {peak} min {min_pos}"
        );
    }

    #[test]
    fn ranges_are_continuous_between_neighbours() {
        // Soft edges + globe wrap in one property: between any two physically
        // adjacent hexes (which, near the date line, sit on opposite sides of the
        // old longitude seam), a clade's concentration may not jump by a large
        // amount. The old boxed model stepped 1→0 across a box wall; this must not.
        use genesis_core::biology_view::LineageSelector::Species;
        let view = GeneratedBiologyView::new(42);
        let world = rich_world(); // uniform climate ⇒ only proximity varies
        let year = world.current_year;
        let id = an_endemic_species(&view, year);

        let conc: Vec<f32> = (0..world.cell_count())
            .map(|h| view.clade_concentration(&world, HexId(h), Species(id)))
            .collect();
        let mut worst = 0.0f32;
        for h in 0..world.cell_count() {
            let a = conc[h as usize];
            for nb in world.grid.neighbors(HexId(h)) {
                let b = conc[nb.0 as usize];
                worst = worst.max((a - b).abs());
            }
        }
        // Adjacent hexes are ~1–2° apart, so a smooth field barely changes across
        // one step. A hard box wall would give ~0.8 here.
        assert!(
            worst < 0.25,
            "range must be spatially continuous (incl. across the date line); \
             worst neighbour jump was {worst}"
        );
    }

    #[test]
    fn map_paint_matches_present_species() {
        // The consistency invariant: the map paints a clade at a hex iff a species
        // of that clade is actually present there — both routed through the one
        // shared predicate, so they cannot diverge.
        use genesis_core::biology_view::LineageSelector::Species;
        let view = GeneratedBiologyView::new(42);
        let world = rich_world();
        let year = world.current_year;
        let id = an_endemic_species(&view, year);
        let root = view
            .ledger
            .as_ref()
            .unwrap()
            .iter()
            .find(|l| l.name_seed == id)
            .unwrap()
            .id
            .0;

        for h in (0..world.cell_count()).step_by(23) {
            let painted = view.clade_concentration(&world, HexId(h), Species(id)) > 0.0;
            let idx = view.range.get().expect("primed by clade_concentration");
            let present = view
                .present_lineages_at(&world, h as usize)
                .iter()
                .any(|p| idx.in_subtree(root, p.lineage.id.0));
            assert_eq!(
                painted, present,
                "map paint must match the present-species set at hex {h}"
            );
        }
    }
}
