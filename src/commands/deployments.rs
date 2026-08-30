use crate::utils::deploy_history::{
    get_record, last_successful, load_history, set_verified, update_status, DeployStatus,
};
use crate::utils::deployment_monitor;
use crate::utils::deployment_monitoring_service::{
    render_monitoring_dashboard, DeploymentAlertEngine, DeploymentHealthChecker, DeploymentTracker,
};
use crate::utils::deployment_verify::{
    generate_ci_snippet, load_report, save_report, DeploymentVerifier,
};
use crate::utils::print as p;
use crate::utils::{config, horizon};
use anyhow::Result;
use clap::{Args, Subcommand};
use colored::*;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum DeploymentsCommands {
    /// List all recorded deployments
    History(HistoryArgs),
    /// Roll back to a previous deployment version
    Rollback(RollbackArgs),
    /// Verify that a deployment is live on-chain
    Verify(VerifyArgs),
    /// Show an overview dashboard of recent deployments
    Dashboard(DashboardArgs),
    /// Approve a pending deployment
    Approve(ApproveArgs),
    /// Show a saved deployment verification report
    Report(ReportArgs),
    /// Generate CI snippet for automated deployment verification
    Ci(CiArgs),
    /// Run deployment monitoring service, real-time status updates, health checks, and alerting
    Monitor(MonitorArgs),
}

#[derive(Args)]
pub struct HistoryArgs {
    /// Filter by network
    #[arg(long)]
    pub network: Option<String>,
    /// Show only successful deployments
    #[arg(long)]
    pub success_only: bool,
    /// Maximum number of records to show
    #[arg(long, default_value = "20")]
    pub limit: usize,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct RollbackArgs {
    /// Deployment ID to roll back to (prefix match supported)
    #[arg(long)]
    pub id: String,
    /// Network to use
    #[arg(long, default_value = "testnet")]
    pub network: String,
    /// Wallet name to use for signing
    #[arg(long)]
    pub wallet: Option<String>,
    /// Skip confirmation prompt
    #[arg(long)]
    pub yes: bool,
}

#[derive(Args)]
pub struct VerifyArgs {
    /// Deployment ID to verify (prefix match supported)
    #[arg(long)]
    pub id: String,
    /// Save verification result to history
    #[arg(long)]
    pub save: bool,
    /// Save detailed verification report to disk
    #[arg(long)]
    pub report: bool,
    /// Output report as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct ReportArgs {
    /// Deployment ID to show report for
    #[arg(long)]
    pub id: String,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct CiArgs {
    /// Deployment ID to embed in CI snippet
    #[arg(long)]
    pub id: String,
    /// Network context
    #[arg(long, default_value = "testnet")]
    pub network: String,
    /// CI platform
    #[arg(long, default_value = "github", value_parser = ["github", "gitlab"])]
    pub platform: String,
}

#[derive(Args)]
pub struct DashboardArgs {
    /// Network to filter by
    #[arg(long)]
    pub network: Option<String>,
    /// Number of recent deployments to show per network
    #[arg(long, default_value = "5")]
    pub recent: usize,
}

#[derive(Args)]
pub struct MonitorArgs {
    /// Network to analyze
    #[arg(long, default_value = "testnet")]
    pub network: String,
    /// Optional contract ID to scope the analysis
    #[arg(long)]
    pub contract: Option<String>,
    /// Optional wallet name to scope the health check
    #[arg(long)]
    pub wallet: Option<String>,
    /// Render live monitoring dashboard with real-time status and health check matrix
    #[arg(long)]
    pub dashboard: bool,
    /// Enable health check verification
    #[arg(long)]
    pub health_check: bool,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct ApproveArgs {
    /// Deployment ID to approve
    #[arg(long)]
    pub id: String,
    /// Approver name or wallet
    #[arg(long)]
    pub approver: String,
}

pub async fn handle(cmd: DeploymentsCommands) -> Result<()> {
    match cmd {
        DeploymentsCommands::History(args) => handle_history(args),
        DeploymentsCommands::Rollback(args) => handle_rollback(args),
        DeploymentsCommands::Verify(args) => handle_verify(args).await,
        DeploymentsCommands::Dashboard(args) => handle_dashboard(args),
        DeploymentsCommands::Approve(args) => handle_approve(args),
        DeploymentsCommands::Report(args) => handle_report(args),
        DeploymentsCommands::Ci(args) => handle_ci(args),
        DeploymentsCommands::Monitor(args) => handle_monitor(args),
    }
}

fn handle_history(args: HistoryArgs) -> Result<()> {
    p::header("Deployment History");

    let mut records = load_history()?;

    if let Some(ref net) = args.network {
        records.retain(|r| &r.network == net);
    }
    if args.success_only {
        records.retain(|r| r.status == DeployStatus::Success);
    }

    if records.is_empty() {
        p::info("No deployment records found.");
        p::info("Deployments are recorded when `starforge deploy --execute` is used.");
        return Ok(());
    }

    let shown: Vec<_> = records.iter().rev().take(args.limit).collect();

    if args.json {
        println!("{}", serde_json::to_string_pretty(&shown)?);
        return Ok(());
    }

    p::separator();
    println!(
        "  {:<10}  {:<10}  {:<10}  {:<12}  {:<16}  {}",
        "ID".dimmed(),
        "Network".dimmed(),
        "Status".dimmed(),
        "Wallet".dimmed(),
        "Timestamp".dimmed(),
        "Contract / WASM".dimmed(),
    );
    println!("  {}", "─".repeat(90).dimmed());

    for rec in &shown {
        let status_colored = match rec.status {
            DeployStatus::Success => rec.status.to_string().green().to_string(),
            DeployStatus::Failed => rec.status.to_string().red().to_string(),
            DeployStatus::RolledBack => rec.status.to_string().yellow().to_string(),
            DeployStatus::Pending => rec.status.to_string().cyan().to_string(),
        };
        let contract = rec
            .contract_id
            .as_deref()
            .unwrap_or(&rec.wasm_path)
            .chars()
            .take(28)
            .collect::<String>();

        println!(
            "  {:<10}  {:<10}  {:<10}  {:<12}  {:<16}  {}",
            rec.id[..8.min(rec.id.len())].cyan(),
            rec.network.as_str(),
            status_colored,
            rec.wallet.chars().take(10).collect::<String>(),
            rec.timestamp.get(..16).unwrap_or(&rec.timestamp),
            contract,
        );
    }
    p::separator();
    println!("  Showing {} of {} records.", shown.len(), records.len());
    Ok(())
}

fn handle_rollback(args: RollbackArgs) -> Result<()> {
    p::header("Deployment Rollback");
    config::validate_network(&args.network)?;

    let record = get_record(&args.id)?
        .ok_or_else(|| anyhow::anyhow!("No deployment found with ID prefix '{}'", args.id))?;

    if record.status != DeployStatus::Success {
        anyhow::bail!(
            "Deployment '{}' has status '{}'. Only successful deployments can be rolled back to.",
            record.id,
            record.status
        );
    }

    p::separator();
    p::kv("Deployment ID", &record.id);
    p::kv("WASM hash", &record.wasm_hash);
    p::kv("Network", &record.network);
    p::kv(
        "Contract",
        record.contract_id.as_deref().unwrap_or("(not recorded)"),
    );
    p::kv("Originally deployed", &record.timestamp);
    println!();

    let cfg = config::load()?;
    let wallet = if let Some(name) = &args.wallet {
        cfg.wallets
            .iter()
            .find(|w| &w.name == name)
            .ok_or_else(|| anyhow::anyhow!("Wallet '{}' not found", name))?
    } else if !cfg.wallets.is_empty() {
        &cfg.wallets[0]
    } else {
        anyhow::bail!("No wallets found. Create one with `starforge wallet create <name>`")
    };

    if !args.yes {
        use dialoguer::Confirm;
        let ok = Confirm::new()
            .with_prompt(format!(
                "Roll back to deployment '{}'?",
                &record.id[..8.min(record.id.len())]
            ))
            .default(false)
            .interact()
            .unwrap_or(false);
        if !ok {
            p::info("Rollback cancelled.");
            return Ok(());
        }
    }

    let contract_id = record
        .contract_id
        .as_deref()
        .unwrap_or("CXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX");

    // Record the rollback in history: appends a rollback record linked to the
    // target and marks any later successful deployment on this network as
    // rolled-back, so `deployments history`/`dashboard` reflect the revert.
    let rollback_id = crate::utils::deploy_history::record_rollback(&record, &wallet.name)?;

    p::separator();
    p::success("Rollback recorded in deployment history.");
    p::kv("Rollback record", &rollback_id[..8.min(rollback_id.len())]);
    p::kv("Reverted to", &record.id[..8.min(record.id.len())]);
    println!();
    p::info("Run this to revert the contract on-chain:");
    println!();
    println!(
        "  {}",
        format!(
            "stellar contract invoke \\\n    --id {} \\\n    --source {} \\\n    --network {} \\\n    -- upgrade --new-wasm-hash {}",
            contract_id, wallet.public_key, args.network, record.wasm_hash
        )
        .cyan()
    );
    p::separator();
    Ok(())
}

async fn handle_verify(args: VerifyArgs) -> Result<()> {
    p::header("Deployment Verification");

    let record = get_record(&args.id)?
        .ok_or_else(|| anyhow::anyhow!("No deployment found with ID prefix '{}'", args.id))?;

    p::kv("Deployment ID", &record.id);
    p::kv("Network", &record.network);
    p::kv("WASM hash", &record.wasm_hash);
    if let Some(ref cid) = record.contract_id {
        p::kv("Contract ID", cid);
    }
    println!();

    let wasm_path = PathBuf::from(&record.wasm_path);
    let verifier = DeploymentVerifier::new(record.clone()).with_wasm_file(&wasm_path)?;
    let mut report = verifier.verify_all().await?;

    // Wallet activity check
    let cfg = config::load()?;
    if let Some(wallet) = cfg.wallets.iter().find(|w| w.name == record.wallet) {
        match horizon::fetch_account(&wallet.public_key, &record.network).await {
            Ok(_) => {
                report
                    .checks
                    .push(crate::utils::deployment_verify::VerificationCheck {
                        name: "wallet_active".to_string(),
                        category: "functionality".to_string(),
                        status: crate::utils::deployment_verify::CheckStatus::Passed,
                        detail: format!("Wallet '{}' is active on-chain", record.wallet),
                    });
            }
            Err(e) => {
                report
                    .checks
                    .push(crate::utils::deployment_verify::VerificationCheck {
                        name: "wallet_active".to_string(),
                        category: "functionality".to_string(),
                        status: crate::utils::deployment_verify::CheckStatus::Warning,
                        detail: format!("Could not verify wallet: {}", e),
                    });
            }
        }
    }

    report.passed = report
        .checks
        .iter()
        .all(|c| c.status != crate::utils::deployment_verify::CheckStatus::Failed);

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        for check in &report.checks {
            let icon = match check.status {
                crate::utils::deployment_verify::CheckStatus::Passed => "✓".green(),
                crate::utils::deployment_verify::CheckStatus::Failed => "✗".red(),
                crate::utils::deployment_verify::CheckStatus::Warning => "!".yellow(),
                crate::utils::deployment_verify::CheckStatus::Skipped => "–".dimmed(),
            };
            println!(
                "  {} [{}] {} — {}",
                icon, check.category, check.name, check.detail
            );
        }
        println!();
        let passed_count = report
            .checks
            .iter()
            .filter(|c| c.status == crate::utils::deployment_verify::CheckStatus::Passed)
            .count();
        p::kv(
            "Checks passed",
            &format!("{}/{}", passed_count, report.checks.len()),
        );
    }

    if args.report || args.save {
        let path = save_report(&report)?;
        if !args.json {
            p::success(&format!("Verification report saved to {}", path.display()));
        }
    }

    if args.save {
        set_verified(&record.id, report.passed)?;
    }

    if report.passed {
        if !args.json {
            p::success("Deployment verification complete");
        }
    } else if !args.json {
        p::warn("Some verification checks failed");
    }
    Ok(())
}

fn handle_report(args: ReportArgs) -> Result<()> {
    p::header("Deployment Verification Report");
    let report = load_report(&args.id)?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    p::kv("Deployment ID", &report.deployment_id);
    p::kv("Network", &report.network);
    p::kv("Timestamp", &report.timestamp);
    p::kv("Passed", &report.passed.to_string());
    p::kv("Expected WASM hash", &report.wasm_hash_expected);
    if let Some(ref hash) = report.wasm_hash_onchain {
        p::kv("On-chain WASM hash", hash);
    }
    println!();
    for check in &report.checks {
        println!("  [{}] {} — {}", check.category, check.name, check.status);
    }
    Ok(())
}

fn handle_monitor(args: MonitorArgs) -> Result<()> {
    p::header("Deployment Monitoring Service");
    config::validate_network(&args.network)?;

    let report = deployment_monitor::analyze_deployments(&args.network, args.contract.as_deref())?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    if args.dashboard || args.health_check {
        let tracker = DeploymentTracker::new();

        // Populate tracker with history for visual status
        let records = load_history().unwrap_or_default();
        for (idx, r) in records.iter().enumerate().take(5) {
            let tr_id = format!("dep-{}", &r.id[..8.min(r.id.len())]);
            tracker.start_tracking(&tr_id, &r.network, &r.wallet);
            if r.status == DeployStatus::Success {
                tracker.mark_completed(
                    &tr_id,
                    r.contract_id.as_deref().unwrap_or("C000"),
                    None,
                    r.duration_ms.unwrap_or(1000),
                );
            } else if r.status == DeployStatus::Failed {
                tracker.mark_failed(&tr_id, "Deployment failed during invocation");
            }
        }

        let tracks = tracker.get_active_tracks();
        let health_checks = DeploymentHealthChecker::check_network_health(
            &args.network,
            args.contract.as_deref(),
            args.wallet.as_deref(),
        );
        let service_alerts = DeploymentAlertEngine::generate_alerts(&tracks, &health_checks);

        let dashboard_view =
            render_monitoring_dashboard(&tracks, &health_checks, &service_alerts, &args.network);
        println!("{}", dashboard_view);
        return Ok(());
    }

    p::separator();
    p::kv("Network", &report.network);
    p::kv("Deployments", &report.total_deployments.to_string());
    p::kv("Success rate", &format!("{:.1}%", report.success_rate));
    p::kv("Error rate", &format!("{:.1}%", report.error_rate));
    p::kv("Avg duration", &format!("{:.0} ms", report.avg_duration_ms));
    p::kv("Tracked wallets", &report.unique_wallets.to_string());
    p::kv("Tracked contracts", &report.unique_contracts.to_string());
    println!();

    p::info("Alerts");
    for alert in &report.alerts {
        println!(
            "  [{}] {} — {}",
            alert.severity.to_uppercase(),
            alert.title,
            alert.detail
        );
        println!("    → {}", alert.recommendation);
    }

    println!();
    p::info("Predictions");
    for prediction in &report.predictions {
        println!(
            "  [{}] {} — {}",
            prediction.confidence, prediction.title, prediction.detail
        );
        println!("    → {}", prediction.recommended_action);
    }

    println!();
    p::info("Recent trend indicators");
    for trend in &report.history {
        println!(
            "  {}: {:.0} ms ({})",
            trend.label, trend.value, trend.direction
        );
    }

    p::separator();
    Ok(())
}

fn handle_ci(args: CiArgs) -> Result<()> {
    p::header("Deployment Verification CI");
    let snippet = generate_ci_snippet(&args.id, &args.network);
    match args.platform.as_str() {
        "github" => {
            println!("Add to .github/workflows/deploy-verify.yml:\n");
            println!("{}", snippet);
        }
        "gitlab" => {
            println!("Add to .gitlab-ci.yml:\n");
            println!(
                "verify_deployment:\n  script:\n    - starforge deployments verify --id {} --save --report\n",
                args.id
            );
        }
        _ => println!("{}", snippet),
    }
    Ok(())
}

fn handle_dashboard(args: DashboardArgs) -> Result<()> {
    p::header("Deployment Dashboard");

    let records = load_history()?;

    if records.is_empty() {
        p::info("No deployments recorded yet.");
        p::info("Run `starforge deploy --execute` to record a deployment.");
        return Ok(());
    }

    let networks: Vec<String> = {
        let mut seen = std::collections::HashSet::new();
        let mut nets = Vec::new();
        for r in &records {
            if seen.insert(r.network.clone())
                && args.network.as_ref().map_or(true, |n| n == &r.network)
            {
                nets.push(r.network.clone());
            }
        }
        nets
    };

    let total = records.len();
    let success = records
        .iter()
        .filter(|r| r.status == DeployStatus::Success)
        .count();
    let failed = records
        .iter()
        .filter(|r| r.status == DeployStatus::Failed)
        .count();
    let pending = records
        .iter()
        .filter(|r| r.status == DeployStatus::Pending)
        .count();

    p::separator();
    p::kv("Total deployments", &total.to_string());
    p::kv("Successful", &success.to_string());
    p::kv("Failed", &failed.to_string());
    p::kv("Pending approval", &pending.to_string());
    p::kv(
        "Success rate",
        &format!(
            "{:.1}%",
            if total > 0 {
                success as f64 / total as f64 * 100.0
            } else {
                0.0
            }
        ),
    );

    for net in &networks {
        println!();
        println!("  {} {}", "▶".cyan(), net.bright_white().bold());
        let recent: Vec<_> = records
            .iter()
            .rev()
            .filter(|r| &r.network == net)
            .take(args.recent)
            .collect();

        for rec in recent {
            let status_colored = match rec.status {
                DeployStatus::Success => "✓".green().to_string(),
                DeployStatus::Failed => "✗".red().to_string(),
                DeployStatus::RolledBack => "↩".yellow().to_string(),
                DeployStatus::Pending => "…".cyan().to_string(),
            };
            println!(
                "    {} {} | {} | {}",
                status_colored,
                rec.id[..8.min(rec.id.len())].dimmed(),
                rec.timestamp.get(..16).unwrap_or(&rec.timestamp).dimmed(),
                rec.contract_id
                    .as_deref()
                    .unwrap_or(&rec.wasm_path)
                    .chars()
                    .take(40)
                    .collect::<String>()
                    .white(),
            );
        }
    }

    if let Ok(Some(last)) = last_successful(networks.first().map_or("testnet", |n| n.as_str())) {
        println!();
        p::kv(
            "Last successful",
            &format!(
                "{} on {}",
                &last.id[..8.min(last.id.len())],
                last.timestamp.get(..16).unwrap_or(&last.timestamp)
            ),
        );
    }

    p::separator();
    Ok(())
}

fn handle_approve(args: ApproveArgs) -> Result<()> {
    p::header("Deployment Approval");

    let mut records = load_history()?;
    let rec = records
        .iter_mut()
        .find(|r| r.id == args.id || r.id.starts_with(&args.id))
        .ok_or_else(|| anyhow::anyhow!("No deployment found with ID prefix '{}'", args.id))?;

    if rec.status != DeployStatus::Pending {
        anyhow::bail!(
            "Deployment '{}' is not pending approval (status: {})",
            rec.id,
            rec.status
        );
    }

    rec.approved_by = Some(args.approver.clone());
    rec.status = DeployStatus::Success;

    let id = rec.id.clone();
    crate::utils::deploy_history::save_history(&records)?;

    p::kv("Deployment ID", &id);
    p::kv("Approved by", &args.approver);
    p::success("Deployment approved and marked as successful");
    Ok(())
}
