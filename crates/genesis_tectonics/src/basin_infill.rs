//! Closed-depression sediment infill on plate surfaces.
//!
//! Accreted / oceanic crust patches can sit as enclosed lows inside a
//! continental footprint. Heal skips them (`continental_crust == false`) and
//! gravitational collapse only shaves relief above 5 km, so ~1 km dry pits
//! persist forever. This pass fills those closed depressions toward their
//! spill level at a geological τ-rate, burying the floor as
//! [`BedrockType::Sedimentary`] without flipping the continental-crust flag.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use genesis_core::HexId;
use genesis_core::data::{BedrockType, WATER_NONE, WorldData};

use crate::plate::PlateRegistry;
use crate::plate_surface::modify_surface_at_world_hex;
use crate::projection::ProjectionCache;

/// Ignore sub-threshold dimples; only fill real pits (m).
pub const DEPRESSION_MIN_RAISE_M: f32 = 30.0;
/// Gap-closure e-folding time (years). Deep basins persist ~tens of My.
pub const INFILL_TAU_YEARS: f64 = 5_000_000.0;
/// Hard per-tick raise cap (m) so a single Ancient tick cannot cliff-fill.
pub const INFILL_MAX_M_PER_TICK: f32 = 400.0;
/// Monotone flood epsilon (m); mirrors hydrology routing.
pub const FILL_EPSILON_M: f32 = 1.0e-3;
/// Deepest a **dry** cell may sit below sea level before it is filled to its
/// spill in a single tick instead of the gradual rate (m). Earth's real floor
/// for dry land is the Dead Sea shore (~−430 m); nothing dry sits a kilometre
/// under the sea. Enclosed oceanic-crust scraps that the trench pass keeps
/// pulling toward the abyssal baseline every tick (faster than the gradual cap
/// recovers) are the offenders this clears. Wet basins (lakes, marginal seas)
/// are exempt — they persist and fill slowly.
pub const DRY_SUBSEA_MAX_DEPTH_M: f32 = 500.0;

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

/// Raise closed continental-interior depressions toward their ocean spill
/// level. Writes plate surfaces (persists across rebuilds).
///
/// `open_ocean` must mark the connected below-sea component(s) covering
/// ≥ [`crate::accretion::OPEN_OCEAN_MIN_FRACTION`] of cells — the same mask
/// accretion uses. Only those cells seed the priority flood; every interior
/// below-sea pit is therefore a fillable depression, not "ocean."
pub fn fill_closed_depressions(
    data: &mut WorldData,
    registry: &mut PlateRegistry,
    cache: &ProjectionCache,
    open_ocean: &[bool],
    tick_year: i64,
    interval_years: f64,
) {
    let n = data.cell_count() as usize;
    if n == 0 || open_ocean.len() < n || interval_years <= 0.0 {
        return;
    }

    let filled = priority_flood_spill(data, open_ocean);
    let Some(filled) = filled else {
        return;
    };

    let fraction = 1.0 - (-interval_years / INFILL_TAU_YEARS).exp();
    if fraction <= 0.0 {
        return;
    }
    let sea = data.sea_level_m;
    let dry_floor = sea - DRY_SUBSEA_MAX_DEPTH_M;

    for i in 0..n {
        if open_ocean[i] {
            continue;
        }
        if data.ice_load_m.get(i).copied().unwrap_or(0.0) > 0.0 {
            continue;
        }
        let elev = data.elevation_mean[i];
        let gap = filled[i] - elev;
        if gap <= DEPRESSION_MIN_RAISE_M {
            continue;
        }
        let water = data.water_level_m.get(i).copied().unwrap_or(WATER_NONE);
        let wet = water.is_finite() && water > elev;
        // A dry cell far below sea is unphysical: the trench pass keeps
        // dragging these enclosed oceanic scraps toward the abyssal baseline
        // faster than the gradual cap recovers, so they sit dry a kilometre
        // under the sea forever. Fill them flush to their spill (the lowest
        // rim) in one tick. Wet basins and cells within Dead-Sea range of sea
        // level keep the gradual, cap-limited rate so real lakes and large
        // basins still evolve slowly.
        let new_elev = if !wet && elev < dry_floor {
            filled[i]
        } else {
            let raise = ((gap as f64) * fraction).min(f64::from(INFILL_MAX_M_PER_TICK)) as f32;
            if raise <= 0.0 {
                continue;
            }
            elev + raise
        };
        modify_surface_at_world_hex(registry, data, cache, HexId(i as u32), tick_year, |f| {
            f.elevation_m = new_elev;
            f.bedrock = BedrockType::Sedimentary;
        });
        data.elevation_mean[i] = new_elev;
        if i < data.bedrock_type.len() {
            data.bedrock_type[i] = BedrockType::Sedimentary;
        }
    }
}

/// Barnes 2014 priority flood (+epsilon). Returns spill/fill elevation per
/// cell, or `None` when there is no open-ocean seed (nothing to drain toward).
fn priority_flood_spill(data: &WorldData, open_ocean: &[bool]) -> Option<Vec<f32>> {
    let n = data.cell_count() as usize;
    let grid = &data.grid;
    let elev = &data.elevation_mean;

    let mut filled = elev.clone();
    let mut flooded = vec![false; n];
    let mut heap: BinaryHeap<FillNode> = BinaryHeap::new();

    for i in 0..n {
        if !open_ocean[i] {
            continue;
        }
        flooded[i] = true;
        heap.push(FillNode {
            level: filled[i],
            hex: i as u32,
        });
    }
    if heap.is_empty() {
        return None;
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
            if j >= n || open_ocean[j] {
                continue;
            }
            let new_level = elev[j].max(level + FILL_EPSILON_M);
            if !flooded[j] || new_level < filled[j] {
                flooded[j] = true;
                filled[j] = new_level;
                heap.push(FillNode {
                    level: new_level,
                    hex: neighbor.0,
                });
            }
        }
    }

    Some(filled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accretion::label_water_components;
    use crate::history::run_formation;
    use crate::plate::TectonicsState;
    use genesis_core::World;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{HexId, create_world};

    const GEOLOGICAL_YEARS: f64 = 500_000.0;

    fn formed_world() -> (World, TectonicsState) {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        let mut state = TectonicsState::new();
        run_formation(&mut world, &mut state);
        let n = world.data.cell_count() as usize;
        if world.data.ice_load_m.len() < n {
            world.data.ice_load_m = vec![0.0; n];
        }
        (world, state)
    }

    /// Land hex with ≥5 neighbors, not open ocean, suitable for a closed pit.
    fn pick_interior_land(data: &WorldData, open_ocean: &[bool]) -> usize {
        let n = data.cell_count() as usize;
        (0..n)
            .find(|&i| {
                !open_ocean[i]
                    && data.elevation_mean[i] > data.sea_level_m
                    && data.grid.neighbors(HexId(i as u32)).len() >= 5
                    && data
                        .grid
                        .neighbors(HexId(i as u32))
                        .iter()
                        .all(|nb| !open_ocean[nb.0 as usize])
            })
            .expect("interior land hex")
    }

    fn plant_pit(
        world: &mut World,
        registry: &mut PlateRegistry,
        cache: &ProjectionCache,
        pit: usize,
        depth_below_rim_m: f32,
    ) -> f32 {
        let rim = world
            .data
            .grid
            .neighbors(HexId(pit as u32))
            .iter()
            .map(|nb| world.data.elevation_mean[nb.0 as usize])
            .fold(f32::MAX, f32::min);
        let deep = rim - depth_below_rim_m;
        world.data.elevation_mean[pit] = deep;
        world.data.ice_load_m[pit] = 0.0;
        modify_surface_at_world_hex(registry, &world.data, cache, HexId(pit as u32), 0, |f| {
            f.elevation_m = deep;
        });
        deep
    }

    fn run_infill(world: &mut World, state: &mut TectonicsState, interval: f64) {
        let water = label_water_components(&world.data);
        let mask = water.open_ocean_mask();
        fill_closed_depressions(
            &mut world.data,
            &mut state.registry,
            &state.projection,
            &mask,
            1_000_000_000,
            interval,
        );
    }

    #[test]
    fn wet_pit_rises_gradually_toward_rim() {
        let (mut world, mut state) = formed_world();
        let water = label_water_components(&world.data);
        let mask = water.open_ocean_mask();
        let pit = pick_interior_land(&world.data, &mask);
        let deep = plant_pit(
            &mut world,
            &mut state.registry,
            &state.projection,
            pit,
            1_200.0,
        );
        // Wet basin (a lake bed): sediment fills it at the gradual, capped rate.
        world.data.water_level_m[pit] = deep + 1.0;

        run_infill(&mut world, &mut state, GEOLOGICAL_YEARS);
        let after = world.data.elevation_mean[pit];
        assert!(
            after > deep + 20.0,
            "wet pit bed should rise; was {deep}, now {after}"
        );
        assert!(
            after < deep + INFILL_MAX_M_PER_TICK + 1.0,
            "wet basin must respect the gradual cap; now {after}"
        );
        assert_eq!(world.data.bedrock_type[pit], BedrockType::Sedimentary);
    }

    #[test]
    fn deep_dry_pit_filled_to_spill_in_one_tick() {
        let (mut world, mut state) = formed_world();
        let water = label_water_components(&world.data);
        let mask = water.open_ocean_mask();
        let pit = pick_interior_land(&world.data, &mask);
        // Plant the floor to an absolute target well below the dry floor
        // (Dead Sea, the real floor for dry land, is ~-430 m), and keep it dry.
        let sea = world.data.sea_level_m;
        let rim = world
            .data
            .grid
            .neighbors(HexId(pit as u32))
            .iter()
            .map(|nb| world.data.elevation_mean[nb.0 as usize])
            .fold(f32::MAX, f32::min);
        let depth = rim - (sea - 1_500.0);
        let deep = plant_pit(&mut world, &mut state.registry, &state.projection, pit, depth);
        world.data.water_level_m[pit] = WATER_NONE;
        assert!(
            deep < sea - DRY_SUBSEA_MAX_DEPTH_M,
            "test needs a dry pit below the dry floor; deep={deep}, sea={sea}"
        );

        let filled = priority_flood_spill(&world.data, &mask).expect("open ocean");
        run_infill(&mut world, &mut state, GEOLOGICAL_YEARS);
        let after = world.data.elevation_mean[pit];
        assert!(
            after > deep + INFILL_MAX_M_PER_TICK,
            "dry sub-sea pit must snap past the gradual cap; was {deep}, now {after}"
        );
        assert!(
            (after - filled[pit]).abs() < 1.0,
            "dry sub-sea pit must reach its spill {}; now {after}",
            filled[pit]
        );
        assert!(
            after >= world.data.sea_level_m,
            "a land-rimmed dry pit must end at or above sea; now {after} vs sea {}",
            world.data.sea_level_m
        );
    }

    #[test]
    fn mountain_open_slope_untouched() {
        let (mut world, mut state) = formed_world();
        let water = label_water_components(&world.data);
        let mask = water.open_ocean_mask();
        let mountain = pick_interior_land(&world.data, &mask);
        let peak = world.data.sea_level_m + 4_000.0;
        world.data.elevation_mean[mountain] = peak;
        world.data.ice_load_m[mountain] = 0.0;
        modify_surface_at_world_hex(
            &mut state.registry,
            &world.data,
            &state.projection,
            HexId(mountain as u32),
            0,
            |f| {
                f.elevation_m = peak;
            },
        );

        run_infill(&mut world, &mut state, GEOLOGICAL_YEARS);
        assert!(
            (world.data.elevation_mean[mountain] - peak).abs() < 1.0,
            "open high ground must not enter the fill cohort"
        );
    }

    #[test]
    fn ice_loaded_pit_skipped() {
        let (mut world, mut state) = formed_world();
        let water = label_water_components(&world.data);
        let mask = water.open_ocean_mask();
        let pit = pick_interior_land(&world.data, &mask);
        let deep = plant_pit(
            &mut world,
            &mut state.registry,
            &state.projection,
            pit,
            1_200.0,
        );
        world.data.ice_load_m[pit] = 250.0;

        run_infill(&mut world, &mut state, GEOLOGICAL_YEARS);
        assert!(
            (world.data.elevation_mean[pit] - deep).abs() < 1e-3,
            "ice-loaded hex must be left for GIA"
        );
    }

    #[test]
    fn infill_is_deterministic() {
        let (mut a, mut state_a) = formed_world();
        let (mut b, mut state_b) = formed_world();
        let water = label_water_components(&a.data);
        let mask = water.open_ocean_mask();
        let pit = pick_interior_land(&a.data, &mask);
        for (world, state) in [(&mut a, &mut state_a), (&mut b, &mut state_b)] {
            plant_pit(world, &mut state.registry, &state.projection, pit, 1_500.0);
        }
        run_infill(&mut a, &mut state_a, GEOLOGICAL_YEARS);
        run_infill(&mut b, &mut state_b, GEOLOGICAL_YEARS);
        assert_eq!(a.data.elevation_mean, b.data.elevation_mean);
    }

    #[test]
    fn deep_wet_basin_not_filled_in_one_geological_tick() {
        let (mut world, mut state) = formed_world();
        let water = label_water_components(&world.data);
        let mask = water.open_ocean_mask();
        let pit = pick_interior_land(&world.data, &mask);
        let deep = plant_pit(
            &mut world,
            &mut state.registry,
            &state.projection,
            pit,
            1_800.0,
        );
        // A deep lake/marginal sea: wet, so it evolves at the gradual cap and
        // is not snap-filled — Caspian-scale basins persist for tens of My.
        world.data.water_level_m[pit] = deep + 1.0;

        let filled = priority_flood_spill(&world.data, &mask).expect("open ocean");
        let gap = filled[pit] - deep;
        assert!(
            gap > INFILL_MAX_M_PER_TICK + 50.0,
            "test needs a deep gap; got {gap}"
        );

        run_infill(&mut world, &mut state, GEOLOGICAL_YEARS);
        let after = world.data.elevation_mean[pit];
        let raised = after - deep;
        assert!(
            raised <= INFILL_MAX_M_PER_TICK + 1e-3,
            "raise {raised} must not exceed cap {}",
            INFILL_MAX_M_PER_TICK
        );
        assert!(
            after < filled[pit] - DEPRESSION_MIN_RAISE_M,
            "deep wet basin must remain a depression after one Geological tick"
        );
    }
}
