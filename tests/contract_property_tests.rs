//! Property-based tests for Soroban contract testing infrastructure.
//!
//! These tests use `proptest` to automatically generate inputs and verify
//! invariants that must hold for all valid (and many invalid) inputs in the
//! contract testing, WASM validation, and mock execution paths.
//!
//! Run with:
//!   cargo test --test contract_property_tests
//!
//! Increase iterations for deeper coverage:
//!   PROPTEST_CASES=5000 cargo test --test contract_property_tests

#![allow(dead_code, unused_imports)]

use proptest::prelude::*;
use starforge::utils::contract_mocks::{
    MockAddress, MockContractClient, MockEnvironment, MockStorage, StorageKey,
};
use starforge::utils::mock_soroban::validate_wasm;
use starforge::utils::wasm_hash::{compute_wasm_hash, BuildEnvironment};

// ─────────────────────────────────────────────────────────────────────────────
// 1. WASM validation — property tests
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    /// Any byte sequence shorter than 8 bytes must be rejected.
    #[test]
    fn prop_short_wasm_rejected(data in prop::collection::vec(any::<u8>(), 0..8)) {
        let result = validate_wasm(&data);
        prop_assert!(result.is_err(), "short WASM should be rejected");
    }

    /// Any byte sequence with a valid 4-byte magic header but < 8 bytes is rejected.
    #[test]
    fn prop_magic_header_short_rejected(data in prop::collection::vec(any::<u8>(), 0..4)) {
        let mut input = b"\0asm".to_vec();
        input.extend(data);
        let result = validate_wasm(&input);
        prop_assert!(result.is_err(), "short WASM with magic header should be rejected");
    }

    /// Any byte sequence >= 8 bytes with a valid magic header is accepted.
    #[test]
    fn prop_valid_magic_header_accepted(data in prop::collection::vec(any::<u8>(), 4..1024)) {
        let mut input = b"\0asm".to_vec();
        input.extend(data);
        let result = validate_wasm(&input);
        prop_assert!(result.is_ok(), "WASM with valid header and >= 8 bytes should be accepted");
    }

    /// Any byte sequence without the magic header is rejected.
    #[test]
    fn prop_missing_magic_header_rejected(data in prop::collection::vec(any::<u8>(), 8..1024)) {
        prop_assume!(!data.starts_with(b"\0asm"));
        let result = validate_wasm(&data);
        prop_assert!(result.is_err(), "WASM without magic header should be rejected");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. WASM hash computation — property tests
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    /// WASM hash of empty bytes must be rejected.
    #[test]
    fn prop_empty_wasm_hash_rejected(_data: ()) {
        let result = compute_wasm_hash(&[], BuildEnvironment::Linux);
        prop_assert!(result.is_err(), "empty WASM should be rejected");
    }

    /// WASM hash of bytes without magic header must be rejected.
    #[test]
    fn prop_no_magic_wasm_hash_rejected(data in prop::collection::vec(any::<u8>(), 8..1024)) {
        prop_assume!(!data.starts_with(b"\0asm"));
        let result = compute_wasm_hash(&data, BuildEnvironment::Linux);
        prop_assert!(result.is_err(), "WASM without magic header should be rejected");
    }

    /// WASM hash of valid WASM is always a 64-char lowercase hex string.
    #[test]
    fn prop_valid_wasm_hash_format(data in prop::collection::vec(any::<u8>(), 4..1024)) {
        let mut input = b"\0asm".to_vec();
        input.extend(data);
        if let Ok(hash) = compute_wasm_hash(&input, BuildEnvironment::Linux) {
            prop_assert_eq!(hash.len(), 64);
            prop_assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
            prop_assert!(hash.chars().all(|c| !c.is_ascii_uppercase()));
        }
    }

    /// WASM hash is deterministic: same input always produces same output.
    #[test]
    fn prop_wasm_hash_deterministic(data in prop::collection::vec(any::<u8>(), 4..1024)) {
        let mut input = b"\0asm".to_vec();
        input.extend(data);
        if let Ok(h1) = compute_wasm_hash(&input, BuildEnvironment::Linux) {
            let h2 = compute_wasm_hash(&input, BuildEnvironment::Linux).unwrap();
            prop_assert_eq!(h1, h2);
        }
    }

    /// WASM hash on unsupported environment is rejected.
    #[test]
    fn prop_unsupported_env_rejected(data in prop::collection::vec(any::<u8>(), 8..1024)) {
        let mut input = b"\0asm".to_vec();
        input.extend(data);
        let result = compute_wasm_hash(&input, BuildEnvironment::Unsupported("bsd".into()));
        prop_assert!(result.is_err(), "unsupported environment should be rejected");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Mock contract invocation — property tests
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    /// Invoking a function N times always records exactly N calls.
    #[test]
    fn prop_call_count_matches_invocations(
        function in "[a-z_]{1,20}",
        repeat in 0u8..10,
    ) {
        let client = MockContractClient::new(MockAddress::contract(1));
        for _ in 0..repeat {
            let _ = client.invoke(&function, vec![], None, 100);
        }
        prop_assert_eq!(client.call_count(&function), repeat as usize);
        prop_assert_eq!(client.total_calls(), repeat as usize);
    }

    /// Pre-configured return values are returned deterministically.
    #[test]
    fn prop_mock_return_is_deterministic(
        function in "[a-z_]{1,20}",
        value in any::<i64>(),
    ) {
        let client = MockContractClient::new(MockAddress::contract(1));
        client.mock_return(&function, serde_json::json!(value));
        let result1 = client.invoke(&function, vec![], None, 100);
        let result2 = client.invoke(&function, vec![], None, 100);
        prop_assert!(result1.is_ok());
        prop_assert!(result2.is_ok());
        prop_assert_eq!(result1.unwrap(), result2.unwrap());
    }

    /// Pre-configured errors are returned deterministically.
    #[test]
    fn prop_mock_error_is_deterministic(
        function in "[a-z_]{1,20}",
        error_msg in "[a-z_]{1,30}",
    ) {
        let client = MockContractClient::new(MockAddress::contract(1));
        client.mock_error(&function, &error_msg);
        let result1 = client.invoke(&function, vec![], None, 100);
        let result2 = client.invoke(&function, vec![], None, 100);
        prop_assert!(result1.is_err());
        prop_assert!(result2.is_err());
        prop_assert_eq!(result1.unwrap_err(), result2.unwrap_err());
    }

    /// Error takes priority over return value when both are configured.
    #[test]
    fn prop_error_takes_priority_over_return(
        function in "[a-z_]{1,20}",
    ) {
        let client = MockContractClient::new(MockAddress::contract(1));
        client.mock_return(&function, serde_json::json!(42u64));
        client.mock_error(&function, "error");
        let result = client.invoke(&function, vec![], None, 100);
        prop_assert!(result.is_err(), "error should take priority over return value");
    }

    /// Reset clears all call history and configurations.
    #[test]
    fn prop_reset_clears_state(
        function in "[a-z_]{1,20}",
        repeat in 1u8..5,
    ) {
        let client = MockContractClient::new(MockAddress::contract(1));
        client.mock_return(&function, serde_json::json!(1u64));
        for _ in 0..repeat {
            let _ = client.invoke(&function, vec![], None, 100);
        }
        prop_assert_eq!(client.total_calls(), repeat as usize);
        client.reset();
        prop_assert_eq!(client.total_calls(), 0);
        prop_assert_eq!(client.call_count(&function), 0);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Mock storage — property tests
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    /// Setting and getting a value always returns the same value.
    #[test]
    fn prop_storage_set_get_roundtrip(
        key in "[a-z_]{1,20}",
        value in any::<i64>(),
    ) {
        let mut storage = MockStorage::new();
        let storage_key = StorageKey::instance(&key);
        storage.set(storage_key.clone(), serde_json::json!(value));
        let retrieved = storage.get(&storage_key);
        prop_assert!(retrieved.is_some());
        prop_assert_eq!(retrieved.unwrap(), &serde_json::json!(value));
    }

    /// Removing a key makes it absent.
    #[test]
    fn prop_storage_remove_makes_absent(
        key in "[a-z_]{1,20}",
        value in any::<i64>(),
    ) {
        let mut storage = MockStorage::new();
        let storage_key = StorageKey::persistent(&key);
        storage.set(storage_key.clone(), serde_json::json!(value));
        prop_assert!(storage.has(&storage_key));
        storage.remove(&storage_key);
        prop_assert!(!storage.has(&storage_key));
    }

    /// Storage length matches the number of unique keys set.
    #[test]
    fn prop_storage_len_matches_keys(
        keys in prop::collection::vec("[a-z]{1,10}", 0..20),
        value in any::<i64>(),
    ) {
        let mut storage = MockStorage::new();
        let unique_count = keys.iter().collect::<std::collections::HashSet<_>>().len();
        for key in &keys {
            storage.set(StorageKey::instance(key), serde_json::json!(value));
        }
        prop_assert_eq!(storage.len(), unique_count);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. Mock address — property tests
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    /// Account addresses always start with 'G' and are 57 chars (GA + 55 digits).
    #[test]
    fn prop_account_address_format(id in any::<u32>()) {
        let addr = MockAddress::account(id);
        let s = addr.as_str();
        prop_assert!(s.starts_with('G'));
        prop_assert_eq!(s.len(), 57);
    }

    /// Contract addresses always start with 'C' and are 63 chars (C + 62 hex).
    #[test]
    fn prop_contract_address_format(id in any::<u32>()) {
        let addr = MockAddress::contract(id);
        let s = addr.as_str();
        prop_assert!(s.starts_with('C'));
        prop_assert_eq!(s.len(), 63);
    }

    /// Display trait matches as_str.
    #[test]
    fn prop_display_matches_as_str(id in any::<u32>()) {
        let addr = MockAddress::account(id);
        prop_assert_eq!(format!("{}", addr), addr.as_str());
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. Mock environment — property tests
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    /// Reset clears storage, events, and auth but preserves ledger config.
    #[test]
    fn prop_env_reset_preserves_ledger(
        initial_seq in 1u32..1000,
    ) {
        let mut env = MockEnvironment::new();
        env.ledger = env.ledger.clone().at_sequence(initial_seq);
        env.storage.set(StorageKey::instance("test"), serde_json::json!(1u64));
        env.emit_event(
            MockAddress::contract(1),
            vec![serde_json::json!("test")],
            serde_json::json!({"data": 1}),
        );
        let account = MockAddress::account(1);
        env.auth.auto_approve(account.clone());
        env.auth.require_auth(&account, &MockAddress::contract(1), "test_fn");
        env.auth.auto_approve(MockAddress::account(1));
        env.auth.require_auth(&MockAddress::account(1), &MockAddress::contract(1), "test");

        prop_assert!(!env.storage.is_empty());
        prop_assert!(!env.events.is_empty());
        prop_assert!(env.auth.auth_count() > 0);

        env.reset();

        prop_assert!(env.storage.is_empty());
        prop_assert!(env.events.is_empty());
        prop_assert_eq!(env.auth.auth_count(), 0);
        prop_assert_eq!(env.ledger.sequence, initial_seq);
    }

    /// advance_ledger increments sequence and timestamp.
    #[test]
    fn prop_ledger_advance_increments(
        initial_seq in 1u32..1000,
        ledgers in 1u32..100,
    ) {
        let mut env = MockEnvironment::new();
        env.ledger = env.ledger.clone().at_sequence(initial_seq);
        let initial_ts = env.ledger.timestamp;
        env.advance_ledger(ledgers);
        prop_assert_eq!(env.ledger.sequence, initial_seq + ledgers);
        prop_assert_eq!(env.ledger.timestamp, initial_ts + u64::from(ledgers) * 5);
    }
}
