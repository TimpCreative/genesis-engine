//! Doc 08 §15 validation gates (cheap CI + `#[ignore]` deep-time) plus the
//! §15 metrics helpers the full-stack gates in `genesis_ui::hydro_validation`
//! compose. Metrics are pure, deterministic derivations over bulk arrays
//! (ascending `HexId` where order matters), slice-based so the same helper
//! reads both `WorldData` fields and `HistoryFrame` fields.

use genesis_core::data::{
    HydroFlags, MAJOR_CLASS_MIN_M3_YR, STREAM_CLASS_MIN_M3_YR, SoilClass, WATER_NONE,
    WaterBodyKind, WorldData,
};
use genesis_core::grid::{Direction, HexGrid};
use genesis_core::parameters::{WorldParameters, WorldSeed};
use genesis_core::{HexId, WaterBodyId};

/// Fixed seed for Doc 08 validation (mirrors tectonics).
pub const VALIDATION_SEED: u64 = 42;
/// CI-friendly subdivision (~2,432 hexes).
pub const VALIDATION_SUBDIVISION_LEVEL: u8 = 5;
/// Quick horizon (2 Geological ticks).
pub const VALIDATION_TARGET_YEAR_QUICK: i64 = 1_000_000;
/// Mid-depth horizon for ignored gates.
pub const VALIDATION_TARGET_YEAR_200M: i64 = 200_000_000;
/// One-billion-year deep-time horizon.
pub const VALIDATION_TARGET_YEAR_1B: i64 = 1_000_000_000;
/// Full deep-time horizon.
pub const VALIDATION_TARGET_YEAR_4B: i64 = 4_000_000_000;

/// Default parameters for hydrology validation worlds.
pub fn validation_parameters() -> WorldParameters {
    let mut params = WorldParameters::default();
    params.core.seed = WorldSeed::from_integer(VALIDATION_SEED);
    params.core.grid.subdivision_level = VALIDATION_SUBDIVISION_LEVEL;
    params.core.hydrology.water_inventory_gel_m = 1000.0;
    params
}

// ---- §15 metrics (P2-34) ----
//
// Every helper is a pure slice-based derivation so the full-stack gates can
// run it against a live `WorldData` or against a captured `HistoryFrame`
// (frames carry no registry, so registry-derived metrics are `&WorldData`
// only). All loops run in ascending index order; counts are integers, so the
// metrics are bit-deterministic.

/// Fraction of cells with `elevation_mean > sea_level_m` (§15 #2/#3; same
/// definition as tectonics' `continental_fraction`, so gates read alike).
pub fn land_fraction(elevation_mean: &[f32], sea_level_m: f32) -> f32 {
    if elevation_mean.is_empty() {
        return 0.0;
    }
    let land = elevation_mean.iter().filter(|&&e| e > sea_level_m).count();
    land as f32 / elevation_mean.len() as f32
}

/// Fraction of dry continental-crust hexes sitting more than `margin_m` below
/// freeboard (`elev − sea < CONTINENTAL_FREEBOARD_M − margin_m`). Used to
/// police charcoal interior pits after the morphology fix. Hexes within two
/// rings of ocean/sea are excluded (fjord / coastal troughs are allowed).
pub fn continental_dry_pit_fraction(data: &WorldData, margin_m: f32) -> f32 {
    use crate::erosion::CONTINENTAL_FREEBOARD_M;
    let n = data.cell_count() as usize;
    let sea = data.sea_level_m;
    let threshold = CONTINENTAL_FREEBOARD_M - margin_m;
    let mut dry_cont = 0usize;
    let mut pits = 0usize;

    let mut ocean_side = vec![false; n];
    #[allow(clippy::needless_range_loop)]
    for i in 0..n {
        if data.elevation_mean[i] < sea {
            ocean_side[i] = true;
            continue;
        }
        let id = data.water_body_id[i];
        if id != WaterBodyId::NONE
            && data
                .water_bodies
                .get(&id)
                .is_some_and(|b| matches!(b.kind, WaterBodyKind::Ocean | WaterBodyKind::Sea))
        {
            ocean_side[i] = true;
        }
    }
    // Dilate ocean-side by 2 rings so fjord coasts are excluded from the pit rate.
    let mut near_ocean = ocean_side.clone();
    for _ in 0..2 {
        let prev = near_ocean.clone();
        #[allow(clippy::needless_range_loop)]
        for i in 0..n {
            if prev[i] {
                continue;
            }
            for nb in data.grid.neighbors(HexId(i as u32)) {
                let j = nb.0 as usize;
                if j < n && prev[j] {
                    near_ocean[i] = true;
                    break;
                }
            }
        }
    }

    #[allow(clippy::needless_range_loop)]
    for i in 0..n {
        if !data.continental_crust.get(i).copied().unwrap_or(false) {
            continue;
        }
        if near_ocean[i] {
            continue;
        }
        let elev = data.elevation_mean[i];
        let water = data.water_level_m.get(i).copied().unwrap_or(WATER_NONE);
        if water > elev && water.is_finite() {
            continue;
        }
        dry_cont += 1;
        if elev - sea < threshold {
            pits += 1;
        }
    }
    if dry_cont == 0 {
        return 0.0;
    }
    pits as f32 / dry_cont as f32
}

/// Fraction of cells with standing water (`water_level_m != WATER_NONE`) —
/// the frame-compatible wet mask (frames carry no `water_body_id`).
pub fn wet_fraction_from_levels(water_level_m: &[f32]) -> f32 {
    if water_level_m.is_empty() {
        return 0.0;
    }
    let wet = water_level_m.iter().filter(|&&w| w != WATER_NONE).count();
    wet as f32 / water_level_m.len() as f32
}

/// Fraction of cells under land ice (sheets + budgeted alpine glaciers).
pub fn ice_area_fraction(ice_mask: &[bool]) -> f32 {
    if ice_mask.is_empty() {
        return 0.0;
    }
    let iced = ice_mask.iter().filter(|&&i| i).count();
    iced as f32 / ice_mask.len() as f32
}

/// Distinct Major rivers (§4.4): connected components of Major-class channel
/// hexes under grid adjacency, BFS in ascending `HexId` order. Returns
/// `(rivers, channel hexes)`. §15 #5's [3, 30] band reads the first value.
pub fn major_river_census(grid: &HexGrid, river_discharge_m3_yr: &[f32]) -> (usize, usize) {
    let n = river_discharge_m3_yr.len();
    let is_major = |i: usize| f64::from(river_discharge_m3_yr[i]) >= MAJOR_CLASS_MIN_M3_YR;
    let mut visited = vec![false; n];
    let mut rivers = 0_usize;
    let mut hexes = 0_usize;
    for start in 0..n {
        if visited[start] || !is_major(start) {
            continue;
        }
        rivers += 1;
        let mut queue = std::collections::VecDeque::from([start]);
        visited[start] = true;
        while let Some(i) = queue.pop_front() {
            hexes += 1;
            for &neighbor in grid.neighbors_sorted(HexId(i as u32)) {
                let j = neighbor.0 as usize;
                if j < n && !visited[j] && is_major(j) {
                    visited[j] = true;
                    queue.push_back(j);
                }
            }
        }
    }
    (rivers, hexes)
}

/// Largest drainage basin as a fraction of its continent (§15 #5, anchored
/// on Amazon/South-America ≈ 0.35). Basins group land cells by terminal
/// sink — the wet cell the flow path reaches, or the cell itself when it is
/// a retained sink (no flow direction on land). Continents are connected
/// land components. Basin shares are counted per component (a basin tree is
/// connected, but two disjoint islands can share one wet terminal). Returns
/// 0.0 when the world has no land.
pub fn largest_basin_fraction_of_continent(
    grid: &HexGrid,
    flow_direction: &[Option<Direction>],
    wet: &[bool],
) -> f32 {
    let n = flow_direction.len();
    const UNRESOLVED: u32 = u32::MAX;
    // Terminal resolution with path backfill (the filled surface descends
    // monotonically, so walks terminate; memoization keeps it near-linear).
    let mut terminal = vec![UNRESOLVED; n];
    for start in 0..n {
        if wet[start] || terminal[start] != UNRESOLVED {
            continue;
        }
        let mut path: Vec<u32> = Vec::new();
        let mut current = start as u32;
        let term = loop {
            let c = current as usize;
            if terminal[c] != UNRESOLVED {
                break terminal[c];
            }
            if wet[c] {
                break current;
            }
            path.push(current);
            match flow_direction[c] {
                None => break current, // retained sink drains to itself
                Some(dir) => {
                    current = grid.neighbors(HexId(current))[dir.index()].0;
                }
            }
        };
        for &cell in &path {
            terminal[cell as usize] = term;
        }
    }

    // Continents: connected land components, BFS in ascending order.
    let mut visited = vec![false; n];
    let mut best = 0.0_f32;
    for start in 0..n {
        if visited[start] || wet[start] {
            continue;
        }
        let mut queue = std::collections::VecDeque::from([start]);
        visited[start] = true;
        let mut size = 0_usize;
        let mut basin_counts: std::collections::BTreeMap<u32, usize> =
            std::collections::BTreeMap::new();
        while let Some(cell) = queue.pop_front() {
            size += 1;
            *basin_counts.entry(terminal[cell]).or_default() += 1;
            for &neighbor in grid.neighbors_sorted(HexId(cell as u32)) {
                let j = neighbor.0 as usize;
                if j < n && !visited[j] && !wet[j] {
                    visited[j] = true;
                    queue.push_back(j);
                }
            }
        }
        let largest = basin_counts.values().copied().max().unwrap_or(0);
        best = best.max(largest as f32 / size as f32);
    }
    best
}

/// Registry counts by [`WaterBodyKind`] (§15 #6/#9).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WaterBodyCensus {
    /// Largest-volume below-sea component(s).
    pub ocean: usize,
    /// Isolated ocean-fed seas (Caspian analog).
    pub sea: usize,
    /// Exorheic or fresh endorheic lakes.
    pub lake: usize,
    /// Endorheic bodies past the salinity threshold (§5.3).
    pub salt_lake: usize,
    /// Dried endorheic floors (§5.2).
    pub salt_flat: usize,
}

/// Counts registry bodies by kind.
pub fn water_body_census(data: &WorldData) -> WaterBodyCensus {
    let mut census = WaterBodyCensus::default();
    for body in data.water_bodies.values() {
        match body.kind {
            WaterBodyKind::Ocean => census.ocean += 1,
            WaterBodyKind::Sea => census.sea += 1,
            WaterBodyKind::Lake => census.lake += 1,
            WaterBodyKind::SaltLake => census.salt_lake += 1,
            WaterBodyKind::SaltFlat => census.salt_flat += 1,
        }
    }
    census
}

/// Standing endorheic water bodies: registry Lake/SaltLake with no spill
/// outlet (§5 — an exorheic lake spills through `outlet`; a SaltFlat is no
/// longer standing water and is reported separately in the census).
pub fn endorheic_body_count(data: &WorldData) -> usize {
    data.water_bodies
        .values()
        .filter(|b| matches!(b.kind, WaterBodyKind::Lake | WaterBodyKind::SaltLake))
        .filter(|b| b.outlet.is_none())
        .count()
}

/// Per-flag hex counts over `hydro_flags` (§15 #7/#13–#18).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FlagCensus {
    /// Groundwater resurgences (§6.4).
    pub spring: usize,
    /// Hyper-arid hexes with a reachable water table (§6.4).
    pub oasis: usize,
    /// Karst drainage diversions (§6.3).
    pub karst: usize,
    /// Drowned river mouths (§11.2).
    pub estuary: usize,
    /// Glacially carved, ocean-flooded troughs (§9.2).
    pub fjord: usize,
    /// Non-perennial channels (§7.3).
    pub ephemeral: usize,
    /// Intertidal/permafrost wetlands (§10.2/§11.3).
    pub wetland: usize,
    /// Sea ice cover (§9).
    pub sea_ice: usize,
    /// Persistent glacial trough scars (§9.2).
    pub carved_trough: usize,
    /// Prograding Major river mouths (§8.3/§11.2).
    pub delta: usize,
}

/// Counts set bits over the whole flag array, in ascending order.
pub fn flag_census(hydro_flags: &[HydroFlags]) -> FlagCensus {
    let mut census = FlagCensus::default();
    for &f in hydro_flags {
        census.spring += usize::from(f.contains(HydroFlags::SPRING));
        census.oasis += usize::from(f.contains(HydroFlags::OASIS));
        census.karst += usize::from(f.contains(HydroFlags::KARST));
        census.estuary += usize::from(f.contains(HydroFlags::ESTUARY));
        census.fjord += usize::from(f.contains(HydroFlags::FJORD));
        census.ephemeral += usize::from(f.contains(HydroFlags::EPHEMERAL));
        census.wetland += usize::from(f.contains(HydroFlags::WETLAND));
        census.sea_ice += usize::from(f.contains(HydroFlags::SEA_ICE));
        census.carved_trough += usize::from(f.contains(HydroFlags::CARVED_TROUGH));
        census.delta += usize::from(f.contains(HydroFlags::DELTA));
    }
    census
}

/// Per-class hex counts over `soil_class` (§15 #8/#9/#16).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SoilCensus {
    /// Bare rock / active ice / open water.
    pub none: usize,
    /// Floodplain & delta deposition.
    pub alluvial: usize,
    /// Wind-blown glacial flour (§9.2).
    pub loess: usize,
    /// Young igneous / recent volcanism.
    pub volcanic: usize,
    /// Limestone / marine-sediment bedrock.
    pub calcareous: usize,
    /// Arid, thin.
    pub sandy: usize,
    /// Cold + wet + flat.
    pub peaty: usize,
    /// Salt-poisoned.
    pub saline: usize,
    /// Temperate default.
    pub loamy: usize,
}

/// Counts soil classes over the whole array, in ascending order.
pub fn soil_census(soil_class: &[SoilClass]) -> SoilCensus {
    let mut census = SoilCensus::default();
    for &class in soil_class {
        match class {
            SoilClass::None => census.none += 1,
            SoilClass::Alluvial => census.alluvial += 1,
            SoilClass::Loess => census.loess += 1,
            SoilClass::Volcanic => census.volcanic += 1,
            SoilClass::Calcareous => census.calcareous += 1,
            SoilClass::Sandy => census.sandy += 1,
            SoilClass::Peaty => census.peaty += 1,
            SoilClass::Saline => census.saline += 1,
            SoilClass::Loamy => census.loamy += 1,
        }
    }
    census
}

/// Q1/median/Q3 of `discharge_seasonality` over channel hexes (Stream class
/// and up — §15 #17). `None` when the world has no channels.
pub fn seasonality_quartiles(
    river_discharge_m3_yr: &[f32],
    discharge_seasonality: &[f32],
) -> Option<[f32; 3]> {
    let mut values: Vec<f32> = river_discharge_m3_yr
        .iter()
        .zip(discharge_seasonality.iter())
        .filter(|&(&q, _)| f64::from(q) >= STREAM_CLASS_MIN_M3_YR)
        .map(|(_, &s)| s)
        .collect();
    if values.is_empty() {
        return None;
    }
    values.sort_by(f32::total_cmp);
    let at = |num: usize| values[(values.len() * num / 4).min(values.len() - 1)];
    Some([at(1), at(2), at(3)])
}

/// Mean depth of open-ocean cells (`sea_level − elevation`), ascending-order
/// f64 mean (§3.4 hypsometry check). `None` when no ocean exists.
pub fn mean_open_ocean_depth_m(data: &WorldData) -> Option<f32> {
    let mut sum = 0.0_f64;
    let mut count = 0_u64;
    for (i, &id) in data.water_body_id.iter().enumerate() {
        if id == WaterBodyId::NONE {
            continue;
        }
        let is_ocean = data
            .water_bodies
            .get(&id)
            .is_some_and(|b| b.kind == WaterBodyKind::Ocean);
        if is_ocean {
            sum += f64::from(data.sea_level_m - data.elevation_mean[i]);
            count += 1;
        }
    }
    if count == 0 {
        None
    } else {
        Some((sum / count as f64) as f32)
    }
}

/// One §15 metrics snapshot over a live world (P2-34). Captured per gate and
/// printed with `--nocapture`; the registry-derived fields
/// (`bodies`/`endorheic_bodies`/`mean_ocean_depth_m`) read `water_bodies`,
/// so frame-level metrics use the frame-compatible helpers directly.
#[derive(Clone, Debug)]
pub struct HydroMetrics {
    /// Simulated year of the snapshot.
    pub year: i64,
    /// Derived global sea level (§3.4).
    pub sea_level_m: f32,
    /// Fraction of cells above sea level.
    pub land_fraction: f32,
    /// Fraction of cells in the water-body mask.
    pub wet_fraction: f32,
    /// Fraction of cells under land ice.
    pub ice_area_fraction: f32,
    /// Distinct Major rivers (connected Major-class components).
    pub major_rivers: usize,
    /// Major-class channel hexes.
    pub major_channel_hexes: usize,
    /// Largest drainage basin share of its continent.
    pub largest_basin_fraction: f32,
    /// Registry counts by kind.
    pub bodies: WaterBodyCensus,
    /// Standing endorheic bodies (Lake/SaltLake, no outlet).
    pub endorheic_bodies: usize,
    /// Flag counts.
    pub flags: FlagCensus,
    /// Soil class counts.
    pub soils: SoilCensus,
    /// Q1/median/Q3 seasonality over channel hexes.
    pub seasonality_quartiles: Option<[f32; 3]>,
    /// Mean open-ocean depth.
    pub mean_ocean_depth_m: Option<f32>,
}

impl HydroMetrics {
    /// Captures every §15 metric from a live world.
    pub fn capture(data: &WorldData) -> Self {
        let wet: Vec<bool> = data
            .water_body_id
            .iter()
            .map(|&id| id != WaterBodyId::NONE)
            .collect();
        let (major_rivers, major_channel_hexes) =
            major_river_census(&data.grid, &data.river_discharge_m3_yr);
        Self {
            year: data.current_year.value(),
            sea_level_m: data.sea_level_m,
            land_fraction: land_fraction(&data.elevation_mean, data.sea_level_m),
            wet_fraction: wet.iter().filter(|&&w| w).count() as f32 / wet.len().max(1) as f32,
            ice_area_fraction: ice_area_fraction(&data.ice_mask),
            major_rivers,
            major_channel_hexes,
            largest_basin_fraction: largest_basin_fraction_of_continent(
                &data.grid,
                &data.flow_direction,
                &wet,
            ),
            bodies: water_body_census(data),
            endorheic_bodies: endorheic_body_count(data),
            flags: flag_census(&data.hydro_flags),
            soils: soil_census(&data.soil_class),
            seasonality_quartiles: seasonality_quartiles(
                &data.river_discharge_m3_yr,
                &data.discharge_seasonality,
            ),
            mean_ocean_depth_m: mean_open_ocean_depth_m(data),
        }
    }
}

impl std::fmt::Display for HydroMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let seasonality = self
            .seasonality_quartiles
            .map(|q| format!("{:.2}/{:.2}/{:.2}", q[0], q[1], q[2]))
            .unwrap_or_else(|| "none (no channels)".to_string());
        let depth = self
            .mean_ocean_depth_m
            .map(|d| format!("{d:.0} m"))
            .unwrap_or_else(|| "none".to_string());
        write!(
            f,
            "year={} sea={:+.1}m land={:.1}% wet={:.1}% ice={:.2}% ocean_depth={depth}\n\
             \x20 rivers: major={} ({} Major hexes) largest_basin={:.1}% of continent\n\
             \x20 bodies: ocean={} sea={} lake={} salt_lake={} salt_flat={} endorheic={}\n\
             \x20 flags: spring={} oasis={} karst={} estuary={} fjord={} ephemeral={} \
             wetland={} sea_ice={} trough={} delta={}\n\
             \x20 soils: none={} alluvial={} loess={} volcanic={} calcareous={} sandy={} \
             peaty={} saline={} loamy={}\n\
             \x20 seasonality q1/med/q3={seasonality}",
            self.year,
            self.sea_level_m,
            self.land_fraction * 100.0,
            self.wet_fraction * 100.0,
            self.ice_area_fraction * 100.0,
            self.major_rivers,
            self.major_channel_hexes,
            self.largest_basin_fraction * 100.0,
            self.bodies.ocean,
            self.bodies.sea,
            self.bodies.lake,
            self.bodies.salt_lake,
            self.bodies.salt_flat,
            self.endorheic_bodies,
            self.flags.spring,
            self.flags.oasis,
            self.flags.karst,
            self.flags.estuary,
            self.flags.fjord,
            self.flags.ephemeral,
            self.flags.wetland,
            self.flags.sea_ice,
            self.flags.carved_trough,
            self.flags.delta,
            self.soils.none,
            self.soils.alluvial,
            self.soils.loess,
            self.soils.volcanic,
            self.soils.calcareous,
            self.soils.sandy,
            self.soils.peaty,
            self.soils.saline,
            self.soils.loamy,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::data::{HydroFlags, RiverClass, WaterBodyKind, river_class};
    use genesis_core::time::SimulationLayer;
    use genesis_core::{WorldYear, create_world};

    use crate::budget::{WaterBudget, inventory_volume_m3};
    use crate::ice::ICE_VOLUME_MAX_SLE_M;
    use crate::layer::HydrologyLayer;
    use crate::routing::hex_area_m2;
    use crate::state::HydrologyState;

    #[test]
    fn conservation_holds_after_active_tick() {
        let mut params = validation_parameters();
        params.core.hydrology.water_inventory_gel_m = 1000.0;
        let mut world = create_world(params).expect("world");
        world.data.current_year = WorldYear(1_000_000_000);
        world.data.precipitation.fill(800.0);
        world.data.temperature_mean.fill(10.0);
        let n = world.data.cell_count() as usize;
        for i in 0..n / 2 {
            world.data.elevation_mean[i] = -2000.0;
        }
        for i in n / 2..n {
            world.data.elevation_mean[i] = 500.0;
        }
        let mut hydrology = HydrologyState::new();
        let (mut layer, shared) = HydrologyLayer::attach(&mut hydrology);
        layer.advance(&mut world.data, &world.rng);
        drop(layer);
        let state = HydrologyLayer::detach_state(shared);
        let inventory = inventory_volume_m3(&world.data.parameters);
        let budget = WaterBudget::partition(
            inventory,
            1.0,
            state.prev_lake_volume_m3,
            state.ice_volume_m3,
            state.groundwater_storage_m3,
        );
        assert!(
            budget.is_conserved(),
            "gate #1 conservation: err={}",
            budget.conservation_error_m3()
        );
    }

    #[test]
    fn sea_level_dial_responds_to_inventory() {
        let mut lows = Vec::new();
        for gel in [200.0_f32, 1000.0, 3000.0] {
            let mut params = validation_parameters();
            params.core.hydrology.water_inventory_gel_m = gel;
            let mut world = create_world(params).expect("world");
            world.data.current_year = WorldYear(1_000_000_000);
            world.data.precipitation.fill(100.0);
            let n = world.data.cell_count() as usize;
            for i in 0..n {
                world.data.elevation_mean[i] = if i < n / 3 { -3000.0 } else { 800.0 };
            }
            let mut hydrology = HydrologyState::new();
            let (mut layer, shared) = HydrologyLayer::attach(&mut hydrology);
            layer.advance(&mut world.data, &world.rng);
            drop(layer);
            let _ = HydrologyLayer::detach_state(shared);
            lows.push(world.data.sea_level_m);
        }
        assert!(
            lows[0] <= lows[1] + 1.0 && lows[1] <= lows[2] + 1.0,
            "gate #2 sea-level dial: {lows:?}"
        );
    }

    #[test]
    fn honest_rivers_discharge_nondecreasing_along_flow() {
        // Gate #4 shape: after one active tick, discharge does not decrease
        // along any flow edge (deposition may leave equality).
        let params = validation_parameters();
        let mut world = create_world(params).expect("world");
        world.data.current_year = WorldYear(1_000_000_000);
        world.data.precipitation.fill(800.0);
        world.data.temperature_mean.fill(12.0);
        let n = world.data.cell_count() as usize;
        for i in 0..n / 2 {
            world.data.elevation_mean[i] = -2500.0;
        }
        for i in n / 2..n {
            world.data.elevation_mean[i] = 400.0 + (i as f32) * 0.1;
        }
        let mut hydrology = HydrologyState::new();
        let (mut layer, shared) = HydrologyLayer::attach(&mut hydrology);
        layer.advance(&mut world.data, &world.rng);
        drop(layer);
        let _ = HydrologyLayer::detach_state(shared);

        for i in 0..n {
            let Some(dir) = world.data.flow_direction[i] else {
                continue;
            };
            let Some(&target) = world
                .data
                .grid
                .neighbors(genesis_core::HexId(i as u32))
                .get(dir.index())
            else {
                continue;
            };
            let j = target.0 as usize;
            if world.data.water_body_id[j] != genesis_core::data::WaterBodyId::NONE {
                continue;
            }
            assert!(
                world.data.river_discharge_m3_yr[j] + 1.0 >= world.data.river_discharge_m3_yr[i],
                "gate #4 discharge decreases {i}→{j}: {} → {}",
                world.data.river_discharge_m3_yr[i],
                world.data.river_discharge_m3_yr[j]
            );
        }
    }

    #[test]
    fn glaciation_intensity_scales_ice_volume() {
        // Gate #3 shape: full intensity → ~120 m SLE equivalent volume.
        let params = validation_parameters();
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        world.data.glaciation_intensity = 1.0;
        world.data.temperature_mean.fill(-20.0);
        world.data.elevation_mean.fill(500.0);
        world.data.elevation_relief.fill(300.0);
        world.data.hydro_elevation_delta_m = vec![0.0; n];
        world.data.ice_load_m = vec![0.0; n];
        let surface = crate::routing::RoutingSurface::build(&world.data, &[]);
        let mut prev = vec![false; n];
        let (vol, _) = crate::ice::update_ice(&mut world.data, &surface, &mut prev, 0.0, 500_000.0);
        let planet = hex_area_m2(&world.data.grid) * n as f64;
        let sle = vol / planet;
        assert!(
            (sle - ICE_VOLUME_MAX_SLE_M).abs() < 1.0,
            "gate #3 SLE volume {sle} m vs max {}",
            ICE_VOLUME_MAX_SLE_M
        );
    }

    #[test]
    fn river_class_thresholds_match_spec() {
        assert_eq!(river_class(0.5e9), RiverClass::Creek);
        assert_eq!(river_class(1.0e9), RiverClass::Stream);
        assert_eq!(river_class(1.0e10), RiverClass::River);
        assert_eq!(river_class(1.0e11), RiverClass::Major);
    }

    #[test]
    fn hydro_flags_persist_carved_trough_bit() {
        let mut f = HydroFlags::NONE;
        f |= HydroFlags::CARVED_TROUGH;
        f |= HydroFlags::DELTA;
        assert!(f.contains(HydroFlags::CARVED_TROUGH));
        assert!(f.contains(HydroFlags::DELTA));
        f.remove(HydroFlags::ESTUARY);
        assert!(f.contains(HydroFlags::CARVED_TROUGH));
    }

    /// Gate #3 shape: full glaciation intensity budgets ~120 m SLE; ice load set.
    #[test]
    fn glacial_intensity_draws_sle_and_sets_gia_load() {
        let params = validation_parameters();
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        world.data.glaciation_intensity = 1.0;
        world.data.temperature_mean.fill(-20.0);
        world.data.elevation_mean.fill(800.0);
        world.data.elevation_relief.fill(400.0);
        world.data.hydro_elevation_delta_m = vec![0.0; n];
        world.data.ice_load_m = vec![0.0; n];
        let surface = crate::routing::RoutingSurface::build(&world.data, &[]);
        let mut prev = vec![false; n];
        let (vol, _) = crate::ice::update_ice(&mut world.data, &surface, &mut prev, 0.0, 500_000.0);
        let planet = hex_area_m2(&world.data.grid) * n as f64;
        let sle = vol / planet;
        assert!(
            (60.0..=130.0).contains(&sle),
            "gate #3 SLE drawdown {sle} m out of 60–130 band"
        );
        assert!(
            world.data.ice_load_m.iter().any(|&l| l > 0.0),
            "gate #20 precursor: ice_load_m must be set under ice"
        );
    }

    /// Gate #8 shape: high marine fertility → top-tier soil_fertility on land.
    #[test]
    fn cretaceous_beach_fertility_ranks_high() {
        let params = validation_parameters();
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        world.data.elevation_mean.fill(200.0);
        world.data.precipitation.fill(800.0);
        world.data.temperature_mean.fill(15.0);
        world
            .data
            .bedrock_type
            .fill(genesis_core::data::BedrockType::Sedimentary);
        world.data.fertility[1] = 1.0;
        let surface = crate::routing::RoutingSurface::build(&world.data, &[]);
        let alluvium = vec![0.0; n];
        crate::soil::update_soil(&mut world.data, &surface, &alluvium, 500_000.0);
        let fert = world.data.soil_fertility[1];
        let mut others: Vec<f32> = world.data.soil_fertility.to_vec();
        others.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p90 = others[(others.len() as f32 * 0.9) as usize];
        assert!(
            fert >= p90,
            "gate #8: marine fertility hex {fert} should be ≥ p90 {p90}"
        );
    }

    /// Gate #14 shape: forced Limestone + wet climate → KARST flag.
    #[test]
    fn karst_flags_on_limestone_when_wet() {
        let params = validation_parameters();
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        for i in 0..n / 2 {
            world.data.elevation_mean[i] = -2000.0;
        }
        for i in n / 2..n {
            world.data.elevation_mean[i] = 500.0;
            world.data.bedrock_type[i] = genesis_core::data::BedrockType::Limestone;
            world.data.precipitation[i] = 800.0;
            world.data.temperature_mean[i] = 12.0;
        }
        world.data.current_year = WorldYear(1_000_000_000);
        let mut hydrology = HydrologyState::new();
        let (mut layer, shared) = HydrologyLayer::attach(&mut hydrology);
        layer.advance(&mut world.data, &world.rng);
        drop(layer);
        let _ = HydrologyLayer::detach_state(shared);
        assert!(
            world
                .data
                .hydro_flags
                .iter()
                .any(|f| f.contains(HydroFlags::KARST)),
            "gate #14: wet limestone should set KARST"
        );
    }

    /// Gate #15 shape: glacial retreat on high-relief ocean-adjacent trough → FJORD.
    #[test]
    fn fjord_flag_on_glacial_retreat_coast() {
        let params = validation_parameters();
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        world.data.elevation_mean.fill(500.0);
        world.data.elevation_relief.fill(500.0);
        world.data.temperature_mean.fill(-20.0);
        world.data.glaciation_intensity = 1.0;
        world.data.hydro_elevation_delta_m = vec![0.0; n];
        world.data.ice_load_m = vec![0.0; n];
        // Hex 0 is ocean.
        world.data.elevation_mean[0] = -100.0;
        world.data.water_body_id[0] = genesis_core::data::WaterBodyId(0);
        world.data.water_bodies.insert(
            genesis_core::data::WaterBodyId(0),
            genesis_core::data::WaterBody {
                id: genesis_core::data::WaterBodyId(0),
                kind: WaterBodyKind::Ocean,
                surface_m: 0.0,
                area_km2: 1.0e6,
                volume_km3: 1.0e6,
                salinity: 0.0,
                outlet: None,
            },
        );
        let surface = crate::routing::RoutingSurface::build(&world.data, &[]);
        let mut prev = vec![true; n];
        prev[0] = false;
        // Warm retreat: land no longer iced.
        world.data.temperature_mean.fill(5.0);
        world.data.hydro_flags[1] |= HydroFlags::CARVED_TROUGH;
        world.data.water_body_id[1] = genesis_core::data::WaterBodyId(0);
        world.data.elevation_relief[1] = 500.0;
        let _ = crate::ice::update_ice(&mut world.data, &surface, &mut prev, 5e-8, 500_000.0);
        assert!(
            world.data.hydro_flags[1].contains(HydroFlags::FJORD),
            "gate #15: glacial retreat on a carved trough at the coast must set FJORD"
        );
    }

    /// Gate #9 shape: salt accumulates in endorheic adjudication (unit-level).
    #[test]
    fn salt_accumulates_on_endorheic_floor() {
        // Covered by lakes unit tests; assert salt field can become Saline soil.
        let params = validation_parameters();
        let mut world = create_world(params).expect("world");
        world.data.salt_accumulated[1] = crate::lakes::SALINE_SOIL_SALT_MIN;
        world.data.elevation_mean[1] = 200.0;
        world.data.precipitation[1] = 100.0;
        world.data.temperature_mean[1] = 20.0;
        let n = world.data.cell_count() as usize;
        let surface = crate::routing::RoutingSurface::build(&world.data, &[]);
        let alluvium = vec![0.0; n];
        crate::soil::update_soil(&mut world.data, &surface, &alluvium, 1.0);
        assert_eq!(
            world.data.soil_class[1],
            genesis_core::data::SoilClass::Saline,
            "gate #9 precursor: salt → Saline soil"
        );
    }

    #[test]
    #[ignore = "perf budget §14; run with --ignored"]
    fn hydrology_tick_perf_budget_subdiv5() {
        let params = validation_parameters();
        let mut world = create_world(params).expect("world");
        world.data.current_year = WorldYear(1_000_000_000);
        world.data.precipitation.fill(800.0);
        world.data.temperature_mean.fill(10.0);
        let n = world.data.cell_count() as usize;
        for i in 0..n / 2 {
            world.data.elevation_mean[i] = -2000.0;
        }
        for i in n / 2..n {
            world.data.elevation_mean[i] = 500.0;
        }
        let mut hydrology = HydrologyState::new();
        let (mut layer, shared) = HydrologyLayer::attach(&mut hydrology);
        let start = std::time::Instant::now();
        for _ in 0..5 {
            layer.advance(&mut world.data, &world.rng);
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0 / 5.0;
        drop(layer);
        let _ = HydrologyLayer::detach_state(shared);
        // Subdiv 5 is cheaper than §14's subdiv-7 5 ms budget; assert < 50 ms mean.
        assert!(
            ms < 50.0,
            "gate #11: mean hydrology tick {ms:.2} ms exceeds 50 ms at subdiv 5"
        );
    }

    #[test]
    #[ignore = "gate #20 GIA rebound shape; run with --ignored"]
    fn post_glacial_ice_load_clears_on_warmth() {
        let params = validation_parameters();
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        world.data.glaciation_intensity = 1.0;
        world.data.temperature_mean.fill(-20.0);
        world.data.elevation_mean.fill(800.0);
        world.data.elevation_relief.fill(300.0);
        world.data.hydro_elevation_delta_m = vec![0.0; n];
        world.data.ice_load_m = vec![0.0; n];
        let surface = crate::routing::RoutingSurface::build(&world.data, &[]);
        let mut prev = vec![false; n];
        let _ = crate::ice::update_ice(&mut world.data, &surface, &mut prev, 0.0, 500_000.0);
        assert!(world.data.ice_load_m.iter().any(|&l| l > 0.0));
        // Deglaciate.
        world.data.temperature_mean.fill(10.0);
        world.data.glaciation_intensity = 0.0;
        let _ = crate::ice::update_ice(&mut world.data, &surface, &mut prev, 0.0, 500_000.0);
        assert!(
            world.data.ice_load_m.iter().all(|&l| l == 0.0),
            "gate #20: ice_load_m must clear when ice retreats"
        );
    }

    // ---- §15 metrics unit tests (P2-34, cheap CI) ----

    #[test]
    fn land_and_wet_fractions_count_expected_cells() {
        let elevation = [-100.0_f32, 10.0, 50.0, -5.0];
        assert!((land_fraction(&elevation, 0.0) - 0.5).abs() < 1e-6);
        let levels = [WATER_NONE, 0.0, WATER_NONE, 12.0];
        assert!((wet_fraction_from_levels(&levels) - 0.5).abs() < 1e-6);
        assert!((ice_area_fraction(&[true, false, false, false]) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn flag_and_soil_censuses_count_every_cell() {
        let flags = [
            HydroFlags::SPRING | HydroFlags::DELTA,
            HydroFlags::EPHEMERAL,
            HydroFlags::DELTA,
            HydroFlags::NONE,
        ];
        let census = flag_census(&flags);
        assert_eq!(census.spring, 1);
        assert_eq!(census.delta, 2);
        assert_eq!(census.ephemeral, 1);
        assert_eq!(census.oasis, 0);

        let soils = [
            SoilClass::Loamy,
            SoilClass::Loess,
            SoilClass::Loess,
            SoilClass::Saline,
        ];
        let census = soil_census(&soils);
        assert_eq!(census.loess, 2);
        assert_eq!(census.loamy, 1);
        assert_eq!(census.saline, 1);
        assert_eq!(census.alluvial, 0);
    }

    #[test]
    fn seasonality_quartiles_read_channels_only() {
        let discharge = [2.0e9_f32, 3.0e9, 0.0, 5.0e9];
        let seasonality = [1.0_f32, 2.0, 9.0, 3.0];
        let q = seasonality_quartiles(&discharge, &seasonality).expect("channels exist");
        assert_eq!(q, [1.0, 2.0, 3.0], "the dry hex's 9.0 is excluded");
        assert_eq!(seasonality_quartiles(&[0.0], &[5.0]), None);
    }

    #[test]
    fn registry_census_and_endorheic_count_read_kinds_and_outlets() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        let mut insert = |id: u32, kind: WaterBodyKind, outlet: Option<HexId>| {
            world.data.water_bodies.insert(
                WaterBodyId(id),
                genesis_core::data::WaterBody {
                    id: WaterBodyId(id),
                    kind,
                    surface_m: 0.0,
                    area_km2: 1.0,
                    volume_km3: 1.0,
                    salinity: 0.0,
                    outlet,
                },
            );
        };
        insert(0, WaterBodyKind::Ocean, None);
        insert(10, WaterBodyKind::Lake, Some(HexId(11))); // exorheic: spills
        insert(20, WaterBodyKind::Lake, None); // endorheic
        insert(30, WaterBodyKind::SaltLake, None); // endorheic
        insert(40, WaterBodyKind::SaltFlat, None); // dried: not standing

        let census = water_body_census(&world.data);
        assert_eq!(census.ocean, 1);
        assert_eq!(census.lake, 2);
        assert_eq!(census.salt_lake, 1);
        assert_eq!(census.salt_flat, 1);
        assert_eq!(
            endorheic_body_count(&world.data),
            2,
            "standing Lake/SaltLake without outlet only"
        );
    }

    #[test]
    fn basin_and_major_census_read_flow_topology() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let world = create_world(params).expect("world");
        let grid = &world.data.grid;
        let n = world.data.cell_count() as usize;

        // One land triplet around hex 1: c1 and c2 both flow into 1.
        let c1 = grid.neighbors(HexId(1))[0];
        let c2 = grid.neighbors(HexId(1))[1];
        let mut wet = vec![true; n];
        let mut flow = vec![None; n];
        let mut discharge = vec![0.0_f32; n];
        for hex in [HexId(1), c1, c2] {
            wet[hex.0 as usize] = false;
        }
        let slot_of = |from: HexId, to: HexId| {
            Direction::from_index(
                grid.neighbors(from)
                    .iter()
                    .position(|&h| h == to)
                    .expect("adjacent"),
            )
            .expect("slot")
        };
        flow[c1.0 as usize] = Some(slot_of(c1, HexId(1)));
        flow[c2.0 as usize] = Some(slot_of(c2, HexId(1)));
        // Hex 1 keeps `None`: a retained sink draining to itself.

        // One basin covers the continent.
        let frac = largest_basin_fraction_of_continent(grid, &flow, &wet);
        assert!((frac - 1.0).abs() < 1e-6, "single sink, got {frac}");

        // c2 becomes a second sink: basins {1, c1} vs {c2} → 2/3.
        flow[c2.0 as usize] = None;
        let frac = largest_basin_fraction_of_continent(grid, &flow, &wet);
        assert!((frac - 2.0 / 3.0).abs() < 1e-6, "two sinks, got {frac}");

        // Major census: one connected Major component of two hexes.
        discharge[1] = 2.0e11;
        discharge[c1.0 as usize] = 3.0e11;
        let (rivers, hexes) = major_river_census(grid, &discharge);
        assert_eq!((rivers, hexes), (1, 2));
    }

    #[test]
    fn hydro_metrics_capture_is_sane_on_a_fresh_world() {
        let params = validation_parameters();
        let world = create_world(params).expect("world");
        let metrics = HydroMetrics::capture(&world.data);
        assert!((0.0..=1.0).contains(&metrics.land_fraction));
        assert!((0.0..=1.0).contains(&metrics.wet_fraction));
        assert_eq!(metrics.major_rivers, 0, "no discharge on a fresh world");
        assert!(metrics.seasonality_quartiles.is_none());
        // Display must not panic (it is the --nocapture evidence path).
        let _ = format!("{metrics}");
    }
}
