# 06 — Tectonics Module Specification

**Document Type:** Tier 2 — System Specification
**Status:** Draft v0.3
**Last Updated:** May 2026
**Owner:** Brax Johnson
**Implementing Phase:** 1 (Geology Prototype)

**Changelog:**
- v0.3 (May 2026): Recalibrated boundary elevation rates by ~100x (§5.1–§5.4) to prevent saturation. Added coastal-shelf falloff (§5.3) to produce gradient coastlines instead of cliffs. New rates: `OROGENY_RATE=5e-5`, `SUBDUCTION_RATE=1e-4`, `SUBSIDENCE_RATE=2e-5`. Calibration verified by `long_validation_does_not_saturate_elevation` test. See P1-11 calibration prompt.
- v0.2 (May 2026): Incorporated Brax's review feedback. Added planetary rotation influence on plate motion (§2.1). Added motion axis constraints to prevent geometrically weird plate drift (§2.1). Replaced pure Voronoi initial generation with growth-based seeding (§2.2). Split plate count into major (default 7) + minor (default 8) (§2.2). Made plate velocity distribution log-normal with continental-velocity multiplier (§2.4). Added climate-feedback hook for erosion in Phase 2 (§8.2). Replaced limestone bedrock assignment with fertility accumulator field (§8.4) — `BedrockType::Limestone` transition deferred to Phase 4 Biology. Updated open questions; resolved items 2 and 4; added items 7 (planetary formation deferred to future doc) and 8 (chaos mode deferred).
- v0.1 (May 2026): Initial draft. Defines plate model, boundary dynamics, hot spots, erosion, event schema, and the user-tunable event granularity system.

## 1. Purpose and Scope

This document specifies the **tectonics simulation layer** — the first real simulation layer in Genesis Engine. Tectonics produces the foundational physical geography of a world: continents, oceans, mountain ranges, plate boundaries, and the bedrock that underlies everything else.

### 1.1 Goals

Tectonics must:

1. **Produce plausible continental configurations** from a deterministic seed. Two worlds with the same seed must produce byte-identical tectonic histories.
2. **Cover the full geological era** (typically year 0 through ~4.5 billion years) with multiple continental reorganizations along the way.
3. **Write to `WorldData` bulk arrays** that climate, hydrology, and biology will read from in later phases: `elevation_mean`, `elevation_relief`, `bedrock_type`, `plate_id`, and `sea_level_m`.
4. **Generate events** describing what happened, at a granularity controllable by the user.
5. **Run at acceptable speed.** Per Phase 1 goals (Architecture §12), this is the riskiest assumption to validate — multi-billion-year simulation completing in minutes rather than hours.
6. **Stand up to light scientific scrutiny.** Worldbuilders who happen to be geology enthusiasts should recognize the dynamics as "yeah, roughly how it works" — not academically rigorous, but not magical either.

### 1.2 Non-Goals

This is explicitly NOT a research-grade plate tectonics simulator. We are not modeling:

- Subduction angles, slab pull, ridge push at numerically accurate rates
- Viscous mantle flow or convection cells
- Isostatic adjustment beyond a simple proxy
- Mineralogical composition or igneous petrology
- Earthquake mechanics
- Continental drift at correct velocities (Earth: ~2-15 cm/year; we'll use values that produce visually interesting results at reasonable tick counts)
- Specific Earth historical reconstruction (Pangea, Pannotia, Rodinia, etc. — though our worlds will go through analogous supercontinent cycles)

If the player is a geophysicist, they will find things to nitpick. That's acceptable. The goal is plausible worldbuilding, not academic accuracy.

### 1.3 Dependencies

Reads from:
- `WorldData.grid` (the hex grid — topology and geographic positions)
- `WorldData.parameters.core.geology` (plate count, velocity scale, volcanism scale, continental fraction)
- `WorldData.parameters.core.planet` (radius — affects timescales; not heavily used in v1)
- `WorldRng` — uses named streams (§4.4)

Writes to:
- `WorldData.elevation_mean` (per-hex, meters)
- `WorldData.elevation_relief` (per-hex, meters)
- `WorldData.bedrock_type` (per-hex)
- `WorldData.plate_id` (per-hex)
- `WorldData.fertility` (per-hex, 0.0-1.0; **new field added in Phase 1, see §8.4**)
- `WorldData.sea_level_m` (global, slowly drifts as ocean basins change)

Produces:
- `EventKind` variants per §6
- Possibly updates `EventLog` significance distribution (most events Trace, rare ones Pivotal)

Does not write to:
- Biology arrays (`biome`, `biomass`)
- Civilization arrays
- Temperature or precipitation (climate's job)

## 2. The Plate Model

### 2.1 Plate Representation

```rust
pub struct Plate {
    pub id: PlateId,
    pub plate_type: PlateType,
    pub plate_class: PlateClass,        // Major vs Minor — affects target size
    pub seed_hex: HexId,                // Geographic anchor; doesn't move (see §2.2)
    pub motion_axis: Vec3,              // Unit vector; rotation axis on the sphere
    pub motion_rate_rad_per_year: f64,  // Angular velocity in radians/year
    pub age_year: WorldYear,            // When this plate was created (or last reorganized)
    pub target_fraction: f32,           // Target fraction of sphere this plate covers (used during growth seeding)
}

pub enum PlateType {
    Continental,
    Oceanic,
}

pub enum PlateClass {
    Major,    // Earth-scale continent or major ocean (Pacific, Eurasia)
    Minor,    // Smaller plate (Arabia, Caribbean, Juan de Fuca)
}
```

**Why a rotation axis instead of a velocity vector?** On a sphere, "moving in a direction" only makes sense locally. Plates as rigid spherical caps rotate about an Euler pole — this is how real geophysics describes plate motion, and it produces correct behavior near the poles (a velocity vector in lat/lon would wrap incorrectly).

The motion axis is a unit `Vec3` representing the rotation pole. Each tick, the plate's hex membership shifts as the plate "rotates" by `motion_rate_rad_per_year * tick_interval_years`.

**Motion axis constraints.** To avoid plates that drift in geometrically weird ways (e.g., circling a pole repeatedly), motion axes are sampled with two constraints:
1. The axis must not be exactly aligned with the planet's rotation axis (z-axis in our coordinate frame). Reason: such axes produce purely east-west drift, which over geological time produces no continental rearrangement.
2. The axis should be a reasonable distance from the plate's own centroid — neither passing through it (plate spins in place) nor exactly antipodal to it (plate makes great-circle laps). We sample axes uniformly on the sphere, then reject any whose angular distance from the plate's centroid is less than 30° or more than 150°.

**Planetary rotation influence (light science).** Real plate motion is driven by mantle convection, which is weakly influenced by planetary rotation rate. We model this loosely: the median plate velocity scales gently with `WorldParameters.core.planet.rotation_period_hours` relative to Earth's 24-hour day:

```
rotation_factor = sqrt(24.0 / rotation_period_hours)  // faster planet → faster mantle → faster plates
effective_velocity_scale = plate_velocity_scale * rotation_factor
```

This produces noticeably faster plate motion on a 12-hour-day planet (factor ≈ 1.41), slower on a 48-hour-day planet (factor ≈ 0.71). The relationship is loose; we don't pretend it's physically derivable. It's a hand-wavy bridge to give the rotation parameter geological consequence.

### 2.2 Plate Membership and Generation

Each hex has a `PlateId`. Membership is determined by **growth-based seeding** at world formation, then by **rotated-seed Voronoi re-partition** for ongoing simulation.

#### Initial generation (year 0 only): seed-then-grow

1. **Place major plate seeds.** Sample `initial_major_plate_count` seed hexes using Poisson-disk-like distribution to prevent two seeds from spawning too close. Each major plate gets a `target_fraction` sampled from a distribution centered on `0.50 / initial_major_plate_count` with ±50% variation. This produces some larger plates and some smaller ones (Earth-like variation: Pacific Plate is huge, North American is medium, etc.).

2. **Grow major plates to ~50% coverage.** Each plate has a "growth budget" equal to `target_fraction * total_cells`. Growth proceeds in rounds: each plate picks one of its boundary hexes (a hex it already owns whose neighbor is unowned) and claims a random unowned neighbor. Tie-breaking is deterministic by HexId for unowned candidates within the same plate's expansion. Plates that have hit their growth budget skip their turn.

3. **Stop major plate growth at 50% coverage** of the total sphere. The remaining ~50% is unassigned.

4. **Place minor plate seeds in unassigned territory.** Sample `initial_minor_plate_count` minor plate seeds from unowned hexes, again with Poisson-disk-like spacing. Each minor plate gets a smaller `target_fraction` (~0.03-0.07 of the sphere).

5. **Grow all plates simultaneously** until every hex has an owner. Stochastic per-round expansion using the `tectonics.plate_seeds` stream determines which plate gets to grow in which round, weighted by remaining target_fraction (plates further from their target grow more often).

This produces organic-looking plate boundaries (not perfectly geometric like pure Voronoi), with deliberate size variation between major and minor plates.

**Default plate counts** (via `WorldParameters.core.geology`):
- `initial_major_plate_count`: 7 (range 6-9; matches Earth's 7 major plates)
- `initial_minor_plate_count`: 8 (range 6-10; matches Earth's roughly 8 minor plates)
- Total: 13-19 plates, Earth-like richness

#### Ongoing simulation: rotated-seed Voronoi

After initial generation, the simple Voronoi rule applies for re-partitioning as plates move:

```
for each hex h:
    plate[h] = argmin over plates p of: angular_distance(h.center, p.effective_position)
```

Where `effective_position` is computed by rotating the original seed position by the plate's accumulated rotation about its motion axis.

Critically: **plate seeds rotate with the plate** in the abstract sense, but we don't actually move the seed hex (it's a fixed `HexId`). Instead, we conceptually treat each plate as having a "current position" derived from its motion axis and total elapsed motion. The Voronoi partition is recomputed when plate motion has accumulated enough that boundaries would visibly move.

For implementation, this means each tick we:
1. Update each plate's `accumulated_rotation_rad` (small per-tick increment)
2. Compute each plate's *effective* current position as a rotation of its original seed hex by `accumulated_rotation_rad` about its `motion_axis`
3. Re-partition hexes to plates based on effective positions

The re-partition uses the rotated seed positions, not the original ones. Over 4.5 billion years, plates wander significantly across the sphere even though the `seed_hex` field never changes.

**Why grow-based for initial, Voronoi for ongoing?** Initial generation only happens once per world, and the organic boundaries matter for the world's "character." Ongoing re-partition happens every tick and needs to be fast — Voronoi from rotated positions is O(n*p) per tick which is acceptable. The initial grow phase establishes the organic boundary character; subsequent Voronoi recomputation preserves it approximately as plates drift.

### 2.3 Plate Type Distribution

Per `WorldParameters.core.geology.initial_continental_fraction` (default 0.29, matching Earth):

- Roughly that fraction of plates start as `Continental`
- The rest are `Oceanic`
- Specifically: `num_continental = round(total_plates * initial_continental_fraction)`
- Continental plates have higher initial elevation (~500m mean), oceanic plates lower (~-3500m mean)

Continental plates preferentially get assigned to *major* plate slots (so most continents are big), with smaller continental plates as minor (Arabian-style). Oceanic plates fill the remaining slots; the largest oceanic plate often ends up as a "Pacific" — a major oceanic plate covering 15-25% of the sphere.

### 2.4 Plate Velocity

Real Earth plates move at 0.5-15 cm/year. The Pacific Plate moves ~10 cm/year; the African Plate moves ~1-2 cm/year; the Antarctic Plate is nearly stationary. We want this variation, not uniform motion.

Each plate's `motion_rate_rad_per_year` is sampled from a **log-normal distribution** centered on Earth-like values scaled by `WorldParameters.core.geology.plate_velocity_scale` and the rotation factor from §2.1:

```
effective_velocity_scale = plate_velocity_scale * sqrt(24.0 / rotation_period_hours)
median_cm_per_year = 5.0 * effective_velocity_scale
sigma = 0.6  // log-normal sigma, produces 0.5x-15x variation per plate
```

In radians per year on the planet's surface:

```
plate_rate_rad_per_year = sample_log_normal(median_cm_per_year, sigma) * 1e-5 / planet_radius_km
```

Why log-normal: it produces realistic skew. Most plates near the median, some much faster, rare ones very slow — matching observed Earth dynamics. Pure uniform or Gaussian distributions don't capture this.

**Plate type also biases velocity.** Continental plates carry more mass and tend to move slower; oceanic plates can move faster. Apply a 0.7x multiplier to continental plate rates after sampling. This is the simplification that gives us "Africa moves slowly, Pacific moves quickly."



## 3. Boundary Detection and Classification

### 3.1 Identifying Boundaries

A **boundary hex** is one with at least one neighbor belonging to a different plate. Each tick (after re-partition), the tectonics layer iterates all hexes and flags those with cross-plate neighbors.

Stored as a derived structure (not in `WorldData` — recomputed each tick):

```rust
struct BoundaryInfo {
    boundary_hexes: Vec<HexId>,
    // For each boundary hex, the set of neighbor plates it touches.
    plate_contacts: BTreeMap<HexId, BTreeSet<PlateId>>,
}
```

### 3.2 Classifying Boundary Type

For each boundary hex `h` and each cross-plate neighbor `n`:

1. Compute the relative velocity between plate A (h's plate) and plate B (n's plate) at this geographic location.
2. Decompose into a component **normal** to the boundary (perpendicular to the edge between A and B) and a component **tangential** to the boundary.
3. Classify:
   - **Divergent:** Normal component is negative (plates separating)
   - **Convergent:** Normal component is positive (plates approaching)
   - **Transform:** Tangential component dominates (plates sliding past)

The threshold for "tangential dominates" is when `|normal_velocity| < 0.3 * |tangential_velocity|`. Otherwise the boundary is divergent or convergent based on normal velocity sign.

### 3.3 Boundary Subtype (Convergent only)

Convergent boundaries split by plate type:

- **Continental-Continental:** Mountain building. Two continents collide and crumple.
- **Oceanic-Oceanic:** Island arc formation. One plate subducts, volcanic islands form.
- **Continental-Oceanic:** Subduction zone. Oceanic plate descends. Coastal mountains and arc volcanism on the continental side.

### 3.4 Velocity Computation at a Point

Given two plates A and B with motion axes `ω_A` and `ω_B` and rates `r_A` and `r_B`, the velocity of plate A at point `p` on the sphere is:

```
v_A(p) = (ω_A * r_A) × p
```

(Cross product, treating ω as a vector pointing along the axis with magnitude r.)

The relative velocity at point p (movement of A relative to B) is:

```
v_rel(p) = v_A(p) - v_B(p)
```

For boundary classification, we project this onto the local boundary frame (normal and tangent vectors at p).

## 4. Per-Tick Algorithm

### 4.1 Tick Interval

Tectonic tick interval scales with the current `Era`. The layer stays **active through Prehistoric and Ancient**; only **Recent** is dormant (present-day snapshot).

| Era | Tick Interval | Rationale |
|-----|---------------|-----------|
| Formation | 1 tick at year 0 only | Initial plate generation |
| Geological | 500,000 years | Continental drift moves visibly per tick |
| Prehistoric | 2,000,000 years | Coarser resolution; plates still drift and boundaries evolve |
| Ancient | 10,000,000 years | Even coarser; slow residual tectonics over long spans |
| Recent | Layer dormant | No simulation, just state |

**Why tectonics does not stop at life emergence:** Real planetary tectonics runs continuously for billions of years. Treating the Geological era as the only active window was incorrect for a god-mode worldbuilding tool that simulates full planetary history—continents must keep advecting, colliding, and eroding from 500 Myr through the end of the simulation. Life emergence marks a narrative era boundary, not the end of plate tectonics.

Geological-era interval is configurable via `WorldParameters.core.geology.tick_interval_overrides_years` (optional map per era; falls back to defaults). Prehistoric and Ancient intervals are fixed constants in v1.

Total tick count for default Earth-analog world:
- Formation: 1 tick
- Geological: (life_emergence_year - 0) / 500,000 = ~1,000 ticks (assuming life emerges at 500 million years)
- Prehistoric: (sapience_year - life_emergence) / 2,000,000 = ~2,000 ticks
- Ancient: (recent_boundary - sapience) / 10,000,000 = small number
- **Total: ~3,000-4,000 ticks over the full planetary history**

### 4.2 Per-Tick Steps

In order:

1. **Update plate motion accumulators.** For each plate, increment `accumulated_rotation_rad` by `motion_rate_rad_per_year * tick_interval_years`.
2. **Re-partition hexes to plates.** Recompute `WorldData.plate_id[hex]` for all hexes based on each plate's current effective position.
3. **Detect boundary hexes and classify boundary types** (§3).
4. **Apply boundary effects to elevation** (§5).
5. **Apply hot spot effects** (§7).
6. **Apply erosion** (§8).
7. **Check for plate reorganization events** (§4.5).
8. **Update sea level** (§4.6).
9. **Emit events** based on what happened this tick (§6).

Each step uses a distinct RNG stream (§4.4) for any randomness, ensuring tick determinism.

### 4.3 First-Tick Initialization

At year 0 (Formation era, one-time tick), tectonics performs:

1. Place `initial_major_plate_count` + `initial_minor_plate_count` seed hexes via the growth-based seeding algorithm in §2.2.
2. Assign each plate a type per `initial_continental_fraction`, biasing continental plates toward major slots.
3. Sample each plate's motion axis (uniform on sphere with constraints from §2.1) and rate (log-normal scaled by effective velocity scale from §2.4).
4. Grow plates outward as described in §2.2 to produce the initial partition → `plate_id` for every hex.
5. Set initial elevation: continental plates ~500m, oceanic plates ~-3500m, with small per-hex random variation (~±200m).
6. Set initial `bedrock_type`: continental plates start as `Igneous` (basement rock), oceanic plates as `OceanicCrust`.
7. Set initial `sea_level_m` to 0 (calibrated so that ocean basins fill but continents emerge).
8. Initialize `fertility` to 0.0 for all hexes.
9. Emit one `EventKind::WorldFormation` event.

(Note: planetary formation/cooling sequences — molten phase, ocean condensation — are deferred to a future doc per §17 item 7. Phase 1's first tick treats the world as already past those stages.)

### 4.4 RNG Streams

Tectonics uses these named streams (derived from `WorldRng::stream(name)`):

| Stream Name | Purpose |
|---|---|
| `tectonics.plate_seeds` | Initial plate seed hex selection |
| `tectonics.plate_axes` | Plate motion axis sampling |
| `tectonics.plate_rates` | Plate motion rate sampling |
| `tectonics.plate_types` | Continental vs oceanic assignment |
| `tectonics.initial_elevation_noise` | Per-hex initial elevation variation |
| `tectonics.reorganization_check` | Per-tick check for whether a plate reorganization occurs |
| `tectonics.reorganization_action` | If reorganizing, which plates and how |
| `tectonics.hotspot_locations` | Initial hot spot positions |
| `tectonics.hotspot_activity` | Per-tick activity at each hot spot |
| `tectonics.volcanism` | Stochastic volcanic eruptions at boundaries |
| `tectonics.erosion_noise` | Per-tick erosion variation |

Each is initialized once at plate creation and re-derived deterministically every tick. Different streams ensure that, e.g., tweaking volcanism logic doesn't change initial plate layout.

### 4.5 Plate Reorganization

Real plate tectonics is not static — plates split, merge, and change motion direction over hundreds of millions of years. Modeling this gives our worlds varied geological history (multiple supercontinent cycles, not just one static configuration).

Each Geological-era tick, with probability `0.001 * geology_activity_scale` (default ~once per 500 million simulated years), a reorganization event occurs. Reorganization is one of:

- **Plate split** (40% of events): A randomly-chosen large plate splits along a chosen axis. Creates a new plate with a related but distinct motion axis. Continental plate splits often produce a new ocean basin between the two halves.
- **Plate motion change** (40% of events): A randomly-chosen plate gets a new motion axis. Models the "the plate slowed down and changed direction" that happens in real Earth history.
- **Plate merger** (20% of events): Two adjacent plates merge into one. Often happens after extensive continental collision when the boundary effectively locks up.

Each reorganization emits an event with `Significance::Pivotal` (these are the supercontinent-cycle-defining moments).

### 4.6 Sea Level Drift

Per Doc 04 §3.3, sea level is variable (not fixed at zero). Tectonic activity affects ocean basin volume:

- More active divergent boundaries (mid-ocean ridges) → ridges displace water → sea level rises
- Less active periods → ridges subside → sea level falls

Each tick, sea level adjusts by a small amount derived from total divergent boundary length:

```
delta_sea_level_m = (current_divergent_length_km - baseline_divergent_length_km) * 1e-6
```

Plus a slow long-term trend toward equilibrium (sea level can't run away over billions of years). The result: sea level oscillates by tens of meters over geological time, with rare excursions of ±100m during major reorganizations.

## 5. Elevation Update Rules

### 5.1 Divergent Boundaries

At a divergent boundary, two plates separate. New crust forms in the gap.

For each boundary hex h at a divergent boundary:
- Elevation decreases toward the oceanic baseline (-3500m) at a rate proportional to `relative_velocity_magnitude * tick_interval_years`
- Specifically: `elevation_mean[h] -= velocity_cm_per_year * tick_interval_years * subsidence_rate`
- Where `subsidence_rate ≈ 2e-5 m per cm of separation` (calibrated to produce ~3 km deepening over 100 million years of sustained divergence)
- Bedrock changes to `OceanicCrust` if it was previously something else
- Plate ID is reassigned (the boundary hex now clearly belongs to whichever plate it's farther into)

If divergence happens within a continental plate (rifting), elevation drops more slowly and bedrock stays `Igneous` until the rift becomes oceanic (after ~50 million years of sustained divergence).

### 5.2 Convergent: Continental-Continental

Two continents collide. Crust crumples upward.

For each boundary hex h at a continental-continental boundary:
- `elevation_mean[h] += orogeny_rate * velocity_cm_per_year * tick_interval_years`
- Where `orogeny_rate ≈ 5e-5 m per cm of convergence` (calibrated to produce ~5 km elevation over 100 million years of sustained collision)
- `elevation_relief[h] += orogeny_rate * 0.3 * velocity_cm_per_year * tick_interval_years` (mountains get rougher)
- Bedrock changes to `Metamorphic` (collision metamorphism)

Effect spreads inland — hexes within 2-3 hexes of the boundary on the continental side also gain elevation, with falloff.

### 5.3 Convergent: Oceanic-Continental

Oceanic plate subducts under continental plate.

For each boundary hex h on the **oceanic side**:
- Elevation decreases sharply (forming a trench): `elevation_mean[h] -= subduction_rate * velocity * tick_interval`
- `subduction_rate ≈ 1e-4 m per cm` (calibrated to produce ~10 km trench over 100 million years of sustained subduction)

For each boundary hex h on the **continental side** (within 3 hexes inland):
- Elevation increases (coastal mountains)
- Volcanism is likely (see §5.5)
- Bedrock changes to `Igneous` (volcanic rock from arc magmatism)

**Coastal shelf (oceanic plate):** From the continental boundary hex, gentle subsidence spreads onto the oceanic plate for up to 2 hexes, using falloff fractions `[0.4, 0.15]` of the trench delta per ring. This produces a continental shelf → deep ocean gradient instead of an instant cliff at the plate boundary.

### 5.4 Convergent: Oceanic-Oceanic

One oceanic plate subducts under the other. Island arcs form on the upper plate.

For each boundary hex h on the **upper plate side**:
- Elevation increases sharply (volcanic islands forming)
- Bedrock changes to `Igneous`

For each boundary hex h on the **lower plate side**:
- Elevation decreases (the subducting trench); uses the same `subduction_rate ≈ 1e-4 m per cm` as §5.3

### 5.5 Volcanism (Boundary-Driven)

At convergent boundaries with subduction (oceanic-continental and oceanic-oceanic), stochastic volcanic eruptions occur each tick:

- Each boundary hex on the upper-plate volcanic arc side has a per-tick probability of a volcanic event
- Probability = `0.05 * volcanism_scale` (default ~5% per tick per boundary hex)
- When it fires:
  - Elevation increases by 100-500m at that hex (sampled from a distribution)
  - `elevation_relief[h] += 50-200m` (volcanoes have prominent peaks)
  - Bedrock stays/becomes `Igneous`
  - Emits a `VolcanicEruption` event

### 5.6 Transform Boundaries

Transform boundaries (sliding) don't change elevation significantly, but they affect bedrock:
- Bedrock changes to `Metamorphic` over long durations (transform fault metamorphism)
- No event emission (these are continuous, not punctuated)

### 5.7 Elevation Bounds

Elevation is clamped to a physically plausible range:

- `MIN_ELEVATION_M = -11_000.0` (Marianas Trench depth)
- `MAX_ELEVATION_M = 9_000.0` (slightly above Everest)
- `MAX_RELIEF_M = 5_000.0`

Bounds prevent runaway accumulation from poorly-tuned parameters. If a boundary somehow generates 50 km of elevation, we clamp and log a warning in debug builds.

## 6. Event Schema

This section defines what tectonic events look like and introduces the **user-tunable granularity system**.

### 6.1 New EventKind Variants

Add to `EventKind` in `genesis_core::events::kinds`:

```rust
pub enum EventKind {
    Placeholder { description: String },  // existing
    
    // Tectonic events (Phase 1)
    WorldFormation,
    PlateReorganization {
        action: PlateReorgAction,
        affected_plates: Vec<PlateId>,
    },
    MountainRangeFormed {
        boundary_hexes: Vec<HexId>,
        plates: (PlateId, PlateId),
        peak_elevation_m: f32,
    },
    OceanBasinOpened {
        boundary_hexes: Vec<HexId>,
        plates: (PlateId, PlateId),
    },
    VolcanicEruption {
        hex: HexId,
        elevation_change_m: f32,
        plate: PlateId,
    },
    HotSpotActivity {
        hex: HexId,
        hot_spot_id: HotSpotId,
        elevation_change_m: f32,
    },
    BoundaryTransition {
        hex: HexId,
        from: BoundaryType,
        to: BoundaryType,
    },
    SeaLevelChange {
        delta_m: f32,
        new_sea_level_m: f32,
    },
}

pub enum PlateReorgAction {
    Split { parent: PlateId, child: PlateId },
    Merge { absorbed: PlateId, into: PlateId },
    MotionChange { plate: PlateId, new_axis: Vec3, new_rate: f64 },
}
```

### 6.2 Significance Assignment

Each emitted event gets a `Significance` value indicating how noteworthy it is. Significance is fixed per event variant:

| Variant | Significance | Rationale |
|---|---|---|
| `WorldFormation` | `Pivotal` | The world begins |
| `PlateReorganization` | `Pivotal` | Supercontinent-cycle-defining moments |
| `MountainRangeFormed` | `Major` | Continental-scale geographic features |
| `OceanBasinOpened` | `Major` | New oceans matter for climate and life |
| `VolcanicEruption` (peak > 2000m) | `Notable` | Significant volcanic peaks |
| `VolcanicEruption` (peak ≤ 2000m) | `Minor` | Smaller eruptions |
| `HotSpotActivity` (cumulative > 1km) | `Notable` | Island chains forming |
| `HotSpotActivity` (smaller) | `Trace` | Individual hot spot pulses |
| `BoundaryTransition` | `Trace` | Subtle, gradual changes |
| `SeaLevelChange` (> 50m) | `Notable` | Major sea level excursions |
| `SeaLevelChange` (smaller) | `Trace` | Background drift |

### 6.3 The Granularity System

Per the design discussion: we want to be able to log fine-grained events for analysis, but not blow up save file sizes for ordinary use. The mechanism is a **per-layer event granularity threshold** in `WorldParameters`.

Add to `WorldParameters.core.geology`:

```rust
pub struct GeologyParameters {
    // ---- Existing (from Doc 04 §4.7) ----
    pub initial_continental_fraction: f32,    // Default 0.29 (Earth)
    pub plate_velocity_scale: f32,             // Default 1.0
    pub volcanism_scale: f32,                  // Default 1.0
    
    // ---- New for Phase 1 ----
    
    /// Number of major (large) plates. Default 7. Valid 6-9.
    pub initial_major_plate_count: u8,
    
    /// Number of minor (smaller) plates. Default 8. Valid 6-10.
    pub initial_minor_plate_count: u8,
    
    /// Minimum event significance to log during tectonic simulation.
    /// Events below this threshold are computed and applied to world state
    /// but NOT recorded in the event log. Default `Notable`.
    pub event_granularity: Significance,
    
    /// Admin/debug override for tick interval per era. None = use defaults
    /// from §4.1 table. Not exposed in user UI for v1.
    pub tick_interval_overrides_years: Option<BTreeMap<Era, i64>>,
    
    /// Base erosion rate per year per meter of elevation above sea level.
    /// Default 1e-7. Climate modifies via climate_modifier (Phase 2).
    pub base_erosion_rate_per_year: f64,
}
```

Default values are calibrated for Earth-analog worlds. Validation rules (added to `parameters/validation.rs`):

- `initial_major_plate_count`: 6..=9
- `initial_minor_plate_count`: 6..=10
- `base_erosion_rate_per_year`: positive, finite, < 1e-3

(`initial_plate_count` from the existing schema is removed in favor of major/minor split. Note in changelog.)

Effect:
- At `Significance::Trace`: log everything. Save file grows substantially. Useful for debugging or for users who want every detail.
- At `Significance::Minor`: log Minor and above. Skips Trace events (mostly background sea level drift, hot spot pulses).
- At `Significance::Notable` (default): log Notable and above. Reasonable middle ground.
- At `Significance::Major`: only the big stuff (mountain ranges, ocean basins, reorganizations).
- At `Significance::Pivotal`: only the era-defining moments. Smallest save.

Implementation in the tectonics layer:

```rust
fn maybe_emit(&mut self, event: Event, world: &WorldData) {
    if event.significance >= world.parameters.core.geology.event_granularity {
        // Emit to event log
        self.events_this_tick.push(event);
    }
    // Below threshold: still computed, but not logged
}
```

This lets us:
1. Implement the full event taxonomy (every variant)
2. Measure save file sizes at each granularity level during testing
3. Make an informed choice about defaults based on real data
4. Give power users the option to crank it up for analysis

### 6.4 Event Volume Estimates

At default granularity (`Notable`), expected event counts over 4.5 billion years:

| Event Variant | Estimated Count |
|---|---|
| `WorldFormation` | 1 |
| `PlateReorganization` | 5-15 |
| `MountainRangeFormed` | 20-50 |
| `OceanBasinOpened` | 10-30 |
| `VolcanicEruption` (Notable only) | 500-2,000 |
| `HotSpotActivity` (Notable only) | 100-500 |
| `SeaLevelChange` (Notable only) | 10-30 |
| **Total at Notable** | **~700-2,600 events** |

At `Trace`: roughly 50-200x more events (most of the volume from `VolcanicEruption (Minor)` and per-tick `SeaLevelChange (Trace)`).

These are estimates; Phase 1 implementation will produce real numbers we can use to refine the granularity defaults.

## 7. Hot Spots

### 7.1 Hot Spot Model

Real Earth has ~40-50 hot spots (Hawaii, Iceland, Yellowstone). They're persistent thermal anomalies in the mantle that punch through whatever plate is currently above them. As plates move, hot spots produce volcanic chains.

For our simulation:

```rust
pub struct HotSpot {
    pub id: HotSpotId,
    pub anchor_position: Vec3,   // Fixed in the world frame; doesn't move with plates
    pub activity_rate: f64,      // Per-tick probability of an eruption when a plate is above
    pub age_year: WorldYear,
    pub lifespan_year: WorldYear, // Hot spots eventually die
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct HotSpotId(pub u16);
```

### 7.2 Hot Spot Generation

At world formation:
- `num_hot_spots = round(8 + 16 * (planet_radius_km / earth_radius_km))` — about 12-20 for an Earth-sized world
- Positions sampled uniformly on the sphere via the `tectonics.hotspot_locations` stream
- Each gets a random `activity_rate` (0.01 to 0.1 per tick) and a `lifespan_year` (100M to 1B years)

### 7.3 Hot Spot Dynamics

Each tick:
1. For each hot spot still alive:
   - Find the hex currently above the hot spot's anchor position
   - With probability `activity_rate`, an eruption occurs
   - Eruption raises elevation by 100-1000m, similar to boundary volcanism
   - Emits `HotSpotActivity` event
2. If `current_year - age_year > lifespan_year`, the hot spot dies (removed from active list)
3. Rare new hot spots form (probability `0.0001` per tick) to maintain a roughly stable count over time

### 7.4 Tracks

Hot spot tracks (island chains like Hawaii) emerge naturally from the model: as plates move over the hot spot, the eruption location relative to the plate shifts, producing a linear chain of volcanic features in the plate's frame of reference. We don't need to explicitly model "tracks" — they're an emergent consequence.

## 8. Erosion and Sediment Tracking

### 8.1 Why Erosion Matters

Without erosion, every continental collision adds elevation forever. Earth's mountains have been eroding since they formed; the Appalachians used to be Himalayan-scale. A world without erosion would have implausibly tall mountains everywhere there's ever been a convergent boundary.

### 8.2 Erosion Model

Per tick, each hex with elevation above sea level erodes:

```
erosion_amount_m = elevation_above_sea * base_erosion_rate * climate_modifier(hex) * tick_interval_years
```

Where:
- `base_erosion_rate ≈ 1e-7` per year (calibrated so 5 km of mountain erodes meaningfully over ~100 million years; see open question 3)
- `elevation_above_sea = elevation_mean - sea_level_m`
- `climate_modifier(hex)` is a multiplier from precipitation; default 1.0 when climate is not yet active
- Hexes below sea level don't erode (handled by sediment deposition instead)

**Climate feedback (active in Phase 2):** Once climate is simulated (Phase 2), it ticks before tectonics each Geological-era cycle. Climate writes to `WorldData.precipitation` and `WorldData.temperature_mean`. Tectonics reads these to compute `climate_modifier`:

```
climate_modifier = (precipitation_mm_per_year / 800.0)  // 800 mm/yr is Earth global average
                 * temperature_factor                    // hot or freeze-thaw accelerates erosion
```

In Phase 1 (no climate yet), `climate_modifier = 1.0` uniformly. The tectonics implementation reads from `precipitation` and `temperature_mean` regardless — the field is just always at default values in Phase 1. Phase 2 makes them dynamic, and erosion responds without any code change to tectonics.

Eroded material is distributed to lower-elevation neighbors (the simplest "downhill flow" model). Phase 2 (Hydrology) will refine this with proper drainage networks. For now: each tick, eroded mass moves one hex toward the lowest neighbor.

### 8.3 Sedimentary Bedrock Formation

When eroded material accumulates on a hex with `BedrockType::Igneous` or `BedrockType::Metamorphic`, the bedrock changes to `BedrockType::Sedimentary` over time. Specifically:

- Track cumulative deposition per hex (transient state in the tectonics layer, not stored in `WorldData`)
- When cumulative > threshold (e.g., 500m of accumulated material), bedrock transitions to `Sedimentary`
- This is what creates "fertile ancient seabeds" — areas now above water that used to be below it, with thick sediment

### 8.4 Limestone, Fertility, and the Cretaceous Beach Mechanic

Per the design discussion, **proper limestone formation requires biological deposition (corals, shells)** — which is a Phase 4 (Biology) concern, not tectonics. Tectonics cannot meaningfully decide "this is limestone" without knowing whether sufficient marine biology was present.

What Phase 1 tectonics CAN do:

1. **Track per-hex fertility as a static accumulator.** Add to `WorldData`:
   ```rust
   /// Bio-deposit accumulator. Increased per tick when hex is below sea level
   /// in warm latitudes (the conditions where carbonates and organic matter
   /// accumulate). Static once set — represents historical conditions.
   /// Phase 4 (Biology) refines accumulation rate based on actual biomass.
   pub fertility: Vec<f32>,  // 0.0 to 1.0, monotonically increasing
   ```

2. **Increment fertility per tick** for hexes that are:
   - Below sea level (`elevation_mean < sea_level_m`)
   - In tropical/subtropical latitudes (current |lat| < 30°)
   - Shallow (water depth < 200m above the hex — proxy for "shallow shelf seas")
   
   Increment by a small per-tick amount (e.g., 0.001 per tick). After ~1000 ticks (500 million years) of these conditions, fertility approaches 1.0.

3. **Do not change `BedrockType` to `Limestone` in Phase 1.** Bedrock stays `Sedimentary` for these accumulating shelf-sea hexes. Phase 4 will introduce the bedrock transition based on full biology dynamics — by the time those hexes drift inland, they'll have the high fertility tag indicating "this region was a shallow sea for a long time."

The key property your design wants: **fertility is monotonic and static**. Once a hex has accumulated fertility, drifting north out of the tropics doesn't decrease it. The biological deposits are already there. This is correctly modeled by "increment per tick, never decrement" — exactly the historical-latitude tracking you described.

Phase 4 will use the fertility field to make biology and biome decisions ("this region has rich soil because it was a shallow sea long ago"). Phase 5 (Civilization) will use it for settlement and population — the "fertile crescent" hexes are where civilizations cluster.



## 9. Performance Targets

### 9.1 Time Budget

For a default Earth-analog world at subdivision level 7 (~22K hexes):
- **Total tectonic simulation:** 60 seconds or less on target hardware (M-series MacBook Pro)
- **Per-tick cost at Geological era:** 15-30ms (for ~3,000 ticks at this rate)
- **Initial plate generation:** under 200ms
- **Memory overhead:** under 10MB beyond `WorldData`

At subdivision level 8 (~65K hexes): targets multiply by ~3x. Still acceptable.

### 9.2 What Makes This Fast

- All hex operations are O(1) lookups in `Vec<f32>` arrays (no allocations per hex per tick)
- Plate-to-hex Voronoi recomputation only when accumulated motion exceeds a threshold (not every tick)
- Boundary detection iterates hexes once, classifying via neighbor lookups
- Hot spot count is small (~20), checks are negligible
- Erosion is per-hex but parallelizable (Phase 1 keeps it sequential; can parallelize later)

### 9.3 Measurement

Phase 1 ships with built-in profiling:
- Per-tick timing logged when `RUST_LOG=genesis_tectonics=debug` is set
- Per-step timing within a tick (motion update, partition, boundary, elevation, hot spots, erosion)
- Total simulation time logged at `info` level on completion

Performance regressions will be caught by a `tectonics_full_history_completes_within_budget` test in CI.

## 10. Determinism Requirements

Standard Genesis Engine determinism rules apply (Doc 04 §6):

1. All randomness via `WorldRng::stream(name)` with named streams (§4.4)
2. All collections sorted before iteration where order matters (BTreeMap, BTreeSet — never std HashMap)
3. Floating-point math in f64 for accumulating quantities, f32 acceptable for per-hex storage
4. No reliance on wall-clock time anywhere in the simulation
5. Plate IDs and Hot Spot IDs assigned in deterministic order

Additional tectonics-specific rules:

6. **Reorganization events** use the `tectonics.reorganization_check` stream for the per-tick probability check, then `tectonics.reorganization_action` for which plates and how. Two streams ensures that changing the reorganization probability doesn't shift downstream plate selection.
7. **Plate motion** is computed in f64 and stored in f32 — accumulation happens in f64 to avoid drift over thousands of ticks.
8. **Re-partition order:** when iterating hexes for Voronoi assignment, iterate by `HexId` ascending. This ensures ties (a hex equidistant from two plates) break deterministically.

A snapshot test must verify: same seed produces byte-identical `WorldData` after full geological simulation.

## 11. Validation Criteria

How do we know tectonics is producing plausible output? These are sanity checks, not unit tests.

After running full geological simulation on a default Earth-analog world, the result should satisfy:

1. **Continental fraction:** 25-35% of hexes are above sea level (Earth: ~29%)
2. **Plate count:** Final plate count is between 5 and 15 (started with 8, may have split/merged)
3. **Mountain ranges exist:** At least 3 distinct contiguous regions of elevation > 3000m
4. **Ocean basins exist:** At least 1 contiguous region of elevation < -3000m covering > 1000 hexes
5. **Bedrock diversity:** All 6 `BedrockType` variants are present in the final world
6. **No runaway elevation:** Maximum elevation < 9000m, minimum > -11000m
7. **Sea level plausible:** Final `sea_level_m` is within ±200m of 0
8. **Event count sensible:** At default `Notable` granularity, event count is 500-3000 (loose bounds)

Implementation tests verify each of these against a fixed seed.

## 12. Edge Cases and Open Questions

### 12.1 What if a plate has zero hexes?

After Voronoi re-partition, a plate could theoretically have no hexes (if other plates have grown to surround it). For v1: a plate with zero hexes for 10M+ years is considered "extinct" and removed from the active plate list. Its `PlateId` is never reused.

### 12.2 Multi-plate convergent boundaries (triple junctions)

A hex can have neighbors from 2 or more different plates. The boundary classification then runs pairwise; the hex applies effects from all classifications additively. Triple junctions are where particularly active geology happens — this naturally produces complex boundary regions in our simulation.

### 12.3 Hot spot vs boundary volcanism interactions

A hot spot under a divergent boundary (like Iceland) would have both processes active. We let both apply additively. Result: extra-volcanic regions where these overlap. Plausible.

### 12.4 Initial plate seeding near pentagons

Pentagons have 5 neighbors instead of 6. If a plate's seed hex is a pentagon, nothing special happens — Voronoi partitioning doesn't care about neighbor count. Boundary detection naturally handles the 5-neighbor case.

### 12.5 What if `event_granularity` is set above `Pivotal`?

Then no events get logged. World state is still computed correctly, but the chronicle is empty. This is an extreme but valid configuration ("simulate but don't record"). Phase 1 implementation should not crash on this; tests verify.

## 13. File Organization

Implementation lives in `crates/genesis_tectonics/`:

```
genesis_tectonics/
├── Cargo.toml
└── src/
    ├── lib.rs              # public API + TectonicsLayer impl
    ├── plate.rs            # Plate, PlateType, Plate registry
    ├── motion.rs           # Plate motion math (rotation about axis)
    ├── partition.rs        # Voronoi partition (hex → plate)
    ├── boundary.rs         # Boundary detection and classification
    ├── elevation.rs        # Per-boundary-type elevation update rules
    ├── volcanism.rs        # Boundary-driven and hot spot volcanism
    ├── hotspots.rs         # Hot spot model
    ├── erosion.rs          # Erosion and sedimentation
    ├── reorganization.rs   # Plate split / merge / motion change
    └── events.rs           # Event emission and granularity gating
```

Depends on:
- `genesis_core` (data structures, RNG, time, events)
- `glam` (vector math, already pulled in via genesis_core's grid)

No Bevy dependency. The tectonics layer is engine-agnostic.

## 14. Out of Scope for Phase 1

Explicitly NOT included; deferred to later phases:

- **Climate effects on erosion:** Phase 2 (Climate) will introduce climate-dependent erosion rates (more rain → more erosion). Phase 1 uses uniform erosion.
- **Soil composition:** Doc 08 (Hydrology & Soil). Bedrock type sets the stage; soil is built on top.
- **Magnetic field and pole reversals:** Cool worldbuilding hook but not relevant for any v1 simulation.
- **Detailed mineral composition:** Beyond the 6 BedrockType variants, no individual mineral tracking.
- **Realistic timescales for non-Earth planets:** A radically different planet radius or gravity could justify scaled tectonic rates. Phase 1 treats `planet.radius_km` and `planet.gravity_g` as informational only.
- **Tidal forces from moons:** Some real geophysics ties tides to plate motion. We ignore this.

## 15. Implementation Plan (Phase 1 Sub-Steps)

Like Phase 0 was broken into 8 implementation prompts, Phase 1 will be broken into sub-steps. Preliminary breakdown:

1. **Plate generation and storage** — `Plate` struct, initial seeding, partition
2. **Plate motion and re-partition** — motion math, accumulated rotation, partition refresh
3. **Boundary detection and classification** — identifying and typing boundaries
4. **Elevation updates per boundary type** — the core dynamics
5. **Hot spots** — separate from boundary dynamics
6. **Erosion and bedrock evolution** — closes the elevation loop
7. **Plate reorganization** — split, merge, motion change
8. **Event emission and granularity gating** — the chronicle
9. **Integration and validation** — register with `TickCoordinator`, run full history, verify validation criteria
10. **Rendering integration** — `genesis_render` learns to color hexes by elevation

Each step will be a separate prompt with its own spec, tests, and review cycle, following the Phase 0 process.

## 16. Implementation Notes for the AI Agent

Per Doc 04 §16, this section addresses agents implementing the spec.

1. **Read this entire doc** before starting any sub-step. The pieces interact — you can't implement boundaries without understanding plate motion, can't do elevation without understanding boundaries.
2. **Use `BTreeMap` and `BTreeSet`** for all collections. Never `std::HashMap`.
3. **Each new sub-prompt will reference specific sections of this doc.** If a prompt seems to contradict this doc, surface the contradiction before resolving it.
4. **Performance is a feature.** If the easiest implementation is slow, that's still acceptable for first-pass; we'll optimize after correctness. But report timings.
5. **Surface every assumption** that goes beyond what's specified here. Default values, parameter ranges, calibration constants — flag them in your summary so we can refine the doc.

## 17. Open Questions for Doc Review

Items deliberately deferred or still uncertain:

1. **Tick intervals (§4.1):** Default 500K-year Geological ticks. May need adjustment based on observed quality. **Tunable via parameters as an admin/debug knob; not exposed in user UI for v1.** Plan: add `WorldParameters.core.geology.tick_interval_overrides_years: Option<BTreeMap<Era, i64>>` so devs can experiment, default `None` uses the table above. (Status: noted, implement in Phase 1.)

2. **Plate count defaults (§2):** ✅ Resolved. 7 major + 8 minor (configurable 6-9 major, 6-10 minor).

3. **Erosion rate calibration (§8.2):** Bumped to `1e-7` per year as a starting estimate. **Climate-aware in Phase 2** via `climate_modifier`. Calibrate during Phase 1 implementation by checking that mountain ranges erode visibly but don't disappear in geological-era timeframes.

4. **Limestone formation (§8.4):** ✅ Resolved. Phase 1 tracks fertility (a static monotonic accumulator); Phase 4 handles biological deposition and the `Limestone` bedrock transition.

5. **Hot spot lifespan (§7.2):** Still rough. 100M-1B years feels right; empirical tuning during Phase 1.

6. **Should the validation criteria (§11) be unit tests, or run only manually?** Recommendation: **both**. Implement as tests with loose tolerances (e.g., "continental fraction is 0.20-0.40" rather than "exactly 0.29"). This catches drastic regressions without false positives from seed variation. Manual review for visual sanity.

7. **Planetary formation / cooling sequence (new):** Pre-tectonic state setup — molten planet cooling, ocean condensation, initial sea level rise from ~−5000m to current — is a real worldbuilding concern but not strictly tectonics. **Deferred to its own future doc (likely paired with Doc 07 Climate, since cooling and ocean formation are climate-tectonics interactions).** The first Formation-era tick in Phase 1 can include placeholder logic: instantaneously set initial elevations and sea level; future doc replaces with a multi-tick cooling sequence.

8. **Chaos mode (new):** Worth considering as a global toggle that relaxes physics constraints — wild plate motion, multiple life-emergence events, etc. **Deferred to its own future doc**, noted here so we don't lose it. Likely a `chaos_intensity: f32` parameter in core geology, climate, biology each.

These get resolved during Phase 1 implementation. Right now they're noted as deliberately open.

---

*End of Doc 06 v0.2.*

*Next step: implementation prompt 1.0 (Phase 1) — initial plate generation and partition.*
