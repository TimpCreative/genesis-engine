# 10 — Civilization Module Specification

**Document Type:** Tier 2 — System Specification
**Status:** Planned — **stub, not yet drafted**
**Last Updated:** July 2026
**Owner:** Brax Johnson
**Implementing Phase:** 5 (Civilization)

**Changelog:**
- v0.1 (July 2026): Stub created to reserve the document number (Doc 09 already references "Doc 10" as Civilization ~15 times) and to record design decisions made before drafting begins — following the placeholder pattern in Doc 05 §"Rule for Future Docs".

---

## Status

This is a **placeholder**. The full Civilization specification is not written yet. It exists so that:

1. Cross-references from Doc 09 (Biology) — which names this document as the owner of domestication, sapient behavior, agriculture, food/carrying-capacity, and the sapience handoff — resolve to a real file.
2. Decisions made *before* drafting (below) are not lost.

When the real spec is drafted, expand this file in place.

---

## 1. Handoffs already committed by Doc 09

Doc 09 (Biology) hands the following to this module; the full spec must honor these contracts:

- **Sapience emergence** (Doc 09 §10.3) — biology emits `SapienceEmerged { lineage, province }` at an emergent (not fixed) year and hands off a sapient lineage; this module stamps the realized year and takes over sapient behavior. Multiple independent sapient lineages are possible.
- **Domestication / pinning** (Doc 09 §10.2) — the stateful exception: a domesticated or pinned species is promoted to a tracked entity in `WorldData`, allowed to diverge from its pure-seed identity. Selective-breeding-for-traits is a Doc 10 mechanism built on this hook.
- **Food / carrying capacity** — marine productivity and nekton biomass (fisheries), terrestrial primary productivity, and domesticable species are read as the food base.
- **The `humanoid_sapients` toggle** (Doc 09 §10.3) constrains sapient morphology upstream; downstream behavior/culture is this module's concern.
- **Naming** of species, taxa, and kingdoms is keyed on `LineageId`/`SpeciesId` and owned here or in Export (Doc 09 §1.6, Doc 15).
- **Plague/disease** as a civilization event is a tracked future refinement (Doc 09 §19.4).

---

## 2. Recorded Design Notes (pre-draft)

### 2.1 Time resolution after agriculture

Recorded July 2026. The tick-cadence framework lives in **Doc 05 §B.1** (the
milestone ratchet); this is the civilization-specific rung.

- Once **agriculture** (and later tech milestones) are reached, the Recent-era
  1k-year tick is **too coarse**. Historical change — nations, wars, technology —
  is fast on that scale, and users must be able to slow all the way down to
  **1-year ticks** to watch it unfold.
- Civilization therefore **extends the milestone ratchet below Recent**: each of
  its own milestones (agriculture, writing, industrialization, …) can ratchet the
  tick floor finer, the same way life/sapience milestones do for biology.
- **The load-bearing constraint is per-tick *cost*, not tick frequency.** Biology
  keeps deep-time affordable by simulating a bounded set of guilds per province
  and generating species lazily (Doc 09 §5.1, §8.2). **Civilization must adopt an
  analogous bounded-simulation / lazy-detail seam** — simulate a bounded state
  (nations, not individuals; aggregate flows, not every person) and generate the
  fine narrative/detail on demand. Without this, 1-year ticks over a busy world
  with dozens of nations are unaffordable, and "one year at a time" fails on
  contact with a real world.
- **Decision to make when drafting:** pin the civ simulation *granularity* (what
  the atomic simulated unit is, and its per-tick cost) *before* committing to the
  achievable tick floor. The floor is a function of that granularity.

### 2.2 The cadence seam (implemented July 2026)

Ahead of the full module, the **tick-cadence seam** is in place so the multi-rate
coordinator is proven for intelligent species: `genesis_civilization::CivilizationLayer`
(a `SimulationLayer`) is registered last in both coordinators (one-shot
`generate_full_history` and live `run_live_simulation`). It is dormant until
`sapience_emergence_year`, then ticks at `CIV_ANCIENT_TICK_YEARS = 1_000`
(Ancient) and `CIV_RECENT_TICK_YEARS = 25` (Recent) — decade-to-millennium
resolution the geological layers never reach, while tectonics sits dormant. These
are **placeholders**; the milestone ratchet (§2.1) will drive them below Recent
(toward 1-year ticks) once agriculture/tech milestones and the bounded-simulation
seam exist.

The layer's `advance` is **inert** — it records the tick and touches neither the
RNG nor `WorldData`, so wiring it in cannot change the physical/biology outcome
(verified: life still emerges at 265 My; `advance_does_not_mutate_world_or_rng`).
`CivilizationState` currently holds only observability counters. Real dynamics
replace the inert `advance` and grow `CivilizationState` — the registration,
attach/detach sharing (for a live-stepping worker to read civ state), and
era-gating are already correct.

---

## 3. To Be Specified

(Non-exhaustive — captured so the eventual draft has a starting outline.)

- Sapient societies: population, settlement, subsistence → agriculture transition.
- The tech tree and its coupling to the Doc 11 rule engine (shared with biology traits).
- Nations, territory, diplomacy, war; trade and resource flows.
- Domestication and selective breeding (built on the Doc 09 §10.2 hook).
- Culture, religion, language (naming).
- Bounded-simulation / lazy-detail seam (§2.1) — the architectural prerequisite for fine ticks.
- Civilization events and interventions.
- Determinism, performance, modding, validation (per the Tier 2 spec template).

---

*Stub — expand in place when Phase 5 drafting begins.*
