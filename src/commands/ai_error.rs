//! AI Error Handling and Recovery Commands
//!
//! Provides commands for managing AI error handling, viewing analytics,
//! and configuring fallback providers.

use crate::utils::{ai_error_handler::AiErrorHandler, print as p};
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub enum AiErrorCommands {
    /// Show AI error statistics and recovery metrics
    Stats,

    /// Reset error analytics
    Reset,

    /// List available AI providers and their status
    ListProviders,

    /// Enable or disable a specific provider
    ToggleProvider {
        /// Provider name (e.g., ollama, openai, anthropic)
        provider: String,

        /// Enable the provider
        #[arg(long)]
        enable: bool,

        /// Disable the provider
        #[arg(long)]
        disable: bool,
    },

    /// Test error recovery with a simulated failure
    TestRecovery {
        /// Number of simulated failures before success
        #[arg(long, default_value_t = 2)]
        failures: u32,
    },
}

pub async fn handle(cmd: AiErrorCommands) -> Result<()> {
    match cmd {
        AiErrorCommands::Stats => handle_stats().await,
        AiErrorCommands::Reset => handle_reset().await,
        AiErrorCommands::ListProviders => handle_list_providers().await,
        AiErrorCommands::ToggleProvider {
            provider,
            enable,
            disable,
        } => handle_toggle_provider(&provider, enable, disable).await,
        AiErrorCommands::TestRecovery { failures } => handle_test_recovery(failures).await,
    }
}

async fn handle_stats() -> Result<()> {
    p::header("AI Error Analytics");
    p::separator();

    let handler = AiErrorHandler::new();
    let analytics = handler.get_analytics().await;

    p::kv("Total Errors", &analytics.total_errors.to_string());
    p::kv(
        "Successful Recoveries",
        &analytics.successful_recoveries.to_string(),
    );
    p::kv(
        "Failed Recoveries",
        &analytics.failed_recoveries.to_string(),
    );

    let recovery_rate = analytics.recovery_rate();
    p::kv("Recovery Rate", &format!("{:.1}%", recovery_rate * 100.0));

    println!();
    p::info("Errors by Category:");
    println!();

    let headers = &["Category", "Count"];
    let mut rows: Vec<Vec<String>> = analytics
        .errors_by_category
        .iter()
        .map(|(cat, count)| vec![cat.user_friendly_name().to_string(), count.to_string()])
        .collect();

    rows.sort_by(|a, b| b[1].cmp(&a[1]));

    if rows.is_empty() {
        p::info("No errors recorded yet.");
    } else {
        p::table(headers, &rows);
    }

    println!();
    p::info("Errors by Provider:");
    println!();

    let headers = &["Provider", "Count"];
    let mut rows: Vec<Vec<String>> = analytics
        .errors_by_provider
        .iter()
        .map(|(provider, count)| vec![provider.clone(), count.to_string()])
        .collect();

    rows.sort_by(|a, b| b[1].cmp(&a[1]));

    if rows.is_empty() {
        p::info("No errors recorded yet.");
    } else {
        p::table(headers, &rows);
    }

    p::separator();
    Ok(())
}

async fn handle_reset() -> Result<()> {
    p::header("Reset AI Error Analytics");
    p::separator();

    let handler = AiErrorHandler::new();
    handler.reset_analytics().await;

    p::success("Error analytics have been reset.");
    p::separator();
    Ok(())
}

async fn handle_list_providers() -> Result<()> {
    p::header("AI Providers");
    p::separator();

    let handler = AiErrorHandler::new();
    let providers = handler.get_providers();

    let headers = &["Provider", "Priority", "Status"];
    let rows: Vec<Vec<String>> = providers
        .iter()
        .map(|p| {
            vec![
                p.name.clone(),
                p.priority.to_string(),
                if p.enabled {
                    "Enabled ✓"
                } else {
                    "Disabled ✗"
                }
                .to_string(),
            ]
        })
        .collect();

    p::table(headers, &rows);
    p::separator();
    Ok(())
}

async fn handle_toggle_provider(provider: &str, enable: bool, disable: bool) -> Result<()> {
    p::header(&format!("Toggle Provider: {}", provider));
    p::separator();

    if enable && disable {
        anyhow::bail!("Cannot both enable and disable a provider. Use only one flag.");
    }

    let enabled = if enable {
        true
    } else if disable {
        false
    } else {
        // Toggle current state
        let handler = AiErrorHandler::new();
        let providers = handler.get_providers();
        let current = providers.iter().find(|p| p.name == provider);

        match current {
            Some(p) => !p.enabled,
            None => anyhow::bail!(
                "Provider '{}' not found. Available providers: {}",
                provider,
                providers
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    };

    let mut handler = AiErrorHandler::new();
    handler.set_provider_enabled(provider, enabled).await;

    let status = if enabled { "enabled" } else { "disabled" };
    p::success(&format!("Provider '{}' has been {}.", provider, status));
    p::separator();
    Ok(())
}

async fn handle_test_recovery(failures: u32) -> Result<()> {
    p::header("Test Error Recovery");
    p::separator();
    p::kv("Simulated Failures", &failures.to_string());
    println!();

    let handler = AiErrorHandler::new();
    let mut attempt_count = 0;

    let result = handler
        .execute_with_recovery(
            |provider| async move {
                attempt_count += 1;
                if attempt_count <= failures {
                    anyhow::bail!("Simulated failure from {}", provider);
                }
                Ok("Success!")
            },
            "ollama",
        )
        .await;

    match result {
        Ok(_) => {
            p::success(&format!(
                "Recovery successful after {} attempts.",
                attempt_count
            ));
        }
        Err(e) => {
            p::error(&format!(
                "Recovery failed after {} attempts: {}",
                attempt_count, e
            ));
        }
    }

    // Show updated analytics
    println!();
    let analytics = handler.get_analytics().await;
    p::kv("Total Errors", &analytics.total_errors.to_string());
    p::kv(
        "Recovery Rate",
        &format!("{:.1}%", analytics.recovery_rate() * 100.0),
    );

    p::separator();
    Ok(())
}
