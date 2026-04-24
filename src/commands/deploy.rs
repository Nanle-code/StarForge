use crate::utils::{config, horizon, print as p};
use anyhow::Result;
use clap::Args;
use colored::*;
use std::fs;
use std::path::PathBuf;

#[derive(Args)]
pub struct DeployArgs {
    /// Path to the compiled .wasm file
    #[arg(long)]
    pub wasm: PathBuf,
    /// Network to deploy to
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
    /// Wallet name to use for deployment
    #[arg(long)]
    pub wallet: Option<String>,
    /// Skip confirmation prompt
    #[arg(long, default_value = "false")]
    pub yes: bool,
}

pub fn handle(args: DeployArgs) -> Result<()> {
    p::header("Deploy Soroban Contract");

    if !args.wasm.exists() {
        anyhow::bail!(
            "WASM file not found: {:?}\nRun `stellar contract build` first.",
            args.wasm
        );
    }

    let wasm_bytes = fs::read(&args.wasm)?;
    let wasm_size_kb = wasm_bytes.len() as f64 / 1024.0;

    p::separator();
    p::kv("WASM file", &args.wasm.display().to_string());
    p::kv("WASM size", &format!("{:.1} KB", wasm_size_kb));
    p::kv("Network", &args.network);

    if wasm_size_kb > 128.0 {
        p::warn(&format!(
            "WASM is {:.1} KB — Soroban limit is 128 KB. Optimize with --release.",
            wasm_size_kb
        ));
    }

    let cfg = config::load()?;
    let wallet = if let Some(ref wallet_name) = args.wallet {
        cfg.wallets
            .iter()
            .find(|w| &w.name == wallet_name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Wallet '{}' not found. Run `starforge wallet list`",
                    wallet_name
                )
            })?
    } else if !cfg.wallets.is_empty() {
        p::info(&format!(
            "No --wallet specified. Using: {}",
            cfg.wallets[0].name.cyan()
        ));
        &cfg.wallets[0]
    } else {
        anyhow::bail!(
            "No wallets found. Create one first:\n  starforge wallet create deployer --fund"
        );
    };

    p::kv("Wallet", &wallet.name);
    p::kv_accent("Public Key", &wallet.public_key);
    p::separator();

    if args.network == "mainnet" {
        p::warn("You are deploying to MAINNET. This costs real XLM.");
    }

    if !args.yes {
        println!();
        print!("  Proceed? [y/N] ");
        use std::io::BufRead;
        let line = std::io::stdin()
            .lock()
            .lines()
            .next()
            .unwrap_or(Ok(String::new()))?;
        if !matches!(line.trim().to_lowercase().as_str(), "y" | "yes") {
            p::info("Deployment cancelled.");
            return Ok(());
        }
    }

    println!();
    p::step(1, 3, "Verifying account on-chain…");
    let account = horizon::fetch_account(&wallet.public_key, &args.network).map_err(|e| {
        anyhow::anyhow!(
            "Account not active on {}: {}\nFund it with: starforge wallet fund {}",
            args.network,
            e,
            wallet.name
        )
    })?;

    let xlm = account
        .balances
        .iter()
        .find(|b| b.asset_type == "native")
        .map(|b| b.balance.as_str())
        .unwrap_or("0");
    p::kv_accent("XLM Balance", &format!("{} XLM", xlm));

    p::step(2, 3, "Calculating WASM hash…");
    let hash_val = wasm_bytes.iter().enumerate().fold(0u64, |acc, (i, &b)| {
        acc.wrapping_add((b as u64).wrapping_mul(i as u64 + 1))
    });
    let wasm_hash = format!("{:016x}", hash_val);
    p::kv("WASM hash (local)", &wasm_hash);

    p::step(3, 3, "Generating stellar CLI command…");

    println!();
    p::separator();
    println!(
        "  {} {}",
        "✓".green().bold(),
        "Ready! Run this to complete the deployment:".bright_white()
    );
    println!();
    println!("  {}", "stellar contract deploy \\".cyan());
    println!(
        "    {}",
        format!("--wasm {} \\", args.wasm.display()).cyan()
    );
    println!(
        "    {}",
        format!("--source {} \\", wallet.public_key).cyan()
    );
    println!("    {}", format!("--network {}", args.network).cyan());
    println!();
    p::info("Install the Stellar CLI: https://developers.stellar.org/docs/tools/stellar-cli");
    p::separator();

    Ok(())
}
