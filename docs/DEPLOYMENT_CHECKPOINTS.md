# Deployment Checkpointing and Resumability Guide

StarForge automatically persists deployment progress to disk, enabling deployment operations to resume safely after unexpected interruptions (network dropouts, process kills, RPC timeouts) without duplicating succeeded steps. Re-running a deployment command that already fully succeeded is an idempotent safe no-op.

---

## Key Features

- **Automatic Checkpointing**: State is saved atomically to disk after each successful deployment step.
- **Resumability**: On restart, StarForge detects existing checkpoint files for the session and resumes from the first uncompleted step.
- **Idempotency**: Executing a deployment pipeline that is already completed reports `"already up to date"` and returns immediately without re-executing actions.
- **Content Staleness Detection**: Checkpoint sessions match raw binary content (SHA-256 hash) of the WASM file and deployment configuration parameters. If WASM bytecode or deployment configuration changes between runs, the existing checkpoint is flagged as stale, a warning is printed, and a fresh deployment begins.
- **Corrupted State Resilience**: If a checkpoint file is corrupted, truncated, or unparseable, StarForge prints a warning, discards the corrupted file, and proceeds cleanly with a fresh deployment.
- **Schema Versioning**: Checkpoint files include `schema_version` (current: `1`). Version mismatches trigger an automatic migration warning and fresh initialization.
- **Concurrency Protection**: Session operations acquire a file lock (`.lock`) to prevent concurrent process executions from corrupting checkpoint state.

---

## User CLI Usage

### Basic Command

When running deployment automation:

```bash
starforge deployment-automate run --wasm ./target/wasm32-unknown-unknown/release/my_contract.wasm --network testnet
```

If the run is interrupted at step 3, simply re-run the exact same command. StarForge will output:

```text
[checkpoint] Resuming deployment session 'a1b2c3d4' (checkpoint schema v1).
[checkpoint] Step 'pre_deployment_validation' already completed (reusing cached result).
[checkpoint] Step 'automated_testing' already completed (reusing cached result).
Running deployment execution...
```

### Forcing a Fresh Deployment (`--fresh` / `--force`)

To ignore any existing checkpoints and execute all steps from scratch:

```bash
starforge deployment-automate run --wasm ./my_contract.wasm --network testnet --fresh
```

`--force` is supported as a CLI alias to `--fresh`.

---

## Security Considerations

1. **No Secrets Saved**: Checkpoint JSON files contain state identifiers, hashes, step outcomes, and public metrics. **Private keys and secret parameters are never saved** to checkpoint files.
2. **File Permissions**: Checkpoint files are written under `~/.starforge/checkpoints/` using restricted user permissions.

---

## Migration and Compatibility Notes

- **Schema Version**: `schema_version = 1`. Future StarForge releases with checkpoint schema changes will increment `schema_version`. Older checkpoint files will produce a clear warning and reset to a fresh deployment run automatically.
- **Backward Compatibility**: Projects built with earlier versions of StarForge will automatically initialize checkpointing on their next deployment operation.
