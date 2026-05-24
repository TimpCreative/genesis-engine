# Phase 1 Tectonics — Implementation Review (P1-1 through P1-10)

**Document:** `06-REVIEW` (companion to [`06-tectonics-module-specification.md`](./06-tectonics-module-specification.md) v0.2)  
**Status:** Phase 1 implementation complete (Doc §15 steps 1–10)  
**Repository:** [TimpCreative/genesis-engine](https://github.com/TimpCreative/genesis-engine)  
**Review period:** May 2026  
**HEAD at review:** `960657a` (`Render hexes by elevation for geology smoke test (P1-10)`)

This document is a **full record** of the Phase 1 geology prototype: what was built, which git commits landed it, which agent prompts drove each slice, how slices were validated, and how to run or inspect the result today.

---

## 1. Purpose and scope

### 1.1 What Phase 1 set out to prove

Genesis Engine’s core bet for geology is that a **deterministic**, **hex-keyed**, **multi-million-year** plate-tectonics loop can run fast enough to be useful — and produce Earth-plausible bulk terrain in `WorldData` before climate, biology, or civilization layers exist.

Phase 1 (Doc 06) delivers:

- Initial **plate partition** at Formation (year 0)
- **Euler-pole drift** and Voronoi **re-partition** each Geological tick
- **Boundary detection and classification**
- **Elevation and bedrock** updates from boundary type
- **Volcanism** (boundary arcs + mantle hot spots)
- **Erosion**, sediment tracking, shallow-sea **fertility**
- **Plate reorganization**, **sea level drift**, and a **tectonic event chronicle**
- **Validation** (§11 metrics, determinism, perf budget)
- **Elevation-colored rendering** so humans can see the result

### 1.2 What this review does *not* cover

- **Phase 0** (grid, `WorldData`, ticks, render scaffold) — see commits before `01ef998`
- **Phase 2+** (climate-aware erosion, hydrology, biology, full 4.5B-year production runs)
- **Doc edits** during implementation (user-owned unless explicitly requested)

### 1.3 Agent session archive

Implementation was done in Cursor agent sessions. The **primary transcript** for P1-1 validation through P1-10 (prompts, validation reports, commit message drafts) is:

- [Agent transcript `f65ad57f-3ee2-40d2-a5d3-691aa44ee898`](https://github.com/TimpCreative/genesis-engine) — local path: `.cursor/projects/.../agent-transcripts/f65ad57f-3ee2-40d2-a5d3-691aa44ee898.jsonl`

Prompts were authored with [`00-prompt-template.md`](./00-prompt-template.md). **Full prompt bodies** are in that transcript (search for `=== START PROMPT ===`); this review summarizes each prompt’s **task** and records **outcomes**. Re-pasting entire prompts here would duplicate thousands of lines without adding authority — the transcript is the canonical prompt archive.

---

## 2. Mapping: Doc §15 steps ↔ prompts ↔ commits

| Doc §15 step | Prompt ID | Primary commit(s) | Notes |
|--------------|-----------|-------------------|--------|
| 1. Plate generation and storage | **P1-1**, **P1-1.5** | `01ef998` | P1-1.5 remediated RNG/centroid/growth; folded into same era as P1-1 |
| 2. Plate motion and re-partition | **P1-2** | `7f2444c` | |
| 3. Boundary detection and classification | **P1-3** | `2fb3654` | |
| 4. Elevation updates per boundary type | **P1-4** | `044d5c3` | Formation terrain + per-tick boundary elevation |
| 5. Hot spots | **P1-5**, **P1-5.1**, **P1-6** | `0217472`, `6f5e935` | P1-5 volcanism; P1-5.1 `stream_at`; P1-6 hot spots (squash commit documents both) |
| 6. Erosion and bedrock evolution | **P1-7** | `fb0843e` | |
| 7. Plate reorganization | **P1-8** | `24d4062` | With sea level + events |
| 8. Event emission and granularity | **P1-8** | `24d4062` | |
| 9. Integration and validation | **P1-9** | `24393f8` | |
| 10. Rendering integration | **P1-10** | `960657a` | |

**Intermediate / squash commits** (still on `main`, useful for archaeology):

| Commit | Role |
|--------|------|
| `0217472` | Volcanism + `stream_at` + event flush before the large P1-1–P1-6 squash message |
| `6f5e935` | Comprehensive squash documenting P1-1–P1-6 in one message |

---

## 3. Git commit chronology (GitHub `main`)

Remote: `https://github.com/TimpCreative/genesis-engine.git`

Commits from first tectonics code through P1-10 (newest first):

### `960657a` — P1-10

```
Render hexes by elevation for geology smoke test (P1-10)

Complete Doc 06 Phase 1 step 10: replace Phase 0 HexId rainbow coloring with
deterministic elevation-based fills from WorldData.

genesis_render:
- elevation_color / hex_fill_color: piecewise-linear ramp from -11_000 m (deep
  ocean blue) through sea_level_m (shallow water / coast) to +9_000 m (snow).
  Pentagons use the same ramp; five-sided geometry distinguishes them.
- render_world_if_dirty reads elevation_mean and sea_level_m per hex.
- Export MIN_ELEVATION_M, MAX_ELEVATION_M; unit tests for determinism, ocean
  depth, land vs ocean, peak vs lowland, and clamp.

genesis_app:
- Window title "Genesis Engine — Geology Smoke Test".
- Log genesis_tectonics::summarize_world after generate_full_history_with_tectonics.

Read-only visualization; no simulation changes. R regenerate still uses
non-tectonic create_world (pre-existing).

Phase 1 tectonics (Doc 06 steps 1–10) is now complete.

Co-authored-by: Cursor <cursoragent@cursor.com>
```

### `24393f8` — P1-9

```
Add Doc 06 §11 validation suite, determinism gates, and perf budget (P1-9)

Codify Phase 1 tectonics integration proof without rendering: fixed-seed
(42) metrics, connected-component checks, and summarize_world for diagnostics.

validation.rs:
- Continental fraction, elevation bounds, sea level, plate count, BFS mountain
  regions (>3000 m), deep ocean basins (scaled min area min(1000, n/4)), and
  Phase 1 bedrock diversity (Igneous/Sedimentary/Metamorphic/OceanicCrust;
  Limestone deferred per §8.4).
- run_validation_world() at subdiv 5; quick target 1M years (CI), full target
  100M years (#[ignore]) for mountains, basins, bedrock, and Notable event
  volume (500–15_000 at 100M; quick proxy >0 at 1M).

Integration tests:
- validation_quick_suite_passes / validation_full_suite_passes (ignored)
- world_data_identical_after_validation_run (tectonics WorldData fields)
- event_granularity_pivotal_logs_only_pivotal_events (§12.5)
- tectonics_full_history_completes_within_budget (10M years < 30s; subdiv 7
  profiling test ignored)

layer.rs: debug_tick_step logs per-step elapsed_ms when RUST_LOG=debug (§9.3).

Loose bounds documented: land fraction 20–40% (§17 Q6), event cap above §6.4
nominal at 100M years. Run long suite: cargo test -p genesis_tectonics -- --ignored

Next: P1-10 elevation rendering.
Co-authored-by: Cursor <cursoragent@cursor.com>
```

### `24d4062` — P1-8

```
Complete Phase 1 tectonic simulation loop: reorganization, sea level, events (P1-8)

Finish Doc 06 Geological-era steps 7–9 so each tick runs erosion, then plate
reorganization, sea level drift, and chronicle emission. This closes the
core tectonics pipeline started in P1-1–P1-7 (plates → boundaries → terrain →
volcanism → hot spots → erosion → reorg/sea level/events).

genesis_core:
- Extend EventKind with WorldFormation, PlateReorganization, MountainRangeFormed,
  OceanBasinOpened, BoundaryTransition, and SeaLevelChange (plus existing
  VolcanicEruption and HotSpotActivity).
- Add BoundaryType and PlateReorgAction with serde round-trip tests.
- Add geology_activity_scale to GeologyParameters (default 1.0, validated).

genesis_tectonics:
- events.rs: alloc_event_id and maybe_emit for event_granularity gating; volcanism
  and hot spots refactored to use shared emission (terrain still applies when
  below threshold).
- reorganization.rs: per-tick split (40%) / motion change (40%) / merge (20%) via
  stream_at(reorganization_check|action); extinct-plate purge after 10M years
  empty; optional split-boundary subsidence; repartition after reorg.
- sea_level.rs: divergent-boundary-length delta (×1e-6), equilibrium damping,
  optional ±100 m excursion on reorg; SeaLevelChange events (Notable if |Δ|>50m).
- boundary_events.rs: MountainRangeFormed (CC uplift/peak heuristics),
  OceanBasinOpened (divergent subsidence/oceanic crust), BoundaryTransition
  (canonical edge dedup vs previous tick).
- layer.rs: tick order — snapshot elevation → boundary elevation → volcanism →
  hot spots → erosion → reorg (re-detect boundaries if fired) → sea level →
  boundary events → clamp_terrain; WorldFormation (Pivotal) at Formation.
- plate.rs: last_nonempty_year, elevation_at_tick_start, previous_edge_class,
  baseline_divergent_length_km; motion.rs: sample_motion_axis for reorg reuse.

Tests: integration at 1M years — WorldFormation, SeaLevelChange at Trace,
|sea_level_m| ≤ 200, plate count in [5, 15], deterministic event log; unit
tests for reorg, sea level, boundary events, and granularity.

Deferred to P1-9: Doc §11 full validation suite and perf budget; P1-10: render
by elevation. Known doc/code notes: stream_at vs §4.4 stream(name); separate
BoundaryType in core vs BoundaryClass in tectonics.

Co-authored-by: Cursor <cursoragent@cursor.com>
```

### `fb0843e` — P1-7

```
Add geological erosion, sediment routing, and shallow-sea fertility (P1-7)

Implement Doc 06 §8 on each Geological tick after hot spots: land erosion
from base_erosion_rate_per_year and tick interval, per-hex noise via
stream_at("tectonics.erosion_noise", tick_year), downhill routing to the
lowest neighbor (tie-break lowest HexId), and cumulative deposition on
TectonicsState for Igneous/Metamorphic → Sedimentary at 500 m. Increment
monotonic fertility for submerged tropical shallow shelf hexes (+0.001 per
tick). Phase 1 climate_modifier stays 1.0; single clamp_terrain at end of
the erosion step (removed hotspot mid-tick clamp).

Tests: unit coverage in erosion.rs; integration for mean land elevation
vs zero-rate control, fertility accumulation, and 1M-year elevation/fertility
determinism.

Co-authored-by: Cursor <cursoragent@cursor.com>
```

### `6f5e935` — P1-1 through P1-6 (squash documentation)

```
Add genesis_tectonics: Phase 1 geology through hot spots (P1-1–P1-6)

Introduce the Phase 1 tectonic simulation crate and wire it into world
generation so seeded worlds get plates, moving boundaries, sculpted
terrain, subduction volcanism, and mantle hot spots before erosion or
the event chronicle are complete.

[... extensive body: tick order, RNG streams, Formation, determinism,
explicitly not in commit: erosion §8, reorganization §4.5, full events §6,
§11 validation, elevation render ...]

Co-authored-by: Cursor <cursoragent@cursor.com>
```

*(Full message is in git; abbreviated here. See `git show 6f5e935` for the complete squash narrative.)*

### `0217472` — P1-5 / P1-5.1 era

```
Add genesis_tectonics: Phase 1 geology through boundary volcanism

Introduce deterministic plate tectonics for Genesis Engine: growth-based
formation, Euler-pole drift with Voronoi repartition, boundary classification,
terrain sculpting, and subduction-arc volcanism with VolcanicEruption events.

- genesis_tectonics crate (no Bevy): plates, motion, partition, boundaries,
  formation terrain, boundary elevation, volcanism
- genesis_core: geology params, fertility, TickCoordinator year-0 fix,
  advance_with_coordinator, WorldRng::stream_at for per-tick streams
- genesis_app: history to 1M years before render

WorldRng::stream_at fixes per-tick volcanism replay; Formation streams unchanged.
Rendering still uses hex index colors until a follow-up reads elevation_mean.

Co-authored-by: Cursor <cursoragent@cursor.com>
```

### `044d5c3` — P1-4

```
feat(tectonics): formation terrain and boundary-driven elevation (P1-4)

Set continental and oceanic baseline elevation and bedrock at Formation,
then sculpt terrain each Geological tick from classified plate boundaries
(divergent subsidence, convergent orogeny/subduction, transform bedrock).

Uses BoundaryInfo from P1-3, accumulates hex deltas in HexId order, and
clamps to Doc 06 §5.7 bounds. Subduction side rules: OC by owner plate type;
OO by faster plate (tie → lower PlateId). Render unchanged; elevation is in
WorldData for a future render pass.

Co-authored-by: Cursor <cursoragent@cursor.com>
```

### `2fb3654` — P1-3

```
feat(tectonics): boundary detection and classification (P1-3)

Detect boundary hexes and classify cross-plate edges after each Geological
tick repartition. BoundaryInfo on TectonicsState stores sorted boundary hexes,
plate contacts, and owner-centric directed edges (divergent, convergent with
subtype, transform) from relative surface velocity at hex centers.

- genesis_tectonics: boundary.rs, surface_velocity_m_per_year, layer wiring
- Deterministic scan (ascending HexId), BTreeMap/BTreeSet, no RNG in §3
- tracing::debug boundary hex count per tick; elevation unchanged

Co-authored-by: Cursor <cursoragent@cursor.com>
```

### `7f2444c` — P1-2

```
feat(tectonics): plate motion, Voronoi repartition, and tick integration (P1-2)

Add accumulated rotation and effective-position Voronoi repartition on
Geological-era ticks, TectonicsLayer on the tick coordinator, formation at
year 0, and genesis_app wiring so worlds have real plate assignments.

Co-authored-by: Cursor <cursoragent@cursor.com>
```

### `01ef998` — P1-1 (+ P1-1.5 remediation content)

```
feat(tectonics): initial plate generation aligned with Doc 06

Implement growth-based seeding in genesis_tectonics, expand GeologyParameters
(major/minor plate counts, event granularity, erosion rate), and add WorldData
fertility. Remediate RNG streams, minor target fractions, centroid-based motion
axes, and Poisson seed fallback per the Phase 1 specification.

Co-authored-by: Cursor <cursoragent@cursor.com>
```

### Pre-tectonics doc baseline (context)

| Commit | Summary |
|--------|---------|
| `8268656` | docs(01,02,06): Phase 1 review feedback; defer planetary formation, chaos |
| `dc41539` | docs(05,06): Doc 06 Tectonics v0.1 drafted |

---

## 4. Prompt index (P1-1 – P1-10)

| ID | Doc step | Prompt task (summary) | Transcript |
|----|----------|----------------------|------------|
| **P1-1** | §15.1 | Create `genesis_tectonics`; growth-based plate seeding; `GeologyParameters` major/minor; `WorldData.fertility`; full `plate_id` coverage at Formation; **no** tick simulation | [Appendix C — P1-1](#p1-1--initial-plate-generation) (transcript `7ab9bbb6`, line ~538) |
| **P1-1.5** | Remediation | Fix RNG stream names (`plate_seeds`, `plate_axes`, `plate_rates`); minor targets 3–7% each; motion axis from **centroid**; random growth neighbor; Poisson fallback | [Appendix C — P1-1.5](#p1-15--remediation) (transcript `f65ad57f`, line ~18) |
| **P1-2** | §15.2 | `accumulated_rotation_rad`; `motion.rs`; `partition.rs`; `TectonicsLayer`; `advance_with_coordinator`; first tick at `world_start_year`; app history to 1M years | [Appendix C — P1-2](#p1-2--plate-motion-voronoi-re-partition-tick-integration) |
| **P1-3** | §15.3 | `boundary.rs`; `BoundaryInfo`; velocity classification; no elevation | [Appendix C — P1-3](#p1-3--boundary-detection-and-classification) |
| **P1-4** | §15.4 | `initial_terrain.rs`; `elevation.rs`; Formation + per-tick boundary sculpting; §5.7 clamp | [Appendix C — P1-4](#p1-4--terrain-sculpting) |
| **P1-5** | §5.5 / events | `volcanism.rs`; `VolcanicEruption`; arc hexes; `events.rs` flush; `event_granularity` | [Appendix C — P1-5](#p1-5--boundary-driven-volcanism) |
| **P1-5.1** | Fix | `WorldRng::stream_at(name, tick_key)`; volcanism per-tick replay bug | [Appendix C — P1-5.1](#p1-51--per-tick-volcanism-rng) |
| **P1-6** | §15.5 | `hotspots.rs`; `HotSpotId`; `HotSpotActivity`; Formation seed + per-tick activity | [Appendix C — P1-6](#p1-6--hot-spots) |
| **P1-7** | §15.6 / §8 | `erosion.rs`; routing; fertility; `stream_at` erosion noise | [Appendix C — P1-7](#p1-7--erosion-sediment-fertility) |
| **P1-8** | §15.7–8 | `reorganization.rs`; `sea_level.rs`; full `EventKind`; `boundary_events.rs`; `maybe_emit` | [Appendix C — P1-8](#p1-8--reorganization-sea-level-events) |
| **P1-9** | §15.9 | `validation.rs`; §11 tests; perf budget; §12.5 pivotal-only log test | [Appendix C — P1-9](#p1-9--validation-and-performance-gates) |
| **P1-10** | §15.10 | `elevation_color` in `genesis_render`; app title + `summarize_world` log | [Appendix C — P1-10](#p1-10--elevation-based-hex-coloring) |

---

## 5. Slice-by-slice implementation record

### P1-1 — Initial plate generation

**Goal:** Every hex has a `plate_id`; `PlateRegistry` exists; parameters and `fertility` field ready for later ticks.

**Delivered:**

| Area | Artifacts |
|------|-----------|
| Crate | `crates/genesis_tectonics/` (no Bevy) |
| Core params | `GeologyParameters`: `initial_major_plate_count`, `initial_minor_plate_count`, `event_granularity`, `tick_interval_overrides_years`, `base_erosion_rate_per_year` |
| Core data | `WorldData.fertility: Vec<f32>`; `PlateId` serde |
| Plates | `plate.rs`: `Plate`, `PlateType`, `PlateClass`, `PlateRegistry` |
| Generation | `initial_generation.rs`: Poisson-style seeds, major/minor growth, continental fraction |

**RNG streams (after P1-1.5):** `tectonics.plate_seeds`, `tectonics.plate_axes`, `tectonics.plate_rates`

**Validation (session):** 151 tests workspace; 10 tectonics tests; plate count 13–19; full hex coverage.

**Explicitly not in slice:** motion, boundaries, elevation, ticks.

---

### P1-1.5 — Remediation

**Goal:** Align implementation with Doc 06 §2.1, §2.2, §4.4 without expanding scope.

**Fixes:**

- Single `plate_seeds` stream for seeds + growth picks + neighbor draws
- Minor `target_fraction` sampled **individually** in 0.03–0.07 (not normalized bundle)
- Motion axis constraint uses **plate centroid**, not seed hex
- Growth: random unowned frontier hex (candidates sorted by `HexId`)
- Poisson fallback: relax spacing ×0.85, then deterministic fill

**Files:** `initial_generation.rs` only.

**Validation:** 155 workspace tests; 14 tectonics tests.

---

### P1-2 — Plate motion and tick integration

**Goal:** Geological-era ticks move plates and reassign hexes.

**Delivered:**

| Module | Responsibility |
|--------|----------------|
| `motion.rs` | `advance_plate_motion`; `effective_position_direction` |
| `partition.rs` | Voronoi by effective position; `HexId` order; tie → lowest `PlateId` |
| `layer.rs` | `TectonicsLayer` implements `SimulationLayer` |
| `history.rs` | `generate_full_history_with_tectonics` |
| `genesis_core` | `advance_with_coordinator`; `TickCoordinator` first tick at `world_start_year` |
| `genesis_app` | Depends on `genesis_tectonics`; history to 1M years |

**Geological tick (this slice):** motion → repartition only.

**Validation:** 169 workspace tests; 25 tectonics; determinism on `plate_id`; some hexes change after one tick.

---

### P1-3 — Boundary detection and classification

**Goal:** After repartition, know which hexes are on plate boundaries and how each cross-plate edge is classified.

**Delivered:**

| Type | Purpose |
|------|---------|
| `BoundaryClass` | Divergent / Transform / Convergent(subtype) |
| `ClassifiedEdge` | Per directed edge: neighbor, other plate, velocities |
| `BoundaryInfo` | `boundary_hexes`, `plate_contacts`, `edges` |

**Algorithm:** `surface_velocity_m_per_year` at hex center; transform if `|normal| < 0.3 * |tangential|`; convergent subtypes from plate types.

**Geological tick:** motion → repartition → **boundaries**.

**Validation:** 184 workspace tests; 40 tectonics; no elevation change.

---

### P1-4 — Formation terrain and boundary elevation

**Goal:** Populate `elevation_mean`, `bedrock_type`, `sea_level_m`; sculpt terrain from boundaries each tick.

**Delivered:**

| Module | Responsibility |
|--------|----------------|
| `initial_terrain.rs` | Formation: ~500 m continental / ~-3500 m oceanic ± noise; bedrock; fertility 0 |
| `elevation.rs` | §5.1–§5.4, §5.6; inland BFS spread; `clamp_terrain` §5.7 |
| `layer.rs` | Formation: plates → terrain; Geological: … → **elevation** |

**Subduction rules:** OC by owner type; OO faster `motion_rate_rad_per_year` (tie → lower `PlateId`).

**Geological tick:** motion → repartition → boundaries → **elevation** → clamp.

**Validation:** 198 workspace tests; 54 tectonics; continental mean elev > oceanic.

---

### P1-5 — Boundary volcanism

**Goal:** Subduction-arc eruptions; `VolcanicEruption` events; terrain always applies, log gated.

**Delivered:**

| Item | Detail |
|------|--------|
| `volcanism.rs` | Arc hex collection; `tectonics.volcanism` stream (later `stream_at`) |
| `events.rs` | `flush_events_to_branch` |
| `EventKind::VolcanicEruption` | In `genesis_core` |
| `TectonicsState` | `pending_events`, `next_event_id` |

**Significance:** peak proxy > 2000 m → Notable, else Minor.

**Geological tick:** … → elevation → **volcanism** → clamp.

**Validation:** 207 workspace tests; 63 tectonics.

---

### P1-5.1 — Per-tick volcanism RNG

**Goal:** Different eruptions on different ticks with same seed (deterministic per year).

**Problem:** `WorldRng::stream(name)` returned identical sequence every call.

**Fix:** `WorldRng::stream_at(name, tick_key)` with `xxh3_64(seed || name || tick_key)`; volcanism uses `tick_year`.

**Files:** `genesis_core/src/rng/mod.rs`; `volcanism.rs`.

**Validation:** Cross-tick differentiation tests; Formation streams unchanged.

---

### P1-6 — Hot spots

**Goal:** Mantle hot spots independent of plate boundaries (Doc §7).

**Delivered:**

| Item | Detail |
|------|--------|
| `HotSpotId` | `genesis_core::data::ids` |
| `hotspots.rs` | Formation: `round(8 + 16 × r/r_earth)` → **24** at Earth radius |
| Streams | `hotspot_locations` (Formation), `hotspot_activity` (`stream_at`) |
| `EventKind::HotSpotActivity` | Cumulative uplift → Notable threshold |
| `HotSpotRegistry` | On `TectonicsState` |

**Geological tick:** … → volcanism → **hot spots** (clamp moved to erosion step in P1-7).

**Validation:** Hot spot determinism; integration with Trace granularity.

---

### P1-7 — Erosion, sediment, fertility

**Goal:** Doc §8 — erode land, route sediment, increment fertility on shallow tropical seas.

**Delivered:**

| Item | Detail |
|------|--------|
| `erosion.rs` | `apply_land_erosion`; `route_eroded_mass`; `increment_shallow_tropical_fertility` |
| `TectonicsState.cumulative_deposition_m` | > 500 m → `Sedimentary` on Igneous/Metamorphic |
| RNG | `stream_at("tectonics.erosion_noise", tick_year)` ∈ [0.95, 1.05] |
| Climate | `climate_modifier_phase1` → 1.0 |

**Geological tick:** … → hot spots → **erosion** (includes clamp).

**Validation:** 87+ tectonics tests; mean land elevation decreases vs zero-rate control; fertility > 0 at 1M years.

---

### P1-8 — Reorganization, sea level, events

**Goal:** Complete per-tick loop steps 7–9 and event taxonomy.

**Delivered:**

| Module | Responsibility |
|--------|----------------|
| `reorganization.rs` | P = 0.001 × `geology_activity_scale`; split/merge/motion 40/20/40; extinct plates ≥10M y empty |
| `sea_level.rs` | Divergent length × 1e-6; damping; reorg ±100 m excursion |
| `boundary_events.rs` | Mountain / ocean / transition events |
| `events.rs` | `maybe_emit`, `alloc_event_id` |
| `genesis_core` | Full `EventKind` + `PlateReorgAction` + `BoundaryType` |

**Formation:** `WorldFormation` (Pivotal).

**Final Geological tick order:**

1. Motion  
2. Repartition  
3. Boundaries  
4. Snapshot `elevation_at_tick_start`  
5. Boundary elevation  
6. Volcanism  
7. Hot spots  
8. Erosion (+ fertility + clamp)  
9. Reorganization (optional re-detect boundaries)  
10. Sea level  
11. Boundary events  
12. `clamp_terrain`

**Validation:** Event log determinism; sea level ±200 m at 1M years; plate count [5, 15].

---

### P1-9 — Validation and performance

**Goal:** Prove plausibility without rendering.

**Delivered:**

| Item | Detail |
|------|--------|
| `validation.rs` | Metrics, BFS regions, `summarize_world`, `run_validation_world` |
| Seed | `VALIDATION_SEED = 42`, subdiv 5 |
| Quick CI | 1M years — fraction, plates, bounds, sea level, events > 0 |
| Full `#[ignore]` | 100M years — mountains, basins, bedrock, 500–15k Notable events |
| Perf | 10M years < 30 s at subdiv 5 |
| `debug_tick_step` | Per-step ms when `RUST_LOG=genesis_tectonics=debug` |

**Land fraction:** 20–40% band (seed 42 ≈ 39% at tested years; above nominal §11 25–35%).

**Validation:** 109 tectonics tests (2 ignored by default); 134 core tests.

---

### P1-10 — Elevation rendering

**Goal:** Visualize `elevation_mean` in the app.

**Delivered:**

| Item | Detail |
|------|--------|
| `genesis_render/src/color.rs` | `elevation_color`, `hex_fill_color`; piecewise linear −11k…+9k |
| `systems.rs` | Read `elevation_mean[idx]`, `sea_level_m` per hex |
| `genesis_app` | Title "Genesis Engine — Geology Smoke Test"; `summarize_world` log |

**Validation:** 16 render color tests; manual `cargo run -p genesis_app`.

**Phase 1 complete:** Doc §15 steps 1–10 implemented.

---

## 6. Final architecture (as of `960657a`)

### 6.1 Crate dependency graph (tectonics path)

```
genesis_app
  ├── genesis_render  (reads WorldData only)
  ├── genesis_tectonics
  │     └── genesis_core
  └── genesis_core
```

`genesis_core` does **not** depend on `genesis_tectonics` (layers register via `advance_with_coordinator`).

### 6.2 `genesis_tectonics` modules

| File | Role |
|------|------|
| `initial_generation.rs` | Formation plate layout |
| `initial_terrain.rs` | Formation elevation/bedrock |
| `plate.rs` | Plates, registries, `TectonicsState` |
| `motion.rs` | Euler-pole motion |
| `partition.rs` | Voronoi repartition |
| `boundary.rs` | Boundary detection/classification |
| `elevation.rs` | Boundary-driven terrain |
| `volcanism.rs` | Arc eruptions |
| `hotspots.rs` | Mantle hot spots |
| `erosion.rs` | Erosion, routing, fertility |
| `reorganization.rs` | Split/merge/motion change |
| `sea_level.rs` | Sea level drift |
| `boundary_events.rs` | Chronicle events from boundaries |
| `events.rs` | Event ID allocation, flush, `maybe_emit` |
| `layer.rs` | `TectonicsLayer` tick orchestration |
| `history.rs` | `generate_full_history_with_tectonics`, `run_formation` |
| `validation.rs` | §11 metrics and test helpers |

### 6.3 State split: `World` vs `TectonicsState`

| Lives in `WorldData` / `World` | Lives in `TectonicsState` |
|------------------------------|---------------------------|
| `elevation_mean`, `elevation_relief` | `PlateRegistry` |
| `bedrock_type`, `plate_id`, `fertility` | `BoundaryInfo` |
| `sea_level_m` | `HotSpotRegistry` |
| (future climate fields) | `cumulative_deposition_m` |
| | `pending_events` → flushed to root `EventLog` |
| | `elevation_at_tick_start`, `previous_edge_class` |
| | `baseline_divergent_length_km` |

### 6.4 RNG model (Phase 1)

| Pattern | Use |
|---------|-----|
| `WorldRng::stream(name)` | One-shot Formation: plate seeds, axes, rates, initial elevation noise, hot spot **locations** |
| `WorldRng::stream_at(name, tick_year)` | Per Geological tick: volcanism, hot spot **activity**, erosion noise, reorganization check/action |

Doc §4.4 table says `stream(name)` for all streams; **implementation standardizes on `stream_at` for per-tick rolls** (documented in commit messages).

### 6.5 Default app behavior

- Subdivision **6** (~7.3k hexes) for interactive mesh build time  
- `generate_full_history_with_tectonics` to **1M years** (Formation + 2 × 500k Geological ticks)  
- Equirectangular hex map colored by **elevation**  
- **R** key regenerates a **non-tectonic** world (wall-clock seed) — pre-existing limitation  

---

## 7. Verification matrix (current)

| Gate | Command | Expected |
|------|---------|----------|
| Build | `cargo build --workspace` | Success |
| Tests | `cargo test --workspace` | All pass; 2 ignored in `genesis_tectonics` |
| Long validation | `cargo test -p genesis_tectonics -- --ignored` | `validation_full_suite_passes` ok (~1 s) |
| Format | `cargo fmt --check` | Clean |
| Lint | `cargo clippy --workspace --all-targets` | Clean (or pre-existing warnings documented) |
| Visual | `cargo run -p genesis_app` | Blue ocean, green/brown land, pale peaks |
| Debug timings | `RUST_LOG=genesis_tectonics=debug cargo run -p genesis_app` | Per-step `elapsed_ms` each Geological tick |

**Test counts (representative):** `genesis_tectonics` 109 pass + 2 ignored; `genesis_core` 134; `genesis_render` 16.

---

## 8. Known gaps, doc drift, and follow-ups

| Topic | Notes |
|-------|--------|
| **Limestone / §11.5** | `BedrockType::Limestone` exists; Phase 1 never assigns it (Phase 4 biology) |
| **Hot spot count prose** | §7.2 says "12–20"; formula gives **24** at Earth radius |
| **Land fraction §11** | Nominal 25–35%; validation uses 20–40% for seed 42 |
| **Event volume §6.4** | Table assumes 4.5B years; 100M-year validation allows up to 15k Notable |
| **Reorg elevation** | After reorg, boundaries re-detected but §5 elevation not re-run same tick |
| **Sea level / reorg RNG** | Reorg excursion reuses `reorganization_action` stream_at (deterministic but coupled) |
| **Deposition vs elevation** | Eroded mass does not raise sink `elevation_mean` (cumulative deposition only) |
| **R regenerate** | No tectonics; non-deterministic seed — not a geology demo |
| **4.5B production run** | Not required for Phase 1 sign-off; perf tested to 10M years |
| **README** | Still describes "Pre-implementation" in places — housekeeping |
| **Doc 04 vs 06 events** | Doc 04 shows nested event structs; code uses flat `EventKind` enum per Doc 06 |

---

## 9. How to see what was accomplished

### 9.1 Visual (recommended)

```bash
cargo run -p genesis_app
```

- Wait for startup (tectonics to 1M years, then window).  
- Read terminal: `summarize_world` line (land %, elev range, sea level, plates, events).  
- Pan (left-drag), zoom (scroll).  
- **Do not press R** if you want to see geology — R creates a fresh non-tectonic world.

### 9.2 Automated proof

```bash
cargo test --workspace
cargo test -p genesis_tectonics validation_quick_suite_passes
cargo test -p genesis_tectonics validation_full_suite -- --ignored
```

### 9.3 Richer terrain (optional local edit)

In `genesis_app/src/main.rs`, change target to `WorldYear(100_000_000)` for more geological ticks (slower startup, more mountains/events). Use `summarize_world` output to compare.

### 9.4 Inspect without GPU

```rust
// In tests or a small binary:
let (world, state) = genesis_tectonics::run_validation_world(WorldYear(1_000_000))?;
println!("{}", genesis_tectonics::summarize_world(&world, &state));
```

---

## 10. What comes next (outside this review)

| Track | Description |
|-------|-------------|
| **Housekeeping** | Update root `README.md`; CI note for `--ignored` tests; optional fix **R** to run tectonics |
| **Phase 2** | Climate module (Doc 07 when written); dynamic `precipitation` / `temperature_mean`; erosion `climate_modifier` |
| **Long history** | Optional 4.5B-year soak tests; calibrate event counts vs §6.4 |
| **Rendering** | Doc 14 territory: legends, biomes, political mode, globe view |

---

## Appendix A — `genesis_core` changes accumulated (Phase 1)

- `GeologyParameters`: major/minor counts, `event_granularity`, `tick_interval_overrides_years`, `base_erosion_rate_per_year`, `geology_activity_scale`
- `WorldData.fertility`
- `PlateId`, `HotSpotId` serde
- `EventKind`: `WorldFormation`, `PlateReorganization`, `MountainRangeFormed`, `OceanBasinOpened`, `VolcanicEruption`, `HotSpotActivity`, `BoundaryTransition`, `SeaLevelChange`
- `BoundaryType`, `PlateReorgAction`
- `TickCoordinator`: first tick at `world_start_year`
- `lifecycle::advance_with_coordinator`
- `WorldRng::stream_at`

---

## Appendix B — Prompt retrieval guide

Full verbatim prompt text is inlined in **Appendix C** below (recovered from agent transcripts on 2026-05-19).

| Transcript ID | Role |
|---------------|------|
| [f65ad57f](f65ad57f-3ee2-40d2-a5d3-691aa44ee898) | P1-1 validation through P1-10; prompts P1-1.5–P1-10 |
| [7ab9bbb6](7ab9bbb6-d623-4e08-a2f2-3f608927a8d1) | Original **P1-1** prompt (Doc 06 §15 step 1) |

Prompts are also structurally reproducible from [`00-prompt-template.md`](./00-prompt-template.md) + the task summaries in §4.

---

## Appendix C — Recovered agent prompts (full text)

Verbatim prompts recovered from Cursor agent transcripts. Each block was wrapped with `=== START PROMPT ===` / `=== END PROMPT ===` in chat for copy-paste into Agent mode.

| Prompt | Primary transcript | Line (approx.) |
|--------|-------------------|----------------|
| **P1-1** | `7ab9bbb6` | 538 |
| **P1-1.5** | `f65ad57f` | 18 |
| **P1-2** | `f65ad57f` | 32 |
| **P1-3** | `f65ad57f` | 45 |
| **P1-4** | `f65ad57f` | 53 |
| **P1-5** | `f65ad57f` | 65 |
| **P1-5.1** | `f65ad57f` | 76 |
| **P1-6** | `f65ad57f` | 85 |
| **P1-7** | `f65ad57f` | 97 |
| **P1-8** | `f65ad57f` | 121 |
| **P1-9** | `f65ad57f` | 142 |
| **P1-10** | `f65ad57f` | 159 |

Session links: [f65ad57f](f65ad57f-3ee2-40d2-a5d3-691aa44ee898) (validation + P1-1.5–P1-10 prompts), [7ab9bbb6](7ab9bbb6-d623-4e08-a2f2-3f608927a8d1) (P1-1 original prompt).

### P1-1 — Initial plate generation

*Source: transcript `7ab9bbb6`, assistant message ~line 538.*

```markdown
=== START PROMPT ===

Read these documents before starting, in order:

CONTRIBUTING-AI.md — collaboration guide
docs/03-glossary-and-naming.md — terminology
docs/06-tectonics-module-specification.md v0.2 — the full spec for Phase 1. Read every section, but pay particular attention to:

§1 (Purpose and Scope)
§2 (The Plate Model) — especially §2.1, §2.2, §2.4
§4.3 (First-Tick Initialization)
§4.4 (RNG Streams)
§6.3 (Event Granularity System)
§8.4 (Fertility field on WorldData)
§10 (Determinism Requirements)
§13 (File Organization)
§16 (Implementation Notes)

docs/04-data-layer-specification.md v0.5 — for context on WorldData, WorldParameters, RNG
Existing code in crates/genesis_core/src/ for style

If you cannot access any of these, stop and tell me.
Your Task
Implement the first slice of Phase 1: initial plate generation. This produces a world at year 0 with plates assigned to every hex via growth-based seeding. No plate motion, no boundary effects, no per-tick simulation.
The deliverable is a working genesis_tectonics::initial_generation module that, given a World, populates plate_id for every hex and stores the resulting Plate registry. A test can verify that a default world has 13-19 plates, valid PlateIds, and complete hex coverage.
What "Done" Looks Like
After this prompt:

New crate genesis_tectonics exists with the scaffold from Doc 06 §13
WorldData gains a fertility: Vec<f32> field initialized to 0.0
GeologyParameters gains the new fields per Doc 06 §6.3
parameters::validation validates the new fields
Initial plate generation works: given a World, produces a PlateRegistry and updates WorldData.plate_id for every hex
Pentagons get plates assigned just like hexes (no special handling needed beyond what the grid already provides)
All existing tests still pass
New tests verify generation correctness

Implementation Plan
Part 1: Expand GeologyParameters
In crates/genesis_core/src/parameters/core.rs, update GeologyParameters:
rustuse std::collections::BTreeMap;

use crate::events::Significance;
use crate::time::{Era, WorldYear};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeologyParameters {
    /// Fraction of plates that are continental at world formation. Default 0.29 (Earth).
    pub initial_continental_fraction: f32,
    
    /// Plate motion scale factor relative to Earth-like values. Default 1.0.
    pub plate_velocity_scale: f32,
    
    /// Volcanism intensity multiplier. Default 1.0.
    pub volcanism_scale: f32,
    
    /// Number of major (large) plates at world formation. Default 7. Valid range 6-9.
    pub initial_major_plate_count: u8,
    
    /// Number of minor (smaller) plates at world formation. Default 8. Valid range 6-10.
    pub initial_minor_plate_count: u8,
    
    /// Minimum event significance to record during tectonic simulation.
    /// Events below this threshold are computed and applied but NOT logged.
    /// Default `Significance::Notable`.
    pub event_granularity: Significance,
    
    /// Admin/debug override for tick interval per era (years). None = use the
    /// defaults from Doc 06 §4.1.
    pub tick_interval_overrides_years: Option<BTreeMap<Era, i64>>,
    
    /// Base erosion rate per year per meter of elevation above sea level.
    /// Default 1e-7. Climate modifies via climate_modifier (Phase 2).
    pub base_erosion_rate_per_year: f64,
}
Update the Default impl in parameters/mod.rs to use the new defaults:
rustgeology: GeologyParameters {
    initial_continental_fraction: 0.29,
    plate_velocity_scale: 1.0,
    volcanism_scale: 1.0,
    initial_major_plate_count: 7,
    initial_minor_plate_count: 8,
    event_granularity: Significance::Notable,
    tick_interval_overrides_years: None,
    base_erosion_rate_per_year: 1e-7,
},
Part 2: Add Validation Rules
In crates/genesis_core/src/parameters/validation.rs, add validation for the new fields:
rust// Major plate count: 6-9
if !(6..=9).contains(&self.core.geology.initial_major_plate_count) {
    return Err(ParameterValidationError::InvalidField {
        field: "geology.initial_major_plate_count".into(),
        message: format!(
            "must be 6-9, got {}",
            self.core.geology.initial_major_plate_count
        ),
    });
}

// Minor plate count: 6-10
if !(6..=10).contains(&self.core.geology.initial_minor_plate_count) {
    return Err(ParameterValidationError::InvalidField {
        field: "geology.initial_minor_plate_count".into(),
        message: format!(
            "must be 6-10, got {}",
            self.core.geology.initial_minor_plate_count
        ),
    });
}

// Erosion rate: positive, finite, < 1e-3
let rate = self.core.geology.base_erosion_rate_per_year;
if !rate.is_finite() || rate <= 0.0 || rate >= 1e-3 {
    return Err(ParameterValidationError::InvalidField {
        field: "geology.base_erosion_rate_per_year".into(),
        message: format!("must be positive, finite, and < 1e-3; got {rate}"),
    });
}
Add tests in parameters/mod.rs tests block for each validation rule.
Part 3: Add fertility Field to WorldData
In crates/genesis_core/src/data/mod.rs, add fertility to WorldData:
rustpub struct WorldData {
    // ... existing fields up through Biological Layer ...
    
    /// Biome assignment per hex.
    pub biome: Vec<BiomeId>,
    /// Total biomass in tons per hex.
    pub biomass: Vec<f32>,
    /// Bio-deposit accumulator from shallow tropical seas. Monotonic; never decreases.
    /// Phase 1 tectonics increments this for hexes in shallow tropical conditions.
    /// Phase 4 biology will refine accumulation rate and drive bedrock transitions.
    pub fertility: Vec<f32>,
    
    // ... rest unchanged ...
}
Place fertility immediately after biomass in the struct definition (keeps biology-related fields grouped).
Update WorldData::new to initialize fertility: vec![0.0; n].
Update the existing test bulk_array_lengths_match_cell_count to verify world.fertility.len() == n.
Update the existing test default_values_level_4 to verify fertility is all zeros.
Part 4: Create genesis_tectonics Crate
Per Doc 06 §13, scaffold the crate:
crates/genesis_tectonics/
├── Cargo.toml
└── src/
    ├── lib.rs              # public API, type exports
    ├── plate.rs            # Plate, PlateType, PlateClass, PlateId (re-export from core), PlateRegistry
    └── initial_generation.rs   # initial seeding and growth algorithm
Add the crate to the workspace Cargo.toml. Dependencies:
toml[package]
name = "genesis_tectonics"
version.workspace = true
edition.workspace = true

[dependencies]
genesis_core = { path = "../genesis_core" }
glam = "0.30"
rand = { version = "0.8", features = ["small_rng"] }
serde = { version = "1", features = ["derive"] }
thiserror = "2"
Do NOT add bevy as a dependency. This crate is engine-agnostic.
Part 5: Implement the Plate Types
In crates/genesis_tectonics/src/plate.rs:
rust//! Plate types and registry.

use std::collections::BTreeMap;

use genesis_core::time::WorldYear;
use genesis_core::{HexId, PlateId};
use glam::DVec3;
use serde::{Deserialize, Serialize};

/// Whether the plate is continental (lighter, thicker, higher elevation)
/// or oceanic (denser, thinner, lower elevation).
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub enum PlateType {
    Continental,
    Oceanic,
}

/// Major (large; Earth-scale continent or ocean) versus Minor (smaller).
/// Affects target size during initial growth seeding.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub enum PlateClass {
    Major,
    Minor,
}

/// A tectonic plate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plate {
    pub id: PlateId,
    pub plate_type: PlateType,
    pub plate_class: PlateClass,
    
    /// HexId of the seed hex used for this plate's geographic anchor.
    /// Does not change after creation; effective position is computed from
    /// this seed plus accumulated rotation about the motion axis.
    pub seed_hex: HexId,
    
    /// Unit vector representing the Euler-pole rotation axis. Constrained to
    /// produce sensible plate motion (see Doc 06 §2.1).
    pub motion_axis: [f64; 3],
    
    /// Angular velocity in radians per year. Always positive.
    pub motion_rate_rad_per_year: f64,
    
    /// World year this plate was created (or last reorganized).
    pub age_year: WorldYear,
    
    /// Target fraction of the sphere this plate covers. Used during growth
    /// seeding; informational thereafter.
    pub target_fraction: f32,
}

/// All plates in a world, keyed by `PlateId`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlateRegistry {
    plates: BTreeMap<PlateId, Plate>,
    next_id: u16,
}

impl PlateRegistry {
    pub fn new() -> Self {
        Self {
            plates: BTreeMap::new(),
            next_id: 0,
        }
    }
    
    pub fn insert(&mut self, plate: Plate) {
        self.plates.insert(plate.id, plate);
    }
    
    pub fn get(&self, id: PlateId) -> Option<&Plate> {
        self.plates.get(&id)
    }
    
    pub fn iter(&self) -> impl Iterator<Item = &Plate> {
        self.plates.values()
    }
    
    pub fn count(&self) -> usize {
        self.plates.len()
    }
    
    /// Allocates the next sequential PlateId. Used during initial generation.
    pub(crate) fn next_id(&mut self) -> PlateId {
        let id = PlateId(self.next_id);
        self.next_id += 1;
        id
    }
}

impl Default for PlateRegistry {
    fn default() -> Self {
        Self::new()
    }
}
Note that PlateId is re-exported from genesis_core — we use the existing newtype, not a new one. Same for HexId and WorldYear.
The motion_axis uses [f64; 3] for serialization stability (glam types don't all derive Serialize cleanly across versions). Convert to/from glam::DVec3 at use sites.
Part 6: Implement Initial Plate Generation
In crates/genesis_tectonics/src/initial_generation.rs:
This is the substantive algorithm. Per Doc 06 §2.2, the process is:

Place major plate seeds with Poisson-disk-like distribution
Compute target fractions for each plate
Grow major plates to ~50% coverage
Place minor plate seeds in unowned territory
Grow all plates simultaneously until every hex has an owner
Assign plate types (continental vs oceanic) biasing continental toward major
Sample motion axes and rates with the constraints from Doc 06 §2.1, §2.4

rust//! Initial plate generation at world formation (year 0).

use std::collections::{BTreeMap, BTreeSet};

use genesis_core::time::WorldYear;
use genesis_core::{HexId, PlateId, World};
use rand::Rng;
use rand::seq::SliceRandom;

use crate::plate::{Plate, PlateClass, PlateRegistry, PlateType};

/// Performs initial plate generation. Mutates `world.data.plate_id` for every hex
/// and returns the `PlateRegistry` containing the generated plates.
///
/// Should be called exactly once per world, at year 0.
pub fn generate_initial_plates(world: &mut World) -> PlateRegistry {
    let params = &world.data.parameters.core.geology;
    let major_count = params.initial_major_plate_count as usize;
    let minor_count = params.initial_minor_plate_count as usize;
    let total_cells = world.data.grid.cell_count() as usize;
    
    let mut registry = PlateRegistry::new();
    
    // Step 1: Place major plate seeds
    let major_seeds = place_seeds_poisson_disk(
        world,
        major_count,
        None, // no exclusion set yet
        "tectonics.plate_seeds.major",
    );
    
    // Compute major plate target fractions
    let major_target_fractions = sample_target_fractions(
        world,
        major_count,
        0.50,
        "tectonics.plate_seeds.major_targets",
    );
    
    // Initialize plate ownership: each major seed claims its own hex
    let mut plate_id_for_hex: Vec<Option<PlateId>> = vec![None; total_cells];
    let mut major_plate_ids = Vec::with_capacity(major_count);
    
    for (i, &seed_hex) in major_seeds.iter().enumerate() {
        let id = registry.next_id();
        plate_id_for_hex[seed_hex.0 as usize] = Some(id);
        major_plate_ids.push(id);
        
        let plate = Plate {
            id,
            plate_type: PlateType::Continental, // assigned properly below
            plate_class: PlateClass::Major,
            seed_hex,
            motion_axis: [0.0, 0.0, 1.0],  // placeholder; assigned below
            motion_rate_rad_per_year: 0.0,  // placeholder
            age_year: WorldYear::FORMATION,
            target_fraction: major_target_fractions[i],
        };
        registry.insert(plate);
    }
    
    // Step 2: Grow major plates to ~50% of total cells
    let major_growth_target = (total_cells as f64 * 0.50) as usize;
    grow_plates_to_coverage(
        world,
        &registry,
        &mut plate_id_for_hex,
        &major_plate_ids,
        major_growth_target,
        "tectonics.plate_growth.major",
    );
    
    // Step 3: Place minor plate seeds in unowned territory
    let unowned_hexes: Vec<HexId> = (0..total_cells)
        .filter(|&i| plate_id_for_hex[i].is_none())
        .map(|i| HexId(i as u32))
        .collect();
    
    let minor_seeds = place_seeds_in_pool(
        world,
        &unowned_hexes,
        minor_count,
        "tectonics.plate_seeds.minor",
    );
    
    let minor_target_fractions = sample_target_fractions(
        world,
        minor_count,
        0.50, // remaining 50% spread across minor plates
        "tectonics.plate_seeds.minor_targets",
    );
    
    let mut minor_plate_ids = Vec::with_capacity(minor_count);
    for (i, &seed_hex) in minor_seeds.iter().enumerate() {
        let id = registry.next_id();
        plate_id_for_hex[seed_hex.0 as usize] = Some(id);
        minor_plate_ids.push(id);
        
        let plate = Plate {
            id,
            plate_type: PlateType::Continental, // assigned properly below
            plate_class: PlateClass::Minor,
            seed_hex,
            motion_axis: [0.0, 0.0, 1.0],
            motion_rate_rad_per_year: 0.0,
            age_year: WorldYear::FORMATION,
            target_fraction: minor_target_fractions[i],
        };
        registry.insert(plate);
    }
    
    // Step 4: Grow all plates until every hex has an owner
    let all_plate_ids: Vec<PlateId> = major_plate_ids
        .iter()
        .chain(minor_plate_ids.iter())
        .copied()
        .collect();
    grow_plates_to_coverage(
        world,
        &registry,
        &mut plate_id_for_hex,
        &all_plate_ids,
        total_cells,
        "tectonics.plate_growth.final",
    );
    
    // Verify every hex now has a plate
    debug_assert!(plate_id_for_hex.iter().all(|p| p.is_some()));
    
    // Step 5: Assign plate types per continental fraction
    assign_plate_types(world, &mut registry);
    
    // Step 6: Sample motion axes and rates for each plate
    assign_plate_motion(world, &mut registry);
    
    // Step 7: Write plate_id to WorldData
    for (i, plate_opt) in plate_id_for_hex.iter().enumerate() {
        world.data.plate_id[i] = plate_opt.expect("all hexes have plates after growth");
    }
    
    registry
}

// --- Helper functions ---

/// Selects `count` hexes from the world with approximate Poisson-disk spacing
/// (no two seeds too close together).
fn place_seeds_poisson_disk(
    world: &World,
    count: usize,
    exclude: Option<&BTreeSet<HexId>>,
    rng_stream: &str,
) -> Vec<HexId> {
    let total = world.data.grid.cell_count() as usize;
    let mut rng = world.rng.stream(rng_stream);
    
    // Minimum angular distance between seeds, derived from desired count.
    // For N evenly-spaced seeds on a sphere, mean angular distance is ~acos(1 - 2/N).
    // Require at least 0.6x of that mean for Poisson-disk effect.
    let min_dist_rad = (1.0_f64 - 2.0 / count as f64).acos() * 0.6;
    
    let mut seeds: Vec<HexId> = Vec::with_capacity(count);
    let mut attempts = 0u32;
    let max_attempts = count as u32 * 100;
    
    while seeds.len() < count && attempts < max_attempts {
        let candidate_idx = rng.gen_range(0..total);
        let candidate = HexId(candidate_idx as u32);
        attempts += 1;
        
        if let Some(ex) = exclude {
            if ex.contains(&candidate) {
                continue;
            }
        }
        
        let cand_pos = world.data.grid.cell_center_direction(candidate);
        let cand_vec = glam::DVec3::new(cand_pos[0], cand_pos[1], cand_pos[2]);
        
        let too_close = seeds.iter().any(|&existing| {
            let existing_pos = world.data.grid.cell_center_direction(existing);
            let existing_vec = glam::DVec3::new(existing_pos[0], existing_pos[1], existing_pos[2]);
            let dot = cand_vec.dot(existing_vec).clamp(-1.0, 1.0);
            dot.acos() < min_dist_rad
        });
        
        if !too_close {
            seeds.push(candidate);
        }
    }
    
    // Fallback: if we couldn't satisfy spacing, fill remaining with random unused hexes
    while seeds.len() < count {
        let candidate_idx = rng.gen_range(0..total);
        let candidate = HexId(candidate_idx as u32);
        if !seeds.contains(&candidate) {
            seeds.push(candidate);
        }
    }
    
    seeds
}

/// Like `place_seeds_poisson_disk` but selects from a provided pool.
fn place_seeds_in_pool(
    world: &World,
    pool: &[HexId],
    count: usize,
    rng_stream: &str,
) -> Vec<HexId> {
    let mut rng = world.rng.stream(rng_stream);
    let mut available: Vec<HexId> = pool.to_vec();
    available.shuffle(&mut rng);
    
    // Use the same Poisson-disk-style filtering but on the shuffled pool.
    let min_dist_rad = (1.0_f64 - 2.0 / count as f64).acos() * 0.4;
    let mut seeds: Vec<HexId> = Vec::with_capacity(count);
    
    for &candidate in &available {
        if seeds.len() == count {
            break;
        }
        
        let cand_pos = world.data.grid.cell_center_direction(candidate);
        let cand_vec = glam::DVec3::new(cand_pos[0], cand_pos[1], cand_pos[2]);
        
        let too_close = seeds.iter().any(|&existing| {
            let existing_pos = world.data.grid.cell_center_direction(existing);
            let existing_vec = glam::DVec3::new(existing_pos[0], existing_pos[1], existing_pos[2]);
            let dot = cand_vec.dot(existing_vec).clamp(-1.0, 1.0);
            dot.acos() < min_dist_rad
        });
        
        if !too_close {
            seeds.push(candidate);
        }
    }
    
    // Fallback: fill with whatever's left if Poisson disk was too strict
    if seeds.len() < count {
        for &h in &available {
            if seeds.len() == count {
                break;
            }
            if !seeds.contains(&h) {
                seeds.push(h);
            }
        }
    }
    
    seeds
}

/// Samples target fractions for `count` plates summing approximately to `total_target`.
/// Each plate's fraction is centered on `total_target / count` with ±50% variation.
fn sample_target_fractions(
    world: &World,
    count: usize,
    total_target: f32,
    rng_stream: &str,
) -> Vec<f32> {
    let mut rng = world.rng.stream(rng_stream);
    let mean = total_target / count as f32;
    
    let mut fractions: Vec<f32> = (0..count)
        .map(|_| {
            let variation: f32 = rng.gen_range(0.5..1.5);
            mean * variation
        })
        .collect();
    
    // Normalize so they sum to total_target
    let sum: f32 = fractions.iter().sum();
    if sum > 0.0 {
        for f in &mut fractions {
            *f *= total_target / sum;
        }
    }
    
    fractions
}

/// Grows plates outward from their currently-owned hexes until total coverage
/// reaches `target_coverage` (in absolute hex count).
///
/// Each round picks a plate (weighted by remaining target_fraction) and grows
/// it by one hex chosen from its current boundary. Continues until target reached
/// or no more growth possible.
fn grow_plates_to_coverage(
    world: &World,
    registry: &PlateRegistry,
    plate_id_for_hex: &mut [Option<PlateId>],
    active_plate_ids: &[PlateId],
    target_coverage: usize,
    rng_stream: &str,
) {
    let mut rng = world.rng.stream(rng_stream);
    let grid = &world.data.grid;
    
    // Track current size of each plate
    let mut current_size: BTreeMap<PlateId, usize> = active_plate_ids
        .iter()
        .map(|&id| (id, 0usize))
        .collect();
    for plate_opt in plate_id_for_hex.iter() {
        if let Some(id) = plate_opt {
            if let Some(sz) = current_size.get_mut(id) {
                *sz += 1;
            }
        }
    }
    
    let total_owned = current_size.values().sum::<usize>();
    let mut total_owned = total_owned;
    
    while total_owned < target_coverage {
        // Pick a plate to grow this round. Weight by remaining target fraction.
        let plate_id = pick_next_plate_to_grow(
            registry,
            active_plate_ids,
            &current_size,
            &mut rng,
        );
        
        // Find this plate's growth candidates: unowned neighbors of any owned hex.
        let candidate = find_growth_candidate(
            grid,
            plate_id,
            plate_id_for_hex,
            &mut rng,
        );
        
        match candidate {
            Some(hex) => {
                plate_id_for_hex[hex.0 as usize] = Some(plate_id);
                *current_size.get_mut(&plate_id).expect("plate id valid") += 1;
                total_owned += 1;
            }
            None => {
                // This plate has no growth candidates. Skip and try other plates.
                // If no plate has candidates, growth is stuck — break.
                if !any_plate_can_grow(grid, active_plate_ids, plate_id_for_hex) {
                    break;
                }
            }
        }
    }
}

/// Picks the plate that's furthest below its target fraction, with weighted random tie-breaking.
fn pick_next_plate_to_grow(
    registry: &PlateRegistry,
    active_plate_ids: &[PlateId],
    current_size: &BTreeMap<PlateId, usize>,
    rng: &mut rand::rngs::SmallRng,
) -> PlateId {
    // Compute "neediness" for each plate: target_fraction - current_fraction
    // Higher neediness = plate is further behind its target = more likely to grow.
    
    let total_hexes: usize = current_size.values().sum::<usize>().max(1);
    
    let weights: Vec<(PlateId, f64)> = active_plate_ids
        .iter()
        .map(|&id| {
            let target = registry
                .get(id)
                .map(|p| p.target_fraction as f64)
                .unwrap_or(0.0);
            let current = current_size.get(&id).copied().unwrap_or(0) as f64 / total_hexes as f64;
            let neediness = (target - current).max(0.0);
            (id, neediness)
        })
        .collect();
    
    let total_weight: f64 = weights.iter().map(|(_, w)| w).sum();
    
    if total_weight <= 0.0 {
        // All plates have reached target; pick any active plate uniformly
        return active_plate_ids[rng.gen_range(0..active_plate_ids.len())];
    }
    
    let mut roll = rng.gen_range(0.0..total_weight);
    for (id, w) in &weights {
        roll -= *w;
        if roll <= 0.0 {
            return *id;
        }
    }
    
    // Fallback (shouldn't reach here)
    *active_plate_ids.last().expect("non-empty")
}

/// Finds an unowned hex adjacent to a hex currently owned by `plate_id`.
/// Returns None if no such hex exists.
fn find_growth_candidate(
    grid: &genesis_core::HexGrid,
    plate_id: PlateId,
    plate_id_for_hex: &[Option<PlateId>],
    rng: &mut rand::rngs::SmallRng,
) -> Option<HexId> {
    // Iterate hexes owned by this plate; for each, check neighbors for unowned.
    // Collect candidates, then pick one randomly for organic growth.
    
    let mut candidates: Vec<HexId> = Vec::new();
    
    for (i, plate_opt) in plate_id_for_hex.iter().enumerate() {
        if *plate_opt != Some(plate_id) {
            continue;
        }
        let hex = HexId(i as u32);
        for &neighbor in grid.neighbors(hex) {
            if plate_id_for_hex[neighbor.0 as usize].is_none() {
                candidates.push(neighbor);
            }
        }
    }
    
    candidates.sort_by_key(|h| h.0);
    candidates.dedup();
    
    if candidates.is_empty() {
        return None;
    }
    
    let idx = rng.gen_range(0..candidates.len());
    Some(candidates[idx])
}

fn any_plate_can_grow(
    grid: &genesis_core::HexGrid,
    active_plate_ids: &[PlateId],
    plate_id_for_hex: &[Option<PlateId>],
) -> bool {
    for &plate_id in active_plate_ids {
        for (i, plate_opt) in plate_id_for_hex.iter().enumerate() {
            if *plate_opt != Some(plate_id) {
                continue;
            }
            let hex = HexId(i as u32);
            for &neighbor in grid.neighbors(hex) {
                if plate_id_for_hex[neighbor.0 as usize].is_none() {
                    return true;
                }
            }
        }
    }
    false
}

/// Assigns Continental vs Oceanic to plates, biasing continental toward major class.
fn assign_plate_types(world: &World, registry: &mut PlateRegistry) {
    let continental_fraction = world.data.parameters.core.geology.initial_continental_fraction;
    let total = registry.count();
    let num_continental = ((total as f32) * continental_fraction).round() as usize;
    
    // Sort plates by class (major first), then by id for deterministic order.
    let mut plate_ids: Vec<PlateId> = registry
        .iter()
        .map(|p| p.id)
        .collect();
    plate_ids.sort_by_key(|id| {
        let p = registry.get(*id).unwrap();
        (
            match p.plate_class {
                PlateClass::Major => 0,
                PlateClass::Minor => 1,
            },
            id.0,
        )
    });
    
    // Assign Continental to the first num_continental plates (which are all Major if possible).
    let mut updates = Vec::new();
    for (i, &id) in plate_ids.iter().enumerate() {
        let plate_type = if i < num_continental {
            PlateType::Continental
        } else {
            PlateType::Oceanic
        };
        updates.push((id, plate_type));
    }
    
    for (id, plate_type) in updates {
        if let Some(plate) = registry.plates_mut().get_mut(&id) {
            plate.plate_type = plate_type;
        }
    }
}

/// Samples motion axes and rates per Doc 06 §2.1 and §2.4.
fn assign_plate_motion(world: &World, registry: &mut PlateRegistry) {
    use std::f64::consts::PI;
    
    let mut rng = world.rng.stream("tectonics.plate_motion");
    let params = &world.data.parameters.core.geology;
    let planet_params = &world.data.parameters.core.planet;
    
    // Effective velocity scale per Doc 06 §2.1: rotation factor
    let rotation_factor = (24.0 / planet_params.rotation_period_hours).sqrt() as f64;
    let effective_scale = params.plate_velocity_scale as f64 * rotation_factor;
    
    let median_cm_per_year = 5.0 * effective_scale;
    let sigma: f64 = 0.6;
    
    let plate_ids: Vec<PlateId> = registry.iter().map(|p| p.id).collect();
    
    for id in plate_ids {
        // Sample motion axis: uniform on sphere, with constraints
        let axis = sample_motion_axis(world, id, registry, &mut rng);
        
        // Sample rate: log-normal
        let log_sample: f64 = sample_log_normal(&mut rng, sigma);
        let mut rate_cm_per_year = median_cm_per_year * log_sample;
        
        // Continental plates move slower (0.7x)
        if let Some(plate) = registry.get(id) {
            if plate.plate_type == PlateType::Continental {
                rate_cm_per_year *= 0.7;
            }
        }
        
        let rate_rad_per_year = (rate_cm_per_year * 1e-5) / planet_params.radius_km;
        
        if let Some(plate) = registry.plates_mut().get_mut(&id) {
            plate.motion_axis = [axis.x, axis.y, axis.z];
            plate.motion_rate_rad_per_year = rate_rad_per_year;
        }
    }
}

/// Samples a unit vector on the sphere that satisfies the motion-axis constraints
/// from Doc 06 §2.1:
/// 1. Not aligned with the planet's rotation axis (z)
/// 2. Angular distance from plate's centroid is in [30°, 150°]
fn sample_motion_axis(
    world: &World,
    plate_id: PlateId,
    registry: &PlateRegistry,
    rng: &mut rand::rngs::SmallRng,
) -> glam::DVec3 {
    use std::f64::consts::PI;
    
    // Get plate's centroid direction (use seed hex as proxy)
    let seed_hex = registry
        .get(plate_id)
        .expect("plate exists")
        .seed_hex;
    let seed_pos = world.data.grid.cell_center_direction(seed_hex);
    let seed_vec = glam::DVec3::new(seed_pos[0], seed_pos[1], seed_pos[2]);
    
    let z_axis = glam::DVec3::Z;
    
    for _attempt in 0..100 {
        // Sample uniform on sphere
        let u = rng.gen::<f64>();
        let v = rng.gen::<f64>();
        let theta = 2.0 * PI * u;
        let phi = (2.0 * v - 1.0).acos();
        let axis = glam::DVec3::new(
            phi.sin() * theta.cos(),
            phi.sin() * theta.sin(),
            phi.cos(),
        );
        
        // Constraint 1: not aligned with z-axis
        let z_alignment = axis.dot(z_axis).abs();
        if z_alignment > 0.95 {
            continue;
        }
        
        // Constraint 2: angular distance to seed in [30°, 150°]
        let to_seed = axis.dot(seed_vec).clamp(-1.0, 1.0).acos();
        if !(PI * 30.0 / 180.0..=PI * 150.0 / 180.0).contains(&to_seed) {
            continue;
        }
        
        return axis;
    }
    
    // Fallback: return an axis perpendicular to the seed vector
    let perp = if seed_vec.dot(z_axis).abs() < 0.99 {
        seed_vec.cross(z_axis).normalize()
    } else {
        seed_vec.cross(glam::DVec3::X).normalize()
    };
    perp
}

fn sample_log_normal(rng: &mut rand::rngs::SmallRng, sigma: f64) -> f64 {
    // Box-Muller transform to get a normal sample, then exp it.
    use std::f64::consts::PI;
    let u1: f64 = rng.gen_range(1e-10..1.0);
    let u2: f64 = rng.gen();
    let z = (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos();
    (z * sigma).exp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::{create_world, WorldParameters};
    
    fn build_test_world() -> World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5; // small for fast tests
        create_world(params).expect("default world valid")
    }
    
    #[test]
    fn generates_expected_plate_count() {
        let mut world = build_test_world();
        let registry = generate_initial_plates(&mut world);
        let expected_total = (world.data.parameters.core.geology.initial_major_plate_count
            + world.data.parameters.core.geology.initial_minor_plate_count) as usize;
        assert_eq!(registry.count(), expected_total);
    }
    
    #[test]
    fn every_hex_has_a_plate() {
        let mut world = build_test_world();
        let _registry = generate_initial_plates(&mut world);
        for plate_id in &world.data.plate_id {
            assert_ne!(*plate_id, PlateId::NONE);
        }
    }
    
    #[test]
    fn plate_ids_are_valid() {
        let mut world = build_test_world();
        let registry = generate_initial_plates(&mut world);
        for plate_id in &world.data.plate_id {
            assert!(registry.get(*plate_id).is_some());
        }
    }
    
    #[test]
    fn major_minor_split_correct() {
        let mut world = build_test_world();
        let registry = generate_initial_plates(&mut world);
        let expected_major = world.data.parameters.core.geology.initial_major_plate_count as usize;
        let expected_minor = world.data.parameters.core.geology.initial_minor_plate_count as usize;
        let major_count = registry
            .iter()
            .filter(|p| p.plate_class == PlateClass::Major)
            .count();
        let minor_count = registry
            .iter()
            .filter(|p| p.plate_class == PlateClass::Minor)
            .count();
        assert_eq!(major_count, expected_major);
        assert_eq!(minor_count, expected_minor);
    }
    
    #[test]
    fn continental_fraction_approximate() {
        let mut world = build_test_world();
        let registry = generate_initial_plates(&mut world);
        let target_fraction = world.data.parameters.core.geology.initial_continental_fraction;
        let total = registry.count() as f32;
        let continental = registry
            .iter()
            .filter(|p| p.plate_type == PlateType::Continental)
            .count() as f32;
        let actual_fraction = continental / total;
        // Within ±1 plate of target
        let tolerance = 1.0 / total;
        assert!(
            (actual_fraction - target_fraction).abs() <= tolerance,
            "expected ~{target_fraction}, got {actual_fraction}"
        );
    }
    
    #[test]
    fn determinism_same_seed_same_result() {
        let mut world_a = build_test_world();
        let mut world_b = build_test_world();
        let registry_a = generate_initial_plates(&mut world_a);
        let registry_b = generate_initial_plates(&mut world_b);
        
        assert_eq!(registry_a.count(), registry_b.count());
        for hex_idx in 0..world_a.data.plate_id.len() {
            assert_eq!(world_a.data.plate_id[hex_idx], world_b.data.plate_id[hex_idx]);
        }
    }
    
    #[test]
    fn different_seeds_produce_different_results() {
        use genesis_core::WorldSeed;
        
        let mut params_a = WorldParameters::default();
        params_a.core.grid.subdivision_level = 5;
        params_a.core.seed = WorldSeed::from_integer(1);
        
        let mut params_b = WorldParameters::default();
        params_b.core.grid.subdivision_level = 5;
        params_b.core.seed = WorldSeed::from_integer(2);
        
        let mut world_a = create_world(params_a).unwrap();
        let mut world_b = create_world(params_b).unwrap();
        
        generate_initial_plates(&mut world_a);
        generate_initial_plates(&mut world_b);
        
        let mut differences = 0;
        for hex_idx in 0..world_a.data.plate_id.len() {
            if world_a.data.plate_id[hex_idx] != world_b.data.plate_id[hex_idx] {
                differences += 1;
            }
        }
        
        // At least 50% of hexes should differ between seeds
        let threshold = world_a.data.plate_id.len() / 2;
        assert!(
            differences > threshold,
            "expected substantial differences between seeds, got {differences}/{}",
            world_a.data.plate_id.len()
        );
    }
    
    #[test]
    fn motion_axes_are_unit_length() {
        let mut world = build_test_world();
        let registry = generate_initial_plates(&mut world);
        for plate in registry.iter() {
            let axis = glam::DVec3::new(plate.motion_axis[0], plate.motion_axis[1], plate.motion_axis[2]);
            let len = axis.length();
            assert!(
                (len - 1.0).abs() < 1e-6,
                "plate {:?} motion axis length {len}",
                plate.id
            );
        }
    }
    
    #[test]
    fn motion_rates_are_reasonable() {
        let mut world = build_test_world();
        let registry = generate_initial_plates(&mut world);
        for plate in registry.iter() {
            assert!(plate.motion_rate_rad_per_year > 0.0);
            // Convert back to cm/year to sanity check
            let planet_radius = world.data.parameters.core.planet.radius_km;
            let cm_per_year = plate.motion_rate_rad_per_year * planet_radius / 1e-5;
            // Reasonable range: 0.1 to 30 cm/year on default Earth
            assert!(
                cm_per_year > 0.05 && cm_per_year < 50.0,
                "plate {:?} velocity {} cm/year out of range",
                plate.id,
                cm_per_year
            );
        }
    }
    
    #[test]
    fn no_pentagon_special_handling_needed() {
        let mut world = build_test_world();
        let _registry = generate_initial_plates(&mut world);
        // All 12 pentagons should have valid plate assignments
        for pentagon_id in 0..12u32 {
            assert_ne!(world.data.plate_id[pentagon_id as usize], PlateId::NONE);
        }
    }
}
Add a plates_mut method on PlateRegistry:
rustimpl PlateRegistry {
    // ... existing methods ...
    
    pub(crate) fn plates_mut(&mut self) -> &mut BTreeMap<PlateId, Plate> {
        &mut self.plates
    }
}
Part 7: lib.rs
rust//! Tectonic simulation for Genesis Engine.
//!
//! Phase 1 deliverable: initial plate generation only. Motion, boundary effects,
//! and per-tick simulation are added in subsequent prompts.

pub mod initial_generation;
pub mod plate;

pub use initial_generation::generate_initial_plates;
pub use plate::{Plate, PlateClass, PlateRegistry, PlateType};
What "Done" Looks Like
After your changes:

cargo build --workspace succeeds
cargo test --workspace passes — all existing tests + the new ones in genesis_tectonics
cargo fmt --check passes
cargo clippy --workspace --all-targets is clean
cargo tree -p genesis_tectonics --depth 1 shows no bevy dependency
cargo run -p genesis_app still opens the smoke test window (no changes to rendering yet — the colored hexes you see are still grid-based, not plate-based; plate coloring is a later prompt)

What's NOT in Scope

No plate motion or per-tick simulation — that's prompt P1-2
No boundary detection or classification — prompt P1-3
No elevation updates from boundaries — prompt P1-4
No hot spots, volcanism, or erosion — later prompts
No event emission yet — except WorldFormation is added in this prompt
No rendering changes — Phase 3 work
No WorldFormation event integration with the event log — just emit a placeholder log line for now via tracing::info! in generate_initial_plates

If you find yourself reaching for any of the above, stop and confirm.
Process

Read Doc 06 v0.2 fully. This prompt implements about 20% of it (just initial generation). Future prompts implement the rest.
Update GeologyParameters and WorldData first. Run tests; existing tests should pass with the new fields.
Validate the new parameter fields. Add tests.
Create the genesis_tectonics crate scaffold. Update workspace Cargo.toml.
Implement Plate types and PlateRegistry.
Implement generate_initial_plates with all helpers. This is the substantive piece.
Write the tests listed above. Run them.
Run the full verification suite.
Show actual command outputs.
Do not commit.

When You Finish
Provide a summary with:

Directory tree of files added/changed
Actual command outputs from build/test/fmt/clippy
New test count (~10 in tectonics, plus updates to existing tests)
Confirmation genesis_tectonics has no Bevy dependency
Confirmation genesis_core still has no Bevy dependency
Memory size of a typical PlateRegistry at default settings (rough estimate is fine)
Any decisions you made that weren't fully specified
Any inconsistencies you noticed in Doc 06
Any sections that need clarification before P1-2 (plate motion)

Ask clarifying questions before starting if anything is unclear. If everything is clear, say so and begin.

=== END PROMPT ===
```

### P1-1.5 — Remediation

*Source: transcript `f65ad57f`, assistant message ~line 18.*

```markdown
=== START PROMPT ===

Read these documents before starting, in order:

- `CONTRIBUTING-AI.md` — collaboration guide
- `docs/03-glossary-and-naming.md` — terminology
- `docs/06-tectonics-module-specification.md` v0.2 — Phase 1 tectonics
  - Read every section, but pay particular attention to:
    - §2.1 (Motion axis constraints — centroid, not seed)
    - §2.2 (Initial plate layout — minor target fractions, growth neighbor selection)
    - §4.4 (RNG stream names)
    - §10 (Determinism requirements)
- `docs/04-data-layer-specification.md` — WorldData, parameters, RNG (no code changes expected unless you discover a conflict)
- Existing code in `crates/genesis_tectonics/src/` and `crates/genesis_core/src/` for style

If you cannot access any of these, stop and tell me.

## Your Task

Remediate P1-1 initial plate generation so behavior matches Doc 06 where P1-1 already claimed compliance. This is a correctness-and-contract pass on `generate_initial_plates` — not new simulation features.

Deliverable: updated `initial_generation.rs` (and minimal `plate.rs` changes if needed) with spec-aligned motion-axis constraints, RNG stream names, minor-plate target fractions, and documented growth-neighbor selection. All existing P1-1 tests must still pass; add targeted tests for each fix.

Do not wire Formation tick, do not add `accumulated_rotation_rad`, do not implement per-tick Voronoi re-partition.

## What "Done" Looks Like

After this prompt:

- Motion-axis rejection uses each plate’s **centroid** (mean of owned hex center directions), not `seed_hex`, per Doc 06 §2.1
- RNG streams match Doc 06 §4.4 for this slice:
  - `tectonics.plate_seeds` — seed placement and growth plate-selection rounds
  - `tectonics.plate_axes` — motion axis sampling
  - `tectonics.plate_rates` — log-normal rate sampling
  (Remove or migrate `tectonics.plate_motion`, `tectonics.plate_seeds.major`, `tectonics.plate_growth.*`, etc.)
- Minor plate `target_fraction` values are sampled ~0.03–0.07 of the **sphere** each, not normalized to a 0.50 major budget
- Growth neighbor selection behavior is explicit: either (a) random unowned neighbor with `HexId` tie-break per §2.2, or (b) deterministic lowest-`HexId` with a one-line code comment + note in your summary flagging the doc deviation for Brax to accept or reject
- Poisson-disk seed fallback cannot silently violate spacing without a test documenting the behavior on constrained grids
- `cargo build --workspace` succeeds
- `cargo test --workspace` passes — all existing tests plus new/updated tests in `genesis_tectonics`
- `cargo fmt --check` passes
- `cargo clippy --workspace --all-targets` is clean (no warnings, or explicit justifications)
- `cargo tree -p genesis_tectonics --depth 1` — confirm no Bevy
- Do not commit unless explicitly asked

## Determinism (this slice)

- RNG stream names: `tectonics.plate_seeds`, `tectonics.plate_axes`, `tectonics.plate_rates` only (for initial generation randomness)
- Sort collections before use when order affects state: frontier candidates, plate iteration, eligible-plate lists
- No system time, no file I/O during ticks

## Implementation Plan

Remediate in dependency order: RNG names and target fractions first (partition layout may shift), then growth neighbor policy, then motion-axis centroid.

### Part 1: Align RNG stream names

**Files:** `crates/genesis_tectonics/src/initial_generation.rs`

**Changes:**
- Replace fine-grained stream names with Doc 06 §4.4 names
- Use `tectonics.plate_seeds` for Poisson seed placement, growth round plate selection, and (if implementing random neighbors) per-plate neighbor choice
- Split motion into `tectonics.plate_axes` and `tectonics.plate_rates`

**Tests:**
- Existing `determinism_same_seed_same_result` still passes (update if stream rename is intentional breaking change — document in summary)
- Add or update a test that two worlds with the same seed still match after remediation

### Part 2: Fix minor `target_fraction` sampling

**Files:** `crates/genesis_tectonics/src/initial_generation.rs`

**Changes:**
- Sample each minor plate’s `target_fraction` in ~0.03–0.07 of total cells (deterministic via `tectonics.plate_seeds` or a dedicated draw during minor setup)
- Do not pass `total_target: 0.50` to minor plates (that budget is for majors only)

**Tests:**
- Assert each minor plate’s `target_fraction` is in `0.03..=0.07` after generation (with tolerance for normalization if any)

### Part 3: Motion-axis constraint uses plate centroid

**Files:** `crates/genesis_tectonics/src/initial_generation.rs`

**Changes:**
- After full growth, compute each plate’s centroid from hexes assigned in `world.data.plate_id`
- Apply 30°–150° angular-distance rejection against centroid, not `seed_hex`
- Keep z-axis alignment rejection (>0.95) unchanged

**Tests:**
- Unit test: synthetic plate with offset centroid vs seed — axis rejected/accepted differently than seed-only logic would produce
- Existing `motion_axes_are_unit_length` still passes

### Part 4: Growth neighbor selection policy

**Files:** `crates/genesis_tectonics/src/initial_generation.rs`

**Changes:**
- **Preferred:** random unowned frontier neighbor per §2.2, `HexId` tie-break on equal draws, via `tectonics.plate_seeds`
- **Acceptable alternative:** keep lowest-`HexId` deterministic pick, with comment citing §2.2 and summary note for doc review

**Tests:**
- If random: same-seed determinism test still passes
- If deterministic: no new test required beyond existing determinism test

### Part 5: Poisson-disk fallback hardening

**Files:** `crates/genesis_tectonics/src/initial_generation.rs`

**Changes:**
- Document or tighten fallback when `max_attempts` exhausts (e.g. relax distance gradually, or deterministic fill ordered by `HexId`)
- Avoid duplicate seeds in fallback path

**Tests:**
- Small-grid or high-count edge case: seed placement completes without panic and respects spacing where possible

## What's NOT in Scope

- Storing `PlateRegistry` on `World` or Bevy resources (P1-2)
- Formation-tick wiring / `generate_initial_plates` called from lifecycle (P1-2)
- `accumulated_rotation_rad`, per-tick motion, Voronoi re-partition (P1-2)
- Doc 06 §4.3 steps 5–9: elevation, bedrock, sea level, `WorldFormation` event (P1-2 or P1-3)
- Boundary detection, elevation dynamics, hot spots, erosion, reorganization (P1-3+)
- Editing `/docs/` (flag inconsistencies in summary only)
- Rendering changes (Phase 3)

If you find yourself reaching for any of the above, stop and confirm.

## Process

1. Read `docs/06-tectonics-module-specification.md` fully. This prompt implements approximately 5% additional alignment on top of P1-1 (~25% of Doc 06 total).
2. Implement in the order of the Implementation Plan. Run tests after each logical chunk when practical.
3. Run the full verification suite. Show **actual command output** in your summary.
4. Follow project rules in `.cursor/rules/project.mdc` and `CONTRIBUTING-AI.md` (determinism, glossary terms, no scope creep).
5. Do not edit `/docs/` unless I explicitly ask.

## When You Finish

Provide a summary with:

- Directory tree of files added/changed
- Actual command outputs from build / test / fmt / clippy (and `cargo tree -p genesis_tectonics --depth 1`)
- New and updated test counts
- Confirm `genesis_tectonics` and `genesis_core` have no Bevy dependency
- Note whether stream renames changed same-seed `plate_id` layouts (expected once)
- Decisions you made that were not fully specified in the docs (especially growth neighbor random vs deterministic)
- Inconsistencies you noticed in the authoritative specs
- Sections that need clarification before P1-2

Ask clarifying questions before starting if anything is unclear.
If everything is clear, say so and begin.

=== END PROMPT ===
```

### P1-2 — Plate motion, Voronoi re-partition, tick integration

*Source: transcript `f65ad57f`, assistant message ~line 32.*

```markdown
=== START PROMPT ===

Read these documents before starting, in order:

- `CONTRIBUTING-AI.md` — collaboration guide
- `docs/03-glossary-and-naming.md` — terminology
- `docs/06-tectonics-module-specification.md` v0.2 — Phase 1 tectonics
  - Read every section, but pay particular attention to:
    - §2.2 (Ongoing simulation: rotated-seed Voronoi)
    - §4.1–§4.2 (Tick intervals; per-tick steps 1–2 only)
    - §4.3 steps 1–4 (Formation uses existing `generate_initial_plates`)
    - §10 (Determinism — `HexId` iteration order, `f64` accumulation)
    - §13 (File organization — `motion.rs`, `partition.rs`, `TectonicsLayer`)
- `docs/04-data-layer-specification.md` — `WorldData`, `SimulationLayer`, `TickCoordinator`, RNG
- `docs/02-architecture-overview.md` §12 Phase 1 — geology prototype build order
- Existing code in `crates/genesis_tectonics/src/` and `crates/genesis_core/src/time/ticks.rs` for style

If you cannot access any of these, stop and tell me.

## Your Task

Implement **P1-2: plate motion, Voronoi re-partition, and tick integration** (Doc 06 §15 step 2).

Deliverables:

1. **`accumulated_rotation_rad`** on `Plate`; motion math rotates each plate’s seed center direction about its `motion_axis`.
2. **`motion.rs`** — increment accumulator per tick; compute effective position for partition.
3. **`partition.rs`** — Voronoi reassignment of `WorldData.plate_id` from effective positions; iterate hexes in ascending `HexId`; ties break on lowest `PlateId`.
4. **`TectonicsState`** holding `PlateRegistry` (and formation flag); **`TectonicsLayer`** implementing `genesis_core::SimulationLayer`.
5. **Lifecycle wiring** so Formation runs `generate_initial_plates` at year 0 and Geological ticks run motion + repartition only (Doc §4.2 steps 1–2).
6. **`genesis_app`** depends on `genesis_tectonics` and runs formation (or short history) so worlds are not left at `PlateId::NONE`.

Do **not** put `PlateRegistry` inside `genesis_core::World` (keeps `genesis_core` independent of `genesis_tectonics`).

## Determinism (this slice)

- No new RNG streams required for motion/partition (deterministic geometry).
- Re-partition: iterate `HexId` ascending; tie-break `PlateId` ascending.
- Accumulate rotation in `f64` on `Plate`.
- Sort any plate iteration where order affects state (`BTreeMap` / sorted `PlateId` lists).
- No system time, no file I/O during ticks.

## What "Done" Looks Like

After this prompt:

- `Plate` has `accumulated_rotation_rad: f64` (0.0 at formation).
- Formation tick (year 0): `generate_initial_plates` runs once; every hex has a valid `plate_id`; registry stored in `TectonicsState`.
- Geological-era ticks: each tick increments rotation then repartitions all hexes; same seed + same tick count → identical `plate_id` layout.
- `TickCoordinator` runs Formation at `world_start_year` (not skipped — see Part 6).
- `genesis_tectonics` exposes a registration/advance helper usable from tests and `genesis_app` without `genesis_core` → `genesis_tectonics` dependency.
- `cargo build --workspace` succeeds
- `cargo test --workspace` passes — all existing tests plus new tests in `genesis_tectonics` and any updated `genesis_core` tick tests
- `cargo fmt --check` passes
- `cargo clippy --workspace --all-targets` is clean (no warnings, or explicit justifications)
- `cargo tree -p genesis_tectonics --depth 1` — no Bevy
- `cargo tree -p genesis_core --depth 1` — no Bevy
- `cargo build -p genesis_app` succeeds
- Do not commit unless explicitly asked

## Implementation Plan

Implement bottom-up: plate field → motion → partition → layer → coordinator fix → app wiring.

### Part 1: Extend `Plate` and `TectonicsState`

**Files:** `crates/genesis_tectonics/src/plate.rs`, `crates/genesis_tectonics/src/lib.rs`

**Changes:**
- Add `pub accumulated_rotation_rad: f64` to `Plate` (`Serialize`/`Deserialize` if `Plate` already derives them).
- Initialize to `0.0` in `initial_generation.rs` when plates are created.
- Add `pub struct TectonicsState { pub registry: PlateRegistry, pub formation_complete: bool }` with sensible `Default` / `new()`.
- Re-export `TectonicsState` from `lib.rs`.

**Tests:**
- Serialization round-trip or default state smoke test if you add serde to state.

### Part 2: Motion math (`motion.rs`)

**Files:** `crates/genesis_tectonics/src/motion.rs`, `crates/genesis_tectonics/src/lib.rs`

**Changes:**
- `effective_position_direction(grid, plate) -> [f64; 3]` — rotate seed hex center about `motion_axis` by `accumulated_rotation_rad` (unit vector out).
- `advance_plate_motion(plate, tick_interval_years: f64)` — increment `accumulated_rotation_rad`.
- Use `glam` / `f64`; normalize outputs.

**Tests:**
- Zero rotation → effective position equals seed direction.
- Known axis + rate + interval → expected angle within tolerance.
- Axis remains unit length after rotation math.

### Part 3: Voronoi partition (`partition.rs`)

**Files:** `crates/genesis_tectonics/src/partition.rs`, `crates/genesis_tectonics/src/lib.rs`

**Changes:**
- `repartition_hexes(world_data, registry, grid)` — for each hex in `HexId` order, assign nearest plate by angular distance (dot product on unit vectors).
- Tie-break: lowest `PlateId`.
- Mutate `world_data.plate_id` in place.

**Tests:**
- Two plates, controlled effective positions → known hex ownership.
- Tie case: equidistant hex assigns to lower `PlateId`.
- Full grid repartition after synthetic rotation changes some assignments.

### Part 4: `TectonicsLayer` (`SimulationLayer`)

**Files:** `crates/genesis_tectonics/src/layer.rs` (or `lib.rs`), `crates/genesis_tectonics/src/lib.rs`

**Changes:**
- `struct TectonicsLayer { state: TectonicsState }` (or `&mut TectonicsState` held by caller — layer must own or borrow registry across ticks).
- `impl SimulationLayer for TectonicsLayer`:
  - `name()` → `"tectonics"`
  - `tick_interval(current, params)`:
    - `Era::Formation` at `world_start_year` → return Geological default interval (`500_000` years, or `params.core.geology.tick_interval_overrides_years` for `Era::Geological`)
    - `Era::Geological` → same interval
    - All other eras → `0` (dormant)
  - `advance(world, rng)`:
    - If `!formation_complete` and `Era::Formation`: call `generate_initial_plates` via `World` adapter or pass grid+params+rng from `WorldData` + external `WorldRng` (see note below), store registry, set flag, return.
    - If `Era::Geological`: for each plate in deterministic `PlateId` order, advance motion; then `repartition_hexes`.
    - Return empty `Vec<()>` for now (events deferred).
- Public helper, e.g. `pub fn register_tectonics(coordinator: &mut TickCoordinator, state: TectonicsState) -> TectonicsLayer` or `pub fn generate_full_history_with_tectonics(world: &mut World, state: &mut TectonicsState, target: WorldYear, progress: impl FnMut(...))` that registers the layer and calls `genesis_core::lifecycle::generate_full_history` internals.

**Note:** `SimulationLayer::advance` receives `WorldData` + `WorldRng`, not `World`. Either:
- extend the layer to hold `&mut World` only in the helper path used at runtime, or
- add a thin `advance_tectonics(world: &mut World, state: &mut TectonicsState)` used from a wrapper that the coordinator calls via a small adapter layer in tests/app.

Pick the smallest change that keeps `genesis_core` free of `genesis_tectonics`. Document your choice in the summary.

**Tests:**
- Formation-only: registry populated, all hexes assigned.
- One Geological tick at subdiv 5: `plate_id` differs from post-formation snapshot for at least some hexes (or rotation nonzero and partition stable).
- Two Geological ticks, same seed: deterministic `plate_id`.

### Part 5: `genesis_app` wiring

**Files:** `crates/genesis_app/Cargo.toml`, `crates/genesis_app/src/main.rs`

**Changes:**
- Add `genesis_tectonics` dependency.
- After `create_world`, run formation (via tectonics helper) so `WorldResource` has real plates before render.
- Optional: `generate_full_history` to `WorldYear(500_000)` or `1_000_000` for a minimal motion smoke — only if it stays fast at subdiv 6.

**Tests:**
- Existing `genesis_app` plugin build test still passes.

### Part 6: Formation tick at year 0 (`TickCoordinator`)

**Files:** `crates/genesis_core/src/time/ticks.rs`, tests in same file

**Changes:**
- Fix coordinator so the **first** scheduled tick is at `world_start_year`, not `start + interval`. Today `advance_to` from `WorldYear::FORMATION` with interval 100 skips year 0 (see `advance_to_ticks_every_100_years` test).
- After fix: first tick year is `world_start_year`, then `start + interval`, etc.
- Update affected tick coordinator tests accordingly.

**Tests:**
- Recording layer receives `WorldYear::FORMATION` (or `world_start_year`) as first tick when starting at formation.

### Part 7: Integration tests in `genesis_tectonics`

**Files:** `crates/genesis_tectonics/src/lib.rs` or `tests/`

**Changes:**
- End-to-end: `create_world` + tectonics helper → formation → N geological ticks → determinism snapshot on `plate_id`.
- `generate_full_history` with tectonics registered advances `current_year` correctly.

**Tests:**
- As above; keep subdiv level low (5 or 6) for speed.

## What's NOT in Scope

- Doc 06 §4.3 steps **5–9** (initial elevation, bedrock, sea level tweak, fertility assert, `WorldFormation` event) — **P1-2b**
- Doc 06 §4.2 steps **3–9** (boundaries, elevation dynamics, hot spots, erosion, reorganization, sea level drift, events) — **P1-3+**
- Boundary detection / classification — **P1-3**
- Rendering by elevation or plate color — Phase 3 / Doc §15 step 10
- Partition motion threshold / “only repartition when accumulated motion > ε” — optional optimization later; repartition every Geological tick is fine for P1-2
- `EventKind::WorldFormation` (still placeholder in core) — **P1-2b or P1-8**
- Editing `/docs/` unless I explicitly ask
- New crate dependencies without asking

If you find yourself reaching for any of the above, stop and confirm.

## Process

1. Read `docs/06-tectonics-module-specification.md` fully. This prompt implements approximately **20%** additional coverage (~45% of Doc 06 cumulative with P1-1 + P1-1.5).
2. Implement in the order of the Implementation Plan. Run tests after each logical chunk when practical.
3. Run the full verification suite. Show **actual command output** in your summary.
4. Follow project rules in `.cursor/rules/project.mdc` and `CONTRIBUTING-AI.md` (determinism, glossary terms, no scope creep).
5. Do not edit `/docs/` unless I explicitly ask.

## When You Finish

Provide a summary with:

- Directory tree of files added/changed
- Actual command outputs from build / test / fmt / clippy (and `cargo tree` / `cargo build -p genesis_app`)
- New and updated test counts
- Confirm `genesis_tectonics` and `genesis_core` have no Bevy dependency
- How `TectonicsState` is owned and passed (app `Resource`, test-local, etc.)
- How Formation year-0 tick was fixed in `TickCoordinator`
- Decisions you made that were not fully specified in the docs
- Inconsistencies you noticed in the authoritative specs
- Sections that need clarification before **P1-2b** (initial elevation/bedrock) and **P1-3** (boundaries)

Ask clarifying questions before starting if anything is unclear.
If everything is clear, say so and begin.

=== END PROMPT ===
```

### P1-3 — Boundary detection and classification

*Source: transcript `f65ad57f`, assistant message ~line 45.*

```markdown
=== START PROMPT ===

Read these documents before starting, in order:

- `CONTRIBUTING-AI.md` — collaboration guide
- `docs/03-glossary-and-naming.md` — terminology (hex, plate, world)
- `docs/06-tectonics-module-specification.md` v0.2 — Phase 1 tectonics
  - Read every section, but pay particular attention to:
    - §3 (Boundary detection and classification)
    - §3.4 (Velocity computation at a point)
    - §4.2 step 3 (Per-tick: detect and classify — this prompt only)
    - §10 (Determinism)
    - §12.2 (Triple junctions — pairwise classification)
    - §13 (`boundary.rs` module layout)
- `docs/04-data-layer-specification.md` — `HexGrid::neighbors`, `WorldData.plate_id`
- Existing code in `crates/genesis_tectonics/src/` (`layer.rs`, `motion.rs`, `partition.rs`, `plate.rs`)

If you cannot access any of these, stop and tell me.

## Your Task

Implement **P1-3: boundary detection and classification** (Doc 06 §15 step 3).

After each Geological-era tick’s motion update and Voronoi re-partition (already in `TectonicsLayer`), detect all **boundary hexes** (hexes with at least one neighbor on a different plate) and **classify** each cross-plate neighbor pair as divergent, convergent, or transform, including convergent subtypes (continental–continental, oceanic–oceanic, continental–oceanic).

Deliverables:

1. **`boundary.rs`** — types and algorithms per Doc 06 §3.1–§3.4.
2. **`BoundaryInfo`** stored on **`TectonicsState`** (derived each tick; **not** new fields on `WorldData`).
3. **`TectonicsLayer`** calls boundary detection/classification after `repartition_hexes` on Geological ticks.
4. Public API exported from `genesis_tectonics` for tests and future P1-4 (`elevation.rs`).

Do **not** modify `elevation_mean`, `bedrock_type`, or emit events in this slice.

## Determinism (this slice)

- Boundary detection: iterate hexes in ascending `HexId` order.
- Neighbor pairs and plate contacts: use `BTreeSet` / sorted order where iteration affects stored results.
- Classification is deterministic from plate motion (no RNG in §3).
- Tie-breaking for any ambiguous geometric case: lowest `HexId` or lowest `PlateId` — document in summary.
- No system time, no file I/O during ticks.

## What "Done" Looks Like

After this prompt:

- `BoundaryInfo` contains `boundary_hexes: Vec<HexId>` (sorted) and `plate_contacts: BTreeMap<HexId, BTreeSet<PlateId>>` per Doc 06 §3.1.
- Each boundary hex has one or more **classified edges** to neighbor plates (pairwise). Triple junctions list multiple contacts (Doc §12.2).
- Classification uses relative surface velocity at the hex center per §3.4: `v_A(p) = (ω_A × p)`, `v_rel = v_A - v_B`, decomposed into normal/tangent to the local edge frame.
- Transform when `|normal| < 0.3 * |tangential|`; else divergent (normal negative) or convergent (normal positive) per §3.2.
- Convergent edges carry subtype per §3.3 from the two plates’ `PlateType`s.
- Geological tick order in `TectonicsLayer`: motion → repartition → **boundaries** (unchanged Formation path).
- `cargo build --workspace` succeeds
- `cargo test --workspace` passes — all existing tests plus new tests in `genesis_tectonics`
- `cargo fmt --check` passes
- `cargo clippy --workspace --all-targets` is clean (no warnings, or explicit justifications)
- `cargo tree -p genesis_tectonics --depth 1` — no Bevy
- Do not commit unless explicitly asked

## Implementation Plan

### Part 1: Boundary types (`boundary.rs`)

**Files:** `crates/genesis_tectonics/src/boundary.rs`, `crates/genesis_tectonics/src/lib.rs`

**Changes:**
- Define types (names may follow spec intent; use glossary terms):

```rust
// Illustrative — align with Doc 06 §3; adjust as needed and document.
pub enum BoundaryClass {
    Divergent,
    Convergent(ConvergentSubtype),
    Transform,
}

pub enum ConvergentSubtype {
    ContinentalContinental,
    OceanicOceanic,
    ContinentalOceanic, // doc: continental-oceanic subduction
}

pub struct ClassifiedEdge {
    pub neighbor_hex: HexId,
    pub other_plate: PlateId,
    pub class: BoundaryClass,
    // optional: normal_velocity_m_per_year, tangential_velocity_m_per_year for P1-4
}

pub struct BoundaryInfo {
    pub boundary_hexes: Vec<HexId>,
    pub plate_contacts: BTreeMap<HexId, BTreeSet<PlateId>>,
    /// Per boundary hex, sorted classified edges (e.g. BTreeMap or Vec sorted by neighbor HexId).
    pub edges: BTreeMap<HexId, Vec<ClassifiedEdge>>,
}
```

- `ConvergentSubtype` from `(PlateType, PlateType)` of owner vs neighbor plate (§3.3).

**Tests:**
- Subtype mapping for all three convergent pairs.

### Part 2: Velocity and local frame (§3.4)

**Files:** `crates/genesis_tectonics/src/boundary.rs` (or `motion.rs` helpers if shared)

**Changes:**
- `surface_velocity_at(hex_center_dir, motion_axis, motion_rate_rad_per_year) -> DVec3` using `ω = axis * rate`, `v = ω × p` (convert rad/year at sphere radius to m/year using `planet.radius_km` from `WorldData.parameters`).
- For boundary hex `h` and neighbor `n` on plate B: build edge normal/tangent on the sphere (great-circle geometry between `h` and `n` centers; normal points from owner plate toward neighbor plate for sign convention).
- Project `v_rel` onto normal and tangent; apply §3.2 threshold `0.3`.

**Tests:**
- Synthetic ω/r and known `p`: expected divergent vs convergent vs transform.
- Sign convention: separating plates → divergent (negative normal per doc).

### Part 3: Detection and classification

**Files:** `crates/genesis_tectonics/src/boundary.rs`

**Changes:**
- `detect_and_classify_boundaries(data: &WorldData, registry: &PlateRegistry) -> BoundaryInfo`
- Scan all hexes in `HexId` order; for each neighbor with `plate_id[neighbor] != plate_id[h]`, classify pair `(owner_plate, neighbor_plate)` once per directed edge or store undirected pairs consistently (document choice).
- Populate `boundary_hexes`, `plate_contacts`, `edges`.
- Pentagons: use `grid.neighbors(hex)` as-is (5 neighbors); no special case beyond doc §12.4.

**Tests:**
- Two-plate synthetic world: shared edge hexes appear in `boundary_hexes`.
- Triple junction fixture (three plates meeting): one hex contacts ≥2 foreign plates.
- `plate_contacts` matches neighbor scan.
- Full `generate_full_history_with_tectonics` to 1M years: `boundary_hexes` non-empty on default seed.

### Part 4: Wire into `TectonicsState` and `TectonicsLayer`

**Files:** `crates/genesis_tectonics/src/plate.rs`, `crates/genesis_tectonics/src/layer.rs`, `crates/genesis_tectonics/src/lib.rs`

**Changes:**
- Add `pub boundaries: BoundaryInfo` (or `Option<BoundaryInfo>` cleared before Formation) to `TectonicsState`.
- After `repartition_hexes` on Geological ticks: `state.boundaries = detect_and_classify_boundaries(world, &state.registry)`.
- Re-export `BoundaryInfo`, `BoundaryClass`, `ConvergentSubtype`, `detect_and_classify_boundaries` from `lib.rs`.

**Tests:**
- Integration: after one geological tick, `state.boundaries.boundary_hexes.len() > 0`.
- Determinism: two worlds, same seed, same `boundary_hexes` and edge classes at year 1M.

### Part 5: Optional debug timing

**Files:** `crates/genesis_tectonics/src/layer.rs`

**Changes:**
- If cheap: `tracing::debug!` boundary hex count per tick when `RUST_LOG=genesis_tectonics=debug` (Doc §9.3). Skip if it adds noise.

## What's NOT in Scope

- **P1-2b** — Doc 06 §4.3 steps 5–8 (initial elevation, bedrock, sea level on Formation)
- **P1-4** — Doc 06 §5 elevation/bedrock updates per boundary type (§4.2 step 4)
- **P1-5+** — hot spots, erosion, reorganization, sea level drift (§4.2 steps 5–8)
- **Events** — `BoundaryTransition`, `MountainRangeFormed`, etc. (§6; P1-8 or later)
- Changing `WorldData` schema for boundaries (spec: derived only)
- Rendering / `genesis_render` changes
- Editing `/docs/` unless I explicitly ask
- New crate dependencies without asking

If you find yourself reaching for any of the above, stop and confirm.

## Process

1. Read `docs/06-tectonics-module-specification.md` §3 fully. This prompt implements ~15% of Doc 06 (~60% cumulative with P1-1, P1-1.5, P1-2).
2. Implement in Implementation Plan order; run tests after each part.
3. Run the full verification suite. Show **actual command output** in your summary.
4. Follow `.cursor/rules/project.mdc` and `CONTRIBUTING-AI.md`.
5. Do not edit `/docs/` unless I explicitly ask.

## When You Finish

Provide a summary with:

- Directory tree of files added/changed
- Actual command outputs from build / test / fmt / clippy
- New and updated test counts
- Confirm no Bevy on `genesis_tectonics` / `genesis_core`
- Sign convention used for normal velocity (divergent negative)
- How directed edges are stored (h→n vs undirected pairs)
- Sample counts on default seed at year 1M (boundary hex count, % of grid)
- Decisions not fully specified in the docs
- Spec inconsistencies noticed
- Sections that need clarification before **P1-4** (elevation dynamics)

Ask clarifying questions before starting if anything is unclear.
If everything is clear, say so and begin.

=== END PROMPT ===
```

### P1-4 — Terrain sculpting

*Source: transcript `f65ad57f`, assistant message ~line 53.*

```markdown
=== START PROMPT ===

Read these documents before starting, in order:

- `CONTRIBUTING-AI.md` — collaboration guide
- `docs/03-glossary-and-naming.md` — terminology
- `docs/06-tectonics-module-specification.md` v0.2 — Phase 1 tectonics
  - Read every section, but pay particular attention to:
    - §4.3 steps 5–8 (Formation: initial elevation, bedrock, sea level, fertility)
    - §5.1–§5.4, §5.6–§5.7 (Elevation rules, bounds; skip §5.5 volcanism for this prompt)
    - §4.2 step 4 (Per-tick: apply boundary effects to elevation)
    - §4.4 (`tectonics.initial_elevation_noise`)
    - §10 (Determinism)
    - §12.2 (Triple junctions — additive effects)
- `docs/04-data-layer-specification.md` — `WorldData.elevation_mean`, `elevation_relief`, `bedrock_type`, `sea_level_m`
- Existing code: `crates/genesis_tectonics/src/boundary.rs`, `layer.rs`, `initial_generation.rs`, `crates/genesis_core/src/data/enums.rs`

If you cannot access any of these, stop and tell me.

## Your Task

Implement **P1-4: terrain sculpting** — Formation baseline elevation plus per-tick boundary elevation/bedrock updates (Doc 06 §15 steps 4 + deferred §4.3 steps 5–8).

**Part A (Formation, year 0):** After `generate_initial_plates_data`, set per-hex `elevation_mean` (~500m continental / ~-3500m oceanic ± noise), `bedrock_type` (`Igneous` / `OceanicCrust`), confirm `sea_level_m == 0`, `fertility == 0`.

**Part B (Geological ticks):** After `detect_and_classify_boundaries`, apply §5.1–§5.4 and §5.6 to `elevation_mean`, `elevation_relief`, and `bedrock_type` using `BoundaryInfo` and classified edge velocities. Clamp per §5.7.

Do **not** implement §5.5 boundary volcanism, event emission, or rendering changes in this slice.

## Determinism (this slice)

- `tectonics.initial_elevation_noise` for Formation per-hex variation (§4.4)
- Iterate boundary hexes and inland spread in ascending `HexId` order
- Subduction side (upper/lower oceanic) must be deterministic (e.g. oceanic plate subducts under continental; for oceanic–oceanic, lower `PlateId` subducts or faster plate — document choice)
- No system time, no file I/O during ticks
- Volcanism RNG deferred — no `tectonics.volcanism` in this prompt

## What "Done" Looks Like

After this prompt:

- Formation sets non-flat elevation and bedrock matching plate type; tests assert continental/oceanic bands and bedrock variants
- Each Geological tick: motion → repartition → boundaries → **elevation/bedrock updates** → clamp
- Divergent, convergent (all three subtypes), and transform rules applied per §5; inland falloff for continental–continental orogeny (2–3 hexes)
- `MIN_ELEVATION_M` / `MAX_ELEVATION_M` / `MAX_RELIEF_M` enforced (§5.7)
- `cargo build --workspace` / `cargo test --workspace` / `cargo fmt --check` / `cargo clippy --workspace --all-targets` all pass
- `cargo tree -p genesis_tectonics --depth 1` — no Bevy
- Optional: `cargo run -p genesis_app` — terrain should differ visually if render uses `elevation_mean` (if not, note in summary; render wiring may be a follow-up)
- Do not commit unless explicitly asked

## Implementation Plan

### Part 1: Formation initial terrain (`initial_terrain.rs` or extend `initial_generation.rs`)

**Files:** `crates/genesis_tectonics/src/`, `layer.rs`

**Changes:**
- `apply_formation_terrain(data: &mut WorldData, registry: &PlateRegistry, rng: &WorldRng)`
- Per hex: elevation from owner plate `PlateType` (~500m / ~-3500m) + `rng.stream("tectonics.initial_elevation_noise")` ±200m
- Bedrock: continental → `Igneous`, oceanic → `OceanicCrust`
- `sea_level_m = 0`; assert or set `fertility` to 0
- Call from Formation path in `TectonicsLayer` after plate generation (same tick)

**Tests:**
- Post-formation: mean continental hex elevation > oceanic hex elevation
- Bedrock types match plate types on sample hexes
- Same seed → identical `elevation_mean` snapshot

### Part 2: Elevation module (`elevation.rs`)

**Files:** `crates/genesis_tectonics/src/elevation.rs`, `lib.rs`

**Changes:**
- Constants: `MIN_ELEVATION_M`, `MAX_ELEVATION_M`, `MAX_RELIEF_M`, `subsidence_rate`, `orogeny_rate`, `subduction_rate` per §5 (document values in summary)
- `apply_boundary_elevation(data, registry, boundaries, tick_interval_years: f64)`:
  - Walk `boundaries.boundary_hexes` in `HexId` order
  - For each `ClassifiedEdge`, apply §5.1–§5.4 using `normal_velocity_m_per_year` (convert to cm/year if formulas use cm)
  - Continental–continental: inland spread 2–3 hexes on continental side with falloff
  - Oceanic–continental / oceanic–oceanic: determine oceanic vs continental **side** using `owner_hex` plate type and `other_plate`; trench on subducting side, uplift on overriding side (resolve P1-3 gap)
  - Transform (§5.6): gradual `Metamorphic` bedrock only; minimal elevation change
  - Divergent (§5.1): subsidence; `OceanicCrust` bedrock; optional continental rifting slower drop (defer sustained-rift timer to simple heuristic or note as follow-up)
- Do **not** reassign `plate_id` at divergent boundaries unless spec requires — if implemented, test determinism

**Tests:**
- Synthetic boundary fixture: divergent edge lowers elevation; convergent CC raises; OC trench on oceanic side
- Clamping: input that would exceed `MAX_ELEVATION_M` is capped
- Triple junction: multiple edges on one hex apply additively without panic

### Part 3: Wire into `TectonicsLayer`

**Files:** `crates/genesis_tectonics/src/layer.rs`

**Changes:**
- Formation: `generate_initial_plates_data` → `apply_formation_terrain`
- Geological: after `detect_and_classify_boundaries`, call `apply_boundary_elevation` with `interval_years`, then clamp all hexes (or clamp inside elevation module)

**Tests:**
- Integration: history to 1M years — `elevation_mean` min < -1000, max > 0 on default seed (loose sanity)
- Determinism: same seed → identical elevation arrays at 1M years

### Part 4: Optional render smoke note

**Files:** `crates/genesis_render` only if elevation coloring already exists or is trivial

**Changes:**
- If render still flat: document; do not block P1-4 on Phase 3 render work

## What's NOT in Scope

- §5.5 boundary volcanism and `tectonics.volcanism` stream — **P1-5**
- Hot spots (§7) — **P1-6**
- Erosion (§8) — **P1-7**
- Events (`WorldFormation`, `MountainRangeFormed`, `VolcanicEruption`) — **P1-8**
- Sea level drift (§4.6) — later
- Plate reorganization — **P1-7**
- Repartition motion threshold (§9.2) — optimization later
- Editing `/docs/` unless I explicitly ask
- New dependencies without asking

If you find yourself reaching for any of the above, stop and confirm.

## Process

1. Read Doc 06 §5 fully. This prompt adds ~20% spec coverage (~80% cumulative Phase 1 tectonics core loop).
2. Implement Parts 1–3 in order; run tests after each part.
3. Full verification suite with actual output in summary.
4. Follow `.cursor/rules/project.mdc` and `CONTRIBUTING-AI.md`.
5. Do not edit `/docs/` unless I explicitly ask.

## When You Finish

Provide a summary with:

- Files added/changed tree
- Build / test / fmt / clippy outputs
- Formation elevation stats (continental vs oceanic means) on default seed
- Elevation min/max/mean at 1M years after 2 geological ticks
- Subduction side rule used for OC and OO boundaries
- Calibration constants chosen and any deviations from §5 rates
- Decisions not in the spec; inconsistencies noticed
- What **P1-5** should cover next

Ask clarifying questions before starting if unclear.
If everything is clear, say so and begin.

=== END PROMPT ===
```

### P1-5 — Boundary-driven volcanism

*Source: transcript `f65ad57f`, assistant message ~line 65.*

```markdown
=== START PROMPT ===

Read these documents before starting, in order:

- `CONTRIBUTING-AI.md` — collaboration guide
- `docs/03-glossary-and-naming.md` — terminology
- `docs/06-tectonics-module-specification.md` v0.2 — Phase 1 tectonics
  - Read every section, but pay particular attention to:
    - §4.2 step 4–5 (boundary elevation, then hot spots — this prompt is §5.5 only)
    - §4.4 (`tectonics.volcanism` stream)
    - §5.5 (Boundary-driven volcanism)
    - §6.1–§6.3 (`VolcanicEruption`, significance, `event_granularity`)
    - §10 (Determinism)
- `docs/04-data-layer-specification.md` — `Event`, `EventLog`, `Significance`, `BranchTree`
- Existing code: `crates/genesis_tectonics/src/elevation.rs` (OC/OO subduction rules), `boundary.rs`, `layer.rs`, `history.rs`, `crates/genesis_core/src/events/`

If you cannot access any of these, stop and tell me.

## Your Task

Implement **P1-5: boundary-driven volcanism** (Doc 06 §5.5 + minimal event plumbing).

On each **Geological** tick, after `apply_boundary_elevation` (and its clamp), stochastically run volcanic eruptions at **subduction arc** boundary hexes (oceanic–continental and oceanic–oceanic convergent boundaries). Apply terrain changes **always**; record `VolcanicEruption` events in the root branch `EventLog` only when significance meets `event_granularity`.

Deliverables:

1. **`genesis_core`:** Add `EventKind::VolcanicEruption { hex, elevation_change_m, plate }` per Doc 06 §6.1 (keep `Placeholder`; do not add unrelated event variants).
2. **`volcanism.rs`:** Arc-hex detection, RNG rolls, elevation/relief/bedrock updates, event construction + granularity gating.
3. **Wire** into `TectonicsLayer` Geological tick; flush pending events into `world.branch_tree` from `generate_full_history_with_tectonics`.
4. **Tests** for determinism, state application, significance filtering, and arc-hex eligibility.

## Determinism (this slice)

- RNG stream: **`tectonics.volcanism`** only for eruption rolls and magnitude sampling (§4.4).
- Iterate candidate arc hexes in ascending **`HexId`** order; within a hex, process convergent edges in stable order (e.g. sorted `neighbor_hex`).
- **`EventId` allocation** must be deterministic (monotonic counter on `TectonicsState` reset per world, or derived from tick year + sequence — document choice).
- No `chrono` / wall-clock in simulation paths.
- Changing volcanism logic must not alter `tectonics.plate_seeds` or other streams (separate stream).

## What "Done" Looks Like

After this prompt:

- Default `volcanism_scale = 1.0` → per-tick eruption probability **`0.05`** per eligible arc boundary hex (§5.5).
- On eruption: `elevation_mean` += **100–500 m** (sampled), `elevation_relief` += **50–200 m**, bedrock → **`Igneous`**; then clamp via existing `clamp_terrain`.
- **Arc hexes only:** OC convergent → owner hex is **continental** side; OO convergent → owner hex is on **overriding** (non-subducting) plate — reuse `subducting_plate_id` from `elevation.rs` (same rules as P1-4).
- **Significance** (§6.2): after applying eruption, if peak proxy `elevation_mean + elevation_relief` **> 2000 m** → `Notable`, else → `Minor`.
- **`maybe_emit` behavior:** terrain always updated; `EventLog::push` only if `significance >= parameters.core.geology.event_granularity`.
- `generate_full_history_with_tectonics` leaves events on root branch; tests can read `world.branch_tree.get(BranchId::ROOT).event_log`.
- `cargo build --workspace` / `cargo test --workspace` / `cargo fmt --check` / `cargo clippy --workspace --all-targets` all pass.
- `cargo tree -p genesis_tectonics --depth 1` — no Bevy.
- Do not commit unless explicitly asked.

## Implementation Plan

### Part 1: `EventKind::VolcanicEruption` in genesis_core

**Files:** `crates/genesis_core/src/events/kinds.rs`, `events/mod.rs` tests, any `match` sites that must stay exhaustive (persistence tests, etc.)

**Changes:**
- Add variant with fields from Doc 06 §6.1 (`HexId`, `PlateId` from `genesis_core`).
- Update existing tests using `Placeholder` only; add serde round-trip for `VolcanicEruption`.

**Tests:**
- Serialize/deserialize `EventKind::VolcanicEruption`.

### Part 2: Event flush path (minimal plumbing)

**Files:** `crates/genesis_tectonics/src/plate.rs`, `history.rs`, optionally `genesis_core/src/branches/mod.rs` if you need `root_mut` helper

**Changes:**
- `TectonicsState`: `pending_events: Vec<Event>`, `next_event_id: u64` (or equivalent).
- `fn flush_events_to_branch(world: &mut World, state: &mut TectonicsState)` — push `pending_events` to `world.branch_tree.get_mut(BranchId::ROOT).event_log` with `Event { id, year: world.data.current_year, branch_id: ROOT, location: EventLocation::Hex(hex), significance, kind }`, then clear pending.
- Call flush at end of `generate_full_history_with_tectonics` (and optionally after each tick if tests need mid-history — prefer end-of-run + unit tests on `apply_boundary_volcanism` directly).

**Tests:**
- Flush pushes events retrievable from branch log.

### Part 3: `volcanism.rs`

**Files:** `crates/genesis_tectonics/src/volcanism.rs`, `lib.rs`

**Changes:**
- `pub const VOLCANISM_STREAM: &str = "tectonics.volcanism";`
- `pub fn apply_boundary_volcanism(data, registry, boundaries, rng, volcanism_scale, event_granularity, tick_year, branch_id, next_event_id) -> (Vec<Event>, u64)`  
  Or mutate `TectonicsState` in place and return events to queue.
- **Eligibility:** For each `hex` in `boundaries.boundary_hexes` (sorted), for each `ClassifiedEdge` with `BoundaryClass::Convergent(ContinentalOceanic | OceanicOceanic)`, include hex if it is arc side per P1-4 subduction rules.
- **Roll:** `rng.stream(VOLCANISM_STREAM)`; `if roll < 0.05 * volcanism_scale` then erupt.
- **Magnitudes:** uniform or doc-consistent sample: elev Δ ∈ [100, 500], relief Δ ∈ [50, 200] (meters).
- **Significance** from post-eruption peak proxy; build `EventKind::VolcanicEruption`.
- **Granularity:** push to returned/collected events only if `significance >= event_granularity`; still apply terrain if below threshold.
- Call `clamp_terrain(data)` after all eruptions this tick.

**Tests:**
- Arc eligibility: OC oceanic owner hex does not erupt; continental owner can (with forced RNG).
- `volcanism_scale = 0` → no terrain change, no events.
- Forced probability 1.0 (test helper or scale override) → elevation increases, bedrock `Igneous`.
- Determinism: same seed → same eruption hexes and magnitudes at fixed year.
- Granularity `Notable` filters `Minor` eruptions from log but still applies elevation (use low peak hex).

### Part 4: Wire `TectonicsLayer`

**Files:** `crates/genesis_tectonics/src/layer.rs`

**Changes:**
- Geological tick order:

  `motion` → `repartition` → `detect_and_classify_boundaries` → `apply_boundary_elevation` → **`apply_boundary_volcanism`** → (clamp already inside modules).

- Pass `world.current_year`, `BranchId::ROOT`, `rng`, `volcanism_scale`, `event_granularity` into volcanism step.
- Append emitted events to `TectonicsState.pending_events`.

**Tests:**
- Integration: history to 1M years with default params → at least one `VolcanicEruption` in log when `event_granularity` set to `Minor` or `Trace` for test (or use high `volcanism_scale` in test params only).

## What's NOT in Scope

- Hot spot volcanism (§7) — **P1-6**
- Erosion (§8) — **P1-7**
- `WorldFormation`, `MountainRangeFormed`, `OceanBasinOpened`, reorganization, sea level events — later prompts
- Changing `SimulationLayer::advance` to `Vec<Event>` globally (use `TectonicsState.pending_events` unless a small core change is clearly better — document choice)
- `genesis_render` elevation coloring — separate prompt
- Editing `/docs/` unless I explicitly ask
- New dependencies without asking

If you find yourself reaching for any of the above, stop and confirm.

## Process

1. Read Doc 06 §5.5 and §6.2–§6.3 fully. ~10% more of Doc 06 (~90% cumulative for core geology loop minus erosion/hotspots).
2. Implement Parts 1–4 in order; run tests after each part.
3. Full verification suite with **actual command output** in summary.
4. Follow `.cursor/rules/project.mdc` and `CONTRIBUTING-AI.md`.
5. Do not edit `/docs/` unless I explicitly ask.

## When You Finish

Provide a summary with:

- Files added/changed tree
- Build / test / fmt / clippy outputs
- New test counts
- Arc-hex rule restated (OC / OO)
- Eruption counts on default seed at 1M years (with test `event_granularity` noted)
- How `EventId` is assigned
- Decisions not in spec; inconsistencies noticed
- Sections for **P1-6** (hot spots) and optional **render-by-elevation** follow-up

Ask clarifying questions before starting if unclear.
If everything is clear, say so and begin.

=== END PROMPT ===
```

### P1-5.1 — Per-tick volcanism RNG

*Source: transcript `f65ad57f`, assistant message ~line 76.*

```markdown
=== START PROMPT ===

Read these documents before starting, in order:

- `CONTRIBUTING-AI.md` — collaboration guide
- `docs/06-tectonics-module-specification.md` v0.2 — §4.4 (RNG streams), §5.5 (volcanism)
- `crates/genesis_core/src/rng/mod.rs` — how `WorldRng::stream` works today
- `crates/genesis_tectonics/src/volcanism.rs` — `apply_boundary_volcanism`

If you cannot access any of these, stop and tell me.

## Your Task

**P1-5.1: Per-tick volcanism RNG** — fix a bug where every Geological tick replays the same `tectonics.volcanism` roll sequence because `WorldRng::stream(name)` always returns a fresh RNG with identical initial state.

Eruption rolls and magnitude samples must differ across ticks while remaining deterministic for the same `(seed, year, hex visit order)`.

**Minimal scope:** volcanism only. Do not change `plate_seeds`, `plate_axes`, `plate_rates`, or `initial_elevation_noise` behavior.

## What "Done" Looks Like

- Two Geological ticks at different years (e.g. 500_000 vs 1_000_000) with the same arc hex set can produce **different** eruption outcomes (not identical replay of tick 1’s rolls).
- Same seed + same year + same world state before volcanism → **identical** eruption results (determinism preserved).
- Existing P1-5 tests still pass; add at least one test proving cross-tick variation.
- `cargo build --workspace`, `cargo test --workspace`, `cargo fmt --check`, `cargo clippy --workspace --all-targets` all pass.
- Do not commit unless explicitly asked.

## Implementation Plan

### Part 1: Tick-scoped stream derivation (prefer `genesis_core`)

**Files:** `crates/genesis_core/src/rng/mod.rs`, `crates/genesis_core/src/lib.rs` (re-export if needed)

**Changes (pick one approach; document in summary):**

**Option A (preferred):** Add e.g. `WorldRng::stream_at(&self, name: &str, tick_key: u64) -> SmallRng` that hashes `effective_seed || name || tick_key`. Existing `stream(name)` unchanged.

**Option B:** In `volcanism.rs` only, derive a one-off seed: `xxhash(effective_seed, "tectonics.volcanism", tick_year)` without a new public API — only if you want zero core API change.

Use `tick_key = world.current_year.value() as u64` (or `tick_year` already passed into `apply_boundary_volcanism`).

**Tests (`genesis_core` or `genesis_tectonics`):**
- `stream_at` with same name + different `tick_key` → different first roll.
- Same name + same `tick_key` → identical first roll.

### Part 2: Wire volcanism to tick-scoped stream

**Files:** `crates/genesis_tectonics/src/volcanism.rs`

**Changes:**
- Replace `rng.stream(VOLCANISM_STREAM)` with tick-scoped variant using `tick_year` from `apply_boundary_volcanism` arguments.
- Keep stream **logical name** `tectonics.volcanism` in the hash input so Doc §4.4 naming stays meaningful.

**Tests:**
- `eruptions_differ_across_ticks_with_same_arc_fixture`: run volcanism twice on identical `WorldData` + `BoundaryInfo` at `WorldYear(500_000)` vs `WorldYear(1_000_000)` with high `volcanism_scale`; assert `pending_events` or elevations are not byte-identical (or first roll would have fired differently).
- Existing `eruptions_are_deterministic_for_fixed_seed` still passes when **year is held constant**.

### Part 3: Regression check

**Files:** none new

**Changes:**
- Run full `genesis_tectonics` + integration tests; confirm `history_records_volcanic_eruptions_with_trace_granularity` still passes.

## What's NOT in Scope

- Persisting RNG state across ticks on `TectonicsState` (unless Option A is insufficient — justify)
- Changing other streams or Formation one-shot streams
- Hot spots, erosion, events, render
- Editing `/docs/` unless I explicitly ask

## Process

1. Implement Part 1 → Part 2 → Part 3.
2. Show actual command outputs in summary.
3. Explain hash inputs for the chosen approach.

## When You Finish

- Files changed
- How tick enters the stream seed (formula)
- Test names added
- Confirm plate generation determinism unchanged (same-seed `plate_id` snapshot still matches P1-1 test or spot-check)
- Note any other per-tick consumers that will need `stream_at` later (erosion, reorganization)

Ask clarifying questions if unclear; otherwise begin.

=== END PROMPT ===
```

### P1-6 — Hot spots

*Source: transcript `f65ad57f`, assistant message ~line 85.*

```markdown
=== START PROMPT ===

Read these documents before starting, in order:

- `CONTRIBUTING-AI.md` — collaboration guide
- `docs/03-glossary-and-naming.md` — terminology
- `docs/06-tectonics-module-specification.md` v0.2 — Phase 1 tectonics
  - Read every section, but pay particular attention to:
    - §4.2 step 5 (Hot spots in per-tick order)
    - §4.4 (`tectonics.hotspot_locations`, `tectonics.hotspot_activity`)
    - §7 (Hot spot model, generation, dynamics, tracks)
    - §6.1–§6.3 (`HotSpotActivity`, significance table)
    - §10 (Determinism)
    - §13 (`hotspots.rs`)
- `docs/04-data-layer-specification.md` — `Event`, `EventLog`, `HexGrid`
- Existing code: `crates/genesis_tectonics/src/` (`layer.rs`, `volcanism.rs`, `events.rs`, `plate.rs`, `initial_terrain.rs`), `crates/genesis_core/src/rng/mod.rs` (`stream_at`), `crates/genesis_core/src/events/`

If you cannot access any of these, stop and tell me.

## Your Task

Implement **P1-6: hot spots** (Doc 06 §7 + §4.2 step 5).

**Formation:** Seed initial hot spots (count from planet radius, uniform sphere positions, activity rate and lifespan sampled).

**Each Geological tick** (after boundary volcanism, before erosion — erosion is out of scope): for each living hot spot, find the hex under its fixed anchor, roll for eruption, apply elevation/bedrock, optionally spawn rare new hot spots, remove expired hot spots.

**Events:** Add `EventKind::HotSpotActivity`; queue via existing `TectonicsState.pending_events` + `flush_events_to_branch` pattern from P1-5.

Use **`WorldRng::stream_at`** for per-tick hot spot RNG (`tectonics.hotspot_activity`). Use **`stream`** (no tick key) for Formation-time `tectonics.hotspot_locations`.

## Determinism (this slice)

- Formation hot spots: `rng.stream("tectonics.hotspot_locations")`; iterate/spawn in deterministic order (sorted `HotSpotId` or creation order).
- Per-tick: `rng.stream_at("tectonics.hotspot_activity", tick_year.value() as u64)`.
- Process active hot spots in ascending `HotSpotId` order each tick.
- New hot spot placement: deterministic from stream_at rolls at that tick.
- No system time, no file I/O during ticks.

## What "Done" Looks Like

After this prompt:

- `HotSpot`, `HotSpotId`, `HotSpotRegistry` (or equivalent) live in `genesis_tectonics` (and `HotSpotId` in `genesis_core` if required by `EventKind`).
- Formation creates ~12–20 hot spots for Earth-radius worlds per §7.2 formula.
- Geological tick order: motion → repartition → boundaries → boundary elevation → boundary volcanism → **hot spots** → clamp.
- Eruption: +100–1000 m `elevation_mean`, bedrock → `Igneous`, `clamp_terrain` after hot spot step.
- `activity_rate` ∈ [0.01, 0.1] per **Geological tick** (doc wording; document if you interpret as per-year).
- Lifespan: hot spot dies when `current_year - age_year > lifespan_duration` (sample 100M–1B years at birth).
- Rare spawn: probability `0.0001` per Geological tick (one roll per tick, or per doc intent — document).
- `HotSpotActivity` events: terrain always applied; log only if `significance >= event_granularity`.
- Significance per §6.2: track **cumulative uplift per hot spot**; `Notable` if cumulative > 1000 m, else `Trace` for that event.
- `cargo build --workspace` / `cargo test --workspace` / `cargo fmt --check` / `cargo clippy --workspace --all-targets` pass.
- No Bevy on `genesis_tectonics` / `genesis_core`.
- Do not commit unless explicitly asked.

## Implementation Plan

### Part 1: IDs and event variant (`genesis_core`)

**Files:** `crates/genesis_core/src/data/ids.rs` (or `events/kinds.rs`), `events/kinds.rs`, `events/mod.rs`, `lib.rs`

**Changes:**
- `HotSpotId(pub u16)` with `Serialize`/`Deserialize` if events need it.
- `EventKind::HotSpotActivity { hex: HexId, hot_spot_id: HotSpotId, elevation_change_m: f32 }`.
- Serde round-trip test.

**Tests:**
- `HotSpotActivity` serializes.

### Part 2: Hot spot types (`hotspots.rs`)

**Files:** `crates/genesis_tectonics/src/hotspots.rs`, `plate.rs`, `lib.rs`

**Changes:**

```rust
// Illustrative — match Doc §7; add fields as needed.
pub struct HotSpot {
    pub id: HotSpotId,
    pub anchor_position: [f64; 3],  // unit Vec3, fixed world frame
    pub activity_rate: f64,         // per Geological tick probability
    pub age_year: WorldYear,
    pub lifespan_years: i64,          // duration until death (not end year)
    pub cumulative_uplift_m: f32,     // for §6.2 significance (not in doc sketch; document)
}

pub struct HotSpotRegistry { /* BTreeMap<HotSpotId, HotSpot> */ }
```

- `TectonicsState.hotspots: HotSpotRegistry` (default empty).
- `fn generate_initial_hotspots(data: &WorldData, rng: &WorldRng) -> HotSpotRegistry`
  - `count = round(8.0 + 16.0 * (radius_km / 6371.0))`
  - Uniform sphere positions via `tectonics.hotspot_locations`
  - `activity_rate` ∈ [0.01, 0.1], `lifespan_years` ∈ [100_000_000, 1_000_000_000]

**Tests:**
- Earth-radius count in expected range (12–20).
- Positions are unit length; IDs unique.

### Part 3: Hex under anchor + per-tick dynamics

**Files:** `crates/genesis_tectonics/src/hotspots.rs`

**Changes:**
- `fn hex_at_anchor(grid, anchor: [f64;3]) -> HexId` — argmin angular distance over all hexes (`HexId` ascending tie-break).
- `fn apply_hotspot_tick(data, state, rng, tick_year, event_granularity, branch_id)`:
  1. Iterate hot spots by ascending `HotSpotId`.
  2. Remove if `tick_year - age_year > lifespan_years`.
  3. For survivors: find hex, roll `activity_rate` with `stream_at("tectonics.hotspot_activity", tick_year)`.
  4. On eruption: elev Δ ∈ [100, 1000] m, `Igneous`, update `cumulative_uplift_m`, assign significance, maybe_emit.
  5. After loop: with probability `0.0001`, spawn one new hot spot (position/rate/lifespan from same streams/rules; document stream split).
  6. `clamp_terrain(data)`.

**Tests:**
- `hex_at_anchor` tie-break lowest `HexId`.
- Forced `activity_rate = 1.0` raises elevation at underlying hex.
- Two ticks, same seed, same year → identical; different `tick_year` → can differ (stream_at).
- Expired hot spot removed and does not erupt.
- Cumulative significance: after enough eruptions, event significance becomes `Notable`.

### Part 4: Wire Formation and `TectonicsLayer`

**Files:** `layer.rs`, `history.rs`, `initial_generation` path if needed

**Changes:**
- Formation (after `apply_formation_terrain`): `state.hotspots = generate_initial_hotspots(world, rng)`.
- `run_formation` in `history.rs`: same.
- Geological block: after `apply_boundary_volcanism`, call `apply_hotspot_tick(...)`.

**Tests:**
- Integration: `run_formation` → `state.hotspots.count() > 0`.
- `generate_full_history_with_tectonics` to 1M years with `event_granularity = Trace` → at least one `HotSpotActivity` in root log (use high activity or test-only params if needed).

### Part 5: Re-exports and regression

**Files:** `lib.rs`

**Changes:**
- Export `HotSpot`, `HotSpotRegistry`, `generate_initial_hotspots`, `apply_hotspot_tick`, `hex_at_anchor` as appropriate.

**Tests:**
- All P1-1–P1-5.1 tests still pass.
- `determinism_same_seed_same_result` (plates) unchanged.

## What's NOT in Scope

- Erosion (§8) — **P1-7**
- Plate reorganization, sea level drift — later
- `WorldFormation` and other event variants — **P1-8**
- Hot spot tracks as explicit geometry — emergent only (§7.4)
- Rendering / elevation colors — separate prompt
- Editing `/docs/` unless I explicitly ask
- New dependencies without asking

If you find yourself reaching for any of the above, stop and confirm.

## Process

1. Read Doc 06 §7 fully (~10% spec coverage; ~95% cumulative for core geology minus erosion/reorg).
2. Implement Parts 1–5; test after each part.
3. Full verification with actual outputs in summary.
4. Follow `.cursor/rules/project.mdc` and `CONTRIBUTING-AI.md`.
5. Do not edit `/docs/` unless I explicitly ask.

## When You Finish

Provide a summary with:

- Files added/changed tree
- Build / test / fmt / clippy outputs
- Hot spot count on default Earth-analog at Formation
- Sample eruption / event counts at 1M years (note test `event_granularity`)
- How `lifespan_years` and `activity_rate` are interpreted (per tick vs per year)
- How new-hot-spot spawn RNG is separated from per-spot rolls
- Decisions not in spec; inconsistencies noticed
- Ready for **P1-7** (erosion) and note to use `stream_at` for `tectonics.erosion_noise`

Ask clarifying questions if unclear.
If everything is clear, say so and begin.

=== END PROMPT ===
```

### P1-7 — Erosion, sediment, fertility

*Source: transcript `f65ad57f`, assistant message ~line 97.*

```markdown
=== START PROMPT ===

Read these documents before starting, in order:

- `CONTRIBUTING-AI.md` — collaboration guide
- `docs/03-glossary-and-naming.md` — terminology
- `docs/06-tectonics-module-specification.md` v0.2 — Phase 1 tectonics
  - Read every section, but pay particular attention to:
    - §4.2 step 6 (Erosion after hot spots)
    - §4.4 (`tectonics.erosion_noise`)
    - §8.1–§8.4 (Erosion, sedimentary bedrock, fertility)
    - §10 (Determinism)
    - §13 (`erosion.rs`)
- `docs/04-data-layer-specification.md` — `WorldData` fields, `base_erosion_rate_per_year`
- Existing code: `crates/genesis_tectonics/src/layer.rs`, `elevation.rs` (`clamp_terrain`), `plate.rs` (`TectonicsState`), `crates/genesis_core/src/grid/mod.rs` (`center_lat_lon`)

If you cannot access any of these, stop and tell me.

## Your Task

Implement **P1-7: erosion, sediment routing, and shallow-sea fertility** (Doc 06 §8 + §4.2 step 6).

On each **Geological** tick, after hot spots:

1. **Erode** land hexes above sea level; route eroded mass to the **lowest-elevation neighbor** (tie-break lowest `HexId`).
2. Track **cumulative deposition** per hex (tectonics-layer state, not `WorldData`); when cumulative deposition exceeds threshold, set bedrock to **`Sedimentary`** on `Igneous` / `Metamorphic` hexes.
3. **Increment `fertility`** monotonically for hexes below sea level, tropical latitude, and shallow water depth.

Use `parameters.core.geology.base_erosion_rate_per_year` (default `1e-7`). Phase 1 **`climate_modifier = 1.0`** uniformly (climate arrays exist but are not active — do not zero out erosion because `precipitation` defaults to 0).

Use **`WorldRng::stream_at("tectonics.erosion_noise", tick_year)`** only if you add per-hex erosion variation per §4.4; tie-breaking for equal neighbors may use deterministic `HexId` order without RNG.

## Determinism (this slice)

- Iterate hexes in ascending **`HexId`** order for erosion and fertility passes.
- Neighbor selection: lowest neighbor elevation; tie → lowest **`HexId`**.
- `fertility`: only increase, never decrease; cap at `1.0`.
- `stream_at` for `tectonics.erosion_noise` if used — keyed by tick year.
- No system time, no file I/O during ticks.

## What "Done" Looks Like

After this prompt:

- Geological tick order ends with: … → hot spots → **`apply_erosion_and_fertility`** (or equivalent) → `clamp_terrain`.
- Land erosion: `erosion_amount = elevation_above_sea * base_erosion_rate * climate_modifier * tick_interval_years`; subtract from `elevation_mean` (and optionally reduce `elevation_relief` proportionally — document choice).
- Submerged hexes (`elevation_mean < sea_level_m`): no erosion; may receive deposited mass from neighbors.
- `TectonicsState` holds `cumulative_deposition_m` (e.g. `BTreeMap<HexId, f32>` or `Vec<f32>` indexed by hex).
- Deposition threshold **500 m** → `BedrockType::Sedimentary` on eligible source types.
- Fertility: `|latitude_deg| < 30`, depth `sea_level_m - elevation_mean < 200 m`, increment **0.001 per Geological tick** (doc §8.4), clamp `fertility` to `[0, 1]`.
- `cargo build --workspace` / `cargo test --workspace` / `cargo fmt --check` / `cargo clippy --workspace --all-targets` pass.
- All P1-1–P1-6 tests still pass.
- Do not commit unless explicitly asked.

## Implementation Plan

### Part 1: `erosion.rs` — core erosion pass

**Files:** `crates/genesis_tectonics/src/erosion.rs`, `lib.rs`

**Changes:**
- `pub fn climate_modifier_phase1(_data: &WorldData, _hex: HexId) -> f64` → `1.0` (stub for Phase 2 hook from §8.2).
- `pub fn apply_land_erosion(data, tick_interval_years, base_rate) -> BTreeMap<HexId, f64>`  
  Returns eroded **mass per source hex** (meters of material removed), only where `elevation_mean > sea_level_m`.
- Subtract erosion from `elevation_mean` (preserve non-negative elevation relative to reasonable floor or allow negative only below sea — document; prefer not to drive land far below sea level from erosion alone).

**Tests:**
- Mountain hex above sea erodes; submerged hex does not.
- Zero `base_rate` → no change.
- Same inputs → same erosion map (determinism).

### Part 2: Sediment routing and bedrock

**Files:** `erosion.rs`, `plate.rs` (`TectonicsState`)

**Changes:**
- Add `pub cumulative_deposition_m: Vec<f32>` (or map) on `TectonicsState`, sized at Formation / reset with world.
- `route_eroded_mass(data, state, eroded_per_hex)` — for each source hex in `HexId` order, add eroded mass to **lowest neighbor** (compare `elevation_mean`; tie lowest `HexId`; include pentagon 5-neighbor case).
- On deposit hex: add to `cumulative_deposition_m`; if cumulative > **500.0** and bedrock is `Igneous` or `Metamorphic`, set `Sedimentary`.

**Tests:**
- Erosion on high hex increases deposition on lower neighbor.
- Enough cumulative deposition flips bedrock to `Sedimentary`.
- Tie-break to lowest `HexId` when two neighbors share elevation.

### Part 3: Fertility increment (§8.4)

**Files:** `erosion.rs` or `fertility.rs`

**Changes:**
- `increment_shallow_tropical_fertility(data, tick_interval_years)` — per hex ascending `HexId`:
  - `elevation_mean < sea_level_m`
  - `|center_lat_lon(hex).0| < 30°` (convert rad → deg)
  - `depth_m = sea_level_m - elevation_mean < 200.0`
  - `fertility[hex] = (fertility[hex] + 0.001).min(1.0)` (per **tick**, not scaled by years unless doc interpretation says otherwise — document)
- **Do not** set `BedrockType::Limestone`.

**Tests:**
- Tropical submerged shallow hex gains fertility; land hex does not.
- High-latitude submerged hex does not (or minimal).
- Fertility never decreases when hex later rises above sea (simulate elevation increase — fertility unchanged).

### Part 4: Wire `TectonicsLayer` + optional noise

**Files:** `layer.rs`, `lib.rs`

**Changes:**
- After `apply_hotspot_tick`, call unified step e.g. `apply_erosion_tick(world, state, rng, interval_years)` that runs erosion → routing → fertility → `clamp_terrain`.
- Optional: multiply erosion by `(1.0 + small_noise)` from `stream_at("tectonics.erosion_noise", tick_year)` with noise ∈ [-0.05, 0.05] or similar — document if omitted.

**Tests:**
- Integration: history to 1M years — max land elevation decreases or relief changes vs run without erosion (compare disabled rate in test only, or assert mountains lower after many ticks with high rate).
- `geological_ticks_are_deterministic` still passes for elevation + fertility snapshots.

### Part 5: Initialize deposition buffer

**Files:** `history.rs`, `layer.rs` Formation path, `TectonicsState::default`

**Changes:**
- Size `cumulative_deposition_m` when world/grid is known (Formation or first Geological tick).
- Default `0.0` for all hexes.

## What's NOT in Scope

- Sea level drift (§4.6) — later with reorganization pass
- Plate reorganization (§4.5) — **P1-8**
- Phase 2 climate formula for `climate_modifier` (implement stub only)
- `BedrockType::Limestone` / biology-driven bedrock — Phase 4
- Hydrology drainage networks — Phase 2
- New `EventKind` variants for erosion — optional later
- Rendering — separate prompt
- Editing `/docs/` unless I explicitly ask
- New dependencies without asking

If you find yourself reaching for any of the above, stop and confirm.

## Process

1. Read Doc 06 §8 fully (~10% spec; ~98% cumulative for Phase 1 geology minus reorg/events/sea-level).
2. Implement Parts 1–5; test after each part.
3. Full verification with actual outputs in summary.
4. Follow `.cursor/rules/project.mdc` and `CONTRIBUTING-AI.md`.
5. Do not edit `/docs/` unless I explicitly ask.

## When You Finish

Provide a summary with:

- Files added/changed tree
- Build / test / fmt / clippy outputs
- Sample: mean land elevation before/after 1M years (default params)
- Max `fertility` and count of hexes with `fertility > 0` at 1M years
- Deposition / `Sedimentary` conversion counts if measurable
- Decisions: relief erosion yes/no, fertility per tick vs per year, noise usage
- Spec gaps noted
- Ready for **P1-8** (reorganization + remaining events)

Ask clarifying questions if unclear.
If everything is clear, say so and begin.

=== END PROMPT ===
```

### P1-8 — Reorganization, sea level, events

*Source: transcript `f65ad57f`, assistant message ~line 121.*

```markdown
=== START PROMPT ===

Read these documents before starting, in order:

- `CONTRIBUTING-AI.md` — collaboration guide
- `docs/03-glossary-and-naming.md` — terminology
- `docs/06-tectonics-module-specification.md` v0.2 — Phase 1 tectonics
  - Read every section, but pay particular attention to:
    - §4.2 steps 7–9 (reorganization, sea level, emit events)
    - §4.3 step 9 (`WorldFormation` event)
    - §4.4 (`tectonics.reorganization_check`, `tectonics.reorganization_action`)
    - §4.5 (Plate reorganization)
    - §4.6 (Sea level drift)
    - §6 (Event schema, significance, granularity)
    - §10 (Determinism — reorganization streams)
    - §12.1 (Extinct plates with zero hexes)
    - §13 (`reorganization.rs`, `events.rs`)
- `docs/04-data-layer-specification.md` v0.5 — `Event`, `Significance`, `sea_level_m`, event log rules (§8)
- Existing code: `crates/genesis_tectonics/src/` (especially `layer.rs`, `events.rs`, `boundary.rs`, `plate.rs`, `partition.rs`, `history.rs`)

If you cannot access any of these, stop and tell me.

## Your Task

Implement **P1-8: plate reorganization, sea level drift, and the remaining Phase 1 tectonic event kinds** (Doc 06 §4.5–§4.6, §6, tick steps 7–9).

After erosion on each Geological tick:

1. **Reorganization** — stochastic split / motion change / merge per §4.5, using separate RNG streams.
2. **Sea level** — update `WorldData.sea_level_m` from divergent-boundary activity plus equilibrium damping per §4.6.
3. **Event emission** — complete the tectonic `EventKind` taxonomy in `genesis_core`, centralize granularity gating, emit `WorldFormation` at Formation, and emit per-tick / per-action events where the spec defines them.

Refactor existing volcanism and hot-spot paths to use the shared emit helper (behavior unchanged; still apply terrain even when below `event_granularity`).

No rendering, no §11 full-history validation suite (that is **P1-9**).

## What "Done" Looks Like

After this prompt:

- Geological tick order matches §4.2: … → `apply_erosion_tick` → **reorganization** → **sea level** → (boundary-derived events as specified) → single final clamp if any step can change elevation without going through erosion’s clamp.
- `EventKind` in `genesis_core` includes: `WorldFormation`, `PlateReorganization`, `MountainRangeFormed`, `OceanBasinOpened`, `BoundaryTransition`, `SeaLevelChange` (plus existing `VolcanicEruption`, `HotSpotActivity`, `Placeholder`).
- `PlateReorgAction` (or equivalent) lives in `genesis_core` and serializes with `EventKind`.
- Formation emits one `WorldFormation` event (`Pivotal`, `EventLocation::Global`) — not only a `tracing::info!`.
- Reorganization probability: `0.001 * geology_activity_scale` per Geological tick (~once per 500M years at scale 1.0). Action mix: split 40%, motion change 40%, merge 20%.
- Sea level uses divergent-boundary length delta formula from §4.6 plus damping so it cannot run away over long histories; final `sea_level_m` stays plausible (§11.7: within ±200 m of 0 for default Earth-analog at 1M-year test worlds).
- `maybe_emit` (or equivalent) gates `pending_events` by `parameters.core.geology.event_granularity`; terrain/state updates always apply.
- `cargo build --workspace` succeeds
- `cargo test --workspace` passes — all existing tests plus new unit/integration tests in `genesis_tectonics` and `genesis_core` (serde round-trips for new `EventKind` variants)
- `cargo fmt --check` passes
- `cargo clippy --workspace --all-targets` is clean
- Do not commit unless explicitly asked

## Determinism (this slice)

- Per-tick RNG: `WorldRng::stream_at("tectonics.reorganization_check", tick_year)` and `stream_at("tectonics.reorganization_action", tick_year)` — same pattern as volcanism / erosion (Doc §10 item 6; note §4.4 table says `stream(name)` but codebase standard is `stream_at` for per-tick rolls).
- Plate / hex iteration: ascending `PlateId` / `HexId`; `BTreeMap` / `BTreeSet` only.
- After reorganization or merge: call `repartition_hexes` so `plate_id` stays consistent.
- No system time, no file I/O during ticks.

## Implementation Plan

Implement in this order; run tests after each part when practical.

### Part 1: `EventKind` + `PlateReorgAction` in `genesis_core`

**Files:** `crates/genesis_core/src/events/kinds.rs`, `events/mod.rs`, `lib.rs`; update any `match` exhaustiveness (branches tests, persistence tests).

**Changes:**
- Add variants per Doc 06 §6.1 (use `[f64; 3]` for axes to match `Plate::motion_axis`, not `glam` in core if that avoids a new dependency).
- Add `PlateReorgAction` enum: `Split { parent, child }`, `Merge { absorbed, into }`, `MotionChange { plate, new_axis, new_rate }`.
- For `BoundaryTransition`, map `from` / `to` to the existing `BoundaryClass` in `genesis_tectonics` **or** add a small serializable `BoundaryType` in core — pick one approach and document it; do not duplicate incompatible enums.
- Serde round-trip tests for each new variant.

**Tests:**
- `serde_json` round-trip per variant in `events/mod.rs`.

### Part 2: Centralized event emission (`events.rs`)

**Files:** `crates/genesis_tectonics/src/events.rs`, `plate.rs` (`next_event_id` already exists)

**Changes:**
- `pub fn maybe_emit(state: &mut TectonicsState, event: Event, granularity: Significance)` — push to `pending_events` only if `event.significance >= granularity`.
- `pub fn alloc_event_id(state: &mut TectonicsState) -> EventId` — monotonic counter (reuse pattern from volcanism/hotspots).
- Refactor `volcanism.rs` and `hotspots.rs` to call `maybe_emit` instead of duplicating push logic.

**Tests:**
- Granularity `Pivotal` skips `Trace` / `Minor` sea-level and small hot-spot events but still applies terrain (mirror existing volcanism granularity tests).

### Part 3: `WorldFormation` at Formation

**Files:** `layer.rs` Formation branch, `history.rs` `run_formation`, `initial_generation.rs` (remove or keep tracing as duplicate — event is canonical)

**Changes:**
- After plates + terrain + hot spots + deposition buffer init, emit `WorldFormation` with `Significance::Pivotal`, `EventLocation::Global`, year = formation year.
- Respect `event_granularity` (always passes for any non-absurd threshold).

**Tests:**
- Formation leaves exactly one `WorldFormation` in `pending_events` (or root log after flush in history test).

### Part 4: `reorganization.rs`

**Files:** `crates/genesis_tectonics/src/reorganization.rs`, `lib.rs`, `parameters/core.rs` + `validation.rs` + `mod.rs` defaults

**Changes:**
- Add `geology_activity_scale: f32` to `GeologyParameters` (default `1.0`, validated positive finite) — spec §4.5 references it but it is not in code yet.
- `pub fn maybe_reorganize(world, state, rng, tick_year, tick_interval_years) -> bool` (whether a reorg fired).
- **Check stream:** `roll < 0.001 * geology_activity_scale` (document if you scale by tick interval or treat as per-tick probability per spec literal).
- **Action stream** (only if check passes): choose split (40%) / motion change (40%) / merge (20%).
  - **Split:** pick a “large” plate (document threshold: e.g. hex count > 5% of cells or among top-N by area). Create child `PlateId`, related motion axis, update registry, `age_year = tick_year`, repartition. Continental splits: optional mild rift subsidence on boundary hexes between halves (heuristic OK; full 50M-year rift→ocean deferred).
  - **Motion change:** resample axis/rate (reuse constraints from initial motion sampling in `initial_generation` / `motion.rs`).
  - **Merge:** pick two plates sharing a boundary contact (from `state.boundaries.plate_contacts`); merge into lower `PlateId`, remove absorbed plate from registry, repartition.
- Emit `PlateReorganization` (`Pivotal`) via `maybe_emit`.
- **Extinct plates (§12.1):** after repartition, remove plates with zero hexes for `>= 10_000_000` years (track `last_nonempty_year` on `Plate` or in state). Never reuse `PlateId`.

**Tests:**
- Deterministic reorg for fixed seed + forced probability (test-only parameter override or inject max activity scale in test params).
- Split increases plate count; merge decreases; motion change preserves count.
- Repartition still covers all hexes after reorg.

### Part 5: Sea level (`sea_level.rs` or `reorganization.rs`)

**Files:** new module or section in `reorganization.rs`, `layer.rs`

**Changes:**
- `pub fn update_sea_level(data, boundaries, state, tick_year)`:
  - Compute `current_divergent_length_km` from classified divergent edges (sum edge lengths in km; document hex-edge length approximation using `planet.radius_km` and center-to-center angles).
  - Store `baseline_divergent_length_km` on first Geological update in `TectonicsState` (or Formation snapshot).
  - `delta_sea_level_m = (current - baseline) * 1e-6` per §4.6.
  - Add equilibrium term: e.g. `delta -= sea_level_m * k_eq` with small `k_eq` so level drifts toward 0 over long runs (document constant).
  - Optional: on `PlateReorganization`, add ±up to 100 m excursion (spec prose) via action stream — document magnitude.
  - Update `data.sea_level_m`.
- Emit `SeaLevelChange { delta_m, new_sea_level_m }` with significance `Notable` if `|delta_m| > 50`, else `Trace`; gate with `maybe_emit`.

**Tests:**
- More divergent boundaries → sea level rises vs baseline fixture.
- Damping prevents unbounded drift over many ticks in a synthetic test.

### Part 6: Boundary-derived events (§6.1 remaining)

**Files:** `events.rs` or `boundary_events.rs`, `plate.rs` (optional `previous_boundary_classes: BTreeMap<(HexId, HexId), BoundaryClass>`)

**Changes:**
- **`MountainRangeFormed` (`Major`):** after boundary elevation this tick, detect CC convergent boundary hexes where tick uplift ≥ threshold OR local peak `elevation_mean` ≥ 3000 m along a contiguous CC boundary run (document heuristic; keep deterministic scan order).
- **`OceanBasinOpened` (`Major`):** divergent boundary hexes where elevation dropped toward oceanic baseline this tick by ≥ threshold, or new `OceanicCrust` bedrock on divergent hex — document rule.
- **`BoundaryTransition` (`Trace`):** compare previous vs current class per directed edge `(hex, neighbor_hex)`; emit on change. Store previous snapshot in `TectonicsState` updated end of tick.
- All gated via `maybe_emit`.

**Tests:**
- Synthetic fixture with CC convergence produces `MountainRangeFormed` at `Major` granularity.
- Edge class flip produces `BoundaryTransition` at `Trace` granularity.
- Do not spam thousands of `BoundaryTransition` events per tick without a dedup rule — if needed, coalesce per hex per tick and document.

### Part 7: Wire `TectonicsLayer` tick order

**Files:** `layer.rs`

**Changes:**
- After `apply_erosion_tick`:
  1. `maybe_reorganize` (may repartition — consider whether boundaries must be recomputed; if yes, re-run `detect_and_classify_boundaries` before sea level, or compute sea level from pre-reorg boundaries — **document choice**).
  2. `update_sea_level`
  3. `emit_boundary_events` / snapshot update
- Ensure elevation changes from reorg (if any) get `clamp_terrain` once at end of tick.

**Tests:**
- Integration: `generate_full_history_with_tectonics` to 1M years — root event log contains `WorldFormation`; at `Trace` granularity, contains some `SeaLevelChange`; plate count in [5, 15] if any reorgs fired (loose assert or seed-fixed assert).
- Event log determinism: two runs → identical `pending_events` flushed to root log (extend existing determinism tests).

## What's NOT in Scope

- Doc 06 §11 full validation battery and `tectonics_full_history_completes_within_budget` (**P1-9**)
- Rendering elevation colors (**P1-10**)
- Climate-driven erosion modifier (**Phase 2**)
- Planetary cooling / formation sequence (**§17.7**)
- Chaos mode (**§17.8**)
- Editing `/docs/` unless I explicitly ask
- New dependencies without asking
- Changing tick interval table or repartition motion threshold (§9.2) unless required for correctness

If you find yourself reaching for any of the above, stop and confirm.

## Process

1. Read Doc 06 §4.5–§4.6 and §6 fully. This prompt implements ~15% of the spec by line count but completes the **simulation loop**; validation/render are follow-ups.
2. Implement Parts 1–7 in order; run tests after each part.
3. Full verification with **actual command output** in summary.
4. Follow `.cursor/rules/project.mdc` and `CONTRIBUTING-AI.md`.
5. Do not edit `/docs/` unless I explicitly ask.

## When You Finish

Provide a summary with:

- Directory tree of files added/changed
- Actual outputs from build / test / fmt / clippy
- Event counts at `Notable` vs `Trace` for 1M-year default seed (rough table)
- Plate count at start vs end of 1M-year run; reorganization event count
- Final `sea_level_m` at 1M years
- Decisions: reorg probability scaling, large-plate threshold, boundary-event heuristics, reorg vs boundary recompute order, `geology_activity_scale` default
- Spec gaps / doc drift flagged (`stream` vs `stream_at`, Doc 04 vs Doc 06 event shapes, `BoundaryType` naming)
- Ready for **P1-9** (§11 validation tests + perf budget)

Ask clarifying questions before starting if anything is unclear.
If everything is clear, say so and begin.

=== END PROMPT ===
```

### P1-9 — Validation and performance gates

*Source: transcript `f65ad57f`, assistant message ~line 142.*

```markdown
=== START PROMPT ===

Read these documents before starting, in order:

- `CONTRIBUTING-AI.md` — collaboration guide
- `docs/03-glossary-and-naming.md` — terminology
- `docs/06-tectonics-module-specification.md` v0.2 — Phase 1 tectonics
  - Read every section, but pay particular attention to:
    - §9 (Performance targets and measurement)
    - §10 (Determinism — full `WorldData` snapshot)
    - §11 (Validation criteria — primary deliverable)
    - §12.5 (`event_granularity` above Pivotal)
    - §15 step 9 (Integration and validation)
    - §16 (Agent notes — flag assumptions)
- `docs/04-data-layer-specification.md` v0.5 — `WorldData`, `EventLog`, determinism (§6)
- Existing code: `crates/genesis_tectonics/src/lib.rs` (integration tests), `history.rs`, `layer.rs`

If you cannot access any of these, stop and tell me.

## Your Task

Implement **P1-9: integration validation and performance gates** for Phase 1 tectonics.

This prompt adds **no new geology mechanics**. It codifies Doc 06 §11 sanity checks, strengthens determinism coverage (§10), adds a performance budget test (§9.1 / §9.3), and handles the §12.5 granularity edge case. The goal is **quantitative proof the simulation works** without elevation rendering (that is **P1-10**).

Deliverables:

1. A `validation` module with reusable metric functions and connected-component helpers.
2. Integration tests that run `generate_full_history_with_tectonics` on a **fixed validation seed** and assert §11 criteria (with documented loose bounds).
3. A determinism test comparing full `WorldData` (or a canonical hash) after a long run.
4. `tectonics_full_history_completes_within_budget` per §9.3 (CI-friendly configuration documented).
5. Optional: per-step `tracing::debug!` timings in `layer.rs` if not already sufficient for §9.3.

## Determinism (this slice)

- All validation tests use **`WorldSeed::from_integer(VALIDATION_SEED)`** (pick a constant, e.g. `42`, document it in the module).
- Use the same `WorldParameters` defaults as production except subdivision level and target year where noted.
- Connected-component and metric scans iterate hexes in ascending **`HexId`** order.
- No system time in assertions; perf test may use `std::time::Instant` **only** for measurement, not simulation state.

## What "Done" Looks Like

After this prompt:

- `crates/genesis_tectonics/src/validation.rs` exists with public or `pub(crate)` helpers used by tests.
- **§11 tests** exist (one test per criterion or one grouped `validation_criteria_met` with clear failures). Each documents the target year and subdivision level used.
- **Determinism:** same seed → identical `WorldData` fields that tectonics owns (at minimum: `elevation_mean`, `elevation_relief`, `bedrock_type`, `plate_id`, `fertility`, `sea_level_m`) after validation run.
- **§12.5:** `event_granularity = Pivotal` → root event log empty (or only Pivotal if any), world still generates without panic.
- **Perf:** `tectonics_full_history_completes_within_budget` passes in default `cargo test`; use subdivision **5** and a documented year cap (e.g. 10M–50M years) and time budget (e.g. 30s) unless you justify stricter Doc 06 §9.1 targets with `#[ignore]` for local runs.
- Existing P1-1–P1-8 tests still pass.
- `cargo build --workspace` / `cargo test --workspace` / `cargo fmt --check` / `cargo clippy --workspace --all-targets` pass.
- Do not commit unless explicitly asked.

### §11 criteria to implement (Doc 06 §11)

Use **loose tolerances** on a fixed seed. Flag in summary if a criterion cannot pass at CI-friendly history length.

| # | Criterion | Implementation notes |
|---|-----------|----------------------|
| 1 | Continental fraction 25–35% land | `elevation_mean > sea_level_m` / cell_count |
| 2 | Plate count 5–15 | `state.registry.count()` after run |
| 3 | ≥3 contiguous land regions with elevation > 3000 m | BFS/DFS on hex neighbors above threshold |
| 4 | ≥1 ocean region elevation < −3000 m, area > 1000 hexes | BFS below threshold; **scale min area** if grid at subdiv 5 has fewer than 1000 hexes total — use `min(1000, cell_count / 4)` or similar and document |
| 5 | Bedrock diversity | **Phase 1 reality:** `Limestone` is never assigned yet (§8.4). Assert all **five** tectonic types: `Igneous`, `Sedimentary`, `Metamorphic`, `OceanicCrust`, and at least one of `Unknown` cleared (or all five non-Unknown). Flag spec §11.5 vs Phase 1 in summary. |
| 6 | Elevation bounds | min > −11000, max < 9000 (already clamped; assert anyway) |
| 7 | Sea level ±200 m | `sea_level_m.abs() <= 200` |
| 8 | Event count 500–3000 at `Notable` | Requires **long** history (~hundreds of M years or more). Use a **separate** test with higher `target_year` and/or `#[ignore]` for CI; quick suite may only assert `> 0` events at `Notable` over 1M years |

**History length strategy (required decision in summary):**

- **CI quick suite:** `subdivision_level = 5`, `target_year = 1_000_000` (or 10M) — keeps existing integration fast; some §11 checks may be relaxed or skipped with `#[ignore = "needs long history"]`.
- **Validation suite:** `target_year` ≥ `100_000_000` (200+ Geological ticks) or `500_000_000` for mountain/reorg/event-count checks; still subdiv 5 unless perf allows 6–7.

Do not run 4.5B years in default CI without `#[ignore]` unless it completes in <30s on your machine and you document it.

## Implementation Plan

### Part 1: `validation.rs` — metrics and region detection

**Files:** `crates/genesis_tectonics/src/validation.rs`, `lib.rs`

**Changes:**
- Constants: `VALIDATION_SEED`, `VALIDATION_SUBDIVISION_LEVEL` (5 for CI), `VALIDATION_TARGET_YEAR_QUICK`, `VALIDATION_TARGET_YEAR_FULL`.
- `fn validation_parameters() -> WorldParameters` — fixed seed, subdiv, default geology.
- `continental_fraction(data) -> f32`
- `count_connected_regions(grid, data, predicate) -> Vec<usize>` — region sizes in hex count, deterministic BFS order.
- `bedrock_types_present(data) -> BTreeSet<BedrockType>`
- `elevation_bounds(data) -> (f32, f32)`
- `event_count_at_granularity(world, min_significance) -> usize`

**Tests:**
- Unit tests on tiny synthetic `WorldData` fixtures for BFS and fraction math.

### Part 2: §11 integration tests

**Files:** `crates/genesis_tectonics/src/lib.rs` or `validation.rs` `#[cfg(test)]`

**Changes:**
- `fn run_validation_world(target_year) -> (World, TectonicsState)` helper.
- `validation_quick_suite_passes` — 1M years, criteria 1, 2, 6, 7 + determinism subset.
- `validation_full_suite_passes` — long target, criteria 3, 4, 5, 8; mark `#[ignore]` if >10s.
- Each failure message prints actual vs expected (fraction, counts, region sizes).

**Tests:**
- Self-explanatory assert messages for debugging without visuals.

### Part 3: Full `WorldData` determinism (§10)

**Files:** `lib.rs` integration tests

**Changes:**
- Extend or add `world_data_identical_after_validation_run` — two worlds, same params/seed/year, compare all tectonics-relevant `WorldData` vectors and `sea_level_m`.
- Optional: canonical hash helper (serde or manual float bits) if equality is too strict; prefer exact `assert_eq!` on vectors if deterministic.

**Tests:**
- Must pass on quick target year at subdiv 5.

### Part 4: `event_granularity` edge case (§12.5)

**Files:** `lib.rs` test

**Changes:**
- Run formation + short geological history with `event_granularity = Significance::Pivotal`.
- Assert root `event_log` is empty **or** contains only `WorldFormation` / `PlateReorganization` (Pivotal kinds).
- Assert `elevation_mean` still changes vs formation-only control (simulation runs).

### Part 5: Performance budget (§9.3)

**Files:** `lib.rs` or `validation.rs`

**Changes:**
- `#[test] fn tectonics_full_history_completes_within_budget()`
- `Instant::now()` around `generate_full_history_with_tectonics` with validation params, subdiv 5, documented year cap.
- Assert `elapsed < BUDGET_SECS` (e.g. 30).
- Log elapsed at `eprintln!` or `tracing` for human diagnosis.
- Optional `#[ignore]` sibling at subdiv 7 for local profiling.

**Tests:**
- Budget test passes in `cargo test -p genesis_tectonics`.

### Part 6: Optional per-step timing (§9.3)

**Files:** `layer.rs`

**Changes:**
- If not already present: `tracing::debug!` with step labels and `Instant` deltas for motion, partition, boundaries, elevation, volcanism, hotspots, erosion, reorg, sea level, boundary events, clamp.
- Only when `tracing` enabled; no allocation hot path in release.

**Tests:**
- None required; manual `RUST_LOG=genesis_tectonics=debug` note in summary.

### Part 7: Summary logging helper (optional, no render)

**Files:** `validation.rs`

**Changes:**
- `fn summarize_world(data, state) -> String` — land %, min/max elev, sea level, plate count, bedrock histogram, event count.
- Call from one integration test or expose for `genesis_app` `info!` later (do not wire app unless trivial).

## What's NOT in Scope

- **P1-10:** `genesis_render` elevation coloring
- New geology mechanics, parameters (unless required for validation seed)
- Full 4.5B-year CI run without `#[ignore]`
- Editing `/docs/` unless I explicitly ask
- Snapshot files on disk (in-memory assert only unless I ask for golden files)
- Limestone bedrock assignment (Phase 4); document §11.5 gap instead

If you find yourself reaching for any of the above, stop and confirm.

## Process

1. Read Doc 06 §9–§11 and existing `lib.rs` integration tests (~15% new code, ~100% Phase 1 loop coverage).
2. Implement Parts 1–5 minimum; Part 6–7 optional.
3. Run full verification; show **actual** test output counts and one `summarize_world` example line for quick + full runs.
4. Follow `.cursor/rules/project.mdc` and `CONTRIBUTING-AI.md`.
5. Do not edit `/docs/` unless I explicitly ask.

## When You Finish

Provide a summary with:

- Files added/changed tree
- Table: each §11 criterion → test name → target year → pass/fail
- Determinism and perf test results (actual timings)
- Event counts at `Notable` for quick vs full target years
- Decisions: history lengths, ocean region min size scaling, Limestone handling
- Whether CI default `cargo test --workspace` stays under acceptable time
- Ready for **P1-10** (elevation render)

Ask clarifying questions before starting if anything is unclear.
If everything is clear, say so and begin.

=== END PROMPT ===
```

### P1-10 — Elevation-based hex coloring

*Source: transcript `f65ad57f`, assistant message ~line 159.*

```markdown
=== START PROMPT ===

Read these documents before starting, in order:

- `CONTRIBUTING-AI.md` — collaboration guide
- `docs/03-glossary-and-naming.md` — terminology
- `docs/06-tectonics-module-specification.md` v0.2 — Phase 1 tectonics
  - Read every section, but pay particular attention to:
    - §15 step 10 (Rendering integration — color hexes by elevation)
    - §5.7 (Elevation clamps: −11_000 m .. 9_000 m)
- `docs/02-architecture-overview.md` — rendering is downstream of simulation (reads `WorldData`, never writes)
- Existing code:
  - `crates/genesis_render/src/color.rs` (Phase 0 `hex_color` by `HexId`)
  - `crates/genesis_render/src/systems.rs` (`render_world_if_dirty`)
  - `crates/genesis_app/src/main.rs` (runs tectonics, inserts `WorldResource`)

If you cannot access any of these, stop and tell me.

## Your Task

Implement **P1-10: elevation-based hex coloring** in `genesis_render` (Doc 06 §15 step 10).

Replace the Phase 0 rainbow (`hex_color(hex, …)`) with colors derived from **`WorldData.elevation_mean`** and **`WorldData.sea_level_m`**, so running `genesis_app` after tectonics shows oceans, continents, and mountains visually.

This is a **read-only visualization** change: no simulation logic, no new `WorldData` fields, no edits to `genesis_tectonics` behavior.

## Determinism (this slice)

- `elevation_color(elevation_m, sea_level_m, …)` must be a **pure function** of its inputs (no `Instant`, no RNG, no `HexId` hue).
- Same elevation + sea level → same `Color` (test with fixed floats).
- Pentagons may use the same elevation ramp or a fixed accent; document choice.

## What "Done" Looks Like

After this prompt:

- `render_world_if_dirty` colors each hex from `world.data.elevation_mean[hex]` and `world.data.sea_level_m` (not from `HexId`).
- Land vs water is visually obvious at default smoke-test worlds (subdiv 6, 1M years).
- High elevations read lighter/whiter than lowlands; deep ocean reads darker than shallow shelf.
- `genesis_app` window title reflects Phase 1 geology smoke test (not “Phase 0” only).
- Optional: `info!` one line from `genesis_tectonics::summarize_world` after generation (app already depends on tectonics).
- Unit tests in `genesis_render` for the color mapping (no full Bevy app required for core tests).
- `cargo build --workspace` / `cargo test --workspace` / `cargo fmt --check` / `cargo clippy --workspace --all-targets` pass.
- Do not commit unless explicitly asked.

## Implementation Plan

### Part 1: Elevation color ramp (`color.rs`)

**Files:** `crates/genesis_render/src/color.rs`, `lib.rs` exports if needed

**Changes:**
- Add `pub fn elevation_color(elevation_m: f32, sea_level_m: f32) -> Color` (and keep or deprecate `hex_color` — prefer single public API used by render).
- Recommended ramp (document constants at top of module):
  - **Below sea level:** blue scale from deep (−11_000 m) to shallow (near `sea_level_m`).
  - **At/above sea level:** green/brown lowlands → tan highlands → snow white near 9_000 m.
  - Use linear interpolation between 3–5 stops; clamp input to [`MIN_ELEVATION_M`, `MAX_ELEVATION_M`] matching tectonics (`-11_000`, `9_000`) or import from a single shared constant if you add a tiny helper in `genesis_core` — **prefer local constants in render** to avoid new cross-crate coupling unless already shared.
- `hex_color` for pentagons: either call `elevation_color` for that hex’s elevation, or keep red outline + elevation fill — **pentagons must remain distinguishable** in tests.

**Tests:**
- Deep ocean bluer/darker than shallow submerged hex.
- Land greener/brighter than ocean at same world.
- Peak (8000 m) lighter than sea-level land.
- Determinism: same inputs → identical `Color` (compare sRGBA components with epsilon).
- Pentagon test updated for new behavior.

### Part 2: Wire rendering (`systems.rs`)

**Files:** `crates/genesis_render/src/systems.rs`

**Changes:**
- In `render_world_if_dirty`, replace:
  ```rust
  hex_color(hex, is_pentagon)
  ```
  with elevation-based color:
  ```rust
  let idx = hex.0 as usize;
  let elev = world_res.0.data.elevation_mean[idx];
  let sea = world_res.0.data.sea_level_m;
  elevation_color(elev, sea) // + pentagon handling if separate
  ```
- Update module comment from “Phase 0 smoke test” to elevation visualization.
- Do **not** change projection, mesh generation, or camera unless required for visibility.

**Tests:**
- Existing `genesis_render` / plugin smoke tests still pass.

### Part 3: App polish (`genesis_app`)

**Files:** `crates/genesis_app/src/main.rs`

**Changes:**
- Window title: e.g. `"Genesis Engine — Geology Smoke Test"` (elevation-colored map).
- After `generate_full_history_with_tectonics`, optional:
  ```rust
  info!("{}", genesis_tectonics::summarize_world(&world, &tectonics));
  ```
  (requires public `summarize_world` — already exported from `genesis_tectonics`).

**Tests:**
- Existing `app_plugins_build_without_panicking` still passes.

### Part 4: Manual verification note (for your summary)

**Not automated in CI:** run `cargo run -p genesis_app` and confirm visually:
- Blue ocean basins, green/brown continents, white peaks where validation reports mountain regions.
- No regression: hex mesh still renders, pan/zoom still work.

## What's NOT in Scope

- Plate ID colors, bedrock, fertility, or biome overlays
- Legends, color bars, UI widgets (`genesis_ui`)
- Globe / orthographic projections (Doc 14 territory)
- Writing to `WorldData` from render
- Fixing `handle_regenerate` wall-clock seed (pre-existing; separate task)
- Editing `/docs/` unless I explicitly ask
- New dependencies beyond workspace `bevy` / `genesis_core`
- Longer history in app (keep 1M years unless you need more contrast — document if you change)

If you find yourself reaching for any of the above, stop and confirm.

## Process

1. Read render path end-to-end (~5% new code; completes Doc 06 Phase 1 implementation list).
2. Implement Parts 1–3; run tests after each part.
3. Full verification with actual command output; include one sentence on manual visual check.
4. Follow `.cursor/rules/project.mdc` and `CONTRIBUTING-AI.md`.
5. Do not edit `/docs/` unless I explicitly ask.

## When You Finish

Provide a summary with:

- Files changed tree
- Color ramp description (stops in meters relative to `sea_level_m`)
- Build / test / fmt / clippy outputs
- Sample: min/max elevation and sea level for default app world
- Screenshot description or what the user should see when running the app
- Decisions: pentagon styling, dynamic vs fixed ramp bounds
- **Phase 1 tectonics complete** — suggested follow-ups: README update, Phase 2 climate spec

Ask clarifying questions before starting if anything is unclear.
If everything is clear, say so and begin.

=== END PROMPT ===
```


---

*End of Phase 1 implementation review. Maintained as a historical record alongside Doc 06 v0.2.*
