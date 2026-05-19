//! Core infrastructure for Genesis Engine: hex grid, world data, time, RNG, persistence.

pub mod data;
pub mod grid;

pub use data::{
    BedrockType, BiomeId, NationId, PlateId, SettlementId, SpeciesId, WorldData, WorldYear,
};
pub use grid::isea3h;
pub use grid::{Direction, GridError, HexGrid, HexId};
