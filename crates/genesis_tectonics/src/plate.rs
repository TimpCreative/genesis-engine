//! Plate types and registry.

use std::collections::BTreeMap;

use genesis_core::time::WorldYear;
use genesis_core::{HexId, HotSpotId, PlateId};
use serde::{Deserialize, Serialize};

use crate::plate_surface::PlateSurface;

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

    /// Last year this plate owned at least one hex (§12.1 extinct-plate purge).
    pub last_nonempty_year: WorldYear,

    /// Plate-local surface features indexed by [`HexId`].
    pub surface: PlateSurface,
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

    pub(crate) fn remove(&mut self, id: PlateId) {
        self.plates.remove(&id);
    }
}

impl Default for PlateRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// A fixed mantle thermal anomaly; anchor does not move with plates (Doc 06 §7.1).
#[derive(Clone, Debug)]
pub struct HotSpot {
    pub id: HotSpotId,
    /// Unit vector in the world frame; plates drift over this point.
    pub anchor_position: [f64; 3],
    /// Per Geological tick probability of eruption when alive (§7.1).
    pub activity_rate: f64,
    pub age_year: WorldYear,
    /// Simulated lifetime in years from birth; not an end year (§7.2).
    pub lifespan_years: i64,
    /// Running uplift for §6.2 significance assignment.
    pub cumulative_uplift_m: f32,
}

/// Active hot spots keyed by id for deterministic iteration.
#[derive(Clone, Debug, Default)]
pub struct HotSpotRegistry {
    hotspots: BTreeMap<HotSpotId, HotSpot>,
    next_id: u16,
}

impl HotSpotRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn count(&self) -> usize {
        self.hotspots.len()
    }

    pub fn get(&self, id: HotSpotId) -> Option<&HotSpot> {
        self.hotspots.get(&id)
    }

    pub fn hotspot_ids(&self) -> Vec<HotSpotId> {
        self.hotspots.keys().copied().collect()
    }

    pub(crate) fn insert(&mut self, hotspot: HotSpot) {
        self.hotspots.insert(hotspot.id, hotspot);
    }

    pub(crate) fn remove(&mut self, id: HotSpotId) {
        self.hotspots.remove(&id);
    }

    pub(crate) fn next_id(&mut self) -> HotSpotId {
        let id = HotSpotId(self.next_id);
        self.next_id += 1;
        id
    }

    pub(crate) fn hotspots_mut(&mut self) -> &mut BTreeMap<HotSpotId, HotSpot> {
        &mut self.hotspots
    }

    pub(crate) fn seed_next_id(&mut self, next: u16) {
        self.next_id = next;
    }
}

use genesis_core::events::BoundaryType;

use crate::boundary::BoundaryInfo;

/// Runtime tectonics state held by the app or test harness (not in `genesis_core::World`).
#[derive(Clone, Debug, Default)]
pub struct TectonicsState {
    pub registry: PlateRegistry,
    pub formation_complete: bool,
    /// Boundary hexes and classified edges; recomputed each Geological tick.
    pub boundaries: BoundaryInfo,
    /// Mantle hot spots; seeded at Formation, updated each Geological tick.
    pub hotspots: HotSpotRegistry,
    /// Accumulated eroded mass deposited per hex (§8.3); not persisted in `WorldData`.
    pub cumulative_deposition_m: Vec<f32>,
    /// `elevation_mean` snapshot before boundary elevation this tick (boundary events).
    pub elevation_at_tick_start: Vec<f32>,
    /// Prior tick directed edge classes for `BoundaryTransition` detection.
    pub previous_edge_class: BTreeMap<(genesis_core::HexId, genesis_core::HexId), BoundaryType>,
    /// Baseline divergent boundary length for sea level (§4.6); set on first Geological tick.
    pub baseline_divergent_length_km: Option<f64>,
    /// Events queued during ticks; flushed to root branch at end of history generation.
    pub pending_events: Vec<genesis_core::events::Event>,
    /// Monotonic counter for [`EventId`](genesis_core::events::EventId) allocation.
    pub next_event_id: u64,
    /// Plate reorganizations fired during geological ticks (diagnostics).
    pub reorg_count: u64,
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

#[cfg(test)]
impl Plate {
    pub fn test_plate(
        id: u16,
        plate_type: PlateType,
        seed: u32,
        rate: f64,
        cell_count: usize,
    ) -> Self {
        Self {
            id: PlateId(id),
            plate_type,
            plate_class: PlateClass::Major,
            seed_hex: HexId(seed),
            motion_axis: [0.0, 0.0, 1.0],
            motion_rate_rad_per_year: rate,
            age_year: WorldYear::FORMATION,
            target_fraction: 0.5,
            accumulated_rotation_rad: 0.0,
            last_nonempty_year: WorldYear::FORMATION,
            surface: PlateSurface::new(cell_count),
        }
    }
}
