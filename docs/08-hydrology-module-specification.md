# 08 — Hydrology Module Specification

**Document Type:** Tier 2 — System Specification
**Status:** Stub / Draft v0.1
**Last Updated:** July 2026
**Owner:** Brax Johnson
**Implementing Phase:** 2 (Climate & Hydrology)

**Changelog:**
- v0.1 (July 2026): Initial stub. Records the *provisional* surface-flow model shipped in the Phase 2/3 prototype (`genesis_hydrology`) and the open requirements the full hydrology pass must satisfy. Not yet a complete spec.

---

## 1. Purpose and Scope

Hydrology turns the climate layer's per-hex precipitation and the tectonic elevation field into **surface water**: drainage directions, river discharge, lakes, and (later) groundwater, wetlands, and coastal deltas. It sits above tectonics (Phase 1) and climate (Phase 2) and feeds biology (Phase 4) — rivers and lakes are where fertility, settlement, and trade concentrate.

Everything in `genesis_hydrology` today is **provisional**: enough to render believable rivers and lakes in the prototype, not the final hydrological model. This document exists to (a) record what the provisional model does so it is not mistaken for the spec, and (b) capture the requirements the real pass must meet.

## 2. Provisional model (as implemented)

`crates/genesis_hydrology/src/flow.rs`, run each tick after tectonics and climate:

1. **Depression-filled routing surface.** Real hex-grid elevation has a local pit at a large fraction of hexes (grid quantization plus micro-relief), so naive steepest-descent routing dead-ends nearly everywhere and produces a lake at every pit. Surface water is therefore routed over a **priority-flood depression-filled copy** of the land elevation (Barnes 2014, "+epsilon" variant): pits are raised to their spill level so every non-basin land hex has a monotone downhill path to the sea. The fill is a scratch buffer — the tectonic `elevation_mean` field is never mutated. Deterministic: a `(level, HexId)`-keyed min-heap and fixed neighbor order, no RNG.
2. **Genuine-basin lakes.** A connected depression is kept as an **endorheic sink** (a real lake) only if it is both large (`BASIN_MIN_HEXES`) and deep (`BASIN_MIN_DEPTH_M`); small/shallow pits are filled and routed through. The basin bottom becomes the sink where accumulated discharge pools.
3. **Flow directions and accumulation.** Steepest descent over the filled surface (ties → lowest `HexId`); runoff = precipitation × hex area × `RUNOFF_COEFFICIENT`, accumulated upstream-to-downstream in filled-elevation order. Discharge crossing into ocean leaves the land system.
4. **Rendering (`genesis_render/src/rivers.rs`).** A thin-line polyline overlay draws a river segment along each downstream step whose discharge exceeds `RIVER_SOURCE_FLOW_MULTIPLE × mean local runoff`, and a small disc at each retained lake bottom. It is elevation-palette based and rebuilds only when the displayed frame's year changes.

### 2.1 Known limitations of the provisional model

- No lake **spill / overflow / merging**: a retained basin pools but does not yet overflow into a downstream channel once full, nor merge adjacent basins into one water body.
- No **groundwater, infiltration, evaporation balance, or seasonal variation** — a single annual-mean runoff coefficient.
- No **river-driven erosion / incision or delta deposition** feeding back into elevation (channels are drawn, not carved).
- Fixed, view-independent river threshold (see §3).

## 3. Open requirements for the full pass

1. **View-distance-dependent river LOD.** River/stream visibility must scale with zoom, not a single global threshold. At the whole-planet view only major trunk rivers should render; as the camera zooms in, progressively smaller tributaries appear, down to every creek at single-hex view. The current `RIVER_SOURCE_FLOW_MULTIPLE` is tuned for the world view only and is a placeholder for this LOD system.
2. **Lake spill and basin hydrology.** Retained basins should fill to a water level, overflow at the spill point into a downstream river, and merge when adjacent — proper endorheic-vs-exorheic lake behavior.
3. **Hydrological honesty (user constraint).** Rivers must flow downstream and never originate from nothing; water that cannot continue downhill pools as a lake. The provisional model already respects this and the full pass must preserve it.
4. **Erosion / deposition coupling.** River incision and sediment deposition (deltas, floodplains, alluvial fans) should couple back into the tectonic/soil layers.
5. **Determinism** — same seed → byte-identical hydrology, as elsewhere in the engine.

## 4. Non-Goals (this phase)

Full physical hydrodynamics, real-time flood simulation, and sub-annual weather-driven flow are out of scope. Hydrology targets geologically-plausible steady-state drainage per tick, not storm-by-storm routing.
