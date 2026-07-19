# 08 — Hydrology & Soil Module Specification

**Document Type:** Tier 2 — System Specification
**Status:** Draft v0.6
**Last Updated:** July 2026
**Owner:** Brax Johnson
**Implementing Phase:** 2 (Climate & Hydrology — simulation largely landed; §15 exit gates + P2-34 calibration still open)

**Changelog:**
- v0.6 (July 2026): **Zero-trust reconciliation with implementation.** Schema (§2.4) documents `u16` `HydroFlags` (`CARVED_TROUGH`, `DELTA`), pending elevation/GIA/crust arrays, and `glaciation_intensity`. §2.2 clarifies one-tick lag for ice/lake/GW budget terms. §3.1 marks `runoff_coefficient_base` superseded by the PET/AET partition (§4.2). §6.4 / §7.1 document v1 proxies (Igneous hot springs; monsoon via climate-regime + coast distance). §8.5 elevation writes via pending `hydro_elevation_delta_m` consumed by tectonics. §10.3 fertility matches the shipped class-base blend. §12.4 HistoryFrame matches `genesis_ui`. §13 adds `SaltLakeFormed` and names `InlandSeaReconnected`. §14 SoA budget raised to ≤ 48 B/cell. §15 notes which gates are CI-cheap vs `--ignored` deep-time.
- v0.5 (July 2026): **Sea level as a pure output (owner direction).** New §3.5 establishes the principle: no hex stores an ocean/land identity — wet-or-dry is the per-tick relationship `water_level_m > elevation_mean`, recomputed against a sea level that responds to every real driver. Crust *type* (continental vs oceanic lithosphere) remains tectonic geology; water *coverage* is always derived (a continental shelf is continental crust that happens to be submerged right now — glacials expose it as Doggerland-style plains). Adds the **thermosteric term** (warm oceans expand — hothouse seas ride higher on the same water mass), documents which drivers are explicit terms vs **emergent from the flooding solve** (mid-ocean-ridge volume, shelf sedimentation, continental-area change — the retired Doc 06 §4.7 heuristic faked ridge volume; the bathymetry now provides it for real), and adds **glacial isostatic adjustment** to §9.1 (ice loading depresses crust; post-glacial rebound raises shorelines). New validation gates #19–20.
- v0.4 (July 2026): **Hydrology-master revision** (owner direction: "Doc 8 is the hydrology master — if it's not there, it's not getting added later"). Groundwater & karst (§6), seasonal discharge regimes & floods (§7), and glacial carving (§9.2) are promoted from open questions into the specification proper. New light Coastal Waters section (§11): tides from `moon_count`, estuary-vs-delta adjudication, intertidal wetlands. New **Domain Ownership Map** (§1.5) sweeping every water-adjacent phenomenon to an owning doc so nothing is orphaned. Climate–hydrology tandem coupling made explicit (§2.3), including large lakes becoming climate moisture sources (lake effect) in the migration plan. Soil gains `Loess` (wind-blown glacial flour — a second deep-fertility mechanic beside the Cretaceous beach). Schema, events, validation gates, performance budgets, and the prompt plan extended accordingly.
- v0.3 (July 2026): **Full specification.** Four structural decisions settled with the owner: (1) **conserved planetary water budget** — sea level *derived* by flooding the hypsometry against a fixed inventory; (2) **hydrology owns erosion routing** — stream-power incision and network sediment transport replace tectonics' elevation-proportional erosion; (3) **soil specified here** ("Hydrology & Soil" per Doc 01's map); (4) **evaporation-balance lakes** — endorheic salt lakes and salt flats emerge from climate, not thresholds.
- v0.2 (July 2026): **Provisional model removed from the game.** The Phase 2/3 provisional surface-flow model, Rivers render mode, and `flow_direction`/`flow_volume`/`soil_fertility` fields were deleted (rivers rendered at year 0 on a molten planet). Water is out of the game until this spec lands.
- v0.1 (July 2026): Initial stub recording the provisional model as prior art.

---

## 1. Purpose and Scope

Hydrology & Soil turns the climate layer's precipitation and the tectonic elevation field into **water in all its planetary forms and the ground it shapes**: the ocean's actual coastline, drainage networks, rivers with seasonal regimes, lakes, groundwater, springs and oases, ice sheets and the valleys they carve, sediment landforms, coastal waters, and soil. It sits above tectonics (Phase 1) and climate (Phase 2) and feeds biology (Phase 4) and civilization (Phase 5).

**Completeness charter:** this document is the *hydrology master*. Every water-domain phenomenon the engine will ever simulate must be specified here (fully, or as a v1-simplified model with its refinement listed in §16) or explicitly assigned to another doc in the ownership map (§1.5). Nothing water-shaped gets added later from outside this doc.

This module is where the project's central conviction pays off most visibly: **a fertile river valley exists because a mountain range rose, caught rain, and shed sediment into it for fifty million years.** Every output must be traceable to that kind of chain.

### 1.1 Goals

1. **A conserved planetary water budget.** Total water is a world parameter, partitioned every tick between ocean, ice, lakes, and groundwater; sea level is *derived* by flooding the hypsometry, never integrated by heuristic. Ice ages expose land bridges; closing basins raise the sea; the water dial spans near-dry to archipelago worlds.
2. **Visible water returns.** Oceans, rivers, lakes, ice, and coastal waters render again (removed in Doc 06 P1-22).
3. **Hydrologically honest rivers** (standing owner constraint): rivers flow downstream, never originate from nothing, and pool where they cannot continue. Discharge reflects a real upstream basin.
4. **Evaporation-balance lakes.** Wet basins overflow onward; arid basins equilibrate as endorheic salt lakes; salt accumulates monotonically; dried lakes leave salt flats.
5. **Hydrology owns erosion routing and sediment transport** — valleys, floodplains, deltas, and shelf wedges as landforms with causal history, discharging tectonics' explicit IOUs.
6. **Groundwater.** Recharge, baseflow, water tables, springs, oases, and karst — the invisible half of the water cycle that decides which rivers survive the dry season and where desert civilizations can exist.
7. **Seasonal regime.** Every river carries a flow-regime characterization (snowmelt, monsoonal, glacial, stable, ephemeral) and a flood-pulse magnitude — annual-mean simulation, seasonally honest description.
8. **Ice as landscape sculptor.** Ice sheets and alpine glaciers are budgeted water *and* erosive agents: fjords, overdeepened trough lakes, moraine dams, outwash, and loess are consequences of glacial cycles.
9. **Soil.** Depth, class, and fertility from bedrock + climate + sediment + the marine fertility accumulator + glacial flour — the "Cretaceous beach" exit criterion, plus its glacial sibling.
10. **Deterministic and fast.** Same seed → byte-identical water history, within ~10% of the per-tick budget.

### 1.2 Non-Goals (permanent scope boundaries, not deferrals)

- **No physical hydrodynamics.** No shallow-water equations, wave action, or storm surge. Tides are a static derived range (§11.1), not dynamics.
- **No storm/sub-annual *simulation*.** Seasonal regime is a per-tick derived characterization (§7), not weather. Individual flood *events* as narrative beats are a possible future rule-engine concern, not simulation.
- **No biological water chemistry.** Nutrients, eutrophication, reef growth: Phase 4, consuming this doc's fields.
- **No human water infrastructure.** Dams, canals, irrigation, wells: Doc 10, consuming this doc's fields.
- **No 3D cave topology.** Karst produces flags, springs, and routing effects; cave *geometry* is sub-hex/naming content.

### 1.3 Dependencies

**Reads:** `elevation_mean`, `elevation_relief`, `bedrock_type`, `plate_id` (tectonics); `precipitation`, `temperature_mean`, `temperature_range`, `wind_direction_rad`, climate regime, glaciation state (climate); `fertility` (tectonics' marine accumulator, Doc 06 §8.4); the plate-surface write API + `ProjectionCache` (§8.5).

**Writes:** `sea_level_m` (**ownership moves from tectonics**, §17.1); all §2.4 arrays and the water-body registry; elevation via pending `hydro_elevation_delta_m` consumed by tectonics (§8.5); soil arrays.

**Consumed by:** climate (ocean mask + large lakes as moisture geography, one tick lagged, §2.3), rendering (§12), biology (soil, water access, wetlands, intertidal), civilization (rivers, navigability, springs/oases, floodplains).

### 1.4 What This Phase Delivers

1. This document, fully specified before implementation
2. Re-created crate `genesis_hydrology`, a `SimulationLayer` after climate
3. Water budget, flooding solver, drainage, lakes, groundwater/karst, seasonal regime, erosion/sediment, glacial carving, soil, coastal waters
4. Tectonics hand-off (§17.1) and climate migration (§17.2), with Doc 06/07 changelog edits in the same prompts
5. Rendering: water-aware ramp, river LOD, lake/ice/fjord/oasis display, Soil mode; HistoryFrame water fields; **Water inventory** setup-menu knob
6. Validation gates per §15

### 1.5 Domain Ownership Map

The completeness sweep. Every water-adjacent phenomenon, and where it lives:

| Phenomenon | Owner | Where |
|---|---|---|
| Ocean geometry, coastline, sea level | **This doc** | §3 |
| Sea-level drivers: ice, thermosteric, ridge volume, sedimentation, GIA | **This doc** (explicit or emergent — §3.5 factor model) | §3.5, §9.1 |
| Ocean *physics* (currents, gyres, SST effects on air) | Climate | Doc 07 §8, on hydrology's geometry |
| Precipitation, humidity, monsoons, evaporation *drivers* | Climate | Doc 07 §9 |
| Drainage networks, rivers, discharge | **This doc** | §4 |
| Lakes, inland seas, salt lakes, salt flats | **This doc** | §5 |
| Groundwater, water table, springs, oases, karst | **This doc** | §6 |
| Seasonal flow regimes, flood pulses, ephemeral rivers | **This doc** | §7 |
| Snowpack (intra-annual store) | **This doc** (regime input only) | §7.2 |
| Erosion, sediment transport, deposition landforms | **This doc** | §8 |
| Waterfalls / fall lines, navigability classes | **This doc** (derived) | §4.5 |
| Ice sheets, sea ice, alpine glaciers (mass + budget) | **This doc** | §9 |
| Glacial carving: fjords, trough lakes, moraines, outwash, loess | **This doc** | §9.2 |
| Glaciation *state* (when/how cold), ice albedo | Climate | Doc 07 §12 |
| Permafrost (drainage effect) | **This doc** | §7.4 |
| Tidal range, estuaries, intertidal zones | **This doc** | §11 |
| Soil depth, class, fertility | **This doc** | §10 |
| Wetland *tagging* (physical) | **This doc** | §10.2 |
| Wetland/mangrove/reef *ecosystems* | Biology | Doc 09, consuming our tags |
| Hot springs / geothermal surface water | **This doc** (flag, volcanism-fed) | §6.4 |
| Water in biomes, species, habitability formula | Biology | Doc 09 |
| Dams, canals, irrigation, wells, water conflict | Civilization | Doc 10 |
| River/lake/sea *naming* | Civilization/Export | Doc 10/15, keyed on `WaterBodyId` |
| Sub-hex creeks, waterfall vignettes, cave content | Rendering/zoom | Contract here (§12.3), implementation Doc 14 |
| Chaos-mode water (reverse rivers, etc.) | Future chaos doc | Doc 01 §9.5.3 |

If a future feature request is water-shaped and not on this table, it amends **this doc** first.

---

## 2. Architectural Overview

### 2.1 Layer Position and Cadence

`HydrologyLayer` registers **after** climate; tick intervals match climate's per-era cadence. During Formation it tracks condensation (§3.3); full activation at first standing water + nonzero precipitation.

### 2.2 Per-Tick Sequence

1. **Inventory update** (§3): condensation (Formation); partition the budget using **previous-tick** ice volume, lake volume, and groundwater storage (intentional one-tick lag — same pattern as the climate tandem §2.3). Same-tick ice/GW are recomputed later in this sequence and feed the *next* tick's flood.
2. **Flooding solve** (§3.4): `sea_level_m`, ocean mask, `water_level_m`.
3. **Drainage network** (§4): routing surface, flow directions, surface runoff accumulation.
4. **Groundwater pass** (§6): recharge, karst diversion, baseflow return, water table, springs/oases.
5. **Lake balance** (§5): evaporation balance with total inflow (surface + baseflow); registry; salt.
6. **Seasonal regime** (§7): regime class, seasonality index, flood magnitude, perennial/ephemeral (after water tables, which regime consumes).
7. **Ice & carving** (§9): masks, budgeted ice volume from `glaciation_intensity`, GIA load targets, glacial erosion, retreat landforms.
8. **Erosion & sediment** (§8): hillslope + stream-power + glacial inputs; routing; deposition → `hydro_elevation_delta_m`.
9. **Soil update** (§10). 10. **Coastal derivations** (§11). 11. **Events** (§13).

Steps 2–6 are primarily stateless derivations. Persistent accumulators: salt, alluvium, soil depth, groundwater storage, ice mask (previous-tick, for retreat diffing), carved-trough / delta flags, ice-load targets for tectonics isostasy.

### 2.3 The Climate–Hydrology Tandem

The two layers are one water cycle split by ownership — **hydrology is where water is; climate is what water does to air** — coupled both directions with a one-tick lag (500 ky: negligible; the pattern Doc 07 §13.2 established):

**Hydrology → climate:** ocean geometry (mask, basins) for distance-to-ocean, currents, and continentality; **large water bodies as moisture sources** — the migration (§17.2) upgrades climate's `distance_to_ocean_km` to distance-to-*water* including bodies ≥ `LAKE_CLIMATE_MIN_HEXES` (default 8), so a Caspian-scale inland sea moderates its shores and feeds lake-effect precipitation; ice-sheet extent as an albedo/temperature input to the glaciation model.

**Climate → hydrology:** precipitation (the input), temperature (evaporation, snow, ice, **and the thermosteric sea-level term §3.5.1** — global warming literally raises the sea), wind (loess transport, §9.2), glaciation intensity (ice volume), regime classification (soil, seasonality).

The user-visible loop, e.g.: warm equatorial ocean (hydrology geometry) → strong evaporation and warm currents (climate) → heavy coastal precipitation (climate) → monsoon-regime rivers and deltas (hydrology) → floodplain soil (hydrology) → rainforest and river civilizations (later phases). Each arrow is a separate, inspectable system; the loop is the product.

### 2.4 New Data (schema)

Bulk arrays on `WorldData` (SoA, snapshot-persisted):

```rust
/// Water surface elevation over this hex (ocean level, lake level), or
/// WATER_NONE = f32::NEG_INFINITY when dry. Depth = water_level_m - elevation_mean.
pub water_level_m: Vec<f32>,
/// Standing-water body membership. WaterBodyId::NONE when dry.
pub water_body_id: Vec<WaterBodyId>,
/// Steepest-descent drainage direction on the routed surface. None for
/// ocean/lake-interior/retained-sink hexes.
pub flow_direction: Vec<Option<Direction>>,
/// Annual river discharge, m³/year (surface runoff + baseflow).
pub river_discharge_m3_yr: Vec<f32>,
/// Peak-season/annual-mean discharge ratio (≥ 1.0). 1.0 = perfectly stable.
pub discharge_seasonality: Vec<f32>,
/// Depth to the saturated zone, meters. 0 = water table at surface.
pub water_table_depth_m: Vec<f32>,
/// Accumulated salt (monotonic, arbitrary units). Nonzero + dry = salt flat.
pub salt_accumulated: Vec<f32>,
/// Land-ice mask (sheets + alpine glaciers thick enough to budget).
pub ice_mask: Vec<bool>,
/// Packed per-hex hydrology feature flags (`HydroFlags` is a `u16` newtype):
/// bits 0–7: SPRING | OASIS | KARST | ESTUARY | FJORD | EPHEMERAL | WETLAND | SEA_ICE
/// bits 8–9: CARVED_TROUGH (persistent glacial scar) | DELTA (Major mouth progradation)
pub hydro_flags: Vec<HydroFlags>,
/// Soil — §10.
pub soil_depth_m: Vec<f32>,
pub soil_fertility: Vec<f32>,
pub soil_class: Vec<SoilClass>,
/// Pending fluvial/glacial elevation change (m), consumed next tectonics tick (§8.5).
pub hydro_elevation_delta_m: Vec<f32>,
/// Continental-crust mask rebuilt from plate surfaces (§8.1 freeboard; tectonics owns truth).
pub continental_crust: Vec<bool>,
/// Ice-load depression target for GIA (m); hydrology writes, tectonics isostasy applies (§9.1).
pub ice_load_m: Vec<f32>,
```

Global (not per-cell) hydrology-related state on `WorldData`:

```rust
pub sea_level_m: f32,              // owned by hydrology (§3.4 / §17.1)
pub glaciation_intensity: f32,     // 0..=1; climate writes each tick (§17.2); ice volume (§9.1)
```

Water-body registry (sparse, deterministic, in `WorldData` — survives snapshots, feeds events and naming):

```rust
pub struct WaterBody {
    pub id: WaterBodyId,        // = lowest HexId of the basin: stable & deterministic
    pub kind: WaterBodyKind,    // Ocean | Sea | Lake | SaltLake | SaltFlat
    pub surface_m: f32,
    pub area_km2: f64,
    pub volume_km3: f64,
    pub salinity: f32,
    pub outlet: Option<HexId>,
}
```

`WaterBodyId`: `u32`, sentinel `NONE = u32::MAX`, in `genesis_core::data::ids`.

---

## 3. The Planetary Water Budget

### 3.1 Parameters

```rust
pub struct HydrologyParameters {
    /// Total surface-water inventory as a global equivalent layer (GEL), meters
    /// over the whole sphere. Earth ≈ 2700. Band: 100 (near-dry) … 8000+.
    pub water_inventory_gel_m: f32,       // Immutable. Default 2700.0. MENU KNOB.
    /// DEPRECATED / unused in v1 physics: kept for save compatibility.
    /// Surface runoff is derived from the PET/AET/infiltration partition (§4.2),
    /// not a single global runoff coefficient.
    pub runoff_coefficient_base: f32,     // 0.4 — do not wire new consumers
    pub open_water_evap_factor: f32,      // 1.2
    pub groundwater_capacity_m: f32,      // 30.0 — GEL-equivalent aquifer capacity
}
```

The setup menu exposes `water_inventory_gel_m` as the **Water knob** (500–6000, step 250), sibling of the Continental crust % knob. `ClimateInitialParameters.initial_sea_level_m` is retired from use (kept in schema for save compatibility).

### 3.2 The Accounting Identity

Every tick, f64, fixed order (ascending `HexId` sums):

```
inventory = atmosphere_reserve (Formation only)
          + ocean_volume + Σ lake_volumes + ice_volume + groundwater_storage
```

Asserted in debug builds to 1e-6 relative. No leak paths: drying lakes, melting ice, and draining aquifers all return to the ocean term. Snowpack is intra-annual (nets to zero at annual mean) and deliberately **not** a reservoir — it shapes seasonality only (§7.2).

### 3.3 Formation: the Seas Fill

Doc 07 §3.5's sea-level curve is reinterpreted (Doc 07 edit ships with implementation) as the **condensed fraction** of the inventory: Molten 0–50 My → 0; Cooling 50–200 My → 0.35; Condensation 200–350 My → 0.90; Stabilization 350–500 My → 1.0. Sea level is *derived* from condensed volume via §3.4 — a low-inventory world visibly ends with small seas; "the oceans are still filling" survives, now honestly. Groundwater storage fills to its aridity-equilibrium during Condensation. Rivers first appear during Condensation.

### 3.4 The Flooding Solve

Given `ocean_volume = condensed − ice − Σ lakes (prev tick) − groundwater_storage`:

1. **Bathtub level.** Apply the thermosteric adjustment (§3.5.1) to get `effective_ocean_volume`. Sort cells by `elevation_mean` ascending (tie: `HexId`); walk the prefix accumulating `hex_area × (L − elev)` until it reaches the effective volume; interpolate exact `L`. Closed-form, deterministic.
2. **Ocean mask.** BFS (ascending-`HexId` frontiers) over `{elev < L}` components. The largest-volume component is **the ocean**; other below-`L` components are *candidate seas* handed to §5 — ocean-fed seas or doomed endorheic basins by their climate. Endorheic-adjudicated surplus returns as one closed-form correction `ΔL = returned / ocean_area` (residual folds into next tick).
3. **Write** `sea_level_m = L`; `water_level_m`/`water_body_id` for ocean cells.

**Emergent showcase (intentional):** tectonics closes a strait → the orphaned sea leaves the ocean component → evaporation balance takes over → drawdown, hypersalinity, salt flats. The Messinian Salinity Crisis as a consequence; demoable via `InlandSeaIsolated` events.

**Perf:** the sort is the hot spot (§14) — radix or near-sorted re-sort.

### 3.5 Sea Level Is an Output, Never an Input

The governing principle (owner direction): **no hex is "ocean" or "continent" by assignment.** A hex has an elevation; the planet has a derived water surface; wet-or-dry is the relationship between them, recomputed every tick. Two rules make this concrete:

1. **Crust type ≠ water coverage.** Continental vs oceanic *lithosphere* (`continental_crust`, `BedrockType::OceanicCrust`) is real, permanent geology owned by tectonics — buoyant granite vs dense basalt is *why* basins exist. But no system may store, cache, or assume a hex's submerged status. "Is ocean" is always the computed predicate `water_level_m > elevation_mean`, this tick. A continental shelf is continental crust that happens to be underwater right now; at glacial maximum the same hexes are dry coastal plain (Doggerland, the Sunda shelf), and every downstream system — erosion, fertility accumulation, climate's ocean mask, rendering — follows the water line automatically because they all read the derived fields.
2. **Every driver moves the same one number** through the flooding solve. There are no side-channel sea-level adjustments anywhere in the engine.

**The factor model.** What moves sea level, and how each driver enters — balancing accuracy against cost, most drivers are *emergent* (already in the bathymetry or the budget) and cost nothing extra:

| Driver | Sign & scale (Earth reference) | How it enters |
|---|---|---|
| **Ice partition** | glacials −60…−130 m; deglaciation reverses | Explicit: ice volume debits the ocean term (§9.1) |
| **Thermal expansion** (thermosteric) | ≈ +0.7 m per +1 °C ocean-mean; hothouse ≈ +5…+15 m | Explicit: §3.5.1 |
| **Mid-ocean-ridge volume** | fast-spreading epochs ≈ +50…+150 m over 10⁷–10⁸ yr | **Emergent:** young ridge crust is minted shallow and thermally subsides (Doc 06); an active-spreading world has shallower basins in its hypsometry, so the same water stands higher. The retired Doc 06 §4.7 heuristic faked exactly this; the bathymetry now provides it for real. |
| **Shelf/basin sedimentation** | slow rise over 10⁸ yr | **Emergent:** §8.3 deposition shallows basins in the hypsometry |
| **Continental area change** (accretion, subduction erosion) | slow, either sign | **Emergent:** hypsometry again |
| **Lake & groundwater partition** | small (±1–2 m); arid↔humid epochs | Explicit: budget terms (§3.2, §6.1) |
| **Formation condensation** | −∞ → 0 over the first 500 My | Explicit: §3.3 |
| **Glacial isostatic adjustment** | *local* relative change, ±100s of m under/near ice | Explicit, local: §9.1 — moves the *land*, not the water |

**Consequences to render and narrate, not special-case:** transgression/regression (shorelines walking inland and back as `water_level_m` shifts — no extra mechanism, the field just changes), land bridges at glacial maxima, post-glacial rebound coasts (§9.1), and hothouse worlds with flooded continental interiors. `SeaLevelMilestone` events (§13) narrate the excursions.

### 3.5.1 The Thermosteric Term

Warm water occupies more volume. At the level solve, the ocean's *effective* volume is:

```
effective_ocean_volume = ocean_volume × (1 + THERMOSTERIC_BETA × (T_ocean − T_REF))
```

with `T_ocean` = global mean temperature (the ocean equilibrates within a 500 ky tick; no lag state needed), `T_REF = 15 °C`, and `THERMOSTERIC_BETA = 1.9e-4 /°C` (≈ +0.7 m of sea level per +1 °C over Earth-mean 3.7 km depth). One scalar multiply — free — and it delivers the owner's requirement directly: **global warming raises the sea; ice ages drop it twice over** (ice debit + thermal contraction, correctly reinforcing). The §3.2 conservation identity is unaffected: it accounts *mass* (volume at reference temperature); thermosteric expansion enters only the volume→level mapping.

---

## 4. The Drainage Network

### 4.1 Routing Surface (prior art, kept)

Priority-flood depression-filled scratch surface (Barnes 2014, +epsilon; heap keyed `(level, HexId)`), seeded from the ocean mask and ocean-adjacent land. Produces the **depression tree** (§5) and monotone downhill paths. Never touches `elevation_mean`.

### 4.2 Water Partition (per land hex, annual)

```
PET_mm      = max(0, 700 + 40 × temperature_mean)
AET_mm      = min(precipitation, PET_mm)
available   = max(0, precipitation − AET_mm)
infiltration= available × infiltration_fraction(hex)   // → groundwater recharge, §6.1
runoff      = available − infiltration                  // → surface routing
```

`infiltration_fraction`: base 0.35, modulated — `×2.0` on KARST (§6.3), `×1.5` on deep/`Sandy` soil, `×0.2` where `temperature_mean < PERMAFROST_TEMP_C = −8` (frozen ground sheds, §7.4), `→ 0.05` on bare rock, clamped `[0.02, 0.9]`. All constants `pub const` calibration surface.

### 4.3 Flow Directions and Discharge

Steepest descent over the filled surface (ties → lowest neighbor `HexId`); accumulate `runoff × hex_area` upstream→downstream in descending filled-elevation order (tie: ascending `HexId`). **Baseflow (§6.2) is added to channel hexes before accumulation**, so `river_discharge_m3_yr` is total annual flow. Discharge entering ocean/lakes leaves the network (lakes credit it as inflow).

### 4.4 River Classes

| Class | Discharge (m³/yr) | Reference |
|---|---|---|
| Creek | < 1e9 | sub-hex only (§12.3) |
| Stream | 1e9–1e10 | — |
| River | 1e10–1e11 | Rhine ≈ 7e10 |
| Major | > 1e11 | Danube 2e11, Congo 1.3e12, Amazon 5.5e12 |

### 4.5 Waterfalls, Fall Lines, Navigability (derived here; Doc 10 consumes)

- **Waterfall/rapids flag:** channel hex whose drop to its flow target exceeds `WATERFALL_MIN_DROP_M = 150` *or* which crosses a hard→soft bedrock contact with drop ≥ 60 m. Render vignette at zoom; portage/head-of-navigation for Doc 10.
- **Navigability class** per channel hex: `Navigable` (River+ class, slope below threshold, perennial), `SeasonallyNavigable` (monsoonal/nival regime, high seasonality), `Unnavigable` (rapids/waterfall/ephemeral). The **fall line** — the last waterfall before the sea — emerges as the classic city site; Doc 10 reads it, this doc computes it.

---

## 5. Lakes and Inland Seas (Evaporation Balance)

### 5.1 Basin Adjudication

Per depression, bottom-up, deterministic order (basin-bottom `HexId`):

```
I        = Σ entering discharge (incl. baseflow) + precipitation on lake surface
E(level) = lake_area(level) × max(0, 800 + 45 × T) × open_water_evap_factor
```

- `I ≥ E(spill)` → **exorheic**: stands at spill; `I − E` continues downstream from the spill hex (chains compose up the depression tree; sub-basins filling to a shared spill *are* one lake).
- else → **endorheic**: solve `E(level) = I` by fixed 24-iteration bisection (monotone; deterministic; sub-meter exact).

Equilibrium applies instantly each tick — at 500 ky ticks, real lakes equilibrate thousands of times over. (Recent-era relaxation constant: §16.)

**Emergent showcase (intentional):** glacials cut evaporation and shift precipitation → arid-basin lakes swell into **pluvial lakes** (Lake Bonneville), then shrink to salt lakes and flats in interglacials (Great Salt Lake + Bonneville flats). Falls out of the balance; no special code.

### 5.2 Candidate Seas

Non-ocean below-`L` components (§3.4) run the same balance seeded with their bathtub volume: sustained → isolated **Sea** (Caspian analog); unsustainable → drawdown → salt. Surplus returns to the ocean term.

### 5.3 Salt

Endorheic bodies bank `Δsalt = I × SALT_LOAD_FACTOR` per tick on basin-floor hexes (monotonic, like `fertility`). Salinity = salt/volume — shrinking lakes get saltier (Dead Sea); total drying leaves a `SaltFlat` body and a soil penalty. Registry kinds update on thresholds.

---

## 6. Groundwater and Karst

The invisible half of the cycle. v1 is a deliberately simple, deterministic model with real consequences: baseflow (perennial rivers), water tables (wells later), springs, oases, karst. No lateral aquifer solver — recharge follows the surface drainage topology, which is honest at hex scale (~7,700 km² at level 8: groundwater basins and surface basins largely coincide).

### 6.1 Recharge and Storage

`recharge = infiltration × hex_area` (§4.2). Per-hex aquifer storage fills toward `groundwater_capacity_m × hex_area` (excess → immediate baseflow). Global `groundwater_storage` (the §3.2 reservoir) is the deterministic sum; it drifts slowly with climate (wet epochs bank water, arid epochs drain — a small, honest sea-level signal).

### 6.2 Baseflow

Stored water discharges at `BASEFLOW_RATE = 0.02 /tick` of storage, routed one pass along `flow_direction` and **injected into channel hexes** before accumulation (§4.3). Consequences: rivers keep flowing through arid seasons and short dry epochs; the perennial/ephemeral distinction (§7.3) becomes computable; desert trunk rivers fed by distant mountain recharge exist (the Nile pattern).

### 6.3 Karst

Hexes with `bedrock ∈ {Limestone}` or `soil_class == Calcareous`, and `precipitation > 400 mm`: **KARST flag**. Effects: infiltration ×2.0 (§4.2); a `KARST_UNDERGROUND_FRACTION = 0.5` share of would-be runoff routes *underground* along the flow path, re-emerging at the first non-karst hex (or after 2 hexes) as a **SPRING-flagged resurgence** injected as baseflow — disappearing streams and great karst springs. Karst + `water_table_depth_m > 20` → cave-rich (sub-hex/naming content; no 3D topology). Renders: surface discharge visibly thins across karst belts.

### 6.4 Water Table, Springs, Oases, Hot Springs

- `water_table_depth_m = aridity_offset(P/PET) × (1 − proximity_factor)`, where `aridity_offset` spans 2 m (humid) → 60 m (hyper-arid) and `proximity_factor` decays with flow-path distance from perennial water. 0 at rivers/lakes/wetlands. A proxy, not a solver — calibration surface.
- **SPRING flag:** water table < 2 m on a slope hex with upstream recharge area above threshold (plus karst resurgences).
- **OASIS flag:** arid-regime hex (`precipitation < 100 mm`) with `water_table_depth_m < 5` — reachable only via upstream-recharge proximity, i.e. mountains feed them. Renders a small water speck; biology/civ hook. The Sahara pattern emerges: oases string along the flow paths draining wetter highlands.
- **Hot springs:** SPRING coincident with volcanic/hotspot activity tags geothermal — flavor + Doc 10 settlement attractor. **v1 proxy:** `BedrockType::Igneous` at the spring hex (tectonics does not yet expose a dedicated hotspot occupancy flag on `WorldData`).

---

## 7. Seasonal Regime and Floods

Annual-mean simulation, seasonally honest **characterization**, recomputed per tick from climate fields — no sub-annual stepping.

### 7.1 Regime Classification (per channel hex, from its basin's climate)

| Regime | Condition (basin-weighted) | Seasonality |
|---|---|---|
| `Stable` | equatorial/oceanic: low `temperature_range`, low precip variance | 1.0–1.5 |
| `Monsoonal` | Tropical/Subtropical climate regime within ~300 km of water **and** precip ≥ 1000 mm (v1 proxy — climate does not yet expose a dedicated monsoon flag) | 3–8 |
| `Nival` (snowmelt) | winter mean < 0 °C, summer > 0 °C: snowpack stores winter precip, spring pulse | 2–5 |
| `Glacial` | upstream ice mask: summer-melt-fed, reliable | 1.5–2.5 |
| `Ephemeral` | §7.3 | effectively ∞ |

Winter/summer proxies: `temperature_mean ∓ temperature_range/2`. `discharge_seasonality` stores the ratio; **flood magnitude** `= discharge × seasonality` drives the floodplain-hazard input reserved for habitability and the alluvial soil bonus (flood pulses build floodplains — §8.3 deposition weights by it).

### 7.2 Snowpack

Intra-annual store only (not a §3.2 reservoir): fraction of precipitation falling below 0 °C banks and releases in the melt season — it sets `Nival` seasonality and shifts effective erosion timing (folded into the climate modifier). Explicitly *not* multi-year firn (that's §9 ice).

### 7.3 Perennial vs Ephemeral

A channel is **perennial** if baseflow alone (§6.2) sustains `≥ EPHEMERAL_BASEFLOW_MIN` at that hex; else **EPHEMERAL flag** (wadi): flows only in the wet pulse, renders dashed/seasonal, `Unnavigable`, no riparian soil bonus. This is the groundwater system paying rent: without §6 the distinction is impossible.

### 7.4 Permafrost

`temperature_mean < −8 °C`: infiltration → 0.2× (frozen ground, §4.2), water tables pinned shallow, wetland-prone flats (thermokarst flavor via WETLAND flag). No excess ice/thaw dynamics in v1.

---

## 8. Erosion and Sediment Transport

Hydrology is the sole authority for water-driven erosion and all sediment routing. Tectonics retains uplift/subsidence, isostasy and freeboard rebound, thermal subsidence, gravitational collapse (rock mechanics), and its pre-water dry-weathering fallback (active only before hydrology activates).

### 8.1 Hillslope Denudation

Successor of tectonics' elevation-proportional erosion at the same calibrated scale (`base_erosion_rate_per_year` 5e-8, Doc 06 §8.2 bedrock multipliers, Doc 07 §13 climate modifier). Every land hex; mass enters the local sediment load. Freeboard carries over: continental crust erodes toward `sea + CONTINENTAL_FREEBOARD_M` (live tectonics constant — reference, don't re-pin), oceanic-crust land toward sea level.

### 8.2 Stream-Power Incision

For channel hexes (`discharge ≥ STREAM_CLASS_MIN`):

```
incision = K_channel × bedrock_mult × climate_mod × sqrt(discharge_norm) × slope × tick_years
```

`K_channel` start 2e-7/yr; calibrated so a Major river on soft rock cuts hundreds of meters in 50–100 My. Fluvial incision floors at the downstream water level. (Glacial erosion may go deeper — §9.2.)

### 8.3 Routing and Deposition

Load (hillslope + incision + glacial) rides the network downstream, one ordered pass. Capacity `∝ discharge × slope`; excess deposits: **floodplains** (low slope; weighted by flood magnitude §7.1), **lakes** (perfect traps — endorheic basins genuinely infill over deep time), **deltas** (mouth + submerged neighbors, building toward but never above water level → progradation), **shelves** (mouth overflow). Deposition ≥ 500 m cumulative → `Sedimentary` bedrock (mechanism moves here from tectonics); the accumulator persists as soil's alluvium input.

### 8.4 Mass Conservation

`Σ eroded = Σ deposited + ocean_sink` per tick (diagnostic total; abyssal fans not spatially modeled). Debug-asserted.

### 8.5 Integration Constraint (critical)

Elevation is authoritative on **birth-frame plate surfaces**. Hydrology never writes `elevation_mean` directly and never calls `modify_surface_at_world_hex` itself. Instead it accumulates pending change in `hydro_elevation_delta_m`; the next tectonics Geological tick consumes that buffer via `modify_surface_at_world_hex` (modify-only; silently skips projection holes — correct and load-bearing, Doc 06 P1-21), using the tick's `ProjectionCache`. Changes appear at the subsequent world rebuild (same visibility tectonic erosion had). GIA ice-load depression uses the parallel `ice_load_m` path into tectonics isostasy (§9.1).

---

## 9. Ice

### 9.1 Masses and the Budget

- **Ice sheets:** land, `temperature_mean < −12 °C`. **Alpine glaciers:** land, `temperature_mean < −4 °C`, not sheet, positive relief (mountain ice — temperature already encodes lapse rate). Both set `ice_mask`.
- Global ice volume: `glaciation_intensity × ICE_VOLUME_MAX`, calibrated ≈ **120 m sea-level-equivalent** at full glacial (Earth's LGM) — debited before the flooding solve. **Glacials lower the sea and open land bridges; that is the point.**
- **Sea ice:** display flag (`ocean && temperature_mean < −2 °C`); floating, no budget effect.
- Ice suppresses soil development (`SoilClass::None` while iced).
- **Glacial isostatic adjustment (GIA):** ice is a crustal load. Hydrology sets per-hex `ice_load_m` to `ICE_LOAD_DEPRESSION_M = 250` under ice (0 when clear). Tectonics applies the load through the existing plate-surface isostasy pathway (`apply_ice_load_isostasy`), relaxing toward `sea + freeboard − load` at the epeirogenic rebound rate. On retreat the load clears and freeboard rebound raises the crust. At 500 ky ticks this is near-equilibrium each tick. Payoff: deglaciated coasts *rise* out of the sea over subsequent ticks — raised shorelines and emergent archipelagos (the Scandinavia/Hudson Bay pattern), and briefly-flooded post-glacial margins. This is *relative* sea-level change — it moves the land, complementing §3.5's movements of the water.

### 9.2 Glacial Carving (the sculptor)

Glaciated hexes erode at `GLACIAL_EROSION_FACTOR = 2.5×` the hillslope rate, routed down-ice along the same network, with one decisive difference from rivers: **overdeepening** — glacial erosion may cut to `downstream water level − OVERDEEPEN_MAX_M = 400`, below the fluvial floor. Products, on retreat (previous-tick mask diff):

- **Fjords:** carved troughs now ocean-flooded on high-relief coasts → FJORD flag (deep, narrow, spectacular — rendering + naming).
- **Trough / finger lakes:** inland overdeepenings and **terminal-moraine dams** (a fraction of carved load deposits as a ridge at the glacier terminus hex) become closed basins — the depression tree and §5 balance then make real lakes of them automatically. The Great Lakes / Lago di Como pattern emerges from three systems composing.
- **Outwash & loess:** carved load exiting the ice margin deposits as outwash; a share lofts **downwind** (climate's `wind_direction_rad`) up to `LOESS_RANGE = 3` hexes → `SoilClass::Loess`, deep and top-tier fertile. The US-Midwest/China pattern: **a second deep-fertility mechanic beside the Cretaceous beach** — future breadbaskets exist because ice ground mountains into flour and wind spread it.
- Carved-trough tags persist as `HydroFlags::CARVED_TROUGH` across ticks (like `fertility`): the landscape remembers its glaciations. Major mouths may carry `HydroFlags::DELTA` when progradation deposits at the land–ocean interface (§8.3).

---

## 10. Soil

### 10.1 Fields and Classes

`soil_depth_m` (`+weathering(bedrock, climate) + deposition − erosion`, clamp [0, 50]; weathering ~1e-5 m/yr scaled by bedrock/climate) and:

```rust
pub enum SoilClass {
    None,        // bare rock / active ice / open water
    Alluvial,    // floodplain & delta deposition
    Loess,       // wind-blown glacial flour (§9.2) — deep, fertile
    Volcanic,    // young Igneous / recent volcanism
    Calcareous,  // Limestone / marine-sediment bedrock
    Sandy,       // arid, thin
    Peaty,       // cold + wet + flat (wetland)
    Saline,      // salt-poisoned
    Loamy,       // temperate default
}
```

Deterministic decision tree, priority: ice/water → Saline → Loess → Alluvial → Volcanic → Calcareous → Peaty → Sandy → Loamy.

### 10.2 Wetlands

Land, low filled-surface gradient, and (discharge-through ∨ lake-adjacent ∨ water table < 1 m) → WETLAND flag + `Peaty`/`Alluvial`. Coastal intertidal wetlands: §11.3. Biology makes marshes, bogs, and mangroves of them.

### 10.3 Fertility

v1 uses a **class-base blend** (shipped), not a fully expanded multi-factor weighted sum. Class encodes alluvium / loess / volcanic / saline / climate-ish defaults; marine `fertility` and soil depth add on top:

```
class_base = match soil_class {
    None | Saline => 0.0,   // salt flats stay infertile
    Sandy => 0.25, Loamy => 0.55, Calcareous => 0.5,
    Volcanic => 0.7, Peaty => 0.45, Alluvial => 0.75, Loess => 0.9,
}
soil_fertility = clamp(class_base + 0.40 × fertility + clamp(soil_depth_m/10, 0, 0.2), 0, 1)
```

Twin validation contracts still hold: ancient-shallow-sea hexes now above water should rank top-decile (the Cretaceous beach, §15 #8); loess belts downwind of retreated ice rank alongside them (#16). A fuller weighted formula (explicit alluvium/loess/climate factors) remains a calibration refinement if gates fail.

### 10.4 Habitability Hook

Reserved inputs for Doc 09's formula: fresh water access (river class / lake / spring / oasis), flood exposure, salinity, wetland. Not computed here.

---

## 11. Coastal Waters

Light but present — the land–sea seam is where biology and civilization concentrate.

### 11.1 Tidal Range

Static derived global value: `tidal_range_m = 0.4 + 1.2 × moon_count` (0 moons ≈ solar-only 0.4 m; 1 ≈ 1.6 m; 2 ≈ 2.8 m). Coastal-geometry amplification (funnel bays) is future scope (§16). Consumed by §11.2–11.3 and Doc 09/10.

### 11.2 Estuary vs Delta

At River/Major mouths: heavy sediment load → **delta** (§8.3 wins); low load with `tidal_range ≥ 1.0` → **ESTUARY flag** (drowned mouth, brackish, superb harbor — Doc 10 will love them). Adjudicated per mouth per tick by load-vs-threshold.

### 11.3 Intertidal Zones

Low-relief coasts with `tidal_range ≥ 1.5` → WETLAND-flagged shore hexes (mudflats/saltmarsh/mangrove-precursor by climate) — biology's coastal nurseries.

---

## 12. Rendering, LOD, and History Frames

### 12.1 Water-Aware Terrain

The P1-22 dry ramp becomes water-aware: `water_level_m > elevation` renders depth-tinted water (shelf→abyss; lakes same field); salt flats pale; ice white; fjords read as narrow deep incursions (they'll be striking). **Soil** render mode joins the set; ephemeral channels render dashed/muted vs solid perennial rivers.

### 12.2 Rivers at World View

Polylines along `flow_direction`, width by class; world view renders River+Major only. Overlay rebuilds only when the displayed frame changes.

### 12.3 View-Distance River LOD (binding, owner-directed)

World view → Major/River; regional → +Stream; local/hex → every creek. Creeks are sub-hex procedural content — first consumer of Doc 04 §3.7's `SubHexTerrainGenerator`. Contract: seed `(effective_seed, "hydrology.subhex", hex_id)`; deterministic tributary fractal whose trunk matches macro entry/exit directions and whose summed discharge equals the macro value; waterfalls/springs/cave mouths as vignette features. Same hex, same zoom → identical creeks. **Specified now; implemented with Doc 14.**

### 12.4 History Frames

`genesis_ui::HistoryFrame` carries (per hex, in addition to elevation/climate fields already framed):

| Field | Notes |
|---|---|
| `water_level_m` | f32 |
| `river_discharge_m3_yr` | f32 |
| `hydro_flags` | `HydroFlags` (`u16`) — not packed with ice |
| `ice_mask` | separate `Vec<bool>` (clearer scrub than packing into flags) |
| `soil_fertility` | f32 |
| `soil_class` | `SoilClass` |
| `flow_direction` | for river overlay rebuild on scrub |
| `salt_accumulated` | salt-flat tint on scrub |

Whole-frame budget ≈ **36 B/cell** in the UI formula (includes elevation + climate + hydrology extras). Scrub rule stands: render only what a frame carries (including river overlay rebuild when `flow_direction` / discharge change).

---

## 13. Events

Granularity per Doc 06 §6.3; emitted, never consumed. Registry-diff events key on stable `WaterBodyId`.

| Event | Significance | Trigger |
|---|---|---|
| `OceansBeginForming` / `OceansStabilized` | Major | moves here from climate formation |
| `SeaLevelMilestone { level_m, delta_m }` | Notable; Major ≥ 50 m | replaces tectonics' `SeaLevelChange` |
| `LakeFormed` / `LakeDried { body }` | Notable; Major ≥ 50 hexes | registry diff |
| `InlandSeaIsolated` / `InlandSeaReconnected { body }` | Major | ocean-connectivity change (Messinian) |
| `SaltLakeFormed { hex, salinity }` | Notable | first SaltLake registry appearance |
| `SaltFlatFormed { region }` | Minor | first SaltFlat / dry+salty appearance |
| `RiverCourseShifted { region }` | Notable | Major-river path diverges ≥ N hexes (avulsion) |
| `GlacialMaximum { sea_level_drop_m }` | Pivotal | ice-volume peak |
| `FjordsCarved { region }` | Notable | FJORD flags appear on retreat |
| `OasisFormed { hex }` | Notable | OASIS flag appears |
| `GreatSpringEmerges { hex }` | Minor | karst resurgence above discharge threshold |

---

## 14. Determinism and Performance

**Determinism:** pure derivations; the only RNG stream is `"hydrology.subhex"` (render-time, never simulation state). Fixed orders everywhere (sorts/heaps/BFS tie-broken by `HexId`; `BTreeMap` only); f64 fixed-order accounting; fixed 24-iteration bisections; accumulators follow the `fertility` precedent. Byte-identical replay gated (§15 #10).

**Budgets** (baseline: 4 B-year run ≈ 127 s at subdiv 7, Doc 06 v0.13):

| Item | Budget |
|---|---|
| Hydrology tick, subdiv 7 | ≤ 5 ms (flood ~1, drainage ~1, groundwater+lakes+regime ~1.5, sediment+ice+soil ~1.5) |
| Hydrology tick, subdiv 8 | ≤ 15 ms |
| Full 4 B run overhead, subdiv 7 | ≤ +25 s |
| New bulk arrays | ≤ **48 B/cell** (≈ 3.1 MB at subdiv 8) — §2.4 hydrology fields + `hydro_elevation_delta_m` + `continental_crust` + `ice_load_m` + `u16` flags |

`GENESIS_SLOW_TICK_STEP_MS` instrumentation (hydrology should log slow steps the same way tectonics does); scratch buffers reused, zero allocation in per-tick loops.

---

## 15. Validation Criteria

Doc 06 §11 pattern: cheap per-tick debug asserts + `--ignored` deep-time gates at 200 M / 1 B / 4 B.

**Status note (v0.6):** gates #1, #2 (shape), #4 (shape), and #19 ship as default-CI unit tests. Gates #3, #5–#18, #20 and perf (#11) are `--ignored` deep-time / full-stack tests (P2-34). Phase 2 hydrology **exit** requires the ignored suite to pass on the validation seed — not merely to exist as stubs.

1. **Conservation:** identity error < 1e-6 relative, every tick (now includes groundwater).
2. **Sea-level dial:** monotonic land-fraction response over a 3-point inventory sweep.
3. **Glacial excursion:** 60–130 m drawdown at glacial max; land fraction measurably rises (bridges).
4. **Honest rivers:** every river hex continues strictly downstream or terminates in water; discharge non-decreasing along trunks (deposition tolerance); zero rivers before oceans.
5. **Drainage realism:** largest basin within Earth-plausible fraction of its continent; Major count at 1 B in [3, 30] (subdiv 7).
6. **Endorheic realism:** ≥ 1 endorheic lake in an arid interior at 1 B; hyper-arid sweep → endorheic drainage fraction exceeds oceanic.
7. **Deltas:** ≥ half of stable Major mouths show progradation after 500 My.
8. **Cretaceous beach:** high-`fertility` uplifted hexes rank top-decile in `soil_fertility`.
9. **Salt story:** ≥ 1 SaltLake or SaltFlat by 2 B on default world.
10. **Determinism:** byte-identical `WorldData` (all new arrays + registry) at 200 M / 1 B / 4 B.
11. **Perf:** §14 budgets hold.
12. **Tectonic gates stay green** with hydrology active (erosion hand-off must not destabilize deep time).
13. **Perennial/ephemeral:** mountain-fed desert trunks stay perennial (Nile pattern); unfed desert channels flag EPHEMERAL; distinction flips correctly on a humid↔arid parameter sweep.
14. **Karst:** limestone / calcareous / high-fertility sedimentary belts show springs and surface-discharge thinning; karst spring discharge re-emerges downstream (network mass balance holds through the diversion).
15. **Fjords:** after ≥ 1 full glacial cycle, FJORD flags exist on glaciated high-relief coasts (seed-swept).
16. **Loess belt:** post-retreat, `Loess` soil exists downwind of former ice margins and ranks top-decile fertility.
17. **Seasonality:** monsoon-regime rivers top-quartile `discharge_seasonality`; equatorial `Stable` rivers bottom-quartile; Nival regimes appear only where winters freeze.
18. **Oases:** on a seed with arid basins adjacent to wet highlands, OASIS flags appear along the connecting flow paths.
19. **Thermosteric sign & scale:** a greenhouse-intensity sweep (same seed, same inventory) shows warmer worlds standing 5–25 m higher at equal ice state; icehouse worlds lower. Sea level must respond to climate alone, water mass unchanged.
20. **Post-glacial rebound:** hexes deglaciated for ≥ 10 My stand measurably higher than at their glaciated minimum; at least one formerly-iced coast shows emergent (newly dry) shoreline hexes within 50 My of deglaciation.

---

## 16. Open Questions (tracked refinements — the systems themselves are in)

1. Lake fill/drain relaxation constant for Recent-era fine ticks (instant equilibrium is correct at 500 ky).
2. Salt burial/recycling over deep time (currently monotonic forever).
3. Delta compaction & isostatic loading response (Mississippi-delta subsidence).
4. Tidal amplification by coastal geometry (funnel bays); tidal bores.
5. Aquifer overdraft / fossil water as a civilization-era mechanic (Doc 10 consumes §6 fields).
6. Channel character (braided vs meandering) as a rendering/naming refinement.
7. Sub-hex cave network content generation (karst flags exist; content is Doc 14/15 scope).
8. Excess-ice permafrost dynamics (thermokarst lake fields) beyond the drainage effect.

---

## 17. Integration & Migration (ships inside this phase's prompts)

### 17.1 Tectonics (Doc 06 edits)

- **§4.7 Sea Level Drift retired — superseded by emergence, not merely deleted:** `update_sea_level`, the ridge-length heuristic, and tectonics' `SeaLevelChange` are removed; hydrology owns `sea_level_m`. The physical effect §4.7 approximated (active ridges displacing water) now arises for real from the flooding solve, because young shallow ridge crust is in the bathymetry (§3.5 factor table). Tectonics reads the derived level one tick lagged (freeboard, coast logic) as before, and — per §3.5 rule 1 — no tectonic pass may cache a hex's wet/dry status across ticks.
- **§8.2 routing + §8.3 threshold superseded** by §8 here; tectonics keeps isostasy, rebound, collapse, dry-weathering fallback.

### 17.2 Climate (Doc 07 edits)

- §3.5 → condensed-fraction curve (§3.3); formation ocean events move here.
- Distance-to-ocean/currents/basins consume hydrology's previous-tick ocean mask; **`distance_to_ocean_km` upgrades to distance-to-water including bodies ≥ `LAKE_CLIMATE_MIN_HEXES`** (lake effect, §2.3).
- Glaciation state gains the ice-volume mapping (§9.1) and may read ice-mask extent.

### 17.3 Rendering / UI / Frames

Water-aware ramp, river overlay + LOD thresholds, Soil mode, dashed ephemerals, fjord/oasis/estuary display, `HistoryFrame` fields, Water-inventory knob.

---

## 18. Implementation Prompt Plan (estimate)

Continues Phase 2 numbering: **P2-20 … ~P2-34.**

1. **P2-20** — Crate scaffold, `HydrologyParameters`, schema fields/ids, layer registration
2. **P2-21** — Water budget + flooding solve + derived sea level incl. thermosteric term (§3.5.1); retire Doc 06 §4.7; conservation + factor-model gates
3. **P2-22** — Formation condensation (Doc 07 edit); ocean-event ownership move
4. **P2-23** — Routing surface + drainage + discharge
5. **P2-24** — Lake balance, depression tree, registry, salt, candidate seas
6. **P2-25** — Groundwater: recharge/storage/baseflow, water table, springs, oases, karst
7. **P2-26** — Seasonal regime, floods, perennial/ephemeral, permafrost effect
8. **P2-27** — Erosion hand-off: hillslope + stream power + sediment; tectonics retirement; deep-time recalibration
9. **P2-28** — Ice masses + glacial sea-level coupling + GIA loading/rebound (§9.1) + land-bridge & rebound gates
10. **P2-29** — Glacial carving: overdeepening, fjords, moraine lakes, outwash, loess
11. **P2-30** — Soil system + fertility blend + both fertility gates
12. **P2-31** — Coastal: tides, estuaries, intertidal wetlands
13. **P2-32** — Climate migration (ocean mask + distance-to-water/lake effect)
14. **P2-33** — Rendering: water ramp, river LOD, Soil mode, frames, menu knob
15. **P2-34** — Events, validation suite completion, performance pass, 3-seed × 3-inventory calibration sweep, Phase 2 exit review

Sub-hex creek synthesis (§12.3): specified here, implemented with Doc 14.

---

## Appendix A. Prior Art: the Removed Provisional Model

A provisional surface-flow model shipped briefly in Phase 2/3 and was removed in P1-22 (changelog v0.2). Kept from it: the priority-flood routing surface (§4.1), steepest-descent + `HexId` tie-breaking (§4.3), the runoff formulation shape (§4.2), and the render lesson that overlays rebuild only on frame change (§12.2). Why it died: water rendered before oceans existed; lakes were thresholds, not climate consequences; one view-independent river threshold; no conservation; no terrain feedback. Its honest-routing property survives as Goal 3 and Gate 4.

---

*End of Hydrology & Soil Module Specification.*
