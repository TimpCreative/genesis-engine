//! Biology simulation layer (Doc 09) — the first Layer 1 module.
//!
//! Owns (across future slices) the trait morphospace walk, guild/province
//! dynamics, the ecological ledger, and the `genesis_core::biology_view`
//! adapter. **P4-3** makes the layer live: biogenesis at a marine vent and the
//! innovation-gated microbial era (see `docs/09-biology-foundation-plan.md`).

pub mod biogenesis;
pub mod biome;
pub mod core_graph;
pub mod events;
pub mod evolution;
pub mod guild;
pub mod layer;
pub mod ledger;
pub mod microbial;
pub mod morphospace;
pub mod naming;
pub mod population;
pub mod province;
pub mod richness;
pub mod speciation;
pub mod state;
pub mod trait_data;
pub mod view;

pub use biome::{assign_biomes, biome_name};
pub use core_graph::core_morphospace;
pub use events::flush_events_to_branch;
pub use evolution::{
    NeutralPayoff, SelectivePayoff, WalkParams, biased_walk_step, candidate_weights,
};
pub use guild::{Guild, GuildRoster, core_guilds, fills_guild};
pub use layer::{BiologyLayer, DEFAULT_BIOLOGY_TICK_YEARS};
pub use ledger::{Ledger, LineageRecord, rank_for_tier};
pub use morphospace::{RawTraitNode, TraitAxis, TraitGraph, TraitNode, TraitSet, TraitTag};
pub use population::{compute_biomass, compute_guild_occupancy};
pub use province::{BiogeographicProvince, ProvinceRegistry, Realm, label_provinces};
pub use richness::{
    compute_primary_productivity, compute_richness, occupied_guild_count, species_in_guild,
};
pub use speciation::build_radiation;
pub use state::{BiologyState, Milestone, Origin};
pub use view::GeneratedBiologyView;
