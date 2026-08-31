//! Deployment optimization commands.
//!
//! Provides CLI commands for AI-powered deployment optimization.

use crate::utils::{
    deployment_optimizer::{optimize_deployment, DeploymentOptimizerConfig, OptimizationLevel},
    print as p,
};
use anyhow::Result;
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum DeploymentOptimizeCommands {
    /// Analyze and optimize deployment for cost, speed, and reliability
    Analyze {
        /// Path to the WASM file to optimize
        #[arg(long, value_name = "FILE")]
        wasm: PathBuf,

        /// Target networks to analyze (comma-separated)
        #[arg(long, default_value = "testnet,mainnet")]
        networks: String,

        /// Optimization level (basic, standard, aggressive)
        #[arg(long, default_value = "standard")]
        level: String,

        /// Enable cost optimization
        #[arg(long, default_value = "true")]
        cost: bool,

        /// Enable speed optimization
        #[arg(long, default_value = "true")]
        speed: bool,

        /// Enable reliability optimization
        #[arg(long, default_value = "true")]
        reliability: bool,

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Show optimization history
    History {
        /// Number of recent optimizations to show
        #[arg(long, default_value = "10")]
        limit: usize,
    },

    /// Compare optimization results
    Compare {
        /// Original deployment ID
        #[arg(long)]
        original: String,

        /// Optimized deployment ID
        #[arg(long)]
        optimized: String,
    },
}

pub async fn handle(cmd: DeploymentOptimizeCommands) -> Result<()> {
    match cmd {
        DeploymentOptimizeCommands::Analyze {
            wasm,
            networks,
            level,
            cost,
            speed,
            reliability,
            json,
        } => handle_analyze(wasm, networks, level, cost, speed, reliability, json).await,
        DeploymentOptimizeCommands::History { limit } => handle_history(limit),
        DeploymentOptimizeCommands::Compare {
            original,
            optimized,
        } => handle_compare(original, optimized),
    }
}

async fn handle_analyze(
    wasm: PathBuf,
    networks: String,
    level: String,
    cost: bool,
    speed: bool,
    reliability: bool,
    json: bool,
) -> Result<()> {
    p::header("Deployment Optimization Analysis");
    p::separator();

    let optimization_level = match level.as_str() {
        "basic" => OptimizationLevel::Basic,
        "standard" => OptimizationLevel::Standard,
        "aggressive" => OptimizationLevel::Aggressive,
        _ => {
            p::warn(&format!(
                "Unknown optimization level '{}', using 'standard'",
                level
            ));
            OptimizationLevel::Standard
        }
    };

    let target_networks: Vec<String> = networks.split(',').map(|s| s.trim().to_string()).collect();

    let config = DeploymentOptimizerConfig {
        wasm_path: wasm.to_string_lossy().to_string(),
        target_networks,
        optimization_level,
        enable_cost_optimization: cost,
        enable_speed_optimization: speed,
        enable_reliability_optimization: reliability,
    };

    p::kv("WASM File", &wasm.display().to_string());
    p::kv("Optimization Level", &optimization_level.to_string());
    p::kv(
        "Cost Optimization",
        if cost { "enabled" } else { "disabled" },
    );
    p::kv(
        "Speed Optimization",
        if speed { "enabled" } else { "disabled" },
    );
    p::kv(
        "Reliability Optimization",
        if reliability { "enabled" } else { "disabled" },
    );
    println!();

    let spinner = p::spinner("Analyzing deployment optimization opportunities...");
    let result = optimize_deployment(&config)?;
    spinner.finish_and_clear();

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        print_optimization_result(&result);
    }

    p::separator();
    Ok(())
}

fn print_optimization_result(
    result: &crate::utils::deployment_optimizer::DeploymentOptimizationResult,
) {
    // Cost savings
    p::header("Cost Optimization");
    p::kv(
        "Original Gas Cost",
        &format!("{} stroops", result.original_gas_cost),
    );
    p::kv(
        "Optimized Gas Cost",
        &format!("{} stroops", result.optimized_gas_cost),
    );
    p::kv(
        "Gas Savings",
        &format!("{:.1}%", result.gas_savings_percentage),
    );
    println!();

    // Speed improvements
    p::header("Speed Optimization");
    p::kv(
        "Original Deployment Time",
        &format!("{} ms", result.original_deployment_time_ms),
    );
    p::kv(
        "Optimized Deployment Time",
        &format!("{} ms", result.optimized_deployment_time_ms),
    );
    p::kv(
        "Time Improvement",
        &format!("{:.1}%", result.time_improvement_percentage),
    );
    println!();

    // Resource utilization
    p::header("Resource Utilization");
    p::kv(
        "CPU Usage",
        &format!("{:.1}%", result.resource_utilization.cpu_usage_percentage),
    );
    p::kv(
        "Memory Usage",
        &format!("{:.1} MB", result.resource_utilization.memory_usage_mb),
    );
    p::kv(
        "Network Bandwidth",
        &format!(
            "{:.1} Mbps",
            result.resource_utilization.network_bandwidth_mbps
        ),
    );
    p::kv(
        "Storage I/O",
        &format!("{:.1}%", result.resource_utilization.storage_io_percentage),
    );
    p::kv(
        "Optimization Potential",
        &format!("{:.1}%", result.resource_utilization.optimization_potential),
    );
    println!();

    // Network selection
    p::header("Network Selection");
    p::kv(
        "Recommended Network",
        &result.network_selection.recommended_network,
    );
    p::kv(
        "Estimated Cost",
        &format!("${:.6}", result.network_selection.estimated_cost_usd),
    );
    p::kv(
        "Estimated Time",
        &format!(
            "{:.1} seconds",
            result.network_selection.estimated_time_seconds
        ),
    );
    p::kv(
        "Reliability Score",
        &format!("{:.2}", result.network_selection.reliability_score),
    );

    if !result.network_selection.alternatives.is_empty() {
        println!();
        p::info("Alternative Networks:");
        for alt in &result.network_selection.alternatives {
            println!(
                "  - {}: ${:.6} ({:.1}s, reliability: {:.2})",
                alt.network_name,
                alt.estimated_cost_usd,
                alt.estimated_time_seconds,
                alt.reliability_score
            );
            for trade_off in &alt.trade_offs {
                println!("    • {}", trade_off);
            }
        }
    }
    println!();

    // Batch optimization
    p::header("Batch Optimization");
    p::kv(
        "Can Batch",
        if result.batch_optimization.can_batch {
            "yes"
        } else {
            "no"
        },
    );
    if result.batch_optimization.can_batch {
        p::kv(
            "Batch Size",
            &result.batch_optimization.batch_size.to_string(),
        );
        p::kv(
            "Estimated Savings",
            &format!(
                "{:.1}%",
                result.batch_optimization.estimated_savings_percentage
            ),
        );
        println!();
        p::info("Recommended Batch Order:");
        for (i, step) in result
            .batch_optimization
            .recommended_batch_order
            .iter()
            .enumerate()
        {
            println!("  {}. {}", i + 1, step);
        }
    }
    println!();

    // Scheduling optimization
    p::header("Scheduling Optimization");
    p::kv(
        "Optimal Deployment Time",
        &result.scheduling_optimization.optimal_deployment_time,
    );
    p::kv(
        "Estimated Cost Reduction",
        &format!(
            "{:.1}%",
            result.scheduling_optimization.estimated_cost_reduction
        ),
    );
    p::kv(
        "Network Congestion",
        &result
            .scheduling_optimization
            .network_conditions
            .congestion_level,
    );
    p::kv(
        "Gas Price Trend",
        &result
            .scheduling_optimization
            .network_conditions
            .gas_price_trend,
    );
    p::kv(
        "Recommended Action",
        &result
            .scheduling_optimization
            .network_conditions
            .recommended_action,
    );
    println!();

    // Optimization suggestions
    if !result.optimization_suggestions.is_empty() {
        p::header(&format!(
            "Optimization Suggestions ({})",
            result.optimization_suggestions.len()
        ));

        for suggestion in &result.optimization_suggestions {
            println!();
            println!(
                "  [{}] {} - {}",
                suggestion.priority.to_uppercase(),
                suggestion.id,
                suggestion.title
            );
            println!("  Category: {}", suggestion.category);
            println!("  Description: {}", suggestion.description);
            if suggestion.estimated_gas_savings > 0 {
                println!(
                    "  Estimated Gas Savings: {} stroops",
                    suggestion.estimated_gas_savings
                );
            }
            if suggestion.estimated_time_savings_ms > 0 {
                println!(
                    "  Estimated Time Savings: {} ms",
                    suggestion.estimated_time_savings_ms
                );
            }
            println!(
                "  Implementation Effort: {}",
                suggestion.implementation_effort
            );
            if let Some(example) = &suggestion.code_example {
                println!("  Code Example: {}", example);
            }
        }
        println!();
    } else {
        p::success("No optimization suggestions - deployment is already optimized");
        println!();
    }
}

fn handle_history(_limit: usize) -> Result<()> {
    p::header("Deployment Optimization History");
    p::separator();

    p::info("Optimization history feature coming soon");
    p::info("Optimization results are currently stored in-memory only");

    p::separator();
    Ok(())
}

fn handle_compare(original: String, optimized: String) -> Result<()> {
    p::header("Deployment Optimization Comparison");
    p::separator();

    p::kv("Original Deployment", &original);
    p::kv("Optimized Deployment", &optimized);

    p::info("Comparison feature coming soon");
    p::info("This will show detailed metrics comparison between deployments");

    p::separator();
    Ok(())
}
