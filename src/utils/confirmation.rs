use crate::utils::interactive;
use crate::utils::print as p;
use anyhow::Result;
use colored::*;
use std::io::{BufRead, Write};

/// Explicit opt-in required to skip destructive confirmations via `--yes` or env.
/// Documented as unsafe; logs a prominent warning when used.
pub const ENV_UNSAFE_SKIP_CONFIRMATION: &str = "STARFORGE_UNSAFE_SKIP_CONFIRMATION";

/// Maximum characters accepted for a challenge response (paste-guard).
const MAX_CHALLENGE_INPUT_LEN: usize = 128;

/// Risk level for operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

impl RiskLevel {
    pub fn display(&self) -> colored::ColoredString {
        match self {
            RiskLevel::Low => "LOW".green(),
            RiskLevel::Medium => "MEDIUM".yellow(),
            RiskLevel::High => "HIGH".red(),
        }
    }
}

/// Destructive actions that require a typed challenge phrase and unsafe opt-in
/// to bypass confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestructiveAction {
    MainnetDeploy,
    SecretReveal,
    AccountMerge,
    MainnetTransaction,
    ContractInvoke,
}

impl DestructiveAction {
    pub fn default_challenge_phrase(self) -> &'static str {
        match self {
            DestructiveAction::MainnetDeploy => "deploy-mainnet",
            DestructiveAction::SecretReveal => "reveal-secret",
            DestructiveAction::AccountMerge => "merge-account",
            DestructiveAction::MainnetTransaction => "send-mainnet",
            DestructiveAction::ContractInvoke => "invoke-mainnet",
        }
    }

    pub fn log_label(self) -> &'static str {
        match self {
            DestructiveAction::MainnetDeploy => "mainnet_deploy",
            DestructiveAction::SecretReveal => "secret_reveal",
            DestructiveAction::AccountMerge => "account_merge",
            DestructiveAction::MainnetTransaction => "mainnet_transaction",
            DestructiveAction::ContractInvoke => "contract_invoke",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationOutcome {
    Confirmed,
    Cancelled,
    SkippedDryRun,
    SkippedUnsafeBypass,
}

/// Configuration for confirmation prompts
pub struct ConfirmationConfig {
    /// Risk level of the operation
    pub risk_level: RiskLevel,
    /// Network being used (for mainnet warnings)
    pub network: String,
    /// Whether to skip confirmation (from --yes flag)
    pub skip_confirm: bool,
    /// Whether this is a dry-run/preview
    pub dry_run: bool,
    /// Custom confirmation message
    pub prompt: Option<String>,
    /// Whether to require typing "yes" for high-risk operations (legacy)
    pub require_type_confirmation: bool,
    /// Destructive action category (enables challenge phrase + unsafe bypass rules)
    pub destructive_action: Option<DestructiveAction>,
    /// Override the default challenge phrase for this action
    pub challenge_phrase: Option<String>,
}

impl Default for ConfirmationConfig {
    fn default() -> Self {
        Self {
            risk_level: RiskLevel::Medium,
            network: "testnet".to_string(),
            skip_confirm: false,
            dry_run: false,
            prompt: None,
            require_type_confirmation: false,
            destructive_action: None,
            challenge_phrase: None,
        }
    }
}

impl ConfirmationConfig {
    /// Resolve the challenge phrase for this confirmation, if any.
    pub fn resolved_challenge_phrase(&self) -> Option<String> {
        if let Some(phrase) = &self.challenge_phrase {
            return Some(phrase.clone());
        }
        self.destructive_action
            .map(|action| action.default_challenge_phrase().to_string())
    }

    fn requires_challenge(&self) -> bool {
        self.resolved_challenge_phrase().is_some()
            || self.require_type_confirmation
            || self.risk_level == RiskLevel::High
    }
}

/// Display a prominent mainnet warning
pub fn display_mainnet_warning(network: &str) {
    if network == "mainnet" {
        println!();
        p::separator();
        println!(
            "{} {}",
            "⚠ WARNING:".red().bold(),
            "You are operating on MAINNET".bright_red().bold()
        );
        println!(
            "{}",
            "  This will use REAL funds and cannot be undone.".bright_red()
        );
        println!(
            "{}",
            "  Double-check all addresses, amounts, and parameters.".bright_red()
        );
        p::separator();
        println!();
    }
}

/// Display an operation summary before confirmation
pub struct OperationSummary {
    pub title: String,
    pub items: Vec<(String, String)>,
    pub network: String,
    pub risk_level: RiskLevel,
}

impl OperationSummary {
    pub fn new(title: String, network: String, risk_level: RiskLevel) -> Self {
        Self {
            title,
            items: Vec::new(),
            network,
            risk_level,
        }
    }

    pub fn add(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.items.push((key.into(), value.into()));
        self
    }

    pub fn display(&self) {
        p::header(&self.title);
        p::separator();

        // Display risk level
        p::kv("Risk Level", &self.risk_level.display().to_string());
        p::kv("Network", &self.network);

        println!();

        // Display all items
        for (key, value) in &self.items {
            p::kv(key, value);
        }

        p::separator();
    }
}

fn truthy_env(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

/// Returns true when the caller explicitly opted into the unsafe confirmation bypass.
pub fn unsafe_skip_confirmation_enabled() -> bool {
    truthy_env(ENV_UNSAFE_SKIP_CONFIRMATION)
}

/// Validate a challenge response against the expected phrase.
/// Rejects multiline paste, overlong input, and case-normalized shortcuts.
pub fn validate_challenge_response(input: &str, expected: &str) -> bool {
    if input.contains('\n') || input.contains('\r') || input.contains('\t') {
        return false;
    }
    if input.len() > MAX_CHALLENGE_INPUT_LEN {
        return false;
    }
    input.trim() == expected
}

fn log_confirmation_outcome(
    config: &ConfirmationConfig,
    outcome: ConfirmationOutcome,
    action_label: Option<&str>,
) {
    let action = action_label
        .or_else(|| config.destructive_action.map(|a| a.log_label()))
        .unwrap_or("generic");
    tracing::info!(
        confirmation_action = action,
        confirmation_network = %config.network,
        confirmation_risk = ?config.risk_level,
        confirmation_outcome = ?outcome,
        confirmation_unsafe_bypass = unsafe_skip_confirmation_enabled(),
        "confirmation prompt completed"
    );
}

fn allow_skip_confirm(config: &ConfirmationConfig) -> Result<bool> {
    if !config.skip_confirm {
        return Ok(false);
    }

    if config.destructive_action.is_some() {
        if !unsafe_skip_confirmation_enabled() {
            anyhow::bail!(
                "Refusing to skip confirmation for destructive action without explicit unsafe opt-in. \
                 Set {}=1 only in controlled automation (see docs/CONFIRMATION_UX.md). \
                 Pass --yes together with that env var to bypass.",
                ENV_UNSAFE_SKIP_CONFIRMATION
            );
        }
        p::warn(&format!(
            "UNSAFE: skipping destructive confirmation via --yes and {}",
            ENV_UNSAFE_SKIP_CONFIRMATION
        ));
        log_confirmation_outcome(config, ConfirmationOutcome::SkippedUnsafeBypass, None);
        return Ok(true);
    }

    Ok(true)
}

/// Request user confirmation for an operation
pub fn confirm_operation(summary: &OperationSummary, config: &ConfirmationConfig) -> Result<bool> {
    // Display mainnet warning if applicable
    display_mainnet_warning(&config.network);

    // Display operation summary
    summary.display();

    // If dry-run, show preview message and return true
    if config.dry_run {
        println!();
        p::info("Dry-run mode: This is a preview only. No changes will be made.");
        println!();
        log_confirmation_outcome(config, ConfirmationOutcome::SkippedDryRun, None);
        return Ok(true);
    }

    // Skip confirmation if requested (with destructive-action guardrails)
    if allow_skip_confirm(config)? {
        println!();
        if config.destructive_action.is_none() {
            p::info("Skipping confirmation (--yes flag provided)");
        }
        println!();
        if config.destructive_action.is_none() {
            log_confirmation_outcome(config, ConfirmationOutcome::SkippedUnsafeBypass, None);
        }
        return Ok(true);
    }

    // Fail clearly instead of blocking on stdin: a non-interactive caller
    // (CI, a piped script) has no one to answer this prompt. The `--yes`
    // flag (checked above) is the secure, headless alternative.
    interactive::ensure_interactive("confirmation", "Pass --yes to confirm non-interactively.")?;

    // Request confirmation
    println!();

    let prompt = config
        .prompt
        .as_deref()
        .unwrap_or("Proceed with this operation?");

    let confirmed = if config.requires_challenge() {
        let expected = config
            .resolved_challenge_phrase()
            .unwrap_or_else(|| "yes".to_string());

        print!(
            "  {} [type exactly '{}']: ",
            prompt.bright_white(),
            expected.bright_yellow()
        );
        std::io::stdout().flush()?;

        let line = std::io::stdin()
            .lock()
            .lines()
            .next()
            .unwrap_or(Ok(String::new()))?;

        validate_challenge_response(&line, &expected)
    } else {
        // Simple y/N confirmation
        print!("  {} [y/N]: ", prompt.bright_white());
        std::io::stdout().flush()?;

        let line = std::io::stdin()
            .lock()
            .lines()
            .next()
            .unwrap_or(Ok(String::new()))?;

        matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
    };

    if !confirmed {
        println!();
        p::info("Operation cancelled.");
        log_confirmation_outcome(config, ConfirmationOutcome::Cancelled, None);
        return Ok(false);
    }

    println!();
    log_confirmation_outcome(config, ConfirmationOutcome::Confirmed, None);
    Ok(true)
}

/// Display a preview of what will happen without executing
pub fn display_preview(summary: &OperationSummary) {
    p::header("Preview Mode");
    p::separator();
    p::kv("Risk Level", &summary.risk_level.display().to_string());
    p::kv("Network", &summary.network);
    println!();

    for (key, value) in &summary.items {
        p::kv(key, value);
    }

    p::separator();
    println!();
    p::info("This is a preview. Use --execute to perform this operation.");
    println!();
}

/// Validate that user has confirmed the action
pub fn validate_confirmation(
    network: &str,
    skip_confirm: bool,
    dry_run: bool,
    risk_level: RiskLevel,
) -> ConfirmationConfig {
    ConfirmationConfig {
        risk_level,
        network: network.to_string(),
        skip_confirm,
        dry_run,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_level_display() {
        assert!(RiskLevel::Low.display().to_string().contains("LOW"));
        assert!(RiskLevel::Medium.display().to_string().contains("MEDIUM"));
        assert!(RiskLevel::High.display().to_string().contains("HIGH"));
    }

    #[test]
    fn test_operation_summary_builder() {
        let summary =
            OperationSummary::new("Test".to_string(), "testnet".to_string(), RiskLevel::Low)
                .add("Key1", "Value1")
                .add("Key2", "Value2");

        assert_eq!(summary.items.len(), 2);
        assert_eq!(summary.items[0].0, "Key1");
        assert_eq!(summary.items[1].0, "Key2");
    }

    #[test]
    fn test_confirmation_config_default() {
        let config = ConfirmationConfig::default();
        assert_eq!(config.risk_level, RiskLevel::Medium);
        assert_eq!(config.network, "testnet");
        assert!(!config.skip_confirm);
        assert!(!config.dry_run);
    }

    #[test]
    fn challenge_phrase_must_match_exactly() {
        assert!(validate_challenge_response("deploy-mainnet", "deploy-mainnet"));
        assert!(!validate_challenge_response("Deploy-mainnet", "deploy-mainnet"));
        assert!(!validate_challenge_response("yes", "deploy-mainnet"));
        assert!(validate_challenge_response("  reveal-secret  ", "reveal-secret"));
    }

    #[test]
    fn challenge_rejects_multiline_paste() {
        assert!(!validate_challenge_response(
            "deploy-mainnet\nyes",
            "deploy-mainnet"
        ));
        assert!(!validate_challenge_response(
            "deploy-mainnet\r\n",
            "deploy-mainnet"
        ));
        assert!(!validate_challenge_response(
            "deploy\tmainnet",
            "deploy-mainnet"
        ));
    }

    #[test]
    fn challenge_rejects_overlong_input() {
        let long = "a".repeat(MAX_CHALLENGE_INPUT_LEN + 1);
        assert!(!validate_challenge_response(&long, "deploy-mainnet"));
    }

    #[test]
    fn destructive_action_default_phrases_are_stable() {
        assert_eq!(
            DestructiveAction::MainnetDeploy.default_challenge_phrase(),
            "deploy-mainnet"
        );
        assert_eq!(
            DestructiveAction::SecretReveal.default_challenge_phrase(),
            "reveal-secret"
        );
    }

    #[test]
    fn resolved_challenge_phrase_prefers_override() {
        let config = ConfirmationConfig {
            destructive_action: Some(DestructiveAction::AccountMerge),
            challenge_phrase: Some("alice".into()),
            ..Default::default()
        };
        assert_eq!(config.resolved_challenge_phrase(), Some("alice".into()));
    }

    // ── CI / non-interactive prompting ───────────────────────────────────────

    fn clear_env() {
        std::env::remove_var(interactive::ENV_NON_INTERACTIVE);
        std::env::remove_var("CI");
        std::env::remove_var(ENV_UNSAFE_SKIP_CONFIRMATION);
    }

    #[test]
    fn confirm_operation_fails_fast_in_ci_without_yes() {
        let _guard = interactive::env_test_lock().lock().unwrap();
        clear_env();
        std::env::set_var("CI", "1");

        let summary =
            OperationSummary::new("Deploy".to_string(), "testnet".to_string(), RiskLevel::Low);
        let config = ConfirmationConfig {
            risk_level: RiskLevel::Low,
            network: "testnet".to_string(),
            skip_confirm: false,
            dry_run: false,
            ..Default::default()
        };

        let err = confirm_operation(&summary, &config)
            .unwrap_err()
            .to_string();
        assert!(err.contains("--yes"), "got: {}", err);

        clear_env();
    }

    #[test]
    fn confirm_operation_skip_confirm_bypasses_ci_check() {
        let _guard = interactive::env_test_lock().lock().unwrap();
        clear_env();
        std::env::set_var("CI", "1");

        let summary =
            OperationSummary::new("Deploy".to_string(), "testnet".to_string(), RiskLevel::Low);
        let config = ConfirmationConfig {
            risk_level: RiskLevel::Low,
            network: "testnet".to_string(),
            skip_confirm: true,
            dry_run: false,
            ..Default::default()
        };

        assert!(confirm_operation(&summary, &config).unwrap());

        clear_env();
    }

    #[test]
    fn confirm_operation_dry_run_bypasses_ci_check() {
        let _guard = interactive::env_test_lock().lock().unwrap();
        clear_env();
        std::env::set_var("CI", "1");

        let summary =
            OperationSummary::new("Deploy".to_string(), "testnet".to_string(), RiskLevel::Low);
        let config = ConfirmationConfig {
            risk_level: RiskLevel::Low,
            network: "testnet".to_string(),
            skip_confirm: false,
            dry_run: true,
            ..Default::default()
        };

        assert!(confirm_operation(&summary, &config).unwrap());

        clear_env();
    }

    #[test]
    fn destructive_skip_requires_unsafe_env() {
        let _guard = interactive::env_test_lock().lock().unwrap();
        clear_env();
        std::env::set_var("CI", "1");

        let summary =
            OperationSummary::new("Deploy".to_string(), "mainnet".to_string(), RiskLevel::High);
        let config = ConfirmationConfig {
            risk_level: RiskLevel::High,
            network: "mainnet".to_string(),
            skip_confirm: true,
            dry_run: false,
            destructive_action: Some(DestructiveAction::MainnetDeploy),
            ..Default::default()
        };

        let err = confirm_operation(&summary, &config)
            .unwrap_err()
            .to_string();
        assert!(err.contains(ENV_UNSAFE_SKIP_CONFIRMATION), "got: {}", err);

        clear_env();
    }

    #[test]
    fn destructive_skip_allowed_with_unsafe_env() {
        let _guard = interactive::env_test_lock().lock().unwrap();
        clear_env();
        std::env::set_var("CI", "1");
        std::env::set_var(ENV_UNSAFE_SKIP_CONFIRMATION, "1");

        let summary =
            OperationSummary::new("Deploy".to_string(), "mainnet".to_string(), RiskLevel::High);
        let config = ConfirmationConfig {
            risk_level: RiskLevel::High,
            network: "mainnet".to_string(),
            skip_confirm: true,
            dry_run: false,
            destructive_action: Some(DestructiveAction::MainnetDeploy),
            ..Default::default()
        };

        assert!(confirm_operation(&summary, &config).unwrap());

        clear_env();
    }
}
