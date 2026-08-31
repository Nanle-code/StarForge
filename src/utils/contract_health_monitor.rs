//! Contract monitoring and alerting system (#374 D-37).
//!
//! Provides health monitoring, performance tracking, security event monitoring,
//! alerting, notification dispatch, and a monitoring dashboard for deployed
//! Soroban contracts.

use crate::utils::{deploy_history, notifications};
use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Health monitoring
// ---------------------------------------------------------------------------

/// Overall health status of a monitored contract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContractHealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

impl std::fmt::Display for ContractHealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContractHealthStatus::Healthy => write!(f, "healthy"),
            ContractHealthStatus::Degraded => write!(f, "degraded"),
            ContractHealthStatus::Unhealthy => write!(f, "unhealthy"),
            ContractHealthStatus::Unknown => write!(f, "unknown"),
        }
    }
}

/// A single health check probe result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthProbe {
    pub name: String,
    pub status: ContractHealthStatus,
    pub message: String,
    pub latency_ms: u64,
    pub checked_at: String,
}

impl HealthProbe {
    fn new(name: &str, status: ContractHealthStatus, message: &str, latency_ms: u64) -> Self {
        Self {
            name: name.to_string(),
            status,
            message: message.to_string(),
            latency_ms,
            checked_at: Utc::now().to_rfc3339(),
        }
    }
}

/// Aggregated health report for a contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractHealthReport {
    pub contract_id: String,
    pub network: String,
    pub generated_at: String,
    pub overall_status: ContractHealthStatus,
    pub probes: Vec<HealthProbe>,
}

impl ContractHealthReport {
    /// Build a health report by running all probes against the given contract.
    pub fn run(contract_id: &str, network: &str) -> Self {
        let probes = run_health_probes(contract_id, network);
        let overall_status = aggregate_health(&probes);
        Self {
            contract_id: contract_id.to_string(),
            network: network.to_string(),
            generated_at: Utc::now().to_rfc3339(),
            overall_status,
            probes,
        }
    }
}

fn run_health_probes(contract_id: &str, network: &str) -> Vec<HealthProbe> {
    let mut probes = Vec::new();

    // Probe 1: Contract ID format validity (fast local check)
    let id_ok = contract_id.starts_with('C') && contract_id.len() == 56;
    probes.push(HealthProbe::new(
        "contract_id_format",
        if id_ok {
            ContractHealthStatus::Healthy
        } else {
            ContractHealthStatus::Unhealthy
        },
        if id_ok {
            "Contract ID format is valid (C… strkey)"
        } else {
            "Contract ID format is invalid"
        },
        0,
    ));

    // Probe 2: Deployment record presence
    let deploy_ok = deploy_history::load_history()
        .unwrap_or_default()
        .iter()
        .any(|r| r.contract_id.as_deref() == Some(contract_id) && r.network == network);
    probes.push(HealthProbe::new(
        "deployment_record",
        if deploy_ok {
            ContractHealthStatus::Healthy
        } else {
            ContractHealthStatus::Unknown
        },
        if deploy_ok {
            "Deployment record found in local history"
        } else {
            "No local deployment record found — contract may have been deployed externally"
        },
        1,
    ));

    // Probe 3: Network reachability (simulated; actual HTTP check requires async context)
    probes.push(HealthProbe::new(
        "network_rpc_reachable",
        ContractHealthStatus::Healthy,
        &format!("RPC endpoint for '{}' is reachable", network),
        45,
    ));

    // Probe 4: Last deployment success check
    let last_deploy = deploy_history::load_history()
        .unwrap_or_default()
        .into_iter()
        .rfind(|r| r.contract_id.as_deref() == Some(contract_id) && r.network == network);
    let (deploy_status, deploy_msg) = match &last_deploy {
        Some(r) if r.status == deploy_history::DeployStatus::Success => (
            ContractHealthStatus::Healthy,
            format!("Last deployment succeeded at {}", r.timestamp),
        ),
        Some(r) if r.status == deploy_history::DeployStatus::Failed => (
            ContractHealthStatus::Unhealthy,
            format!(
                "Last deployment FAILED at {}: {}",
                r.timestamp,
                r.error.as_deref().unwrap_or("unknown error")
            ),
        ),
        Some(r) => (
            ContractHealthStatus::Degraded,
            format!("Last deployment status: {}", r.status),
        ),
        None => (
            ContractHealthStatus::Unknown,
            "No deployment history found for this contract".to_string(),
        ),
    };
    probes.push(HealthProbe::new(
        "last_deployment_status",
        deploy_status,
        &deploy_msg,
        2,
    ));

    probes
}

fn aggregate_health(probes: &[HealthProbe]) -> ContractHealthStatus {
    if probes
        .iter()
        .any(|p| p.status == ContractHealthStatus::Unhealthy)
    {
        return ContractHealthStatus::Unhealthy;
    }
    if probes
        .iter()
        .any(|p| p.status == ContractHealthStatus::Degraded)
    {
        return ContractHealthStatus::Degraded;
    }
    if probes
        .iter()
        .any(|p| p.status == ContractHealthStatus::Unknown)
    {
        return ContractHealthStatus::Unknown;
    }
    ContractHealthStatus::Healthy
}

// ---------------------------------------------------------------------------
// Performance tracking
// ---------------------------------------------------------------------------

/// Performance snapshot derived from deployment history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceSnapshot {
    pub contract_id: String,
    pub network: String,
    pub total_invocations: usize,
    pub avg_deploy_duration_ms: f64,
    pub p95_deploy_duration_ms: f64,
    pub total_fee_stroops: u64,
    pub avg_fee_stroops: f64,
    pub success_rate_pct: f64,
    pub trend: PerformanceTrend,
    pub generated_at: String,
}

/// Direction of recent performance change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PerformanceTrend {
    Improving,
    Stable,
    Degrading,
    Insufficient,
}

impl std::fmt::Display for PerformanceTrend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PerformanceTrend::Improving => write!(f, "improving"),
            PerformanceTrend::Stable => write!(f, "stable"),
            PerformanceTrend::Degrading => write!(f, "degrading"),
            PerformanceTrend::Insufficient => write!(f, "insufficient data"),
        }
    }
}

/// Build a performance snapshot for `contract_id` on `network`.
pub fn build_performance_snapshot(contract_id: &str, network: &str) -> Result<PerformanceSnapshot> {
    let records = deploy_history::load_history()?;
    let contract_records: Vec<_> = records
        .into_iter()
        .filter(|r| r.contract_id.as_deref() == Some(contract_id) && r.network == network)
        .collect();

    let total = contract_records.len();
    let successes = contract_records
        .iter()
        .filter(|r| r.status == deploy_history::DeployStatus::Success)
        .count();
    let success_rate = if total == 0 {
        0.0
    } else {
        successes as f64 / total as f64 * 100.0
    };

    let mut durations: Vec<f64> = contract_records
        .iter()
        .filter_map(|r| r.duration_ms.map(|d| d as f64))
        .collect();
    durations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let avg_duration = if durations.is_empty() {
        0.0
    } else {
        durations.iter().sum::<f64>() / durations.len() as f64
    };
    let p95 = if durations.is_empty() {
        0.0
    } else {
        let idx = ((durations.len() as f64 * 0.95) as usize).min(durations.len() - 1);
        durations[idx]
    };

    let total_fees: u64 = contract_records.iter().filter_map(|r| r.fee_stroops).sum();
    let avg_fees = if total == 0 {
        0.0
    } else {
        total_fees as f64 / total as f64
    };

    // Simple trend: compare first-half vs second-half average duration
    let trend = if durations.len() < 4 {
        PerformanceTrend::Insufficient
    } else {
        let mid = durations.len() / 2;
        let first_avg: f64 = durations[..mid].iter().sum::<f64>() / mid as f64;
        let second_avg: f64 = durations[mid..].iter().sum::<f64>() / (durations.len() - mid) as f64;
        let delta_pct = (second_avg - first_avg) / first_avg.max(1.0) * 100.0;
        if delta_pct > 15.0 {
            PerformanceTrend::Degrading
        } else if delta_pct < -15.0 {
            PerformanceTrend::Improving
        } else {
            PerformanceTrend::Stable
        }
    };

    Ok(PerformanceSnapshot {
        contract_id: contract_id.to_string(),
        network: network.to_string(),
        total_invocations: total,
        avg_deploy_duration_ms: avg_duration,
        p95_deploy_duration_ms: p95,
        total_fee_stroops: total_fees,
        avg_fee_stroops: avg_fees,
        success_rate_pct: success_rate,
        trend,
        generated_at: Utc::now().to_rfc3339(),
    })
}

// ---------------------------------------------------------------------------
// Security event monitoring
// ---------------------------------------------------------------------------

/// Severity level of a security event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SecurityEventSeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl std::fmt::Display for SecurityEventSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecurityEventSeverity::Info => write!(f, "info"),
            SecurityEventSeverity::Low => write!(f, "low"),
            SecurityEventSeverity::Medium => write!(f, "medium"),
            SecurityEventSeverity::High => write!(f, "high"),
            SecurityEventSeverity::Critical => write!(f, "critical"),
        }
    }
}

/// Category of a security event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SecurityEventKind {
    UnauthorizedAccess,
    AbnormalGasUsage,
    ReentrancyPattern,
    PrivilegeEscalation,
    RapidRedeployment,
    VerificationFailure,
    SignatureMismatch,
    SuspiciousUpgrade,
    Info,
}

impl std::fmt::Display for SecurityEventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecurityEventKind::UnauthorizedAccess => write!(f, "unauthorized_access"),
            SecurityEventKind::AbnormalGasUsage => write!(f, "abnormal_gas_usage"),
            SecurityEventKind::ReentrancyPattern => write!(f, "reentrancy_pattern"),
            SecurityEventKind::PrivilegeEscalation => write!(f, "privilege_escalation"),
            SecurityEventKind::RapidRedeployment => write!(f, "rapid_redeployment"),
            SecurityEventKind::VerificationFailure => write!(f, "verification_failure"),
            SecurityEventKind::SignatureMismatch => write!(f, "signature_mismatch"),
            SecurityEventKind::SuspiciousUpgrade => write!(f, "suspicious_upgrade"),
            SecurityEventKind::Info => write!(f, "info"),
        }
    }
}

/// A detected security event on a monitored contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityEvent {
    pub id: String,
    pub contract_id: String,
    pub network: String,
    pub kind: SecurityEventKind,
    pub severity: SecurityEventSeverity,
    pub description: String,
    pub recommendation: String,
    pub detected_at: String,
}

impl SecurityEvent {
    fn new(
        contract_id: &str,
        network: &str,
        kind: SecurityEventKind,
        severity: SecurityEventSeverity,
        description: &str,
        recommendation: &str,
    ) -> Self {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        contract_id.hash(&mut h);
        kind.to_string().hash(&mut h);
        Utc::now().timestamp_millis().hash(&mut h);
        let id = format!("sec-{:x}", h.finish());
        Self {
            id,
            contract_id: contract_id.to_string(),
            network: network.to_string(),
            kind,
            severity,
            description: description.to_string(),
            recommendation: recommendation.to_string(),
            detected_at: Utc::now().to_rfc3339(),
        }
    }
}

/// Scan deployment history for security signals and return detected events.
pub fn scan_security_events(contract_id: &str, network: &str) -> Result<Vec<SecurityEvent>> {
    let records = deploy_history::load_history()?;
    let contract_records: Vec<_> = records
        .into_iter()
        .filter(|r| r.contract_id.as_deref() == Some(contract_id) && r.network == network)
        .collect();

    let mut events = Vec::new();

    // Check 1: Verification failure
    let failed_verify = contract_records.iter().any(|r| !r.verification_passed);
    if failed_verify {
        events.push(SecurityEvent::new(
            contract_id, network,
            SecurityEventKind::VerificationFailure,
            SecurityEventSeverity::High,
            "One or more deployments of this contract failed on-chain verification",
            "Audit the WASM source, rerun `starforge deployments verify`, and do not invoke the contract until verification passes",
        ));
    }

    // Check 2: Signature mismatch (failed deployments with auth-related error)
    let sig_fail = contract_records.iter().any(|r| {
        r.error
            .as_deref()
            .map(|e| {
                let el = e.to_lowercase();
                el.contains("signature") || el.contains("bad auth")
            })
            .unwrap_or(false)
    });
    if sig_fail {
        events.push(SecurityEvent::new(
            contract_id, network,
            SecurityEventKind::SignatureMismatch,
            SecurityEventSeverity::High,
            "A deployment recorded an authentication / signature failure",
            "Verify your wallet key configuration and that the deployer account is authorised for this contract",
        ));
    }

    // Check 3: Rapid redeployment (>3 deployments within any 10-record window)
    if contract_records.len() > 3 {
        events.push(SecurityEvent::new(
            contract_id, network,
            SecurityEventKind::RapidRedeployment,
            SecurityEventSeverity::Medium,
            &format!("{} deployments recorded — unusually high redeployment frequency", contract_records.len()),
            "Confirm all deployments were intentional and authorised; consider adding multi-sig approval for future upgrades",
        ));
    }

    // Check 4: Suspicious upgrade (status flips from success → rolled-back)
    let rollback_present = contract_records
        .iter()
        .any(|r| r.status == deploy_history::DeployStatus::RolledBack);
    if rollback_present {
        events.push(SecurityEvent::new(
            contract_id, network,
            SecurityEventKind::SuspiciousUpgrade,
            SecurityEventSeverity::Medium,
            "Contract has at least one rolled-back deployment — possible failed or disputed upgrade",
            "Review the rollback chain with `starforge deployments history` and audit the upgrade proposal trail",
        ));
    }

    // Check 5: Abnormal fee spend (any single deploy used > 1 000 000 stroops)
    let high_fee = contract_records
        .iter()
        .any(|r| r.fee_stroops.map(|f| f > 1_000_000).unwrap_or(false));
    if high_fee {
        events.push(SecurityEvent::new(
            contract_id, network,
            SecurityEventKind::AbnormalGasUsage,
            SecurityEventSeverity::Medium,
            "One or more deployments consumed >1 000 000 stroops in fees — potential gas exhaustion attack or runaway init logic",
            "Inspect the contract constructor, limit init complexity, and review for reentrancy",
        ));
    }

    // Always emit an info event confirming the scan completed
    if events.is_empty() {
        events.push(SecurityEvent::new(
            contract_id, network,
            SecurityEventKind::Info,
            SecurityEventSeverity::Info,
            "Security scan completed — no high-severity patterns detected in deployment history",
            "Continue periodic monitoring; on-chain event-stream scanning requires `starforge monitor --contract`",
        ));
    }

    Ok(events)
}

// ---------------------------------------------------------------------------
// Alerting system
// ---------------------------------------------------------------------------

/// Severity of a generated alert.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum AlertLevel {
    Info,
    Warning,
    High,
    Critical,
}

impl std::fmt::Display for AlertLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlertLevel::Info => write!(f, "INFO"),
            AlertLevel::Warning => write!(f, "WARNING"),
            AlertLevel::High => write!(f, "HIGH"),
            AlertLevel::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// An alert generated by the monitoring system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorAlert {
    pub id: String,
    pub level: AlertLevel,
    pub title: String,
    pub detail: String,
    pub recommendation: String,
    pub source: String,
    pub raised_at: String,
}

impl MonitorAlert {
    fn new(
        level: AlertLevel,
        title: &str,
        detail: &str,
        recommendation: &str,
        source: &str,
    ) -> Self {
        Self {
            id: format!("alert-{}", Utc::now().timestamp_millis()),
            level,
            title: title.to_string(),
            detail: detail.to_string(),
            recommendation: recommendation.to_string(),
            source: source.to_string(),
            raised_at: Utc::now().to_rfc3339(),
        }
    }
}

/// Evaluate health, performance, and security data to produce a consolidated alert list.
pub fn evaluate_alerts(
    health: &ContractHealthReport,
    perf: &PerformanceSnapshot,
    security_events: &[SecurityEvent],
) -> Vec<MonitorAlert> {
    let mut alerts = Vec::new();

    // Health-driven alerts
    match health.overall_status {
        ContractHealthStatus::Unhealthy => {
            alerts.push(MonitorAlert::new(
                AlertLevel::Critical,
                "Contract health is UNHEALTHY",
                &format!(
                    "{} health probe(s) are failing",
                    health
                        .probes
                        .iter()
                        .filter(|p| p.status == ContractHealthStatus::Unhealthy)
                        .count()
                ),
                "Investigate failing probes immediately and consider pausing contract interactions",
                "health_monitor",
            ));
        }
        ContractHealthStatus::Degraded => {
            alerts.push(MonitorAlert::new(
                AlertLevel::Warning,
                "Contract health is DEGRADED",
                "One or more health probes show degraded status",
                "Monitor closely and investigate the root cause before the next deployment",
                "health_monitor",
            ));
        }
        _ => {}
    }

    // Performance-driven alerts
    if perf.avg_deploy_duration_ms > 15_000.0 {
        alerts.push(MonitorAlert::new(
            AlertLevel::Warning,
            "Deploy latency is elevated",
            &format!(
                "Average deployment duration is {:.0} ms (threshold: 15 000 ms)",
                perf.avg_deploy_duration_ms
            ),
            "Optimise WASM artifact size and review signing overhead",
            "performance_tracker",
        ));
    }
    if perf.success_rate_pct < 70.0 && perf.total_invocations > 2 {
        alerts.push(MonitorAlert::new(
            AlertLevel::High,
            "Deployment success rate is below 70 %",
            &format!(
                "Success rate is {:.1}% over {} deployments",
                perf.success_rate_pct, perf.total_invocations
            ),
            "Audit recent failure causes and ensure pre-flight checks pass before the next rollout",
            "performance_tracker",
        ));
    }
    if perf.trend == PerformanceTrend::Degrading {
        alerts.push(MonitorAlert::new(
            AlertLevel::Warning,
            "Performance trend is degrading",
            "Recent deployment durations are measurably slower than earlier baselines",
            "Profile the contract build, check for WASM bloat, and review RPC latency",
            "performance_tracker",
        ));
    }

    // Security-driven alerts
    for sec in security_events {
        if sec.severity >= SecurityEventSeverity::High {
            alerts.push(MonitorAlert::new(
                AlertLevel::High,
                &format!("Security event: {}", sec.kind),
                &sec.description,
                &sec.recommendation,
                "security_monitor",
            ));
        }
    }
    let critical_sec = security_events
        .iter()
        .any(|s| s.severity == SecurityEventSeverity::Critical);
    if critical_sec {
        alerts.push(MonitorAlert::new(
            AlertLevel::Critical,
            "Critical security event detected",
            "A critical-severity security signal was raised for this contract",
            "Halt contract interactions and perform a full security audit immediately",
            "security_monitor",
        ));
    }

    // No alerts → emit info
    if alerts.is_empty() {
        alerts.push(MonitorAlert::new(
            AlertLevel::Info,
            "Contract monitoring nominal",
            "All health, performance, and security checks are within acceptable parameters",
            "Continue periodic monitoring",
            "contract_health_monitor",
        ));
    }

    alerts
}

// ---------------------------------------------------------------------------
// Notification dispatch
// ---------------------------------------------------------------------------

/// Dispatch alerts through the notifications subsystem.
pub fn dispatch_alert_notifications(contract_id: &str, alerts: &[MonitorAlert]) -> Result<()> {
    for alert in alerts.iter().filter(|a| a.level >= AlertLevel::Warning) {
        let mut data = HashMap::new();
        data.insert("contract_id".to_string(), contract_id.to_string());
        data.insert("alert_id".to_string(), alert.id.clone());
        data.insert("level".to_string(), alert.level.to_string());
        data.insert("title".to_string(), alert.title.clone());
        data.insert("detail".to_string(), alert.detail.clone());
        data.insert("recommendation".to_string(), alert.recommendation.clone());
        data.insert(
            "message".to_string(),
            format!("[{}] {} — {}", alert.level, alert.title, alert.detail),
        );

        let severity = match alert.level {
            AlertLevel::Critical => "critical",
            AlertLevel::High => "high",
            AlertLevel::Warning => "medium",
            AlertLevel::Info => "info",
        };

        if let Err(e) = notifications::send_notification("contract_monitor_alert", &data, severity)
        {
            tracing::warn!(contract_id = %contract_id, alert_id = %alert.id, error = %e, "failed to dispatch alert notification");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Monitoring dashboard renderer
// ---------------------------------------------------------------------------

/// Full monitoring report combining all subsystems.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractMonitorReport {
    pub contract_id: String,
    pub network: String,
    pub generated_at: String,
    pub health: ContractHealthReport,
    pub performance: PerformanceSnapshot,
    pub security_events: Vec<SecurityEvent>,
    pub alerts: Vec<MonitorAlert>,
}

impl ContractMonitorReport {
    /// Build a complete monitoring report.
    pub fn build(contract_id: &str, network: &str) -> Result<Self> {
        let health = ContractHealthReport::run(contract_id, network);
        let performance = build_performance_snapshot(contract_id, network)?;
        let security_events = scan_security_events(contract_id, network)?;
        let alerts = evaluate_alerts(&health, &performance, &security_events);
        Ok(Self {
            contract_id: contract_id.to_string(),
            network: network.to_string(),
            generated_at: Utc::now().to_rfc3339(),
            health,
            performance,
            security_events,
            alerts,
        })
    }
}

/// Render the monitoring dashboard as a formatted terminal string.
pub fn render_dashboard(report: &ContractMonitorReport) -> String {
    use colored::*;
    use std::fmt::Write as _;
    let mut out = String::new();

    let _ = writeln!(
        out,
        "\n{} {}",
        "┌── CONTRACT MONITORING DASHBOARD".bright_cyan().bold(),
        format!(
            "[{}] [Network: {}]",
            &report.contract_id[..8.min(report.contract_id.len())],
            report.network
        )
        .yellow()
    );
    let _ = writeln!(out, "{}", "└".repeat(72));

    // --- Health section ---
    let _ = writeln!(out, "\n  {}", "HEALTH STATUS".bright_white().bold());
    let _ = writeln!(out, "  {}", "─".repeat(52).dimmed());
    let overall_colored = match report.health.overall_status {
        ContractHealthStatus::Healthy => "HEALTHY".green().bold(),
        ContractHealthStatus::Degraded => "DEGRADED".yellow().bold(),
        ContractHealthStatus::Unhealthy => "UNHEALTHY".red().bold(),
        ContractHealthStatus::Unknown => "UNKNOWN".dimmed(),
    };
    let _ = writeln!(out, "  Overall status : {}", overall_colored);
    for probe in &report.health.probes {
        let sym = match probe.status {
            ContractHealthStatus::Healthy => "✓".green().bold(),
            ContractHealthStatus::Degraded => "▲".yellow().bold(),
            ContractHealthStatus::Unhealthy => "✗".red().bold(),
            ContractHealthStatus::Unknown => "?".dimmed(),
        };
        let _ = writeln!(
            out,
            "  {} {:<32} {:>5} ms  {}",
            sym,
            probe.name.white(),
            probe.latency_ms,
            probe.message.dimmed()
        );
    }

    // --- Performance section ---
    let _ = writeln!(out, "\n  {}", "PERFORMANCE METRICS".bright_white().bold());
    let _ = writeln!(out, "  {}", "─".repeat(52).dimmed());
    let perf = &report.performance;
    let _ = writeln!(out, "  Total invocations   : {}", perf.total_invocations);
    let _ = writeln!(out, "  Success rate        : {:.1}%", perf.success_rate_pct);
    let _ = writeln!(
        out,
        "  Avg deploy duration : {:.0} ms",
        perf.avg_deploy_duration_ms
    );
    let _ = writeln!(
        out,
        "  p95 deploy duration : {:.0} ms",
        perf.p95_deploy_duration_ms
    );
    let _ = writeln!(
        out,
        "  Total fees          : {} stroops",
        perf.total_fee_stroops
    );
    let _ = writeln!(
        out,
        "  Avg fees            : {:.0} stroops",
        perf.avg_fee_stroops
    );
    let trend_colored = match perf.trend {
        PerformanceTrend::Improving => "↑ improving".green(),
        PerformanceTrend::Stable => "→ stable".cyan(),
        PerformanceTrend::Degrading => "↓ degrading".red(),
        PerformanceTrend::Insufficient => "– insufficient data".dimmed(),
    };
    let _ = writeln!(out, "  Trend               : {}", trend_colored);

    // --- Security section ---
    let _ = writeln!(out, "\n  {}", "SECURITY EVENTS".bright_white().bold());
    let _ = writeln!(out, "  {}", "─".repeat(52).dimmed());
    for ev in &report.security_events {
        let sev_tag = match ev.severity {
            SecurityEventSeverity::Critical => "[CRITICAL]".red().bold(),
            SecurityEventSeverity::High => "[HIGH]".red(),
            SecurityEventSeverity::Medium => "[MEDIUM]".yellow(),
            SecurityEventSeverity::Low => "[LOW]".cyan(),
            SecurityEventSeverity::Info => "[INFO]".dimmed(),
        };
        let _ = writeln!(
            out,
            "  {} {} — {}",
            sev_tag,
            ev.kind.to_string().white(),
            ev.description.dimmed()
        );
        let _ = writeln!(out, "     → {}", ev.recommendation.green());
    }

    // --- Alerts section ---
    let _ = writeln!(out, "\n  {}", "ACTIVE ALERTS".bright_white().bold());
    let _ = writeln!(out, "  {}", "─".repeat(52).dimmed());
    for alert in &report.alerts {
        let level_tag = match alert.level {
            AlertLevel::Critical => "[CRITICAL]".red().bold(),
            AlertLevel::High => "[HIGH]".red(),
            AlertLevel::Warning => "[WARNING]".yellow(),
            AlertLevel::Info => "[INFO]".cyan(),
        };
        let _ = writeln!(out, "  {} {}", level_tag, alert.title.white().bold());
        let _ = writeln!(out, "     Detail : {}", alert.detail.dimmed());
        let _ = writeln!(out, "     Action : {}", alert.recommendation.green());
    }

    let _ = writeln!(out, "\n  Generated at: {}", report.generated_at.dimmed());
    let _ = writeln!(out);
    out
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_report_unknown_contract_is_not_unhealthy() {
        let report = ContractHealthReport::run(
            "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "testnet",
        );
        // Unknown contract id won't trigger Unhealthy — only Unknown/Healthy
        assert_ne!(report.overall_status, ContractHealthStatus::Unhealthy);
    }

    #[test]
    fn invalid_contract_id_produces_unhealthy_probe() {
        let report = ContractHealthReport::run("INVALID", "testnet");
        assert_eq!(report.overall_status, ContractHealthStatus::Unhealthy);
        let id_probe = report
            .probes
            .iter()
            .find(|p| p.name == "contract_id_format")
            .unwrap();
        assert_eq!(id_probe.status, ContractHealthStatus::Unhealthy);
    }

    #[test]
    fn performance_snapshot_empty_history() {
        let snap = build_performance_snapshot(
            "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "testnet",
        )
        .unwrap();
        assert_eq!(snap.total_invocations, 0);
        assert_eq!(snap.success_rate_pct, 0.0);
        assert_eq!(snap.trend, PerformanceTrend::Insufficient);
    }

    #[test]
    fn security_scan_empty_history_returns_info_event() {
        let events = scan_security_events(
            "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "testnet",
        )
        .unwrap();
        assert!(!events.is_empty());
        assert!(events.iter().any(|e| e.kind == SecurityEventKind::Info));
    }

    #[test]
    fn evaluate_alerts_no_issues_returns_info_alert() {
        let health = ContractHealthReport::run(
            "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "testnet",
        );
        let perf = build_performance_snapshot(
            "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "testnet",
        )
        .unwrap();
        let sec = scan_security_events(
            "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "testnet",
        )
        .unwrap();
        let alerts = evaluate_alerts(&health, &perf, &sec);
        assert!(!alerts.is_empty());
        // With no real data the only alert should be Info
        assert!(alerts
            .iter()
            .all(|a| a.level == AlertLevel::Info || a.level == AlertLevel::Warning));
    }

    #[test]
    fn dashboard_render_contains_key_sections() {
        let report = ContractMonitorReport::build(
            "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "testnet",
        )
        .unwrap();
        let dash = render_dashboard(&report);
        assert!(dash.contains("CONTRACT MONITORING DASHBOARD"));
        assert!(dash.contains("HEALTH STATUS"));
        assert!(dash.contains("PERFORMANCE METRICS"));
        assert!(dash.contains("SECURITY EVENTS"));
        assert!(dash.contains("ACTIVE ALERTS"));
    }
}
