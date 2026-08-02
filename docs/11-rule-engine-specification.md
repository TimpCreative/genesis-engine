# 11 — Rule Engine Specification

**Document Type:** Tier 2 — System Specification
**Status:** Draft v0.1 — P4-R landed (model + evaluator)
**Last Updated:** July 2026
**Owner:** Brax Johnson
**Implementing Phase:** 4 (Biology) — shared with Phase 5 (Technology)

**Changelog:**
- v0.1 (July 2026): Initial draft + first implementation (`genesis_rules`). Selects the rule **format** Architecture §"pluggable rule format" deferred here — a **declarative predicate AST**, not a scripting language — and specifies the model, evaluation, determinism, consumer mappings, and modding/safety. Originally scoped as the "Technology Rule System"; generalized to the shared biology+technology evaluator per Doc 09 §2.8.

---

## 1. Purpose and Scope

The engine is **one small, deterministic, data-driven evaluator** shared by every system whose behavior is moddable content rather than hardcoded Rust: biology (trait-step legality, guild membership, innovation thresholds — Doc 09 §2.8), technology (Doc 10), and later biome definitions (Doc 09 §4.6) and civilization rules. Per Architecture §"pluggable rule format", the commitment is that **rules are external content, evaluated by an engine that does not know what specific rules exist**.

### 1.1 The format decision (declarative AST, not scripting)

Architecture deferred the choice between a declarative data format, an embedded scripting language (Lua/Rhai), and a custom DSL, under three hard constraints: **moddable, deterministic, safe to evaluate against untrusted content**. This spec chooses a **declarative predicate AST** authored as serde data (§2):

- **Deterministic by construction.** A finite boolean tree over predicates has no wall-clock, no I/O, no unbounded iteration, and produces a *boolean* — nothing that can float-diverge the simulation's branching topology (Doc 09 §14). Scripting languages are the opposite: making Lua/Rhai byte-reproducible across platforms and versions is a continuous fight.
- **Safe against untrusted mods.** There is no code execution — a malicious rule can at worst be verbose. A scripting sandbox is a large, ongoing security surface.
- **Cheap and cache-resident.** The AST is small static content, evaluated in microseconds at speciation/innovation events, not per-hex-per-tick.

The cost — less raw expressiveness than a scripting language — is acceptable: the predicates in §2 cover the biology and tech gates, and the set is extensible content-side.

### 1.2 Scope boundary — gates, not scores

The engine answers **legality / membership** questions that are *boolean*: "are the prerequisites met?", "does this trait set fill this guild?", "has this innovation threshold been crossed?". It does **not** own **continuous scoring** — the biased walk's proximity weights and selective payoff (Doc 09 §2.4–§2.5) are weights, not rules, and live in the consuming module. A clean split: the engine says *whether* a step is allowed; biology decides *how likely* an allowed step is.

### 1.3 Non-Goals

- No scripting, no arithmetic expression language, no user-defined functions (v1).
- No mutation — evaluation is a pure read; rules never write world state.
- No scoring/weighting (see §1.2).
- No cross-entity queries (a rule sees one subject's facts, not the whole world).

---

## 2. The Rule Model

Implemented in `genesis_rules` (`model.rs`, `engine.rs`). The types below are the contract.

**Predicate** — a leaf test:

```rust
pub enum Predicate {
    HasTrait(TraitId),
    LacksTrait(TraitId),
    HasAll(Vec<TraitId>),                       // prerequisite gate (Doc 09 §2.3)
    HasAny(Vec<TraitId>),
    HasNone(Vec<TraitId>),                       // exclusion gate (Doc 09 §2.3)
    ScalarAtLeast { key: ScalarKey, min: f64 },  // e.g. env:o2_fraction ≥ 0.1
    ScalarBelow   { key: ScalarKey, max: f64 },
}
```

**Rule** — a boolean tree; children evaluate in vector order:

```rust
pub enum Rule { Pred(Predicate), All(Vec<Rule>), Any(Vec<Rule>), Not(Box<Rule>), Const(bool) }
```

**FactContext** — what a rule is evaluated against: the subject's `BTreeSet<TraitId>` plus a `BTreeMap<ScalarKey, f64>` of named environment scalars. Ordered collections keep evaluation deterministic. The consumer populates it (the engine is agnostic to what a scalar *means*).

**RuleRegistry** — a `BTreeMap<RuleId, Rule>` of named rules: the moddable content bag. `RuleId` and `ScalarKey` are namespaced content strings (`"core:guild.large_predator"`, `"env:o2_fraction"`), stable across save/mod versions (a dense runtime index is a future perf refinement, §9).

---

## 3. Evaluation & Determinism

- **`evaluate(&Rule, &FactContext) -> bool`** is pure, recursive, side-effect-free; recursion follows fixed vector order (Doc 09 §14).
- **Missing scalar reads as `0.0`** — deterministic default: an unset scalar fails a `≥` test and passes a `<` test rather than erroring.
- **`RuleRegistry::eval` is fail-closed** — an unknown rule id evaluates to `false`, so an unrecognized gate never grants a capability.
- No floating-point value ever feeds back into branching topology; comparisons yield booleans. Continuous quantities stay in the consumer under its own fixed-point/ordering discipline.

---

## 4. Consumers & Mappings

| Consumer | Question | Rule shape |
|---|---|---|
| Biology — trait-step legality (Doc 09 §2.3) | Is this walk step reachable? | `trait_gate(prereqs, exclusions)` = `All[HasAll(prereqs), HasNone(exclusions)]` |
| Biology — guild membership (Doc 09 §4.1) | Does this trait set fill this role? | e.g. `All[HasTrait(heterotrophy), HasAny[limbed, swim], ScalarAtLeast{size…}]` |
| Biology — innovation thresholds (Doc 09 §3.3) | Can this unlock fire? | `All[HasTrait(multicellular), ScalarAtLeast{o2…}]` |
| Technology (Doc 10) | Are this tech's prerequisites met? | prereq tech/resource predicates (same shape, tech content) |
| Biomes (Doc 09 §4.6) | Does this hex qualify for this biome? | climate/soil scalar envelopes + producer-trait presence |

`genesis_rules::trait_gate(prerequisites, exclusions)` is a provided helper for the ubiquitous biology reachability gate.

---

## 5. Content, Modding & Safety

- Rules are **content**: authored data (serde; RON/JSON/TOML at the mod loader's discretion), keyed by namespaced `RuleId`. The core pack ships the base corpus; mods **insert or override** by id (last writer wins, deterministic by load order — Architecture §10).
- **Safe by construction:** a rule is a finite data tree — no code, no I/O, no loops — so evaluating untrusted mod content cannot execute anything or diverge determinism.
- Physics/engine constants and the determinism rules are engine-owned, not rule content (consistent with Doc 09 §16).

---

## 6. Crate & Integration

- **`genesis_rules`** — depends only on `genesis_core` (for `TraitId`); no Bevy. Consumed by `genesis_biology` (P4-2 onward), `genesis_tech` (Doc 10), and later `genesis_civilization`.
- The mod loader (`genesis_mods`) is the future producer of `RuleRegistry` content from disk; until then registries are built in code by the consuming layer.

---

## 7. Determinism & Performance

- The registry is small, static, cache-resident; evaluation is a handful of set/map lookups per gate. Budgeted well inside biology's per-tick allowance (Doc 09 §15).
- Future perf: intern `RuleId`/`ScalarKey`/`TraitId` to dense indices; precompile hot rules to bitset ops over the trait set.

---

## 8. Validation

Covered by `genesis_rules` tests (6): every predicate; `All`/`Any`/`Not`/`Const` including empty-`All`⇒true / empty-`Any`⇒false; missing-scalar-as-0.0; the `trait_gate` reachability pattern (prereq present + exclusion absent ⇒ true; missing prereq ⇒ false; exclusion present ⇒ false); registry fail-closed on unknown id; serde round-trip (rules are content). Add: byte-identical evaluation across runs when the biology determinism suite lands (Doc 09 §17 #10).

---

## 9. Open Questions (future)

1. **Dense id interning** for `RuleId`/`ScalarKey` (perf; §7).
2. **Richer predicates** — count comparisons, scalar-vs-scalar comparisons, ranges — added as content needs surface.
3. **Rule references** — a rule invoking another by id (composition/reuse) with cycle detection.
4. **Typed scalar keys** — a registry of known scalar keys for authoring validation, vs the current open string space.

---

## 10. Implementation Status

- **P4-R (landed):** `genesis_rules` crate — `model.rs` (Predicate/Rule/ids), `engine.rs` (FactContext/evaluate/RuleRegistry/trait_gate), 6 tests, clippy+fmt clean, added to the workspace. Consumed first by biology P4-2 (trait morphospace).

---

*Living spec — extend as biology (P4-2/P4-5) and technology (Doc 10) exercise it.*
