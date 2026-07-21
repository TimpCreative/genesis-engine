//! Deep-time continental pit heal and sub-freeboard lift.
//!
//! Formation already fills single-hex noise pits once. Over Gyr, incision (now
//! floored), GIA, rifts, and sutures still leave charcoal dry pits and drowned
//! freeboard crust. This pass raises those artifacts without a global smooth.

use genesis_core::HexId;
use genesis_core::data::{WATER_NONE, WorldData};

use crate::erosion::CONTINENTAL_FREEBOARD_M;
use crate::plate::PlateRegistry;
use crate::plate_surface::modify_surface_at_world_hex;
use crate::projection::ProjectionCache;

/// Elevation drop below the lowest neighbor that marks a lone pit (m).
pub const PIT_DEPTH_THRESHOLD_M: f32 = 150.0;
/// Residual depth left when a pit is raised toward neighbors (m).
pub const PIT_FILL_MARGIN_M: f32 = 50.0;
/// How far below freeboard a dry continental hex must sit to enter the lift cohort (m).
pub const HEAL_DEPTH_TRIGGER_M: f32 = 200.0;
/// Fraction of the freeboard gap closed per Geological tick for deep pits.
pub const HEAL_LIFT_FRACTION_PER_TICK: f32 = 0.35;
/// Hard per-tick lift cap (m) so recovery cannot explode.
pub const HEAL_MAX_LIFT_M_PER_TICK: f32 = 150.0;

/// Pass A (single-hex pits) then Pass B (sub-freeboard lift) on continental crust.
///
/// Deterministic: hexes visited in ascending `HexId` order. Neighbor reads for
/// Pass A use a pre-pass elevation snapshot so order does not cascade.
pub fn heal_continental_surface(
    data: &mut WorldData,
    registry: &mut PlateRegistry,
    cache: &ProjectionCache,
    tick_year: i64,
) {
    fill_single_hex_continental_pits(data, registry, cache, tick_year);
    lift_sub_freeboard_continental(data, registry, cache, tick_year);
}

fn fill_single_hex_continental_pits(
    data: &mut WorldData,
    registry: &mut PlateRegistry,
    cache: &ProjectionCache,
    tick_year: i64,
) {
    let n = data.cell_count() as usize;
    let snapshot: Vec<f32> = data.elevation_mean.clone();
    let mut raises: Vec<(u32, f32)> = Vec::new();

    for i in 0..n {
        if !data.continental_crust.get(i).copied().unwrap_or(false) {
            continue;
        }
        if data.ice_load_m.get(i).copied().unwrap_or(0.0) > 0.0 {
            continue;
        }
        let elev = snapshot[i];
        let mut min_neighbor = f32::MAX;
        for neighbor in data.grid.neighbors(HexId(i as u32)) {
            let j = neighbor.0 as usize;
            if j < n {
                min_neighbor = min_neighbor.min(snapshot[j]);
            }
        }
        if min_neighbor < f32::MAX && elev < min_neighbor - PIT_DEPTH_THRESHOLD_M {
            raises.push((i as u32, min_neighbor - PIT_FILL_MARGIN_M));
        }
    }

    for (hex, new_elev) in raises {
        modify_surface_at_world_hex(registry, data, cache, HexId(hex), tick_year, |feature| {
            feature.elevation_m = new_elev;
        });
        let idx = hex as usize;
        if idx < data.elevation_mean.len() {
            data.elevation_mean[idx] = new_elev;
        }
    }
}

fn lift_sub_freeboard_continental(
    data: &mut WorldData,
    registry: &mut PlateRegistry,
    cache: &ProjectionCache,
    tick_year: i64,
) {
    let n = data.cell_count() as usize;
    let sea = data.sea_level_m;
    let mut lifts: Vec<(u32, f32)> = Vec::new();

    for i in 0..n {
        if !data.continental_crust.get(i).copied().unwrap_or(false) {
            continue;
        }
        if data.ice_load_m.get(i).copied().unwrap_or(0.0) > 0.0 {
            continue;
        }
        let elev = data.elevation_mean[i];
        let water = data.water_level_m.get(i).copied().unwrap_or(WATER_NONE);
        if water > elev && water.is_finite() {
            continue;
        }
        // Doc 06 §5.12: heal lifts toward the swell-shifted freeboard, so
        // epeirogenic basins are honored, not healed away.
        let target = sea
            + CONTINENTAL_FREEBOARD_M
            + crate::epeirogeny::target_offset_at_world(data, registry, cache, HexId(i as u32));
        if elev >= target - HEAL_DEPTH_TRIGGER_M {
            continue;
        }
        // Isolated ocean-surrounded fragments that heal re-floats here are
        // removed afterward by the display de-speckle
        // ([`crate::coast_cleanup::despeckle_mask`]), so heal lifts drowned
        // continental crust freely (interior and shelf) to hold land fraction.
        let gap = target - elev;
        let lift = (gap * HEAL_LIFT_FRACTION_PER_TICK).min(HEAL_MAX_LIFT_M_PER_TICK);
        if lift > 0.0 {
            lifts.push((i as u32, elev + lift));
        }
    }

    for (hex, new_elev) in lifts {
        modify_surface_at_world_hex(registry, data, cache, HexId(hex), tick_year, |feature| {
            feature.elevation_m = new_elev;
        });
        let idx = hex as usize;
        if idx < data.elevation_mean.len() {
            data.elevation_mean[idx] = new_elev;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::run_formation;
    use crate::plate::TectonicsState;
    use genesis_core::World;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{HexId, create_world};

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

    #[test]
    fn single_hex_pit_is_raised() {
        let (mut world, mut state) = formed_world();
        let n = world.data.cell_count() as usize;
        let pit = (0..n)
            .find(|&i| {
                world.data.continental_crust[i]
                    && world.data.elevation_mean[i] > world.data.sea_level_m
                    && world.data.grid.neighbors(HexId(i as u32)).len() >= 5
            })
            .expect("continental hex");
        let min_n = world
            .data
            .grid
            .neighbors(HexId(pit as u32))
            .iter()
            .map(|nb| world.data.elevation_mean[nb.0 as usize])
            .fold(f32::MAX, f32::min);
        let deep = min_n - PIT_DEPTH_THRESHOLD_M - 100.0;
        world.data.elevation_mean[pit] = deep;
        world.data.ice_load_m[pit] = 0.0;
        world.data.water_level_m[pit] = WATER_NONE;
        modify_surface_at_world_hex(
            &mut state.registry,
            &world.data,
            &state.projection,
            HexId(pit as u32),
            0,
            |f| {
                f.elevation_m = deep;
            },
        );

        heal_continental_surface(
            &mut world.data,
            &mut state.registry,
            &state.projection,
            1_000_000_000,
        );
        assert!(
            world.data.elevation_mean[pit] > deep + 50.0,
            "pit should rise; was {deep}, now {}",
            world.data.elevation_mean[pit]
        );
    }

    #[test]
    fn mountain_untouched_by_sub_freeboard_lift() {
        let (mut world, mut state) = formed_world();
        let mountain = (0..world.data.cell_count() as usize)
            .find(|&i| world.data.continental_crust[i])
            .expect("continental hex");
        let peak = world.data.sea_level_m + 4000.0;
        world.data.elevation_mean[mountain] = peak;
        world.data.ice_load_m[mountain] = 0.0;
        world.data.water_level_m[mountain] = WATER_NONE;
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
        heal_continental_surface(
            &mut world.data,
            &mut state.registry,
            &state.projection,
            1_000_000_000,
        );
        assert!(
            (world.data.elevation_mean[mountain] - peak).abs() < 1.0,
            "mountain must not enter heal cohort"
        );
    }

    #[test]
    fn ice_loaded_pit_skipped() {
        let (mut world, mut state) = formed_world();
        let pit = (0..world.data.cell_count() as usize)
            .find(|&i| world.data.continental_crust[i])
            .expect("continental hex");
        let before = world.data.sea_level_m - 500.0;
        world.data.elevation_mean[pit] = before;
        world.data.ice_load_m[pit] = 250.0;
        world.data.water_level_m[pit] = WATER_NONE;
        modify_surface_at_world_hex(
            &mut state.registry,
            &world.data,
            &state.projection,
            HexId(pit as u32),
            0,
            |f| {
                f.elevation_m = before;
            },
        );
        heal_continental_surface(
            &mut world.data,
            &mut state.registry,
            &state.projection,
            1_000_000_000,
        );
        assert!(
            (world.data.elevation_mean[pit] - before).abs() < 1e-3,
            "ice-loaded hex must be left for GIA"
        );
    }

    #[test]
    fn heal_is_deterministic() {
        let (mut a, mut state_a) = formed_world();
        let (mut b, mut state_b) = formed_world();
        let n = a.data.cell_count() as usize;
        for w in [&mut a, &mut b] {
            for i in 0..n {
                if w.data.continental_crust[i] && i % 17 == 0 {
                    w.data.elevation_mean[i] = w.data.sea_level_m - 800.0;
                    w.data.water_level_m[i] = WATER_NONE;
                    w.data.ice_load_m[i] = 0.0;
                }
            }
        }
        heal_continental_surface(
            &mut a.data,
            &mut state_a.registry,
            &state_a.projection,
            1_000_000_000,
        );
        heal_continental_surface(
            &mut b.data,
            &mut state_b.registry,
            &state_b.projection,
            1_000_000_000,
        );
        assert_eq!(a.data.elevation_mean, b.data.elevation_mean);
    }
}
