# Contract Rollback Testing Framework

StarForge includes a rollback-specific test harness for validating that a Soroban contract upgrade can be safely reverted without losing critical contract state.

The harness is designed for CI and local pre-release checks. It compares a previous WASM rollback target with an upgraded WASM, applies one or more rollback scenarios against a deterministic mock state model, and fails when preserved keys, state invariants, data integrity checks, or performance budgets are violated.

## Quick Start

Run the default rollback scenario:

```bash norun
starforge test \
  --wasm target/wasm32v1-none/release/contract_v2.wasm \
  --rollback \
  --previous-wasm target/wasm32v1-none/release/contract_v1.wasm \
  --report json
```

Run custom scenarios:

```bash norun
starforge test \
  --wasm ./contract_v2.wasm \
  --rollback \
  --previous-wasm ./contract_v1.wasm \
  --rollback-scenario ./rollback-scenarios/token-balances.json \
  --rollback-scenario ./rollback-scenarios/admin-controls.json \
  --rollback-performance-budget-ms 500 \
  --report html
```

Reports are written under StarForge's reports directory in the local StarForge config folder.

## What the Harness Validates

| Acceptance criterion | Harness coverage |
| --- | --- |
| Rollback harness works | `starforge test --rollback` runs a dedicated rollback harness over previous/upgraded WASM pairs. |
| State preservation tested | `preserved_keys` compare values immediately before upgrade with values after rollback. |
| Scenario testing | One JSON file can define a single scenario; another can define an array of scenarios. Pass multiple `--rollback-scenario` flags to compose suites. |
| Integrity checks | `key_exists`, `key_absent`, `equals`, `checksum_unchanged`, `numeric_sum_equals`, and `no_unexpected_keys` checks are supported. |
| Performance testing | Each scenario receives a duration check against `max_duration_ms` or `--rollback-performance-budget-ms`. |
| Documentation | This document defines workflow, schema, and CI usage. |

## Scenario Model

A scenario models the storage lifecycle around an upgrade and rollback:

1. `initial_state` seeds contract storage.
2. `pre_upgrade_mutations` optionally create state that should exist immediately before the upgrade.
3. The harness snapshots this pre-upgrade state.
4. `upgrade_mutations` simulate migration or behavior introduced by the upgraded WASM.
5. `rollback_mutations` simulate the rollback path back to the previous contract version.
6. `preserved_keys`, `expected_after_rollback`, and `integrity_checks` validate the final state.

This deterministic mock model is intentionally independent of a live chain so that rollback safety tests can run quickly in CI. It should be used alongside integration or testnet rollback drills for final release validation.

## Scenario Schema Example

```json
{
  "name": "token_balances_survive_rollback",
  "description": "Critical balances and supply remain intact when v2 is rolled back to v1.",
  "initial_state": {
    "admin": "GADMIN",
    "balance:alice": 1000,
    "balance:bob": 500,
    "total_supply": 1500,
    "schema_version": 1
  },
  "pre_upgrade_mutations": [],
  "upgrade_mutations": [
    { "operation": "set", "key": "schema_version", "value": 2 },
    { "operation": "set", "key": "feature:new_accounting", "value": true }
  ],
  "rollback_mutations": [
    { "operation": "set", "key": "schema_version", "value": 1 }
  ],
  "preserved_keys": [
    "admin",
    "balance:alice",
    "balance:bob",
    "total_supply"
  ],
  "expected_after_rollback": {
    "schema_version": 1,
    "balance:alice": 1000,
    "balance:bob": 500,
    "total_supply": 1500
  },
  "integrity_checks": [
    { "kind": "key_exists", "key": "admin" },
    {
      "kind": "checksum_unchanged",
      "keys": ["admin", "balance:alice", "balance:bob", "total_supply"]
    },
    {
      "kind": "numeric_sum_equals",
      "keys": ["balance:alice", "balance:bob"],
      "expected_sum": 1500
    }
  ],
  "max_duration_ms": 1000
}
```

A file may also contain an array of scenarios:

```json
[
  { "name": "scenario_one", "initial_state": {}, "max_duration_ms": 1000 },
  { "name": "scenario_two", "initial_state": {}, "max_duration_ms": 1000 }
]
```

## Mutation Operations

| Operation | Required fields | Behavior |
| --- | --- | --- |
| `set` | `key`, `value` | Writes or replaces a state value. |
| `delete` | `key` | Removes a key from state. |
| `increment` | `key`, integer `value` | Adds the integer value to an existing numeric key, or starts at `0` when absent. |

## Integrity Check Types

| Check | Required fields | Purpose |
| --- | --- | --- |
| `key_exists` | `key` | Ensures a critical key remains present after rollback. |
| `key_absent` | `key` | Ensures a temporary or unsafe migration key is removed after rollback. |
| `equals` | `key`, `value` | Ensures a key has an exact final value. |
| `checksum_unchanged` | optional `keys` | Compares a canonical SHA-256 checksum before upgrade and after rollback. If `keys` is omitted, the full state map is compared. |
| `numeric_sum_equals` | `keys`, `expected_sum` | Ensures a set of numeric values preserves a supply or balance total. |
| `no_unexpected_keys` | `allowed_keys` | Fails if rollback leaves any keys outside an allowlist. |

## Recommended Rollback Test Suite

For each upgrade, add scenarios that cover:

- balances, allowances, ownership/admin keys, authorization lists, and supply counters;
- storage schema migrations and reverse migrations;
- rollback after partially completed feature initialization;
- rollback after user activity on the upgraded version;
- removal of temporary migration keys;
- performance budgets for high-volume state maps.

## CI Example

```yaml
- name: Rollback safety tests
  run: |
    cargo run -- test \
      --wasm artifacts/contract_v2.wasm \
      --rollback \
      --previous-wasm artifacts/contract_v1.wasm \
      --rollback-scenario tests/rollback/token-balances.json \
      --rollback-scenario tests/rollback/admin-controls.json \
      --rollback-performance-budget-ms 750 \
      --report json
```

A non-zero exit indicates at least one rollback scenario failed and the upgrade should not be shipped until data preservation is fixed.
