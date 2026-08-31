# Test Fixtures

This directory contains binary fixtures and documentation for StarForge's test suite.

## Contract Fixture Factory

The `starforge::utils::contract_fixtures` module provides pre-built fixture factories
for integration tests. These fixtures create deterministic, reusable test contexts
for common Soroban contract patterns.

### Available Fixtures

| Factory Function | Purpose | Accounts | Storage Seeds |
|-----------------|---------|----------|---------------|
| `counter_fixture()` | Simple counter contract | admin, caller | count (instance) |
| `token_fixture()` | SEP-41 token contract | admin, minter, user_a, unauthorized | total_supply (persistent), decimals (instance) |
| `auth_fixture()` | Authorization/RBAC contract | admin, operator, viewer, unauthorized | admin (instance), operator_role (persistent) |
| `multisig_fixture(n)` | Multisig contract with n+1 signers | admin + n signers | signers (persistent), threshold (instance) |
| `liquidity_pool_fixture()` | DEX/AMM contract | lp_provider, trader | reserve_a/b (persistent), fee_bps (instance) |
| `deterministic_fixture(seed)` | Snapshot-friendly fixture with seed | admin, user | count (instance) |

### Quick Start

```rust
use starforge::utils::contract_fixtures::{
    counter_fixture, token_fixture, auth_fixture,
    FixtureRegistry, FixturePhase,
};

#[test]
fn my_test() {
    // Use a single fixture
    let mut fixture = counter_fixture();
    let ctx = fixture.setup().unwrap();
    assert_eq!(ctx.phase, FixturePhase::Active);
    let admin = ctx.account("admin").unwrap();
    // ... test logic ...
    fixture.teardown().unwrap();
}

#[test]
fn multi_contract_test() {
    // Use a registry for multiple fixtures
    let mut registry = FixtureRegistry::new();
    registry.register(token_fixture());
    registry.register(auth_fixture());
    registry.setup_all().unwrap();
    // ... test logic across contracts ...
    registry.teardown_all().unwrap();
}
```

### Deterministic Fixtures for Snapshot Testing

Use `deterministic_fixture(seed)` for tests that need stable outputs:

```rust
use starforge::utils::contract_fixtures::{
    deterministic_fixture, save_fixture_snapshot, load_fixture_snapshot,
};

#[test]
fn snapshot_stability() {
    let mut f = deterministic_fixture(42);
    let ctx = f.setup().unwrap().clone();
    let path = std::path::Path::new("tests/fixtures/snapshots/auth_42.json");
    save_fixture_snapshot(&ctx, &path).unwrap();

    // Later, verify the snapshot hasn't changed
    let loaded = load_fixture_snapshot(&path).unwrap();
    assert_eq!(loaded.name, ctx.name);
}
```

### Auth Fixture for RBAC Testing

The `auth_fixture()` models a contract with role-based access control:

```rust
use starforge::utils::contract_fixtures::auth_fixture;

#[test]
fn admin_can_grant_roles() {
    let mut fixture = auth_fixture();
    let ctx = fixture.setup().unwrap();

    let admin = ctx.account("admin").unwrap();
    let operator = ctx.account("operator").unwrap();
    let unauthorized = ctx.account("unauthorized").unwrap();

    // admin has full access
    let admin_actions = ctx.value("admin_actions").unwrap();
    assert!(admin_actions.as_array().unwrap().contains(&serde_json::json!("grant_role")));

    // operator has limited access
    let authorized_actions = ctx.value("authorized_actions").unwrap();
    assert!(authorized_actions.as_array().unwrap().contains(&serde_json::json!("transfer")));

    // unauthorized has no access
    assert_eq!(unauthorized.secret_key, None);
    assert_eq!(unauthorized.balance, 0);
}
```

### Building Custom Fixtures

Use `FixtureBuilder` to create custom fixtures:

```rust
use starforge::utils::contract_fixtures::{
    FixtureBuilder, TestAccount, AccountRole,
    StorageSeed, StorageDurability,
};

let fixture = FixtureBuilder::new("my_custom")
    .with_account(TestAccount {
        id: "owner".into(),
        address: "G...".into(),
        secret_key: Some("S...".into()),
        balance: 1_000_000_000,
        role: AccountRole::Admin,
    })
    .with_storage(StorageSeed {
        key: "balance".into(),
        value: serde_json::json!(0u64),
        durability: StorageDurability::Persistent,
    })
    .with_value("token_name", serde_json::json!("MyToken"))
    .with_metadata("version", "1.0.0")
    .build();
```

### File Fixtures

| File | Purpose |
|------|---------|
| `minimal.wasm` | Structurally minimal WASM module (header only) for hash tests |
| `ai_docs_counter.rs` | Counter contract source for AI documentation generation tests |
| `json_contracts/` | JSON contract fixtures for CLI contract stability tests |
| `snapshots/` | Snapshot files for deterministic fixture comparison |
| `soroban_rpc/` | Mock Soroban RPC responses for offline testing |

### minimal.wasm

A **structurally minimal** WebAssembly module consisting of only the WASM
magic number and version field — the smallest valid (parseable) WASM header:

| Offset | Bytes                       | Description            |
|--------|-----------------------------|------------------------|
| 0–3    | `00 61 73 6d`               | WASM magic `\0asm`     |
| 4–7    | `01 00 00 00`               | WASM version 1         |

### How it was generated

```python
data = bytes([0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00])
with open("tests/fixtures/minimal.wasm", "wb") as f:
    f.write(data)
```

### Known SHA-256 digest

```
93a44bbb96c751218e4c00d479e4c14358122a389acca16205b1e4d0dc5f9476
```

Verified with:

```sh
# PowerShell
Get-FileHash tests\fixtures\minimal.wasm -Algorithm SHA256

# Unix
sha256sum tests/fixtures/minimal.wasm

# Python
python -c "import hashlib; print(hashlib.sha256(open('tests/fixtures/minimal.wasm','rb').read()).hexdigest())"
```

### Relationship to Soroban / `stellar contract`

The hash produced by `starforge deploy` for a given `.wasm` file is the
**raw SHA-256 of the file bytes**, which is the same value that
`stellar contract inspect --wasm <file>` reports as the contract hash before
upload. After upload, the Soroban ledger stores contracts keyed by this same
digest.

> **Note**: `stellar contract deploy` derives the on-chain contract ID from
> the deployer's address and a salt, not from the WASM hash directly. The
> WASM hash is used to deduplicate uploaded code — uploading the same bytes
> twice is a no-op on Soroban.

### Used by smoke and unit tests

`minimal.wasm` supports deploy hash unit tests in `src/commands/deploy.rs`.
Broader CLI smoke coverage lives in `tests/cli_smoke.rs` and `scripts/e2e-smoke.sh`.
