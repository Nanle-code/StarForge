//! `starforge cost` — AI-assisted deployment cost management: budgets,
//! forecasting, cross-network comparison, and aggregate reporting.
//!
//! Builds on top of `starforge gas estimate`'s cost-history store; run that
//! command first (or with `--save`, the default) to build up the history
//! this command reports, forecasts, and enforces budgets against.

use crate::utils::{
    batch_forecast as bf, config, cost_estimation as ce, cost_management as cm, print as p,
    simulation_resources as sr,
};
use anyhow::Result;
use clap::Subcommand;
use colored::*;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum CostCommands {
    /// Manage recurring spending budgets per network
    Budget {
        #[command(subcommand)]
        action: BudgetAction,
    },
    /// Estimate a wasm's deployment cost and check it against configured
    /// budgets for that network (budget enforcement)
    Check {
        /// Path to the compiled wasm
        wasm: PathBuf,
        /// Target network
        #[arg(long, default_value = "testnet")]
        network: String,
        /// Exit with a non-zero status if any budget would be exceeded
        /// (suitable for gating CI/CD deploy pipelines)
        #[arg(long)]
        enforce: bool,
    },
    /// Project future deployment costs for a network from historical trend
    Forecast {
        /// Network to forecast
        #[arg(long, default_value = "testnet")]
        network: String,
        /// Number of future deployments to project
        #[arg(long, default_value = "3")]
        periods: usize,
    },
    /// Forecast aggregate fees for a batch of planned invokes BEFORE any of
    /// them are submitted, so the batch can be vetted against a budget and
    /// avoid running out of XLM mid-batch
    ForecastBatch {
        /// Path to the batch manifest (JSON or YAML) of invoke intents
        #[arg(value_name = "MANIFEST")]
        manifest: PathBuf,
        /// Default network when the manifest does not specify one
        #[arg(long, default_value = "testnet")]
        network: String,
        /// Safety margin, in percent, applied to simulated resource fees
        #[arg(long, default_value_t = sr::DEFAULT_FEE_MARGIN_PERCENT)]
        margin: u32,
        /// Per-operation inclusion (base) fee in stroops
        #[arg(long, default_value_t = sr::DEFAULT_INCLUSION_FEE_STROOPS)]
        inclusion_fee: u64,
        /// Exit with a non-zero status if the forecast exceeds the manifest
        /// budget or any per-invoke fee cap (suitable for gating CI/CD)
        #[arg(long)]
        enforce: bool,
    },
    /// Compare estimated deployment cost for the same wasm across networks
    CompareNetworks {
        /// Path to the compiled wasm
        wasm: PathBuf,
        /// Comma-separated list of networks to compare
        #[arg(long, default_value = "testnet,mainnet,futurenet")]
        networks: String,
    },
    /// Aggregate cost report: totals, averages, cost-driver breakdown, and
    /// the most common optimization opportunities across deployment history
    Report {
        /// Filter to a single network (omit for all networks)
        #[arg(long)]
        network: Option<String>,
    },
    /// Price a `simulateTransaction` response: report CPU, memory, footprint,
    /// and the minimum resource fee, then check it against configured budgets
    Resources {
        /// Path to a saved `simulateTransaction` JSON response
        #[arg(long)]
        file: PathBuf,
        /// Network whose budgets the resulting fee is checked against
        #[arg(long, default_value = "testnet")]
        network: String,
        /// Safety margin, in percent, applied to the minimum resource fee
        #[arg(long, default_value_t = sr::DEFAULT_FEE_MARGIN_PERCENT)]
        margin: u32,
        /// Per-operation inclusion (base) fee in stroops
        #[arg(long, default_value_t = sr::DEFAULT_INCLUSION_FEE_STROOPS)]
        inclusion_fee: u64,
        /// Exit with a non-zero status if the fee would exceed a budget
        /// (suitable for gating CI/CD pipelines)
        #[arg(long)]
        enforce: bool,
    },
}

#[derive(Subcommand)]
pub enum BudgetAction {
    /// Set (or replace) the budget for a network
    Set {
        /// Network this budget applies to
        #[arg(long, default_value = "testnet")]
        network: String,
        /// Spending cap in XLM per period
        #[arg(long)]
        amount: f64,
        /// Reset period: daily, weekly, or monthly
        #[arg(long, default_value = "monthly")]
        period: String,
        /// Optional human-readable label
        #[arg(long)]
        label: Option<String>,
    },
    /// List all configured budgets and their current status
    List,
    /// Show spend-to-date vs. limit for one or all budgets
    Status {
        /// Filter to a single network (omit for all)
        #[arg(long)]
        network: Option<String>,
    },
    /// Remove the budget configured for a network
    Remove {
        /// Network whose budget to remove
        #[arg(long)]
        network: String,
    },
}

pub async fn handle(cmd: CostCommands) -> Result<()> {
    match cmd {
        CostCommands::Budget { action } => budget(action),
        CostCommands::Check {
            wasm,
            network,
            enforce,
        } => check(wasm, network, enforce),
        CostCommands::Forecast { network, periods } => forecast(network, periods),
        CostCommands::ForecastBatch {
            manifest,
            network,
            margin,
            inclusion_fee,
            enforce,
        } => forecast_batch(&manifest, &network, margin, inclusion_fee, enforce).await,
        CostCommands::CompareNetworks { wasm, networks } => compare_networks(wasm, networks),
        CostCommands::Report { network } => report(network),
        CostCommands::Resources {
            file,
            network,
            margin,
            inclusion_fee,
            enforce,
        } => resources(file, network, margin, inclusion_fee, enforce),
    }
}

/// Outcome of checking a simulated resource fee against one budget.
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceBudgetCheck {
    pub network: String,
    pub limit_xlm: f64,
    pub spent_xlm: f64,
    pub projected_spent_xlm: f64,
    pub would_exceed: bool,
}

/// Project the period spend for each budget on `network` once a transaction
/// costing `fee_xlm` is submitted.
///
/// Pure function — takes budgets and history as slices so the decision logic is
/// testable without touching disk.
pub fn check_resource_fee_against(
    fee_xlm: f64,
    network: &str,
    budgets: &[cm::Budget],
    history: &[ce::CostHistoryEntry],
) -> Vec<ResourceBudgetCheck> {
    budgets
        .iter()
        .filter(|b| b.network == network)
        .map(|b| {
            let status = cm::budget_status_for(b, history);
            let projected = status.spent_xlm + fee_xlm;
            ResourceBudgetCheck {
                network: b.network.clone(),
                limit_xlm: b.limit_xlm,
                spent_xlm: status.spent_xlm,
                projected_spent_xlm: projected,
                would_exceed: projected > b.limit_xlm,
            }
        })
        .collect()
}

fn resources(
    file: PathBuf,
    network: String,
    margin: u32,
    inclusion_fee: u64,
    enforce: bool,
) -> Result<()> {
    config::validate_file_path(&file, Some("json"))?;
    config::validate_network(&network)?;

    let raw = std::fs::read_to_string(&file).map_err(|e| {
        anyhow::anyhow!(
            "Failed to read simulation response {}: {}",
            file.display(),
            e
        )
    })?;

    let resources =
        sr::parse_simulation_response_str(&raw).map_err(|e| anyhow::anyhow!("{}", e))?;
    let plan =
        sr::plan_fee(&resources, margin, inclusion_fee).map_err(|e| anyhow::anyhow!("{}", e))?;

    sr::render_report(&resources, &plan);

    let fee_xlm = plan.recommended_fee_xlm();
    let budgets = cm::load_budgets()?;
    let history = ce::load_cost_history()?;
    let checks = check_resource_fee_against(fee_xlm, &network, &budgets, &history);

    println!();
    if checks.is_empty() {
        p::info(&format!(
            "No budget configured for '{}'. Set one with: starforge cost budget set --network {} --amount <xlm>",
            network, network
        ));
        return Ok(());
    }

    let mut exceeded = false;
    for check in &checks {
        p::kv_accent("Budget network", &check.network);
        p::kv("Limit", &format!("{:.7} XLM", check.limit_xlm));
        p::kv("Spent this period", &format!("{:.7} XLM", check.spent_xlm));
        p::kv("This transaction", &format!("{:.7} XLM", fee_xlm));
        p::kv(
            "Projected",
            &format!("{:.7} XLM", check.projected_spent_xlm),
        );
        if check.would_exceed {
            exceeded = true;
            p::warn("This transaction would exceed the budget for the current period.");
        } else {
            p::success("Within budget.");
        }
    }

    if exceeded && enforce {
        anyhow::bail!("Budget enforcement failed: simulated resource fee exceeds the budget");
    }

    Ok(())
}

fn budget(action: BudgetAction) -> Result<()> {
    match action {
        BudgetAction::Set {
            network,
            amount,
            period,
            label,
        } => {
            config::validate_network(&network)?;
            let period = cm::BudgetPeriod::parse(&period)?;
            let budget = cm::set_budget(&network, period, amount, label)?;

            p::header("Cost Budget — Set");
            p::success(&format!(
                "Budget set for '{}': {:.7} XLM / {}",
                budget.network, budget.limit_xlm, budget.period
            ));
            if let Some(label) = &budget.label {
                p::kv("Label", label);
            }
            Ok(())
        }
        BudgetAction::List => {
            let budgets = cm::load_budgets()?;
            p::header("Cost Budgets");
            if budgets.is_empty() {
                p::info("No budgets configured. Set one with: starforge cost budget set --network <net> --amount <xlm>");
                return Ok(());
            }
            let headers = &["Network", "Period", "Limit (XLM)", "Label"];
            let rows: Vec<Vec<String>> = budgets
                .iter()
                .map(|b| {
                    vec![
                        b.network.clone(),
                        b.period.to_string(),
                        format!("{:.7}", b.limit_xlm),
                        b.label.clone().unwrap_or_else(|| "—".to_string()),
                    ]
                })
                .collect();
            p::table(headers, &rows);
            Ok(())
        }
        BudgetAction::Status { network } => {
            let statuses = cm::budget_status(network.as_deref())?;
            p::header("Cost Budget Status");
            if statuses.is_empty() {
                p::info("No matching budgets configured.");
                return Ok(());
            }
            for status in &statuses {
                println!();
                p::kv_accent("Network", &status.budget.network);
                p::kv("Period", &status.budget.period.to_string());
                p::kv("Limit", &format!("{:.7} XLM", status.budget.limit_xlm));
                p::kv("Spent", &format!("{:.7} XLM", status.spent_xlm));
                p::kv("Remaining", &format!("{:.7} XLM", status.remaining_xlm));
                p::kv("Used", &format!("{:.1}%", status.percent_used));
                p::kv(
                    "Deployments in period",
                    &status.deployments_in_period.to_string(),
                );
                if status.exceeded {
                    p::warn("Budget exceeded for the current period.");
                }
            }
            Ok(())
        }
        BudgetAction::Remove { network } => {
            let removed = cm::remove_budget(&network)?;
            p::header("Cost Budget — Remove");
            if removed {
                p::success(&format!("Budget removed for '{}'", network));
            } else {
                p::info(&format!("No budget was configured for '{}'", network));
            }
            Ok(())
        }
    }
}

fn check(wasm: PathBuf, network: String, enforce: bool) -> Result<()> {
    config::validate_file_path(&wasm, Some("wasm"))?;
    config::validate_network(&network)?;

    p::header("Cost Budget Check");
    p::kv("Wasm", &wasm.display().to_string());
    p::kv("Network", &network);

    let estimate = ce::estimate_deployment_cost(&wasm, &network)?;
    p::kv("Estimated fee", &estimate.fee_xlm_display());

    let results = cm::check_budget(&estimate)?;

    println!();
    if results.is_empty() {
        p::info(&format!(
            "No budget configured for '{}'. Set one with: starforge cost budget set --network {} --amount <xlm>",
            network, network
        ));
        return Ok(());
    }

    let mut any_exceeded = false;
    for result in &results {
        let label = result
            .status
            .budget
            .label
            .clone()
            .unwrap_or_else(|| result.status.budget.network.clone());
        if result.would_exceed {
            any_exceeded = true;
            println!(
                "{} Budget '{}' would be exceeded: {:.7} XLM projected vs {:.7} XLM limit",
                "✗".red().bold(),
                label,
                result.projected_spent_xlm,
                result.status.budget.limit_xlm
            );
        } else {
            println!(
                "{} Budget '{}' OK: {:.7} XLM projected vs {:.7} XLM limit ({:.1}% used)",
                "✓".green(),
                label,
                result.projected_spent_xlm,
                result.status.budget.limit_xlm,
                (result.projected_spent_xlm / result.status.budget.limit_xlm) * 100.0
            );
        }
    }

    if any_exceeded && enforce {
        anyhow::bail!("Deployment blocked: one or more budgets would be exceeded (--enforce)");
    }

    Ok(())
}

fn forecast(network: String, periods: usize) -> Result<()> {
    config::validate_network(&network)?;

    p::header("Cost Forecast");
    p::kv("Network", &network);

    let forecast = cm::forecast_costs(&network, periods)?;

    p::kv("Sample size", &forecast.sample_size.to_string());
    p::kv("Average fee", &format!("{:.7} XLM", forecast.avg_fee_xlm));
    p::kv(
        "Trend",
        &format!(
            "{:+.7} XLM per deployment",
            forecast.trend_xlm_per_deployment
        ),
    );
    p::kv(
        "Confidence",
        &format!("{:?}", forecast.confidence).to_lowercase(),
    );

    println!();
    p::info("Projected costs:");
    for p_cost in &forecast.projected {
        println!(
            "  +{} deployment(s) → {:.7} XLM",
            p_cost.deployment_offset, p_cost.projected_fee_xlm
        );
    }

    if forecast.confidence == cm::ForecastConfidence::Low {
        println!();
        p::warn("Low confidence: fewer than 3 historical deployments for this network.");
    }

    Ok(())
}

/// Forecast the aggregate fee for a batch of planned invokes from a manifest,
/// printing per-item estimates plus the batch total.
async fn forecast_batch(
    manifest_path: &Path,
    default_network: &str,
    margin: u32,
    inclusion_fee: u64,
    enforce: bool,
) -> Result<()> {
    let manifest = bf::load_manifest(manifest_path)?;

    p::header("Batch Invoke Cost Forecast");
    p::kv("Manifest", &manifest_path.display().to_string());
    p::kv("Default network", default_network);
    if let Some(budget) = manifest.budget_xlm {
        p::kv("Budget", &format!("{:.7} XLM", budget));
    }
    p::kv("Invokes", &manifest.invokes.len().to_string());

    if manifest.invokes.is_empty() {
        println!();
        p::warn("Manifest contains no invokes — nothing to forecast.");
        return Ok(());
    }

    println!();
    let forecast =
        bf::estimate_batch_forecast(&manifest, default_network, margin, inclusion_fee).await?;

    let headers = &[
        "#",
        "Call",
        "Network",
        "Fee (stroops)",
        "Fee (XLM)",
        "Source",
        "Variance",
    ];
    let rows: Vec<Vec<String>> = forecast
        .items
        .iter()
        .map(|item| {
            vec![
                (item.index + 1).to_string(),
                item.label().to_string(),
                item.network.clone(),
                item.fee_stroops.to_string(),
                format!("{:.7}", item.fee_xlm),
                format!("{:?}", item.source).to_lowercase(),
                if item.high_variance {
                    "HIGH".to_string()
                } else {
                    "ok".to_string()
                },
            ]
        })
        .collect();
    p::table(headers, &rows);

    println!();
    p::kv("Simulated", &forecast.simulated_count.to_string());
    p::kv(
        "Heuristic (not simulated)",
        &forecast.heuristic_count.to_string(),
    );
    p::kv(
        "Min / Max / Median",
        &format!(
            "{} / {} / {} stroops",
            forecast.min_fee_stroops, forecast.max_fee_stroops, forecast.median_fee_stroops
        ),
    );
    p::kv(
        "Average per invoke",
        &format!("{} stroops", forecast.avg_fee_stroops),
    );
    p::kv_accent(
        "Estimated batch total",
        &format!(
            "{} stroops ({:.7} XLM)",
            forecast.total_fee_stroops, forecast.total_fee_xlm
        ),
    );

    if forecast.would_exceed_budget {
        println!();
        p::warn("The batch would exceed the manifest budget.");
    }

    if forecast.high_variance_count > 0 {
        println!();
        p::info("High-variance calls:");
        for item in forecast.items.iter().filter(|item| item.high_variance) {
            println!(
                "  [{}] {} — {}",
                item.index + 1,
                item.label(),
                item.variance_reasons.join("; ")
            );
            for error in &item.errors {
                println!("      error: {}", error);
            }
        }
    }

    for warning in &forecast.warnings {
        println!();
        p::warn(warning);
    }

    if enforce
        && (forecast.would_exceed_budget || forecast.items.iter().any(|e| e.would_exceed_cap))
    {
        anyhow::bail!(
            "Batch forecast exceeds the configured budget or a per-invoke fee cap (--enforce)"
        );
    }

    Ok(())
}

fn compare_networks(wasm: PathBuf, networks: String) -> Result<()> {
    config::validate_file_path(&wasm, Some("wasm"))?;

    let network_list: Vec<String> = networks
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    p::header("Network Cost Comparison");
    p::kv("Wasm", &wasm.display().to_string());
    p::kv("Networks", &network_list.join(", "));
    println!();

    let comparisons = cm::compare_networks(&wasm, &network_list)?;

    let headers = &[
        "Network",
        "Multiplier",
        "Adjusted Fee (stroops)",
        "Adjusted Fee (XLM)",
    ];
    let rows: Vec<Vec<String>> = comparisons
        .iter()
        .map(|c| {
            vec![
                c.network.clone(),
                format!("{:.2}x", c.multiplier),
                c.adjusted_total_stroops.to_string(),
                format!("{:.7}", c.adjusted_total_xlm),
            ]
        })
        .collect();
    p::table(headers, &rows);

    if let Some(cheapest) = comparisons.first() {
        println!();
        p::success(&format!(
            "Cheapest: {} ({:.7} XLM)",
            cheapest.network, cheapest.adjusted_total_xlm
        ));
    }

    Ok(())
}

fn report(network: Option<String>) -> Result<()> {
    p::header("Deployment Cost Report");
    if let Some(net) = &network {
        p::kv("Network", net);
    } else {
        p::kv("Network", "all");
    }

    let report = cm::generate_cost_report(network.as_deref())?;

    println!();
    if report.deployment_count == 0 {
        p::info("No cost history recorded yet. Run `starforge gas estimate <wasm> --network <net>` first.");
        return Ok(());
    }

    p::kv("Deployments", &report.deployment_count.to_string());
    p::kv("Total spent", &format!("{:.7} XLM", report.total_spent_xlm));
    p::kv("Average fee", &format!("{:.7} XLM", report.avg_fee_xlm));
    p::kv("Min fee", &format!("{:.7} XLM", report.min_fee_xlm));
    p::kv("Max fee", &format!("{:.7} XLM", report.max_fee_xlm));

    println!();
    p::info("Cost driver breakdown:");
    println!("  Gas:     {:.1}%", report.gas_share_percent);
    println!("  Storage: {:.1}%", report.storage_share_percent);
    println!("  Base:    {:.1}%", report.base_share_percent);

    if !report.top_suggestion_categories.is_empty() {
        println!();
        p::info("Most common optimization opportunities:");
        for (category, count) in report.top_suggestion_categories.iter().take(5) {
            println!("  {} — seen in {} deployment(s)", category, count);
        }
    }

    if let Some(entry) = &report.most_expensive {
        println!();
        p::info("Most expensive deployment:");
        p::kv("Wasm", &entry.estimate.wasm_path);
        p::kv("Network", &entry.estimate.network);
        p::kv("Fee", &entry.estimate.fee_xlm_display());
        p::kv("Date", &entry.estimate.estimated_at);
    }

    Ok(())
}

#[cfg(test)]
mod resource_budget_tests {
    use super::*;

    fn budget(network: &str, limit_xlm: f64) -> cm::Budget {
        cm::Budget {
            network: network.to_string(),
            period: cm::BudgetPeriod::Monthly,
            limit_xlm,
            label: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn reports_within_budget_for_a_small_fee() {
        let checks = check_resource_fee_against(0.001, "testnet", &[budget("testnet", 1.0)], &[]);
        assert_eq!(checks.len(), 1);
        assert!(!checks[0].would_exceed);
        assert!((checks[0].projected_spent_xlm - 0.001).abs() < f64::EPSILON);
    }

    #[test]
    fn ignores_budgets_for_other_networks() {
        let checks = check_resource_fee_against(5.0, "mainnet", &[budget("testnet", 1.0)], &[]);
        assert!(checks.is_empty());
    }

    #[test]
    fn fee_exactly_at_the_limit_is_not_an_overrun() {
        let checks = check_resource_fee_against(1.0, "testnet", &[budget("testnet", 1.0)], &[]);
        assert!(!checks[0].would_exceed);
    }

    #[test]
    fn fee_above_the_limit_is_flagged() {
        let checks =
            check_resource_fee_against(1.000_000_1, "testnet", &[budget("testnet", 1.0)], &[]);
        assert!(checks[0].would_exceed);
    }
}
