# 09B — Per-hex state inputs for biology: current vs missing (handoff)

**Document Type:** Tier 3 — Handoff spec / expansion backlog
**Status:** Draft for a dedicated hex-state agent (biology stays a separate track).
**Owner:** Brax Johnson
**Purpose:** Give the agent expanding per-hex world state a complete map of (a) what biology reads today and (b) every environmental input that *should* drive evolution but is missing or unread — so hex substrate can grow while the biology track consumes it.

**Scope.** This document inventories the per-hex and global state on `WorldData` (`crates/genesis_core/src/data/mod.rs`), records exactly which fields the biology crate reads today (verified against `crates/genesis_biology/src/{richness,biome,province,view,population,biogenesis,microbial,layer}.rs`), then enumerates — exhaustively — the hex-level environmental state that life should respond to but that is missing or underused. Field names below are the literal `WorldData` identifiers.

---

## Part 1 — What exists on `WorldData` today

`WorldData` is a Struct-of-Arrays: every per-hex field is a `Vec<_>` indexed by `HexId.0`, sized to `grid.cell_count()`. Globals are bare scalars. "Biology reads?" is the ground truth from the current biology crate.

### 1.0 Infrastructure

| Field | Type | Biology reads? |
|---|---|---|
| `grid` | `HexGrid` | **Yes** — `grid.center_lat_lon(hex)` for latitude→light/insolation and biogeographic region; `grid.neighbors(hex)` for province flood-fill, dispersal adjacency, and vent volcanic-adjacency. |
| `parameters` | `WorldParameters` | **Yes** — `core.seed`, `core.biology.{complexity_pressure, novelty_temperature}`, `core.time.*`. Not per-hex. |
| `current_year` | `WorldYear` | **Yes** — gates ledger extancy, event years, tree snapshots. |

### 1.1 Physical Layer (Layer 0 — tectonics/base climate)

| Field | Type | Biology reads? |
|---|---|---|
| `elevation_mean` | `Vec<f32>` | **Yes** — land/wet test (`water_level_m > elevation_mean`), marine depth (`water_level_m − elevation_mean`), vent deep-ocean depth, realm classification. |
| `elevation_relief` | `Vec<f32>` | **No.** Never read. (Vertical range within a hex — topographic heterogeneity.) |
| `bedrock_type` | `Vec<BedrockType>` | **Yes** — `Igneous` drives the volcanic disturbance term (`richness.rs`) and the vent volcanic-adjacency proxy (`biogenesis.rs`). All other variants ignored. |
| `plate_id` | `Vec<PlateId>` | **No.** Never read, despite Doc 09 §6.4 naming plate splits as the primary **allopatry** signal. |
| `temperature_mean` | `Vec<f32>` | **Yes** — Gaussian `temp_factor` in productivity; marine + terrestrial. |
| `temperature_range` | `Vec<f32>` | **Yes** — `stability_factor` in `richness.rs` (low interannual range breeds specialists). |
| `precipitation` | `Vec<f32>` | **Yes** — `moisture_factor` in productivity; forest-vs-grassland threshold in `biome.rs`. |
| `habitability` | `Vec<f32>` | **No.** Civ-oriented composite; biology computes its own productivity/richness. |

### 1.2 Climate Layer (populated by genesis_climate)

| Field | Type | Biology reads? |
|---|---|---|
| `wind_direction_rad` | `Vec<f32>` | **No.** |
| `wind_speed_m_s` | `Vec<f32>` | **No.** |
| `ocean_current_vec` | `Vec<[f32;2]>` | **No.** Never read, though currents drive upwelling/nutrients and ocean-anoxia extinction (Doc 09 §7.2). |
| `distance_to_ocean_km` | `Vec<f32>` | **No.** (Continentality proxy; unused.) |
| `basin_id` | `Vec<BasinId>` | **No.** Ocean-basin id; biology re-derives marine connectivity via its own flood-fill. |
| `climate_regime` | `Vec<ClimateRegimePlaceholder>` | **Yes** — `land_biome()`; `biome_zone()` province subdivision. |

### 1.3 Hydrology & Soil Layer (Doc 08)

| Field | Type | Biology reads? |
|---|---|---|
| `water_level_m` | `Vec<f32>` | **Yes** — the land/wet/depth backbone. |
| `water_body_id` | `Vec<WaterBodyId>` | **Yes** — keyed into `water_bodies` for Lake/SaltLake → Freshwater realm and Lake biome. |
| `flow_direction` | `Vec<Option<Direction>>` | **No.** River drainage graph — unused, though Doc 09 §10.1a makes rivers dispersal corridors. |
| `river_discharge_m3_yr` | `Vec<f32>` | **No.** Riparian/freshwater productivity and corridor strength — unused. |
| `discharge_seasonality` | `Vec<f32>` | **No.** Flood-pulse / disturbance regime — unused. |
| `water_table_depth_m` | `Vec<f32>` | **No.** Sub-surface moisture, wetland/oasis support — unused. |
| `salt_accumulated` | `Vec<f32>` | **No.** Salinity/toxicity — unused. |
| `ice_mask` | `Vec<bool>` | **Yes** — ice disturbance term; Ice-cap biome; the new non-ice microbial-floor gate. |
| `hydro_flags` | `Vec<HydroFlags>` | **No.** Entire set unused: `SPRING, OASIS, KARST, ESTUARY, FJORD, EPHEMERAL, WETLAND, SEA_ICE, CARVED_TROUGH, DELTA` — ready-made habitat/nursery signals (Doc 08 §10.2, §11.2–11.3). |
| `soil_depth_m` | `Vec<f32>` | **No.** Only `soil_fertility` read; rooting-zone depth ignored. |
| `soil_fertility` | `Vec<f32>` | **Yes** — terrestrial productivity term. |
| `soil_class` | `Vec<SoilClass>` | **No.** `Alluvial/Loess/Volcanic/Calcareous/Sandy/Peaty/Saline/Loamy` — substrate identity unused; only the derived fertility scalar consumed. |
| `hydro_elevation_delta_m` | `Vec<f32>` | **No.** Internal hydrology→tectonics coupling. |
| `continental_crust` | `Vec<bool>` | **No.** |
| `ice_load_m` | `Vec<f32>` | **No.** GIA coupling. |
| `gia_rebound_applied_m` | `Vec<f32>` | **No.** Diagnostic. |
| `water_bodies` | `BTreeMap<WaterBodyId, WaterBody>` | **Partly** — only `WaterBody.kind`. `salinity, area_km2, volume_km3, surface_m, outlet` **unused**, though lake area/volume/age is the island-analog endemism signal (Doc 09 §10.1a). |

### 1.4 Global Physical Scalars

| Field | Type | Biology reads? |
|---|---|---|
| `sea_level_m` | `f32` | **No.** Sea-level-shock extinction (shelf drowning/exposure, §7.2) has no input. |
| `global_temperature_c` | `f32` | **No.** Rapid-climate-shock extinction has no input. |
| `glaciation_intensity` | `f32` | **No.** Global glacial regime unused (only per-hex `ice_mask`). |
| `atmospheric_oxygen_fraction` | `f32` | **Yes — read AND written.** `microbial.rs` accumulates O₂ from oxygenic photosynthesis and reads it back for the Great-Oxygenation / eukaryogenesis / multicellularity gates. The one real biology↔atmosphere loop wired today — the template for all §11 feedbacks. |

### 1.5 Biological Layer (biology owns these — mostly outputs)

| Field | Type | Biology reads? |
|---|---|---|
| `biome` | `Vec<BiomeId>` | **Write + read** — `assign_biomes`; barren check + `view.rs`. |
| `biomass` | `Vec<f32>` | **Write + read** — `compute_biomass`; surfaced by `view.rs`. |
| `biotic_richness` | `Vec<f32>` | **Write + read** — R scalar; `compute_richness` → guild occupancy. |
| `primary_productivity` | `Vec<f32>` | **Write + read** — energy base → R + biomass. |
| `province_id` | `Vec<ProvinceId>` | **Write** — `label_provinces` (registry is the live read path). |
| `dominant_lineage` | `Vec<LineageId>` | **Declared, not populated.** No writer; `view.rs` resolves dominant lineage from the ledger. |
| `fertility` | `Vec<f32>` | **No** (by biology). Marine bio-deposit accumulator; tectonics increments, hydrology folds into `soil_fertility`. Doc 09 §11.3: biology *should* refine it (reef carbonate) — not yet wired. |

### 1.6 Civilizational Layer

| Field | Type | Biology reads? |
|---|---|---|
| `population` | `Vec<u64>` | **No.** (Human population, not biological biomass.) |
| `settlement_id` | `Vec<Option<SettlementId>>` | **No.** |
| `nation_id` | `Vec<Option<NationId>>` | **No.** |

### 1.7 Summary of the current biology read-set

Biology reads only: `grid`, `parameters`, `current_year`, `elevation_mean`, `bedrock_type` (Igneous only), `temperature_mean`, `temperature_range`, `precipitation`, `climate_regime`, `water_level_m`, `water_body_id` (+`water_bodies[].kind`), `ice_mask`, `soil_fertility`, `atmospheric_oxygen_fraction`, and its own output arrays. **Everything else on `WorldData` is invisible to life.** Part 2 catalogs that gap.

---

## Part 2 — What is MISSING or underused (the expansion backlog)

Each item gives **what it is**, **why it matters for evolution/selection**, **source** (doc/§ or "new"), and **how biology would use it**. *(field exists, unread)* already sits on `WorldData` and only needs a consumer; *(new)* needs a new field plus a producer in the owning physical layer.

### A. Atmosphere & chemistry

**A1. Local / global CO₂ concentration** *(new — currently only in `ClimateState.atmospheric_composition.co2_ppm`, not on `WorldData`)*
- Why: CO₂ is a direct photosynthesis substrate — the productivity ceiling and carbon-drawdown feedback both hinge on it; high-CO₂ greenhouse worlds run hotter and more productive.
- Source: Doc 07 §11.1, Doc 09 §5.2 ("climate + soil + water + **CO₂**"), §11.2.
- Use: add a CO₂ term to `primary_productivity`; write back a drawdown delta (photosynthesis + organic burial) for climate one tick later.

**A2. Depth/altitude-resolved O₂** *(field exists as a single global scalar)*
- Why: real selection cares about *local* O₂ — stratified anoxic ocean bottoms, high-altitude hypoxia; anoxic basins crash marine guilds.
- Source: Doc 09 §3.3, §7.2, §11.1.
- Use: gate aerobic guilds locally; flag low-O₂ marine provinces as anoxia-extinction-prone.

**A3. UV / insolation flux** *(new)*
- Why: UV is lethal to unshielded life and was first-order before an ozone layer — it kept early life aquatic/subsurface and later selected for UV screens; insolation sets the photosynthetic budget.
- Source: Doc 07 §4.2; Doc 09 §3.4; new for explicit UV.
- Use: gate land colonization on tolerable surface UV; scale photic-zone productivity by insolation.

**A4. Air pressure / altitude hypoxia** *(new)*
- Why: high-altitude hypoxia limits large-bodied/high-metabolism life and sets an elevational range limit.
- Source: new (Doc 07 §4.3 lapse-rate).
- Use: elevational cap on body plans/metabolic tiers; a montane-endemism driver.

### B. Climate detail (beyond mean T / mean precip)

**B1. Temperature seasonality** *(field exists, partly used)* — `temperature_range` read for stability, not phenology.
- Why: seasonality selects for dormancy, migration, storage, fast generations; sets the growing season.
- Source: Doc 07 §4.8; Doc 09 §4.4, §6.1.
- Use: modulate `generations_elapsed` and specialist-vs-generalist bias by seasonal swing.

**B2. Precipitation seasonality / aridity index** *(new; only annual `precipitation`)*
- Why: a savanna and a rainforest can share annual totals but differ wholly; dry seasons drive deciduousness, fire, drought guilds.
- Source: Doc 07 §9.4, §10; Doc 09 §4.6.
- Use: refine forest/savanna/grassland beyond the single 800 mm threshold; drive fire regime (F1).

**B3. Growing-season length / degree-days** *(new)*
- Why: the single most predictive terrestrial-productivity variable; short seasons cap what can complete a life cycle.
- Source: new (from Doc 07 §4.1 + §4.8).
- Use: augment the Gaussian `temp_factor` with a degree-day term; gate perennial vs annual strategies.

**B4. Humidity / vapor-pressure deficit** *(new)*
- Why: desiccation stress governs land colonization and terrestrial water economy; drives the evapotranspiration feedback.
- Source: Doc 09 §3.4, §11.7.
- Use: gate land-colonization traits; feed forest→precipitation recycling.

**B5. Cloud cover** *(new)* — attenuates insolation/UV (A3), buffers extremes; cloud forests are a distinct niche. Doc 07 circulation; Doc 09 §11.

**B6. Temperature / precipitation extremes** *(new; means only today)*
- Why: tolerance is set by extremes, not means; a killing frost/heatwave sets range limits and extinction risk.
- Source: Doc 09 §7.1, §7.2.
- Use: per-lineage tolerance envelopes → background-extinction hazard and biome-lag stress zones (§4.6).

### C. Hydrology & water chemistry

**C1. Salinity (marine + lake)** *(field exists, unread)* — `WaterBody.salinity`, `salt_accumulated`.
- Why: marine/brackish/freshwater is a hard barrier; salt flats are halophile-only; estuaries are a nursery realm.
- Source: Doc 08 §5, §11.2, §10.1; Doc 09 §10.1a.
- Use: a salinity axis on realm/province; halophily gate; anadromy keyed to gradients.

**C2. Water pH / alkalinity** *(new)*
- Why: governs carbonate reef-builder shell precipitation (the limestone feedback); acidification is an extinction mechanism.
- Source: Doc 09 §10.1, §11.3.
- Use: gate carbonate deposition and reef-builder guilds; drive `BedrockType::Limestone` into tectonics.

**C3. Nutrient supply / upwelling (N, P, Si, Fe)** *(underused proxy)* — `richness.rs` has only a crude depth-based `marine_nutrient_factor`; `ocean_current_vec` is unread.
- Why: marine production is nutrient-limited over most of the ocean; upwelling zones are the great fisheries.
- Source: Doc 09 §5.2; Doc 07 §8.
- Use: replace the depth proxy with a real upwelling term from `ocean_current_vec` + coastline geometry.

**C4. Dissolved O₂ in water** *(new; distinct from A2)* — benthic/deep life is O₂-limited; dead zones exclude aerobic guilds; anoxia extinction acts here (Doc 09 §7.2).

**C5. Turbidity / photic-zone depth** *(new; `DELTA` + `river_discharge` hint)* — how deep light reaches, set by sediment/plankton. Doc 08 §8.3/§11.2; Doc 09 §5.2. Sets photic-zone depth for D1.

**C6. River / lake connectivity graph** *(field exists, unread)* — `flow_direction`, `river_discharge_m3_yr`, `water_body_id`.
- Why: rivers are corridors **within** a drainage but barriers **between** — the mechanism behind freshwater endemism (cichlid flocks) and anadromy.
- Source: Doc 09 §10.1a, §6.4.
- Use: build freshwater province adjacency from drainage connectivity, not raw hex neighbors.

**C7. Tidal range & intertidal zonation** *(field/flag exists, unread)* — global `tidal_range_m`, `HydroFlags::{ESTUARY, WETLAND, DELTA}`. Richest coastal nurseries + the land-colonization launch zone. Doc 08 §11.1–11.3; Doc 09 §3.4, §10.1.

**C8. Wetlands / marshes / bogs / mangroves** *(flag exists, unread)* — `HydroFlags::WETLAND` + `SoilClass::Peaty`. Carbon-rich, high-productivity, distinctive-guild, transition-zone habitat. Doc 08 §10.2, §11.3; Doc 09 §10.1a.

**C9. Springs / oases / karst** *(flags exist, unread)* — `HydroFlags::{SPRING, OASIS, KARST}`. Desert refugia sustaining relict/endemic populations. Doc 08 §6, §10.4.

### D. Marine specifics

**D1. Depth zones (photic / mesopelagic / benthic / abyssal)** *(underused)* — only a linear depth proxy today. Assign marine guilds by depth band; producers to the photic zone; chemosynthetic communities in the deep. Doc 09 §10.1, §5.2.

**D2. Shelf vs pelagic vs reef** *(underused)* — shelves host the bulk of marine biomass; reefs are hotspots + carbonate factories. Doc 09 §5.2, §10.1; Doc 06 §8.4. Place reef-builders where warm+shallow+clear+alkaline.

**D3. Ocean currents as a biological field** *(field exists, unread)* — `ocean_current_vec`. Larval/plankton dispersal, upwelling (C3), coastal warm/cold regimes, stratification/anoxia. Doc 07 §8; Doc 09 §5.2, §7.2.

### E. Soil & geochemistry

**E1. Soil nutrients (N / P / K explicit)** *(new; only scalar `soil_fertility`)*
- Why: N-fixation is a keystone innovation that *raises the productivity ceiling*; P-limitation caps old leached tropical soils.
- Source: Doc 09 §11 (N-fixation feedback — flagged future), §4.1 (N-fixer guild).
- Use: N-fixer guild raises local N → productivity feedback; P caps rainforest productivity on old substrates.

**E2. Soil class / substrate identity** *(field exists, unread)* — `soil_class`. Substrate-specialist guilds and edaphic endemism (calcareous/sand/serpentine analogs). Doc 08 §10.1; Doc 09 §4.6.

**E3. Soil depth / rooting zone** *(field exists, unread)* — `soil_depth_m`. Caps root biomass/tree size; deep soils buffer drought. Biology also *writes back* soil deepening (Doc 09 §11.4).

**E4. Toxicity (salinity, metals, serpentine, acid-sulfate)** *(partial)* — `salt_accumulated`, `SoilClass::Saline` exist; metals do not. Exclusion filter + hyper-specialist endemics (metallophytes/halophytes).

**E5. Soil age / maturity / weathering stage** *(new)* — literally the `age_since_disturbance` term the richness model wants (Doc 09 §4.4); currently proxied only by ice/`Igneous`.

### F. Disturbance regimes

**F1. Fire regime** *(new; from B2 aridity + biomass + lightning/season)* — maintains grasslands/savannas against forest, selects fire-adapted traits, resets succession. Doc 07 §10 + Doc 09 §4.6.

**F2. Volcanism recency / flood-basalt events** *(underused)* — only static `Igneous` read; no event-driven recency or LIP field. Local reset + **global mass-extinction trigger** (Deccan/Siberian analog). Doc 06; Doc 09 §7.2, §4.4.

**F3. Storm / cyclone exposure** *(new; from `wind_speed_m_s`, SST, latitude)* — coastal/forest windthrow + salt spray, structural-trait selection. Doc 07 §7.

**F4. Flooding / discharge-seasonality regime** *(field exists, unread)* — `discharge_seasonality`, `DELTA`, floodplain `Alluvial`. Both disturbance and productivity subsidy (nutrient renewal). Doc 08 §7; Doc 09 §5.2.

**F5. Relief instability — landslide/erosion hazard** *(field exists, unread)* — `elevation_relief` (+`hydro_elevation_delta_m`). Chronically disturbed steep terrain: early-successional but heterogeneous (many microhabitats → allopatry). Doc 09 §4.4; Doc 08.

**F6. Glacial cycling / ice history** *(partial)* — per-hex `ice_mask` read; global `glaciation_intensity`, `ice_load_m`, `CARVED_TROUGH`/`Loess` not. Repeated glaciation resets diversity and pumps speciation (refugia → recolonization). Doc 07 §12; Doc 09 §4.4, §7.2.

### G. Light & energy

**G1. Day length by latitude & season** *(new)* — photoperiod is the master phenological cue (flowering, breeding, migration, dormancy) and sets the polar productivity collapse independent of temperature. Doc 07 §4.2, §4.8.

**G2. Seasonality of light / insolation integral** *(new)* — the true photosynthesis energy budget; high-latitude summers are intensely productive but brief. Replace the crude `cos(lat)` marine light term. Doc 07 §4.2; Doc 09 §5.2.

### H. Connectivity & barriers (dispersal / allopatric speciation)

**H1. Tectonic plate identity as an allopatry signal** *(field exists, unread)* — `plate_id`. Continental splits are the archetypal allopatric-speciation driver. Doc 09 §6.4. Detect province severance on plate split → force lineage bifurcation.

**H2. Mountain-range barriers** *(new; from `elevation_mean` + `elevation_relief`)* — distinct biotas per flank + elevational endemism. Doc 09 §6.4. Gate province adjacency by an elevation-barrier cost.

**H3. Ocean / strait barriers for terrestrial life** *(derivable)* — `water_level_m`, `basin_id`. Wallace's-Line isolation; new seaways create allopatry. Doc 09 §6.4.

**H4. Desert / aridity barriers** *(derivable from B2 + `climate_regime`)* — deserts split biotas and pump vicariance as they shift. Doc 09 §6.4.

**H5. Dispersal corridors** *(new; inverse of barriers)* — river valleys, land bridges, coastlines, current streams → range expansion + faunal interchange. Doc 09 §6.4, §10.1a.

**H6. Island isolation / area** *(underused)* — island size/isolation and lake area unscored. Drives weird endemic radiations + neutral drift; lakes are the freshwater analog. Doc 09 §6.4, §10.1a. Scale down `selective_payoff` for small/isolated provinces.

### I. Resources (specific food substrates)

**I1. Detritus / dead organic matter pool** *(new)* — the base of the decomposer/detritivore branch (a guaranteed guild), independent of grazing. Doc 09 §4.3, §5.3.

**I2. Carrion pool** *(new; from mortality flux)* — feeds the scavenger guild; a distinct energy pathway. Doc 09 §4.1, §5.3.

**I3. Structural producer substrates (canopy / ground-cover / plankton)** *(new)* — vertical partitioning lets herbivores specialize (browser/grazer/granivore). Doc 09 §4.1. Split `primary_productivity` into structural layers.

**I4. Nectar / pollination & seed-dispersal rewards** *(new)* — mutualism reward resources; major radiation engines (angiosperm–insect co-radiation). Doc 09 §4.1, §5.3.

---

## Part 3 — Notes for the implementing agent

1. **Cheapest wins are the unread fields already on `WorldData`** — no producer work needed, just a consumer in the biology crate: `hydro_flags` (WETLAND/ESTUARY/DELTA/SPRING/OASIS/FJORD/EPHEMERAL/KARST — C7–C9, F4), `soil_class` (E2), `soil_depth_m` (E3), `discharge_seasonality` (F4), `salt_accumulated` + `WaterBody.salinity` (C1), `flow_direction` + `river_discharge_m3_yr` (C6), `ocean_current_vec` (C3/D3), `plate_id` (H1), `elevation_relief` (F5/H2), `WaterBody.{area_km2,volume_km3}` (H6), and globals `sea_level_m` / `global_temperature_c` / `glaciation_intensity` (extinction triggers, §7.2).

2. **Genuinely missing state that needs a producer + new field:** CO₂ on `WorldData` (A1 — currently trapped in `ClimateState`), UV/insolation (A3), air pressure (A4), growing-season/degree-days (B3), humidity/VPD (B4), cloud (B5), extremes (B6), water pH (C2), dissolved O₂ (C4), turbidity/photic depth (C5), explicit N/P/K (E1), soil-age (E5), fire regime (F1), storm exposure (F3), photoperiod (G1), resource pools (I1–I4). File each against its owning physical layer (climate/hydrology/tectonics) or add as biology-internal province state (Doc 09 §5.1).

3. **Province-resolution vs hex-resolution.** Doc 09 §5.1 runs dynamics per-province, with per-hex fields *derived* for rendering. Many Part-2 inputs (nutrients, detritus, food substrates, connectivity) are naturally province-scoped state, not new per-hex `Vec`s. Prefer province state for dynamics; reserve new `WorldData` `Vec`s for genuinely per-hex physical inputs (UV, CO₂-if-local, soil fields, flags) and rendering outputs.

4. **The feedback fields (Doc 09 §11) are writes, not reads,** and must stay forward-in-time / one-tick-lagged: CO₂ drawdown (A1), `soil_fertility`/`soil_depth_m` enrichment (E1/E3), `fertility`→`Limestone` carbonate (C2/D2), albedo, biotic weathering, evapotranspiration (B4), biogenic greenhouse gases. Wire these as biology→physical writes consumed the following tick, mirroring the working `atmospheric_oxygen_fraction` loop in `microbial.rs`.

5. **Key file references:** field definitions/defaults in `crates/genesis_core/src/data/mod.rs`; hydrology enums/flags in `crates/genesis_core/src/data/hydrology.rs`; current consumers in `crates/genesis_biology/src/{richness.rs, biome.rs, province.rs (connectivity flood-fill — home for barrier costs H1–H6), population.rs (cascade/food web — home for resource substrates I1–I4), view.rs, biogenesis.rs, microbial.rs (the O₂ feedback template)}`.
