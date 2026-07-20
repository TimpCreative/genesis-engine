# Prep-09 — Viewing Shell & Biology Presentation Prep

**Document Type:** Tier 2 — Prep / UI Specification
**Status:** Draft v0.1
**Last Updated:** July 2026
**Owner:** Brax Johnson
**Implementing Phase:** 3.5 — between Phase 3 (Rendering MVP) and Phase 4 (Biology)

**Changelog:**
- v0.1 (July 2026): Initial draft. Specifies the viewing shell to build **before** Doc 09 (Biology) so the game can present life the moment biology simulates it. Establishes the **read-side `BiologyView` seam** (a stub now, a Doc 09 adapter later), the top-bar layer selector, the era-banded event-pip timeline, the enriched inspector dock, the Tree-of-Life and Bestiary overlays, and the layered pixel-art creature renderer. Pulls a reviewable slice of Doc 14 (Rendering & UI) forward under a stub data contract.
- v0.2 (July 2026): **Moved all creature rendering out to Doc 09 §8.5** (it belongs with evolution). This prep now shows species as text + trait chips with a reserved illustration slot; the shell, seam, layers, timeline, inspector, and overlays are unaffected. The prompt plan drops the creature-renderer slice — **now 7 slices**.

---

## 1. Purpose and Scope

Doc 09 (Biology) will produce biomes, a diversity field, an ecological ledger, a tree of life, and lazily-generated species. None of that is worth simulating if there is nowhere to *see* it. Today the viewer is thin: a map, a HUD line, a scrub timeline ([`crates/genesis_ui/src/ui.rs`](../crates/genesis_ui/src/ui.rs)), and a 320px inspector dock whose **Life** tab already exists but reads *"Not simulated yet."* ([`crates/genesis_ui/src/hex_inspect.rs`](../crates/genesis_ui/src/hex_inspect.rs)). Render modes cycle blindly on the `M` key through five physical layers ([`crates/genesis_render/src/render_mode.rs`](../crates/genesis_render/src/render_mode.rs)).

This document specifies the **presentation shell** — top bar, layer selector, upgraded timeline, enriched inspector, and the Tree-of-Life and Bestiary views — and the **data seam** that lets all of it be built and reviewed **now, against a stub**, then switch to real biology with **zero UI rework** when Doc 09 lands. Creature illustration is out of scope here — it lives with evolution in Doc 09 §8.5; the shell only reserves the slot.

### 1.1 The Governing Idea: build the surface against a stub, swap the source at Doc 09

The whole plan hinges on one seam. The UI never reaches into biology internals; it reads a **`BiologyView`** — a small read-only trait that answers exactly the questions the presentation asks ("what biome is this hex?", "how rich is it?", "generate the assemblage here", "give me the tree at this year", "what life events fall in this year range?").

- **Now:** a `StubBiologyView` answers deterministically from fields that already exist (`habitability`, `soil_fertility`, `climate_regime`, elevation/water) plus the world seed. It invents plausible biomes, a richness scalar, fake guild rosters, and fake species with real-looking trait vectors. Everything renders; nothing is real yet.
- **At Doc 09:** `genesis_biology` provides an adapter implementing the same trait over the real ledger and the `WorldData` biology arrays it writes (Doc 09 §8.6). The UI code does not change — the stub is unregistered and the real view registered in its place.

This is why the shell can ship first: the presentation is decoupled from the simulation by a contract, exactly as rendering is already decoupled from simulation by reading `WorldData` and never writing it (Architecture §15).

### 1.2 What is real now vs. stubbed until Doc 09

Not all of this is placeholder. A large part improves *physical-world* viewing immediately:

| Surface | Status before Doc 09 |
|---|---|
| Top bar + labelled layer selector | **Real** — replaces the blind `M`-cycle for all five existing modes |
| Timeline era bands | **Real** — geological era from year |
| Timeline event pips | **Real** — from the existing tectonic/climate/hydrology event log (§5.1) |
| Enriched Terrain/Climate/Water inspector tabs | **Real** — richer reads of existing fields |
| Biome / Biomass / Diversity map layers | **Stub** — `StubBiologyView` coloring |
| Life inspector tab (guilds, richness, species card) | **Stub** |
| Bestiary overlay | **Stub** |
| Tree-of-Life overlay | **Stub** |
| Species cards (name, guild, trait chips, text) | **Stub** — no illustration; all creature rendering is Doc 09 §8.5 |

So this is not a pile of fake screens: it is a real UI upgrade for the physical layers **plus** a reviewable biology surface waiting for its data (illustration excepted — that arrives with Doc 09).

### 1.3 Goals

1. **A structural shell** — top bar (layer selector + year/era readout + view buttons), map, right dock, bottom timeline — that has a home for every Doc 09 output.
2. **A legible deep-time timeline** — era bands and clickable event pips so a 4-billion-year scrub has landmarks, not a featureless line.
3. **A clean `BiologyView` seam** so Doc 09 integrates by implementing one trait, not by touching UI code.
4. **The Tree-of-Life and Bestiary views** as full-screen overlays, browsable against stub data (species shown as text + trait chips; illustration deferred to Doc 09).
5. **No regressions** — the physical-layer viewing (elevation/climate/water/soil, scrub, screenshots) stays green.

### 1.4 Non-Goals (this prep)

- **No biology simulation.** No guilds, ledger, trait morphospace, speciation, or evolution — all Doc 09.
- **No real species.** Stub species are seeded fabrications for layout; they carry no ecological truth and are discarded when Doc 09 lands.
- **No new projections or globe view.** Equirectangular only, as today; projections are Doc 14.
- **No editing/intervention UI.** Meteor placement and friends are Doc 09 §12 / Doc 14.
- **No creature rendering at all.** Species are shown as text + trait chips; the entire creature renderer — construction grammars, pixel-art part atlas, `CreatureTraits`/`GrowthForm`, the compositor — is Doc 09 §8.5, shipping with evolution. The shell only reserves the card/tree-node slot the illustration will later fill.
- **No civilization surface.** The Society tab stays a placeholder.

### 1.5 Dependencies

**Reads (existing):** `WorldData` fields (elevation, temperature, precipitation, `climate_regime`, water, `soil_fertility`, `soil_class`, `habitability`, `biome`, `biomass`); `RenderMode`/`CurrentRenderMode`/`hex_color_for_mode`; `HistoryFrame`/`GenEvent`/`WorldTimeline`; the branch `event_log` (`genesis_core::events`: `Event`, `EventKind`, `EventLocation`, `Significance`, `EventLog`); `SelectedHex`, `WorldResource`.

**Adds:** the `BiologyView` trait + `StubBiologyView`; top-bar, timeline, and overlay UI; new `RenderMode` variants (stub-backed).

**Consumed by (later):** Doc 09 (implements `BiologyView`, fills the `WorldData` biology arrays and `HistoryFrame` biology fields); Doc 14 (absorbs this shell as its first UI slice; expands the part atlas and projections).

### 1.6 Relationship to Doc 14

Doc 14 (Rendering & UI Specification, Architecture §15) is not yet written. This prep deliberately implements a **slice** of it early, scoped to what unblocks Doc 09 review. Where this doc and Doc 14 disagree later, **Doc 14 wins**; the seam here (the `BiologyView` trait) is chosen to survive that.

---

## 2. The `BiologyView` Seam

The single most important artifact of this prep. A read-only trait the presentation consumes; the only thing Doc 09 must implement to light up the whole shell.

### 2.1 The trait (read-side contract)

```rust
/// Everything the presentation layer asks of "life", decoupled from how it is
/// produced. Implemented by `StubBiologyView` now; by a `genesis_biology`
/// adapter at Doc 09. Pure reads — never mutates world state.
pub trait BiologyView {
    /// Biome id for a hex (BiomeId::NONE if unassigned/ocean-unlit).
    fn biome_at(&self, hex: HexId) -> BiomeId;

    /// Biotic richness scalar R ∈ [0,1] (Doc 09 §4.4) for a hex.
    fn richness_at(&self, hex: HexId) -> f32;

    /// Total living biomass proxy for a hex (render/heatmap only).
    fn biomass_at(&self, hex: HexId) -> f32;

    /// Occupied functional guilds at a hex, headline-first.
    fn occupied_guilds(&self, hex: HexId) -> Vec<GuildSummary>;

    /// The generated species assemblage for a hex (Doc 09 §8.4), materialized
    /// on demand. Stub fabricates; Doc 09 lazily generates from the ledger.
    fn assemblage(&self, hex: HexId) -> Assemblage;

    /// A snapshot of the tree of life as of `year` (Doc 09 §9.3): live branches
    /// present, extinct branches greyed. Stub fabricates a small tree.
    fn tree_snapshot(&self, year: WorldYear) -> TreePeek;

    /// Life-relevant events overlapping [from, to] for timeline pips (§5).
    fn life_events(&self, from: WorldYear, to: WorldYear) -> Vec<LifeEventPip>;
}
```

Supporting read-model types (`GuildSummary`, `Assemblage`, `SpeciesPeek`, `TreePeek`, `TreeNodePeek`, `LifeEventPip`) are **presentation DTOs**, not the biology domain model. They are intentionally lossy — enough to draw a card, a row, a node, a pip. Doc 09's rich `LineageRecord`/`TraitSet` map *into* them; the UI never sees the full model. `SpeciesPeek` carries text + trait chips only and reserves an initially-empty **illustration handle** that Doc 09's creature renderer (§8.5 there) fills — the shell draws the card identically either way.

### 2.2 Where it lives (crate boundary)

The trait and its DTOs live in **`genesis_ui`** (the consumer owns its read model), because `genesis_ui` cannot depend on `genesis_biology` (which will not exist until Doc 09) without an inversion. `StubBiologyView` also lives in `genesis_ui`. At Doc 09, `genesis_biology` gains a thin `BiologyLedgerView` adapter; whether the adapter lives in `genesis_biology` or a tiny `genesis_biology_view` bridge crate is a Doc 09 decision — either satisfies the trait. The active view is a Bevy `Resource` (`Box<dyn BiologyView + Send + Sync>` behind a newtype) selected at world load.

### 2.3 Determinism of the stub

The stub must be deterministic (same seed + same `WorldData` → same biomes, species, tree) so screenshots and reviews are stable. It draws from `hash(world_seed, "prep9.stub", hex, query)` streams, mirroring the engine's stream discipline, and derives biome/richness as pure functions of existing fields (a crude habitability→biome table plus latitude/soil). It is explicitly **not** ecologically valid and carries a `// STUB` marker so nothing downstream mistakes it for simulation.

---

## 3. Screen Structure

The Viewing screen (`AppScreen::Viewing`) gains a persistent frame around the map. Three tiers, per the layout review:

```
┌─ TOP BAR ─────────────────────────────────────────────────────────┐
│ [Elevation|Climate|Biome|Biomass|Diversity|Society]  3.20 By ·     │
│                                Proterozoic · O₂ 12%   [🌳][📖][⚡]  │
├───────────────────────────────────────────────┬───────────────────┤
│                                                │  INSPECTOR DOCK   │
│                    MAP                          │  Hex 4821 · 12°N  │
│              (equirectangular)                  │  Terrain Climate  │
│                                                │  Water [Life] Soc  │
│                                                │  …tab body…       │
├────────────────────────────────────────────────┴───────────────────┤
│ ◀ [▶ Play] ▶▶50My/s  [era bands ● ● ◆ ●  |playhead]  3.20By  [Menu]│
└─────────────────────────────────────────────────────────────────────┘

  Full-screen overlays (from top-bar buttons): 🌳 Tree of Life · 📖 Bestiary · ⚡ Events
```

- **Top bar** (new persistent node on Viewing): left = the layer selector (§4); center = year + geological era + one headline global (O₂/sea level; O₂ is stub until Doc 09/07 wiring); right = buttons opening the overlays (§7–§8) and the events log.
- **Map**: unchanged rendering; it remains the calm default surface.
- **Right dock**: the existing inspector ([`hex_inspect.rs`](../crates/genesis_ui/src/hex_inspect.rs)); tabs and per-hex reads enriched (§6). Stays 320px.
- **Bottom timeline**: the existing scrub bar (`ui.rs`), upgraded with era bands and event pips (§5). Existing play/scrub/keyboard machinery is reused.
- **Overlays**: Tree of Life and Bestiary are **full-screen takeovers** over the map, not dock tabs — a phylogeny does not fit in 320px. Opened from the top bar, dismissed with `Esc` (which must clear an open overlay before it clears a selection or exits, extending the existing `Esc` ladder in `viewer_escape`).

The window itself should honor OS resize/maximize (verify the Bevy window is resizable, not fixed) so the overlays have room; the map and dock reflow with width.

---

## 4. The Layer Selector (top bar)

Today `RenderMode` has five variants cycled by `cycle_render_mode_on_keypress` on `M`. Nine-plus modes on a blind cycle is unusable; the modes become **visible labelled tabs** in the top bar, with `M` retained as a "cycle" shortcut.

### 4.1 New render modes (stub-backed)

Extend `RenderMode` ([`render_mode.rs`](../crates/genesis_render/src/render_mode.rs)) and `hex_color_for_mode` ([`color.rs`](../crates/genesis_render/src/color.rs)) with:

- `Biome` — categorical fill from `BiologyView::biome_at` (a biome→palette table; Architecture §15 "biome scientific coloring").
- `Biomass` — sequential heatmap from `biomass_at`.
- `Diversity` — sequential heatmap from `richness_at` (the latitudinal-gradient view Doc 09 §17 validates).
- `Society` — placeholder (flat/among-existing), reserved for Doc 10.

The biology modes read through the active `BiologyView` resource, so they color from the stub now and from real data at Doc 09 with no color-code change. Selector state and `CurrentRenderMode` stay the single source of truth.

### 4.2 Interaction

Clicking a tab sets `CurrentRenderMode`, flips `ColorsDirty`, and re-tints. The active tab is highlighted (reuse the inspector-tab active/idle color pattern). A one-line legend for the active mode sits under the map (the mockup's bottom-left chip) so categorical/heatmap scales are readable.

---

## 5. The Timeline (era bands + event pips)

The scrub bar becomes the primary landmark surface for deep time. Machinery (`WorldTimeline`, play/pause, hold-to-scrub, `HISTORY_STRIDE_YEARS = 10 My`) is reused; this is additive.

### 5.1 Event pips (real today)

The branch **already has an event log** with a `Significance` gate (`Trace < Minor < Notable < Major < Pivotal`). Pips render events at or above a user threshold, positioned by year:

- **Physical (real now):** `WorldFormation`, `OceansBeginForming`/`OceansStabilized`, `MountainRangeFormed`, `OceanBasinOpened`, `VolcanicEruption` (clustered), `GlaciationBegan`/`GlaciationEnded`, `SeaLevelMilestone`, `GlacialMaximum`, … (`genesis_core::events::EventKind`).
- **Biological (stub now, real at Doc 09):** `BiologyView::life_events` supplies `LifeEventPip`s (life emerged, Great Oxygenation, mass extinction, sapience). Doc 09 will emit the real `EventKind` variants (§13 of Doc 09); until then the stub fabricates a plausible handful so the pip UI is exercised.

**Plumbing note:** events currently reach the branch `event_log` only after generation flushes (`flush_events_to_branch`). For live pips during streaming, add an event channel to the `GenEvent` stream (a `GenEvent::Events(Vec<Event>)` batch) *or* read the branch log once generation completes. The streaming route is preferred so pips appear as history buffers in (YouTube-style, matching the existing frame streaming).

Pip glyphs: round for milestones/innovations, diamond for extinctions/catastrophes, color by category (see the mockup). Hover shows the event summary; click jumps the playhead to that year.

### 5.2 Era bands

A background band strip segments the bar by geological era, derived from year via a static table (Hadean/Archean/Proterozoic/Phanerozoic sub-eras). This is purely a year→label/color function — real now, no simulation needed. Bands sit behind the buffered-region and playhead fills already in `refresh_hud`.

---

## 6. The Inspector Dock (enriched)

The dock keeps its tab set (`InspectorTab`), with real upgrades to physical tabs and the Life tab wired to `BiologyView`.

### 6.1 Life tab (stub-backed)

Replace the `"Not simulated yet."` body in `format_life` with a `BiologyView` read:

- Biome (name from `biome_at`), richness `R` with a "near cap" hint (Doc 09 §4.5), occupied-guild count from `occupied_guilds`.
- Dominant lineage headline + a **species card**: a generated name, guild, and trait chips, with a reserved illustration slot (filled by Doc 09's creature renderer, §8.5 there — empty in this prep).
- A **"Generate assemblage (N guilds) →"** affordance that opens the Bestiary overlay (§8) focused on this hex — the seam between cheap always-on fields and on-demand generation (Doc 09 §8.4).

Because this reads the trait, it shows fabricated life now and real life at Doc 09 unchanged.

### 6.2 Physical tabs (real upgrades)

Small, real improvements while we are here: Terrain/Climate/Water tabs surface a few more of the fields they already have access to (e.g. relief, temperature range, discharge class) — no new data, just fuller reads. Keeps the dock honest and useful pre-biology.

---

## 7. The Tree-of-Life Overlay (stub-backed)

A full-screen overlay (opened from the top-bar 🌳) rendering `BiologyView::tree_snapshot(current_year)`:

- Trunk + major branches laid out from the `TreePeek` node list; **time-aware** — branches extinct at the viewed year are greyed and non-inspectable (Doc 09 §9.3); walking the timeline forward grows the tree, scrubbing back prunes it (the stub honors this by filtering nodes on origin/extinction year).
- Each node shows its rank and defining trait (stub assigns tiers→ranks per Doc 09 §9.2) and links to its Bestiary entry.
- Layout is a simple tidy-tree; pan/zoom within the overlay. No lazy fine-branch expansion in prep (a Doc 09/14 refinement) — the stub returns a bounded tree.

Dismiss with `Esc` or the top-bar toggle.

---

## 8. The Bestiary Overlay (stub-backed)

A full-screen overlay (📖, or the Life-tab "Generate assemblage" button) rendering `BiologyView::assemblage(hex)`:

- A grid/list of `SpeciesPeek`s for the focused hex/province, each a card: name, guild, trait chips, and a **text description** (stubbed here; Doc 09 §8.5 v1 generates it deterministically from the trait list), with a reserved illustration slot Doc 09's renderer fills.
- **Hierarchical drill-down** scaffold: Family → Genus → Species headers (Doc 09 §8.4). The stub returns a shallow, bounded set; the UI supports scroll-to-expand batching so Doc 09's real generation drops in.
- Filter by guild; sort by size/trophic level.

The Bestiary and Tree are two lenses on the same stub ledger, kept consistent by seeding both from the same `hash(seed, hex/lineage, …)` streams.

---

## 9. Species Representation in the Shell

Species appear in three places — the Life-tab card (§6.1), the Bestiary cards (§8), and the Tree-of-Life nodes (§7). In this prep they are drawn as **name + guild + trait chips + a short text description**, all from the `SpeciesPeek` DTO. There is **no creature illustration in this prep.**

All creature rendering — the axis-driven pixel-art compositor, its construction grammars, the part atlas, and the `CreatureTraits`/`GrowthForm` model — is specified and implemented in **Doc 09 §8.5**, because it is a pure function of a species' evolved trait set and belongs with evolution. The shell's only obligation is to **reserve the slot**: every species card and tree node leaves a fixed illustration area (empty here) that Doc 09's renderer later fills, with no card or layout rework. That reserved handle lives on `SpeciesPeek` (§2.1).

This keeps the viewing-shell overhaul completely independent of creature art: an agent can build and polish the entire shell against text-and-chips species, and illustrations light up when Doc 09 lands.

---

## 10. Data & Stub Schema

- **`RenderMode`** gains `Biome`, `Biomass`, `Diversity`, `Society` (§4.1).
- **`BiologyView`** trait + DTOs + `StubBiologyView` (§2), as a `genesis_ui` module; registered as a Bevy resource at world load.
- **`HistoryFrame`**: add **placeholder biology fields** (`biome: Vec<BiomeId>`, `biomass: Vec<f32>`, `biotic_richness: Vec<f32>`) captured as empty/zero now, so the frame schema is Doc-09-ready and scrubbing biology layers "just works" once Doc 09 fills them. Guard `apply` on length like the existing water fields.
- **Era table** and **biome→palette table**: static content in `genesis_render`/`genesis_ui`.

No `WorldData` schema change is required by this prep — Doc 09 §8.6 owns the biology arrays. If it is convenient to add the (zeroed) arrays early to stabilize `HistoryFrame`, that is permitted but optional.

---

## 11. Determinism & Performance

- **Determinism:** the stub view is a pure function of `(seed, WorldData, query)` — no wall-clock, no unordered iteration; `BTreeMap`/sorted order for any collection that reaches the screen. Screenshots must be reproducible.
- **Performance:** presentation is off the simulation hot path. Budgets: layer re-tint ≤ existing color pass; Bestiary/Tree open ≤ 50 ms against the stub.
- The stub must **never** run during headless generation or ticks — it is a viewer-only resource.

---

## 12. Validation

Shape checks, not pixel checks:

1. **Seam:** swapping `StubBiologyView` for a second dummy view changes all biology surfaces (map layers, Life tab, Bestiary, Tree) with no other code change — proves the decoupling.
2. **Layer selector:** every `RenderMode` (old + new) is reachable from the top bar and colors the map; `M` still cycles.
3. **Timeline:** physical event pips appear at correct years for a default run; clicking a pip jumps the playhead; era bands cover the full span.
4. **Inspector:** the Life tab renders biome/richness/guilds/species for a land hex and degrades gracefully over ocean/ice.
5. **Overlays:** Tree and Bestiary open/close via top bar and `Esc`; the `Esc` ladder (overlay → selection → menu) is correct; every species card and tree node exposes the reserved (empty) illustration slot.
6. **No regression:** existing `genesis_ui`/`genesis_render` tests pass; physical-layer screenshots unchanged; scrub color tests green.
7. **Perf:** no frame-time regression on the Viewing screen at subdivision 8.

---

## 13. Integration & Migration (how Doc 09 plugs in)

When Doc 09 lands, integration is:

1. **Implement `BiologyView`** in a `genesis_biology` adapter over the real ledger + `WorldData` biology arrays; register it instead of `StubBiologyView`. All biology surfaces go live unchanged.
2. **Illustrate species** with Doc 09's creature renderer (§8.5 there): it fills the illustration slot the shell reserved on every species card and tree node — no card or layout change.
3. **Fill `HistoryFrame`'s biology fields** from real `biome`/`biomass`/`biotic_richness`; the Biome/Biomass/Diversity layers become scrub-accurate.
4. **Emit real biology `EventKind`s** (Doc 09 §13); the timeline pips switch from stub to real via `life_events`.
5. **Delete the stub** and its `// STUB` markers.

No screen, layout, tab, overlay, or card is rewritten — the illustration simply appears in the slot already reserved for it. That is the entire point of doing this first.

---

## 14. Implementation Prompt Plan

Seven slices, each independently reviewable in the running app. Order matters: the seam first, then the chrome that depends on it. (Creature illustration is **not** here — it is Doc 09 §8.5; every species card and tree node below reserves its slot.)

1. **Prep9-1 — The `BiologyView` seam + `StubBiologyView`.** Trait + DTOs + deterministic stub over existing fields; registered as a resource at world load. No visible change yet; unblocks everything. *(Files: new `genesis_ui/src/biology_view.rs`; `lib.rs` wiring.)*
2. **Prep9-2 — Top bar + layer selector.** Persistent top-bar node on Viewing; labelled layer tabs driving `CurrentRenderMode`; add `Biome`/`Biomass`/`Diversity`/`Society` render modes coloring through the view; keep `M`. *(Files: `ui.rs`; `render_mode.rs`; `color.rs`.)*
3. **Prep9-3 — Timeline era bands + event pips.** Era band strip; event pips from the branch log (add `GenEvent::Events` streaming or post-run read); hover/click-to-jump; significance threshold. *(Files: `ui.rs`; `worldgen.rs`; small `genesis_core` event accessor if needed.)*
4. **Prep9-4 — Enriched inspector.** Life tab wired to `BiologyView` (biome/richness/guilds/dominant + assemblage button); species card as name + guild + trait chips with a reserved illustration slot; fuller physical tabs. *(Files: `hex_inspect.rs`.)*
5. **Prep9-5 — Bestiary overlay.** Full-screen assemblage browser; species cards (name + guild + trait chips + text, illustration slot reserved for Doc 09); Family→Genus→Species drill-down scaffold; open from top bar + Life button. *(Files: new `bestiary.rs`; `ui.rs` overlay state.)*
6. **Prep9-6 — Tree-of-Life overlay.** Full-screen phylogeny from `tree_snapshot`; time-aware greying; node → Bestiary link; pan/zoom. *(Files: new `tree_of_life.rs`.)*
7. **Prep9-7 — Integration contract + polish + validation.** `Esc` ladder for overlays; `HistoryFrame` biology placeholder fields; the Doc 09 integration checklist (§13) encoded as doc-comments on the seam; validation suite (§12); screenshot pass. *(Files: across; `worldgen.rs`.)*

Each slice ships behind the same acceptance bar as the rest of the repo: `cargo build/test/fmt/clippy` green, no regression to physical-layer viewing, and a screenshot or short clip showing the new surface against the stub.

---

*End of Viewing Shell & Biology Presentation Prep.*
