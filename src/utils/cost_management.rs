//! AI-assisted deployment cost management: budgets, enforcement, forecasting,
//! cross-network comparison, and aggregate reporting.
//!
//! Builds entirely on top of [`crate::utils::cost_estimation`] — this module
//! owns no fee-calculation logic of its own, only the higher-level workflows
//! (budgeting, trend projection, reporting) that operate over a history of
//! [`CostEstimate`]s.

use anyhow::Result;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::utils::config;
use crate::utils::cost_estimation::{self as ce, CostEstimate, CostHistoryEntry};

// ── Budgets ──────────────────────────────────────────────────────────────────

/// How often a [`Budget`]'s spending window resets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BudgetPeriod {
    Daily,
    Weekly,
    Monthly,
}

impl BudgetPeriod {
    pub fn duration(self) -> ChronoDuration {
        match self {
            BudgetPeriod::Daily => ChronoDuration::days(1),
            BudgetPeriod::Weekly => ChronoDuration::days(7),
            BudgetPeriod::Monthly => ChronoDuration::days(30),
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "daily" | "day" => Ok(BudgetPeriod::Daily),
            "weekly" | "week" => Ok(BudgetPeriod::Weekly),
            "monthly" | "month" => Ok(BudgetPeriod::Monthly),
            _ => anyhow::bail!(
                "Unknown budget period '{}'. Use 'daily', 'weekly', or 'monthly'.",
                s
            ),
        }
    }
}

impl std::fmt::Display for BudgetPeriod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BudgetPeriod::Daily => write!(f, "daily"),
            BudgetPeriod::Weekly => write!(f, "weekly"),
            BudgetPeriod::Monthly => write!(f, "monthly"),
        }
    }
}

/// A recurring spending cap for deployments on a given network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Budget {
    pub network: String,
    pub period: BudgetPeriod,
    pub limit_xlm: f64,
    pub label: Option<String>,
    pub created_at: String,
}

fn budgets_path() -> PathBuf {
    config::config_dir().join("cost_budgets.json")
}

/// Load all persisted budgets from disk.
pub fn load_budgets() -> Result<Vec<Budget>> {
    let path = budgets_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data).unwrap_or_default())
}

/// Persist the full list of budgets to disk.
pub fn save_budgets(budgets: &[Budget]) -> Result<()> {
    let path = budgets_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_string_pretty(budgets)?)?;
    Ok(())
}

/// Set (or replace) the budget for a network. A network has at most one
/// active budget at a time — setting a new one replaces the old.
pub fn set_budget(
    network: &str,
    period: BudgetPeriod,
    limit_xlm: f64,
    label: Option<String>,
) -> Result<Budget> {
    if limit_xlm <= 0.0 {
        anyhow::bail!("Budget limit must be positive, got {}", limit_xlm);
    }
    let mut budgets = load_budgets()?;
    budgets.retain(|b| b.network != network);
    let budget = Budget {
        network: network.to_string(),
        period,
        limit_xlm,
        label,
        created_at: Utc::now().to_rfc3339(),
    };
    budgets.push(budget.clone());
    save_budgets(&budgets)?;
    Ok(budget)
}

/// Remove the budget configured for a network. Returns `true` if one existed.
pub fn remove_budget(network: &str) -> Result<bool> {
    let mut budgets = load_budgets()?;
    let before = budgets.len();
    budgets.retain(|b| b.network != network);
    let removed = budgets.len() != before;
    save_budgets(&budgets)?;
    Ok(removed)
}

// ── Budget status / enforcement ─────────────────────────────────────────────

/// Spend-to-date snapshot for a single budget.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetStatus {
    pub budget: Budget,
    pub period_start: String,
    pub spent_xlm: f64,
    pub remaining_xlm: f64,
    pub percent_used: f64,
    pub exceeded: bool,
    pub deployments_in_period: usize,
}

fn parse_rfc3339(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Sum of `total_fee_xlm` for history entries on `network` recorded within
/// `[now - period, now]`.
fn spend_in_period(
    history: &[CostHistoryEntry],
    network: &str,
    period: BudgetPeriod,
    now: DateTime<Utc>,
) -> (f64, usize) {
    let window_start = now - period.duration();
    let mut spent = 0.0;
    let mut count = 0;
    for entry in history {
        if entry.estimate.network != network {
            continue;
        }
        if let Some(ts) = parse_rfc3339(&entry.estimate.estimated_at) {
            if ts >= window_start && ts <= now {
                spent += entry.estimate.total_fee_xlm;
                count += 1;
            }
        }
    }
    (spent, count)
}

/// Compute the current status of a single budget against a supplied history
/// slice. Pure function — does not touch disk.
pub fn budget_status_for(budget: &Budget, history: &[CostHistoryEntry]) -> BudgetStatus {
    let now = Utc::now();
    let (spent, count) = spend_in_period(history, &budget.network, budget.period, now);
    let remaining = budget.limit_xlm - spent;
    let percent_used = if budget.limit_xlm > 0.0 {
        (spent / budget.limit_xlm) * 100.0
    } else {
        0.0
    };
    BudgetStatus {
        budget: budget.clone(),
        period_start: (now - budget.period.duration()).to_rfc3339(),
        spent_xlm: spent,
        remaining_xlm: remaining,
        percent_used,
        exceeded: spent > budget.limit_xlm,
        deployments_in_period: count,
    }
}

/// Compute status for all configured budgets, optionally filtered to a
/// single network. Loads budgets and cost history from disk.
pub fn budget_status(network: Option<&str>) -> Result<Vec<BudgetStatus>> {
    let budgets = load_budgets()?;
    let history = ce::load_cost_history()?;
    Ok(budgets
        .iter()
        .filter(|b| network.map_or(true, |n| b.network == n))
        .map(|b| budget_status_for(b, &history))
        .collect())
}

/// Result of checking a prospective deployment's cost against a budget.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetCheckResult {
    pub status: BudgetStatus,
    /// What the period spend would become if this estimate were recorded.
    pub projected_spent_xlm: f64,
    pub would_exceed: bool,
}

/// Check `estimate` against every budget configured for its network, given a
/// supplied history slice (which should not yet include `estimate`). Pure
/// function — does not touch disk.
pub fn check_budget_against(
    estimate: &CostEstimate,
    budgets: &[Budget],
    history: &[CostHistoryEntry],
) -> Vec<BudgetCheckResult> {
    budgets
        .iter()
        .filter(|b| b.network == estimate.network)
        .map(|b| {
            let status = budget_status_for(b, history);
            let projected = status.spent_xlm + estimate.total_fee_xlm;
            BudgetCheckResult {
                would_exceed: projected > status.budget.limit_xlm,
                projected_spent_xlm: projected,
                status,
            }
        })
        .collect()
}

/// Check `estimate` against every budget configured for its network. Loads
/// budgets and cost history from disk.
pub fn check_budget(estimate: &CostEstimate) -> Result<Vec<BudgetCheckResult>> {
    let budgets = load_budgets()?;
    let history = ce::load_cost_history()?;
    Ok(check_budget_against(estimate, &budgets, &history))
}

// ── Forecasting ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ForecastConfidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectedCost {
    /// How many deployments ahead of the last recorded one this projects to.
    pub deployment_offset: usize,
    pub projected_fee_xlm: f64,
}

/// A trend-based projection of future deployment costs for a network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostForecast {
    pub network: String,
    pub sample_size: usize,
    pub avg_fee_xlm: f64,
    /// Linear trend in XLM per deployment; positive means costs are rising.
    pub trend_xlm_per_deployment: f64,
    pub projected: Vec<ProjectedCost>,
    pub confidence: ForecastConfidence,
}

/// Ordinary least-squares fit of `y = slope * x + intercept`, where `x` is
/// the zero-based index into `ys`.
fn least_squares(ys: &[f64]) -> (f64, f64) {
    let n = ys.len() as f64;
    let x_mean = (ys.len() as f64 - 1.0) / 2.0;
    let y_mean = ys.iter().sum::<f64>() / n;

    let mut num = 0.0;
    let mut den = 0.0;
    for (i, y) in ys.iter().enumerate() {
        let x = i as f64;
        num += (x - x_mean) * (y - y_mean);
        den += (x - x_mean).powi(2);
    }

    if den.abs() < f64::EPSILON {
        return (0.0, y_mean);
    }

    let slope = num / den;
    let intercept = y_mean - slope * x_mean;
    (slope, intercept)
}

/// Project future deployment costs for `network` from a supplied,
/// chronologically-ordered history slice. Pure function — does not touch
/// disk.
pub fn forecast_from(
    history: &[CostHistoryEntry],
    network: &str,
    periods_ahead: usize,
) -> Result<CostForecast> {
    let ys: Vec<f64> = history
        .iter()
        .filter(|e| e.estimate.network == network)
        .map(|e| e.estimate.total_fee_xlm)
        .collect();

    let n = ys.len();
    if n == 0 {
        anyhow::bail!(
            "No cost history for network '{}'. Run `starforge gas estimate <wasm> --network {}` \
             a few times first so a trend can be established.",
            network,
            network
        );
    }

    let avg = ys.iter().sum::<f64>() / n as f64;
    let (slope, intercept) = least_squares(&ys);

    let confidence = if n >= 10 {
        ForecastConfidence::High
    } else if n >= 3 {
        ForecastConfidence::Medium
    } else {
        ForecastConfidence::Low
    };

    let steps = periods_ahead.max(1);
    let projected = (1..=steps)
        .map(|i| {
            let x = (n - 1 + i) as f64;
            ProjectedCost {
                deployment_offset: i,
                projected_fee_xlm: (slope * x + intercept).max(0.0),
            }
        })
        .collect();

    Ok(CostForecast {
        network: network.to_string(),
        sample_size: n,
        avg_fee_xlm: avg,
        trend_xlm_per_deployment: slope,
        projected,
        confidence,
    })
}

/// Project future deployment costs for `network`, loading history from disk.
pub fn forecast_costs(network: &str, periods_ahead: usize) -> Result<CostForecast> {
    let mut history = ce::load_cost_history()?;
    history.retain(|e| e.estimate.network == network);
    history.sort_by(|a, b| a.estimate.estimated_at.cmp(&b.estimate.estimated_at));
    forecast_from(&history, network, periods_ahead)
}

// ── Network cost comparison ─────────────────────────────────────────────────

/// Heuristic relative fee multiplier per network, approximating typical
/// congestion / fee-market differences between Soroban networks. `1.0` is
/// the baseline. This is a local approximation for side-by-side comparison,
/// not a live fee-market query.
pub fn network_fee_multiplier(network: &str) -> f64 {
    match network {
        "mainnet" => 1.15,
        "testnet" => 1.0,
        "futurenet" => 0.9,
        "docker-testnet" | "local" => 0.1,
        _ => 1.0,
    }
}

/// A single network's entry in a cross-network cost comparison.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkCostComparison {
    pub network: String,
    pub multiplier: f64,
    pub base_total_stroops: u64,
    pub adjusted_total_stroops: u64,
    pub adjusted_total_xlm: f64,
}

/// Estimate deployment cost for the same wasm across multiple networks so
/// they can be compared side by side, cheapest first.
pub fn compare_networks(
    wasm_path: &Path,
    networks: &[String],
) -> Result<Vec<NetworkCostComparison>> {
    if networks.is_empty() {
        anyhow::bail!("Provide at least one network to compare");
    }
    let mut results = Vec::with_capacity(networks.len());
    for network in networks {
        let est = ce::estimate_deployment_cost(wasm_path, network)?;
        let multiplier = network_fee_multiplier(network);
        let adjusted = (est.total_fee_stroops as f64 * multiplier) as u64;
        results.push(NetworkCostComparison {
            network: network.clone(),
            multiplier,
            base_total_stroops: est.total_fee_stroops,
            adjusted_total_stroops: adjusted,
            adjusted_total_xlm: adjusted as f64 / 10_000_000.0,
        });
    }
    results.sort_by_key(|a| a.adjusted_total_stroops);
    Ok(results)
}

// ── Aggregate reporting ─────────────────────────────────────────────────────

/// An aggregate cost report over a slice of deployment history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostReport {
    pub network: Option<String>,
    pub deployment_count: usize,
    pub total_spent_xlm: f64,
    pub avg_fee_xlm: f64,
    pub min_fee_xlm: f64,
    pub max_fee_xlm: f64,
    pub gas_share_percent: f64,
    pub storage_share_percent: f64,
    pub base_share_percent: f64,
    /// Suggestion categories seen across history, most frequent first.
    pub top_suggestion_categories: Vec<(String, usize)>,
    pub most_expensive: Option<CostHistoryEntry>,
}

fn empty_report(network: Option<&str>) -> CostReport {
    CostReport {
        network: network.map(str::to_string),
        deployment_count: 0,
        total_spent_xlm: 0.0,
        avg_fee_xlm: 0.0,
        min_fee_xlm: 0.0,
        max_fee_xlm: 0.0,
        gas_share_percent: 0.0,
        storage_share_percent: 0.0,
        base_share_percent: 0.0,
        top_suggestion_categories: Vec::new(),
        most_expensive: None,
    }
}

/// Build an aggregate cost report from a supplied history slice, optionally
/// filtered to a single network. Pure function — does not touch disk.
pub fn generate_cost_report_from(
    history: &[CostHistoryEntry],
    network: Option<&str>,
) -> CostReport {
    let filtered: Vec<&CostHistoryEntry> = history
        .iter()
        .filter(|e| network.map_or(true, |n| e.estimate.network == n))
        .collect();

    if filtered.is_empty() {
        return empty_report(network);
    }

    let n = filtered.len();
    let fees: Vec<f64> = filtered.iter().map(|e| e.estimate.total_fee_xlm).collect();
    let total_xlm: f64 = fees.iter().sum();
    let min_fee = fees.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_fee = fees.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    let total_gas: u64 = filtered
        .iter()
        .map(|e| e.estimate.gas.total_gas_stroops)
        .sum();
    let total_storage: u64 = filtered
        .iter()
        .map(|e| e.estimate.storage.total_storage_stroops)
        .sum();
    let total_base: u64 = filtered
        .iter()
        .map(|e| e.estimate.base_fee_stroops + e.estimate.large_contract_surcharge_stroops)
        .sum();
    let total_stroops = (total_gas + total_storage + total_base).max(1);

    let mut category_counts: HashMap<String, usize> = HashMap::new();
    for entry in &filtered {
        for s in &entry.estimate.suggestions {
            *category_counts.entry(s.category.clone()).or_insert(0) += 1;
        }
    }
    let mut top_suggestion_categories: Vec<(String, usize)> = category_counts.into_iter().collect();
    top_suggestion_categories.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let most_expensive = filtered
        .iter()
        .max_by(|a, b| {
            a.estimate
                .total_fee_stroops
                .cmp(&b.estimate.total_fee_stroops)
        })
        .map(|e| (*e).clone());

    CostReport {
        network: network.map(str::to_string),
        deployment_count: n,
        total_spent_xlm: total_xlm,
        avg_fee_xlm: total_xlm / n as f64,
        min_fee_xlm: min_fee,
        max_fee_xlm: max_fee,
        gas_share_percent: total_gas as f64 / total_stroops as f64 * 100.0,
        storage_share_percent: total_storage as f64 / total_stroops as f64 * 100.0,
        base_share_percent: total_base as f64 / total_stroops as f64 * 100.0,
        top_suggestion_categories,
        most_expensive,
    }
}

/// Build an aggregate cost report, loading history from disk.
pub fn generate_cost_report(network: Option<&str>) -> Result<CostReport> {
    let history = ce::load_cost_history()?;
    Ok(generate_cost_report_from(&history, network))
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::cost_estimation::{
        CostOptimizationSuggestion, GasBreakdown, StorageFeeBreakdown,
    };

    fn make_estimate(
        network: &str,
        total_stroops: u64,
        estimated_at: DateTime<Utc>,
    ) -> CostEstimate {
        CostEstimate {
            network: network.to_string(),
            wasm_path: "dummy.wasm".to_string(),
            wasm_sha256: "abc".to_string(),
            wasm_size_bytes: 100,
            gas: GasBreakdown {
                cpu_instructions: 0,
                memory_bytes: 0,
                cpu_fee_stroops: total_stroops / 2,
                memory_fee_stroops: 0,
                total_gas_stroops: total_stroops / 2,
            },
            storage: StorageFeeBreakdown {
                wasm_upload_bytes: 0,
                wasm_upload_fee_stroops: 0,
                instance_storage_stroops: total_stroops / 2,
                estimated_data_entries: 0,
                data_entries_fee_stroops: 0,
                total_storage_stroops: total_stroops / 2,
            },
            base_fee_stroops: 0,
            large_contract_surcharge_stroops: 0,
            total_fee_stroops: total_stroops,
            total_fee_xlm: total_stroops as f64 / 10_000_000.0,
            suggestions: vec![CostOptimizationSuggestion {
                category: "gas".to_string(),
                message: "test".to_string(),
                estimated_savings_stroops: 0,
            }],
            estimated_at: estimated_at.to_rfc3339(),
        }
    }

    fn make_entry(
        id: &str,
        network: &str,
        total_stroops: u64,
        estimated_at: DateTime<Utc>,
    ) -> CostHistoryEntry {
        CostHistoryEntry {
            id: id.to_string(),
            estimate: make_estimate(network, total_stroops, estimated_at),
        }
    }

    // ── BudgetPeriod ─────────────────────────────────────────────────────

    #[test]
    fn budget_period_parses_common_spellings() {
        assert_eq!(BudgetPeriod::parse("daily").unwrap(), BudgetPeriod::Daily);
        assert_eq!(BudgetPeriod::parse("Week").unwrap(), BudgetPeriod::Weekly);
        assert_eq!(
            BudgetPeriod::parse("MONTHLY").unwrap(),
            BudgetPeriod::Monthly
        );
        assert!(BudgetPeriod::parse("fortnightly").is_err());
    }

    // ── Budget status ────────────────────────────────────────────────────

    #[test]
    fn budget_status_reports_spend_within_window() {
        let now = Utc::now();
        let budget = Budget {
            network: "testnet".to_string(),
            period: BudgetPeriod::Weekly,
            limit_xlm: 1.0,
            label: None,
            created_at: now.to_rfc3339(),
        };
        let history = vec![
            make_entry("a", "testnet", 3_000_000, now - ChronoDuration::days(1)),
            make_entry("b", "testnet", 2_000_000, now - ChronoDuration::days(2)),
            // Outside the weekly window — must not count.
            make_entry("c", "testnet", 100_000_000, now - ChronoDuration::days(30)),
            // Different network — must not count.
            make_entry("d", "mainnet", 100_000_000, now - ChronoDuration::days(1)),
        ];

        let status = budget_status_for(&budget, &history);
        assert_eq!(status.deployments_in_period, 2);
        assert!((status.spent_xlm - 0.5).abs() < 1e-9);
        assert!(!status.exceeded);
    }

    #[test]
    fn budget_status_flags_exceeded() {
        let now = Utc::now();
        let budget = Budget {
            network: "testnet".to_string(),
            period: BudgetPeriod::Daily,
            limit_xlm: 0.1,
            label: None,
            created_at: now.to_rfc3339(),
        };
        let history = vec![make_entry("a", "testnet", 5_000_000, now)];
        let status = budget_status_for(&budget, &history);
        assert!(status.exceeded);
        assert!(status.remaining_xlm < 0.0);
        assert!(status.percent_used > 100.0);
    }

    // ── Budget enforcement ───────────────────────────────────────────────

    #[test]
    fn check_budget_flags_would_exceed() {
        let now = Utc::now();
        let budgets = vec![Budget {
            network: "testnet".to_string(),
            period: BudgetPeriod::Daily,
            limit_xlm: 0.5,
            label: None,
            created_at: now.to_rfc3339(),
        }];
        let history = vec![make_entry("a", "testnet", 4_000_000, now)]; // 0.4 XLM spent
        let candidate = make_estimate("testnet", 2_000_000, now); // +0.2 XLM => 0.6 total

        let results = check_budget_against(&candidate, &budgets, &history);
        assert_eq!(results.len(), 1);
        assert!(results[0].would_exceed);
        assert!((results[0].projected_spent_xlm - 0.6).abs() < 1e-9);
    }

    #[test]
    fn check_budget_only_matches_same_network() {
        let now = Utc::now();
        let budgets = vec![Budget {
            network: "mainnet".to_string(),
            period: BudgetPeriod::Daily,
            limit_xlm: 0.01,
            label: None,
            created_at: now.to_rfc3339(),
        }];
        let candidate = make_estimate("testnet", 5_000_000, now);
        let results = check_budget_against(&candidate, &budgets, &[]);
        assert!(results.is_empty());
    }

    // ── Forecasting ──────────────────────────────────────────────────────

    #[test]
    fn forecast_errors_with_no_history() {
        let result = forecast_from(&[], "testnet", 3);
        assert!(result.is_err());
    }

    #[test]
    fn forecast_projects_rising_trend() {
        let now = Utc::now();
        let history: Vec<CostHistoryEntry> = (0..5)
            .map(|i| {
                make_entry(
                    &format!("e{i}"),
                    "testnet",
                    1_000_000 * (i as u64 + 1),
                    now - ChronoDuration::days(5 - i),
                )
            })
            .collect();

        let forecast = forecast_from(&history, "testnet", 2).unwrap();
        assert_eq!(forecast.sample_size, 5);
        assert!(
            forecast.trend_xlm_per_deployment > 0.0,
            "cost is rising, trend should be positive"
        );
        assert_eq!(forecast.projected.len(), 2);
        // Projections should continue the upward trend beyond the last sample.
        assert!(forecast.projected[0].projected_fee_xlm > forecast.avg_fee_xlm);
        assert_eq!(forecast.confidence, ForecastConfidence::Medium);
    }

    #[test]
    fn forecast_flat_history_has_zero_trend() {
        let now = Utc::now();
        let history: Vec<CostHistoryEntry> = (0..4)
            .map(|i| make_entry(&format!("e{i}"), "testnet", 1_000_000, now))
            .collect();
        let forecast = forecast_from(&history, "testnet", 1).unwrap();
        assert!(forecast.trend_xlm_per_deployment.abs() < 1e-9);
    }

    // ── Network comparison ───────────────────────────────────────────────

    #[test]
    fn network_multiplier_orders_mainnet_above_testnet() {
        assert!(network_fee_multiplier("mainnet") > network_fee_multiplier("testnet"));
        assert!(network_fee_multiplier("testnet") > network_fee_multiplier("local"));
        assert_eq!(network_fee_multiplier("some-custom-net"), 1.0);
    }

    #[test]
    fn compare_networks_rejects_empty_list() {
        let result = compare_networks(Path::new("nonexistent.wasm"), &[]);
        assert!(result.is_err());
    }

    // ── Reporting ────────────────────────────────────────────────────────

    #[test]
    fn report_is_empty_for_no_history() {
        let report = generate_cost_report_from(&[], Some("testnet"));
        assert_eq!(report.deployment_count, 0);
        assert_eq!(report.total_spent_xlm, 0.0);
        assert!(report.most_expensive.is_none());
    }

    #[test]
    fn report_aggregates_across_history() {
        let now = Utc::now();
        let history = vec![
            make_entry("a", "testnet", 1_000_000, now),
            make_entry("b", "testnet", 3_000_000, now),
            make_entry("c", "mainnet", 9_000_000, now),
        ];

        let report = generate_cost_report_from(&history, Some("testnet"));
        assert_eq!(report.deployment_count, 2);
        assert!((report.total_spent_xlm - 0.4).abs() < 1e-9);
        assert!((report.avg_fee_xlm - 0.2).abs() < 1e-9);
        assert_eq!(report.most_expensive.as_ref().unwrap().id, "b");
        assert_eq!(report.top_suggestion_categories[0].0, "gas");
        assert_eq!(report.top_suggestion_categories[0].1, 2);
    }

    #[test]
    fn report_without_network_filter_includes_all() {
        let now = Utc::now();
        let history = vec![
            make_entry("a", "testnet", 1_000_000, now),
            make_entry("b", "mainnet", 1_000_000, now),
        ];
        let report = generate_cost_report_from(&history, None);
        assert_eq!(report.deployment_count, 2);
        assert!(report.network.is_none());
    }

    // ── Budget JSON round-trip ───────────────────────────────────────────

    #[test]
    fn budget_serializes_round_trip() {
        let budget = Budget {
            network: "testnet".to_string(),
            period: BudgetPeriod::Monthly,
            limit_xlm: 5.0,
            label: Some("staging deploys".to_string()),
            created_at: Utc::now().to_rfc3339(),
        };
        let json = serde_json::to_string(&budget).unwrap();
        let decoded: Budget = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.network, "testnet");
        assert_eq!(decoded.period, BudgetPeriod::Monthly);
        assert_eq!(decoded.limit_xlm, 5.0);
    }
}
