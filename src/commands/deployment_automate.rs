//! Deployment automation commands.
//!
//! Provides CLI commands for AI-powered deployment automation.

use crate::utils::{
    deployment_automation::{run_automation_pipeline, AutomationLevel, DeploymentAutomationConfig},
    print as p,
};
use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum DeploymentAutomateCommands {
    /// Run automated deployment pipeline
    Run {
        /// Path to the WASM file to deploy
        #[arg(long, value_name = "FILE")]
        wasm: PathBuf,

        /// Target network
        #[arg(long, default_value = "testnet")]
        network: String,

        /// Wallet name to use
        #[arg(long)]
        wallet: Option<String>,

        /// Automation level (basic, standard, full)
        #[arg(long, default_value = "standard")]
        level: String,

        /// Enable pre-deployment validation
        #[arg(long, default_value = "true")]
        pre_deployment_checks: bool,

        /// Enable automated testing
        #[arg(long, default_value = "true")]
        automated_testing: bool,

        /// Enable post-deployment verification
        #[arg(long, default_value = "true")]
        post_deployment_verification: bool,

        /// Enable rollback automation on failure
        #[arg(long, default_value = "true")]
        rollback_automation: bool,

        /// Enable monitoring setup
        #[arg(long, default_value = "true")]
        monitoring_setup: bool,

        /// Ignore existing checkpoints and start a fresh deployment run
        #[arg(long, alias = "force")]
        fresh: bool,

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Show automation history
    History {
        /// Number of recent automations to show
        #[arg(long, default_value = "10")]
        limit: usize,
    },

    /// Configure automation settings
    Config {
        /// Set default automation level
        #[arg(long)]
        level: Option<String>,

        /// Enable/disable pre-deployment checks by default
        #[arg(long)]
        pre_deployment_checks: Option<bool>,

        /// Enable/disable automated testing by default
        #[arg(long)]
        automated_testing: Option<bool>,

        /// Enable/disable post-deployment verification by default
        #[arg(long)]
        post_deployment_verification: Option<bool>,

        /// Enable/disable rollback automation by default
        #[arg(long)]
        rollback_automation: Option<bool>,

        /// Enable/disable monitoring setup by default
        #[arg(long)]
        monitoring_setup: Option<bool>,
    },
}

pub async fn handle(cmd: DeploymentAutomateCommands) -> Result<()> {
    match cmd {
        DeploymentAutomateCommands::Run {
            wasm,
            network,
            wallet,
            level,
            pre_deployment_checks,
            automated_testing,
            post_deployment_verification,
            rollback_automation,
            monitoring_setup,
            fresh,
            json,
        } => {
            handle_run(
                wasm,
                network,
                wallet,
                level,
                pre_deployment_checks,
                automated_testing,
                post_deployment_verification,
                rollback_automation,
                monitoring_setup,
                fresh,
                json,
            )
            .await
        }
        DeploymentAutomateCommands::History { limit } => handle_history(limit),
        DeploymentAutomateCommands::Config {
            level,
            pre_deployment_checks,
            automated_testing,
            post_deployment_verification,
            rollback_automation,
            monitoring_setup,
        } => handle_config(
            level,
            pre_deployment_checks,
            automated_testing,
            post_deployment_verification,
            rollback_automation,
            monitoring_setup,
        ),
    }
}

async fn handle_run(
    wasm: PathBuf,
    network: String,
    wallet: Option<String>,
    level: String,
    pre_deployment_checks: bool,
    automated_testing: bool,
    post_deployment_verification: bool,
    rollback_automation: bool,
    monitoring_setup: bool,
    fresh: bool,
    json: bool,
) -> Result<()> {
    p::header("Deployment Automation Pipeline");
    p::separator();

    let automation_level = match level.as_str() {
        "basic" => AutomationLevel::Basic,
        "standard" => AutomationLevel::Standard,
        "full" => AutomationLevel::Full,
        _ => {
            p::warn(&format!(
                "Unknown automation level '{}', using 'standard'",
                level
            ));
            AutomationLevel::Standard
        }
    };

    let config = DeploymentAutomationConfig {
        wasm_path: wasm.to_string_lossy().to_string(),
        network,
        wallet,
        enable_pre_deployment_checks: pre_deployment_checks,
        enable_automated_testing: automated_testing,
        enable_post_deployment_verification: post_deployment_verification,
        enable_rollback_automation: rollback_automation,
        enable_monitoring_setup: monitoring_setup,
        automation_level,
        fresh,
    };

    p::kv("WASM File", &config.wasm_path);
    p::kv("Network", &config.network);
    if let Some(ref wallet) = config.wallet {
        p::kv("Wallet", wallet);
    }
    p::kv("Automation Level", &automation_level.to_string());
    p::kv(
        "Pre-deployment Checks",
        if pre_deployment_checks {
            "enabled"
        } else {
            "disabled"
        },
    );
    p::kv(
        "Automated Testing",
        if automated_testing {
            "enabled"
        } else {
            "disabled"
        },
    );
    p::kv(
        "Post-deployment Verification",
        if post_deployment_verification {
            "enabled"
        } else {
            "disabled"
        },
    );
    p::kv(
        "Rollback Automation",
        if rollback_automation {
            "enabled"
        } else {
            "disabled"
        },
    );
    p::kv(
        "Monitoring Setup",
        if monitoring_setup {
            "enabled"
        } else {
            "disabled"
        },
    );
    println!();

    let spinner = p::spinner("Running automation pipeline...");
    let result = run_automation_pipeline(&config).await?;
    spinner.finish_and_clear();

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        print_automation_result(&result);
    }

    p::separator();
    Ok(())
}

fn print_automation_result(result: &crate::utils::deployment_automation::CompleteAutomationResult) {
    p::kv("Automation ID", &result.automation_id);
    p::kv("Timestamp", &result.timestamp);
    p::kv("Automation Level", &result.automation_level);
    p::kv(
        "Overall Success",
        if result.overall_success { "yes" } else { "no" },
    );
    p::kv("Summary", &result.summary);
    println!();

    // Pre-deployment validation
    if let Some(ref validation) = result.pre_deployment_validation {
        p::header("Pre-deployment Validation");
        p::kv("Status", &validation.overall_status);
        p::kv(
            "Approved",
            if validation.approved_for_deployment {
                "yes"
            } else {
                "no"
            },
        );

        println!();
        p::info("Validation Checks:");
        for check in &validation.checks {
            let status_color = if check.status == "pass" {
                "green"
            } else {
                "red"
            };
            println!(
                "  [{}] {}",
                check.status.as_str().color(status_color),
                check.check_name
            );
            println!("    Description: {}", check.description);
            if let Some(ref details) = check.details {
                println!("    Details: {}", details);
            }
            if let Some(ref fix) = check.fix_suggestion {
                println!("    Fix: {}", fix);
            }
        }

        println!();
        p::info("Gas Estimation:");
        p::kv(
            "Estimated Gas",
            &format!(
                "{} stroops",
                validation.gas_estimation.estimated_gas_stroops
            ),
        );
        p::kv(
            "Estimated Cost",
            &format!("${:.6}", validation.gas_estimation.estimated_cost_usd),
        );
        p::kv("Confidence", &validation.gas_estimation.confidence_level);
        println!();
    }

    // Automated testing
    if let Some(ref testing) = result.automated_testing {
        p::header("Automated Testing");
        p::kv("Status", &testing.overall_status);
        p::kv("Coverage", &format!("{:.1}%", testing.coverage_percentage));
        p::kv("Passed", &testing.passed_tests.to_string());
        p::kv("Failed", &testing.failed_tests.to_string());
        p::kv("Skipped", &testing.skipped_tests.to_string());

        println!();
        p::info("Test Results:");
        for test in &testing.test_results {
            let status_color = if test.status == "pass" {
                "green"
            } else {
                "red"
            };
            println!(
                "  [{}] {} ({}ms)",
                test.status.as_str().color(status_color),
                test.test_name,
                test.duration_ms
            );
            if let Some(ref output) = test.output {
                println!("    Output: {}", output);
            }
            if let Some(ref error) = test.error_message {
                println!("    Error: {}", error);
            }
        }
        println!();
    }

    // Deployment execution
    if let Some(ref deployment) = result.deployment_execution {
        p::header("Deployment Execution");
        p::kv("Status", &deployment.status);
        p::kv("Deployment ID", &deployment.deployment_id);
        if let Some(ref contract_id) = deployment.contract_id {
            p::kv("Contract ID", contract_id);
        }
        if let Some(ref tx_hash) = deployment.transaction_hash {
            p::kv("Transaction Hash", tx_hash);
        }
        p::kv("Gas Used", &format!("{} stroops", deployment.gas_used));
        p::kv("Cost", &format!("${:.6}", deployment.cost_usd));
        p::kv(
            "Deployment Time",
            &format!("{} ms", deployment.deployment_time_ms),
        );
        if let Some(ref error) = deployment.error_message {
            p::kv("Error", error);
        }
        println!();
    }

    // Post-deployment verification
    if let Some(ref verification) = result.post_deployment_verification {
        p::header("Post-deployment Verification");
        p::kv("Status", &verification.overall_status);

        println!();
        p::info("Verification Checks:");
        for check in &verification.verifications {
            let status_color = if check.status == "pass" {
                "green"
            } else {
                "red"
            };
            println!(
                "  [{}] {}",
                check.status.as_str().color(status_color),
                check.check_name
            );
            println!("    Description: {}", check.description);
            if let Some(ref details) = check.details {
                println!("    Details: {}", details);
            }
        }

        println!();
        p::info("Contract Inspection:");
        p::kv("Contract ID", &verification.contract_inspection.contract_id);
        p::kv("WASM Hash", &verification.contract_inspection.wasm_hash);
        p::kv(
            "Storage Entries",
            &verification.contract_inspection.storage_entries.to_string(),
        );
        p::kv("Status", &verification.contract_inspection.status);
        println!();
    }

    // Rollback automation
    if let Some(ref rollback) = result.rollback_automation {
        p::header("Rollback Automation");
        p::kv("Status", &rollback.status);
        p::kv("Success", if rollback.success { "yes" } else { "no" });
        p::kv("Reason", &rollback.reason);
        if let Some(ref prev_contract) = rollback.previous_contract_id {
            p::kv("Previous Contract", prev_contract);
        }
        if let Some(ref tx_hash) = rollback.rollback_transaction_hash {
            p::kv("Rollback Transaction", tx_hash);
        }
        println!();
    }

    // Monitoring setup
    if let Some(ref monitoring) = result.monitoring_setup {
        p::header("Monitoring Setup");
        p::kv("Status", &monitoring.status);

        println!();
        p::info("Monitoring Configuration:");
        p::kv("Contract ID", &monitoring.monitoring_config.contract_id);
        println!("  Events to Monitor:");
        for event in &monitoring.monitoring_config.events_to_monitor {
            println!("    • {}", event);
        }
        println!("  Alert Thresholds:");
        println!(
            "    • Balance: {} XLM",
            monitoring
                .monitoring_config
                .alert_thresholds
                .balance_threshold_xlm
        );
        println!(
            "    • Error Rate: {:.1}%",
            monitoring
                .monitoring_config
                .alert_thresholds
                .error_rate_threshold
                * 100.0
        );
        println!(
            "    • Gas Cost: {} stroops",
            monitoring
                .monitoring_config
                .alert_thresholds
                .gas_cost_threshold_stroops
        );
        println!("  Notification Channels:");
        for channel in &monitoring.monitoring_config.notification_channels {
            println!("    • {}", channel);
        }

        println!();
        p::info("Alerts Configured:");
        for alert in &monitoring.alerts_configured {
            println!("  • {}", alert);
        }
        println!();
    }
}

fn handle_history(limit: usize) -> Result<()> {
    p::header("Deployment Automation History");
    p::separator();

    p::info("Automation history feature coming soon");
    p::info("Automation results are currently stored in-memory only");

    p::separator();
    Ok(())
}

fn handle_config(
    level: Option<String>,
    pre_deployment_checks: Option<bool>,
    automated_testing: Option<bool>,
    post_deployment_verification: Option<bool>,
    rollback_automation: Option<bool>,
    monitoring_setup: Option<bool>,
) -> Result<()> {
    p::header("Deployment Automation Configuration");
    p::separator();

    if let Some(lvl) = level {
        p::kv("Default Automation Level", &lvl);
    }
    if let Some(pre_check) = pre_deployment_checks {
        p::kv(
            "Pre-deployment Checks",
            if pre_check { "enabled" } else { "disabled" },
        );
    }
    if let Some(auto_test) = automated_testing {
        p::kv(
            "Automated Testing",
            if auto_test { "enabled" } else { "disabled" },
        );
    }
    if let Some(post_verify) = post_deployment_verification {
        p::kv(
            "Post-deployment Verification",
            if post_verify { "enabled" } else { "disabled" },
        );
    }
    if let Some(rollback) = rollback_automation {
        p::kv(
            "Rollback Automation",
            if rollback { "enabled" } else { "disabled" },
        );
    }
    if let Some(monitor) = monitoring_setup {
        p::kv(
            "Monitoring Setup",
            if monitor { "enabled" } else { "disabled" },
        );
    }

    p::info("Configuration saved to ~/.starforge/config.toml");

    p::separator();
    Ok(())
}
