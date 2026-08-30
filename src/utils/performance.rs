use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
    /// Average memory usage captured by caller (bytes)
    pub avg_memory_used_bytes: Option<f64>,
    /// Max memory usage captured by caller (bytes)
    pub max_memory_used_bytes: Option<f64>,
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
    /// Optional memory usage captured by the caller (bytes)
    pub memory_used: Option<u64>,
    pub network: String,
}

thread_local! {
    static TEST_METRICS_DIR: std::cell::RefCell<Option<PathBuf>> = std::cell::RefCell::new(None);
}

fn metrics_dir() -> Result<PathBuf> {
    if let Some(dir) = TEST_METRICS_DIR.with(|d| d.borrow().clone()) {
        if !dir.exists() {
            fs::create_dir_all(&dir)?;
        }
        return Ok(dir);
    }
    let dir = crate::utils::config::config_dir().join("metrics");
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
    let memory_values: Vec<f64> = gas_history
        .iter()
        .filter_map(|r| r.memory_used.map(|m| m as f64))
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
    let (avg_memory, max_memory) = if memory_values.is_empty() {
        (None, None)
    } else {
        let avg = memory_values.iter().sum::<f64>() / memory_values.len() as f64;
        let max = memory_values.iter().cloned().fold(0.0_f64, f64::max);
        (Some(avg), Some(max))
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
            avg_memory_used_bytes: avg_memory,
            max_memory_used_bytes: max_memory,
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
            memory_used: None,
            network: self.network,
        })?;

        Ok(())
    }
}

// ── Bottleneck Identification ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BottleneckAnalysis {
    pub contract_id: String,
    pub network: String,
    pub bottleneck_operations: Vec<String>,
    pub high_gas_operations: Vec<String>,
    pub memory_leaks_detected: bool,
    pub overall_score: f64,
}

pub fn analyze_bottlenecks(contract_id: &str) -> Result<BottleneckAnalysis> {
    let gas_history = get_gas_history(contract_id)?;
    if gas_history.is_empty() {
        return Err(anyhow::anyhow!(
            "No gas history found for contract: {}",
            contract_id
        ));
    }

    let mut operation_frequencies: HashMap<String, usize> = HashMap::new();
    let mut operation_gas: HashMap<String, u64> = HashMap::new();
    let mut operation_memory: HashMap<String, u64> = HashMap::new();
    let mut operation_mem_count: HashMap<String, usize> = HashMap::new();

    for record in &gas_history {
        *operation_frequencies
            .entry(record.operation.clone())
            .or_insert(0) += 1;
        operation_gas
            .entry(record.operation.clone())
            .and_modify(|g| *g += record.gas_used)
            .or_insert(record.gas_used);
        if let Some(mem) = record.memory_used {
            operation_memory
                .entry(record.operation.clone())
                .and_modify(|m| *m += mem)
                .or_insert(mem);
            *operation_mem_count
                .entry(record.operation.clone())
                .or_insert(0) += 1;
        }
    }

    let total_gas: u64 = gas_history.iter().map(|r| r.gas_used).sum();
    let total_executions = gas_history.len() as f64;

    let mut bottleneck_operations: Vec<String> = operation_frequencies
        .iter()
        .filter(|(_, freq)| **freq as f64 / total_executions > 0.3)
        .map(|(op, _)| op.clone())
        .collect();

    let high_gas_operations: Vec<String> = operation_gas
        .iter()
        .filter(|(_, gas)| **gas as f64 / total_executions as f64 > 50_000.0)
        .map(|(op, _)| op.clone())
        .collect();

    for (op, total_mem) in &operation_memory {
        let count = operation_mem_count.get(op).copied().unwrap_or(1) as f64;
        let avg_mem = *total_mem as f64 / count;
        if avg_mem > 10_000_000.0 && !bottleneck_operations.contains(op) {
            bottleneck_operations.push(op.clone());
        }
    }

    let success_rate = gas_history.iter().filter(|r| r.success).count() as f64 / total_executions;
    let avg_execution_time =
        gas_history.iter().map(|r| r.execution_time_ms).sum::<u64>() as f64 / total_executions;

    let mem_records: Vec<u64> = gas_history.iter().filter_map(|r| r.memory_used).collect();
    let memory_leaks_detected = if mem_records.len() >= 4 {
        let half = mem_records.len() / 2;
        let first_half_avg: f64 = mem_records
            .iter()
            .take(half)
            .map(|m| *m as f64)
            .sum::<f64>()
            / half as f64;
        let second_half_avg: f64 = mem_records
            .iter()
            .skip(half)
            .map(|m| *m as f64)
            .sum::<f64>()
            / (mem_records.len() - half) as f64;
        second_half_avg > first_half_avg * 1.2
    } else {
        avg_execution_time > 10000.0
    };

    let mut score = 100.0;
    if success_rate < 0.95 {
        score -= (0.95 - success_rate) * 30.0;
    }
    if avg_execution_time > 5000.0 {
        score -= ((avg_execution_time - 5000.0) / 5000.0) * 25.0;
    }
    if memory_leaks_detected {
        score -= 15.0;
    }
    if !mem_records.is_empty() {
        let avg_mem: f64 =
            mem_records.iter().map(|m| *m as f64).sum::<f64>() / mem_records.len() as f64;
        if avg_mem > 50_000_000.0 {
            score -= ((avg_mem - 50_000_000.0) / 50_000_000.0) * 15.0;
        }
    }

    Ok(BottleneckAnalysis {
        contract_id: contract_id.to_string(),
        network: gas_history[0].network.clone(),
        bottleneck_operations,
        high_gas_operations,
        memory_leaks_detected,
        overall_score: score.max(0.0),
    })
}

// ── Performance Regression Detection ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegressionPoint {
    pub timestamp: String,
    pub gas_used: u64,
    pub execution_time_ms: u64,
    pub memory_used: Option<u64>,
    pub success: bool,
    pub regression_detected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegressionReport {
    pub contract_id: String,
    pub network: String,
    pub baseline_avg: f64,
    pub current_avg: f64,
    pub regression_percentage: f64,
    pub regression_points: Vec<RegressionPoint>,
    pub trends: Vec<String>,
}

pub fn detect_regression(contract_id: &str, period_hours: u64) -> Result<RegressionReport> {
    let gas_history = get_gas_history(contract_id)?;
    if gas_history.is_empty() {
        return Err(anyhow::anyhow!(
            "No gas history found for contract: {}",
            contract_id
        ));
    }

    let now = chrono::Utc::now();
    let cutoff = now - chrono::Duration::hours(period_hours as i64);
    let recent_records: Vec<_> = gas_history
        .iter()
        .filter(|r| {
            chrono::DateTime::parse_from_rfc3339(&r.timestamp)
                .map(|t| t >= cutoff)
                .unwrap_or(false)
        })
        .collect();

    let baseline_records: Vec<_> = gas_history
        .iter()
        .filter(|r| {
            chrono::DateTime::parse_from_rfc3339(&r.timestamp)
                .map(|t| t < cutoff)
                .unwrap_or(false)
        })
        .collect();

    let baseline_gas: Vec<f64> = baseline_records.iter().map(|r| r.gas_used as f64).collect();
    let current_gas: Vec<f64> = recent_records.iter().map(|r| r.gas_used as f64).collect();
    let baseline_time: Vec<f64> = baseline_records
        .iter()
        .map(|r| r.execution_time_ms as f64)
        .collect();
    let current_time: Vec<f64> = recent_records
        .iter()
        .map(|r| r.execution_time_ms as f64)
        .collect();
    let baseline_mem: Vec<f64> = baseline_records
        .iter()
        .filter_map(|r| r.memory_used.map(|m| m as f64))
        .collect();
    let current_mem: Vec<f64> = recent_records
        .iter()
        .filter_map(|r| r.memory_used.map(|m| m as f64))
        .collect();

    let baseline_avg = if !baseline_gas.is_empty() {
        baseline_gas.iter().sum::<f64>() / baseline_gas.len() as f64
    } else {
        0.0
    };

    let current_avg = if !current_gas.is_empty() {
        current_gas.iter().sum::<f64>() / current_gas.len() as f64
    } else {
        0.0
    };

    let baseline_time_avg = if !baseline_time.is_empty() {
        baseline_time.iter().sum::<f64>() / baseline_time.len() as f64
    } else {
        0.0
    };
    let current_time_avg = if !current_time.is_empty() {
        current_time.iter().sum::<f64>() / current_time.len() as f64
    } else {
        0.0
    };

    let baseline_mem_avg = if !baseline_mem.is_empty() {
        baseline_mem.iter().sum::<f64>() / baseline_mem.len() as f64
    } else {
        0.0
    };
    let current_mem_avg = if !current_mem.is_empty() {
        current_mem.iter().sum::<f64>() / current_mem.len() as f64
    } else {
        0.0
    };

    let mut regression_percentage = 0.0;
    if baseline_avg > 0.0 {
        regression_percentage = ((current_avg - baseline_avg) / baseline_avg) * 100.0;
    }

    let regression_points: Vec<RegressionPoint> = recent_records
        .iter()
        .map(|r| {
            let mut detected = false;
            if baseline_avg > 0.0 && (r.gas_used as f64) > baseline_avg * 1.2 {
                detected = true;
            }
            if baseline_avg > 0.0 && (r.gas_used as f64) < baseline_avg * 0.8 {
                detected = true;
            }
            if baseline_time_avg > 0.0 && (r.execution_time_ms as f64) > baseline_time_avg * 1.25 {
                detected = true;
            }
            if baseline_time_avg > 0.0 && (r.execution_time_ms as f64) < baseline_time_avg * 0.75 {
                detected = true;
            }
            if let Some(mem) = r.memory_used {
                if baseline_mem_avg > 0.0 && (mem as f64) > baseline_mem_avg * 1.2 {
                    detected = true;
                }
                if baseline_mem_avg > 0.0 && (mem as f64) < baseline_mem_avg * 0.8 {
                    detected = true;
                }
            }

            RegressionPoint {
                timestamp: r.timestamp.clone(),
                gas_used: r.gas_used,
                execution_time_ms: r.execution_time_ms,
                memory_used: r.memory_used,
                success: r.success,
                regression_detected: detected,
            }
        })
        .collect();

    let mut trends: Vec<String> = Vec::new();
    if baseline_avg > 0.0 && current_avg > baseline_avg * 1.15 {
        trends.push("Gas usage has increased by more than 15% compared to baseline".to_string());
    } else if baseline_avg > 0.0 && current_avg < baseline_avg * 0.85 {
        trends.push("Gas usage has decreased by more than 15% compared to baseline".to_string());
    } else {
        trends.push("Gas usage is within acceptable range of baseline".to_string());
    }

    if baseline_time_avg > 0.0 && current_time_avg > baseline_time_avg * 1.15 {
        trends
            .push("Execution time has increased by more than 15% compared to baseline".to_string());
    } else if baseline_time_avg > 0.0 && current_time_avg < baseline_time_avg * 0.85 {
        trends
            .push("Execution time has decreased by more than 15% compared to baseline".to_string());
    }

    if baseline_mem_avg > 0.0 && current_mem_avg > baseline_mem_avg * 1.15 {
        trends.push("Memory usage has increased by more than 15% compared to baseline".to_string());
    } else if baseline_mem_avg > 0.0 && current_mem_avg < baseline_mem_avg * 0.85 {
        trends.push("Memory usage has decreased by more than 15% compared to baseline".to_string());
    }

    Ok(RegressionReport {
        contract_id: contract_id.to_string(),
        network: gas_history[0].network.clone(),
        baseline_avg,
        current_avg,
        regression_percentage,
        regression_points,
        trends,
    })
}

// ── Profiling Comparison ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileSnapshot {
    pub contract_id: String,
    pub timestamp: String,
    pub operation: String,
    pub gas_used: u64,
    pub execution_time_ms: u64,
    pub success: bool,
    pub memory_used: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonReport {
    pub contract_id: String,
    pub comparison_date: String,
    pub snapshots: Vec<ProfileSnapshot>,
    pub performance_differences: BTreeMap<String, f64>,
    pub recommendations: Vec<String>,
}

pub fn compare_profiles(contract_id: &str, hours_back: u64) -> Result<ComparisonReport> {
    let gas_history = get_gas_history(contract_id)?;
    if gas_history.is_empty() {
        return Err(anyhow::anyhow!(
            "No gas history found for contract: {}",
            contract_id
        ));
    }

    let cutoff = chrono::Utc::now() - chrono::Duration::hours(hours_back as i64);
    let recent_records: Vec<_> = gas_history
        .iter()
        .filter(|r| {
            chrono::DateTime::parse_from_rfc3339(&r.timestamp)
                .map(|t| t >= cutoff)
                .unwrap_or(false)
        })
        .collect();

    let mut snapshots: Vec<ProfileSnapshot> = Vec::new();
    for record in recent_records {
        snapshots.push(ProfileSnapshot {
            contract_id: contract_id.to_string(),
            timestamp: record.timestamp.clone(),
            operation: record.operation.clone(),
            gas_used: record.gas_used,
            execution_time_ms: record.execution_time_ms,
            success: record.success,
            memory_used: record.memory_used.map(|m| m as usize),
        });
    }

    let mut performance_differences: BTreeMap<String, f64> = BTreeMap::new();

    if snapshots.len() >= 2 {
        let avg_gas_current: f64 =
            snapshots.iter().map(|s| s.gas_used as f64).sum::<f64>() / snapshots.len() as f64;

        let avg_gas_earlier = if snapshots.len() >= 4 {
            let earlier: Vec<_> = snapshots.iter().take(snapshots.len() / 2).collect();
            earlier.iter().map(|s| s.gas_used as f64).sum::<f64>() / earlier.len() as f64
        } else {
            avg_gas_current
        };

        if avg_gas_earlier > 0.0 {
            performance_differences.insert(
                "gas_usage_difference".to_string(),
                ((avg_gas_current - avg_gas_earlier) / avg_gas_earlier) * 100.0,
            );
        }

        let avg_time_current: f64 = snapshots
            .iter()
            .map(|s| s.execution_time_ms as f64)
            .sum::<f64>()
            / snapshots.len() as f64;

        let avg_time_earlier = if snapshots.len() >= 4 {
            let earlier: Vec<_> = snapshots.iter().take(snapshots.len() / 2).collect();
            earlier
                .iter()
                .map(|s| s.execution_time_ms as f64)
                .sum::<f64>()
                / earlier.len() as f64
        } else {
            avg_time_current
        };

        if avg_time_earlier > 0.0 {
            performance_differences.insert(
                "execution_time_difference".to_string(),
                ((avg_time_current - avg_time_earlier) / avg_time_earlier) * 100.0,
            );
        }

        let mem_current: Vec<usize> = snapshots.iter().filter_map(|s| s.memory_used).collect();
        if !mem_current.is_empty() && snapshots.len() >= 4 {
            let half = snapshots.len() / 2;
            let earlier_mem: Vec<usize> = snapshots
                .iter()
                .take(half)
                .filter_map(|s| s.memory_used)
                .collect();
            let later_mem: Vec<usize> = snapshots
                .iter()
                .skip(half)
                .filter_map(|s| s.memory_used)
                .collect();

            if !earlier_mem.is_empty() && !later_mem.is_empty() {
                let avg_mem_earlier: f64 =
                    earlier_mem.iter().map(|m| *m as f64).sum::<f64>() / earlier_mem.len() as f64;
                let avg_mem_later: f64 =
                    later_mem.iter().map(|m| *m as f64).sum::<f64>() / later_mem.len() as f64;

                if avg_mem_earlier > 0.0 {
                    performance_differences.insert(
                        "memory_usage_difference".to_string(),
                        ((avg_mem_later - avg_mem_earlier) / avg_mem_earlier) * 100.0,
                    );
                }
            }
        }
    }

    let mut recommendations: Vec<String> = Vec::new();
    if let Some(gas_diff) = performance_differences.get("gas_usage_difference") {
        if *gas_diff > 20.0 {
            recommendations.push("Gas usage has increased significantly. Consider optimizing storage access patterns.".to_string());
        } else if *gas_diff < -20.0 {
            recommendations
                .push("Gas usage has decreased significantly. Good optimization work!".to_string());
        }
    }

    if let Some(time_diff) = performance_differences.get("execution_time_difference") {
        if *time_diff > 20.0 {
            recommendations.push(
                "Execution time has increased. Investigate for potential bottlenecks.".to_string(),
            );
        } else if *time_diff < -20.0 {
            recommendations.push("Execution time has improved significantly.".to_string());
        }
    }

    if let Some(mem_diff) = performance_differences.get("memory_usage_difference") {
        if *mem_diff > 20.0 {
            recommendations.push(
                "Memory usage has increased. Check for leaks or unnecessary allocations."
                    .to_string(),
            );
        } else if *mem_diff < -20.0 {
            recommendations.push("Memory usage has improved significantly.".to_string());
        }
    }

    Ok(ComparisonReport {
        contract_id: contract_id.to_string(),
        comparison_date: chrono::Utc::now().to_rfc3339(),
        snapshots,
        performance_differences,
        recommendations,
    })
}

// ── Performance Dashboard ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceDashboard {
    pub contract_id: String,
    pub network: String,
    pub timestamp: String,
    pub summary: PerformanceSummary,
    pub bottleneck_analysis: BottleneckAnalysis,
    pub regression_report: RegressionReport,
    pub comparison_report: ComparisonReport,
    pub alerts: Vec<AlertConfig>,
}

pub fn generate_dashboard(contract_id: &str, network: &str) -> Result<PerformanceDashboard> {
    let report = generate_report(contract_id, network)?;
    let bottleneck = analyze_bottlenecks(contract_id)?;
    let regression = detect_regression(contract_id, 24)?;
    let comparison = compare_profiles(contract_id, 24)?;
    let metrics = get_contract_metrics(contract_id)?;

    Ok(PerformanceDashboard {
        contract_id: contract_id.to_string(),
        network: network.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        summary: report.summary,
        bottleneck_analysis: bottleneck,
        regression_report: regression,
        comparison_report: comparison,
        alerts: metrics.alerts,
    })
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
            memory_used: None,
            network: "testnet".to_string(),
        };

        let mut records: Vec<GasUsageRecord> = Vec::new();
        records.push(record.clone());
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
            avg_memory_used_bytes: None,
            max_memory_used_bytes: None,
        };
        assert_eq!(summary.total_executions, 0);
        assert_eq!(summary.success_rate, 100.0);
    }

    static TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn run_with_temp_home<F>(test: F)
    where
        F: FnOnce(),
    {
        let _guard = TEST_MUTEX.lock().unwrap();
        let orig_home = std::env::var("HOME").ok();
        let orig_userprofile = std::env::var("USERPROFILE").ok();

        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", tmp.path());
        std::env::set_var("USERPROFILE", tmp.path());
        TEST_METRICS_DIR.with(|d| *d.borrow_mut() = Some(tmp.path().to_path_buf()));

        test();

        TEST_METRICS_DIR.with(|d| *d.borrow_mut() = None);
        if let Some(h) = orig_home {
            std::env::set_var("HOME", h);
        } else {
            std::env::remove_var("HOME");
        }
        if let Some(up) = orig_userprofile {
            std::env::set_var("USERPROFILE", up);
        } else {
            std::env::remove_var("USERPROFILE");
        }
    }

    #[test]
    fn test_analyze_bottlenecks() {
        run_with_temp_home(|| {
            let contract_id = format!(
                "TEST_{}_{}",
                chrono::Utc::now().timestamp_millis(),
                rand::random::<u64>()
            );
            let base_time = chrono::Utc::now();

            for i in 0..10 {
                let record = GasUsageRecord {
                    contract_id: contract_id.clone(),
                    operation: if i % 3 == 0 {
                        "transfer".to_string()
                    } else {
                        "query".to_string()
                    },
                    gas_used: (i * 1000 + 500) as u64,
                    timestamp: (base_time + chrono::Duration::seconds(i as i64)).to_rfc3339(),
                    success: i % 5 != 0,
                    execution_time_ms: (i * 100 + 100) as u64,
                    memory_used: None,
                    network: "testnet".to_string(),
                };
                record_gas_usage(&record).unwrap();
            }

            let loaded = get_gas_history(&contract_id).unwrap();
            assert_eq!(loaded.len(), 10);

            let analysis = analyze_bottlenecks(&contract_id).unwrap();
            assert!(analysis.overall_score >= 0.0);
            assert!(!analysis.bottleneck_operations.is_empty());
        });
    }

    #[test]
    fn test_detect_regression() {
        run_with_temp_home(|| {
            let contract_id = format!(
                "REGRESSION_{}_{}",
                chrono::Utc::now().timestamp_millis(),
                rand::random::<u64>()
            );
            let base_time = chrono::Utc::now();

            for i in 0..10 {
                let record = GasUsageRecord {
                    contract_id: contract_id.clone(),
                    operation: "operation".to_string(),
                    gas_used: if i < 5 {
                        10000 + i as u64 * 500
                    } else {
                        15000 + i as u64 * 500
                    },
                    timestamp: (base_time + chrono::Duration::seconds(i as i64)).to_rfc3339(),
                    success: true,
                    execution_time_ms: if i < 5 {
                        500 + i as u64 * 50
                    } else {
                        1000 + i as u64 * 50
                    },
                    memory_used: None,
                    network: "testnet".to_string(),
                };
                record_gas_usage(&record).unwrap();
            }

            let loaded = get_gas_history(&contract_id).unwrap();
            assert_eq!(loaded.len(), 10);

            let report = detect_regression(&contract_id, 24).unwrap();
            assert!(!report.regression_points.is_empty());
            assert!(!report.trends.is_empty());
        });
    }

    #[test]
    fn test_memory_recording_and_summary() {
        run_with_temp_home(|| {
            let contract_id = format!(
                "MEM_{}_{}",
                chrono::Utc::now().timestamp_millis(),
                rand::random::<u64>()
            );
            let base_time = chrono::Utc::now();

            for i in 0..5 {
                let record = GasUsageRecord {
                    contract_id: contract_id.clone(),
                    operation: "transfer".to_string(),
                    gas_used: 10000,
                    timestamp: (base_time + chrono::Duration::seconds(i as i64)).to_rfc3339(),
                    success: true,
                    execution_time_ms: 100,
                    memory_used: Some(1_000_000 + i as u64 * 100_000),
                    network: "testnet".to_string(),
                };
                record_gas_usage(&record).unwrap();
            }

            let report = generate_report(&contract_id, "testnet").unwrap();
            assert_eq!(report.summary.total_executions, 5);
            assert!(report.summary.avg_memory_used_bytes.is_some());
            assert!(report.summary.max_memory_used_bytes.is_some());
            assert!(report.summary.avg_memory_used_bytes.unwrap() > 1_000_000.0);
            assert!(report.summary.max_memory_used_bytes.unwrap() >= 1_400_000.0);
        });
    }

    #[test]
    fn test_bottleneck_with_memory() {
        run_with_temp_home(|| {
            let contract_id = format!(
                "BOTTLENECK_MEM_{}_{}",
                chrono::Utc::now().timestamp_millis(),
                rand::random::<u64>()
            );
            let base_time = chrono::Utc::now();

            for i in 0..8 {
                let record = GasUsageRecord {
                    contract_id: contract_id.clone(),
                    operation: if i % 2 == 0 {
                        "heavy".to_string()
                    } else {
                        "light".to_string()
                    },
                    gas_used: 10000 + i as u64 * 1000,
                    timestamp: (base_time + chrono::Duration::seconds(i as i64)).to_rfc3339(),
                    success: true,
                    execution_time_ms: 200 + i as u64 * 10,
                    memory_used: if i % 2 == 0 {
                        Some(15_000_000)
                    } else {
                        Some(1_000_000)
                    },
                    network: "testnet".to_string(),
                };
                record_gas_usage(&record).unwrap();
            }

            let analysis = analyze_bottlenecks(&contract_id).unwrap();
            assert!(analysis.overall_score >= 0.0);
        });
    }

    #[test]
    fn test_memory_regression_detection() {
        run_with_temp_home(|| {
            let contract_id = format!(
                "MEM_REG_{}_{}",
                chrono::Utc::now().timestamp_millis(),
                rand::random::<u64>()
            );
            let base_time = chrono::Utc::now();

            for i in 0..10 {
                let record = GasUsageRecord {
                    contract_id: contract_id.clone(),
                    operation: "op".to_string(),
                    gas_used: 10000,
                    timestamp: (base_time + chrono::Duration::seconds(i as i64)).to_rfc3339(),
                    success: true,
                    execution_time_ms: 100,
                    memory_used: Some(if i < 5 { 1_000_000 } else { 2_000_000 }),
                    network: "testnet".to_string(),
                };
                record_gas_usage(&record).unwrap();
            }

            let report = detect_regression(&contract_id, 24).unwrap();
            assert!(!report.regression_points.is_empty());
            for rp in &report.regression_points {
                assert!(rp.memory_used.is_some());
            }
        });
    }

    #[test]
    fn test_compare_profiles() {
        run_with_temp_home(|| {
            let contract_id = format!(
                "COMPARE_{}_{}",
                chrono::Utc::now().timestamp_millis(),
                rand::random::<u64>()
            );
            let base_time = chrono::Utc::now();

            for i in 0..8 {
                let record = GasUsageRecord {
                    contract_id: contract_id.clone(),
                    operation: "test_op".to_string(),
                    gas_used: 10000 + i as u64 * 1000,
                    timestamp: (base_time - chrono::Duration::seconds((8 - i) as i64)).to_rfc3339(),
                    success: true,
                    execution_time_ms: 500 + i * 50,
                    memory_used: Some(1_000_000 + i as u64 * 200_000),
                    network: "testnet".to_string(),
                };
                record_gas_usage(&record).unwrap();
            }

            let loaded = get_gas_history(&contract_id).unwrap();
            assert_eq!(loaded.len(), 8);

            let report = compare_profiles(&contract_id, 24).unwrap();
            assert_eq!(report.snapshots.len(), 8);
            for s in &report.snapshots {
                assert!(s.memory_used.is_some());
            }
            assert!(
                !report.performance_differences.is_empty() || report.recommendations.is_empty()
            );
        });
    }
}
