//! Core infrastructure for Genesis Engine: hex grid, world data, time, RNG, persistence.

pub mod branches;
pub mod data;
pub mod events;
pub mod grid;
pub mod interventions;
pub mod lifecycle;
pub mod parameters;
pub mod persistence;
pub mod rng;
pub mod time;
pub mod world;

pub use branches::{Branch, BranchError, BranchId, BranchTree};
pub use data::{
    BedrockType, BiomeId, HotSpotId, NationId, PlateId, SettlementId, SpeciesId, WorldData,
    WorldYear,
};
pub use events::{Event, EventId, EventKind, EventLocation, EventLog, Significance};
pub use grid::isea3h;
pub use grid::{Direction, GridError, HexGrid, HexId};
pub use interventions::{
    Intervention, InterventionAction, InterventionId, InterventionLog, InterventionScope,
};
pub use lifecycle::{
    CreateWorldError, GenerationError, GenerationProgress, ProgressCallback,
    advance_with_coordinator, create_world, generate_full_history,
};
pub use parameters::{
    BiologyParameters, CivilizationParameters, ClimateInitialParameters, CoreParameters,
    GeologyParameters, GridParameters, ModEntry, ModManifest, ParameterExtensions,
    ParameterValidationError, ParameterValue, ParameterValueData, PlanetParameters, TimeParameters,
    WorldParameters, WorldSeed,
};
pub use persistence::{LoadedWorld, PersistenceError, SaveMeta, load_world, save_world};
pub use rng::{WorldRng, compute_effective_seed};
pub use time::{Era, SimulationLayer, TickCoordinator, WorldTime};
pub use world::World;
