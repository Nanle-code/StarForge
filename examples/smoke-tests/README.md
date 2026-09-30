# Sample: post-deploy smoke tests

This sample shows the `[[smoke_tests]]` section of `starforge-project.toml`.
The tests target the `simple-counter` template.

| File | What it shows |
|------|---------------|
| [`starforge-project.toml`](starforge-project.toml) | Passing tests: two contract invocations and one shell check |
| [`failing/starforge-project.toml`](failing/starforge-project.toml) | One deliberately failing assertion |

## Try it

1. Build a counter contract from `templates/examples/simple-counter`. It
   exposes `get_count`, `increment` and `reset`.
2. Copy `starforge-project.toml` from this directory to the root of that
   project.
3. Deploy from inside the project. The smoke tests run once the deploy is
   confirmed:

```bash norun
starforge deploy --wasm path/to/counter.wasm \
  --wallet deployer --network testnet --yes --execute
```

Expected output after the deploy report:

```text
  SMOKE PASS  counter-starts-at-zero (402 ms)
  SMOKE PASS  increment-returns-one (5130 ms)
  SMOKE PASS  contract-id-is-a-strkey (6 ms)

✓ 3 smoke test(s) passed
```

With `failing/starforge-project.toml` in place instead, the deploy still
succeeds, but the command exits with code **9** (`SMOKE_TEST_FAILURE`):

```text
  SMOKE PASS  counter-starts-at-zero (398 ms)
  SMOKE FAIL  counter-is-42 (401 ms): expected output "42", got "0"

✗ 1 of 2 smoke test(s) failed. The deployment succeeded and the contract is live.
```

Pass `--skip-smoke` to deploy without running the tests. The full schema is
in [docs/SMOKE_TESTS.md](../../docs/SMOKE_TESTS.md).
