//! Reindexes plate surface features after rotation so rebuild lookups stay consistent.

use std::collections::BTreeMap;

use genesis_core::data::WorldData;
use genesis_core::{HexId, PlateId};

use crate::frames::world_to_plate_local;
use crate::plate::PlateRegistry;
use crate::plate_surface::{PlateSurface, SurfaceFeature};

/// For each populated plate-local cell, finds the world hex that currently displays it
/// (under the pre-repartition `plate_id` assignment), then stores the feature at the
/// plate-local index returned by [`world_to_plate_local`] for that world position.
///
/// Called after [`crate::motion::advance_plate_motion`] and before
/// [`crate::partition::repartition_hexes`]. Cost is O(cell_count) per tick.
pub fn remap_plate_surfaces_after_motion(data: &WorldData, registry: &mut PlateRegistry) {
    let grid = &data.grid;
    let n = data.cell_count() as usize;

    let mut local_to_world: BTreeMap<PlateId, BTreeMap<HexId, HexId>> = BTreeMap::new();

    for i in 0..n {
        let world_hex = HexId(i as u32);
        let plate_id = data.plate_id[i];
        if plate_id == PlateId::NONE {
            continue;
        }
        let Some(plate) = registry.get(plate_id) else {
            continue;
        };
        let local = world_to_plate_local(grid, world_hex, plate);
        local_to_world
            .entry(plate_id)
            .or_default()
            .entry(local)
            .and_modify(|existing| {
                if world_hex < *existing {
                    *existing = world_hex;
                }
            })
            .or_insert(world_hex);
    }

    for plate in registry.plates_mut().values_mut() {
        let Some(lut) = local_to_world.get(&plate.id) else {
            continue;
        };

        let mut gathered: Vec<(HexId, SurfaceFeature)> = Vec::new();
        for (idx, slot) in plate.surface.features.iter().enumerate() {
            let Some(feature) = slot else {
                continue;
            };
            let l_old = HexId(idx as u32);
            let world_hex = lut.get(&l_old).copied().unwrap_or(l_old);
            let l_new = world_to_plate_local(grid, world_hex, plate);
            gathered.push((l_new, feature.clone()));
        }

        let mut remapped = PlateSurface::new(n);
        for (l_new, feature) in gathered {
            remapped.set(l_new, feature);
        }
        plate.surface = remapped;
    }
}
