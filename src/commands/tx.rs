use anyhow::Result;
use clap::{Args, Subcommand};
use colored::*;

use crate::utils::{config, horizon, print as p};

#[derive(Args)]
pub struct TxArgs {
    #[command(subcommand)]
    pub command: TxCommands,
}

#[derive(Subcommand)]
pub enum TxCommands {
    /// Send a Stellar payment transaction
    Send(SendArgs),
    /// Fetch and display recent transactions for a Stellar account
    History {
        public_key: String,

        /// number of transactions
        #[arg(short, long, default_value_t = 10)]
        limit: u8,

        #[arg(short, long)]
        network: Option<String>,
    },
}

#[derive(Args)]
pub struct SendArgs {
    /// Wallet name to send from
    #[arg(long)]
    pub from: String,
    /// Destination public key
    #[arg(long)]
    pub to: String,
    /// Amount to send
    #[arg(long)]
    pub amount: String,
    /// Asset to send (XLM or CODE:ISSUER format)
    #[arg(long, default_value = "XLM")]
    pub asset: String,
    /// Network to use
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
    /// Skip confirmation prompt
    #[arg(long, default_value = "false")]
    pub yes: bool,
}

pub fn handle(args: TxArgs) -> Result<()> {
    match args.command {
        TxCommands::Send(args) => handle_send(args),
        TxCommands::History {
            public_key,
            limit,
            network,
        } => handle_history(public_key, limit, network),
    }
}

fn handle_send(args: SendArgs) -> Result<()> {
    p::header("Send Stellar Payment");

    // Load configuration and find wallet
    let cfg = config::load()?;
    let wallet = cfg.wallets
        .iter()
        .find(|w| w.name == args.from)
        .ok_or_else(|| anyhow::anyhow!("Wallet '{}' not found. Run `starforge wallet list`", args.from))?;

    // Validate wallet has secret key
    if wallet.secret_key.is_none() {
        anyhow::bail!("Wallet '{}' has no secret key stored", args.from);
    }

    // Parse asset
    let (asset_code, asset_issuer) = parse_asset(&args.asset)?;

    // Validate amount
    let amount_f64: f64 = args.amount.parse()
        .map_err(|_| anyhow::anyhow!("Invalid amount: {}", args.amount))?;
    if amount_f64 <= 0.0 {
        anyhow::bail!("Amount must be positive");
    }

    p::separator();
    p::kv("From Wallet", &wallet.name);
    p::kv("From Address", &wallet.public_key);
    p::kv("To Address", &args.to);
    p::kv("Amount", &format!("{} {}", args.amount, args.asset));
    p::kv("Network", &args.network);

    if args.network == "mainnet" {
        p::warn("You are sending on MAINNET. This will cost real XLM.");
    }

    p::separator();

    // Step 1: Fetch source account info
    println!();
    p::step(1, 3, "Fetching source account info…");
    let source_account = horizon::fetch_account(&wallet.public_key, &args.network)
        .map_err(|e| anyhow::anyhow!(
            "Source account not found on {}: {}\nFund it with: starforge wallet fund {}",
            args.network, e, wallet.name
        ))?;

    // Check balance if sending XLM
    if asset_code.is_none() {
        let xlm_balance = source_account.balances.iter()
            .find(|b| b.asset_type == "native")
            .map(|b| b.balance.parse::<f64>().unwrap_or(0.0))
            .unwrap_or(0.0);
        
        p::kv("XLM Balance", &format!("{:.7} XLM", xlm_balance));
        
        if xlm_balance < amount_f64 + 0.00001 { // Reserve for fees
            anyhow::bail!("Insufficient XLM balance. Have: {:.7}, Need: {:.7} + fees", xlm_balance, amount_f64);
        }
    }

    // Step 2: Validate destination account
    p::step(2, 3, "Validating destination account…");
    match horizon::fetch_account(&args.to, &args.network) {
        Ok(_) => p::kv("Destination", "✓ Account exists"),
        Err(_) => {
            if asset_code.is_none() {
                p::kv("Destination", "⚠ Account will be created (requires 1 XLM minimum)");
                if amount_f64 < 1.0 {
                    anyhow::bail!("Destination account doesn't exist. Minimum 1 XLM required to create account.");
                }
            } else {
                anyhow::bail!("Destination account doesn't exist and cannot be created with non-native assets");
            }
        }
    }

    // Step 3: Build and simulate transaction
    p::step(3, 3, "Building and simulating transaction…");
    let tx_result = horizon::build_and_simulate_payment(
        &wallet.public_key,
        &args.to,
        &args.amount,
        asset_code.as_deref(),
        asset_issuer.as_deref(),
        &source_account.sequence,
        &args.network,
    )?;

    p::kv("Estimated Fee", &format!("{:.7} XLM", tx_result.fee as f64 / 10_000_000.0));
    p::kv("Transaction XDR", &format!("{}...", &tx_result.transaction_xdr[..20]));

    // Confirmation prompt
    if !args.yes {
        println!();
        print!("  Proceed with payment? [y/N] ");
        use std::io::BufRead;
        let line = std::io::stdin().lock().lines().next()
            .unwrap_or(Ok(String::new()))?;
        if !matches!(line.trim().to_lowercase().as_str(), "y" | "yes") {
            p::info("Payment cancelled.");
            return Ok(());
        }
    }

    // Submit transaction
    println!();
    p::info("Submitting transaction…");
    let submit_result = horizon::submit_payment_transaction(
        &tx_result.transaction_xdr,
        wallet.secret_key.as_ref().unwrap(),
        &args.network,
    )?;

    println!();
    p::separator();
    println!("  {} {}", "✓".green().bold(), "Payment sent successfully!".bright_white());
    println!();
    p::kv_accent("Transaction Hash", &submit_result.hash);
    
    let explorer_base = if args.network == "mainnet" {
        "https://stellar.expert/explorer/public/tx"
    } else {
        "https://stellar.expert/explorer/testnet/tx"
    };
    
    p::kv("Stellar Expert", &format!("{}/{}", explorer_base, submit_result.hash));
    p::separator();

    Ok(())
}

fn parse_asset(asset: &str) -> Result<(Option<String>, Option<String>)> {
    if asset.to_uppercase() == "XLM" {
        Ok((None, None))
    } else if asset.contains(':') {
        let parts: Vec<&str> = asset.split(':').collect();
        if parts.len() != 2 {
            anyhow::bail!("Invalid asset format. Use CODE:ISSUER or XLM");
        }
        Ok((Some(parts[0].to_string()), Some(parts[1].to_string())))
    } else {
        anyhow::bail!("Invalid asset format. Use CODE:ISSUER or XLM");
    }
}

fn handle_history(public_key: String, limit: u8, network_override: Option<String>) -> Result<()> {
    let limit = limit.min(50);

    let network = network_override.unwrap_or_else(|| {
        config::load()
            .map(|c| c.network)
            .unwrap_or_else(|_| "testnet".to_string())
    });

    println!();
    println!("  {} {}", "◆".cyan().bold(), "Transaction History".white().bold());
    println!("  {} {}", "Account :".dimmed(), public_key.yellow());
    println!("  {} {}", "Network :".dimmed(), network.cyan());
    println!("  {} {}", "Showing :".dimmed(), format!("last {} txs", limit).white());
    println!("  {}", "─".repeat(72).dimmed());

    match horizon::fetch_transactions(&public_key, &network, limit) {
        Err(e) => {
            println!("\n  {} {}\n", "✗".red().bold(), e.to_string().red());
        }
        Ok(txs) if txs.is_empty() => {
            println!("\n  {} No transactions found for this account.\n", "!".yellow().bold());
        }
        Ok(txs) => {
            print_transactions(&txs, &network);
        }
    }

    Ok(())
}

fn print_transactions(txs: &[horizon::TransactionRecord], network: &str) {
    
    println!(
        "  {:<14}  {:<6}  {:<4}  {:<12}  {}",
        "Hash".dimmed(),
        "Status".dimmed(),
        "Ops".dimmed(),
        "Fee (XLM)".dimmed(),
        "Timestamp (UTC)".dimmed(),
    );
    println!("  {}", "─".repeat(72).dimmed());

    for tx in txs {
        let short_hash = format!("{}…", &tx.hash[..12]);

        let status = if tx.successful {
            "✓ ok".green().to_string()
        } else {
            "✗ fail".red().to_string()
        };

        // returns fee in stroops; 1 XLM is equal to 10_000_000 stroops
        let fee_xlm = tx
            .fee_charged
            .parse::<f64>()
            .map(|s| format!("{:.7}", s / 10_000_000.0))
            .unwrap_or_else(|_| tx.fee_charged.clone());

        let ts = tx
            .created_at
            .replace('T', " ")
            .get(..16)
            .unwrap_or(&tx.created_at)
            .to_string();

        println!(
            "  {:<14}  {:<6}  {:<4}  {:<12}  {}",
            short_hash.white(),
            status,
            tx.operation_count.to_string().white(),
            fee_xlm.yellow(),
            ts.dimmed(),
        );
    }

    println!("  {}", "─".repeat(72).dimmed());

    // footer === Stellar Expert deep link
    let explorer_base = if network == "mainnet" {
        "https://stellar.expert/explorer/public/tx"
    } else {
        "https://stellar.expert/explorer/testnet/tx"
    };

    if let Some(first) = txs.first() {
        println!(
            "\n  {} {}/{}\n",
            "🔗 Latest tx:".dimmed(),
            explorer_base,
            first.hash.cyan()
        );
    }
}