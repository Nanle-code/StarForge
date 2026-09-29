# StarForge Command Cheat Sheet

> **Auto-generated** from clap command metadata by `build.rs`. Do not edit by hand.
> Regenerate with `cargo build` (build.rs rewrites this file), then commit the result.
> See `DEVELOPER_GUIDE.md` → “Command cheat sheet” for details.

`starforge` — ⚡ Stellar & Soroban developer productivity CLI

## Usage

```bash norun
starforge <command> [options]
```

Global options: `--json`, `--quiet`/`-q`, `--log-format`, `--log-dir`, `--correlation-id`, `--non-interactive`, `-h`/`--help`, `-V`/`--version`.

## Top-level commands

| Command | Description |
|---|---|
| `account` | On-chain account lifecycle with sponsored reserves (CAP-33) |
| `advanced-perf` | Advanced contract performance analysis and profiling tools |
| `ai` | Local LLM assistant for Soroban contracts (audit, explain, test, optimise, profile) |
| `ai-accessibility` | AI accessibility features — screen reader, voice commands, text simplification |
| `ai-audit` | AI-powered security audit for Soroban contracts using Claude |
| `ai-contract-suggest` | AI contract function suggestions (context-aware suggestions based on contract type) |
| `ai-debug` | AI-powered contract debugging assistant (error analysis, bug identification, fix suggestions) |
| `ai-deployment-test` | AI-driven deployment testing commands |
| `ai-doc-qa` | AI documentation Q&A (answer questions about StarForge, Stellar, and Soroban docs with citations) |
| `ai-feedback` | AI feedback and learning system (record feedback, track quality, learn preferences) |
| `ai-ide` | AI-powered IDE integration commands |
| `ai-navigate` | AI-driven definitions, references, code graphs, dependencies, and contextual search |
| `ai-plan` | AI project planning assistant — requirements, architecture, timeline, risks |
| `ai-profile` | AI-driven performance profiling commands |
| `ai-property-test` | AI property-based testing (discover properties, generate tests, validate invariants) |
| `ai-quality-gate` | Configurable code quality, security, performance, coverage, docs, and license gates |
| `ai-recommend` | AI best practice recommendations (analyze contracts, scan projects, improvement plans) |
| `ai-route` | Intelligent AI model selection and routing based on task complexity and preferences |
| `ai-search` | AI code search and discovery (search code, find patterns, similar code) |
| `ai-security-training` | AI-driven security training: lessons, exercises, progress tracking |
| `ai-telemetry` | AI usage telemetry and analytics: calls, tokens, latency, cost, opt-out |
| `ai-test` | AI-driven testing assistance (generate, optimize, analyze, maintain tests) |
| `ai-test-maintain` | AI-driven test maintenance commands |
| `alias` | Manage per-network contract and account aliases |
| `analytics` | Contract deployment analytics, dashboards, and reporting |
| `approval` | Approval workflow for contract deployments (multi-level approvals, audit, compliance) |
| `audit` | Run a comprehensive security audit on a Soroban contract |
| `backup` | Backup and disaster recovery for contract state and code |
| `benchmark` | Performance benchmarking utilities and industry-standard comparisons |
| `collab` | AI-driven collaboration tools: code review, conflict resolution, knowledge sharing, contribution tracking |
| `complete` | Smart contract completion assistant |
| `completions` | Generate shell completions for bash, zsh, and fish |
| `config` | Manage starforge configuration (telemetry, network) |
| `contract` | Contract operations (invoke, inspect, etc.) |
| `contract-monitor` | Contract health monitoring, performance tracking, security events, alerting, and dashboard |
| `cost` | AI-assisted deployment cost management: budgets, forecasting, cross-network comparison, and reporting |
| `debug` | Debug Soroban contracts with breakpoints, stepping, and inspection |
| `deploy` | Deploy a compiled Soroban contract (.wasm) |
| `deployments` | Deployment history, rollback, verification, and dashboard |
| `diagnostics` | Run connectivity diagnostics for attached Ledger/Trezor devices |
| `docs` | Contract documentation portal (generate, view, search) |
| `explain` | Analyze and explain smart contract code using AI |
| `gas` | Gas analysis and optimization helpers |
| `generate` | Generate smart contracts from natural language prompts |
| `governance` | Contract upgrade governance (proposals, voting, timelock, audit) |
| `info` | Show starforge config and environment info |
| `inspect` | Deep contract storage inspection (state, key, storage) |
| `lint` | Static analysis and linting for Soroban contracts |
| `migrate` | Contract storage migration tools (transform, validate, rollback) |
| `monitor` | Live monitoring (contract events or wallet threshold) |
| `multisig` | Manage multi-signature transactions |
| `mutate` | AI mutation testing for Soroban contracts |
| `network` | View or switch the active network (testnet/mainnet) |
| `new` | Generate Soroban project boilerplate |
| `nl` | Natural language command interface |
| `node` | Local Soroban devnet (Docker quickstart) |
| `optimize` | Analyse and optimize compiled WASM / Rust contract source for gas and size |
| `orchestrate` | Multi-contract deployment orchestration |
| `perf` | Contract performance monitoring and metrics dashboard |
| `pipeline` | Visual pipeline builder for contract deployment workflows |
| `plugin` | Manage third-party plugins |
| `project` | Project scaffolding and AI-driven project management |
| `template` | Manage community contract templates, versions, and the registry |
| `tool` | Developer-environment utilities: tutorials, natural language, PR checks |
| `wallet` | Manage test wallets (create, list, fund, sign), transactions, and devices |

## `account` subcommands

| Subcommand | Description |
|---|---|
| `create --sponsor <WALLET> --to <G...>` | Create an account with sponsored reserves (CAP-33, --fee-payer, --yes) |
| `end-sponsorship --wallet <WALLET>` | Release a sponsor's reserve (--fee-payer, --yes) |

## `wallet` subcommands

| Subcommand | Description |
|---|---|
| `create <NAME>` | Create and store a keypair (--fund, --encrypt, --mnemonic) |
| `list` | List saved wallets |
| `show <NAME>` | Show wallet metadata and balance (--reveal) |
| `fund <NAME>` | Fund via Friendbot when configured |
| `remove <NAME>` | Delete a saved wallet |
| `rename <OLD> <NEW>` | Rename a wallet entry |
| `merge` | Account merge (--from, --to, --yes) |
| `rotate <NAME>` | Rotate keys in place |
| `export <NAME>` | Export backup JSON |
| `import` | Import from file or --mnemonic |
| `sign` | Sign a payload with a saved wallet |
| `multisig` | Multi-signature account management |
| `tx` | Fetch a transaction for the account |
| `auth` | SEP-10 web authentication against an anchor |
| `diagnostics` | Ledger/Trezor connectivity diagnostics |

## `contract` subcommands

| Subcommand | Description |
|---|---|
| `invoke` | Invoke a deployed Soroban contract function |
| `invoke-script` | Run an ordered YAML or JSON invocation script (--dry-run) |
| `build` | Build a contract with StarForge provenance metadata |
| `inspect` | Inspect a deployed Soroban contract instance |
| `upload` | Upload a WASM binary to the Stellar network |
| `generate-bindings <WASM>` | Generate typed client bindings (--lang rust\|ts\|python\|go) |
| `storage <state\|key\|storage>` | Deep storage inspection |
| `debug` | Breakpoints, stepping, and inspection |
| `repl` | Interactive REPL for local contract testing |
| `test --wasm <FILE>` | Run contract tests (--coverage, --fixture, --report) |
| `audit` | Run a comprehensive security audit |
| `security` | Hardening, validation, monitoring, incidents |
| `governance` | Upgrade proposals, voting, timelock, audit |
| `upgrade` | Propose, approve, execute, roll back an upgrade |
| `verify` | Run formal verification |
| `migrate` | Storage migrations (transform, validate, rollback) |
| `generate` | Generate contracts from natural language |
| `explain` | Explain contract code with AI |
| `lint` | Static analysis and linting |
| `optimize` | WASM/source optimisation for gas and size |
| `gas` | Gas analysis, diffs, estimates, alerts |
| `metrics` | Performance metrics, dashboards, regression baselines |
| `profile` | Advanced performance analysis and profiling |
| `benchmark` | Performance benchmarks and comparisons |
| `docs` | Contract documentation portal |
| `mutate` | AI mutation testing |
| `monitor` | Live contract event or wallet-threshold monitoring |
| `health` | Contract health monitoring and alerting |

## `deploy` subcommands

| Subcommand | Description |
|---|---|
| `run --wasm <FILE>` | Prepare a Soroban deployment (--simulate, --execute) |
| `history` | Deployment history, rollback, verification, dashboard |
| `env` | Manage dev/staging/production environments |
| `schedule` | Schedule deployments with approval workflows |
| `orchestrate` | Multi-contract deployment orchestration |
| `pipeline` | Visual pipeline builder for deployment workflows |
| `approval` | Multi-level deployment approvals |
| `cost` | Budgets, forecasting, and cross-network comparison |
| `analytics` | Deployment analytics and reporting |
| `backup` | Backup and disaster recovery |

## `network` subcommands

| Subcommand | Description |
|---|---|
| `show` | Show current active network |
| `switch <NAME>` | Switch the active network (testnet, mainnet, custom) |
| `add` | Add a custom network endpoint |
| `test` | Test connectivity to a network |
| `node` | Local Soroban devnet (Docker quickstart) |
| `simulate` | Local network simulation and testing |
| `snapshot` | Deterministic live-ledger snapshots |

## `template` subcommands

| Subcommand | Description |
|---|---|
| `list` | List marketplace templates |
| `search <QUERY>` | Search templates |
| `show <ID>` | Template details |
| `init <ID> <DIR>` | Scaffold from template |
| `publish` | Publish template metadata |
| `remove <ID>` | Remove local template entry |
| `vcs` | Template version control (branch, changelog) |
| `registry` | Interact with the remote template registry |

## `plugin` subcommands

| Subcommand | Description |
|---|---|
| `install` | Install a third-party plugin |
| `list` | List installed plugins |
| `verify` | Verify a plugin signature |
| `audit` | Audit a plugin |

## `ai` subcommands

| Subcommand | Description |
|---|---|
| `local <status\|models\|pull\|ask\|…>` | Local LLM assistant (Ollama) |
| `debug` | Error analysis and fix suggestions |
| `navigate` | Definitions, references, code graphs |
| `gate` | Code quality, security, coverage, license gates |
| `security-audit` | AI security audit of a contract |
| `tests` | Generate, optimize, and analyze tests |
| `test-maintain` | Keep the test suite healthy |
| `deploy-test` | AI-driven deployment testing |
| `property-test` | Discover properties, validate invariants |
| `search` | Code search and pattern discovery |
| `recommend` | Best practice recommendations |
| `route` | Model selection and routing |
| `plan` | Requirements, architecture, timeline, risks |
| `suggest` | Context-aware contract function suggestions |
| `docs` | Documentation Q&A with citations |
| `profiling` | Performance profiling |
| `feedback` | Record feedback, track quality |
| `telemetry` | AI usage telemetry and cost |
| `training` | Security training lessons and progress |
| `accessibility` | Screen reader, voice, text simplification |
| `ide` | Editor snippets and task providers |
| `prompts` | Prompt templates and versioning |
| `help` | Contextual help for commands and workflows |

## `config` subcommands

| Subcommand | Description |
|---|---|
| `show` | Show effective configuration (user config + project lockfile) |
| `set <KEY> <VALUE>` | Set a configuration key/value pair |
| `set-encryption` | Set global wallet encryption parameters (Argon2id) |
| `doctor` | Validate configuration and check network connectivity |
| `db` | SQLite database management |
| `info` | Show config and environment info |
| `telemetry` | Telemetry settings and opt-out |
| `flags` | AI feature flags, rollouts, rollback |
| `privacy` | Anonymization, consent, and reporting |

## `project` subcommands

| Subcommand | Description |
|---|---|
| `new` | Generate Soroban project boilerplate |
| `task` | Create, assign, track, and complete tasks |
| `progress` | Visualize task progress |
| `sprint` | Sprint planning and burndown |
| `collab` | Code review, conflict resolution, knowledge base |

## `tool` subcommands

| Subcommand | Description |
|---|---|
| `tutorial` | Interactive, step-by-step CLI tutorials |
| `nl <INPUT>` | Natural language command interface |
| `pr` | Check PR readiness (CI green, no conflicts) |
| `bug-report` | Prefilled environment bug report |

