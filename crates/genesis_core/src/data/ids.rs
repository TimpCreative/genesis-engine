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

/// Identifier for a mantle hot spot (Doc 06 §7).
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub struct HotSpotId(pub u16);

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

/// Identifies a single ocean basin. Land hexes have [`BasinId::NONE`].
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default, Serialize, Deserialize,
)]
pub struct BasinId(pub u16);

impl BasinId {
    /// Sentinel value for land hexes (not in any ocean basin).
    pub const NONE: BasinId = BasinId(u16::MAX);
}

/// Identifies a standing-water body (ocean, sea, lake) in the hydrology
/// registry (Doc 08 §2.4). The id is the lowest [`HexId`](crate::HexId) of the
/// body's basin, making it stable and deterministic. Dry hexes have
/// [`WaterBodyId::NONE`].
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default, Serialize, Deserialize,
)]
pub struct WaterBodyId(pub u32);

impl WaterBodyId {
    /// Sentinel value for dry hexes (not in any water body).
    pub const NONE: WaterBodyId = WaterBodyId(u32::MAX);
}

/// Identifies a biogeographic province — a connected region of similar biome
/// and connectivity, the granularity biology simulates at (Doc 09 §5.1). Hexes
/// with no province have [`ProvinceId::NONE`].
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default, Serialize, Deserialize,
)]
pub struct ProvinceId(pub u16);

impl ProvinceId {
    /// Sentinel value for hexes not in any province.
    pub const NONE: ProvinceId = ProvinceId(u16::MAX);
}

/// Content-driven functional-guild index — a way of making a living, keyed by
/// realm and refined by biome (Doc 09 §4.1).
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default, Serialize, Deserialize,
)]
pub struct GuildId(pub u16);

impl GuildId {
    /// Sentinel value indicating no guild.
    pub const NONE: GuildId = GuildId(u16::MAX);
}

/// Identifies a functional lineage in the ecological ledger. Stable across
/// save/load; assigned at branch (speciation) events (Doc 09 §8.1). The headline
/// lineage of an unpopulated hex is [`LineageId::NONE`].
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub struct LineageId(pub u64);

impl LineageId {
    /// Sentinel value for hexes with no headline lineage.
    pub const NONE: LineageId = LineageId(u64::MAX);
}

/// Dense index into the loaded trait-morphospace registry (content, not
/// `WorldData`). Save files reference traits by namespaced string; the runtime
/// uses this index (Doc 09 §2.1).
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Serialize, Deserialize)]
pub struct TraitId(pub u32);
