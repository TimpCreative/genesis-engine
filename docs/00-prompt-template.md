# Agent Prompt Template

This file defines the **standard prompt shell** for scoped Genesis Engine implementation sessions. Copy everything between the delimiters into a new agent chat, replace every `{{PLACEHOLDER}}`, and delete optional sections you do not need for that task.

**Do not commit filled-in prompts** unless you are archiving a completed slice for reference. The template stays generic; task specifics live in the chat or in specs.

---

## How to use

1. Copy from `=== START PROMPT ===` through `=== END PROMPT ===`.
2. Replace all `{{PLACEHOLDER}}` values (search for `{{` in your editor).
3. Remove any **Optional** sections that do not apply.
4. Paste into Cursor (or your agent) in **Agent mode** for implementation work.
5. Keep specs authoritative; the prompt scopes *what slice* to implement, not a second spec.

---

## Template

```markdown
=== START PROMPT ===

Read these documents before starting, in order:

- `CONTRIBUTING-AI.md` — collaboration guide
- `docs/03-glossary-and-naming.md` — terminology
- `{{PRIMARY_SPEC_PATH}}` v{{PRIMARY_SPEC_VERSION}} — {{PRIMARY_SPEC_ONE_LINE}}
  - Read every section, but pay particular attention to:
    - {{FOCUS_SECTION_1}}
    - {{FOCUS_SECTION_2}}
    - {{FOCUS_SECTION_3}}
- `{{SECONDARY_SPEC_PATH}}` v{{SECONDARY_SPEC_VERSION}} — {{SECONDARY_SPEC_ONE_LINE}}
- Existing code in `{{STYLE_REFERENCE_PATH}}/` for style

If you cannot access any of these, stop and tell me.

## Your Task

{{TASK_SUMMARY}}

{{TASK_DELIVERABLE}}

{{TASK_EXPLICIT_EXCLUSIONS}}

## What "Done" Looks Like

After this prompt:

- {{DONE_CRITERION_1}}
- {{DONE_CRITERION_2}}
- {{DONE_CRITERION_3}}
- `cargo build --workspace` succeeds
- `cargo test --workspace` passes — all existing tests plus {{NEW_TEST_SCOPE}}
- `cargo fmt --check` passes
- `cargo clippy --workspace --all-targets` is clean (no warnings, or explicit justifications)
- {{ADDITIONAL_VERIFICATION_COMMAND}}
- Do not commit unless explicitly asked

## Implementation Plan

{{IMPLEMENTATION_PLAN_INTRO}}

### Part 1: {{PART_1_TITLE}}

**Files:** `{{PART_1_FILES}}`

**Changes:**
- {{PART_1_CHANGE_1}}
- {{PART_1_CHANGE_2}}

**Tests:**
- {{PART_1_TESTS}}

### Part 2: {{PART_2_TITLE}}

**Files:** `{{PART_2_FILES}}`

**Changes:**
- {{PART_2_CHANGE_1}}

**Tests:**
- {{PART_2_TESTS}}

{{ADDITIONAL_PARTS_AS_NEEDED}}

## What's NOT in Scope

- {{OUT_OF_SCOPE_1}}
- {{OUT_OF_SCOPE_2}}
- {{OUT_OF_SCOPE_3}}

If you find yourself reaching for any of the above, stop and confirm.

## Process

1. Read `{{PRIMARY_SPEC_PATH}}` fully. This prompt implements approximately {{SPEC_SLICE_FRACTION}} of that spec.
2. Implement in the order of the Implementation Plan. Run tests after each logical chunk when practical.
3. Run the full verification suite. Show **actual command output** in your summary.
4. Follow project rules in `.cursor/rules/project.mdc` and `CONTRIBUTING-AI.md` (determinism, glossary terms, no scope creep).
5. {{PROCESS_EXTRA_RULE}}

## When You Finish

Provide a summary with:

- Directory tree of files added/changed
- Actual command outputs from build / test / fmt / clippy (and any extras listed above)
- New and updated test counts
- {{DEPENDENCY_CONFIRMATION}}
- {{MEMORY_OR_PERF_NOTE_IF_RELEVANT}}
- Decisions you made that were not fully specified in the docs
- Inconsistencies you noticed in the authoritative specs
- Sections that need clarification before {{NEXT_PROMPT_ID}}

Ask clarifying questions before starting if anything is unclear.
If everything is clear, say so and begin.

=== END PROMPT ===
```

---

## Placeholder reference

| Placeholder | Meaning |
|-------------|---------|
| `{{PRIMARY_SPEC_PATH}}` | Main Tier 2 spec for this slice (e.g. `docs/06-tectonics-module-specification.md`) |
| `{{PRIMARY_SPEC_VERSION}}` | Spec version string (e.g. `0.2`) |
| `{{PRIMARY_SPEC_ONE_LINE}}` | One-line purpose of the spec |
| `{{FOCUS_SECTION_N}}` | Spec sections to emphasize (e.g. `§4.3 (First-Tick Initialization)`) |
| `{{SECONDARY_SPEC_PATH}}` | Supporting spec (e.g. data layer) |
| `{{STYLE_REFERENCE_PATH}}` | Crate or path for idioms (e.g. `crates/genesis_core/src`) |
| `{{TASK_SUMMARY}}` | What to build, in 1–3 sentences |
| `{{TASK_DELIVERABLE}}` | Concrete outputs (modules, APIs, fields written) |
| `{{TASK_EXPLICIT_EXCLUSIONS}}` | One-line “no X, no Y” for this slice |
| `{{DONE_CRITERION_N}}` | Acceptance bullets specific to the slice |
| `{{NEW_TEST_SCOPE}}` | Where new tests live (e.g. `genesis_tectonics`) |
| `{{ADDITIONAL_VERIFICATION_COMMAND}}` | Extra checks (e.g. `cargo tree -p …`, `cargo run -p genesis_app`) |
| `{{IMPLEMENTATION_PLAN_INTRO}}` | Optional sentence ordering parts (e.g. “core first, then crate”) |
| `{{PART_N_*}}` | Per-part files, changes, tests — add or remove parts as needed |
| `{{OUT_OF_SCOPE_N}}` | Future prompts / phases — keeps agent from expanding scope |
| `{{SPEC_SLICE_FRACTION}}` | Rough fraction of spec covered (e.g. `20%`) |
| `{{PROCESS_EXTRA_RULE}}` | Session-specific process (e.g. “Do not edit `/docs/`”) |
| `{{DEPENDENCY_CONFIRMATION}}` | e.g. “Confirm `genesis_tectonics` has no Bevy dependency” |
| `{{MEMORY_OR_PERF_NOTE_IF_RELEVANT}}` | Rough size/perf estimate, or delete line |
| `{{NEXT_PROMPT_ID}}` | Label for follow-up work (e.g. `P1-2`) |

---

## Optional sections

Append inside the delimiters when needed. Delete the heading if unused.

### Optional: Dependencies

```markdown
## Dependencies

- Do not add new crates without asking.
- Allowed for this slice: {{ALLOWED_DEPS}}
```

### Optional: Determinism

```markdown
## Determinism (this slice)

- RNG stream names: {{RNG_STREAM_LIST}}
- Sort collections before use when order affects state: {{SORT_REQUIREMENTS}}
- No system time, no file I/O during ticks
```

### Optional: Terminology

```markdown
## Terminology

Use canonical terms from `docs/03-glossary-and-naming.md` only (e.g. hex, branch, intervention, world).
```

### Optional: Fixed API or code shape

Use when the contract must match exactly; otherwise point at spec sections.

```markdown
### Part N: {{TITLE}}

**Contract (must match):**

\`\`\`rust
{{CODE_SNIPPET}}
\`\`\`
```

---

## Examples (filled fragments)

These illustrate placeholder usage only. They are **not** a task to run.

### Example: Required reading block

```markdown
- `docs/06-tectonics-module-specification.md` v0.2 — Phase 1 tectonics
  - Read every section, but pay particular attention to:
    - §2.2 (Initial plate layout)
    - §4.4 (RNG streams)
    - §10 (Determinism requirements)
- `docs/04-data-layer-specification.md` v0.5 — WorldData, parameters, RNG
- Existing code in `crates/genesis_core/src/` for style
```

### Example: Your Task

```markdown
## Your Task

Implement the first slice of Phase 1: initial plate generation at year 0.

Deliverable: `genesis_tectonics::initial_generation` assigns `plate_id` for every hex and returns a `PlateRegistry`. No plate motion, boundaries, or per-tick simulation in this slice.
```

### Example: What "Done" Looks Like (extra bullets)

```markdown
- New crate `genesis_tectonics` scaffolded per spec §13
- `WorldData` gains `fertility: Vec<f32>` initialized to `0.0`
- Default world yields 13–19 plates with full hex coverage in tests
```

### Example: Implementation Plan part

```markdown
### Part 1: Expand GeologyParameters

**Files:** `crates/genesis_core/src/parameters/core.rs`, `parameters/mod.rs`, `parameters/validation.rs`

**Changes:**
- Add fields per spec §6.3
- Update `Default` and validation ranges

**Tests:**
- Validation tests for each new field in `parameters/mod.rs`
```

### Example: What's NOT in Scope

```markdown
## What's NOT in Scope

- Per-tick plate motion (prompt P1-2)
- Boundary detection (prompt P1-3)
- Rendering changes (Phase 3)
```

### Example: When You Finish (dependency line)

```markdown
- Confirm `genesis_tectonics` has no Bevy dependency; confirm `genesis_core` still has no Bevy dependency
```

---

## Conventions

- **Paths:** backticks, repo-relative (`crates/genesis_core/...`).
- **Specs:** `Doc 06 §2.2` or `` `docs/06-...` §2.2 `` — pick one style per prompt and stay consistent.
- **Code in prompts:** only for fixed APIs; behavior details belong in Tier 2 specs.
- **Agent mode:** implementation prompts assume the agent may edit the repo and run `cargo`.
- **Commits:** default to “do not commit” unless the task says otherwise.
