# Gas Optimization Guide for Soroban Contracts

This guide covers practical techniques for reducing gas usage in Soroban smart
contracts, along with how to use the StarForge gas tooling to measure and track
improvements.

---

## Quick Start

```bash norun
# Profile a compiled contract
starforge gas profile ./target/wasm32v1-none/release/my_contract.wasm

# Compare two builds
starforge gas compare baseline.wasm optimized.wasm

# View report history
starforge gas history

# Print this guide in-terminal
starforge gas guide
```

---

## Understanding the Soroban Fee Model

Soroban charges fees across two dimensions:

| Component | What drives it |
|---|---|
| **Upload fee** | WASM binary size (bytes) |
| **Execution fee** | CPU instructions executed per invocation |
| **Read/write fee** | Ledger entries read or written |
| **Events fee** | Events emitted per invocation |
| **Auth fee** | Number of `require_auth()` calls |

Reducing **binary size** lowers the one-time upload cost.
Reducing **instruction count** lowers every invocation cost.

---

## 1. Binary Size Reduction

The Soroban upload limit is **128 KB**. Every byte costs gas on upload.

### Measuring where the bytes go: `starforge optimize size`

Before optimizing, find out which sections of the module are actually large.
The `size` command breaks the binary down by WASM section category (code /
data / custom) and checks the result against configurable budgets
(see `starforge-size-budget.example.toml`):

```bash norun
# Soroban 128 KiB total check only
starforge optimize size --wasm contract.wasm

# Against a project budget file; fails (exit 1) when a budget is exceeded
starforge optimize size --wasm contract.wasm --budget starforge-size-budget.toml \
  --fail-on-overage

# Machine-readable breakdown for CI
starforge optimize size --wasm contract.wasm --budget starforge-size-budget.toml --json
```

A large **custom** section usually means debug/toolchain metadata (`--strip-debug`,
`strip = true`); large **data** points at embedded strings; large **code** is
function bodies best attacked with the Cargo profile below plus `optimize
transform`. Budget failures print the next suggested commands for the category
that actually failed.

### Cargo release profile

```toml
[profile.release]
opt-level   = "z"       # minimize size (vs "s" for balanced, "3" for speed)
lto         = true      # link-time optimization across crates
codegen-units = 1       # single codegen unit enables better LTO
strip       = true      # strip symbol tables (Rust 1.59+)
panic       = "abort"   # eliminates unwinding infrastructure
```

### Post-build optimization

```bash norun
# Install binaryen
cargo install wasm-opt        # or: brew install binaryen

# Apply maximum size optimizations
wasm-opt -Oz -o contract_opt.wasm contract.wasm

# Or use the Stellar CLI optimizer
stellar contract optimize --wasm contract.wasm --wasm-out contract_opt.wasm
```

### Dependency hygiene

```bash norun
# Audit the dependency tree
cargo tree --duplicates

# Disable default features
soroban-sdk = { version = "21", default-features = false }

# Remove unused features from your Cargo.toml
```

---

## 2. Panic & Error Handling

Panic strings are embedded in the WASM binary and bloat it significantly.

```rust
// ❌ Avoid: long panic messages inflate binary
let val = map.get(key).expect("Failed to retrieve value from contract storage map");

// ✅ Prefer: short symbolic error codes
use soroban_sdk::{contracterror, panic_with_error};

#[contracterror]
#[derive(Debug, Clone, Copy)]
pub enum ContractError {
    NotFound      = 1,
    Unauthorized  = 2,
    InvalidInput  = 3,
}

let val = map.get(key).ok_or(ContractError::NotFound)?;
```

---

## 3. Removing Debug Code

Debug logging is a no-op in Soroban but still bloats the binary.

```rust
// ❌ Avoid in contract code
println!("debug: value = {:?}", value);
log::debug!("Processing item {}", i);

// ✅ Gate debug code with cfg attributes
#[cfg(feature = "debug")]
log::debug!("Processing item {}", i);
```

Set in Cargo.toml:
```toml
[features]
debug = []
```

Build for production without the `debug` feature:
```bash norun
cargo build --release --target wasm32v1-none
```

---

## 4. Storage & State Optimization

Storage operations are among the most expensive in Soroban.

```rust
// ❌ Avoid: multiple redundant reads of the same key
let a = env.storage().instance().get(&DataKey::Config);
let b = env.storage().instance().get(&DataKey::Config); // duplicate read!

// ✅ Cache reads in local variables
let config: Config = env.storage().instance().get(&DataKey::Config)
    .unwrap_or_default();
// use `config` throughout the function

// ❌ Avoid: storing large structs when only a field changes
let mut config: BigConfig = storage.get(&key)?;
config.counter += 1;
storage.set(&key, &config); // entire struct re-serialized

// ✅ Prefer: separate small keys for frequently-updated fields
storage.set(&DataKey::Counter, &(counter + 1));
```

### Storage type selection

| Type | Use when | Cost |
|---|---|---|
| `Temporary` | Data that can expire (caches, nonces) | Cheapest |
| `Persistent` | Long-lived state (balances, config) | Medium |
| `Instance` | Contract metadata (admin, initialized) | Bundled with instance |

---

## 5. Computation & CPU Gas

```rust
// ❌ Avoid: nested storage reads in loops
for i in 0..n {
    let val: u64 = env.storage().persistent().get(&DataKey::Item(i))?;
    // ...
}

// ✅ Prefer: read all at once or batch with soroban_sdk::Vec
let items: soroban_sdk::Vec<u64> = env.storage()
    .persistent()
    .get(&DataKey::Items)?;
for item in items.iter() {
    // ...
}

// ✅ Use integer bit tricks instead of division
let is_even = (n & 1) == 0;        // instead of n % 2 == 0
let half    = n >> 1;               // instead of n / 2
let aligned = (n + 7) & !7;        // align to 8 without division
```

---

## 6. Contract Architecture

```rust
// ❌ Avoid: giant monolithic contracts
#[contract]
pub struct EverythingContract; // 80 KB of logic...

// ✅ Prefer: focused single-responsibility contracts
#[contract] pub struct TokenContract;   // token logic only
#[contract] pub struct VaultContract;   // vault logic, calls TokenContract
#[contract] pub struct GovernanceContract;  // governance, calls both

// ❌ Avoid: WASM start function for initialization
// (auto-run on every invocation, wastes gas)

// ✅ Prefer: explicit init function
#[contractimpl]
impl MyContract {
    pub fn initialize(env: Env, admin: Address) {
        // called once by deployer
    }
}
```

---

## 7. Using the StarForge Tooling

### Continuous profiling in CI

```bash
cargo build --release --target wasm32v1-none
starforge gas profile target/wasm32v1-none/release/my_contract.wasm --fail-on-critical
starforge gas compare artifacts/baseline.wasm target/wasm32v1-none/release/my_contract.wasm
```

### Reading the optimization score

| Score | Grade | Meaning |
|---|---|---|
| 80–100 | Excellent | Well-optimized, ready for mainnet |
| 50–79 | Good | Minor issues, acceptable for testnet |
| 20–49 | Fair | Multiple findings, optimize before mainnet |
| 0–19 | Poor | Critical issues, do not deploy |

### Finding IDs reference

| ID | Kind | Severity | Description |
|---|---|---|---|
| GAS-001 | binary-size | Critical/High | WASM at or near 128 KB limit |
| GAS-002 | binary-size-medium | Medium | WASM > 64 KB |
| GAS-003 | debug-info | High | Debug/name sections present |
| GAS-004 | panic-strings | Medium | Verbose panic strings detected |
| GAS-005 | debug-logging | Medium | `println`/`eprintln` calls detected |
| GAS-006 | excessive-imports | Medium | >20 imported host functions |
| GAS-007 | excessive-exports | Low | >30 exported functions |
| GAS-008 | multiple-memories | High | >1 linear memory (deployment blocker) |
| GAS-009 | start-function | Medium | WASM start function present |
| GAS-010 | excessive-globals | Low | >15 global variables |
| GAS-011 | data-segments | Low | >10 data segments |
| GAS-012 | high-instruction-density | Info | High CPU cost estimate |

---

## 8. Benchmarking Suite

Run the Criterion benchmarks to profile the gas analyzer itself:

```bash norun
# Full benchmark suite (includes gas analyzer benchmarks)
cargo bench

# Gas-specific benchmarks only
cargo bench gas

# Compare against a saved baseline
cargo bench -- --save-baseline before
# ... make changes ...
cargo bench -- --baseline before
```

The gas benchmarks cover:
- `gas_section_parsing` — WASM binary parser throughput at 8/32/64/128 KB
- `gas_finding_generation` — Optimization pattern scan at various sizes
- `gas_cost_computation` — Fee arithmetic breakdown
- `gas_version_comparison` — Two-pass diff analysis throughput

---

## Further Reading

- [Soroban Documentation — Fees & Metering](https://developers.stellar.org/docs/smart-contracts/resource-limits-fees)
- [Stellar Core — Fee Schedule](https://github.com/stellar/stellar-core/blob/master/src/main/Config.h)
- [Binaryen wasm-opt](https://github.com/WebAssembly/binaryen)
- [Rust WASM Book — Shrinking .wasm Size](https://rustwasm.github.io/docs/book/reference/code-size.html)
