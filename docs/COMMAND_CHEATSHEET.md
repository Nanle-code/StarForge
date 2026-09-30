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
| `ai` | AI-assisted development: local assistant, audits, tests, search, planning |
| `alias` | Manage per-network contract and account aliases |
| `asset` | Classic Stellar assets: SAC contract id lookup and wrap/deploy |
| `completions` | Generate shell completions for bash, zsh, fish, and powershell |
| `config` | Manage starforge configuration, telemetry, feature flags, and privacy |
| `contract` | Contract operations (invoke, build, test, audit, upgrade, inspect, monitor, ttl) |
| `deploy` | Deploy a compiled Soroban contract and manage the deployment lifecycle |
| `dev` | Watch contract sources and rebuild/redeploy on save |
| `network` | View or switch the active network, run a local node, simulate, snapshot |
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
| `ttl show|extend` | Inspect and extend ledger-entry TTLs (--warn-below, --ledgers) |

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

