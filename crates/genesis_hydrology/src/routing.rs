//! The routing surface (Doc 08 §4.1, §4.3): priority-flood depression fill
//! (Barnes 2014, +epsilon), the depression tree that feeds §5, steepest-
//! descent flow directions, and the descending-order discharge accumulation.
//!
//! Pure derivation rebuilt every tick from `elevation_mean` and the flooding
//! solve's ocean/candidate masks — the fill lives in scratch buffers and never
//! touches `elevation_mean`. Deterministic: a `(level, HexId)`-keyed min-heap
//! and fixed iteration orders, no RNG.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};

use genesis_core::data::WorldData;
use genesis_core::grid::Direction;
use genesis_core::{HexGrid, HexId};

use crate::solve::CandidateSea;

/// Sentinel for "cell belongs to no depression / no candidate sea".
pub const NONE: u32 = u32::MAX;

/// Minimum connected area (hexes) for an enclosed depression to survive as a
/// real endorheic basin rather than being filled and routed through (§4.1
/// prior art; Earth-analog calibration — the Caspian/Aral class of basins).
pub const BASIN_MIN_HEXES: usize = 12;

/// Minimum depth below the spill point (m) for an enclosed depression to
/// survive as a real endorheic basin (§4.1 prior art).
pub const BASIN_MIN_DEPTH_M: f32 = 400.0;

/// Tiny gradient imposed on filled flats so routing stays monotone toward the
/// outlet (the "+epsilon" priority-flood variant).
pub const FILL_EPSILON_M: f32 = 1.0e-3;

/// Minimum fill raise (m) for a cell to count as a depression cell. Must far
/// exceed any accumulated +epsilon chain (path-length × [`FILL_EPSILON_M`]
/// stays well under a meter even at subdivision 8): without the threshold,
/// every flat plain would read as one giant depression and a single genuine
/// pit would flip the whole plain to "retained".
pub const DEPRESSION_MIN_RAISE_M: f32 = 1.0;

/// Mean hex area for this grid (m²). Cells are equal-area on the ISEA3H grid.
pub fn hex_area_m2(grid: &HexGrid) -> f64 {
    grid.hex_area_km2(HexId(0)) * 1.0e6
}

/// Min-heap node for the priority flood, ordered lowest level then lowest hex.
struct FillNode {
    level: f32,
    hex: u32,
}
impl PartialEq for FillNode {
    fn eq(&self, other: &Self) -> bool {
        self.level == other.level && self.hex == other.hex
    }
}
impl Eq for FillNode {}
impl Ord for FillNode {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max-heap; invert so the lowest level (then lowest
        // HexId) is popped first. Deterministic total order.
        other
            .level
            .total_cmp(&self.level)
            .then_with(|| other.hex.cmp(&self.hex))
    }
}
impl PartialOrd for FillNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// One retained depression — a node of the §5 depression tree.
#[derive(Clone, Debug)]
pub struct Depression {
    /// Lowest-elevation cell (tie: lowest `HexId`) — the lake's stable id.
    pub bottom: u32,
    /// Member cells in ascending order (cells the flood raised).
    pub cells: Vec<u32>,
    /// Land cell just outside the component the lake spills onto.
    pub spill_hex: u32,
    /// Overflow level: the filled elevation at the spill cell.
    pub spill_level_m: f32,
    /// Depression this one spills into (traced along flow directions from
    /// the spill cell); `None` when the spill reaches ocean or a candidate.
    pub parent: Option<u32>,
}

/// The per-tick routing surface and everything derived from the fill.
#[derive(Clone, Debug)]
pub struct RoutingSurface {
    /// Depression-filled scratch elevation (m). Plain cells carry the outer
    /// ocean-seeded fill; cells of a retained depression carry the basin-
    /// confined fill that grades strictly down to the basin bottom (§4.1/§5).
    pub filled_m: Vec<f32>,
    /// Downstream hex per land cell (steepest descent over the filled
    /// surface); `None` for water cells and retained basin bottoms.
    pub flow_target: Vec<Option<u32>>,
    /// Land cells in descending filled-elevation order (tie: ascending
    /// `HexId`) — the §4.3 accumulation order, upstream before downstream.
    pub order_desc: Vec<u32>,
    /// Retained depressions (the depression tree), ascending bottom `HexId`.
    pub depressions: Vec<Depression>,
    /// Cell → index into `depressions`, or [`NONE`].
    pub depression_of: Vec<u32>,
    /// Cell → index into the outcome's candidate list, or [`NONE`].
    pub candidate_of: Vec<u32>,
}

impl RoutingSurface {
    /// True when the cell is standing water this tick (ocean or candidate
    /// sea — the fill's closed boundary).
    pub fn is_water(&self, data: &WorldData, cell: u32) -> bool {
        self.candidate_of[cell as usize] != NONE
            || data.water_body_id[cell as usize] != genesis_core::WaterBodyId::NONE
    }

    /// Builds the routing surface from the post-solve world (§4.1).
    ///
    /// The flood's closed boundary is every wet cell — the written ocean
    /// mask plus the candidate-sea components (standing water awaiting §5.2
    /// adjudication) — so land drains honestly toward either.
    pub fn build(data: &WorldData, candidates: &[CandidateSea]) -> Self {
        let n = data.cell_count() as usize;
        let grid = &data.grid;
        let elev = &data.elevation_mean;

        let mut candidate_of = vec![NONE; n];
        for (index, candidate) in candidates.iter().enumerate() {
            for &cell in &candidate.cells {
                candidate_of[cell as usize] = index as u32;
            }
        }
        let is_closed = |i: usize| {
            candidate_of[i] != NONE || data.water_body_id[i] != genesis_core::WaterBodyId::NONE
        };

        // §4.1 priority flood (Barnes 2014, +epsilon): ocean + candidates are
        // the spill boundary. Seed the heap with every wet cell at its true
        // elevation. Pops are non-decreasing in level, so the first candidate
        // a cell receives is its minimum (standard Dijkstra argument); the
        // stale-entry skip covers decrease-key repushes.
        let mut filled = elev.clone();
        let mut closed = vec![false; n];
        let mut heap: BinaryHeap<FillNode> = BinaryHeap::new();
        for i in 0..n {
            if is_closed(i) {
                closed[i] = true;
                heap.push(FillNode {
                    level: filled[i],
                    hex: i as u32,
                });
            }
        }
        // No standing water anywhere (unit-test worlds; production only calls
        // build once standing water exists): drain the whole landmass to its
        // lowest cell, which then plays the role of a depression bottom.
        let mut sump: Option<u32> = None;
        if heap.is_empty() {
            let min_cell = (0..n as u32)
                .min_by(|&a, &b| {
                    elev[a as usize]
                        .total_cmp(&elev[b as usize])
                        .then_with(|| a.cmp(&b))
                })
                .expect("grid is non-empty");
            closed[min_cell as usize] = true;
            heap.push(FillNode {
                level: filled[min_cell as usize],
                hex: min_cell,
            });
            sump = Some(min_cell);
        }
        while let Some(FillNode { level, hex }) = heap.pop() {
            let i = hex as usize;
            // Stale heap entry: a lower path already won.
            if level > filled[i] {
                continue;
            }
            let neighbors = grid.neighbors_sorted(HexId(hex));
            for &neighbor in neighbors {
                let j = neighbor.0 as usize;
                if j >= n || is_closed(j) {
                    continue;
                }
                let new_level = elev[j].max(level + FILL_EPSILON_M);
                if !closed[j] || new_level < filled[j] {
                    closed[j] = true;
                    filled[j] = new_level;
                    heap.push(FillNode {
                        level: new_level,
                        hex: neighbor.0,
                    });
                }
            }
        }

        // Label connected components of raised (depression) cells, ascending.
        let depression_mask: Vec<bool> = (0..n)
            .map(|i| {
                !closed_is_water(i, &candidate_of, data)
                    && filled[i] > elev[i] + DEPRESSION_MIN_RAISE_M
            })
            .collect();
        let mut depressions: Vec<Depression> = Vec::new();
        let mut depression_of = vec![NONE; n];
        let mut visited = vec![false; n];
        for start in 0..n {
            if visited[start] || !depression_mask[start] {
                continue;
            }
            let mut component: Vec<u32> = Vec::new();
            let mut queue: VecDeque<u32> = VecDeque::new();
            visited[start] = true;
            queue.push_back(start as u32);
            while let Some(cell) = queue.pop_front() {
                component.push(cell);
                let neighbors = grid.neighbors_sorted(HexId(cell));
                for &neighbor in neighbors {
                    let j = neighbor.0 as usize;
                    if j < n && !visited[j] && depression_mask[j] {
                        visited[j] = true;
                        queue.push_back(neighbor.0);
                    }
                }
            }
            component.sort_unstable();

            let mut max_depth = 0.0_f32;
            let mut bottom = component[0];
            for &cell in &component {
                max_depth = max_depth.max(filled[cell as usize] - elev[cell as usize]);
                let (e_cell, e_bottom) = (elev[cell as usize], elev[bottom as usize]);
                if e_cell < e_bottom || (e_cell == e_bottom && cell < bottom) {
                    bottom = cell;
                }
            }
            if component.len() < BASIN_MIN_HEXES || max_depth < BASIN_MIN_DEPTH_M {
                continue; // filled and routed through — not a real basin.
            }

            // Retained basin: find the spill on the filled surface. If there
            // is no land outlet, leave the outer fill in place and route
            // through (no depression id is assigned).
            let mut spill: Option<(f32, u32)> = None;
            for &cell in &component {
                for neighbor in grid.neighbors(HexId(cell)) {
                    let j = neighbor.0 as usize;
                    if j >= n
                        || depression_mask[j]
                        || candidate_of[j] != NONE
                        || data.water_body_id[j] != genesis_core::WaterBodyId::NONE
                    {
                        continue;
                    }
                    let candidate = (filled[j], neighbor.0);
                    if spill.is_none_or(|best| candidate < best) {
                        spill = Some(candidate);
                    }
                }
            }
            let Some((spill_level_m, spill_hex)) = spill else {
                continue; // no land outlet: keep fill, route through.
            };
            let index = depressions.len() as u32;
            for &cell in &component {
                depression_of[cell as usize] = index;
            }
            depressions.push(Depression {
                bottom,
                cells: component,
                spill_hex,
                spill_level_m,
                parent: None,
            });
        }

        // Endorheic closure (§4.1/§5): within each retained depression, re-flood
        // from the basin bottom at its true elevation, so every cell drains
        // strictly downhill toward the bottom. The outer fill grades toward the
        // spill — left in place it would leak a closed basin out its own outlet.
        // One confined flood per basin, O(cells log cells) total; this replaces
        // the old revert-and-repair-sinks passes.
        for (index, depression) in depressions.iter().enumerate() {
            for &cell in &depression.cells {
                filled[cell as usize] = f32::INFINITY;
            }
            let bottom = depression.bottom as usize;
            filled[bottom] = elev[bottom];
            let mut heap = BinaryHeap::new();
            heap.push(FillNode {
                level: elev[bottom],
                hex: depression.bottom,
            });
            while let Some(FillNode { level, hex }) = heap.pop() {
                let i = hex as usize;
                if level > filled[i] {
                    continue; // stale entry: a lower path already won.
                }
                for &neighbor in grid.neighbors_sorted(HexId(hex)) {
                    let j = neighbor.0 as usize;
                    if j >= n || depression_of[j] != index as u32 {
                        continue;
                    }
                    let new_level = elev[j].max(level + FILL_EPSILON_M);
                    if new_level < filled[j] {
                        filled[j] = new_level;
                        heap.push(FillNode {
                            level: new_level,
                            hex: neighbor.0,
                        });
                    }
                }
            }
        }

        // Depression bottoms (only) may remain sinks; every other land cell
        // must drain. Track bottoms explicitly — basin side cells drain toward
        // the bottom on the confined fill. A boundary-less world drains to
        // its sump instead.
        let mut is_depression_bottom = vec![false; n];
        for depression in &depressions {
            is_depression_bottom[depression.bottom as usize] = true;
        }
        if let Some(s) = sump {
            is_depression_bottom[s as usize] = true;
        }

        // §4.3 flow directions: steepest descent over the filled surface,
        // ties to the lowest neighbor HexId. Water cells and depression
        // bottoms get None. Depression cells may only target cells of their
        // own basin — a closed basin must not leak through its outlet.
        let mut flow_target: Vec<Option<u32>> = vec![None; n];
        for i in 0..n {
            if closed_is_water(i, &candidate_of, data) || is_depression_bottom[i] {
                continue;
            }
            let own = filled[i];
            let own_depression = depression_of[i];
            let mut best: Option<(f32, u32)> = None;
            for neighbor in grid.neighbors(HexId(i as u32)) {
                let j = neighbor.0 as usize;
                if j >= n
                    || filled[j] >= own
                    || (own_depression != NONE && depression_of[j] != own_depression)
                {
                    continue;
                }
                let candidate = (filled[j], neighbor.0);
                if best.is_none_or(|b| candidate < b) {
                    best = Some(candidate);
                }
            }
            flow_target[i] = best.map(|(_, target)| target);
        }

        // By construction every non-water, non-bottom land cell now drains:
        // the outer flood leaves a strictly-lower neighbor for every plain
        // cell, and each confined flood does the same within its basin.
        debug_assert!(
            (0..n).all(|i| {
                closed_is_water(i, &candidate_of, data)
                    || is_depression_bottom[i]
                    || flow_target[i].is_some()
            }),
            "§4.1 routing surface left an orphan sink"
        );

        // Accumulation order: descending filled elevation, ascending HexId.
        let mut order_desc: Vec<u32> = (0..n as u32)
            .filter(|&i| !closed_is_water(i as usize, &candidate_of, data))
            .collect();
        order_desc.sort_by(|&a, &b| {
            filled[b as usize]
                .total_cmp(&filled[a as usize])
                .then_with(|| a.cmp(&b))
        });

        // Depression-tree parent links: trace the spill cell downstream until
        // it reaches water (parent None) or enters another depression.
        let mut surface = Self {
            filled_m: filled,
            flow_target,
            order_desc,
            depressions,
            depression_of,
            candidate_of,
        };
        let parents: Vec<Option<u32>> = surface
            .depressions
            .iter()
            .map(|depression| surface.trace_parent(depression.spill_hex))
            .collect();
        for (depression, parent) in surface.depressions.iter_mut().zip(parents) {
            depression.parent = parent;
        }
        surface
    }

    /// Follows flow targets from `start` until they reach water (`None`) or
    /// enter a depression (`Some(index)`). Step-capped for safety; the filled
    /// surface descends monotonically so the trace terminates.
    fn trace_parent(&self, start: u32) -> Option<u32> {
        let mut current = start;
        for _ in 0..self.flow_target.len() {
            let depression = self.depression_of[current as usize];
            if depression != NONE {
                return Some(depression);
            }
            current = self.flow_target[current as usize]?;
        }
        None
    }

    /// Writes `WorldData.flow_direction` from the resolved targets (the
    /// neighbor slot pointing at each cell's flow target).
    pub fn write_flow_directions(&self, data: &mut WorldData) {
        let n = data.cell_count() as usize;
        for i in 0..n {
            data.flow_direction[i] = match self.flow_target[i] {
                None => None,
                Some(target) => data
                    .grid
                    .neighbors(HexId(i as u32))
                    .iter()
                    .position(|neighbor| neighbor.0 == target)
                    .and_then(Direction::from_index),
            };
        }
    }
}

/// True when the cell is closed water (ocean mask or candidate sea).
fn closed_is_water(i: usize, candidate_of: &[u32], data: &WorldData) -> bool {
    candidate_of[i] != NONE || data.water_body_id[i] != genesis_core::WaterBodyId::NONE
}

/// Inflow bookkeeping for the §5 balance: discharge (surface + baseflow)
/// arriving at each retained depression's bottom and at each candidate sea.
#[derive(Clone, Debug, Default)]
pub struct FlowAccumulation {
    /// Total annual discharge per land cell, m³/yr (runoff + baseflow, §4.3).
    pub discharge_m3_yr: Vec<f64>,
    /// Accumulated baseflow-only discharge per land cell, m³/yr (§7.3's
    /// perennial test).
    pub baseflow_m3_yr: Vec<f64>,
    /// Accumulated groundwater recharge per land cell, m³/yr (§6.4's
    /// upstream-recharge-area proxy for springs).
    pub recharge_m3_yr: Vec<f64>,
    /// Upstream cell count per land cell (itself included).
    pub upstream_cells: Vec<f64>,
    /// Upstream iced-cell count per land cell (§7.1 Glacial regime).
    pub upstream_ice: Vec<f64>,
    /// Upstream temperature sums for basin-weighted regime climate (§7.1).
    pub temperature_sum: Vec<f64>,
    /// Upstream temperature-range sums (§7.1).
    pub temperature_range_sum: Vec<f64>,
    /// Discharge arriving at each depression's bottom (m³/yr), by index.
    pub depression_inflow_m3_yr: Vec<f64>,
    /// Discharge entering each candidate sea (m³/yr), by index.
    pub candidate_inflow_m3_yr: Vec<f64>,
}

impl FlowAccumulation {
    /// §4.3 accumulation: seeds every land cell with its runoff and baseflow,
    /// then passes totals downstream in descending filled-elevation order.
    /// Discharge entering ocean or a candidate sea leaves the network (the
    /// candidate credits it as inflow); discharge reaching a retained basin
    /// bottom is that depression's inflow.
    pub fn accumulate(
        data: &WorldData,
        surface: &RoutingSurface,
        runoff_seed_m3_yr: &[f64],
        baseflow_seed_m3_yr: &[f64],
        recharge_seed_m3_yr: &[f64],
    ) -> Self {
        let n = data.cell_count() as usize;
        let mut acc = Self {
            discharge_m3_yr: vec![0.0; n],
            baseflow_m3_yr: vec![0.0; n],
            recharge_m3_yr: vec![0.0; n],
            upstream_cells: vec![0.0; n],
            upstream_ice: vec![0.0; n],
            temperature_sum: vec![0.0; n],
            temperature_range_sum: vec![0.0; n],
            depression_inflow_m3_yr: vec![0.0; surface.depressions.len()],
            candidate_inflow_m3_yr: vec![0.0; 0],
        };
        for i in 0..n {
            if surface.is_water(data, i as u32) {
                continue;
            }
            acc.discharge_m3_yr[i] = runoff_seed_m3_yr[i] + baseflow_seed_m3_yr[i];
            acc.baseflow_m3_yr[i] = baseflow_seed_m3_yr[i];
            acc.recharge_m3_yr[i] = recharge_seed_m3_yr[i];
            acc.upstream_cells[i] = 1.0;
            acc.upstream_ice[i] = f64::from(data.ice_mask[i] as u8);
            acc.temperature_sum[i] = f64::from(data.temperature_mean[i]);
            acc.temperature_range_sum[i] = f64::from(data.temperature_range[i]);
        }
        // Size the candidate vector by the actual max index + 1.
        let candidate_count = surface
            .candidate_of
            .iter()
            .filter(|&&c| c != NONE)
            .map(|&c| c as usize + 1)
            .max()
            .unwrap_or(0);
        acc.candidate_inflow_m3_yr = vec![0.0; candidate_count];

        for &i in &surface.order_desc {
            let Some(target) = surface.flow_target[i as usize] else {
                // Retained basin bottom: the accumulated discharge is the
                // depression's entering flow (§5.1's I, before precipitation).
                let depression = surface.depression_of[i as usize];
                if depression != NONE {
                    acc.depression_inflow_m3_yr[depression as usize] +=
                        acc.discharge_m3_yr[i as usize];
                }
                continue;
            };
            let j = target as usize;
            let candidate = surface.candidate_of[j];
            if candidate != NONE {
                // Discharge entering a candidate sea leaves the network and
                // credits the §5.2 balance.
                acc.candidate_inflow_m3_yr[candidate as usize] += acc.discharge_m3_yr[i as usize];
                continue;
            }
            if surface.is_water(data, target) {
                continue; // discharges into the ocean; leaves the land system.
            }
            acc.discharge_m3_yr[j] += acc.discharge_m3_yr[i as usize];
            acc.baseflow_m3_yr[j] += acc.baseflow_m3_yr[i as usize];
            acc.recharge_m3_yr[j] += acc.recharge_m3_yr[i as usize];
            acc.upstream_cells[j] += acc.upstream_cells[i as usize];
            acc.upstream_ice[j] += acc.upstream_ice[i as usize];
            acc.temperature_sum[j] += acc.temperature_sum[i as usize];
            acc.temperature_range_sum[j] += acc.temperature_range_sum[i as usize];
        }
        acc
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{WorldYear, create_world};

    fn test_world() -> genesis_core::World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        world.data.current_year = WorldYear(1_000_000_000);
        world
    }

    /// Floods hex 0 as a one-cell ocean and leaves the rest a flat plain.
    /// Flat terrain routes through the fill's +epsilon gradient with no
    /// spurious local pits; tests sink their own basins explicitly.
    fn ramp_world() -> genesis_core::World {
        let mut world = test_world();
        let n = world.data.cell_count() as usize;
        for i in 0..n {
            world.data.elevation_mean[i] = 100.0;
        }
        world.data.elevation_mean[0] = -100.0;
        world.data.sea_level_m = 0.0;
        world.data.water_level_m[0] = 0.0;
        world.data.water_body_id[0] = genesis_core::WaterBodyId(0);
        world
    }

    #[test]
    fn flow_targets_point_downhill_on_the_filled_surface() {
        let world = ramp_world();
        let surface = RoutingSurface::build(&world.data, &[]);
        for i in 0..world.data.cell_count() {
            if let Some(target) = surface.flow_target[i as usize] {
                assert!(
                    surface.filled_m[target as usize] < surface.filled_m[i as usize],
                    "flow at hex {i} must descend the filled surface"
                );
            }
        }
    }

    #[test]
    fn water_cells_have_no_flow_target() {
        let world = ramp_world();
        let surface = RoutingSurface::build(&world.data, &[]);
        assert_eq!(surface.flow_target[0], None);
        assert!(surface.order_desc.iter().all(|&i| i != 0));
    }

    #[test]
    fn single_hex_pit_routes_through_the_fill() {
        let mut world = ramp_world();
        let pit = HexId(1000);
        assert!(
            world.data.grid.neighbors(pit).iter().all(|nb| nb.0 != 0),
            "test pit must not touch the ocean cell"
        );
        world.data.elevation_mean[pit.0 as usize] = 50.0; // below the plain
        let surface = RoutingSurface::build(&world.data, &[]);
        assert!(
            surface.flow_target[pit.0 as usize].is_some(),
            "a single-hex pit must drain through the depression fill"
        );
        assert!(surface.depressions.is_empty(), "one hex is no real basin");
    }

    #[test]
    fn large_deep_basin_is_retained_as_a_depression() {
        let mut world = ramp_world();
        // Grow a connected 20-cell blob far from the ocean and sink it 1000 m.
        let n = world.data.cell_count() as usize;
        let mut basin: Vec<u32> = vec![2000];
        let mut in_basin = vec![false; n];
        in_basin[2000] = true;
        let mut cursor = 0;
        while basin.len() < 20 {
            let cell = basin[cursor];
            for neighbor in world.data.grid.neighbors(HexId(cell)) {
                if !in_basin[neighbor.0 as usize] && neighbor.0 != 0 {
                    in_basin[neighbor.0 as usize] = true;
                    basin.push(neighbor.0);
                    if basin.len() >= 20 {
                        break;
                    }
                }
            }
            cursor += 1;
        }
        for &cell in &basin {
            world.data.elevation_mean[cell as usize] = -500.0; // below its rim
        }
        // Keep a rim of non-basin cells at ramp height so the pit is enclosed.
        let surface = RoutingSurface::build(&world.data, &[]);
        assert_eq!(surface.depressions.len(), 1, "the basin is retained");
        let depression = &surface.depressions[0];
        assert_eq!(depression.cells.len(), 20);
        assert_eq!(
            surface.flow_target[depression.bottom as usize], None,
            "the basin bottom is a retained sink"
        );
        for &cell in &depression.cells {
            assert_eq!(surface.depression_of[cell as usize], 0);
        }
    }

    #[test]
    fn accumulation_grows_downstream_and_credits_depressions() {
        let mut world = ramp_world();
        let n = world.data.cell_count() as usize;
        // Sink a 20-cell basin; its bottom must collect upstream discharge.
        let mut basin: Vec<u32> = vec![2000];
        let mut in_basin = vec![false; n];
        in_basin[2000] = true;
        let mut cursor = 0;
        while basin.len() < 20 {
            let cell = basin[cursor];
            for neighbor in world.data.grid.neighbors(HexId(cell)) {
                if !in_basin[neighbor.0 as usize] && neighbor.0 != 0 {
                    in_basin[neighbor.0 as usize] = true;
                    basin.push(neighbor.0);
                    if basin.len() >= 20 {
                        break;
                    }
                }
            }
            cursor += 1;
        }
        for &cell in &basin {
            world.data.elevation_mean[cell as usize] = -500.0;
        }
        let surface = RoutingSurface::build(&world.data, &[]);
        let seed = vec![1.0e6; n];
        let zeros = vec![0.0; n];
        let acc = FlowAccumulation::accumulate(&world.data, &surface, &seed, &zeros, &zeros);

        // Every land cell carries at least its own seed.
        for &i in &surface.order_desc {
            assert!(acc.discharge_m3_yr[i as usize] >= 1.0e6);
        }
        // The depression's inflow equals the discharge at its bottom and
        // covers all 20 basin cells plus any upstream land.
        assert_eq!(surface.depressions.len(), 1);
        let inflow = acc.depression_inflow_m3_yr[0];
        assert!(
            inflow >= 20.0 * 1.0e6,
            "basin inflow {inflow} must collect its cells"
        );
        // Discharge is non-decreasing along every flow edge (gate #4 shape).
        for &i in &surface.order_desc {
            if let Some(target) = surface.flow_target[i as usize]
                && !surface.is_water(&world.data, target)
                && surface.depression_of[target as usize] == NONE
            {
                assert!(
                    acc.discharge_m3_yr[target as usize] >= acc.discharge_m3_yr[i as usize],
                    "discharge must not drop along {i} -> {target}"
                );
            }
        }
    }

    #[test]
    fn build_is_deterministic() {
        let a = ramp_world();
        let b = ramp_world();
        let sa = RoutingSurface::build(&a.data, &[]);
        let sb = RoutingSurface::build(&b.data, &[]);
        assert_eq!(sa.filled_m, sb.filled_m);
        assert_eq!(sa.flow_target, sb.flow_target);
        assert_eq!(sa.order_desc, sb.order_desc);
    }

    #[test]
    fn modulo_pit_plateau_has_no_orphan_sinks() {
        // Same terrain shape as the layer gate: half-ocean basin + chaotic
        // plateau pits. Every land cell must drain or sit in a retained
        // depression (gate §15 #4).
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 7;
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        let mut basin = vec![false; n];
        let mut queue = std::collections::VecDeque::new();
        basin[0] = true;
        queue.push_back(0_u32);
        let mut count = 1;
        while count < n / 2 {
            let cell = queue.pop_front().expect("grid is connected");
            for neighbor in world.data.grid.neighbors(HexId(cell)) {
                let j = neighbor.0 as usize;
                if !basin[j] {
                    basin[j] = true;
                    count += 1;
                    queue.push_back(neighbor.0);
                    if count >= n / 2 {
                        break;
                    }
                }
            }
        }
        for (i, &in_basin) in basin.iter().enumerate() {
            world.data.elevation_mean[i] = if in_basin {
                -2000.0
            } else {
                500.0 + (i % 13) as f32 * 5.0
            };
        }
        world.data.sea_level_m = -3.8;
        for i in 0..n {
            if world.data.elevation_mean[i] < world.data.sea_level_m {
                world.data.water_body_id[i] = genesis_core::WaterBodyId(0);
            }
        }
        let surface = RoutingSurface::build(&world.data, &[]);
        let orphans = (0..n)
            .filter(|&i| {
                world.data.water_body_id[i] == genesis_core::WaterBodyId::NONE
                    && surface.depression_of[i] == NONE
                    && surface.flow_target[i].is_none()
            })
            .count();
        assert_eq!(
            orphans, 0,
            "no land cell may lack both a flow target and a depression"
        );
        // Depression bottoms may be sinks; every other land cell drains.
        for i in 0..n {
            if world.data.water_body_id[i] != genesis_core::WaterBodyId::NONE {
                continue;
            }
            if surface.flow_target[i].is_some() {
                continue;
            }
            let dep = surface.depression_of[i];
            assert_ne!(dep, NONE, "orphan land hex {i}");
            assert_eq!(
                surface.depressions[dep as usize].bottom, i as u32,
                "only depression bottoms may lack a flow target"
            );
        }
    }
}
