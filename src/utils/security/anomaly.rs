use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyFinding {
    pub kind: String,
    pub severity: String,
    pub contract_id: String,
    pub message: String,
    pub metric: f64,
    pub threshold: f64,
}

/// Sliding-window anomaly detector for contract event streams.
pub struct AnomalyDetector {
    contract_id: String,
    window: Duration,
    event_counts: Vec<(Instant, u32)>,
    value_samples: Vec<f64>,
    baseline_rate: f64,
    spike_multiplier: f64,
}

impl AnomalyDetector {
    pub fn new(contract_id: impl Into<String>) -> Self {
        Self {
            contract_id: contract_id.into(),
            window: Duration::from_secs(60),
            event_counts: Vec::new(),
            value_samples: Vec::new(),
            baseline_rate: 5.0,
            spike_multiplier: 3.0,
        }
    }

    pub fn record_event(&mut self, numeric_value: Option<f64>) -> Option<AnomalyFinding> {
        let now = Instant::now();
        self.event_counts.push((now, 1));
        self.prune_old(now);

        if let Some(v) = numeric_value {
            self.value_samples.push(v);
            if self.value_samples.len() > 100 {
                self.value_samples.remove(0);
            }
            if let Some(anomaly) = self.detect_value_outlier(v) {
                return Some(anomaly);
            }
        }

        self.detect_rate_spike()
    }

    fn prune_old(&mut self, now: Instant) {
        self.event_counts
            .retain(|(t, _)| now.duration_since(*t) <= self.window);
    }

    fn events_per_minute(&self) -> f64 {
        self.event_counts.iter().map(|(_, c)| *c as f64).sum()
    }

    fn detect_rate_spike(&self) -> Option<AnomalyFinding> {
        let rate = self.events_per_minute();
        let threshold = self.baseline_rate * self.spike_multiplier;
        if rate > threshold && rate > self.baseline_rate {
            Some(AnomalyFinding {
                kind: "event-rate-spike".into(),
                severity: "high".into(),
                contract_id: self.contract_id.clone(),
                message: format!(
                    "Event rate spike detected: {:.1} events/min (baseline {:.1})",
                    rate, self.baseline_rate
                ),
                metric: rate,
                threshold,
            })
        } else {
            None
        }
    }

    fn detect_value_outlier(&self, value: f64) -> Option<AnomalyFinding> {
        if self.value_samples.len() < 5 {
            return None;
        }
        let mean = self.value_samples.iter().sum::<f64>() / self.value_samples.len() as f64;
        let variance = self
            .value_samples
            .iter()
            .map(|v| (v - mean).powi(2))
            .sum::<f64>()
            / self.value_samples.len() as f64;
        let stddev = variance.sqrt().max(1.0);
        let z = (value - mean).abs() / stddev;
        if z > 3.0 {
            Some(AnomalyFinding {
                kind: "value-outlier".into(),
                severity: "medium".into(),
                contract_id: self.contract_id.clone(),
                message: format!(
                    "Statistical outlier: value {:.4} (mean {:.4}, z-score {:.2})",
                    value, mean, z
                ),
                metric: z,
                threshold: 3.0,
            })
        } else {
            None
        }
    }
}

/// Aggregate anomaly stats across multiple contracts.
#[derive(Default)]
pub struct AnomalyAggregator {
    by_contract: HashMap<String, u32>,
}

impl AnomalyAggregator {
    pub fn record(&mut self, contract_id: &str) {
        *self.by_contract.entry(contract_id.to_string()).or_insert(0) += 1;
    }

    pub fn top_contracts(&self, limit: usize) -> Vec<(String, u32)> {
        let mut entries: Vec<_> = self
            .by_contract
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        entries.sort_by_key(|b| std::cmp::Reverse(b.1));
        entries.truncate(limit);
        entries
    }
}
