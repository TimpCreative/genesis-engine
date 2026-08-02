//! Persistent biology simulation state, owned across ticks.
//!
//! P4-3 holds the realized origin, the low-resolution microbial-era genome, the
//! oxygenation proxy, the once-only milestone set, and a buffered event queue
//! (flushed to the branch by [`crate::events::flush_events_to_branch`]). The
//! full biogeographic-province registry and ecological ledger (Doc 09 §5.1,
//! §8.1) arrive in later slices.

use std::collections::BTreeSet;

use genesis_core::HexId;
use genesis_core::events::Event;
use genesis_core::time::WorldYear;

use crate::core_graph::core_morphospace;
use crate::guild::{GuildRoster, core_guilds};
use crate::ledger::Ledger;
use crate::morphospace::{TraitGraph, TraitSet};
use crate::province::ProvinceRegistry;

/// The realized origin of life (Doc 09 §3.1) — a marine vent hex and the year.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Origin {
    pub hex: HexId,
    pub year: WorldYear,
}

/// Microbial-era milestones, each surfaced once (Doc 09 §3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Milestone {
    LifeEmerged,
    OxygenicPhotosynthesis,
    GreatOxygenation,
    Eukaryogenesis,
    Multicellularity,
}

/// Biology's persistent, branch-scoped simulation state.
#[derive(Debug)]
pub struct BiologyState {
    /// The loaded core morphospace (static content, Doc 09 §2.9).
    pub(crate) graph: TraitGraph,
    /// The functional-guild roster (static content, Doc 09 §4.1).
    pub(crate) guilds: GuildRoster,
    /// `Some` once biogenesis has succeeded (single origin, Doc 09 §3.1).
    pub(crate) origin: Option<Origin>,
    /// The evolving microbial-biosphere genome (low-resolution, Doc 09 §3.3).
    pub(crate) root_genome: TraitSet,
    /// Cached mirror of the world's atmospheric O₂ (`WorldData::
    /// atmospheric_oxygen_fraction`), kept in sync each tick so `o2_fraction()`
    /// callers need not hold the world. The world field is the authority (§11.1).
    pub(crate) o2_fraction: f32,
    /// Milestones already fired.
    pub(crate) milestones: BTreeSet<Milestone>,
    /// Biogeographic provinces for the current geography (Doc 09 §5.1).
    pub(crate) provinces: ProvinceRegistry,
    /// The recorded tree of life, built at the multicellular radiation (§8.1).
    pub(crate) ledger: Ledger,
    /// Events buffered for the branch log.
    pub(crate) pending_events: Vec<Event>,
    /// Monotonic event-id counter.
    pub(crate) next_event_id: u64,
    /// Signature of the geography/climate inputs the last time the heavy biology
    /// fields (provinces, richness, biomes, biomass, occupancy) were recomputed —
    /// the dirty-flag that lets a heavy-stride tick skip the O(n) recompute when
    /// nothing relevant has changed (Doc 09 §15; limitations 9 & 13).
    pub(crate) heavy_signature: Option<u64>,
}

impl Default for BiologyState {
    fn default() -> Self {
        let graph = core_morphospace();
        let guilds = core_guilds(&graph);
        Self {
            graph,
            guilds,
            origin: None,
            root_genome: TraitSet::new(),
            o2_fraction: 0.0,
            milestones: BTreeSet::new(),
            provinces: ProvinceRegistry::default(),
            ledger: Ledger::default(),
            pending_events: Vec::new(),
            next_event_id: 0,
            heavy_signature: None,
        }
    }
}

impl BiologyState {
    /// Creates empty biology state (with the core graph loaded).
    pub fn new() -> Self {
        Self::default()
    }

    /// The realized origin, if life has emerged.
    pub fn origin(&self) -> Option<Origin> {
        self.origin
    }

    /// The current oxygenation proxy ∈ [0, 0.21].
    pub fn o2_fraction(&self) -> f32 {
        self.o2_fraction
    }

    /// The microbial-era genome (read-only).
    pub fn root_genome(&self) -> &TraitSet {
        &self.root_genome
    }

    /// Whether a milestone has fired.
    pub fn has_milestone(&self, milestone: Milestone) -> bool {
        self.milestones.contains(&milestone)
    }

    /// Number of buffered (not-yet-flushed) events.
    pub fn pending_event_count(&self) -> usize {
        self.pending_events.len()
    }

    /// The current biogeographic provinces (Doc 09 §5.1).
    pub fn provinces(&self) -> &ProvinceRegistry {
        &self.provinces
    }

    /// The functional-guild roster (Doc 09 §4.1).
    pub fn guilds(&self) -> &GuildRoster {
        &self.guilds
    }

    /// The recorded tree of life (Doc 09 §8.1).
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    /// Consumes the state, yielding the recorded ledger (for hand-off to the
    /// viewer / export).
    pub fn into_ledger(self) -> Ledger {
        self.ledger
    }
}
