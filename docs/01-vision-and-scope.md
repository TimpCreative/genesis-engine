# Genesis Engine — Vision & Scope

**Document Type:** Tier 1 — Foundational
**Status:** Draft v0.1
**Last Updated:** May 2026
**Owner:** Brax Johnson

---

## 1. What Genesis Engine Is

Genesis Engine is a deterministic worldbuilding simulator. It generates fictional planets from physical first principles — plate tectonics, climate, hydrology, biology — and then simulates the emergence and history of life and civilization on those planets across geological timescales.

It is not a game in the traditional sense. There are no win conditions, no scoring, no opponent. It is a tool for authors, worldbuilders, and curious hobbyists who want to generate worlds with the depth and internal consistency that emerge only when deep history actually informs the present.

The central design conviction: **every interesting thing about a world should be a consequence of something else, not a fiat declaration.** If a region has fertile soil, it's because of bedrock from an ancient sea. If a civilization rose there, it's because the soil supported it. If that civilization built a trade empire, it's because of resources and geography. The chain of causality is the product.

## 2. Who Genesis Engine Is For

**Primary audience:** Authors and serious worldbuilders. People building fictional worlds for novels, tabletop RPGs, video games, or pure creative pleasure who care about internal consistency and depth.

**Secondary audience:** Simulation hobbyists. People who enjoy watching emergent systems unfold and experimenting with parameters.

**Tertiary audience (deferred):** Educators and students of geography, geology, anthropology — people who would find value in a teaching tool that demonstrates how deep history shapes present conditions.

**Not the audience:** Players seeking gameplay loops, strategic challenges, or action. This is not a 4X game. There is no Civilization VI here.

## 3. Design Pillars

These are the load-bearing convictions of the project. Every feature decision should be evaluated against them.

**Determinism is sacred.** Same seed plus same interventions equals the same world, every time, on every machine. This enables sharing, reproducibility, and trust in the simulation.

**Emergence over scripting.** Behaviors should arise from rule interactions, not be hardcoded. Civilizations cluster on fertile soil because the simulation weighs fertility, not because we wrote "civilizations cluster on fertile soil."

**Depth without ceiling.** The data-driven rule system means the simulation can extend indefinitely — into magic, into far-future tech, into species we haven't imagined — by adding rules, not rewriting the engine.

**Branching is first-class.** Users can fork timelines and compare alternate histories. This isn't a feature; it's a fundamental property of how the simulation works.

**Editability with intelligent scope.** Users can change anything in their world. Changes propagate to downstream systems naturally but do not retroactively rewrite physics. A boost to one city's food supply is not allowed to invalidate global tectonics.

**Modding is a first-class capability.** Users can author content packs that extend or replace engine content — technology rules, biomes, species traits, naming systems, eventually magic systems. The engine is the runtime; the content is the world. This is supported from day one, not retrofitted later.

**The data is the product, the visuals are the interface.** Genesis Engine succeeds if it produces compelling worlds, even with mediocre visuals. It fails if it produces beautiful but shallow worlds. Render budget follows simulation budget.

## 4. The Delight Test

The moment Genesis Engine is working: a user runs a simulation, watches a continent form and life evolve and civilizations rise. They rewind 50 million years, place a meteor, and let the simulation run forward to the same future date. They compare the two timelines side by side and see that one continent now has a desert kingdom where the other has a maritime trading empire — and the causal chain is fully reconstructible.

If users do this and tell their friends about it, Genesis Engine has succeeded.

## 5. Explicit Non-Goals

Naming what we are *not* building is as important as naming what we are.

- **Not a real-time strategy game.** No combat micromanagement, no player-controlled factions, no tech tree to "complete."
- **Not a graphically impressive 3D world.** 2D cartographer-style maps, multi-projection support, clean and beautiful but not photorealistic.
- **Not a multiplayer experience.** Single-user. Worlds are shared as files, not via servers.
- **Not a perfect scientific simulation.** Plausible, internally consistent, and emergent — but optimized for narrative interest and computational feasibility, not academic accuracy.
- **Not a generative AI product.** The simulation is deterministic and entirely classical. LLMs may, far in the future, help generate narrative prose from event logs, but they are not part of the simulation core.
- **Not browser-based.** Desktop application only for the foreseeable future. Performance requirements likely preclude web deployment.
- **Not open source.** Proprietary for the foreseeable future. May be reconsidered after launch.

## 6. Success Criteria

Genesis Engine v1 is successful if:

- A user can generate a world with full geological, climatic, biological, and civilizational history in reasonable time on a modern laptop.
- The simulation is reproducibly deterministic across runs and machines.
- Users can fork timelines and compare branches.
- Users can intervene at any level without breaking the simulation.
- The world can be exported as documents (timeline, gazetteer, bestiary, maps) that authors can actually use in their work.
- Demonstrable causal chains exist — users can trace why a region looks the way it does back through deep history.
- Users can author mods that extend the engine's content, and shared worlds remain reproducible when their required mods are present.

## 7. Technical Foundation Summary

These choices are documented in detail in the Architecture Overview. Listed here so the Vision doc stands alone.

- **Language:** Rust
- **Engine/Framework:** Bevy (ECS + rendering)
- **Grid:** Icosahedral hex grid (ISEA3H) — no edges, minimal polar distortion
- **Data Layout:** Struct-of-Arrays for cache-friendly bulk processing
- **Time Model:** Layered tick system with appropriate scales per simulation layer
- **Determinism:** Seed-based with explicit RNG management
- **Save Format:** Seed + parameters + intervention log (compact, shareable, regenerable)
- **Platform:** Desktop (Windows / macOS / Linux)

## 8. Project Roadmap Philosophy

Genesis Engine is being built as a long-term passion project, not a venture-backed startup. There is no shipping deadline, no investor pressure. The project will progress at sustainable pace and ship when it's ready.

Development proceeds module by module. Each module has a specification document before implementation begins. Each module has comprehensive tests before being considered complete. Each module is independently usable to the degree possible. The project is always in a working state.

Riskiest assumptions are validated first via prototypes. The single biggest risk is multi-scale simulation performance, so an early prototype will validate that plate tectonics simulation can run at acceptable speed on target hardware before further architecture is committed.

AI coding agents do the implementation. The author provides specifications, architectural decisions, design judgments, and review. This shapes documentation: specs must be precise enough for an AI to implement correctly with minimal guesswork.

## 9. Companion Documents

Genesis Engine is documented across twenty specifications and reference documents. Each is summarized here so the project structure is visible from a single starting point.

### Tier 1 — Foundational

These exist before any code. They establish vision, architecture, and shared language.

**01. Vision & Scope** (this document) — The "what and why" of Genesis Engine. Audience, motivation, pillars, non-goals, success criteria. The constitutional document of the project.

**02. Architecture Overview** — The high-level technical blueprint. Module list, data flow, layer model, tick system, branching model, save format philosophy. The map of how all systems connect.

**03. Glossary & Naming Conventions** — Project-wide terminology and naming standards. Ensures consistency across documents and across AI coding sessions. Grows over time as new terms are introduced.

### Tier 2 — System Specifications

One per simulation subsystem. Written immediately before implementing that subsystem. Each is the contract the implementation must satisfy.

**04. Data Layer Specification** — The ECS schema. Every component, every field, every unit and range. The hex grid definition. The branch and intervention log formats. The data contract everything else builds against.

**05. Tick System & Time Management Specification** *(absorbed into Doc 04 §7)*

The tick system, layer ordering rules, `WorldYear`/`Era`/`WorldTime` types, and the main lifecycle loop (`create_world`, `generate_full_history`) are specified in Doc 04 §7 and implemented in `genesis_core::time` and `genesis_core::lifecycle`. A separate document proved unnecessary — the tick system is small enough to specify alongside the data layer.

Snapshot intervals, buffer management for branch rewinding, and edit-mode behavior during simulation will be specified in a future doc (likely paired with the persistence layer, Doc 13) once Phase 1+ surfaces concrete requirements. Stubbed snapshot infrastructure already exists in `genesis_core::persistence::snapshots`.

**06. Tectonics Module Specification** — Plate generation, drift dynamics, collision and subduction, mountain building, volcanism, geological layering. Produces the elevation field and bedrock composition the rest of the simulation builds on.

**07. Climate Module Specification** — Solar incidence, atmospheric cells, prevailing winds, orographic precipitation, ocean currents, temperature distribution, seasonal variation. Depends on tectonic outputs.

**08. Hydrology & Soil Specification** — River formation via erosion algorithms, lake and delta dynamics, soil composition derived from bedrock plus climate plus accumulated organic material. Where the "fertile ancient seabed" mechanic lives.

**09. Biology Module Specification** — Procedural species generation, biome propagation, trophic aggregates, Lotka-Volterra dynamics for predator-prey populations, extinction events, adaptive radiation when landmasses isolate.

**10. Civilization Module Specification** — Settlement spawning, population dynamics, nation formation, cultural drift, language families, religion, trade networks, conflict. The largest single specification.

**11. Technology Rule System Specification** — The data-driven rule engine that handles technological emergence. Rule definition format, prerequisite evaluation, probability calculation. Plus the initial corpus of technology rules. Also the foundation for future magic systems and far-future tech.

**11a. Mod System Specification** — The content pack format, loading and validation pipeline, mod manifest schema, conflict resolution rules, namespace conventions, mod-aware seed computation. Foundational because modding is supported from day one; written alongside Phase 0 implementation.

### Tier 3 — Cross-Cutting Concerns

Systems that touch many modules. Written early because they constrain everything else.

**12. Determinism & Reproducibility Specification** — RNG strategy, fixed-point versus floating-point math rules, ordering guarantees, testing strategy for determinism. The non-negotiable foundation that makes the whole project trustable.

**13. Save Format & Intervention Log Specification** — On-disk file formats, versioning and migration strategy, the seed-plus-interventions sharing format, branch tree representation.

**14. Rendering & UI Specification** — Map projections, style modes (cartographer, political, biome, satellite, heatmap), zoom level transitions, sprite system, UI layout, intervention interaction patterns, branch navigation interface.

**15. Export Specification** — Output document formats for timeline chronicles, gazetteers, bestiaries, map exports at various resolutions, compiled "setting bibles" for authors to use in their work.

### Tier 4 — Development Process

These define how Genesis Engine gets built, not what it is.

**16. Testing & Validation Strategy** — What gets unit tested versus integration tested versus snapshot tested. Per-module test requirements. Visual regression testing approach. Performance benchmark methodology.

**17. AI Coding Collaboration Guide** — How to work with AI coding agents on this project specifically. Per-session setup, what context to provide, how to spec a task at the right granularity, review checklist, common pitfalls. Specific to this project's reality: the author is not writing the code.

**18. Performance Budget Document** — Hard numerical targets. Target hardware, maximum simulation cells per layer, target simulation years per real-time second by era, maximum memory footprint, maximum save file size, maximum startup and load times. The objective standard new features are evaluated against.

### Tier 5 — Living Documents

Updated continuously throughout development.

**19. Decision Log** — Every significant architectural decision with date, context, alternatives considered, choice made, and rationale. Append-only. The institutional memory of the project — answers "why did we do X" without requiring archaeology.

**20. Roadmap & Risk Register** — Current priorities, known risks with mitigations, near-term and far-term goals. The operational document that drives what gets worked on next.

## 10. The Author's Stake

Genesis Engine is a passion project. It is being built because it would be cool to exist, because no existing tool does what it should do, and because the author wants to use it for his own writing. It will progress as time allows alongside other commitments. Its success is measured by whether it eventually produces worlds worth exploring, not by timeline or revenue.

This is the constitutional commitment to long-term thinking: the project is built right, not fast.

---

*End of Vision & Scope.*