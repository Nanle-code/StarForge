# StarForge Command Cheat Sheet

> **Auto-generated** from clap command metadata by `build.rs`. Do not edit by hand.
> Regenerate with `cargo build` (build.rs rewrites this file), then commit the result.
> See `DEVELOPER_GUIDE.md` → “Command cheat sheet” for details.

`starforge` — ⚡ Stellar & Soroban developer productivity CLI

## Usage

```
starforge <command> [options]
```

Global options: `--json`, `--quiet`/`-q`, `--log-format`, `--log-dir`, `--correlation-id`, `--non-interactive`, `-h`/`--help`, `-V`/`--version`.

## Top-level commands

| Command | Description |
|---|---|
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
| `privacy` | Privacy protection, anonymization, consent, and reporting |
| `project` | AI-driven project management for task tracking, sprints, resources, risks, and timelines |
| `prompts` | Manage AI prompt templates and versioning |
| `registry` | Interact with the remote template registry |
| `schedule` | Schedule deployments for future execution with approval workflows |
| `security` | Security hardening, validation, and monitoring |
| `shell` | Interactive REPL for local Soroban contract testing |
| `simulate` | Local network simulation and testing environment |
| `telemetry` | Manage telemetry settings directly |
| `template` | Manage community contract templates from the marketplace |
| `template-vcs` | Template version control (versioning, branching, changelog) |
| `test` | Contract testing utilities for Soroban wasm |
| `tutorial` | Interactive CLI tutorials |
| `tx` | Fetch a transaction for the account |
| `upgrade` | Contract upgrade management (propose, approve, execute, rollback) |
| `verify` | Run formal verification on a contract |
| `wallet` | Manage test wallets (create, list, fund, show, remove) |

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
| `multisig` | Multisig helpers |

## `contract` subcommands

| Subcommand | Description |
|---|---|
| `invoke` | Invoke a deployed Soroban contract function |
| `invoke-script` | Run an ordered YAML or JSON invocation script (--dry-run) |
| `inspect` | Inspect a deployed Soroban contract instance |
| `upload` | Upload a WASM binary to the Stellar network |
| `generate-bindings <WASM>` | Generate typed client bindings (--lang rust\|ts\|python\|go) |
| `call-graph` | Visualize cross-contract call graph from Soroban source |
| `deps` | Manage contract dependencies |
| `version` | Track contract versions, resolve conflicts, migrations |

## `deploy` subcommands

| Subcommand | Description |
|---|---|
| `deploy --wasm <FILE>` | Prepare a Soroban deployment (--simulate, --execute) |

## `inspect` subcommands

| Subcommand | Description |
|---|---|
| `inspect storage` | Deep storage inspection (state, key, storage) |

## `network` subcommands

| Subcommand | Description |
|---|---|
| `show` | Show current active network |
| `switch <NAME>` | Switch the active network (testnet, mainnet, custom) |
| `add` | Add a custom network endpoint |
| `test` | Test connectivity to a network |

## `config` subcommands

| Subcommand | Description |
|---|---|
| `show` | Show current global configuration |
| `set <KEY> <VALUE>` | Set a configuration key/value pair |
| `set-encryption` | Set global wallet encryption parameters (Argon2id) |
| `doctor` | Validate configuration and check network connectivity |
| `db` | SQLite database management |

## `template` subcommands

| Subcommand | Description |
|---|---|
| `list` | List marketplace templates |
| `search <QUERY>` | Search templates |
| `show <ID>` | Template details |
| `init <ID> <DIR>` | Scaffold from template |
| `publish` | Publish template metadata |
| `remove <ID>` | Remove local template entry |

## `plugin` subcommands

| Subcommand | Description |
|---|---|
| `install` | Install a third-party plugin |
| `list` | List installed plugins |
| `run` | Run a plugin command |

## `test` subcommands

| Subcommand | Description |
|---|---|
| `test --wasm <FILE>` | Run Soroban contract tests (--coverage, --fixture, --report) |

## `gas` subcommands

| Subcommand | Description |
|---|---|
| `analyse <WASM>` | Heuristic gas/cpu report |
| `optimize` | Lightweight WASM shrink pass |
| `diff <OLD> <NEW>` | Compare estimated costs |

## `security` subcommands

| Subcommand | Description |
|---|---|
| `audit <PATH>` | Run built-in Soroban analysis (--format, --ci, --track) |
| `remediation list` | Review tracked audit and pentest remediation items |

## `governance` subcommands

| Subcommand | Description |
|---|---|
| `propose` | Create upgrade proposal (--contract-id, --wasm, --threshold) |
| `list` | List proposals |
| `show` | Show proposal details and votes |
| `vote` | Cast a vote (--for / --against) |
| `reject` | Reject a proposal |
| `execute` | Execute after timelock and threshold met |
| `emergency` | Emergency upgrade (bypasses timelock) |
| `audit` | Show governance audit trail |

## `upgrade` subcommands

| Subcommand | Description |
|---|---|
| `prepare` | Validate upgrade WASM |
| `auto compat` | Compare old/new WASM ABI and storage layout |
| `auto plan` | Generate compatibility-aware upgrade plan |
| `propose` | Create governance proposal |
| `list / status` | List pending proposals |
| `approve` | Approve proposal |
| `execute` | Execute approved upgrade |
| `rollback` | Roll back contract version |
| `history` | Show upgrade history |

## `multisig` subcommands

| Subcommand | Description |
|---|---|
| `wizard` | Interactive transaction proposal builder |
| `create` | Create a proposal with threshold, signers, metadata |
| `status <FILE>` | Show signature collection progress |
| `verify <FILE>` | Validate signatures and threshold readiness |
| `notify <FILE>` | Queue signature request notifications |
| `export / import` | Share proposal JSON between signers |

## `tutorial` subcommands

| Subcommand | Description |
|---|---|
| `list` | List tutorials |
| `start <SLUG>` | Begin a guided flow |
| `next` | Mark step complete and show next milestone |
| `status` | Show active tutorial and current step |

## `simulate` subcommands

| Subcommand | Description |
|---|---|
| `resources` | Report CPU, memory, footprint, and minimum resource fee |

## `cost` subcommands

| Subcommand | Description |
|---|---|
| `resources` | Price a simulation and check against budgets (--enforce) |

## `advanced-perf` subcommands

| Subcommand | Description |
|---|---|
| `profile <WASM>` | Profile a compiled Soroban contract artifact |
| `analyze <CONTRACT>` | Analyze recorded runtime metrics |
| `compare <CONTRACT>` | Compare profiles across time windows |
| `generate-dashboard <CONTRACT>` | Show the recorded-metrics dashboard |

## `docs` subcommands

| Subcommand | Description |
|---|---|
| `generate <CONTRACT>` | Generate documentation (--source, --lang) |
| `extract <PATH>` | Extract rustdoc comments |
| `show / list / search` | Browse the local docs store |
| `html / api-ref / publish` | HTML site and publishing helpers |

