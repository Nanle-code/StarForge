# Module Inventory

This inventory identifies the owning implementation for template management
and network simulation. Shared data and package operations belong to the
canonical modules below; command presentation and focused supporting services
remain in their respective modules.

## Templates

| Concept | Implementation files | Canonical owner |
| --- | --- | --- |
| Registry entries, sources, compatibility, lookup, filtering, caching, package installation, publishing, updates, and rollback | `src/utils/templates.rs`; registry consumers in `src/commands/template.rs`, `src/commands/registry.rs`, and `src/commands/new.rs` | `crate::utils::templates` |
| CLI parsing, orchestration, output, and command workflows, including Init, Analyze, Feedback, Import, and Fetch/Install adapters | `src/commands/template.rs` | `crate::commands::template` |
| Registry JSON schema and field validation | `src/utils/template_schema.rs`; used by `src/utils/templates.rs` | `crate::utils::template_schema` |
| Package digests, Sigstore signing, provenance verification, and signed-template policy | `src/utils/template_provenance.rs`; integrated by `src/utils/templates.rs` | `crate::utils::template_provenance` |
| Usage analytics, community feedback, quality reports, and sentiment | `src/utils/template_analytics.rs`; invoked by template commands | `crate::utils::template_analytics` |
| Local recommendations and recommendation usage history | `src/utils/template_recommender.rs`; reads entries from `src/utils/templates.rs` | `crate::utils::template_recommender` |
| AI-assisted template customization and customization history | `src/utils/template_customization_ai.rs` | `crate::utils::template_customization_ai` |
| Template-to-project integration analysis and integration tests | `src/utils/template_integration.rs` | `crate::utils::template_integration` |
| Template performance analysis | `src/utils/template_performance.rs` | `crate::utils::template_performance` |
| Security scanning and risk analysis | `src/utils/template_security_scanner.rs` | `crate::utils::template_security_scanner` |
| Git-backed template versions, changelogs, collaboration, and releases | `src/utils/template_vcs.rs` | `crate::utils::template_vcs` |
| AI-assisted version analysis, compatibility checks, and migration guides | `src/utils/template_version_ai.rs`; uses `src/utils/template_vcs.rs` | `crate::utils::template_version_ai` |

`src/utils/templates.rs` is the sole registry/package implementation. The
former `src/utils/template.rs` duplicated CLI orchestration and forwarded
package work to the registry API. Its Init, Analyze, Feedback, Import, and
source-install behavior is now owned by `src/commands/template.rs`, which uses
the canonical registry and analytics services. The focused `template_*`
services above are not alternate registries and remain separate.

## Network Simulation

| Concept | Implementation files | Canonical owner |
| --- | --- | --- |
| Simulator state, accounts, contracts, ledger, invocation, receipts, latency, and snapshot-file persistence | `src/utils/network_simulator/simulator.rs` | `crate::utils::network_simulator::simulator` |
| Seeded randomness and deterministic identifiers | `src/utils/network_simulator/deterministic.rs` | `crate::utils::network_simulator::deterministic` |
| Conditional RPC and transaction failure injection | `src/utils/network_simulator/failure.rs` | `crate::utils::network_simulator::failure` |
| Snapshot capture, restore, import, and export | `src/utils/network_simulator/state.rs`; coordinated by `simulator.rs` | `crate::utils::network_simulator::state` |
| Ledger time control | `src/utils/network_simulator/time.rs`; coordinated by `simulator.rs` | `crate::utils::network_simulator::time` |
| Built-in fixture scenarios and serialized step-oriented scenarios | `src/utils/network_simulator/scenarios.rs`; consumed by `src/commands/simulate.rs` and `tests/network_simulation.rs` | `crate::utils::network_simulator::scenarios` |
| Public simulator API and convenience re-exports | `src/utils/network_simulator/mod.rs` | `crate::utils::network_simulator` |
| Former single-file API shape (`SimLedgerState`, `SimEvent`, seeded constructor, named snapshots, legacy invoke results and JSON) | `src/utils/network_simulator/compat.rs`; state transitions delegate to `simulator.rs` | `crate::utils::network_simulator::compat` (`LegacyNetworkSimulator` re-export) |

The former `src/utils/network_sim.rs` contained a second simulator state
engine. Its important public data and call contracts remain available through
the `compat` facade. That facade uses the canonical engine for account,
contract, and ledger state, retaining only the legacy-shaped state/event view
and deterministic result format required for compatibility. New simulator
features and the canonical root `NetworkSimulator` remain owned by
`network_simulator`.