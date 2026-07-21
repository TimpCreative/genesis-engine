//! The `BiologyView` read seam (Prep-09 §2).
//!
//! Everything the presentation layer asks of "life", decoupled from how it is
//! produced. `StubBiologyView` (in `genesis_ui`) answers it deterministically
//! from existing physical fields today; a `genesis_biology` adapter answers it
//! over the real ledger at Doc 09, and the UI does not change.
//!
//! The trait and its DTOs live here (pure Rust, no Bevy, no presentation) so
//! that both the consumer (`genesis_ui`) and the future producer
//! (`genesis_biology`) can depend on it without an inversion — the app wires the
//! chosen implementation as a resource at world load.
//!
//! These DTOs are intentionally **lossy** presentation types — enough to draw a
//! card, a row, a node, a pip. Doc 09's rich `LineageRecord`/`TraitSet` map
//! *into* them; the UI never sees the full model.

use crate::data::{BiomeId, WorldData};
use crate::grid::HexId;
use crate::time::WorldYear;

/// One functional guild's occupancy at a hex.
#[derive(Clone, Debug, PartialEq)]
pub struct GuildSummary {
    pub name: String,
    pub occupied: bool,
}

/// A species as the presentation needs it. A text card in Prep-09; the
/// `species_id` + `trait_chips` feed Doc 09's creature renderer into the same
/// card with no layout change.
#[derive(Clone, Debug, PartialEq)]
pub struct SpeciesPeek {
    /// Determinism anchor (becomes `SpeciesId` at Doc 09).
    pub species_id: u64,
    pub name: String,
    pub guild: String,
    /// Taxonomic grouping for the Bestiary drill-down (Family/Genus scaffold).
    pub family: String,
    pub trait_chips: Vec<String>,
    pub description: String,
}

/// The generated species assemblage for a hex (Prep-09 §8).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Assemblage {
    pub biome_name: String,
    pub richness: f32,
    pub occupied_guilds: u32,
    pub guild_capacity: u32,
    pub species: Vec<SpeciesPeek>,
}

/// One node of the tree-of-life peek (Prep-09 §7).
#[derive(Clone, Debug, PartialEq)]
pub struct TreeNodePeek {
    pub id: u64,
    pub parent: Option<u64>,
    pub name: String,
    /// Linnaean rank by **nesting depth** (kingdom → phylum → … → species), for
    /// the hover tooltip — monotonic, unlike the trait-tier rank.
    pub rank: String,
    /// Depth from the root (0 = LUCA), for indentation.
    pub depth: u32,
    /// The species id (`SpeciesPeek::species_id`) so a node opens its detail.
    pub species_id: u64,
    pub defining_trait: String,
    pub origin_year: i64,
    /// `None` while extant; a year once the branch is extinct.
    pub extinction_year: Option<i64>,
}

/// A snapshot of the tree of life as of a viewed year.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TreePeek {
    pub nodes: Vec<TreeNodePeek>,
}

/// Category of a life event, for pip glyph/color (Prep-09 §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifeEventCategory {
    /// Life emerges / a kingdom originates.
    Origin,
    /// A key innovation / adaptive radiation.
    Innovation,
    /// A mass extinction / catastrophe.
    Extinction,
    /// A biosphere milestone (oxygenation, sapience).
    Milestone,
}

/// A life-relevant event for a timeline pip.
#[derive(Clone, Debug, PartialEq)]
pub struct LifeEventPip {
    pub year: i64,
    pub label: String,
    pub category: LifeEventCategory,
}

/// Full detail for one species (its detail panel), including the Linnaean
/// **classification** ladder — the separate-from-phylogeny classification view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpeciesDetail {
    pub name: String,
    pub guild: String,
    pub family: String,
    pub description: String,
    pub trait_chips: Vec<String>,
    /// Each visible trait as `(display name, plain-English definition)` — for
    /// hover tooltips on the trait chips.
    pub trait_details: Vec<(String, String)>,
    /// Classification from a deep rank down to this species: `(rank, clade name)`
    /// pairs, e.g. `[("kingdom","Animals"), ("class","…"), … ("species","…")]`.
    pub classification: Vec<(String, String)>,
}

/// A species' trophic neighborhood — the "who eats whom" web (Doc 09 §5.3),
/// materialized on demand for its region.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FoodWeb {
    /// What this species eats (the guild one trophic level below, in-region).
    pub prey: Vec<SpeciesPeek>,
    /// What eats this species (the guild one level above, in-region).
    pub predators: Vec<SpeciesPeek>,
    /// Others sharing its guild in the same region (competitors).
    pub competitors: Vec<SpeciesPeek>,
}

/// Read-only contract the presentation consumes for "life". Pure reads — never
/// mutates world state. Spatial queries take `&WorldData` so the view is a pure
/// function of `(its own state, world, query)` (Prep-09 §11), which is what
/// keeps the stub and the Doc 09 adapter interchangeable.
pub trait BiologyView: Send + Sync {
    /// Biome id for a hex (`BiomeId::NONE` for ocean / unassigned).
    fn biome_at(&self, data: &WorldData, hex: HexId) -> BiomeId;

    /// Display name for a biome id.
    fn biome_name(&self, biome: BiomeId) -> String;

    /// Biotic richness scalar R ∈ [0,1] (Doc 09 §4.4) for a hex.
    fn richness_at(&self, data: &WorldData, hex: HexId) -> f32;

    /// Living-biomass proxy ∈ [0,1] for a hex (heatmap only).
    fn biomass_at(&self, data: &WorldData, hex: HexId) -> f32;

    /// Occupied functional guilds at a hex, headline-first.
    fn occupied_guilds(&self, data: &WorldData, hex: HexId) -> Vec<GuildSummary>;

    /// The generated species assemblage for a hex (materialized on demand).
    fn assemblage(&self, data: &WorldData, hex: HexId) -> Assemblage;

    /// A snapshot of the tree of life as of `year`: branches present, extinct
    /// branches marked (Doc 09 §9.3).
    fn tree_snapshot(&self, year: WorldYear) -> TreePeek;

    /// Life-relevant events overlapping `[from, to]` for timeline pips (§5).
    fn life_events(&self, from: WorldYear, to: WorldYear) -> Vec<LifeEventPip>;

    /// Full detail for a species by its id (a `SpeciesPeek::species_id`), incl.
    /// its classification ladder. Default `None`; the Doc 09 adapter overrides it.
    fn species_detail(&self, _species_id: u64) -> Option<SpeciesDetail> {
        None
    }

    /// The species' trophic neighbors (prey / predators / competitors) in its
    /// region, generated on demand (Doc 09 §5.3). Default empty.
    fn food_web(&self, _species_id: u64, _year: WorldYear) -> FoodWeb {
        FoodWeb::default()
    }

    /// The whole living catalog at `year` — every extant species across regions,
    /// ordered most-prominent-first (the global Bestiary, no hex needed). Default
    /// empty.
    fn species_catalog(&self, _year: WorldYear) -> Vec<SpeciesPeek> {
        Vec::new()
    }

    /// The name of the era's dominant clade at `year` — e.g. "Age of the
    /// Ventopus" (Doc 09 §9). Default `None`.
    fn dominant_clade(&self, _year: WorldYear) -> Option<String> {
        None
    }
}
