# 09-IMPL — Biology Sim Foundation Plan (P4-1 → P4-5)

**Document Type:** Tier 3 — Implementation Plan
**Status:** **Through P4-11 — Doc 09 foundation complete + limitations pass.** All slices P4-1…P4-11 landed. **P4-8/P4-9/P4-10 now build a real recorded lineage ledger** (LUCA → kingdoms → … → species by biased-walk descent-with-modification, with extinction), plumbed to the viewer (`generate_full_history` returns it → `GenEvent::BiologyLedger` → the adapter reads it). The Tree of Life is a **true family tree back to LUCA** (~280 lineages, extinct lines greyed), species in the Bestiary/Life-tab are **coherent real lineages**, and the map shows real simulated biomes/biomass/diversity. **Limitations pass:** 13 of the 14 documented limitations are now fixed/improved (see the section below) — clickable species detail + Linnaean classification panel, geography-tied endemism, reversal/loss walk steps, stalled microbial-only worlds, real shared-state O₂, volcanic-vent biogenesis, biome-similarity provinces, marine-nutrient richness v2, a dirty-flagged heavy-field recompute, and a tick coordinator that wakes dormant-at-start layers (biology's Formation workaround removed). Only caller-owned `BiologyState` (branch persistence) is deferred. Build/test/clippy green (46 biology, 142 core, 6 rules).
**Last Updated:** July 2026
**Owner:** Brax Johnson
**Implementing Phase:** 4 (Biology)

Detailed execution plan for the **foundation** slices of Doc 09 (Biology). Refines Doc 09 §20's high-level P4-N list with the code as it actually stands and the owner's decisions below. Doc 09 is the authority on *what*; this is the authority on *sequence and files*.

## Decisions (locked, July 2026)

1. **Rule engine: build Doc 11 first.** Biology's trait-graph legality and guild membership are data-driven rules from the start, not hardcoded Rust. Doc 11 is a new slice (**P4-R**) inserted before P4-2.
2. **Integration: always-on at emergence.** No `biology.enabled` flag. The layer is registered in the real generation path; it is dormant until biogenesis (P4-3) gives it work.
3. **Trait graph: fuller core graph (~100+ nodes)** authored in P4-2, across all §2.2 axes — not a minimal to-multicellular subset.
4. **This step: write this plan, then implement P4-1.**

## Current state (what already exists)

- **The `BiologyView` read seam is done** (Prep-09, shipped): `genesis_core::biology_view::{BiologyView, Assemblage, SpeciesPeek, TreePeek, TreeNodePeek, LifeEventPip, GuildSummary}`. `StubBiologyView` (genesis_ui) implements it; `ActiveBiologyView(Box<dyn BiologyView>)` (genesis_render) is the swap point, inserted at `genesis_ui::ui.rs:998`. The **6-step swap checklist** lives on `ActiveBiologyView` in `genesis_render/src/resources.rs`.
- `WorldData` already has `biome: Vec<BiomeId>`, `biomass: Vec<f32>`, `fertility: Vec<f32>`.
- `BiologyParameters` exists with 3 fields (`life_emergence_year`, `mutation_rate_scale`, `extinction_scale`).
- `genesis_biology` is an **empty stub crate** (in the workspace, wired to nothing).
- Layer pattern: `attach(&mut State) -> (Layer, Rc<RefCell<State>>)`, `detach_state`, `SimulationLayer { name, tick_interval, advance -> Vec<()> }`, registered in `genesis_ui::worldgen::generate_full_history` after hydrology.

## The real `BiologyView` adapter comes later

P4-1 does **not** touch the seam. Once a ledger exists (P4-3+), a `genesis_biology` adapter implements `genesis_core::biology_view::BiologyView` over the real ledger and replaces `StubBiologyView` via the resources.rs checklist (one line in `ui.rs` + filling `HistoryFrame` biology fields + real event pips). Until then the stub keeps the shell alive.

---

## P4-1 — Crate scaffold + data layer + params + dormant layer registration  *(this slice)*

**Goal:** `genesis_biology` becomes a real layer crate registered in the generation path (dormant), and `WorldData`/`BiologyParameters` gain the Doc 09 §8.6/§3.1 surface. No simulation behavior yet.

**Files & changes:**
- `genesis_core/src/data/ids.rs` — add `ProvinceId(u16)` (NONE=MAX), `GuildId(u16)` (NONE=MAX), `LineageId(u64)` (NONE=MAX), `TraitId(u32)`. (`SpeciesId(u32)` already exists.) Mirror the existing derive set incl. `Serialize/Deserialize`.
- `genesis_core/src/data/mod.rs` — re-export the new ids; add per-hex arrays `biotic_richness: Vec<f32>`, `primary_productivity: Vec<f32>`, `province_id: Vec<ProvinceId>`, `dominant_lineage: Vec<LineageId>` in the Layer-1 block; init in `WorldData::new`; extend the array-size test.
- `genesis_core/src/parameters/core.rs` — extend `BiologyParameters` with `biogenesis_rate_scale`, `multiple_origins`, `novelty_temperature`, `complexity_pressure`, `sapience_enabled`, `humanoid_sapients` (Doc 09 §3.1); re-doc `life_emergence_year` as the ramp **target/expected**, realized year an output.
- `genesis_core/src/parameters/mod.rs` — defaults (`biogenesis_rate_scale 1.0`, `multiple_origins false`, `novelty_temperature 1.0`, `complexity_pressure 1.0`, `sapience_enabled true`, `humanoid_sapients false`).
- `genesis_core/src/parameters/validation.rs` — `validate_scale` for the three new f32s.
- `genesis_biology/Cargo.toml` — deps: `genesis_core`, `serde`, `tracing`.
- `genesis_biology/src/{lib,state,layer}.rs` — `BiologyState` (Default, empty for now); `BiologyLayer` (attach/detach); `SimulationLayer` impl with **`tick_interval` = 0 (dormant)** and no-op `advance` (P4-3 activates both).
- `genesis_ui/Cargo.toml` + `genesis_ui/src/worldgen.rs` — construct `BiologyState` **inside** `generate_full_history` and register `BiologyLayer` after hydrology (no signature change → zero call-site churn; re-plumb to caller-owned state at P4-3).

**Tests:** param defaults + validation ranges; `WorldData::new` sizes the 4 new arrays; a world still generates with the biology layer registered; determinism smoke (same seed → same arrays).

**Done when:** `cargo build/test/fmt/clippy` green across the workspace; the default run is unaffected (dormant layer); the physical-layer screenshots/tests still pass.

---

## P4-R — Doc 11 Rule Engine  *(landed)*

Doc 11 drafted ([docs/11-rule-engine-specification.md](11-rule-engine-specification.md)) and implemented as the new **`genesis_rules`** crate: a declarative predicate AST (`Predicate`/`Rule`), `FactContext` (trait set + named scalars), pure `evaluate`, a fail-closed `RuleRegistry`, and the `trait_gate(prereqs, exclusions)` helper for the Doc 09 §2.3 reachability gate. Deterministic, safe against untrusted mod content, no FP-divergent branching (Doc 09 §14). 6 tests, clippy+fmt clean, added to the workspace. Consumed first by P4-2.

## P4-2 — Trait morphospace (on the rule engine)  *(landed)*

Shipped in `genesis_biology`: `morphospace.rs` (`TraitAxis`, `TraitTag`, `TraitNode`, `TraitGraph` with string→dense id resolution + symmetrized exclusions + `candidate_steps`, `TraitSet` genome); `core_graph.rs` (**70 hand-authored nodes across all 14 §2.2 axes** — microbial progression, the producer/animal/fungus basins, the reachable sapience line); `evolution.rs` (the biased walk §2.4 — `candidate_weights`/`biased_walk_step` scoring `proximity × gate × payoff × novelty_factor`, legality via `genesis_rules::trait_gate`, `SelectivePayoff` hook with `NeutralPayoff` default). 10 tests (graph validity, reachability, walk determinism, novelty-temperature response, a greedy walk reaching multicellularity + nerves). Clippy+fmt clean.

**Deferred within P4-2** (noted, not blocking): reversal/loss steps (gain-only for now, so progressive axes like organization accumulate rather than replace — no exclusion); ecological `SelectivePayoff` (wired to guilds in P4-5); expanding the graph toward the full ~200–400 nodes and externalizing to a moddable data file (Doc 09 §2.8/§16).

## P4-3 — Biogenesis + microbial era + innovation thresholds  *(landed)*

**Biology is live.** Shipped: `biogenesis.rs` (deterministic marine-vent origin — hazard ramp scaled to `life_emergence_year`, first success in ascending `HexId` order, single origin, §3.1–3.2); `microbial.rs` (the innovation-gated low-res era — a single biosphere genome walks the deep tiers, O₂-gated + complexity-biased so the §3.3 order emerges: oxygenic photosynthesis → Great Oxygenation → eukaryogenesis → multicellularity); `state.rs` (origin, root genome, O₂ proxy, milestone set, event buffer); `events.rs` (`flush_events_to_branch`, mirroring the physical layers). New `EventKind`s in `genesis_core`: `LifeEmerged`, `EvolutionaryInnovation { InnovationKind }`, `GreatOxygenation`. Graph amended: added `core:eukaryote` so eukaryogenesis precedes multicellularity. `worldgen.rs` flushes biology events. 16 biology tests (life emerges at a deep vent, climbs the ladder in order, deterministic origin, oxygenation-before-eukaryogenesis) + 141 core.

**Fixed en route:** the tick coordinator permanently skips a layer that reports `interval == 0` at world start (`init_next_ticks` → `i64::MAX`), so biology must stay scheduled with a coarse Formation cadence and no-op internally until deep ocean exists (not return 0).

**Verified end-to-end:** the `#[ignore]` full-run test in `worldgen.rs` (uses `generate_full_history` directly — the streaming wrapper returns the year-0 clone + render frames, so its event log is empty; surfacing biology events to the viewer is separate event-stream plumbing). A default 1-By run: **life emerges ~278 My at a deep-ocean vent, then photosynthesis (280) → Great Oxygenation (286, O₂ 0.11) → eukaryogenesis (286) → multicellularity (287)** — correct §3.3 order. (Microbial-era *tempo* is fast at v1; pacing tuning via `complexity_pressure`/tick cadence is a later refinement.)

**Deferred:** caller-owned `BiologyState` persistence across branches; the O₂ climate coupling (§11); fuller per-hex reflection (biomass/richness) — P4-4/P4-5.

## P4-4 — Biogeographic provinces  *(landed)*

Shipped `province.rs`: `Realm` (Marine/Terrestrial/Freshwater from water state), `BiogeographicProvince`, `ProvinceRegistry`, and `label_provinces` — flood-fill over `grid.neighbors` into same-realm connected components, discovered in ascending `HexId` order so `ProvinceId`s are **deterministic** (lowest member hex is the stable key, §5.1), with dispersal-adjacency (`neighbors`) and `world.province_id` filled. Recomputed each biology tick once life exists (stored on `BiologyState`; a dirty-flag optimization is later). 3 tests (dense coverage, realm assignment, determinism). Guild occupancy / food web / richness — the rest of §5.1 — are P4-5.

## P4-5 — Guilds, niches, richness + saturation cap  *(landed)*

Shipped `guild.rs` (the `GuildRoster` — 8 core guilds across marine/terrestrial, each a **membership `Rule`** on `genesis_rules`; `fills_guild` evaluates a trait set against a guild — the §4.1 rule-based membership machinery) and `richness.rs` (real `primary_productivity` from climate+soil+water+light §5.2; the biotic-richness scalar `R` from productivity × stability × province-area × disturbance §4.4 — **latitudinal gradient emergent, not hardcoded**; the saturation cap `species_in_guild` §4.5; province-level `richness` + `occupied_guild_count`). `advance` now fills `primary_productivity` and `biotic_richness` each post-origin tick. 5 tests (membership rules, saturation shape, productivity favors warm/wet/fertile, richness fills arrays). The `biotic_richness` array now carries real data — the Diversity map layer's eventual source.

**Deferred:** actual guild *occupancy* (which lineage fills which guild in a province) needs the lineage ledger + speciation (P4-8/P4-10) — a province currently derives only a guild *count* from R. Guaranteed-vs-contingent niche cascade (§4.3) and the food web (§5.3) come with occupancy.

---

## P4-6 — Population dynamics + guild occupancy  *(landed)*

`population.rs`: per-hex living **biomass** = productivity × (1 + richness) (fills `world.biomass` — the Biomass map layer's data), and per-province **guild occupancy** by the guaranteed→contingent cascade (§4.3, producers/decomposers first, then herbivores/predators) up to the count richness supports. Simplified: no carrying-capacity relaxation or trophic food-web dynamics (§5.2–5.3) yet.

## P4-7 — Emergent biomes  *(landed)*

`biome.rs`: 13-biome content table; `assign_biomes` maps `climate_regime` × water × precipitation → `world.biome`, gated on life (no producers ⇒ barren). A full 1-By run yields 12 biome types (ocean, tropical forest, savanna, hot desert, boreal, ice cap, …). Simplified: land biomes appear as soon as life exists (no land-colonization gating, §3.4); biome = regime, not yet regime × *actual producer strategy present* (§4.6).

## P4-11 — Lazy generation + the real `BiologyView` adapter  *(landed)*

`view.rs` — `GeneratedBiologyView` implements `genesis_core::biology_view::BiologyView`, **swapped in for `StubBiologyView`** (`genesis_ui::ui` on `GenEvent::InitialWorld`). It is self-contained: biome/richness/biomass read the real simulated `WorldData` arrays (carried in `HistoryFrame`s so scrubbing shows real data); occupied guilds recompute from richness+realm; **species assemblages** generate lazily and deterministically from `(seed, hex, guild, index)` — binomial names, family, trait chips, description; the **tree** and **life-event pips** generate structurally. Verified end-to-end (`biology_on_the_map` ignored test).

## P4-8 / P4-9 / P4-10 — Speciation / extinction / recorded ledger  *(landed)*

`ledger.rs` (`LineageRecord`/`Ledger` + `rank_for_tier` — the Doc 09 §9.2 Linnaean rank from a trait's tier) and `speciation.rs` (`build_radiation`): at the multicellular threshold the biosphere radiates from LUCA into kingdom clades (by metabolism), each radiating by the **biased walk** — every child gains one trait, so descent-with-modification is real and a clade resembles its ancestors. Leaves specialize into guilds; a deterministic hazard marks ~35% extinct. The forest (~280 lineages) is a pure function of the seed, built once, stored on `BiologyState`, returned by `generate_full_history`, streamed to the viewer (`GenEvent::BiologyLedger`), and read by the adapter for the Tree of Life (real phylogeny + ranks, time-aware, extinct greyed) and the Bestiary (species = real lineages of the hex's occupied guilds). **Still simplified (§6/§7):** no per-tick speciation triggers tied to geography (allopatry, niche divergence), extinction is a flat hazard not a selective mass extinction, and `dominant_lineage` isn't yet written per-hex.

## Known Limitations & Deferred Work (revisit / fix later)

Honest notes accumulated during the foundation build — simplifications and bugs to return to, not blockers.

1. **Microbial-era tempo** (P4-3). **[MUCH IMPROVED]** Added a 4%/tick step propensity (punctuated equilibrium — mostly stasis) and slowed O₂ accrual, so the ladder now spans ~210 My (life 278 → photosynthesis 305 → GOE 405 → eukaryogenesis 460 → multicellularity 488) instead of ~9 My. Still faster than Earth's billions; a proper generations-vs-ticks model (§5.5) tied to run length is the fuller fix.
2. ~~**The streaming path drops the biology event log**~~ **[FIXED]** — `generate_world_streaming` now reads the branch event log after generation and emits `GenEvent::LifeEvents(Vec<LifeEventPip>)`; the viewer stores it (`RealLifeEvents`) and `refresh_life_pips` replaces the fabricated timeline pips with the real chronicle (true years).
3. **The tick coordinator can't wake a dormant-at-start layer** (P4-3, `genesis_core`). **[FIXED]** — `TickCoordinator` now re-polls a layer that reports `interval == 0` on a coarse cadence (`DORMANT_REPOLL_YEARS`) instead of parking it at `i64::MAX` forever, so a layer can wake once world state gives it work (proven by `dormant_at_start_layer_wakes_when_it_gets_work`). Biology's Formation workaround is **removed**: it now returns `0` (dormant) during Formation and the coordinator wakes it when oceans form — verified end-to-end (`life_emerges_in_a_full_run`, release).
4. **Gain-only walk** (P4-2). **[FIXED]** — the reversal mechanic (§2.3) is implemented: `evolution::biased_evolution_step` scores **loss** steps alongside gains, weighted `1/(1+reversal_cost)` and gated by prerequisite-integrity (a trait can't be shed while something present still depends on it). The microbial biosphere walk exercises it (`MICROBIAL_LOSS_BIAS`), so e.g. a chemoautotroph can shed `chemosynthesis` once it turns phototroph. The recorded macro-radiation stays **gain-only by design** (a legible descent-with-modification tree); progressive-axis grades are prerequisite-linked so `unicellular` legitimately remains in the genome as a developmental prerequisite, but the presentation now **collapses superseded grades** (`view::visible_traits`), so a multicellular species reads as "multicellular", not the whole unicellular→…→multicellular ladder.
5. **Single-genome microbial biosphere** (P4-3). **[PARTLY FIXED]** — **"never oxygenates" stalled worlds are now modeled** (§3.3): `speciation::is_microbial_only(seed, complexity_pressure)` deterministically stalls a fraction of low-`complexity_pressure` worlds (the default 1.0 stalls none, so every existing run is unchanged), and `build_microbial_only` records their sparse tree — LUCA plus divergent microbial metabolism lineages (a bacterial-mat planet: microbial Tree of Life, empty Bestiary by design). Such a world never fires the O₂/eukaryogenesis/multicellularity milestones. *Still simplified:* normal (non-stalled) worlds still climb via a single driving biosphere genome (the LUCA→kingdom split records the divergence at the ledger level, but there is no live multi-population microbial ecosystem before the macro radiation).
6. **O₂ is an internal proxy** (P4-3). **[FIXED on the biology side]** — atmospheric O₂ is now **real shared world state** (`WorldData::atmospheric_oxygen_fraction`, genesis_core): oxygenic photosynthesis accrues it, and the Great Oxygenation / eukaryogenesis / multicellularity gates read that world field (the `BiologyState::o2_fraction` proxy is now just a synced mirror for accessor callers). *Still deferred (climate side, Doc 07 §11):* climate consuming O₂ for the temperature/CO₂-drawdown feedback — biology now exposes the coupling variable, but climate doesn't yet read it.
7. **Biogenesis suitability = deep ocean only** (P4-3). **[FIXED]** — the vent origin now requires **deep ocean adjacent to volcanism/hotspot** (§3.1): `suitable_vent_hexes` keeps deep-ocean hexes on or beside `Igneous` bedrock (tectonics' hotspot/ridge/arc marker), falling back to plain deep ocean only when a young ocean has no submarine volcanism yet, so life still arises. Origin lands at a real submarine hydrothermal vent.
8. **`BiologyState` is local, not caller-owned** (P4-1/P4-3) — branch-scoped persistence and caller-side event flush re-plumbing deferred.
9. ~~**Provinces recomputed every tick**~~ **[FIXED with #13]** — the province flood-fill runs inside the throttled heavy-field block, now itself dirty-flag-gated (see #13), so it recomputes only when the geography/climate inputs actually changed.
10. **Provinces are realm-based only** (P4-4). **[FIXED]** — `label_provinces` now flood-fills on realm **and** a coarse `biome_zone` (from `climate_regime`: tropical / desert / temperate / boreal / polar), the §5.1 "similar biome" key, so a continent's tropical and boreal halves become distinct provinces. Grouped rather than the raw 11 regimes to avoid over-fragmentation; still deterministic (ascending-`HexId` discovery). This also feeds the dirty-flag (climate change ⇒ re-label) and gives endemism/richness a biome-coherent granularity.
11. **Guild occupancy** (P4-5). **[MOSTLY FIXED]** Occupancy is now tied to the ledger: a guild is occupied at a hex only if the region has an extant lineage that fills it, with the guaranteed-vs-contingent cascade (§4.3 — producers/decomposers guaranteed, herbivores/predators contingent on their prey guild + enough R). *Still deferred:* the explicit food-web graph and carrying-capacity relaxation *dynamics* (§5.2–5.3, trophic cascades under perturbation) — occupancy is static per snapshot, not a simulated population system.
12. **Richness/productivity model is v1** (P4-5). **[IMPROVED to v2]** — two of the named gaps are closed: **marine productivity** now includes a **nutrient/upwelling** factor from water depth (shallow shelves/upwelling coasts rich, the deep open ocean oligotrophic → `DEEP_NUTRIENT_FLOOR`), not just light×temperature; and **disturbance** now compounds glacial ice **and** recently-resurfaced volcanic crust (`Igneous` bedrock) rather than ice alone (a better `age_since_disturbance` proxy, §4.4). *Still v1:* the factor curves remain hand-tuned, and marine nutrients are a depth proxy, not a real current/upwelling-circulation model.
13. **Biology fields recomputed on a 5 My stride** (P4-5/6/7). **[FIXED]** — the heavy fields (provinces/richness/biomes/biomass/occupancy) still refresh at most every 5 My, but a **dirty-flag** now guards even that: each heavy opportunity computes a cheap O(n) FNV-1a `terrain_signature` over the geography/climate inputs (elevation, water, regime, precipitation, temperature, soil) plus the land-colonization gate, and skips the whole recompute when it matches the last one (`BiologyState::heavy_signature`). Static late-history geography now costs one hash per stride instead of a full flood-fill + richness + biome + biomass + occupancy pass. *Still simplified:* the 5 My stride can still lag a very fast geography change between opportunities.
14. **Endemism now works, but via geo-regions, not real provinces** (P4-8/9/10). **[PARTLY FIXED]** Lineages are tagged with a biogeographic region and each kingdom radiates a distinct subtree per region (`BIOGEOGRAPHIC_REGIONS = 12`); the adapter maps each hex to a region by lat/lon, so **different areas show distinct endemic clades** (coherent within a region). *Still simplified:* regions are a geometric lat×lon grid, not the real simulated provinces/continents; speciation still isn't triggered by allopatry/climate events (§6); extinction is a flat 35% hazard, not a selective mass extinction (§7); and the radiation is built in one pass at multicellularity, not progressively per tick.
15. ~~**Life-event timeline pips are generated milestones**~~ **[FIXED with #2]** — pips now come from the real event log with true years. (The adapter's `life_events` fabrication remains only as a pre-generation-complete fallback.)
18. ~~**No clickable species detail**~~ **[FIXED for the Bestiary]** — Bestiary species cards are now clickable buttons (hover-highlighted, "▸ details" affordance) that open a detail modal via a new `BiologyView::species_detail(species_id) -> Option<SpeciesDetail>`: name, guild·family, description, the full genome as chips, and the classification ladder. *Still text-only:* the hex inspector's Life-tab species list (it renders formatted text, not per-species entities).
19. **Tree of Life doubles as classification** **[ADDRESSED via the detail panel]** — the Tree of Life stays a real family tree; the *separate* Linnaean view now lives in each species' detail panel as a properly nested Kingdom→…→Species classification ladder (`SpeciesDetail::classification`, ranked by ancestry depth, not trait tier, so it's always a clean nested hierarchy). *Still deferred:* a global browsable Kingdom→Species classification tree (the per-species ladder covers the drill-down need); the family-tree list still caps at 160 rows with a "+N more".
16. **Biomass** (P4-6). **[IMPROVED]** Now producer-anchored — total standing biomass ∝ primary productivity × the trophic-pyramid sum (§5.2), so diversity redistributes biomass rather than inflating it. (Per-guild carrying-capacity relaxation is still the deeper model; magnitudes remain arbitrary.)
17. ~~**Land biomes appear at first (marine) life**~~ **[FIXED]** — land biomes are now gated on multicellularity (a proxy for land colonization, §3.4); continents stay barren and only the ocean is alive until then. (A dedicated `LandColonization` trait/event would be the precise gate.)

## Determinism & performance (all slices)

- Streams seeded `(seed, "biology.<phase>", …)`; ascending `HexId`/`LineageId`/`ProvinceId` order; `BTreeMap`/`BTreeSet` only; fixed-point for long-compounding accumulators (Doc 09 §14).
- Budget: biology tick ≤ 5 ms subdiv 7, full 4-By overhead ≤ +30 s (Doc 09 §15). P4-1's dormant layer costs nothing.

*Living plan — update as slices land.*
