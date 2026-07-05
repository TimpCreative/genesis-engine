//! Placeholder types for climate fields. The full regime enum lives in `genesis_climate`;
//! this is the storage type for [`WorldData`](super::WorldData) per-hex labels.

/// Per-hex Köppen-like regime label stored in [`WorldData::climate_regime`](super::WorldData::climate_regime).
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
#[derive(Default)]
pub enum ClimateRegimePlaceholder {
    #[default]
    Unset = 0,
    Tropical = 1,
    Subtropical = 2,
    HotDesert = 3,
    ColdDesert = 4,
    Mediterranean = 5,
    Temperate = 6,
    ContinentalCool = 7,
    Boreal = 8,
    Tundra = 9,
    Polar = 10,
}
