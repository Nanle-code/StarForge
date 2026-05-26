mod commands;
pub mod plugins;
mod utils;

use clap::{Parser, Subcommand};
use colored::*;

#[derive(Parser)]
#[command(
    name = "starforge",
    about = "âš¡ Stellar & Soroban developer productivity CLI",
    long_about = "starforge is an open-source CLI toolkit for developers building on the Stellar network.\nManage wallets, deploy Soroban contracts, and scaffold new projects â€” all from your terminal.",
    version = "0.1.0"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(long, short = 'q', global = true)]
    quiet: bool,

    #[arg(long, global = true, default_value = "human", value_parser = ["human", "json"])]
    log_format: String,

    #[arg(long, global = true)]
    log_dir: Option<std::path::PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    #[command(subcommand)]
    Wallet(commands::wallet::WalletCommands),
    #[command(subcommand)]
    New(commands::new::NewCommands),
    #[command(subcommand)]
    Contract(commands::contract::ContractCommands),
    #[command(subcommand)]
    Inspect(commands::inspect::InspectCommands),
    Deploy(commands::deploy::DeployArgs),
    Info,
    Tx(commands::tx::TxArgs),
    #[command(subcommand)]
    Network(commands::network::NetworkCommands),
    #[command(subcommand)]
    Completions(commands::completions::CompletionShell),
    Shell(commands::shell::ShellArgs),
    Monitor(commands::monitor::MonitorArgs),
    #[command(subcommand)]
    Tutorial(commands::tutorial::TutorialCommands),
    Benchmark(commands::benchmark::BenchmarkArgs),
    Test(commands::test::TestArgs),
    #[command(subcommand)]
    Gas(commands::gas::GasCommands),
    #[command(subcommand)]
    Plugin(commands::plugin::PluginCommands),
    #[command(subcommand)]
    Template(commands::template::TemplateCommands),
    #[command(subcommand)]
    Upgrade(commands::upgrade::UpgradeCommands),
    #[command(external_subcommand)]
    External(Vec<String>),
}

fn main() {
    let cli = Cli::parse();

    let log_cfg =
        utils::logging::config_from_env(Some(cli.log_format.as_str()), cli.log_dir.clone());
    if let Err(e) = utils::logging::init(log_cfg) {
        eprintln!("Warning: failed to initialise logger: {}", e);
    }

    if !cli.quiet {
        print_banner();
    }

    let command_name = match &cli.command {
        Commands::Wallet(_) => "wallet",
        Commands::New(_) => "new",
        Commands::Contract(_) => "contract",
        Commands::Inspect(_) => "inspect",
        Commands::Deploy(_) => "deploy",
        Commands::Info => "info",
        Commands::Tx(_) => "tx",
        Commands::Network(_) => "network",
        Commands::Completions(_) => "completions",
        Commands::Shell(_) => "shell",
        Commands::Monitor(_) => "monitor",
        Commands::Tutorial(_) => "tutorial",
        Commands::Benchmark(_) => "benchmark",
        Commands::Test(_) => "test",
        Commands::Gas(_) => "gas",
        Commands::Plugin(_) => "plugin",
        Commands::Template(_) => "template",
        Commands::Upgrade(_) => "upgrade",
        Commands::External(_) => "external",
    }
    .to_string();

    let start = std::time::Instant::now();
    let result = match cli.command {
        Commands::Wallet(cmd) => commands::wallet::handle(cmd),
        Commands::New(cmd) => commands::new::handle(cmd),
        Commands::Contract(cmd) => commands::contract::handle(cmd),
        Commands::Inspect(cmd) => commands::inspect::handle(cmd),
        Commands::Deploy(args) => commands::deploy::handle(args),
        Commands::Info => commands::info::handle(),
        Commands::Tx(args) => commands::tx::handle(args),
        Commands::Network(cmd) => commands::network::handle(cmd),
        Commands::Completions(shell) => commands::completions::handle(shell),
        Commands::Shell(args) => commands::shell::handle(args),
        Commands::Monitor(args) => commands::monitor::handle(args),
        Commands::Tutorial(cmd) => commands::tutorial::handle(cmd),
        Commands::Benchmark(args) => commands::benchmark::handle(args),
        Commands::Test(args) => commands::test::handle(args),
        Commands::Gas(args) => commands::gas::handle(args),
        Commands::Plugin(args) => commands::plugin::handle(args),
        Commands::Template(args) => commands::template::handle(args),
        Commands::Upgrade(args) => commands::upgrade::handle(args),
        Commands::External(args) => handle_external_plugin(args),
    };
    let duration = start.elapsed();

    let _ = utils::telemetry::track_event(
        &command_name,
        serde_json::json!({
            "success": result.is_ok(),
            "duration_ms": duration.as_millis(),
        }),
    );

    if let Err(e) = result {
        eprintln!("\n  {} {}\n", "âœ— Error:".red().bold(), e);
        std::process::exit(1);
    }
}

fn handle_external_plugin(args: Vec<String>) -> anyhow::Result<()> {
    use anyhow::Context;

    if args.is_empty() {
        anyhow::bail!("No plugin command provided");
    }

    let plugin_name = &args[0];
    let plugin_args = &args[1..];

    let reg = plugins::registry::load_registry().unwrap_or_default();
    if reg.plugins.is_empty() {
        anyhow::bail!(
            "Unknown command '{}'. No plugins installed.\n\nTry: starforge plugin install <name> --path <lib>",
            plugin_name
        );
    }

    let mut pm = plugins::PluginManager::new();
    for pl in &reg.plugins {
        unsafe {
            pm.load_plugin(&pl.path)
                .with_context(|| format!("Failed to load plugin '{}' from {}", pl.name, pl.path))?;
        }
    }

    pm.execute(plugin_name, plugin_args)
        .map_err(|e| anyhow::anyhow!(e))
}

fn print_banner() {
    println!(
        "{}",
        "\n  â–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ•—â–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ•— â–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ•— â–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ•— â–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ•— â–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ•— â–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ•—  â–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ•— â–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ•—\n  â–ˆâ–ˆâ•”â•â•â•â•â•â•šâ•â•â–ˆâ–ˆâ•”â•â•â•â–ˆâ–ˆâ•”â•â•â–ˆâ–ˆâ•—â–ˆâ–ˆâ•”â•â•â–ˆâ–ˆâ•—â–ˆâ–ˆâ•”â•â•â•â•â•â–ˆâ–ˆâ•”â•â•â•â–ˆâ–ˆâ•—â–ˆâ–ˆâ•”â•â•â–ˆâ–ˆâ•—â–ˆâ–ˆâ•”â•â•â•â•â• â–ˆâ–ˆâ•”â•â•â•â•â•\n  â–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ•—   â–ˆâ–ˆâ•‘   â–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ•‘â–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ•”â•â–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ•—  â–ˆâ–ˆâ•‘   â–ˆâ–ˆâ•‘â–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ•”â•â–ˆâ–ˆâ•‘  â–ˆâ–ˆâ–ˆâ•—â–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ•—  \n  â•šâ•â•â•â•â–ˆâ–ˆâ•‘   â–ˆâ–ˆâ•‘   â–ˆâ–ˆâ•”â•â•â–ˆâ–ˆâ•‘â–ˆâ–ˆâ•”â•â•â–ˆâ–ˆâ•—â–ˆâ–ˆâ•”â•â•â•  â–ˆâ–ˆâ•‘   â–ˆâ–ˆâ•‘â–ˆâ–ˆâ•”â•â•â–ˆâ–ˆâ•—â–ˆâ–ˆâ•‘   â–ˆâ–ˆâ•‘â–ˆâ–ˆâ•”â•â•â•  \n  â–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ•‘   â–ˆâ–ˆâ•‘   â–ˆâ–ˆâ•‘  â–ˆâ–ˆâ•‘â–ˆâ–ˆâ•‘  â–ˆâ–ˆâ•‘â–ˆâ–ˆâ•‘     â•šâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ•”â•â–ˆâ–ˆâ•‘  â–ˆâ–ˆâ•‘â•šâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ•”â•â–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ–ˆâ•—\n  â•šâ•â•â•â•â•â•â•   â•šâ•â•   â•šâ•â•  â•šâ•â•â•šâ•â•  â•šâ•â•â•šâ•â•      â•šâ•â•â•â•â•â• â•šâ•â•  â•šâ•â• â•šâ•â•â•â•â•â• â•šâ•â•â•â•â•â•â•\n"
            .cyan()
            .bold()
    );
    println!(
        "  {} {}\n",
        "âš¡ Stellar & Soroban Developer CLI".bright_white(),
        "v0.1.0".dimmed()
    );
}
