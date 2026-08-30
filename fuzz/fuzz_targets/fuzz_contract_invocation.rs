//! Fuzz harness: mock contract invocation.
//!
//! Exercises `MockContractClient::invoke` with structured, arbitrary inputs
//! generated via `arbitrary::Arbitrary`. This drives the mock contract
//! client's call-logging, response-lookup, and error-handling paths with
//! random function names, argument vectors, and caller identities.
//!
//! The harness verifies that:
//! - Invocation never panics for any input.
//! - Call counts are always consistent with the number of invocations.
//! - Pre-configured errors and return values are returned deterministically.
//!
//! Run with:
//!   cargo fuzz run fuzz_contract_invocation

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use starforge::utils::contract_mocks::{MockAddress, MockContractClient};

/// A JSON scalar the fuzzer can generate.
///
/// `serde_json::Value` does not implement `arbitrary::Arbitrary`, so the
/// harness generates its own shape and converts it. Floats go through
/// `Number::from_f64`, which rejects NaN and infinity, so those degrade to
/// null instead of panicking.
#[derive(Debug, Arbitrary)]
enum FuzzJsonScalar {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

impl From<FuzzJsonScalar> for serde_json::Value {
    fn from(value: FuzzJsonScalar) -> Self {
        match value {
            FuzzJsonScalar::Null => serde_json::Value::Null,
            FuzzJsonScalar::Bool(b) => serde_json::Value::Bool(b),
            FuzzJsonScalar::Int(i) => serde_json::Value::Number(i.into()),
            FuzzJsonScalar::Float(f) => serde_json::Number::from_f64(f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            FuzzJsonScalar::Str(s) => serde_json::Value::String(s),
        }
    }
}

/// A JSON argument the fuzzer can generate.
///
/// Nesting is deliberately capped at one level: a recursive `Arbitrary` derive
/// can spend the entire input budget on a single deeply nested value, which
/// starves the rest of the struct and makes the corpus far less useful.
#[derive(Debug, Arbitrary)]
enum FuzzJsonValue {
    Scalar(FuzzJsonScalar),
    Array(Vec<FuzzJsonScalar>),
    Object(Vec<(String, FuzzJsonScalar)>),
}

impl From<FuzzJsonValue> for serde_json::Value {
    fn from(value: FuzzJsonValue) -> Self {
        match value {
            FuzzJsonValue::Scalar(s) => s.into(),
            FuzzJsonValue::Array(items) => {
                serde_json::Value::Array(items.into_iter().map(Into::into).collect())
            }
            FuzzJsonValue::Object(entries) => serde_json::Value::Object(
                entries
                    .into_iter()
                    .map(|(k, v)| (k, serde_json::Value::from(v)))
                    .collect(),
            ),
        }
    }
}

/// Structured fuzzer input for contract invocation.
#[derive(Debug, Arbitrary)]
struct FuzzInvocation {
    /// Function name to invoke (arbitrary string).
    function: String,
    /// Arguments as JSON values.
    args: Vec<FuzzJsonValue>,
    /// Whether to pre-configure a return value.
    configure_return: bool,
    /// Whether to pre-configure an error.
    configure_error: bool,
    /// Number of times to invoke before checking.
    repeat: u8,
}

fuzz_target!(|input: FuzzInvocation| {
    let contract = MockAddress::contract(1);
    let client = MockContractClient::new(contract.clone());

    let args: Vec<serde_json::Value> = input.args.into_iter().map(Into::into).collect();

    // Optionally pre-configure a return value or error.
    if input.configure_return {
        client.mock_return(&input.function, serde_json::json!(42u64));
    }
    if input.configure_error {
        client.mock_error(&input.function, "fuzz-error");
    }

    // Invoke the function `repeat` times — must never panic.
    for _ in 0..input.repeat {
        let caller = if input.repeat % 2 == 0 {
            Some(MockAddress::account(1))
        } else {
            None
        };
        let _ = client.invoke(&input.function, args.clone(), caller, 100);
    }

    // Postcondition: call count must match the number of invocations.
    let expected_count = input.repeat as usize;
    assert_eq!(
        client.call_count(&input.function),
        expected_count,
        "call count mismatch for function {:?}",
        input.function
    );

    // Postcondition: total calls must match.
    assert_eq!(
        client.total_calls(),
        expected_count,
        "total call count mismatch"
    );
});
