# Genesis Engine — Glossary & Naming Conventions

**Document Type:** Tier 1 — Foundational
**Status:** Draft v0.1
**Last Updated:** May 2026
**Owner:** Brax Johnson

---

## 1. Purpose of This Document

Consistent terminology is essential for a project where AI coding agents are doing implementation across many sessions. Different sessions will not remember what the last session called something. Inconsistent naming produces inconsistent code, which produces bugs.

This document defines the canonical term for every concept in Genesis Engine. When writing specs, code, comments, or UI text, use the term defined here. When the AI proposes a different name, correct it to the canonical name.

This document is **living** — terms are added as new concepts emerge. When a new concept is introduced in any specification, its term is added here.

## 2. How to Use This Document

When starting an AI coding session: paste or reference relevant sections of this glossary in the context. The AI should adopt these terms verbatim.

When reviewing AI output: check that terminology matches this document. Flag deviations.

When you (Brax) notice ambiguity: add or refine an entry. Better to over-specify than under-specify.

## 3. Naming Conventions for Code

### Rust Identifiers

- **Types (structs, enums, traits):** `PascalCase` — e.g., `HexCell`, `BiomeType`, `InterventionEvent`
- **Functions and methods:** `snake_case` — e.g., `advance_tick`, `get_neighbors`, `compute_rainfall`
- **Constants:** `SCREAMING_SNAKE_CASE` — e.g., `MAX_PLATES`, `EARTH_GRAVITY`
- **Modules:** `snake_case` — e.g., `tectonics`, `climate`, `hex_grid`
- **Generic type parameters:** Single uppercase letter or short `PascalCase` — e.g., `T`, `Rng`, `Layer`

### File Naming

- Source files: `snake_case.rs`
- Specification documents: `NN-document-name.md` where NN is the document number
- Test fixtures: `snake_case.json` or `snake_case.bin`
- Save files: User-chosen names with `.gen` extension

### Module Organization

Each major simulation module is a top-level crate or module:

- `genesis_core` — Core infrastructure (hex grid, time, RNG, data layer)
- `genesis_mods` — Mod system, content loader, rule engine
- `genesis_tectonics` — Tectonics simulation
- `genesis_climate` — Climate simulation
- `genesis_hydrology` — Hydrology and soil
- `genesis_biology` — Biology and ecosystems
- `genesis_civilization` — Civilization simulation
- `genesis_tech` — Technology rule system
- `genesis_render` — Rendering and visualization
- `genesis_ui` — User interface
- `genesis_export` — Document export
- `genesis_app` — The application binary that wires everything together

## 4. Core Terminology

### World & Geography

**World** — A complete simulated planet. One save file contains one world (which may have many branches).

**Planet** — Used interchangeably with "world" in user-facing text. In code, prefer "world."

**Hex / Hex Cell** — A single discrete cell on the planet's surface. The atomic unit of geographic data. Indexed by `HexId`. Always called "hex" in code, never "tile" or "cell" or "square."

**HexId** — Unique integer identifier for a hex. Used to index into all bulk data arrays.

**Pentagon** — One of the 12 cells at icosahedron vertices that has 5 neighbors instead of 6. Pentagons are hexes for naming purposes ("a hex with 5 neighbors"), but their pentagonal nature must be handled in neighbor-iteration code.

**Neighbor** — An adjacent hex sharing an edge. Hexes have 6 neighbors; pentagons have 5.

**Grid** — The full set of hexes covering the planet. The grid is fixed at world generation and never changes structurally.

**Resolution** — The subdivision level of the grid. Higher resolution means smaller hexes and more of them. The macro grid is the lowest resolution used for global simulation. Higher resolutions are procedurally manifested on zoom-in.

**Region** — A user-meaningful geographic area, typically multiple connected hexes sharing some property (a desert, a watershed, a nation). Not a fixed data structure; defined contextually.

**Continent** — A connected landmass at the macro resolution. Detected algorithmically from elevation data.

### Time

**Tick** — A single advance of one simulation layer by its current tick interval. "Tectonics tick" or "civilization tick."

**Tick Rate** — How many simulated years pass per tick for a given layer in a given era. Varies by layer and by era.

**Era** — A named period of simulation history with characteristic tick rates: Formation, Geological, Prehistoric, Ancient, Recent. The era determines tick rates for each layer.

**Year** — One Earth-year of simulated time. The base unit of time in user-facing displays. (Worlds may have different orbital periods, but we standardize on Earth-years for display unless the user requests otherwise.)

**Snapshot** — A saved state of the simulation at a specific (year, branch) point. Used for time-scrubbing and rollback.

**Snapshot Interval** — How often snapshots are taken. Varies by era — coarser for old eras, finer for recent eras.

**Buffer** — Pre-computed simulation results ahead of the user's current view point. The buffer extends in time, not in space.

### Branches & Interventions

**Branch** — An independent timeline. Save files contain a tree of branches.

**Root Branch** — The original, pre-intervention timeline generated from the seed alone.

**Branch Point** / **Divergence Point** — The (parent branch, year) where a child branch forks from its parent.

**Intervention** — A user-initiated change to the world. Every intervention is a discrete, serializable event.

**Intervention Log** — The append-only list of interventions on a given branch.

**Intervention Scope** — How far an intervention can propagate. Defined values: `Local` (one hex or one entity), `Regional` (a contiguous area), `Global` (planet-wide). Most interventions are Local.

**Cascade** — The downstream simulation effects of an intervention propagating through later ticks. Cascades happen via normal simulation, not via special-case code.

### Determinism

**Seed** — The master integer that determines all randomization. Same seed plus same parameters plus same interventions equals same world.

**Sub-seed** — A seed derived from the master seed plus a stream identifier. Used so that different modules' randomization is independent. Changing the Climate algorithm shouldn't perturb Tectonics outputs.

**RNG Stream** — A named random number generator instance derived from a sub-seed. E.g., "tectonics.plate_assignment" or "biology.species_traits."

**Determinism Test** — A test that verifies same inputs produce same outputs across runs and machines.

### Data Architecture

**ECS** — Entity Component System. The architectural pattern Genesis Engine uses. Bevy provides the implementation.

**Entity** — An identified object in the ECS, typically used for individuated things like settlements or nations.

**Component** — A data type attached to an entity.

**System** — A function that operates on entities with specific components. In Genesis Engine, simulation logic is implemented as systems.

**SoA / Struct-of-Arrays** — Data layout where each field is its own flat array. Used for hex-keyed data. Contrast with AoS (Array-of-Structs).

**Bulk Array** — A flat array indexed by HexId, holding one field's value for every hex. The default storage for hex-level data.

### Physical Layer

**Elevation** — Height above (or depth below) sea level for each hex, in meters.

**Bedrock Type** — The category of underlying rock for each hex. Influences soil composition. Enum: `Igneous`, `Sedimentary`, `Metamorphic`, with subtypes for specific compositions (limestone, granite, etc.).

**Plate** — A tectonic plate. Each plate has a unique ID, a velocity vector, a type (oceanic or continental), and a set of hexes currently belonging to it.

**Soil Fertility** — A derived value indicating how productive a hex's soil is for agriculture. Computed from bedrock, climate, and organic accumulation.

**Biome** — A classification of a hex's ecological character: temperate forest, tropical rainforest, tundra, desert, etc. Enum with named variants.

**Precipitation** — Annual rainfall in millimeters for each hex. Output of the climate simulation.

**Temperature** — Annual mean temperature in degrees Celsius for each hex. May also track seasonal range.

### Biological Layer

**Species** — A procedurally generated organism type. Each species has traits (size, diet, climate tolerance, etc.).

**Trophic Aggregate** — A regional pool representing total biomass at a trophic level (large herbivores, small game, apex predators, etc.). Used for ecosystem dynamics without simulating individual organisms.

**Carrying Capacity** — The maximum sustainable population for a region given its biome and resources.

**Phylogenetic Tree** — The tree of evolutionary descent for all species in the world. Stored as parent-child relationships.

**Endemism** — A species being present only in a specific isolated region.

### Civilizational Layer

**Settlement** — A populated place. Categorized by size: Hamlet, Village, Town, City, Metropolis.

**Population** — The number of people in a settlement. Aggregated; individuals are not simulated globally.

**Nation** — A political entity controlling territory. May contain multiple settlements.

**Culture** — A set of cultural traits associated with a population. Includes language family, religion, customs.

**Language Family** — A group of related languages descended from a proto-language.

**Religion** — A belief system with associated traits and a population that practices it.

**Technology** — A capability unlocked by a society. Technologies are defined by rules in the Technology Rule System.

**Tech Rule** — A rule defining how a technology can emerge, its prerequisites, its probability, and its effects.

**Trade Route** — A connection between settlements along which goods flow. Affects both endpoints' economies.

### Narrative Layer

**Event** — A discrete occurrence in simulated history. Has a year, location, type, and structured payload.

**Event Log** — The full chronological list of events for a branch. The basis for timeline exports.

**Chronicle** — An exported, formatted version of the event log. May be filtered (by region, by era, by topic).

**Named Entity** — An entity with a generated proper name (a city, nation, character). Names are generated from language-family naming rules.

### Rendering & UI

**Projection** — A mapping from spherical hex data to 2D screen coordinates. Available: Equirectangular, Orthographic, Icosahedral Net.

**Style Mode** — A rendering style for the map. Available: Cartographer, Political, Biome, Satellite, Heatmap (with selectable variable).

**Layer Toggle** — A UI control to show/hide a data layer overlay (e.g., trade routes, climate zones).

**Zoom Level** — Discrete levels of detail: Planetary, Continental, Regional, Local. Each level reveals more procedurally generated detail.

**Sprite** — A small graphical asset (mountain symbol, tree symbol, settlement icon) overlaid on the map.

**Edit Mode** — A UI state where the simulation is paused and the user is preparing interventions. Confirming an edit creates a branch.

### Export

**Export** — Generating a user-readable document from simulation data.

**Timeline Export** — A formatted chronicle of events.

**Gazetteer** — A reference document listing geographic features and settlements.

**Bestiary** — A reference document listing species.

**Setting Bible** — A compiled, comprehensive reference document covering the whole world.

**Map Export** — A high-resolution image of the map in a chosen projection and style mode.

### Modding

**Mod / Content Pack** — A user-authored or third-party collection of content definitions loaded by the engine. The two terms are used interchangeably; "content pack" is preferred in technical contexts, "mod" in user-facing contexts.

**Core Pack** — The default content pack shipped with the engine. Other packs extend or override its content.

**Mod Manifest** — The ordered list of content packs active for a given world. Stored with the save file's seed-and-parameters section.

**Content Identifier** — A namespaced unique identifier for a content entry, e.g., `core:agriculture` or `medievalmagic:rune_smithing`. The string before the colon is the namespace (typically the pack identifier); the string after is the entry identifier.

**Namespace** — The portion of a content identifier identifying its source pack. Prevents identifier collisions between packs.

**Rule** — A data-driven content entry that the rule engine evaluates against world state. Used by technology, magic, and other emergence systems.

**Rule Engine** — The generic evaluator that processes rules against world state. Does not know what specific rules exist; only how to evaluate the rule format.

**Conflict** — A situation where two mods modify the same content entry. Resolved by load order (later wins) or by explicit mod metadata.

**Validation** — The process of checking a loaded content pack for schema correctness, reference correctness, and determinism compliance.

**Effective Seed** — The world seed combined with the mod manifest hash. Used for all randomization. Different mod manifests with the same base seed produce different effective seeds and different worlds.

## 5. Terms to Avoid

Specific words that have caused confusion and should not be used in Genesis Engine context:

- **"Tile"** — Use "hex." "Tile" implies a square grid.
- **"Cell"** (as a synonym for hex) — Use "hex." "Cell" is too generic.
- **"World tick"** — Use the specific layer: "tectonics tick," "civilization tick." There is no single global tick.
- **"Player"** — Use "user." There is no player in the gameplay sense.
- **"Game"** — Use "simulation" or "engine" in technical contexts. "Tool" or "application" in user-facing contexts. Genesis Engine is not a game.
- **"Map"** — Acceptable in user-facing UI but ambiguous in code. In code, prefer "world," "grid," "render," or "projection" as appropriate.
- **"Tree of life"** — Use "phylogenetic tree" in code and specs. "Tree of life" is fine in user-facing text.
- **"Save game"** — Use "save file" or "world file."
- **"Reset"** — Ambiguous. Use specific actions: "regenerate," "discard branch," "return to root."

## 6. User-Facing vs. Code-Facing Names

Some concepts have a technical name (used in code and specs) and a user-facing name (used in UI and documentation). When both exist, the technical name appears first.

| Technical | User-Facing |
|-----------|-------------|
| Hex | (not exposed in UI) |
| Branch | Timeline |
| Intervention | Change / Edit |
| Snapshot | (not exposed in UI) |
| Trophic Aggregate | Ecosystem |
| Soil Fertility | Land Productivity |
| Bedrock Type | Geology |
| Phylogenetic Tree | Tree of Life |
| Tech Rule | (not exposed in UI) |
| Style Mode | Map Style |
| Era | Age / Era |

## 7. Reserved Names

The following names are reserved for system use and should not be used as user-supplied names for things like cities or nations:

- Names beginning with "Branch_"
- Names beginning with "Snapshot_"
- The strings "Root," "Default," "Untitled"
- Names containing characters reserved for file paths or save format delimiters

User-supplied names will be validated and sanitized; reserved names are rejected with a clear error.

## 8. Versioning of This Document

Because this is a living document, every addition or change should be reflected in the version number and the Decision Log (Doc 19) should reference significant additions.

When the AI proposes a term not in this document, the protocol is:

1. Check if an existing term covers the concept; if so, use it.
2. If a new term is genuinely needed, the author adds it to this document explicitly.
3. The AI is not authorized to invent terminology unilaterally.

---

*End of Glossary & Naming Conventions.*
