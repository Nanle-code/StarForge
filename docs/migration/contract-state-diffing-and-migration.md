# Contract State Diffing and Migration Tools

## 1. Overview
StarForge provides a snapshot-based contract state migration and diffing engine for Soroban smart contracts. It enables deterministic schema transitions, automated pre-migration backups, invariant validation, and instantaneous rollbacks.

## 2. Migration Workflow & Commands

### 2.1 State Snapshotting and Diffing
```bash
# Capture a contract storage snapshot
starforge migrate snapshot --contract-id <ID> --version v1 --output snapshot-v1.json

# Diff state between two version snapshots
starforge migrate diff --from snapshot-v1.json --to snapshot-v2.json
```

### 2.2 Script Generation & Testing
```bash
# Generate migration rules template or script
starforge migrate init --from v1 --to v2 --output migration-rules.json

# Dry-run rules in testing framework
starforge migrate test --rules migration-rules.json --sample snapshot-v1.json
```

### 2.3 Execution, Validation & Rollback
```bash
# Execute migration (creates automated backup)
starforge migrate run --input snapshot-v1.json --rules migration-rules.json --output snapshot-v2.json

# Validate state transitions and schema invariants
starforge migrate validate --snapshot snapshot-v2.json --rules migration-rules.json

# Roll back to pre-migration backup
starforge migrate rollback --migration-id <ID>
```
