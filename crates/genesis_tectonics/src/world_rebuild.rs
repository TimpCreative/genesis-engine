//! Rebuilds [`WorldData`] per-hex fields from plate surfaces.

use genesis_core::HexId;
use genesis_core::data::{BedrockType, WorldData};

use crate::frames::{birth_hex_to_current_world, current_world_to_birth_hex};
use crate::plate::PlateRegistry;
use crate::plate_surface::type_baseline;
use crate::projection::ProjectionCache;

/// Cache-driven variant of [`rebuild_world_from_plate_surfaces`]: resolves each
/// world hex's displayed feature through the projection table instead of
/// forward-rotating every stored feature (three rebuilds per tick otherwise
/// dominate at subdivision level 8). Falls back to the direct rebuild when the
/// cache does not cover the world's current ownership.
pub fn rebuild_world_from_plate_surfaces_cached(
    data: &mut WorldData,
    registry: &PlateRegistry,
    cache: &ProjectionCache,
) {
    if !cache.covers(data) {
        rebuild_world_from_plate_surfaces(data, registry);
        return;
    }

    // Per-index writes into world field arrays (gather-parallel; see
    // genesis_core::parallel). Ownership / registry / cache are read-only.
    use rayon::prelude::*;
    let plate_ids = &data.plate_id[..];
    let elev = &mut data.elevation_mean[..];
    let relief = &mut data.elevation_relief[..];
    let bedrock = &mut data.bedrock_type[..];
    let fertility = &mut data.fertility[..];
    let crust = &mut data.continental_crust[..];

    elev.par_iter_mut()
        .zip(relief.par_iter_mut())
        .zip(bedrock.par_iter_mut())
        .zip(fertility.par_iter_mut())
        .zip(crust.par_iter_mut())
        .enumerate()
        .for_each(
            |(i, ((((elev_i, relief_i), bedrock_i), fertility_i), crust_i))| {
                let plate_id = plate_ids[i];
                let Some(plate) = registry.get(plate_id) else {
                    *elev_i = 0.0;
                    *relief_i = 0.0;
                    *bedrock_i = BedrockType::Unknown;
                    *fertility_i = 0.0;
                    *crust_i = false;
                    return;
                };
                let feature = cache
                    .birth_hex_at_covered(i)
                    .and_then(|birth| plate.surface.get(birth));
                match feature {
                    Some(feature) => {
                        *elev_i = feature.elevation_m;
                        *relief_i = feature.relief_m;
                        *bedrock_i = feature.bedrock;
                        *fertility_i = feature.fertility;
                        *crust_i = feature.continental_crust;
                    }
                    None => {
                        let (e, b) = type_baseline(plate.plate_type);
                        *elev_i = e;
                        *relief_i = 0.0;
                        *bedrock_i = b;
                        *fertility_i = 0.0;
                        *crust_i = plate.plate_type == crate::plate::PlateType::Continental;
                    }
                }
            },
        );
}

/// Rebuilds `elevation_mean`, `elevation_relief`, `bedrock_type`, `fertility`,
/// and `continental_crust` from each plate's surface storage. Called after
/// motion/repartition and again after surface mutations each Geological tick.
pub fn rebuild_world_from_plate_surfaces(data: &mut WorldData, registry: &PlateRegistry) {
    let n = data.cell_count() as usize;
    let grid = &data.grid;

    for i in 0..n {
        let plate_id = data.plate_id[i];
        match registry.get(plate_id) {
            Some(plate) => {
                let (elev, bedrock) = type_baseline(plate.plate_type);
                data.elevation_mean[i] = elev;
                data.elevation_relief[i] = 0.0;
                data.bedrock_type[i] = bedrock;
                data.fertility[i] = 0.0;
                data.continental_crust[i] =
                    plate.plate_type == crate::plate::PlateType::Continental;
            }
            None => {
                data.elevation_mean[i] = 0.0;
                data.elevation_relief[i] = 0.0;
                data.bedrock_type[i] = BedrockType::Unknown;
                data.fertility[i] = 0.0;
                data.continental_crust[i] = false;
            }
        }
    }

    let mut written_priority: Vec<Option<(f32, i64, u32)>> = vec![None; n];

    for (plate_id, plate) in registry.iter_sorted() {
        for (birth_idx, slot) in plate.surface.features.iter().enumerate() {
            let Some(feature) = slot else {
                continue;
            };
            let birth_hex = HexId(birth_idx as u32);
            let current_world = birth_hex_to_current_world(grid, birth_hex, plate);
            let w = current_world.0 as usize;
            if w >= n {
                continue;
            }

            if data.plate_id[w] != plate_id {
                continue;
            }

            let candidate_priority = (feature.elevation_m, feature.age_year, birth_idx as u32);
            let should_write = match written_priority[w] {
                None => true,
                Some((e, a, b)) => {
                    candidate_priority.0 > e
                        || (candidate_priority.0 == e && candidate_priority.1 > a)
                        || (candidate_priority.0 == e
                            && candidate_priority.1 == a
                            && candidate_priority.2 < b)
                }
            };

            if should_write {
                data.elevation_mean[w] = feature.elevation_m;
                data.elevation_relief[w] = feature.relief_m;
                data.bedrock_type[w] = feature.bedrock;
                data.fertility[w] = feature.fertility;
                data.continental_crust[w] = feature.continental_crust;
                written_priority[w] = Some(candidate_priority);
            }
        }
    }

    // Resolve projection holes (owned hexes no feature forward-projected onto)
    // by INVERSE-rotating each hole to its birth hex and reading the plate's
    // material there — the same result the cached rebuild gets from
    // `ProjectionCache::birth_hex_for`, just recomputed since this fallback has
    // no cache. Empty birth slot keeps the plate-type baseline already written
    // above. This replaces neighbor-mean smoothing, which dipped continental
    // holes below sea level and speckled continents with phantom water.
    #[allow(clippy::needless_range_loop)]
    for i in 0..n {
        if written_priority[i].is_some() {
            continue;
        }
        let plate_id = data.plate_id[i];
        let Some(plate) = registry.get(plate_id) else {
            continue;
        };
        let birth = current_world_to_birth_hex(grid, HexId(i as u32), plate);
        let Some(feature) = plate.surface.get(birth) else {
            continue;
        };
        data.elevation_mean[i] = feature.elevation_m;
        data.elevation_relief[i] = feature.relief_m;
        data.bedrock_type[i] = feature.bedrock;
        data.fertility[i] = feature.fertility;
        data.continental_crust[i] = feature.continental_crust;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::time::WorldYear;
    use genesis_core::{PlateId, create_world};

    use crate::frames::birth_hex_to_current_world;
    use crate::initial_terrain::CONTINENTAL_BASE_ELEVATION_M;
    use crate::plate::{Plate, PlateClass, PlateRegistry, PlateType};
    use crate::plate_surface::{PlateSurface, SurfaceFeature};

    fn test_registry(cell_count: usize) -> PlateRegistry {
        let mut registry = PlateRegistry::new();
        registry.insert(Plate {
            id: PlateId(0),
            plate_type: PlateType::Continental,
            plate_class: PlateClass::Major,
            seed_hex: HexId(0),
            motion_axis: [0.0, 0.0, 1.0],
            motion_rate_rad_per_year: 1e-8,
            age_year: WorldYear::FORMATION,
            target_fraction: 0.5,
            accumulated_rotation_rad: 0.0,
            last_nonempty_year: WorldYear::FORMATION,
            surface: PlateSurface::new(cell_count),
            forward_world_hint: Vec::new(),
        });
        registry.insert(Plate {
            id: PlateId(1),
            plate_type: PlateType::Oceanic,
            plate_class: PlateClass::Major,
            seed_hex: HexId(100),
            motion_axis: [0.0, 1.0, 0.0],
            motion_rate_rad_per_year: 5e-9,
            age_year: WorldYear::FORMATION,
            target_fraction: 0.3,
            accumulated_rotation_rad: 0.0,
            last_nonempty_year: WorldYear::FORMATION,
            surface: PlateSurface::new(cell_count),
            forward_world_hint: Vec::new(),
        });
        registry
    }

    #[test]
    fn empty_surfaces_produce_baseline_elevations() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        let registry = test_registry(n);

        for (i, pid) in world.data.plate_id.iter_mut().enumerate() {
            *pid = if i < n / 2 { PlateId(0) } else { PlateId(1) };
        }

        rebuild_world_from_plate_surfaces(&mut world.data, &registry);

        assert!(
            world.data.elevation_mean[0] > 0.0,
            "continental baseline should be positive"
        );
        assert!(
            world.data.elevation_mean[n - 1] < 0.0,
            "oceanic baseline should be negative"
        );
    }

    #[test]
    fn populated_surfaces_produce_correct_elevations() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        let mut registry = test_registry(n);

        for i in 0..n {
            world.data.plate_id[i] = PlateId(0);
        }

        {
            let plate = registry.plates_mut().get_mut(&PlateId(0)).unwrap();
            plate.surface.set(
                HexId(5),
                SurfaceFeature {
                    elevation_m: 3000.0,
                    relief_m: 500.0,
                    bedrock: BedrockType::Metamorphic,
                    fertility: 0.1,
                    age_year: 0,
                    continental_crust: false,
                    root_m: 0.0,
                },
            );
        }

        rebuild_world_from_plate_surfaces(&mut world.data, &registry);

        assert_eq!(world.data.elevation_mean[5], 3000.0);
        assert_eq!(world.data.elevation_relief[5], 500.0);
        assert_eq!(world.data.bedrock_type[5], BedrockType::Metamorphic);
        assert!((world.data.fertility[5] - 0.1).abs() < 1e-6);
    }

    #[test]
    fn rotation_moves_features_in_world_view() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        let mut registry = test_registry(n);

        for i in 0..n {
            world.data.plate_id[i] = PlateId(0);
        }

        let birth_hex = HexId(50);
        let peak_elev = 4000.0_f32;

        {
            let plate = registry.plates_mut().get_mut(&PlateId(0)).unwrap();
            plate.surface.set(
                birth_hex,
                SurfaceFeature {
                    elevation_m: peak_elev,
                    relief_m: 800.0,
                    bedrock: BedrockType::Igneous,
                    fertility: 0.0,
                    age_year: 0,
                    continental_crust: false,
                    root_m: 0.0,
                },
            );
        }

        rebuild_world_from_plate_surfaces(&mut world.data, &registry);
        let world_at_zero = birth_hex_to_current_world(
            &world.data.grid,
            birth_hex,
            registry.get(PlateId(0)).unwrap(),
        );
        assert_eq!(
            world.data.elevation_mean[world_at_zero.0 as usize],
            peak_elev
        );

        {
            let plate = registry.plates_mut().get_mut(&PlateId(0)).unwrap();
            plate.accumulated_rotation_rad = 0.5;
        }

        rebuild_world_from_plate_surfaces(&mut world.data, &registry);
        let world_after_rotation = birth_hex_to_current_world(
            &world.data.grid,
            birth_hex,
            registry.get(PlateId(0)).unwrap(),
        );

        assert_ne!(world_at_zero, world_after_rotation);
        assert_eq!(
            world.data.elevation_mean[world_after_rotation.0 as usize],
            peak_elev
        );
        assert!(
            world.data.elevation_mean[world_at_zero.0 as usize] < peak_elev - 100.0
                || (world.data.elevation_mean[world_at_zero.0 as usize]
                    - CONTINENTAL_BASE_ELEVATION_M)
                    .abs()
                    < 100.0,
            "old world position should no longer carry the peak after rotation"
        );
    }

    #[test]
    fn collisions_resolve_by_higher_elevation() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        let mut registry = test_registry(n);

        for i in 0..n {
            world.data.plate_id[i] = PlateId(0);
        }

        {
            let plate = registry.plates_mut().get_mut(&PlateId(0)).unwrap();
            plate.accumulated_rotation_rad = 1.75;
        }

        let mut first_for_target: std::collections::BTreeMap<HexId, HexId> =
            std::collections::BTreeMap::new();
        let mut collision_pair: Option<(HexId, HexId, HexId)> = None;
        for i in 0..n {
            let birth = HexId(i as u32);
            let target = birth_hex_to_current_world(
                &world.data.grid,
                birth,
                registry.get(PlateId(0)).unwrap(),
            );
            if let Some(existing) = first_for_target.get(&target).copied() {
                collision_pair = Some((existing, birth, target));
                break;
            }
            first_for_target.insert(target, birth);
        }
        let (birth_a, birth_b, target) = collision_pair.expect("expected at least one collision");

        {
            let plate = registry.plates_mut().get_mut(&PlateId(0)).unwrap();
            plate.surface.set(
                birth_a,
                SurfaceFeature {
                    elevation_m: 1200.0,
                    relief_m: 20.0,
                    bedrock: BedrockType::Sedimentary,
                    fertility: 0.1,
                    age_year: 1,
                    continental_crust: false,
                    root_m: 0.0,
                },
            );
            plate.surface.set(
                birth_b,
                SurfaceFeature {
                    elevation_m: 4500.0,
                    relief_m: 200.0,
                    bedrock: BedrockType::Igneous,
                    fertility: 0.3,
                    age_year: 2,
                    continental_crust: false,
                    root_m: 0.0,
                },
            );
        }

        rebuild_world_from_plate_surfaces(&mut world.data, &registry);
        assert_eq!(world.data.elevation_mean[target.0 as usize], 4500.0);
    }

    #[test]
    fn non_owning_plate_does_not_write_collided_hex() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        let mut registry = test_registry(n);

        // Every hex is owned by plate 0; plate 1 owns nothing.
        for i in 0..n {
            world.data.plate_id[i] = PlateId(0);
        }

        // Plate 1 has zero rotation, so its birth hex projects onto itself — a world
        // hex owned by plate 0. The ownership guard must reject the write even though
        // this feature far outranks plate 0's baseline in collision priority.
        let birth_hex = HexId(60);
        {
            let plate = registry.plates_mut().get_mut(&PlateId(1)).unwrap();
            plate.surface.set(
                birth_hex,
                SurfaceFeature {
                    elevation_m: 9000.0,
                    relief_m: 1000.0,
                    bedrock: BedrockType::Metamorphic,
                    fertility: 0.5,
                    age_year: 100,
                    continental_crust: false,
                    root_m: 0.0,
                },
            );
        }

        rebuild_world_from_plate_surfaces(&mut world.data, &registry);

        let target = birth_hex_to_current_world(
            &world.data.grid,
            birth_hex,
            registry.get(PlateId(1)).unwrap(),
        );
        let w = target.0 as usize;
        assert!(
            (world.data.elevation_mean[w] - CONTINENTAL_BASE_ELEVATION_M).abs() < 1e-6,
            "hex owned by plate 0 must keep its baseline, got {}",
            world.data.elevation_mean[w]
        );
        assert_ne!(world.data.bedrock_type[w], BedrockType::Metamorphic);
    }

    #[test]
    fn gaps_keep_plate_baseline() {
        let mut params = WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let mut world = create_world(params).expect("world");
        let n = world.data.cell_count() as usize;
        let registry = test_registry(n);
        for i in 0..n {
            world.data.plate_id[i] = PlateId(0);
        }

        rebuild_world_from_plate_surfaces(&mut world.data, &registry);
        assert!(
            world
                .data
                .elevation_mean
                .iter()
                .all(|e| (*e - CONTINENTAL_BASE_ELEVATION_M).abs() < 1e-6)
        );
    }
}
