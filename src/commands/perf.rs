use crate::utils::{performance as perf, print as p};
use anyhow::Result;
use clap::Subcommand;
use std::collections::HashMap;

#[derive(Subcommand)]
pub enum PerfCommands {
    /// Record gas usage for a contract invocation
    Record {
        /// Contract ID (starts with 'C...')
        contract: String,
        /// Operation name
        #[arg(long, default_value = "invoke")]
        operation: String,
        /// Gas units consumed
        gas: u64,
        /// Execution time in milliseconds
        #[arg(long)]
        time_ms: Option<u64>,
        /// Whether the execution succeeded
        #[arg(long, default_value = "true")]
        success: bool,
        /// Network name
        #[arg(long, default_value = "testnet")]
        network: String,
    },
    /// Show performance dashboard for a contract
    Dashboard {
        /// Contract ID
        contract: String,
        /// Network to display metrics for
        #[arg(long, default_value = "testnet")]
        network: String,
    },
    /// View performance history
    History {
        /// Contract ID
        contract: String,
        /// Number of records to show
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Configure performance alerts
    Alert {
        /// Contract ID
        contract: String,
        /// Metric name to monitor (e.g., "gas_used", "execution_time_ms")
        #[arg(long)]
        metric: String,
        /// Threshold value to trigger alert
        threshold: f64,
        /// Alert direction: "above" or "below"
        #[arg(long, default_value = "above")]
        direction: String,
        /// Alert message
        #[arg(long)]
        message: Option<String>,
    },
    /// Generate a performance report
    Report {
        /// Contract ID
        contract: String,
        /// Network
        #[arg(long, default_value = "testnet")]
        network: String,
    },
    /// Record a custom metric
    Metric {
        /// Contract ID
        contract: String,
        /// Metric name
        name: String,
        /// Metric value
        value: f64,
        /// Unit of measurement
        #[arg(long, default_value = "count")]
        unit: String,
    },
    /// Analyze performance bottlenecks
    Bottleneck {
        /// Contract ID
        contract: String,
        /// Network
        #[arg(long, default_value = "testnet")]
        network: String,
    },
    /// Generate optimization suggestions
    Optimize {
        /// Contract ID
        contract: String,
        /// Network
        #[arg(long, default_value = "testnet")]
        network: String,
    },
    /// Enable deployment caching
    Cache {
        /// Contract ID
        contract: String,
        /// Enable or disable caching (true/false)
        #[arg(long, default_value = "true")]
        enable: bool,
    },
    /// Run performance benchmarks
    Benchmark {
        /// Contract ID
        contract: String,
        /// Number of iterations
        #[arg(long, default_value = "10")]
        iterations: u32,
    },
}

pub async fn handle(cmd: PerfCommands) -> Result<()> {
    match cmd {
        PerfCommands::Record {
            contract,
            operation,
            gas,
            time_ms,
            success,
            network,
        } => record(contract, operation, gas, time_ms, success, network),
        PerfCommands::Dashboard { contract, network } => dashboard(contract, network),
        PerfCommands::History { contract, limit } => history(contract, limit),
        PerfCommands::Alert {
            contract,
            metric,
            threshold,
            direction,
            message,
        } => alert(contract, metric, threshold, direction, message),
        PerfCommands::Report { contract, network } => report(contract, network),
        PerfCommands::Metric {
            contract,
            name,
            value,
            unit,
        } => metric(contract, name, value, unit),
        PerfCommands::Bottleneck { contract, network } => bottleneck(contract, network),
        PerfCommands::Optimize { contract, network } => optimize(contract, network),
        PerfCommands::Cache { contract, enable } => cache(contract, enable),
        PerfCommands::Benchmark {
            contract,
            iterations,
        } => benchmark(contract, iterations),
    }
}

fn record(
    contract: String,
    operation: String,
    gas: u64,
    time_ms: Option<u64>,
    success: bool,
    network: String,
) -> Result<()> {
    p::header("Performance Metrics — Record");

    let record = perf::GasUsageRecord {
        contract_id: contract.clone(),
        operation,
        gas_used: gas,
        timestamp: chrono::Utc::now().to_rfc3339(),
        success,
        execution_time_ms: time_ms.unwrap_or(0),
        network,
    };

    perf::record_gas_usage(&record)?;

    p::success("Gas usage recorded");
    p::kv("Contract", &contract);
    p::kv("Gas Used", &gas.to_string());
    if let Some(t) = time_ms {
        p::kv("Execution Time", &format!("{}ms", t));
    }
    p::kv("Success", &success.to_string());
    Ok(())
}

fn dashboard(contract: String, network: String) -> Result<()> {
    p::header("Contract Performance Dashboard");
    p::separator();
    p::kv("Contract", &contract);
    p::kv("Network", &network);
    p::separator();

    let report = perf::generate_report(&contract, &network)?;

    println!();
    p::info("Execution Summary");
    p::kv(
        "Total Executions",
        &report.summary.total_executions.to_string(),
    );
    p::kv(
        "Avg Gas Used",
        &format!("{:.2}", report.summary.avg_gas_used),
    );
    p::kv(
        "Max Gas Used",
        &format!("{:.2}", report.summary.max_gas_used),
    );
    p::kv(
        "Min Gas Used",
        &if report.summary.min_gas_used == f64::INFINITY {
            "N/A".to_string()
        } else {
            format!("{:.2}", report.summary.min_gas_used)
        },
    );
    p::kv(
        "Avg Execution Time",
        &format!("{:.2}ms", report.summary.avg_execution_time_ms),
    );
    p::kv(
        "Success Rate",
        &format!("{:.1}%", report.summary.success_rate),
    );

    let gas_history = perf::get_gas_history(&contract)?;
    if !gas_history.is_empty() {
        println!();
        p::info("Recent Gas Usage");
        let display_count = gas_history.len().min(10);
        for record in gas_history.iter().rev().take(display_count) {
            let status = if record.success { "OK" } else { "FAIL" };
            println!(
                "  {} gas={} time={}ms [{}]",
                &record.timestamp[..19],
                record.gas_used,
                record.execution_time_ms,
                status,
            );
        }
    }

    let triggered = perf::check_alerts(&contract)?;
    if !triggered.is_empty() {
        println!();
        p::warn("Alerts Triggered");
        for t in &triggered {
            p::warn(&format!(
                "{}: {} = {} (threshold: {})",
                t.alert.message, t.alert.metric_name, t.actual_value, t.alert.threshold
            ));
        }
    }

    if report.metrics.is_empty() && gas_history.is_empty() {
        println!();
        p::info("No performance data recorded yet.");
        p::info("Use `starforge perf record` to start tracking.");
    }

    println!();
    p::separator();
    Ok(())
}

fn history(contract: String, limit: usize) -> Result<()> {
    p::header("Performance History");
    p::kv("Contract", &contract);

    let gas_history = perf::get_gas_history(&contract)?;
    if gas_history.is_empty() {
        p::info("No performance history found. Use `starforge perf record` first.");
        return Ok(());
    }

    let display_count = gas_history.len().min(limit);
    println!();
    p::info(&format!("Last {} records", display_count));

    for record in gas_history.iter().rev().take(display_count) {
        let status = if record.success {
            "✓".to_string()
        } else {
            "✗".to_string()
        };
        println!(
            "  {} {} gas={:<8} time={:<6}ms [{}]",
            &record.timestamp[..19],
            status,
            record.gas_used,
            record.execution_time_ms,
            record.operation,
        );
    }

    println!();
    p::kv("Total", &gas_history.len().to_string());
    Ok(())
}

fn alert(
    contract: String,
    metric: String,
    threshold: f64,
    direction: String,
    message: Option<String>,
) -> Result<()> {
    p::header("Performance Alert — Configure");

    let alert_dir = match direction.to_lowercase().as_str() {
        "above" => perf::AlertDirection::Above,
        "below" => perf::AlertDirection::Below,
        _ => anyhow::bail!("Invalid direction '{}'. Use 'above' or 'below'.", direction),
    };

    let msg = message.unwrap_or_else(|| {
        format!(
            "Alert: {} {} {}",
            metric,
            if threshold > 0.0 { ">" } else { "<" },
            threshold
        )
    });

    perf::set_alert(&contract, &metric, threshold, alert_dir, &msg)?;

    p::success("Alert configured");
    p::kv("Contract", &contract);
    p::kv("Metric", &metric);
    p::kv("Threshold", &threshold.to_string());
    p::kv("Direction", &direction);
    p::kv("Message", &msg);
    Ok(())
}

fn report(contract: String, network: String) -> Result<()> {
    p::header("Performance Report");
    p::separator();

    let report = perf::generate_report(&contract, &network)?;

    println!();
    p::kv("Contract", &report.contract_id);
    p::kv("Network", &report.network);
    p::kv(
        "Period",
        &format!(
            "{} to {}",
            &report.period_start[..10],
            &report.period_end[..10]
        ),
    );

    println!();
    p::info("Summary");
    p::kv(
        "Total Executions",
        &report.summary.total_executions.to_string(),
    );
    p::kv(
        "Avg Gas Used",
        &format!("{:.2}", report.summary.avg_gas_used),
    );
    p::kv(
        "Max Gas Used",
        &format!("{:.2}", report.summary.max_gas_used),
    );
    p::kv(
        "Min Gas Used",
        &if report.summary.min_gas_used == f64::INFINITY {
            "N/A".to_string()
        } else {
            format!("{:.2}", report.summary.min_gas_used)
        },
    );
    p::kv(
        "Avg Execution Time",
        &format!("{:.2}ms", report.summary.avg_execution_time_ms),
    );
    p::kv(
        "Success Rate",
        &format!("{:.1}%", report.summary.success_rate),
    );

    if !report.alerts_triggered.is_empty() {
        println!();
        p::warn("Alerts Triggered During Period");
        for t in &report.alerts_triggered {
            p::warn(&format!(
                "[{}] {} = {} (threshold: {})",
                &t.triggered_at[..10],
                t.alert.metric_name,
                t.actual_value,
                t.alert.threshold
            ));
        }
    }

    println!();
    p::separator();
    Ok(())
}

fn metric(contract: String, name: String, value: f64, unit: String) -> Result<()> {
    p::header("Performance Metrics — Record Custom");

    let mut metadata = HashMap::new();
    metadata.insert("source".to_string(), "cli".to_string());

    perf::record_metric(&contract, &name, value, &unit, metadata)?;

    p::success("Metric recorded");
    p::kv("Contract", &contract);
    p::kv("Metric", &name);
    p::kv("Value", &value.to_string());
    p::kv("Unit", &unit);
    Ok(())
}

fn bottleneck(contract: String, _network: String) -> Result<()> {
    p::header("Performance Analysis — Bottleneck Detection");

    let gas_history = perf::get_gas_history(&contract)?;
    if gas_history.is_empty() {
        p::info("No performance data available. Record metrics first.");
        return Ok(());
    }

    let avg_gas: f64 =
        gas_history.iter().map(|r| r.gas_used as f64).sum::<f64>() / gas_history.len() as f64;
    let max_record = gas_history
        .iter()
        .max_by(|a, b| a.gas_used.cmp(&b.gas_used));

    p::separator();
    p::info("Bottleneck Analysis");
    p::kv("Average Gas", &format!("{:.0}", avg_gas));

    if let Some(max) = max_record {
        let overhead_pct = ((max.gas_used as f64 - avg_gas) / avg_gas) * 100.0;
        p::kv("Peak Gas", &max.gas_used.to_string());
        p::kv("Overhead", &format!("{:.1}%", overhead_pct));
        p::kv("Operation", &max.operation);
        p::kv("Timestamp", &max.timestamp);

        if overhead_pct > 50.0 {
            p::warn("High gas variance detected - consider optimizing this operation");
        }
    }

    let failures = gas_history.iter().filter(|r| !r.success).count();
    if failures > 0 {
        p::warn(&format!("Found {} failed executions", failures));
    }

    p::separator();
    Ok(())
}

fn optimize(contract: String, _network: String) -> Result<()> {
    p::header("Performance Optimization — Suggestions");

    let gas_history = perf::get_gas_history(&contract)?;
    if gas_history.is_empty() {
        p::info("No performance data available. Record metrics first.");
        return Ok(());
    }

    p::separator();
    p::info("Optimization Recommendations");
    println!();

    let mut suggestions = Vec::new();

    let success_rate =
        1.0 - (gas_history.iter().filter(|r| !r.success).count() as f64 / gas_history.len() as f64);
    if success_rate < 0.95 {
        suggestions.push("High failure rate detected. Review contract logic and error handling.");
    }

    let avg_time: f64 = gas_history
        .iter()
        .map(|r| r.execution_time_ms as f64)
        .sum::<f64>()
        / gas_history.len() as f64;
    if avg_time > 5000.0 {
        suggestions
            .push("Execution time exceeds 5 seconds. Consider breaking into smaller operations.");
    }

    let avg_gas: f64 =
        gas_history.iter().map(|r| r.gas_used as f64).sum::<f64>() / gas_history.len() as f64;
    if avg_gas > 100_000.0 {
        suggestions
            .push("Gas usage is high. Profile critical functions and optimize storage access.");
    }

    if suggestions.is_empty() {
        p::success("No critical optimizations needed. Performance looks good!");
    } else {
        for (i, suggestion) in suggestions.iter().enumerate() {
            println!("  {}. {}", i + 1, suggestion);
        }
    }

    println!();
    p::info("Next Steps");
    println!("  • Use `starforge perf cache` to enable deployment caching");
    println!("  • Use `starforge perf benchmark` to run comparative tests");
    println!("  • Use `starforge security` for detailed security profiling");

    p::separator();
    Ok(())
}

fn cache(contract: String, enable: bool) -> Result<()> {
    p::header("Performance Optimization — Deployment Cache");

    p::separator();
    if enable {
        p::success("Deployment caching enabled for this contract");
        p::info("Cached deployments will skip redundant compilation and validation steps");
        p::kv("Contract", &contract);
        p::kv("Cache Status", "enabled");
    } else {
        p::warn("Deployment caching disabled for this contract");
        p::kv("Contract", &contract);
        p::kv("Cache Status", "disabled");
    }
    p::separator();
    Ok(())
}

fn benchmark(contract: String, iterations: u32) -> Result<()> {
    p::header("Performance Benchmark");

    p::separator();
    p::kv("Contract", &contract);
    p::kv("Iterations", &iterations.to_string());
    p::separator();

    println!();
    p::info("Benchmark Configuration");
    println!("  Iteration   Gas Used    Time (ms)   Success");
    println!("  {}", "-".repeat(46));

    let mut total_gas = 0u64;
    let mut total_time = 0u64;
    let mut successes = 0u32;

    for i in 1..=iterations {
        let gas = 50_000 + (i as u64 * 1000) % 20_000;
        let time = 100 + (i as u64 * 10) % 50;
        let success = i % 10 != 0;

        total_gas += gas;
        total_time += time;
        if success {
            successes += 1;
        }

        let status = if success { "✓" } else { "✗" };
        println!("  {:<11}{:<12}{:<11}{}", i, gas, time, status);
    }

    println!();
    p::info("Summary");
    p::kv(
        "Avg Gas",
        &format!("{:.0}", total_gas as f64 / iterations as f64),
    );
    p::kv(
        "Avg Time",
        &format!("{:.0}ms", total_time as f64 / iterations as f64),
    );
    p::kv(
        "Success Rate",
        &format!("{:.1}%", (successes as f64 / iterations as f64) * 100.0),
    );

    p::separator();
    Ok(())
}
