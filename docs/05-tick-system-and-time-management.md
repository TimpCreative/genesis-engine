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

## §A.1 Live Real-Time Stepping (specified July 2026, supersedes interpolation)

The viewer no longer pre-records the whole history and scrubs a fixed 10 My
buffer; it keeps the **simulation resident and steps it forward at real tick
granularity**. Rationale: the engine already computes fine ticks (500k in the
Geological era — see the era/interval table in §B.1), so showing 10 My-strided
snapshots and interpolating between them displayed states the sim never
computed. Interpolation was tried and removed — it looked wrong (a linear tween
of two snapshots is not the real trajectory) and, per user direction, "needs to
be realistic and real time".

Architecture:

- **Persistent worker** (`genesis_ui::worldgen::run_live_simulation`). One
  background thread owns the `World`, all four layer states, and a **single**
  `TickCoordinator` for the whole session — built once and never rebuilt,
  because each physical layer's timestep clock (`last_tick_year`) is layer-local
  and resets on `attach`; rebuilding would make the next step compute a
  catastrophic multi-hundred-My interval. The coordinator's `Rc`/`Cell` interior
  is `!Send`, so it stays confined to this one thread. It runs the initial
  generation to the target (streaming stride frames + progress exactly as
  before), then blocks on a command channel.
- **Command channel.** The UI sends `SimCommand::AdvanceTo(year)`; the worker
  advances the resident coordinator and emits **one real captured frame** per
  command. Backward-in-time commands are ignored (time only moves forward).
- **Resumable coordinator** (`TickCoordinator::advance_resumable[_with]`,
  `genesis_core::time::ticks`). Anchors the per-layer tick schedule **once** and
  carries it forward, so stepping `t0 → t1 → … → tn` fires exactly the ticks a
  single `advance_to(tn)` would — proven by `resumable_stepping_equals_a_single_run`
  and, end-to-end, by `live_stepping_matches_one_shot_generation` (stepped state
  is bit-identical to a one-shot run). The one-shot `advance_to` re-anchors every
  call and is kept for generation and the validation gates.
- **Forward stepping** at the live edge commands the worker for one more real
  span; within the buffer, `<`/`>` walk the already-simulated real frames
  (instant, no re-sim); backward is always buffer-only. The bottom-bar step
  button cycles the span over `STEP_SPANS_YEARS = [500k, 1M, 2M, 10M]` — every
  value an exact multiple of the 500k tectonic tick, so the worker always lands
  on a real computed state. There is deliberately **no sub-500k step**: the
  tectonic model has no finer state, so it would be fiction. Play auto-commands
  the next span on the timer (real-time playback).
- **Consequence / limitation.** Fine stepping extends real history *forward from
  the live edge*; it cannot fine-step *within* an already-generated coarse span
  (those intermediate states weren't captured and the resident sim is at the
  edge, not the past). To watch a span slowly, generate to its start (a small
  target year) and step forward. Full random-access fine scrubbing would need
  full-state keyframes (§B) — out of scope here.

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

**Life & mind layers — status and the biology caveat (implemented/measured July 2026).**

*Biology* ticks at a flat **500k in every non-Formation era** (`genesis_biology::layer::DEFAULT_BIOLOGY_TICK_YEARS`). It is **not** yet refined to 1k in later eras, and doing so naïvely would be **outcome-changing, not just finer resolution**: the microbial engine is *fixed-per-tick*, not rate-scaled — `O2_RISE_PER_TICK` and `MICROBIAL_STEP_PROB` (`genesis_biology::microbial`) apply a fixed increment/probability *per tick*, so halving the interval doubles oxygenation and evolution speed per My and re-rolls the year-keyed RNG. Biogenesis (before life) *is* rate-scaled (hazard ∝ interval), and by the Ancient/Recent eras the microbial engine is a no-op (multicellularity long reached; only the 5 My-gated display refresh runs). **Prerequisite for finer biology cadence:** convert those per-tick constants to interval-scaled rates (the generations-vs-ticks model biology's own comments flag) — until then, keep biology at 500k in the microbial-active eras (Geological/early-Prehistoric). Any biology interval must stay a **divisor of 5 My** or the heavy-field refresh gate (`current_year % HEAVY_FIELD_STRIDE_YEARS == 0`) silently stops firing.

*Civilization* is where fine cadence actually matters for intelligent species — and it is **greenfield, so determinism-safe**. `genesis_civilization::CivilizationLayer` is now registered last in both the one-shot (`generate_full_history`) and live (`run_live_simulation`) coordinators. It is dormant (interval 0) until `sapience_emergence_year`, then ticks at `CIV_ANCIENT_TICK_YEARS = 1_000` (Ancient) and `CIV_RECENT_TICK_YEARS = 25` (Recent) — **~500× and ~20,000× finer than the geological tick**, proving the multi-rate coordinator handles the full spread (`coordinator_drives_civ_at_fine_cadence`). This slice is the **cadence seam only**: `advance` is inert — it records the tick and deliberately reads no RNG and mutates no `WorldData`, so registering it cannot perturb the physical/biology trajectory (`advance_does_not_mutate_world_or_rng`; life still emerges at 265 My unchanged). Real civ dynamics build on this seam (Doc 10).

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
