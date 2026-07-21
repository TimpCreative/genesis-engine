//! The Genesis rule engine (Doc 11) — a small, deterministic, data-driven
//! evaluator shared by biology (trait legality, guild membership, innovation
//! thresholds — Doc 09 §2.8) and, later, technology and civilization (Doc 10).
//!
//! **Format decision (Architecture §"pluggable rule format"): a declarative
//! predicate AST**, not an embedded scripting language. A finite boolean tree
//! over predicates is deterministic and safe to evaluate against untrusted mod
//! content by construction — no I/O, no unbounded loops, no floating-point
//! divergence in branching (comparisons yield booleans; Doc 09 §14).
//!
//! **Scope boundary:** the engine answers *legality/membership* questions
//! (boolean gates — "are the prerequisites met?", "does this trait set fill this
//! guild?"). Continuous scoring — the biased walk's proximity weights and
//! selective payoff (Doc 09 §2.4–§2.5) — stays in the consuming module; it is
//! not a rule.

pub mod engine;
pub mod model;

pub use engine::{FactContext, RuleRegistry, evaluate, trait_gate};
pub use model::{Predicate, Rule, RuleId, ScalarKey};
