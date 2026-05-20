//! Stable identifiers for sparse entities and content-driven hex properties.

use serde::{Deserialize, Serialize};

/// Identifier for a settlement entity.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct SettlementId(pub u32);

/// Identifier for a nation entity.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct NationId(pub u32);

/// Identifier for a species entity.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct SpeciesId(pub u32);

/// Identifier for a tectonic plate assignment on a hex.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub struct PlateId(pub u16);

impl PlateId {
    /// Sentinel value indicating no plate assignment.
    pub const NONE: PlateId = PlateId(u16::MAX);
}

/// Content-driven biome index for a hex (see mod biome registry in later phases).
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct BiomeId(pub u16);

impl BiomeId {
    /// Sentinel value indicating no biome assignment.
    pub const NONE: BiomeId = BiomeId(u16::MAX);
}
