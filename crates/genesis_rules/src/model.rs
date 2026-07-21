//! The rule AST — declarative, serde-serializable content (Doc 11 §2).

use genesis_core::data::TraitId;
use serde::{Deserialize, Serialize};

/// Content id for a named rule, namespaced like other content ids
/// (e.g. `"core:guild.large_predator"`). Save files reference rules by this
/// stable string; a dense runtime index is a future perf refinement (Doc 11 §8).
pub type RuleId = String;

/// Namespaced key for a world/environment scalar fact (e.g. `"env:o2_fraction"`,
/// `"env:temperature_c"`). The engine is agnostic to what these mean — the
/// consumer populates them (Architecture §"rules are external content").
pub type ScalarKey = String;

/// A leaf test over the fact context. Deterministic and side-effect-free.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Predicate {
    /// The subject has this trait.
    HasTrait(TraitId),
    /// The subject lacks this trait.
    LacksTrait(TraitId),
    /// The subject has *every* listed trait (prerequisite gate, Doc 09 §2.3).
    HasAll(Vec<TraitId>),
    /// The subject has *at least one* listed trait.
    HasAny(Vec<TraitId>),
    /// The subject has *none* of the listed traits (exclusion gate, Doc 09 §2.3).
    HasNone(Vec<TraitId>),
    /// A world/environment scalar is ≥ `min` (missing scalar reads as 0.0).
    ScalarAtLeast { key: ScalarKey, min: f64 },
    /// A world/environment scalar is < `max` (missing scalar reads as 0.0).
    ScalarBelow { key: ScalarKey, max: f64 },
}

/// A boolean rule tree over predicates. Children are evaluated in vector order —
/// fixed and deterministic (Doc 09 §14).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Rule {
    /// A single predicate.
    Pred(Predicate),
    /// True iff *every* child is true (AND). Empty ⇒ true.
    All(Vec<Rule>),
    /// True iff *any* child is true (OR). Empty ⇒ false.
    Any(Vec<Rule>),
    /// Logical negation.
    Not(Box<Rule>),
    /// A constant — a placeholder or explicit default.
    Const(bool),
}
