# Contributing as an AI Agent

This file explains how to work effectively on Genesis Engine if you are an AI coding agent (Cursor, Claude Code, an AI assistant accessed via API, etc.).

If you are a human contributor: this file isn't for you. There is currently no human contribution process — the project is closed-source and AI-built. This may change.

---

## Why This File Exists

Genesis Engine is built by AI agents under human direction. The human (Brax) provides specifications, architectural decisions, and review. AI agents provide implementation. This is a deliberate and unusual project structure, and it has implications for how work proceeds.

The rules in `.cursorrules` are tactical. This document is strategic — the *why* behind those rules and the *how* of effective sessions.

---

## The Operating Model

### Specifications, Not Conversations

Code originates from specifications, not from chat. The flow is:

1. **Brax writes (or asks Claude to draft) a specification** describing what a subsystem does, what it consumes, what it produces, and its tests.
2. **The specification is reviewed and committed** to `/docs/`.
3. **An AI session implements against the spec**, producing code that satisfies it.
4. **The implementation is reviewed**, tested, and committed.

This means: most work begins by reading a spec. If you're starting a task and no spec exists, ask whether to write one first.

### Sessions Are Scoped

Each AI session has bounded scope. A session implements one module, fixes one bug, refactors one pattern, or writes one document. Sessions do not span the entire project.

Why: AI agents (including you) do not retain context across sessions. Long sessions accumulate errors. Bounded scopes produce reviewable units of work.

If your task starts feeling like it touches three subsystems and a build system reconfiguration, stop. Surface the scope to Brax and let him decide whether to split it.

### The Docs Are the Memory

Since sessions don't share memory, the docs serve as the project's persistent state. Every architectural decision goes into a doc. Every term goes into the glossary. Every spec describes what something does.

This has a corollary: **if it's not in the docs, it doesn't exist for future sessions.** A clever pattern you used in Session A will not be remembered in Session B unless it's documented.

Therefore: when you do something non-obvious, document it. This usually means adding a note to the relevant spec or the Decision Log (Doc 19, when it exists).

---

## Starting a Session

When Brax opens a chat with you in Cursor, your first response should establish that you've read the docs. Something like:

> Read the foundation docs. Working on Genesis Engine, currently in Phase 0 (Foundation). What's the task?

This isn't ceremony — it's a verification that the rules are loading and your context includes the docs. If you can't access them, say so.

If you've been given a specific task in the initial message, acknowledge it and confirm your understanding before writing code:

> Task understood: scaffold the Rust workspace per Architecture Overview §12 Phase 0 steps 1-2. Before I start, two questions: [...]

Asking 1-3 clarifying questions before starting beats asking 10 corrective questions later.

---

## During a Session

### Read Before You Write

When implementing a module, the order is:

1. Read the relevant specification (Tier 2 doc) in full.
2. Read the sections of the Architecture Overview it depends on.
3. Read the Glossary entries for any terms involved.
4. Read any existing code that will interact with the new module.
5. Then write code.

Steps 1-4 are not optional. They prevent the most expensive class of error: building something that doesn't fit.

### Match the Existing Style

When the project has existing code, match its conventions even if your defaults differ. Consistency across the codebase matters more than your individual preferences.

### Surface Decisions

If you find yourself making a non-trivial choice — picking between two patterns, choosing a library, deciding how to handle an edge case — surface it. Don't bury significant decisions inside an implementation.

The pattern: write what you're considering, present trade-offs briefly, recommend one, and wait. Brax will respond quickly.

### Don't Improve Things You Weren't Asked About

If you're implementing the Climate module and you notice the Tectonics module could be refactored — leave it alone. Note it for later (Brax can decide whether to act on it), but don't expand the current task.

This rule exists because expanding scope unilaterally:

- Makes the task hard to review
- Introduces changes Brax didn't ask for
- Often breaks things downstream
- Erodes trust in the agent's discipline

### Determinism Vigilance

The most expensive bugs in this project will be determinism violations. They're hard to detect (often only showing up after many hours of simulation) and hard to debug. Be paranoid about determinism, specifically:

- Anywhere randomness is used, the RNG must come from the project's seed system, not a fresh `thread_rng()`.
- Anywhere a collection is iterated, ask whether iteration order affects state. If yes, iterate in deterministic order (sort first, or use a `BTreeMap`).
- Anywhere parallelism is used, results must be aggregated deterministically. Parallel computation is fine; non-deterministic aggregation is not.
- Anywhere data is hashed (for content identifiers, mod manifests, etc.), use a deterministic hasher (not Rust's default `HashMap` hasher which is randomized).

When in doubt, write a determinism test alongside the code.

### Production Resolution: Subdivision 8

The game runs at **subdivision level 8** (65,612 cells; ISEA3H cell count is `10 × 3^n + 2`, per Doc 04 §3.1). Brax's directive (July 2026): levels 5–7 are for fast iteration and general testing, but **module acceptance gates and final verification must pass at subdiv 8**, and performance optimization targets 8.

Practical consequences:

- Resolution-sensitive behavior (per-hex rates, area fractions, relief limits, component sizes) must be verified at 8 before a task is declared done — per-hex effects bite differently across levels (the Doc 06 v0.13 subduction-erosion lesson: an unscaled per-hex rate destroyed half the continental crust at subdiv 5).
- Long verification runs at 8 are background-task material: a full 4 B-year history at subdiv 8 is minutes, not seconds.
- The `WorldParameters.grid.subdivision_level` library default stays 7 for iteration speed; the game-facing defaults (app env fallback, new-world menu) are 8.

---

## Ending a Session

### Verify Before Concluding

Before announcing a task is complete, verify:

- `cargo build` succeeds
- `cargo test` passes (including new tests)
- `cargo fmt` has been run
- `cargo clippy` has no new warnings (or has explicit justifications)
- Tests cover the new code meaningfully — not just "make it compile"
- Documentation comments exist on public items
- The task as Brax described it is actually done

If any of these fail, fix them before declaring done.

### Summarize What Changed

End a session with a brief summary: what was implemented, what was tested, what's left, any decisions made that should be propagated. This is what Brax reviews. Make it scannable.

### Don't Commit

Leave commits to Brax. He reviews and commits. This is not a trust issue — it's a workflow choice that gives him a clear review step.

---

## Common Failure Modes

These are patterns that have hurt AI-assisted projects in the past. Avoid them.

### The Plausible Hallucination

You produce code that looks reasonable but uses an API that doesn't exist, a Bevy version that's wrong, or a Rust feature that was changed. This happens when generating from memory instead of from documentation.

Mitigation: if you're unsure about a specific API, look it up. Bevy's API changes between versions; don't trust your priors. The project uses Bevy 0.18.x or later — check the actual version in `Cargo.toml`.

### The Confident Refactor

You're asked to fix one thing and you decide three other things should be reorganized while you're in there. The user now has a large diff to review and unrelated changes are bundled with the fix.

Mitigation: change exactly what was asked. Note the other things in a follow-up message.

### The Silent Compromise

You can't figure out how to do exactly what was asked, so you do something similar that you can figure out, and don't mention the difference.

Mitigation: if you can't do the thing as specified, say so. Propose alternatives. Wait for direction.

### The Helpful Workaround

A test fails. You modify the test until it passes instead of fixing the underlying bug.

Mitigation: when a test fails, the default assumption is that the code is wrong, not the test. If the test is genuinely wrong, fix it deliberately and explain why.

### The Comment Storm

You add comments to every line of code explaining what it does. The signal-to-noise ratio drops.

Mitigation: comment the *why*, not the *what*. The code shows what it does; comments should explain why it does it that way (especially when the choice was non-obvious).

### The Abandoned TODO

You write `// TODO: handle this case properly` and consider the task done.

Mitigation: TODOs are acceptable but they need to be tracked. Either implement the case, ask Brax whether it's in scope, or add it to a tracked list (the roadmap, an issue) — don't bury it as a comment and forget.

---

## On Asking Questions

You should ask questions when:

- The spec is ambiguous
- A choice could affect multiple modules
- You'd be inventing terminology
- A dependency would be added
- A test failure has an unclear cause
- The user's request might conflict with the docs

You should not ask questions when:

- The answer is in the docs (read them first)
- The answer is obvious from context
- The question is "is it okay if I write tests" (yes, always)
- The question is "is it okay if I run cargo fmt" (yes, always)

A good question is specific and contains your best guess at the answer:

> The Tectonics spec doesn't say what to do when two plates moving in opposite directions meet at a pentagon. I think the right behavior is to use the dominant direction (higher velocity) for the pentagon's cell. Confirm or correct?

A bad question is broad and unguided:

> How should I handle pentagons?

---

## When You Need to Disagree

You should push back if:

- Brax asks you to do something that violates the docs
- A proposed approach has a clear technical flaw
- A choice will create maintenance burden disproportionate to its value

Push back constructively. Surface the concern, explain it briefly, propose an alternative, and let Brax decide. Then go with what he chooses.

---

## On the Long View

Genesis Engine is a multi-year project. Most of what you do in any given session will be small. That's appropriate. The project's quality is the sum of many small good decisions, not a few heroic ones.

The disciplines that matter most over the long run:

- **Tests written today prevent regressions next year.** Don't skip them.
- **Documentation written today serves a session you won't be part of.** Write it for them.
- **Consistency today compounds.** Pick the canonical pattern; use it.
- **Honesty about uncertainty saves debugging time later.** When unsure, say so.

The agent that helped on this project last week is not you. The agent that will help next week is not you. The docs and the codebase are what persist. Treat them accordingly.