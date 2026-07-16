//! Surface water flow: directions and accumulated discharge (Doc 08 scope).
//!
//! Surface water is routed over a *depression-filled* copy of the land surface
//! (Barnes 2014 priority-flood): spurious single-hex and small pits — abundant
//! on a fine hex grid — are flooded so water flows continuously to the sea
//! instead of dead-ending everywhere. Only genuinely large, deep enclosed
//! basins are kept as endorheic sinks (real lakes). The tectonic
//! `elevation_mean` field is never mutated; the fill lives in a scratch buffer.
//! Deterministic: a `(level, HexId)`-keyed min-heap and fixed iteration orders,
//! no RNG.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};

use genesis_core::data::WorldData;
use genesis_core::grid::Direction;
use genesis_core::{HexGrid, HexId};

/// Fraction of annual precipitation that becomes surface runoff (the rest
/// evaporates or infiltrates). Earth land average is ~0.35–0.45.
pub const RUNOFF_COEFFICIENT: f64 = 0.4;

/// Minimum connected area (hexes) for an enclosed depression to survive as a
/// real endorheic lake rather than being filled and routed through.
pub const BASIN_MIN_HEXES: usize = 12;

/// Minimum depth below the spill point (m) for an enclosed depression to
/// survive as a real endorheic lake. Mirrors the tectonic geologic-lake depth.
pub const BASIN_MIN_DEPTH_M: f32 = 400.0;

/// Tiny gradient imposed on filled flats so routing stays monotone toward the
/// outlet (the "+epsilon" priority-flood variant).
const FILL_EPSILON_M: f32 = 1.0e-3;

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
            .partial_cmp(&self.level)
            .unwrap_or(Ordering::Equal)
            .then_with(|| other.hex.cmp(&self.hex))
    }
}
impl PartialOrd for FillNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Priority-flood the land surface toward the ocean spill boundary. Returns a
/// filled-elevation scratch buffer and a `retained_sink` mask marking the
/// bottoms of genuine (large, deep) basins that stay endorheic. Ocean cells
/// keep their true elevation; land pits are raised to their spill level so
/// every non-basin land hex has a monotone downhill path to the sea.
fn depression_filled_land(data: &WorldData) -> (Vec<f32>, Vec<bool>) {
    let n = data.cell_count() as usize;
    let sea = data.sea_level_m;
    let grid = &data.grid;
    let elev = &data.elevation_mean;

    let mut filled = elev.clone();
    let mut closed = vec![false; n];
    let mut heap: BinaryHeap<FillNode> = BinaryHeap::new();

    // Ocean is the spill boundary: mark it closed at true elevation.
    for i in 0..n {
        if elev[i] < sea {
            closed[i] = true;
        }
    }
    // Seed the flood with land cells that touch the ocean (the outlets).
    for i in 0..n {
        if closed[i] {
            continue;
        }
        let hex = HexId(i as u32);
        let touches_ocean = grid
            .neighbors(hex)
            .iter()
            .any(|nb| (nb.0 as usize) < n && elev[nb.0 as usize] < sea);
        if touches_ocean {
            closed[i] = true;
            filled[i] = elev[i];
            heap.push(FillNode {
                level: filled[i],
                hex: i as u32,
            });
        }
    }

    while let Some(FillNode { level, hex }) = heap.pop() {
        let mut nbrs: Vec<HexId> = grid.neighbors(HexId(hex)).to_vec();
        nbrs.sort_by_key(|h| h.0);
        for nb in nbrs {
            let j = nb.0 as usize;
            if j >= n || closed[j] {
                continue;
            }
            closed[j] = true;
            filled[j] = elev[j].max(level + FILL_EPSILON_M);
            heap.push(FillNode {
                level: filled[j],
                hex: nb.0,
            });
        }
    }

    // Keep genuinely large, deep depressions as endorheic lakes; flatten the
    // rest so they route through. A depression cell is one the flood raised;
    // snapshot the mask up front so the retention pass can mutate `filled`.
    let depression: Vec<bool> = (0..n)
        .map(|i| filled[i] > elev[i] + FILL_EPSILON_M * 0.5)
        .collect();
    let mut retained_sink = vec![false; n];
    let mut visited = vec![false; n];
    for start in 0..n {
        if visited[start] || !depression[start] {
            continue;
        }
        let mut component: Vec<usize> = Vec::new();
        let mut queue: VecDeque<usize> = VecDeque::new();
        visited[start] = true;
        queue.push_back(start);
        while let Some(c) = queue.pop_front() {
            component.push(c);
            let mut nbrs: Vec<HexId> = grid.neighbors(HexId(c as u32)).to_vec();
            nbrs.sort_by_key(|h| h.0);
            for nb in nbrs {
                let j = nb.0 as usize;
                if j < n && !visited[j] && depression[j] {
                    visited[j] = true;
                    queue.push_back(j);
                }
            }
        }
        let mut max_depth = 0.0_f32;
        let mut bottom = component[0];
        for &c in &component {
            max_depth = max_depth.max(filled[c] - elev[c]);
            if elev[c] < elev[bottom] || (elev[c] == elev[bottom] && c < bottom) {
                bottom = c;
            }
        }
        if component.len() >= BASIN_MIN_HEXES && max_depth >= BASIN_MIN_DEPTH_M {
            // Real lake: revert to true elevation so water pools at the bottom.
            for &c in &component {
                filled[c] = elev[c];
            }
            retained_sink[bottom] = true;
        }
    }

    (filled, retained_sink)
}

/// Runoff assumed when the climate layer has not populated precipitation
/// (tectonics-only worlds), mm/year.
pub const DEFAULT_PRECIPITATION_MM: f32 = 800.0;

/// Mean hex area for this grid (m²).
pub fn hex_area_m2(grid: &HexGrid) -> f64 {
    let radius_m = grid.planet_radius_km() * 1000.0;
    4.0 * std::f64::consts::PI * radius_m * radius_m / f64::from(grid.cell_count())
}

/// Writes `WorldData.flow_direction`: steepest-descent neighbor slot for land
/// hexes over the depression-filled surface, `None` for ocean hexes and
/// retained endorheic basin bottoms. Ties break toward the lowest neighbor
/// `HexId`. Routing over the filled surface means spurious pits drain through
/// to the sea instead of dead-ending.
pub fn compute_flow_directions(data: &mut WorldData) {
    let n = data.cell_count() as usize;
    let sea = data.sea_level_m;
    let (filled, retained_sink) = depression_filled_land(data);

    for i in 0..n {
        let hex = HexId(i as u32);
        if data.elevation_mean[i] < sea || retained_sink[i] {
            data.flow_direction[i] = None;
            continue;
        }

        let own_elevation = filled[i];
        let mut best: Option<(Direction, f32, HexId)> = None;
        for (slot, &neighbor) in data.grid.neighbors(hex).iter().enumerate() {
            let j = neighbor.0 as usize;
            if j >= n {
                continue;
            }
            let neighbor_elevation = filled[j];
            if neighbor_elevation >= own_elevation {
                continue;
            }
            let Some(direction) = Direction::from_index(slot) else {
                continue;
            };
            let replace = match &best {
                None => true,
                Some((_, best_elevation, best_id)) => {
                    neighbor_elevation < *best_elevation
                        || (neighbor_elevation == *best_elevation && neighbor < *best_id)
                }
            };
            if replace {
                best = Some((direction, neighbor_elevation, neighbor));
            }
        }
        data.flow_direction[i] = best.map(|(direction, _, _)| direction);
    }
}

/// Writes `WorldData.flow_volume` (m³/year): per-hex runoff accumulated along
/// flow directions. Land hexes are processed in descending elevation order
/// (ties: ascending `HexId`) so upstream volume is complete before it passes
/// downstream. Ocean hexes carry no flow volume.
pub fn compute_flow_accumulation(data: &mut WorldData) {
    let n = data.cell_count() as usize;
    let sea = data.sea_level_m;
    let area_m2 = hex_area_m2(&data.grid);
    let climate_active = data.precipitation.iter().any(|&p| p > 0.0);
    let (filled, _retained_sink) = depression_filled_land(data);

    // Seed with local runoff.
    for i in 0..n {
        if data.elevation_mean[i] < sea {
            data.flow_volume[i] = 0.0;
            continue;
        }
        let precip_mm = if climate_active {
            data.precipitation[i]
        } else {
            DEFAULT_PRECIPITATION_MM
        };
        // mm/year × m² × 1e-3 → m³/year.
        data.flow_volume[i] = (f64::from(precip_mm) * area_m2 * 1e-3 * RUNOFF_COEFFICIENT) as f32;
    }

    // Descending FILLED elevation, ascending id: upstream-before-downstream on
    // the routed surface, matching the flow directions.
    let mut order: Vec<usize> = (0..n).filter(|&i| data.elevation_mean[i] >= sea).collect();
    order.sort_by(|&a, &b| {
        filled[b]
            .partial_cmp(&filled[a])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.cmp(&b))
    });

    for &i in &order {
        let Some(direction) = data.flow_direction[i] else {
            continue;
        };
        let hex = HexId(i as u32);
        let neighbors = data.grid.neighbors(hex);
        let Some(&downstream) = neighbors.get(direction.index()) else {
            continue;
        };
        let j = downstream.0 as usize;
        if j >= n || data.elevation_mean[j] < sea {
            // Discharges into the ocean; volume leaves the land system.
            continue;
        }
        data.flow_volume[j] += data.flow_volume[i];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::create_world;
    use genesis_core::parameters::WorldParameters;

    fn test_world() -> genesis_core::World {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        create_world(params).expect("world")
    }

    #[test]
    fn flow_directions_point_downhill() {
        let mut world = test_world();
        let n = world.data.cell_count() as usize;
        for i in 0..n {
            world.data.elevation_mean[i] = (i % 100) as f32 * 10.0 + 1.0;
        }
        world.data.sea_level_m = 0.0;
        compute_flow_directions(&mut world.data);

        for i in 0..n {
            let hex = HexId(i as u32);
            if let Some(direction) = world.data.flow_direction[i] {
                let neighbor = world.data.grid.neighbors(hex)[direction.index()];
                assert!(
                    world.data.elevation_mean[neighbor.0 as usize] < world.data.elevation_mean[i],
                    "flow at hex {i} must point downhill"
                );
            }
        }
    }

    #[test]
    fn ocean_hexes_have_no_flow() {
        let mut world = test_world();
        let n = world.data.cell_count() as usize;
        for i in 0..n {
            world.data.elevation_mean[i] = -1000.0;
        }
        world.data.sea_level_m = 0.0;
        compute_flow_directions(&mut world.data);
        compute_flow_accumulation(&mut world.data);
        for i in 0..n {
            assert!(world.data.flow_direction[i].is_none());
            assert_eq!(world.data.flow_volume[i], 0.0);
        }
    }

    #[test]
    fn accumulation_grows_downstream() {
        let mut world = test_world();
        let n = world.data.cell_count() as usize;
        // A single tall peak at hex 100 sloping to its neighbors: neighbors
        // must accumulate the peak's runoff on top of their own.
        for i in 0..n {
            world.data.elevation_mean[i] = 100.0;
        }
        let peak = HexId(100);
        world.data.elevation_mean[peak.0 as usize] = 2000.0;
        let downhill = world.data.grid.neighbors(peak)[0];
        world.data.elevation_mean[downhill.0 as usize] = 50.0;
        world.data.sea_level_m = 0.0;

        compute_flow_directions(&mut world.data);
        compute_flow_accumulation(&mut world.data);

        let base_runoff = (f64::from(DEFAULT_PRECIPITATION_MM)
            * hex_area_m2(&world.data.grid)
            * 1e-3
            * RUNOFF_COEFFICIENT) as f32;
        let downstream_volume = world.data.flow_volume[downhill.0 as usize];
        assert!(
            downstream_volume > base_runoff * 1.5,
            "downstream hex should carry upstream volume: {downstream_volume} vs {base_runoff}"
        );
    }

    #[test]
    fn accumulation_uses_precipitation_when_climate_active() {
        let mut world = test_world();
        let n = world.data.cell_count() as usize;
        for i in 0..n {
            world.data.elevation_mean[i] = 100.0;
            world.data.precipitation[i] = if i == 0 { 2400.0 } else { 800.0 };
        }
        world.data.sea_level_m = 0.0;
        compute_flow_directions(&mut world.data);
        compute_flow_accumulation(&mut world.data);
        assert!(
            world.data.flow_volume[0] > world.data.flow_volume[1] * 2.5,
            "3x precipitation should give ~3x runoff"
        );
    }

    #[test]
    fn flow_is_deterministic() {
        let mut a = test_world();
        let mut b = test_world();
        for world in [&mut a, &mut b] {
            let n = world.data.cell_count() as usize;
            for i in 0..n {
                world.data.elevation_mean[i] = ((i * 37) % 500) as f32 - 100.0;
            }
            world.data.sea_level_m = 0.0;
            compute_flow_directions(&mut world.data);
            compute_flow_accumulation(&mut world.data);
        }
        assert_eq!(a.data.flow_direction, b.data.flow_direction);
        assert_eq!(a.data.flow_volume, b.data.flow_volume);
    }

    #[test]
    fn single_hex_pit_routes_through_after_depression_fill() {
        // A lone pit (below all its neighbors) on land with an ocean outlet
        // must drain through the priority-flood fill rather than dead-ending as
        // an endorheic sink — a single hex is far below BASIN_MIN_HEXES.
        let mut world = test_world();
        let n = world.data.cell_count() as usize;
        for i in 0..n {
            world.data.elevation_mean[i] = 100.0;
        }
        world.data.sea_level_m = 0.0;
        world.data.elevation_mean[0] = -100.0; // an ocean outlet

        let pit = HexId(1000);
        assert!(
            world.data.grid.neighbors(pit).iter().all(|nb| nb.0 != 0),
            "test pit must not be adjacent to the ocean outlet"
        );
        world.data.elevation_mean[pit.0 as usize] = 50.0;

        compute_flow_directions(&mut world.data);
        assert!(
            world.data.flow_direction[pit.0 as usize].is_some(),
            "a single-hex pit must drain through the depression fill"
        );
    }
}
