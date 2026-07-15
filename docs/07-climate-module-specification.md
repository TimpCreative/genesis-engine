# 07 — Climate Module Specification

**Document Type:** Tier 2 — System Specification
**Status:** Draft v0.2
**Last Updated:** May 2026
**Owner:** Brax Johnson
**Implementing Phase:** 2 (Climate & Hydrology)

**Changelog:**
- v0.2 (July 2026): **Phase 2 complete (P2-10–P2-12).** Milankovitch-like orbital cycles implemented with STRETCHED periods (short 2.3M years, long 4x): real ~100k-year cycles are sub-tick at the 500k-year Geological cadence and sample as anti-correlated noise, so the model uses periods the tick rate can resolve (deviation from §12.1 noted). Glaciation state machine with asymmetric thresholds — deep-cold onset (-2.2°C), soft continuation (-0.8°C), shallow exit (+0.3°C) — so glacials are episodic excursions rather than half of history; emits Pivotal GlaciationBegan/GlaciationEnded events. Orbital modifier feeds the temperature field globally. Köppen-like regime classification (§10.2 decision tree) writes WorldData.climate_regime each tick (ocean hexes stay Unset); new Climate Regime render mode and screenshot step. Erosion climate feedback (§13 / Doc 06 §8.2) active: precipitation/800mm modifier with frozen damping. Validation §17 #5 (≥7 regimes) and #9 (≥1 glaciation cycle per 1B years) covered by tests.
- v0.1 (May 2026): Initial draft. Defines planetary formation sequence, temperature, atmospheric circulation, precipitation, ocean circulation, climate-tectonics feedback, climate variability/ice ages, and event schema.

---

## 1. Purpose and Scope

This document specifies the **climate simulation layer** — Phase 2 of Genesis Engine. Climate sits on top of tectonics (Phase 1) and produces the per-hex temperature, precipitation, and wind patterns that biology (Phase 4) and civilization (Phase 5) will read.

### 1.1 Goals

Climate must:

1. **Produce plausible per-hex climate values** — mean annual temperature, precipitation, prevailing wind direction, seasonal variation — given the current world state (elevation, plate positions, latitude, planetary parameters)
2. **Cover the entire simulated history**, from the molten formation phase through the present-day climate, with continuous evolution as continents drift and atmospheric composition changes
3. **Couple back into tectonics** through the climate-feedback erosion hook stubbed in Phase 1 (high-precipitation regions erode faster; arid regions retain elevation)
4. **Run at acceptable speed** — climate ticks alongside tectonics in the Geological era and may tick more frequently in later eras when shorter-timescale variability matters
5. **Stand up to light scientific scrutiny** — atmospheric circulation cells, monsoons, rain shadows, ocean current effects on coastal climates, ice ages all behave the way a geography hobbyist would expect
6. **Be deterministic** — same seed produces byte-identical climate evolution

### 1.2 Non-Goals

This is explicitly NOT a research-grade climate model. We are not modeling:

- Daily weather, individual storms, hurricane tracks
- Atmospheric chemistry beyond a single composite "greenhouse forcing" parameter
- Detailed cloud physics, aerosol effects
- Stratospheric circulation, jet stream meanders
- Specific Earth historical reconstructions (Younger Dryas, Holocene optimum, etc.)
- Thermohaline circulation in detail (we model surface gyres only)
- Tidal effects on climate
- Photosynthetic atmospheric oxygenation as a coupled system (Phase 4 Biology handles this loosely)
- Anthropogenic climate change from civilizations (Phase 5 will add a hook)

If a player is a climate scientist, they will find things to nitpick. That's acceptable. The goal is plausible worldbuilding, not academic accuracy.

### 1.3 Dependencies

**Reads from:**
- `WorldData.elevation_mean` (drives orographic precipitation, lapse rate)
- `WorldData.sea_level_m` (determines coastal vs. continental hexes)
- `WorldData.plate_id` and `plate_origin` (continents persist with their plates; climate over time follows them)
- `WorldData.parameters.core.planet` — radius, rotation_period_hours, axial_tilt_deg, orbital_eccentricity, solar_luminosity_relative_to_sol (some of these may be new — added by Phase 2 if not present in Phase 0/1)
- `WorldData.parameters.core.climate` — new struct added in Phase 2

**Writes to:**
- `WorldData.temperature_mean_c` (per-hex annual mean temperature, Celsius)
- `WorldData.temperature_range_c` (per-hex annual seasonal range)
- `WorldData.precipitation_mm` (per-hex annual precipitation, mm)
- `WorldData.wind_direction_rad` (per-hex prevailing wind, radians from north, clockwise)
- `WorldData.wind_speed_m_s` (per-hex mean wind speed, m/s)
- `WorldData.ocean_current_vec` (per-hex ocean current vector, m/s) — for ocean hexes only
- `WorldData.distance_to_ocean_km` (per-hex distance to nearest ocean) — computed by climate but useful broadly
- `WorldData.climate_regime` (per-hex regime label — Tropical, Subtropical, Temperate, Boreal, Polar, etc.)
- `WorldData.atmospheric_composition` (global state: CO2 ppm, water vapor index, oxygen fraction — coarse single-value tracking)
- `WorldData.global_mean_temperature_c` (global average for trend tracking)

Many of these are new bulk arrays added in Phase 2. The Phase 0 `WorldData` had placeholder fields for some (`temperature_mean`, `precipitation`); Phase 2 replaces or extends them.

**Produces events** per §11.

### 1.4 What Phase 2 Delivers

Concrete deliverables for Phase 2:

1. This document (Doc 07) — fully specified before implementation begins
2. New crate `genesis_climate` implementing all climate logic, registered as a `SimulationLayer`
3. New bulk arrays added to `WorldData` (climate fields listed above)
4. Climate-tectonics feedback realized: tectonics reads precipitation and updates its `climate_modifier` for erosion
5. Rendering: `genesis_render` learns to display per-hex temperature, precipitation, and biome overlays (selectable via key bind)
6. Validation: per Doc 06 §11 pattern, a set of metrics that catch drastic regressions

### 1.5 Estimated Sub-Prompt Count

Phase 1 tectonics took ~13 sub-prompts (P1-1 through P1-13). Phase 2 climate is broader; estimate **15-20 sub-prompts** broken roughly along the section boundaries of this doc:

1. Planetary formation sequence (§3) — initial atmospheric and ocean state, multi-tick cooling
2. Temperature model (§4) — insolation, lapse rate, ocean buffering
3. Distance-to-ocean field (§5) — continental vs. maritime gradient
4. Atmospheric circulation cells (§6) — Hadley/Ferrel/polar from rotation rate
5. Wind field (§7) — per-hex prevailing wind from circulation
6. Ocean surface currents (§8) — gyres from wind + basin geometry
7. Precipitation (§9) — base + orographic + monsoon + ocean current effects
8. Climate regimes/biomes proto (§10) — Köppen-like classification
9. Atmospheric composition over time (§11) — outgassing, weathering, biological feedback hooks
10. Long-period variability — orbital cycles, ice ages (§12)
11. Climate-tectonics feedback wire-up (§13)
12. Eccentricity and chaos mode hooks (§14)
13. Save/load round-trip
14. Rendering — temperature heatmap, precipitation overlay, biome view
15. Validation criteria implementation
16. Performance tuning
17-20. Reserved for integration, follow-ups, and any sub-system that turns out larger than expected

---

## 2. Architectural Overview

Climate is more interconnected than tectonics. Where tectonics has a relatively simple causal chain (plate motion → boundaries → elevation), climate has bidirectional couplings: temperature affects circulation, circulation affects ocean currents, ocean currents affect coastal temperature, all of them affect precipitation, and precipitation feeds back to tectonics via erosion.

We resolve this by computing climate in a **fixed sequence per tick** that approximates steady-state for the current world configuration. This is the standard approach for slow-tick climate models — we don't simulate weather minute-by-minute; we ask "given current elevation, plate positions, atmospheric composition, and solar input, what's the average climate at each hex?"

### 2.1 Per-Tick Climate Sequence

In order, per Geological tick (after tectonics has updated the world for this tick):

1. **Insolation update** — compute solar input per latitude given current axial tilt, orbital eccentricity, and luminosity
2. **Atmospheric circulation** — compute circulation cell boundaries and intensities given rotation rate and pole-equator temperature gradient
3. **Distance-to-ocean** — recompute per-hex distance to nearest ocean (continents have drifted; this changes)
4. **Base temperature** — compute per-hex temperature from insolation, elevation, latitude, continentality
5. **Wind field** — derive per-hex wind direction and speed from circulation cells
6. **Ocean surface currents** — compute current vectors from wind and basin geometry
7. **Temperature adjustment from ocean currents** — coastal hexes get adjusted by currents (warm currents warm coasts, cold currents cool them)
8. **Precipitation** — compute per-hex precipitation from temperature, wind, elevation, and ocean proximity
9. **Climate regime assignment** — classify each hex into a Köppen-like regime
10. **Atmospheric composition update** — slow drift over time (outgassing, weathering)
11. **Long-period modifiers** — apply ice-age regime shifts if active
12. **Climate-tectonics feedback** — update `climate_modifier` field that tectonics will read next tick
13. **Event emission** — record significant climate changes (regime shifts, ice ages, drought periods)

Each step depends only on prior steps and on `WorldData` produced by tectonics. No step references future steps. This makes the computation single-pass per tick.

### 2.2 Climate Ticks vs. Tectonic Ticks

Climate and tectonics both remain **active through Prehistoric and Ancient**. Tectonics uses era-appropriate coarser intervals in later eras (Doc 06 §4.1); climate ticks more frequently where shorter-timescale variability matters:

| Era | Tectonic Tick | Climate Tick |
|-----|---------------|--------------|
| Formation | 1 at year 0 (P1); extended in P2-2 | 5M years (see §3) |
| Geological | 500K years | 500K years |
| Prehistoric | 2M years | 500K years (more frequent than tectonics) |
| Ancient | 10M years | 100K years (much more frequent) |
| Recent | dormant | 1K years |

**Why climate does not stop at life emergence:** Real planetary climate runs continuously for billions of years. Life emergence marks a narrative era boundary, not the end of physical simulation. A god-mode worldbuilding tool simulating full planetary history must keep climate evolving through Prehistoric and Ancient even when tectonic change slows.

Runtime controls live in `WorldParameters.core.climate` (event granularity, formation skip, eccentricity, chaos). Initial boundary conditions remain in `climate_initial`.

This is an important design point: in later eras, tectonics changes the world slowly, but climate keeps evolving (ice ages, regional changes, anthropogenic effects later). Climate ticks at the cadence that matters for its era.

Implementation note: both climate and tectonics register as `SimulationLayer` with their own `tick_interval` per Era. The tick coordinator (Doc 04 §7.3) handles dispatch. Climate registers **after** tectonics so each climate tick sees updated terrain (§13).

Approximate climate tick counts (default Earth-analog):
- Geological: (life_emergence_year) / 500,000 ≈ 1,000
- Prehistoric: (sapience_year - life_emergence) / 500,000 ≈ 8,000
- Ancient: (recent_boundary - sapience) / 100,000 = small number

### 2.3 Climate Layer State

A `ClimateState` struct (analogous to `TectonicsState` from Phase 1) holds:

- `pending_events: Vec<Event>` — flushed per tick
- `next_event_id: u64`
- `atmospheric_composition: AtmosphericComposition` — current global state
- `cumulative_orbital_phase_rad: f64` — for Milankovitch-like cycles
- `current_glaciation: GlaciationState` — global glaciation regime
- `previous_regime: Vec<Option<ClimateRegime>>` — per-hex regime tracking for regime-shift event emission
- Various derived caches recomputed each tick

`ClimateState` is not serialized as part of `WorldData`. It's held alongside `TectonicsState` at the app layer and reconstituted from `WorldData` snapshots if needed.

---

## 3. Planetary Formation Sequence

The first major chunk of climate work is modeling the planet's transition from initial molten state to a stable climate able to support life. This was deferred from Phase 1 to here because the dynamics are climate-dominated, not tectonic.

### 3.1 Why Formation Matters

A real planet doesn't begin with tectonics already running on a cool surface. The sequence is:

1. **Hadean** (first ~500M years): surface partially molten, intense bombardment, no liquid water, dense CO2/H2O steam atmosphere
2. **Cooling phase**: surface temperature drops, water vapor begins to condense
3. **Ocean formation**: liquid water accumulates as the surface temperature crosses below 100°C; takes millions of years to fill basins
4. **Stable climate**: sea level rises from initial dry state to current; atmospheric composition stabilizes as CO2 gets sequestered into limestone (after biology emerges, but tectonics handles this loosely via the fertility field)

For our worldbuilding tool, the user should be able to scroll to year 100M and see "the oceans are still filling," scroll to year 500M and see "young oceans, early life arrival window," scroll to year 2B and see "established climate, multicellular life arriving soon" — each frame plausible.

### 3.2 Formation Era Tick Schedule

The Formation era is no longer "1 tick at year 0" (the Phase 1 simplification). It now covers years 0 through `formation_end_year` (default 500M years), with the following sub-phases:

| Sub-phase | Year Range | Tick Interval | Description |
|-----------|------------|---------------|-------------|
| Molten | 0 – 50M | 5M years | Surface above boiling point; no water; tectonics dormant |
| Cooling | 50M – 200M | 5M years | Surface dropping toward 100°C; tectonics begins; water vapor in atmosphere |
| Condensation | 200M – 350M | 5M years | Water vapor condenses; rain falls for millions of years; oceans form |
| Stabilization | 350M – 500M | 5M years | Ocean basins fill; atmospheric composition begins to drift toward modern values |

`formation_end_year` is a parameter; default 500M. After it, the Geological era begins with its 500K-year tectonics ticks.

Per Doc 06, tectonics is dormant in the Molten sub-phase and active starting in Cooling. This is a Phase 2 addition — Phase 1 had tectonics active from year 0.

### 3.3 Planetary Cooling Model

We track `global_mean_temperature_c` as a global state value, evolving per tick during Formation. The model:

```
T(t) = T_inf + (T_initial - T_inf) * exp(-t / tau)
```

Where:
- `T_initial = 2000.0°C` (molten surface temperature; rough)
- `T_inf = 15.0°C` (equilibrium target after formation)
- `tau = 80M years` (cooling time constant; tuned to produce ~500M year formation)

This is an asymptotic exponential decay. Real planetary cooling is more complex (involves crustal radioactive heating, atmospheric greenhouse, etc.) but this curve produces the right shape: rapid initial cooling, slow approach to equilibrium.

**Per-hex temperature in Formation** is the global mean plus latitude variation (smaller during Formation since the atmosphere is well-mixed and dense), minus elevation lapse rate (small effect when surface is hot).

### 3.4 Atmospheric Composition over Formation

`AtmosphericComposition` struct:

```rust
pub struct AtmosphericComposition {
    pub co2_ppm: f32,                  // CO2 partial pressure in parts per million
    pub water_vapor_index: f32,        // 0.0 = dry, 1.0 = saturated
    pub oxygen_fraction: f32,          // 0.0 to ~0.21 (Earth)
    pub greenhouse_forcing: f32,       // Composite greenhouse strength in degrees C
}
```

During Formation, evolution:

| Sub-phase | CO2 (ppm) | Water Vapor | Oxygen | Greenhouse |
|-----------|-----------|-------------|--------|------------|
| Molten | 100,000+ | 1.0 (saturated steam) | 0.0 | Extreme (+100°C+) |
| Cooling | 50,000 | 0.8 | 0.0 | High (+40°C) |
| Condensation | 10,000 | 0.5 | 0.0 | Moderate (+15°C) |
| Stabilization | 3,000 | 0.4 | 0.0 | Modest (+5°C) |
| (Post-formation Geological start) | 1,000 | 0.4 | 0.0 | Earth-like baseline |

Oxygen stays at 0 throughout Formation because oxygen is biological. Phase 4 (Biology) will introduce a slow rise as photosynthetic life evolves.

### 3.5 Ocean Formation

Sea level is governed by `WorldData.sea_level_m`. During Formation:

- **Molten** (years 0 – 50M): all water is in atmosphere as vapor. `sea_level_m = -3000m` (the entire ocean basin is dry, only the basin bottoms are below the "if oceans existed" baseline).
- **Cooling** (50M – 200M): condensation begins. `sea_level_m` rises linearly toward `-1500m`.
- **Condensation** (200M – 350M): heavy rains. `sea_level_m` rises toward `-300m`.
- **Stabilization** (350M – 500M): ocean basins approach modern levels. `sea_level_m` reaches `0m` by year 500M.

This produces a worldbuilding-friendly visualization: scrub to year 100M and see "the oceans are still filling" (most of the planet is dry land with deep basins); scrub to year 500M and see "established oceans, ready for life."

After Formation, sea level is driven by tectonic dynamics (Doc 06 §4.6) plus glaciation effects (§12 below).

### 3.6 Events Emitted During Formation

- `EventKind::PlanetaryCoolingMilestone { surface_temp_c }` — Notable, fired every time `global_mean_temperature_c` crosses a 100°C threshold downward
- `EventKind::OceansBeginForming { sea_level_m }` — Major, fired when condensation sub-phase starts
- `EventKind::OceansStabilized { sea_level_m }` — Major, fired when condensation ends and Stabilization begins
- `EventKind::FormationComplete { final_temperature_c, final_co2_ppm }` — Pivotal, fired at end of Formation era

### 3.7 Open Question: Formation Era Override

A user might want to skip Formation and start at "stable planet" for fast worldbuilding ("I just want to write a story; give me Earth-like at year 0"). Parameter:

```rust
pub skip_planetary_formation: bool,  // Default false
```

When true, year 0 starts in Geological era with Stabilization-end values pre-applied. This is a power-user convenience, not the default experience.

---

## 4. Temperature Model

After Formation completes, we compute per-hex temperature each climate tick from physical factors.

### 4.1 Mean Annual Temperature Per Hex

```
T(hex) = T_baseline(lat)
       + T_elevation_adjustment(elev)
       + T_continentality_adjustment(distance_to_ocean)
       + T_ocean_current_adjustment(hex)
       + T_glaciation_adjustment(global_state)
       + T_greenhouse_adjustment(atmospheric_composition)
```

Each term is computed separately and summed. f64 precision for accumulation, stored as f32 in `WorldData`.

### 4.2 Baseline (Insolation by Latitude)

Solar input varies with latitude. Higher latitudes receive less energy per unit area (sunlight at oblique angles). For Earth-like axial tilt (~23°) the annual mean follows roughly:

```
T_baseline(lat) = T_equator * cos(lat)^p
```

With:
- `T_equator = 30.0°C` (Earth-like, before continentality and elevation adjustments)
- `lat` is latitude in radians (absolute value; northern and southern hemispheres are symmetric in annual mean)
- `p = 1.0` for Earth-like; larger `p` makes poles colder relative to equator

Variation:
- `T_equator` shifts with `atmospheric_composition.greenhouse_forcing` and `solar_luminosity_relative_to_sol`
- `p` shifts with `axial_tilt_deg` (high tilt = more even insolation across latitudes)

### 4.3 Elevation Adjustment (Lapse Rate)

Standard atmospheric lapse rate: temperature drops ~6.5°C per 1000m of elevation. We use this directly.

```
T_elevation_adjustment(elev) = -6.5e-3 * max(0, elev - sea_level_m)
```

Underwater hexes don't have elevation lapse — they're at the sea floor regardless of absolute elevation.

### 4.4 Continentality

Continental interiors swing more dramatically with season than coastal hexes. We don't model seasonality directly in `temperature_mean_c` (that's the annual mean), but we DO output `temperature_range_c` (the seasonal swing).

```
T_continentality_adjustment(distance_km) = -3.0 * sigmoid((distance_km - 500.0) / 200.0)
```

Sigmoid centered at 500 km, transitioning over ~400 km, asymptoting to -3°C. Continental interiors are slightly colder on average and have much larger seasonal swings.

```
T_range(hex) = T_range_base(lat) * (1.0 + continentality_factor(distance_km))
```

Where `T_range_base(lat)` is the base seasonal swing at that latitude (small at equator, large at high latitudes) and `continentality_factor` scales it up for continental interiors.

### 4.5 Ocean Current Adjustment

If a hex is coastal and adjacent to a warm or cold ocean current, its temperature is adjusted:

```
T_ocean_current_adjustment(hex) = adjacent_current_anomaly_c * coast_proximity_factor
```

Computed during the ocean currents step (§8). Coastal hexes within 2 hexes of a strong current can be shifted ±5°C from where they'd otherwise be. This is what gives the UK its mild climate despite being at the latitude of Labrador, and what makes the Atacama desert cold despite being near the equator.

### 4.6 Glaciation Adjustment

When a major glaciation is active (§12), high-latitude hexes get an additional cooling.

```
T_glaciation_adjustment = -10.0 * glaciation_intensity * (1.0 - cos(lat))
```

Glaciation intensity ranges 0.0-1.0. The cosine term makes the effect zero at equator and full at poles.

### 4.7 Greenhouse Adjustment

Direct addition from atmospheric composition:

```
T_greenhouse_adjustment = atmospheric_composition.greenhouse_forcing
```

Already in degrees C. Set during Formation; drifts slowly post-Formation as CO2 cycles through outgassing and weathering.

### 4.8 Temperature Range (Seasonal Swing)

Stored as `temperature_range_c` per hex. The annual temperature varies from `mean - range/2` to `mean + range/2`.

```
T_range(hex) = T_range_base(lat) * (1.0 + continentality_factor) + range_bonus_high_obliquity
```

- `T_range_base(0°) ≈ 5°C` (equatorial, low swing)
- `T_range_base(45°) ≈ 25°C` (mid-latitudes, large swing)
- `T_range_base(70°) ≈ 40°C` (subarctic, very large swing)

High axial tilt and high eccentricity amplify `T_range`. A planet with 45° axial tilt has dramatic seasons everywhere.

### 4.9 Determinism Notes

All temperature math is deterministic from `WorldData` state. No RNG used. The output is byte-identical for same inputs across runs.

---

## 5. Distance-to-Ocean Field

Several climate calculations need "how far is this hex from the nearest ocean?" We compute it as a derived per-tick field.

### 5.1 Algorithm

Multi-source BFS from all hexes below sea level:

1. Initialize distance to 0 for all ocean hexes, ∞ for all land hexes
2. Queue all ocean hexes
3. BFS outward through land hexes; each step adds the great-circle distance between adjacent hex centers
4. Result: per-hex distance to nearest ocean in km

```rust
pub fn compute_distance_to_ocean(data: &mut WorldData) {
    let n = data.cell_count() as usize;
    let grid = &data.grid;
    let sea_level = data.sea_level_m;
    
    let mut distances = vec![f32::INFINITY; n];
    let mut queue = std::collections::VecDeque::new();
    
    for i in 0..n {
        if data.elevation_mean[i] < sea_level {
            distances[i] = 0.0;
            queue.push_back(HexId(i as u32));
        }
    }
    
    while let Some(hex) = queue.pop_front() {
        let current_dist = distances[hex.0 as usize];
        let mut neighbors: Vec<HexId> = grid.neighbors(hex).to_vec();
        neighbors.sort_by_key(|h| h.0);  // determinism
        for neighbor in neighbors {
            let n_idx = neighbor.0 as usize;
            if data.elevation_mean[n_idx] < sea_level {
                continue;  // Already ocean
            }
            let step_km = grid.distance_km(hex, neighbor) as f32;
            let new_dist = current_dist + step_km;
            if new_dist < distances[n_idx] {
                distances[n_idx] = new_dist;
                queue.push_back(neighbor);
            }
        }
    }
    
    data.distance_to_ocean_km = distances;
}
```

### 5.2 Complexity

O(n) where n is hex count. Each hex is visited a small bounded number of times. At level 7 (22K hexes), this runs in single-digit milliseconds.

### 5.3 Edge Cases

- World with no oceans: all distances are infinity. Climate model treats this as "uniformly continental."
- World with only ocean: all distances are zero. Climate model treats this as "uniformly maritime."
- Isolated inland seas: count as ocean for distance purposes.

---

## 6. Atmospheric Circulation Cells

Real atmospheres organize into circulation cells (Hadley, Ferrel, polar). The number and size of cells depends on planetary rotation rate. We compute this per tick.

### 6.1 Why Rotation Matters

Faster rotation → stronger Coriolis force → atmospheric flow breaks into more, narrower bands. Slower rotation → weaker Coriolis → flow organizes into fewer, larger cells. The dimensionless number governing this is the Rossby number; we simplify by deriving cell count from rotation period.

Earth (24-hour day): 3 cells per hemisphere (Hadley 0°-30°, Ferrel 30°-60°, polar 60°-90°)
Faster rotators (8-12 hours): 4-5 cells per hemisphere
Slower rotators (48+ hours): 2 cells per hemisphere
Tidally locked or extremely slow: 1 cell per hemisphere

### 6.2 Computing Cell Count

```rust
pub fn cells_per_hemisphere(rotation_period_hours: f64) -> u8 {
    let ratio = rotation_period_hours / 24.0;
    // Slower planets have fewer cells; faster planets have more
    let cells_f = 3.0 / ratio.sqrt();
    cells_f.round().clamp(1.0, 6.0) as u8
}
```

Calibration:
- 6h rotation → ~6 cells per hemisphere (cap)
- 24h rotation → 3 cells (Earth)
- 96h rotation → ~1.5 cells, rounds to 2
- 240h+ rotation → 1 cell

### 6.3 Cell Boundaries

For N cells per hemisphere, boundaries are at:

```
boundary_latitudes = [k * (90° / N) for k in 1..N]
```

For N=3: boundaries at 30° and 60°. For N=4: at 22.5°, 45°, 67.5°.

Cells alternate in circulation direction:
- Cell 1 (equator-most): rising at low-latitude edge, descending at high-latitude edge (Hadley pattern)
- Cell 2: opposite (Ferrel-like)
- Cell 3: same as cell 1 (polar Hadley-like)
- Alternating thereafter

### 6.4 Wind Direction by Cell

Each cell has a prevailing surface wind direction determined by:
- Whether air at the surface is moving equatorward or poleward in this cell
- The Coriolis deflection (rightward in northern hemisphere, leftward in southern)

For Earth's 3-cell setup, surface winds:
- Hadley (0°-30°): trade winds, easterly (NE in N, SE in S)
- Ferrel (30°-60°): westerlies
- Polar (60°-90°): polar easterlies

For different cell counts, the pattern alternates similarly. The implementation table:

```rust
pub fn cell_surface_wind_direction(cell_index: u8, lat_rad: f64) -> (f64, f64) {
    // Returns (east_west, north_south) component magnitudes
    let coriolis_sign = if lat_rad >= 0.0 { 1.0 } else { -1.0 };
    let is_easterly_cell = cell_index % 2 == 0;  // Hadley-like cells have easterlies
    let ew = if is_easterly_cell { -1.0 } else { 1.0 };  // east-positive convention
    let ns = -coriolis_sign;  // simplified; depends on within-cell position
    (ew, ns)
}
```

### 6.5 Cell Intensity

Each cell's wind strength scales with:
- Pole-equator temperature gradient (larger gradient → stronger circulation)
- Inversely with rotation rate (we already accounted for this in cell count, but a small additional damping for very fast rotators)

```rust
pub fn cell_intensity(equator_pole_temp_diff_c: f32, rotation_factor: f32) -> f32 {
    let base = (equator_pole_temp_diff_c / 50.0).clamp(0.1, 2.0);
    base * rotation_factor
}
```

---

## 7. Wind Field

Per-hex wind direction and speed derived from atmospheric circulation.

### 7.1 Per-Hex Wind

Each hex's wind is determined by:
1. Which circulation cell it's in (from latitude)
2. The cell's prevailing wind direction
3. Local modifications from elevation (winds slow over high terrain) and ocean/land contrast (land/sea breezes — but we don't model diurnal cycles, just average)

```rust
pub fn compute_wind(hex: HexId, data: &WorldData, cells: &CirculationCells) -> Wind {
    let (lat, _lon) = data.grid.center_lat_lon(hex);
    let cell = cells.cell_for_latitude(lat);
    let (ew, ns) = cell_surface_wind_direction(cell.index, lat);
    let speed = cell.intensity * wind_base_speed_m_s;
    
    // Elevation damping: surface friction at higher elevation is more
    let elev = data.elevation_mean[hex.0 as usize];
    let elev_factor = if elev > 1000.0 {
        (1.0 - ((elev - 1000.0) / 5000.0).min(0.5))
    } else {
        1.0
    };
    
    let direction_rad = ew.atan2(ns);  // simplified; full vector math in impl
    Wind {
        direction_rad,
        speed_m_s: speed * elev_factor,
    }
}
```

### 7.2 Storage

Per-hex:
- `wind_direction_rad: f32` — radians from north, clockwise (so 0=N, π/2=E, π=S, 3π/2=W)
- `wind_speed_m_s: f32` — m/s

### 7.3 Land/Sea Breezes

Not modeled in v1. These are diurnal phenomena (day/night cycle); our smallest tick is 100K years. The average effect over a year is small compared to the prevailing wind.

---

## 8. Ocean Surface Currents

Wind drives surface ocean currents. Continents constrain them. Result: gyres in each ocean basin, plus narrower coastal currents.

### 8.1 Gyre Formation

Each ocean basin (a contiguous region of below-sea-level hexes bordered by continents or boundaries) develops a gyre. The gyre direction:
- Wind-driven (trade winds drive equatorial currents east-to-west)
- Coriolis deflection turns currents (rightward in N, leftward in S)
- Continents block the flow, forcing it to circulate

Result: clockwise gyres in the Northern Hemisphere, counter-clockwise in the Southern. (For ocean basins large enough.)

### 8.2 Algorithm

For Phase 2, we use a simplified algorithm:

1. Identify ocean basins via connected-component analysis of below-sea-level hexes
2. For each basin, find its centroid
3. For each ocean hex in the basin, compute its position relative to the centroid
4. Assign a tangential current vector (counter-clockwise tangent in S, clockwise in N), scaled by:
   - Distance from centroid (zero at centroid, maximum at basin edge)
   - Local wind strength
   - Coriolis factor (latitude-dependent)
5. Add a westward component near the equator (equatorial currents from trade winds)

This produces gyre-like patterns without requiring full fluid simulation. Real oceanography is much more complex; this is the "looks right" approximation.

### 8.3 Warm vs. Cold Currents

A current carries the temperature of where it came from. Current flowing from equator toward pole = warm. Current flowing from pole toward equator = cold.

Per-hex computation:
- For each ocean hex, look at the upstream direction (opposite of current vector)
- The temperature anomaly carried by the current = temperature at upstream hex - temperature at current hex
- This anomaly is applied (with falloff) to the current hex and its coastal neighbors

This is what gives the Gulf Stream effect on northwestern Europe.

### 8.4 Implementation Note

This is the trickiest single piece of Phase 2. The naive approach (full Navier-Stokes) is way out of scope. The simplified approach (centroid-based gyres with Coriolis) is more like a heuristic than a model.

A reasonable Phase 2 implementation:
- Detect ocean basins (BFS over below-sea-level hexes)
- For each basin, identify "warm injection points" (equatorial edge) and "cold injection points" (polar edge)
- Compute a vector field: each ocean hex's current = weighted average of:
  - Wind-derived tangent
  - Bias toward continental coasts (currents tend to flow along coasts)
  - Coriolis deflection

We accept that this won't be physically rigorous. It produces plausible currents and that's the goal.

### 8.5 Storage

Per ocean hex:
- `ocean_current_vec: [f32; 2]` — (eastward, northward) velocity components in m/s

Land hexes have this set to (0, 0).

---

## 9. Precipitation

Precipitation has multiple sources and is the most multi-factor field in the climate model.

### 9.1 Components

```
precipitation(hex) = base(temp, lat)
                   + orographic(wind, elevation, neighbors)
                   + monsoon_modifier(season, ocean_distance)
                   + ocean_current_modifier(adjacent_currents)
                   + glaciation_modifier(global)
```

### 9.2 Base Precipitation

Determined by temperature (warm air holds more moisture) and latitude (circulation cells produce wet bands near equator and at 60° latitudes, dry bands at 30° and poles).

```
base(temp, lat) = warmth_factor(temp) * latitude_modulation(lat)
```

`latitude_modulation(lat)` is high at equator (intertropical convergence zone), dips at ~30° (subtropical highs → deserts), peaks again at ~60° (storm tracks), low at poles.

For non-3-cell circulations, the latitude pattern follows the cell boundaries computed in §6.

### 9.3 Orographic Precipitation

When wind hits a mountain range, air rises, cools, and dumps moisture. The windward side gets enhanced precipitation; the leeward side gets a rain shadow.

```rust
fn orographic_modifier(hex, wind_direction, elevation) -> f32 {
    // Find the upwind and downwind neighbors
    let upwind = neighbor_in_direction(hex, opposite(wind_direction));
    let downwind = neighbor_in_direction(hex, wind_direction);
    
    let elev_here = data.elevation_mean[hex.0 as usize];
    let elev_upwind = data.elevation_mean[upwind.0 as usize];
    let elev_downwind = data.elevation_mean[downwind.0 as usize];
    
    // Windward side: enhanced precipitation if rising terrain
    if elev_here > elev_upwind {
        let rise = elev_here - elev_upwind;
        return (rise / 1000.0).min(1.5) * orographic_strength;
    }
    
    // Leeward side: rain shadow if descending terrain after high upwind
    if elev_upwind > elev_here + 500.0 {
        return -0.7 * orographic_strength;  // strong rain shadow
    }
    
    0.0
}
```

This is what creates the Pacific Northwest (windward Sierras get rain, eastern Washington is shadow) and the Atacama (Andes block easterly moisture from the Amazon).

### 9.4 Monsoon Modifier

Monsoons happen when:
- A large continent borders a large ocean at low-to-mid latitudes
- Seasonal heating creates a pressure gradient that reverses prevailing winds

We model this as a seasonal precipitation pulse on continental coasts in tropical/subtropical latitudes:

```rust
fn monsoon_modifier(hex, distance_to_ocean, lat, ...) -> f32 {
    if distance_to_ocean > 200.0 || lat.abs() > 30°.to_rad() {
        return 0.0;  // no monsoon
    }
    let intensity = (1.0 - distance_to_ocean / 200.0) * lat_factor;
    intensity * monsoon_bonus_mm
}
```

This affects the *seasonal precipitation* not the *annual mean* — but for simplicity in Phase 2 we apply it as an annual mean bonus and track the seasonal pattern in `temperature_range_c`-like fields (or defer detailed seasonality to later).

### 9.5 Ocean Current Modifier

Coastal hexes adjacent to warm currents get more precipitation (warm water → more evaporation → more rain downstream). Cold currents → less precipitation, often arid coasts.

```rust
fn ocean_current_modifier(hex, adjacent_currents) -> f32 {
    let current_temp_anomaly = adjacent_currents.temperature_anomaly_c();
    let strength = adjacent_currents.strength();
    current_temp_anomaly * strength * COAST_PRECIP_SENSITIVITY
}
```

### 9.6 Glaciation Modifier

During major glaciations, atmospheric moisture content drops (cooler air holds less). Global precipitation decreases by ~20% at maximum glaciation.

### 9.7 Storage

Per hex:
- `precipitation_mm: f32` — annual precipitation, mm (Earth global range: 0 in Atacama to 11000+ in Cherrapunji)

### 9.8 Validation

After running, the precipitation field should satisfy:
- Highest values near equatorial coasts (Amazon, Indonesia analog) — 2000-4000mm
- Lowest values in subtropical interiors and rain shadows — < 100mm
- Mid-latitude coasts: 500-1500mm
- Polar regions: < 300mm

If any of these are wildly off, the model is wrong.

---

## 10. Climate Regimes / Köppen-Like Classification

A Köppen-like classification labels each hex with a climate regime. Biology (Phase 4) uses this to assign biomes.

### 10.1 Regime Categories

```rust
pub enum ClimateRegime {
    Tropical,           // Hot, wet year-round (rainforest, savanna)
    Subtropical,        // Hot, seasonal precipitation
    HotDesert,          // Hot, very dry
    ColdDesert,         // Cool, very dry
    Mediterranean,      // Mild, wet winters, dry summers
    Temperate,          // Moderate, distinct seasons
    ContinentalCool,    // Cold winters, warm summers (continental interior)
    Boreal,             // Cold, evergreen forests
    Tundra,             // Very cold, treeless
    Polar,              // Extreme cold, ice
}
```

10 regimes is a simplification of Köppen's 30+; we keep it tractable.

### 10.2 Classification Rules

A decision tree on `(temperature_mean, temperature_range, precipitation)`:

```rust
fn classify(temp_mean: f32, temp_range: f32, precip: f32) -> ClimateRegime {
    if temp_mean < -10.0 { return Polar; }
    if temp_mean < 0.0 { 
        return if precip < 100.0 { Tundra } else { Boreal };
    }
    if temp_mean < 10.0 {
        return if precip < 200.0 { ColdDesert } else { ContinentalCool };
    }
    if temp_mean < 20.0 {
        if precip < 250.0 { return HotDesert; }
        if precip < 600.0 && temp_range > 20.0 { return Mediterranean; }
        return Temperate;
    }
    if temp_mean < 25.0 {
        if precip < 250.0 { return HotDesert; }
        return Subtropical;
    }
    // temp_mean >= 25.0
    if precip < 250.0 { return HotDesert; }
    Tropical
}
```

Tunable thresholds. Initial values approximate Köppen. Adjust based on validation outputs.

---

## 11. Atmospheric Composition Over Time

After Formation, atmospheric composition continues to evolve slowly.

### 11.1 CO2 Cycle

CO2 enters the atmosphere via:
- Volcanic outgassing (proportional to tectonic activity)
- (Future) civilization emissions

CO2 leaves the atmosphere via:
- Weathering (rocks absorb CO2; rate scales with continental area and precipitation)
- (Future) biological sequestration into limestone (Phase 4)

Per tick:

```rust
let outgassing_kg = base_outgassing * tectonic_activity_factor;
let weathering_kg = base_weathering 
    * continental_area_fraction 
    * mean_continental_precipitation_factor;
let net_change_ppm = (outgassing_kg - weathering_kg) * KG_TO_PPM;
state.atmospheric_composition.co2_ppm += net_change_ppm;
```

This produces slow drift over geological time. Periods of high tectonic activity → CO2 builds up → greenhouse warms → more weathering → CO2 stabilizes. Self-regulating to a degree, like Earth's actual carbon cycle.

### 11.2 Oxygen

Oxygen stays at 0.0 throughout Phase 2 (no biology yet). Phase 4 will introduce a slow rise as photosynthetic life emerges and evolves.

### 11.3 Greenhouse Forcing

Derived from CO2 and water vapor:

```rust
fn greenhouse_forcing(comp: &AtmosphericComposition) -> f32 {
    // Simplified: logarithmic in CO2, linear in water vapor
    let co2_term = (comp.co2_ppm / 280.0).ln() * 4.0;  // 4°C per doubling
    let h2o_term = comp.water_vapor_index * 10.0;
    co2_term + h2o_term
}
```

280 ppm is the pre-industrial Earth baseline. Doubling CO2 to 560 ppm adds ~4°C — the standard "climate sensitivity" estimate. Doubling again to 1120 adds another ~4°C. Logarithmic.

---

## 12. Long-Period Variability: Orbital Cycles and Ice Ages

Earth has gone through major glaciations driven by orbital cycles (Milankovitch). For our purposes:

### 12.1 Milankovitch-Like Cycles

Two cycles of different periods:
- Short cycle: ~100K years (orbital eccentricity)
- Long cycle: ~400K years (orbital plane variation)

We track `cumulative_orbital_phase_rad` and compute:

```rust
fn orbital_temperature_modifier(state: &ClimateState) -> f32 {
    let short = (state.cumulative_orbital_phase_rad).sin();
    let long = (state.cumulative_orbital_phase_rad * 0.25).sin();  // 4x slower
    (short * 0.5 + long * 0.5) * MILANKOVITCH_AMPLITUDE_C
}
```

`MILANKOVITCH_AMPLITUDE_C = 3.0` for Earth-like effects. Larger eccentricity amplifies.

### 12.2 Glaciation State

`GlaciationState` enum:

```rust
pub enum GlaciationState {
    Interglacial { intensity: f32 },  // intensity = how warm; 0-1
    Transition,
    Glacial { intensity: f32 },       // intensity = how cold; 0-1
}
```

State machine:
- Interglacial → Transition when global temp drops below threshold
- Transition → Glacial when temp continues falling
- Glacial → Transition when temp rises
- Transition → Interglacial when temp continues rising

Driven by Milankovitch + CO2 + ice albedo feedback (cold → ice → reflects sunlight → colder, runaway until equilibrium).

### 12.3 Glaciation Effects

When `Glacial`:
- High-latitude hexes get heavy snow/ice cover (`bedrock_type` may shift; precipitation pattern changes)
- Sea level drops (water locked in ice sheets)
- Continental interiors cool dramatically
- Precipitation drops globally ~20%

When `Interglacial`:
- Standard climate
- Sea level normal
- Mild high-latitude climate

### 12.4 Events

- `EventKind::GlaciationBegins { intensity, dominant_continents }` — Pivotal
- `EventKind::GlaciationEnds { duration_years, max_intensity }` — Pivotal
- `EventKind::InterglacialPeak { peak_temp_c }` — Major

---

## 13. Climate-Tectonics Feedback

The hook we stubbed in P1-11/12 lives here. Climate writes `precipitation_mm`; tectonics' erosion reads it and modulates erosion rate.

### 13.1 Implementation

In `crates/genesis_tectonics/src/erosion.rs`, the existing `base_erosion_rate_per_year` is multiplied by `climate_modifier`:

```rust
let climate_modifier = if data.precipitation_mm[hex_idx] > 0.0 {
    (data.precipitation_mm[hex_idx] / 800.0)  // 800mm is global baseline
        .clamp(0.1, 3.0)                       // 10x range cap
} else {
    1.0  // No climate yet (Phase 1 fallback)
};
let erosion_amount = base_erosion_rate 
    * climate_modifier 
    * bedrock_erosion_multiplier(...) 
    * elevation_above_sea 
    * tick_interval_years;
```

Temperature also affects erosion (freeze-thaw cycles, chemical weathering rate):

```rust
let temp_factor = if temp_mean_c > 0.0 && temp_mean_c < 25.0 {
    1.0 + (temp_mean_c / 25.0) * 0.5  // warmer = more chemical weathering
} else if temp_mean_c <= 0.0 {
    1.3  // freeze-thaw is aggressive
} else {
    1.5  // tropical chemical weathering
};
```

Combined: rainforests erode fastest, deserts slowest, polar regions get freeze-thaw boost.

### 13.2 Order of Operations

Per tick:
1. Tectonics runs (uses last tick's climate data for erosion)
2. Climate runs (sees this tick's elevation)

So climate is always "one tick behind" in the causal chain. This is a fine approximation — over 500K years, the difference between current-tick and last-tick climate is negligible for erosion purposes.

---

## 14. Eccentricity and Chaos Mode Hooks

Per the design discussion, two advanced parameters:

### 14.1 Orbital Eccentricity

```rust
pub orbital_eccentricity: f32,  // Default 0.0 (circular)
```

Default 0.0. Range 0.0-0.5. Higher values produce:
- More extreme seasonality (one hemisphere gets very hot at perihelion, very cold at aphelion)
- Asymmetric seasons in length
- Possibly habitability concerns at very high values

Implementation: eccentricity scales `temperature_range_c` by `1 + 5 * eccentricity` and adds a seasonal phase shift between hemispheres.

### 14.2 Chaos Mode

```rust
pub climate_chaos_intensity: f32,  // Default 0.0 (off)
```

When > 0:
- Increases Milankovitch amplitude
- Allows rare extreme events (super-glaciations, hot-house climates) at random times
- Loosens the self-regulating CO2 cycle (allows runaway scenarios)

Implementation deferred to a future doc per Doc 01 §9.5.3. For Phase 2, the parameter exists but doesn't yet drive behavior. Wired up properly when chaos mode gets its own spec.

---

## 15. Event Schema

Climate events follow the granularity pattern established in tectonics (Doc 06 §6.3). New `EventKind` variants:

```rust
pub enum EventKind {
    // ... existing tectonic variants ...
    
    // Formation events
    PlanetaryCoolingMilestone { surface_temp_c: f32 },
    OceansBeginForming { sea_level_m: f32 },
    OceansStabilized { sea_level_m: f32 },
    FormationComplete { final_temperature_c: f32, final_co2_ppm: f32 },
    
    // Climate regime events
    ClimateRegimeShift {
        hex: HexId,
        from: ClimateRegime,
        to: ClimateRegime,
    },
    
    // Glaciation events
    GlaciationBegins { intensity: f32 },
    GlaciationEnds { duration_years: i64, max_intensity: f32 },
    InterglacialPeak { peak_temp_c: f32 },
    
    // Atmospheric events
    AtmosphericCompositionShift {
        from_co2_ppm: f32,
        to_co2_ppm: f32,
    },
    
    // Ocean events  
    OceanCurrentRedirection {
        affected_basin_centroid: HexId,
    },
    
    // Drought/wet events
    MajorDroughtPeriod {
        region_centroid: HexId,
        intensity: f32,
        duration_years: i64,
    },
}
```

### 15.1 Significance Assignment

| Variant | Significance |
|---------|--------------|
| `PlanetaryCoolingMilestone` | Notable |
| `OceansBeginForming` | Major |
| `OceansStabilized` | Major |
| `FormationComplete` | Pivotal |
| `ClimateRegimeShift` (regional) | Minor |
| `ClimateRegimeShift` (continental scale, biome-shift) | Notable |
| `GlaciationBegins` | Pivotal |
| `GlaciationEnds` | Pivotal |
| `InterglacialPeak` | Major |
| `AtmosphericCompositionShift` (>20% CO2 change) | Notable |
| `OceanCurrentRedirection` | Major |
| `MajorDroughtPeriod` | Notable |

### 15.2 Climate Event Granularity

A new parameter:

```rust
pub struct ClimateParameters {
    // ... other fields ...
    pub event_granularity: Significance,  // Default Notable
}
```

Same mechanism as tectonics. Events below threshold are computed but not logged.

---

## 16. Performance Targets

### 16.1 Per-Tick Cost (Level 7, M-series Mac)

| Step | Budget |
|------|--------|
| Insolation | 1 ms |
| Circulation cells | < 1 ms |
| Distance to ocean | 5 ms |
| Temperature | 5 ms |
| Wind field | 3 ms |
| Ocean currents | 15 ms |
| Precipitation | 8 ms |
| Regime classification | 3 ms |
| Atmospheric composition | < 1 ms |
| Long-period variability | < 1 ms |
| Events | < 1 ms |
| **Total per Geological tick** | **~50 ms** |

This is roughly double tectonics' per-tick cost — acceptable.

### 16.2 Memory

New bulk arrays:

| Field | Per-hex bytes | Total at level 7 |
|-------|---------------|-------------------|
| `temperature_mean_c` | 4 | 88 KB |
| `temperature_range_c` | 4 | 88 KB |
| `precipitation_mm` | 4 | 88 KB |
| `wind_direction_rad` | 4 | 88 KB |
| `wind_speed_m_s` | 4 | 88 KB |
| `ocean_current_vec` | 8 | 176 KB |
| `distance_to_ocean_km` | 4 | 88 KB |
| `climate_regime` | 1 | 22 KB |
| **Total** | **33** | **~720 KB** |

Modest addition.

### 16.3 Whole-History Budget

Full 4.5B year simulation should add no more than 2-3 minutes to total generation time compared to Phase 1.

---

## 17. Validation Criteria

After running full climate simulation on a default Earth-analog world, the result should satisfy:

1. **Global temperature distribution**: Mean across all hexes between -20°C and +35°C; most hexes between 0°C and 30°C
2. **Latitude gradient**: Equatorial hexes (lat 0±10°) average > 20°C; polar hexes (lat ±70-90°) average < 0°C
3. **Precipitation distribution**: At least 10% of land hexes receive < 250mm (deserts), at least 10% receive > 1500mm (rainforests/temperate wet)
4. **Rain shadows visible**: At least one continental region shows a clear windward-wet / leeward-dry pattern (a hex within 5 hexes of a mountain range shows precipitation < 30% of its upwind neighbor)
5. **Climate regimes present**: At least 7 of the 10 regimes exist somewhere in the world
6. **No saturation**: No hex at exactly 0mm precipitation (impossible) or > 10000mm (extreme; should be rare)
7. **Atmospheric composition**: CO2 between 200 and 5000 ppm at any post-Formation time
8. **Sea level evolution during Formation**: Sea level rises monotonically from initial -3000m to ~0m
9. **Glaciations occur**: At least one full glaciation cycle in a 1 billion year simulation (with Earth-like parameters)
10. **Coastal moderation**: Coastal hexes (distance_to_ocean < 100km) have temperature_range_c values < their continental neighbors at same latitude

Implementation tests verify each. Tolerances per Doc 06 §11 pattern — loose enough to allow seed variation, tight enough to catch regressions.

---

## 18. Edge Cases and Open Questions

### 18.1 No-Ocean World

If sea level drops far enough or initial elevation is wrong, a world could have no ocean hexes. Climate falls back to:
- All hexes treated as continental
- No ocean currents
- Distance-to-ocean is infinity (treated as max continental factor)
- Precipitation comes only from base atmospheric circulation + orographic

Plausible result: extremely arid world with seasonal extremes. Cool worldbuilding case for a barren planet.

### 18.2 All-Ocean World

If sea level rises above all elevations:
- All hexes are ocean
- No continents → no continental climate effects
- Currents form gyres in the global ocean
- Precipitation roughly latitude-banded

Plausible result: water world.

### 18.3 Locked Rotation

`rotation_period_hours >> 1000` (basically tidally locked). Implications:
- 1 circulation cell per hemisphere
- Day side super-hot, night side super-cold (but we don't model day/night, just averages)
- Average temperature distribution gets weird

We use the same model regardless; output will look unusual but won't crash.

### 18.4 Extreme Eccentricity

`orbital_eccentricity > 0.3`. Implications:
- Massive seasonal swings
- Potentially uninhabitable conditions

Climate model handles this via `temperature_range_c` amplification. Worldbuilders who set this high get exactly the chaos they asked for.

### 18.5 Very High Tilt

`axial_tilt_deg > 60°`. The poles get more sunlight than the equator on annual average. Climate model handles this via `p` parameter in §4.2.

### 18.6 Open Question: Should ocean currents persist between ticks?

Currents in real oceans don't change instantly — they have inertia. Our model recomputes them fresh each tick from current wind + geometry. This is fine for our timescale (500K years per tick) but feels physically loose.

For Phase 2, we recompute fresh. Future refinement could blend with previous-tick currents.

### 18.7 Open Question: Glaciation thresholds

The temperature thresholds for glaciation onset are loose guesses. Empirically tune during Phase 2 implementation.

### 18.8 Open Question: Should `temperature_range_c` track summer vs winter separately, or just amplitude?

Currently just amplitude (annual mean ± half-range). For biome assignment, summer high vs winter low matters separately (deciduous forests need warm summers AND cold winters; tropical rainforests need warm year-round). For Phase 2 we use amplitude; Phase 4 (Biology) decides if it needs more.

---

## 19. File Organization

Implementation lives in `crates/genesis_climate/`:

```
genesis_climate/
├── Cargo.toml
└── src/
    ├── lib.rs                  # public API + ClimateLayer
    ├── formation.rs            # planetary cooling and ocean formation (§3)
    ├── temperature.rs          # per-hex temperature (§4)
    ├── ocean_distance.rs       # distance-to-ocean BFS (§5)
    ├── circulation.rs          # atmospheric cells (§6)
    ├── wind.rs                 # per-hex wind field (§7)
    ├── ocean_currents.rs       # gyres and currents (§8)
    ├── precipitation.rs        # per-hex precipitation (§9)
    ├── regimes.rs              # Köppen-like classification (§10)
    ├── atmosphere.rs           # atmospheric composition (§11)
    ├── variability.rs          # ice ages, Milankovitch (§12)
    ├── feedback.rs             # climate-tectonics integration (§13)
    ├── events.rs               # event emission and granularity
    ├── layer.rs                # SimulationLayer impl
    └── validation.rs           # §17 metrics
```

Depends on:
- `genesis_core` (data structures, RNG, events, time)
- `genesis_tectonics` (for plate state queries — read-only)
- `glam` (vector math)

No Bevy dependency. Climate layer is engine-agnostic.

---

## 20. Out of Scope for Phase 2

Explicitly NOT included; deferred:

- **Detailed atmospheric chemistry** — single greenhouse forcing parameter, not CO2/CH4/H2O separately
- **Stratospheric processes** — surface-only model
- **Clouds and aerosols** — handled implicitly via greenhouse forcing
- **Jet streams in detail** — circulation cells are coarse-grained
- **Anthropogenic climate change** — Phase 5 (Civilization) will add a hook
- **Detailed oceanography** — surface currents only; no thermohaline circulation; no deep-water masses
- **Tidal forces** — not modeled
- **Solar variability over time** — single luminosity parameter
- **Coupled biological feedbacks beyond CO2** — Phase 4 will refine
- **Resolution finer than the hex grid** — sub-hex climate variability isn't tracked
- **Storms / weather events** — too short-timescale for our ticks

---

## 21. Implementation Plan (Phase 2 Sub-Steps)

Preliminary breakdown, similar to Doc 06 §15:

1. **`genesis_climate` crate scaffolding + new parameters** — set up the crate, define `ClimateParameters`, add new bulk arrays to `WorldData`
2. **Formation: Hadean and cooling phase** — global temperature, atmospheric composition evolution, basic state machine
3. **Formation: ocean condensation and sea level rise** — multi-tick sea level evolution during formation sub-phases
4. **Distance-to-ocean BFS** — implement and test
5. **Circulation cells from rotation rate** — compute cell boundaries and intensities
6. **Wind field per hex** — derive from cells
7. **Base temperature model** — insolation, lapse rate, continentality
8. **Ocean basin identification and gyre detection** — connected components of below-sea-level hexes
9. **Ocean currents and temperature transport** — gyres + warm/cold current effects
10. **Base precipitation** — latitude-banded + temperature-driven
11. **Orographic precipitation and rain shadows** — wind interacting with terrain
12. **Climate regime classification** — Köppen-like decision tree
13. **Atmospheric composition slow drift** — CO2 cycle over geological time
14. **Long-period variability** — Milankovitch + glaciation state machine
15. **Climate-tectonics feedback wire-up** — connect erosion to precipitation
16. **Event emission** — all climate events with significance
17. **Save/load support** — new bulk arrays serialized
18. **Rendering: temperature heatmap** — color-by-temperature overlay
19. **Rendering: precipitation overlay**
20. **Rendering: climate regime overlay**
21. **Validation tests** — §17 metrics as automated checks
22. **Performance tuning** — profile, optimize hot loops

22 sub-steps. Some may combine; some may split. Phase 2 is bigger than Phase 1.

---

## 22. Implementation Notes for the AI Agent

Per Doc 04 §16 and Doc 06 §16 patterns:

1. **Read this entire doc** before starting any sub-step. Climate is highly interconnected; isolated implementation produces bugs.
2. **Use `BTreeMap` / `BTreeSet` exclusively.** Never `std::HashMap`.
3. **f64 for accumulation, f32 for storage.** Standard Genesis Engine convention.
4. **Each sub-prompt references this doc by section.** If a prompt seems to contradict this doc, surface it before resolving.
5. **Surface every assumption beyond what's specified.** Calibration constants, threshold values, tuning numbers — flag them in your summary.
6. **The temperature/precipitation interactions are tricky.** Test with extreme cases (no ocean, all ocean, very fast rotation, very high tilt) to surface bugs early.

---

## 23. Open Questions for Doc Review

1. **Formation tick interval (5M years)** — fine-grained enough to see ocean condensation, but at 100 ticks across 500M years it's a lot. Worth tuning during Phase 2 implementation.

2. **Glaciation thresholds** — pure guess work in §12. Calibrate empirically.

3. **Ocean current gyre algorithm** — the "simplified centroid approach" in §8.2 is heuristic. May need iteration to look right. Reserve a follow-up prompt if needed.

4. **Climate regime thresholds** — Köppen-like decision tree in §10. Standard Köppen values used as starting point; may need adjustment.

5. **Should precipitation distinguish seasonal vs annual?** Currently we store annual total. For biology, seasonal distribution matters (Mediterranean climates have dry summers, wet winters). v1 uses annual; revisit if Phase 4 needs more.

6. **Atmospheric composition: per-planet vs per-region?** Currently global. Real atmospheres have regional variation but we treat composition as well-mixed. Fine for Phase 2; revisit if needed.

7. **CO2 cycle calibration** — outgassing and weathering rates need tuning to produce stable CO2 over geological time.

8. **Tidally-locked planets** — our model produces *something* but it's not physically meaningful for these worlds. Worth flagging in user-facing docs that highly unusual planets may have unusual climate output.

---

*End of Doc 07 v0.1.*

*Next step: write Phase 2 implementation prompt P2-1 (`genesis_climate` crate scaffolding plus parameter additions).*
