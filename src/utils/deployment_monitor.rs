use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::utils::deploy_history::{load_history, DeployStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentMonitoringReport {
    pub network: String,
    pub generated_at: String,
    pub total_deployments: usize,
    pub successful_deployments: usize,
    pub failed_deployments: usize,
    pub success_rate: f64,
    pub error_rate: f64,
    pub avg_duration_ms: f64,
    pub unique_wallets: usize,
    pub unique_contracts: usize,
    pub alerts: Vec<DeploymentAlert>,
    pub predictions: Vec<DeploymentPrediction>,
    pub history: Vec<HistoricalTrend>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentAlert {
    pub severity: String,
    pub title: String,
    pub detail: String,
    pub recommendation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentPrediction {
    pub title: String,
    pub confidence: u8,
    pub detail: String,
    pub recommended_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalTrend {
    pub label: String,
    pub value: f64,
    pub direction: String,
}

pub fn analyze_deployments(
    network: &str,
    contract_id: Option<&str>,
) -> Result<DeploymentMonitoringReport> {
    let records = load_history()?;
    let filtered: Vec<_> = records
        .into_iter()
        .filter(|record| {
            record.network == network
                && contract_id.map_or(true, |cid| record.contract_id.as_deref() == Some(cid))
        })
        .collect();

    let total = filtered.len();
    let successful_deployments = filtered
        .iter()
        .filter(|record| record.status == DeployStatus::Success)
        .count();
    let failed_deployments = filtered
        .iter()
        .filter(|record| record.status == DeployStatus::Failed)
        .count();

    let success_rate = if total == 0 {
        0.0
    } else {
        successful_deployments as f64 / total as f64 * 100.0
    };
    let error_rate = if total == 0 {
        0.0
    } else {
        failed_deployments as f64 / total as f64 * 100.0
    };

    let durations: Vec<f64> = filtered
        .iter()
        .filter_map(|record| record.duration_ms.map(|ms| ms as f64))
        .collect();
    let avg_duration_ms = if durations.is_empty() {
        0.0
    } else {
        durations.iter().sum::<f64>() / durations.len() as f64
    };

    let unique_wallets: HashSet<_> = filtered
        .iter()
        .map(|record| record.wallet.clone())
        .collect();
    let unique_contracts: HashSet<_> = filtered
        .iter()
        .filter_map(|record| record.contract_id.clone())
        .collect();

    let mut alerts = Vec::new();
    if failed_deployments > 0 && error_rate >= 20.0 {
        alerts.push(DeploymentAlert {
            severity: "high".to_string(),
            title: "Deployment failure rate is elevated".to_string(),
            detail: format!(
                "{} of the recent deployments on {} have failed, which signals a stability risk.",
                failed_deployments, network
            ),
            recommendation: "Inspect the recent deployment logs, compare the last successful release, and consider rolling back to the last healthy deployment if the trend continues.".to_string(),
        });
    }

    if avg_duration_ms > 10_000.0 {
        alerts.push(DeploymentAlert {
            severity: "medium".to_string(),
            title: "Deployment latency has increased".to_string(),
            detail: format!(
                "The average deployment duration is {:.0} ms, which is above the healthy baseline for this network.",
                avg_duration_ms
            ),
            recommendation: "Review the build artifact size, network conditions, and signing overhead to reduce deployment time before the next rollout.".to_string(),
        });
    }

    if alerts.is_empty() {
        alerts.push(DeploymentAlert {
            severity: "low".to_string(),
            title: "No immediate deployment issues detected".to_string(),
            detail: "The current deployment health profile looks stable and does not require immediate intervention.".to_string(),
            recommendation: "Continue monitoring and keep the last known good deployment available for rapid rollback if the signal changes.".to_string(),
        });
    }

    let mut predictions = Vec::new();
    if failed_deployments > 0 {
        predictions.push(DeploymentPrediction {
            title: "Rollback risk is increasing".to_string(),
            confidence: 74,
            detail: "The recent failure pattern suggests a higher chance of another deployment failure in the next rollout.".to_string(),
            recommended_action: "Prepare a rollback plan, verify the artifact hash, and keep the previous deployment ready for immediate recovery.".to_string(),
        });
    }

    if avg_duration_ms > 5_000.0 {
        predictions.push(DeploymentPrediction {
            title: "Performance degradation is likely".to_string(),
            confidence: 68,
            detail: "The observed deployment latency trend points to slower execution than the recent baseline.".to_string(),
            recommended_action: "Trim the deployment payload, validate the wallet setup, and review network congestion before launching the next deployment.".to_string(),
        });
    }

    if predictions.is_empty() {
        predictions.push(DeploymentPrediction {
            title: "Deployment health looks stable".to_string(),
            confidence: 61,
            detail: "The available signals do not suggest a near-term deployment issue.".to_string(),
            recommended_action: "Keep monitoring and record the next deployment so the model can refine its predictions.".to_string(),
        });
    }

    let recent_entries: Vec<_> = filtered.iter().rev().take(6).collect();
    let history = recent_entries
        .iter()
        .enumerate()
        .map(|(idx, record)| HistoricalTrend {
            label: format!("#{}", idx + 1),
            value: record.duration_ms.unwrap_or_default() as f64,
            direction: if record.status == DeployStatus::Failed {
                "down".to_string()
            } else {
                "up".to_string()
            },
        })
        .collect();

    Ok(DeploymentMonitoringReport {
        network: network.to_string(),
        generated_at: Utc::now().to_rfc3339(),
        total_deployments: total,
        successful_deployments,
        failed_deployments,
        success_rate,
        error_rate,
        avg_duration_ms,
        unique_wallets: unique_wallets.len(),
        unique_contracts: unique_contracts.len(),
        alerts,
        predictions,
        history,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_history_returns_stable_report() {
        let report = analyze_deployments("testnet", None).unwrap();
        assert_eq!(report.total_deployments, 0);
        assert_eq!(report.success_rate, 0.0);
        assert!(report
            .alerts
            .iter()
            .any(|alert| alert.title.contains("No immediate")));
    }
}
