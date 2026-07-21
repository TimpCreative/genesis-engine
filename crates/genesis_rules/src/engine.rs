//! Fact context, evaluator, and the named-rule registry (Doc 11 §2–§3).

use std::collections::{BTreeMap, BTreeSet};

use genesis_core::data::TraitId;

use crate::model::{Predicate, Rule, RuleId, ScalarKey};

/// The facts a rule is evaluated against: a subject's trait set plus named
/// environment scalars. Both are ordered collections so iteration — and thus
/// evaluation — is deterministic (Doc 09 §14).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FactContext {
    traits: BTreeSet<TraitId>,
    scalars: BTreeMap<ScalarKey, f64>,
}

impl FactContext {
    /// An empty context (no traits, no scalars).
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder: adds a trait.
    pub fn with_trait(mut self, id: TraitId) -> Self {
        self.traits.insert(id);
        self
    }

    /// Builder: sets a scalar.
    pub fn with_scalar(mut self, key: impl Into<ScalarKey>, value: f64) -> Self {
        self.scalars.insert(key.into(), value);
        self
    }

    /// Adds a trait to the subject.
    pub fn insert_trait(&mut self, id: TraitId) {
        self.traits.insert(id);
    }

    /// Sets a world/environment scalar.
    pub fn set_scalar(&mut self, key: impl Into<ScalarKey>, value: f64) {
        self.scalars.insert(key.into(), value);
    }

    /// Whether the subject has `id`.
    pub fn has_trait(&self, id: TraitId) -> bool {
        self.traits.contains(&id)
    }

    /// A scalar's value; **missing scalars read as 0.0** — a deterministic
    /// default so a rule referencing an unset scalar fails a `≥` test and passes
    /// a `<` test rather than erroring.
    pub fn scalar(&self, key: &str) -> f64 {
        self.scalars.get(key).copied().unwrap_or(0.0)
    }

    /// The subject's trait set.
    pub fn traits(&self) -> &BTreeSet<TraitId> {
        &self.traits
    }
}

/// Evaluates `rule` against `facts`. Pure, deterministic, side-effect-free
/// (Doc 09 §14): recursion follows fixed vector order and comparisons are total.
pub fn evaluate(rule: &Rule, facts: &FactContext) -> bool {
    match rule {
        Rule::Pred(p) => eval_predicate(p, facts),
        Rule::All(rules) => rules.iter().all(|r| evaluate(r, facts)),
        Rule::Any(rules) => rules.iter().any(|r| evaluate(r, facts)),
        Rule::Not(inner) => !evaluate(inner, facts),
        Rule::Const(b) => *b,
    }
}

fn eval_predicate(p: &Predicate, facts: &FactContext) -> bool {
    match p {
        Predicate::HasTrait(id) => facts.has_trait(*id),
        Predicate::LacksTrait(id) => !facts.has_trait(*id),
        Predicate::HasAll(ids) => ids.iter().all(|id| facts.has_trait(*id)),
        Predicate::HasAny(ids) => ids.iter().any(|id| facts.has_trait(*id)),
        Predicate::HasNone(ids) => !ids.iter().any(|id| facts.has_trait(*id)),
        Predicate::ScalarAtLeast { key, min } => facts.scalar(key) >= *min,
        Predicate::ScalarBelow { key, max } => facts.scalar(key) < *max,
    }
}

/// The canonical trait-step legality gate (Doc 09 §2.3): a step is legal iff
/// **every** prerequisite is present and **none** of the exclusions are.
pub fn trait_gate(prerequisites: Vec<TraitId>, exclusions: Vec<TraitId>) -> Rule {
    Rule::All(vec![
        Rule::Pred(Predicate::HasAll(prerequisites)),
        Rule::Pred(Predicate::HasNone(exclusions)),
    ])
}

/// A named set of rules — the moddable content the engine evaluates without
/// knowing what specific rules exist (Architecture §"pluggable rule format").
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuleRegistry {
    rules: BTreeMap<RuleId, Rule>,
}

impl RuleRegistry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts (or overrides) a named rule. Mods override by re-inserting an id.
    pub fn insert(&mut self, id: impl Into<RuleId>, rule: Rule) {
        self.rules.insert(id.into(), rule);
    }

    /// Looks up a rule by id.
    pub fn get(&self, id: &str) -> Option<&Rule> {
        self.rules.get(id)
    }

    /// Evaluates a named rule. An **unknown id is `false`** (fail-closed — an
    /// unrecognized gate never grants a capability).
    pub fn eval(&self, id: &str, facts: &FactContext) -> bool {
        self.rules.get(id).is_some_and(|r| evaluate(r, facts))
    }

    /// Number of registered rules.
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(n: u32) -> TraitId {
        TraitId(n)
    }

    #[test]
    fn trait_predicates() {
        let facts = FactContext::new().with_trait(t(1)).with_trait(t(2));
        assert!(evaluate(&Rule::Pred(Predicate::HasTrait(t(1))), &facts));
        assert!(evaluate(&Rule::Pred(Predicate::LacksTrait(t(9))), &facts));
        assert!(evaluate(
            &Rule::Pred(Predicate::HasAll(vec![t(1), t(2)])),
            &facts
        ));
        assert!(!evaluate(
            &Rule::Pred(Predicate::HasAll(vec![t(1), t(3)])),
            &facts
        ));
        assert!(evaluate(
            &Rule::Pred(Predicate::HasAny(vec![t(3), t(2)])),
            &facts
        ));
        assert!(evaluate(
            &Rule::Pred(Predicate::HasNone(vec![t(8), t(9)])),
            &facts
        ));
        assert!(!evaluate(
            &Rule::Pred(Predicate::HasNone(vec![t(2), t(9)])),
            &facts
        ));
    }

    #[test]
    fn scalar_predicates_default_missing_to_zero() {
        let facts = FactContext::new().with_scalar("env:o2_fraction", 0.21);
        assert!(evaluate(
            &Rule::Pred(Predicate::ScalarAtLeast {
                key: "env:o2_fraction".into(),
                min: 0.1
            }),
            &facts
        ));
        assert!(!evaluate(
            &Rule::Pred(Predicate::ScalarAtLeast {
                key: "env:o2_fraction".into(),
                min: 0.5
            }),
            &facts
        ));
        // Missing scalar reads as 0.0: fails ≥, passes <.
        assert!(!evaluate(
            &Rule::Pred(Predicate::ScalarAtLeast {
                key: "env:missing".into(),
                min: 0.1
            }),
            &facts
        ));
        assert!(evaluate(
            &Rule::Pred(Predicate::ScalarBelow {
                key: "env:missing".into(),
                max: 0.1
            }),
            &facts
        ));
    }

    #[test]
    fn combinators() {
        let facts = FactContext::new().with_trait(t(1));
        let all_true = Rule::All(vec![Rule::Const(true), Rule::Const(true)]);
        let all_false = Rule::All(vec![Rule::Const(true), Rule::Const(false)]);
        let any_true = Rule::Any(vec![Rule::Const(false), Rule::Const(true)]);
        assert!(evaluate(&all_true, &facts));
        assert!(!evaluate(&all_false, &facts));
        assert!(evaluate(&any_true, &facts));
        assert!(evaluate(&Rule::Not(Box::new(Rule::Const(false))), &facts));
        // Empty All ⇒ true, empty Any ⇒ false.
        assert!(evaluate(&Rule::All(vec![]), &facts));
        assert!(!evaluate(&Rule::Any(vec![]), &facts));
    }

    #[test]
    fn trait_gate_matches_doc9_reachability() {
        // Step legal iff all prereqs present and no exclusions present (Doc 09 §2.3).
        let gate = trait_gate(vec![t(1), t(2)], vec![t(3)]);
        let ok = FactContext::new().with_trait(t(1)).with_trait(t(2));
        let missing_prereq = FactContext::new().with_trait(t(1));
        let has_exclusion = FactContext::new()
            .with_trait(t(1))
            .with_trait(t(2))
            .with_trait(t(3));
        assert!(evaluate(&gate, &ok));
        assert!(!evaluate(&gate, &missing_prereq));
        assert!(!evaluate(&gate, &has_exclusion));
    }

    #[test]
    fn registry_eval_and_fail_closed() {
        let mut reg = RuleRegistry::new();
        reg.insert("core:has_metabolism", Rule::Pred(Predicate::HasTrait(t(1))));
        let facts = FactContext::new().with_trait(t(1));
        assert!(reg.eval("core:has_metabolism", &facts));
        // Unknown rule id fails closed.
        assert!(!reg.eval("core:does_not_exist", &facts));
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn rules_are_serde_content() {
        let rule = trait_gate(vec![t(1)], vec![t(2)]);
        let json = serde_json::to_string(&rule).expect("serialize");
        let back: Rule = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(rule, back);
    }
}
