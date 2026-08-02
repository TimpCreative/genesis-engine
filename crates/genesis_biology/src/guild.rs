//! Guilds — functional ecological roles (Doc 09 §4.1).
//!
//! A guild is "a way of making a living", bounded moddable content keyed by
//! realm. Membership is a **rule** (Doc 11): a lineage fills a guild when its
//! trait set satisfies the guild's `membership` rule. The roster is authored
//! against the core morphospace by trait name.
//!
//! P4-5 lands the roster + membership machinery + the saturation cap (§4.5).
//! *Occupancy* — which guild is filled by which lineage in a province — needs
//! the lineage ledger and speciation (P4-8/P4-10) and is not yet computed; a
//! province currently derives only an occupied-guild *count* from richness
//! (§4.4, see [`crate::richness`]).

use genesis_core::data::GuildId;
use genesis_rules::{FactContext, Predicate, Rule, evaluate};

use crate::morphospace::{TraitGraph, TraitSet};
use crate::province::Realm;

/// One functional guild and the rule that decides membership (Doc 09 §4.1).
#[derive(Clone, Debug)]
pub struct Guild {
    pub id: GuildId,
    pub name: &'static str,
    pub realm: Realm,
    pub membership: Rule,
}

/// The bounded, moddable guild roster (Doc 09 §4.1).
#[derive(Clone, Debug, Default)]
pub struct GuildRoster {
    guilds: Vec<Guild>,
}

impl GuildRoster {
    pub fn len(&self) -> usize {
        self.guilds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.guilds.is_empty()
    }

    pub fn get(&self, id: GuildId) -> Option<&Guild> {
        self.guilds.get(id.0 as usize)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Guild> {
        self.guilds.iter()
    }

    /// Guilds belonging to `realm`.
    pub fn for_realm(&self, realm: Realm) -> impl Iterator<Item = &Guild> {
        self.guilds.iter().filter(move |g| g.realm == realm)
    }
}

/// Whether `trait_set` fills `guild` (Doc 09 §4.1) — evaluates the membership
/// rule against the trait set via the rule engine (Doc 11).
pub fn fills_guild(guild: &Guild, trait_set: &TraitSet) -> bool {
    let mut facts = FactContext::new();
    for id in trait_set.iter() {
        facts.insert_trait(id);
    }
    evaluate(&guild.membership, &facts)
}

/// A coarse "specificity" score for a membership rule: how many traits it
/// *requires*. Used to pick the **most-specific** guild a lineage fills, so a
/// predator that still carries ancestral `chemosynthesis` (inherited from LUCA)
/// is tagged a predator, not the loose `producer` guild — the fix for every
/// lineage collapsing onto `producer`. More required traits ⇒ more specific.
pub fn rule_specificity(rule: &Rule) -> u32 {
    match rule {
        Rule::Pred(Predicate::HasTrait(_)) => 1,
        Rule::Pred(Predicate::HasAll(v)) => v.len() as u32,
        Rule::Pred(Predicate::HasAny(_)) => 1,
        Rule::Pred(_) => 0,
        Rule::All(rs) => rs.iter().map(rule_specificity).sum(),
        Rule::Any(rs) => rs.iter().map(rule_specificity).max().unwrap_or(0),
        Rule::Not(_) | Rule::Const(_) => 0,
    }
}

/// The hand-authored core guild roster, resolved against `graph`. A starter set
/// across both realms; the full roster (Doc 09 §4.1, §10.1) is expandable
/// content.
pub fn core_guilds(graph: &TraitGraph) -> GuildRoster {
    let t = |name: &str| graph.id_of(name).expect("core trait");
    let has = |name: &str| Predicate::HasTrait(t(name));
    let all = |preds: Vec<Predicate>| Rule::All(preds.into_iter().map(Rule::Pred).collect());
    let producer = || {
        Rule::Pred(Predicate::HasAny(vec![
            t("core:oxygenic_photosynthesis"),
            t("core:chemosynthesis"),
        ]))
    };
    let decomposer = || {
        Rule::Pred(Predicate::HasAny(vec![
            t("core:absorptive_decomposition"),
            t("core:detritivore"),
        ]))
    };

    use Realm::{Marine, Terrestrial};
    let defs: Vec<(&'static str, Realm, Rule)> = vec![
        ("producer", Terrestrial, producer()),
        (
            "herbivore",
            Terrestrial,
            all(vec![has("core:heterotrophy"), has("core:folivore")]),
        ),
        (
            "apex_predator",
            Terrestrial,
            all(vec![
                has("core:heterotrophy"),
                has("core:hypercarnivore"),
                has("core:limbed_walk"),
            ]),
        ),
        ("decomposer", Terrestrial, decomposer()),
        ("phytoplankton", Marine, producer()),
        (
            "filter_feeder",
            Marine,
            all(vec![has("core:heterotrophy"), has("core:filter_feeder")]),
        ),
        (
            "nekton_predator",
            Marine,
            all(vec![
                has("core:heterotrophy"),
                has("core:swim"),
                has("core:hypercarnivore"),
            ]),
        ),
        ("marine_decomposer", Marine, decomposer()),
    ];

    let guilds = defs
        .into_iter()
        .enumerate()
        .map(|(i, (name, realm, membership))| Guild {
            id: GuildId(i as u16),
            name,
            realm,
            membership,
        })
        .collect();
    GuildRoster { guilds }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core_graph::core_morphospace;

    fn trait_set(graph: &TraitGraph, names: &[&str]) -> TraitSet {
        names.iter().map(|n| graph.id_of(n).unwrap()).collect()
    }

    fn guild<'a>(roster: &'a GuildRoster, name: &str) -> &'a Guild {
        roster.iter().find(|g| g.name == name).unwrap()
    }

    #[test]
    fn roster_covers_both_realms() {
        let graph = core_morphospace();
        let roster = core_guilds(&graph);
        assert!(roster.for_realm(Realm::Terrestrial).count() >= 3);
        assert!(roster.for_realm(Realm::Marine).count() >= 3);
    }

    #[test]
    fn membership_rules_match_trait_sets() {
        let graph = core_morphospace();
        let roster = core_guilds(&graph);

        let photo = trait_set(&graph, &["core:oxygenic_photosynthesis"]);
        assert!(fills_guild(guild(&roster, "producer"), &photo));
        assert!(!fills_guild(guild(&roster, "apex_predator"), &photo));

        let predator = trait_set(
            &graph,
            &[
                "core:heterotrophy",
                "core:hypercarnivore",
                "core:limbed_walk",
            ],
        );
        assert!(fills_guild(guild(&roster, "apex_predator"), &predator));
        assert!(!fills_guild(guild(&roster, "producer"), &predator));

        let fungus = trait_set(&graph, &["core:absorptive_decomposition"]);
        assert!(fills_guild(guild(&roster, "decomposer"), &fungus));
    }
}
