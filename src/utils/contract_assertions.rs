use crate::utils::contract_mocks::{MockAddress, MockEnvironment, MockEventLog, StorageKey};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The outcome of a single assertion check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssertionStatus {
    Passed,
    Failed,
}

/// Rich result produced by each assertion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssertionResult {
    pub name: String,
    pub status: AssertionStatus,
    pub message: String,
    pub expected: Option<String>,
    pub actual: Option<String>,
    pub hint: Option<String>,
}

impl AssertionResult {
    pub fn pass(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: AssertionStatus::Passed,
            message: message.into(),
            expected: None,
            actual: None,
            hint: None,
        }
    }

    pub fn fail(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: AssertionStatus::Failed,
            message: message.into(),
            expected: None,
            actual: None,
            hint: None,
        }
    }

    pub fn with_expected(mut self, expected: impl Into<String>) -> Self {
        self.expected = Some(expected.into());
        self
    }

    pub fn with_actual(mut self, actual: impl Into<String>) -> Self {
        self.actual = Some(actual.into());
        self
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn is_passed(&self) -> bool {
        self.status == AssertionStatus::Passed
    }
}

impl fmt::Display for AssertionResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let icon = if self.is_passed() { "✓" } else { "✗" };
        write!(f, "{} [{}] {}", icon, self.name, self.message)?;
        if let (Some(exp), Some(act)) = (&self.expected, &self.actual) {
            write!(f, "\n    expected: {}\n    actual:   {}", exp, act)?;
        }
        if let Some(hint) = &self.hint {
            write!(f, "\n    hint: {}", hint)?;
        }
        Ok(())
    }
}

/// Collects multiple assertion results and provides summary reporting.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AssertionSuite {
    pub results: Vec<AssertionResult>,
}

impl AssertionSuite {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, result: AssertionResult) {
        self.results.push(result);
    }

    pub fn passed(&self) -> usize {
        self.results.iter().filter(|r| r.is_passed()).count()
    }

    pub fn failed(&self) -> usize {
        self.results.iter().filter(|r| !r.is_passed()).count()
    }

    pub fn total(&self) -> usize {
        self.results.len()
    }

    pub fn all_passed(&self) -> bool {
        self.results.iter().all(|r| r.is_passed())
    }

    pub fn failures(&self) -> Vec<&AssertionResult> {
        self.results.iter().filter(|r| !r.is_passed()).collect()
    }

    pub fn merge(&mut self, other: AssertionSuite) {
        self.results.extend(other.results);
    }
}

impl fmt::Display for AssertionSuite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Assertions: {}/{} passed", self.passed(), self.total())?;
        for result in &self.results {
            writeln!(f, "  {}", result)?;
        }
        Ok(())
    }
}

// ── Storage assertions ─────────────────────────────────────────────────────

/// Assert that a specific storage key equals an expected value.
pub fn assert_storage_eq(
    env: &MockEnvironment,
    key: &StorageKey,
    expected: &serde_json::Value,
) -> AssertionResult {
    let name = format!("storage[{}/{}] == {}", key.scope, key.key, expected);
    match env.storage.get(key) {
        Some(actual) if actual == expected => AssertionResult::pass(&name, "storage value matches"),
        Some(actual) => AssertionResult::fail(&name, "storage value mismatch")
            .with_expected(expected.to_string())
            .with_actual(actual.to_string())
            .with_hint("Check the contract's storage write logic"),
        None => AssertionResult::fail(
            &name,
            format!("key '{}/{}' not found in storage", key.scope, key.key),
        )
        .with_expected(expected.to_string())
        .with_hint("Ensure the contract initialises this storage key"),
    }
}

/// Assert that a storage key is absent.
pub fn assert_storage_absent(env: &MockEnvironment, key: &StorageKey) -> AssertionResult {
    let name = format!("storage[{}/{}] is absent", key.scope, key.key);
    if !env.storage.has(key) {
        AssertionResult::pass(&name, "storage key is absent as expected")
    } else {
        AssertionResult::fail(&name, "storage key unexpectedly present")
            .with_hint("Ensure the contract deletes this key when appropriate")
    }
}

/// Assert that a storage key is present (any value).
pub fn assert_storage_present(env: &MockEnvironment, key: &StorageKey) -> AssertionResult {
    let name = format!("storage[{}/{}] is present", key.scope, key.key);
    if env.storage.has(key) {
        AssertionResult::pass(&name, "storage key exists")
    } else {
        AssertionResult::fail(&name, "storage key is missing")
            .with_hint("Ensure the contract writes to this key during initialisation")
    }
}

/// Assert that a storage value satisfies a numeric comparison (as i128).
pub fn assert_storage_numeric(
    env: &MockEnvironment,
    key: &StorageKey,
    comparator: NumericComparator,
    expected: i128,
) -> AssertionResult {
    let name = format!(
        "storage[{}/{}] {} {}",
        key.scope, key.key, comparator, expected
    );
    let actual_val = match env.storage.get(key) {
        Some(v) => v,
        None => {
            return AssertionResult::fail(
                &name,
                format!("key '{}/{}' not found", key.scope, key.key),
            )
        }
    };

    let actual = match actual_val
        .as_i64()
        .map(i128::from)
        .or_else(|| actual_val.as_u64().map(i128::from))
    {
        Some(n) => n,
        None => {
            return AssertionResult::fail(&name, "storage value is not numeric")
                .with_actual(actual_val.to_string())
        }
    };

    let passes = match comparator {
        NumericComparator::Eq => actual == expected,
        NumericComparator::Ne => actual != expected,
        NumericComparator::Gt => actual > expected,
        NumericComparator::Gte => actual >= expected,
        NumericComparator::Lt => actual < expected,
        NumericComparator::Lte => actual <= expected,
    };

    if passes {
        AssertionResult::pass(&name, "numeric assertion passed")
    } else {
        AssertionResult::fail(&name, "numeric assertion failed")
            .with_expected(format!("{} {}", comparator, expected))
            .with_actual(actual.to_string())
    }
}

/// Comparator for numeric storage assertions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NumericComparator {
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
}

impl fmt::Display for NumericComparator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NumericComparator::Eq => write!(f, "=="),
            NumericComparator::Ne => write!(f, "!="),
            NumericComparator::Gt => write!(f, ">"),
            NumericComparator::Gte => write!(f, ">="),
            NumericComparator::Lt => write!(f, "<"),
            NumericComparator::Lte => write!(f, "<="),
        }
    }
}

// ── Balance assertions ─────────────────────────────────────────────────────

/// Assert that a token balance equals the expected amount.
pub fn assert_balance_eq(
    env: &MockEnvironment,
    token: &str,
    address: &str,
    expected: i128,
) -> AssertionResult {
    let actual = env.balances.get(token, address);
    let name = format!("balance({}, {}) == {}", token, address, expected);
    if actual == expected {
        AssertionResult::pass(&name, "balance matches")
    } else {
        AssertionResult::fail(&name, "balance mismatch")
            .with_expected(expected.to_string())
            .with_actual(actual.to_string())
            .with_hint("Check mint/burn/transfer logic")
    }
}

/// Assert that a token balance is at least the given amount.
pub fn assert_balance_gte(
    env: &MockEnvironment,
    token: &str,
    address: &str,
    min: i128,
) -> AssertionResult {
    let actual = env.balances.get(token, address);
    let name = format!("balance({}, {}) >= {}", token, address, min);
    if actual >= min {
        AssertionResult::pass(&name, "balance is sufficient")
    } else {
        AssertionResult::fail(&name, "balance is too low")
            .with_expected(format!(">= {}", min))
            .with_actual(actual.to_string())
    }
}

// ── Event assertions ───────────────────────────────────────────────────────

/// Assert that at least one event with the given topic was emitted.
pub fn assert_event_emitted(log: &MockEventLog, topic: &str) -> AssertionResult {
    let name = format!("event '{}' emitted", topic);
    let matches = log.by_topic(topic);
    if !matches.is_empty() {
        AssertionResult::pass(&name, format!("{} event(s) found", matches.len()))
    } else {
        AssertionResult::fail(&name, "no matching event found")
            .with_hint("Ensure the contract emits this event in the expected code path")
    }
}

/// Assert that no event with the given topic was emitted.
pub fn assert_event_not_emitted(log: &MockEventLog, topic: &str) -> AssertionResult {
    let name = format!("event '{}' not emitted", topic);
    let matches = log.by_topic(topic);
    if matches.is_empty() {
        AssertionResult::pass(&name, "event correctly absent")
    } else {
        AssertionResult::fail(&name, format!("unexpected '{}' event found", topic))
            .with_hint("The contract should not emit this event in this scenario")
    }
}

/// Assert that exactly `n` events with the given topic were emitted.
pub fn assert_event_count(log: &MockEventLog, topic: &str, n: usize) -> AssertionResult {
    let actual = log.by_topic(topic).len();
    let name = format!("event '{}' count == {}", topic, n);
    if actual == n {
        AssertionResult::pass(&name, format!("{} event(s) as expected", n))
    } else {
        AssertionResult::fail(&name, "unexpected event count")
            .with_expected(n.to_string())
            .with_actual(actual.to_string())
    }
}

/// Assert that an event with the given topic has a specific data field.
pub fn assert_event_data(
    log: &MockEventLog,
    topic: &str,
    field: &str,
    expected: &serde_json::Value,
) -> AssertionResult {
    let name = format!("event['{}'].data.{} == {}", topic, field, expected);
    let events = log.by_topic(topic);
    if events.is_empty() {
        return AssertionResult::fail(&name, format!("no '{}' event found", topic));
    }
    for event in &events {
        if let Some(actual) = event.data.get(field) {
            if actual == expected {
                return AssertionResult::pass(&name, "event data matches");
            } else {
                return AssertionResult::fail(&name, "event data field mismatch")
                    .with_expected(expected.to_string())
                    .with_actual(actual.to_string());
            }
        }
    }
    AssertionResult::fail(&name, format!("field '{}' not found in event data", field))
}

// ── Auth assertions ────────────────────────────────────────────────────────

/// Assert that an address authorised a specific function call.
pub fn assert_auth_called(
    env: &MockEnvironment,
    address: &MockAddress,
    function: &str,
) -> AssertionResult {
    let name = format!("auth({}) called for '{}'", address, function);
    if env.auth.was_authorised(address, function) {
        AssertionResult::pass(&name, "authorization check passed")
    } else {
        AssertionResult::fail(&name, "authorization was not recorded")
            .with_hint("Ensure require_auth() is called for this function/address combination")
    }
}

/// Assert the total number of auth checks recorded.
pub fn assert_auth_count(env: &MockEnvironment, expected: usize) -> AssertionResult {
    let actual = env.auth.auth_count();
    let name = format!("auth_count == {}", expected);
    if actual == expected {
        AssertionResult::pass(&name, "correct number of auth checks")
    } else {
        AssertionResult::fail(&name, "unexpected auth check count")
            .with_expected(expected.to_string())
            .with_actual(actual.to_string())
    }
}

// ── Invocation result assertions ───────────────────────────────────────────

/// Assert that an invocation returned the expected value.
pub fn assert_return_value(
    result: &Result<serde_json::Value, String>,
    expected: &serde_json::Value,
) -> AssertionResult {
    let name = format!("return value == {}", expected);
    match result {
        Ok(actual) if actual == expected => AssertionResult::pass(&name, "return value matches"),
        Ok(actual) => AssertionResult::fail(&name, "return value mismatch")
            .with_expected(expected.to_string())
            .with_actual(actual.to_string()),
        Err(e) => AssertionResult::fail(&name, "invocation returned error")
            .with_actual(e.clone())
            .with_hint("Unexpected error — check contract error handling"),
    }
}

/// Assert that an invocation returned an error containing the given substring.
pub fn assert_error_contains(
    result: &Result<serde_json::Value, String>,
    substring: &str,
) -> AssertionResult {
    let name = format!("error contains '{}'", substring);
    match result {
        Err(e) if e.contains(substring) => AssertionResult::pass(&name, "error message matches"),
        Err(e) => AssertionResult::fail(&name, "error message does not match")
            .with_expected(format!("contains '{}'", substring))
            .with_actual(e.clone()),
        Ok(v) => AssertionResult::fail(&name, "expected error but got success")
            .with_actual(v.to_string())
            .with_hint("This call should have failed but returned a value"),
    }
}

/// Assert that an invocation succeeded (no error).
pub fn assert_ok(result: &Result<serde_json::Value, String>) -> AssertionResult {
    let name = "invocation succeeds";
    match result {
        Ok(_) => AssertionResult::pass(name, "call succeeded"),
        Err(e) => AssertionResult::fail(name, "call returned unexpected error")
            .with_actual(e.clone())
            .with_hint("Check that arguments and auth are correctly configured"),
    }
}

/// Assert that an invocation failed (returned any error).
pub fn assert_err(result: &Result<serde_json::Value, String>) -> AssertionResult {
    let name = "invocation fails";
    match result {
        Err(_) => AssertionResult::pass(name, "call failed as expected"),
        Ok(v) => AssertionResult::fail(name, "expected failure but call succeeded")
            .with_actual(v.to_string())
            .with_hint("Ensure the mock is configured to return an error for this call"),
    }
}

// ── Ledger assertions ──────────────────────────────────────────────────────

/// Assert the mock ledger is at or past a given sequence.
pub fn assert_ledger_gte(env: &MockEnvironment, min_sequence: u32) -> AssertionResult {
    let actual = env.ledger.sequence;
    let name = format!("ledger_sequence >= {}", min_sequence);
    if actual >= min_sequence {
        AssertionResult::pass(&name, format!("ledger at {}", actual))
    } else {
        AssertionResult::fail(&name, "ledger sequence too low")
            .with_expected(format!(">= {}", min_sequence))
            .with_actual(actual.to_string())
    }
}

// ── Fluent assertion builder ───────────────────────────────────────────────

/// Fluent builder that runs a sequence of assertions against a single environment.
pub struct ContractAssertions<'a> {
    env: &'a MockEnvironment,
    suite: AssertionSuite,
}

impl<'a> ContractAssertions<'a> {
    pub fn new(env: &'a MockEnvironment) -> Self {
        Self {
            env,
            suite: AssertionSuite::new(),
        }
    }

    pub fn storage_eq(mut self, key: StorageKey, expected: serde_json::Value) -> Self {
        self.suite
            .push(assert_storage_eq(self.env, &key, &expected));
        self
    }

    pub fn storage_absent(mut self, key: StorageKey) -> Self {
        self.suite.push(assert_storage_absent(self.env, &key));
        self
    }

    pub fn storage_present(mut self, key: StorageKey) -> Self {
        self.suite.push(assert_storage_present(self.env, &key));
        self
    }

    pub fn balance_eq(mut self, token: &str, address: &str, expected: i128) -> Self {
        self.suite
            .push(assert_balance_eq(self.env, token, address, expected));
        self
    }

    pub fn balance_gte(mut self, token: &str, address: &str, min: i128) -> Self {
        self.suite
            .push(assert_balance_gte(self.env, token, address, min));
        self
    }

    pub fn event_emitted(mut self, topic: &str) -> Self {
        self.suite
            .push(assert_event_emitted(&self.env.events, topic));
        self
    }

    pub fn event_not_emitted(mut self, topic: &str) -> Self {
        self.suite
            .push(assert_event_not_emitted(&self.env.events, topic));
        self
    }

    pub fn event_count(mut self, topic: &str, n: usize) -> Self {
        self.suite
            .push(assert_event_count(&self.env.events, topic, n));
        self
    }

    pub fn auth_called(mut self, address: &MockAddress, function: &str) -> Self {
        self.suite
            .push(assert_auth_called(self.env, address, function));
        self
    }

    pub fn auth_count(mut self, expected: usize) -> Self {
        self.suite.push(assert_auth_count(self.env, expected));
        self
    }

    pub fn ledger_gte(mut self, min_sequence: u32) -> Self {
        self.suite.push(assert_ledger_gte(self.env, min_sequence));
        self
    }

    pub fn finish(self) -> AssertionSuite {
        self.suite
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::contract_mocks::{counter_env, token_env, MockAddress};

    #[test]
    fn storage_eq_passes() {
        let env = counter_env();
        let result = assert_storage_eq(
            &env,
            &StorageKey::instance("count"),
            &serde_json::json!(0u64),
        );
        assert!(result.is_passed());
    }

    #[test]
    fn storage_eq_fails_on_mismatch() {
        let env = counter_env();
        let result = assert_storage_eq(
            &env,
            &StorageKey::instance("count"),
            &serde_json::json!(99u64),
        );
        assert!(!result.is_passed());
        assert!(result.expected.is_some());
        assert!(result.actual.is_some());
    }

    #[test]
    fn storage_absent_for_missing_key() {
        let env = counter_env();
        let result = assert_storage_absent(&env, &StorageKey::persistent("nonexistent"));
        assert!(result.is_passed());
    }

    #[test]
    fn balance_eq_assertion() {
        let env = token_env(500_000);
        let token = MockAddress::contract(10).0;
        let admin = MockAddress::account(10).0;
        let result = assert_balance_eq(&env, &token, &admin, 500_000);
        assert!(result.is_passed());
    }

    #[test]
    fn event_emitted_assertion() {
        let mut env = counter_env();
        env.emit_event(
            MockAddress::contract(1),
            vec![serde_json::json!("increment")],
            serde_json::json!({"new_count": 1}),
        );
        let result = assert_event_emitted(&env.events, "increment");
        assert!(result.is_passed());
    }

    #[test]
    fn event_not_emitted_assertion() {
        let env = counter_env();
        let result = assert_event_not_emitted(&env.events, "transfer");
        assert!(result.is_passed());
    }

    #[test]
    fn event_count_assertion() {
        let mut env = counter_env();
        for i in 0..3u32 {
            env.emit_event(
                MockAddress::contract(1),
                vec![serde_json::json!("increment")],
                serde_json::json!({"new_count": i + 1}),
            );
        }
        let result = assert_event_count(&env.events, "increment", 3);
        assert!(result.is_passed());
    }

    #[test]
    fn event_data_assertion() {
        let mut env = counter_env();
        env.emit_event(
            MockAddress::contract(1),
            vec![serde_json::json!("mint")],
            serde_json::json!({"amount": 1000}),
        );
        let result = assert_event_data(&env.events, "mint", "amount", &serde_json::json!(1000));
        assert!(result.is_passed());
    }

    #[test]
    fn auth_assertions() {
        let mut env = counter_env();
        let admin = MockAddress::account(1);
        let contract = MockAddress::contract(1);
        env.auth.auto_approve(admin.clone());
        env.auth.require_auth(&admin, &contract, "increment");

        let result = assert_auth_called(&env, &admin, "increment");
        assert!(result.is_passed());

        let count_result = assert_auth_count(&env, 1);
        assert!(count_result.is_passed());
    }

    #[test]
    fn return_value_assertion() {
        let result: Result<serde_json::Value, String> = Ok(serde_json::json!(42));
        let assertion = assert_return_value(&result, &serde_json::json!(42));
        assert!(assertion.is_passed());

        let mismatch = assert_return_value(&result, &serde_json::json!(0));
        assert!(!mismatch.is_passed());
    }

    #[test]
    fn error_contains_assertion() {
        let result: Result<serde_json::Value, String> = Err("unauthorized access".into());
        let assertion = assert_error_contains(&result, "unauthorized");
        assert!(assertion.is_passed());

        let miss = assert_error_contains(&result, "overflow");
        assert!(!miss.is_passed());
    }

    #[test]
    fn assertion_suite_all_passed() {
        let env = counter_env();
        let suite = ContractAssertions::new(&env)
            .storage_eq(StorageKey::instance("count"), serde_json::json!(0u64))
            .storage_present(StorageKey::instance("count"))
            .storage_absent(StorageKey::persistent("nonexistent"))
            .event_not_emitted("transfer")
            .finish();

        assert!(suite.all_passed());
        assert_eq!(suite.failed(), 0);
        assert_eq!(suite.total(), 4);
    }

    #[test]
    fn assertion_suite_collects_failures() {
        let env = counter_env();
        let suite = ContractAssertions::new(&env)
            .storage_eq(StorageKey::instance("count"), serde_json::json!(0u64))
            .storage_eq(StorageKey::instance("count"), serde_json::json!(999u64))
            .finish();

        assert!(!suite.all_passed());
        assert_eq!(suite.passed(), 1);
        assert_eq!(suite.failed(), 1);
    }

    #[test]
    fn numeric_storage_comparator() {
        let env = counter_env();
        let result = assert_storage_numeric(
            &env,
            &StorageKey::instance("count"),
            NumericComparator::Eq,
            0,
        );
        assert!(result.is_passed());

        let gt_result = assert_storage_numeric(
            &env,
            &StorageKey::instance("count"),
            NumericComparator::Lt,
            100,
        );
        assert!(gt_result.is_passed());
    }
}
