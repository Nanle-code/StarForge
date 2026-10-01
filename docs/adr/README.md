# StarForge Architecture Decision Records (ADRs)

Architecture Decision Records (ADRs) document consequential design and architectural decisions made in the StarForge project. They capture the context, options evaluated, trade-offs, and final decisions to ensure consistency and prevent architectural drift.

## When to Write an ADR

You should submit an ADR alongside your PR when a change:
1. Introduces a new subsystem, CLI engine, or foundational data structure.
2. Modifies communication protocols, plugin ABIs, or external interfaces.
3. Changes security defaults, cryptographic algorithms, or storage models.
4. Changes user privacy, telemetry semantics, or configuration migration paths.

## ADR Workflow

1. Copy [`template.md`](./template.md) to `docs/adr/NNNN-your-decision-title.md` (using the next sequential 4-digit number).
2. Set status to `Proposed`.
3. Fill in context, decision drivers, considered options, and trade-offs.
4. Include the ADR in your Pull Request for maintainer discussion.
5. Upon merging, update status to `Accepted`.

## Index of Architecture Decisions

| ADR | Title | Status | Date |
|---|---|---|---|
| [0001](./0001-record-architecture-decisions.md) | Record Architecture Decisions | Accepted | 2026-09-01 |
| [0002](./0002-bindings-layout-and-multi-language-generation.md) | Multi-Language Client Bindings Layout | Accepted | 2026-09-05 |
| [0003](./0003-plugin-abi-and-sandboxing.md) | External Plugin ABI & Sandboxing Boundaries | Accepted | 2026-09-10 |
| [0004](./0004-telemetry-privacy-and-opt-in-defaults.md) | Privacy-Preserving Telemetry & Strict Opt-In | Accepted | 2026-09-15 |
| [0005](./0005-deterministic-simulation-profiles.md) | Deterministic Soroban Simulation Profiles for CI | Accepted | 2026-09-20 |
| [0006](./0006-config-schema-migrations.md) | Versioned Configuration Schema Migrations | Accepted | 2026-09-22 |
| [0007](./0007-noun-verb-command-tree.md) | Noun-Verb Command Tree for the `starforge` CLI | Accepted | 2026-09-27 |
