# Contract Upgrade Governance System

## 1. Overview
StarForge provides an on-chain/off-chain contract upgrade governance system for Soroban smart contracts. It guarantees safe, auditable, and multi-step upgrade lifecycles.

## 2. Governance Workflow
```
[Propose Upgrade] ──> [Voting Period] ──> [Threshold Reached (Passed)] ──> [Timelock Delay] ──> [Execution Ready] ──> [Execute Upgrade]
                                     └──> [Quorum Failed (Rejected)]
```

## 3. CLI Commands
- `starforge governance propose --contract-id <ID> --wasm <PATH> --description <DESC> --threshold <N> --timelock <SECS>`
- `starforge governance vote --proposal-id <ID> --for / --against`
- `starforge governance show --proposal-id <ID>`
- `starforge governance execute --proposal-id <ID>`
- `starforge governance emergency --proposal-id <ID> --reason <REASON>`
- `starforge governance audit --proposal-id <ID>`
- `starforge governance dashboard`
- `starforge governance config show / set`

## 4. Emergency Upgrades
Emergency upgrades bypass the standard timelock delay but require explicit cryptographic multi-guardian quorum approval (`emergency_quorum`). All emergency actions generate high-priority audit log entries.

## 5. Upgrade Rehearsal (Forked-State Dry Run)

Static checks (`migrate introspect`, `migrate hazards`) compare *layouts*; a rehearsal adds a
**dynamic** check against real state — the place where most upgrade bugs surface. Before
proposing an upgrade, replay a scripted set of calls against a snapshot of the live contract
and confirm that existing storage still decodes and key functions still behave.

```bash
starforge upgrade rehearse \
  --contract <CONTRACT_ID> \
  --wasm new.wasm \
  --calls rehearsal.toml \
  --snapshot snapshot.json \
  --json > rehearsal-report.json
```

| Flag | Description |
|---|---|
| `--contract <ID>` | Contract being upgraded (falls back to `contract` in the script). |
| `--wasm <PATH>` | The new compiled `.wasm`; its hash is recorded in the report. |
| `--calls <PATH>` | Rehearsal script (TOML) describing the calls to replay. |
| `--snapshot <PATH>` | Ledger snapshot (JSON) captured from the live contract. |
| `--json` | Emit the report as JSON instead of text. |

### Rehearsal script (`rehearsal.toml`)

```toml
[[calls]]
name = "read_total_supply"
function = "total_supply"
expect = "1000"

[[calls]]
name = "pause_contract"
function = "set_paused"
args = [true]
expect = "ok"

# Layout the new contract expects. A key whose type differs from the
# snapshot is reported as a storage decode failure.
[[storage]]
key = "total_supply"
value_type = "u128"
tier = "persistent"
```

### What the report contains

- **Storage decode check** — keys checked, added keys, dropped keys, and every
  key that can no longer be decoded in place (type or storage-tier change).
- **Scripted calls** — per call: expected vs. actual value and pass/fail/error.
- **Overall verdict** — passes only when storage still decodes *and* every call passes.

The rehearsal exits non-zero when it fails, so it can gate a proposal in CI. The
rendered report (text or `--json`) is stable and deterministic and can be attached
directly to the governance proposal for reviewers.

> Rehearsal is a local, offline check: it never submits a transaction and performs
> no network I/O. Capture the snapshot first with `starforge snapshot create`.

