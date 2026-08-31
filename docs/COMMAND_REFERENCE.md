# StarForge Command Reference

Browse every top-level command and its most important flags. For wallet, template, and transaction details see also [API_REFERENCE.md](../API_REFERENCE.md).

## Global options

| Flag | Description |
|------|-------------|
| `-q, --quiet` | Suppress banner and decorative output |
| `--log-format human\|json` | Structured log format (default: `human`) |
| `--log-dir <PATH>` | Optional rotating log directory |
| `--correlation-id <ID>` | Tie every log line of this invocation together (8–64 chars of `[A-Za-z0-9_-]`); defaults to `$STARFORGE_CORRELATION_ID` or a generated value — see [CORRELATION_IDS.md](CORRELATION_IDS.md) |
| `-h, --help` | Command help |
| `-V, --version` | CLI version |

## Quick workflow examples

```bash
# Environment check
starforge info

# Wallet + network
starforge wallet create deployer --fund
starforge network show

# Templates + deploy
starforge template list
starforge deploy --wasm ./contract.wasm --wallet deployer --simulate

# Guided tutorial
starforge tutorial start hello-world
starforge tutorial next
```

---

## `wallet`

| Subcommand | Purpose |
|------------|---------|
| `create <NAME>` | Create and store a keypair (`--fund`, `--encrypt`, `--mnemonic`) |
| `list` | List saved wallets |
| `show <NAME>` | Show wallet metadata and balance (`--reveal`) |
| `fund <NAME>` | Fund via Friendbot when configured |
| `remove <NAME>` | Delete a saved wallet |
| `rename <OLD> <NEW>` | Rename a wallet entry |
| `merge` | Account merge (`--from`, `--to`, `--yes`) |
| `rotate <NAME>` | Rotate keys in place (`--fund`, `--encrypt`, `--mem`, `--iterations`) |
| `export <NAME> --output <FILE>` | Export backup JSON |
| `import` | Import from file or `--mnemonic` |
| `sign` | Sign a payload with a saved wallet |
| `multisig` | Multisig helpers (create, add-signer, submit) |

`import --file` accepts a plaintext backup JSON or an encrypted bundle, detected
automatically. See [WALLET_IMPORT_SECURITY.md](WALLET_IMPORT_SECURITY.md) for the
limits enforced on untrusted backup files.

---

## `multisig`

| Subcommand | Purpose |
|------------|---------|
| `wizard` | Interactive transaction proposal builder |
| `create` | Create a proposal with threshold, signers, metadata, and optional transaction XDR |
| `status <FILE>` | Show visual signature collection progress |
| `verify <FILE>` | Validate signatures, duplicates, pending signers, and threshold readiness |
| `notify <FILE>` | Queue signature request notifications for pending signers |
| `export <FILE>` / `import <FILE>` | Share proposal JSON between signers |
| `templates` / `from-template` | Use common scenarios like escrow, company treasury, DAO, vault, and payment |

```bash
starforge multisig wizard
starforge multisig create --threshold 2 --signers alice,bob,carol \
  --title "Treasury payment" --transaction-xdr <XDR>
starforge multisig status proposal.json
starforge multisig verify proposal.json
starforge multisig notify proposal.json --message "Please sign the treasury payment"
```

---

## `new`

| Subcommand | Purpose |
|------------|---------|
| `contract <NAME>` | Scaffold Soroban contract (`--template`) |
| `dapp <NAME>` | Scaffold Stellar dApp frontend |

---

## `contract` / `inspect` / `deploy`

| Command | Purpose |
|---------|---------|
| `contract invoke` | Invoke contract function (`--simulate`) |
| `contract invoke-script` | Run an ordered YAML or JSON invocation script (`--dry-run`) |
| `contract inspect` | Inspect deployed contract metadata |
| `contract generate-bindings <WASM_FILE>` | Generate Rust or TypeScript wrappers (`--lang rust\|ts`) |
| `inspect storage` | Deep storage inspection |
| `deploy --wasm <FILE>` | Prepare Soroban deployment |

**`deploy` flags:** `--network`, `--wallet`, `--optimize`, `--simulate`, `--yes`, `--execute`, `--policy`, `--checklist`

Deploy policy files (`starforge-deploy-policy.toml`) gate allowed networks,
required reviewers, and checklist items. Validate in CI with
`starforge deploy-policy check`. See [DEPLOY_POLICY.md](DEPLOY_POLICY.md).

Destructive confirmations (mainnet deploy, secret reveal) require typed
challenge phrases; automation bypass needs `STARFORGE_UNSAFE_SKIP_CONFIRMATION=1`.
See [CONFIRMATION_UX.md](CONFIRMATION_UX.md).

`--simulate` and `--dry-run` print the simulated CPU, memory, and ledger
footprint alongside the minimum resource fee and a recommended fee that
includes a safety margin. See [SIMULATION_RESOURCES.md](SIMULATION_RESOURCES.md).

```bash
starforge deploy --wasm target/wasm32v1-none/release/token.wasm \
  --wallet deployer --network testnet --simulate

starforge deploy --wasm ./token.wasm --optimize --yes --execute

starforge contract generate-bindings ./token.wasm --lang rust
```

## `deploy-policy`

| Subcommand | Purpose |
|------------|---------|
| `init [FILE]` | Write a documented default policy (TOML or YAML) |
| `check --config <FILE>` | Validate policy schema and simulate deploy context (CI-friendly) |

See [DEPLOY_POLICY.md](DEPLOY_POLICY.md).

### Invocation scripts

Repeatable contract calls can be stored as `.yaml`, `.yml`, or `.json` files.
Each script has `version: 1` and an ordered `steps` list. Steps support typed
arguments, `${ENV_VAR}` interpolation, and assertions such as
`return_equals`, `return_contains`, `error_contains`, `event_contains`, and
`fee_at_most`.

```yaml
version: 1
steps:
  - name: set value
    contract_id: ${CONTRACT_ID}
    function: set_value
    args:
      - type: string
        value: ${VALUE}
    assertions:
      - type: return_contains
        value: ok
```

Preview a script without loading wallets or contacting Soroban RPC:

```bash
starforge contract script ./ops.yaml --dry-run
```

Run it in CI after exporting required variables. A step submits only when it
sets `submit: true` and names a configured wallet; otherwise it simulates.

```bash
export CONTRACT_ID=CA...
export VALUE=ready
starforge contract script ./ops.yaml --network testnet
```

---

## `test`

| Flag | Purpose |
|------|---------|
| `--wasm <FILE>` | Compiled Soroban WASM under test |
| `--fixture <FILE>` | JSON/TOML contract test suite with fixtures, mocks, and assertions |
| `--source <FILE>` | Contract source used for generated tests or coverage |
| `--coverage` | Include source coverage summary |
| `--coverage-out <FILE>` | Write a dedicated coverage report |
| `--coverage-format html\|json\|markdown\|text` | Format for `--coverage-out` |
| `--coverage-goal <PCT>` | Minimum overall coverage percentage |
| `--function-coverage-goal <PCT>` | Minimum function coverage percentage |
| `--line-coverage-goal <PCT>` | Minimum line coverage percentage |
| `--branch-coverage-goal <PCT>` | Minimum branch coverage percentage |
| `--coverage-ci` | Fail when configured coverage goals are missed |
| `--coverage-ci-workflow-out <FILE>` | Generate a GitHub Actions coverage workflow |
| `--report html\|json\|junit` | Write a test report (`junit` is available for fixture suites) |
| `--testnet` | Validate Soroban testnet integration for the run |
| `--testnet-dry-run` | Validate testnet configuration without probing RPC health |

```bash
starforge test --wasm ./target/contract.wasm \
  --fixture ./contract-tests.json --coverage --source ./src/lib.rs --report html

starforge test --wasm ./target/contract.wasm --source ./src/lib.rs \
  --coverage --coverage-out coverage.html --coverage-format html \
  --coverage-ci --coverage-goal 85 --branch-coverage-goal 70

starforge test --wasm ./target/contract.wasm --source ./src/lib.rs \
  --coverage-ci-workflow-out .github/workflows/starforge-coverage.yml

starforge test --wasm ./target/contract.wasm \
  --fixture ./contract-tests.toml --testnet --testnet-dry-run
```

Fixture suites support named storage fixtures, mocked contract calls, and assertions such as `state_equals`, `state_exists`, `return_equals`, `event_emitted`, `fee_at_most`, and `mock_called`.
Coverage analysis tracks Soroban contract functions, line spans, branch paths, uncovered functions, threshold goals, and HTML/JSON/Markdown/text reports.

---

## `network` / `node`

| Command | Purpose |
|---------|---------|
| `network show` | Show configured networks |
| `network switch <NAME>` | Set active network |
| `network add` | Add custom Horizon/RPC/Friendbot endpoints |
| `network test` | Connectivity probe |
| `node start` | Start local quickstart devnet (`--port`) |

---

## `tx`

| Subcommand | Purpose |
|------------|---------|
| `tx send` | Payment (`--from`, `--to`, `--amount`, `--asset`) |
| `tx batch` | Batch operations from JSON (`--file`, `--from`) |
| `tx history <PUBKEY>` | Recent transactions (`--limit`, `--cursor`, `--successful`) |

---

## `template`

| Subcommand | Purpose |
|------------|---------|
| `template list` | List marketplace templates |
| `template search <QUERY>` | Search templates |
| `template show <ID>` | Template details |
| `template init <ID> <DIR>` | Scaffold from template |
| `template publish` | Publish template metadata |
| `template remove <ID>` | Remove local template entry |

When downloading template archives from a remote registry, the CLI automatically verifies the SHA-256 checksum if provided by the registry prior to extraction. Archives from registries that omit a checksum field are accepted without verification.

---

## `gas`

| Subcommand | Purpose |
|------------|---------|
| `gas analyze <WASM>` | Heuristic gas/cpu report (`--network`) |
| `gas optimize --target <IN> --output <OUT>` | Lightweight WASM shrink pass |
| `gas diff <OLD> <NEW>` | Compare estimated costs |

---

## `simulate` / `cost` — resource fees

| Command | Purpose |
|---------|---------|
| `simulate resources --file <JSON>` | Report CPU, memory, footprint, and minimum resource fee from a saved `simulateTransaction` response |
| `simulate resources --contract <ID> --function <NAME>` | The same, simulated live against Soroban RPC |
| `cost resources --file <JSON>` | Price a simulation and check it against configured budgets (`--enforce` to gate CI) |
| `cost forecast-batch <MANIFEST>` | Forecast aggregate fees for a batch of planned invokes before submission (per-item estimates + totals, high-variance calls highlighted) |

Shared flags: `--margin <PERCENT>` (default `20`), `--inclusion-fee <STROOPS>`
(default `100`). `simulate resources` also takes `--json`.

```bash
starforge simulate resources --file simulation.json --json
starforge simulate resources --contract CCPYZ... --function balance --network testnet
starforge cost resources --file simulation.json --network mainnet --enforce
starforge cost forecast-batch batch-invoke-manifest.json --network testnet --enforce
```

Full reference: [SIMULATION_RESOURCES.md](SIMULATION_RESOURCES.md) and
[BATCH_FORECAST.md](BATCH_FORECAST.md).

---

## `advanced-perf`

| Subcommand | Purpose |
|------------|---------|
| `advanced-perf profile <WASM>` | Profile a compiled Soroban contract artifact |
| `advanced-perf profile <WASM> --baseline <JSON>` | Detect gas, execution-time, or memory regressions against a saved profile |
| `advanced-perf profile <WASM> --dashboard <HTML>` | Generate a local performance dashboard |
| `advanced-perf analyze <CONTRACT>` | Analyze recorded runtime metrics for bottlenecks |
| `advanced-perf detect-regression <CONTRACT>` | Detect regressions from recorded metric history |
| `advanced-perf compare <CONTRACT>` | Compare recorded profiles across time windows |
| `advanced-perf generate-dashboard <CONTRACT>` | Show the recorded-metrics performance dashboard |

```bash
starforge advanced-perf profile ./target/wasm32-unknown-unknown/release/token.wasm \
  --label token --dashboard ./target/token-profile.html

starforge advanced-perf profile ./target/wasm32-unknown-unknown/release/token.wasm \
  --baseline ~/.starforge/contract_profiles/profile-abc123def456.json \
  --output ./target/token-profile.json
```

The artifact profiler reports estimated execution time, memory usage, bottlenecks,
baseline regression detection, comparison deltas, and a dashboard summary.

---

## `docs`

AI-assisted documentation generation for Soroban contracts (issue #499).

| Subcommand | Purpose |
|------------|---------|
| `docs generate <CONTRACT> --source <FILE.rs>` | Generate comprehensive Markdown docs from rustdoc + AI enrichment |
| `docs generate <CONTRACT> --source <FILE.rs> --lang rust,ts,python,go` | Multi-language usage examples |
| `docs generate <CONTRACT> --source <FILE.rs> --output docs.md --rustdoc-out stubs.rs` | Write Markdown + rustdoc stubs |
| `docs extract <PATH> [--format json\|markdown]` | Extract rustdoc comments |
| `docs show / list / search / versions / export` | Browse the local docs store (`~/.starforge/docs`) |
| `docs html / api-ref / publish` | HTML site, API reference, and publish helpers |

```bash
starforge docs generate counter --name Counter \
  --source ./contracts/counter/src/lib.rs \
  --lang rust,ts,python \
  --output ./docs/counter.md

starforge docs export counter
starforge docs show counter
```

With `--source`, StarForge extracts `///` / `//!` rustdoc comments, documents functions and types,
infers architecture / storage layout / security notes, and emits multi-language usage guides.
Set `STARFORGE_AI_API_KEY` (optional `STARFORGE_AI_BASE_URL`, `STARFORGE_AI_MODEL`) to refine prose via an OpenAI-compatible API.

---

## `security`

| Subcommand | Purpose |
|------------|---------|
| `audit <PATH>` | Run built-in Soroban analysis plus optional Slither/Mythril integrations |
| `audit --format json\|html --out <FILE>` | Generate machine-readable or HTML audit reports |
| `audit --ci --min-score <N>` | Fail when the audit score is below the CI threshold |
| `audit --ci-workflow-out <FILE>` | Generate a GitHub Actions workflow for security audits |
| `audit --track` | Create remediation tracker items for findings |
| `remediation list` | Review tracked audit and pentest remediation items |

```bash
starforge security audit ./contracts/token/src/lib.rs --format html --out audit.html
starforge security audit ./contracts/token/src/lib.rs --ci --min-score 85
starforge security audit ./contracts/token/src/lib.rs \
  --ci-workflow-out .github/workflows/starforge-security.yml
```

External tools are optional. StarForge runs built-in Soroban heuristics every time and records whether Slither/Mythril were completed, failed, skipped, or unavailable.

---

## `upgrade`

| Subcommand | Purpose |
|------------|---------|
| `upgrade prepare` | Validate upgrade WASM (`--contract-id`, `--wasm`) |
| `upgrade auto compat` | Compare old/new WASM ABI and storage layout (`--old-wasm`, `--new-wasm`) |
| `upgrade auto plan` | Generate compatibility-aware upgrade plan and migration template |
| `upgrade propose` | Create governance proposal |
| `upgrade list` / `status` | List pending proposals |
| `upgrade approve` | Approve proposal |
| `upgrade execute` | Execute approved upgrade |
| `upgrade rollback` | Roll back contract version |
| `upgrade history` | Show upgrade history |

---

## `governance`

Contract upgrade governance with voting, timelock, audit trail, and emergency upgrades.

| Subcommand | Purpose |
|------------|---------|
| `governance propose` | Create upgrade proposal (`--contract-id`, `--wasm`, `--threshold`, `--timelock`) |
| `governance list` | List proposals with optional filters |
| `governance show` | Show proposal details and votes |
| `governance vote` | Cast vote (`--for` or `--against`) |
| `governance reject` | Reject a proposal |
| `governance execute` | Execute after timelock and threshold met |
| `governance emergency` | Emergency upgrade (bypasses timelock) |
| `governance audit` | Show governance audit trail |
| `governance dashboard` | Governance summary dashboard |
| `governance config show/set` | View or update governance defaults |

See [GOVERNANCE.md](GOVERNANCE.md) for the full workflow.

---

## `tutorial`

| Subcommand | Purpose |
|------------|---------|
| `tutorial list` | List tutorials under `./tutorials/` |
| `tutorial start <SLUG>` | Begin guided flow (resets progress) |
| `tutorial next` | Mark step complete and show next milestone |
| `tutorial status` | Show active tutorial and current step |

---

## Utility commands

| Command | Purpose |
|---------|---------|
| `info` | Version, config path, network health, Stellar CLI detection |
| `shell` | Interactive local REPL with persistent history and tab completion |
| `monitor` | Live event/threshold monitoring |
| `benchmark` | CLI performance benchmarks |
| `test` | Soroban WASM test runner |
| `lint <PATH>` | Static Soroban source lint |
| `plugin install/list/run` | Dynamic plugin management |
| `completions <SHELL>` | bash/zsh/fish/powershell completions |

### `monitor`

Live monitoring of contracts or wallets, including Soroban event streaming, routing, alerting, persistence, replay, and dashboard output.

| Option | Purpose |
|--------|---------|
| `--contract <ID>` | Contract ID to monitor via Soroban RPC |
| `--events <EVENTS>` | Comma-separated event names to filter |
| `--type <TYPE>` | Soroban event type filter (`contract`, `system`, `diagnostic`) |
| `--topic <TOPIC>` | Topic segment matcher, comma-separated, with `*` wildcards |
| `--value <VALUE>` | Match event payload text |
| `--transport <TRANSPORT>` | `auto`, `websocket`, or `http` transport selection |
| `--websocket-url <URL>` | Override the derived WebSocket endpoint |
| `--route <NAME=PATTERN>` | Route matching events into named lanes; repeatable |
| `--alert <RULE>` | Alert rule in `pattern`, `severity:pattern`, or `severity:pattern:message` form |
| `--persist [PATH]` | Persist matching events to JSONL, using the default StarForge event store path when PATH is omitted |
| `--replay <PATH>` | Replay events from a JSONL event store instead of connecting live |
| `--dashboard` | Render the event analytics dashboard |
| `--trigger <PATTERN=COMMAND>` | Execute a shell command when a pattern matches; repeatable |
| `--allow-triggers` | Required explicit opt-in before event triggers execute shell commands |
| `--wallet <NAME>` | Wallet name to monitor |
| `--threshold <AMOUNT>` | XLM threshold for notifications |
| `--balance-alert <AMOUNT>` | Alert when wallet balance drops below this amount |
| `--network <NETWORK>` | Network to use |
| `--interval <SECONDS>` | Poll interval in seconds |

Examples:

```bash
starforge monitor --contract CCPYZ... --transport websocket --dashboard
starforge monitor --contract CCPYZ... --route swaps=swap --alert high:mint --persist
starforge monitor --contract CCPYZ... --replay ~/.starforge/events/testnet-CCPYZ....jsonl --dashboard
starforge monitor --contract CCPYZ... --trigger mint=./on-mint.sh --allow-triggers
```

Event stores use JSON Lines. Replay skips malformed records and deduplicates events by
network, contract ID, and Soroban event ID. Triggers inherit event metadata through
`STARFORGE_NETWORK`, `STARFORGE_CONTRACT_ID`, `STARFORGE_EVENT_ID`,
`STARFORGE_EVENT_LEDGER`, `STARFORGE_EVENT_TYPE`, `STARFORGE_EVENT_TOPIC`, and
`STARFORGE_EVENT_VALUE`. Treat trigger commands as trusted local code; they are disabled
unless `--allow-triggers` is explicitly provided.

---

## External plugins

```bash
starforge plugin install my-plugin --path ./libmy_plugin.so
starforge my-plugin <args>
```

## See also

- [API_REFERENCE.md](../API_REFERENCE.md) — detailed per-command examples and output samples
- [DEVELOPER_GUIDE.md](../DEVELOPER_GUIDE.md) — contributing and local development
- [SIMULATION_RESOURCES.md](SIMULATION_RESOURCES.md) — CPU, memory, footprint, and resource fees
- [CORRELATION_IDS.md](CORRELATION_IDS.md) — correlating structured logs across an invocation
- [CONFIGURATION.md](CONFIGURATION.md) — config parsing, overlays, and validation rules
- [WALLET_IMPORT_SECURITY.md](WALLET_IMPORT_SECURITY.md) — limits on untrusted wallet backups
