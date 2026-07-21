# 05 — Tick System & Time Management Specification

**Document Type:** Tier 2 — System Specification (absorbed)
**Status:** Absorbed v1.2
**Last Updated:** July 2026
**Owner:** Brax Johnson
**Implementing Phase:** 0 (Foundation, complete)

## Status: Absorbed into Other Documents

This document was originally planned as a standalone Tier 2 specification covering the simulation tick system. During Phase 0 implementation, it became clear that the tick system was small enough to specify alongside the data layer, and that several items originally scoped here naturally belonged with other documents.

This doc now exists as an **index pointer** so future readers don't go looking for content that was moved.

## Where to Find What

| Original Scope | Now Lives In |
|---|---|
| `WorldYear` / `Era` / `WorldTime` types | Doc 04 §7.1, §7.2 |
| `SimulationLayer` trait | Doc 04 §7.3 |
| `TickCoordinator` design and ordering rules | Doc 04 §7.3 |
| Tick interval scheduling | Doc 04 §7.3, plus per-module specs (Doc 06 onward) |
| Layer registration | Doc 04 §7.3, plus `genesis_core::lifecycle` (Doc 04 §11) |
| Main lifecycle loop (`create_world`, `generate_full_history`) | Doc 04 §11.1, §11.2 |
| Era boundary derivation from parameters | Doc 04 §7.2 |
| Branch divergence mechanics | Doc 04 §9 |
| Snapshot intervals and buffer management | **In-memory history buffering: §A below** (July 2026). On-disk snapshot format remains with Doc 13 (Save Format) |
| Edit-mode behavior during simulation | **To be specified** alongside Phase 6 (Branching & Interventions UX) |

## Implementation Status

Tick system is implemented in:
- `genesis_core::time` — `WorldYear`, `Era`, `WorldTime`, `SimulationLayer`, `TickCoordinator`
- `genesis_core::lifecycle` — `create_world`, `generate_full_history`, `GenerationProgress`

Tests covering the tick system are spread across:
- `genesis_core::time::tests`
- `genesis_core::time::ticks::tests`
- `genesis_core::lifecycle::tests`

## Why This Doc Still Exists

Three reasons:

1. **Numbering stability.** The original Doc 01 plan referenced documents by number. Keeping a Doc 05 placeholder means cross-references in other docs (and in commit history, ADRs, etc.) don't break or become ambiguous.

2. **Onboarding clarity.** A reader scanning the docs folder shouldn't see a gap between 04 and 06 and wonder if they're missing something.

3. **Forward placeholder.** The items deferred above (edit-mode behavior) will eventually need a real specification. When that happens, this doc can either be expanded or replaced with a pointer to wherever the content actually lands.

## §A. In-Memory History Buffering (specified July 2026, Phase 3 viewer)

The interactive viewer's timeline scrubbing is served by in-memory **history
frames**, not disk snapshots. Decision record:

- A `HistoryFrame` captures only the RENDERABLE per-hex fields (`elevation_mean`,
  `temperature_mean`, `precipitation`, `climate_regime`, `flow_volume`) plus
  `sea_level_m` and the year — ~0.5 MB at subdivision 7. The grid and
  non-rendered simulation state are never duplicated. Render modes may only
  read fields a frame carries; anything else is stale during scrubbing.
- **Streaming (July 2026 revision):** generation runs on a background thread
  and STREAMS `GenEvent`s to the viewer: stage markers (grid build, formation),
  a display clone of the world at year 0, history frames as they are captured,
  throttled progress, and completion. The viewer opens on the FIRST frame and
  the timeline grows behind it like a video buffer; playback stalls at the
  live edge until more frames arrive. `genesis_ui::worldgen::{GenEvent,
  generate_world_streaming}`.
- Scrub cadence is fixed at **10 My** (`HISTORY_STRIDE_YEARS`) so timeline
  steps are identical at 1 By and 4.5 By. Soft memory budget remains
  `max_frames = clamp(256 MB / (cells × ~40 B), 16, 256)` as an advisory;
  long high-resolution runs can exceed it. Always include the first tick and
  the final state
  (`genesis_ui::worldgen::{HISTORY_STRIDE_YEARS, max_history_frames,
  history_stride_years}`).
- Scrubbing copies a frame's fields onto the displayed `WorldData` and sets the
  render layer's `ColorsDirty` flag: chunk meshes are recolored in place via
  their vertex-color buffers (the grid is immutable within a run); no meshes
  or materials are rebuilt. Hold-to-scrub repeats at 60 ms after a 350 ms
  initial delay.
- Frames are display-only. Re-simulation, branching, and byte-exact restore
  go through the deterministic pipeline and (future) Doc 13 disk snapshots —
  a `HistoryFrame` is NOT a save state.

## §B. Forward Design Notes (recorded July 2026, not yet implemented)

### §B.1 Time resolution, tick cadence, and the milestone ratchet

Recorded July 2026 from a design conversation about how finely time can be
watched and simulated once life — and later civilization — exists. Not yet
implemented; the target home for cadence rules once Doc 09/Doc 10 land.

**Three separate knobs, often conflated.** Keeping them distinct is the whole
point of this note:

1. **Tick interval (correctness).** The sim's internal integration step. Plate
   motion, climate, and evolution integrate *per tick*, so this cannot be made
   arbitrarily coarse without the simulation diverging — it has a per-layer
   *floor*. Milestones **lower** the floor (finer ticks) as complexity rises.
   You cannot "jump 50 My" through an active biosphere; the sim must step it.
2. **Fast-forward rate (throughput).** How much simulated time is computed per
   second of wall-clock, and how far the user may skip ahead. A throttle, not a
   correctness parameter. This is where "cap the jump so the system can buffer"
   belongs. Streaming already stalls playback at the live edge (§A), so the
   viewer cannot outrun generation; what is unhandled is that generation itself
   slows sharply once life/civ are active.
3. **Capture / scrub stride (display + memory).** How finely history frames are
   kept for scrubbing. Currently a flat 10 My (§A) — decoupled from tick
   interval; a pure viewer/memory decision (Prep-09 concern).

**Current per-era tick intervals (from `layer.rs` in each crate):**

| Era | Tectonics | Climate | Hydrology |
|---|---|---|---|
| Formation | 500k | 5 My | 5 My |
| Geological | 500k | 500k | 500k |
| Prehistoric (life→sapience) | 2 My | 500k | 500k |
| Ancient (post-sapience) | 10 My | 100k | 100k |
| Recent | **0 = dormant** | 1k | 1k |

Note the asymmetry: **tectonics coarsens and goes dormant** toward the present
(continents don't move perceptibly in 1k years), while climate/hydrology — and,
by extension, biology — **refine to 1k**. So the "does a 500k jump obliterate an
evolving species?" worry is inverted by design: the layers that move slowly stop
ticking exactly when you zoom into fine time, so there is no coarse jump to
obliterate anyone. Within a tick, change is rate-bounded (motion = velocity ×
interval), and biology reads and responds each tick (Doc 09 §6.3 migrate/adapt/
die; §4.6 biomes migrate and lag). Obliteration = change outrunning migration =
a mass-extinction shock (Doc 09 §7.2), which is a feature, not a tick artifact.
Doc 09 §5.5 guarantees tick cadence "never changes what is true."

**The milestone ratchet (the design direction).** Era → tick interval already
*is* this ladder; the change is that the boundaries must become **milestone-
triggered, not fixed years** — required anyway because Doc 09 turns
`life_emergence_year` and `sapience_emergence_year` into *outputs* (Doc 09 §3.1,
§10.3), so the fixed-year boundaries of Doc 04 §7.2 no longer exist. When biology
emits `LifeEmerged` → enter Prehistoric; `SapienceEmerged` → Ancient/Recent; each
ratchets the cadence finer. A world where life never emerges stays coarse
forever. **Civilization (Doc 10) extends the ladder downward:** agriculture and
later tech milestones drive the floor below Recent's 1k, toward **1-year ticks**,
because historical change (nations, wars, technology) is fast on that scale and
users will want to watch it a year at a time.

**The real constraint is per-tick *cost*, not tick *frequency*.** Ten thousand
1-year ticks is trivial; ten thousand ticks each simulating a busy world is not.
Biology bounds its cost with the guild/ledger trick + lazy generation (Doc 09
§5.1, §8.2). **Civilization has no such bound yet** — if Doc 10 simulates nations/
wars/individuals per-hex-per-year, fine civ ticks are unaffordable. So *the tick
floor achievable at each rung is set by how bounded Doc 10 keeps its per-tick
work* — a Doc 10 granularity decision, not a viewer decision. Pin civ simulation
granularity before committing to "1 year at a time."

**Recommended shape: adaptive, compute-budgeted fast-forward with a milestone
ceiling.** Advance as fast as the sim computes within a per-frame ms budget (the
`GENESIS_SLOW_TICK_STEP_MS` hook, Doc 09 §15), with a milestone-driven *ceiling*
on rate as a UX guarantee (never blast past the invention of agriculture at
10 My/s, regardless of hardware). This self-tunes to hardware and scene weight
where a hardcoded cap does not. Pair with a **variable, era-aware capture stride**
(coarse in deep time, fine in the short life-/civ-rich recent eras) so scrub
resolution follows cadence without blowing the frame-memory budget.

Cross-refs: Doc 04 §7.2 (Era), Doc 09 §5.5 / §8.2 (tick-robustness, two-speed
interaction), Doc 10 §"Time resolution after agriculture" (civ cadence floor),
Prep-09 (viewer capture stride).

### §B.2 River/lake rendering is pre-Doc-08 provisional

- The viewer draws rivers as discharge-thresholded polylines along
  `flow_direction` paths and pools lake discs at endorheic sinks
  (`genesis_render::rivers`). Real hydrology — lake filling and spill,
  groundwater, deltas — is Doc 08 scope and will replace this presentation
  layer's assumptions.

## Rule for Future Docs

If a planned Tier 2 spec turns out to be small enough to absorb into a related doc during implementation, follow this pattern:

1. Mark the absorbed doc as "absorbed" in Doc 01 §"Documents"
2. Replace the original doc's content with an index pointer like this one
3. Update the changelog
4. Cross-reference from the absorbing doc back to this index

This keeps the documentation surface honest about what exists vs. what was planned, without breaking external references.
