# Contract Fuzzing & Property-Based Testing Guide

StarForge ships a built-in fuzzing engine plus `proptest` and `cargo-fuzz`
integration for discovering edge cases and vulnerabilities in Soroban
contracts.

There are two complementary layers:

1. **In-process engine** (`starforge fuzz ...`) — runs on stable Rust, no extra
   tooling. Great for fast feedback and CI.
2. **Guided fuzzing** (`cargo-fuzz` / libFuzzer) — coverage-guided, runs on
   nightly. Lives in [`fuzz/`](fuzz/README.md).

---

## 1. Property-based testing

Express an invariant; let the engine search for a counterexample and shrink it
to a minimal reproducer.

```bash
# Run the built-in token-model suite (supply conservation, no underflow):
starforge fuzz property --iterations 5000 --seed 42
```

Output on success:

```
Property-Based Testing
  Suite                token transfer model (supply conservation)
  Iterations           5000
  Cases run            5000
✓ All properties held across every generated input
```

When a property fails, the **minimal counterexample** and shrink-step count are
printed so you can reproduce deterministically with the same `--seed`.

### Writing your own properties

`proptest`-based examples live in
[`tests/contract_property_tests.rs`](tests/contract_property_tests.rs):

```rust
use proptest::prelude::*;
use starforge::utils::fuzzing::TokenModel;

proptest! {
    #[test]
    fn transfer_conserves_supply(from in 0u64..1_000_000, to in 0u64..1_000_000, amount in 0u64..2_000_000) {
        let model = TokenModel { from_balance: from, to_balance: to };
        let before = model.total();
        if let Ok(after) = model.transfer(amount) {
            prop_assert_eq!(after.total(), before);
        }
    }
}
```

The engine's own API (`run_property`, `ArgSpec`, `FuzzType`, `FuzzValue`) is
public and can drive properties over typed `ScVal`-style inputs with shrinking.

---

## 2. WASM validator fuzzing

Byte-mutate a real contract binary and confirm the validator rejects malformed
input gracefully (never panics):

```bash
starforge fuzz wasm --wasm target/contract.wasm --iterations 5000 --seed 7
```

```
  Rejected (graceful)  4983
  Accepted             17
  Crashes (panics)     0
✓ Validator handled every mutated input gracefully
```

A non-zero crash count exits non-zero and prints crash-input prefixes for
triage.

---

## 3. Mutation testing

Generate source mutants (flipped operators, dropped `require_auth`, …) and
report which would survive a well-written test suite — surviving mutants point
at untested logic.

```bash
starforge fuzz mutate --source src/lib.rs
starforge fuzz mutate --source src/lib.rs --json   # machine-readable
```

```
Mutation Testing
  Mutants generated    24
  Mutation score       79.2%
  Killed               19
  Survived             5
⚠ Surviving mutants indicate untested logic:
  L48 [boundary] replace `<` with `<=`
  ...
```

---

## 4. Coverage reporting

The fuzzing engine surfaces coverage signals through:

- **Property pass/fail ratios** and shrink depth (`starforge fuzz property`).
- **Accept/reject/crash counts** for validator fuzzing (`starforge fuzz wasm`).
- **Mutation score** as a proxy for test-suite coverage (`starforge fuzz mutate`).

For line/branch coverage of the fuzz harnesses themselves, use the standard
toolchain, e.g. `cargo +nightly fuzz coverage fuzz_wasm_validator` or
`cargo llvm-cov`.

---

## 5. Corpus management

Interesting inputs are persisted under `~/.starforge/fuzz/`:

```bash
starforge fuzz corpus --list
starforge fuzz corpus --show wasm-crashes
```

---

## 6. Guided fuzzing (cargo-fuzz)

See [`fuzz/README.md`](fuzz/README.md). Quick start:

```bash
cargo install cargo-fuzz
cargo +nightly fuzz run fuzz_wasm_validator -- -max_total_time=60
cargo +nightly fuzz run fuzz_state_diff     -- -max_total_time=60
```

---

## 7. CI pipeline

[`.github/workflows/fuzz.yml`](.github/workflows/fuzz.yml) runs:

- **`property`** (blocking) — property-based + engine tests on stable.
- **`cargo-fuzz`** (non-blocking, nightly + scheduled) — time-boxed guided
  fuzzing of each target.

This keeps fast, deterministic checks gating PRs while running deeper guided
fuzzing on a schedule.
