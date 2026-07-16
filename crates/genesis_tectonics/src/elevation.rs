//! Per-tick boundary elevation and bedrock updates (Doc 06 §5).

use std::collections::{BTreeMap, VecDeque};

use genesis_core::data::{BedrockType, WorldData};
use genesis_core::time::WorldYear;
use genesis_core::{HexId, PlateId};

use crate::boundary::{BoundaryClass, BoundaryInfo, ClassifiedEdge, ConvergentSubtype};
use crate::frames::current_world_to_birth_hex;
use crate::initial_terrain::CONTINENTAL_BASE_ELEVATION_M;
use crate::plate::{Plate, PlateRegistry, PlateType};
use crate::projection::ProjectionCache;

/// Minimum elevation (Mariana Trench depth), meters (§5.7).
pub const MIN_ELEVATION_M: f32 = -11_000.0;

/// Maximum elevation, meters (§5.7).
pub const MAX_ELEVATION_M: f32 = 9_000.0;

/// Maximum vertical relief within a hex, meters (§5.7).
pub const MAX_RELIEF_M: f32 = 5_000.0;

/// Equilibrium depth for normal oceanic seafloor (m). Divergent subsidence asymptotes here.
pub const OCEAN_FLOOR_BASELINE_M: f32 = -4000.0;

/// Equilibrium height for continental orogeny (m). Uplift slows as elevation approaches this.
pub const MOUNTAIN_EQUILIBRIUM_M: f32 = 7000.0;

/// Equilibrium depth for subduction trenches (m).
pub const TRENCH_EQUILIBRIUM_M: f32 = -8500.0;

/// Divergent subsidence: m per (cm/year × year) (§5.1).
/// Calibrated: 5 cm/yr × 500K years × 2e-5 = 50 m per tick.
/// Produces ~3 km of seafloor deepening over 100M years of sustained divergence.
pub const SUBSIDENCE_RATE: f64 = 2e-5;

/// Continental–continental orogeny rate (§5.2).
/// Calibrated: 5 cm/yr × 500K years × 5e-5 = 125 m per tick.
/// Produces ~5 km mountain over 100M years of sustained collision (Himalayan scale).
pub const OROGENY_RATE: f64 = 5e-5;

/// Subduction trench deepening rate (§5.3–§5.4).
/// Calibrated: 5 cm/yr × 500K years × 1e-4 = 250 m per tick.
/// Produces ~10 km trench over 100M years of sustained subduction (Mariana scale).
pub const SUBDUCTION_RATE: f64 = 1e-4;

/// Continental rifting subsidence multiplier (§5.1 heuristic).
pub const CONTINENTAL_RIFT_SUBSIDENCE_FACTOR: f64 = 0.3;

/// Inland orogeny spread depth for continental–continental (§5.2).
/// Two rings: persistent material-footprint boundaries uplift the same hexes
/// every tick, so wider spreads turn whole plate margins into plateaus.
pub const CC_INLAND_HEXES: u32 = 2;

/// Inland uplift spread for oceanic–continental (§5.3).
pub const OC_INLAND_HEXES: u32 = 2;

/// Coastal shelf spread depth on the oceanic side of oceanic–continental boundaries (§5.3).
pub const COASTAL_SHELF_HEXES: u32 = 2;

/// Falloff for coastal shelf depth (fraction of trench delta per hex ring).
const COASTAL_SHELF_FALLOFF: [f64; 2] = [0.4, 0.15];

/// Coastal uplift fraction of orogeny delta on continental boundary hex.
/// Calibrated for persistent boundaries: equilibrium against erosion sits at
/// coastal-range height, not plateau height.
const OC_COASTAL_UPLIFT_FACTOR: f64 = 0.25;

/// Island-arc uplift fraction of subduction delta on overriding oceanic hex.
/// Calibrated against Igneous erosion (bedrock multiplier 0.10): equilibrium
/// arc height ≈ uplift / (erosion_rate × multiplier) ≈ 1000 m — island chains
/// near sea level, not continuous 6000 m volcanic walls along every trench.
const OO_ARC_UPLIFT_FACTOR: f64 = 0.02;

const INLAND_FALLOFF: [f64; 3] = [1.0, 0.67, 0.33];

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
struct SurfaceKey {
    plate_id: PlateId,
    birth_hex: HexId,
}

#[derive(Default)]
struct HexDeltas {
    elev: f64,
    relief: f64,
    bedrock: Option<BedrockType>,
    age_year: i64,
    /// World elevation when this delta was collected (for baseline feature creation).
    base_elev_m: f32,
}

/// Applies boundary-driven elevation and bedrock changes to plate surfaces.
pub fn apply_boundary_elevation(
    data: &WorldData,
    registry: &mut PlateRegistry,
    cache: &ProjectionCache,
    boundaries: &BoundaryInfo,
    tick_interval_years: f64,
    tick_year: WorldYear,
) {
    let mut deltas: BTreeMap<SurfaceKey, HexDeltas> = BTreeMap::new();

    for &hex in &boundaries.boundary_hexes {
        let owner_plate_id = data.plate_id[hex.0 as usize];
        let Some(owner_plate) = registry.get(owner_plate_id) else {
            continue;
        };
        let edges = match boundaries.edges.get(&hex) {
            Some(e) => e,
            None => continue,
        };

        for edge in edges {
            let Some(other_plate) = registry.get(edge.other_plate) else {
                continue;
            };
            apply_edge(
                data,
                registry,
                cache,
                &mut deltas,
                hex,
                edge,
                owner_plate_id,
                owner_plate,
                other_plate,
                tick_interval_years,
                tick_year,
            );
        }
    }

    apply_surface_deltas(registry, &deltas);
}

/// Clamps elevation and relief to physically plausible ranges (§5.7).
pub fn clamp_terrain(data: &mut WorldData) {
    for i in 0..data.elevation_mean.len() {
        data.elevation_mean[i] = data.elevation_mean[i].clamp(MIN_ELEVATION_M, MAX_ELEVATION_M);
        data.elevation_relief[i] = data.elevation_relief[i].clamp(0.0, MAX_RELIEF_M);
    }
}

/// Oceanic–oceanic: faster plate subducts; tie → lower `PlateId`.
pub fn subducting_plate_id(
    owner_plate_id: PlateId,
    other_plate_id: PlateId,
    owner_plate: &Plate,
    other_plate: &Plate,
) -> PlateId {
    let owner_rate = owner_plate.motion_rate_rad_per_year;
    let other_rate = other_plate.motion_rate_rad_per_year;
    if owner_rate > other_rate {
        owner_plate_id
    } else if other_rate > owner_rate {
        other_plate_id
    } else if owner_plate_id < other_plate_id {
        owner_plate_id
    } else {
        other_plate_id
    }
}

fn velocity_cm_per_year(edge: &ClassifiedEdge) -> f64 {
    edge.normal_velocity_m_per_year.abs() * 100.0
}

fn surface_key(
    data: &WorldData,
    registry: &PlateRegistry,
    cache: &ProjectionCache,
    world_hex: HexId,
    plate_id: PlateId,
) -> SurfaceKey {
    let plate = registry
        .get(plate_id)
        .expect("plate exists for surface key");
    // The cache maps a hex to its OWNER's birth frame; deltas that target a
    // neighboring plate's surface must fall back to the direct inversion.
    let idx = world_hex.0 as usize;
    let cached = if data.plate_id.get(idx) == Some(&plate_id) {
        cache.birth_hex_for(data, world_hex)
    } else {
        None
    };
    SurfaceKey {
        plate_id,
        birth_hex: cached
            .unwrap_or_else(|| current_world_to_birth_hex(&data.grid, world_hex, plate)),
    }
}

fn delta_entry<'a>(
    data: &WorldData,
    registry: &PlateRegistry,
    cache: &ProjectionCache,
    deltas: &'a mut BTreeMap<SurfaceKey, HexDeltas>,
    world_hex: HexId,
    plate_id: PlateId,
) -> &'a mut HexDeltas {
    let key = surface_key(data, registry, cache, world_hex, plate_id);
    let entry = deltas.entry(key).or_default();
    let idx = world_hex.0 as usize;
    if idx < data.elevation_mean.len() {
        entry.base_elev_m = data.elevation_mean[idx];
    }
    entry
}

fn tag_boundary_age(entry: &mut HexDeltas, tick_year: WorldYear) {
    entry.age_year = tick_year.value();
}

/// Elevation above which every uplift source tapers off (m).
///
/// With material plate footprints, convergent boundaries persist at the same
/// hexes for hundreds of ticks; stacked per-edge orogeny plus volcanism would
/// otherwise drive them into the §5.7 clamp. Isostasy: crust roots deepen and
/// uplift stalls as mountains approach the support limit.
pub const UPLIFT_TAPER_START_M: f32 = 6000.0;

/// Scales a positive uplift delta by remaining headroom: 1.0 at or below
/// [`UPLIFT_TAPER_START_M`], linearly down to 0.0 at [`MAX_ELEVATION_M`].
pub fn uplift_headroom_factor(elevation_m: f32) -> f32 {
    if elevation_m <= UPLIFT_TAPER_START_M {
        return 1.0;
    }
    ((MAX_ELEVATION_M - elevation_m) / (MAX_ELEVATION_M - UPLIFT_TAPER_START_M)).clamp(0.0, 1.0)
}

/// Depth below which every subsidence source tapers off (m). Multiple trench
/// edges can stack their deltas on one hex; the taper keeps depths asymptotic
/// to the §5.7 floor instead of slamming into the clamp.
pub const SUBSIDENCE_TAPER_START_M: f32 = -9_500.0;

/// Hard floor for boundary-driven subsidence (m): slightly past the trench
/// equilibrium (−8500), well above the §5.7 clamp (−11000).
pub const BOUNDARY_SUBSIDENCE_FLOOR_M: f32 = -9_000.0;

/// Scales a negative elevation delta by remaining depth headroom: 1.0 at or
/// above [`SUBSIDENCE_TAPER_START_M`], linearly down to 0.0 at
/// [`MIN_ELEVATION_M`].
pub fn subsidence_headroom_factor(elevation_m: f32) -> f32 {
    if elevation_m >= SUBSIDENCE_TAPER_START_M {
        return 1.0;
    }
    ((elevation_m - MIN_ELEVATION_M) / (SUBSIDENCE_TAPER_START_M - MIN_ELEVATION_M)).clamp(0.0, 1.0)
}

fn asymptotic_fraction(distance_m: f32, scale_height_m: f32) -> f64 {
    if distance_m <= 0.0 || scale_height_m <= 0.0 {
        return 0.0;
    }
    ((distance_m / scale_height_m).min(1.0)) as f64
}

#[allow(clippy::too_many_arguments)]
fn apply_edge(
    data: &WorldData,
    registry: &PlateRegistry,
    cache: &ProjectionCache,
    deltas: &mut BTreeMap<SurfaceKey, HexDeltas>,
    owner_hex: HexId,
    edge: &ClassifiedEdge,
    owner_plate_id: PlateId,
    owner_plate: &Plate,
    other_plate: &Plate,
    tick_interval_years: f64,
    tick_year: WorldYear,
) {
    let v_cm = velocity_cm_per_year(edge);
    let idx = owner_hex.0 as usize;
    let current_elev = if idx < data.elevation_mean.len() {
        data.elevation_mean[idx]
    } else {
        0.0
    };

    match edge.class {
        BoundaryClass::Divergent => {
            apply_divergent(
                data,
                registry,
                cache,
                deltas,
                owner_hex,
                owner_plate_id,
                owner_plate,
                current_elev,
                v_cm,
                tick_interval_years,
                tick_year,
            );
        }
        BoundaryClass::Convergent(ConvergentSubtype::ContinentalContinental) => {
            apply_continental_continental(
                data,
                registry,
                cache,
                deltas,
                owner_hex,
                owner_plate_id,
                owner_plate,
                current_elev,
                v_cm,
                tick_interval_years,
                tick_year,
            );
        }
        BoundaryClass::Convergent(ConvergentSubtype::ContinentalOceanic) => {
            apply_continental_oceanic(
                data,
                registry,
                cache,
                deltas,
                owner_hex,
                owner_plate_id,
                owner_plate,
                edge.other_plate,
                current_elev,
                v_cm,
                tick_interval_years,
                tick_year,
            );
        }
        BoundaryClass::Convergent(ConvergentSubtype::OceanicOceanic) => {
            apply_oceanic_oceanic(
                data,
                registry,
                cache,
                deltas,
                owner_hex,
                owner_plate_id,
                edge.other_plate,
                owner_plate,
                other_plate,
                current_elev,
                v_cm,
                tick_interval_years,
                tick_year,
            );
        }
        BoundaryClass::Transform => {
            let entry = delta_entry(data, registry, cache, deltas, owner_hex, owner_plate_id);
            entry.bedrock = Some(BedrockType::Metamorphic);
            tag_boundary_age(entry, tick_year);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_divergent(
    data: &WorldData,
    registry: &PlateRegistry,
    cache: &ProjectionCache,
    deltas: &mut BTreeMap<SurfaceKey, HexDeltas>,
    hex: HexId,
    owner_plate_id: PlateId,
    owner_plate: &Plate,
    current_elev: f32,
    velocity_cm_per_year: f64,
    tick_interval_years: f64,
    tick_year: WorldYear,
) {
    let is_continental = owner_plate.plate_type == PlateType::Continental;
    let (baseline, bedrock, subsidence_scale) = if is_continental {
        (
            CONTINENTAL_BASE_ELEVATION_M,
            BedrockType::Igneous,
            CONTINENTAL_RIFT_SUBSIDENCE_FACTOR,
        )
    } else {
        (OCEAN_FLOOR_BASELINE_M, BedrockType::OceanicCrust, 1.0)
    };

    let distance_above_baseline = current_elev - baseline;
    let entry = delta_entry(data, registry, cache, deltas, hex, owner_plate_id);
    entry.bedrock = Some(bedrock);
    tag_boundary_age(entry, tick_year);

    if distance_above_baseline <= 0.0 {
        return;
    }

    let scale_height_m = if is_continental { 2000.0 } else { 4000.0 };
    let driving_fraction = asymptotic_fraction(distance_above_baseline, scale_height_m);
    let max_delta = velocity_cm_per_year * tick_interval_years * SUBSIDENCE_RATE * subsidence_scale;
    let delta = max_delta * driving_fraction;
    entry.elev -= delta;
}

#[allow(clippy::too_many_arguments)]
fn apply_continental_continental(
    data: &WorldData,
    registry: &PlateRegistry,
    cache: &ProjectionCache,
    deltas: &mut BTreeMap<SurfaceKey, HexDeltas>,
    owner_hex: HexId,
    owner_plate_id: PlateId,
    owner_plate: &Plate,
    current_elev: f32,
    velocity_cm_per_year: f64,
    tick_interval_years: f64,
    tick_year: WorldYear,
) {
    if owner_plate.plate_type != PlateType::Continental {
        return;
    }

    let entry = delta_entry(data, registry, cache, deltas, owner_hex, owner_plate_id);
    entry.bedrock = Some(BedrockType::Metamorphic);
    tag_boundary_age(entry, tick_year);

    let distance_below_equilibrium = MOUNTAIN_EQUILIBRIUM_M - current_elev;
    if distance_below_equilibrium <= 0.0 {
        return;
    }

    let scale_height_m = 7000.0;
    let driving_fraction = asymptotic_fraction(distance_below_equilibrium, scale_height_m);
    let max_orogeny = velocity_cm_per_year * tick_interval_years * OROGENY_RATE;
    let orogeny = max_orogeny * driving_fraction;
    let relief = orogeny * 0.3;

    entry.elev += orogeny;
    entry.relief += relief;

    spread_inland(
        data,
        registry,
        cache,
        deltas,
        owner_hex,
        owner_plate_id,
        CC_INLAND_HEXES,
        tick_year,
        |d, falloff| {
            d.elev += orogeny * falloff;
            d.relief += relief * falloff;
        },
    );
}

#[allow(clippy::too_many_arguments)]
fn apply_continental_oceanic(
    data: &WorldData,
    registry: &PlateRegistry,
    cache: &ProjectionCache,
    deltas: &mut BTreeMap<SurfaceKey, HexDeltas>,
    owner_hex: HexId,
    owner_plate_id: PlateId,
    _owner_plate: &Plate,
    other_plate_id: PlateId,
    current_elev: f32,
    velocity_cm_per_year: f64,
    tick_interval_years: f64,
    tick_year: WorldYear,
) {
    // Which side subducts depends on the crust at THIS hex, not the owning
    // plate's type: plates carry mixed crust, and only oceanic lithosphere
    // sinks into a trench.
    match crate::boundary::hex_crust_is_oceanic(data, registry, cache, owner_hex) {
        true => {
            let entry = delta_entry(data, registry, cache, deltas, owner_hex, owner_plate_id);
            entry.bedrock = Some(BedrockType::OceanicCrust);
            tag_boundary_age(entry, tick_year);

            let distance_above_equilibrium = current_elev - TRENCH_EQUILIBRIUM_M;
            if distance_above_equilibrium <= 0.0 {
                return;
            }

            let scale_height_m = 4500.0;
            let driving_fraction = asymptotic_fraction(distance_above_equilibrium, scale_height_m);
            let max_trench = velocity_cm_per_year * tick_interval_years * SUBDUCTION_RATE;
            let trench = max_trench * driving_fraction;
            entry.elev -= trench;
        }
        false => {
            let entry = delta_entry(data, registry, cache, deltas, owner_hex, owner_plate_id);
            entry.bedrock = Some(BedrockType::Igneous);
            tag_boundary_age(entry, tick_year);

            let distance_below_equilibrium = MOUNTAIN_EQUILIBRIUM_M - current_elev;
            if distance_below_equilibrium <= 0.0 {
            } else {
                let scale_height_m = 7000.0;
                let driving_fraction =
                    asymptotic_fraction(distance_below_equilibrium, scale_height_m);
                let max_uplift = velocity_cm_per_year
                    * tick_interval_years
                    * OROGENY_RATE
                    * OC_COASTAL_UPLIFT_FACTOR;
                let uplift = max_uplift * driving_fraction;
                entry.elev += uplift;

                spread_inland(
                    data,
                    registry,
                    cache,
                    deltas,
                    owner_hex,
                    owner_plate_id,
                    OC_INLAND_HEXES,
                    tick_year,
                    |d, falloff| {
                        d.elev += uplift * falloff;
                    },
                );
            }

            spread_coastal_shelf(
                data,
                registry,
                cache,
                deltas,
                owner_hex,
                other_plate_id,
                COASTAL_SHELF_HEXES,
                velocity_cm_per_year,
                tick_interval_years,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_oceanic_oceanic(
    data: &WorldData,
    registry: &PlateRegistry,
    cache: &ProjectionCache,
    deltas: &mut BTreeMap<SurfaceKey, HexDeltas>,
    owner_hex: HexId,
    owner_plate_id: PlateId,
    other_plate_id: PlateId,
    owner_plate: &Plate,
    other_plate: &Plate,
    current_elev: f32,
    velocity_cm_per_year: f64,
    tick_interval_years: f64,
    tick_year: WorldYear,
) {
    let subducting = subducting_plate_id(owner_plate_id, other_plate_id, owner_plate, other_plate);
    let max_trench = velocity_cm_per_year * tick_interval_years * SUBDUCTION_RATE;

    if owner_plate_id == subducting {
        let entry = delta_entry(data, registry, cache, deltas, owner_hex, owner_plate_id);
        entry.bedrock = Some(BedrockType::OceanicCrust);
        tag_boundary_age(entry, tick_year);

        let distance_above_equilibrium = current_elev - TRENCH_EQUILIBRIUM_M;
        if distance_above_equilibrium <= 0.0 {
            return;
        }

        let scale_height_m = 4500.0;
        let driving_fraction = asymptotic_fraction(distance_above_equilibrium, scale_height_m);
        let trench = max_trench * driving_fraction;
        entry.elev -= trench;
    } else {
        let entry = delta_entry(data, registry, cache, deltas, owner_hex, owner_plate_id);
        entry.bedrock = Some(BedrockType::Igneous);
        tag_boundary_age(entry, tick_year);

        let distance_below_equilibrium = MOUNTAIN_EQUILIBRIUM_M - current_elev;
        if distance_below_equilibrium <= 0.0 {
            return;
        }

        let scale_height_m = 7000.0;
        let driving_fraction = asymptotic_fraction(distance_below_equilibrium, scale_height_m);
        let max_uplift = max_trench * OO_ARC_UPLIFT_FACTOR;
        let uplift = max_uplift * driving_fraction;
        entry.elev += uplift;
    }
}

/// Spreads gentle subsidence onto the oceanic plate's hexes adjacent to a continental
/// boundary, producing a continental shelf → deep ocean gradient instead of a cliff.
#[allow(clippy::too_many_arguments)]
fn spread_coastal_shelf(
    data: &WorldData,
    registry: &PlateRegistry,
    cache: &ProjectionCache,
    deltas: &mut BTreeMap<SurfaceKey, HexDeltas>,
    boundary_hex: HexId,
    oceanic_plate_id: PlateId,
    max_depth: u32,
    velocity_cm_per_year: f64,
    tick_interval_years: f64,
) {
    let grid = &data.grid;
    let n = data.plate_id.len();
    let trench_delta = velocity_cm_per_year * tick_interval_years * SUBDUCTION_RATE;

    let mut visited = BTreeMap::<HexId, u32>::new();
    let mut queue = VecDeque::new();

    let mut neighbors: Vec<HexId> = grid.neighbors(boundary_hex).to_vec();
    neighbors.sort_by_key(|h| h.0);
    for neighbor in neighbors {
        let idx = neighbor.0 as usize;
        if idx >= n || data.plate_id[idx] != oceanic_plate_id {
            continue;
        }
        visited.insert(neighbor, 1);
        queue.push_back(neighbor);
    }

    while let Some(current) = queue.pop_front() {
        let depth = *visited.get(&current).unwrap_or(&0);
        if depth == 0 || depth as usize > COASTAL_SHELF_FALLOFF.len() {
            continue;
        }

        let falloff = COASTAL_SHELF_FALLOFF[depth as usize - 1];
        let idx = current.0 as usize;
        let current_elev = if idx < data.elevation_mean.len() {
            data.elevation_mean[idx]
        } else {
            0.0
        };
        let distance_above = current_elev - TRENCH_EQUILIBRIUM_M;
        if distance_above <= 0.0 {
            continue;
        }
        let driving_fraction = asymptotic_fraction(distance_above, 4500.0);
        let delta = trench_delta * falloff * driving_fraction;
        let entry = delta_entry(data, registry, cache, deltas, current, oceanic_plate_id);
        entry.elev -= delta;

        if depth >= max_depth {
            continue;
        }

        let mut next_neighbors: Vec<HexId> = grid.neighbors(current).to_vec();
        next_neighbors.sort_by_key(|h| h.0);
        for next in next_neighbors {
            let idx = next.0 as usize;
            if visited.contains_key(&next) || idx >= n || data.plate_id[idx] != oceanic_plate_id {
                continue;
            }
            visited.insert(next, depth + 1);
            queue.push_back(next);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spread_inland(
    data: &WorldData,
    registry: &PlateRegistry,
    cache: &ProjectionCache,
    deltas: &mut BTreeMap<SurfaceKey, HexDeltas>,
    start: HexId,
    plate_id: PlateId,
    max_depth: u32,
    tick_year: WorldYear,
    mut apply: impl FnMut(&mut HexDeltas, f64),
) {
    let grid = &data.grid;
    let n = data.plate_id.len();
    let mut visited = BTreeMap::<HexId, u32>::new();
    visited.insert(start, 0);

    let mut queue: VecDeque<HexId> = VecDeque::new();
    queue.push_back(start);

    while let Some(current) = queue.pop_front() {
        let depth = *visited.get(&current).unwrap_or(&0);
        if depth >= max_depth {
            continue;
        }

        let mut neighbors: Vec<HexId> = grid.neighbors(current).to_vec();
        neighbors.sort_by_key(|h| h.0);

        for neighbor in neighbors {
            if visited.contains_key(&neighbor) {
                continue;
            }
            let idx = neighbor.0 as usize;
            if idx >= n || data.plate_id[idx] != plate_id {
                continue;
            }
            let next_depth = depth + 1;
            if next_depth > max_depth {
                continue;
            }
            visited.insert(neighbor, next_depth);
            queue.push_back(neighbor);

            let falloff = INLAND_FALLOFF
                .get(next_depth as usize - 1)
                .copied()
                .unwrap_or(0.0);
            if falloff > 0.0 {
                let entry = delta_entry(data, registry, cache, deltas, neighbor, plate_id);
                apply(entry, falloff);
                tag_boundary_age(entry, tick_year);
            }
        }
    }
}

fn apply_surface_deltas(registry: &mut PlateRegistry, deltas: &BTreeMap<SurfaceKey, HexDeltas>) {
    for (key, delta) in deltas {
        let Some(plate) = registry.plates_mut().get_mut(&key.plate_id) else {
            continue;
        };
        // Modify-only, like every other write path: a featureless slot is a
        // transient projection hole. Minting here (with EITHER crust flag)
        // plants wrong-crust features wherever a drifting boundary sweeps
        // across holes — at subdivision 8 (~0.45 hex drift per tick) that
        // painted speckle trails of subsiding false-crust "water" through
        // continents. The dropped delta is recovered next tick when the hole
        // snaps back to feature-backed.
        let Some(existing) = plate.surface.get(key.birth_hex) else {
            continue;
        };
        let mut feature = existing.clone();

        let mut elev_delta = delta.elev as f32;
        if elev_delta > 0.0 {
            elev_delta *= uplift_headroom_factor(feature.elevation_m);
            feature.elevation_m += elev_delta;
        } else if elev_delta < 0.0 {
            elev_delta *= subsidence_headroom_factor(feature.elevation_m);
            // Deltas are computed against the DISPLAYED hex elevation but
            // applied to the birth feature; quantization can pair them with a
            // deeper feature than the display showed, bypassing the per-source
            // equilibria (trench −8500, divergent −4000). Floor boundary
            // subsidence just past the deepest equilibrium so trenches
            // asymptote instead of racing to the −11000 clamp.
            feature.elevation_m =
                (feature.elevation_m + elev_delta).max(BOUNDARY_SUBSIDENCE_FLOOR_M);
        }
        feature.relief_m += delta.relief as f32;
        if let Some(bedrock) = delta.bedrock {
            feature.bedrock = bedrock;
        }
        if delta.age_year > 0 {
            feature.age_year = delta.age_year;
        }
        plate.surface.set(key.birth_hex, feature);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::time::WorldYear;
    use genesis_core::{HexGrid, PlateId};

    use crate::boundary::{BoundaryClass, BoundaryInfo, ClassifiedEdge, ConvergentSubtype};
    use crate::plate::{Plate, PlateClass, PlateRegistry, PlateType};
    use crate::plate_surface::{PlateSurface, SurfaceFeature, baseline_feature};
    use crate::world_rebuild::rebuild_world_from_plate_surfaces;

    const EARTH_RADIUS_KM: f64 = 6371.0;

    fn plate_at(id: u16, plate_type: PlateType, seed: u32, rate: f64, cell_count: usize) -> Plate {
        Plate {
            id: PlateId(id),
            plate_type,
            plate_class: PlateClass::Major,
            seed_hex: HexId(seed),
            motion_axis: [0.0, 0.0, 1.0],
            motion_rate_rad_per_year: rate,
            age_year: WorldYear::FORMATION,
            target_fraction: 0.5,
            accumulated_rotation_rad: 0.0,
            last_nonempty_year: WorldYear::FORMATION,
            surface: PlateSurface::new(cell_count),
        }
    }

    fn seed_surfaces_from_world(data: &WorldData, registry: &mut PlateRegistry) {
        for hex in data.grid.iter() {
            let idx = hex.0 as usize;
            let plate_id = data.plate_id[idx];
            if plate_id == PlateId::NONE {
                continue;
            }
            let Some(plate) = registry.plates_mut().get_mut(&plate_id) else {
                continue;
            };
            plate.surface.set(
                hex,
                SurfaceFeature {
                    elevation_m: data.elevation_mean[idx],
                    relief_m: data.elevation_relief[idx],
                    bedrock: data.bedrock_type[idx],
                    fertility: data.fertility[idx],
                    age_year: 0,
                    continental_crust: false,
                },
            );
        }
    }

    fn apply_and_rebuild(
        data: &mut WorldData,
        registry: &mut PlateRegistry,
        boundaries: &BoundaryInfo,
        interval: f64,
        year: WorldYear,
    ) {
        populate_surfaces(data, registry);
        apply_boundary_elevation(
            data,
            registry,
            &ProjectionCache::empty(),
            boundaries,
            interval,
            year,
        );
        rebuild_world_from_plate_surfaces(data, registry);
    }

    fn world_with_plates(level: u8) -> (WorldData, PlateRegistry) {
        let grid = HexGrid::new(level, EARTH_RADIUS_KM).expect("grid");
        let cell_count = grid.cell_count() as usize;
        let params = WorldParameters::default();
        let data = WorldData::new(grid, params);
        let mut registry = PlateRegistry::new();
        registry.insert(plate_at(0, PlateType::Oceanic, 0, 1e-8, cell_count));
        registry.insert(plate_at(1, PlateType::Oceanic, 500, 5e-9, cell_count));
        (data, registry)
    }

    fn set_half_half(data: &mut WorldData) {
        let n = data.plate_id.len();
        for (i, pid) in data.plate_id.iter_mut().enumerate() {
            *pid = if i < n / 2 { PlateId(0) } else { PlateId(1) };
        }
        for elev in &mut data.elevation_mean {
            *elev = 0.0;
        }
    }

    /// Populates every owned hex lacking a feature with a baseline one at the
    /// displayed elevation, mirroring formation: delta flush is modify-only,
    /// so tests must not rely on featureless slots being minted. Pre-seeded
    /// features are left untouched.
    fn populate_surfaces(data: &WorldData, registry: &mut PlateRegistry) {
        for (i, &pid) in data.plate_id.iter().enumerate() {
            if pid == PlateId::NONE {
                continue;
            }
            let Some(plate) = registry.plates_mut().get_mut(&pid) else {
                continue;
            };
            let hex = HexId(i as u32);
            if plate.surface.get(hex).is_some() {
                continue;
            }
            let plate_type = plate.plate_type;
            let mut feature = baseline_feature(plate_type, 0);
            feature.elevation_m = data.elevation_mean[i];
            plate.surface.set(hex, feature);
        }
    }

    #[test]
    fn divergent_edge_lowers_elevation() {
        let (mut data, mut registry) = world_with_plates(4);
        set_half_half(&mut data);
        let hex = HexId(10);
        let mut boundaries = BoundaryInfo::default();
        boundaries.boundary_hexes.push(hex);
        boundaries.edges.insert(
            hex,
            vec![ClassifiedEdge {
                neighbor_hex: HexId(11),
                other_plate: PlateId(1),
                class: BoundaryClass::Divergent,
                normal_velocity_m_per_year: -0.05,
                tangential_velocity_m_per_year: 0.0,
            }],
        );

        apply_and_rebuild(
            &mut data,
            &mut registry,
            &boundaries,
            500_000.0,
            WorldYear(500_000),
        );
        assert!(data.elevation_mean[hex.0 as usize] < 0.0);
        assert_eq!(data.bedrock_type[hex.0 as usize], BedrockType::OceanicCrust);
    }

    #[test]
    fn convergent_cc_raises_elevation() {
        let (mut data, mut registry) = world_with_plates(4);
        let cell_count = data.plate_id.len();
        registry.insert(plate_at(2, PlateType::Continental, 200, 1e-8, cell_count));
        registry.insert(plate_at(3, PlateType::Continental, 800, 1e-8, cell_count));
        let n = data.plate_id.len();
        for (i, pid) in data.plate_id.iter_mut().enumerate() {
            *pid = if i < n / 2 { PlateId(2) } else { PlateId(3) };
        }
        for elev in &mut data.elevation_mean {
            *elev = 100.0;
        }

        let hex = HexId(10);
        let mut boundaries = BoundaryInfo::default();
        boundaries.boundary_hexes.push(hex);
        boundaries.edges.insert(
            hex,
            vec![ClassifiedEdge {
                neighbor_hex: HexId(11),
                other_plate: PlateId(3),
                class: BoundaryClass::Convergent(ConvergentSubtype::ContinentalContinental),
                normal_velocity_m_per_year: 0.05,
                tangential_velocity_m_per_year: 0.0,
            }],
        );

        apply_and_rebuild(
            &mut data,
            &mut registry,
            &boundaries,
            500_000.0,
            WorldYear(500_000),
        );
        assert!(data.elevation_mean[hex.0 as usize] > 100.0);
        assert_eq!(data.bedrock_type[hex.0 as usize], BedrockType::Metamorphic);
    }

    #[test]
    fn divergent_at_ocean_baseline_does_not_subside_further() {
        let (mut data, mut registry) = world_with_plates(4);
        set_half_half(&mut data);
        let hex = HexId(10);
        data.elevation_mean[hex.0 as usize] = OCEAN_FLOOR_BASELINE_M;
        seed_surfaces_from_world(&data, &mut registry);

        let mut boundaries = BoundaryInfo::default();
        boundaries.boundary_hexes.push(hex);
        boundaries.edges.insert(
            hex,
            vec![ClassifiedEdge {
                neighbor_hex: HexId(11),
                other_plate: PlateId(1),
                class: BoundaryClass::Divergent,
                normal_velocity_m_per_year: -0.05,
                tangential_velocity_m_per_year: 0.0,
            }],
        );

        apply_and_rebuild(
            &mut data,
            &mut registry,
            &boundaries,
            500_000.0,
            WorldYear(500_000),
        );
        assert!(
            (data.elevation_mean[hex.0 as usize] - OCEAN_FLOOR_BASELINE_M).abs() < 1.0,
            "at ocean baseline, divergent subsidence should stop"
        );
    }

    #[test]
    fn orogeny_near_equilibrium_adds_minimal_uplift() {
        let (mut data, mut registry) = world_with_plates(4);
        let cell_count = data.plate_id.len();
        registry.insert(plate_at(2, PlateType::Continental, 200, 1e-8, cell_count));
        registry.insert(plate_at(3, PlateType::Continental, 800, 1e-8, cell_count));
        let n = data.plate_id.len();
        for (i, pid) in data.plate_id.iter_mut().enumerate() {
            *pid = if i < n / 2 { PlateId(2) } else { PlateId(3) };
        }

        let hex = HexId(10);
        data.elevation_mean[hex.0 as usize] = MOUNTAIN_EQUILIBRIUM_M - 50.0;

        let mut boundaries = BoundaryInfo::default();
        boundaries.boundary_hexes.push(hex);
        boundaries.edges.insert(
            hex,
            vec![ClassifiedEdge {
                neighbor_hex: HexId(11),
                other_plate: PlateId(3),
                class: BoundaryClass::Convergent(ConvergentSubtype::ContinentalContinental),
                normal_velocity_m_per_year: 0.05,
                tangential_velocity_m_per_year: 0.0,
            }],
        );

        let before = data.elevation_mean[hex.0 as usize];
        apply_and_rebuild(
            &mut data,
            &mut registry,
            &boundaries,
            500_000.0,
            WorldYear(500_000),
        );
        let after = data.elevation_mean[hex.0 as usize];
        assert!(
            after - before < 20.0,
            "near mountain equilibrium, uplift should be small (got {} m)",
            after - before
        );
    }

    #[test]
    fn convergent_oc_trench_on_oceanic_owner() {
        let (mut data, mut registry) = world_with_plates(4);
        let cell_count = data.plate_id.len();
        registry.insert(plate_at(2, PlateType::Oceanic, 100, 1e-8, cell_count));
        registry.insert(plate_at(3, PlateType::Continental, 900, 1e-8, cell_count));

        let hex = HexId(50);
        data.plate_id[hex.0 as usize] = PlateId(2);
        let neighbor = data.grid.neighbors(hex)[0];
        data.plate_id[neighbor.0 as usize] = PlateId(3);
        data.elevation_mean[hex.0 as usize] = 0.0;
        // Trench side selection keys on per-hex crust.
        data.bedrock_type[hex.0 as usize] = BedrockType::OceanicCrust;

        let mut boundaries = BoundaryInfo::default();
        boundaries.boundary_hexes.push(hex);
        boundaries.edges.insert(
            hex,
            vec![ClassifiedEdge {
                neighbor_hex: neighbor,
                other_plate: PlateId(3),
                class: BoundaryClass::Convergent(ConvergentSubtype::ContinentalOceanic),
                normal_velocity_m_per_year: 0.05,
                tangential_velocity_m_per_year: 0.0,
            }],
        );

        apply_and_rebuild(
            &mut data,
            &mut registry,
            &boundaries,
            500_000.0,
            WorldYear(500_000),
        );
        assert!(data.elevation_mean[hex.0 as usize] < 0.0);
    }

    #[test]
    fn clamping_caps_extreme_elevation() {
        let grid = HexGrid::new(4, EARTH_RADIUS_KM).expect("grid");
        let cell_count = grid.cell_count() as usize;
        let params = WorldParameters::default();
        let mut data = WorldData::new(grid, params);
        data.elevation_mean[0] = 50_000.0;

        let mut registry = PlateRegistry::new();
        registry.insert(plate_at(0, PlateType::Continental, 0, 1e-8, cell_count));
        registry.insert(plate_at(1, PlateType::Continental, 500, 1e-8, cell_count));
        data.plate_id[0] = PlateId(0);
        data.plate_id[1] = PlateId(1);

        let hex = HexId(0);
        let mut boundaries = BoundaryInfo::default();
        boundaries.boundary_hexes.push(hex);
        boundaries.edges.insert(
            hex,
            vec![ClassifiedEdge {
                neighbor_hex: HexId(1),
                other_plate: PlateId(1),
                class: BoundaryClass::Convergent(ConvergentSubtype::ContinentalContinental),
                normal_velocity_m_per_year: 1.0,
                tangential_velocity_m_per_year: 0.0,
            }],
        );

        apply_and_rebuild(
            &mut data,
            &mut registry,
            &boundaries,
            500_000.0,
            WorldYear(500_000),
        );
        clamp_terrain(&mut data);
        assert!(data.elevation_mean[0] <= MAX_ELEVATION_M);
    }

    #[test]
    fn triple_junction_accumulates_without_panic() {
        let (mut data, mut registry) = world_with_plates(4);
        let cell_count = data.plate_id.len();
        registry.insert(plate_at(2, PlateType::Oceanic, 200, 1e-8, cell_count));

        let hex = HexId(100);
        let neighbors: Vec<HexId> = data.grid.neighbors(hex).to_vec();
        data.plate_id[hex.0 as usize] = PlateId(0);
        data.plate_id[neighbors[0].0 as usize] = PlateId(1);
        data.plate_id[neighbors[1].0 as usize] = PlateId(2);

        let mut boundaries = BoundaryInfo::default();
        boundaries.boundary_hexes.push(hex);
        boundaries.edges.insert(
            hex,
            vec![
                ClassifiedEdge {
                    neighbor_hex: neighbors[0],
                    other_plate: PlateId(1),
                    class: BoundaryClass::Divergent,
                    normal_velocity_m_per_year: -0.05,
                    tangential_velocity_m_per_year: 0.0,
                },
                ClassifiedEdge {
                    neighbor_hex: neighbors[1],
                    other_plate: PlateId(2),
                    class: BoundaryClass::Transform,
                    normal_velocity_m_per_year: 0.0,
                    tangential_velocity_m_per_year: 0.1,
                },
            ],
        );

        apply_and_rebuild(
            &mut data,
            &mut registry,
            &boundaries,
            500_000.0,
            WorldYear(500_000),
        );
        assert_eq!(data.bedrock_type[hex.0 as usize], BedrockType::Metamorphic);
    }

    #[test]
    fn subducting_plate_faster_rate() {
        let fast = plate_at(0, PlateType::Oceanic, 0, 2e-8, 100);
        let slow = plate_at(1, PlateType::Oceanic, 1, 1e-8, 100);
        assert_eq!(
            subducting_plate_id(PlateId(0), PlateId(1), &fast, &slow),
            PlateId(0)
        );
    }

    #[test]
    fn continental_oc_spreads_coastal_shelf_on_oceanic_neighbor() {
        let (mut data, mut registry) = world_with_plates(4);
        let cell_count = data.plate_id.len();
        registry.insert(plate_at(2, PlateType::Oceanic, 100, 1e-8, cell_count));
        registry.insert(plate_at(3, PlateType::Continental, 900, 1e-8, cell_count));

        let continental_hex = HexId(50);
        let oceanic_neighbor = data.grid.neighbors(continental_hex)[0];
        data.plate_id[continental_hex.0 as usize] = PlateId(3);
        data.plate_id[oceanic_neighbor.0 as usize] = PlateId(2);
        data.elevation_mean[continental_hex.0 as usize] = 200.0;
        data.elevation_mean[oceanic_neighbor.0 as usize] = -500.0;
        let oceanic_elev_before = data.elevation_mean[oceanic_neighbor.0 as usize];

        let mut boundaries = BoundaryInfo::default();
        boundaries.boundary_hexes.push(continental_hex);
        boundaries.edges.insert(
            continental_hex,
            vec![ClassifiedEdge {
                neighbor_hex: oceanic_neighbor,
                other_plate: PlateId(2),
                class: BoundaryClass::Convergent(ConvergentSubtype::ContinentalOceanic),
                normal_velocity_m_per_year: 0.05,
                tangential_velocity_m_per_year: 0.0,
            }],
        );

        apply_and_rebuild(
            &mut data,
            &mut registry,
            &boundaries,
            500_000.0,
            WorldYear(500_000),
        );
        assert!(
            data.elevation_mean[oceanic_neighbor.0 as usize] < oceanic_elev_before,
            "coastal shelf should lower oceanic neighbor elevation"
        );
    }

    #[test]
    fn subducting_plate_tie_breaks_lower_id() {
        let a = plate_at(0, PlateType::Oceanic, 0, 1e-8, 100);
        let b = plate_at(1, PlateType::Oceanic, 1, 1e-8, 100);
        assert_eq!(
            subducting_plate_id(PlateId(0), PlateId(1), &a, &b),
            PlateId(0)
        );
    }
}
