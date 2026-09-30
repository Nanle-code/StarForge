# Unified `--dry-run` semantics

`--dry-run` means the same thing for every state-changing command:

1. **Simulate fully.** The command resolves and validates everything it would
   need — inputs, wallet and network selection, target files, estimated costs.
2. **Show the plan.** It prints every operation it *would* perform, plus any
   estimated cost or impact, in a consistent format.
3. **Change nothing.** No file is written and no transaction is submitted.
   Re-running the same command without `--dry-run` applies the plan.

The flag is global, so it is accepted before or after a subcommand:

```console
$ starforge --dry-run wallet create deployer
$ starforge wallet create deployer --dry-run
$ starforge --json config set telemetry.enabled true --dry-run
```

> The `deploy` and `contract invoke-script` commands already had their own
> `--dry-run` handling. They now share the same semantics and also honour the
> global flag, so both placements behave identically.

## Human-readable plan

Every command renders the same shape: a command header, a summary, a table of
planned operations with per-operation details, optional cost/impact rows,
optional warnings, and a closing notice that nothing changed.

```text
Dry-run plan: config set
Set configuration key 'telemetry.enabled' to 'true'

Planned operations:
  #   KIND              TARGET                            DESCRIPTION
  1   config.write      configuration store               would persist 'telemetry.enabled' = 'true'
      Config file: /home/me/.starforge/config.toml
      Database: /home/me/.starforge/starforge.db

No changes applied — this was a dry run. Re-run without --dry-run to apply the plan.
```

## Machine-readable plan

Add `--json` (or set `STARFORGE_OUTPUT_JSON=1`) to receive the plan in the CLI's
standard JSON envelope instead:

```console
$ starforge --json network add staging --horizon-url https://horizon.staging.example --dry-run
```

```json
{
  "version": 1,
  "ok": true,
  "data": {
    "command": "network add",
    "summary": "Add custom network 'staging'",
    "network": "staging",
    "dry_run": true,
    "operations": [
      {
        "kind": "config.write",
        "target": "staging",
        "description": "would add custom network 'staging' to the configuration",
        "details": [{ "label": "Horizon", "value": "https://horizon.staging.example" }]
      }
    ],
    "writes_filesystem": true,
    "submits_transactions": false
  }
}
```

`dry_run` is always `true`, `writes_filesystem` and `submits_transactions`
describe what applying the plan would do, and `operations` is always present
(possibly empty).

## Inventory of covered mutating commands

| Family | Mutating subcommands with a plan |
| --- | --- |
| `wallet` | `create`, `fund`, `remove`, `rename`, `merge`, `rotate`, `export`, `import`, `import-shares`, `tune-kdf`, `multisig create`, `multisig sign`, `multisig submit` |
| `config` | `set`, `set-encryption`, `db init`, `db migrate`, `db backup`, `db restore`, `db export --out` |
| `network` | `switch`, `add`, `remove`, `rename` |
| `template` | `install`, `publish`, `remove`, `new`, `fetch`, `update`, `rollback`, `customize`, `customize-rollback`, `docs --output` |
| `plugin` | `install`, `uninstall`, `update` |
| `deploy` | full deployment plan (pre-existing dry run, now unified) |
| `contract invoke-script` | resolved call list (pre-existing dry run, now unified) |

Read-only commands (`list`, `show`, `search`, `info`, `verify`, `audit`, `test`,
`lint`, `validate`, `config db query/status/check`, hardware inspection, …) have
nothing to simulate, so `--dry-run` is accepted and they run normally.

## Implementation

The semantics live in a single shared module, `utils::dry_run`:

- `set_enabled` / `is_enabled` hold the process-wide flag, set once in `main`
  from the global `--dry-run` argument.
- `DryRunPlan`, `PlannedOperation`, and `PlanField` build the common plan.
- `DryRunPlan::emit(json)` renders the human table or the JSON envelope.

Each mutating command handler checks `is_enabled()` first, builds its plan with
the shared types, and returns *before* touching the filesystem or the network.
Read-only handlers fall through unchanged. New command families can adopt the
same guarantee by emitting a `DryRunPlan` instead of performing their mutation.

## Tests

- `src/utils/dry_run.rs` unit-tests the plan builder, the human renderer, and
  the JSON shape (including the omitting of unset optional fields).
- `tests/dry_run_unified.rs` runs the real binary against an isolated config
  directory and asserts that a dry run leaves configuration, networks, wallets,
  plugins, and template files untouched, and that `--json` emits the plan.
