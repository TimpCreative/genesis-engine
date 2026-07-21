//! The ecological ledger — the recorded tree of life (Doc 09 §8.1, §9).
//!
//! A forest of [`LineageRecord`]s rooted at LUCA. Each lineage descends from its
//! parent with **one trait added** (descent with modification), so a clade
//! genuinely resembles its ancestors, and the **Linnaean rank** of a branch is
//! the tier of the trait that distinguishes it (Doc 09 §9.2). Phylogeny (the
//! parent structure) and classification (the ranks) are two readings of the same
//! ledger. Built by [`crate::speciation`]; consumed by the `BiologyView` adapter.

use genesis_core::data::{GuildId, LineageId, TraitId};
use genesis_core::time::WorldYear;

use crate::morphospace::TraitSet;

/// One functional lineage in the tree of life (Doc 09 §8.1).
#[derive(Clone, Debug, PartialEq)]
pub struct LineageRecord {
    pub id: LineageId,
    pub parent: Option<LineageId>,
    pub origin_year: WorldYear,
    /// `Some(year)` once extinct; `None` while extant (greyed in the tree).
    pub extinction_year: Option<WorldYear>,
    /// The genome at this node (a subset of the morphospace).
    pub trait_set: TraitSet,
    /// The trait gained vs. the parent — drives the taxonomic rank (§9.2).
    pub trait_delta: Option<TraitId>,
    /// Functional role once specialized; `GuildId::NONE` for internal clades.
    pub guild: GuildId,
    /// Biogeographic region this clade is endemic to (`None` = cosmopolitan /
    /// LUCA / kingdom roots), for endemism (Doc 09 §6.4).
    pub region: Option<u16>,
    /// Deterministic naming anchor.
    pub name_seed: u64,
}

impl LineageRecord {
    /// Whether this lineage is extant at `year` (arisen and not yet extinct).
    pub fn alive_at(&self, year: WorldYear) -> bool {
        self.origin_year <= year && self.extinction_year.is_none_or(|e| e > year)
    }
}

/// The tree of life: a dense lineage forest (`LineageId(k)` = `lineages[k]`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ledger {
    pub lineages: Vec<LineageRecord>,
}

impl Ledger {
    /// Adds a lineage, assigning its dense id, and returns that id.
    pub fn push(&mut self, mut record: LineageRecord) -> LineageId {
        let id = LineageId(self.lineages.len() as u64);
        record.id = id;
        self.lineages.push(record);
        id
    }

    pub fn get(&self, id: LineageId) -> Option<&LineageRecord> {
        self.lineages.get(id.0 as usize)
    }

    pub fn len(&self) -> usize {
        self.lineages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lineages.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &LineageRecord> {
        self.lineages.iter()
    }

    /// Lineages of a given guild that are extant at `year`.
    pub fn extant_in_guild(
        &self,
        guild: GuildId,
        year: WorldYear,
    ) -> impl Iterator<Item = &LineageRecord> {
        self.lineages
            .iter()
            .filter(move |l| l.guild == guild && l.alive_at(year))
    }

    /// Lineages of `guild` endemic to `region` (or cosmopolitan) extant at
    /// `year` — the basis of endemic assemblages (Doc 09 §6.4).
    pub fn extant_in_guild_region(
        &self,
        guild: GuildId,
        region: u16,
        year: WorldYear,
    ) -> impl Iterator<Item = &LineageRecord> {
        self.lineages.iter().filter(move |l| {
            l.guild == guild && l.alive_at(year) && l.region.is_none_or(|r| r == region)
        })
    }
}

/// The Linnaean rank a distinguishing trait's tier confers (Doc 09 §9.2).
pub fn rank_for_tier(tier: u8) -> &'static str {
    match tier {
        0 => "kingdom",
        1 => "phylum",
        2 => "class",
        3 => "order",
        4 => "family",
        5 => "genus",
        _ => "species",
    }
}
