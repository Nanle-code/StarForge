use crate::utils::interactive;
use crate::utils::print as p;
use anyhow::Result;
use colored::*;
use std::io::{BufRead, Write};

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
    /// Whether to require typing "yes" for high-risk operations
    pub require_type_confirmation: bool,
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
        }
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
        return Ok(true);
    }

    // Skip confirmation if requested
    if config.skip_confirm {
        println!();
        p::info("Skipping confirmation (--yes flag provided)");
        println!();
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

    if config.require_type_confirmation || summary.risk_level == RiskLevel::High {
        // Require typing "yes" for high-risk operations
        print!("  {} [type 'yes' to confirm]: ", prompt.bright_white());
        std::io::stdout().flush()?;

        let line = std::io::stdin()
            .lock()
            .lines()
            .next()
            .unwrap_or(Ok(String::new()))?;

        if line.trim().to_lowercase() != "yes" {
            println!();
            p::info("Operation cancelled.");
            return Ok(false);
        }
    } else {
        // Simple y/N confirmation
        print!("  {} [y/N]: ", prompt.bright_white());
        std::io::stdout().flush()?;

        let line = std::io::stdin()
            .lock()
            .lines()
            .next()
            .unwrap_or(Ok(String::new()))?;

        if !matches!(line.trim().to_lowercase().as_str(), "y" | "yes") {
            println!();
            p::info("Operation cancelled.");
            return Ok(false);
        }
    }

    println!();
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

    // ── CI / non-interactive prompting ───────────────────────────────────────

    fn clear_env() {
        std::env::remove_var(interactive::ENV_NON_INTERACTIVE);
        std::env::remove_var("CI");
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
}
