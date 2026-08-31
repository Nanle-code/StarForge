//! # Failure Injection
//!
//! Provides configurable failure modes that can be injected into the
//! simulator to test how contracts and clients handle errors.

use serde::{Deserialize, Serialize};

// ── Failure Modes ─────────────────────────────────────────────────────────────

/// All possible failure modes the simulator can inject.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FailureMode {
    /// Simulate an RPC timeout (no response).
    RpcTimeout,
    /// Simulate an RPC connection refused error.
    RpcConnectionRefused,
    /// Return a JSON-RPC error with the given code.
    RpcError { code: i64 },
    /// Reject a transaction with insufficient fee.
    InsufficientFee,
    /// Reject a transaction due to invalid authorization.
    BadAuth,
    /// Reject a contract invocation with a panic.
    ContractPanic,
    /// Reject a contract invocation with a custom error.
    ContractError { code: u32, message: String },
    /// Simulate that an account does not exist.
    AccountNotFound,
    /// Simulate that a contract does not exist.
    ContractNotFound,
    /// Simulate insufficient account balance.
    InsufficientBalance,
    /// Simulate a ledger sequence number mismatch.
    LedgerSequenceMismatch,
    /// Simulate resource exhaustion (CPU/ memory budget exceeded).
    BudgetExceeded,
    /// Inject random transaction failures at the given probability (0.0..1.0).
    RandomFailure(f64),
}

/// A rule that activates a failure mode when certain conditions are met.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureRule {
    /// Human-readable name for this rule.
    pub name: String,
    /// The failure mode to inject.
    pub mode: FailureMode,
    /// Optional: Only activate for this RPC method name
    /// (e.g. "simulateTransaction", "sendTransaction", "getHealth").
    pub rpc_method_filter: Option<String>,
    /// Optional: Only activate for this contract ID.
    pub contract_id_filter: Option<String>,
    /// Optional: Only activate for this account public key.
    pub account_filter: Option<String>,
    /// Probability that this rule fires (0.0..1.0). 1.0 = always.
    pub probability: f64,
    /// Number of times this rule has fired so far.
    pub times_fired: u64,
    /// Maximum number of activations before the rule is removed (0 = unlimited).
    pub max_activations: u64,
}

impl FailureRule {
    pub fn new(name: &str, mode: FailureMode) -> Self {
        Self {
            name: name.to_string(),
            mode,
            rpc_method_filter: None,
            contract_id_filter: None,
            account_filter: None,
            probability: 1.0,
            times_fired: 0,
            max_activations: 0,
        }
    }

    pub fn with_rpc_method(mut self, method: &str) -> Self {
        self.rpc_method_filter = Some(method.to_string());
        self
    }

    pub fn with_contract(mut self, contract_id: &str) -> Self {
        self.contract_id_filter = Some(contract_id.to_string());
        self
    }

    pub fn with_account(mut self, account: &str) -> Self {
        self.account_filter = Some(account.to_string());
        self
    }

    pub fn with_probability(mut self, prob: f64) -> Self {
        self.probability = prob.clamp(0.0, 1.0);
        self
    }

    pub fn with_max_activations(mut self, max: u64) -> Self {
        self.max_activations = max;
        self
    }

    /// Check if this rule should fire given the current context.
    pub fn should_fire(
        &mut self,
        rpc_method: &str,
        contract_id: Option<&str>,
        account: Option<&str>,
        rng_probability: f64,
    ) -> bool {
        // Check max activations.
        if self.max_activations > 0 && self.times_fired >= self.max_activations {
            return false;
        }

        // Check RPC method filter.
        if let Some(ref method) = self.rpc_method_filter {
            if method != rpc_method {
                return false;
            }
        }

        // Check contract filter.
        if let Some(ref filter) = self.contract_id_filter {
            match contract_id {
                Some(id) if id == filter => {}
                _ => return false,
            }
        }

        // Check account filter.
        if let Some(ref filter) = self.account_filter {
            match account {
                Some(acct) if acct == filter => {}
                _ => return false,
            }
        }

        // Check probability.
        if rng_probability > self.probability {
            return false;
        }

        self.times_fired += 1;
        true
    }

    /// Create a pre-defined "RPC timeout" rule.
    pub fn rpc_timeout() -> Self {
        Self::new("rpc-timeout", FailureMode::RpcTimeout)
    }

    /// Create a pre-defined "contract panic" rule.
    pub fn contract_panic(contract_id: &str) -> Self {
        Self::new("contract-panic", FailureMode::ContractPanic).with_contract(contract_id)
    }

    /// Create a pre-defined "insufficient fee" rule.
    pub fn insufficient_fee() -> Self {
        Self::new("insufficient-fee", FailureMode::InsufficientFee)
    }
}

// ── Failure Injector ──────────────────────────────────────────────────────────

/// Manages failure rules and determines whether a given operation should fail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureInjector {
    /// Active failure rules.
    rules: Vec<FailureRule>,
    /// Whether failure injection is enabled globally.
    pub enabled: bool,
}

impl FailureInjector {
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            enabled: false,
        }
    }

    /// Enable failure injection.
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disable failure injection.
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Add a failure rule.
    pub fn add_rule(&mut self, rule: FailureRule) {
        self.rules.push(rule);
    }

    /// Remove a failure rule by name.
    pub fn remove_rule(&mut self, name: &str) -> bool {
        let before = self.rules.len();
        self.rules.retain(|r| r.name != name);
        before != self.rules.len()
    }

    /// Clear all failure rules.
    pub fn clear_rules(&mut self) {
        self.rules.clear();
    }

    /// Get a reference to all rules.
    pub fn rules(&self) -> &[FailureRule] {
        &self.rules
    }

    /// Check if any active rule should fire, returning the failure mode.
    /// Returns `None` if no rule fires.
    pub fn check(
        &mut self,
        rpc_method: &str,
        contract_id: Option<&str>,
        account: Option<&str>,
        rng_probability: f64,
    ) -> Option<FailureMode> {
        if !self.enabled {
            return None;
        }

        for rule in &mut self.rules {
            if rule.should_fire(rpc_method, contract_id, account, rng_probability) {
                return Some(rule.mode.clone());
            }
        }

        None
    }

    /// Check if all rules have been exhausted.
    pub fn is_exhausted(&self) -> bool {
        if self.rules.is_empty() {
            return true;
        }
        self.rules
            .iter()
            .all(|r| r.max_activations > 0 && r.times_fired >= r.max_activations)
    }

    /// Reset all rule counters.
    pub fn reset_counters(&mut self) {
        for rule in &mut self.rules {
            rule.times_fired = 0;
        }
    }

    /// Return the number of active rules.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

impl Default for FailureInjector {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate a JSON-RPC error response for the given failure mode.
pub fn failure_to_rpc_error(mode: &FailureMode) -> (i64, String) {
    match mode {
        FailureMode::RpcTimeout => (-32000, "RPC request timed out".to_string()),
        FailureMode::RpcConnectionRefused => (-32001, "RPC connection refused".to_string()),
        FailureMode::RpcError { code } => (*code, format!("RPC error (code {})", code)),
        FailureMode::InsufficientFee => (-32002, "Insufficient fee for transaction".to_string()),
        FailureMode::BadAuth => (-32003, "Bad authorization for transaction".to_string()),
        FailureMode::ContractPanic => (-32010, "Contract invocation panicked".to_string()),
        FailureMode::ContractError { code, message } => (-32010 - *code as i64, message.clone()),
        FailureMode::AccountNotFound => (-32004, "Account not found".to_string()),
        FailureMode::ContractNotFound => (-32005, "Contract not found".to_string()),
        FailureMode::InsufficientBalance => (-32006, "Insufficient account balance".to_string()),
        FailureMode::LedgerSequenceMismatch => {
            (-32007, "Ledger sequence number mismatch".to_string())
        }
        FailureMode::BudgetExceeded => (-32008, "CPU/memory budget exceeded".to_string()),
        FailureMode::RandomFailure(_) => (-32009, "Random injected failure".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injector_disabled_by_default() {
        let injector = FailureInjector::new();
        assert!(!injector.enabled);
    }

    #[test]
    fn enable_and_disable() {
        let mut injector = FailureInjector::new();
        injector.enable();
        assert!(injector.enabled);
        injector.disable();
        assert!(!injector.enabled);
    }

    #[test]
    fn no_failure_when_disabled() {
        let mut injector = FailureInjector::new();
        injector.add_rule(FailureRule::new("test", FailureMode::ContractPanic));
        let result = injector.check("simulateTransaction", None, None, 0.5);
        assert!(result.is_none());
    }

    #[test]
    fn rule_fires_when_enabled() {
        let mut injector = FailureInjector::new();
        injector.enable();
        injector.add_rule(FailureRule::new("test", FailureMode::ContractPanic));
        let result = injector.check("simulateTransaction", None, None, 0.5);
        assert_eq!(result, Some(FailureMode::ContractPanic));
    }

    #[test]
    fn rpc_method_filter() {
        let mut injector = FailureInjector::new();
        injector.enable();
        injector.add_rule(
            FailureRule::new("test", FailureMode::InsufficientFee)
                .with_rpc_method("sendTransaction"),
        );

        // Should not fire for a different method.
        let result = injector.check("simulateTransaction", None, None, 0.5);
        assert!(result.is_none());

        // Should fire for the matched method.
        let result = injector.check("sendTransaction", None, None, 0.5);
        assert_eq!(result, Some(FailureMode::InsufficientFee));
    }

    #[test]
    fn contract_filter() {
        let mut injector = FailureInjector::new();
        injector.enable();
        injector.add_rule(FailureRule::new("test", FailureMode::ContractPanic).with_contract("C1"));

        // Wrong contract.
        let result = injector.check("simulateTransaction", Some("C2"), None, 0.5);
        assert!(result.is_none());

        // Correct contract.
        let result = injector.check("simulateTransaction", Some("C1"), None, 0.5);
        assert_eq!(result, Some(FailureMode::ContractPanic));
    }

    #[test]
    fn probability_filter() {
        let mut injector = FailureInjector::new();
        injector.enable();
        injector
            .add_rule(FailureRule::new("rare", FailureMode::ContractPanic).with_probability(0.0));

        // Probability 0.0 should never fire.
        for _ in 0..10 {
            let result = injector.check("simulateTransaction", None, None, 0.5);
            assert!(result.is_none());
        }
    }

    #[test]
    fn max_activations() {
        let mut injector = FailureInjector::new();
        injector.enable();
        injector.add_rule(
            FailureRule::new("limited", FailureMode::ContractPanic).with_max_activations(2),
        );

        assert!(injector.check("sim", None, None, 1.0).is_some());
        assert!(injector.check("sim", None, None, 1.0).is_some());
        assert!(injector.check("sim", None, None, 1.0).is_none());
    }

    #[test]
    fn remove_rule() {
        let mut injector = FailureInjector::new();
        injector.add_rule(FailureRule::new("r1", FailureMode::ContractPanic));
        injector.add_rule(FailureRule::new("r2", FailureMode::RpcTimeout));
        assert_eq!(injector.rule_count(), 2);
        assert!(injector.remove_rule("r1"));
        assert_eq!(injector.rule_count(), 1);
        assert!(!injector.remove_rule("nonexistent"));
    }

    #[test]
    fn clear_rules() {
        let mut injector = FailureInjector::new();
        injector.add_rule(FailureRule::new("a", FailureMode::ContractPanic));
        injector.add_rule(FailureRule::new("b", FailureMode::RpcTimeout));
        injector.clear_rules();
        assert_eq!(injector.rule_count(), 0);
    }

    #[test]
    fn reset_counters() {
        let mut injector = FailureInjector::new();
        injector.enable();
        injector
            .add_rule(FailureRule::new("r", FailureMode::ContractPanic).with_max_activations(1));
        injector.check("sim", None, None, 1.0);
        assert!(injector.check("sim", None, None, 1.0).is_none());
        injector.reset_counters();
        assert!(injector.check("sim", None, None, 1.0).is_some());
    }

    #[test]
    fn failure_to_rpc_error_returns_valid_codes() {
        let modes = vec![
            FailureMode::RpcTimeout,
            FailureMode::RpcConnectionRefused,
            FailureMode::RpcError { code: -123 },
            FailureMode::RpcError { code: -32099 },
            FailureMode::InsufficientFee,
            FailureMode::BadAuth,
            FailureMode::ContractPanic,
            FailureMode::ContractError {
                code: 1,
                message: "nope".to_string(),
            },
            FailureMode::AccountNotFound,
            FailureMode::ContractNotFound,
            FailureMode::InsufficientBalance,
            FailureMode::LedgerSequenceMismatch,
            FailureMode::BudgetExceeded,
            FailureMode::RandomFailure(0.5),
        ];

        for mode in modes {
            let (code, message) = failure_to_rpc_error(&mode);
            assert!(code < 0, "expected negative error code, got {}", code);
            assert!(!message.is_empty(), "expected non-empty error message");
        }
    }
}
