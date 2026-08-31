use crate::utils::bridge::{
    load_config, load_transfers,
    monitoring::BridgeMonitor,
    providers::{self, BridgeTransferRequest, TransferStatus},
    record_transfer,
    routes::RouteRegistry,
    save_config,
    security::SecurityVerifier,
    state::StateSynchronizer,
    BridgeTransferRecord,
};
use crate::utils::print as p;
use anyhow::Result;
use chrono::Utc;
use clap::{Args, Subcommand};
use colored::*;

#[derive(Subcommand)]
pub enum BridgeCommands {
    /// Initiate a cross-chain transfer
    Transfer(TransferArgs),
    /// Check status of a bridge transfer
    Status(StatusArgs),
    /// List available bridge routes
    Routes(RoutesArgs),
    /// Configure bridge settings
    Configure(ConfigureArgs),
    /// Synchronize cross-chain state
    Sync(SyncArgs),
    /// Run security verification on a transfer
    Verify(VerifyArgs),
    /// Show bridge monitoring alerts
    Monitor(MonitorArgs),
    /// List transfer history
    History(HistoryArgs),
}

#[derive(Args)]
pub struct TransferArgs {
    #[arg(long)]
    pub source: String,
    #[arg(long)]
    pub dest: String,
    #[arg(long, default_value = "USDC")]
    pub asset: String,
    #[arg(long)]
    pub amount: u64,
    #[arg(long)]
    pub sender: String,
    #[arg(long)]
    pub recipient: String,
    #[arg(long)]
    pub provider: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct StatusArgs {
    #[arg(long)]
    pub id: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct RoutesArgs {
    #[arg(long)]
    pub source: Option<String>,
    #[arg(long)]
    pub dest: Option<String>,
    #[arg(long)]
    pub asset: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct ConfigureArgs {
    #[arg(long)]
    pub show: bool,
    #[arg(long)]
    pub enable: Option<bool>,
    #[arg(long)]
    pub default_provider: Option<String>,
    #[arg(long)]
    pub max_amount: Option<u64>,
    #[arg(long)]
    pub require_proof: Option<bool>,
}

#[derive(Args)]
pub struct SyncArgs {
    #[arg(long, default_value = "stellar-testnet")]
    pub source: String,
    #[arg(long, default_value = "ethereum-sepolia")]
    pub dest: String,
    #[arg(long, default_value = "1000")]
    pub source_ledger: u32,
    #[arg(long, default_value = "5000")]
    pub dest_ledger: u32,
}

#[derive(Args)]
pub struct VerifyArgs {
    #[arg(long)]
    pub source: String,
    #[arg(long)]
    pub dest: String,
    #[arg(long, default_value = "USDC")]
    pub asset: String,
    #[arg(long)]
    pub amount: u64,
    #[arg(long)]
    pub sender: String,
    #[arg(long)]
    pub recipient: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct MonitorArgs {
    #[arg(long)]
    pub acknowledge: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct HistoryArgs {
    #[arg(long)]
    pub limit: usize,
    #[arg(long)]
    pub json: bool,
}

pub async fn handle(cmd: BridgeCommands) -> Result<()> {
    match cmd {
        BridgeCommands::Transfer(args) => handle_transfer(args),
        BridgeCommands::Status(args) => handle_status(args),
        BridgeCommands::Routes(args) => handle_routes(args),
        BridgeCommands::Configure(args) => handle_configure(args),
        BridgeCommands::Sync(args) => handle_sync(args),
        BridgeCommands::Verify(args) => handle_verify(args),
        BridgeCommands::Monitor(args) => handle_monitor(args),
        BridgeCommands::History(args) => handle_history(args),
    }
}

fn handle_transfer(args: TransferArgs) -> Result<()> {
    p::header("Cross-Chain Bridge Transfer");

    let config = load_config()?;
    if !config.enabled {
        anyhow::bail!("Bridge is disabled. Run `starforge bridge configure --enable true`");
    }

    let request = BridgeTransferRequest {
        source_network: args.source.clone(),
        dest_network: args.dest.clone(),
        asset: args.asset.clone(),
        amount: args.amount,
        sender: args.sender.clone(),
        recipient: args.recipient.clone(),
    };

    let verifier = SecurityVerifier::new(config.clone());
    let security = verifier.verify_transfer(&request);
    if !security.passed {
        anyhow::bail!("Security verification failed. Run `starforge bridge verify` for details.");
    }

    let registry = RouteRegistry::new(config.routes.clone());
    let route = registry
        .best_route(&args.source, &args.dest, &args.asset)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No route found for {} → {} ({})",
                args.source,
                args.dest,
                args.asset
            )
        })?;

    let provider_name = args.provider.as_deref().unwrap_or(&route.provider);
    let provider = config
        .providers
        .iter()
        .find(|p| p.name == provider_name && p.enabled)
        .ok_or_else(|| anyhow::anyhow!("Provider '{}' not found or disabled", provider_name))?;

    p::kv("Route", &route.id);
    p::kv("Provider", &provider.name);
    p::kv("Fee", &format!("{} bps", route.fee_bps));
    p::kv("Est. time", &format!("{}s", route.estimated_time_secs));

    let result = providers::initiate_transfer(provider, &request)?;

    let record = BridgeTransferRecord {
        id: result.transfer_id.clone(),
        source_network: args.source,
        dest_network: args.dest,
        asset: args.asset,
        amount: args.amount,
        sender: args.sender,
        recipient: args.recipient,
        status: result.status.to_string(),
        tx_hash_source: result.source_tx_hash.clone(),
        tx_hash_dest: result.dest_tx_hash.clone(),
        created_at: Utc::now().to_rfc3339(),
        completed_at: None,
        security_verified: true,
    };
    record_transfer(record)?;

    let mut sync = StateSynchronizer::load().unwrap_or_else(|_| StateSynchronizer::new());
    sync.mark_pending(&result.transfer_id);
    sync.save()?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    p::success("Transfer initiated");
    p::kv("Transfer ID", &result.transfer_id);
    if let Some(ref tx) = result.source_tx_hash {
        p::kv("Source TX", tx);
    }
    p::kv("Status", &result.status.to_string());
    Ok(())
}

fn handle_status(args: StatusArgs) -> Result<()> {
    p::header("Bridge Transfer Status");

    let transfers = load_transfers()?;
    let record = transfers
        .iter()
        .find(|t| t.id == args.id || t.id.starts_with(&args.id))
        .ok_or_else(|| anyhow::anyhow!("Transfer '{}' not found", args.id))?;

    let config = load_config()?;
    let provider = config
        .providers
        .first()
        .ok_or_else(|| anyhow::anyhow!("No providers configured"))?;
    let status = providers::poll_transfer_status(provider, &record.id)?;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "record": record,
                "live_status": status.to_string(),
            }))?
        );
        return Ok(());
    }

    p::kv("Transfer ID", &record.id);
    p::kv("Status", &status.to_string());
    p::kv(
        "Source",
        &format!("{} → {}", record.source_network, record.asset),
    );
    p::kv("Dest", &record.dest_network);
    p::kv("Amount", &record.amount.to_string());
    if let Some(ref tx) = record.tx_hash_source {
        p::kv("Source TX", tx);
    }
    if let Some(ref tx) = record.tx_hash_dest {
        p::kv("Dest TX", tx);
    }
    p::kv("Security verified", &record.security_verified.to_string());

    if status == TransferStatus::Completed {
        p::success("Transfer completed");
    } else {
        p::info(&format!("Transfer in progress: {}", status));
    }
    Ok(())
}

fn handle_routes(args: RoutesArgs) -> Result<()> {
    p::header("Bridge Routes");

    let config = load_config()?;
    let registry = RouteRegistry::new(config.routes);

    let routes = match (&args.source, &args.dest) {
        (Some(src), Some(dst)) => registry.find(src, dst, args.asset.as_deref()),
        _ => registry.all().iter().collect(),
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&routes)?);
        return Ok(());
    }

    if routes.is_empty() {
        p::info("No routes match the given filters.");
        return Ok(());
    }

    for route in routes {
        println!(
            "  {} {} → {} ({}) via {}",
            "•".cyan(),
            route.source_network,
            route.dest_network,
            route.asset,
            route.provider.bright_white()
        );
        println!(
            "    fee: {} bps | min: {} | max: {} | ~{}s",
            route.fee_bps, route.min_amount, route.max_amount, route.estimated_time_secs
        );
    }
    Ok(())
}

fn handle_configure(args: ConfigureArgs) -> Result<()> {
    let mut config = load_config()?;

    if args.show
        || (args.enable.is_none()
            && args.default_provider.is_none()
            && args.max_amount.is_none()
            && args.require_proof.is_none())
    {
        p::header("Bridge Configuration");
        p::kv("Enabled", &config.enabled.to_string());
        p::kv("Default provider", &config.default_provider);
        p::kv("Providers", &config.providers.len().to_string());
        p::kv("Routes", &config.routes.len().to_string());
        p::kv(
            "Max transfer",
            &config.security.max_transfer_amount.to_string(),
        );
        p::kv(
            "Require proof",
            &config.security.require_proof_verification.to_string(),
        );
        p::kv("Monitoring", &config.monitoring.enabled.to_string());
        return Ok(());
    }

    if let Some(enabled) = args.enable {
        config.enabled = enabled;
    }
    if let Some(ref provider) = args.default_provider {
        config.default_provider = provider.clone();
    }
    if let Some(max) = args.max_amount {
        config.security.max_transfer_amount = max;
    }
    if let Some(require) = args.require_proof {
        config.security.require_proof_verification = require;
    }

    save_config(&config)?;
    p::success("Bridge configuration saved");
    Ok(())
}

fn handle_sync(args: SyncArgs) -> Result<()> {
    p::header("Bridge State Synchronization");

    let mut sync = StateSynchronizer::load().unwrap_or_else(|_| StateSynchronizer::new());
    sync.sync(
        &args.source,
        &args.dest,
        args.source_ledger,
        args.dest_ledger,
    );
    sync.save()?;

    p::kv("Source ledger", &args.source_ledger.to_string());
    p::kv("Dest ledger", &args.dest_ledger.to_string());
    p::kv("In sync", &sync.is_in_sync(1000).to_string());
    p::kv("Pending", &sync.state().pending_transfers.len().to_string());
    p::kv(
        "Completed",
        &sync.state().completed_transfers.len().to_string(),
    );
    p::success("State synchronized");
    Ok(())
}

fn handle_verify(args: VerifyArgs) -> Result<()> {
    p::header("Bridge Security Verification");

    let config = load_config()?;
    let request = BridgeTransferRequest {
        source_network: args.source,
        dest_network: args.dest,
        asset: args.asset,
        amount: args.amount,
        sender: args.sender,
        recipient: args.recipient,
    };

    let verifier = SecurityVerifier::new(config);
    let report = verifier.verify_transfer(&request);

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    for check in &report.checks {
        let icon = match check.result {
            crate::utils::bridge::security::SecurityCheck::Passed => "✓".green(),
            crate::utils::bridge::security::SecurityCheck::Failed => "✗".red(),
            crate::utils::bridge::security::SecurityCheck::Warning => "!".yellow(),
            crate::utils::bridge::security::SecurityCheck::Skipped => "–".dimmed(),
        };
        println!("  {} {} — {}", icon, check.name, check.detail);
    }

    if report.passed {
        p::success("Security verification passed");
    } else {
        p::warn("Security verification failed");
    }
    Ok(())
}

fn handle_monitor(args: MonitorArgs) -> Result<()> {
    let config = load_config()?;
    let mut monitor = BridgeMonitor::new(config);
    monitor.load_alerts()?;

    if let Some(ref alert_id) = args.acknowledge {
        if monitor.acknowledge(alert_id) {
            monitor.save_alerts()?;
            p::success(&format!("Alert {} acknowledged", alert_id));
        } else {
            anyhow::bail!("Alert '{}' not found", alert_id);
        }
        return Ok(());
    }

    p::header("Bridge Monitoring");
    p::kv(
        "Unacknowledged alerts",
        &monitor.unacknowledged_count().to_string(),
    );

    if args.json {
        println!("{}", serde_json::to_string_pretty(monitor.alerts())?);
        return Ok(());
    }

    if monitor.alerts().is_empty() {
        p::info("No alerts. Monitoring is active.");
    } else {
        for alert in monitor.alerts() {
            let ack = if alert.acknowledged { "✓" } else { " " };
            println!(
                "  [{}] {} {} — {}",
                ack,
                alert.severity.bright_white(),
                alert.id.chars().take(8).collect::<String>().dimmed(),
                alert.message
            );
        }
    }
    Ok(())
}

fn handle_history(args: HistoryArgs) -> Result<()> {
    p::header("Bridge Transfer History");

    let transfers = load_transfers()?;
    let limit = if args.limit == 0 { 20 } else { args.limit };
    let shown: Vec<_> = transfers.iter().rev().take(limit).collect();

    if args.json {
        println!("{}", serde_json::to_string_pretty(&shown)?);
        return Ok(());
    }

    if shown.is_empty() {
        p::info("No transfers recorded yet.");
        return Ok(());
    }

    for t in shown {
        println!(
            "  {} {} | {} → {} | {} {} | {}",
            if t.status == "completed" {
                "✓".green()
            } else {
                "…".yellow()
            },
            t.id[..8.min(t.id.len())].dimmed(),
            t.source_network,
            t.dest_network,
            t.amount,
            t.asset,
            t.status
        );
    }
    Ok(())
}
