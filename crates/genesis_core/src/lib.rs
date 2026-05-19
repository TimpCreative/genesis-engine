//! Core infrastructure for Genesis Engine: hex grid, world data, time, RNG, persistence.

pub mod data;
pub mod grid;
pub mod parameters;
pub mod rng;
pub mod time;

pub use data::{
    BedrockType, BiomeId, NationId, PlateId, SettlementId, SpeciesId, WorldData, WorldYear,
};
pub use grid::isea3h;
pub use grid::{Direction, GridError, HexGrid, HexId};
pub use parameters::{
    BiologyParameters, CivilizationParameters, ClimateInitialParameters, CoreParameters,
    GeologyParameters, GridParameters, ModEntry, ModManifest, ParameterExtensions,
    ParameterValidationError, ParameterValue, ParameterValueData, PlanetParameters, TimeParameters,
    WorldParameters, WorldSeed,
};
pub use rng::{WorldRng, compute_effective_seed};
pub use time::{Era, SimulationLayer, TickCoordinator, WorldTime};
