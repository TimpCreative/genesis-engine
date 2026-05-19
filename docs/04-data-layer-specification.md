# Genesis Engine — Data Layer Specification

**Document Type:** Tier 2 — System Specification
**Status:** Draft v0.4
**Last Updated:** May 2026
**Owner:** Brax Johnson
**Implementing Phase:** 0 (Foundation)

**Changelog:**
- v0.4 (May 2026): Clarified §5.1 that `parameters` field is added in step 4. Added sentinel constant documentation to §5.3 (`PlateId::NONE`, `BiomeId::NONE`). Corrected §16 file organization to reflect actual code layout (`Direction` lives in `grid::ids`, not `data::enums`).
- v0.3 (May 2026): Rewrote §3.3.1 to describe Vince/Kristensen topological neighbor scheme (replacing geometric k-NN approach). Updated §3.4 to reference topological construction. Added algorithm source citations.
- v0.2 (May 2026): Extended cell count table to levels 0-4. Added §3.3.1 (Class I/II parity) and §3.3.2 (geodesic vs. Snyder projection). Reflects clarifications surfaced by step 1 implementation.
- v0.1 (May 2026): Initial draft.

---

## 1. Purpose of This Document

This specification defines the data layer of Genesis Engine — the structures, schemas, and conventions that every other subsystem builds on. It is the contract between all simulation modules.

Specifically, it defines:

- The hex grid model, coordinate system, and physical scaling
- The `WorldParameters` data structure (immutable world recipe)
- Per-hex bulk array fields (the macro grid's properties)
- ECS components for sparse/individuated entities
- Time representation
- Event log structure
- Branch and intervention log structure
- Save file formats (recipe / interventions / snapshots)
- Conventions for units, naming, error handling, logging, and randomness

It does **not** define:

- Algorithms for tectonics, climate, biology, civilization (those live in their own specs)
- The mod content schema (Mod System Specification)
- Rendering details (Rendering & UI Specification)
- Byte-level layout of binary snapshots (Save Format Specification)

This document is intended to be implementable. Cursor or another AI agent should be able to produce working scaffolding from it without needing to invent significant structure.

## 2. Foundational Conventions

### 2.1 Language and Edition

- **Language:** Rust, edition 2024
- **Minimum Rust version:** Latest stable at time of implementation
- **Crate organization:** Per Glossary §3 (Module Organization)

### 2.2 Units

All internal code uses SI metric units:

| Quantity | Unit | Type |
|----------|------|------|
| Distance | meters | `f32` for ephemeral, fixed-point for accumulating |
| Area | square meters or square kilometers | `f64` |
| Temperature | degrees Celsius | `f32` |
| Time (long-scale) | years | `WorldYear(i64)` (newtype) |
| Time (real-time UX) | seconds | standard `f32` or `Duration` |
| Mass | kilograms | `f64` |
| Pressure | hectopascals (hPa) | `f32` |
| Precipitation | millimeters/year | `f32` |
| Population | count of individuals | `u64` |
| Velocity (tectonic) | meters/year | `f32` |

User-facing display may convert to Imperial or other units via the render layer. Conversion is a presentation concern and never affects stored data or simulation logic.

### 2.3 Naming Conventions

Strictly per Glossary §3 and §4. The AI implementation must use canonical terms. Specifically:

- Types: `PascalCase`
- Functions and methods: `snake_case`
- Constants: `SCREAMING_SNAKE_CASE`
- Modules and crates: `snake_case`
- The word "hex" not "tile"
- The word "branch" not "timeline" (in code)
- The word "intervention" not "edit" (in code)

### 2.4 Error Handling

- **Library crates** (`genesis_core`, `genesis_tectonics`, etc.): use `thiserror`-derived enum error types. Each crate defines its own `Error` enum with specific variants.
- **Application crate** (`genesis_app`): use `anyhow::Result` for top-level error handling.
- **No `unwrap()` or `expect()` outside tests** unless accompanied by an explicit comment justifying why the operation cannot fail.

### 2.5 Logging

- Use the `tracing` crate for all logging.
- Log levels:
  - `trace!` — hot-path diagnostics (off by default in release)
  - `debug!` — useful for development, off by default in release
  - `info!` — major lifecycle events (world generated, branch created, save loaded)
  - `warn!` — recoverable problems or unusual conditions
  - `error!` — unrecoverable problems

### 2.6 Determinism Requirements

Every randomized operation in Genesis Engine must be deterministic given the world's seed and active mod manifest.

- All randomness flows through the engine's RNG system (defined in §6).
- No use of `rand::thread_rng()`.
- No use of `std::time::SystemTime` or `Instant` to affect simulation state.
- `HashMap` and `HashSet` from `std` are forbidden in code that affects simulation state because their iteration order is randomized; use `BTreeMap` and `BTreeSet`, or `IndexMap`/`IndexSet` from the `indexmap` crate.
- When parallelism is used, results must be aggregated in deterministic order.
- Floating-point math is acceptable for ephemeral within-tick calculations; quantities that accumulate over many ticks must use fixed-point arithmetic.

Full determinism strategy is defined in the Determinism Specification (Doc 12). This document establishes the baseline requirements.

## 3. The Hex Grid

### 3.1 Overview

Genesis Engine uses an icosahedral hex grid (ISEA3H — Icosahedral Snyder Equal Area, aperture 3 Hexagon). The grid covers a spherical planet surface with hexagonal cells. Twelve cells are pentagons (5 neighbors); all others are hexagons (6 neighbors).

The grid is defined by a **subdivision level** (an integer). Higher subdivision levels produce more, smaller hexes. The total cell count at subdivision level `n` is `10 × 3^n + 2`.

| Subdivision | Total Cells | Hex Area on Earth (km²) | Hex Area on Mars (km²) |
|-------------|-------------|--------------------------|-------------------------|
| 0 | 12 | 42,500,000 | 11,400,000 |
| 1 | 32 | 15,900,000 | 4,260,000 |
| 2 | 92 | 5,540,000 | 1,490,000 |
| 3 | 272 | 1,870,000 | 502,000 |
| 4 | 812 | 627,000 | 168,000 |
| 5 | 2,432 | 209,800 | 56,200 |
| 6 | 7,292 | 69,950 | 18,700 |
| 7 | 21,872 | 23,300 | 6,250 |
| 8 | 65,612 | 7,770 | 2,080 |
| 9 | 196,832 | 2,590 | 695 |
| 10 | 590,492 | 865 | 232 |

**Default subdivision level: 8** (65,612 cells). This is the value used unless `WorldParameters.grid.subdivision_level` overrides it.

**Note on level 0:** at subdivision level 0, all 12 cells are pentagons (the 12 icosahedron vertices). This is useful as a degenerate case for testing but not for actual worldbuilding. Levels 5–9 are the practical range for v1 worlds.

### 3.2 Hex Identifiers

External code refers to hexes by opaque `HexId`. The internal coordinate representation is hidden behind the grid module.

```rust
/// Opaque identifier for a hex cell. Stable across runs given the same world.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct HexId(pub u32);
```

`HexId` values are dense: a grid with N cells uses `HexId(0)` through `HexId(N-1)` consecutively. This allows direct indexing into bulk arrays.

### 3.3 Pentagons

Exactly 12 cells are pentagons. Their positions are fixed at the 12 icosahedron vertices and known at grid construction. The grid module exposes:

```rust
pub fn is_pentagon(&self, hex: HexId) -> bool;
```

Most algorithms do not need to special-case pentagons because neighbor iteration is variable-length (see §3.4). Pentagons surface to external code only when explicitly queried.

### 3.3.1 Class I / Class II Parity (handled by the Vince/Kristensen scheme)

ISEA3H hexes alternate orientation between subdivision levels. At even-numbered levels, hexes are oriented one way relative to their face's local frame (commonly called "Class I"); at odd-numbered levels, they are rotated 30° relative to even levels ("Class II"). This is a fundamental property of aperture-3 hexagonal grids and cannot be avoided.

Genesis Engine handles parity automatically via the **Vince A3-coordinate scheme** (Vince 2006), as described and corrected by Kristensen (2021). The scheme uses closed-form neighbor rules that differ between even and odd resolutions — the rules themselves encode the parity. There is no separate "parity handling" step; the algorithm just works for both classes.

Cells are internally represented as:

```rust
enum Isea3hCoord {
    Pentagon { vertex: u8 },                                          // one of 12 icosahedron vertices
    Edge { v_i: u8, v_j: u8, h_i: i32, h_j: i32 },                    // hex on an icosahedron edge
    Interior { v_i: u8, v_j: u8, v_k: u8, h_i: i32, h_j: i32, h_k: i32 }, // hex inside a face
}
```

Vertex indices use Kristensen's "Hamiltonian" indexing (Figure 6 in her blog post) where the opposite pole of vertex `v` is `(v + 6) mod 12`, and the 5 vertices adjacent to a focal vertex follow a regular pattern depending only on whether the focal vertex is even or odd.

Neighbors are computed by closed-form rules from Kristensen §"Cell neighbourhood rules," including the edge-crossing function `ω(x, {y, z})` that returns the third vertex of the face adjacent across edge `{y, z}`.

**The `Direction` enum semantics** still apply: a hex's `Direction::D0` does not point the same way across levels. Direction indices are relative to each hex's local orientation (determined by sorting topological neighbors by geographic bearing). Algorithms that depend on absolute directional semantics (e.g., "the neighbor most north of this hex") must use the geographic coordinate functions (§3.5), not direction indices.

**Algorithm source references:**
- Vince, A. (2006). "Indexing the aperture 3 hexagonal discrete global grid." *Journal of Visual Communication and Image Representation* 17(6): 1227-1236.
- Kristensen, N. (2021). "Finding cell neighbours in an ISEA3H global grid in dggridR." https://nadiah.org/2021/09/29/find-cell-neighbours-isea3h/

**Known erratum:** Kristensen's prose description of the even-resolution offset rule #6 is `(0, -1, -1)`, but her reference R implementation uses `(0, -1, +1)`. The R version produces correct cell counts and symmetric neighbor graphs at all tested levels; Genesis Engine follows the R implementation.

### 3.3.2 Projection Choice: Geodesic vs. Snyder

The "S" in ISEA3H stands for Snyder, referring to the Snyder equal-area icosahedral projection that produces hexes of exactly equal area. Genesis Engine v1 uses **geodesic barycentric centers** instead — cell centers are computed by barycentric interpolation between face vertex unit vectors, then normalized onto the sphere. This is simpler to implement and platform-stable.

The trade-off: cells are not exactly equal-area. Hexes near pentagons are slightly smaller than hexes far from pentagons. Worst-case area variation is on the order of 10–20% in the immediate vicinity of pentagon cells. For most purposes (climate, biology, civilization) this is well below the simulation's other uncertainties and does not affect outcomes meaningfully.

If, in later phases, we observe biases concentrated near the 12 pentagon locations (e.g., systematically larger or smaller populations clustered there), we can implement full Snyder projection. The architecture supports this swap without affecting external code, since `Isea3hCoord` is opaque to the rest of the engine.

### 3.4 Neighbor Lookup

Neighbors are precomputed at grid construction using the topological rules described in §3.3.1 (Vince/Kristensen scheme). At construction time, the engine computes each cell's neighbors via closed-form rules — no geometric search. The resulting `HexId`-to-`[HexId]` tables are stored for O(1) lookup at runtime.

The interface:

```rust
/// Returns the neighbors of a hex. Returns 5 entries for pentagons, 6 for hexes.
pub fn neighbors(&self, hex: HexId) -> &[HexId];

/// Returns the neighbor in a specific direction, if one exists.
/// For pentagons, returns None for the "missing" direction.
pub fn neighbor_in_direction(&self, hex: HexId, direction: Direction) -> Option<HexId>;
```

Where `Direction` is an enum:

```rust
pub enum Direction {
    /// Six standard directions, indexed 0-5
    D0, D1, D2, D3, D4, D5,
}
```

The directions are defined in a consistent local frame per hex (each hex has its own concept of "direction 0"). The topological neighbors are sorted by geographic bearing from the hex's center to assign Direction indices: `D0` is the neighbor at the smallest bearing angle (closest to due north), proceeding clockwise. Algorithms requiring global direction (e.g., "north") must use the geographic coordinate functions in §3.5, not direction indices, since `D0` for one hex is not the same direction as `D0` for another.

### 3.5 Geographic Coordinates

Each hex has a center point on the sphere, expressed as latitude and longitude in radians:

```rust
pub fn center_lat_lon(&self, hex: HexId) -> (f64, f64); // (latitude, longitude) in radians
```

Conversions to Cartesian coordinates (for rendering), surface distance between hexes, and bearing between hexes are provided by the grid module.

### 3.6 Hex Area

A hex's physical area depends on the planet's radius. The grid module computes this from `WorldParameters.planet_radius_km`:

```rust
pub fn hex_area_km2(&self, hex: HexId) -> f64;
```

Cells are approximately equal-area (this is a property of the ISEA3H system), with minor variation near pentagons. For most simulation purposes, treat hex area as uniform; use the per-hex area function only when precision matters.

### 3.7 Sub-Resolution Detail

The macro grid is the lowest resolution used for global simulation. When the user zooms in beyond the macro scale, finer detail is procedurally generated from the macro hex's properties.

Doc 04 commits to the architectural pattern; implementation of sub-resolution generation is deferred:

```rust
/// Generates a deterministic finer heightmap for a single hex.
/// Implementation deferred to later phase (hydrology / rendering).
pub trait SubHexTerrainGenerator {
    fn generate(&self, hex: HexId, world: &WorldData, seed: u64) -> SubHexTerrain;
}
```

The macro data must include enough information for plausible sub-hex terrain to be generated:

- `elevation_mean` — average elevation of the hex
- `elevation_relief` — vertical range within the hex (flat plains vs mountainous)
- Slope vectors to neighbors (derivable from neighbor elevations)

See §5.1 for these fields' definitions.

### 3.8 Grid Construction Cost

The hex grid is constructed once per world load. The expected cost at subdivision level 8 (~65K cells) is well under 1 second on target hardware. The constructed grid is read-only thereafter.

### 3.9 No Edges

The hex grid is a complete spherical surface. There are no edges. Code must not assume the grid has boundaries. A tectonic plate moving "east" traverses neighbor relationships continuously around the sphere; it does not wrap.

When the grid is projected for 2D rendering, edge artifacts in the rendering are a render-layer concern (§14 of Architecture Overview), not a data-layer concern.

## 4. World Parameters

`WorldParameters` is the immutable recipe for a world. Combined with the seed and the intervention log, it fully determines the world's state at any time.

### 4.1 Structure

```rust
pub struct WorldParameters {
    pub core: CoreParameters,
    pub extensions: ParameterExtensions,
}

/// Engine-defined parameters. Always present.
pub struct CoreParameters {
    pub seed: WorldSeed,
    pub mod_manifest: ModManifest,
    pub planet: PlanetParameters,
    pub grid: GridParameters,
    pub time: TimeParameters,
    pub geology: GeologyParameters,
    pub climate_initial: ClimateInitialParameters,
    pub biology: BiologyParameters,
    pub civilization: CivilizationParameters,
}

/// Mod-defined parameter fields. Validated against the mod manifest.
/// Empty for vanilla worlds.
pub struct ParameterExtensions {
    pub fields: BTreeMap<String, ParameterValue>,
}
```

### 4.2 Mutability Categories

Each field in `CoreParameters` is tagged with a mutability category:

- **Immutable:** Cannot be changed after world creation, ever, not even by intervention. Changes would invalidate the world.
- **Tunable:** Can be modified by intervention. A modification becomes an entry in the intervention log scoped appropriately.
- **Derived:** Computed from other parameters at generation; not directly set by the user.

The tag for each field is shown in the schema below.

### 4.3 The Seed

```rust
pub struct WorldSeed {
    /// The canonical integer seed.
    pub value: u64,
    /// The original user input, if a string. Stored for display.
    pub user_input: Option<String>,
}
```

**Mutability: Immutable.**

User input may be either:

- A numeric integer (interpreted directly as the seed value)
- A string (hashed deterministically to a `u64`)

String hashing uses **XXH3_64** (from the `xxhash-rust` crate) with a fixed key. This is platform-independent and stable across Rust versions. The hash function and its key are fixed by Doc 04 and must not change without a corresponding world format version bump.

### 4.4 Mod Manifest

```rust
pub struct ModManifest {
    /// Ordered list of mods. Order matters for conflict resolution.
    pub mods: Vec<ModEntry>,
}

pub struct ModEntry {
    /// Stable identifier of the mod (e.g., "core", "medievalmagic").
    pub id: String,
    /// Semver version of the mod.
    pub version: String,
    /// Optional hash of the mod's content for integrity checking.
    pub content_hash: Option<String>,
}
```

**Mutability: Immutable.**

The manifest's effect on determinism: at world generation, the engine computes an **effective seed** from `WorldSeed` and `ModManifest`:

```
effective_seed = xxh3_64(seed.value || manifest_canonical_bytes)
```

Where `manifest_canonical_bytes` is the manifest serialized in a canonical, sorted form. The effective seed is what RNG streams derive from (see §6).

### 4.5 Planet Parameters

```rust
pub struct PlanetParameters {
    /// Planet radius in kilometers. Earth: 6371.0
    pub radius_km: f64,                          // Immutable
    /// Surface gravity in g (Earth = 1.0).
    pub gravity_g: f32,                          // Immutable
    /// Axial tilt in degrees. Earth: 23.4
    pub axial_tilt_degrees: f32,                 // Immutable
    /// Rotation period in hours. Earth: 24.0
    pub rotation_period_hours: f64,              // Immutable
    /// Orbital period in Earth-days. Earth: 365.25
    pub orbital_period_days: f64,                // Immutable
    /// Number of suns (for exotic configurations). v1 supports only 1.
    pub star_count: u8,                          // Immutable
    /// Number of moons. Affects tides. v1 supports 0-2.
    pub moon_count: u8,                          // Immutable
    /// Whether the planet is tidally locked. v1 supports false only.
    pub tidally_locked: bool,                    // Immutable
}
```

**v1 Validation:** `star_count == 1`, `moon_count <= 2`, `tidally_locked == false`. Out-of-range values are accepted in the data structure (for forward compatibility) but rejected with a clear error during world generation.

### 4.6 Grid Parameters

```rust
pub struct GridParameters {
    /// ISEA3H subdivision level. Default 8 (65,612 cells).
    pub subdivision_level: u8,                   // Immutable
}
```

**v1 Validation:** subdivision level must be 5-9 inclusive. Level 10 (~590K cells) is documented but not validated for v1 — likely requires hardware beyond v1 targets.

### 4.7 Time Parameters

```rust
pub struct TimeParameters {
    /// Year at which the world begins (0 = formation).
    pub world_start_year: WorldYear,             // Immutable, almost always 0
    /// Year at which the user is placed by default after generation.
    /// Default: 4_500_000_000 (matching Earth's age).
    pub default_user_year: WorldYear,            // Immutable
    /// Year at which the simulation will stop generating events automatically.
    /// Default: same as default_user_year. Can be extended by user later.
    pub simulation_end_year: WorldYear,          // Tunable (via intervention)
}
```

### 4.8 Geology Parameters

```rust
pub struct GeologyParameters {
    /// Initial number of tectonic plates. Earth has 7-8 major plates.
    pub initial_plate_count: u8,                 // Immutable
    /// Fraction of surface that begins as continental crust (0.0-1.0).
    /// Earth is approximately 0.29.
    pub initial_continental_fraction: f32,       // Immutable
    /// Plate velocity scale factor. 1.0 = Earth-typical.
    pub plate_velocity_scale: f32,               // Immutable
    /// Volcanism intensity scale. 1.0 = Earth-typical.
    pub volcanism_scale: f32,                    // Immutable
}
```

### 4.9 Climate Initial Parameters

```rust
pub struct ClimateInitialParameters {
    /// Mean global surface temperature at simulation start, in Celsius.
    pub initial_mean_temperature_c: f32,         // Immutable
    /// Initial sea level relative to mean elevation, in meters.
    /// Sea level varies during simulation; this is just the starting value.
    pub initial_sea_level_m: f32,                // Immutable
    /// Atmospheric pressure at sea level, in hectopascals. Earth: 1013.25
    pub surface_pressure_hpa: f32,               // Immutable
    /// Greenhouse intensity multiplier. 1.0 = Earth-like.
    pub greenhouse_intensity: f32,               // Immutable
}
```

### 4.10 Biology Parameters

```rust
pub struct BiologyParameters {
    /// Year at which biology system activates. Default 500_000_000 (500 Mya from start).
    pub life_emergence_year: WorldYear,          // Immutable
    /// Mutation rate scale. 1.0 = Earth-like.
    pub mutation_rate_scale: f32,                // Immutable
    /// Extinction event probability scale. 1.0 = Earth-like.
    pub extinction_scale: f32,                   // Immutable
}
```

### 4.11 Civilization Parameters

```rust
pub struct CivilizationParameters {
    /// Year at which sapience emerges. If unset, derived from biology.
    pub sapience_emergence_year: Option<WorldYear>, // Tunable
    /// Technology emergence rate multiplier. 1.0 = Earth-like history.
    pub tech_rate_scale: f32,                    // Tunable
    /// Cultural drift rate multiplier.
    pub cultural_drift_scale: f32,               // Tunable
    /// Conflict frequency multiplier.
    pub conflict_scale: f32,                     // Tunable
}
```

### 4.12 Parameter Extensions

Mods can add parameter fields under their namespace:

```rust
pub struct ParameterValue {
    pub source_mod: String,
    pub field_name: String,
    pub value: ParameterValueData,
}

pub enum ParameterValueData {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Enum { type_id: String, variant: String },
}
```

Mods declare their extension schema in their content pack. Validation ensures:

- Field names are unique within a mod's namespace
- No mod extension shadows a `CoreParameters` field
- Values match the declared type

Full details in the Mod System Specification.

### 4.13 Presets

The core content pack defines named presets — bundles of `WorldParameters` values that represent common starting points:

- `earth_like` (the default)
- `young_volcanic` (high volcanism, young world)
- `archipelago` (high water coverage)
- `cold` (low temperature)
- `dense` (high subdivision level, more detail)

Presets are mod content, not engine code. Mods can define additional presets.

### 4.14 Serialization

`WorldParameters` serializes to TOML for human readability:

```toml
[seed]
value = 17854213
user_input = "lmao420bestseedof@allt1me"

[mod_manifest]
mods = [
    { id = "core", version = "1.0.0" }
]

[planet]
radius_km = 6371.0
gravity_g = 1.0
axial_tilt_degrees = 23.4
# ... and so on
```

The TOML representation is the canonical shared format. Internally, parameters may use a more efficient representation; the TOML form is for storage and sharing.

## 5. The Data Layer

### 5.1 Bulk Arrays

Per-hex data lives in bulk arrays (Struct-of-Arrays pattern). These arrays are owned by the `WorldData` struct in `genesis_core`.

**Note on `parameters` field:** Phase 0 step 3 implements `WorldData` without the `parameters: WorldParameters` field, because `WorldParameters` is implemented in step 4. The schema below shows the final shape; the step-3 implementation omits `parameters` and the step-4 implementation adds it. Similarly, `current_year` is typed as a placeholder `WorldYear(i64)` in step 3 and replaced with the full `WorldYear` from `genesis_core::time` in step 4.

```rust
pub struct WorldData {
    /// The grid this data corresponds to. Immutable after construction.
    pub grid: HexGrid,

    /// The parameters this world was created from. Immutable.
    /// (Added in step 4 alongside the `genesis_core::parameters` module.)
    pub parameters: WorldParameters,

    /// The current simulation time.
    pub current_year: WorldYear,

    // ---- Physical Layer (Layer 0) ----
    /// Mean elevation in meters, signed (negative = below sea level baseline).
    /// Indexed by HexId.
    pub elevation_mean: Vec<f32>,

    /// Vertical range within the hex, in meters.
    pub elevation_relief: Vec<f32>,

    /// Bedrock composition. See BedrockType enum.
    pub bedrock_type: Vec<BedrockType>,

    /// Plate ID the hex currently belongs to.
    pub plate_id: Vec<PlateId>,

    /// Mean annual temperature in Celsius.
    pub temperature_mean: Vec<f32>,

    /// Annual temperature range (max - min).
    pub temperature_range: Vec<f32>,

    /// Annual precipitation in mm/year.
    pub precipitation: Vec<f32>,

    /// Habitability score (0.0-1.0). Derived from temperature, precipitation, hazards.
    pub habitability: Vec<f32>,

    /// Which neighbor (Direction) water primarily flows toward. None = no flow (e.g., ocean).
    pub flow_direction: Vec<Option<Direction>>,

    /// Volume of water passing through the hex, in m³/year.
    pub flow_volume: Vec<f32>,

    /// Soil fertility score (0.0-1.0). Derived from bedrock, climate, organics.
    pub soil_fertility: Vec<f32>,

    // ---- Global Physical State ----
    /// Current global sea level in meters (relative to baseline).
    pub sea_level_m: f32,

    /// Current mean global temperature in Celsius.
    pub global_temperature_c: f32,

    // ---- Biological Layer (Layer 1) ----
    /// Biome type for each hex.
    pub biome: Vec<BiomeId>,

    /// Total biomass in tons per hex.
    pub biomass: Vec<f32>,

    // ---- Civilizational Layer (Layer 2) ----
    /// Population in each hex. Most hexes will be 0.
    pub population: Vec<u64>,

    /// Settlement ID if a settlement exists in this hex, else None.
    pub settlement_id: Vec<Option<SettlementId>>,

    /// Nation ID controlling the hex, if any.
    pub nation_id: Vec<Option<NationId>>,
}
```

**Implementation notes for Phase 0:**

- All arrays are sized to `grid.cell_count()`.
- All arrays are initialized to sensible defaults (zero elevation, no plate, no biome, etc.).
- The simulation modules (Phase 1+) will populate these fields.

Additional fields will be added as later phases require them. The pattern: add a new `Vec<T>` field to `WorldData`. Do not nest into sub-structs unless there is a strong organizational reason — flat is fast.

### 5.2 Sparse Entities (ECS)

For entities that don't exist per-hex (settlements, nations, species, etc.), Genesis Engine uses Bevy ECS components.

The `WorldData` struct above lives as a Bevy resource. Entities live in the Bevy world. Components reference hexes by `HexId` and reference each other by Bevy entity IDs.

Phase 0 defines these entity types as empty markers; their components will be elaborated in later specs:

```rust
// Settlements
#[derive(Component)] pub struct Settlement;
#[derive(Component)] pub struct SettlementLocation(pub HexId);

// Nations
#[derive(Component)] pub struct Nation;

// Species
#[derive(Component)] pub struct Species;

// Cultures
#[derive(Component)] pub struct Culture;

// Languages
#[derive(Component)] pub struct LanguageFamily;
```

Phase 0 implementation registers these types with Bevy but does not spawn any of them. Subsequent phases (biology, civilization) will add their full component sets.

### 5.3 Stable Identifiers

Entities have stable IDs that survive save/load:

```rust
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct SettlementId(pub u32);

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct NationId(pub u32);

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct SpeciesId(pub u32);

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct PlateId(pub u16);

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct BiomeId(pub u16);
```

`BiomeId` is content-driven: a biome is a content entry from a mod. The `BiomeId` is an index into the loaded biome registry. Save files reference biomes by their content identifier string (`"core:temperate_forest"`) for stability across mod versions.

**Sentinel values for sparse defaults.** `PlateId` and `BiomeId` reserve `u16::MAX` as a "none" sentinel. This avoids the memory overhead of wrapping every hex's `plate_id` and `biome` field in `Option<T>`:

```rust
impl PlateId {
    /// Sentinel value indicating no plate assignment.
    pub const NONE: PlateId = PlateId(u16::MAX);
}

impl BiomeId {
    /// Sentinel value indicating no biome assignment.
    pub const NONE: BiomeId = BiomeId(u16::MAX);
}
```

Code that constructs bulk arrays for these fields uses the sentinel as the default:

```rust
plate_id: vec![PlateId::NONE; cell_count],
biome:    vec![BiomeId::NONE; cell_count],
```

The simulation modules replace the sentinel with real assignments as they run. Reading code can detect "no assignment" by comparing to the `NONE` constant.

For other ID types (`SettlementId`, `NationId`, `SpeciesId`), which appear in `Option<T>` fields rather than bulk arrays, no sentinel is needed — `None` itself represents "no assignment."

### 5.4 Enums

Phase 0 defines stable enum types where used:

```rust
pub enum BedrockType {
    Unknown,
    Igneous,
    Sedimentary,
    Metamorphic,
    OceanicCrust,
    Limestone, // explicit variant for the soil-fertility chain
    // Additional variants added as needed; this is not yet exhaustive.
}
```

Where enums need to be moddable (biomes, technologies, cultural traits), they are stored as content-driven IDs, not Rust enums.

## 6. Randomness and Seeds

### 6.1 The Effective Seed

```rust
pub fn effective_seed(seed: &WorldSeed, manifest: &ModManifest) -> u64 {
    // XXH3_64 of seed.value concatenated with the canonical mod manifest serialization
}
```

The effective seed is computed once at world load and stored. All RNG streams derive from it.

### 6.2 RNG Streams

Genesis Engine uses **named, deterministic RNG streams**. Each subsystem operation that needs randomness identifies a stream by name; the engine deterministically derives an RNG from `(effective_seed, stream_name)`.

```rust
pub struct WorldRng {
    effective_seed: u64,
}

impl WorldRng {
    /// Derives a deterministic RNG for the given stream name.
    /// Calling with the same stream name always returns an RNG with the
    /// same internal state.
    pub fn stream(&self, name: &str) -> SmallRng;
}
```

Stream names use dotted hierarchical naming: `"tectonics.plate_assignment"`, `"climate.precipitation"`, `"biology.species_traits.body_size"`.

**Critical determinism rule:** an RNG obtained via `stream(name)` is freshly derived each call from a fixed state. Mutations to that RNG do not persist between calls. If a system needs to make many random choices in sequence (e.g., assigning traits to 1000 species), it obtains a stream RNG once and uses it for the full operation, ensuring stable ordering.

For ordering-sensitive operations, the system must iterate inputs in deterministic order (e.g., sorted by HexId or SpeciesId) before drawing from the RNG.

### 6.3 Underlying RNG

The RNG implementation is `rand::rngs::SmallRng` (a fast PRNG with adequate quality for simulation). The choice is documented here; changing it requires a world format version bump.

## 7. Time

### 7.1 WorldYear

```rust
/// A year in world time. Year 0 = world formation.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct WorldYear(pub i64);
```

`i64` gives a range vastly exceeding any practical world age. Negative values are reserved for hypothetical "before formation" scenarios and are not used in v1.

### 7.2 WorldTime

```rust
/// Rich representation of a world time, with formatting helpers.
pub struct WorldTime {
    pub year: WorldYear,
    pub era: Era,
}

pub enum Era {
    Formation,    // 0 to formation_end
    Geological,   // formation_end to life_emergence
    Prehistoric,  // life_emergence to sapience_emergence
    Ancient,      // sapience_emergence to "ancient/recent boundary"
    Recent,       // ancient/recent boundary to present
}
```

Era boundaries are derived from `WorldParameters` (which set `life_emergence_year` and `sapience_emergence_year`). Specific year ranges within `Era` are computed at world load.

Display formatting is the rendering layer's concern. `WorldTime` provides:

```rust
impl WorldTime {
    /// Returns the year as a signed integer.
    pub fn year_value(&self) -> i64;
    /// Returns years before the current simulation time, for human display.
    pub fn years_before(&self, now: WorldYear) -> i64;
}
```

### 7.3 Tick System Interface

Phase 0 implements the tick coordinator skeleton; detailed tick rates per layer per era are defined in the Tick System Specification (Doc 05).

```rust
pub trait SimulationLayer {
    /// Returns the tick interval (in years) for this layer at the given time.
    fn tick_interval(&self, current_time: WorldYear) -> i64;

    /// Advances this layer by one tick.
    /// Returns events generated during this tick.
    fn advance(&mut self, world: &mut WorldData, rng: &WorldRng) -> Vec<Event>;
}
```

Layers are coordinated by the `TickCoordinator`:

```rust
pub struct TickCoordinator {
    layers: Vec<Box<dyn SimulationLayer>>,
}

impl TickCoordinator {
    /// Advances simulation until `current_year >= target_year`.
    /// Layers tick in order; each layer ticks as often as its interval requires.
    pub fn advance_to(&mut self, target_year: WorldYear, world: &mut WorldData, rng: &WorldRng);
}
```

## 8. Events

### 8.1 Event Structure

```rust
pub struct Event {
    pub id: EventId,
    pub year: WorldYear,
    pub branch_id: BranchId,
    pub location: EventLocation,
    pub significance: Significance,
    pub kind: EventKind,
}

pub struct EventId(pub u64);

pub enum EventLocation {
    Hex(HexId),
    Region(Vec<HexId>),
    Global,
    None,
}

pub enum Significance {
    Trace,   // Low-level events (rarely shown)
    Minor,   // Routine occurrences
    Notable, // Worth showing in detail views
    Major,   // Default chronicle view
    Pivotal, // World-shaping events
}

pub enum EventKind {
    // Phase 0 defines the discriminator; later phases populate variants.
    // Examples (full set defined in respective module specs):
    GeologicalFormation(GeologicalFormationEvent),
    VolcanicEruption(VolcanicEruptionEvent),
    ImpactEvent(ImpactEvent),
    SeaLevelChange(SeaLevelChangeEvent),
    SpeciesEmergence(SpeciesEmergenceEvent),
    SpeciesExtinction(SpeciesExtinctionEvent),
    SettlementFounded(SettlementFoundedEvent),
    NationFormed(NationFormedEvent),
    Conflict(ConflictEvent),
    TechnologyEmergence(TechnologyEmergenceEvent),
    // ... more added as modules define them
}
```

For Phase 0, only the `Event` struct, `EventId`, `EventLocation`, `Significance`, and an empty `EventKind` enum are defined. Variants are added by later phases.

### 8.2 The Event Log

```rust
pub struct EventLog {
    /// Events on this branch, in chronological order.
    /// Pre-divergence events are inherited from the parent branch by reference.
    events: Vec<Event>,

    /// The year at which this log diverges from its parent.
    /// Events at year < divergence_year are inherited from parent.
    divergence_year: WorldYear,

    /// The parent branch's ID, or None if this is the root.
    parent_branch: Option<BranchId>,
}

impl EventLog {
    /// Appends a new event. Used by simulation modules.
    pub fn push(&mut self, event: Event);

    /// Iterates events visible on this branch up to a given year.
    /// Includes inherited events from parent branches.
    pub fn iter_up_to(&self, year: WorldYear) -> impl Iterator<Item = &Event>;

    /// Filters events by significance threshold.
    pub fn iter_significant(&self, min: Significance) -> impl Iterator<Item = &Event>;
}
```

**Critical rule:** events are emitted by simulation systems but never consumed by other simulation systems. Inter-system communication happens via `WorldData` state, not via the event log. The event log is for the user and for export.

### 8.3 Event Log Storage

Per-branch event logs serialize to **JSONL** (one JSON object per line). Append-only on disk. One file per branch.

Format example:

```jsonl
{"id":1,"year":1200000000,"branch_id":0,"location":{"Hex":48723},"significance":"Major","kind":{"VolcanicEruption":{"severity":"Major"}}}
{"id":2,"year":1200000100,"branch_id":0,"location":"Global","significance":"Notable","kind":{"SeaLevelChange":{"delta_m":-2.4}}}
```

This format is human-readable, diffable, mergeable, and append-friendly. Compression (gzip) is applied when the log exceeds a threshold size; details in Save Format Spec.

## 9. Branches and Interventions

### 9.1 Branch Structure

```rust
pub struct Branch {
    pub id: BranchId,
    pub parent: Option<BranchId>,
    pub divergence_year: WorldYear,
    pub name: String,
    pub created_at_real_time: chrono::DateTime<chrono::Utc>, // metadata only
    pub intervention_log: InterventionLog,
    pub event_log: EventLog,
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct BranchId(pub u32);
```

The root branch has `parent: None` and `divergence_year: 0`.

### 9.2 Branch Tree

```rust
pub struct BranchTree {
    branches: BTreeMap<BranchId, Branch>,
    next_id: u32,
}

impl BranchTree {
    pub fn root(&self) -> &Branch;
    pub fn get(&self, id: BranchId) -> Option<&Branch>;
    pub fn children_of(&self, parent: BranchId) -> Vec<BranchId>;
    pub fn create_branch(&mut self, parent: BranchId, divergence_year: WorldYear, name: String) -> BranchId;
}
```

### 9.3 Interventions

```rust
pub struct Intervention {
    pub id: InterventionId,
    pub year: WorldYear,
    pub branch_id: BranchId,
    pub scope: InterventionScope,
    pub action: InterventionAction,
    pub created_at_real_time: chrono::DateTime<chrono::Utc>, // metadata only
}

pub struct InterventionId(pub u64);

pub enum InterventionScope {
    Local,    // Affects one hex or one entity
    Regional, // Affects multiple connected hexes
    Global,   // Affects the entire world
}

pub enum InterventionAction {
    // Phase 0 defines the discriminator; later phases populate variants.
    // Examples:
    AdjustHexProperty { hex: HexId, property: HexProperty, value: PropertyValue },
    AdjustParameter { parameter: ParameterPath, value: ParameterValueData },
    SpawnSettlement { hex: HexId, initial_population: u64 },
    RenameEntity { entity: EntityRef, new_name: String },
    PlaceEvent { event_kind: EventKind, location: EventLocation, year: WorldYear },
    // ... more added as modules define them
}
```

Phase 0 defines the structure but leaves variants empty. Each later module adds the intervention actions it supports.

### 9.4 Intervention Log

```rust
pub struct InterventionLog {
    interventions: Vec<Intervention>,
}

impl InterventionLog {
    pub fn push(&mut self, intervention: Intervention);
    pub fn iter_up_to(&self, year: WorldYear) -> impl Iterator<Item = &Intervention>;
    pub fn iter_in_range(&self, start: WorldYear, end: WorldYear) -> impl Iterator<Item = &Intervention>;
}
```

Storage: JSONL, one file per branch. Same format and rationale as the event log.

### 9.5 Applying Interventions

When the simulation advances and encounters interventions at the current year, they are applied **before** that year's simulation ticks. This makes interventions feel like discrete decisions: "at year X, the user did Y, then time proceeded."

```rust
pub fn apply_interventions_for_year(world: &mut WorldData, log: &InterventionLog, year: WorldYear);
```

## 10. Save File Format

### 10.1 Three-Layer Structure

A complete world consists of three layers, stored separately:

1. **Recipe** — `world.toml` — seed, parameters, mod manifest, branch tree metadata
2. **Logs** — `branches/{branch_id}/events.jsonl`, `branches/{branch_id}/interventions.jsonl`
3. **Cache** — `branches/{branch_id}/snapshots/` — binary, compressed, regenerable

### 10.2 Directory Layout

```
my_world.gen/
├── world.toml                              # The Recipe
├── branch_tree.toml                        # Branch metadata
├── branches/
│   ├── 0/                                  # Root branch
│   │   ├── events.jsonl
│   │   ├── interventions.jsonl             # Empty for root by definition
│   │   └── snapshots/
│   │       ├── y0000000000.snap
│   │       ├── y0500000000.snap
│   │       └── ...
│   └── 1/                                  # First branch
│       ├── events.jsonl
│       ├── interventions.jsonl
│       └── snapshots/
└── ...
```

The `.gen` suffix marks a Genesis Engine world. The "file" is actually a directory; the application opens it as a save.

### 10.3 The Recipe (`world.toml`)

Contains:

- `WorldParameters` (full serialization)
- Genesis Engine version that created the world
- Save format version
- Creation timestamp (metadata)

Sharing a world means sharing `world.toml` + the intervention logs for relevant branches. The recipient regenerates everything else.

### 10.4 Snapshots

Snapshots are binary serialized `WorldData` states at specific years. Format: `bincode` v2 with gzip compression.

Snapshot intervals are era-dependent (defined in Tick System Spec). Phase 0 implements infrastructure for snapshots; actual snapshot creation/restoration is wired in later phases.

### 10.5 Versioning

Every save file includes:

- `genesis_engine_version` — the engine version that created it
- `save_format_version` — the format version (incremented when format changes)

When loading, the engine checks the format version. Older versions may require migration (defined in Save Format Spec, Doc 13).

## 11. Initialization and Lifecycle

### 11.1 World Creation

```rust
pub fn create_world(parameters: WorldParameters) -> Result<WorldData, CreateWorldError> {
    // 1. Validate parameters
    // 2. Compute effective seed
    // 3. Construct hex grid
    // 4. Initialize WorldData with default values
    // 5. Initialize root branch
    // 6. Return ready-to-simulate WorldData
}
```

Phase 0 implements steps 1-5 with default WorldData (zero elevation, etc.). Phase 1 (tectonics) will add an "initial geology" step.

### 11.2 World Generation (Full History)

```rust
pub fn generate_full_history(
    world: &mut WorldData,
    target_year: WorldYear,
    progress: impl FnMut(GenerationProgress),
) -> Result<(), GenerationError>;
```

Runs the simulation from `world.current_year` to `target_year`, emitting events into the root branch's event log. The `progress` callback is invoked periodically so the UI can show progress (and stream major events to the user as they happen, per the Factorio-style preview pattern).

Phase 0 implements the function skeleton; actual simulation happens in later phases.

### 11.3 World Save and Load

```rust
pub fn save_world(world: &WorldData, branches: &BranchTree, path: &Path) -> Result<(), SaveError>;
pub fn load_world(path: &Path) -> Result<(WorldData, BranchTree), LoadError>;
```

Phase 0 implements save/load for the recipe (TOML), branch tree, and intervention/event logs (JSONL). Snapshot save/load is stubbed (the methods exist but write/read empty files).

## 12. Crate Layout for Phase 0

The data layer lives in `genesis_core`. Phase 0 organizes it as:

```
crates/genesis_core/src/
├── lib.rs              # Crate root, public re-exports
├── grid/
│   ├── mod.rs          # HexGrid public API; Direction and HexId types
│   ├── isea3h.rs       # ISEA3H coordinate math (Vince/Kristensen)
│   ├── neighbors.rs    # Neighbor table construction (topological)
│   ├── geography.rs    # Lat/lon, area, distance, bearing
│   ├── ids.rs          # HexId, Direction
│   └── error.rs        # GridError
├── data/
│   ├── mod.rs          # WorldData struct (re-exports Direction from grid)
│   ├── enums.rs        # BedrockType
│   └── ids.rs          # SettlementId, NationId, SpeciesId, PlateId, BiomeId
├── parameters/
│   ├── mod.rs          # WorldParameters
│   ├── core.rs         # CoreParameters sub-structs
│   ├── extensions.rs   # ParameterExtensions
│   └── serialization.rs # TOML I/O
├── time/
│   ├── mod.rs          # WorldYear, WorldTime, Era
│   └── ticks.rs        # SimulationLayer trait, TickCoordinator
├── rng/
│   └── mod.rs          # WorldRng, effective_seed
├── events/
│   ├── mod.rs          # Event, EventLog
│   └── kinds.rs        # EventKind enum (initially empty)
├── interventions/
│   ├── mod.rs          # Intervention, InterventionLog
│   └── actions.rs      # InterventionAction enum (initially empty)
├── branches/
│   └── mod.rs          # Branch, BranchTree, BranchId
├── persistence/
│   ├── mod.rs          # save_world, load_world
│   ├── recipe.rs       # TOML serialization
│   ├── logs.rs         # JSONL serialization
│   └── snapshots.rs    # Binary snapshot stubs
├── lifecycle/
│   └── mod.rs          # create_world, generate_full_history
└── error.rs            # Error types
```

This organization is the recommended starting point. The implementing AI may propose reorganization in the summary, but should not deviate without flagging the change.

## 13. Bare Rendering for Phase 0

Per the build order in Architecture Overview §12, Phase 0 includes a **bare rendering pass** so the agent (and user) can visually confirm the grid is correctly constructed.

The deliverable: `cargo run` launches a window showing the hex grid as a colored sphere or equirectangular projection. Each hex is rendered in a flat color based on its `HexId` (e.g., HSV hue derived from ID, or simple gradient). Pentagons are rendered in a distinct color (bright red) for visual verification.

The rendering crate (`genesis_render`) implements this. Phase 0 rendering scope:

- One projection (equirectangular is easiest)
- Flat-colored hex polygons
- Pentagons marked distinctly
- Camera pan and zoom
- No data overlays, no UI chrome beyond a "regenerate" hotkey

This is a smoke test rendering, not a finished visualization. Full rendering is Phase 3.

## 14. Testing Requirements

Phase 0 implementation must include:

### 14.1 Unit Tests

- Hex grid: cell count matches expected for each subdivision level
- Hex grid: neighbor lookups return correct counts (6 for hexes, 5 for pentagons)
- Hex grid: round-trip lat/lon → hex → lat/lon is reasonable (within hex)
- Hex grid: exactly 12 pentagons exist for valid subdivisions
- WorldSeed: string and integer inputs produce expected effective seeds
- WorldSeed: same string always hashes to same integer
- ModManifest: canonical serialization is order-independent for equivalent manifests
- WorldYear: arithmetic and comparison work correctly
- Era: era detection from year is correct given parameters
- RNG: same stream name from same effective seed produces same sequence
- RNG: different stream names produce different sequences
- EventLog: events serialize and deserialize round-trip
- InterventionLog: events serialize and deserialize round-trip
- BranchTree: parent-child queries return correct results

### 14.2 Integration Tests

- Create a world from parameters, save it, load it back, verify equality
- Create a world, fork a branch, verify branch tree structure
- Construct hex grids at multiple subdivision levels, verify expected cell counts
- Determinism test: create the same world twice from same seed+params+manifest, verify byte-identical `WorldData` after construction

### 14.3 Determinism Snapshot Tests

A test that creates a known world from a fixed seed and verifies the SHA-256 of the canonical-serialized `WorldData` matches a stored expected hash. This catches drift if any randomization or ordering changes.

### 14.4 Performance Sanity Tests

- Grid construction at subdivision level 8 completes in under 1 second
- World load (recipe only, no snapshots) completes in under 100ms

These are sanity checks, not full performance budgets. Performance budgets are defined in Doc 18.

## 15. Out of Scope for Phase 0

Explicitly not implemented in Phase 0:

- Any actual simulation (tectonics, climate, biology, civilization)
- Snapshot creation or restoration
- Real rendering beyond the bare smoke test
- UI for parameter configuration
- UI for intervention authoring
- Mod content loading (the manifest structure exists; loading mod content is Mod System Spec)
- Sub-resolution terrain generation
- Export systems
- The "Factorio-style preview" UI (data structures support it; UI is later)

The deliverable of Phase 0 is **a working data layer that simulation modules can plug into**. The world doesn't *do* anything yet, but it has structure, persists correctly, and is ready to be filled.

## 16. Implementation Notes for the AI Agent

When implementing this specification:

1. **Read the foundation docs first** if not already in context.
2. **Implement crate-by-crate**, starting with `genesis_core`'s grid module. Get the hex grid solid before anything else depends on it.
3. **Write tests alongside implementation**, not after. The tests in §14 are the minimum; add more as patterns emerge.
4. **When you encounter an ambiguity in this spec**, surface it before implementing. Do not invent details.
5. **Avoid over-engineering**. Phase 0 is foundational, but it should not be 30,000 lines of code. Aim for clear, minimal implementations of each item.
6. **The bare rendering (§13) goes last** in the Phase 0 implementation order. Get the data layer rock-solid first.

The implementation order recommended:

1. `genesis_core::grid::isea3h` — coordinate math
2. `genesis_core::grid` — full grid API with neighbor table
3. `genesis_core::data` — WorldData struct and enums
4. `genesis_core::parameters` — WorldParameters
5. `genesis_core::rng` — RNG streams
6. `genesis_core::time` — WorldYear, Era, tick coordinator skeleton
7. `genesis_core::events` and `genesis_core::interventions` — log structures
8. `genesis_core::branches` — branch tree
9. `genesis_core::persistence` — save/load
10. `genesis_core::lifecycle` — create_world, generate_full_history skeleton
11. `genesis_render` — bare rendering pass

Each step should be a separable commit. The user (Brax) reviews each before proceeding to the next, if he chooses. The agent should pause between major steps to summarize and confirm.

---

*End of Data Layer Specification.*
