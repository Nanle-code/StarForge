# Contract invoke guide

`starforge contract invoke` simulates (and optionally submits) a call against a
deployed Soroban contract. This guide covers the invoke path, including
**automatic restore** of archived ledger entries.

## Basic usage

```bash norun
starforge contract invoke C… hello --to Alice --network testnet
starforge contract invoke C… transfer --from G… --to G… --amount 100 \
  --network testnet --wallet deployer --submit
```

| Flag | Purpose |
|------|---------|
| `--network` | `testnet`, `mainnet`, `docker-testnet`, or any configured custom network |
| `--wallet` | Local/hardware wallet used for signing submit and restore |
| `--submit` | Sign and broadcast after a successful simulation |
| `--auto-restore` | Restore archived entries without an interactive prompt |
| `--yes` | Skip confirmation prompts (scripted / CI) |
| `--json` | Machine-readable output (includes `restored`) |

## Archived entries and `restorePreamble`

When a contract instance, WASM code, or persistent data entry has been archived,
`simulateTransaction` returns a `restorePreamble` instead of a usable footprint.
Without a restore, the invoke fails with confusing host errors.

StarForge detects `restorePreamble` in the simulation result and:

1. Shows the restore cost (`restorePreamble.minResourceFee`) before confirming
2. Builds, signs, and submits a `RestoreFootprintOp` (unless cancelled)
3. Re-simulates the original invoke against the restored ledger
4. Continues with the normal submit confirmation when `--submit` is set

### Interactive restore

```bash norun
starforge contract invoke C… increment --network testnet --wallet deployer --submit
```

If the simulation needs a restore, you are prompted with the restore fee.
Confirm to restore, then confirm again to submit the invoke.

### Non-interactive / CI

```bash norun
starforge contract invoke C… increment \
  --network docker-testnet --wallet deployer \
  --auto-restore --yes --submit --json
```

`--json` output includes:

```json
{
  "restored": true,
  "restore_fee_stroops": 250000,
  "restore_tx_hash": "…",
  "submitted": true,
  "tx_hash": "…"
}
```

`restored` is `false` when no restore was required.

## Related: TTL inspection and extension

Use `starforge contract ttl` to inspect and extend entry lifetimes before they
archive:

```bash norun
starforge contract ttl show C… --network testnet
starforge contract ttl show C… --warn-below 10000 --json   # cron / monitor
starforge contract ttl extend C… --ledgers 100000 --wallet deployer --submit
```

See [COMMAND_REFERENCE.md](COMMAND_REFERENCE.md) and
[SIMULATION_RESOURCES.md](SIMULATION_RESOURCES.md) for resource-fee details.
