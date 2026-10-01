# Pre-Mainnet Deployment Checklist

`starforge deploy checklist` reports the readiness checks for a WASM artifact,
wallet, and network. It does not submit a transaction. The same required checks
run automatically before `starforge deploy run --execute --network mainnet`.

```bash
starforge deploy checklist --wasm ./target/wasm32v1-none/release/contract.wasm \
  --network mainnet --wallet deployer

# Machine-readable report; exits non-zero when any required check fails.
starforge deploy checklist --wasm ./contract.wasm --network mainnet --wallet deployer --json
```

## Checks

- **WASM hash reproducibility** — computes the artifact SHA-256 and compares it
  with the reviewed project pin. Generate that pin from a clean reproducible build; see
  [REPRODUCIBLE_BUILDS.md](REPRODUCIBLE_BUILDS.md).
- **Simulation success** — calls Soroban RPC to simulate the deploy and fails
  on RPC errors or simulation diagnostics.
- **Balance** — reads the native XLM balance from Horizon and compares it to the
  project's configured minimum.
- **Authorization setup** — confirms a local signing key or a selected,
  connected hardware wallet is available and that wallet policy permits the
  target network.
- **Network identity** — compares the Horizon-advertised passphrase with the
  configured passphrase for the selected network.

All checks are reported. Only checks named in `required_checks` gate a deploy;
other failures are shown as advisory warnings.

## Project customization

Set `[deployment_checklist]` in the project-root `starforge-project.toml`. The
project lockfile is shared, secret-free configuration. All five checks are
required by default; use `required_checks` to make a subset blocking. Set the
minimum account balance and pin the reviewed reproducible artifact hash:

```toml
[deployment_checklist]
expected_wasm_hash = "<64-character-reviewed-sha256>"
minimum_balance_xlm = 5.0
required_checks = [
  "wasm_hash_reproducibility",
  "simulation_success",
  "balance",
  "auth_setup",
  "network_identity",
]
```

The accepted check IDs are `wasm_hash_reproducibility`, `simulation_success`,
`balance`, `auth_setup`, and `network_identity`. Unknown IDs, duplicates,
malformed hashes, and invalid balance thresholds are rejected when loading the
project manifest. The hash pin must be updated when the reviewed artifact
changes.

## Mainnet override

A failed required check stops an executed mainnet deployment before signing or
submission. To proceed despite the reported failures, an operator must
explicitly pass `--override-checklist` to `deploy run`; `--yes` does not bypass
this gate. The override is recorded in command output and does not change the
checklist report. Review every failure before overriding.

```bash
starforge deploy run --wasm ./contract.wasm --network mainnet \
  --wallet deployer --execute --override-checklist
```
