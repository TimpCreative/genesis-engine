//! Gravitational collapse: relief beyond rock strength relaxes without water
//! (Doc 06 §8.5).
//!
//! Erosion is not the only way mountains come down. Crust thickened past what
//! rock strength can hold spreads under its own weight — orogenic collapse,
//! the process extending Tibet today — and it needs no river, no rain, no
//! climate. This pass caps the elevation step between adjacent hexes at a
//! geologically sustainable maximum. Three regimes per edge:
//!
//! - **Low side at trench depth** (≤ [`COLLAPSE_TRENCH_FLOOR_M`]): the loose
//!   [`COLLAPSE_TRENCH_RELIEF_M`] cap, enforced ONE-SIDED — the arc side
//!   sheds, the floor never moves. That step is the subduction interface,
//!   not a single rock face (Earth's trench-to-summit profiles reach ~15 km
//!   over ~200 km), so arc height near trenches is bounded but not sawed off
//!   at the rock-face cap. Symmetric transfer here is forbidden: at tick
//!   scale it becomes an elevation conveyor from continents into live
//!   trenches that flattens all relief. Fossil-floor healing is §5.8/§5.9's
//!   job, not this pass's.
//! - **Low side submerged** (but above trench depth): the high side sheds
//!   the full excess into the ocean basin (sediment space is effectively
//!   infinite; Doc 8 will route it). This is what saws off coastal cliffs —
//!   a peak may not stand a vertical mile above the adjacent seafloor.
//! - **Both sides land**: the excess spreads symmetrically (high sheds, low
//!   receives — plateau extension, mass conserved).

use genesis_core::HexId;
use genesis_core::data::WorldData;
use genesis_core::time::WorldYear;

use crate::plate::PlateRegistry;
use crate::plate_surface::{modify_surface_at_world_hex, surface_feature_exists_at};
use crate::projection::ProjectionCache;

/// Maximum elevation step sustainable between adjacent hexes (m). Earth's
/// biggest mountain face (Nanga Parbat's Rupal face) is ~4,600 m — a single
/// exceptional wall, not routine terrain. Beyond this, over-steepened crust
/// collapses and spreads under its own weight.
pub const COLLAPSE_MAX_ADJACENT_RELIEF_M: f32 = 5_000.0;

/// Relaxation time for gravitational collapse (years). Post-orogenic
/// extension decays a belt over tens of My even with zero erosion; a
/// 500k-year tick therefore removes ~5% of the excess relief per pass. Do
/// NOT shorten this below a geological tick: with relax → 1.0 per pass the
/// coastal-clip branch flattens coastal relief faster than the margin
/// geometry can recover, the water realm fragments into <1% components, and
/// §5.8 obduction + §5.9 enclosure-infill cascade — measured at τ = 250k:
/// crust area 27–64% (buoyant paving), zero hexes below −3,645 m (trenches
/// infilled), world relief compressed to the isostatic band [−3,500, +800]
/// by 1B years. At τ = 10 My the budget is stable through 4.5B years. The
/// price: actively pumped margins equilibrate ABOVE the cap (measured
/// 7–12 km non-trench steps at 1B) — bounded, but not a hard 5 km. The hard
/// cap needs a pump-side relief limit (refuse uplift that would break the
/// step cap, spreading it to the next inland ring instead), which is a
/// separate change; see Doc 06 §8.5.
pub const COLLAPSE_RELAX_YEARS: f64 = 10_000_000.0;

/// Pairs whose low side sits at or below this elevation are trench-to-arc
/// profiles (m, negative = below sea). They get the looser
/// [`COLLAPSE_TRENCH_RELIEF_M`] cap instead of the rock-face cap: that step
/// is the subduction interface, not a single rock face (Earth's real
/// trench-to-summit profiles reach ~15 km over ~200 km, which a coarse hex
/// grid must represent as one or two adjacent steps).
pub const COLLAPSE_TRENCH_FLOOR_M: f32 = -6_000.0;

/// Maximum step allowed across a trench-adjacent pair (m). Earth's extreme
/// trench-to-summit profile is ~15 km (Andes–Peru–Chile Trench over ~200 km,
/// which a coarse hex grid must represent as one or two adjacent steps).
/// Enforced ONE-SIDED — the high (arc) side sheds, the floor is never
/// lifted and never deepened by this pass: a symmetric transfer at tick
/// scale is an elevation conveyor from continents into live trenches that
/// flattens all planetary relief to the isostatic band within 1B years
/// (measured: world relief compressed to [−3,500, +800], zero mountain
/// regions, crust area >60% via the resulting basin cascade). Fossil-floor
/// healing is §5.8/§5.9's job, not this pass's.
pub const COLLAPSE_TRENCH_RELIEF_M: f32 = 15_000.0;

/// Relaxes adjacent-hex elevation steps beyond rock strength. Reads the
/// tick-start `elevation_mean` field (a one-tick lag on a fast process) and
/// writes through to plate surfaces; both sides of an edge must carry real
/// features so the pass never mints crust into projection holes.
/// Deterministic: ascending hex order, each edge processed once, no RNG.
pub fn apply_gravitational_collapse(
    data: &WorldData,
    registry: &mut PlateRegistry,
    cache: &ProjectionCache,
    tick_interval_years: f64,
    tick_year: WorldYear,
) {
    let relax = tick_interval_years / COLLAPSE_RELAX_YEARS;
    if relax <= 0.0 {
        return;
    }
    let relax = relax.min(1.0);
    let tick_value = tick_year.value();
    let sea = data.sea_level_m;
    let n = data.elevation_mean.len();

    for i in 0..n {
        let hex = HexId(i as u32);
        let elev_i = data.elevation_mean[i];
        let neighbors = data.grid.neighbors_sorted(hex);
        for &neighbor in neighbors {
            let j = neighbor.0 as usize;
            if j <= i || j >= n {
                continue; // each edge once
            }
            let elev_j = data.elevation_mean[j];
            let low_elev = elev_i.min(elev_j);
            let trench = low_elev <= COLLAPSE_TRENCH_FLOOR_M;
            let cap = if trench {
                COLLAPSE_TRENCH_RELIEF_M
            } else {
                COLLAPSE_MAX_ADJACENT_RELIEF_M
            };
            let gap = (elev_i - elev_j).abs() - cap;
            if gap <= 0.0 {
                continue;
            }
            if !surface_feature_exists_at(data, registry, cache, hex)
                || !surface_feature_exists_at(data, registry, cache, neighbor)
            {
                continue;
            }
            let (high, low) = if elev_i >= elev_j {
                (hex, neighbor)
            } else {
                (neighbor, hex)
            };
            if low_elev < sea || trench {
                // One-sided shed: at coast-to-basin edges the high side dumps
                // the excess into the ocean basin (sediment space is
                // effectively infinite; Doc 8 will route it); at trench pairs
                // the arc side is clipped to the trench-profile bound. In
                // both cases the LOW side is never moved by this pass.
                let transfer = (f64::from(gap) * relax) as f32;
                modify_surface_at_world_hex(registry, data, cache, high, tick_value, |f| {
                    f.elevation_m -= transfer;
                });
            } else {
                // Continental interior: the excess spreads symmetrically
                // (mass-conserving plateau extension).
                let transfer = (f64::from(gap) * 0.5 * relax) as f32;
                modify_surface_at_world_hex(registry, data, cache, high, tick_value, |f| {
                    f.elevation_m -= transfer;
                });
                modify_surface_at_world_hex(registry, data, cache, low, tick_value, |f| {
                    f.elevation_m += transfer;
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::data::BedrockType;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{HexGrid, PlateId, create_world};

    use crate::plate::{Plate, PlateClass, PlateType};
    use crate::plate_surface::{PlateSurface, SurfaceFeature};
    use crate::world_rebuild::rebuild_world_from_plate_surfaces;

    const EARTH_RADIUS_KM: f64 = 6371.0;

    /// One continental plate owning the whole grid, with two adjacent
    /// features whose elevations the test sets directly.
    fn one_plate_world() -> (
        genesis_core::World,
        crate::plate::PlateRegistry,
        HexId,
        HexId,
    ) {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;

        let grid = HexGrid::new(5, EARTH_RADIUS_KM).expect("grid");
        let high = HexId(100);
        let low = grid.neighbors(high)[0];

        let mut plate = Plate {
            id: PlateId(0),
            plate_type: PlateType::Continental,
            plate_class: PlateClass::Major,
            seed_hex: HexId(0),
            motion_axis: [0.0, 0.0, 1.0],
            motion_rate_rad_per_year: 0.0,
            age_year: WorldYear::FORMATION,
            target_fraction: 1.0,
            accumulated_rotation_rad: 0.0,
            last_nonempty_year: WorldYear::FORMATION,
            surface: PlateSurface::new(n),
            forward_world_hint: Vec::new(),
        };
        let feature = |elevation_m: f32| SurfaceFeature {
            elevation_m,
            relief_m: 0.0,
            bedrock: BedrockType::Igneous,
            fertility: 0.0,
            age_year: 0,
            continental_crust: true,
        };
        for i in 0..n {
            plate.surface.set(HexId(i as u32), feature(800.0));
        }
        plate.surface.set(high, feature(9_000.0));
        plate.surface.set(low, feature(-500.0));

        let mut registry = crate::plate::PlateRegistry::new();
        registry.insert(plate);
        for pid in world.data.plate_id.iter_mut() {
            *pid = PlateId(0);
        }
        rebuild_world_from_plate_surfaces(&mut world.data, &registry);
        (world, registry, high, low)
    }

    fn surface_sum(registry: &crate::plate::PlateRegistry) -> f64 {
        registry
            .iter()
            .flat_map(|p| p.surface.features.iter().flatten())
            .map(|f| f64::from(f.elevation_m))
            .sum()
    }

    #[test]
    fn over_steepened_edges_clip_across_regimes() {
        let (world, mut registry, high, low) = one_plate_world();
        let cache = ProjectionCache::empty();
        let neighbor_count = world.data.grid.neighbors(high).len();

        apply_gravitational_collapse(
            &world.data,
            &mut registry,
            &cache,
            500_000.0,
            WorldYear(500_000),
        );

        let plate = registry.get(PlateId(0)).unwrap();
        let hi = plate.surface.get(high).unwrap().elevation_m;
        let lo = plate.surface.get(low).unwrap().elevation_m;
        // relax = 0.05 at a 500k tick (τ = 10 My). Pit edge (9,000 vs −500,
        // submerged low): the spike sheds the excess × relax into the ocean
        // (225) and the pit is unchanged. Plateau edges (9,000 vs 800, both
        // land): symmetric spread, 80 each way.
        let pit_shed = (9_500.0 - COLLAPSE_MAX_ADJACENT_RELIEF_M) as f64 * 0.05;
        let plateau_transfer = (8_200.0 - COLLAPSE_MAX_ADJACENT_RELIEF_M) as f64 * 0.5 * 0.05;
        let expected_hi = 9_000.0 - pit_shed - plateau_transfer * (neighbor_count - 1) as f64;
        assert!(
            (f64::from(hi) - expected_hi).abs() < 1.0,
            "high side sheds on every over-steepened edge: {hi} vs {expected_hi}"
        );
        assert!(
            (f64::from(lo) - (-500.0)).abs() < 1e-3,
            "submerged low side is unchanged (mass exports to the ocean): {lo}"
        );
    }

    #[test]
    fn sub_threshold_relief_is_untouched() {
        let (mut world, mut registry, high, _low) = one_plate_world();
        let cache = ProjectionCache::empty();
        // Drop the spike to exactly the threshold: no transfer anywhere.
        {
            let plate = registry.plates_mut().get_mut(&PlateId(0)).unwrap();
            let mut f = plate.surface.get(high).unwrap().clone();
            f.elevation_m = 4_500.0;
            plate.surface.set(high, f);
        }
        rebuild_world_from_plate_surfaces(&mut world.data, &registry);
        let before = surface_sum(&registry);

        apply_gravitational_collapse(
            &world.data,
            &mut registry,
            &cache,
            500_000.0,
            WorldYear(500_000),
        );

        let after = surface_sum(&registry);
        assert_eq!(before, after, "nothing above the cap must move");
    }

    #[test]
    fn projection_holes_are_never_written() {
        let (mut world, mut registry, high, low) = one_plate_world();
        let cache = ProjectionCache::empty();
        let neighbor_count = world.data.grid.neighbors(high).len();
        // Remove the low side's feature: that edge must be skipped, so the
        // spike only sheds onto its baseline-plateau edges and the hole stays
        // a hole (no crust minted out of display data).
        {
            let plate = registry.plates_mut().get_mut(&PlateId(0)).unwrap();
            plate.surface.clear(low);
        }
        rebuild_world_from_plate_surfaces(&mut world.data, &registry);

        apply_gravitational_collapse(
            &world.data,
            &mut registry,
            &cache,
            500_000.0,
            WorldYear(500_000),
        );

        let plateau_transfer = (8_200.0 - COLLAPSE_MAX_ADJACENT_RELIEF_M) as f64 * 0.5 * 0.05;
        let expected_hi = 9_000.0 - plateau_transfer * (neighbor_count - 1) as f64;
        let plate = registry.get(PlateId(0)).unwrap();
        let hi = plate.surface.get(high).unwrap().elevation_m;
        assert!(
            (f64::from(hi) - expected_hi).abs() < 1.0,
            "spike sheds only onto real features: {hi} vs {expected_hi}"
        );
        assert!(plate.surface.get(low).is_none(), "holes must not be minted");
    }

    #[test]
    fn trench_pairs_clip_the_arc_without_moving_the_floor() {
        let (mut world, mut registry, high, low) = one_plate_world();
        let cache = ProjectionCache::empty();
        let neighbor_count = world.data.grid.neighbors(high).len();
        // Drop the low side to trench depth: that edge gets the loose trench
        // cap (15,000), enforced one-sided — the arc sheds, the floor is
        // never lifted (no elevation conveyor into live trenches).
        {
            let plate = registry.plates_mut().get_mut(&PlateId(0)).unwrap();
            let mut f = plate.surface.get(low).unwrap().clone();
            f.elevation_m = -8_500.0;
            plate.surface.set(low, f);
        }
        rebuild_world_from_plate_surfaces(&mut world.data, &registry);

        apply_gravitational_collapse(
            &world.data,
            &mut registry,
            &cache,
            500_000.0,
            WorldYear(500_000),
        );

        // relax = 0.05: trench edge (17,500 step over a 15,000 cap) sheds
        // 125 one-sided; plateau edges (8,200 over 5,000) transfer 80.
        let trench_shed = (17_500.0 - COLLAPSE_TRENCH_RELIEF_M) as f64 * 0.05;
        let plateau_transfer = (8_200.0 - COLLAPSE_MAX_ADJACENT_RELIEF_M) as f64 * 0.5 * 0.05;
        let expected_hi = 9_000.0 - trench_shed - plateau_transfer * (neighbor_count - 1) as f64;
        let plate = registry.get(PlateId(0)).unwrap();
        let hi = plate.surface.get(high).unwrap().elevation_m;
        let lo = plate.surface.get(low).unwrap().elevation_m;
        assert!(
            (f64::from(hi) - expected_hi).abs() < 1.0,
            "trench edge clips the arc, plateau edges spread: {hi} vs {expected_hi}"
        );
        assert!(
            (f64::from(lo) - (-8_500.0)).abs() < 1e-3,
            "the trench floor never moves: {lo}"
        );
    }
}
