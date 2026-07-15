# 05 — Tick System & Time Management Specification

**Document Type:** Tier 2 — System Specification (absorbed)
**Status:** Absorbed v1.1
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
  non-rendered simulation state are never duplicated.
- Frames are captured during generation by the tick observer
  (`TickCoordinator::advance_to_with`), at a stride of
  `max(target_year / 64, 500_000)` years — at most **64 frames per run**
  (~32 MB at subdivision 7), always including the first tick and the final
  state. `genesis_ui::worldgen::{HistoryFrame, history_stride_years}`.
- Scrubbing copies a frame's fields onto the displayed `WorldData` and sets the
  render layer's `ColorsDirty` flag: hex materials are recolored without
  rebuilding meshes (the grid is immutable within a run). Scrub cost is
  ~one memcpy of the frame plus one material pass.
- Frames are display-only. Re-simulation, branching, and byte-exact restore
  go through the deterministic pipeline and (future) Doc 13 disk snapshots —
  a `HistoryFrame` is NOT a save state.

## Rule for Future Docs

If a planned Tier 2 spec turns out to be small enough to absorb into a related doc during implementation, follow this pattern:

1. Mark the absorbed doc as "absorbed" in Doc 01 §"Documents"
2. Replace the original doc's content with an index pointer like this one
3. Update the changelog
4. Cross-reference from the absorbing doc back to this index

This keeps the documentation surface honest about what exists vs. what was planned, without breaking external references.
