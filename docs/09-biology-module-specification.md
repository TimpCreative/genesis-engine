# 09 — Biology Module Specification

**Document Type:** Tier 2 — System Specification
**Status:** Draft v0.1
**Last Updated:** July 2026
**Owner:** Brax Johnson
**Implementing Phase:** 4 (Biology)

**Changelog:**
- v0.1 (July 2026): Initial full draft. Establishes the module's governing architecture — **"simulate niches, generate species"** — settled across a multi-round design conversation with the owner. The coupled, deterministic simulation runs over *functional guilds* per *biogeographic province*, producing a compact **ecological ledger**; individual named species, their traits, morphology, and the fine phylogenetic tree are **generated lazily on demand** as a pure function of the seed plus that ledger, never stored until the user asks. Evolution is a **biased walk through a trait morphospace** (a moddable graph of possible traits, with tiers, prerequisites, proximity weights, and directional asymmetry), pulled by ecological selection and tuned by a single **novelty-temperature** dial (Earth-like → alien → weird). Covers origin of life (with biosphere sterilization and serial re-emergence), the guild/niche/richness model, carrying-capacity population dynamics, punctuated speciation under all pressures (with neutral drift in isolated populations), selective mass extinction and recovery, lazy generation and the branch-aware determinism contract, full Linnaean taxonomy, marine and freshwater parity (endemic lake radiations), the domestication stateful exception, emergent sapience, eight cross-layer feedbacks, biology-facing interventions (the meteor/delight-test path), the event schema, determinism, performance, modding, and validation.
- v0.2 (July 2026): Expanded §8.5 into the **axis-driven creature renderer** — structural axes select a construction grammar (axial-segmented / branching-recursive / aggregate-clustered / modular-radial / unitary-simple) and every axis contributes color-shiftable pixel-art parts + palette; kingdoms are an emergent *look*, never a mode, so novel kingdoms fall out of independent grammar/decoration choice. All creature rendering moved here from the viewing-shell prep (Prep-09) so it ships with evolution; the prep only reserves the illustration slot.

---

## 1. Purpose and Scope

Biology is the first **Layer 1** module (Architecture §2). It sits on the completed physical Layer 0 — tectonics (elevation, bedrock), climate (temperature, precipitation, regime, glaciation), hydrology & soil (water, soil, fertility) — and produces the living world: where life is, what it is, how it is related, how it changes, and how it reshapes the planet beneath it. It feeds civilization (Doc 10), export (the bestiary, Doc 15), and the branch-comparison delight test (Doc 01 §4).

This is the module where the project's central conviction — **emergence over scripting** — is hardest to keep and easiest to fake. A world cannot store its billions of species, yet it must *feel* like it has them, remain deterministic, and run across ~4 billion years at millennia-scale ticks. The whole design is the resolution of that tension.

### 1.1 The Governing Thesis: Simulate Niches, Generate Species

Ecology is a **graph** (species interact — predator/prey, pollination, competition); evolution is a **tree** (a lineage's descendants are a function of that lineage and its environment). You cannot lazily generate a graph in isolation, but you can lazily generate a tree. The module splits the problem along exactly that seam:

- **Simulated forward, deterministically, and cheaply — over *functional guilds*, not species.** A biome fills a *bounded* set of ecological roles (producer, large browser, small grazer, insectivore-analog, mid predator, apex predator, decomposer, pollinator-equivalent, reef-builder, …). The count of roles is bounded and roughly constant over time — it is set by thermodynamics (energy must flow producer → consumer → decomposer on any planet). This is the coupled dynamical system, and it is small.

- **Generated lazily — the taxonomy is a decoration over the simulated ecology.** "What is the apex predator of this boreal forest at 2.1 By?" resolves through a deterministic function of the seed and the simulated record. Named species, their trait sets, morphology, and the fine phylogeny never exist until the user asks, and are **constrained by the ledger**: the generator can only manifest a predator where and when the simulation recorded that guild occupied. Generated creatures can never contradict the simulated ecology, because they are seeded *from* it.

One sentence: **the niche is simulated; the creature is generated.**

Two engines drive it, coupled by selection:

1. **The trait morphospace** (§2) — the "physics of life": the space of possible traits and the paths between them. This is the *genome* layer.
2. **The guild ecology** (§4–§5) — the flow of energy through functional roles per province. This is the *selection* layer. Unfilled energy-rich niches create selective gradients that pull lineages' walks through the morphospace toward the traits that exploit them.

### 1.2 Completeness Charter

Like Doc 08, this document is a **master**: every biological phenomenon the engine will ever simulate must be specified here (fully, or as a v1-simplified model with its refinement listed in §19), or explicitly assigned to another doc in the ownership map (§1.6). Biology is too entangled to bolt features on later — the trait graph, the guild roster, and the ledger schema constrain everything downstream.

### 1.3 Goals

1. **A real, open-ended tree of life** that expands forever without ever being computed all at once — infinite lazily-generated species over a bounded simulated ledger.
2. **Genuine evolution, not archetype assignment** — species arise by a biased walk through a trait morphospace, so a world's life is emergent and path-dependent, and convergent forms arise for real reasons.
3. **Alien by default, tunable to Earth-like or weirder** via one novelty-temperature dial over one shared trait graph.
4. **The full biodiversity gradient** — thousands of species in a rich equatorial hex, dozens on tundra, from a single richness scalar; a hard-earned but reachable *sapient tree* as easily as a familiar grazer.
5. **Honest ecology** — niches with free energy always get filled (by *something*); relationships (pollination, symbiosis) are simulated, never assumed; food webs cascade; extinctions reshape.
6. **Full Linnaean classification** — Kingdom → Phylum → Class → Infraclass → Order → Superfamily → Family → Genus → Species, each rank annotated with its defining traits.
7. **A visualizable bestiary** — every species describable (v1) and eventually illustratable, deterministically, from its trait set.
8. **The living planet feeds back** — biology reshapes atmosphere, climate, rock, soil, and coastline.
9. **Deterministic and performant** — same seed → byte-identical life history; the coupled simulation stays within budget across deep time.

### 1.4 Non-Goals (permanent scope boundaries)

- **No individual-organism simulation.** The atomic simulated unit is a functional lineage's biomass in a province; individuals never exist as state (they may be *narrated* at civilization scale, Doc 10).
- **No genetics below the trait level.** No base pairs, alleles, or Mendelian inheritance; the "genome" is a trait set walked through the morphospace. (A mod could add finer genetics; the engine does not.)
- **No behavioral / individual-agent AI.** Animal behavior is aggregate ecology, not simulated cognition — even for sapients, whose *individual* behavior is Doc 10's concern.
- **No real-time ecosystem physics.** Annual/geological means, not seasonal population cycles at organism resolution (guild biomass may cycle as flavor, §5.4).
- **No hand-authored species or bestiary content.** Every creature is generated; the engine ships trait/guild/rule content, never creatures.

### 1.5 Dependencies

**Reads:**
- Tectonics: `elevation_mean`, `bedrock_type`, `plate_id`, volcanism/flood-basalt events, plate splits/merges (allopatry signal), the marine `fertility` accumulator (Doc 06 §8.4).
- Climate: `temperature_mean`, `temperature_range`, `precipitation`, `climate_regime`, glaciation state, atmospheric composition (O₂/CO₂, Doc 07 §11).
- Hydrology & Soil (Doc 08): `water_level_m`/ocean & lake geometry, `soil_class`, `soil_fertility`, `soil_depth_m`, water access (river/spring/oasis flags), wetland tags, the connected-component labeling used for provinces, condensation timeline (first liquid water).

**Writes:**
- `biome`, `biomass`, and new arrays/registries (§8.6).
- Refines `fertility` → biological enrichment (§11.4).
- Cross-layer feedback fields consumed by climate/hydrology/tectonics (§11), one tick lagged.

**Consumed by:** civilization (Doc 10 — food/carrying capacity, domesticable species, sapient origin), export (Doc 15 — bestiary, tree of life), rendering (Doc 14 — biome maps, creature illustration, tree-of-life view).

### 1.6 Domain Ownership Map

| Phenomenon | Owner | Where |
|---|---|---|
| Trait space, evolution mechanics | **This doc** | §2, §6 |
| Origin of life, microbial era, oxygenation *biology* | **This doc** | §3 |
| Guilds, niches, food webs, biomass | **This doc** | §4, §5 |
| Biomes (assignment/emergence) | **This doc** | §4.6 |
| Species, morphology, bestiary generation | **This doc** | §8 |
| Phylogeny, Linnaean taxonomy | **This doc** | §9 |
| Marine ecosystems | **This doc** (full parity) | §10.1 |
| Mass extinction *biology* (selectivity, recovery) | **This doc** | §7 |
| Extinction *triggers* (impact, volcanism, climate, sea level) | Physical layers, consumed here | §7.2 |
| Atmospheric O₂/CO₂ *chemistry* | Climate | Doc 07 §11, driven by §11 here |
| Soil, water access, wetlands *physical* | Hydrology | Doc 08, consumed here |
| Limestone/carbonate *rock* | Tectonics bedrock, deposited by §11.3 here | Doc 06 |
| Domestication, sapient behavior, agriculture | Civilization | Doc 10, from §10.2/§10.3 handoff |
| Creature illustration *rendering* | Rendering | Doc 14, contract §8.5 here |
| Bestiary/tree export | Export | Doc 15, from §9 here |
| Species/taxon *naming* | Civ/Export | Doc 10/15, keyed on `LineageId`/`SpeciesId` |
| Magic/exotic metabolism | Future mods | via §2 trait content |
| Chaos-mode biology (multi-origin, radical rates) | This doc (parameters) + future chaos doc | §3.1, Doc 01 §9.5.3 |

Any future biology-shaped feature amends **this doc** first.

---

## 2. The Trait Morphospace (the Genome Layer)

The space of all possible traits is a **graph** — a cloud of trait nodes connected by weighted, partly-directed edges. An organism's genome is a **subset** of that graph. Evolution is a **biased walk**: each speciation event, a lineage steps to an adjacent region of the cloud. This one mechanism produces convergence, kingdoms, novel body plans, and the reachable-but-rare sapient tree.

### 2.1 Trait Nodes

```rust
pub struct TraitNode {
    pub id: TraitId,               // namespaced content id, e.g. "core:cellulose_wall"
    pub axis: TraitAxis,           // which functional dimension (see §2.2)
    pub tier: u8,                  // 0 = deep/fundamental … 6 = shallow/leaf (drives taxonomy, §9)
    pub prerequisites: Vec<TraitId>,   // DIRECTED: unreachable until all are present
    pub exclusions: Vec<TraitId>,      // cannot coexist (structural/energetic contradiction)
    pub proximity: Vec<(TraitId, f32)>,// weighted neighbors: co-occurrence / reachability affinity
    pub reversal_cost: f32,            // how hard this trait is to LOSE once gained (directional asymmetry)
    pub base_energy_cost: f32,         // metabolic overhead; payoff is contextual (§2.5), this is the debit
    pub tags: Vec<TraitTag>,           // consumed by the rule engine and by morphology/description generation
}

pub struct TraitId(pub u32);   // index into the loaded trait registry (content, not WorldData)
```

`TraitId` follows the content-id pattern (like `BiomeId`): save files reference traits by namespaced string (`"core:image_eye"`) for stability across mod versions; the runtime uses the dense index.

### 2.2 Trait Axes and Tiers

Traits are organized on **axes** (functional dimensions) and **tiers** (evolutionary depth). Tier governs taxonomic rank (§9); axis governs which functional role a trait contributes to. The core content pack ships approximately these (moddable, extensible):

| Axis | Example traits | Typical tier |
|---|---|---|
| **Metabolism** | photosynthesis-analog, chemosynthesis, heterotrophy, absorptive-decomposition, mixotrophy | 0 |
| **Organization** | unicellular, colonial, multicellular, differentiated-tissue | 0 |
| **Structure** | cellulose-analog wall, chitin-analog, mineral endoskeleton, mineral exoskeleton, hydrostatic, silica frustule | 1 |
| **Symmetry** | radial, bilateral, spherical, asymmetric, segmented | 1 |
| **Motility** | sessile, ciliary, muscular crawl, limbed walk, swim, jet, powered flight | 2 |
| **Thermoregulation** | poikilotherm, homeotherm-analog | 2 |
| **Nervous / cognition** | none, nerve-net, ganglia, centralized brain, complex cognition, **sapience** | 2→5 |
| **Sensory** | chemoreception, photoreception, image-forming eye, electroreception, echolocation, magnetoreception | 3 |
| **Reproduction** | binary fission, spores, seed-analog, external eggs, internal gestation, self-mediated vs animal-mediated dispersal | 3 |
| **Integument** | naked, cuticle, scale-analog, feather-analog, fur-analog, bark-analog, shell | 4 |
| **Diet specialization** | generalist, folivore, granivore, hypercarnivore, filter-feeder, detritivore, parasite | 4 |
| **Social structure** | solitary, herd/shoal, eusocial, pair-bond | 5 |
| **Size class** | micro … megafauna (a scalar axis, quantized) | 5 |
| **Coloration / display** | (procedural, biome-influenced) | 6 |

**Tiers are the load-bearing idea for taxonomy:** a branch that changes a tier-0 trait is a *Kingdom* split; a tier-1 change is a *Phylum*; and so on down to tier-6 (Species). §9 makes this precise.

### 2.3 Edges: Prerequisites, Proximity, Exclusion, Asymmetry

Four edge kinds encode the shape of the possible:

- **Prerequisite (directed, hard):** `centralized_brain` requires `ganglia` requires `nerve_net` requires `multicellular`. A lineage cannot acquire a trait until every prerequisite is present. This is what makes "certain traits come first."
- **Proximity (weighted, soft):** how *near* two traits are in the cloud — the edge weights in the owner's mental picture. High proximity ⇒ the pair co-occurs readily and one is a cheap step from the other; low proximity ⇒ possible but rare. `nerve_net`↔`muscle` is close; `cellulose_wall`↔`heterotrophy` is far.
- **Exclusion (hard):** contradictions that cannot coexist (`sessile` ✗ `powered_flight`; `giant_size` ✗ `powered_flight` without a strong-structure trait present). Exclusions may be *conditional* (liftable by a third trait).
- **Reversal asymmetry (directional):** a trait's `reversal_cost` makes some steps near-one-way. A cellulose-walled lineage can walk *forward* to nerves, motility, limbs, cognition — a **sapient tree** — but an already-chitinous animal-analog will essentially never walk *back* to cellulose walls. The graph is a landscape with slopes, not a symmetric mesh.

**Consequently, any trait can in principle combine with any other, but the *path* is everything.** A heterotroph-with-cellulose-walls is not forbidden — it is simply almost never *reached*, because no short, selectively-favored path leads there. An autotroph that acquires nerves and muscle is reachable from the plant cluster and, if a world walks it, founds a kingdom no Earth has.

### 2.4 The Biased Walk (a speciation step)

At each speciation event (§6), the descending lineage takes one step. The candidate steps are the trait-graph neighbors of the current genome reachable given prerequisites and exclusions. Each candidate is scored:

```
weight(step) =  proximity_weight(step)                     // graph structure
              × prerequisite_gate(step)                    // 0 if unmet, else 1
              × selective_payoff(step, environment, guilds) // the ecological pull (§2.5)
              × novelty_factor(step)                        // the alien dial (§2.6)
              × directional_factor(step)                   // reversal asymmetry
```

The step is drawn from the normalized weights using a stream seeded by `(effective_seed, "biology.evolution", lineage_id, branch_index)` — deterministic and reproducible. A lineage may also take a **null step** (stasis) — punctuated equilibrium: most lineages hold their genome for long stretches and step only when a trigger and a favorable gradient coincide.

### 2.5 Selective Payoff: Where Ecology Pulls

`selective_payoff` is the coupling to the guild layer (§4–§5). A step is favored when it moves the lineage toward an **unfilled or under-exploited niche with available energy**, and disfavored when it worsens the organism's fit or duplicates a saturated niche. Concretely, payoff rewards steps that:

- unlock or improve access to an energy source the lineage's province offers (a producer step where light/nutrients are abundant; a herbivore step where producer biomass is un-grazed);
- fill a **guaranteed** niche (free energy present — §4.3) — these exert the strongest pull;
- escape competition (a divergence step into an adjacent under-occupied niche);
- suit the local physical environment (thermoregulation in a cold province, water-retention traits in an arid one).

This is the mechanism behind the owner's empty-niche insight: an empty niche with free energy is a selective vacuum, so nearby lineages with tangentially-useful traits are pulled to fill it — and because *many* lineages feel the pull, *many* walk in, and the generator manifests the multiplicity (§8.4). Payoff is **contextual and never stored**: it is recomputed from the current guild/environment state at each event, so the same trait is advantageous in one province and useless in another.

### 2.6 The Novelty Dial (Earth-like ↔ alien ↔ weird)

One parameter tunes how strongly improbable trait-combinations are penalized — a "temperature" on the walk:

```rust
pub novelty_temperature: f32,   // 0.0 = Earth-like clustering … 1.0 = alien (default) … 2.0 = weird
```

`novelty_factor(step) = proximity_weight(step) ^ (1 − clamp(novelty_temperature, 0, ~2))` (schematic): at **low** temperature, low-proximity steps are heavily suppressed and life collapses into familiar dense clusters (recognizable plants/animals/fungi); at **medium** (default), the cloud is explored more freely (convergent-but-different — purple photosynthesizers, hexapod browsers); at **high**, even far steps are reachable (motile forests, nerve-bearing autotrophs). One shared graph, one knob — this is the whole of the owner's "default alien, settable to Earth-like or weirder." The *laws* of life (the graph) are universal across all worlds; only exploration temperature changes, which keeps convergent evolution meaningful.

### 2.7 Emergent Kingdoms and Convergence

- **Kingdoms are dense clusters** the walk settles into: the Plant-analog basin (`photosynthesis + cellulose + sessile + multicellular`), the Animal-analog basin (`heterotrophy + motile + nerve + muscle`), the Fungus-analog basin (`absorptive-decomposition + chitin`). "Plant/Animal/Fungus" are **not engine primitives** — they are one *assembly* of universal parts that a low-temperature (Earth-like) walk tends to produce. A different world settles into different basins; §9 discovers and names them from the tree topology.
- **Convergence is free and correct:** two unrelated lineages under the same niche pressure are pulled toward the same trait region (streamlining, image eyes, fangs) from different starting points, arriving *analogous* not *homologous* — which the taxonomy honors (same Class-level traits, different Phylum). This is why a first look at a world feels alive: the same solutions recur for the same physical reasons, without being scripted.

### 2.8 The Graph Is Moddable Content (Doc 11 Rule Engine)

The trait morphospace ships as **core content**, authored by the engine team, and is a first-class consumer of the Doc 11 rule engine — biology and technology share one data-driven evaluator. A trait is a data record (§2.1); guild membership, innovation thresholds, and step legality are **rules** evaluated against a lineage's trait set. Consequences:

- The core graph is hand-authored (hundreds of nodes — see §2.9); presets (`earth_like`, `alien`, `weird`) are graph-weighting + novelty-temperature bundles, day-one moddable.
- Mods extend the cloud: new metabolisms (a magic-based **manasynthesis** producer strategy for a far-future fantasy pack), new structural chemistries (silicon), new sensory modes — all drop into the same walk.
- Determinism validation (Doc 12) forbids non-deterministic rule operations, as elsewhere.

### 2.9 Right-Sizing: Hundreds of Nodes, Infinite Life

The vast diversity is **combinatorial**, not node-count. A few **hundred** trait nodes yield effectively unbounded viable genomes. Millions of nodes would be unbuildable and unnecessary; millions of *organisms* come from the combinations plus continuous axes (size, coloration). The authored core graph targets ~200–400 nodes across the axes of §2.2. This keeps the "physics of life" a small, static, cache-resident structure loaded once.

---

## 3. Origin of Life and the Microbial Era

### 3.1 Biogenesis (probabilistic, not a fixed year)

Life does not appear at a hardcoded year. Once liquid water exists (hydrology condensation, Doc 08 §3.3) and suitable chemistry is present (hydrothermal-vent hexes: deep ocean adjacent to volcanism/hotspot activity), **biogenesis probability accrues per suitable hex per tick**, scaled by parameters. The first success plants the root of the tree of life at that vent hex and that year; probability then collapses toward zero (single origin), unless the multiple-origins toggle is set.

```rust
// Extends Doc 04 §4.10 BiologyParameters:
pub biogenesis_rate_scale: f32,   // 1.0 = Earth-like tempo. "2× / 0.5× faster" dial.
pub multiple_origins: bool,       // default false; chaos: independent trees of life
pub novelty_temperature: f32,     // §2.6
pub complexity_pressure: f32,     // §3.3, default 1.0
pub sapience_enabled: bool,       // default true
pub humanoid_sapients: bool,      // default false; constrains sapient morphology (§10.3)
```

`life_emergence_year` (Doc 04 §4.10) is **reinterpreted**: no longer a hard switch but the **seed target / expected value** for the ramp, and the *realized* emergence year is an **output** (like sapience). The setup menu presents this as a rate with an estimate — *"life will probably emerge between ~450–800 My at these settings"* — per the owner's direction and Doc 01 §9.5.2's anticipation.

**Re-arming after sterilization.** The probability collapses to near-zero *while life exists*. If the biosphere is ever totally extinguished (§7.5) — a big enough impact, or a `SterilizeRegion` intervention that catches the last refuge — biogenesis re-arms and a fresh tree of life may originate later, unrelated to the first. A world can therefore have serial, independent biospheres across deep time, each a clean causal restart.

### 3.2 Determinism of the Ramp

Per suitable hex per tick, a draw from `stream("biology.biogenesis", hex, tick)` against the accrued probability. Suitable hexes are enumerated in ascending `HexId`; the first success in that deterministic order is the origin. Same seed → same origin hex and year.

### 3.3 The Microbial Era (low-resolution, innovation-gated)

The ~billions of years of single-celled life are **not** simulated at species resolution — that would be dead time and unbounded. Instead the early biosphere advances as a small set of **key-innovation unlocks** reached by the first lineages' walks (§2) through the deep tiers of the trait graph:

1. Prokaryote-analog metabolism (the root)
2. Metabolic diversification (chemotrophy, anoxygenic phototrophy)
3. **Oxygenic photosynthesis** → the oxygenation feedback (§11.1)
4. **Eukaryogenesis** (differentiated-cell organization) — the gateway to complexity
5. **Multicellularity** — the gateway to the macroscopic tree

Each unlock is a rule-engine threshold (Doc 11) with probabilistic, precondition-gated timing. Crucially, **eukaryote and multicellularity timing is emergent** — a world may reach complex life in 200 My or stall for 3 By, which is exactly the "is Earth-life a fluke?" experiment the owner wants observable. `complexity_pressure` is the honest thumb on the scale: at 1.0 the walk toward complexity is unbiased (let the chips fall); above 1.0 the payoff for complexity-enabling steps is boosted so worlds that would otherwise stall get a push. A world that never unlocks multicellularity remains a living-but-microbial planet — a valid, and interesting, outcome.

### 3.4 Marine First

Life begins in the ocean and stays marine until land-colonization traits (desiccation resistance, structural support against gravity, aerial gas exchange) are walked — another innovation threshold, gated on multicellularity and typically on an oxygen-rich atmosphere. Land colonization opens the terrestrial provinces to occupation and triggers a major radiation (and the biotic-weathering feedback, §11.6).

---

## 4. Guilds, Niches, Richness, and Biomes

### 4.1 Guilds (the simulated unit)

A **guild** is a functional ecological role — a way of making a living. The guild roster is **bounded and moddable content**, keyed by realm (marine/terrestrial/freshwater) and refined by biome. Representative terrestrial guilds: primary producer (canopy / ground-cover / nitrogen-fixer-analog), large browser, large grazer, small herbivore, granivore, insectivore-analog, pollinator-equivalent, seed-disperser, mid predator, apex predator, scavenger, decomposer, parasite. Marine guilds: §10.1.

```rust
pub struct GuildId(pub u16);   // content id; NONE = u16::MAX
```

A guild is *occupied* in a province when ≥1 lineage there has a trait set the rule engine accepts as filling that role. Occupancy, biomass, and the guild food-web are the state the simulation actually advances (§5).

### 4.2 Niches

A **niche** is a guild slot in a specific province with a specific available-energy budget. Niches are where selection acts: an unfilled niche with energy is a gradient in the morphospace (§2.5).

### 4.3 Guaranteed vs Contingent Niches

Two tiers, per the owner's empty-niche insight:

- **Guaranteed niches** — where energy is *physically present*, something *will* fill it, always, on a short timescale, because thermodynamics will not leave free energy on the table. Dead biomass ⇒ a decomposer is guaranteed. Sunlit wet substrate ⇒ a producer is guaranteed. These fill regardless of contingency (the nearest adaptable lineage radiates in).
- **Contingent niches** — which open only *after* a prerequisite exists, and fill as a **cascade**: abundant producer biomass opens the large-herbivore niche; large herbivores open the apex-predator niche; and once herbivores boom, predators are pulled in to check them (the food-web graph resolving — §5.3). A specific *solution* (e.g. animal-mediated pollination) may or may not evolve, but the *role* (get pollinated somehow, or reproduce another way) is filled by whatever is adjacent. **We simulate whether relationships form; we generate whichever creature fills them** — never assumed, never cheating.

### 4.4 Richness (the diversity scalar)

A per-hex scalar `biotic_richness` (call it `R`), derived like `habitability` from fields already computed:

```
R = f( primary_productivity,      // energy available: climate + soil + water
       climatic_stability,        // low temperature_range & interannual variance breed specialists
       province_area_connectivity,// larger, better-connected biomes hold more
       age_since_disturbance )    // undisturbed provinces accumulate diversity; glaciated ground resets
```

Latitude is *not* hardcoded — the poles come out species-poor because they are low-productivity, high-variance, recently-glaciated, and small-area; the equator comes out species-rich for the opposite reasons. `R` drives **two** things:

1. **Guild occupancy count** — how many roles are filled (few at the poles, ~all at the equator).
2. **Species multiplicity per guild** — how finely each niche subdivides (a polar large-herbivore guild = a couple of species; a tropical insectivore guild = thousands).

So an equatorial continental hex = ~40 occupied guilds × up to hundreds of species each = **thousands of species**, stored as one scalar plus a short occupied-guild list. The species themselves are generated on demand (§8). The latitudinal diversity gradient is emergent from a field the engine already computes.

### 4.5 The Saturation Cap (competitive exclusion)

Species-per-guild is a **saturating** function of richness, not linear and never unbounded:

```
species_in_guild ≈ S_max_guild × (1 − exp(−R / k_guild))
```

It climbs fast, then flattens — even the richest rainforest hex tops out at a large-but-finite count. This is not merely a safety valve: it is **competitive exclusion** — a habitat can only pack so many species before niches collide and further subdivision fails. "Too much pressure" *is* the cap, and it is ecologically real. There is no hard engine cap; the ceiling is a richness-driven number, materialized lazily (§8.4).

### 4.6 Biomes (emergent, not merely classified)

A biome is the **climate regime × the dominant producer strategy actually present** — emergent, in the spirit of Doc 08's "sea level is an output." A world whose walk never produced tall woody producers has no forest biome, only its climate-equivalent grassland/scrub; a world with motile-photosynthesizer producers has a biome Earth's Köppen map has no name for. Biomes:

- are written to the existing `biome: Vec<BiomeId>` (content-driven ids; the *set* of biomes a world has is partly emergent from which producer clusters exist);
- **migrate with climate** — belts walk poleward under warming, equatorward under cooling — and may **lag** fast climate change, producing stressed mismatch zones with elevated extinction pressure (§7.1);
- have **ecotones** (gradient transition bands) rather than hard edges, derived from the underlying climate/soil gradients.

Biome definitions (climate/soil envelopes, producer requirements) are moddable content, consistent with Architecture §10.

---

## 5. Population Dynamics

### 5.1 Biogeographic Provinces (the simulation granularity)

The coupled dynamics do **not** run per-hex-per-tick — too expensive and unnecessary. They run per **biogeographic province**: a connected region of similar biome and connectivity, labeled by the same connected-component machinery hydrology uses for ocean basins and water bodies (Doc 08). There are a few hundred provinces at most, versus tens of thousands of hexes. Per-hex biomass/biome fields are **derived** from province state for rendering and for civ/food consumers; the *dynamics and speciation* live at province resolution.

```rust
pub struct ProvinceId(pub u16);   // NONE = u16::MAX
pub struct BiogeographicProvince {
    pub id: ProvinceId,
    pub hexes: Vec<HexId>,             // membership (also stored per-hex as province_id)
    pub realm: Realm,                  // Marine | Terrestrial | Freshwater
    pub guilds: BTreeMap<GuildId, GuildState>,  // occupancy, biomass, resident lineages
    pub food_web: FoodWeb,             // graph over occupied guilds
    pub richness: f32,                 // area-aggregated R
    pub neighbors: BTreeSet<ProvinceId>, // dispersal connectivity (barriers gate this)
}
```

Provinces are **dynamic**: continents split, climate shifts, so provinces merge, split, appear, and vanish over time. Province identity is tracked deterministically (lowest member `HexId`, like water bodies) so lineage bookkeeping and events remain stable across relabeling.

### 5.2 Carrying-Capacity Relaxation (not literal Lotka-Volterra)

Literal Lotka-Volterra integration is chaotic, stiff, FP-divergent, and would wreck determinism over billions of years. Instead, each occupied guild's biomass **relaxes toward an equilibrium** each tick:

```
equilibrium(guild) = energy_available_from_below            // producer output, or prey biomass
                   × trophic_efficiency                     // ~10% per level
                   − predation_pressure_from_above
                   capped by environmental_carrying_capacity(province, guild)

biomass(guild) += (equilibrium − biomass(guild)) × relaxation_rate   // bounded step per tick
```

Stable, bounded, deterministic, and **O(guilds × provinces)** — cheap. Producers anchor the base from `primary_productivity` (climate + soil + water + CO₂); each higher tier draws from the tier(s) below via the food web. This trades literal population oscillation for stability — the right trade at geological cadence.

### 5.3 The Guild Food-Web

Per province, a sparse directed graph over occupied guilds (~20–40 nodes) encodes who-eats-whom (and other couplings: pollination, seed dispersal, parasitism, symbiosis). It is a **guild** graph, not a species graph, so it is bounded and cheap, yet **trophic cascades are emergent**: remove the apex guild (a size-selective extinction, §7.3) and mid-predators boom, over-grazing herbivores crash, producers rebound — the graph resolving. Couplings are established/broken by the same niche logic (§4.3): a coupling exists when both endpoints are occupied and a selective path to the relationship was walked.

### 5.4 Optional Cycles

Predator–prey *cycles* (boom-bust) can be surfaced as **flavor** via a lag/overshoot term on the relaxation, off the critical path and off by default. They matter only at Recent-era fine ticks (Doc 04 §7); at geological cadence the equilibrium is the honest quantity.

### 5.5 Evolutionary Tempo vs. Tick Cadence

Evolution is measured in **generations**, not years, and generations elapse faster in warm, productive, short-lived-organism provinces. The number of speciation *opportunities* a lineage gets per tick is `generations_elapsed(province, dt) × per-generation speciation propensity` — so a 500 My coarse tick still yields the correct *number* of branch events (recorded in the ledger), independent of tick length. **Tick rate never changes what is true** (the ledger is a function of seed + elapsed generations, not of how the time was chopped); it only limits what the user can *watch happen* (§8.2). The microbial→complex transition is therefore not artificially instantaneous just because early ticks are coarse.

---

## 6. Speciation

### 6.1 Punctuated Equilibrium

Lineages hold their genome for long stretches (null steps, §2.4) and speciate in bursts at **triggers**. This is both scientifically apt and performant: the tree grows in discrete pops (§8.2), and the ledger stores events, not per-tick states.

### 6.2 Speciation Triggers (all of them — there is no v1 subset)

Per the owner's direction, the full roster, each with a detection hook into the physical layers:

1. **Allopatry (geographic isolation).** A continent split (tectonic plate split, Doc 06), a new strait/sea (hydrology, Doc 08), a rising mountain range or a spreading desert (climate/elevation barrier) severs a province's connectivity. The two sundered populations become independent lineages and walk apart under their now-separate environments. This is the adaptive radiation Architecture §12 names, and it leans entirely on tectonic/hydrologic geography.
2. **Niche divergence (sympatric/parapatric).** A lineage under disruptive selection splits to exploit two adjacent niches (a generalist splitting into specialist grazer + specialist browser).
3. **Adaptive radiation (vacuum-filling).** A newly-opened set of niches — post-extinction empty guilds (§7.4), a newly-colonized realm (land, a new island province) — pulls a burst of branching from the nearest lineages, each walking toward a different empty role.
4. **Competitive exclusion.** When two lineages collide in one niche, the loser must **adapt** (walk to an adjacent niche → a speciation), **migrate** (disperse to another province), or **die** (extinction). All three outcomes are possible and drawn by seed + fitness.

### 6.3 The Three Responses to Environmental Change

When climate/geography shifts under a lineage, it may **migrate, adapt, or die** — and, richly, more than one at once: a population fragment migrates and founds a new lineage while the parent adapts in place and a stressed remnant goes extinct. A splitting continent creates two populations with divergent pressures. The possibilities are combinatorial — this section is, as the owner put it, the largest behavioral surface of the whole simulation, and it is entirely emergent from (trigger → biased walk → ledger event).

### 6.4 Dispersal, Barriers, and Endemism

Lineages spread along province connectivity (`neighbors`, gated by barriers — oceans for non-marine, mountains, climate walls). Isolated provinces (islands, post-rift microcontinents, climatically-marooned refugia) radiate **endemically** — a signature the delight test loves (the Australia / Madagascar / Galápagos pattern), and a direct product of tectonic + hydrologic geography.

**Drift, not only selection.** In small, isolated populations the biased walk (§2.4) is less dominated by `selective_payoff` and more by **neutral drift** — near-neutral trait steps fix by chance (founder effects, genetic bottlenecks). Mechanically, the payoff term's weight is scaled down for low-population/low-connectivity lineages, so isolated endemics wander into idiosyncratic corners of the morphospace that mainland selection would never have favored. This is *why* island life is weird, and it is the difference between a world of relentless optimization and one that feels genuinely historical.

### 6.5 Determinism

Every speciation is `(trigger detected from ledger/physical state) → biased walk step (§2.4, seeded) → recorded ledger event`. No floating-point divergence risk in the *branching topology* because steps are discrete weighted draws; continuous quantities (biomass) use the bounded relaxation (§5.2) and fixed-order summation.

---

## 7. Extinction and Recovery

### 7.1 Background Extinction

A baseline per-lineage hazard each tick, scaled by **vulnerability**: narrow range, high specialization, small population, and biome-mismatch (a lineage stranded by a migrating biome, §4.6) all raise it. Deterministic draw per lineage in `HexId`/`LineageId` order.

### 7.2 Mass Extinctions (full trigger roster, Earth-consistent odds, menu-tunable)

Triggers come from the physical layers and from biology itself:

- **Bolide impact** — the meteor **intervention** (the delight-test centerpiece: rewind, place a meteor, compare timelines). Severity scales with impactor size and location (ocean vs continent).
- **Flood-basalt volcanism** — large igneous provinces from tectonics (Doc 06 volcanism); the Deccan/Siberian-Traps analog.
- **Rapid climate / sea-level shock** — fast glaciation or hothouse swings, large sea-level excursions (Doc 07/08); drowns shelves or exposes them, collapses biomes faster than they migrate.
- **Ocean anoxia** — warm, stratified, low-oxygen oceans (climate + circulation) crash marine guilds.
- **Biological self-poisoning** — an innovation that wrecks the incumbent world, e.g. the **Great Oxygenation** itself (oxygenic photosynthesis poisoning the anaerobic biosphere), or a runaway producer.

Odds and severities default to Earth-consistent bands and are all menu-tunable (`extinction_scale` and per-trigger scales).

### 7.3 Selectivity

Extinctions are **selective**, not uniform — biased by body size (large-bodied, low-population lineages die first), trophic level (apex guilds are fragile), geographic range (endemics die; cosmopolitans survive), and specialization (specialists die; generalists survive). Selectivity is what makes each extinction reshape the world *differently*, so recovery radiates along a new path — the raw material of divergent branches.

### 7.4 Recovery and Adaptive Radiation

After a mass extinction, vacated guilds are energy-rich empty niches (§4.3) exerting enormous pull. Surviving stock radiates to refill them (§6.2 trigger 3), often from unexpected lineages (the "mammals inherit the post-dinosaur world" pattern) — because selectivity culled the former incumbents and left a different adjacent-possible. Recovery time and the identity of the new dominants are emergent and are precisely where a branched timeline diverges from its parent.

### 7.5 Total Sterilization

A severe enough shock — a very large impactor, a runaway climate catastrophe, or a `SterilizeRegion` intervention that reaches the last refuge — can extinguish **all** life. This is rare and hard to reach (deep-ocean vent and endolithic communities are extinction-resistant, so extreme events usually leave a microbial remnant that re-radiates), but it is possible and permitted. On total extinction, biology emits `BiosphereSterilized`, the ledger closes the current tree of life, and biogenesis re-arms (§3.1): the world may originate a **new, unrelated** tree later. Serial independent biospheres are a valid — and dramatic — deep-time outcome.

---

## 8. The Ecological Ledger and Lazy Generation

### 8.1 The Ledger (the simulated, stored truth)

The simulation's persistent output is a compact **ecological ledger**, snapshot-persisted and branch-scoped:

- **Per-province time series:** occupied guilds, biomass per guild, richness, dominant lineages — coarsely sampled (era-dependent stride).
- **Event log:** every speciation, extinction, innovation unlock, radiation, dispersal, and kingdom-founding, each tagged with `LineageId`, province, year, and the **trait delta** (which trait changed). This is the tree of life's trunk and major branches — real, causal history tied to real geographic/climatic triggers.

The ledger is bounded: it scales with lineages-and-events, not with generated species. It is the single source of truth the generator reads.

```rust
pub struct LineageId(pub u64);   // stable across save/load; assigned at branch events

pub struct LineageRecord {
    pub id: LineageId,
    pub parent: Option<LineageId>,
    pub origin_year: WorldYear,
    pub extinction_year: Option<WorldYear>,
    pub trait_set: TraitSet,          // the genome at this node (subset of the morphospace)
    pub trait_delta: TraitId,         // what changed vs. parent (drives taxonomic rank, §9)
    pub guild: GuildId,               // functional role
    pub origin_province: ProvinceId,
    pub range: BTreeSet<ProvinceId>,  // where it lives over time (coarsely)
}
```

### 8.2 Two-Speed Interaction (why you cannot watch at 500 My/tick)

There are two activities at different resolutions, and this resolves the owner's time-scale concern:

- **Deep-time simulation (500 My → 1 ky ticks):** advances the ledger; the user watching a fast-forward sees only **aggregate fields** change — biome belts migrating, a biomass/diversity heatmap brightening and dimming, mass-extinction dips. No individual species are rendered.
- **Inspection (paused or slowed):** the species / tree-of-life / food-web views are a **lens on the ledger**, generated on demand — the tree is *already at its current state* whenever opened, reconstructed from the ledger. Because evolution is punctuated (§6.1), the tree **grows in discrete steps**: branches pop into existence at their speciation events and grey out at their extinction events (walk forward → the tree expands; scrub back → branches vanish). Even at a 1 ky tick you are not watching millions of species appear — you are watching occasional branch-events fire. So: fast-forward for history, pause-and-open for life; the millions are never rendered, only the field, with specifics generated on click.

### 8.3 Lazy Generation (the branch-aware determinism contract)

A named species is a **pure function** of `(branch-aware seed, ledger slice, query)` and is **never stored until asked**. The seed **must** incorporate the branch and the simulated ledger up to the queried time, not merely base seed + coordinates:

```
species_seed = hash( effective_seed, branch_id, ledger_state_hash(≤ query_year), lineage_id, query )
```

This is non-negotiable for the delight test: a creature must be **byte-identical on branches A and B before their divergence point** and **correctly different after** (because the ledger diverged). Get this wrong and side-by-side branch comparison breaks. Same ledger → same creatures, forever.

### 8.4 What the Generator Produces

- **Coherent local assemblages, not species in a vacuum.** Opening a hex generates its **whole species set in one seeded pass**, constrained by the province's guild food-web — so the apex predator it manifests actually preys on the herbivore it manifests, and both are consistent with the ledger's recorded ecology.
- **Hierarchical drill-down.** A rich guild's thousands of species are generated **as the user descends** (Family → Genus → Species, in batches while scrolling), never all at once. The species *count* is a generated integer from richness (§4.4) under the saturation cap (§4.5); only materialization is lazy.
- **Recursive inheritance (descent with modification).** A species' trait set is `parent lineage trait set + seeded mutation + local-environment nudge`, resolved **recursively along the phylogeny** — so a clade *resembles* its ancestors and siblings, and the tree feels like real descent rather than independent noise.

### 8.5 Morphology and Description (the bestiary)

The **trait set is the single source of truth**; text and illustration are both renderings of it (the same pattern as sea level → water rendering in Doc 08).

- **v1 — text.** A deterministic description generated from the trait list (metabolism, structure, symmetry, size, integument, diet, sensory, social) — cheap, and the bestiary needs it regardless.
- **Illustration — the axis-driven creature renderer.** A deterministic 2-D **pixel-art** generator turns the trait vector into an image. Its governing rule is §2.7's: **there is no "plant"/"animal"/"fungus" mode** — kingdoms are emergent clusters, so the renderer is organized by the trait *axes*, never by kingdom, and a kingdom's look emerges the way the kingdom itself does. Two independent decisions:
  1. **The structural axes (organization + structure + symmetry + motility) select a construction *grammar*** — *how* the body is built — from a small, moddable set: **axial-segmented** (a central axis with paired/anchored appendages — bilaterally symmetric animals), **branching-recursive** (an L-system trunk with modular elements at the tips — woody producers, fronds), **aggregate-clustered** (repeated sub-bodies — fungal fruiting bodies, colonies, reef modules), **modular-radial** (elements repeated around a center under radial symmetry), and **unitary-simple** (a single symmetry-driven cell/blob — microbes, amorphous). The grammar is the body-plan skeleton-builder; it exposes the slots the next step fills. A derived `GrowthForm` on the trait set names the chosen grammar.
  2. **Every axis then contributes pixel-art parts and a palette shift** into the grammar's slots: metabolism drives pigment (photosynthetic green/purple vs animal integument); motility attaches locomotor appendages; sensory drops eyes/antennae onto sensory sub-anchors; integument tiles a surface texture; size scales the whole. Parts are **color-shiftable** pixel sprites (grayscale-keyed, tinted at composite — the Architecture §15 sprite principle), so one sprite serves many palettes, and mods (§16) extend the cloud by dropping sprites into the same slots.

  Because grammar and decoration are chosen **independently** from the trait vector, **novel kingdoms fall out for free**: the Appendix-A motile autotroph uses the *branching* grammar (its body is a photosynthetic producer) **and** gets legs and an eye (its motility and sensory axes say so) — a walking, seeing tree, with no "walking-tree mode" anywhere. Convergence is honored the same way — convergent *function* yields convergent *appearance* (green + branching + sessile reads as "producer" regardless of ancestry), for the real reasons of §2.7. Seeded per `SpeciesId`, deterministic, byte-identical every time and identical across branches before divergence (§8.3). **This is a consumer of Doc 04 §3.7's `SubHexTerrainGenerator`-style lazy-detail hook and is implemented with the rendering phase (Doc 14);** the contract (trait vector → grammar → parts → deterministic 2-D pixel creature) is fixed here. The viewing-shell prep (Prep-09) deliberately owns **none** of this — it reserves an illustration slot on every species card and tree node that this renderer fills, so creature art ships with evolution, not with the UI chrome.
- **Humanoid toggle** (`humanoid_sapients`, §10.3) constrains the generator for sapient species specifically (upright, bilateral, four-limbed — the axial-segmented grammar with a fixed slot map) when set — the owner's Star-Trek dial.

### 8.6 New Data Schema

Per-hex bulk arrays (derived, cheap, snapshot-persisted):

```rust
pub biome: Vec<BiomeId>,          // exists; now emergent (§4.6)
pub biomass: Vec<f32>,            // exists; total living biomass, derived from province state
pub biotic_richness: Vec<f32>,    // R (§4.4) — drives generated species count
pub primary_productivity: Vec<f32>,// energy base (climate+soil+water+CO2)
pub province_id: Vec<ProvinceId>, // province membership
pub dominant_lineage: Vec<LineageId>, // headline lineage for quick rendering/labels
```

Sparse / registry state (in `WorldData`, snapshot-persisted, branch-scoped): the province registry (§5.1), the lineage ledger (§8.1), the event log, and the derived taxonomy index (§9). The **trait graph is static content**, loaded once, not in `WorldData`.

`SpeciesId` (Doc 04 §5.3) denotes a **generated** species; it is minted deterministically from `(lineage, query)` and is stable for a given ledger, but species are not stored in `WorldData` — they are regenerated on demand and cached transiently (except the domestication exception, §10.2).

---

## 9. Phylogeny and Linnaean Taxonomy

### 9.1 The Tree

The phylogeny is the lineage forest of §8.1: **simulated** down to the *functional lineage within a province* (real branch events tied to real triggers — the causal chain the project sells), and **generated** below that (the genera and species within a lineage, materialized on demand by recursive inheritance, §8.4). The join is seamless because the generated fine tree hangs off real ledger branch points.

### 9.2 Ranks from Trait Tiers (the elegant part)

Full Linnaean classification falls out of the trait tiers (§2.2). **A branch's taxonomic rank is the tier of the trait that distinguishes it:**

| Rank | Distinguishing trait tier | Example trait change |
|---|---|---|
| Kingdom | 0 — Metabolism / Organization | photosynthesis-analog vs heterotrophy |
| Phylum | 1 — Structure / Symmetry | endoskeleton vs exoskeleton; bilateral vs radial |
| Class | 2a — Motility / Thermoregulation | limbed-walk; homeothermy |
| Infraclass | 2b — Nervous major grade | centralized brain |
| Order | 3 — Sensory / Reproduction | image-eye; live birth |
| Superfamily / Family | 4 — Integument / Diet | fur-analog; hypercarnivory |
| Genus | 5 — Social / Size | herd-living; megafaunal size |
| Species | 6 — Local adaptation / coloration | (leaf) |

Because every ledger branch records the trait that caused it, the entire Kingdom→Species ladder is **derivable from the tree topology**, and the species overview annotates each rank with its **defining trait** — exactly the owner's request. Deep ranks are real simulated history; shallow ranks are generated. Rank assignment is a deterministic post-process over the ledger.

### 9.3 The Tree-of-Life View

- Trunk and major branches from the ledger; fine branches lazily expanded on click ("what did this evolve into?" → recursive generation, §8.4).
- **Time-aware:** a species extinct at the currently-viewed year is greyed/faded and not inspectable unless the user jumps to a year it was alive; species not yet evolved are absent. Walking forward in time grows the tree; scrubbing back prunes it. (The owner's exact specification.)
- Each node links to its generated bestiary entry (§8.5) and its trait set with the defining traits per rank highlighted.

### 9.4 Kingdom Discovery

"Kingdoms" are not predeclared; they are the **deepest clusters** in the realized tree (tier-0 trait basins, §2.7). A world's kingdom set is discovered from its phylogeny — usually a familiar few at low novelty temperature, sometimes a novel one (the motile-autotroph kingdom) at higher temperature. Kingdoms are named (Doc 10/15) and rendered as the tree's primary divisions.

---

## 10. Marine Life, Domestication, and Sapience

### 10.1 Marine Parity

Marine biology uses the **same machinery** at full fidelity — most biomass and the origin of life are marine; there is no reduced ocean model. Marine realm guilds: phytoplankton-analog (the planet's primary producer base), zooplankton, filter-feeder, benthic grazer, nekton predator, apex marine predator, **reef-builder**, decomposer/detritivore, chemosynthetic vent community. Provinces span ocean basins and shelves (from hydrology geometry). Two couplings of note:

- **Reef-builders close the carbonate loop:** sessile mineral-skeleton marine producers/filter-feeders deposit carbonate, driving `BedrockType` toward `Limestone` — the biological limestone Doc 06 §8.4 explicitly deferred to Phase 4 — and refining the marine `fertility` accumulator.
- **Fisheries:** marine productivity and nekton biomass are read by civilization (Doc 10) as a coastal food source.

### 10.1a Freshwater Realm

Rivers and lakes (Doc 08) are their own realm, not a marine footnote. Large, long-lived, isolated lakes are the freshwater analog of islands: a lake basin is a province with hard barriers (a fish cannot cross land), so it radiates its own **endemic** fauna — the African Great Lakes cichlid-flock pattern, hundreds of species from one colonist, and a delight-test showcase that falls straight out of the province + endemism machinery (§6.4). River networks are dispersal corridors *within* a drainage but barriers *between* drainages, so each basin (Doc 08's connected-component labeling) carries a distinct assemblage. Freshwater guilds mirror the marine set at smaller scale; anadromy (lineages that traverse the freshwater↔marine boundary, salmon-analog) is a trait-gated crossing that couples the two realms.

### 10.2 Domestication and Pinning — the Stateful Exception

Everything in this module is regenerable from seed **except** species the user or a civilization has reached into:

- When a civilization **domesticates** a species (Doc 10), or a user **pins** one to follow across time, it is **promoted to a tracked entity** — persisted in `WorldData`, allowed to **diverge from its pure-seed identity** (selective breeding by sapients; user interventions). Selective-breeding-for-traits is a Doc 10 mechanism built on this hook and scheduled later.
- This is the single point where lazy generation becomes stateful, and it is the **biology → civilization handoff surface**. Promotion is explicit and logged; an un-pinned, un-domesticated species remains a pure function of the ledger.

### 10.3 Sapience (emergent, not scheduled)

Sapience is a **deep trait on the cognition axis**, reached by the biased walk like any other, **precondition-gated** (a large centralized brain, manipulators, sociality, and dietary generalism are typical prerequisites) and **probabilistic** — there is **no fixed sapience year**. `sapience_emergence_year` (Doc 04 §4.11) becomes an **output**, not an input: when a lineage crosses the threshold, biology emits the event and **hands off to civilization** (Doc 10), stamping the realized year. Notes:

- **`sapience_enabled`** (default true) can switch it off (a world that stays wild); **`humanoid_sapients`** (default false) constrains the sapient species' generated morphology to the upright/bilateral/four-limbed archetype (the owner's Star-Trek toggle) — otherwise the sapient may be anything the walk produced (a sapient tree, a eusocial colony-mind, a cephalopod-analog).
- *Which* lineage and *where* is emergent — the ecosystem and geography that produced a suitable lineage determine it, which is exactly the causal chain the delight test wants ("a desert kingdom here vs a maritime empire there" traceable to biology).
- Multiple independent sapient lineages are possible (especially at higher novelty/chaos), with downstream consequences for Doc 10.

---

## 11. Cross-Layer Feedbacks (the Living Planet)

Biology reshapes Layer 0. Each loop is realized as biology writing a field that a physical layer reads **one tick later** (the cycle-free lag pattern established by climate/hydrology, Doc 07 §13.2 / Doc 08 §2.3). Eight loops:

1. **Oxygenation.** Oxygenic photosynthesis raises atmospheric O₂ (Doc 07 §11.2) → gates aerobic complex life (§3.3) and shifts climate; the Great Oxygenation is an emergent event (and a possible §7.2 extinction).
2. **Carbon drawdown.** Photosynthesis + organic burial pull CO₂ (Doc 07 §11.1) → long-term cooling; a biological thermostat that interacts with the tectonic/weathering carbon cycle.
3. **Carbonate / limestone deposition.** Marine reef-builders (§10.1) deposit carbonate → `BedrockType::Limestone` (Doc 06), closing the deferred loop and feeding the fertility chain.
4. **Soil organic enrichment.** Terrestrial life deepens and fertilizes soil → refines `soil_fertility` / `soil_depth_m` (Doc 08 §10), strengthening the Cretaceous-beach and loess fertility mechanics with a living contribution.
5. **Albedo.** Vegetation darkens land; its loss (or ice/desert expansion) brightens it → a temperature feedback into climate.
6. **Biotic weathering acceleration.** Roots and lichen-analogs break rock faster than bare chemistry → speeds Doc 08 erosion, soil formation, and CO₂ drawdown; the arrival of land producers steps up the planet's whole weathering rate (as on Earth ~400 Mya).
7. **Evapotranspiration / moisture recycling.** Forests pump water back to the atmosphere → continental interiors become wetter *because* forested (the Amazon makes its own rain); a biology → precipitation feedback into Doc 07/08.
8. **Biogenic greenhouse gases.** Early anaerobic metabolisms (methanogen-analogs) emit strong greenhouse gases → warm a young planet under a faint early sun, a warming feedback preceding the oxygenation cooling.

Deeper loops (nitrogen fixation raising productivity ceilings, bioturbation altering ocean chemistry, plankton cloud-seeding) are noted for future refinement (§19), not v1.

**Architectural note:** these feedbacks are the sharpest test of the "data flows upward, interventions don't rewrite lower layers" rule (Architecture §2). They are permitted precisely because they are **forward-in-time, one-tick-lagged field writes** — biology at tick *t* influences climate at tick *t+1*, never rewriting already-computed history. Determinism is preserved by fixed evaluation order.

---

## 12. Interventions (Biology-Facing)

Per Architecture §2 and Doc 04 §9, interventions flow **downward and forward**: a Layer-1 biology intervention creates (or continues) a branch, propagates forward in time, and never rewrites already-computed Layer-0 history. Biology adds these `InterventionAction` variants (Doc 04 §9.3):

- **`PlaceImpactor { hex, size }`** — the delight-test centerpiece. A bolide is a *physical* event (it also perturbs climate and tectonics), but its defining consequence — a **selective** mass extinction and the divergent recovery radiation that follows (§7) — is computed here. Rewind 50 My, drop a meteor, run forward, compare branches: a different incumbent lineage inherits the world, and the whole causal chain is reconstructable.
- **`SeedLife { hex }` / `SterilizeRegion { region }`** — force an origin, or wipe a region; interacts with the biogenesis re-arm rule (§3.1).
- **`AdjustLineage { lineage, action }`** — boost or suppress biomass, force extinction, or grant a stay of extinction for a chosen lineage and its descendants.
- **`ForceEvolution { lineage, target }`** — push a lineage to take a specific *reachable* trait step, or fire an innovation threshold early ("make *this* clade sapient"). Constrained to steps the morphospace permits (prerequisites/exclusions still apply, §2.3) unless chaos mode is enabled.
- **`IntroduceSpecies { lineage, province }`** — assisted dispersal across a barrier: the invasive-species lever, with the food-web consequences that follow.
- **`PinLineage { lineage }`** — promotes a lineage to a tracked entity (§10.2), enabling divergence from its pure-seed identity and later selective breeding (Doc 10).
- **`AdjustBiologyParameter { … }`** — the tunable knobs (biogenesis rate, novelty temperature, extinction scale, complexity pressure, sapience toggles), effective from the intervention year forward.

All are deterministic and replayable: an intervention-log entry plus the branch-aware seed (§8.3) fully determines the post-divergence ledger and every species generated from it.

## 13. Events

Biology emits events into the branch event log (Doc 04 §8) for the user and export; **events are never consumed by other simulation systems** (inter-system communication is via `WorldData`, per Architecture §14). Because speciation is astronomically frequent, biology follows the **granularity system** (Doc 06 §6.3): the *ledger* records every lineage branch (§8.1), but the *event log* surfaces only those at or above the user's significance threshold — most individual speciations are `Trace` and pruned, while radiations, kingdom foundings, and extinctions rise to the top.

New `EventKind` variants (Doc 04 §8):

| Event | Significance | Trigger |
|---|---|---|
| `LifeEmerged { hex, year }` | Pivotal | first biogenesis (§3.1) |
| `EvolutionaryInnovation { kind, lineage }` | Pivotal / Major | an innovation threshold fires — oxygenic photosynthesis, eukaryogenesis, multicellularity, land colonization, flight, endothermy, … (§3.3) |
| `GreatOxygenation { o2_level }` | Pivotal | oxygenation crosses the aerobic threshold (§11.1), often paired with a `MassExtinction` |
| `KingdomFounded { lineage, defining_trait }` | Pivotal | a new tier-0 clade appears (§9.4) |
| `AdaptiveRadiation { province, guilds_filled }` | Major | a burst of branching fills opened niches (§6.2, §7.4) |
| `SpeciationEvent { lineage, parent }` | Trace → Notable | a lineage branches (granularity-gated; most `Trace`) |
| `LineageExtinction { lineage }` | Minor / Notable | a lineage dies out (§7.1) |
| `MassExtinction { trigger, severity, guilds_emptied }` | Pivotal | a §7.2 trigger crosses threshold; `trigger` names bolide / volcanism / climate / anoxia / biological |
| `BiomeShift { region, from, to }` | Notable | a biome belt migrates or collapses (§4.6) |
| `ReefSystemEstablished { region }` | Notable | marine builders begin carbonate deposition (§10.1, §11.3) |
| `SapienceEmerged { lineage, province }` | Pivotal | the cognition threshold fires; hands off to civilization (§10.3) |
| `BiosphereSterilized { cause }` | Pivotal | total extinction of life (§7.5) — rare, and re-arms biogenesis |

Registry-diff-style events (radiations, biome shifts, reef systems) key on stable `LineageId` / `ProvinceId`, so they are deterministic across replay.

## 14. Determinism

- **Trait walk:** discrete weighted draws seeded by `(seed, "biology.evolution", lineage, branch_index)`; topology cannot FP-diverge.
- **Population dynamics:** bounded relaxation (§5.2) with fixed-order (`ProvinceId`, then `GuildId`) summation; accumulators in fixed-point per the `fertility` precedent (Doc 06 §8.4) where they compound over many ticks.
- **Biogenesis, speciation, extinction draws:** all in ascending `HexId` / `LineageId` / `ProvinceId` order; `BTreeMap`/`BTreeSet` only.
- **Lazy generation:** pure functions of the branch-aware seed + ledger (§8.3); generation order within an assemblage is fixed (`GuildId` then index) so drill-down is stable.
- **Ledger hashing:** `ledger_state_hash(≤ year)` uses a canonical, order-independent digest so the branch-aware seed is stable across save/load.
- Byte-identical replay of the ledger and of any generated species/tree is gated (§17 #10).

---

## 15. Performance

Baseline context: the physical layers run a 4 By history at subdivision 7 in ~2 minutes; biology must not blow that up, and lazy generation must keep interactive inspection snappy.

| Item | Budget |
|---|---|
| Biology tick, subdiv 7 (province dynamics + events) | ≤ 5 ms steady state |
| Biology tick, subdiv 8 | ≤ 12 ms |
| Full 4 By run overhead, subdiv 7 | ≤ +30 s |
| Ledger memory (default world, 4 By) | ≤ ~50 MB (bounded by lineages/events, not species) |
| Generate one hex's assemblage on inspection | ≤ 50 ms (interactive) |
| Expand one tree-of-life node | ≤ 16 ms |

Levers: dynamics at province (~hundreds) not hex (~tens of thousands); the trait graph static and cache-resident; the food-web sparse and small; lazy generation strictly on demand with a transient LRU cache; event log stride era-dependent; scratch buffers reused. `GENESIS_SLOW_TICK_STEP_MS` instrumentation as elsewhere.

---

## 16. Modding

All content-bearing surfaces are data-driven from day one (Architecture §10), via the Doc 11 rule engine:

- **Trait morphospace** — nodes, edges, tiers, tags (§2). Mods add traits (exotic metabolisms, silicon structure, magic) or reweight the graph.
- **Guild roster & food-web templates** — functional roles per realm/biome (§4.1).
- **Biome definitions** — climate/soil envelopes and producer requirements (§4.6).
- **Innovation thresholds** — the rule set gating oxygenation, eukaryogenesis, multicellularity, land colonization, flight, sapience (§3.3, §10.3).
- **Presets** — `earth_like` / `alien` (default) / `weird` bundle a novelty temperature and graph weighting; a `familiar_biochemistry` preset biases toward recognizable kingdoms. All moddable.

Physics constants (trophic efficiency, thermodynamic niche guarantees) and the determinism rules are engine-owned, not moddable.

---

## 17. Validation Criteria

Biology has no single ground truth, so validation checks the **shape** of the history, not any specific outcome. Doc 06 §11 pattern: cheap per-tick debug asserts + `--ignored` deep-time gates. On the Earth-like preset, the default world must satisfy:

1. **Life appears** within a plausible window of the biogenesis estimate; the origin is a marine vent hex.
2. **Complexity progression** occurs in order (prokaryote → oxygenation → eukaryote → multicellular → land) with Earth-order-of-magnitude timing; oxygenation raises O₂ and gates complex life.
3. **Trophic pyramid holds** every tick: producer biomass > herbivore biomass > carnivore biomass, everywhere occupied.
4. **Diversity trajectory** rises over time with sharp dips at mass extinctions and post-extinction radiations; the count of mass extinctions over 4 By is in an Earth-plausible band.
5. **Latitudinal gradient:** equatorial provinces are markedly more species-rich than polar (richness-driven, §4.4).
6. **Endemism:** an isolated province (island / post-rift microcontinent) shows an endemic radiation distinct from mainland lineages.
7. **Guaranteed niches always filled:** no occupied province ever has free producer or decomposer energy sitting unexploited past a short lag.
8. **Convergence:** at least one pair of distantly-related lineages (different Phylum) independently evolves the same Class-level trait under the same niche pressure.
9. **Extinction selectivity & recovery:** a size/trophic-selective extinction is followed by radiation of a *different* incumbent set than before.
10. **Determinism:** byte-identical ledger and byte-identical generated species/tree (sampled) at 1 By / 2 By / 4 By, same seed; identical pre-divergence and correctly divergent post-divergence across a branch.
11. **Sapience is emergent & optional:** appears at no fixed year, in a lineage whose traits satisfy the preconditions; disabling it yields a wild world; the run does not depend on it occurring.
12. **Novelty dial responds:** low temperature yields recognizable clustered kingdoms; high temperature yields non-Earth body plans — over a 3-point sweep.
13. **Feedbacks fire:** oxygenation, carbon drawdown, and biotic weathering measurably move the climate/hydrology fields they target.
14. **Perf:** §15 budgets hold.
15. **Physical gates stay green** with biology active and its feedbacks engaged (tectonics/climate/hydrology validation unbroken).

Strictness is a setting: the default holds the Earth-like preset to this Earth-*shape*; looser modes (and the `weird` preset) relax the shape gates while keeping determinism/perf/physical gates hard, so genuinely alien trajectories are allowed (the owner's direction).

---

## 18. Integration & Migration

- **Doc 04 §4.10/§4.11:** extend `BiologyParameters` (§3.1 fields); `life_emergence_year` reinterpreted as target/output; `sapience_emergence_year` becomes an output. Add `biome` semantics (emergent), and the new bulk arrays / ids (§8.6) — `ProvinceId`, `LineageId`, `GuildId`, `TraitId`, `WaterBodyKind`-style enums as needed.
- **Doc 06:** biology deposits `Limestone` (closing §8.4); flood-basalt/volcanism events feed §7.2.
- **Doc 07:** biology drives O₂/CO₂ (§11.1–11.2), albedo, moisture recycling; consumes `climate_regime`, glaciation, temperature/precip.
- **Doc 08:** biology refines soil fertility/depth; consumes soil, water access, provinces (connected components), condensation timeline; reef carbonate ties to coastal/marine geometry.
- **Doc 10 (Civilization):** consumes food/carrying-capacity, domesticable species (§10.2 handoff), and the sapience emergence event/location (§10.3).
- **Doc 11 (Rule Engine):** biology is a first-class consumer (traits, guilds, innovations).
- **Doc 14 (Rendering):** biome maps, the creature illustrator (§8.5 contract), the tree-of-life view (§9.3).
- **Doc 15 (Export):** bestiary and tree-of-life export from §8/§9.

---

## 19. Open Questions (tracked refinements — the systems themselves are in)

1. **Deeper feedbacks:** nitrogen fixation (productivity ceiling), bioturbation (ocean chemistry), plankton cloud-seeding (albedo).
2. **Recent-era fine ecology:** boom-bust cycles (§5.4), seasonal migration, individual-scale ecology where a civilization observes it.
3. **Symbiosis depth:** endosymbiosis as an explicit trait-fusion step (beyond eukaryogenesis), lichen-style mutualisms as merged guilds.
4. **Disease / parasites** as a coupling that regulates host guilds and later drives civilization plague events (Doc 10).
5. **Coevolutionary arms races** (Red Queen) as an explicit accelerant on the walk for tightly-coupled predator/prey pairs.
6. **Trait loss & vestigialization** rendering (cave-dwellers losing eyes) as bestiary flavor.
7. **Sub-hex ecology** for the creek-scale zoom (Doc 14): individual creature placement synthesized like sub-hex terrain (Doc 04 §3.7).
8. **Multiple-origin worlds** (`multiple_origins`): interacting independent trees, competitive replacement, and the chaos-mode implications.
9. **Selective-breeding mechanics** (Doc 10) built on the §10.2 domestication hook.

---

## 20. Implementation Prompt Plan (estimate)

Phase 4. Estimate **P4-1 … ~P4-16** along the section boundaries:

1. **P4-1** — Crate scaffold (`genesis_biology`), `SimulationLayer` registration after hydrology; extend `BiologyParameters`; new ids/arrays (§8.6).
2. **P4-2** — Trait morphospace: content schema, core graph authoring, the rule-engine trait/guild evaluation (§2, §16).
3. **P4-3** — Biogenesis + microbial era + innovation thresholds (§3); oxygenation feedback stub.
4. **P4-4** — Biogeographic provinces (connected-component labeling, dynamic identity) (§5.1).
5. **P4-5** — Guilds, niches (guaranteed/contingent), richness scalar + saturation cap (§4).
6. **P4-6** — Carrying-capacity population dynamics + guild food-web (§5.2–§5.3).
7. **P4-7** — Biomes emergent + migration/ecotones (§4.6).
8. **P4-8** — Speciation: the biased walk, all triggers, dispersal/endemism, tempo decoupling (§6).
9. **P4-9** — Extinction: background + mass triggers + selectivity + recovery radiation (§7).
10. **P4-10** — The ecological ledger + event log + branch-aware determinism contract (§8.1–§8.3).
11. **P4-11** — Lazy generation: coherent assemblages, recursive inheritance, drill-down, saturation materialization (§8.4).
12. **P4-12** — Phylogeny + Linnaean taxonomy (ranks from tiers) + tree-of-life data (§9).
13. **P4-13** — Marine parity + reef/limestone loop; sapience emergence + handoff; domestication hook (§10).
14. **P4-14** — Cross-layer feedbacks, all eight, with one-tick-lag wiring (§11); Doc 06/07/08 edits.
15. **P4-15** — Rendering: biome maps, bestiary text generation, the axis-driven creature renderer (grammars + color-shiftable pixel parts, §8.5), tree-of-life view; illustration contract to Doc 14.
16. **P4-16** — Events + biology interventions (incl. `PlaceImpactor`, the delight-test path), validation suite (Earth-shape gates), performance pass, 3-seed × 3-preset calibration sweep, Phase 4 exit review.

Creature illustration (§8.5) and sub-hex ecology (§19.7) are specified here, implemented with the rendering/zoom phase (Doc 14).

---

## Appendix A. Worked Example — the Sapient Tree

To make the machinery concrete, trace one improbable-but-reachable outcome the owner asked about:

1. A low-latitude marine lineage walks into the **producer** basin: `photosynthesis + cellulose_wall + multicellular` (a "plant-analog").
2. Land colonization opens; the lineage acquires desiccation resistance and structural support, radiating across a new terrestrial province (a §6.2 trigger-3 radiation).
3. In a densely-competed, disturbance-prone province, `selective_payoff` (§2.5) favors any step toward mobility to chase light — a rare but non-zero pull. A descendant acquires **contractile tissue** (a motility step reachable from multicellular + the right proximity edges), then **nerve-net** (prerequisites now met), then **ganglia**.
4. Continental split (allopatry) isolates this motile-autotroph lineage; free of its ancestral competitors, it radiates — acquiring **limbs**, **centralized brain**, sociality.
5. Preconditions for **sapience** are met; the cognition-axis threshold fires. Biology emits the sapience event and hands a **sapient photosynthetic lineage** to civilization — a walking, thinking tree, its whole causal chain reconstructable in the tree-of-life view, each branch tagged with the trait that made it and the geographic trigger that isolated it.

At **low** novelty temperature this walk is so penalized it essentially never completes (you get familiar plants and animals). At **medium/high** it becomes a rare-but-real outcome — and, being seeded, it is identical every time that world is regenerated, and divergent the moment a branch changes the ledger upstream of it.

---

*End of Biology Module Specification.*
