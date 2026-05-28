//! Per-plate surface feature storage (Doc 06 destination-driven model).

use genesis_core::data::BedrockType;
use genesis_core::data::WorldData;
use genesis_core::{HexId, PlateId};
use serde::{Deserialize, Serialize};

use crate::frames::world_to_plate_local;
use crate::plate::{PlateRegistry, PlateType};

/// Matches [`crate::initial_terrain::CONTINENTAL_BASE_ELEVATION_M`].
const CONTINENTAL_BASELINE_M: f32 = 800.0;
/// Matches [`crate::initial_terrain::OCEANIC_BASE_ELEVATION_M`].
const OCEANIC_BASELINE_M: f32 = -3500.0;

/// A single feature stored on a plate's surface in plate-local coordinates.
///
/// "Plate-local" means the world-frame position if the plate had zero accumulated rotation.
/// When the plate rotates, features stay at the same plate-local positions but their world
/// positions move.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceFeature {
    pub elevation_m: f32,
    pub relief_m: f32,
    pub bedrock: BedrockType,
    pub fertility: f32,
    /// When this feature was created or last meaningfully modified (tie-breaking on merge).
    pub age_year: i64,
}

/// Per-plate surface storage indexed by plate-local [`HexId`].
///
/// Length is always `cell_count` of the shared world hex grid. Most entries are `None`
/// for plates that do not span the whole world.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlateSurface {
    pub features: Vec<Option<SurfaceFeature>>,
}

impl PlateSurface {
    pub fn new(cell_count: usize) -> Self {
        Self {
            features: vec![None; cell_count],
        }
    }

    pub fn get(&self, plate_local_hex: HexId) -> Option<&SurfaceFeature> {
        self.features.get(plate_local_hex.0 as usize)?.as_ref()
    }

    pub fn set(&mut self, plate_local_hex: HexId, feature: SurfaceFeature) {
        let idx = plate_local_hex.0 as usize;
        if idx < self.features.len() {
            self.features[idx] = Some(feature);
        }
    }

    pub fn clear(&mut self, plate_local_hex: HexId) {
        let idx = plate_local_hex.0 as usize;
        if idx < self.features.len() {
            self.features[idx] = None;
        }
    }

    /// Modifies an existing feature in-place if it exists; no-op if absent.
    pub fn modify<F>(&mut self, plate_local_hex: HexId, modifier: F)
    where
        F: FnOnce(&mut SurfaceFeature),
    {
        let idx = plate_local_hex.0 as usize;
        if let Some(slot) = self.features.get_mut(idx) {
            if let Some(feature) = slot.as_mut() {
                modifier(feature);
            }
        }
    }

    /// Returns count of populated cells. Diagnostic only.
    pub fn populated_count(&self) -> u32 {
        self.features.iter().filter(|f| f.is_some()).count() as u32
    }

    /// Merges features from `other` into `self`. On overlap, prefers the newer feature
    /// (higher `age_year`).
    pub fn merge_from(&mut self, other: &PlateSurface) {
        for (i, other_slot) in other.features.iter().enumerate() {
            let Some(other_feature) = other_slot else {
                continue;
            };
            let idx = i as u32;
            match self.features.get_mut(i) {
                Some(self_slot) => match self_slot {
                    None => *self_slot = Some(other_feature.clone()),
                    Some(existing) => {
                        if other_feature.age_year >= existing.age_year {
                            *self_slot = Some(other_feature.clone());
                        }
                    }
                },
                None => break,
            }
            let _ = idx;
        }
    }
}

/// Plate-type baseline elevation and bedrock when no feature is stored.
pub fn type_baseline(plate_type: PlateType) -> (f32, BedrockType) {
    match plate_type {
        PlateType::Continental => (CONTINENTAL_BASELINE_M, BedrockType::Igneous),
        PlateType::Oceanic => (OCEANIC_BASELINE_M, BedrockType::OceanicCrust),
    }
}

/// Effective elevation at a world hex from plate surfaces (includes same-tick boundary writes).
pub fn surface_elevation_at(
    data: &WorldData,
    registry: &PlateRegistry,
    world_hex: HexId,
) -> Option<f32> {
    let idx = world_hex.0 as usize;
    if idx >= data.plate_id.len() {
        return None;
    }
    let plate_id = data.plate_id[idx];
    if plate_id == PlateId::NONE {
        return None;
    }
    let plate = registry.get(plate_id)?;
    let plate_local = world_to_plate_local(&data.grid, world_hex, plate);
    Some(match plate.surface.get(plate_local) {
        Some(feature) => feature.elevation_m,
        None => type_baseline(plate.plate_type).0,
    })
}

/// Default surface feature for a plate type at the given age.
pub fn baseline_feature(plate_type: PlateType, age_year: i64) -> SurfaceFeature {
    let (elev, bedrock) = type_baseline(plate_type);
    SurfaceFeature {
        elevation_m: elev,
        relief_m: 0.0,
        bedrock,
        fertility: 0.0,
        age_year,
    }
}

/// Reads or creates a feature at the world hex's plate-local position, applies `modifier`,
/// and stores it back on the owning plate's surface.
pub fn modify_surface_at_world_hex<F>(
    registry: &mut PlateRegistry,
    data: &WorldData,
    world_hex: HexId,
    tick_year: i64,
    modifier: F,
) where
    F: FnOnce(&mut SurfaceFeature),
{
    let idx = world_hex.0 as usize;
    if idx >= data.plate_id.len() {
        return;
    }
    let plate_id = data.plate_id[idx];
    if plate_id == PlateId::NONE {
        return;
    }

    let (plate_type, plate_local) = {
        let Some(plate) = registry.get(plate_id) else {
            return;
        };
        (
            plate.plate_type,
            world_to_plate_local(&data.grid, world_hex, plate),
        )
    };

    let Some(plate) = registry.plates_mut().get_mut(&plate_id) else {
        return;
    };

    let mut feature = plate
        .surface
        .get(plate_local)
        .cloned()
        .unwrap_or_else(|| baseline_feature(plate_type, tick_year));
    modifier(&mut feature);
    feature.age_year = tick_year;
    plate.surface.set(plate_local, feature);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_prefers_newer_features() {
        let mut a = PlateSurface::new(4);
        let mut b = PlateSurface::new(4);
        a.set(
            HexId(1),
            SurfaceFeature {
                elevation_m: 100.0,
                relief_m: 0.0,
                bedrock: BedrockType::Igneous,
                fertility: 0.0,
                age_year: 100,
            },
        );
        b.set(
            HexId(1),
            SurfaceFeature {
                elevation_m: 200.0,
                relief_m: 0.0,
                bedrock: BedrockType::Metamorphic,
                fertility: 0.0,
                age_year: 200,
            },
        );
        a.merge_from(&b);
        assert_eq!(a.get(HexId(1)).unwrap().elevation_m, 200.0);

        b.set(
            HexId(1),
            SurfaceFeature {
                elevation_m: 50.0,
                relief_m: 0.0,
                bedrock: BedrockType::Sedimentary,
                fertility: 0.0,
                age_year: 50,
            },
        );
        a.merge_from(&b);
        assert_eq!(a.get(HexId(1)).unwrap().elevation_m, 200.0);
    }

    #[test]
    fn surface_is_deterministic() {
        let mut a = PlateSurface::new(8);
        let mut b = PlateSurface::new(8);
        for i in 0..4u32 {
            let f = SurfaceFeature {
                elevation_m: i as f32 * 10.0,
                relief_m: 0.0,
                bedrock: BedrockType::Igneous,
                fertility: 0.0,
                age_year: i64::from(i),
            };
            a.set(HexId(i), f.clone());
            b.set(HexId(i), f);
        }
        assert_eq!(a.features, b.features);
    }
}
