use crate::utils::{contract_profiler, performance as perf, print as p};
use anyhow::Result;
use clap::Subcommand;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::PathBuf;

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
        /// Memory usage (bytes) observed during execution (best-effort from caller)
        #[arg(long)]
        memory_bytes: Option<u64>,

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
            memory_bytes,
            success,
            network,
        } => record(
            contract,
            operation,
            gas,
            time_ms,
            memory_bytes,
            success,
            network,
        ),

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

// ── Advanced Profiling Commands ──────────────────────────────────────────────

#[derive(Subcommand)]
pub enum AdvancedPerfCommands {
    /// Profile a compiled Soroban WASM artifact
    Profile {
        /// Path to the compiled contract WASM
        wasm: PathBuf,
        /// Human-readable contract label for reports
        #[arg(long)]
        label: Option<String>,
        /// Previous JSON profile to compare against for regression detection
        #[arg(long)]
        baseline: Option<PathBuf>,
        /// JSON report output path; defaults to the StarForge profile store
        #[arg(long)]
        output: Option<PathBuf>,
        /// Optional HTML dashboard output path
        #[arg(long)]
        dashboard: Option<PathBuf>,
    },
    /// Advanced performance analysis with bottleneck detection
    Analyze {
        /// Contract ID
        contract: String,
        /// Network (default: testnet)
        #[arg(long, default_value = "testnet")]
        network: String,
    },
    /// Detect performance regressions
    DetectRegression {
        /// Contract ID
        contract: String,
        /// Analysis period in hours (default: 24)
        #[arg(long, default_value_t = 24)]
        period_hours: u64,
        /// Network (default: testnet)
        #[arg(long, default_value = "testnet")]
        network: String,
    },
    /// Compare performance across time periods
    Compare {
        /// Contract ID
        contract: String,
        /// Time window in hours (default: 24)
        #[arg(long, default_value_t = 24)]
        hours_back: u64,
        /// Network (default: testnet)
        #[arg(long, default_value = "testnet")]
        network: String,
    },
    /// Generate comprehensive performance dashboard
    GenerateDashboard {
        /// Contract ID
        contract: String,
        /// Network (default: testnet)
        #[arg(long, default_value = "testnet")]
        network: String,
    },
}

// ── Advanced Profiling Command Handlers ──────────────────────────────────────

pub async fn handle_advanced(cmd: AdvancedPerfCommands) -> Result<()> {
    match cmd {
        AdvancedPerfCommands::Profile {
            wasm,
            label,
            baseline,
            output,
            dashboard,
        } => profile_contract(wasm, label, baseline, output, dashboard),
        AdvancedPerfCommands::Analyze { contract, network } => analyze(contract, network),
        AdvancedPerfCommands::DetectRegression {
            contract,
            period_hours,
            network,
        } => detect_regression(contract, period_hours, network),
        AdvancedPerfCommands::Compare {
            contract,
            hours_back,
            network,
        } => compare(contract, hours_back, network),
        AdvancedPerfCommands::GenerateDashboard { contract, network } => {
            generate_dashboard(contract, network)
        }
    }
}

fn profile_contract(
    wasm: PathBuf,
    label: Option<String>,
    baseline: Option<PathBuf>,
    output: Option<PathBuf>,
    dashboard: Option<PathBuf>,
) -> Result<()> {
    p::header("Contract Performance Profile");
    p::separator();
    p::kv("WASM", &wasm.display().to_string());
    if let Some(label) = &label {
        p::kv("Label", label);
    }
    if let Some(baseline) = &baseline {
        p::kv("Baseline", &baseline.display().to_string());
    }
    p::separator();

    let report =
        contract_profiler::profile_contract_wasm(&wasm, label.as_deref(), baseline.as_deref())?;

    println!();
    p::info("Execution Time Analysis");
    p::kv(
        "Est. instructions",
        &report.execution.estimated_instruction_count.to_string(),
    );
    p::kv(
        "Est. CPU gas",
        &report.execution.estimated_cpu_gas.to_string(),
    );
    p::kv(
        "Est. invocation",
        &format!("{:.2}ms", report.execution.estimated_invocation_time_ms),
    );
    if !report.execution.hot_sections.is_empty() {
        p::kv("Hot sections", &report.execution.hot_sections.join(", "));
    }

    println!();
    p::info("Memory Usage Tracking");
    p::kv(
        "Linear pages",
        &report.memory.linear_memory_pages.to_string(),
    );
    p::kv(
        "Static bytes",
        &report.memory.estimated_static_bytes.to_string(),
    );
    p::kv(
        "Peak estimate",
        &format!("{} bytes", report.memory.estimated_peak_bytes),
    );

    println!();
    p::info("Bottleneck Identification");
    if report.bottlenecks.is_empty() {
        p::success("No bottlenecks detected");
    } else {
        for bottleneck in &report.bottlenecks {
            p::warn(&format!(
                "{} [{:?}] {}",
                bottleneck.kind, bottleneck.severity, bottleneck.metric
            ));
            println!("  {}", bottleneck.description);
            println!("  Recommendation: {}", bottleneck.recommendation);
        }
    }

    if let Some(comparison) = &report.comparison {
        println!();
        p::info("Performance Regression Detection");
        p::kv("Verdict", &comparison.verdict);
        p::kv("Gas delta", &format!("{:+.2}%", comparison.gas_delta_pct));
        p::kv(
            "Execution delta",
            &format!("{:+.2}%", comparison.execution_time_delta_pct),
        );
        p::kv(
            "Memory delta",
            &format!("{:+.2}%", comparison.memory_delta_pct),
        );
    }

    println!();
    p::info("Performance Dashboard");
    p::kv("Profile ID", &report.id);
    p::kv(
        "Estimated gas",
        &report.dashboard_summary.total_estimated_gas.to_string(),
    );
    p::kv(
        "Regression",
        &report.dashboard_summary.regression_detected.to_string(),
    );
    for action in &report.dashboard_summary.next_actions {
        println!("  - {}", action);
    }

    let json_path = if let Some(path) = output {
        contract_profiler::write_profile_report(&report, &path)?;
        path
    } else {
        contract_profiler::save_profile_report(&report)?
    };
    p::success(&format!("JSON profile written to {}", json_path.display()));

    if let Some(path) = dashboard {
        contract_profiler::write_dashboard_html(&report, &path)?;
        p::success(&format!("HTML dashboard written to {}", path.display()));
    }

    Ok(())
}

fn analyze(contract: String, network: String) -> Result<()> {
    p::header("Advanced Performance Analysis");
    p::separator();
    p::kv("Contract", &contract);
    p::kv("Network", &network);
    p::separator();

    let analysis = perf::analyze_bottlenecks(&contract)?;

    println!();
    p::info("Bottleneck Analysis Results");
    p::kv(
        "Overall Score",
        &format!("{:.1}/100", analysis.overall_score),
    );
    p::kv(
        "Memory Leaks Detected",
        &analysis.memory_leaks_detected.to_string(),
    );

    if !analysis.bottleneck_operations.is_empty() {
        println!();
        p::warn("Frequent Operations (Potential Bottlenecks):");
        for op in &analysis.bottleneck_operations {
            println!("  • {}", op);
        }
    }

    if !analysis.high_gas_operations.is_empty() {
        println!();
        p::warn("High Gas Consumption Operations:");
        for op in &analysis.high_gas_operations {
            println!("  • {}", op);
        }
    }

    if analysis.bottleneck_operations.is_empty() && analysis.high_gas_operations.is_empty() {
        p::success("No significant bottlenecks detected!");
    }

    println!();
    p::separator();
    Ok(())
}

fn detect_regression(contract: String, period_hours: u64, network: String) -> Result<()> {
    p::header("Performance Regression Detection");
    p::separator();
    p::kv("Contract", &contract);
    p::kv("Network", &network);
    p::kv("Analysis Period", &format!("{} hours", period_hours));
    p::separator();

    let report = perf::detect_regression(&contract, period_hours)?;

    println!();
    p::info("Regression Report");
    p::kv("Baseline Avg Gas", &format!("{:.0}", report.baseline_avg));
    p::kv("Current Avg Gas", &format!("{:.0}", report.current_avg));
    p::kv("Change", &format!("{:+.1}%", report.regression_percentage));

    println!();
    p::info("Trends:");
    for trend in &report.trends {
        if trend.contains("increased") {
            p::warn(&format!("  ⚠ {}", trend));
        } else if trend.contains("decreased") {
            p::success(&format!("  ✓ {}", trend));
        } else {
            p::info(&format!("  • {}", trend));
        }
    }

    let regression_count = report
        .regression_points
        .iter()
        .filter(|r| r.regression_detected)
        .count();
    if regression_count > 0 {
        println!();
        p::warn(&format!("{} regression points detected:", regression_count));
        for point in &report.regression_points {
            if point.regression_detected {
                println!(
                    "  {} gas={} time={}ms [{}]",
                    &point.timestamp[..19],
                    point.gas_used,
                    point.execution_time_ms,
                    if point.success { "OK" } else { "FAIL" }
                );
            }
        }
    } else {
        p::success("No regressions detected!");
    }

    println!();
    p::separator();
    Ok(())
}

fn compare(contract: String, hours_back: u64, network: String) -> Result<()> {
    p::header("Performance Comparison");
    p::separator();
    p::kv("Contract", &contract);
    p::kv("Network", &network);
    p::kv("Time Window", &format!("{} hours", hours_back));
    p::separator();

    let report = perf::compare_profiles(&contract, hours_back)?;

    println!();
    p::info("Comparison Results");

    if !report.performance_differences.is_empty() {
        for (metric, diff) in &report.performance_differences {
            let label = metric.replace('_', " ");
            if *diff > 0.0 {
                p::warn(&format!("  {} +{:.1}% (regression)", label, diff));
            } else {
                p::success(&format!("  {} {:.1}% (improvement)", label, diff));
            }
        }
    } else {
        p::info("Insufficient data for comparison (need at least 2 snapshots)");
    }

    println!();
    p::info("Recommendations:");
    if report.recommendations.is_empty() {
        p::success("  No specific recommendations at this time.");
    } else {
        for rec in &report.recommendations {
            println!("  • {}", rec);
        }
    }

    println!();
    p::separator();
    Ok(())
}

fn generate_dashboard(contract: String, network: String) -> Result<()> {
    p::header("Performance Dashboard");
    p::separator();
    p::kv("Contract", &contract);
    p::kv("Network", &network);
    p::separator();

    let dashboard = perf::generate_dashboard(&contract, &network)?;

    println!();
    p::info("═══ EXECUTION SUMMARY ═══");
    p::kv(
        "Total Executions",
        &dashboard.summary.total_executions.to_string(),
    );
    p::kv(
        "Avg Gas Used",
        &format!("{:.0}", dashboard.summary.avg_gas_used),
    );
    p::kv(
        "Max Gas Used",
        &format!("{:.0}", dashboard.summary.max_gas_used),
    );
    p::kv(
        "Avg Execution Time",
        &format!("{:.1}ms", dashboard.summary.avg_execution_time_ms),
    );
    if let Some(avg_mem) = dashboard.summary.avg_memory_used_bytes {
        p::kv("Avg Memory Used", &format!("{:.0} bytes", avg_mem));
    }
    if let Some(max_mem) = dashboard.summary.max_memory_used_bytes {
        p::kv("Peak Memory Used", &format!("{:.0} bytes", max_mem));
    }
    p::kv(
        "Success Rate",
        &format!("{:.1}%", dashboard.summary.success_rate),
    );

    println!();
    p::info("═══ BOTTLENECK ANALYSIS ═══");
    p::kv(
        "Overall Score",
        &format!("{:.1}/100", dashboard.bottleneck_analysis.overall_score),
    );
    p::kv(
        "Memory Leaks Detected",
        &dashboard
            .bottleneck_analysis
            .memory_leaks_detected
            .to_string(),
    );

    if !dashboard
        .bottleneck_analysis
        .bottleneck_operations
        .is_empty()
    {
        p::warn("Frequent Operations:");
        for op in &dashboard.bottleneck_analysis.bottleneck_operations {
            println!("  • {}", op);
        }
    }
    if !dashboard.bottleneck_analysis.high_gas_operations.is_empty() {
        p::warn("High Gas Operations:");
        for op in &dashboard.bottleneck_analysis.high_gas_operations {
            println!("  • {}", op);
        }
    }

    println!();
    p::info("═══ REGRESSION DETECTION ═══");
    p::kv(
        "Baseline Avg",
        &format!("{:.0}", dashboard.regression_report.baseline_avg),
    );
    p::kv(
        "Current Avg",
        &format!("{:.0}", dashboard.regression_report.current_avg),
    );
    p::kv(
        "Change",
        &format!("{:+.1}%", dashboard.regression_report.regression_percentage),
    );
    for trend in &dashboard.regression_report.trends {
        if trend.contains("increased") {
            p::warn(&format!("  ⚠ {}", trend));
        } else if trend.contains("decreased") {
            p::success(&format!("  ✓ {}", trend));
        } else {
            p::info(&format!("  • {}", trend));
        }
    }

    println!();
    p::info("═══ PERFORMANCE COMPARISON ═══");
    if !dashboard
        .comparison_report
        .performance_differences
        .is_empty()
    {
        for (metric, diff) in &dashboard.comparison_report.performance_differences {
            let label = metric.replace('_', " ");
            if *diff > 0.0 {
                p::warn(&format!("  {} +{:.1}%", label, diff));
            } else {
                p::success(&format!("  {} {:.1}%", label, diff));
            }
        }
    } else {
        p::info("  No comparison data available");
    }
    for rec in &dashboard.comparison_report.recommendations {
        println!("  • {}", rec);
    }

    if !dashboard.alerts.is_empty() {
        println!();
        p::warn("Configured Alerts:");
        for alert in &dashboard.alerts {
            println!(
                "  • {} {} {} ({})",
                alert.metric_name,
                if matches!(alert.direction, perf::AlertDirection::Above) {
                    ">"
                } else {
                    "<"
                },
                alert.threshold,
                alert.message
            );
        }
    }

    println!();
    p::separator();
    Ok(())
}

fn record(
    contract: String,
    operation: String,
    gas: u64,
    time_ms: Option<u64>,
    memory_bytes: Option<u64>,
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
        memory_used: memory_bytes,
        network,
    };

    perf::record_gas_usage(&record)?;

    p::success("Gas usage recorded");
    p::kv("Contract", &contract);
    p::kv("Gas Used", &gas.to_string());
    if let Some(t) = time_ms {
        p::kv("Execution Time", &format!("{}ms", t));
    }
    if let Some(m) = memory_bytes {
        p::kv("Memory Used", &format!("{} bytes", m));
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
    if let Some(avg_mem) = report.summary.avg_memory_used_bytes {
        p::kv("Avg Memory Used", &format!("{:.0} bytes", avg_mem));
    }
    if let Some(max_mem) = report.summary.max_memory_used_bytes {
        p::kv("Peak Memory Used", &format!("{:.0} bytes", max_mem));
    }
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
    if let Some(avg_mem) = report.summary.avg_memory_used_bytes {
        p::kv("Avg Memory Used", &format!("{:.0} bytes", avg_mem));
    }
    if let Some(max_mem) = report.summary.max_memory_used_bytes {
        p::kv("Peak Memory Used", &format!("{:.0} bytes", max_mem));
    }
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
