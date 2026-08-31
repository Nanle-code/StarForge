# Batch Invoke Cost Forecasting

`starforge cost forecast-batch` prices a whole batch of planned contract
invokes **before any of them are submitted**. A batch that is submitted
blind can run the source account out of XLM partway through, stranding the
remaining calls — forecasting first turns "hope we have enough" into a
number.

## Command

```bash
starforge cost forecast-batch <MANIFEST> [--network <NET>] [--margin <PCT>] [--inclusion-fee <STROOPS>] [--enforce]
```

| Flag | Default | Purpose |
|---|---|---|
| `<MANIFEST>` | required | Path to a JSON or YAML batch manifest of invoke intents |
| `--network <NAME>` | `testnet` | Default network when the manifest does not specify one |
| `--margin <PERCENT>` | `20` | Safety margin over simulated minimum resource fees (`0`–`1000`) |
| `--inclusion-fee <STROOPS>` | `100` | Per-operation inclusion (base) fee |
| `--enforce` | off | Exit non-zero when the forecast exceeds the manifest budget or a per-invoke fee cap |

With `--enforce` the command is a CI gate: it exits non-zero if the batch
would breach the configured `budget_xlm` or any `max_fee_stroops`, so a
pipeline can refuse to run a batch it cannot afford.

## Batch manifest

A manifest is a list of **invoke intents**: contract, function, and
arguments. Fields may reference environment variables with `${VAR}`
interpolation. Each invoke may override the manifest-level network, and may
carry an optional per-call fee cap.

### JSON

```json
{
  "version": 1,
  "network": "testnet",
  "budget_xlm": 0.5,
  "invokes": [
    {
      "name": "mint-to-alice",
      "contract_id": "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABKT4",
      "function": "mint",
      "args": [
        { "value": "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF", "type": "address" },
        { "value": "1000", "type": "int" }
      ],
      "max_fee_stroops": 400000
    },
    {
      "name": "check-balance",
      "contract_id": "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABKT4",
      "function": "balance",
      "args": [
        { "value": "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF", "type": "address" }
      ]
    }
  ]
}
```

A ready-to-edit copy lives at `examples/batch-invoke-manifest.json`.

### YAML

```yaml
version: 1
network: testnet
budget_xlm: 0.5
invokes:
  - name: mint-to-alice
    contract_id: CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABKT4
    function: mint
    args:
      - value: GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF
        type: address
      - value: "1000"
        type: int
    max_fee_stroops: 400000
  - name: check-balance
    contract_id: CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABKT4
    function: balance
    args:
      - value: GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF
        type: address
```

| Field | Required | Meaning |
|---|---|---|
| `version` | yes | Manifest schema version; currently `1`. |
| `network` | no | Default network for every invoke without its own. |
| `budget_xlm` | no | Aggregate batch cap in XLM — exceeded ⇒ flagged, and `--enforce` fails. |
| `invokes[]` | yes | At least one invoke intent. |
| `invokes[].name` | no | Human-readable label shown in the report. |
| `invokes[].contract_id` | yes | Contract to call (Stellar contract strkey). |
| `invokes[].function` | yes | Function to invoke. |
| `invokes[].args[]` | no | `{ value, type }` pairs; `type` is `string`, `symbol`, `int`, `bool`, or `address`. |
| `invokes[].network` | no | Per-invoke network override. |
| `invokes[].max_fee_stroops` | no | Per-call cap in stroops — exceeded ⇒ flagged, and `--enforce` fails. |

## How fees are estimated

Soroban fees come from `simulateTransaction`: the RPC runs the call against
the real ledger and returns the **minimum resource fee**. The forecast uses
the sim-first, heuristic-only-when-necessary strategy:

1. **Simulate** every invoke against its network. The reported fee gets the
   safety `--margin` and the `--inclusion-fee` applied, exactly like
   `starforge cost resources`.
2. **Fall back to a local heuristic** when a simulation fails (contract not
   deployed, RPC unreachable, network misconfigured). Heuristic fees are
   deliberately conservative (scaled by argument count, argument size, and
   function-name length).
3. **Highlight high-variance calls.** A call is flagged `HIGH` when:
   - its fee had to be guessed by the heuristic instead of simulated, or
   - its fee exceeds its per-call `max_fee_stroops`, or
   - its fee is more than 3× away from the batch median (a likely outlier).

The table and totals are printed for every call plus the aggregate, so the
whole-batch liability is visible before submission:

```
Batch Invoke Cost Forecast
──────────────────────────
Manifest              examples/batch-invoke-manifest.json
Default network       testnet
Budget                0.5000000 XLM
Invokes               3

#    Call               Network   Fee (stroops)  Fee (XLM)      Source    Variance
1    approve-allowance  testnet   177243          0.0177243     simulated ok
2    transfer-tokens    testnet   118290          0.0118290     simulated ok
3    balance-check      mainnet   100101          0.0100101     heuristic HIGH

Simulated                    2
Heuristic (not simulated)    1
Min / Max / Median           100101 / 177243 / 118290 stroops
Average per invoke           131878 stroops
Estimated batch total        395634 stroops (0.0395634 XLM)

High-variance calls:
  [3] balance-check — simulation unavailable; fee is a local heuristic
      error: Soroban RPC request to ... failed

⚠ 1 of 3 calls could not be simulated and use local heuristic fees
```

The `estimated batch total` (stroops and XLM) is the number to compare
against the wallet balance before submitting. Simulated calls are the
authoritative prices; a batch with many `HIGH` calls should be re-forecast
once the RPC is reachable.

## See also

- [SIMULATION_RESOURCES.md](SIMULATION_RESOURCES.md) — how simulated resource fees work
- [docs/COMMAND_REFERENCE.md](COMMAND_REFERENCE.md) — every command and flag
- [GAS_OPTIMIZATION_BEST_PRACTICES.md](GAS_OPTIMIZATION_BEST_PRACTICES.md) — reducing fees in the first place