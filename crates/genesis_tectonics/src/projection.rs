//! Per-tick cache of world-hex → birth-hex projections (P1-20, performance).
//!
//! [`crate::partition::repartition_hexes`] already inverse-maps every world hex
//! to the birth-frame feature that claimed it. This cache records that mapping
//! so the rest of the tick (elevation deltas, volcanism, hotspots, erosion,
//! world rebuilds) resolves birth hexes with a table lookup instead of
//! re-deriving a quaternion rotation and running a nearest-hex search per call
//! — the dominant per-tick cost at subdivision level 8.
//!
//! Validity: the mapping depends on plate rotations and hex ownership, both of
//! which change only inside `repartition_hexes` (rotations advance just before
//! it; reorganizations repartition again after rebasing). Every lookup is
//! guarded by the ownership snapshot taken at build time, so a stale or empty
//! cache degrades to "no entry" and callers fall back to direct computation.

use genesis_core::data::WorldData;
use genesis_core::{HexId, PlateId};

const NO_HEX: u32 = u32::MAX;

/// World-hex → owning-plate birth-hex table built by
/// [`crate::partition::repartition_hexes`]. `Default`/[`ProjectionCache::empty`]
/// yields a cache with no entries: all lookups miss and callers compute
/// directly, so tests and cold paths need no special handling.
#[derive(Clone, Debug, Default)]
pub struct ProjectionCache {
    /// Hex ownership at build time; lookups are rejected where the world has
    /// since diverged from this snapshot.
    plate_id: Vec<PlateId>,
    /// Birth hex of the owning plate's material at each world hex
    /// ([`NO_HEX`] where unknown).
    birth_hex: Vec<u32>,
    /// True where a stored feature projected onto the hex (footprint claims
    /// and freshly minted ridge crust). Adopted quantization holes carry a
    /// birth hex for writes but display neighbor-patched values, so rebuild
    /// must not treat them as feature-backed.
    claimed: Vec<bool>,
}

impl ProjectionCache {
    /// A cache with no entries; every lookup misses.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Pre-sized cache with ownership snapshot `owner`; entries are filled by
    /// the partition pass.
    pub(crate) fn with_ownership(owner: &[PlateId]) -> Self {
        Self {
            plate_id: owner.to_vec(),
            birth_hex: vec![NO_HEX; owner.len()],
            claimed: vec![false; owner.len()],
        }
    }

    pub(crate) fn record(&mut self, world_idx: usize, birth_hex: HexId, claimed: bool) {
        if world_idx < self.birth_hex.len() {
            self.birth_hex[world_idx] = birth_hex.0;
            self.claimed[world_idx] = claimed;
        }
    }

    /// Birth hex at `world_idx` when the cache is known to cover current ownership
    /// (caller must have checked [`Self::covers`]).
    pub(crate) fn birth_hex_at_covered(&self, world_idx: usize) -> Option<HexId> {
        let birth = *self.birth_hex.get(world_idx)?;
        (birth != NO_HEX).then_some(HexId(birth))
    }

    /// Birth hex of the plate material at `world_hex`, or `None` when the
    /// cache has no entry or ownership has changed since it was built.
    pub fn birth_hex_for(&self, data: &WorldData, world_hex: HexId) -> Option<HexId> {
        let idx = world_hex.0 as usize;
        let cached_owner = *self.plate_id.get(idx)?;
        if data.plate_id.get(idx) != Some(&cached_owner) {
            return None;
        }
        self.birth_hex_at_covered(idx)
    }

    /// Whether a stored feature projected onto `world_hex` (valid only when
    /// [`Self::covers`] holds).
    pub fn is_claimed(&self, world_hex: HexId) -> bool {
        self.claimed.get(world_hex.0 as usize).copied() == Some(true)
    }

    /// Whether this cache was built for the world's current hex ownership.
    pub fn covers(&self, data: &WorldData) -> bool {
        !self.plate_id.is_empty() && self.plate_id == data.plate_id
    }
}
