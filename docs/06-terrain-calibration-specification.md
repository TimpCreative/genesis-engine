# Terrain Calibration & Tuning — Doc 06 Companion Specification

**Document:** `06-terrain-calibration-specification` (v0.1, draft) — a companion to
Doc 06 (Tectonics), like `06-REVIEW`. Referenced in code as **Doc 06-CAL** to
disambiguate its section numbers from the Tectonics module spec. (Originally
drafted as "Doc 10"; renumbered because slot 10 is Civilization — the doc map
is fully allocated 00–20.)
**Status:** Design approved (3 forks locked); Phases 0–2 & 4 implemented, Phase 3 retracted (see §9)
**Repository:** [TimpCreative/genesis-engine](https://github.com/TimpCreative/genesis-engine)
**Supersedes:** the corrective-pass approach in Doc 06 §6–8 (heal / basin_infill / coast_cleanup / collapse-as-corrector) for *absolute elevation* and *land coverage*. Doc 06 continues to own plate **structure**; this doc owns **magnitudes and targets**.

---

## 1. Purpose

Make terrain **tunable and realistic at the same time**, so that headline properties — land coverage %, mountain density, island density, river/fertile-land density, shelf shape — are **settings we solve for**, not chaotic outputs we chase. This is the foundation for user-facing sliders that move one axis without obliterating the others.

### 1.1 The problem this replaces

The engine runs a **chaotic forward simulation** and reads its targets as **emergent outputs**. Consequences we have hit repeatedly:

- Land coverage is not settable — only `initial_continental_fraction` and `water_inventory_gel_m` are nudged in hope. The realized value wanders.
- The deep-time land fraction sits on a hard validation gate (~0.149 vs a 0.15 floor). Because it is emergent, **any morphology fix perturbs the chaotic trajectory and tips the gate** — filling interior pits breaks land %, and vice-versa. This is the multi-day whack-a-mole.
- Seven passes (`continental_heal`, `basin_infill`, `coast_cleanup`, `collapse`, `accretion`, `subduction_erosion`, display de-speckle) exist **only to bound the emergent absolute-meter field**. They interact and fight.
- No systematic continental-shelf / slope band: baselines jump from `CONTINENTAL_BASELINE_M = +800` straight to `OCEANIC_BASELINE_M = −4000` ([plate_surface.rs](../crates/genesis_tectonics/src/plate_surface.rs)), so a single hex step goes shelf → abyss ("Mariana at the beach").
- The land/ocean **datum floats**: `bathtub_level_m` ([solve.rs](../crates/genesis_hydrology/src/solve.rs)) puts sea level at ~−2345 m absolute in deep time, so dry land reads as a negative number and every tool/agent "fixes" the wrong thing.

### 1.2 The seed of the answer is already in the code

`bathtub_level_m` does not *emerge* the sea level — it **solves** it: pick a water volume, solve for the level that floods the hypsometry. That is exactly the right pattern, applied in exactly one place, for the wrong variable (water volume instead of land %). **This spec generalizes that pattern to all of terrain calibration.**

---

## 2. Core principle — separate WHERE from HOW MUCH

Two concerns are currently tangled in one absolute-meter field. We split them:

| Concern | Owner | Character |
|---|---|---|
| **Structure — WHERE** things are (continents, orogenic belts, ridges, basins, coastlines, cratons) | Plate-tectonics forward sim (Doc 06) | Emergent, physical, coherent. **Kept in full** (deep-time Wilson cycles, supercontinents). |
| **Calibration — HOW MUCH** (meters, land %, mountain/island/river counts, shelf profile, datum) | A **solved transform** onto parameterized target curves | Deterministic, tunable, re-solved each calibration. **New.** |

The physics decides *arrangement*; calibration decides *magnitudes and counts by construction*. Because calibration is re-solved every tick, **a morphology change can never break a target** — we simply re-solve. The chaotic-gate fragility and the whack-a-mole both disappear.

### 2.1 Three design decisions (locked)

1. **Solve-to-target** control model — transfer the physics field onto target curves; targets guaranteed by construction; realism preserved by re-injecting high-frequency structural detail (§5.4).
2. **Structure engine** — keep the full deep-time forward sim; its output contract becomes a *relative* field + labels (§4).
3. **Datum pinned to sea level = 0** — stored elevation *is* height above sea; eustasy becomes a small bounded offset, not a ±2 km floating datum (§6).

---

## 3. The knob surface (the slider contract)

All targets live in a new `TerrainTargets` sub-struct of `CoreParameters` (sits beside `GeologyParameters` / `HydrologyParameters` in [parameters/core.rs](../crates/genesis_core/src/parameters/core.rs)). Every field is `(default, min, max)` and, because targets are solved not emergent, **moving one slider moves only its own axis**.

| Knob | Default | Band | Controls |
|---|---|---|---|
| `land_fraction` | 0.29 | 0.05–0.95 | Area above sea (§5). Exact by construction. |
| `land_fraction_wander` | 0.08 | 0.0–0.20 | Allowed ± per-year excursion around the setpoint (§7). |
| `continental_modal_height_m` | 300 | 0–1500 | Modal land elevation. |
| `orogeny_intensity` | 1.0 | 0–3 | Fatness/height of the mountain (upper) tail → mountain count & height. |
| `abyssal_depth_m` | −4000 | −6000…−2000 | Ocean modal depth. |
| `trench_depth_m` | −9000 | −11000…−4000 | Deep ocean tail. |
| `shelf_fraction` | 0.06 | 0–0.20 | Share of area in the shallow shelf band (fixes "abyss at the beach"). |
| `shelf_depth_m` | −140 | −500…0 | Shelf-break depth. |
| `slope_width_frac` | 0.03 | 0–0.15 | Continental-slope band width. |
| `hypsometric_bimodality` | 1.0 | 0.3–2.0 | Sharpness of the land/ocean split. |
| `island_density` | 1.0 | 0–3 | Oceanic high-spot (island/arc) seeding rate. |
| `river_density` | 1.0 | 0–3 | Number of major fertile river valleys (discharge-percentile cut). |

`water_inventory_gel_m` stays, but its job narrows to **water distribution** (lakes, interior seas, eustatic wobble) — no longer the land/ocean datum (§6).

---

## 4. Layer 1 — Structure engine (output contract change)

Doc 06 keeps producing plates, boundaries, ages, hotspots, reorganization, and erosion. **Change:** its terrain output becomes a *relative, unitless* **tectonic potential** `Φ[i]` per hex plus **labels**, not absolute meters.

- `Φ[i]` — a single scalar where continental crust sits high and oceanic low, with structural contributions added in standardized units: orogeny units along convergent belts, age/thermal-subsidence in ocean basins, craton buoyancy, hotspot bumps, rift thinning. Sign/scale are arbitrary; only **rank matters**.
- Labels per hex (already largely present): `continental_crust`, boundary class, craton vs. margin, oceanic age, hotspot/arc membership. These drive regime-aware mapping and feature controllers.

Hard-coded elevation baselines (`+800`, `−4000`, `−4500`) are **removed** from the structure layer; magnitudes are the calibration layer's job. Erosion/collapse are retained but reframed as **smoothing operators on `Φ`** (relative relief), not fighters of absolute meters.

---

## 5. Layer 2 — Hypsometry transfer (the tunable heart)

### 5.1 Target hypsometric curve `H(p)`

A monotone (PCHIP) function from **area-percentile** `p ∈ [0,1]` (ascending) to **elevation in meters, sea level = 0**, built from the §3 knobs as control points. Earth-like bimodal shape with an explicit shelf/slope band:

```
p = 0.00                          → trench_depth_m           (deep ocean tail)
p ≈ 0.03                          → abyssal_depth_m
p = ocean_frac − slope_width_frac → continental-slope top
p = ocean_frac − shelf_fraction   → shelf_depth_m            (shelf band: shallow)
p = ocean_frac  (= 1−land_frac)   → 0                        (coastline / datum)
p = ocean_frac + coastal_band     → +coastal plain
p ≈ 0.90                          → continental_modal_height_m
p → 1.00                          → orogeny tail scaled by orogeny_intensity  (peaks)
```

The curve **guarantees the distribution**: area above `p = 1 − land_fraction` is land, so land %, shelf share, mountain-tail fatness, and abyssal depth are all exact by construction, independent of what the physics did.

### 5.2 Mapping

Sort hexes by `(Φ[i], HexId)` ascending → each hex's percentile `p_i = rank_i / (N−1)`. Assign `elevation_mean[i] = H(p_i)`. Because continental crust has higher `Φ` by construction, the top-`land_fraction` hexes are predominantly continental; the flex zone (where `land_fraction` differs from crust fraction) lands on continental margins as drowned shelf or emergent coastal plain — geologically sensible, and it **decouples land coverage from crust fraction** (the key win).

### 5.3 Determinism

Sort key `(Φ, HexId)`; monotone curve; no RNG in calibration. Byte-identical given identical `Φ`. (RNG stays only in structure seeding, already deterministic.)

### 5.4 Realism guard (avoid the "equalized" look)

Global histogram-matching alone flattens local variety. Mitigation:

1. **Map the low-frequency component to the curve**: compute `Φ_lo` = neighbor-smoothed `Φ`; use `p_i` from `Φ_lo` for the macro assignment.
2. **Re-inject high-frequency detail**: add back `k · (Φ[i] − Φ_lo[i])` scaled per regime, so mountains keep texture and valleys keep cutting.
3. **Regime-aware** stitching where needed (continental vs. oceanic ranked so a fjord isn't equalized against an abyssal plain).

Result: guaranteed macro-distribution **and** physical meso/micro structure.

---

## 6. Datum & water (fork 3)

- `sea_level_m ≡ 0` is the pinned land/ocean datum (the curve defines everything relative to 0). Stored `elevation_mean` is **height above sea** — mountains positive, ocean negative. This kills the "−2000 m is dry land" confusion at the source (no inspector band-aid needed).
- **Eustasy** becomes a small bounded offset around 0 driven by ice volume + thermosteric expansion (physically ±~120 m glacial, not ±2 km). `water_inventory_gel_m` and the ice/thermosteric terms drive this offset.
- **Water distribution** (the hydrology flooding solve, candidate seas, lakes, endorheic basins) runs unchanged against the pinned datum: the open ≤ 0 basin is ocean; interior basins adjudicate as today. Genuinely deep inland basins fill with water → read blue, never dry pits.

---

## 7. Layer 4 — Temporal controller (land % over time)

Requirement: land % wanders **±5–10 % per year** but averages **within 1–5 % of goal** over the whole sim. This is a setpoint controller with a slack band:

```
raw_signal   = f(Φ distribution)         // continental assembly ↑, dispersal ↓
land_eff     = land_fraction + land_fraction_wander · tanh((raw_signal − mean)/scale)
```

`land_eff` feeds the curve's `ocean_frac` each calibration. The band is symmetric and excursions bounded, so the **long-run mean → setpoint** while natural Wilson-cycle variation still breathes within the band. A low-pass on `raw_signal` prevents per-tick jitter. Deterministic.

---

## 8. Layer 3 — Feature-density controllers

Operate on the calibrated field, each a knob hitting its count by construction:

- **Mountains / high spots** — the curve's upper tail (`orogeny_intensity`); belts land where convergent boundaries are.
- **Islands / low spots** — seed oceanic high-`Φ` bumps at hotspots/ridges at `island_density`; the curve lifts them above sea.
- **Rivers / fertile land** — hydrology already computes discharge; select channels above a discharge percentile tuned to `river_density`; fertility follows floodplains (existing mechanic).

---

## 9. What we keep / reframe / remove

- **Keep:** motion, boundary detect/classify, hotspots, reorganization, partition, projection/rebuild — the structure engine. Hydrology flooding/lakes/rivers. The bathtub *machinery* (repurposed for eustasy + water distribution).
- **Reframe:** erosion & collapse → smoothing operators on `Φ`. GEL/bathtub → water distribution + eustatic offset, not the datum.
- **Retain (Phase 3 finding — deletion retracted):** `continental_heal`, `basin_infill`, and `coast_cleanup` were expected to be redundant, but they clean the raw **structure** the calibration then maps — specifically the multi-hex accreted-oceanic interior pits that the smoothed ranking does *not* dissolve (it lifts isolated 1-hex lows, not whole basins). Gating them off under calibration **doubled** the dry sub-sea perforation (129 → 269 @ subdiv 7, 1B), so they earn their place as structure conditioning and stay. `subduction_erosion` also stays (a gate bounds crust fraction). The hard baseline constants and the "structure emits unitless Φ" refactor are **deferred** — high risk to a result the user is happy with, low user-visible benefit.

---

## 10. Phased migration (never in a broken state)

**Phase 0 — Transfer as a final pass (fast, reversible win).**
Add `TerrainTargets` + `H(p)` + the §5 transfer as the *last* calibration step on top of today's sim, mapping the emergent field onto the target curve. Pin datum to 0. Immediately delivers exact land %, hypsometry, shelf band, and kills interior pits. Corrective passes still present but now no-ops on the calibrated output. **Gate:** land % hits target within 1 % at 1B/2.5B/4.5B; no dry sub-sea interior cells; shelf band visible.

**Phase 1 — Feature controllers + realism guard.** Add mountain/island/river density knobs and the low-freq/high-freq split (§5.4). **Gate:** mountain/island/river counts track their knobs; terrain reads physical, not equalized.

**Phase 2 — Temporal controller.** Add §7. **Gate:** per-year wander within band; long-run mean within 1–5 % over a full 4.5B run.

**Phase 3 — Delete the corrective passes + hard baselines.** Turn off, prove the curve holds, remove. Slim the structure layer to emit `Φ` + labels. **Gate:** all existing tectonics/hydrology gates green with the passes gone.

**Phase 4 — Surface the sliders.** Wire `TerrainTargets` into the settings/menu UI with ranges + live re-generation.

Each phase is independently shippable; Phase 0 alone resolves land %, interior pits, shelves, and the datum confusion.

---

## 11. Verification (per phase)

1. `cargo test` across `genesis_tectonics` / `genesis_hydrology` (incl. `--ignored` deep-time gates) green.
2. Determinism A/B: same seed twice → byte-identical summary.
3. Knob sweeps: vary each `TerrainTargets` field across its band → the intended axis moves, others hold (the "doesn't obliterate" property).
4. Headless dump + subdiv-7/8 elevation screenshots at 1B/2.5B/4.5B: coherent continents, shelf→slope→abyss profile, no dry interior perforation, land % on target.
5. Perf: calibration is O(n log n) (one sort) per tick — confirm it stays within the tick budget at subdiv 8.

---

## 12. Open questions

- Exact `Φ` composition (weights of orogeny/subsidence/craton/hotspot terms) — calibrate against the Earth hypsometric curve.
- Whether the transfer runs every geological tick or every N ticks (perf vs. responsiveness).
- Regime-aware ranking: single global `Φ` (simpler) vs. separate continental/oceanic ranks stitched at the shelf (more control). Start global; revisit if margins look wrong.
