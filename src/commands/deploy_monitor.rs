use crate::utils::deploy_history::{DeployRecord, DeployStatus};
use crate::utils::deploy_monitor as dm;
use crate::utils::print as p;
use crate::utils::{deploy_history, horizon, notifications, soroban};
use anyhow::Result;
use clap::Subcommand;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Subcommand)]
pub enum DeployMonitorCommands {
    /// One-shot health check of tracked deployments
    Status(StatusArgs),
    /// Continuously monitor deployments and report status changes
    Watch(WatchArgs),
    /// Detailed health of a single deployment
    Health(HealthArgs),
    /// Aggregate monitoring dashboard
    Dashboard(DashboardArgs),
    /// Detect and list failed / stuck deployments
    Failures(FailuresArgs),
}

#[derive(clap::Args)]
pub struct StatusArgs {
    /// Only monitor deployments on this network
    #[arg(long)]
    pub network: Option<String>,
    /// Maximum number of deployments to check
    #[arg(long, default_value = "25")]
    pub limit: usize,
}

#[derive(clap::Args)]
pub struct WatchArgs {
    #[arg(long)]
    pub network: Option<String>,
    /// Seconds between monitoring cycles
    #[arg(long, default_value = "15")]
    pub interval: u64,
    /// Send notifications when deployments degrade or fail
    #[arg(long)]
    pub alert: bool,
}

#[derive(clap::Args)]
pub struct HealthArgs {
    /// Deployment id (or unique prefix)
    pub id: String,
}

#[derive(clap::Args)]
pub struct DashboardArgs {
    #[arg(long)]
    pub network: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::Args)]
pub struct FailuresArgs {
    /// Send a notification for each detected failure
    #[arg(long)]
    pub alert: bool,
}

pub async fn handle(cmd: DeployMonitorCommands) -> Result<()> {
    match cmd {
        DeployMonitorCommands::Status(args) => handle_status(args).await,
        DeployMonitorCommands::Watch(args) => handle_watch(args).await,
        DeployMonitorCommands::Health(args) => handle_health(args).await,
        DeployMonitorCommands::Dashboard(args) => handle_dashboard(args).await,
        DeployMonitorCommands::Failures(args) => handle_failures(args),
    }
}

/// Probe live infrastructure for a deployment, caching per-network reachability
/// for the duration of one monitoring pass.
async fn probe(record: &DeployRecord, net_cache: &mut HashMap<String, bool>) -> dm::LivenessProbe {
    let reachable = match net_cache.get(&record.network) {
        Some(v) => *v,
        None => {
            let v = horizon::check_network(&record.network).await;
            net_cache.insert(record.network.clone(), v);
            v
        }
    };

    let contract_live = match (&record.contract_id, reachable, &record.status) {
        (Some(cid), true, DeployStatus::Success) => Some(
            soroban::inspect_contract(cid, &record.network)
                .await
                .is_ok(),
        ),
        _ => None,
    };

    let age = dm::age_secs(&record.timestamp, dm::now_epoch()).unwrap_or(0);
    dm::LivenessProbe {
        network_reachable: reachable,
        contract_live,
        age_secs: age,
    }
}

async fn assess_all(records: &[DeployRecord]) -> Vec<dm::DeploymentHealth> {
    let mut net_cache: HashMap<String, bool> = HashMap::new();
    let mut out = Vec::with_capacity(records.len());
    for r in records {
        let probe = probe(r, &mut net_cache).await;
        out.push(dm::assess_health(r, &probe));
    }
    out
}

fn filtered_records(network: &Option<String>, limit: usize) -> Result<Vec<DeployRecord>> {
    let mut records = deploy_history::load_history()?;
    if let Some(net) = network {
        records.retain(|r| &r.network == net);
    }
    // Newest first.
    records.reverse();
    records.truncate(limit);
    Ok(records)
}

async fn handle_status(args: StatusArgs) -> Result<()> {
    let records = filtered_records(&args.network, args.limit)?;
    p::header("Deployment Health Status");
    if records.is_empty() {
        p::info("No deployments tracked yet.");
        return Ok(());
    }

    let healths = assess_all(&records).await;
    p::separator();
    for h in &healths {
        let badge = format!("{} {}", h.health.symbol(), h.health.label());
        p::kv_accent(
            &h.deployment_id.chars().take(8).collect::<String>(),
            &format!(
                "{} · {} · {}",
                badge,
                h.network,
                h.contract_id.clone().unwrap_or_else(|| "—".to_string())
            ),
        );
        for c in &h.checks {
            let mark = if c.passed { "✓" } else { "✗" };
            println!("      {} {}: {}", mark, c.name, c.detail);
        }
    }
    Ok(())
}

async fn handle_health(args: HealthArgs) -> Result<()> {
    let record = deploy_history::get_record(&args.id)?
        .ok_or_else(|| anyhow::anyhow!("No deployment matching '{}'", args.id))?;
    let mut net_cache = HashMap::new();
    let health = dm::assess_health(&record, &probe(&record, &mut net_cache).await);

    p::header("Deployment Health");
    p::kv("Deployment", &record.id);
    p::kv("Network", &record.network);
    p::kv("Deploy status", &record.status.to_string());
    if let Some(cid) = &record.contract_id {
        p::kv("Contract", cid);
    }
    p::kv_accent(
        "Health",
        &format!("{} {}", health.health.symbol(), health.health.label()),
    );
    p::separator();
    for c in &health.checks {
        let mark = if c.passed {
            "✓".to_string()
        } else {
            "✗".to_string()
        };
        p::kv(&format!("{} {}", mark, c.name), &c.detail);
    }
    Ok(())
}

async fn handle_dashboard(args: DashboardArgs) -> Result<()> {
    let records = filtered_records(&args.network, usize::MAX)?;
    let healths = assess_all(&records).await;
    let summary = dm::summarize(&healths);

    if args.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
        return Ok(());
    }

    p::header("Deployment Monitoring Dashboard");
    p::separator();
    p::kv_accent("Tracked deployments", &summary.total.to_string());
    p::kv("Healthy", &summary.healthy.to_string());
    p::kv("Degraded", &summary.degraded.to_string());
    p::kv("Unhealthy", &summary.unhealthy.to_string());
    p::kv("Unknown", &summary.unknown.to_string());
    p::kv_accent("Health rate", &format!("{:.1}%", summary.health_rate()));

    if !summary.by_network.is_empty() {
        p::header("By Network");
        let rows: Vec<Vec<String>> = summary
            .by_network
            .iter()
            .map(|(net, h)| {
                vec![
                    net.clone(),
                    h.healthy.to_string(),
                    h.degraded.to_string(),
                    h.unhealthy.to_string(),
                    h.unknown.to_string(),
                ]
            })
            .collect();
        p::table(
            &["Network", "Healthy", "Degraded", "Unhealthy", "Unknown"],
            &rows,
        );
    }

    let failures = dm::detect_failures(&records, dm::now_epoch());
    if !failures.is_empty() {
        p::header("Active Failures");
        for f in &failures {
            p::warn(&format!("[{}] {}: {}", f.severity, f.kind, f.message));
        }
    }
    Ok(())
}

fn handle_failures(args: FailuresArgs) -> Result<()> {
    let records = deploy_history::load_history()?;
    let failures = dm::detect_failures(&records, dm::now_epoch());

    p::header("Deployment Failure Detection");
    p::separator();
    if failures.is_empty() {
        p::success("No failed or stuck deployments detected");
        return Ok(());
    }

    for f in &failures {
        p::error(&format!(
            "[{}] {} ({}): {}",
            f.severity,
            f.kind,
            f.deployment_id.chars().take(8).collect::<String>(),
            f.message
        ));
        if args.alert {
            let mut data = HashMap::new();
            data.insert("message".to_string(), format!("{}: {}", f.kind, f.message));
            data.insert("deployment".to_string(), f.deployment_id.clone());
            data.insert("network".to_string(), f.network.clone());
            let _ = notifications::send_notification("deployment-failure", &data, &f.severity);
        }
    }
    dm::log_alerts(&failures)?;
    p::separator();
    anyhow::bail!("{} deployment failure(s) detected", failures.len());
}

async fn handle_watch(args: WatchArgs) -> Result<()> {
    p::header("Deployment Monitoring (live)");
    p::kv("Interval", &format!("{}s", args.interval));
    if let Some(net) = &args.network {
        p::kv("Network", net);
    }
    p::kv("Alerting", if args.alert { "on" } else { "off" });
    p::separator();
    p::info("Watching for status changes. Press Ctrl+C to stop.");

    let running = Arc::new(AtomicBool::new(true));
    {
        let running = Arc::clone(&running);
        ctrlc::set_handler(move || running.store(false, Ordering::SeqCst))?;
    }

    let interval = std::time::Duration::from_secs(args.interval.max(1));

    while running.load(Ordering::SeqCst) {
        let records = filtered_records(&args.network, usize::MAX)?;
        let healths = assess_all(&records).await;
        let previous = dm::load_snapshot()?;
        let changes = dm::diff_snapshot(&previous, &healths);

        for change in &changes {
            let from = change
                .from
                .map(|s| s.label().to_string())
                .unwrap_or_else(|| "new".to_string());
            let short = change.deployment_id.chars().take(8).collect::<String>();
            let line = format!("{} : {} → {}", short, from, change.to.label());
            match change.to {
                dm::HealthStatus::Unhealthy => {
                    p::error(&line);
                    if args.alert {
                        notifications::alert(&format!("Deployment {} is now unhealthy", short));
                    }
                }
                dm::HealthStatus::Degraded => p::warn(&line),
                _ => p::success(&line),
            }
        }

        if changes.is_empty() {
            let s = dm::summarize(&healths);
            p::info(&format!(
                "No changes · {} healthy / {} degraded / {} unhealthy",
                s.healthy, s.degraded, s.unhealthy
            ));
        }

        dm::save_snapshot(&dm::snapshot_from(&healths))?;

        // Sleep in short slices so Ctrl+C is responsive.
        let mut slept = std::time::Duration::ZERO;
        let slice = std::time::Duration::from_millis(200);
        while running.load(Ordering::SeqCst) && slept < interval {
            std::thread::sleep(slice);
            slept += slice;
        }
    }

    p::separator();
    p::success("Monitoring stopped");
    Ok(())
}
