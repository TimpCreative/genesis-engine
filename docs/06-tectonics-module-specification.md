# 06 — Tectonics Module Specification

**Document Type:** Tier 2 — System Specification
**Status:** Draft v0.14
**Last Updated:** July 2026
**Owner:** Brax Johnson
**Implementing Phase:** 1 (Geology Prototype)

**Changelog:**
- v0.14 (July 2026): **Continental collision realism, live-rift ocean protection, deep-time crust budget (P1-25).** (1) *Crustal shortening (§4.2).* Converging continent–continent contests no longer let footprints interpenetrate: the overlap is consumed by shortening — the losing claim's feature is deleted and the winner uplifts `SHORTENING_UPLIFT_M = 250 m` — so colliding continents crumple (Himalaya) instead of ghosting through each other unchanged (the 2.29B→2.52B pass-through caught on owner screenshots). A pair accumulating ≥3 shortening contacts registers in `colliding_pairs` for the jam below. (2) *Collision jam (§4.6).* Plates in a registered collision relax their angular velocities toward the shared mean with τ = 10 My — kinematic suturing: the pair keeps drifting as one welded block instead of one sliding through the other. (3) *Gravitational collapse (§8.5).* A new post-erosion pass spreads relief that exceeds rock strength under its own weight (three-regime relaxation toward the ~5 km cap, `COLLAPSE_RELAX_YEARS = 10 My` — do not shorten; the aggressive version cascaded the world to a puddle, see the constant's comment). Mountains come down without water, as they must until Doc 8 owns erosion. Measured adjacent-hex steps across three gate seeds: 8.3–11.1 km non-trench, ≤16.3 km trench-adjacent. (4) *Crust budget retune (§4.7, §5.10).* Subduction erosion is resolution-normalized (per-hex consumption probability scales with hex width so the margin retreat rate is resolution-independent); `initial_continental_fraction` 0.22→0.30 and `CONTINENTAL_FREEBOARD_M` 550→800 so continents stand higher and the ratchet starts from an honest base. (5) *Live-rift protection (§5.8).* A trapped basin adjacent to an *actively opening* divergent edge is a growing ocean (Red Sea, Gulf of California), not an obduction candidate: the accretion pass no longer converts it (a stalled rift loses protection, mirroring the convergent rule), but its floor is still capped at `MARGINAL_SEA_EQUILIBRIUM_M` — hot young ocean floor never sits at trench depth, and the cap keeps margin-profile relief inside gate #14. Without the guard the pass strangled every infant ocean at birth (newborn below-sea rift floor is a small enclosed component and converted in one tick): measured with 4B flux instrumentation (subdiv 7), accretion events fell 66k → 47–53k and the mid-run crust curve flattened (0.30 → 0.39 across 1B–2.5B, was 0.31 → 0.55). A stronger variant — continental breakup by converting rift margins to oceanic crust — was implemented and REVERTED on gate evidence: conversion width was one hex ring per side, an area term that scales with resolution, and at subdiv 5 it amputated continents to ≤2% crust by 1B (waterworld, zero mountains). Earth creates ocean floor overwhelmingly by ridge accretion — which the sim already has (§4.2 gap minting) — while margins stay continental. (6) *Gates (§11).* #10 asserts continental crust AREA 0.10–0.45 at 1B with land printed informationally (fixed-seed land bands police Wilson-phase noise, not physics); #11's fossil-floor check now excludes components touching ANY live margin, convergent or divergent (Afar/Baikal rift floors are current geology, not fossil relief); #14's trench bucket discriminates at `MARGINAL_SEA_EQUILIBRIUM_M` (−4,500 m) instead of −6,000 m, because live-margin-protected basins sit at exactly the cap — the Andes-class profile (+9,000 clamped peak beside a −4,500 back-arc floor = 13.5 km) is margin geology (Earth's trench-to-summit max ≈ 15 km), while the 12 km limit keeps policing land-vs-land steps. All green ×3 seeds (42/43/44). *Known issues:* (a) the resolution-normalization helpers use the 10·4^level+2 cell-count formula while the grid is ISEA3H (10·3^level+2) — mislabeled but calibrated: the 4^ quirk accidentally yields the physical retreat rate the passing gates were tuned against, and "correcting" it would strengthen the sink ~2.7× and force a full recalibration; flagged, deliberately not churned. (b) *Deep-time ratchet — resolved in this version.* With conversion-split instrumentation the 1B→4B budget read: +28.4k trapped-basin conversions (accelerating: ~210/100My at 100M → ~1,000–1,800/100My after 3B) against −3.8k subduction erosion and an inferred ~−15.6k from shortening + rift gap-minting, net +8.9k hexes: crust 0.30 → 0.59–0.71 at 4B across two realizations of the same physics (Earth: ~0.40, quasi-steady since ~2.5 Ga). The unphysical term was instant one-tick conversion of enclosed basins — on Earth the Mediterranean/Caspian/Black Sea persist as oceanic crust for tens to hundreds of My while suturing proceeds. The fix is §5.8 item 5: gradual suture conversion at `SUTURE_HALF_LIFE_MY`, with the §5.9 enclosure cap applied to unconverted floors so no sealed basin holds abyssal depth while it waits. Calibration measured at 4B (subdiv 7, 3 realizations per rate): 75 My half-life → 13–22% crust (ratchet dead but ocean-heavy); **55 My → 18.5–27.3% crust with land 18.7–25.3%** — adopted as the better match to Earth's present ~29% land. §11 #10/#11 green on all six 4B runs (deepest detached basin −4,500 to −5,097 m) and on the 1B gate ×3 seeds (crust 0.130–0.148, max non-trench step 8.6–11.1 km). Same seed produces different output post-P1-25 (expected).
- v0.13 (July 2026): **Slab-pull plate motion, crust mass balance, tick perf (P1-24).** (1) *Speed is emergent, not sampled (§2.4).* The sampled log-normal base rate + compounding collision damping + 5% stall floor + 2%/tick rift recovery machinery is deleted. Boundary scans now tally each plate's live slab edges (convergent, this plate's oceanic crust downgoing) and ridge edges; the motion step relaxes every rate toward the force set-point `min(15, 1.5 + 10·slab_frac + 2·ridge_frac) × velocity_scale × rotation_factor` cm/yr with a 25 My exponential time constant. Slab-girdled oceanic plates run Pacific-fast, slab-less continents drift Africa-slow, and suturing slows a plate over ~25 My as its trench chokes — the India transient from physics, not a script. This kills both failure modes of the old model: no compounding damping toward zero (the floor is the 1.5 drift base) and no release back to a log-normal high tail (the ceiling is 15 cm/yr; the old tail could sustain ~30–55 cm/yr ≈ a quarter of the planet per 10 My). "Stalled" for Wilson-cadence reorg pressure and split/merge targeting is now an absolute < 2 cm/yr threshold; `base_motion_rates` is gone. Measured at subdiv 7: speeds 2.0–6.4 cm/yr, median 3.3–4.2, at 200M/1B/4B; new §11 gate #13 (0.5–20 × scale at 1B). (2) *Subduction erosion (§5.10).* Accretion only created continental crust, so the fraction ratcheted to ~53% of the sphere by 4B years. Live continent–ocean margins now consume overriding forearc rims at `p = 0.0025`/My per rim hex (compounded per tick length; `tectonics.subduction_erosion` stream), converting them to oceanic crust at trench depth — von Huene & Scholl's erosive margins. The first calibration at 0.01/My over-corrected (continents drained to ~5% by 4B); at 0.0025 the fluxes balance: land 23.5% → 23.9% → 38.1% at 200M/1B/4B (was 27.3 → 33.8 → 48.9), §11 #10–11 green at all three snapshots (4B detached 2.67% → 0.95%). (3) *Tick perf.* One water-realm component labeling per tick, shared by trench enclosure and accretion (was two identical O(n) BFS labelings — exact, since `elevation_mean` only changes at world rebuilds); one `BoundaryInfo` clone removed; slow-step log threshold is env-tunable (`GENESIS_SLOW_TICK_STEP_MS`). Net 4B-year run: 134s → 127s at subdiv 7 with the new physics included; the profile names `repartition_hexes` (~9.5 ms/tick) as the remaining hotspot for a future pass. (4) *Menu: Continental crust % knob.* The setup screen now exposes `initial_continental_fraction` (2-point steps, 5–60%, default 22%) — the physically meaningful lever: ocean-vs-continental composition emerges from crust area at formation, so major/minor (size classes) stay as they are. Same seed produces different output post-P1-24 (expected).
- v0.12 (July 2026): **Deep-time Wilson-cycle pass (P1-23).** Six mechanisms that make the supercontinent cycle actually cycle, plus the §11 gates to hold them. (1) *Ocean-opening splits (§4.5).* Splits now preferentially target stalled plates (rate < 50% of unstressed base — rifts nucleate in welded supercontinents) and choose the child's motion axis from 8 sampled candidates to maximize divergence along the new boundary line (parent seed → midpoint → child seed): splits open young oceans with passive margins instead of shearing or instantly re-colliding. Split/merge weights also regulate the plate census into the §11 #2 band (≥15 plates: splits 40%→20%, mergers 20%→40%; ≤5: splits 60%, mergers off). (2) *Wilson cadence (§4.5).* Reorganization probability is `0.004 * geology_activity_scale * (1 + 2 * stalled_fraction)` — a fully welded world reorganizes up to 3× sooner than a drifting one (the doc previously said a flat 0.001; the code's 0.004 base is now documented and stall-scaled). (3) *Subduction choking (§5.9).* Trench arms skip their delta when the downgoing hex carries continental crust — buoyant debris jams the zone instead of being dragged below the isostatic rebound floor forever. (4) *Trench enclosure (§5.9).* A trench segment sealed off from the open ocean (component < 1% of cells) equilibrates at `MARGINAL_SEA_EQUILIBRIUM_M = −4500 m` (Japan Sea / Mediterranean), infilling at 10× the subduction rate; the accretion pass applies the same cap to live-protected back-arc basins, whose floors previously sat at −8500 m one hex behind the trench line. Deepest detached basin at 200M/1B/4B: exactly −4500 m. (5) *Velocity-gated fossil-trench healing (§5.8).* "Live" subduction now requires closing velocity above the classification threshold — collision-stalled boundaries creeping at the 5% stall floor stop protecting their pits — and deep continental floors in trapped basins lift to the obduction depth (rebound refuses crust below its floor). (6) *Forearc emergence (§5.3).* CO arc uplift spreads 4 rings with falloff `[0.4, 1.0, 0.6, 0.25]` peaking at ring 2, and the coastal hex gets 30% (`FOREARC_UPLIFT_FRACTION`): subduction coasts read forearc → foothills → arc with land on both sides (Chile exists), not mountains dropping into the sea. *New §11 gates #10–12* (land 20–45% at 1B; sub-1% detached water < 2% of cells and ≥ −6000 m; passive-margin coastline ≥ 25%, tracked-informational for now). Calibration at subdiv 7: **200M — all four pass** (plates 15, land 27.3%, detached 0.13%, deepest −4500 m, passive 30.9%); **1B** — plates 14, land 33.8%, detached 1.02%, deepest −4500 m, passive 10.1% (1B regression gate `wilson_cycle_criteria_hold_at_one_billion_years` passes); **4B still off-target** (plates 17, land 48.9%, small detached 2.7%, passive 7.1%): continental crust area ratchets 27%→34%→53% of the sphere because trapped basins convert to permanent land and nothing recycles it — subduction erosion, delamination, and water-volume conservation are the follow-up (Doc 8-adjacent). Same seed produces different output post-P1-23 (expected).
- v0.11 (July 2026): **Water visuals removed pending Doc 8; suture accretion; orogeny recalibration (P1-22).** (1) *No visible water.* Water is removed as a visible surface feature until Doc 8 (hydrology) lands — the renderer uses a dry hypsometric ramp (ocean floor renders as deep charcoal relief, never blue), the Rivers render mode and the provisional `genesis_hydrology` crate are deleted (prior art retained in Doc 08), and climate regime classification now assigns a regime to every hex instead of leaving below-sea hexes `Unset`. `sea_level_m` and Doc 07's internal ocean model (distance-to-ocean, currents, moisture) are unchanged — climate depends on them. (2) *Suture accretion (§5.8).* Below-sea basins trapped between colliding continents no longer persist as permanent inland seas (previously ~150 detached oceanic-crust components at 1B years; trapped crust sat on the −4000 m floor forever because isostasy only lifts continental crust). A new per-tick accretion pass consumes trapped oceanic crust into the continent (obduction) and lifts it to 200 m below sea level for epeirogenic rebound to carry to freeboard. Regression guard: `history_leaves_no_trapped_oceanic_basins`. (3) *Orogeny recalibration.* CC collision uplift spreads 4 rings inland with falloff `[1.0, 0.6, 0.3, 0.15]` (was 2 rings); CO coastal arc uplift factor 0.25→0.10 so subduction coasts no longer rival collision ranges; bedrock erosion multipliers lowered (Metamorphic 0.25→0.15, Igneous 0.10→0.08) and `base_erosion_rate_per_year` default 1e-7→5e-8 so old collision belts persist as visible highlands between Wilson cycles. Same seed produces different output post-P1-22 (expected).
- v0.10 (July 2026): **Level-8 continent-quality fixes (P1-21).** Three fixes to problems that only became visible at subdivision 8. (1) *Projection-hole speckle.* The world rebuild forward-rotates birth-indexed features to world hexes, a many-to-one resample that leaves holes; the old neighbor-mean hole patch dipped those holes below sea level next to any coast/ridge, riddling continents with phantom "lakes" in every render mode. The rebuild now resolves every owned hex (claimed and hole) through the `ProjectionCache`'s birth mapping — reading the plate's actual material, or the land baseline for an empty slot — and the neighbor-mean patch is deleted. (2) *Rebound floor made sea-relative.* `EPEIROGENIC_REBOUND_FLOOR_M` was an absolute −2000 m; once sea level fell to −1500 m, continental shelf crust only 500–1000 m under water sat below the floor and was frozen as permanently drowned. The floor is now `sea_level + offset`, so barely-submerged continental crust rebounds toward its freeboard; interior sub-sea hexes at level 8 fell ~2960→~500. (3) With the speckle gone, honest land fraction rose (the old ~29% was partly the speckle bug undercounting land), so `initial_continental_fraction` was recalibrated 0.29→0.22 for a stable Earthlike 23–27% land across 200M/1B/4.5B. Also: **stalled-plate motion floor.** An interior plate boxed in by converging neighbors was damped toward literal zero with recovery permanently gated off; `recover_motion_rates` now floors every plate at `MIN_STALL_FRACTION` (5%) of its base rate even while colliding, so a sutured interior continent creeps like a craton instead of freezing (min 4.5B displacement rose from a few hundred to ~2500 km). Same seed produces different output post-P1-21 (expected).
- v0.9 (July 2026): **Level-8 generation performance (P1-20), no behavior change.** Full history at subdivision 8 (65,612 hexes, 200M years) dropped from ~137s to ~26s (5.3×), all worlds bit-for-bit identical. Two hot paths were re-engineered: (1) **Formation was O(n²)** — plate growth rescanned each plate's entire owned footprint to find its frontier on every single hex-addition, so the final additions each scanned tens of thousands of hexes. Each plate now maintains its unowned-adjacent frontier as an incrementally-updated `BTreeSet`; a set's ascending `HexId` iteration reproduces the old sort+dedup exactly and the frontier length feeds the same rng draw, so determinism is preserved to the bit. Formation alone: ~13.3s → ~0.08s (168×). (2) **Per-tick surface lookups** re-derived a quaternion rotation and ran a nearest-hex search for every hex in erosion, boundary classification, and the three world-rebuilds per tick. `repartition_hexes` already inverse-maps every claimed hex, so it now also emits a `ProjectionCache` (world→owning-plate birth-hex table, guarded by an ownership snapshot); `surface_elevation_at`, `continental_crust_at`, `modify_surface_at_world_hex`, the boundary-delta keys, and a cache-driven `rebuild_world_from_plate_surfaces_cached` take table lookups, falling back to direct computation when the cache does not cover the current ownership (tests, cold paths). The per-tick erosion noise map also moved from `BTreeMap` to a flat `Vec`. Deep-time gates and all 143 tectonics tests unchanged.
- v0.8 (July 2026): **Wilson-cycle rift recovery and formation pit smoothing (P1-19).** Collision damping previously halved a colliding plate's motion rate permanently, so sutured continents parked forever. Plates now recover toward their unstressed base rate (2%/tick of the remaining gap, ~25M years to resume full speed) whenever they are NOT in an active converging continental collision; bases are tracked per plate and reset when a reorganization assigns new motion. Reorganization motion-changes preferentially target stalled plates (rate < 50% of base), feeding the supercontinent cycle. Formation noise gains a single-hex pit fill (lone dips >150 m below every neighbor rise to 50 m below the lowest) and `min_geologic_lake_depth_m` default rises 200→400 m, eliminating near-black one-hex "holes" in continent interiors while preserving multi-hex basins. Notable-event validation bound raised 15k→17k at 100M years (persistently active plates emit ~0.1% more events).
- v0.7 (July 2026): **Crust identity and isostasy (P1-18).** `SurfaceFeature` gains a permanent `continental_crust` flag set at creation (bedrock labels are overwritten by sediment/volcanism and cannot identify lithosphere). Buoyancy in ownership contests, convergent subtype, and trench-side selection all key on the flag. New isostasy pass: continental crust erodes down to and epeirogenically rebounds up toward a 550 m freeboard (drowned margins re-emerge over ~50–100M years; epicontinental seas are transient, continents permanent), while oceanic crust erodes to sea level and thermally subsides toward the abyssal baseline (abandoned arc/hotspot islands become guyots). Features are minted ONLY at formation, ridge accretion, and boundary flush (never buoyant) — `modify_surface_at_world_hex` is modify-only, closing the margin-creep feedback loop that inflated continents. `initial_continental_fraction` now targets crust AREA (fraction of the sphere) and continental plates are chosen as a spatially connected cluster — a supercontinent start with one world ocean. Island-arc uplift recalibrated against the Igneous erosion multiplier. Result at subdivision 7: land 29% at 1B and 4.5B years (Earth: 29%), one connected deep ocean basin, bounded sea level, coherent continents that rift and collide across the supercontinent cycle. Same seed produces different output post-P1-18 (expected).
- v0.6 (July 2026): **Material plate footprints (P1-17).** Hex ownership now derives from forward-rotating each plate's footprint (its birth-indexed features) instead of Voronoi re-partition around moving seed points — continents drift as coherent material bodies and keep their shapes across deep time. Overlapping claims resolve by buoyancy (continental > older plate > lower id); oceanic crust that loses a genuinely *converging* contest is subducted (destroyed); jitter overlaps at passive contacts are inert. Gaps left by diverging plates accrete young ridge crust (−2700 m) on the adopting plate; quantization holes adopt ownership without minting features. New supporting physics: age-based thermal subsidence of deep crust toward the abyssal baseline, continental collision resistance (converging continental pairs damp each other's motion — suturing), per-hex crust (bedrock/depth) determines convergent subtype and trench side instead of plate type, uplift/subsidence headroom tapers replace clamp saturation, and birth frames re-anchor on motion change/split/merge so axis changes no longer teleport terrain. Formation noise is now multi-octave and spatially correlated (regional highlands/lowlands/plateaus and shelf seas instead of per-hex speckle). Calibration: narrower inland orogeny spread (2 rings), gentler arc/coastal uplift, `surface_remap`-era one-tick repartition expectations relaxed (sub-hex drift per tick is correct). Same seed produces different output post-P1-17 (expected).
- v0.5 (July 2026): **Birth-frame surface indexing (P1-16).** `PlateSurface` arrays are now keyed by *birth* world `HexId` — the world-frame hex where a feature was created — instead of plate-local `HexId`. Display positions are computed by forward-rotating the fixed birth position (`birth_hex_to_current_world`), so quantization error no longer compounds across ticks; writes convert once via `current_world_to_birth_hex`. Removed the `surface_remap` post-motion reindexing step (module deleted). `WorldData` rebuild is now two-phase: plate-type baseline fill, then birth-feature projection with deterministic collision priority (higher elevation, then newer `age_year`, then lower birth index) and a plate-ownership guard (features only render on hexes their plate owns). Same seed may produce different output post-P1-16 (expected).
- v0.4 (May 2026): **Destination-driven plate surfaces (P1-15).** Authoritative terrain (`elevation_mean`, `elevation_relief`, `bedrock_type`, `fertility`) lives on per-plate `PlateSurface` arrays keyed by plate-local `HexId`; `WorldData` is rebuilt twice per Geological tick (after motion/partition and after all surface mutations). Boundary elevation, erosion, hot spots, volcanism, and reorganization subsidence write to surfaces, not directly to `WorldData`. Removed push-model feature advection and `PlateOrigin` tagging. Same seed may produce different output post-P1-15 (expected).
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

- Subduction angles, slab geometry, viscous mantle flow, or convection cells (slab pull IS the motion driver — §2.4 — but as boundary tallies, not numerically accurate force balances)
- Isostatic adjustment beyond a simple proxy
- Mineralogical composition or igneous petrology
- Earthquake mechanics
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

Real Earth plates move at 0.5-15 cm/year. The Pacific Plate moves ~10 cm/year; the African Plate moves ~1-2 cm/year; the Antarctic Plate is nearly stationary. We want this variation, not uniform motion — and on Earth the variation is not random: it is set by **boundary forces**. Slab pull dominates (~90% of the driving budget): plates rimmed by subducting slabs run 6–15 cm/yr (Pacific, Nazca, pre-collision India ~18), plates without slabs drift at 1–3 cm/yr (Africa, Eurasia, Antarctica), and ridge push adds a little. When India's slab choked at the Asian suture, the plate slowed 18→5 cm/yr over ~20 My.

So plate speed is **emergent from boundary geometry**, not a stored constant. Each tick the boundary scan tallies per plate: live convergent edges where the plate's oceanic crust is the downgoing slab (`slab_edges`), live divergent edges (`ridge_edges`), and all edges (`total_edges`) — "live" means closing/opening faster than the 0.005 m/yr classification threshold, so a stalled suture is not a pulling slab. The motion step then relaxes every plate's rate toward its force set-point:

```
rotation_factor = sqrt(24.0 / rotation_period_hours)
target_cm_per_year = min(15, 1.5 + 10.0 * slab_edges/total_edges + 2.0 * ridge_edges/total_edges)
                     * plate_velocity_scale * rotation_factor
rate += (target - rate) * (1 - exp(-tick_years / 25 My))
```

The 1.5 cm/yr drift base is the floor (mantle drag never fully stops a plate — no more boxed-in plates freezing to zero); 15 cm/yr is the sustained ceiling (no more plates crossing a quarter of the planet between screenshots); the 25 My exponential relax is tick-interval independent and produces the India-style slow-motion collision transient for free: an arriving continent chokes the trench (§5.9), the slab term vanishes, and the plate decays toward the drift base over ~25 My.

**Seeded rates.** Newborn plates (formation, splits, motion-changes) still get a sampled rate as an initial condition — log-normal, median `5.0 * plate_velocity_scale * rotation_factor` cm/yr, sigma 0.6, continental plates ×0.7 — because a fresh plate has no tally history yet. The relax converges in ~25 My, so the seed barely matters; it only keeps the first few ticks plausible.

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

1. **Update plate motion.** Relax every plate's motion rate toward its slab-pull force set-point (§2.4), then lock colliding continental pairs into a shared drift (collision jam, §4.6 — using last tick's contact tally; a 1-tick lag is geologically nothing), then advance rotation: increment `accumulated_rotation_rad` by `motion_rate_rad_per_year * tick_interval_years`.
2. **Re-partition hexes to plates.** Recompute `WorldData.plate_id[hex]` for all hexes based on each plate's current effective position. Genuinely converging continental-continental contests consume the loser's crust into the orogen (crustal shortening, §5.2), and the repartition reports which plate pairs are in sustained collision contact (≥ 3 converging continental contacts) to drive next tick's jam (§4.6).
3. **Detect boundary hexes and classify boundary types** (§3).
4. **Apply boundary effects to elevation** (§5).
5. **Apply hot spot effects** (§7).
6. **Apply erosion** (§8).
7. **Apply gravitational collapse** (§8.5) — relax adjacent-hex relief beyond the rock-strength limit.
8. **Check for plate reorganization events** (§4.5).
9. **Update sea level** (§4.7).
10. **Emit events** based on what happened this tick (§6).

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
| `tectonics.plate_rates` | Newborn plate motion rate seeding (§2.4) |
| `tectonics.plate_types` | Continental vs oceanic assignment |
| `tectonics.initial_elevation_noise` | Per-hex initial elevation variation |
| `tectonics.reorganization_check` | Per-tick check for whether a plate reorganization occurs |
| `tectonics.reorganization_action` | If reorganizing, which plates and how |
| `tectonics.hotspot_locations` | Initial hot spot positions |
| `tectonics.hotspot_activity` | Per-tick activity at each hot spot |
| `tectonics.volcanism` | Stochastic volcanic eruptions at boundaries |
| `tectonics.erosion_noise` | Per-tick erosion variation |
| `tectonics.subduction_erosion` | Per-tick forearc consumption draws (§5.10) |

Each is initialized once at plate creation and re-derived deterministically every tick. Different streams ensure that, e.g., tweaking volcanism logic doesn't change initial plate layout.

### 4.5 Plate Reorganization

Real plate tectonics is not static — plates split, merge, and change motion direction over hundreds of millions of years. Modeling this gives our worlds varied geological history (multiple supercontinent cycles, not just one static configuration).

Each Geological-era tick, a reorganization event fires with probability `0.004 * geology_activity_scale * (1 + 2 * stalled_fraction)`, where `stalled_fraction` is the share of plates creeping below the 2 cm/yr stall threshold (§2.4: slab-less plates relax to the ~1.5 cm/yr drift base, so welded continents pile up there while slab-driven plates sit far above it). A welded world — a supercontinent whose plates have sutured to a crawl — breaks up or redirects up to 3× sooner than an actively drifting one. This is the Wilson-cycle pacemaker: assembly stalls the machine, and the stall itself raises the pressure that tears it apart again (~16–30 events per 4.5B years at scale 1.0).

Reorganization is one of (the split/merge weights adjust to hold the plate census in the Earthlike 5–15 band of §11 #2 — at ≥15 plates splits downweight to 20% and mergers rise to 40%, at ≤5 plates splits rise to 60% and mergers switch off; motion changes always keep 40%):

- **Plate split** (normally 40% of events): A large plate (≥5% of grid cells) splits along the bisector between its seed and its farthest hex. The parent is chosen preferentially from *stalled* plates — rifts nucleate in welded supercontinents, not in plates that are already moving. The child gets a freshly sampled motion rate, and its motion axis is picked from 8 sampled candidates to maximize divergence from the parent along the new boundary line (parent seed → midpoint → child seed): the split opens a young ocean between the two halves instead of shearing or immediately re-colliding (§5.1).
- **Plate motion change** (40% of events): A randomly-chosen plate (preferentially a stalled one) gets a new motion axis. Models the "the plate slowed down and changed direction" that happens in real Earth history.
- **Plate merger** (normally 20% of events): Two adjacent plates merge into one. Often happens after extensive continental collision when the boundary effectively locks up.

Each reorganization emits an event with `Significance::Pivotal` (these are the supercontinent-cycle-defining moments).

### 4.6 Collision Locking (Kinematic Jam)

When two continents collide, neither plate bounces off and they do not merge into one plate — on Earth, India and Eurasia remain distinct plates ~50 My after initial contact. What dies is the *relative* motion across the suture: convergence is increasingly accommodated by crustal shortening inside the widening orogen (§5.2) instead of by plate advance, until the two plates are drifting together as one kinematic unit.

Mechanism:

- During repartition (§4.2 step 2), every cross-plate hex contest between continental crust on genuinely converging plates (closing speed above `CONVERGENCE_THRESHOLD_M_PER_YEAR`) counts toward a per-pair contact tally. Pairs with ≥ `COLLISION_CONTACT_HEXES` (3) such contacts are **colliding**.
- In the next tick's motion step, each colliding pair's rotation vectors relax toward their hex-count-weighted shared angular velocity with time constant `COLLISION_JAM_RELAX_YEARS = 10 My` (per-tick factor `1 − exp(−dt/τ)`). Relative velocity across the suture dies over ~10 My; the pair's absolute motion continues — the sutured continent drifts as one body, like India–Eurasia still moving north together today.
- Plate identities, registries, and boundaries are untouched. A jammed pair reads as stalled (< 2 cm/yr), which raises reorganization pressure (§4.5) until a split tears the welded continent apart along a new rift — breakup is the next Wilson cycle, not a bounce.

**Why not a registry merge:** an earlier version of this mechanism welded colliding plates into a single plate after sustained contact. Welds are deterministic while splits are stochastic, so welding outpaced splitting and the plate census collapsed to 3 plates by 1B years (§11 #2 requires 5–15), with the surviving super-plates paving 38% of the sphere. The kinematic jam keeps the census honest while producing the same observable geology.

### 4.7 Sea Level Drift

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

Rifts in our worlds usually originate at reorganization splits (§4.5): the split's divergence-maximizing child axis means the new boundary starts pulling apart immediately, the split-boundary subsidence (50 m on continental hexes along the new boundary) seeds the rift valley, and sustained divergence then carries it through continental rifting to a young ocean basin with passive margins on both sides.

### 5.2 Convergent: Continental-Continental

Two continents collide. Crust crumples upward.

For each boundary hex h at a continental-continental boundary:
- `elevation_mean[h] += orogeny_rate * velocity_cm_per_year * tick_interval_years`
- Where `orogeny_rate ≈ 5e-5 m per cm of convergence` (calibrated to produce ~5 km elevation over 100 million years of sustained collision)
- `elevation_relief[h] += orogeny_rate * 0.3 * velocity_cm_per_year * tick_interval_years` (mountains get rougher)
- Bedrock changes to `Metamorphic` (collision metamorphism)

Effect spreads inland — hexes within 4 hexes of the boundary on the continental side also gain elevation, with falloff `[1.0, 0.6, 0.3, 0.15]` per ring.

**Crustal shortening (partition level).** Boundary deltas alone raised terrain without consuming area, so two converging continents could contest the same hexes indefinitely — the loser was merely hidden while overridden and re-emerged unchanged if the plates separated, which read as continents passing straight through each other. Now, when a genuinely converging continental-continental contest resolves, the losing feature is **consumed** (deleted from its plate's surface — its area has become orogen) and the winning feature gains `SHORTENING_UPLIFT_M = 250 m` scaled by its elevation headroom taper, plus relief at `SHORTENING_RELIEF_FRACTION = 0.3` of the uplift, with bedrock set to `Metamorphic` and `age_year` refreshed. Overlap becomes mountains once, permanently: India–Asia has shortened by > 1,000 km into the Himalaya and the Tibetan Plateau, and neither continent is waiting underneath the other to pop back out.

### 5.3 Convergent: Oceanic-Continental

Oceanic plate subducts under continental plate.

For each boundary hex h on the **oceanic side**:
- Elevation decreases sharply (forming a trench): `elevation_mean[h] -= subduction_rate * velocity * tick_interval`
- `subduction_rate ≈ 1e-4 m per cm` (calibrated to produce ~10 km trench over 100 million years of sustained subduction)
- The trench equilibrium is enclosure-aware (§5.9): open-ocean segments deepen toward −8500 m; segments sealed off from the world ocean infill toward marginal-sea depth (−4500 m)

For each boundary hex h on the **continental side**:
- Elevation increases (coastal mountains) at 10% of the §5.2 orogeny rate (`OC_COASTAL_UPLIFT_FACTOR = 0.10`) — subduction arcs are modest beside continent-continent collision ranges
- The boundary hex itself — the **forearc** (ring 0) — receives only 30% of the arc uplift (`FOREARC_UPLIFT_FRACTION = 0.3`): it rises gently and stays emergent, so there is always low land between the trench and the high peaks (Chile exists between the Peru–Chile Trench and the Andes)
- Uplift spreads 4 rings inland with falloff `[0.4, 1.0, 0.6, 0.25]` per ring (`OC_INLAND_FALLOFF`), peaking at ring 2 — the magmatic arc crest sits ~150–450 km inland, so subduction coasts read as forearc strip → foothills → range instead of mountains dropping straight into the sea
- Volcanism is likely (see §5.5)
- Bedrock changes to `Igneous` (volcanic rock from arc magmatism)

**Coastal shelf (oceanic plate):** From the continental boundary hex, gentle subsidence spreads onto the oceanic plate for up to 2 hexes, using falloff fractions `[0.4, 0.15]` of the trench delta per ring. This produces a continental shelf → deep ocean gradient instead of an instant cliff at the plate boundary.

### 5.4 Convergent: Oceanic-Oceanic

One oceanic plate subducts under the other. Island arcs form on the upper plate.

For each boundary hex h on the **upper plate side**:
- Elevation increases sharply (volcanic islands forming)
- Bedrock changes to `Igneous`

For each boundary hex h on the **lower plate side**:
- Elevation decreases (the subducting trench); uses the same `subduction_rate ≈ 1e-4 m per cm` as §5.3, subject to choking and enclosure (§5.9)

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

### 5.8 Suture Accretion (Trapped Basin Consumption)

When an ocean basin closes, the oceanic floor caught between the colliding continental masses does not persist as a permanent inland sea — it is obducted onto the suture and isostatically rebounds with the continent (the Tethys → Himalaya mechanism). Each Geological tick (after hot spots, before erosion), the accretion pass:

1. Labels connected components of below-sea hexes (ascending-`HexId` BFS, deterministic).
2. Components covering ≥ 1% of all cells are open-ocean realm and never touched.
3. Components touching a **live** convergent edge are active trench / back-arc systems: their basin and crust are left alone (floors deeper than marginal-sea depth are still capped per §5.9). "Live" requires real closing velocity: `normal_velocity > CONVERGENCE_THRESHOLD_M_PER_YEAR` (0.005 m/yr). A boundary still *classified* convergent but stalled — plates creeping below the threshold after the trench choked and their slab pull vanished (§2.4) — no longer protects its basin: the fossil trench heals.
4. Components touching a **live** divergent edge are active rift / infant-ocean systems — a growing ocean (Red Sea, Gulf of California), not a trapped basin: their basin and crust are likewise left alone, with the same §5.9 floor cap (hot young ocean floor sits high, never at trench depth). "Live" mirrors the convergent rule: opening velocity beyond the same 0.005 m/yr threshold (`normal_velocity < −CONVERGENCE_THRESHOLD_M_PER_YEAR`, the same condition §3.2 uses to count ridge edges). A stalled rift no longer protects its basin: the failed-rift sea heals. Without this guard the pass strangled every infant ocean at birth — newborn below-sea rift floor is a small enclosed component and converted in a single tick — feeding the deep-time continental ratchet (see v0.14 changelog).
5. Every other component is trapped: its floor sediments to `MARGINAL_SEA_EQUILIBRIUM_M` at once (§5.9 enclosure — a sealed basin never holds abyssal depth), and each oceanic-crust hex converts to continental crust **gradually**, with per-tick probability compounded from `SUTURE_HALF_LIFE_MY = 55 My` (`1 − 0.5^(Δt/55 My)`, tick-interval independent; per-tick `tectonics.suture_conversion` stream, ascending-hex draws). Suturing is a process, not an event: the Tethys took ~100 My, and Mediterranean/Caspian-style enclosed seas persist as oceanic crust while the suture grinds — so a trapped sea fills in over a Wilson half-cycle instead of flipping to permanent continent in one tick. This bounds the deep-time continental ratchet (instant conversion compounded to 0.59–0.71 crust fraction by 4B years; Earth's has held ~40% since ~2.5 Ga). At conversion the hex obducts: `OceanicCrust` bedrock becomes `Igneous` (ophiolite basement) and lifts to `sea_level − 200 m`. Continental-crust hexes sitting deeper than the obduction depth are lifted there immediately (not stochastically) — epeirogenic rebound refuses crust below its floor (sea − 2000 m), so deep continental floors in fossil trenches would otherwise stay pinned underwater forever (slab-breakoff rebound). Standard epeirogenic rebound (§8.2) then carries everything above the freeboard over the following ~50–100 My.

Without this pass, trapped crust sat on the −4000 m oceanic floor forever (isostasy only lifts continental crust), riddling continents with ~150 detached inland seas by 1B years.

### 5.9 Subduction Choking and Trench Enclosure

Two guards keep trenches honest over deep time; both live in the trench arms of §5.3–§5.4.

**Choking.** Subduction only consumes dense oceanic lithosphere. When continental crust — collision debris, an obducted sliver, a microcontinent — reaches the downgoing side of a trench, the zone chokes: buoyant crust jams instead of sinking, and no trench delta is applied. Without this guard, continental debris on a subducting plate was dragged below −8000 m and pinned under the isostatic rebound floor (§8.2) forever.

**Enclosure.** A trench only stays abyssal while it connects to the abyss. Each tick the boundary-elevation pass labels below-sea connected components (same 1% open-ocean definition as §5.8); a trench segment whose component is *not* open ocean equilibrates at `MARGINAL_SEA_EQUILIBRIUM_M = −4500 m` (Japan Sea ≈ −3700 m, Mediterranean ≈ −5100 m) instead of the abyssal −8500 m. Above that floor the slab still pulls down; below it, sediment infill from the enclosing continents wins at 10× the subduction rate, so a freshly sealed −8500 m segment rises above −6000 m within ~1–2 ticks (1 My) instead of persisting as a fossil pit. Back-arc pits behind the trench line get no trench-arm processing, so the §5.8 accretion pass applies the same cap to live-protected components: their crust and basin survive, but their floor may not pass −4500 m. Both passes share one equilibrium, so neither fights the other. The infill is a placeholder for Doc 8's real sediment transport.

### 5.10 Subduction Erosion (Forearc Consumption)

Suture accretion (§5.8) only *creates* continental crust, so without a sink the continental fraction ratchets upward over deep time (it reached ~53% of the sphere by 4B years). On Earth roughly half of all margins are erosive (von Huene & Scholl): the trench slowly consumes the overriding plate's forearc rim and drags the slivers into the mantle.

Each tick, every boundary hex on the **overriding (continental-crust) side** of a *live* continent–ocean margin (same 0.005 m/yr liveness threshold as §5.8) is consumed with probability `1 − (1 − p)^(tick_years/1 My)` with `p = 0.0025 × (hex_width / reference_width)` — the reference width is subdivision 7's, so the margin retreat rate (~0.15 km/My, the gentle end of Earth's erosive margins) is **resolution-independent**. Unscaled per-hex, a coarse grid consumes ~4× more area per event; at subdiv 5 the raw rate destroyed half to three-quarters of all continental crust within 1B years. A consumed hex becomes oceanic crust at ≥ 1000 m below sea level; the §5.3 trench arm treats it as the downgoing side from the next tick, so the margin migrates inland hex by hex. The per-hex probability compounds with tick length, keeping the long-run rate tick-interval independent; draws come from the per-tick `tectonics.subduction_erosion` stream in ascending-hex order (deterministic). The rate sits at Earth's slow end by design: the model has no arc-accretion source term, so erosion must not outrun formation. Calibration at the reference resolution: at `p = 0.01` erosion overwhelmed accretion and drained the continents to ~5% by 4B years.

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
    /// Default 5e-8. Climate modifies via climate_modifier (Phase 2).
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
- `base_erosion_rate ≈ 5e-8` per year, scaled per hex by bedrock hardness: `Sedimentary` 1.2, `Limestone` 1.0, `Metamorphic` 0.15 (collision belts persist hundreds of My, like the Appalachians), `Igneous` 0.08 (cratons erode over billion-year scales); see open question 3
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

### 8.5 Gravitational Collapse (Rock-Strength Relief Limit)

Erosion is not the only way mountains come down, and on a still-dry world (water returns in Doc 8) it is far too slow to police cliff geometry. Rock has finite strength: overthickened crust spreads under its own weight (**orogenic collapse** — the Tibetan Plateau is extending horizontally today), and no single rock face on Earth exceeds ~4,600 m (Nanga Parbat's Rupal face). The simulation previously had no such limit, so a ~9 km collision peak could stand one hex away from a below-sea floor — a five-mile instant cliff.

After the erosion step each tick, scan adjacent hex pairs and relax steps beyond the cap, with three regimes per edge:

- **Low side at trench depth** (≤ −6,000 m): cap 15,000 m, enforced **one-sided** — the arc side sheds, the floor never moves. That step is the subduction interface, not a single rock face (Earth's trench-to-summit profiles reach ~15 km over ~200 km, which a coarse hex grid must represent as one or two adjacent steps). Symmetric transfer here is forbidden: at tick scale it becomes an elevation conveyor from continents into live trenches that flattens all planetary relief.
- **Low side submerged** (above trench depth): cap 5,000 m, one-sided — the high side sheds the excess into the ocean basin (sediment space is effectively infinite; Doc 8 will route it). This is what saws off coastal cliffs.
- **Both sides land**: cap 5,000 m, symmetric — the excess spreads (high sheds, low receives: mass-conserving plateau extension).

Pairs are skipped unless both hexes carry real surface features, so collapse never mints terrain into projection holes. The relaxation time is 10 My (~5% of the excess per 500k-year tick).

**Two hard-won calibration notes.** (1) Do not shorten the time constant toward instant clipping: with relax → 1.0 per tick the coastal clip flattens coastal relief faster than margin geometry can recover, the water realm fragments into sub-1% components, and §5.8 obduction + §5.9 enclosure-infill cascade — measured: crust paving to 27–64%, trenches infilled to nothing, world relief compressed to the isostatic band [−3,500, +800] by 1B years. At 10 My the budget is stable through 4.5B years. (2) The price of the slow constant is that actively pumped margins equilibrate ABOVE the cap (measured 7–12 km non-trench steps at 1B) — the cliffs are bounded, not eliminated. A hard 5–6 km cap needs a **pump-side relief limit** (the boundary passes refuse uplift that would break the step cap and spread it to the next inland ring instead of moving mass after the fact); that is a separate, deliberate change deferred past Doc 8, when erosion and sediment routing can share the work.

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
9. **No inland seas:** below-sea hexes disconnected from the open ocean never persist as trapped oceanic-crust basins — suture accretion (§5.8) consumes them each tick (regression guard `history_leaves_no_trapped_oceanic_basins`)
10. **Continental crust budget at 1B years:** continental crust covers 12–45% of the sphere — the Wilson cycle redistributes crust but sinks must not consume it (the unscaled subduction-erosion leak ate 50–75% of it at subdiv 5) nor may accretion pave the planet (the pre-v0.13 ratchet). The gate is on crust AREA, not land fraction: land at a snapshot is hostage to Wilson phase, the sea-level walk, and resolution (per-hex sink events bite proportionally harder on coarse grids), so a fixed-seed land band polices noise instead of physics. Land fraction is still measured and printed informationally (measured 13–25% across seeds at subdiv 5–7; early Earth plausibly exposed less land than today) (regression gate `wilson_cycle_criteria_hold_at_one_billion_years`)
11. **No fossil abyss:** at 1B years, detached below-sea cells (water bodies smaller than the 1%-of-cells open-ocean threshold of §5.8 — a larger connected body is a real secondary ocean, not an inland sea) cover < 2% of all cells, and none are deeper than −6000 m — fossil trenches heal (§5.8), subduction chokes rather than dragging continental crust down, and sealed trench segments infill to marginal-sea depth (§5.9). Components touching a **live** margin, convergent or divergent (§5.8 liveness rules), are current geology — an active trench/back-arc still being consumed or a rift floor still being born (Afar, Baikal) — and are excluded from this check
12. **Passive margins exist:** at 1B years, ≥ 25% of coastline hexes are passive margins (more than 2 rings from any convergent boundary) — Atlantic-style trailing edges left by ocean-opening splits (§4.5), not a planet walled in by arcs. *Status: tracked but not yet gated* — the Wilson split machinery roughly doubled the passive share (6.6% → ~16% at 1B); subduction erosion (v0.13) did not lift it further, so divergence-side margin creation is the remaining lever; the regression gate reports it informationally
13. **Earth-like plate speeds:** at 1B years, every plate runs between 0.5 and 20 cm/yr (× `plate_velocity_scale`) — slab pull (§2.4) bounds speeds between the ~1.5 cm/yr drift base and the 15 cm/yr sustained ceiling, so plates neither freeze nor cross the planet between screenshots (regression gate `wilson_cycle_criteria_hold_at_one_billion_years`)
14. **Bounded adjacent relief:** at 1B years, no two adjacent hexes differ by more than 12,000 m (or 16,500 m when the low side sits at or below marginal-sea depth, −4,500 m — trench arms and §5.9-capped live-margin basins alike; Earth's trench-to-summit profiles reach ~15 km over ~200 km, which a coarse hex grid represents as one or two adjacent steps). Gravitational collapse (§8.5) holds steps near its 5,000 m rock-face cap everywhere except actively pumped margins, where it provably equilibrates above the cap (measured 8.3–11.1 km non-trench, 15.5–16.3 km margin-adjacent across the 3 gate seeds); the gate's bands bracket those measurements and catch a deleted/neutered collapse pass (the pre-collapse worst case was an 18 km clamp-pinned step). A hard 5–6 km cap is deferred to the pump-side relief limit (§8.5 note 2)

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
    ├── lib.rs                # public API + TectonicsLayer impl
    ├── layer.rs              # SimulationLayer integration (per-tick pipeline wiring)
    ├── history.rs            # History generation driver (tick coordinator)
    ├── plate.rs              # Plate, PlateType, Plate registry
    ├── plate_surface.rs      # Per-plate birth-indexed surface features
    ├── motion.rs             # Plate motion math (rotation about axis)
    ├── frames.rs             # Birth-frame ↔ world-frame conversion
    ├── projection.rs         # ProjectionCache (world-hex → birth-hex table)
    ├── partition.rs          # Voronoi partition (hex → plate)
    ├── world_rebuild.rs      # Rebuilds WorldData per-hex fields from plate surfaces
    ├── boundary.rs           # Boundary detection and classification
    ├── boundary_events.rs    # Boundary-derived tectonic events
    ├── elevation.rs          # Per-boundary-type elevation update rules
    ├── accretion.rs          # Suture accretion (§5.8, trapped-basin consumption)
    ├── volcanism.rs          # Boundary-driven and hot spot volcanism
    ├── hotspots.rs           # Hot spot model
    ├── erosion.rs            # Erosion and sedimentation
    ├── sea_level.rs          # Sea level drift from divergent boundary length
    ├── coast_cleanup.rs      # Removes geologically unjustified coast artifacts
    ├── collapse.rs           # Gravitational collapse: rock-strength limit on adjacent-hex relief (§8.5)
    ├── collision_jam.rs      # Kinematic lock for colliding continental pairs (§4.6)
    ├── initial_generation.rs # Initial plate generation at world formation
    ├── initial_terrain.rs    # Formation-era initial elevation and bedrock
    ├── reorganization.rs     # Plate split / merge / motion change
    ├── validation.rs         # §11 metrics and CI validation helpers
    ├── diagnostics.rs        # Test-only manual terrain report (--ignored)
    └── events.rs             # Event emission and granularity gating
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
