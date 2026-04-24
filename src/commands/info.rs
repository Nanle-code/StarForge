use crate::utils::{config, horizon, print as p};
use anyhow::Result;
use colored::*;

pub fn handle() -> Result<()> {
    p::header("starforge Environment");
    p::separator();

    let cfg = config::load()?;

    p::kv("Version", "0.1.0");
    p::kv("Config file", &config::config_path().display().to_string());
    p::kv_accent("Network", &cfg.network);
    p::kv("Wallets saved", &cfg.wallets.len().to_string());
    println!();

    p::info("Checking network connectivity…");
    println!();
    for net in ["testnet", "mainnet"] {
        let online = horizon::check_network(net);
        println!(
            "  {} {}  {}",
            "◎".cyan(),
            format!("{:<10}", net),
            if online {
                "online".green().bold()
            } else {
                "unreachable".red().into()
            }
        );
    }

    println!();
    p::separator();
    println!("  {}", "Commands:".bright_white().bold());
    println!();
    let cmds = [
        ("starforge wallet create <n>", "Create a new keypair"),
        ("starforge wallet list", "List saved wallets"),
        ("starforge wallet show <n>", "Show wallet + live balance"),
        ("starforge wallet fund <n>", "Fund via Friendbot (testnet)"),
        ("starforge wallet remove <n>", "Remove a wallet"),
        ("starforge new contract <n>", "Scaffold a Soroban contract"),
        ("starforge new dapp <n>", "Scaffold a Stellar dApp"),
        ("starforge deploy --wasm <f>", "Deploy a compiled contract"),
    ];
    for (cmd, desc) in &cmds {
        println!("  {}  {}", format!("{:<38}", cmd).cyan(), desc.dimmed());
    }
    println!();

    Ok(())
}
