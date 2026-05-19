# Genesis Engine

A deterministic worldbuilding simulator for authors, worldbuilders, and hobbyists.

Genesis Engine generates fictional planets from physical first principles — plate tectonics, climate, hydrology, biology — then simulates the emergence and history of life and civilization on those planets across geological timescales. The result is internally consistent worlds with traceable causal chains: every present-day feature can be followed back through deep history.

It's not a game in the traditional sense. There are no win conditions, no opponents, no scoring. It's a tool for building worlds that hold up under scrutiny.

## Project Status

**Pre-implementation.** Foundation documents complete. Phase 0 (project scaffold) not yet started.

## Documentation

All design documentation lives in [`/docs/`](./docs/). Read in this order:

1. **[Vision & Scope](./docs/01-vision-and-scope.md)** — What Genesis Engine is, who it's for, what success looks like, and the full document map.
2. **[Architecture Overview](./docs/02-architecture-overview.md)** — Technical blueprint. Layer model, module map, hex grid, tick system, branching, modding, build order.
3. **[Glossary & Naming Conventions](./docs/03-glossary-and-naming.md)** — Canonical terminology. Reference constantly.

Additional Tier 2-5 specifications will be written as their corresponding subsystems are implemented.

## For AI Coding Agents

If you are an AI assistant (Cursor, Claude Code, etc.) working on this project, read **[CONTRIBUTING-AI.md](./CONTRIBUTING-AI.md)** before doing anything. It defines how to collaborate effectively on this codebase.

## Tech Stack

- **Language:** Rust (latest stable)
- **Engine:** [Bevy](https://bevyengine.org/) (ECS + rendering)
- **Platforms:** macOS and Windows (Linux likely works but is not a release target)
- **Edition:** Rust 2024

## Building

> Not yet applicable. The project scaffold has not yet been generated. See [Architecture Overview §12 — Build Order](./docs/02-architecture-overview.md) Phase 0 for the planned workspace structure.

Once scaffolding exists:

```sh
cargo build
cargo test
cargo run
```

## License

Proprietary. All rights reserved. Not currently licensed for redistribution or external contribution.