//! Per-plate surface feature storage (Doc 06 destination-driven model).

use genesis_core::data::BedrockType;
use genesis_core::data::WorldData;
use genesis_core::{HexId, PlateId};
use serde::{Deserialize, Serialize};

use crate::frames::current_world_to_birth_hex;
use crate::plate::{PlateRegistry, PlateType};

/// Matches [`crate::initial_terrain::CONTINENTAL_BASE_ELEVATION_M`].
const CONTINENTAL_BASELINE_M: f32 = 800.0;
/// Matches [`crate::initial_terrain::OCEANIC_BASE_ELEVATION_M`].
const OCEANIC_BASELINE_M: f32 = -3500.0;

/// A single feature stored on a plate's surface by birth world-HexId.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceFeature {
    pub elevation_m: f32,
    pub relief_m: f32,
    pub bedrock: BedrockType,
    pub fertility: f32,
    /// When this feature was created or last meaningfully modified (tie-breaking on merge).
    pub age_year: i64,
    /// True for buoyant continental lithosphere. Set at creation and never
    /// changed: bedrock labels get overwritten by sediment and volcanism, but
    /// crust type is permanent. Continental crust rebounds toward the isostatic
    /// freeboard and never thermally subsides; oceanic crust does the reverse.
    #[serde(default)]
    pub continental_crust: bool,
}

/// Per-plate surface storage indexed by BIRTH world-HexId.
///
/// `features[h]` holds the feature born at world hex `h` (at year 0, or when an event
/// created it). The birth index never changes. To find where a feature currently appears,
/// rotate its birth position forward (see `frames::birth_hex_to_current_world`).
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

    pub fn get(&self, birth_hex: HexId) -> Option<&SurfaceFeature> {
        self.features.get(birth_hex.0 as usize)?.as_ref()
    }

    pub fn set(&mut self, birth_hex: HexId, feature: SurfaceFeature) {
        let idx = birth_hex.0 as usize;
        if idx < self.features.len() {
            self.features[idx] = Some(feature);
        }
    }

    pub fn clear(&mut self, birth_hex: HexId) {
        let idx = birth_hex.0 as usize;
        if idx < self.features.len() {
            self.features[idx] = None;
        }
    }

    /// Modifies an existing feature in-place if it exists; no-op if absent.
    pub fn modify<F>(&mut self, birth_hex: HexId, modifier: F)
    where
        F: FnOnce(&mut SurfaceFeature),
    {
        let idx = birth_hex.0 as usize;
        if let Some(Some(feature)) = self.features.get_mut(idx) {
            modifier(feature);
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
    let birth_hex = current_world_to_birth_hex(&data.grid, world_hex, plate);
    Some(match plate.surface.get(birth_hex) {
        Some(feature) => feature.elevation_m,
        None => type_baseline(plate.plate_type).0,
    })
}

/// Elevation below which a featureless hex is presumed oceanic crust (m).
pub const CRUST_FALLBACK_DEPTH_M: f32 = -1500.0;

/// Whether the crust at a world hex is continental, from the owning plate's
/// feature. Hexes without a feature (projection holes) fall back to the
/// displayed bedrock/elevation, which mirrors their neighbors — a continental
/// plate's accreted oceanic apron must not read as continental there.
pub fn continental_crust_at(data: &WorldData, registry: &PlateRegistry, world_hex: HexId) -> bool {
    let idx = world_hex.0 as usize;
    if idx >= data.plate_id.len() {
        return false;
    }
    let plate_id = data.plate_id[idx];
    if plate_id == PlateId::NONE {
        return false;
    }
    let Some(plate) = registry.get(plate_id) else {
        return false;
    };
    let birth_hex = current_world_to_birth_hex(&data.grid, world_hex, plate);
    match plate.surface.get(birth_hex) {
        Some(feature) => feature.continental_crust,
        None => {
            data.bedrock_type[idx] != BedrockType::OceanicCrust
                && data.elevation_mean[idx] >= CRUST_FALLBACK_DEPTH_M
        }
    }
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
        continental_crust: plate_type == PlateType::Continental,
    }
}

/// Reads or creates a feature at the world hex's birth-index position, applies `modifier`,
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

    let (plate_type, birth_hex) = {
        let Some(plate) = registry.get(plate_id) else {
            return;
        };
        (
            plate.plate_type,
            current_world_to_birth_hex(&data.grid, world_hex, plate),
        )
    };

    let Some(plate) = registry.plates_mut().get_mut(&plate_id) else {
        return;
    };

    let _ = plate_type;
    // Modify-only: hexes without a stored feature (projection holes, aprons)
    // display neighbor-derived values, and writing those back would MINT crust
    // out of display data — erosion or volcanism touching a margin hole would
    // spawn continental crust that rebounds, raising its neighbors' display and
    // spawning more: a traveling continentalization wave. Features are only
    // born at formation, ridge accretion, and the boundary delta flush.
    let Some(existing) = plate.surface.get(birth_hex) else {
        return;
    };
    let mut feature = existing.clone();
    modifier(&mut feature);
    feature.age_year = tick_year;
    plate.surface.set(birth_hex, feature);
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
                continental_crust: false,
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
                continental_crust: false,
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
                continental_crust: false,
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
                continental_crust: false,
            };
            a.set(HexId(i), f.clone());
            b.set(HexId(i), f);
        }
        assert_eq!(a.features, b.features);
    }
}
