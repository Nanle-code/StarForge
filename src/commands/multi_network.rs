//! Multi-network deployment commands.
//!
//! Provides CLI commands for AI-driven multi-network deployment support.

use crate::utils::{
    multi_network_deploy::{DeploymentStrategy, MultiNetworkDeployer},
    print as p,
};
use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum MultiNetworkCommands {
    /// Deploy contract to multiple networks
    Deploy {
        /// Path to the WASM file to deploy
        #[arg(long, value_name = "FILE")]
        wasm: PathBuf,

        /// Target networks (comma-separated)
        #[arg(long, default_value = "testnet")]
        networks: String,

        /// Deployment strategy (parallel, sequential, testnet_first)
        #[arg(long, default_value = "testnet_first")]
        strategy: String,

        /// Enable cost optimization
        #[arg(long, default_value = "true")]
        cost_optimization: bool,

        /// Enable risk assessment
        #[arg(long, default_value = "true")]
        risk_assessment: bool,

        /// Output results as JSON
        #[arg(long)]
        json: bool,
    },

    /// Compare networks for deployment
    Compare {
        /// Include custom networks in comparison
        #[arg(long)]
        include_custom: bool,
    },

    /// Add custom network configuration
    AddNetwork {
        /// Network name
        #[arg(long)]
        name: String,

        /// Horizon URL
        #[arg(long)]
        horizon_url: String,

        /// Soroban RPC URL
        #[arg(long)]
        soroban_rpc_url: String,

        /// Network passphrase
        #[arg(long)]
        network_passphrase: String,
    },

    /// List configured networks
    ListNetworks,

    /// Switch active network
    Switch {
        /// Target network name
        network: String,
    },

    /// Show synchronization status
    SyncStatus {
        /// Deployment ID to check
        #[arg(long)]
        deployment_id: Option<String>,
    },
}

pub async fn handle(cmd: MultiNetworkCommands) -> Result<()> {
    match cmd {
        MultiNetworkCommands::Deploy {
            wasm,
            networks,
            strategy,
            cost_optimization,
            risk_assessment,
            json,
        } => {
            handle_deploy(
                wasm,
                networks,
                strategy,
                cost_optimization,
                risk_assessment,
                json,
            )
            .await
        }
        MultiNetworkCommands::Compare { include_custom } => handle_compare(include_custom),
        MultiNetworkCommands::AddNetwork {
            name,
            horizon_url,
            soroban_rpc_url,
            network_passphrase,
        } => handle_add_network(name, horizon_url, soroban_rpc_url, network_passphrase),
        MultiNetworkCommands::ListNetworks => handle_list_networks(),
        MultiNetworkCommands::Switch { network } => handle_switch(network),
        MultiNetworkCommands::SyncStatus { deployment_id } => handle_sync_status(deployment_id),
    }
}

async fn handle_deploy(
    wasm: PathBuf,
    networks: String,
    strategy: String,
    cost_optimization: bool,
    risk_assessment: bool,
    json: bool,
) -> Result<()> {
    p::header("Multi-Network Deployment");
    p::separator();

    let deployment_strategy = match strategy.as_str() {
        "parallel" => DeploymentStrategy::Parallel,
        "sequential" => DeploymentStrategy::Sequential,
        "testnet_first" => DeploymentStrategy::TestnetFirst,
        _ => {
            p::warn(&format!(
                "Unknown strategy '{}', using 'testnet_first'",
                strategy
            ));
            DeploymentStrategy::TestnetFirst
        }
    };

    let mut config = MultiNetworkDeployer::create_default_config();
    config.deployment_strategy = deployment_strategy.clone();
    config.cost_optimization_enabled = cost_optimization;
    config.risk_assessment_enabled = risk_assessment;

    let target_networks: Vec<String> = networks.split(',').map(|s| s.trim().to_string()).collect();

    p::kv("WASM File", &wasm.display().to_string());
    p::kv("Target Networks", &target_networks.join(", "));
    p::kv("Deployment Strategy", &format!("{:?}", deployment_strategy));
    p::kv(
        "Cost Optimization",
        if cost_optimization {
            "enabled"
        } else {
            "disabled"
        },
    );
    p::kv(
        "Risk Assessment",
        if risk_assessment {
            "enabled"
        } else {
            "disabled"
        },
    );
    println!();

    let spinner = p::spinner("Deploying to multiple networks...");
    let result = MultiNetworkDeployer::deploy_to_networks(
        &config,
        wasm.to_string_lossy().as_ref(),
        target_networks,
    )
    .await?;
    spinner.finish_and_clear();

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        print_deployment_result(&result);
    }

    p::separator();
    Ok(())
}

fn print_deployment_result(
    result: &crate::utils::multi_network_deploy::MultiNetworkDeploymentResult,
) {
    p::kv("Deployment ID", &result.deployment_id);
    p::kv("Timestamp", &result.timestamp);
    p::kv("Strategy", &result.strategy);
    println!();

    // Network results
    p::header("Network Deployment Results");
    for (network_name, network_result) in &result.network_results {
        let status_color = match network_result.status {
            crate::utils::multi_network_deploy::DeploymentStatus::Success => "green",
            crate::utils::multi_network_deploy::DeploymentStatus::Failed => "red",
            _ => "yellow",
        };

        println!();
        println!("  Network: {}", network_name);
        println!(
            "  Status: {}",
            format!("{:?}", network_result.status)
                .as_str()
                .color(status_color)
        );
        if let Some(contract_id) = &network_result.contract_id {
            println!("  Contract ID: {}", contract_id);
        }
        if let Some(tx_hash) = &network_result.transaction_hash {
            println!("  Transaction Hash: {}", tx_hash);
        }
        println!("  Gas Used: {} stroops", network_result.gas_used);
        println!("  Cost: ${:.6}", network_result.cost_usd);
        println!(
            "  Deployment Time: {} ms",
            network_result.deployment_time_ms
        );
        if let Some(error) = &network_result.error_message {
            println!("  Error: {}", error);
        }
    }
    println!();

    // Cost summary
    p::header("Cost Summary");
    p::kv(
        "Total Cost",
        &format!("${:.6}", result.cost_summary.total_cost_usd),
    );
    p::kv(
        "Cost Savings",
        &format!("{:.1}%", result.cost_summary.cost_savings_percentage),
    );
    p::kv(
        "Most Cost-Effective",
        &result.cost_summary.most_cost_effective_network,
    );
    println!();
    p::info("Cost by Network:");
    for (network, cost) in &result.cost_summary.cost_by_network {
        println!("  {}: ${:.6}", network, cost);
    }
    println!();

    // Risk assessment
    p::header("Risk Assessment");
    p::kv(
        "Overall Risk Level",
        &result.risk_assessment.overall_risk_level,
    );
    p::kv(
        "Approved for Deployment",
        if result.risk_assessment.approved_for_deployment {
            "yes"
        } else {
            "no"
        },
    );

    if !result.risk_assessment.risk_factors.is_empty() {
        println!();
        p::info("Risk Factors:");
        for factor in &result.risk_assessment.risk_factors {
            println!("  [{}] {}", factor.severity.to_uppercase(), factor.factor);
            println!("    Description: {}", factor.description);
            println!("    Mitigation: {}", factor.mitigation);
        }
    }

    if !result.risk_assessment.recommendations.is_empty() {
        println!();
        p::info("Recommendations:");
        for rec in &result.risk_assessment.recommendations {
            println!("  • {}", rec);
        }
    }
    println!();

    // Synchronization status
    p::header("Synchronization Status");
    p::kv(
        "Synchronized",
        if result.synchronization_status.synchronized {
            "yes"
        } else {
            "no"
        },
    );
    p::kv(
        "Last Sync",
        &result.synchronization_status.last_sync_timestamp,
    );

    if !result
        .synchronization_status
        .synchronized_networks
        .is_empty()
    {
        println!();
        p::info("Synchronized Networks:");
        for net in &result.synchronization_status.synchronized_networks {
            println!("  ✓ {}", net);
        }
    }

    if !result.synchronization_status.failed_networks.is_empty() {
        println!();
        p::warn("Failed Networks:");
        for net in &result.synchronization_status.failed_networks {
            println!("  ✗ {}", net);
        }
    }
    println!();
}

fn handle_compare(_include_custom: bool) -> Result<()> {
    p::header("Network Comparison");
    p::separator();

    let config = MultiNetworkDeployer::create_default_config();
    let comparison = MultiNetworkDeployer::compare_networks(&config);

    p::kv(
        "Recommended Network",
        &comparison.recommended_for_deployment,
    );
    println!();

    p::header("Network Scores");
    for entry in &comparison.networks {
        println!();
        println!("  Network: {}", entry.network_name);
        println!("  Cost Score: {:.1}/100", entry.cost_score);
        println!("  Speed Score: {:.1}/100", entry.speed_score);
        println!("  Reliability Score: {:.1}/100", entry.reliability_score);
        println!("  Overall Score: {:.1}/100", entry.overall_score);

        println!("  Pros:");
        for pro in &entry.pros {
            println!("    • {}", pro);
        }

        println!("  Cons:");
        for con in &entry.cons {
            println!("    • {}", con);
        }
    }

    p::separator();
    Ok(())
}

fn handle_add_network(
    name: String,
    horizon_url: String,
    soroban_rpc_url: String,
    network_passphrase: String,
) -> Result<()> {
    p::header("Add Custom Network");
    p::separator();

    let mut config = MultiNetworkDeployer::create_default_config();
    MultiNetworkDeployer::add_custom_network(
        &mut config,
        name.clone(),
        horizon_url.clone(),
        soroban_rpc_url.clone(),
        network_passphrase.clone(),
    )?;

    p::success(&format!("Network '{}' added successfully", name));
    p::kv("Horizon URL", &horizon_url);
    p::kv("Soroban RPC URL", &soroban_rpc_url);
    p::kv("Network Passphrase", &network_passphrase);

    p::separator();
    Ok(())
}

fn handle_list_networks() -> Result<()> {
    p::header("Configured Networks");
    p::separator();

    let config = MultiNetworkDeployer::create_default_config();

    for (name, net_config) in &config.networks {
        println!();
        println!("  Network: {}", name);
        println!("  Type: {}", net_config.network_type);
        println!("  Horizon URL: {}", net_config.horizon_url);
        println!("  Soroban RPC URL: {}", net_config.soroban_rpc_url);
        println!("  Gas Price: {} stroops", net_config.gas_price);
        println!("  Reliability Score: {:.2}", net_config.reliability_score);
        println!(
            "  Estimated Cost per TX: ${:.6}",
            net_config.estimated_cost_per_tx
        );
        println!(
            "  Confirmation Time: {:.1} seconds",
            net_config.confirmation_time_seconds
        );
    }

    p::separator();
    Ok(())
}

fn handle_switch(network: String) -> Result<()> {
    p::header("Switch Network");
    p::separator();

    let config = MultiNetworkDeployer::create_default_config();
    MultiNetworkDeployer::switch_network(&config, &network)?;

    p::success(&format!("Switched to network '{}'", network));

    p::separator();
    Ok(())
}

fn handle_sync_status(deployment_id: Option<String>) -> Result<()> {
    p::header("Synchronization Status");
    p::separator();

    if let Some(id) = deployment_id {
        p::kv("Deployment ID", &id);
        p::info("Synchronization status lookup coming soon");
    } else {
        p::info("Provide deployment ID with --deployment-id to check status");
    }

    p::separator();
    Ok(())
}
