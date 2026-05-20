# Genesis Engine — Architecture Overview

**Document Type:** Tier 1 — Foundational
**Status:** Draft v0.2
**Last Updated:** May 2026
**Owner:** Brax Johnson

---

## 1. Purpose of This Document

This is the technical blueprint for Genesis Engine. It defines the major modules, how they communicate, the data model that connects them, the time model that drives them, the branching model that makes alternate-history exploration possible, and the build order in which subsystems are implemented.

This document is intentionally high-level. Each module gets its own detailed specification (see Vision & Scope §9 for the document map). What this document establishes is the *shape* of the system — the decisions that constrain every other decision.

## 2. The Layer Model

Genesis Engine is organized into four conceptual layers. Each layer depends on the layers below it. Each layer operates at a different temporal scale.

```
┌─────────────────────────────────────────────────┐
│  Layer 3: Narrative                             │
│  (events, characters, individual histories)     │
│  Tick scale: years                              │
├─────────────────────────────────────────────────┤
│  Layer 2: Civilizational                        │
│  (settlements, nations, cultures, technologies) │
│  Tick scale: decades                            │
├─────────────────────────────────────────────────┤
│  Layer 1: Biological                            │
│  (species, biomes, trophic aggregates)          │
│  Tick scale: millennia                          │
├─────────────────────────────────────────────────┤
│  Layer 0: Physical                              │
│  (tectonics, climate, hydrology, soil)          │
│  Tick scale: tens of thousands of years         │
└─────────────────────────────────────────────────┘
```

Data flows upward: Layer 0 outputs feed Layer 1, which feeds Layer 2, which feeds Layer 3.

Interventions flow downward but with **strict scope limits**: a Layer 3 intervention (rename a character) cannot retroactively modify Layer 0 (rewrite tectonics). A Layer 2 intervention (boost a city's food) propagates *forward in time* through Layer 2 and may influence later Layer 1 changes (overhunting depletes regional fauna) but does not invalidate already-computed Layer 0 history.

This is the architectural commitment that makes user editability tractable. Without strict layer scoping, every edit would require global recomputation.

## 3. Module Map

Within the layer model, the system is organized into concrete modules. Each module is implementable independently with a clean data contract to its neighbors.

```
┌────────────────────────────────────────────────────────────────┐
│                      CORE INFRASTRUCTURE                       │
│  ┌──────────────────┐  ┌──────────────────┐  ┌──────────────┐  │
│  │  Hex Grid (DGGS) │  │   Time/Branches  │  │  RNG / Seeds │  │
│  └──────────────────┘  └──────────────────┘  └──────────────┘  │
│  ┌──────────────────┐  ┌──────────────────┐  ┌──────────────┐  │
│  │   Data Layer     │  │ Intervention Log │  │   Snapshot   │  │
│  │      (ECS)       │  │                  │  │    Cache     │  │
│  └──────────────────┘  └──────────────────┘  └──────────────┘  │
│  ┌──────────────────┐  ┌──────────────────┐                    │
│  │  Mod System &    │  │  Rule Engine     │                    │
│  │  Content Loader  │  │  (data-driven)   │                    │
│  └──────────────────┘  └──────────────────┘                    │
└────────────────────────────────────────────────────────────────┘
                              │
        ┌─────────────────────┼─────────────────────┐
        ▼                     ▼                     ▼
┌──────────────┐      ┌──────────────┐      ┌──────────────┐
│  SIMULATION  │      │  RENDERING   │      │    EXPORT    │
│              │      │              │      │              │
│ • Tectonics  │      │ • Map Render │      │ • Timeline   │
│ • Climate    │      │ • Projections│      │ • Gazetteer  │
│ • Hydrology  │      │ • Style Modes│      │ • Bestiary   │
│ • Biology    │      │ • UI         │      │ • Map Images │
│ • Civilizat. │      │ • Intervent. │      │ • Bible      │
│ • Tech Rules │      │   UX         │      │              │
└──────────────┘      └──────────────┘      └──────────────┘
```

### Core Infrastructure

These services are used by every other module. They are written first and form the foundation.

**Hex Grid (DGGS):** The icosahedral hex grid system that maps the planetary surface. Provides cell coordinates, neighbor lookups, geographic queries, and the pentagon handling needed at the 12 icosahedron vertices. See §4 for the grid model in detail.

**Time & Branches:** Manages the simulation clock, tick scheduling across layers, branch creation, branch switching, and the snapshot intervals that allow time-scrubbing.

**RNG / Seeds:** Centralized random number generation. Every randomization in the simulation pulls from a seeded, deterministic RNG. The seed system supports sub-seeds per module so that changes to one module's algorithm don't perturb other modules' outputs. The active mod set is incorporated into the effective seed (see §10).

**Data Layer (ECS):** The Entity Component System that holds all simulation state. Implemented in Bevy's ECS but with care taken to keep simulation logic engine-independent where possible. Uses Struct-of-Arrays layouts for cache-friendly bulk processing.

**Intervention Log:** Append-only log of every user modification, tagged with branch ID, timestamp, scope, and event payload. This is the shareable, replayable record of how a world diverged from its base seed.

**Snapshot Cache:** Time-indexed cache of simulation state. Allows fast time-scrubbing without re-simulating. Cache invalidation is scoped (see §7).

**Mod System & Content Loader:** Loads external content packs (technology rules, biome definitions, species traits, language naming rules, etc.) at world generation time. See §10.

**Rule Engine:** A generic data-driven evaluator for content rules. Used initially by the Technology subsystem; later by magic systems, far-future tech, and user mods. See §11.

### Simulation

The five simulation modules that produce the world's content. Each has its own specification document. Each consumes the outputs of layers below it and produces outputs the layers above consume.

### Rendering

Visualization of simulation data. Multiple projections (equirectangular, orthographic, icosahedral net), multiple style modes (cartographer, political, biome, satellite, heatmap), zoom-level transitions, and the UI for interventions and branch navigation.

### Export

Document generation. Reads simulation data and produces formatted outputs authors can use: timeline chronicles, gazetteers, bestiaries, high-resolution map images, and compiled setting bibles.

## 4. The Hex Grid Model

Genesis Engine uses an **icosahedral hex grid** (ISEA3H — Icosahedral Snyder Equal Area aperture 3 Hexagon). Understanding this grid is foundational because it determines how every spatial operation works.

### Why an Icosahedral Grid

A square grid wrapped onto a sphere produces severe distortion at the poles (the Mercator problem). Cells near the equator are roughly square; cells near the poles become highly distorted slivers. Worse, "neighbor" relationships break down — a cell near the pole has many more equidistant neighbors than a cell at the equator.

A flat 2D grid simply does not work on a sphere. The poles are special points; the east-west wrap is artificial; fluid dynamics (winds, ocean currents) produce wrong results.

An icosahedral grid eliminates these problems. The icosahedron is a 20-faced polyhedron that approximates a sphere with minimal distortion. Subdividing its triangular faces produces hexagons across the entire surface. Every hex has equidistant neighbors. There are no poles, no edges, no wrap-around.

### The Sphere Has No Edges

This addresses the "west/east edge" concern directly: **there are no edges.** The hex grid is the surface of a sphere. Every hex has neighbors all around it. When the user looks at a 2D rendered map, the apparent east/west edge is a rendering artifact of the projection — the underlying data wraps seamlessly because the sphere has no boundary.

A tectonic plate "moving east" doesn't reach an edge. It traverses neighbor relationships continuously around the sphere. From the simulation's perspective, "east" and "west" only exist as labels for direction vectors; the topology is genuinely spherical.

### The Pentagon Problem (Resolved)

Pure hexagons cannot tile a sphere. Mathematics requires exactly 12 pentagons among the hexagons. These appear at the 12 vertices of the underlying icosahedron and have 5 neighbors instead of 6.

This is handled with a single architectural commitment: **neighbor iteration is always variable-length.** Code never assumes 6 neighbors; it asks the grid "what are this hex's neighbors?" and receives either 5 or 6.

```rust
// Always done this way
for neighbor_id in grid.neighbors(hex_id) {
    // Works for both hexes (6 neighbors) and pentagons (5 neighbors)
}

// Never done this way
for i in 0..6 {  // ❌ Wrong assumption
    let neighbor_id = grid.neighbor(hex_id, i);
}
```

Algorithms that aggregate over neighbors (averaging temperature, propagating biomes, computing wind flow) naturally handle pentagons because they normalize by neighbor count: `sum / count` works whether count is 5 or 6.

Algorithms that have direction-specific logic (a plate moving in a specific direction across a pentagon) use the neighbor list with directional metadata: each neighbor is tagged with its direction vector, so the algorithm picks the closest direction match from however many neighbors exist.

The 12 pentagon locations are fixed and known at grid construction. Code that needs to special-case pentagons (rare) can identify them via `grid.is_pentagon(hex_id)`. For most algorithms, pentagons just work without special handling.

### Resolution and Subdivision

The grid has a base resolution (the macro grid) used for global simulation. Higher resolutions exist procedurally, generated on demand when the user zooms in.

The aperture-3 system means each hex subdivides into approximately 3 child hexes at the next resolution level. A child hex's properties are derived deterministically from its parent's properties plus noise functions seeded by the child's ID. This means zoom-in is reproducible — the same hex, zoomed to the same level, always produces the same finer detail.

The macro resolution is chosen to balance global simulation cost against geographic detail. The target is on the order of 100,000 hexes globally (final number is set in the Performance Budget Document). At this resolution, each hex represents roughly 5,000 square kilometers — coarse, but appropriate for plate tectonics and continental-scale climate.

Higher resolutions (regional, local) reach down to hexes representing a few hundred square meters, suitable for individual cities and local features.

## 5. Data Layer Architecture

The data layer is the heart of the system. Get this right and the rest is achievable. Get it wrong and the simulation will hang at year 100,000.

### Struct-of-Arrays, Not Array-of-Structs

The naive approach — one struct per hex cell containing all its data — produces catastrophic cache misses at scale. Genesis Engine uses Struct-of-Arrays: each property is its own flat array, indexed by hex ID.

```rust
// ❌ Avoided
struct HexCell {
    elevation: f32,
    temperature: f32,
    rainfall: f32,
    biome: BiomeType,
    // ... 40 other fields
}
let world: Vec<HexCell>;  // Cache misses everywhere

// ✅ Used
struct WorldData {
    elevation: Vec<f32>,      // All elevations together
    temperature: Vec<f32>,    // All temperatures together
    rainfall: Vec<f32>,       // All rainfalls together
    biome: Vec<BiomeType>,    // All biomes together
    // ...
}
// Processing one property across the world is a single cache-friendly sweep
```

This is the single most important optimization in the entire architecture. It's also what Bevy's ECS naturally encourages. The temptation to "just add a field to HexCell" must be resisted — fields become new arrays.

### ECS Components vs. Bulk Arrays

Most physical-layer data lives in bulk arrays indexed by hex ID. Bevy ECS components are used for entities that are inherently sparse or individuated: settlements, named characters, nations, named geographic features. The general rule:

- If every hex has a value, it's a bulk array.
- If only some hexes have a value, it's a component on an entity referencing the hex.

### Detailed Schema

The full ECS schema, including every field, unit, range, and default, is defined in the Data Layer Specification (Doc 04). This document establishes only the architectural pattern.

## 6. The Tick System

Different simulation layers operate at radically different time scales. Plate tectonics meaningfully changes on scales of tens of thousands of years. City populations change on scales of years. Running both at the same tick rate is wasteful at one end and impossible at the other.

Genesis Engine uses a **layered tick system**. Each layer has its own tick rate. Ticks are coordinated so that lower layers always advance before higher layers that depend on them.

### Era-Dependent Rates

Tick rates are not fixed — they vary by era. Early in a world's history, civilizational ticks don't exist because there are no civilizations. Tectonic ticks dominate. As the world matures, the relative balance shifts.

A simplified illustration of how tick rates shift across eras:

| Era | Tectonics | Climate | Biology | Civilization | Narrative |
|-----|-----------|---------|---------|--------------|-----------|
| Formation (0–500 Mya) | 10 Ky | 10 Ky | dormant | dormant | dormant |
| Geological (500 Mya–10 Mya before present) | 50 Ky | 10 Ky | 10 Ky | dormant | dormant |
| Prehistoric (10 Mya–10 Kya before present) | 100 Ky | 1 Ky | 1 Ky | 10 Ky | dormant |
| Ancient (10 Kya–2 Kya before present) | 100 Ky | 100 yr | 100 yr | 100 yr | 100 yr |
| Recent (2 Kya–present) | static | 10 yr | 10 yr | 1 yr | 1 yr |

These numbers are illustrative. Actual rates are defined in the Tick System Specification.

### Adaptive Buffering

The simulation runs *ahead* of the user's current viewing time. When the user pans the timeline, they're navigating already-computed state from the snapshot cache. When they reach the buffer's edge, the simulation extends the buffer.

Buffer depth adapts to event density: peaceful eras buffer deeply (the user will scrub through fast), eventful eras buffer shallowly (the user will linger and may intervene). Edit-mode pauses the user's clock and gives the engine free CPU to extend the buffer.

## 7. The Branching Model

Genesis Engine treats branching as a first-class operation. The world is not a single linear timeline; it is a tree of timelines diverging from intervention points.

### How Branches Work

Each save file contains a tree of branches. Each branch has:

- A parent branch (or null, for the root)
- A divergence point (year at which it forked from parent)
- Its own intervention log (changes made on this branch after divergence)
- Its own snapshot cache (computed states on this branch)

Branches are independent after divergence. Changes on a child branch never affect the parent. Two branches share their history up to the divergence point and have wholly independent futures after it.

### Branch Creation

A branch is created automatically when the user intervenes on the current branch and then continues simulation, *or* explicitly when the user requests a "fork here" action without yet making changes.

### Locality of Changes

Most interventions affect only the branch they're made on. This is true by definition: a Layer 2 intervention on Branch B at year 4M doesn't apply to Branch A because Branch A is a different timeline. Even the entity being modified may not exist on the parent branch.

### Single Branch Loaded at a Time

To keep memory and processing tractable, **only one branch is actively loaded at any moment.** Branch switching is an explicit operation that unloads the current branch's working state and loads the target branch from its snapshot cache.

This constraint has implications for comparison features. A "compare Branch A at year 4M vs Branch B at year 4M" view does not load both branches simultaneously. Instead, the system:

1. Loads Branch A, captures the relevant subset of state needed for the comparison (a render of the map, a list of nations, the relevant event log entries), unloads.
2. Loads Branch B, captures the equivalent subset, unloads.
3. Presents both captured subsets in the UI for comparison.

The comparison view shows *cached snapshots of comparison data*, not live simulation state from both branches. This is a deliberate constraint that trades some user experience polish for bounded memory footprint.

The captured comparison data is itself cached so repeated comparisons don't require re-loading. Invalidating the comparison cache happens when either branch is modified.

### Branch UX (Summary)

Detail lives in the Rendering & UI Specification. Architecturally, the system supports:

- Listing all branches with their divergence points
- Switching between branches with explicit load operations
- Comparison views built from sequentially captured snapshots
- Auto-generated divergence reports highlighting key differences

## 8. Save Format Philosophy

Save files contain three things, with very different sizes and characteristics:

**Seed + Parameters** (kilobytes): The deterministic recipe. World seed, planetary parameters, starting conditions, active mod manifest. Tiny, human-readable, version-controllable.

**Intervention Log** (kilobytes to megabytes): Every user change in temporal order, scoped to its branch. Tiny, human-readable, mergeable.

**Snapshot Cache** (gigabytes): Computed simulation states at various times across various branches. Large, binary, regenerable from the above two.

Sharing a world means sharing seed plus parameters plus intervention log plus mod manifest. The recipient regenerates the cache locally. This makes worlds extremely portable — a complete world that took hours to simulate can be transmitted as a small text file, provided the recipient has the same mods installed (or can fetch them).

The on-disk format details, including the **snapshot delta format** (whether snapshots store full states only, or full states at intervals plus deltas between them — a future optimization to be decided when implementation requires it), are deferred to the Save Format Specification (Doc 13).

## 9. Determinism Strategy

Determinism is a design pillar. The engineering implications are extensive enough to warrant their own specification (Doc 12), but the key architectural commitments are:

**Single source of truth for RNG.** Every randomized operation pulls from a named RNG stream derived from the master seed plus a stream identifier. This means changes to one subsystem's algorithm don't perturb other subsystems' outputs.

**Mod set affects effective seed.** The active mod manifest is hashed into the effective seed at world generation time. The same base seed with different mods produces different worlds — but the same base seed with the same mods always produces the same world. This preserves reproducibility under modding (see §10).

**Ordering is explicit.** When operations could be reordered (parallel iteration over hex cells, for instance), results are aggregated in deterministic order before being applied. Parallel execution speeds up the work; the *result* is identical to serial execution.

**Floating-point math is bounded.** Calculations that compound over many iterations (positions drifting over millions of years) use fixed-point arithmetic. Floating-point is used for ephemeral calculations within a tick. Detailed rules are in the Determinism Specification.

**Snapshot validation.** Test infrastructure includes "same seed produces same output" checks at multiple time points. Regression is caught immediately.

## 10. Mod System Architecture

Genesis Engine supports user modding from day 1. This is an architectural commitment that affects every content-bearing subsystem.

### What Modding Means Here

A mod is a collection of content definitions that extends or replaces the engine's default content. Examples of what users can mod:

- **Technology rules:** Define new technologies, change prerequisites, alter probabilities
- **Biomes:** Add new biome types with custom climate ranges and species affinities
- **Species traits:** Define new trait categories or modify existing ones
- **Naming rules:** Custom language-family naming patterns for cities, characters, nations
- **Starting parameters:** Preset world configurations (cold worlds, tidally-locked worlds, archipelago worlds)
- **Resources:** New mineral types, new agricultural products
- **Cultural traits:** New religions, ideologies, customs
- **Magic systems** (future): Entire magical frameworks as mods

The engine ships with a "core" content pack that defines the default ruleset. Users can disable parts of core and replace them with custom content.

### Architectural Implications

Supporting modding day 1 is a discipline, not a feature. The discipline:

**Everything content-bearing is data-driven.** Technology rules are external files, not Rust code. Biome definitions are external files. Species trait taxonomies are external files. Anywhere the engine has "content," that content lives in loadable files.

**The engine code is the runtime.** The engine knows how to evaluate rules, how to apply biome definitions, how to construct species from traits. It does not know what specific technologies, biomes, or traits exist. Those come from content packs.

**Content has stable identifiers.** Every content entry has a unique identifier (namespaced, e.g., `core:agriculture` or `medievalmagic:rune_smithing`). Save files reference content by identifier, not by index. This means worlds remain valid when content packs are updated, as long as identifiers are stable.

**Content is loaded at world generation time only.** Mods cannot change mid-simulation. Loading a saved world re-loads its mod manifest first; if a referenced mod is missing, the user is warned and given options (cancel load, load with substitutions, load and treat missing content as inert).

### Mod Manifest

Every world has a mod manifest: an ordered list of content packs active when the world was created. The manifest includes pack identifier, pack version, and (optionally) pack hash for integrity checking.

The manifest is part of the save file's seed-and-parameters section. It's included in the effective seed computation, so identical seeds with different mod manifests produce different worlds.

When a world is shared, its manifest goes with it. The recipient sees which mods are required, can install missing ones (manually or, eventually, through a mod registry), and then loads the world reproducibly.

### Conflict Resolution

When two mods modify the same content, the later mod in the load order wins by default. Mods can also declare explicit conflicts or dependencies in their metadata. The mod system surfaces conflicts to the user before world generation begins.

### Validation

Loaded content packs are validated before use:

- **Schema validation:** Content conforms to the expected format
- **Reference validation:** All referenced identifiers exist in loaded packs
- **Determinism validation:** Rules don't include non-deterministic operations (file I/O, system time, etc.)

Content packs that fail validation are reported with specific errors and not loaded.

### Mod-Aware Subsystems

Subsystems that consume content from the mod system include:

- **Biology** — biome definitions, species traits, ecosystem rules
- **Civilization** — cultural trait taxonomies, naming rules, religious frameworks
- **Technology** — tech tree rules, prerequisite definitions
- **Resources** — mineral types, agricultural products, trade goods
- **Climate** — climate type classifications (lightly moddable; physical constants are not)
- **Tectonics** — only world preset parameters are moddable; physics is fixed

Some subsystems are deliberately not moddable: core physics constants, hex grid topology, determinism rules. These belong to the engine, not to content.

### Modding UX

The user experience of modding is defined in detail elsewhere, but architecturally:

- Mods live in a known directory structure
- The application UI lists installed mods and their status
- Mods can be enabled/disabled per-world (not globally)
- Mod conflicts surface at world creation time with clear messages

Full details are in the Mod System Specification (a Tier 2 document to be written when modding implementation begins).

## 11. Rule Engine and Pluggable Rule Format

The Technology subsystem, and later the magic subsystem, the cultural-emergence subsystem, and user-authored mod content, all share a common need: data-driven rules that the engine evaluates.

A rule, at its simplest, looks like:

> *"Agriculture emerges in a region when: the region has a settled population of at least 500, the region's biome supports domesticable plants, climate stability over the last 200 years exceeds threshold X. Per-century probability: base 5%, modified by current cultural traits Y and Z."*

The engine needs to evaluate this rule against world state at appropriate ticks, apply effects when it fires, and do so deterministically.

The **pluggable rule format** is the file format and evaluation system for these rules. The decision between formats — declarative JSON/TOML, embedded scripting language (Lua, Rhai), or a custom DSL — is deferred to the Technology Rule System Specification (Doc 11). Each has trade-offs:

- **JSON/TOML:** Simple, safe, easy to validate. Limited expressiveness for complex conditions. Likely sufficient for most rules.
- **Rhai (Rust-native scripting):** More expressive. Sandboxed by design. Adds complexity to determinism guarantees.
- **Lua:** Mature, widely understood. Adds a foreign runtime to the engine.
- **Custom DSL:** Maximum control. Significant authoring burden.

Architecturally, the commitment is that *rules are external content, evaluated by an engine that doesn't know what specific rules exist.* The exact format will be selected when Doc 11 is written, with the constraints that it must be moddable, deterministic, and safe to evaluate against untrusted content.

## 12. Build Order

This section answers: *in what order should the subsystems be implemented?*

The dependency structure dictates much of this. Each subsystem depends on the ones below it. Implementing top-down is impossible (you can't simulate civilizations without a planet); implementing bottom-up matches both dependency order and validation needs (each layer's output is needed to test the next).

The phases below proceed strictly in order. Each phase must be functionally complete and tested before the next begins. "Functionally complete" means: the subsystem produces correct outputs for valid inputs, is covered by tests, and meets its specification's success criteria.

### Phase 0: Foundation

**Goal:** Project scaffold and core infrastructure exist. No simulation yet.

1. Project scaffold (Rust workspace, crate structure per Glossary §3.3)
2. Build system, formatting, linting, basic CI
3. Hex grid (DGGS) — coordinate system, neighbor lookups, pentagon handling
4. RNG / seed system — master seed, sub-seeds, deterministic streams
5. Data layer foundation — ECS setup, bulk array patterns, basic component types
6. Tick system skeleton — multi-layer tick coordinator, era handling
7. Mod system foundation — content loader, manifest, validation framework
8. Save/load skeleton — seed + parameters + intervention log format (no snapshots yet)
9. Testing infrastructure — determinism test framework, snapshot test framework
10. Headless run mode — the engine must be runnable without a UI, for tests

**Exit criteria:** An empty world can be generated from a seed, ticks can advance, content packs can load, and the same seed produces identical empty-world states deterministically.

### Phase 1: Geology Prototype

**Goal:** Validate the riskiest assumption — that multi-scale simulation can run at acceptable speed on target hardware.

1. Plate tectonics — plate generation (growth-based seeding, major + minor plates), drift via Euler-pole rotation, collisions, divergence, subduction
2. Mountain building and volcanism (boundary-driven and hot-spot)
3. Bedrock typing (igneous, oceanic crust, sedimentary, metamorphic) — limestone deferred to Phase 4 (Biology) since carbonate rock requires biological deposition
4. Per-hex fertility tracking (monotonic accumulator) for shallow tropical seas — feeds into Phase 4 biome and bedrock decisions and Phase 5 civilization placement
5. Erosion with climate-feedback hook (uniform in Phase 1, climate-aware in Phase 2)
6. Plate reorganization events (split, merge, motion change) for varied geological history
7. Event emission with user-tunable granularity (Trace through Pivotal)
8. Performance benchmarking against Performance Budget targets

See Doc 06 for full specification.

**Exit criteria:** A 4-billion-year geological history can be generated and replayed deterministically on target hardware within the performance budget. If this exit criterion fails, the project's viability is in question and architectural rethinking is needed before continuing.

**Deferred to future docs (see Doc 01 §9.5):**
- Planetary formation/cooling sequence (pre-tectonic; pairs with Doc 07 Climate)
- Origin of life mechanism (Doc 09 Biology)
- Chaos mode (own future doc)

### Phase 2: Climate and Hydrology

**Goal:** A complete physical Layer 0.

1. Solar incidence, atmospheric cells, prevailing winds
2. Orographic precipitation
3. Ocean currents (simplified)
4. Temperature distribution
5. River formation via erosion algorithms
6. Soil composition derived from bedrock + climate + organic accumulation

**Exit criteria:** A world has plausible climates, rivers form where they should, and the Cretaceous-beach mechanic produces fertile soil from ancient marine bedrock.

### Phase 3: Rendering MVP

**Goal:** The user can see the world. This is intentionally inserted before biology so that further development is visible.

1. Map rendering — equirectangular projection
2. Style modes — cartographer, biome, satellite, heatmap (one or two variables initially)
3. Pan, zoom, basic UI chrome
4. Time-scrubbing UI (no editing yet)

**Exit criteria:** A user can generate a world, view it as a cartographer-style map, scrub through its geological history, and toggle between style modes.

### Phase 4: Biology

**Goal:** Life appears on the world.

1. Biome propagation from climate + soil
2. Procedural species generation
3. Trophic aggregates
4. Lotka-Volterra ecosystem dynamics
5. Extinction events and adaptive radiation
6. Phylogenetic tree tracking

**Exit criteria:** A world has biomes that match its climate, species that fit their biomes, and ecosystems that respond to perturbations.

### Phase 5: Civilization (Pre-Branching)

**Goal:** Civilizations emerge and develop on a single timeline. No user editing yet.

1. Settlement spawning based on carrying capacity
2. Population dynamics
3. Nation formation and territorial control
4. Cultural drift, language families
5. Trade networks
6. Conflict
7. Technology rule engine and initial tech corpus
8. Religion and ideology

**Exit criteria:** A world generates a full simulated history with named places, named nations, languages, cultures, technologies. The event log captures the major beats of that history.

### Phase 6: Branching and Interventions

**Goal:** The delight test becomes possible.

1. Branch data structures and switching
2. Snapshot cache implementation
3. Edit mode UI
4. Intervention authoring and serialization
5. Intervention scope rules and cascade logic
6. Branch comparison views
7. Branch divergence reports

**Exit criteria:** A user can fork a timeline, intervene, simulate forward, and compare branches at chosen time points.

### Phase 7: Export

**Goal:** Authors can take their worlds and use them.

1. Timeline chronicle export (markdown, PDF)
2. Gazetteer
3. Bestiary
4. High-resolution map export (multiple projections, multiple styles)
5. Setting bible compilation

**Exit criteria:** A user can export their world as a set of documents usable in writing or game-prep workflows.

### Phase 8: Polish and Performance

**Goal:** Genesis Engine v1 is releasable.

1. Performance optimization against targets
2. UI polish
3. Onboarding flow and tutorial
4. Documentation for users
5. Mod authoring documentation
6. Error handling and recovery
7. Save file migration tooling

**Exit criteria:** Genesis Engine v1 ships.

### Notes on Build Order

**Not strictly serial.** Some work parallelizes within a phase (rendering and biology development can overlap once Phase 2 is complete). The phase boundaries mark dependency gates, not strict serialization.

**Modding from Phase 0.** Mod support is foundational. Each subsequent phase implements its content as data-driven from the start. This is significantly easier than retrofitting modding later.

**Testing throughout.** Each phase includes its own tests. The testing infrastructure from Phase 0 supports all subsequent phases.

**Specs precede implementation.** Each subsystem's Tier 2 specification is written before that subsystem's implementation begins. Specs are not retrofitted to code.

## 13. Performance Architecture

Performance is achieved through architecture, not optimization. The patterns:

**Data-oriented design** (already covered) eliminates cache misses on the critical path.

**Level-of-detail simulation.** The world is simulated at coarser resolution far from user focus, finer resolution near user focus. Zoom-in triggers procedural subdivision of the focused region.

**Parallel processing.** Bulk operations over hex arrays parallelize naturally across CPU cores. Bevy provides the job system.

**Snapshot caching.** Time-scrubbing reads from cache rather than re-simulating.

**Adaptive tick rates.** Eras with little activity (formation, deep geology) advance with large ticks. Eras with activity advance with small ticks. Average computational load is bounded.

**Statistical aggregates over individual agents.** Populations are statistical aggregates by default. Individual people only manifest when the user zooms in. The whole world is not simulating individual humans.

Hard numerical performance targets are defined in the Performance Budget Document (Doc 18).

## 14. Module Interface Contracts

Every simulation module exposes:

**Inputs:** Specific data layer fields it reads. Documented in the module spec.

**Outputs:** Specific data layer fields it writes. Documented in the module spec.

**Tick interface:** A function signature `advance(world, dt, rng) → events`. The module produces simulation events as part of its output.

**Parameters:** External configuration the user can adjust. Documented and exposed in the UI.

**Content dependencies:** Content packs the module consumes (biome definitions, tech rules, etc.) and how it handles missing content.

**Tests:** Unit tests for individual algorithms, integration tests for module behavior, snapshot tests for determinism.

This contract is what makes AI-driven implementation feasible. An AI session implementing the Climate module needs to know what data Tectonics provides and what Biology consumes — not how Tectonics or Biology work internally.

## 15. Rendering Architecture

The rendering layer is downstream of the simulation. It reads from the data layer but never writes to it. This separation is strict.

**Multiple projections.** The same spherical hex data can be projected as equirectangular world map, orthographic globe view, or unfolded icosahedral net. Projection is a rendering concern; the underlying data is identical.

**Multiple style modes.** Cartographer-style parchment map, political map with national color fills, biome scientific coloring, satellite-style elevation rendering, single-variable heatmaps. Each mode is a different shader/render pass over the same data.

**Sprite system.** A small library of cartographic symbols (mountains, trees by biome, settlements by size, special features) rendered onto the map. Sprites are color-shifted based on context, minimizing the number of assets needed.

**Zoom level transitions.** The user can zoom from planetary view down through continental, regional, and local views. Each transition reveals more detail without re-simulating — detail is procedurally manifested from the simulation's aggregate data.

Full rendering details are in the Rendering & UI Specification (Doc 14).

## 16. Anti-Goals (Architectural)

What this architecture explicitly avoids:

- **No global object graph.** No "World" object holding pointers to "Continent" objects holding pointers to "City" objects. Everything is flat arrays and ECS components.
- **No engine-coupled simulation.** Simulation logic does not depend on Bevy. Bevy is used for ECS infrastructure, rendering, and windowing. Simulation could in principle run headless.
- **No hardcoded content.** No "if it's a desert, then..." special cases. Behaviors emerge from rules operating on data. Content is moddable from day one.
- **No real-time pressure on user.** The simulation does not require the user to react in real-time. Edit mode pauses the clock; the user can take all the time they need.
- **No hidden state.** Every meaningful simulation state is in the data layer, snapshot-able, inspectable, and exportable. There is no "hidden" computation that affects outcomes invisibly.
- **No multi-branch concurrent loading.** Only one branch is in active memory at a time. Comparison features work via cached snapshots, not parallel loaded branches.

---

*End of Architecture Overview.*
