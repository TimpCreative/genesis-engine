//! Suture-zone accretion: trapped oceanic crust is consumed into continents.
//!
//! When an ocean basin closes, the oceanic floor and trench features caught
//! between the colliding continental masses do not persist as permanent
//! inland seas — they are obducted onto the suture and isostatically rebound
//! with the continent (the Tethys → Himalaya mechanism). Each Geological tick
//! this pass finds below-sea components that no longer connect to the open
//! ocean and converts their oceanic-crust hexes to continental crust, lifting
//! them near the surface so standard epeirogenic rebound (see `erosion.rs`)
//! carries them to the freeboard over the following ~50–100 My.
//!
//! Conversion is **gradual**: suturing is a process, not an event — the
//! Tethys took ~100 My to suture, and Mediterranean/Caspian/Black-Sea-style
//! enclosed basins persist as oceanic crust for tens of My while it grinds.
//! Each trapped oceanic hex converts with a per-tick probability compounded
//! from [`SUTURE_HALF_LIFE_MY`] (tick-interval independent, deterministic
//! per-tick stream), so a trapped sea fills in over a Wilson half-cycle
//! instead of flipping in one tick. This bounds the deep-time continental
//! ratchet: instant conversion made every basin enclosure permanent
//! continent the same tick, compounding to 0.59–0.71 crust fraction by 4B
//! years (measured, subdiv 7); Earth's crust has held ~40% since ~2.5 Ga.
//! Meanwhile the enclosed basin floor sediments to marginal-sea depth at
//! once (§5.9 enclosure — a sealed basin never holds abyssal depth).
//!
//! Live subduction is protected: a component adjacent to a convergent edge
//! that is *actively closing* (normal velocity above
//! [`CONVERGENCE_THRESHOLD_M_PER_YEAR`]) is still being consumed at a trench
//! and is left alone. A stalled boundary (collision-damped plates creeping at
//! the stall floor) no longer protects its basin: the fossil trench then
//! heals — oceanic crust is obducted, and deep continental floors are lifted
//! to the obduction depth, since epeirogenic rebound refuses crust below its
//! floor (slab-breakoff rebound). Deterministic: hexes are visited in
//! ascending `HexId` order with the grid's fixed neighbor order, no RNG.
//!
//! Live rifts are protected for the mirror reason: a component adjacent to a
//! divergent edge that is *actively opening* is a growing ocean (Red Sea,
//! Gulf of California), not a trapped basin. Without this guard the pass
//! strangles every infant ocean at birth — newborn below-sea rift floor is a
//! small enclosed component, converts to continental in a single tick, and
//! the ocean never widens. Measured over a 4B-year run (subdiv 7) that
//! ratchet flooded the planet: continental crust grew 31% → 73% while the
//! subduction-erosion sink consumed barely ~4,600 hex-events total. A stalled
//! rift (opening below the threshold) no longer protects its basin.
//!
//! The counter-flow is subduction erosion ([`apply_subduction_erosion`]):
//! accretion only grows continents, so without a sink the continental area
//! ratchets upward over billions of years. On Earth roughly half of all
//! margins are erosive (von Huene & Scholl): the trench slowly consumes the
//! overriding plate's forearc rim, dragging continental slivers into the
//! mantle. That keeps the continental fraction bounded instead of flooding
//! the planet with land.

use std::collections::VecDeque;

use genesis_core::HexId;
use genesis_core::data::{BedrockType, WorldData};
use genesis_core::rng::WorldRng;
use rand::Rng;

use crate::boundary::{BoundaryClass, BoundaryInfo, ConvergentSubtype};
use crate::elevation::MARGINAL_SEA_EQUILIBRIUM_M;
use crate::plate::PlateRegistry;
use crate::plate_surface::{continental_crust_at, modify_surface_at_world_hex};
use crate::projection::ProjectionCache;

/// A below-sea component covering at least this fraction of the sphere is
/// open-ocean realm and never accreted (1% of cells; the world ocean is one
/// giant component, trapped basins are small).
pub const OPEN_OCEAN_MIN_FRACTION: f64 = 0.01;

/// Trapped crust is lifted to this depth below sea level at accretion
/// (obduction emergence); rebound then carries it up to the freeboard.
pub const OBDUCTION_DEPTH_M: f32 = 200.0;

/// Permanent crustal root banked when a terrane accretes to continental
/// crust (Doc 06 §5.2 roots): docked arc basement is thickened crust, so
/// terrane-built landmasses carry hills, never featureless shelf.
pub const ACCRETED_TERRANE_ROOT_M: f32 = 350.0;

/// Continental shelf allowance over the land target (Doc 06 §5.11 crust
/// supply): Earth's continental crust covers ~40% of the surface while land
/// is ~29% — the difference is submerged shelf and slope.
pub const CRUST_SHELF_ALLOWANCE: f64 = 0.08;
/// Cap on continentalization per tick, as a fraction of world cells: the
/// supply controller trickles conversions (~8 cells/My at subdivision 6)
/// rather than flipping provinces wholesale. Outpaces the measured
/// collision-consumption rate (~3 cells/My) while keeping the coastline
/// churn from margin growth below the §7.1 stability gate's threshold.
pub const CONTINENTALIZATION_MAX_PER_TICK: f64 = 0.0005;
/// Only crust standing above this feature-frame elevation may
/// continentalize (shelf grade and up): abyssal floor stays oceanic no
/// matter the deficit.
pub const CONTINENTALIZATION_MIN_ELEVATION_M: f32 = -1000.0;

/// Basement root for continentalized crust, graded by the elevation the
/// crust stood at when it converted: a taller arc or swell is thicker crust
/// (isostasy), so conversion fossilizes the edifice's shape into permanent
/// basement — young continents assembled from converted crust inherit
/// ridge-shaped highlands instead of a uniform featureless plateau.
pub fn continentalized_root_m(elevation_m: f32) -> f32 {
    (ACCRETED_TERRANE_ROOT_M + (elevation_m - CONTINENTALIZATION_MIN_ELEVATION_M).max(0.0) * 0.25)
        .min(crate::elevation::ROOT_MAX_M)
}

/// Deterministic RNG stream for subduction-erosion draws.
pub const SUBDUCTION_EROSION_STREAM: &str = "tectonics.subduction_erosion";

/// Per-million-year probability that one forearc rim hex at a live erosive
/// margin is consumed by the trench, **at the reference resolution of
/// subdivision 7**; [`apply_subduction_erosion`] scales it by hex width so the
/// margin retreat rate (~0.15 km/My — the gentle end of Earth's erosive
/// margins, von Huene & Scholl) is resolution-independent. Unscaled, a coarse
/// grid consumes ~4× more area per event: at subdiv 5 the raw rate destroyed
/// half to three-quarters of all continental crust within 1B years. The sim
/// has no arc-accretion source term, so the rate deliberately sits at the
/// slow end of Earth's range — at 0.01/My (reference) erosion overwhelms
/// accretion and drains the continents to ~5% by 4B years.
pub const SUBDUCTION_EROSION_PROBABILITY_PER_MY: f64 = 0.0025;

/// Subdivision level at which [`SUBDUCTION_EROSION_PROBABILITY_PER_MY`] is
/// calibrated (Doc 06 §5.10).
const SUBDUCTION_EROSION_REFERENCE_SUBDIVISION: u32 = 7;

/// A consumed forearc hex drops at least this far below sea level at
/// conversion; the trench arm deepens it toward trench equilibrium from
/// there on the following ticks.
pub const SUBDUCTION_EROSION_DOWNSTEP_M: f32 = 1000.0;

/// Deterministic RNG stream for suture-conversion draws.
pub const SUTURE_CONVERSION_STREAM: &str = "tectonics.suture_conversion";

/// Half-life of a trapped oceanic basin's conversion to continental crust,
/// in millions of years. Suturing grinds for tens of My on Earth (the
/// Tethys took ~100 My; the Mediterranean and Caspian persist as enclosed
/// oceanic crust today), so conversion is a per-hex half-life process, not
/// a one-tick event. The value is calibrated against 4B-year runs at
/// subdiv 7 across multiple realizations — the measured equilibria are in
/// the v0.14 changelog. No resolution normalization is needed: the draw is
/// per hex per My, so a basin loses a fixed *fraction* of its hexes per
/// My at any grid size.
pub const SUTURE_HALF_LIFE_MY: f64 = 55.0;

/// Per-tick probability that one trapped oceanic-crust hex converts to
/// continental crust, compounded from [`SUTURE_HALF_LIFE_MY`] so the
/// long-run rate is tick-interval independent (mirrors the subduction
/// erosion form: p(0) = 0, p(75 My) = 0.5, p(1 Gyr) ≈ 1).
fn suture_conversion_probability(tick_interval_years: f64) -> f64 {
    1.0 - 0.5_f64.powf(tick_interval_years / (SUTURE_HALF_LIFE_MY * 1_000_000.0))
}

/// Connected-component labeling of the below-sea water realm. Computed once
/// per tick and shared by the elevation pass (trench enclosure) and the
/// accretion pass (trapped-basin detection) — `data.elevation_mean` only
/// changes at world rebuilds, so one labeling is exact for both.
#[derive(Clone, Debug)]
pub struct WaterComponents {
    /// Component id per hex; `usize::MAX` for above-sea hexes.
    pub comp_of: Vec<usize>,
    /// Hex count per component.
    pub comp_sizes: Vec<usize>,
    /// Minimum component size to count as open ocean (§5.9 enclosure).
    pub open_ocean_min: usize,
}

impl WaterComponents {
    /// `true` for a hex whose water body connects to the world ocean.
    pub fn is_open_ocean(&self, idx: usize) -> bool {
        let id = self.comp_of[idx];
        id != usize::MAX && self.comp_sizes[id] >= self.open_ocean_min
    }

    /// Full open-ocean mask for the elevation pass.
    pub fn open_ocean_mask(&self) -> Vec<bool> {
        (0..self.comp_of.len())
            .map(|i| self.is_open_ocean(i))
            .collect()
    }
}

/// Labels below-sea connected components. Deterministic: ascending `HexId`
/// BFS with the grid's fixed neighbor order.
pub fn label_water_components(data: &WorldData) -> WaterComponents {
    let grid = &data.grid;
    let n = data.cell_count() as usize;
    let sea = data.sea_level_m;
    let below_sea = |i: usize| data.elevation_mean[i] < sea;
    let open_ocean_min = (n as f64 * OPEN_OCEAN_MIN_FRACTION).ceil() as usize;
    let mut comp_of = vec![usize::MAX; n];
    let mut comp_sizes: Vec<usize> = Vec::new();
    for start in 0..n {
        if !below_sea(start) || comp_of[start] != usize::MAX {
            continue;
        }
        let id = comp_sizes.len();
        let mut size = 0usize;
        let mut queue = VecDeque::from([start]);
        comp_of[start] = id;
        while let Some(i) = queue.pop_front() {
            size += 1;
            for nb in grid.neighbors(HexId(i as u32)) {
                let j = nb.0 as usize;
                if j < n && below_sea(j) && comp_of[j] == usize::MAX {
                    comp_of[j] = id;
                    queue.push_back(j);
                }
            }
        }
        comp_sizes.push(size);
    }
    WaterComponents {
        comp_of,
        comp_sizes,
        open_ocean_min,
    }
}

/// Open-ocean mask: `true` for below-sea hexes whose connected component
/// covers at least [`OPEN_OCEAN_MIN_FRACTION`] of all cells. Only open-ocean
/// crust can hold an abyssal trench — sealed-off segments infill toward
/// marginal-sea depth (see `MARGINAL_SEA_EQUILIBRIUM_M` in `elevation.rs`).
/// Convenience wrapper; callers that also run the accretion pass should
/// [`label_water_components`] once and share it instead.
pub fn open_ocean_mask(data: &WorldData) -> Vec<bool> {
    label_water_components(data).open_ocean_mask()
}

/// Finds trapped oceanic-crust basins and accretes them onto their
/// surrounding continent. `water` is the tick's shared water-realm labeling
/// (see [`label_water_components`]). Conversion is stochastic with a
/// [`SUTURE_HALF_LIFE_MY`] half-life (see module docs); draws come from the
/// per-tick [`SUTURE_CONVERSION_STREAM`] in ascending-`HexId` order.
/// Returns the number of hexes accreted (converted this tick).
#[allow(clippy::too_many_arguments)]
pub fn accrete_trapped_oceanic_crust(
    data: &WorldData,
    registry: &mut PlateRegistry,
    cache: &ProjectionCache,
    water: &WaterComponents,
    boundaries: &BoundaryInfo,
    rng: &WorldRng,
    tick_year: i64,
    tick_interval_years: f64,
) -> u64 {
    let n = data.cell_count() as usize;
    let sea = data.sea_level_m;
    let probability = suture_conversion_probability(tick_interval_years);
    let mut stream = rng.stream_at(SUTURE_CONVERSION_STREAM, tick_year as u64);

    // Hexes at an actively closing convergent margin (either side of the
    // edge): a live trench and its marginal basins are still being consumed
    // and are left alone. "Live" requires real closing velocity — a
    // collision-stalled boundary still *classified* convergent no longer
    // protects its fossil trench, so healing can proceed.
    let mut active_subduction = vec![false; n];
    for (&hex, edges) in &boundaries.edges {
        for edge in edges {
            let live = matches!(edge.class, BoundaryClass::Convergent(_))
                && edge.normal_velocity_m_per_year
                    > crate::partition::CONVERGENCE_THRESHOLD_M_PER_YEAR;
            if live {
                active_subduction[hex.0 as usize] = true;
                let j = edge.neighbor_hex.0 as usize;
                if j < n {
                    active_subduction[j] = true;
                }
            }
        }
    }

    // Hexes at an actively opening divergent margin (either side of the
    // edge): the rift is minting new ocean floor, so the basin beside it is
    // a growing ocean, not a trapped one. "Live" mirrors the convergent
    // rule — opening velocity beyond the same magnitude threshold
    // (`boundary.rs` counts ridge edges with exactly this condition); a
    // stalled rift stops protecting its basin and healing can proceed.
    let mut active_rift = vec![false; n];
    for (&hex, edges) in &boundaries.edges {
        for edge in edges {
            let opening = matches!(edge.class, BoundaryClass::Divergent)
                && edge.normal_velocity_m_per_year
                    < -crate::partition::CONVERGENCE_THRESHOLD_M_PER_YEAR;
            if opening {
                active_rift[hex.0 as usize] = true;
                let j = edge.neighbor_hex.0 as usize;
                if j < n {
                    active_rift[j] = true;
                }
            }
        }
    }

    // Fold the shared water labeling with live-subduction contact: a trapped
    // component adjacent to an active trench is still being consumed. Same
    // fold for live-rift contact: that component is still being born.
    let mut comp_touches_subduction = vec![false; water.comp_sizes.len()];
    let mut comp_touches_rift = vec![false; water.comp_sizes.len()];
    for (i, &id) in water.comp_of.iter().enumerate() {
        if id != usize::MAX && active_subduction[i] {
            comp_touches_subduction[id] = true;
        }
        if id != usize::MAX && active_rift[i] {
            comp_touches_rift[id] = true;
        }
    }

    let below_sea = |i: usize| data.elevation_mean[i] < sea;
    let open_ocean = |id: usize| water.comp_sizes[id] >= water.open_ocean_min;

    // Accrete oceanic-crust hexes in trapped components that are neither
    // actively subducting nor actively rifting.
    let mut accreted = 0u64;
    for i in 0..n {
        if !below_sea(i) {
            continue;
        }
        let id = water.comp_of[i];
        if open_ocean(id) {
            continue;
        }
        let hex = HexId(i as u32);
        if comp_touches_subduction[id] || comp_touches_rift[id] {
            // Live margin — convergent or divergent: the basin and its crust
            // stay (still being consumed at a trench, or still being born at
            // a rift). But an enclosed basin cannot hold abyssal depth (§5.9
            // enclosure): a cut-off trench segment fills toward marginal-sea
            // depth, and hot young floor over an opening rift sits high
            // (ridge crests ≈ −2.5 km), never at trench depth. Cap the floor
            // at marginal-sea depth either way. Back-arc pits a hex behind
            // the trench line get no trench-arm infill, so this pass is the
            // only thing that sees them; the target matches the trench arm's
            // sealed equilibrium, so the two never fight.
            modify_surface_at_world_hex(registry, data, cache, hex, tick_year, |feature| {
                if feature.elevation_m < MARGINAL_SEA_EQUILIBRIUM_M {
                    feature.elevation_m = MARGINAL_SEA_EQUILIBRIUM_M;
                }
            });
            continue;
        }
        if continental_crust_at(data, registry, cache, hex) {
            // Continental crust in a trapped basin: epeirogenic rebound
            // refuses anything below its floor (sea − 2000 m), so deep
            // fossil-trench floors would stay pinned forever. Basin fill +
            // slab-breakoff rebound lifts them to the obduction depth here;
            // ordinary rebound carries them to the freeboard after.
            modify_surface_at_world_hex(registry, data, cache, hex, tick_year, |feature| {
                if feature.elevation_m < sea - OBDUCTION_DEPTH_M {
                    feature.elevation_m = sea - OBDUCTION_DEPTH_M;
                }
            });
            accreted += 1;
            continue;
        }
        // Trapped oceanic crust. The enclosed basin sediments to
        // marginal-sea depth at once (§5.9 enclosure — a sealed basin never
        // holds abyssal depth), but conversion to continent is gradual: the
        // suture grinds for tens of My (Tethys ≈ 100 My) and the sea fills
        // in hex by hex, so enclosure no longer mints permanent continent
        // the same tick (the deep-time ratchet). Obduction emergence stays
        // instant at conversion.
        let convert = stream.gen_range(0.0..1.0) < probability;
        modify_surface_at_world_hex(registry, data, cache, hex, tick_year, |feature| {
            if feature.elevation_m < MARGINAL_SEA_EQUILIBRIUM_M {
                feature.elevation_m = MARGINAL_SEA_EQUILIBRIUM_M;
            }
            if convert {
                feature.continental_crust = true;
                // Doc 06 §5.2 roots: accreted terranes carry thickened arc
                // crust of their own (Wrangellia, the Cordilleran terranes) —
                // a modest permanent basement root. The surface still accretes
                // at obduction depth (shallow shelf); the root binds later,
                // when orogeny lifts this crust and erosion tries to plane it
                // back below its basement.
                feature.root_m = feature
                    .root_m
                    .max(continentalized_root_m(feature.elevation_m));
                if feature.bedrock == BedrockType::OceanicCrust {
                    // Obducted ocean floor becomes continental basement (ophiolite).
                    feature.bedrock = BedrockType::Igneous;
                }
                if feature.elevation_m < sea - OBDUCTION_DEPTH_M {
                    feature.elevation_m = sea - OBDUCTION_DEPTH_M;
                }
            }
        });
        if convert {
            accreted += 1;
        }
    }
    accreted
}

/// Subduction erosion: live continent–ocean margins slowly consume the
/// overriding plate's forearc rim, returning continental crust to the mantle
/// (the counter-flow to accretion's obduction — without it continental area
/// ratchets upward forever). A consumed hex becomes oceanic crust at trench
/// depth; the CO trench arm treats it as the downgoing side from the next
/// tick. Only margins closing faster than
/// [`crate::partition::CONVERGENCE_THRESHOLD_M_PER_YEAR`] erode — a stalled
/// suture is not consuming anything. Deterministic: candidates are visited
/// in ascending `HexId` order with a per-tick RNG stream; the per-hex
/// probability compounds with tick length so the long-run rate is
/// tick-interval independent. Returns the number of hexes consumed.
pub fn apply_subduction_erosion(
    data: &WorldData,
    registry: &mut PlateRegistry,
    cache: &ProjectionCache,
    boundaries: &BoundaryInfo,
    rng: &WorldRng,
    tick_year: i64,
    tick_interval_years: f64,
) -> u64 {
    // Resolution normalization: the rate above is calibrated at subdivision 7.
    // Retreat per My = p × hex width, and hex width ∝ 1/√cell_count, so
    // scaling p by √(cells/reference_cells) keeps the retreat rate (km/My)
    // identical at any subdivision level.
    let cells = f64::from(data.cell_count());
    let reference_cells = f64::from(10u32 * 4u32.pow(SUBDUCTION_EROSION_REFERENCE_SUBDIVISION) + 2);
    let width_ratio = (cells / reference_cells).sqrt();
    let rate_per_my = SUBDUCTION_EROSION_PROBABILITY_PER_MY * width_ratio;
    let probability = 1.0 - (1.0 - rate_per_my).powf(tick_interval_years / 1_000_000.0);
    if probability <= 0.0 {
        return 0;
    }
    let mut stream = rng.stream_at(SUBDUCTION_EROSION_STREAM, tick_year as u64);
    let sea = data.sea_level_m;

    let mut eroded = 0u64;
    for (&hex, edges) in &boundaries.edges {
        let live_co_margin = edges.iter().any(|edge| {
            matches!(
                edge.class,
                BoundaryClass::Convergent(ConvergentSubtype::ContinentalOceanic)
            ) && edge.normal_velocity_m_per_year
                > crate::partition::CONVERGENCE_THRESHOLD_M_PER_YEAR
        });
        if !live_co_margin {
            continue;
        }
        // Only the overriding (continental) rim is consumed; the oceanic side
        // is the downgoing slab. A hex converted earlier in this loop fails
        // this check, so multi-edge rims convert at most once per tick.
        if !continental_crust_at(data, registry, cache, hex) {
            continue;
        }
        if stream.gen_range(0.0..1.0) >= probability {
            continue;
        }
        modify_surface_at_world_hex(registry, data, cache, hex, tick_year, |feature| {
            feature.continental_crust = false;
            feature.bedrock = BedrockType::OceanicCrust;
            feature.elevation_m = feature.elevation_m.min(sea - SUBDUCTION_EROSION_DOWNSTEP_M);
        });
        eroded += 1;
    }
    eroded
}

/// Doc 06 §5.11: net crust conservation, supply side. Convergence consumes
/// continental area (crustal shortening, subduction erosion), yet Earth's
/// crust budget holds roughly steady over 4 By — arc magmatism and the
/// continentalization of long-emergent, thickened crust replace what
/// collisions eat. Without the counter-flow, measured coverage collapses
/// 37% → 9% over 4.4 By and the hypsometry transfer must promote featureless
/// abyssal floor to land ("false continents").
///
/// The controller holds `continental_crust` coverage at the calibration land
/// target plus [`CRUST_SHELF_ALLOWANCE`] by converting the highest-standing
/// oceanic crust — the arcs, hotspot plateaus, and collision-thickened
/// marginal floors already acting as land — at a bounded per-tick rate.
/// Converted crust receives young basement ([`ACCRETED_TERRANE_ROOT_M`]) and
/// isostasy then lifts it toward the continental freeboard: continents grow
/// at their active margins, exactly where Earth grows them. Deterministic:
/// candidates ordered by descending elevation, ascending `HexId`; no RNG.
pub fn maintain_crust_supply(
    data: &mut WorldData,
    registry: &mut PlateRegistry,
    cache: &ProjectionCache,
    tick_year: i64,
) {
    let n = data.cell_count() as usize;
    if n == 0 {
        return;
    }
    let target_fraction =
        f64::from(data.parameters.core.terrain.land_fraction) + CRUST_SHELF_ALLOWANCE;
    let target_cells = (target_fraction * n as f64).ceil() as usize;
    let current = data.continental_crust.iter().filter(|&&c| c).count();
    if current >= target_cells {
        return;
    }
    let budget = ((n as f64 * CONTINENTALIZATION_MAX_PER_TICK).ceil() as usize)
        .min(target_cells - current);

    // Highest-standing oceanic crust first (descending elevation, HexId tie).
    let mut candidates: Vec<u32> = (0..n as u32)
        .filter(|&i| {
            !data.continental_crust[i as usize]
                && data.elevation_mean[i as usize] >= CONTINENTALIZATION_MIN_ELEVATION_M
        })
        .collect();
    candidates.sort_by(|&a, &b| {
        data.elevation_mean[b as usize]
            .total_cmp(&data.elevation_mean[a as usize])
            .then_with(|| a.cmp(&b))
    });

    let mut converted = 0usize;
    for &cell in &candidates {
        if converted >= budget {
            break;
        }
        let hex = HexId(cell);
        let mut did = false;
        modify_surface_at_world_hex(registry, data, cache, hex, tick_year, |feature| {
            if feature.continental_crust
                || feature.elevation_m < CONTINENTALIZATION_MIN_ELEVATION_M
            {
                return;
            }
            feature.continental_crust = true;
            // Young continentalized basement, graded by standing height
            // (Doc 06 §5.2 roots / §5.11).
            feature.root_m = feature
                .root_m
                .max(continentalized_root_m(feature.elevation_m));
            if feature.bedrock == BedrockType::OceanicCrust {
                feature.bedrock = BedrockType::Igneous;
            }
            did = true;
        });
        if did {
            // Keep the world view in sync for this tick's later steps
            // (erosion freeboard reads it); the next rebuild re-derives it.
            data.continental_crust[cell as usize] = true;
            converted += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use genesis_core::parameters::WorldParameters;
    use genesis_core::time::WorldYear;
    use genesis_core::{PlateId, create_world};

    use super::*;
    use crate::history::run_formation;
    use crate::plate::{PlateType, TectonicsState};
    use crate::plate_surface::surface_elevation_at;
    use crate::validation::{VALIDATION_TARGET_YEAR_QUICK, run_validation_world};

    fn formed_world() -> (genesis_core::World, TectonicsState) {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        // Pin a single supercontinent so these accretion-mechanics tests run on a
        // clean substrate with no trapped inter-continental basins (which the
        // default seed-driven multi-continent layout can produce).
        params.core.geology.continent_cluster_count = 1;
        let mut world = create_world(params).expect("world");
        let mut state = TectonicsState::new();
        run_formation(&mut world, &mut state);
        (world, state)
    }

    /// A hex deep inside a continental plate's land area (far from any coast).
    fn interior_land_hex(world: &genesis_core::World, state: &TectonicsState) -> HexId {
        let data = &world.data;
        let sea = data.sea_level_m;
        for i in 0..data.cell_count() as usize {
            if data.elevation_mean[i] <= sea + 100.0 {
                continue;
            }
            let plate_id = data.plate_id[i];
            let Some(plate) = state.registry.get(plate_id) else {
                continue;
            };
            if plate.plate_type != PlateType::Continental {
                continue;
            }
            let hex = HexId(i as u32);
            let all_neighbors_land = data
                .grid
                .neighbors(hex)
                .iter()
                .all(|nb| data.elevation_mean[nb.0 as usize] > sea);
            if all_neighbors_land {
                return hex;
            }
        }
        panic!("expected an interior continental land hex after formation");
    }

    fn set_pocket(
        world: &mut genesis_core::World,
        state: &mut TectonicsState,
        hex: HexId,
        depth_m: f32,
        oceanic: bool,
    ) {
        let data = &world.data;
        let plate_id = data.plate_id[hex.0 as usize];
        let plate = state.registry.get(plate_id).expect("plate");
        let birth = crate::frames::current_world_to_birth_hex(&data.grid, hex, plate);
        let mut feature =
            plate.surface.get(birth).cloned().unwrap_or_else(|| {
                crate::plate_surface::baseline_feature(PlateType::Continental, 0)
            });
        feature.elevation_m = depth_m;
        feature.bedrock = if oceanic {
            BedrockType::OceanicCrust
        } else {
            BedrockType::Igneous
        };
        feature.continental_crust = !oceanic;
        let plate = state
            .registry
            .plates_mut()
            .get_mut(&plate_id)
            .expect("plate");
        plate.surface.set(birth, feature);
        crate::world_rebuild::rebuild_world_from_plate_surfaces(&mut world.data, &state.registry);
    }

    fn set_oceanic_pocket(
        world: &mut genesis_core::World,
        state: &mut TectonicsState,
        hex: HexId,
        depth_m: f32,
    ) {
        set_pocket(world, state, hex, depth_m, true);
    }

    /// A BoundaryInfo with one convergent edge from `hex` to `neighbor` at the
    /// given closing velocity (m/yr), for live/stalled guard tests.
    fn synthetic_convergent_boundary(
        hex: HexId,
        neighbor: HexId,
        other_plate: genesis_core::PlateId,
        normal_velocity_m_per_year: f64,
    ) -> BoundaryInfo {
        let mut boundaries = BoundaryInfo::default();
        boundaries.edges.insert(
            hex,
            vec![crate::boundary::ClassifiedEdge {
                neighbor_hex: neighbor,
                other_plate,
                class: BoundaryClass::Convergent(
                    crate::boundary::ConvergentSubtype::OceanicOceanic,
                ),
                normal_velocity_m_per_year,
                tangential_velocity_m_per_year: 0.0,
            }],
        );
        boundaries
    }

    /// A BoundaryInfo with one divergent edge from `hex` to `neighbor` at the
    /// given (negative = opening) velocity (m/yr), for live/stalled rift
    /// guard tests.
    fn synthetic_divergent_boundary(
        hex: HexId,
        neighbor: HexId,
        other_plate: genesis_core::PlateId,
        normal_velocity_m_per_year: f64,
    ) -> BoundaryInfo {
        let mut boundaries = BoundaryInfo::default();
        boundaries.edges.insert(
            hex,
            vec![crate::boundary::ClassifiedEdge {
                neighbor_hex: neighbor,
                other_plate,
                class: BoundaryClass::Divergent,
                normal_velocity_m_per_year,
                tangential_velocity_m_per_year: 0.0,
            }],
        );
        boundaries
    }

    #[test]
    fn trapped_oceanic_pocket_is_accreted_and_lifted() {
        let (mut world, mut state) = formed_world();
        let hex = interior_land_hex(&world, &state);
        let sea = world.data.sea_level_m;
        set_oceanic_pocket(&mut world, &mut state, hex, -4000.0);

        let accreted = accrete_trapped_oceanic_crust(
            &world.data,
            &mut state.registry,
            &state.projection,
            &label_water_components(&world.data),
            &state.boundaries,
            &world.rng,
            world.data.current_year.value(),
            1_000_000_000.0,
        );
        assert_eq!(accreted, 1, "the lone trapped pocket should accrete");

        assert!(
            continental_crust_at(&world.data, &state.registry, &state.projection, hex),
            "accreted hex must become continental crust"
        );
        let elev = surface_elevation_at(&world.data, &state.registry, &state.projection, hex)
            .expect("feature");
        assert!(
            (elev - (sea - OBDUCTION_DEPTH_M)).abs() < 1e-3,
            "accreted hex lifted to obduction depth, got {elev}"
        );
    }

    #[test]
    fn open_ocean_floor_is_not_accreted() {
        let (world, mut state) = formed_world();
        // Any below-sea hex of the world ocean: pick the first below-sea hex.
        let sea = world.data.sea_level_m;
        let idx = (0..world.data.cell_count() as usize)
            .find(|&i| world.data.elevation_mean[i] < sea)
            .expect("ocean hexes after formation");
        let hex = HexId(idx as u32);
        let was_continental =
            continental_crust_at(&world.data, &state.registry, &state.projection, hex);

        accrete_trapped_oceanic_crust(
            &world.data,
            &mut state.registry,
            &state.projection,
            &label_water_components(&world.data),
            &state.boundaries,
            &world.rng,
            world.data.current_year.value(),
            1_000_000_000.0,
        );

        assert_eq!(
            was_continental,
            continental_crust_at(&world.data, &state.registry, &state.projection, hex),
            "open-ocean crust must be untouched"
        );
    }

    #[test]
    fn accretion_is_deterministic() {
        let (mut world_a, mut state_a) = formed_world();
        let hex = interior_land_hex(&world_a, &state_a);
        set_oceanic_pocket(&mut world_a, &mut state_a, hex, -8500.0);
        let (mut world_b, mut state_b) = formed_world();
        set_oceanic_pocket(&mut world_b, &mut state_b, hex, -8500.0);

        let a = accrete_trapped_oceanic_crust(
            &world_a.data,
            &mut state_a.registry,
            &state_a.projection,
            &label_water_components(&world_a.data),
            &state_a.boundaries,
            &world_a.rng,
            world_a.data.current_year.value(),
            1_000_000_000.0,
        );
        let b = accrete_trapped_oceanic_crust(
            &world_b.data,
            &mut state_b.registry,
            &state_b.projection,
            &label_water_components(&world_b.data),
            &state_b.boundaries,
            &world_b.rng,
            world_b.data.current_year.value(),
            1_000_000_000.0,
        );
        assert_eq!(a, b);
        let _ = PlateId::NONE; // keep import used if assertions change
    }

    /// Regression guard (§11 #4 companion): after the per-tick accretion pass,
    /// every basin the pass considers convertible has been converted. A second
    /// pass run right after the history must find (almost) nothing to do —
    /// before this pass existed, ~150 detached sub-sea oceanic components
    /// (inland seas) dotted the continents by 1B years. Basins touching a live
    /// convergent or divergent margin are deliberately skipped by both passes
    /// (current geology, §5.8), so they never appear in this count. The
    /// tolerance covers hexes that dip below sea in sub-steps after accretion
    /// within the final tick.
    #[test]
    fn history_leaves_no_trapped_oceanic_basins() {
        let (world, mut state) =
            run_validation_world(WorldYear(VALIDATION_TARGET_YEAR_QUICK)).expect("validation run");
        let accreted = accrete_trapped_oceanic_crust(
            &world.data,
            &mut state.registry,
            &state.projection,
            &label_water_components(&world.data),
            &state.boundaries,
            &world.rng,
            world.data.current_year.value(),
            1_000_000_000.0,
        );
        assert!(
            accreted <= 8,
            "trapped oceanic basins remain after history: a follow-up pass accreted {accreted} hexes"
        );
    }

    #[test]
    fn fossil_continental_trench_floor_is_healed() {
        let (mut world, mut state) = formed_world();
        let hex = interior_land_hex(&world, &state);
        let sea = world.data.sea_level_m;
        set_pocket(&mut world, &mut state, hex, -8000.0, false);

        let accreted = accrete_trapped_oceanic_crust(
            &world.data,
            &mut state.registry,
            &state.projection,
            &label_water_components(&world.data),
            &state.boundaries,
            &world.rng,
            world.data.current_year.value(),
            1_000_000_000.0,
        );
        assert_eq!(accreted, 1, "the lone deep continental pocket should heal");

        let elev = surface_elevation_at(&world.data, &state.registry, &state.projection, hex)
            .expect("feature");
        assert!(
            (elev - (sea - OBDUCTION_DEPTH_M)).abs() < 1e-3,
            "fossil trench floor lifted to obduction depth, got {elev}"
        );
        assert!(
            continental_crust_at(&world.data, &state.registry, &state.projection, hex),
            "healed hex stays continental crust"
        );
    }

    #[test]
    fn live_convergent_edge_protects_component() {
        let (mut world, mut state) = formed_world();
        let hex = interior_land_hex(&world, &state);
        let sea = world.data.sea_level_m;
        // Above the marginal-sea cap: protection means fully untouched.
        set_oceanic_pocket(&mut world, &mut state, hex, -4000.0);
        let neighbor = world.data.grid.neighbors(hex)[0];
        let other_plate = world.data.plate_id[neighbor.0 as usize];
        let boundaries = synthetic_convergent_boundary(hex, neighbor, other_plate, 0.05);

        let accreted = accrete_trapped_oceanic_crust(
            &world.data,
            &mut state.registry,
            &state.projection,
            &label_water_components(&world.data),
            &boundaries,
            &world.rng,
            world.data.current_year.value(),
            1_000_000_000.0,
        );
        assert_eq!(accreted, 0, "a live trench must protect its basin");

        let elev = surface_elevation_at(&world.data, &state.registry, &state.projection, hex)
            .expect("feature");
        assert!(
            (elev - -4000.0).abs() < 1e-3,
            "protected pocket keeps its depth, got {elev} (sea {sea})"
        );
    }

    #[test]
    fn live_marginal_basin_depth_is_capped() {
        let (mut world, mut state) = formed_world();
        let hex = interior_land_hex(&world, &state);
        set_oceanic_pocket(&mut world, &mut state, hex, -8000.0);
        let neighbor = world.data.grid.neighbors(hex)[0];
        let other_plate = world.data.plate_id[neighbor.0 as usize];
        let boundaries = synthetic_convergent_boundary(hex, neighbor, other_plate, 0.05);

        let accreted = accrete_trapped_oceanic_crust(
            &world.data,
            &mut state.registry,
            &state.projection,
            &label_water_components(&world.data),
            &boundaries,
            &world.rng,
            world.data.current_year.value(),
            1_000_000_000.0,
        );
        assert_eq!(
            accreted, 0,
            "a live margin still protects its basin from obduction"
        );

        let elev = surface_elevation_at(&world.data, &state.registry, &state.projection, hex)
            .expect("feature");
        assert!(
            (elev - MARGINAL_SEA_EQUILIBRIUM_M).abs() < 1e-3,
            "sealed basin floor capped at marginal-sea depth, got {elev}"
        );
        assert!(
            !continental_crust_at(&world.data, &state.registry, &state.projection, hex),
            "capped basin keeps its oceanic crust (no obduction)"
        );
    }

    #[test]
    fn stalled_convergent_edge_does_not_protect() {
        let (mut world, mut state) = formed_world();
        let hex = interior_land_hex(&world, &state);
        let sea = world.data.sea_level_m;
        set_oceanic_pocket(&mut world, &mut state, hex, -8000.0);
        let neighbor = world.data.grid.neighbors(hex)[0];
        let other_plate = world.data.plate_id[neighbor.0 as usize];
        // Below CONVERGENCE_THRESHOLD_M_PER_YEAR (0.005): a stalled boundary.
        let boundaries = synthetic_convergent_boundary(hex, neighbor, other_plate, 0.0001);

        let accreted = accrete_trapped_oceanic_crust(
            &world.data,
            &mut state.registry,
            &state.projection,
            &label_water_components(&world.data),
            &boundaries,
            &world.rng,
            world.data.current_year.value(),
            1_000_000_000.0,
        );
        assert_eq!(accreted, 1, "a stalled boundary must not protect its pit");

        let elev = surface_elevation_at(&world.data, &state.registry, &state.projection, hex)
            .expect("feature");
        assert!(
            (elev - (sea - OBDUCTION_DEPTH_M)).abs() < 1e-3,
            "fossil pit healed to obduction depth, got {elev}"
        );
    }

    #[test]
    fn live_divergent_edge_protects_component() {
        let (mut world, mut state) = formed_world();
        let hex = interior_land_hex(&world, &state);
        set_oceanic_pocket(&mut world, &mut state, hex, -4000.0);
        let neighbor = world.data.grid.neighbors(hex)[0];
        let other_plate = world.data.plate_id[neighbor.0 as usize];
        // Opening faster than CONVERGENCE_THRESHOLD_M_PER_YEAR (0.005): a
        // live rift — the basin is a growing ocean, not a trapped one.
        let boundaries = synthetic_divergent_boundary(hex, neighbor, other_plate, -0.05);

        let accreted = accrete_trapped_oceanic_crust(
            &world.data,
            &mut state.registry,
            &state.projection,
            &label_water_components(&world.data),
            &boundaries,
            &world.rng,
            world.data.current_year.value(),
            1_000_000_000.0,
        );
        assert_eq!(accreted, 0, "a live rift must protect its infant ocean");

        let elev = surface_elevation_at(&world.data, &state.registry, &state.projection, hex)
            .expect("feature");
        assert!(
            (elev - -4000.0).abs() < 1e-3,
            "protected rift basin keeps its depth, got {elev}"
        );
        assert!(
            !continental_crust_at(&world.data, &state.registry, &state.projection, hex),
            "protected rift basin keeps its oceanic crust"
        );
    }

    #[test]
    fn stalled_divergent_edge_does_not_protect() {
        let (mut world, mut state) = formed_world();
        let hex = interior_land_hex(&world, &state);
        let sea = world.data.sea_level_m;
        set_oceanic_pocket(&mut world, &mut state, hex, -8000.0);
        let neighbor = world.data.grid.neighbors(hex)[0];
        let other_plate = world.data.plate_id[neighbor.0 as usize];
        // Opening below CONVERGENCE_THRESHOLD_M_PER_YEAR (0.005): dead rift.
        let boundaries = synthetic_divergent_boundary(hex, neighbor, other_plate, -0.0001);

        let accreted = accrete_trapped_oceanic_crust(
            &world.data,
            &mut state.registry,
            &state.projection,
            &label_water_components(&world.data),
            &boundaries,
            &world.rng,
            world.data.current_year.value(),
            1_000_000_000.0,
        );
        assert_eq!(accreted, 1, "a stalled rift must not protect its basin");

        let elev = surface_elevation_at(&world.data, &state.registry, &state.projection, hex)
            .expect("feature");
        assert!(
            (elev - (sea - OBDUCTION_DEPTH_M)).abs() < 1e-3,
            "failed-rift basin healed to obduction depth, got {elev}"
        );
    }

    #[test]
    fn live_divergent_basin_depth_is_capped() {
        let (mut world, mut state) = formed_world();
        let hex = interior_land_hex(&world, &state);
        // A fossil-trench-deep floor caught beside a new rift: enclosure
        // still applies — hot young ocean floor never sits at abyssal depth.
        set_oceanic_pocket(&mut world, &mut state, hex, -8000.0);
        let neighbor = world.data.grid.neighbors(hex)[0];
        let other_plate = world.data.plate_id[neighbor.0 as usize];
        let boundaries = synthetic_divergent_boundary(hex, neighbor, other_plate, -0.05);

        let accreted = accrete_trapped_oceanic_crust(
            &world.data,
            &mut state.registry,
            &state.projection,
            &label_water_components(&world.data),
            &boundaries,
            &world.rng,
            world.data.current_year.value(),
            1_000_000_000.0,
        );
        assert_eq!(
            accreted, 0,
            "a live rift still protects its basin from obduction"
        );

        let elev = surface_elevation_at(&world.data, &state.registry, &state.projection, hex)
            .expect("feature");
        assert!(
            (elev - MARGINAL_SEA_EQUILIBRIUM_M).abs() < 1e-3,
            "rift basin floor capped at marginal-sea depth, got {elev}"
        );
        assert!(
            !continental_crust_at(&world.data, &state.registry, &state.projection, hex),
            "capped rift basin keeps its oceanic crust"
        );
    }

    #[test]
    fn suture_conversion_probability_bounds() {
        assert_eq!(suture_conversion_probability(0.0), 0.0, "zero interval");
        let p_half = suture_conversion_probability(SUTURE_HALF_LIFE_MY * 1_000_000.0);
        assert!((p_half - 0.5).abs() < 1e-12, "one half-life, got {p_half}");
        assert_eq!(
            suture_conversion_probability(1_000_000_000_000.0),
            1.0,
            "astronomical interval saturates"
        );
    }

    #[test]
    fn trapped_basin_floor_caps_before_conversion() {
        let (mut world, mut state) = formed_world();
        let hex = interior_land_hex(&world, &state);
        set_oceanic_pocket(&mut world, &mut state, hex, -8000.0);

        // Zero interval: no conversion can happen, but the §5.9 enclosure cap
        // still applies — a sealed basin never holds abyssal depth, even
        // before the suture starts grinding.
        let accreted = accrete_trapped_oceanic_crust(
            &world.data,
            &mut state.registry,
            &state.projection,
            &label_water_components(&world.data),
            &state.boundaries,
            &world.rng,
            world.data.current_year.value(),
            0.0,
        );
        assert_eq!(accreted, 0, "zero interval converts nothing");

        let elev = surface_elevation_at(&world.data, &state.registry, &state.projection, hex)
            .expect("feature");
        assert!(
            (elev - MARGINAL_SEA_EQUILIBRIUM_M).abs() < 1e-3,
            "unconverted basin floor capped at marginal-sea depth, got {elev}"
        );
        assert!(
            !continental_crust_at(&world.data, &state.registry, &state.projection, hex),
            "unconverted basin keeps its oceanic crust"
        );
    }

    /// A BoundaryInfo with one convergent continent–ocean edge from `hex` to
    /// `neighbor` at the given closing velocity (m/yr).
    fn synthetic_co_boundary(
        hex: HexId,
        neighbor: HexId,
        other_plate: genesis_core::PlateId,
        normal_velocity_m_per_year: f64,
    ) -> BoundaryInfo {
        let mut boundaries = BoundaryInfo::default();
        boundaries.edges.insert(
            hex,
            vec![crate::boundary::ClassifiedEdge {
                neighbor_hex: neighbor,
                other_plate,
                class: BoundaryClass::Convergent(
                    crate::boundary::ConvergentSubtype::ContinentalOceanic,
                ),
                normal_velocity_m_per_year,
                tangential_velocity_m_per_year: 0.0,
            }],
        );
        boundaries
    }

    #[test]
    fn subduction_erosion_consumes_forearc_at_live_margin() {
        let (mut world, mut state) = formed_world();
        let hex = interior_land_hex(&world, &state);
        let sea = world.data.sea_level_m;
        set_pocket(&mut world, &mut state, hex, 300.0, false);
        let neighbor = world.data.grid.neighbors(hex)[0];
        let other_plate = world.data.plate_id[neighbor.0 as usize];
        let boundaries = synthetic_co_boundary(hex, neighbor, other_plate, 0.05);

        // A huge interval compounds the per-My probability to ~1.
        let eroded = apply_subduction_erosion(
            &world.data,
            &mut state.registry,
            &state.projection,
            &boundaries,
            &world.rng,
            world.data.current_year.value(),
            1_000_000_000.0,
        );
        assert_eq!(eroded, 1, "a live CO margin consumes its forearc rim");
        assert!(
            !continental_crust_at(&world.data, &state.registry, &state.projection, hex),
            "consumed forearc becomes oceanic crust"
        );
        let elev = surface_elevation_at(&world.data, &state.registry, &state.projection, hex)
            .expect("feature");
        assert!(
            (elev - (sea - SUBDUCTION_EROSION_DOWNSTEP_M)).abs() < 1e-3,
            "consumed forearc downsteps below sea, got {elev}"
        );
    }

    #[test]
    fn subduction_erosion_ignores_stalled_margin() {
        let (mut world, mut state) = formed_world();
        let hex = interior_land_hex(&world, &state);
        set_pocket(&mut world, &mut state, hex, 300.0, false);
        let neighbor = world.data.grid.neighbors(hex)[0];
        let other_plate = world.data.plate_id[neighbor.0 as usize];
        // Below CONVERGENCE_THRESHOLD_M_PER_YEAR (0.005): a stalled margin.
        let boundaries = synthetic_co_boundary(hex, neighbor, other_plate, 0.0001);

        let eroded = apply_subduction_erosion(
            &world.data,
            &mut state.registry,
            &state.projection,
            &boundaries,
            &world.rng,
            world.data.current_year.value(),
            1_000_000_000.0,
        );
        assert_eq!(eroded, 0, "a stalled margin consumes nothing");
        assert!(
            continental_crust_at(&world.data, &state.registry, &state.projection, hex),
            "stalled margin keeps its forearc"
        );
    }

    #[test]
    fn subduction_erosion_skips_oceanic_side() {
        let (mut world, mut state) = formed_world();
        let hex = interior_land_hex(&world, &state);
        set_oceanic_pocket(&mut world, &mut state, hex, -4000.0);
        let neighbor = world.data.grid.neighbors(hex)[0];
        let other_plate = world.data.plate_id[neighbor.0 as usize];
        let boundaries = synthetic_co_boundary(hex, neighbor, other_plate, 0.05);

        let eroded = apply_subduction_erosion(
            &world.data,
            &mut state.registry,
            &state.projection,
            &boundaries,
            &world.rng,
            world.data.current_year.value(),
            1_000_000_000.0,
        );
        assert_eq!(
            eroded, 0,
            "the downgoing oceanic side is not subduction-eroded"
        );
        assert!(
            !continental_crust_at(&world.data, &state.registry, &state.projection, hex),
            "oceanic side keeps its oceanic crust"
        );
    }

    #[test]
    fn subduction_erosion_zero_interval_is_noop() {
        let (mut world, mut state) = formed_world();
        let hex = interior_land_hex(&world, &state);
        set_pocket(&mut world, &mut state, hex, 300.0, false);
        let neighbor = world.data.grid.neighbors(hex)[0];
        let other_plate = world.data.plate_id[neighbor.0 as usize];
        let boundaries = synthetic_co_boundary(hex, neighbor, other_plate, 0.05);

        let eroded = apply_subduction_erosion(
            &world.data,
            &mut state.registry,
            &state.projection,
            &boundaries,
            &world.rng,
            world.data.current_year.value(),
            0.0,
        );
        assert_eq!(eroded, 0, "no elapsed time, no erosion");
        assert!(continental_crust_at(
            &world.data,
            &state.registry,
            &state.projection,
            hex
        ));
    }
}
