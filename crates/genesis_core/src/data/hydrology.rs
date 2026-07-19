//! Hydrology & soil schema types stored on [`WorldData`](crate::data::WorldData) (Doc 08).
//!
//! Slice 1 (P2-20) defines the schema only: the water-body registry, the
//! per-hex flags, and the soil classes. The systems that fill them (drainage,
//! lakes, ice, soil) arrive in later slices.

use serde::{Deserialize, Serialize};

use crate::data::WaterBodyId;
use crate::grid::HexId;

/// Sentinel for [`WorldData::water_level_m`](crate::data::WorldData) on dry
/// hexes: no standing water above this cell (Doc 08 §2.4). Depth on wet hexes
/// is `water_level_m - elevation_mean`.
pub const WATER_NONE: f32 = f32::NEG_INFINITY;

/// Classification of a standing-water body (Doc 08 §2.4).
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default, Serialize, Deserialize,
)]
pub enum WaterBodyKind {
    /// The largest-volume connected component below sea level.
    #[default]
    Ocean,
    /// A below-sea-level component separate from the ocean (Slice 1 treats
    /// these as ocean-fed; §5 adjudication lands in Slice 2).
    Sea,
    /// An exorheic or overflowing inland body (Slice 2).
    Lake,
    /// An endorheic evaporation-balance body (Slice 2).
    SaltLake,
    /// A dried body leaving accumulated salt (Slice 2).
    SaltFlat,
}

/// One standing-water body in the per-tick registry (Doc 08 §2.4).
///
/// The registry is rebuilt fresh every hydrology tick (stateless derivation,
/// §2.2); [`WaterBody::id`] is the basin's lowest [`HexId`], so bodies keep
/// stable identities across ticks as they grow, shrink, split, and merge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WaterBody {
    /// Stable identifier: lowest [`HexId`] of the basin.
    pub id: WaterBodyId,
    /// Body classification.
    pub kind: WaterBodyKind,
    /// Water surface elevation in meters.
    pub surface_m: f32,
    /// Surface area in km².
    pub area_km2: f64,
    /// Volume in km³.
    pub volume_km3: f64,
    /// Salinity (arbitrary units until §5 salt balance; 0 for oceans).
    pub salinity: f32,
    /// Spill outlet hex for overflowing bodies (§5; `None` until Slice 2).
    pub outlet: Option<HexId>,
}

/// Packed per-hex hydrology feature flags (Doc 08 §2.4).
///
/// A plain `u8` newtype rather than the `bitflags` crate: no first-party crate
/// depends on `bitflags`, and const flags + `contains`/`bitor` cover the
/// module's needs without a new dependency.
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct HydroFlags(pub u16);

impl HydroFlags {
    /// No features.
    pub const NONE: Self = Self(0);
    /// Spring emerges at this hex (§6).
    pub const SPRING: Self = Self(1 << 0);
    /// Oasis at this hex (§6).
    pub const OASIS: Self = Self(1 << 1);
    /// Karst drainage diversion (§6).
    pub const KARST: Self = Self(1 << 2);
    /// Drowned river mouth (§11.2).
    pub const ESTUARY: Self = Self(1 << 3);
    /// Glacially carved, ocean-flooded trough (§9.2).
    pub const FJORD: Self = Self(1 << 4);
    /// Ephemeral (non-perennial) channel (§7).
    pub const EPHEMERAL: Self = Self(1 << 5);
    /// Wetland (§10.2, §11.3).
    pub const WETLAND: Self = Self(1 << 6);
    /// Sea ice cover (§9).
    pub const SEA_ICE: Self = Self(1 << 7);
    /// Persistent glacial trough scar (§9.2) — survives ice retreat.
    pub const CARVED_TROUGH: Self = Self(1 << 8);
    /// Prograding Major river mouth (§8.3 / §11.2).
    pub const DELTA: Self = Self(1 << 9);

    /// True when no flags are set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// True when all bits of `other` are set in `self`.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Clears all bits of `other` from `self`.
    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }
}

impl std::ops::BitOr for HydroFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for HydroFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Soil classification (Doc 08 §10.1).
///
/// Assigned by a deterministic decision tree in the soil system (Slice 2);
/// priority: ice/water → Saline → Loess → Alluvial → Volcanic → Calcareous →
/// Peaty → Sandy → Loamy.
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default, Serialize, Deserialize,
)]
#[repr(u8)]
pub enum SoilClass {
    /// Bare rock / active ice / open water.
    #[default]
    None,
    /// Floodplain & delta deposition.
    Alluvial,
    /// Wind-blown glacial flour (§9.2) — deep, fertile.
    Loess,
    /// Young igneous / recent volcanism.
    Volcanic,
    /// Limestone / marine-sediment bedrock.
    Calcareous,
    /// Arid, thin.
    Sandy,
    /// Cold + wet + flat (wetland).
    Peaty,
    /// Salt-poisoned.
    Saline,
    /// Temperate default.
    Loamy,
}

/// Stream/River/Major class boundaries, m³/yr (Doc 08 §4.4 / §12.2).
pub const STREAM_CLASS_MIN_M3_YR: f64 = 1.0e9;
/// See [`STREAM_CLASS_MIN_M3_YR`].
pub const RIVER_CLASS_MIN_M3_YR: f64 = 1.0e10;
/// See [`STREAM_CLASS_MIN_M3_YR`].
pub const MAJOR_CLASS_MIN_M3_YR: f64 = 1.0e11;

/// §4.4 river classes by annual discharge (shared by sim + render).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum RiverClass {
    /// < 1e9 m³/yr — sub-hex only (§12.3).
    Creek,
    /// 1e9–1e10 m³/yr.
    Stream,
    /// 1e10–1e11 m³/yr.
    River,
    /// > 1e11 m³/yr.
    Major,
}

/// §4.4 classification of a discharge (m³/yr).
pub fn river_class(discharge_m3_yr: f64) -> RiverClass {
    if discharge_m3_yr >= MAJOR_CLASS_MIN_M3_YR {
        RiverClass::Major
    } else if discharge_m3_yr >= RIVER_CLASS_MIN_M3_YR {
        RiverClass::River
    } else if discharge_m3_yr >= STREAM_CLASS_MIN_M3_YR {
        RiverClass::Stream
    } else {
        RiverClass::Creek
    }
}
