use crate::utils::{config, print as p};
use anyhow::Result;
use chrono::Utc;
use clap::{Args, Subcommand};
use colored::Colorize;
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
    /// Analyze deployment trends over time (Issue #545)
    Trends(TrendsArgs),
    /// Predict deployment success and resource usage (Issue #545)
    Predict(PredictArgs),
    /// Health score for contract deployments (Issue #545)
    Health(HealthArgs),
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

// Issue #545: AI Deployment Analytics - New Args
#[derive(Args)]
pub struct TrendsArgs {
    /// Network to analyze
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
    /// Contract ID filter (optional)
    #[arg(long)]
    pub contract_id: Option<String>,
    /// Time window in days (default: 30)
    #[arg(long, default_value_t = 30)]
    pub days: usize,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct PredictArgs {
    /// Contract ID to predict for
    #[arg(long)]
    pub contract_id: String,
    /// Network
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
    /// WASM file size in KB (optional for prediction)
    #[arg(long)]
    pub wasm_size_kb: Option<f64>,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct HealthArgs {
    /// Contract ID to check health
    #[arg(long)]
    pub contract_id: String,
    /// Network
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
    /// Output as JSON
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

// Issue #545: AI Analytics - Trend Analysis
#[derive(Debug, Serialize, Deserialize)]
pub struct TrendAnalysis {
    pub contract_id: Option<String>,
    pub network: String,
    pub time_window_days: usize,
    pub deployment_frequency: f64,  // deployments per day
    pub success_rate_trend: String, // "improving", "declining", "stable"
    pub avg_fee_trend: String,      // "increasing", "decreasing", "stable"
    pub recent_failures: usize,
    pub deployment_velocity: f64, // change in deployment frequency
    pub health_score: f64,        // 0-100
    pub predictions: TrendPredictions,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrendPredictions {
    pub next_deployment_likely_success: bool,
    pub predicted_fee_range: (u64, u64), // (min, max) stroops
    pub risk_factors: Vec<String>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthScore {
    pub contract_id: String,
    pub network: String,
    pub overall_score: f64,     // 0-100
    pub reliability_score: f64, // based on success rate
    pub performance_score: f64, // based on fee efficiency
    pub activity_score: f64,    // based on deployment frequency
    pub risk_level: String,     // "low", "medium", "high"
    pub issues: Vec<String>,
    pub strengths: Vec<String>,
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

pub fn compute_metrics(
    events: &[DeploymentEvent],
    contract_id: Option<&str>,
    network: Option<&str>,
) -> DeploymentMetrics {
    let filtered: Vec<_> = events
        .iter()
        .filter(|e| network.map_or(true, |n| e.network == n))
        .filter(|e| contract_id.map_or(true, |c| e.contract_id == c))
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

// ── Issue #545: Trend Analysis ────────────────────────────────────────────────

/// Analyze deployment trends over time
pub fn analyze_trends(
    events: &[DeploymentEvent],
    network: &str,
    contract_id: Option<&str>,
    days: usize,
) -> TrendAnalysis {
    use chrono::DateTime;

    let cutoff = Utc::now() - chrono::Duration::days(days as i64);

    let filtered: Vec<_> = events
        .iter()
        .filter(|e| e.network == network)
        .filter(|e| contract_id.map_or(true, |c| e.contract_id == c))
        .filter(|e| {
            DateTime::parse_from_rfc3339(&e.timestamp)
                .map(|t| t.with_timezone(&Utc) > cutoff)
                .unwrap_or(false)
        })
        .collect();

    if filtered.is_empty() {
        return TrendAnalysis {
            contract_id: contract_id.map(|s| s.to_string()),
            network: network.to_string(),
            time_window_days: days,
            deployment_frequency: 0.0,
            success_rate_trend: "insufficient-data".to_string(),
            avg_fee_trend: "insufficient-data".to_string(),
            recent_failures: 0,
            deployment_velocity: 0.0,
            health_score: 50.0,
            predictions: TrendPredictions {
                next_deployment_likely_success: true,
                predicted_fee_range: (100, 1000),
                risk_factors: vec!["Insufficient historical data".to_string()],
                recommendations: vec!["Deploy more contracts to build baseline".to_string()],
            },
        };
    }

    let total = filtered.len();
    let successful = filtered.iter().filter(|e| e.success).count();
    let recent_failures = filtered.iter().rev().take(5).filter(|e| !e.success).count();

    let deployment_frequency = total as f64 / days as f64;

    // Calculate success rate trend (compare first half vs second half)
    let mid = total / 2;
    let first_half_success =
        filtered.iter().take(mid).filter(|e| e.success).count() as f64 / mid.max(1) as f64;
    let second_half_success = filtered.iter().skip(mid).filter(|e| e.success).count() as f64
        / (total - mid).max(1) as f64;

    let success_rate_trend = if second_half_success > first_half_success + 0.1 {
        "improving"
    } else if second_half_success < first_half_success - 0.1 {
        "declining"
    } else {
        "stable"
    }
    .to_string();

    // Calculate fee trend
    let fees: Vec<u64> = filtered.iter().filter_map(|e| e.fee_stroops).collect();
    let avg_fee_trend = if fees.len() >= 2 {
        let mid = fees.len() / 2;
        let first_avg: f64 = fees.iter().take(mid).sum::<u64>() as f64 / mid as f64;
        let second_avg: f64 = fees.iter().skip(mid).sum::<u64>() as f64 / (fees.len() - mid) as f64;

        if second_avg > first_avg * 1.2 {
            "increasing"
        } else if second_avg < first_avg * 0.8 {
            "decreasing"
        } else {
            "stable"
        }
    } else {
        "insufficient-data"
    }
    .to_string();

    // Calculate deployment velocity (rate of change in deployment frequency)
    let deployment_velocity = if days >= 14 {
        let mid_point = Utc::now() - chrono::Duration::days((days / 2) as i64);
        let first_half_count = filtered
            .iter()
            .filter(|e| {
                DateTime::parse_from_rfc3339(&e.timestamp)
                    .map(|t| t.with_timezone(&Utc) < mid_point)
                    .unwrap_or(false)
            })
            .count();
        let second_half_count = total - first_half_count;
        (second_half_count as f64 - first_half_count as f64) / (days as f64 / 2.0)
    } else {
        0.0
    };

    // Calculate health score
    let success_rate = successful as f64 / total as f64;
    let health_score = calculate_health_score(success_rate, recent_failures, &success_rate_trend);

    // Generate predictions
    let next_deployment_likely_success = success_rate > 0.7 && recent_failures < 2;

    let predicted_fee_range = if !fees.is_empty() {
        let avg = fees.iter().sum::<u64>() as f64 / fees.len() as f64;
        let min = (avg * 0.8) as u64;
        let max = (avg * 1.5) as u64;
        (min, max)
    } else {
        (100, 1000)
    };

    let mut risk_factors = Vec::new();
    if recent_failures >= 2 {
        risk_factors.push(format!(
            "{} recent deployment failures detected",
            recent_failures
        ));
    }
    if success_rate < 0.5 {
        risk_factors.push(format!("Low success rate: {:.1}%", success_rate * 100.0));
    }
    if success_rate_trend == "declining" {
        risk_factors.push("Success rate is declining over time".to_string());
    }
    if avg_fee_trend == "increasing" {
        risk_factors.push("Deployment costs are increasing".to_string());
    }

    let mut recommendations = Vec::new();
    if recent_failures > 0 {
        recommendations.push(
            "Review recent deployment errors with `starforge deployments history --failures`"
                .to_string(),
        );
    }
    if deployment_frequency < 0.1 {
        recommendations
            .push("Low deployment frequency - consider more regular deployments".to_string());
    }
    if success_rate < 0.8 {
        recommendations.push("Run `starforge security audit` before next deployment".to_string());
    }
    if avg_fee_trend == "increasing" {
        recommendations.push("Optimize WASM size to reduce deployment costs".to_string());
    }
    if recommendations.is_empty() {
        recommendations
            .push("Deployment trends look healthy - continue current practices".to_string());
    }

    TrendAnalysis {
        contract_id: contract_id.map(|s| s.to_string()),
        network: network.to_string(),
        time_window_days: days,
        deployment_frequency,
        success_rate_trend,
        avg_fee_trend,
        recent_failures,
        deployment_velocity,
        health_score,
        predictions: TrendPredictions {
            next_deployment_likely_success,
            predicted_fee_range,
            risk_factors,
            recommendations,
        },
    }
}

fn calculate_health_score(success_rate: f64, recent_failures: usize, trend: &str) -> f64 {
    let mut score = success_rate * 100.0;

    // Penalty for recent failures
    score -= (recent_failures as f64 * 10.0).min(30.0);

    // Adjust for trend
    match trend {
        "improving" => score += 5.0,
        "declining" => score -= 15.0,
        _ => {}
    }

    score.clamp(0.0, 100.0)
}

/// Calculate health score for a contract
pub fn calculate_contract_health(
    events: &[DeploymentEvent],
    contract_id: &str,
    network: &str,
) -> HealthScore {
    let contract_events: Vec<_> = events
        .iter()
        .filter(|e| e.contract_id == contract_id && e.network == network)
        .collect();

    if contract_events.is_empty() {
        return HealthScore {
            contract_id: contract_id.to_string(),
            network: network.to_string(),
            overall_score: 50.0,
            reliability_score: 50.0,
            performance_score: 50.0,
            activity_score: 0.0,
            risk_level: "unknown".to_string(),
            issues: vec!["No deployment history found".to_string()],
            strengths: vec![],
        };
    }

    let total = contract_events.len();
    let successful = contract_events.iter().filter(|e| e.success).count();
    let success_rate = successful as f64 / total as f64;

    // Reliability score (based on success rate)
    let reliability_score = success_rate * 100.0;

    // Performance score (based on fee efficiency)
    let fees: Vec<u64> = contract_events
        .iter()
        .filter_map(|e| e.fee_stroops)
        .collect();
    let performance_score = if !fees.is_empty() {
        let avg_fee = fees.iter().sum::<u64>() as f64 / fees.len() as f64;
        // Lower fees = better performance score (baseline is 5000 stroops)
        ((10000.0 - avg_fee) / 10000.0 * 100.0).clamp(0.0, 100.0)
    } else {
        50.0
    };

    // Activity score (based on deployment frequency)
    let now = Utc::now();
    let last_deployment = contract_events
        .iter()
        .filter_map(|e| chrono::DateTime::parse_from_rfc3339(&e.timestamp).ok())
        .max();

    let activity_score = if let Some(last) = last_deployment {
        let days_since = (now - last.with_timezone(&Utc)).num_days();
        if days_since <= 7 {
            100.0
        } else if days_since <= 30 {
            70.0
        } else if days_since <= 90 {
            40.0
        } else {
            10.0
        }
    } else {
        0.0
    };

    // Calculate overall score (weighted average)
    let overall_score = reliability_score * 0.5 + performance_score * 0.3 + activity_score * 0.2;

    // Determine risk level
    let risk_level = if overall_score >= 80.0 {
        "low"
    } else if overall_score >= 60.0 {
        "medium"
    } else {
        "high"
    }
    .to_string();

    // Identify issues and strengths
    let mut issues = Vec::new();
    let mut strengths = Vec::new();

    if success_rate < 0.7 {
        issues.push(format!("Low success rate: {:.1}%", success_rate * 100.0));
    } else if success_rate >= 0.95 {
        strengths.push(format!(
            "Excellent success rate: {:.1}%",
            success_rate * 100.0
        ));
    }

    if activity_score < 40.0 {
        issues.push("Low deployment activity in recent period".to_string());
    } else if activity_score >= 70.0 {
        strengths.push("Active deployment schedule".to_string());
    }

    if performance_score < 50.0 {
        issues.push("High deployment costs detected".to_string());
    } else if performance_score >= 80.0 {
        strengths.push("Efficient deployment costs".to_string());
    }

    let recent_failures = contract_events
        .iter()
        .rev()
        .take(5)
        .filter(|e| !e.success)
        .count();
    if recent_failures >= 2 {
        issues.push(format!("{} recent deployment failures", recent_failures));
    }

    if issues.is_empty() {
        issues.push("No significant issues detected".to_string());
    }

    HealthScore {
        contract_id: contract_id.to_string(),
        network: network.to_string(),
        overall_score,
        reliability_score,
        performance_score,
        activity_score,
        risk_level,
        issues,
        strengths,
    }
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
        AnalyticsCommands::Trends(args) => handle_trends(args),
        AnalyticsCommands::Predict(args) => handle_predict(args),
        AnalyticsCommands::Health(args) => handle_health(args),
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
        .filter(|e| args.network.as_deref().map_or(true, |n| e.network == n))
        .filter(|e| {
            args.contract_id
                .as_deref()
                .map_or(true, |c| e.contract_id == c)
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
        .filter(|e| args.network.as_deref().map_or(true, |n| e.network == n))
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

// Issue #545: New Command Handlers

fn handle_trends(args: TrendsArgs) -> Result<()> {
    p::header("Deployment Trend Analysis");
    config::validate_network(&args.network)?;

    let events = load_events()?;
    let analysis = analyze_trends(
        &events,
        &args.network,
        args.contract_id.as_deref(),
        args.days,
    );

    if args.json {
        println!("{}", serde_json::to_string_pretty(&analysis)?);
        return Ok(());
    }

    p::separator();
    if let Some(ref c) = analysis.contract_id {
        p::kv("Contract", c);
    }
    p::kv("Network", &analysis.network);
    p::kv(
        "Time window",
        &format!("{} days", analysis.time_window_days),
    );
    p::separator();

    println!("  {}", "Key Metrics".bright_white().bold());
    println!();
    p::kv(
        "Deployment frequency",
        &format!("{:.2} per day", analysis.deployment_frequency),
    );
    p::kv(
        "Success rate trend",
        &match analysis.success_rate_trend.as_str() {
            "improving" => analysis.success_rate_trend.green().to_string(),
            "declining" => analysis.success_rate_trend.red().to_string(),
            _ => analysis.success_rate_trend.yellow().to_string(),
        },
    );
    p::kv(
        "Average fee trend",
        &match analysis.avg_fee_trend.as_str() {
            "increasing" => analysis.avg_fee_trend.red().to_string(),
            "decreasing" => analysis.avg_fee_trend.green().to_string(),
            _ => analysis.avg_fee_trend.yellow().to_string(),
        },
    );
    p::kv("Recent failures", &format!("{}", analysis.recent_failures));
    p::kv(
        "Deployment velocity",
        &format!("{:+.2} deployments/day", analysis.deployment_velocity),
    );
    p::kv("Health score", &format!("{:.1}/100", analysis.health_score));

    println!();
    println!("  {}", "Predictions".bright_white().bold());
    println!();
    let pred = &analysis.predictions;
    let deploy_str = if pred.next_deployment_likely_success {
        format!("{}", "Likely ✓".green())
    } else {
        format!("{}", "At risk ✗".red())
    };
    p::kv("Next deployment success", &deploy_str);
    p::kv(
        "Predicted fee range",
        &format!(
            "{} - {} stroops ({:.5} - {:.5} XLM)",
            pred.predicted_fee_range.0,
            pred.predicted_fee_range.1,
            pred.predicted_fee_range.0 as f64 / 10_000_000.0,
            pred.predicted_fee_range.1 as f64 / 10_000_000.0
        ),
    );

    if !pred.risk_factors.is_empty() {
        println!();
        println!(
            "  {} {}",
            "Risk Factors:".red().bold(),
            format!("({})", pred.risk_factors.len()).red()
        );
        for risk in &pred.risk_factors {
            println!("    {} {}", "⚠".yellow(), risk.dimmed());
        }
    }

    if !pred.recommendations.is_empty() {
        println!();
        println!("  {}", "Recommendations:".cyan().bold());
        for (i, rec) in pred.recommendations.iter().enumerate() {
            println!("    {}. {}", i + 1, rec.white());
        }
    }

    p::separator();
    Ok(())
}

fn handle_predict(args: PredictArgs) -> Result<()> {
    p::header("Deployment Prediction");
    config::validate_network(&args.network)?;

    let events = load_events()?;
    let analysis = analyze_trends(&events, &args.network, Some(&args.contract_id), 30);

    if args.json {
        println!("{}", serde_json::to_string_pretty(&analysis.predictions)?);
        return Ok(());
    }

    p::separator();
    p::kv("Contract", &args.contract_id);
    p::kv("Network", &args.network);
    p::separator();

    let pred = &analysis.predictions;
    println!("  {}", "Prediction Results".bright_white().bold());
    println!();

    let success_icon = if pred.next_deployment_likely_success {
        "✓"
    } else {
        "✗"
    };
    let success_color = if pred.next_deployment_likely_success {
        "green"
    } else {
        "red"
    };

    println!(
        "  {:<25}  {}",
        "Success probability".bright_white(),
        format!(
            "{} {}",
            success_icon,
            if pred.next_deployment_likely_success {
                "High"
            } else {
                "Low"
            }
        )
        .color(success_color)
        .bold()
    );

    println!(
        "  {:<25}  {}",
        "Estimated fee range".bright_white(),
        format!(
            "{} - {} stroops",
            pred.predicted_fee_range.0, pred.predicted_fee_range.1
        )
        .white()
    );

    println!(
        "  {:<25}  {}",
        "Est. cost (XLM)".bright_white(),
        format!(
            "{:.5} - {:.5}",
            pred.predicted_fee_range.0 as f64 / 10_000_000.0,
            pred.predicted_fee_range.1 as f64 / 10_000_000.0
        )
        .white()
    );

    if let Some(size_kb) = args.wasm_size_kb {
        let optimized = if size_kb > 64.0 {
            "Consider optimization"
        } else {
            "Size is acceptable"
        };
        println!(
            "  {:<25}  {} ({})",
            "WASM size".bright_white(),
            format!("{:.1} KB", size_kb).white(),
            optimized.dimmed()
        );
    }

    if !pred.risk_factors.is_empty() {
        println!();
        println!("  {} {}", "⚠".yellow(), "Risk Factors".red().bold());
        for risk in &pred.risk_factors {
            println!("    • {}", risk.white());
        }
    }

    if !pred.recommendations.is_empty() {
        println!();
        println!("  {} {}", "💡".cyan(), "Recommendations".cyan().bold());
        for rec in &pred.recommendations {
            println!("    • {}", rec.white());
        }
    }

    p::separator();
    if pred.next_deployment_likely_success {
        p::success("Deployment conditions are favorable");
    } else {
        p::warn("Review risk factors before deploying");
    }
    Ok(())
}

fn handle_health(args: HealthArgs) -> Result<()> {
    p::header("Contract Health Score");
    config::validate_network(&args.network)?;

    let events = load_events()?;
    let health = calculate_contract_health(&events, &args.contract_id, &args.network);

    if args.json {
        println!("{}", serde_json::to_string_pretty(&health)?);
        return Ok(());
    }

    p::separator();
    p::kv("Contract", &health.contract_id);
    p::kv("Network", &health.network);
    p::separator();

    // Overall score with visual indicator
    let score_color = if health.overall_score >= 80.0 {
        "green"
    } else if health.overall_score >= 60.0 {
        "yellow"
    } else {
        "red"
    };

    println!("  {}", "Health Score".bright_white().bold());
    println!();
    println!(
        "  {:<25}  {} {}",
        "Overall Score".bright_white(),
        format!("{:.1}/100", health.overall_score)
            .color(score_color)
            .bold(),
        health_indicator(health.overall_score)
    );

    println!();
    println!("  {}", "Component Scores".dimmed());
    println!(
        "  {:<25}  {}",
        "  Reliability".white(),
        format!("{:.1}/100", health.reliability_score).white()
    );
    println!(
        "  {:<25}  {}",
        "  Performance".white(),
        format!("{:.1}/100", health.performance_score).white()
    );
    println!(
        "  {:<25}  {}",
        "  Activity".white(),
        format!("{:.1}/100", health.activity_score).white()
    );

    println!();
    let risk_color = match health.risk_level.as_str() {
        "low" => "green",
        "medium" => "yellow",
        _ => "red",
    };
    println!(
        "  {:<25}  {}",
        "Risk Level".bright_white(),
        health.risk_level.to_uppercase().color(risk_color).bold()
    );

    if !health.issues.is_empty() {
        println!();
        println!(
            "  {} {}",
            "Issues".red().bold(),
            format!("({})", health.issues.len()).red()
        );
        for issue in &health.issues {
            println!("    {} {}", "✗".red(), issue.white());
        }
    }

    if !health.strengths.is_empty() {
        println!();
        println!(
            "  {} {}",
            "Strengths".green().bold(),
            format!("({})", health.strengths.len()).green()
        );
        for strength in &health.strengths {
            println!("    {} {}", "✓".green(), strength.white());
        }
    }

    p::separator();

    // Provide actionable recommendations
    if health.overall_score < 60.0 {
        p::warn("Low health score detected - review issues and take corrective action");
        p::info("Run `starforge analytics trends` for detailed trend analysis");
    } else if health.overall_score < 80.0 {
        p::info("Health score is acceptable - monitor for improvements");
    } else {
        p::success("Excellent health score - deployment practices are optimal");
    }

    Ok(())
}

fn health_indicator(score: f64) -> String {
    if score >= 90.0 {
        "🟢 Excellent".to_string()
    } else if score >= 80.0 {
        "🟢 Good".to_string()
    } else if score >= 70.0 {
        "🟡 Fair".to_string()
    } else if score >= 60.0 {
        "🟡 Needs Improvement".to_string()
    } else {
        "🔴 Poor".to_string()
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
}
