use crate::utils::{config, notifications, print as p};
use anyhow::Result;
use chrono::Utc;
use clap::{Args, Subcommand};
use colored::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

// ── CLI definition ────────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum AnalyticsCommands {
    /// Record a deployment event
    Track(TrackArgs),
    /// Show deployment metrics for a contract
    Metrics(MetricsArgs),
    /// List all recorded deployments
    List(ListArgs),
    /// Detect anomalies across recent deployments
    Anomalies(AnomaliesArgs),
    /// Export analytics data as JSON or CSV
    Export(ExportArgs),
    /// Show a visual summary / dashboard of deployments
    Dashboard(DashboardArgs),
    /// Run AI-style deployment monitoring with anomaly detection and alerts
    Monitor(MonitorArgs),
}

#[derive(Args)]
pub struct TrackArgs {
    /// Contract ID that was deployed
    #[arg(long)]
    pub contract_id: String,
    /// Network where the deployment occurred
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
    /// WASM hash of the deployed binary
    #[arg(long)]
    pub wasm_hash: Option<String>,
    /// Deployer wallet public key
    #[arg(long)]
    pub deployer: Option<String>,
    /// Fee paid in stroops
    #[arg(long)]
    pub fee_stroops: Option<u64>,
    /// Transaction hash
    #[arg(long)]
    pub tx_hash: Option<String>,
    /// Arbitrary label for this deployment
    #[arg(long)]
    pub label: Option<String>,
    /// Deployment duration in seconds (build + deploy)
    #[arg(long)]
    pub duration_secs: Option<u64>,
    /// Whether the deployment succeeded
    #[arg(long, default_value = "true")]
    pub success: bool,
    /// Error message if deployment failed
    #[arg(long)]
    pub error: Option<String>,
}

#[derive(Args)]
pub struct MetricsArgs {
    /// Contract ID to show metrics for
    #[arg(long)]
    pub contract_id: Option<String>,
    /// Network filter
    #[arg(long)]
    pub network: Option<String>,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct ListArgs {
    /// Network filter
    #[arg(long)]
    pub network: Option<String>,
    /// Contract filter
    #[arg(long)]
    pub contract_id: Option<String>,
    /// Maximum records to show
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
    /// Show failures only
    #[arg(long)]
    pub failures: bool,
}

#[derive(Args)]
pub struct AnomaliesArgs {
    /// Network to analyse
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
    /// Multiplier above average fee that counts as a fee anomaly (default 3x)
    #[arg(long, default_value_t = 3.0)]
    pub fee_threshold: f64,
    /// Minimum deployments before anomaly detection runs
    #[arg(long, default_value_t = 3)]
    pub min_samples: usize,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct ExportArgs {
    /// Output format: json | csv
    #[arg(long, default_value = "json", value_parser = ["json", "csv"])]
    pub format: String,
    /// Output file path (default: stdout)
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Network filter
    #[arg(long)]
    pub network: Option<String>,
}

#[derive(Args)]
pub struct DashboardArgs {
    /// Network to display
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
}

#[derive(Args)]
pub struct MonitorArgs {
    /// Network to monitor
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
    /// Optional contract filter for focused monitoring
    #[arg(long)]
    pub contract_id: Option<String>,
    /// Polling interval in seconds when --follow is used
    #[arg(long, default_value_t = 5)]
    pub interval: u64,
    /// Keep monitoring until interrupted
    #[arg(long)]
    pub follow: bool,
    /// Output the report as JSON
    #[arg(long)]
    pub json: bool,
}

// ── Data structures ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentEvent {
    pub id: String,
    pub contract_id: String,
    pub network: String,
    pub wasm_hash: Option<String>,
    pub deployer: Option<String>,
    pub fee_stroops: Option<u64>,
    pub tx_hash: Option<String>,
    pub label: Option<String>,
    pub duration_secs: Option<u64>,
    pub success: bool,
    pub error: Option<String>,
    pub timestamp: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeploymentMetrics {
    pub contract_id: Option<String>,
    pub network: Option<String>,
    pub total_deployments: usize,
    pub successful: usize,
    pub failed: usize,
    pub success_rate_pct: f64,
    pub avg_fee_stroops: Option<f64>,
    pub min_fee_stroops: Option<u64>,
    pub max_fee_stroops: Option<u64>,
    pub avg_duration_secs: Option<f64>,
    pub unique_deployers: usize,
    pub unique_contracts: usize,
    pub first_deployment: Option<String>,
    pub last_deployment: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Anomaly {
    pub kind: String,
    pub contract_id: String,
    pub network: String,
    pub description: String,
    pub event_id: String,
    pub timestamp: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MonitoringReport {
    pub network: String,
    pub contract_id: Option<String>,
    pub real_time: bool,
    pub performance_score: u8,
    pub summary: String,
    pub anomalies: Vec<Anomaly>,
    pub predicted_issues: Vec<String>,
    pub alerts: Vec<String>,
    pub historical_analysis: Vec<String>,
}

// ── Storage helpers ───────────────────────────────────────────────────────────

fn analytics_dir() -> Result<PathBuf> {
    let dir = config::config_dir().join("analytics");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

fn events_path() -> Result<PathBuf> {
    Ok(analytics_dir()?.join("deployments.json"))
}

fn load_events() -> Result<Vec<DeploymentEvent>> {
    let path = events_path()?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data).unwrap_or_default())
}

fn save_events(events: &[DeploymentEvent]) -> Result<()> {
    fs::write(events_path()?, serde_json::to_string_pretty(events)?)?;
    Ok(())
}

// ── Metrics computation ───────────────────────────────────────────────────────

pub fn build_monitoring_report(
    events: &[DeploymentEvent],
    network: &str,
    contract_id: Option<&str>,
    real_time: bool,
) -> MonitoringReport {
    let filtered: Vec<_> = events
        .iter()
        .filter(|e| e.network == network)
        .filter(|e| contract_id.is_none_or(|c| e.contract_id == c))
        .collect();

    let metrics = compute_metrics(events, contract_id, Some(network));
    let anomalies = detect_anomalies(events, network, 3.0, 3);

    let mut predicted_issues = Vec::new();
    let mut alerts = Vec::new();

    if metrics.total_deployments >= 3 {
        let failure_ratio = if metrics.total_deployments > 0 {
            metrics.failed as f64 / metrics.total_deployments as f64
        } else {
            0.0
        };
        if failure_ratio >= 0.25 {
            predicted_issues.push(
                "Elevated deployment failures suggest the next rollout may be unstable.".to_string(),
            );
            alerts.push(
                "Review the latest deployment logs and rollback plan before the next release."
                    .to_string(),
            );
        }

        if let Some(avg_fee) = metrics.avg_fee_stroops {
            if avg_fee > 500_000.0 {
                predicted_issues.push(
                    "Fee inflation is trending upward and could increase operational cost."
                        .to_string(),
                );
                alerts.push(
                    "Investigate recent transaction fees and consider batching or gas tuning."
                        .to_string(),
                );
            }
        }

        if let Some(avg_duration) = metrics.avg_duration_secs {
            if avg_duration > 120.0 {
                predicted_issues.push(
                    "Longer deployment durations indicate a likely performance regression."
                        .to_string(),
                );
                alerts.push(
                    "Inspect deployment build and network latency before shipping the next contract."
                        .to_string(),
                );
            }
        }
    }

    if !anomalies.is_empty() {
        predicted_issues.push(
            "Recent anomalies suggest a higher probability of a repeat issue in the next deployment."
                .to_string(),
        );
    }

    for anomaly in &anomalies {
        alerts.push(format!(
            "{} [{}] {}",
            anomaly.kind, anomaly.contract_id, anomaly.description
        ));
    }

    let mut historical_analysis = vec![
        format!("{} deployment events tracked in {}", filtered.len(), network),
        format!(
            "Success rate is {:.1}% with {} failures recorded",
            metrics.success_rate_pct,
            metrics.failed
        ),
    ];

    if let Some(avg_fee) = metrics.avg_fee_stroops {
        historical_analysis.push(format!("Average deployment fee is {:.0} stroops", avg_fee));
    }
    if let Some(avg_duration) = metrics.avg_duration_secs {
        historical_analysis.push(format!("Average deployment duration is {:.1}s", avg_duration));
    }

    let performance_score = if metrics.total_deployments == 0 {
        100u8
    } else {
        let mut score = 100u8;
        if metrics.failed > 0 {
            score = score.saturating_sub((metrics.failed as u8).min(40));
        }
        if metrics.avg_fee_stroops.unwrap_or_default() > 500_000.0 {
            score = score.saturating_sub(10);
        }
        if metrics.avg_duration_secs.unwrap_or_default() > 120.0 {
            score = score.saturating_sub(10);
        }
        score
    };

    MonitoringReport {
        network: network.to_string(),
        contract_id: contract_id.map(|s| s.to_string()),
        real_time,
        performance_score,
        summary: format!(
            "Deployment monitoring for {} is {} and {}",
            network,
            if performance_score >= 80 {
                "stable"
            } else if performance_score >= 50 {
                "degraded"
            } else {
                "at risk"
            },
            if anomalies.is_empty() {
                "no anomalies were detected"
            } else {
                "anomalies require attention"
            }
        ),
        anomalies: anomalies.clone(),
        predicted_issues,
        alerts,
        historical_analysis,
    }
}

pub fn compute_metrics(
    events: &[DeploymentEvent],
    contract_id: Option<&str>,
    network: Option<&str>,
) -> DeploymentMetrics {
    let filtered: Vec<_> = events
        .iter()
        .filter(|e| network.is_none_or(|n| e.network == n))
        .filter(|e| contract_id.is_none_or(|c| e.contract_id == c))
        .collect();

    let total = filtered.len();
    let successful = filtered.iter().filter(|e| e.success).count();
    let failed = total - successful;
    let success_rate = if total > 0 {
        (successful as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    let fees: Vec<u64> = filtered.iter().filter_map(|e| e.fee_stroops).collect();
    let avg_fee = if fees.is_empty() {
        None
    } else {
        Some(fees.iter().sum::<u64>() as f64 / fees.len() as f64)
    };
    let min_fee = fees.iter().copied().min();
    let max_fee = fees.iter().copied().max();

    let durations: Vec<u64> = filtered.iter().filter_map(|e| e.duration_secs).collect();
    let avg_duration = if durations.is_empty() {
        None
    } else {
        Some(durations.iter().sum::<u64>() as f64 / durations.len() as f64)
    };

    let mut deployers = std::collections::HashSet::new();
    let mut contracts = std::collections::HashSet::new();
    for e in &filtered {
        if let Some(ref d) = e.deployer {
            deployers.insert(d.clone());
        }
        contracts.insert(e.contract_id.clone());
    }

    let first = filtered.first().map(|e| e.timestamp.clone());
    let last = filtered.last().map(|e| e.timestamp.clone());

    DeploymentMetrics {
        contract_id: contract_id.map(|s| s.to_string()),
        network: network.map(|s| s.to_string()),
        total_deployments: total,
        successful,
        failed,
        success_rate_pct: success_rate,
        avg_fee_stroops: avg_fee,
        min_fee_stroops: min_fee,
        max_fee_stroops: max_fee,
        avg_duration_secs: avg_duration,
        unique_deployers: deployers.len(),
        unique_contracts: contracts.len(),
        first_deployment: first,
        last_deployment: last,
    }
}

/// Detect anomalies:
/// - High fee (fee > threshold * avg_fee)
/// - Repeated failures for the same contract
/// - Unusually fast or slow deployments
pub fn detect_anomalies(
    events: &[DeploymentEvent],
    network: &str,
    fee_threshold: f64,
    min_samples: usize,
) -> Vec<Anomaly> {
    let net_events: Vec<_> = events.iter().filter(|e| e.network == network).collect();

    if net_events.len() < min_samples {
        return vec![];
    }

    let mut anomalies = Vec::new();

    // Compute average fee
    let fees: Vec<u64> = net_events.iter().filter_map(|e| e.fee_stroops).collect();
    let avg_fee = if fees.len() >= min_samples {
        Some(fees.iter().sum::<u64>() as f64 / fees.len() as f64)
    } else {
        None
    };

    // Fee anomalies
    if let Some(avg) = avg_fee {
        for event in &net_events {
            if let Some(fee) = event.fee_stroops {
                if fee as f64 > avg * fee_threshold {
                    anomalies.push(Anomaly {
                        kind: "high-fee".to_string(),
                        contract_id: event.contract_id.clone(),
                        network: network.to_string(),
                        description: format!(
                            "Fee {} stroops is {:.1}x above average ({:.0} stroops)",
                            fee,
                            fee as f64 / avg,
                            avg
                        ),
                        event_id: event.id.clone(),
                        timestamp: event.timestamp.clone(),
                    });
                }
            }
        }
    }

    // Repeated failures per contract
    let mut failure_counts: HashMap<&str, usize> = HashMap::new();
    for e in &net_events {
        if !e.success {
            *failure_counts.entry(e.contract_id.as_str()).or_insert(0) += 1;
        }
    }
    for (contract, &count) in &failure_counts {
        if count >= 2 {
            anomalies.push(Anomaly {
                kind: "repeated-failure".to_string(),
                contract_id: contract.to_string(),
                network: network.to_string(),
                description: format!(
                    "{} consecutive/recent deployment failure(s) for this contract",
                    count
                ),
                event_id: "aggregate".to_string(),
                timestamp: Utc::now().to_rfc3339(),
            });
        }
    }

    anomalies
}

// ── Serialise to CSV ──────────────────────────────────────────────────────────

fn events_to_csv(events: &[DeploymentEvent]) -> String {
    let mut out = String::from(
        "id,contract_id,network,wasm_hash,deployer,fee_stroops,tx_hash,label,duration_secs,success,error,timestamp\n",
    );
    for e in events {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{}\n",
            e.id,
            e.contract_id,
            e.network,
            e.wasm_hash.as_deref().unwrap_or(""),
            e.deployer.as_deref().unwrap_or(""),
            e.fee_stroops.map(|f| f.to_string()).unwrap_or_default(),
            e.tx_hash.as_deref().unwrap_or(""),
            e.label.as_deref().unwrap_or(""),
            e.duration_secs.map(|d| d.to_string()).unwrap_or_default(),
            e.success,
            e.error.as_deref().unwrap_or(""),
            e.timestamp,
        ));
    }
    out
}

// ── Command handlers ──────────────────────────────────────────────────────────

pub async fn handle(cmd: AnalyticsCommands) -> Result<()> {
    match cmd {
        AnalyticsCommands::Track(args) => handle_track(args),
        AnalyticsCommands::Metrics(args) => handle_metrics(args),
        AnalyticsCommands::List(args) => handle_list(args),
        AnalyticsCommands::Anomalies(args) => handle_anomalies(args),
        AnalyticsCommands::Export(args) => handle_export(args),
        AnalyticsCommands::Dashboard(args) => handle_dashboard(args),
        AnalyticsCommands::Monitor(args) => handle_monitor(args).await,
    }
}

fn handle_track(args: TrackArgs) -> Result<()> {
    p::header("Track Deployment");
    config::validate_network(&args.network)?;

    if args.contract_id.is_empty() {
        anyhow::bail!("--contract-id must not be empty");
    }

    let id = format!(
        "dep-{}-{}",
        &args.contract_id[..args.contract_id.len().min(8)],
        Utc::now().timestamp()
    );

    let event = DeploymentEvent {
        id: id.clone(),
        contract_id: args.contract_id.clone(),
        network: args.network.clone(),
        wasm_hash: args.wasm_hash.clone(),
        deployer: args.deployer.clone(),
        fee_stroops: args.fee_stroops,
        tx_hash: args.tx_hash.clone(),
        label: args.label.clone(),
        duration_secs: args.duration_secs,
        success: args.success,
        error: args.error.clone(),
        timestamp: Utc::now().to_rfc3339(),
    };

    let mut events = load_events()?;
    events.push(event.clone());
    save_events(&events)?;

    p::separator();
    p::kv_accent("Event ID", &id);
    p::kv("Contract", &args.contract_id);
    p::kv("Network", &args.network);
    p::kv("Status", if args.success { "success" } else { "failed" });
    if let Some(fee) = args.fee_stroops {
        p::kv("Fee (stroops)", &fee.to_string());
        p::kv("Fee (XLM)", &format!("{:.7}", fee as f64 / 10_000_000.0));
    }
    p::separator();
    p::success("Deployment event recorded.");

    let mut events = load_events()?;
    events.push(event);
    let report = build_monitoring_report(&events, &args.network, Some(&args.contract_id), false);
    if !report.alerts.is_empty() {
        notifications::alert(&format!(
            "Monitoring alert: {}",
            report.alerts.first().unwrap_or(&"Deployment behavior changed".to_string())
        ));
    }
    Ok(())
}

fn handle_metrics(args: MetricsArgs) -> Result<()> {
    p::header("Deployment Metrics");

    let events = load_events()?;
    let metrics = compute_metrics(
        &events,
        args.contract_id.as_deref(),
        args.network.as_deref(),
    );

    if args.json {
        println!("{}", serde_json::to_string_pretty(&metrics)?);
        return Ok(());
    }

    p::separator();
    if let Some(ref c) = metrics.contract_id {
        p::kv("Contract", c);
    }
    if let Some(ref n) = metrics.network {
        p::kv("Network", n);
    }
    p::kv(
        "Total deployments",
        &format!("{}", metrics.total_deployments),
    );
    p::kv("Successful", &format!("{}", metrics.successful));
    p::kv("Failed", &format!("{}", metrics.failed));
    p::kv("Success rate", &format!("{:.1}%", metrics.success_rate_pct));
    if let Some(avg) = metrics.avg_fee_stroops {
        p::kv("Avg fee (stroops)", &format!("{:.0}", avg));
        p::kv("Avg fee (XLM)", &format!("{:.7}", avg / 10_000_000.0));
    }
    if let Some(min) = metrics.min_fee_stroops {
        p::kv("Min fee (stroops)", &format!("{}", min));
    }
    if let Some(max) = metrics.max_fee_stroops {
        p::kv("Max fee (stroops)", &format!("{}", max));
    }
    if let Some(dur) = metrics.avg_duration_secs {
        p::kv("Avg duration (s)", &format!("{:.1}", dur));
    }
    p::kv("Unique deployers", &format!("{}", metrics.unique_deployers));
    p::kv("Unique contracts", &format!("{}", metrics.unique_contracts));
    if let Some(ref first) = metrics.first_deployment {
        p::kv("First deployment", first.get(..16).unwrap_or(first));
    }
    if let Some(ref last) = metrics.last_deployment {
        p::kv("Last deployment", last.get(..16).unwrap_or(last));
    }
    p::separator();
    Ok(())
}

fn handle_list(args: ListArgs) -> Result<()> {
    p::header("Deployment Events");

    let events = load_events()?;
    let mut filtered: Vec<_> = events
        .iter()
        .filter(|e| args.network.as_deref().is_none_or(|n| e.network == n))
        .filter(|e| {
            args.contract_id
                .as_deref()
                .is_none_or(|c| e.contract_id == c)
        })
        .filter(|e| !args.failures || !e.success)
        .collect();

    // Most recent first
    filtered.reverse();
    let displayed: Vec<_> = filtered.iter().take(args.limit).collect();

    if displayed.is_empty() {
        p::info("No deployment events found. Track one with `starforge analytics track`.");
        return Ok(());
    }

    p::separator();
    println!(
        "  {:<20}  {:<14}  {:<10}  {:<10}  {}",
        "ID".dimmed(),
        "Contract".dimmed(),
        "Network".dimmed(),
        "Status".dimmed(),
        "Timestamp".dimmed(),
    );
    println!("  {}", "─".repeat(75).dimmed());

    for event in displayed {
        let status = if event.success {
            "ok".green().to_string()
        } else {
            "failed".red().to_string()
        };
        let ts = event.timestamp.get(..16).unwrap_or(&event.timestamp);
        println!(
            "  {:<20}  {:<14}  {:<10}  {:<10}  {}",
            event.id.white(),
            short_id(&event.contract_id).cyan(),
            event.network.white(),
            status,
            ts.dimmed(),
        );
    }
    p::separator();
    Ok(())
}

fn handle_anomalies(args: AnomaliesArgs) -> Result<()> {
    p::header("Deployment Anomaly Detection");
    config::validate_network(&args.network)?;

    let events = load_events()?;
    let anomalies = detect_anomalies(&events, &args.network, args.fee_threshold, args.min_samples);

    if args.json {
        println!("{}", serde_json::to_string_pretty(&anomalies)?);
        return Ok(());
    }

    if anomalies.is_empty() {
        p::separator();
        p::info("No anomalies detected.");
        p::separator();
        return Ok(());
    }

    p::separator();
    println!(
        "  {:<18}  {:<16}  {}",
        "Kind".dimmed(),
        "Contract".dimmed(),
        "Description".dimmed(),
    );
    println!("  {}", "─".repeat(72).dimmed());

    for anomaly in &anomalies {
        println!(
            "  {:<18}  {:<16}  {}",
            anomaly.kind.yellow(),
            short_id(&anomaly.contract_id).cyan(),
            anomaly.description.white(),
        );
    }
    p::separator();
    println!(
        "  {} {} anomaly/anomalies detected on {}",
        anomalies.len().to_string().yellow().bold(),
        "total".dimmed(),
        args.network.cyan()
    );
    p::separator();
    Ok(())
}

fn handle_export(args: ExportArgs) -> Result<()> {
    p::header("Export Analytics Data");

    let events = load_events()?;
    let filtered: Vec<_> = events
        .iter()
        .filter(|e| args.network.as_deref().is_none_or(|n| e.network == n))
        .cloned()
        .collect();

    let data = match args.format.as_str() {
        "csv" => events_to_csv(&filtered),
        _ => serde_json::to_string_pretty(&filtered)?,
    };

    if let Some(ref out_path) = args.out {
        fs::write(out_path, &data)?;
        p::success(&format!(
            "Exported {} events to {}",
            filtered.len(),
            out_path.display()
        ));
    } else {
        println!("{}", data);
    }
    Ok(())
}

fn handle_dashboard(args: DashboardArgs) -> Result<()> {
    p::header("Deployment Analytics Dashboard");
    config::validate_network(&args.network)?;

    let events = load_events()?;
    let metrics = compute_metrics(&events, None, Some(&args.network));
    let anomalies = detect_anomalies(&events, &args.network, 3.0, 3);

    p::separator();
    println!("  {} {}", "Network:".dimmed(), args.network.cyan().bold());
    println!();

    // Summary bar
    println!(
        "  {:<28}  {}",
        "Total deployments".bright_white(),
        format!("{}", metrics.total_deployments).white().bold()
    );
    println!(
        "  {:<28}  {}",
        "Success rate".bright_white(),
        format!("{:.1}%", metrics.success_rate_pct).green().bold()
    );
    println!(
        "  {:<28}  {}",
        "Failed deployments".bright_white(),
        if metrics.failed > 0 {
            format!("{}", metrics.failed).red().bold()
        } else {
            "0".green().bold()
        }
    );
    println!(
        "  {:<28}  {}",
        "Unique contracts".bright_white(),
        format!("{}", metrics.unique_contracts).white()
    );
    println!(
        "  {:<28}  {}",
        "Unique deployers".bright_white(),
        format!("{}", metrics.unique_deployers).white()
    );

    if let Some(avg) = metrics.avg_fee_stroops {
        println!(
            "  {:<28}  {} ({:.7} XLM)",
            "Avg fee".bright_white(),
            format!("{:.0} stroops", avg).white(),
            avg / 10_000_000.0
        );
    }

    println!();
    if anomalies.is_empty() {
        println!("  {} {}", "Anomalies:".dimmed(), "none detected".green());
    } else {
        println!(
            "  {} {}",
            "Anomalies:".dimmed(),
            format!("{} detected", anomalies.len()).yellow().bold()
        );
        for a in &anomalies {
            println!(
                "    {} [{}] {}",
                "⚠".yellow(),
                a.kind.yellow(),
                a.description.dimmed()
            );
        }
    }

    // ASCII bar chart of success vs failure
    if metrics.total_deployments > 0 {
        println!();
        let bar_width = 40usize;
        let ok_bars = (metrics.successful as f64 / metrics.total_deployments as f64
            * bar_width as f64) as usize;
        let fail_bars = bar_width - ok_bars;
        println!(
            "  Success/Fail  [{}{}]",
            "█".repeat(ok_bars).green(),
            "░".repeat(fail_bars).red()
        );
    }

    p::separator();
    p::info("Use `starforge analytics anomalies` for detailed anomaly info.");
    p::info("Use `starforge analytics export --format csv` to export data.");
    Ok(())
}

async fn handle_monitor(args: MonitorArgs) -> Result<()> {
    p::header("AI Deployment Monitoring");
    config::validate_network(&args.network)?;

    let mut iteration = 0u64;
    loop {
        let events = load_events()?;
        let report = build_monitoring_report(
            &events,
            &args.network,
            args.contract_id.as_deref(),
            args.follow,
        );

        if args.json {
            println!("{}", serde_json::to_string_pretty(&report)?);
            if !args.follow {
                return Ok(());
            }
        } else {
            p::separator();
            println!("  {} {}", "Network:".dimmed(), args.network.cyan().bold());
            if let Some(ref contract_id) = args.contract_id {
                println!("  {} {}", "Contract:".dimmed(), contract_id.cyan());
            }
            println!("  {} {}", "Status:".dimmed(), report.summary.white());
            println!(
                "  {} {} / 100",
                "Performance score:".dimmed(),
                report.performance_score.to_string().green().bold()
            );
            println!("  {} {}", "Anomalies:".dimmed(), report.anomalies.len());
            println!("  {} {}", "Predicted issues:".dimmed(), report.predicted_issues.len());
            println!("  {} {}", "Alerts:".dimmed(), report.alerts.len());
            println!();
            for item in &report.historical_analysis {
                println!("  • {}", item.dimmed());
            }
            println!();
            if report.predicted_issues.is_empty() {
                p::info("No deployment issues predicted at this time.");
            } else {
                for issue in &report.predicted_issues {
                    println!("  • {}", issue.yellow());
                }
            }
            if !report.alerts.is_empty() {
                println!();
                for alert in &report.alerts {
                    notifications::alert(alert);
                }
            }
        }

        if !args.follow {
            return Ok(());
        }

        iteration += 1;
        if iteration > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(args.interval.max(1))).await;
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn short_id(id: &str) -> String {
    if id.len() > 12 {
        format!("{}…", &id[..12])
    } else {
        id.to_string()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(
        id: &str,
        contract: &str,
        network: &str,
        fee: Option<u64>,
        success: bool,
    ) -> DeploymentEvent {
        DeploymentEvent {
            id: id.to_string(),
            contract_id: contract.to_string(),
            network: network.to_string(),
            wasm_hash: None,
            deployer: Some("GTEST".to_string()),
            fee_stroops: fee,
            tx_hash: None,
            label: None,
            duration_secs: None,
            success,
            error: None,
            timestamp: Utc::now().to_rfc3339(),
        }
    }

    #[test]
    fn compute_metrics_empty() {
        let m = compute_metrics(&[], None, None);
        assert_eq!(m.total_deployments, 0);
        assert_eq!(m.success_rate_pct, 0.0);
        assert!(m.avg_fee_stroops.is_none());
    }

    #[test]
    fn compute_metrics_counts_correctly() {
        let events = vec![
            make_event("e1", "CA", "testnet", Some(1000), true),
            make_event("e2", "CA", "testnet", Some(2000), false),
            make_event("e3", "CB", "testnet", Some(3000), true),
        ];
        let m = compute_metrics(&events, None, Some("testnet"));
        assert_eq!(m.total_deployments, 3);
        assert_eq!(m.successful, 2);
        assert_eq!(m.failed, 1);
        assert!((m.success_rate_pct - 66.666).abs() < 0.01);
        assert_eq!(m.avg_fee_stroops, Some(2000.0));
        assert_eq!(m.unique_contracts, 2);
    }

    #[test]
    fn compute_metrics_filters_by_contract() {
        let events = vec![
            make_event("e1", "CA", "testnet", Some(100), true),
            make_event("e2", "CB", "testnet", Some(200), true),
        ];
        let m = compute_metrics(&events, Some("CA"), Some("testnet"));
        assert_eq!(m.total_deployments, 1);
        assert_eq!(m.avg_fee_stroops, Some(100.0));
    }

    #[test]
    fn compute_metrics_filters_by_network() {
        let events = vec![
            make_event("e1", "CA", "testnet", Some(100), true),
            make_event("e2", "CA", "mainnet", Some(200), true),
        ];
        let m = compute_metrics(&events, None, Some("mainnet"));
        assert_eq!(m.total_deployments, 1);
        assert_eq!(m.avg_fee_stroops, Some(200.0));
    }

    #[test]
    fn detect_anomalies_needs_min_samples() {
        let events = vec![
            make_event("e1", "CA", "testnet", Some(100), true),
            make_event("e2", "CA", "testnet", Some(100), true),
        ];
        // min_samples=3 means no anomalies with only 2 events
        let anomalies = detect_anomalies(&events, "testnet", 3.0, 3);
        assert!(anomalies.is_empty());
    }

    #[test]
    fn detect_anomalies_finds_high_fee() {
        let events = vec![
            make_event("e1", "CA", "testnet", Some(100), true),
            make_event("e2", "CA", "testnet", Some(100), true),
            make_event("e3", "CA", "testnet", Some(100), true),
            make_event("e4", "CA", "testnet", Some(10000), true), // 100x average
        ];
        let anomalies = detect_anomalies(&events, "testnet", 3.0, 3);
        assert!(anomalies.iter().any(|a| a.kind == "high-fee"));
    }

    #[test]
    fn detect_anomalies_finds_repeated_failure() {
        let events = vec![
            make_event("e1", "CA", "testnet", Some(100), true),
            make_event("e2", "CA", "testnet", Some(100), true),
            make_event("e3", "CB", "testnet", Some(100), false),
            make_event("e4", "CB", "testnet", Some(100), false),
        ];
        let anomalies = detect_anomalies(&events, "testnet", 3.0, 2);
        assert!(anomalies
            .iter()
            .any(|a| a.kind == "repeated-failure" && a.contract_id == "CB"));
    }

    #[test]
    fn events_to_csv_has_header() {
        let events = vec![make_event("e1", "CA", "testnet", Some(100), true)];
        let csv = events_to_csv(&events);
        assert!(csv.starts_with("id,contract_id,network"));
        assert!(csv.contains("e1"));
    }

    #[test]
    fn short_id_truncates_long_ids() {
        let id = "GABC123456789XYZ";
        let s = short_id(id);
        assert!(s.contains('…'));
        assert!(s.len() < id.len() + 1);
    }

    #[test]
    fn short_id_leaves_short_ids_intact() {
        let id = "GABC";
        assert_eq!(short_id(id), "GABC");
    }

    #[test]
    fn build_monitoring_report_surfaces_alerts_and_predictions() {
        let events = vec![
            make_event("e1", "CA", "testnet", Some(100), true),
            make_event("e2", "CA", "testnet", Some(100), true),
            make_event("e3", "CA", "testnet", Some(100), false),
            make_event("e4", "CA", "testnet", Some(10_000), true),
        ];

        let report = build_monitoring_report(&events, "testnet", Some("CA"), false);
        assert!(report.performance_score < 100);
        assert!(!report.predicted_issues.is_empty());
        assert!(!report.alerts.is_empty());
        assert!(report.anomalies.iter().any(|a| a.kind == "high-fee"));
    }
}
