# Post-deploy smoke tests

A project can declare smoke tests in its `starforge-project.toml`. When
`starforge deploy --execute` succeeds, StarForge runs them against the freshly
deployed contract. A broken initialisation or configuration then shows up in
the deploy itself instead of in the first user report.

A complete sample project lives in
[`examples/smoke-tests/`](../examples/smoke-tests/).

## When smoke tests run

| Situation | Smoke tests |
|-----------|-------------|
| `deploy --execute` succeeds and the Stellar CLI returns a contract ID | **Run**, in declaration order |
| `deploy --execute` fails | Not run (the deploy error is reported as before) |
| `deploy --execute` succeeds but no contract ID appears in the CLI output | Not run; a warning is printed |
| `deploy` without `--execute`, `--dry-run`, or `--json` | Not run |
| `--skip-smoke` is passed | Not run, and the manifest's `smoke_tests` are not loaded |
| No `smoke_tests` in the manifest, or no manifest | Nothing changes: deploy behaves exactly as before |

Every test runs, even after one fails, so a single report shows everything
that is broken.

## Manifest schema

Add one `[[smoke_tests]]` table per test:

```toml
[[smoke_tests]]
name = "hello-returns-greeting"             # required, unique
invoke = { function = "hello", args = ["--to", "world"] }
expect_contains = "world"
timeout_secs = 60

[[smoke_tests]]
name = "health-script"
command = "./scripts/health.sh"
expect_exit = 0
```

| Key | Type | Required | Meaning |
|-----|------|----------|---------|
| `name` | string | yes | Unique, non-empty name shown in the report |
| `invoke` | table | one of `invoke` / `command` | Contract call on the deployed contract, run as `stellar contract invoke --id <contract> --source <deployer> --network <network> -- <function> <args...>` |
| `invoke.function` | string | yes, with `invoke` | Contract function name |
| `invoke.args` | array of strings | no | Arguments after the function name, passed as-is with no shell involved |
| `command` | string | one of `invoke` / `command` | Shell command, run with `sh -c` (`cmd /C` on Windows) from the directory that contains `starforge-project.toml` |
| `expect_exit` | integer | no (default `0`) | Exit code the check must return |
| `expect_output` | string | no | Stdout must equal this exactly, with surrounding whitespace trimmed |
| `expect_contains` | string | no | Stdout must contain this substring |
| `timeout_secs` | integer, 1–3600 | no (default `60`) | Longer runs are killed and count as a failure |

When several assertions are set, all of them must hold. The exit code is
checked first.

`command` checks receive the deployment through environment variables:

| Variable | Value |
|----------|-------|
| `STARFORGE_CONTRACT_ID` | Contract ID returned by the deploy |
| `STARFORGE_NETWORK` | Network deployed to (`testnet` / `mainnet`) |
| `STARFORGE_SOURCE` | Source account the deploy used |

### Validation

Smoke test declarations are validated when the manifest loads, which happens
**before** the deploy starts. A mistake therefore never surfaces after a
contract is already live. The deploy is refused with a configuration error
(exit code 3) when:

- a test is missing `name`, or two tests share a name
- a test sets both `command` and `invoke`, or neither of them
- `command` or `invoke.function` is empty
- `timeout_secs` is outside 1–3600
- a key is unknown (for example the typo `expect_contain`)

## Results and exit codes

Each test prints one labelled line:

```text
  SMOKE PASS  hello-returns-greeting (412 ms)
  SMOKE FAIL  counter-starts-at-zero (388 ms): expected output "0", got "7"
  SMOKE FAIL  health-script (60001 ms): timed out after 60s
```

A smoke failure is kept separate from a deploy failure:

| Code | Name | Meaning |
|------|------|---------|
| 0 | SUCCESS | Deploy succeeded and every smoke test passed |
| 6 | EXECUTION_ERROR | The deploy itself failed; smoke tests did not run |
| 3 | CONFIG_ERROR | Invalid `smoke_tests` declaration; nothing was deployed |
| **9** | **SMOKE_TEST_FAILURE** | The deploy succeeded, but at least one smoke test failed or timed out |

When a smoke test fails, the deploy is still recorded as successful in
`starforge deployments`, because the contract is on-chain. The error message
says so:

```text
Post-deploy smoke tests failed (1 of 3). The deployment itself succeeded:
contract CABC…XYZ is live on testnet.
```

In CI, a check like `if [ $? -eq 9 ]` separates "the deployed contract
misbehaves" from "the deploy never happened".
