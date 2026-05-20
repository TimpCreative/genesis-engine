//! Plate types and registry.

use std::collections::BTreeMap;

use genesis_core::time::WorldYear;
use genesis_core::{HexId, PlateId};
use serde::{Deserialize, Serialize};

/// Whether the plate is continental (lighter, thicker, higher elevation)
/// or oceanic (denser, thinner, lower elevation).
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub enum PlateType {
    Continental,
    Oceanic,
}

/// Major (large; Earth-scale continent or ocean) versus Minor (smaller).
/// Affects target size during initial growth seeding.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub enum PlateClass {
    Major,
    Minor,
}

/// A tectonic plate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plate {
    pub id: PlateId,
    pub plate_type: PlateType,
    pub plate_class: PlateClass,

    /// HexId of the seed hex used for this plate's geographic anchor.
    /// Does not change after creation; effective position is computed from
    /// this seed plus accumulated rotation about the motion axis.
    pub seed_hex: HexId,

    /// Unit vector representing the Euler-pole rotation axis. Constrained to
    /// produce sensible plate motion (see Doc 06 §2.1).
    pub motion_axis: [f64; 3],

    /// Angular velocity in radians per year. Always positive.
    pub motion_rate_rad_per_year: f64,

    /// World year this plate was created (or last reorganized).
    pub age_year: WorldYear,

    /// Target fraction of the sphere this plate covers. Used during growth
    /// seeding; informational thereafter.
    pub target_fraction: f32,

    /// Total rotation about `motion_axis` since formation, in radians (`f64`).
    pub accumulated_rotation_rad: f64,
}

/// All plates in a world, keyed by `PlateId`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlateRegistry {
    plates: BTreeMap<PlateId, Plate>,
    next_id: u16,
}

impl PlateRegistry {
    pub fn new() -> Self {
        Self {
            plates: BTreeMap::new(),
            next_id: 0,
        }
    }

    pub fn insert(&mut self, plate: Plate) {
        self.plates.insert(plate.id, plate);
    }

    pub fn get(&self, id: PlateId) -> Option<&Plate> {
        self.plates.get(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Plate> {
        self.plates.values()
    }

    pub fn count(&self) -> usize {
        self.plates.len()
    }

    /// Allocates the next sequential PlateId. Used during initial generation.
    pub(crate) fn next_id(&mut self) -> PlateId {
        let id = PlateId(self.next_id);
        self.next_id += 1;
        id
    }

    pub(crate) fn plates_mut(&mut self) -> &mut BTreeMap<PlateId, Plate> {
        &mut self.plates
    }
}

impl Default for PlateRegistry {
    fn default() -> Self {
        Self::new()
    }
}

use crate::boundary::BoundaryInfo;

/// Runtime tectonics state held by the app or test harness (not in `genesis_core::World`).
#[derive(Clone, Debug, Default)]
pub struct TectonicsState {
    pub registry: PlateRegistry,
    pub formation_complete: bool,
    /// Boundary hexes and classified edges; recomputed each Geological tick.
    pub boundaries: BoundaryInfo,
}

impl TectonicsState {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PlateRegistry {
    /// Sorted plate ids for deterministic iteration.
    pub fn plate_ids(&self) -> Vec<PlateId> {
        self.plates.keys().copied().collect()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Plate> {
        self.plates.values_mut()
    }
}
