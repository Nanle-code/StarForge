//! Contract deployment monitoring service.
//!
//! Builds on [`crate::utils::deploy_history`] to provide continuous health
//! monitoring of deployments: liveness probes, failure detection, alerting and
//! an aggregate dashboard.
//!
//! The assessment logic is split from the I/O so it can be unit-tested without
//! a network: pure functions ([`assess_health`], [`detect_failures`],
//! [`summarize`]) operate on records plus [`LivenessProbe`] outcomes, while the
//! command layer performs the actual RPC/Horizon probes.

use crate::utils::config;
use crate::utils::deploy_history::{DeployRecord, DeployStatus};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Health model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

impl HealthStatus {
    pub fn label(&self) -> &'static str {
        match self {
            HealthStatus::Healthy => "healthy",
            HealthStatus::Degraded => "degraded",
            HealthStatus::Unhealthy => "unhealthy",
            HealthStatus::Unknown => "unknown",
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            HealthStatus::Healthy => "✓",
            HealthStatus::Degraded => "◐",
            HealthStatus::Unhealthy => "✗",
            HealthStatus::Unknown => "?",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthCheck {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentHealth {
    pub deployment_id: String,
    pub contract_id: Option<String>,
    pub network: String,
    pub deploy_status: DeployStatus,
    pub health: HealthStatus,
    pub checks: Vec<HealthCheck>,
    pub checked_at: String,
}

/// The result of probing live infrastructure for one deployment. Kept separate
/// from the assessment so health logic is deterministic and testable.
#[derive(Debug, Clone, Default)]
pub struct LivenessProbe {
    /// Whether the target network's RPC/Horizon is reachable.
    pub network_reachable: bool,
    /// `Some(true/false)` if the contract's liveness was checked, `None` if
    /// it could not be (no contract id, or network unreachable).
    pub contract_live: Option<bool>,
    /// Age of the deployment record in seconds.
    pub age_secs: i64,
}

/// How long a `Pending` deployment may sit before it is considered stuck.
pub const STUCK_PENDING_SECS: i64 = 15 * 60;

/// Combine a deployment record with a liveness probe into a health assessment.
pub fn assess_health(record: &DeployRecord, probe: &LivenessProbe) -> DeploymentHealth {
    let mut checks = Vec::new();

    // 1. Recorded deploy status.
    let status_ok = matches!(record.status, DeployStatus::Success);
    checks.push(HealthCheck {
        name: "deploy-status".to_string(),
        passed: status_ok,
        detail: record.status.to_string(),
    });

    // 2. Network reachability.
    checks.push(HealthCheck {
        name: "network".to_string(),
        passed: probe.network_reachable,
        detail: if probe.network_reachable {
            format!("{} reachable", record.network)
        } else {
            format!("{} unreachable", record.network)
        },
    });

    // 3. On-chain contract liveness (when applicable).
    if let Some(live) = probe.contract_live {
        checks.push(HealthCheck {
            name: "contract-liveness".to_string(),
            passed: live,
            detail: if live {
                "contract responds on-chain".to_string()
            } else {
                "contract not found on-chain".to_string()
            },
        });
    }

    let health = derive_status(record, probe);
    DeploymentHealth {
        deployment_id: record.id.clone(),
        contract_id: record.contract_id.clone(),
        network: record.network.clone(),
        deploy_status: record.status.clone(),
        health,
        checks,
        checked_at: now_rfc3339(),
    }
}

fn derive_status(record: &DeployRecord, probe: &LivenessProbe) -> HealthStatus {
    match record.status {
        DeployStatus::Failed => HealthStatus::Unhealthy,
        DeployStatus::RolledBack => HealthStatus::Degraded,
        DeployStatus::Pending => {
            if probe.age_secs > STUCK_PENDING_SECS {
                HealthStatus::Unhealthy // stuck pending
            } else {
                HealthStatus::Degraded // awaiting confirmation
            }
        }
        DeployStatus::Success => {
            if !probe.network_reachable {
                // Can't confirm; don't claim healthy.
                return HealthStatus::Unknown;
            }
            match probe.contract_live {
                Some(true) => HealthStatus::Healthy,
                Some(false) => HealthStatus::Unhealthy, // deployed but missing on-chain
                None => HealthStatus::Healthy,          // network ok, nothing else to verify
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Failure detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureAlert {
    pub deployment_id: String,
    pub contract_id: Option<String>,
    pub network: String,
    pub kind: String,
    pub severity: String,
    pub message: String,
}

/// Detect failed and stuck deployments from history. Pure over the records and
/// a reference timestamp so it is fully testable.
pub fn detect_failures(records: &[DeployRecord], now_epoch: i64) -> Vec<FailureAlert> {
    let mut alerts = Vec::new();
    for r in records {
        match r.status {
            DeployStatus::Failed => alerts.push(FailureAlert {
                deployment_id: r.id.clone(),
                contract_id: r.contract_id.clone(),
                network: r.network.clone(),
                kind: "deploy-failed".to_string(),
                severity: "critical".to_string(),
                message: r
                    .error
                    .clone()
                    .unwrap_or_else(|| "deployment reported failure".to_string()),
            }),
            DeployStatus::Pending => {
                if let Some(age) = age_secs(&r.timestamp, now_epoch) {
                    if age > STUCK_PENDING_SECS {
                        alerts.push(FailureAlert {
                            deployment_id: r.id.clone(),
                            contract_id: r.contract_id.clone(),
                            network: r.network.clone(),
                            kind: "stuck-pending".to_string(),
                            severity: "warning".to_string(),
                            message: format!(
                                "pending for {} minutes without confirmation",
                                age / 60
                            ),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    alerts
}

// ---------------------------------------------------------------------------
// Aggregate summary / dashboard
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkHealth {
    pub healthy: usize,
    pub degraded: usize,
    pub unhealthy: usize,
    pub unknown: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MonitoringSummary {
    pub total: usize,
    pub healthy: usize,
    pub degraded: usize,
    pub unhealthy: usize,
    pub unknown: usize,
    pub by_network: BTreeMap<String, NetworkHealth>,
}

impl MonitoringSummary {
    /// Fraction of monitored deployments that are healthy (0–100).
    pub fn health_rate(&self) -> f64 {
        if self.total == 0 {
            return 100.0;
        }
        (self.healthy as f64 / self.total as f64) * 100.0
    }
}

/// Aggregate a set of per-deployment health assessments.
pub fn summarize(healths: &[DeploymentHealth]) -> MonitoringSummary {
    let mut s = MonitoringSummary {
        total: healths.len(),
        ..Default::default()
    };
    for h in healths {
        let net = s.by_network.entry(h.network.clone()).or_default();
        match h.health {
            HealthStatus::Healthy => {
                s.healthy += 1;
                net.healthy += 1;
            }
            HealthStatus::Degraded => {
                s.degraded += 1;
                net.degraded += 1;
            }
            HealthStatus::Unhealthy => {
                s.unhealthy += 1;
                net.unhealthy += 1;
            }
            HealthStatus::Unknown => {
                s.unknown += 1;
                net.unknown += 1;
            }
        }
    }
    s
}

// ---------------------------------------------------------------------------
// Status-change tracking (for real-time updates)
// ---------------------------------------------------------------------------

/// Persisted snapshot of the last observed health per deployment, used to
/// detect changes between monitoring cycles.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HealthSnapshot {
    pub statuses: BTreeMap<String, HealthStatus>,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusChange {
    pub deployment_id: String,
    pub from: Option<HealthStatus>,
    pub to: HealthStatus,
}

/// Compute health transitions relative to a previous snapshot.
pub fn diff_snapshot(previous: &HealthSnapshot, current: &[DeploymentHealth]) -> Vec<StatusChange> {
    let mut changes = Vec::new();
    for h in current {
        let prev = previous.statuses.get(&h.deployment_id).copied();
        if prev != Some(h.health) {
            changes.push(StatusChange {
                deployment_id: h.deployment_id.clone(),
                from: prev,
                to: h.health,
            });
        }
    }
    changes
}

pub fn snapshot_from(healths: &[DeploymentHealth]) -> HealthSnapshot {
    HealthSnapshot {
        statuses: healths
            .iter()
            .map(|h| (h.deployment_id.clone(), h.health))
            .collect(),
        updated_at: now_rfc3339(),
    }
}

// ---------------------------------------------------------------------------
// Persistence (~/.starforge/deploy-monitor/)
// ---------------------------------------------------------------------------

fn monitor_dir() -> Result<PathBuf> {
    let dir = config::config_dir().join("deploy-monitor");
    if !dir.exists() {
        fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    }
    Ok(dir)
}

pub fn load_snapshot() -> Result<HealthSnapshot> {
    let path = monitor_dir()?.join("snapshot.json");
    if !path.exists() {
        return Ok(HealthSnapshot::default());
    }
    Ok(serde_json::from_str(&fs::read_to_string(&path)?).unwrap_or_default())
}

pub fn save_snapshot(snapshot: &HealthSnapshot) -> Result<()> {
    fs::write(
        monitor_dir()?.join("snapshot.json"),
        serde_json::to_string_pretty(snapshot)?,
    )?;
    Ok(())
}

/// Append failure alerts to a rotating alert log.
pub fn log_alerts(alerts: &[FailureAlert]) -> Result<()> {
    if alerts.is_empty() {
        return Ok(());
    }
    let path = monitor_dir()?.join("alerts.json");
    let mut existing: Vec<FailureAlert> = if path.exists() {
        serde_json::from_str(&fs::read_to_string(&path)?).unwrap_or_default()
    } else {
        Vec::new()
    };
    existing.extend_from_slice(alerts);
    if existing.len() > 1000 {
        let overflow = existing.len() - 1000;
        existing.drain(0..overflow);
    }
    fs::write(path, serde_json::to_string_pretty(&existing)?)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn now_epoch() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Age of an RFC3339 timestamp in seconds relative to `now_epoch`.
pub fn age_secs(timestamp: &str, now_epoch: i64) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|dt| now_epoch - dt.timestamp())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, status: DeployStatus, contract: Option<&str>, ts: &str) -> DeployRecord {
        DeployRecord {
            id: id.to_string(),
            contract_id: contract.map(|c| c.to_string()),
            wasm_path: "c.wasm".to_string(),
            wasm_hash: "hash".to_string(),
            network: "testnet".to_string(),
            wallet: "alice".to_string(),
            timestamp: ts.to_string(),
            status,
            error: None,
            previous_id: None,
            approved_by: None,
            verification_passed: false,
        }
    }

    #[test]
    fn healthy_when_success_and_contract_live() {
        let r = record(
            "1",
            DeployStatus::Success,
            Some("C123"),
            "2026-01-01T00:00:00+00:00",
        );
        let probe = LivenessProbe {
            network_reachable: true,
            contract_live: Some(true),
            age_secs: 10,
        };
        let h = assess_health(&r, &probe);
        assert_eq!(h.health, HealthStatus::Healthy);
        assert!(h
            .checks
            .iter()
            .any(|c| c.name == "contract-liveness" && c.passed));
    }

    #[test]
    fn unhealthy_when_deployed_but_missing_onchain() {
        let r = record(
            "1",
            DeployStatus::Success,
            Some("C123"),
            "2026-01-01T00:00:00+00:00",
        );
        let probe = LivenessProbe {
            network_reachable: true,
            contract_live: Some(false),
            age_secs: 10,
        };
        assert_eq!(assess_health(&r, &probe).health, HealthStatus::Unhealthy);
    }

    #[test]
    fn unknown_when_network_unreachable() {
        let r = record(
            "1",
            DeployStatus::Success,
            Some("C123"),
            "2026-01-01T00:00:00+00:00",
        );
        let probe = LivenessProbe {
            network_reachable: false,
            contract_live: None,
            age_secs: 10,
        };
        assert_eq!(assess_health(&r, &probe).health, HealthStatus::Unknown);
    }

    #[test]
    fn failed_deploy_is_unhealthy() {
        let r = record("1", DeployStatus::Failed, None, "2026-01-01T00:00:00+00:00");
        let probe = LivenessProbe {
            network_reachable: true,
            contract_live: None,
            age_secs: 10,
        };
        assert_eq!(assess_health(&r, &probe).health, HealthStatus::Unhealthy);
    }

    #[test]
    fn stuck_pending_is_unhealthy() {
        let r = record(
            "1",
            DeployStatus::Pending,
            None,
            "2026-01-01T00:00:00+00:00",
        );
        let probe = LivenessProbe {
            network_reachable: true,
            contract_live: None,
            age_secs: STUCK_PENDING_SECS + 1,
        };
        assert_eq!(assess_health(&r, &probe).health, HealthStatus::Unhealthy);
    }

    #[test]
    fn detect_failures_finds_failed_and_stuck() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-01-01T01:00:00+00:00")
            .unwrap()
            .timestamp();
        let records = vec![
            record(
                "ok",
                DeployStatus::Success,
                Some("C1"),
                "2026-01-01T00:59:00+00:00",
            ),
            record(
                "bad",
                DeployStatus::Failed,
                None,
                "2026-01-01T00:30:00+00:00",
            ),
            record(
                "stuck",
                DeployStatus::Pending,
                None,
                "2026-01-01T00:00:00+00:00",
            ),
            record(
                "recent",
                DeployStatus::Pending,
                None,
                "2026-01-01T00:58:00+00:00",
            ),
        ];
        let alerts = detect_failures(&records, now);
        assert_eq!(alerts.len(), 2);
        assert!(alerts.iter().any(|a| a.kind == "deploy-failed"));
        assert!(alerts.iter().any(|a| a.kind == "stuck-pending"));
    }

    #[test]
    fn summary_counts_by_health_and_network() {
        let healths = vec![
            DeploymentHealth {
                deployment_id: "1".into(),
                contract_id: None,
                network: "testnet".into(),
                deploy_status: DeployStatus::Success,
                health: HealthStatus::Healthy,
                checks: vec![],
                checked_at: "t".into(),
            },
            DeploymentHealth {
                deployment_id: "2".into(),
                contract_id: None,
                network: "mainnet".into(),
                deploy_status: DeployStatus::Failed,
                health: HealthStatus::Unhealthy,
                checks: vec![],
                checked_at: "t".into(),
            },
        ];
        let s = summarize(&healths);
        assert_eq!(s.total, 2);
        assert_eq!(s.healthy, 1);
        assert_eq!(s.unhealthy, 1);
        assert_eq!(s.by_network["testnet"].healthy, 1);
        assert_eq!(s.by_network["mainnet"].unhealthy, 1);
        assert_eq!(s.health_rate(), 50.0);
    }

    #[test]
    fn snapshot_diff_detects_transitions() {
        let healths = vec![DeploymentHealth {
            deployment_id: "1".into(),
            contract_id: None,
            network: "testnet".into(),
            deploy_status: DeployStatus::Success,
            health: HealthStatus::Unhealthy,
            checks: vec![],
            checked_at: "t".into(),
        }];
        let mut prev = HealthSnapshot::default();
        prev.statuses.insert("1".to_string(), HealthStatus::Healthy);
        let changes = diff_snapshot(&prev, &healths);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].from, Some(HealthStatus::Healthy));
        assert_eq!(changes[0].to, HealthStatus::Unhealthy);
    }

    #[test]
    fn snapshot_diff_ignores_unchanged() {
        let healths = vec![DeploymentHealth {
            deployment_id: "1".into(),
            contract_id: None,
            network: "testnet".into(),
            deploy_status: DeployStatus::Success,
            health: HealthStatus::Healthy,
            checks: vec![],
            checked_at: "t".into(),
        }];
        let mut prev = HealthSnapshot::default();
        prev.statuses.insert("1".to_string(), HealthStatus::Healthy);
        assert!(diff_snapshot(&prev, &healths).is_empty());
    }
}
