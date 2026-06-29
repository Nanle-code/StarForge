//! Property-based tests for contract logic (D-11), powered by `proptest`.
//!
//! These demonstrate how to express invariants that a correct Soroban contract
//! must uphold and have `proptest` search the input space (with shrinking) for
//! counterexamples. They also exercise StarForge's own fuzzing engine.

use proptest::prelude::*;
use starforge::utils::fuzzing::{self, FuzzConfig, TokenModel};

proptest! {
    /// A transfer never creates or destroys tokens: total supply is conserved.
    #[test]
    fn token_transfer_conserves_supply(from in 0u64..1_000_000, to in 0u64..1_000_000, amount in 0u64..2_000_000) {
        let model = TokenModel { from_balance: from, to_balance: to };
        let before = model.total();
        if let Ok(after) = model.transfer(amount) {
            prop_assert_eq!(after.total(), before);
        }
    }

    /// A successful transfer of `amount` reduces the sender by exactly `amount`.
    #[test]
    fn successful_transfer_debits_sender(from in 0u64..1_000_000, to in 0u64..1_000_000, amount in 0u64..1_000_000) {
        let model = TokenModel { from_balance: from, to_balance: to };
        if let Ok(after) = model.transfer(amount) {
            prop_assert_eq!(after.from_balance, from - amount);
            prop_assert_eq!(after.to_balance, to + amount);
        }
    }

    /// Transfers larger than the balance are always rejected (no underflow).
    #[test]
    fn overspend_is_rejected(from in 0u64..1_000, to in 0u64..1_000) {
        let model = TokenModel { from_balance: from, to_balance: to };
        prop_assert!(model.transfer(from + 1).is_err());
    }

    /// Generated symbols are always within Soroban's identifier constraints.
    #[test]
    fn generated_symbols_are_well_formed(seed in any::<u64>()) {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        if let fuzzing::FuzzValue::Symbol(s) = fuzzing::generate_value(fuzzing::FuzzType::Symbol, &mut rng) {
            prop_assert!(!s.is_empty());
            prop_assert!(s.len() <= 10);
            prop_assert!(s.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
        }
    }
}

/// The built-in property suite must pass deterministically across seeds.
#[test]
fn builtin_token_suite_passes_for_many_seeds() {
    for seed in [1u64, 42, 1000, 9999, u64::MAX] {
        let report = fuzzing::run_token_property_suite(&FuzzConfig {
            iterations: 250,
            seed,
            ..FuzzConfig::default()
        });
        assert!(
            report.passed,
            "seed {seed} failed: {:?}",
            report.failure_message
        );
    }
}

/// The fuzzing engine must surface and shrink a planted bug.
#[test]
fn engine_shrinks_planted_bug() {
    let specs = vec![fuzzing::ArgSpec {
        name: "x".into(),
        ty: fuzzing::FuzzType::U32,
    }];
    let report = fuzzing::run_property(&specs, &FuzzConfig::default(), |input| {
        if let fuzzing::FuzzValue::U32(v) = input[0] {
            // Planted "bug": values above 500 violate the invariant.
            if v > 500 {
                return Err(format!("{v} exceeds limit"));
            }
        }
        Ok(())
    });
    assert!(!report.passed);
    // Shrinking should not increase the counterexample magnitude.
    let original = match report.counterexample.unwrap()[0] {
        fuzzing::FuzzValue::U32(v) => v,
        _ => unreachable!(),
    };
    let shrunk = match report.shrunk_counterexample.unwrap()[0] {
        fuzzing::FuzzValue::U32(v) => v,
        _ => unreachable!(),
    };
    assert!(shrunk <= original);
    assert!(shrunk > 500);
}
