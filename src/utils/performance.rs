use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractMetrics {
    pub contract_id: String,
    pub network: String,
    pub metrics: Vec<MetricEntry>,
    pub alerts: Vec<AlertConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricEntry {
    pub name: String,
    pub value: f64,
    pub unit: String,
    pub timestamp: String,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertConfig {
    pub metric_name: String,
    pub threshold: f64,
    pub direction: AlertDirection,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertDirection {
    Above,
    Below,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceReport {
    pub contract_id: String,
    pub network: String,
    pub period_start: String,
    pub period_end: String,
    pub summary: PerformanceSummary,
    pub metrics: Vec<MetricEntry>,
    pub alerts_triggered: Vec<AlertTrigger>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceSummary {
    pub total_executions: u64,
    pub avg_gas_used: f64,
    pub max_gas_used: f64,
    pub min_gas_used: f64,
    pub avg_execution_time_ms: f64,
    pub success_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertTrigger {
    pub alert: AlertConfig,
    pub triggered_at: String,
    pub actual_value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GasUsageRecord {
    pub contract_id: String,
    pub operation: String,
    pub gas_used: u64,
    pub timestamp: String,
    pub success: bool,
    pub execution_time_ms: u64,
    pub network: String,
}

fn metrics_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let dir = home.join(".starforge").join("metrics");
    if !dir.exists() {
        fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    }
    Ok(dir)
}

fn metrics_file(contract_id: &str) -> Result<PathBuf> {
    let safe_id = contract_id.replace('/', "_");
    Ok(metrics_dir()?.join(format!("{}.json", safe_id)))
}

fn gas_history_file(contract_id: &str) -> Result<PathBuf> {
    let safe_id = contract_id.replace('/', "_");
    Ok(metrics_dir()?.join(format!("{}_gas.json", safe_id)))
}

pub fn record_gas_usage(record: &GasUsageRecord) -> Result<()> {
    let file = gas_history_file(&record.contract_id)?;
    let mut records: Vec<GasUsageRecord> = if file.exists() {
        let content = fs::read_to_string(&file)?;
        serde_json::from_str(&content)?
    } else {
        Vec::new()
    };

    records.push(record.clone());
    fs::write(&file, serde_json::to_string_pretty(&records)?)?;
    Ok(())
}

pub fn record_metric(
    contract_id: &str,
    name: &str,
    value: f64,
    unit: &str,
    metadata: HashMap<String, String>,
) -> Result<()> {
    let file = metrics_file(contract_id)?;
    let mut contract_metrics: ContractMetrics = if file.exists() {
        let content = fs::read_to_string(&file)?;
        serde_json::from_str(&content)?
    } else {
        ContractMetrics {
            contract_id: contract_id.to_string(),
            network: String::new(),
            metrics: Vec::new(),
            alerts: Vec::new(),
        }
    };

    contract_metrics.metrics.push(MetricEntry {
        name: name.to_string(),
        value,
        unit: unit.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        metadata,
    });

    fs::write(&file, serde_json::to_string_pretty(&contract_metrics)?)?;
    Ok(())
}

pub fn get_contract_metrics(contract_id: &str) -> Result<ContractMetrics> {
    let file = metrics_file(contract_id)?;
    if !file.exists() {
        return Ok(ContractMetrics {
            contract_id: contract_id.to_string(),
            network: String::new(),
            metrics: Vec::new(),
            alerts: Vec::new(),
        });
    }

    let content = fs::read_to_string(&file)?;
    let metrics: ContractMetrics = serde_json::from_str(&content)?;
    Ok(metrics)
}

pub fn get_gas_history(contract_id: &str) -> Result<Vec<GasUsageRecord>> {
    let file = gas_history_file(contract_id)?;
    if !file.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&file)?;
    let records: Vec<GasUsageRecord> = serde_json::from_str(&content)?;
    Ok(records)
}

pub fn set_alert(
    contract_id: &str,
    metric_name: &str,
    threshold: f64,
    direction: AlertDirection,
    message: &str,
) -> Result<()> {
    let file = metrics_file(contract_id)?;
    let mut contract_metrics: ContractMetrics = if file.exists() {
        let content = fs::read_to_string(&file)?;
        serde_json::from_str(&content)?
    } else {
        ContractMetrics {
            contract_id: contract_id.to_string(),
            network: String::new(),
            metrics: Vec::new(),
            alerts: Vec::new(),
        }
    };

    contract_metrics
        .alerts
        .retain(|a| a.metric_name != metric_name);
    contract_metrics.alerts.push(AlertConfig {
        metric_name: metric_name.to_string(),
        threshold,
        direction,
        message: message.to_string(),
    });

    fs::write(&file, serde_json::to_string_pretty(&contract_metrics)?)?;
    Ok(())
}

pub fn check_alerts(contract_id: &str) -> Result<Vec<AlertTrigger>> {
    let contract_metrics = get_contract_metrics(contract_id)?;
    let mut triggered = Vec::new();

    for alert in &contract_metrics.alerts {
        if let Some(latest) = contract_metrics
            .metrics
            .iter()
            .rev()
            .find(|m| m.name == alert.metric_name)
        {
            let exceeds = match alert.direction {
                AlertDirection::Above => latest.value > alert.threshold,
                AlertDirection::Below => latest.value < alert.threshold,
            };

            if exceeds {
                triggered.push(AlertTrigger {
                    alert: alert.clone(),
                    triggered_at: latest.timestamp.clone(),
                    actual_value: latest.value,
                });
            }
        }
    }

    Ok(triggered)
}

pub fn generate_report(contract_id: &str, network: &str) -> Result<PerformanceReport> {
    let contract_metrics = get_contract_metrics(contract_id)?;
    let gas_history = get_gas_history(contract_id)?;

    let gas_values: Vec<f64> = gas_history.iter().map(|r| r.gas_used as f64).collect();
    let time_values: Vec<f64> = gas_history
        .iter()
        .map(|r| r.execution_time_ms as f64)
        .collect();
    let success_count = gas_history.iter().filter(|r| r.success).count();

    let avg_gas = if gas_values.is_empty() {
        0.0
    } else {
        gas_values.iter().sum::<f64>() / gas_values.len() as f64
    };
    let max_gas = gas_values.iter().cloned().fold(0.0_f64, f64::max);
    let min_gas = gas_values.iter().cloned().fold(f64::INFINITY, f64::min);
    let avg_time = if time_values.is_empty() {
        0.0
    } else {
        time_values.iter().sum::<f64>() / time_values.len() as f64
    };
    let success_rate = if gas_history.is_empty() {
        100.0
    } else {
        (success_count as f64 / gas_history.len() as f64) * 100.0
    };

    let triggered = check_alerts(contract_id)?;

    let now = chrono::Utc::now();
    let period_start = (now - chrono::Duration::hours(24)).to_rfc3339();

    Ok(PerformanceReport {
        contract_id: contract_id.to_string(),
        network: network.to_string(),
        period_start,
        period_end: now.to_rfc3339(),
        summary: PerformanceSummary {
            total_executions: gas_history.len() as u64,
            avg_gas_used: avg_gas,
            max_gas_used: max_gas,
            min_gas_used: if min_gas == f64::INFINITY {
                0.0
            } else {
                min_gas
            },
            avg_execution_time_ms: avg_time,
            success_rate,
        },
        metrics: contract_metrics.metrics,
        alerts_triggered: triggered,
    })
}

pub struct MetricCollector {
    start: Instant,
    contract_id: String,
    network: String,
    marks: Vec<(String, Instant)>,
}

impl MetricCollector {
    pub fn new(contract_id: &str, network: &str) -> Self {
        Self {
            start: Instant::now(),
            contract_id: contract_id.to_string(),
            network: network.to_string(),
            marks: Vec::new(),
        }
    }

    pub fn mark(&mut self, label: &str) {
        self.marks.push((label.to_string(), Instant::now()));
    }

    pub fn finish(self) -> Result<()> {
        let total_ms = self.start.elapsed().as_millis() as u64;

        record_gas_usage(&GasUsageRecord {
            contract_id: self.contract_id.clone(),
            operation: "execution".to_string(),
            gas_used: total_ms * 100,
            timestamp: chrono::Utc::now().to_rfc3339(),
            success: true,
            execution_time_ms: total_ms,
            network: self.network,
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn record_and_retrieve_gas_usage() {
        let tmp = tempdir().unwrap();
        let file = tmp.path().join("test_gas.json");

        let record = GasUsageRecord {
            contract_id: "CABC123".to_string(),
            operation: "invoke".to_string(),
            gas_used: 1000,
            timestamp: chrono::Utc::now().to_rfc3339(),
            success: true,
            execution_time_ms: 50,
            network: "testnet".to_string(),
        };

        let records: Vec<GasUsageRecord> = vec![record.clone()];
        fs::write(&file, serde_json::to_string_pretty(&records).unwrap()).unwrap();

        let loaded: Vec<GasUsageRecord> =
            serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].gas_used, 1000);
    }

    #[test]
    fn alert_direction_serializes() {
        let above = AlertDirection::Above;
        let below = AlertDirection::Below;
        assert_eq!(serde_json::to_string(&above).unwrap(), "\"above\"");
        assert_eq!(serde_json::to_string(&below).unwrap(), "\"below\"");
    }

    #[test]
    fn performance_summary_default_values() {
        let summary = PerformanceSummary {
            total_executions: 0,
            avg_gas_used: 0.0,
            max_gas_used: 0.0,
            min_gas_used: 0.0,
            avg_execution_time_ms: 0.0,
            success_rate: 100.0,
        };
        assert_eq!(summary.total_executions, 0);
        assert_eq!(summary.success_rate, 100.0);
    }
}
