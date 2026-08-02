<!-- Generated plan for docs/09B-hex-state-inputs-for-biology.md. Sequenced & dependency-checked; ~43 backlog items across A-I. -->

> **Verification note — three corrections to Doc 09B found by reading the code:**
> 1. `tidal_range_m(moon_count)` **does** exist (`genesis_hydrology/src/coastal.rs:20`) and `update_coastal` applies it every tick — 09B C7's "no symbol exists" is wrong. The cheap C7 win is *surfacing* that scalar; the new work is per-hex intertidal zonation.
> 2. The `cos(lat)` light term lives **only** in the marine branch (`genesis_biology/src/richness.rs:84-87`); the terrestrial branch has **no** light term today, so G1/G2 add new terrestrial code, not an edit.
> 3. The `atmospheric_oxygen_fraction` loop (`microbial.rs:123`) is a **single-owner** read-modify-write — it is the template for *Class-A* feedback only. Every §11 feedback where a producer also writes the field (CO₂, soil) is *Class B* and must write the producer's state, not the WorldData mirror. Getting this wrong silently discards the write.

# Doc 09B Implementation Plan — Per-hex State Inputs for Biology

This plan sequences the ~50 environmental inputs biology needs (Doc 09 §4–§11) strictly by dependency, so nothing is scheduled before what it reads exists. Each item carries its assessment ID (A1…I4). The spine is: read what producers already populate (Phase 1), derive what falls out of existing fields for free (Phase 2), stand up the persisted-province-identity + speciation subsystem that the biogeography items all depend on (Phase 3), add genuinely new physical fields and their producers (Phase 4), layer disturbance regimes on top (Phase 5), and only then close the biology→physics feedback loops (Phase 6). Resource/food-web substrates (I1–I4) are treated as a cross-cutting track. Biology stays a pure consumer everywhere except Phase 6. Two conventions — folding every consumed input into `terrain_signature`, and choosing storage by ownership rather than convenience — are established in Phase 0 and inherited by every later phase.

Key files (absolute):
- Field defs / defaults / serialization: `/Users/braxjohnson/TimpCreative/genesis-engine/crates/genesis_core/src/data/mod.rs`
- Hydrology enums/flags: `/Users/braxjohnson/TimpCreative/genesis-engine/crates/genesis_core/src/data/hydrology.rs`
- Biology consumers & feedback template: `/Users/braxjohnson/TimpCreative/genesis-engine/crates/genesis_biology/src/{richness.rs, biome.rs, province.rs, population.rs, microbial.rs, layer.rs, speciation.rs, view.rs}`
- Producers: `genesis_climate/src/{layer.rs, temperature.rs, ocean_currents.rs, carbon.rs, state.rs}`; `genesis_hydrology/src/{soil.rs, lakes.rs, coastal.rs, regime.rs, ice.rs, groundwater.rs}`; `genesis_tectonics/src/{world_rebuild.rs, volcanism.rs, hotspots.rs}`

---

## 0. Architecture & conventions

### 0.1 The state-resolution rule (Doc 09 §5.1: dynamics per-province, per-hex derived for rendering)

Decide storage by **who owns the quantity and how it varies**, not by convenience:

| Storage | Use when | Examples |
|---|---|---|
| **Global scalar `f32` on WorldData** | One planetary number; well-mixed or a single budget | `co2_ppm` (A1), `sea_level_m`, `global_temperature_c`, `glaciation_intensity`, `atmospheric_oxygen_fraction`, `tidal_range_m` (C7 — already exists as a static global from moon count) |
| **Per-hex `Vec<_>` on WorldData** | A *physical layer* owns it, it varies continuously in space, and render/other layers also need it | marine salinity (C1), water pH (C2), dissolved O₂ (C4), soil N/P (E1), soil age (E5), cloud (B5), volcanism recency (F2), per-hex intertidal zonation (C7 — the genuinely new half) |
| **Inline-derived per-hex (no storage)** | Computable from populated fields during the O(n) richness sweep or flood-fill and consumed immediately | photoperiod (G1), insolation integral (G2), UV proxy (A3), aridity index (B2), depth zones (D1), **upwelling/nutrient factor (C3)** |
| **Province-scoped state on `BiologyState`** | *Biology* owns it as a dynamical stock, a graph, or cross-tick history; only meaningful at province grain | persisted province identity (Phase 3), detritus (I1), carrion (I2), nectar (I4), freshwater connectivity graph (C6), barrier/corridor graph edits (H2–H5), fire/storm indices (F1/F3), allopatry & refugia history (H1/H6/F6) |

**Rule of thumb:** a physical layer produces it and it is spatially continuous → per-hex Vec; one planetary number → global scalar; it drops out of already-populated fields and is consumed in the same sweep → derive it inline, materialize no Vec; biology's dynamics produce/consume it as a stock, graph, or cross-tick history → province state. **Do not** create a per-hex Vec for something the dynamics only ever read at province grain, and do not materialize a Vec for a value (C3 upwelling, D1 depth band) computed and consumed inline.

### 0.2 The feedback-write patterns (Phase 6)

There are **two** classes, and choosing the wrong one silently discards the write. The O2 loop is the template for **only the first**.

**Class A — single-owner feedback field (biology is the sole writer).**
Model: `atmospheric_oxygen_fraction`, `microbial.rs:123`. No physical producer ever writes this field, so biology owns it outright:
- The WorldData field is authoritative; the `BiologyState` mirror (`state.o2_fraction`) is a cache synced on the same line.
- Read-modify-write of an increment: `world.field = (world.field + delta).clamp(..)`.
- One-tick-lagged: this tick's write is next tick's starting value.
- The O2 ramp is a pure additive constant (no RNG) — the safest possible shape.

Use Class A **only** when no producer touches the field. `atmospheric_oxygen_fraction` qualifies precisely because climate *reads* it but never writes it.

**Class B — producer-owned field (a physical layer recomputes it every tick).**
`co2_ppm`, `soil_depth_m`, and `soil_N` are each re-derived by their producer every tick — climate's `layer.rs::advance` mirrors `ClimateState.atmospheric_composition` onto WorldData; hydrology's `update_soil` both deposits and erodes soil every tick. A biology write to the WorldData mirror is overwritten next tick and the delta is lost. Therefore:
- Biology writes its delta into the **producer's authoritative state** — `ClimateState.atmospheric_composition.co2_ppm`, hydrology's soil accumulator — **never** the WorldData mirror.
- The producer folds the delta on its next `advance` and re-derives the mirror.
- Intra-tick order is pinned and documented at the field: biology deposits delta → producer.advance folds it and writes the mirror → downstream reads see the new value the following tick.

The O2 loop does **not** demonstrate Class B — it is a single-writer field, and presenting it as validating a two-writer pattern is exactly the trap. Every Phase-6 loop must first be classified A or B.

**Determinism (both classes):** increments are pure functions of already-committed state plus the keyed stream `rng.stream_at("biology.evolution", year.value())`. Never wall-clock, never an unkeyed RNG.

### 0.3 Conventions for a new WorldData field

1. **Define + default** in `genesis_core/src/data/mod.rs` (a `Vec<_>` sized to `cell_count()`, or a scalar with a documented default, e.g. CO₂ = 280.0). Add it to WorldData (de)serialization.
2. **Single writer of the WorldData field:** exactly one layer writes each WorldData field per tick, in that crate's `layer.rs::advance`, in pipeline order (climate: distance→wind→basins→currents; hydrology: solve→routing→groundwater→lakes→regime→soil→coastal→ice). For a Class-B feedback field (0.2), that one writer is the producer; biology contributes only through the producer's input channel, never by writing the field. For a Class-A field, biology is that one writer. Document the owner.
3. **Fold into `terrain_signature`** (`genesis_biology/src/layer.rs`) if biology consumes it and it is sparse or slow-changing — otherwise the `HEAVY_FIELD_STRIDE_YEARS = 5_000_000` stride + dirty-flag skip will freeze the heavy recompute and hide the input. **This is the single most repeated risk across Phases 1–5.**
4. **Rendering** (optional): add a view/color mapping only if the field is displayable and useful.
5. **Province/cross-tick state** lives in serialized `BiologyState`, not on WorldData, and must be deterministic for replay.

---

## Phase 1 — Consumer-only wins (no producer work)

Every field below is **verified populated by a producer**; the work is a pure new consumer in the biology crate. Fastest value, zero producer/serialization change. Every item must be folded into `terrain_signature` (0.3 step 3).

Note on B1: `temperature_range` is already *read* by biology (in `layer.rs::terrain_signature` and `richness.rs::compute_richness`); it is unused **for phenology**, not unread. The hazard here is double-counting the seasonal swing, not a first read.

| # | Field(s) read | Consumer site | One-line use |
|---|---|---|---|
| **B1** | `temperature_range` (already in signature) | shared seasonality term in `richness.rs` (see B3 note) | Growing-season modulation + specialist/generalist bias (Doc 09 §4.4/§6.1) — via the single seasonality→phenology term defined with B3, not a second swing penalty. |
| **C8** | `HydroFlags::WETLAND` + `SoilClass::Peaty` + `water_table_depth_m` | `biome.rs::assign_biomes` (+ `richness.rs`) | High-productivity wetland/marsh biome; shallow water table gives the support signal for its persistence. |
| **C9** | `HydroFlags::{SPRING,OASIS,KARST}` + `water_table_depth_m` | `richness.rs::disturbance`/`productivity_at` (+ province refugia tag) | Desert refugia / spring-fed habitat; water-table depth distinguishes true oases from dry flags. Ship the **habitat tag only** — the relict-lineage value is gated on Phase 3. |
| **D3** | `ocean_current_vec` | `province.rs::label_provinces`/`realm_of` neighbor loop | Weight marine province adjacency by advective connectivity (larval/plankton dispersal). **Unblocks C3.** |
| **E2** | `soil_class` (all 8 classes) | `richness.rs`; `population.rs::compute_guild_occupancy` | Substrate-specialist (edaphic) guild affinities. |
| **E3** | `soil_depth_m` | `richness.rs::productivity_at`; `population.rs::compute_biomass` | Cap tree/root standing biomass; buffer drought where soils are deep. |
| **F4** | `discharge_seasonality` + `HydroFlags::DELTA` + `SoilClass::Alluvial` | `richness.rs::disturbance` **and** `productivity_at` | Floodplain: flood-reset disturbance and nutrient-subsidy productivity — as **one balanced term**, never summed. |
| **F5** | `elevation_relief` + `|hydro_elevation_delta_m|` | `richness.rs::disturbance`; per-province variance in `province.rs` | Steep terrain = chronic disturbance; relief-variance → microhabitat richness bonus. |
| **C1-lake / E4-salt** | `salt_accumulated` (endorheic) + `SoilClass::Saline` + `WaterBody.salinity` (**lakes only**) | `richness.rs::disturbance`; `province.rs::realm_of` | Halophyte exclusion/endemism on real lake & endorheic salinity. **Marine salinity is a stub (`WaterBody.salinity` hardcoded 0.0 for oceans) → Phase 4 (C1).** |
| **Globals/§7.2** | `sea_level_m`, `global_temperature_c`, `glaciation_intensity` | `layer.rs::advance` / `microbial.rs` extinction hook | Global scalars drive mass-extinction / sea-level-regression triggers (read-only). |

**Verification (concrete):**
- Spatial coincidence: on a fixed seed, wetland biome hex-set equals the `WETLAND|Peaty` hex-set exactly (count > 0, set-equality assertion); flood disturbance is nonzero on exactly the `DELTA`/high-`discharge_seasonality` hexes.
- Monotonicity: holding climate constant, a deep-soil hex yields biomass ≥ a shallow-soil hex (E3), asserted on a two-hex fixture.
- Non-uniformity: marine province labels differ between a run with `ocean_current_vec` populated and one with it zeroed (D3).
- **No double-count (B1):** the phenology term must not re-apply swing already encoded in `terrain_signature`/`richness`. Ablation: disabling the new phenology term changes productivity, but enabling it does not multiply the seasonal penalty already present (assert bounded ratio on a fixture).
- **Stride test (critical):** flipping any consumed flag flips `terrain_signature` for that hex (unit test) — proves the input is not stale-skipped.
- Determinism: two runs, same seed → identical world hash.

**Risks:** double-counting (F4 flood = disturbance + subsidy in one model; B1 swing vs existing usage); sparse flags hidden by the 5My stride if not folded into `terrain_signature`; C9 relict value deferred to Phase 3.

**Size:** S–M each; whole phase ~M. No storage, no producer, no serialization change.

---

## Phase 2 — Derivable inputs (no new WorldData Vecs)

Everything here is computed inline from already-populated fields during the richness sweep or the province flood-fill. **No new WorldData Vecs.** Where an item introduces serialized `BiologyState` (C6), that is called out explicitly — the "no new WorldData Vecs" label constrains only WorldData storage, not BiologyState, and BiologyState carries a replay-determinism burden. Split into three independently buildable slices.

### Phase 2A — Light & atmosphere geometry

- **G1 — Photoperiod** (deps none): day-length from `grid` latitude + `planet.axial_tilt_degrees`. No day-of-year is available → use the annualized/solstice-extreme form + a polar-day/polar-night flag. **Consumer: a new light/photoperiod term on the *terrestrial* branch of `richness.rs::productivity_at`** (the terrestrial `else` branch has no light term today) plus phenological cueing. This is new terrestrial code, not an edit to the marine line.
- **G2 — Insolation integral** (deps **G1**): integrate solar-elevation × day-length over the declination sweep from `axial_tilt` + `solar_luminosity_relative_to_sol`. **On land**, it drives the new terrestrial light term (and must gate polar productivity so high-latitude land collapses realistically). **On the marine branch**, it *replaces* the crude `light = (lat).cos()` at `richness.rs:84`.
- **A3 — UV/insolation proxy** (deps none): lat + elevation + luminosity, optionally O₂-screened for pre-GOE high-UV surface. Consumer: `productivity_at` + `biome.rs` land-colonization gate. Real ozone UV is out of scope; this is an explicit proxy.
- **A4 — Air pressure + altitude hypoxia** (deps none): barometric closed form from `elevation_mean` + `planet.{surface_pressure_hpa,gravity_g}`, **including the altitude-O₂ term up front** — global `atmospheric_oxygen_fraction` × elevation band, which needs no new field. Consumer: `population.rs::compute_guild_occupancy` elevational body-plan cap. This fully specifies A4 here so nothing about elevational hypoxia is deferred; A2's altitude-hypoxia half is subsumed by A4 and is **not** a separate later item.

### Phase 2B — Climate & marine derivables

- **B2 — Aridity index** (deps none): P/PET from `precipitation` + `temperature_mean`. Consumer: `biome.rs::land_biome` (split savanna/grassland/forest past the single ~800 mm threshold); `richness.rs` drought stress. Only the aridity *index* is derivable — true intra-annual precip seasonality / dry-season length has no producer → Phase 4.
- **B3 — Growing-season / degree-days** (deps none): the **single authoritative seasonality→phenology model**, integrating a seasonal sinusoid from `temperature_mean` (mean) + `temperature_range` (amplitude). **B1 (Phase 1) and B3 share this one term** — B1 supplies the generalist/specialist bias, B3 the degree-day temp_factor and annual-vs-perennial gating; neither adds an independent swing penalty. Consumer: `productivity_at`.
- **B4 — Humidity / VPD** (deps none): Tetens saturation from `temperature_mean` × moisture proxy from `precipitation`/`distance_to_ocean_km`. Consumer: `biome.rs` desiccation gate on land-colonization traits.
- **B6 — Seasonal temperature-extreme envelope** (deps none): coldest/warmest month = `temperature_mean ∓ temperature_range/2`. **Restricted to the extinction/tolerance-envelope path** — `richness.rs::disturbance` + `microbial.rs::extinct_check` (killing-frost range limits) — and does **not** touch the B1/B3 productivity term, so the shared two fields are not triple-counted. Interannual & precip extremes → Phase 4.
- **D1 — Depth zones** (deps none): band `depth = water_level_m − elevation_mean` (computed at `richness.rs:85`) into photic ≤~200 m / mesopelagic / benthic / abyssal. **D1 owns depth-band *zonation* — routing photosynthetic guilds to photic, chemosynthetic/detritivore to deep, in `population.rs::cascade_order`.** It does **not** own the nutrient magnitude.
- **D2 (shelf half only) — Shelf vs pelagic** (deps none): shallow-over-margin vs deep-open; boost shelf productivity. **The reef-builder half is blocked on C2 (pH) + C5 (turbidity) → hold reefs until Phase 4.** Do NOT ship a warm+shallow-only reef proxy (it massively over-places reefs).
- **C5 — Turbidity / photic-zone depth** (deps none): from `river_discharge_m3_yr`, `HydroFlags::DELTA`, `distance_to_ocean_km`, `primary_productivity`. Feeds D1's photic band. Reads a biology output → order within the tick to consume last tick's committed value, avoiding same-tick staleness.
- **C3 — Nutrient / upwelling factor** (deps **D3**, Phase 1): current-divergence adjacent to coastlines. **C3 owns the nutrient *magnitude* term** of `marine_nutrient_factor`; the final factor composes as `depth_zonation(D1) × nutrient_magnitude(C3)` — D1 places guilds by depth, C3 sets richness. C3 is a **per-hex value derived inline** during the sweep (no Vec materialized), reconciling it with the 0.1 table. Only C3 sources nutrient magnitude; the old depth-only proxy is retired.

### Phase 2C — Static connectivity & barriers (province flood-fill seam)

Barrier-**cost adjacency edits only** — static topology that works without cross-tick lineage machinery. The vicariance *payoff* (endemism divergence over time) and H1/H6 are gated on Phase 3. Order: base barriers → dependent barriers → corridors.

- **H2 — Mountain barriers** (deps none): per-edge cost from `elevation_mean` gradient + `elevation_relief`, gating the `province.rs::label_provinces` same-province test (~line 141).
- **H3 — Ocean/strait barriers** (deps none): extend `realm_of` so water-separated terrestrial provinces are non-adjacent/high-cost; use `basin_id` + seaway width (Wallace's-Line vicariance).
- **H4 — Desert/aridity barriers** (deps **B2**): add an aridity threshold to the province zone key, **reusing B2's exact formula** to avoid two divergent aridity definitions.
- **H5 — Dispersal corridors** (deps **H2, H3**): inverse cost along river valleys, coastlines, land bridges, current streams. Same weighted-adjacency seam.
- **C6 — River/lake connectivity graph** (deps none): freshwater province adjacency from shared-drainage (`flow_direction`, `river_discharge_m3_yr`, `water_body_id`) instead of raw hex neighbors. **New serialized `BiologyState` graph** — rebuilt deterministically each heavy tick (like the province flood-fill); flagged here because it is BiologyState, not a WorldData Vec, and must iterate in sorted order for replay.

**Verification (concrete):**
- Closed-form unit tests: photoperiod = 24 h at the pole in polar summer; aridity = expected P/PET at a known (T,P); depth classifier assigns photic ≤ 200 m; the shared B1/B3 seasonality term reproduces a hand-computed degree-day integral on a fixture.
- **Terrestrial polar gate (G1/G2):** on a fixed seed, land productivity above 70° latitude drops below a pinned ceiling after G2 lands, while the **latitudinal diversity gradient stays emergent** — assert high latitudes are suppressed but not zeroed (regression on a diversity-by-latitude metric).
- **Nutrient composition (D1/C3):** assert `marine_nutrient_factor` equals `depth_zonation × nutrient_magnitude` on a fixture; a coastal upwelling hex outscores an equal-depth non-divergent hex (C3), and guild routing follows the depth band (D1) independent of that magnitude.
- Barrier tests: introducing H2/H3 raises the province-count/endemism metric on a fixed map; H5 corridors lower it; removing them returns the pinned baseline.
- Invariant: WorldData serialized size unchanged (no new Vecs); C6 BiologyState serializes and round-trips deterministically.
- Determinism: flood-fill and C6 build iterate in sorted order → identical labels across two runs.

**Risks:** double-counting latitude (temp already encodes `cos(lat)`; B6 vs B1/B3 seasonality); C3 single-sourcing nutrient magnitude while D1 owns zonation; conditional flood-fill cascading into richness/occupancy/view — thresholds only, never RNG. Each slice (2A/2B/2C) is buildable and shippable on its own.

**Size:** 2A = M; 2B = M; 2C = M (C6 is the L within it).

---

## Phase 3 — Persisted province identity + geography→speciation subsystem

**This is a prerequisite subsystem, scheduled work — not an open question.** Today provinces are rebuilt from scratch each heavy tick with no persisted identity, and `speciation.rs` is a pure seed function with no geography-driven triggers. Every allopatry/refugia item (H1, H6, F6) and the vicariance *payoff* of the H2–H5 barriers depend on machinery that does not exist. The field reads those items need are trivial; **this subsystem is the actual (L-sized) work**, and it must land before them.

Deliverables:
1. **Persisted province identity:** assign stable province IDs that survive across heavy ticks, matching each tick's fresh flood-fill result back to prior identities (by hex overlap / plate composition), stored in serialized `BiologyState`. Deterministic and replay-stable (sorted iteration, keyed decisions only).
2. **Geography→speciation triggers:** wire `speciation.rs` to fire lineage bifurcation when a persisted province splits/severs across ticks, and to scale speciation payoff/drift by province geometry. Draws only from `rng.stream_at("biology.evolution", year)`.
3. **Cross-tick history channels:** per-province plate-composition history (for H1), isolation/area history (for H6), and prior-ice history (consumed by F6 in Phase 5).

On top of this subsystem, land:
- **H1 — Plate-identity allopatry** (deps subsystem): tag each province's `plate_id` composition at flood-fill, fire bifurcation when a plate block severs a persisted province across ticks.
- **H6 — Island isolation / area scaling** (deps subsystem, **H3**): score terrestrial `province.hexes.len()` × hex area + isolation distance, **plus the freshwater-island analog `WaterBody.{area_km2, volume_km3}`** for lake endemism (Part-3 note 1); scale speciation payoff/drift down for small isolated provinces and small lakes.
- **Vicariance payoff of H2–H5:** the static barriers from Phase 2C now produce *divergence over time* rather than only static labels.

**Verification (concrete):**
- Identity stability: on a fixed seed, a province with unchanged geometry keeps its ID across N heavy ticks (assertion); a scripted split produces exactly two child IDs mapped to the parent.
- Deterministic speciation: a scripted plate severance at tick T fires bifurcation at tick T on both runs of the same seed, with identical lineage IDs.
- H6: a small isolated province and a small lake both show elevated endemism vs an equal-climate connected province on a fixed fixture; lake score responds to `area_km2`/`volume_km3`.
- Replay: full serialize/deserialize of the identity + history state round-trips to an identical world hash.

**Risks:** this is the largest single lift in 09B and the hinge for all biogeography; cross-tick history must be serialized and keyed or replays diverge. Keep the matching heuristic deterministic and cheap.

**Size:** L (subsystem) + M (H1/H6 on top).

---

## Phase 4 — New physical fields + producers

Grouped by owning layer. **Cheap = surface an existing value; Expensive = genuinely new physics.**

### Climate

**Cheap (surface existing):**
- **A1 — CO₂ onto WorldData** (deps none): the scalar already lives in `ClimateState.atmospheric_composition.co2_ppm` (default 280.0, driven by `carbon::update_carbon_cycle`) but is not on WorldData. Add one global `f32`, written in `genesis_climate/src/layer.rs` each tick. Consumer: a CO₂ term in `richness.rs::productivity_at`. **Apply as a productivity ceiling/limitation modifier, not additive energy** — CO₂ already drives productivity via greenhouse→temperature, and a naïve additive term double-books. (Drawdown write-back is Phase 6.)

**Expensive (new physics):**
- **B2-seasonality — intra-annual precipitation seasonality / dry-season length** (deps none): new per-hex field + climate producer (the aridity *index* shipped in Phase 2; dry-season length did not). Consumers: deciduousness, drought guilds.
- **B5 — Cloud cover** (deps none): new per-hex field + producer (real physics, or a precip/humidity proxy). Consumer: attenuate insolation/UV (A3/G), cloud-forest niche. A proxy risks double-counting B2/B4.
- **B6-interannual — true temperature & precipitation extremes** (deps **B2-seasonality**): new fields for event/interannual extremes beyond the Phase-2 seasonal envelope.

### Hydrology

- **C1 — Marine salinity** (deps none): **corrected from exists-unread — `WaterBody.salinity` is hardcoded 0.0 for every ocean** (`lakes.rs:449,628`; `solve.rs`). New producer: evaporation-minus-precip balance + enclosed-basin concentration + river-mouth dilution, written as a per-hex marine field. Consumer: salinity axis on `province.rs::realm_of`, brackish/estuary nurseries. Building the marine barrier on the current zeros yields a false uniform ocean — a genuine gap.
- **C7 — Tidal range / intertidal zonation** (deps none): **corrected — `coastal.rs:20 pub fn tidal_range_m(moon_count)` already exists and `update_coastal` applies it every tick.** The *cheap* half is to surface that existing static global scalar onto WorldData (S). The genuinely new work is **per-hex intertidal zonation** (a coastal-band field derived from tidal range + shore relief), consumed for tidal-flat/mangrove guilds. The `HydroFlags::{ESTUARY,WETLAND,DELTA}` those tides set are already consumed in Phase 1 (C8).
- **E5 — Soil age / weathering stage** (deps none): new per-hex field in `soil.rs`, incrementing with quiescence, **reset to 0 by resurfacing events** (volcanism/F2, glaciation, major erosion). Replaces the Igneous/ice proxy in `richness.rs::disturbance` (lines 111–117). Blocks E1. Must fold into `terrain_signature` or the stale-skip freezes ages.
- **E1 — Soil N / P** (deps **E5**): per-hex N + P from weathering + parent material (P declines on old leached substrates, keyed off E5). Consumer: N raises the energy ceiling, P caps it in `productivity_at`. **`soil_fertility` already bundles nutrients — N/P must not stack on it.** (N-fixation write-back is Phase 6.)
- **C2 — Water pH / alkalinity** (deps **A1**): carbonate-chemistry producer forced by WorldData CO₂ (A1) + SST; new per-hex marine field. Gates reef-builders. Unblocks the D2 reef half.
- **C4 — Dissolved O₂ in water** (deps **C1**, **C3**): **C4 owns the marine water-column O₂ field** — solubility from `temperature_mean` + salinity (C1) plus a stratification/dead-zone term coupling productivity (C3). This is the resolution of the former A2↔C4 cycle: A2's marine part is **not a separate field — it reads C4**, and A2's altitude part already folded into A4 (Phase 2A). The only remaining edges are C4→{C1, C3}, which is acyclic; schedule C1 and C3 before C4.
- **E4 — Geochemical toxicity (metals/serpentine/acid-sulfate)** (deps none): salinity axis already in Phase 1; the rest needs a new field + tectonics/hydrology producer. `bedrock_type` does **not** distinguish ultramafic/serpentine today — that provenance must be added.

### Tectonics/soil

- **F2 producer half — volcanism recency field** (deps none): new per-hex `years_since_volcanic_resurfacing` + a global LIP event scalar in `genesis_tectonics/{volcanism.rs, hotspots.rs}` (they write no recency today). Consumed in Phase 5.

### Light

- No new-field work — UV/insolation/photoperiod are Phase-2A derivations; real ozone-UV physics is out of scope.

**Verification (concrete):**
- Field populated & in range: `world.co2_ppm == ClimateState.co2_ppm` each tick (A1 mirror); **marine salinity variance > 0 across oceans** on a fixed seed (directly refutes the C1 stub); N/P within agronomic ranges; per-hex intertidal band nonzero only on low-relief tidal shores (C7).
- Event resets: E5 soil age → 0 on a scripted volcanic/ice/erosion event (unit test); pH responds monotonically to a scripted CO₂ step (C2); C4 dissolved-O₂ falls as SST rises at fixed salinity (unit test).
- Acyclic schedule: C4 unit test constructs C1 and C3 inputs first, then asserts C4 output — no reference to a marine-A2 field.
- Determinism: producer output identical across two runs; basin iteration sorted (C1).
- No-double-count (A1): ablation test — the CO₂ ceiling term does not raise productivity already attributable to the greenhouse-temperature path.

**Risks:** producer cost (C1/C2/C4/B5/E5 are the heavy lifts); C2 pulls in the pH→carbonate→Limestone feedback (Phase 6) that can oscillate; E5 stale-skip freeze if not in `terrain_signature`.

**Size:** A1 = S; C7-surface = S; C1/E4/C7-zonation = M; C2/C4/E1/E5/B5/B6-interannual = L.

---

## Phase 5 — Disturbance regimes

Depend on Phase-2/4 inputs (and Phase 3 for the cross-tick items). Province-resolution (Doc 09 §5.1). Determinism via the keyed `rng.stream_at("biology.evolution", year)` stream or a deterministic climatological rate — never per-event RNG.

- **F5, F4** — already delivered in **Phase 1** (relief instability, flooding regime were exists-unread).
- **F1 — Fire regime** (deps **B2** aridity, Phase 2; reads `world.biomass` fuel): per-province index = aridity × fuel × annualized ignition constant. **`world.biomass` is a biology output — same same-tick-staleness hazard flagged for C5.** Fire must read **last tick's committed biomass** (or be strictly ordered after biomass within the tick); pin the choice. Consumer: wire into **one** authoritative term feeding both `richness.rs::disturbance` succession-reset and `biome.rs` grassland/savanna maintenance — do not compound. **Note the fire→biome→biomass→fire cross-tick loop as a stability risk** (see the loop-stability test below).
- **F3 — Storm/cyclone exposure** (deps: **climate storm-potential producer field, Phase 4**): per decision #1, *climate* owns the cyclogenesis field (SST `temperature_mean` over ocean + `wind_speed_m_s` + latitude), projected onto adjacent coasts. Biology *reads* it here for windthrow disturbance + salt-spray zonation; biology's influence flows back only through the Phase-6 climate feedbacks (evapotranspiration/albedo → circulation), never a second definition.
- **F2 — Volcanism recency / LIP** (deps **F2 producer field, Phase 4**): per-hex recency feeds `disturbance()` (superseding, not stacking on, the static Igneous term); the global LIP event fires a Deccan/Siberian mass-extinction pulse via a milestone hook in `microbial.rs`/`layer.rs::advance`, parallel to the GOE gate.
- **F6 — Glacial cycling / ice history** (deps: `glaciation_intensity`, `ice_load_m`, `CARVED_TROUGH`, `SoilClass::Loess` — all populated but unread — **and the Phase-3 subsystem**): *extend* the existing `ice_mask` disturbance term (do not add a parallel one). The refugia/speciation-pump dynamic uses the **Phase-3 persisted prior-ice history**, not a new physical field.

**Verification (concrete, deterministic — no correlational checks):**
- Fire: on a fixed seed+scenario, the fire index exceeds a pinned threshold in flagged arid high-fuel provinces and is below it elsewhere; a province held at savanna does not transition to forest over N ticks (biome-stability assertion). Loop-stability: over M ticks the fire↔biomass values stay within pinned bounds (no divergence/oscillation).
- F2 LIP: the scripted event drops lineage count at the event tick and emits the milestone **exactly once**.
- F6: on a fixed glacial-cycle scenario, a cycle at tick T produces ≥ N speciation events in flagged refugia provinces (pinned assertion, not correlation).
- F3: storm index is exactly zero outside a defined latitude/SST band and positive on flagged tropical coasts (pinned band assertion).
- Determinism: event timing reproducible across two runs.

**Risks:** double-counting (fire = disturbance + biome maintenance; F2 recency vs static Igneous; F5/H2 both read relief); F2 is the biggest lift (producer + event pathway); cross-tick history (F6) must be serialized and keyed via Phase 3 or replays diverge.

**Size:** F1/F3 = M; F6 = M (on the Phase-3 subsystem); F2 = L (producer + event).

---

## Phase 6 — Feedback loops (biology → physical writes)

All one-tick-lagged. **Classify each loop A or B first (0.2); getting it wrong silently discards the write.** Each loop requires its target field/state to exist. **Conservation:** every loop redistributes mass/energy, never creates it. Ship last, one loop at a time, each behind its own stability test.

| Loop | Class | Writes into | Depends on | Note |
|---|---|---|---|---|
| **A1 — CO₂ drawdown** | **B** | `ClimateState.atmospheric_composition.co2_ppm` (the producer's authoritative state), **not** `world.co2_ppm` | A1 field (Phase 4) | Climate re-mirrors onto WorldData next tick; a mirror-only write is lost. Pin order. |
| **E1 — N-fixation** | **B** | hydrology's soil-N accumulator (producer state), **not** the WorldData mirror | E1 (Phase 4) | Forward-in-time; producer folds the delta. |
| **E3 — Soil deepening** | **B** | hydrology's soil accumulator | E3 read (Phase 1) | `update_soil` already deposits & erodes each tick — writing the mirror fights it and oscillates; deposit the biotic increment into the accumulator so hydrology reconciles it. |
| **C2/D2 — Carbonate → Limestone + fertility refinement** | **B** | tectonics `bedrock_type` → `Limestone` via reef precipitation, **and** the marine bio-deposit refinement of `soil_fertility`/carbonate (Doc 09 §11.3) | C2 (Phase 4) | Folds in the backlog "biology refines fertility" item alongside the Limestone write; bound the rate — pH→carbonate→tectonics can oscillate. |
| **B4 — Evapotranspiration / precip recycling** | **B** | a climate precip-recycling input seam | B4 (Phase 2) | Forest→precip loop (Doc 09 §11.7). |
| **Albedo / biogenic greenhouse** | **B** | climate forcing input from biome cover | biome (existing) | Lowest priority; largest stability risk. |

Note: `atmospheric_oxygen_fraction` (already shipped) is the lone **Class A** field — no producer writes it, so biology owns the WorldData field directly. Every loop above is Class B and must target its producer's state.

**Verification (concrete):**
- **Feedback persistence (the key regression guard):** after a biology drawdown write at tick N, assert the reduced CO₂ is reflected in the **authoritative `ClimateState` value read at tick N+1** — not merely the same-tick WorldData mirror (a mirror-equality test alone would pass even if the delta is overwritten). Same shape for soil-N and soil-depth: the biotic increment survives into the producer's next-tick output.
- Mass balance: carbon/energy conserved across the write (redistribution only), asserted on a fixture.
- Long-run stability: values bounded over M ticks, no divergence/oscillation (especially E3, C2).
- Determinism: two-run identical world hash.

**Risks:** cross-tick coupling determinism; oscillation where two producers touch one field (E3, C2); double-counting energy. Highest-risk phase — one loop at a time.

**Size:** A1 drawdown = M; others = M–L.

---

## Resource / food-web substrates (I1–I4)

**Province/population-scoped `BiologyState`, not per-hex WorldData Vecs** (Doc 09 §5.1). Home: `population.rs` (cascade/food-web) + `guild.rs` roster + fields on `BiogeographicProvince`.

- **I3 — Structural producer substrates** (deps none, **derivable**): fractionate `primary_productivity` into canopy/ground-cover/plankton by a `biome`→structure lookup, inline at guild assignment. Gates browser/grazer/granivore differentiation. **Ship this first of the four** — pure function of populated fields, no storage, no cross-tick state.
- **I1 — Detritus pool** (deps none): new `detritus_stock` on `BiogeographicProvince`, fed by an NPP turnover fraction, drawn down at a temperature/moisture-dependent decomposition rate. Gives the decomposer guild a real energy base. **Redistributes NPP, adds no net energy.** Connects to peat/soil-carbon (E3) later.
- **I2 — Carrion pool** (deps: **heterotroph mortality model — does not exist**): `world.biomass` is a static producer proxy; there is no consumer standing-biomass or death dynamics to source a mortality flux. Requires building the Doc 09 §5.2 mortality half first. **Largest scope risk — do not start until consumer dynamics exist.**
- **I4 — Nectar / pollination rewards** (deps: **new morphospace trait — does not exist**): switches on only when a producer lineage acquires a flowering/angiosperm trait absent from the morphospace. Requires a co-evolutionary trait gate — **outside 09B's WorldData scope**; file against Doc 09 morphospace work.

**Verification:** I3 — herbivore guild occupancy differentiates by structure class on a fixture. I1 — detritus steady-state ≈ NPP×turnover/decomp (analytic check); decomposer occupancy nonzero iff detritus > 0; trophic energy conserved. I2 — gated test asserting it stays zero until a mortality model lands. I4 — gated on trait presence in the ledger. Determinism: trait-emergence and any mortality draw from the keyed biology stream only.

**Size:** I3 = M; I1 = M; I2 = L (drags in consumer dynamics); I4 = L (drags in morphospace).

---

## Recommended FIRST slice (Phase 1, 4 items)

Highest-value, zero-dependency, zero-producer reads that also **establish the two cross-cutting conventions** (terrain_signature folding + province-vs-hex) so every later phase inherits them:

1. **D3 — `ocean_current_vec` → marine dispersal adjacency.** Unblocks C3 and the marine track; exercises the province flood-fill seam.
2. **F4 — flooding regime (`discharge_seasonality` + `DELTA` + `Alluvial`).** Exercises the "one balanced term for disturbance + subsidy" discipline that recurs in F1/F5/F6.
3. **E3 — `soil_depth_m` → biomass cap.** Simple, high-value, per-hex, and sets up the E3 Phase-6 write-back later.
4. **C8 — wetland biome (`WETLAND` + `Peaty` + `water_table_depth_m`).** First new biome; exercises `biome.rs::assign_biomes` and the sparse-flag → `terrain_signature` fold.

Together these touch `richness.rs`, `biome.rs`, `province.rs`, and `layer.rs` (terrain_signature) — every seam later phases build on — while shipping visible ecological change at zero producer/feedback risk. Add **B1** (temperature_range phenology, via the shared seasonality term) as a stretch 5th if capacity allows.

---

## Resolved design decisions (owner-confirmed)

1. **Storm exposure (F3) → climate-owned, biology-influenced.** Climate produces the cyclone/storm-potential field (SST + wind + latitude); biology *reads* it in Phase 5, and biology's influence on it flows through the Phase-6 climate feedbacks (vegetation → evapotranspiration/albedo → the circulation that spawns storms), never a second biology-internal storm definition. Move F3's producer to **Phase 4 (climate)**; F3's consumer stays in Phase 5.

2. **CO₂ (A1) → most-realistic = dual pathway, no double-count.** CO₂ acts on productivity through **two physically distinct channels**: (a) the greenhouse→temperature path, already carried by `temperature_mean`→`temp_factor`; and (b) **direct CO₂ fertilization** — CO₂ is a photosynthesis *substrate*, so it enters productivity as a **co-limitation term** (Liebig minimum with light/water/nutrients), independent of temperature. Model both; the anti-double-count rule is that the fertilization term must be the *substrate* limitation only, never re-inject the temperature effect. (Drawdown write-back = Phase 6, Class B.)

3. **Reefs → generalized chemistry-driven bioconstructors, NOT Earth-carbonate-only.** See the new section below. We do **not** gate reefs on Earth's warm+shallow+clear+alkaline carbonate window; instead the ocean-chemistry panel decides *which biomineral* frames the reef, and bioconstructors emerge with chemistry-appropriate mineralogy (carbonate / silica / exotic). This upgrades C2 from "water pH" to an ocean-chemistry panel and D2's reef half to "place the bioconstructor whose mineral the local chemistry favors."

4. **Phase 3 stays inside 09B — implement now.** The persisted-province-identity + geography→speciation subsystem is in scope here (not a Doc 09 follow-on); it is the hinge for H1/H6/F6 and the vicariance payoff of H2–H5.

---

## Generalized biomineralization / bioconstructor substrate (decision #3)

**Principle.** A "reef" is any wave-resistant biogenic *framework*. Earth builds them from carbonate (coral, coralline algae) in warm/shallow/clear/alkaline water — but that is one point in a space. Framework life can precipitate other minerals: **silica** (hexactinellid glass-sponge reefs — real, in cold *deep* water on Earth), **aragonite vs calcite** carbonate (set by seawater Mg/Ca — Earth alternated "aragonite seas" and "calcite seas" over the Phanerozoic), and, on alien/Archean-like worlds, **iron / other** biominerals in reducing high-Fe oceans. So the hex-state job is to expose the water chemistry that decides *which* mineral is favored; biology (separate track) evolves a **bioconstructor guild whose mineral is the argmax of the local saturation panel**.

**Hex-state deliverable — the ocean-chemistry panel (upgrades C2, Phase 4 hydrology/ocean-chem producer):**
- **Carbonate saturation Ω** — from pH/alkalinity + SST + pressure(depth) + [Ca²⁺]; forced by CO₂ (A1) + weathering influx. High Ω → carbonate framework.
- **Dissolved silica [Si]** — from silicate weathering + hydrothermal input, drawn down by silica biomineralizers. High Si + cold/deep → silica framework.
- **Mg/Ca ratio** — sets aragonite-vs-calcite mineralogy; driven by seafloor spreading rate / hydrothermal exchange (a real Earth control we already have plate machinery for).
- **Dissolved Fe (+ redox)** — coupled to global O₂ (already have `atmospheric_oxygen_fraction`) + local dissolved O₂ (C4); a reducing, Fe-rich ocean opens exotic biomineralization (banded-iron-analog framework).

**Emergence rule (biology track consumes this):** a bioconstructor guild forms where (a) fuel exists — photic-zone light/productivity for photosymbiotic builders, or chemosynthesis in the deep; (b) sediment/turbidity (C5) is low enough; (c) *some* mineral in the panel is supersaturated. The framework **mineral = argmax of the saturation panel**, so the guild is never forbidden by "wrong" chemistry — it just builds with what's available (or none, if nothing precipitates). Determinism and the trait gate live on the biology side.

**Feedback (Phase 6) generalizes** from "carbonate → `BedrockType::Limestone`" to "biomineral → sediment": carbonate→limestone, silica→chert/BIF-analog, etc., each drawing down its own ion. Bound the rates (chemistry↔sediment can oscillate).

This makes reefs a first-class *alien-capable* feature and turns ocean chemistry into a driver of biotic novelty — exactly the intent of the `novelty_temperature` dial.
