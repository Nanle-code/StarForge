# CLI Latency Budgets

StarForge enforces **latency budgets** for its CLI's cold-start and command
dispatch paths.  This ensures that every `starforge` invocation feels snappy
and that performance regressions are caught in CI before they reach users.

---

## What is a latency budget?

A **latency budget** is a per-command upper bound on wall-clock execution time
(e.g. `cli_info` must complete within **300 ms** median).  Budgets are defined
in [`src/utils/latency_budget.rs`](./src/utils/latency_budget.rs) and checked
against Criterion benchmark results.

| Budget label                | Default max (ms) | CV threshold | Description                                |
|-----------------------------|------------------|--------------|--------------------------------------------|
| `cli_cold_start_info`       | 500              | 0.15         | Cold start: `starforge -q info`            |
| `cli_cold_start_help`       | 350              | 0.15         | Cold start: `starforge --help`             |
| `cli_cold_start_version`    | 300              | 0.15         | Cold start: `starforge --version`          |
| `cli_wallet_list`           | 300              | 0.20         | `starforge wallet list`                    |
| `cli_wallet_show`           | 350              | 0.20         | `starforge wallet show <name>`             |
| `cli_network_show`          | 250              | 0.20         | `starforge network show`                   |
| `cli_network_switch`        | 300              | 0.20         | `starforge network switch <net>`           |
| `cli_config_show`           | 300              | 0.20         | `starforge config show`                    |
| `cli_info`                  | 300              | 0.20         | `starforge info`                           |
| `cli_template_list`         | 350              | 0.20         | `starforge template list`                  |
| `cli_template_search`       | 400              | 0.20         | `starforge template search <query>`        |
| `cli_deploy_help`           | 350              | 0.20         | `starforge deploy --help`                  |
| `cli_benchmark_wasm`        | 500              | 0.20         | `starforge benchmark wasm`                 |

> **CV threshold**: the maximum **coefficient of variation** (std_dev / mean)
> before a measurement is flagged as _noisy_ (yellow / warning).  High noise
> means results are unreliable — the run should be retried with more samples.

---

## How budgets are checked

1. **Criterion benchmarks** measure median latency for each path
   ([`benches/benchmarks.rs`](./benches/benchmarks.rs), groups
   `cli_cold_start`, `cli_command_latency`, `latency_budget`).

2. The **latency budget engine** ([`src/utils/latency_budget.rs`](./src/utils/latency_budget.rs))
   compares the measured median against the budget:

   - **PASS**  — median ≤ budget, CV ≤ threshold
   - **FAIL**  — median > budget
   - **NOISY** — median OK, but CV > threshold (retry recommended)
   - **SKIPPED** — budget inactive for this environment
   - **ERROR** — missing measurement or invalid input

3. A **JSON report** is written to `target/criterion/latency-budget-report.json`
   and consumed by CI to gate the pipeline.

---

## Running budget checks locally

```bash
# Run only the latency benchmarks
cargo bench -- locked -- cli_cold_start cli_command_latency latency_budget

# Run the full suite (includes gas analyzer benchmarks)
cargo bench

# Override a budget via environment variable (no code change needed)
STARFORGE_LATENCY_BUDGET_CLI_INFO=800 cargo bench -- cli_cold_start
```

---

## Environment variable overrides

Each budget can be overridden at runtime with the environment variable
`STARFORGE_LATENCY_BUDGET_<UPPER_LABEL>`.

| Value                  | Effect                                  |
|------------------------|-----------------------------------------|
| A positive integer (ms)| Sets the budget's `max_median`          |
| `off` or `0`           | Deactivates the budget (always skipped) |
| Any other string       | Silently ignored (fallback to default)  |

Examples:

```bash
# Double the info budget to 600 ms (useful on slower CI runners)
STARFORGE_LATENCY_BUDGET_CLI_INFO=600 cargo bench -- cli_cold_start

# Deactivate the wallet list budget entirely
STARFORGE_LATENCY_BUDGET_CLI_WALLET_LIST=off cargo bench -- cli_cold_start
```

---

## CI integration

The [`benchmark-latency.yml`](.github/workflows/benchmark-latency.yml) workflow
runs on every push / PR that touches `.rs` files, `Cargo.toml`, or the benches
directory.  It:

1. Builds the project
2. Runs the latency benchmark groups
3. Generates and inspects the JSON budget report
4. **Fails the workflow** if any budget is violated
5. Uploads the Criterion report as a build artefact
6. Comments on the PR with a budget summary table

### PR CI Optimization

For pull requests, the workflow runs only **critical interactive hot paths** to keep CI fast:
- `cli_cold_start_info` - Basic info command (500ms budget)
- `cli_cold_start_help` - Help command (350ms budget)  
- `cli_wallet_list` - Wallet listing (300ms budget)
- `cli_config_show` - Config display (300ms budget)
- `cli_info` - General info (300ms budget)

These paths are marked as "critical hot paths" because they represent the most common interactive commands that users run frequently. Ensuring these remain fast provides the best user experience.

For pushes to main/master, the full latency budget suite runs, including all benchmarks.

---

## Adding a new budget

1. Add a new entry to the `default()` impl of `LatencyBudgets` in
   [`src/utils/latency_budget.rs`](./src/utils/latency_budget.rs):

   ```rust
   LatencyBudget::new_static("cli_my_new_command", 400, Some(0.20), true),
   ```

2. Add a corresponding Criterion benchmark function in
   [`benches/benchmarks.rs`](./benches/benchmarks.rs) with a matching label.

3. Register the function in the `criterion_group!` macro at the bottom of
   `benches/benchmarks.rs`.

4. Run `cargo test --test latency_budget_test` to verify the new budget is
   picked up by the tests.

5. Run `cargo bench -- cli_cold_start cli_command_latency` to establish a
   baseline for the new path.

---

## Security notes

- Latency budgets are **purely local measurements**.  No measurement data is
  transmitted over the network.
- Environment variable overrides are scoped to the current process; they are
  never persisted or saved to config files.
- Budget definitions are compiled into the binary.  There is no external
  configuration file that could be tampered with to alter performance guarantees.

---

## Migration notes

If you are upgrading from a version without latency budgets (before this
feature was introduced), the new CI workflow will start checking budgets
automatically.  To avoid breakage on a slow CI runner:

1. Set the env var overrides listed in the workflow file to generous values.
2. Run one CI cycle, inspect the generated report, then tighten budgets.

---

## Compatibility

- **Platforms**: Linux (CI), macOS, and Windows are all supported.
- **Rust version**: Rust 1.70+ (stable).
- **No external services** required.  Budget checks work fully offline.
