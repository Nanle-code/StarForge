use crate::utils::{config, confirmation, horizon, print as p};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct PayArgs {
    /// Destination account (G...)
    #[arg(long)]
    destination: String,

    /// Amount to send
    #[arg(long)]
    amount: String,

    /// Asset code (optional, defaults to native XLM)
    #[arg(long)]
    asset_code: Option<String>,

    /// Asset issuer's public key (required if asset_code is provided)
    #[arg(long)]
    asset_issuer: Option<String>,

    /// Wallet to use for signing
    #[arg(long)]
    wallet: String,

    /// Network to use (overrides config)
    #[arg(long)]
    network: Option<String>,

    /// Dry-run: simulate only, don't submit
    #[arg(long)]
    dry_run: bool,

    /// Skip confirmation prompt
    #[arg(long, default_value = "false")]
    yes: bool,

    /// Output as JSON
    #[arg(long)]
    json: bool,
}

pub async fn handle(args: PayArgs) -> Result<()> {
    let cfg = config::load()?;
    let network = args.network.as_ref().unwrap_or(&cfg.network);

    // Validate destination
    if !args.destination.starts_with('G') {
        anyhow::bail!("Destination must be a valid Stellar public key (G...)");
    }

    // Validate asset parameters
    if args.asset_code.is_some() != args.asset_issuer.is_some() {
        anyhow::bail!("Both asset_code and asset_issuer must be provided together, or neither");
    }

    if let Some(ref code) = args.asset_code {
        if code.is_empty() || code.len() > 12 {
            anyhow::bail!("Asset code must be 1-12 characters");
        }
    }

    // Validate amount
    if args.amount.parse::<f64>().is_err() {
        anyhow::bail!("Invalid amount format");
    }

    // Find wallet
    let wallet = cfg
        .wallets
        .iter()
        .find(|w| w.name == args.wallet)
        .ok_or_else(|| anyhow::anyhow!("Wallet '{}' not found", args.wallet))?;

    // Fetch account to get sequence number
    let account = horizon::fetch_account(&wallet.public_key, network).await?;
    let sequence = account.sequence.parse::<u64>()? + 1;

    // Build transaction
    let tx_xdr = horizon::build_payment_transaction(
        &wallet.public_key,
        &args.destination,
        &args.amount,
        args.asset_code.as_deref(),
        args.asset_issuer.as_deref(),
        sequence,
        network,
    )?;

    let estimated_fee = 100_000u64; // Base fee for payment operation

    let asset_display = match (&args.asset_code, &args.asset_issuer) {
        (Some(code), Some(issuer)) => format!("{}:{}", code, issuer),
        _ => "XLM (native)".to_string(),
    };

    if !args.json {
        p::header("Payment Transaction");
        p::separator();
        p::kv("From", &wallet.public_key);
        p::kv("To", &args.destination);
        p::kv("Amount", &format!("{} {}", args.amount, args.asset_code.as_deref().unwrap_or("XLM")));
        p::kv("Asset", &asset_display);
        p::kv("Network", network);
        p::kv("Estimated Fee", &format!("{} stroops", estimated_fee));
        println!();
    }

    // Preview
    if args.dry_run || !args.json {
        let risk_level = if *network == "mainnet" {
            confirmation::RiskLevel::High
        } else {
            confirmation::RiskLevel::Medium
        };

        let summary = confirmation::OperationSummary::new(
            "Payment".to_string(),
            network.clone(),
            risk_level,
        )
        .add("From", &wallet.public_key)
        .add("To", &args.destination)
        .add("Amount", &format!("{} {}", args.amount, args.asset_code.as_deref().unwrap_or("XLM")))
        .add("Asset", &asset_display)
        .add("Estimated Fee", format!("{} stroops", estimated_fee));

        if !args.json {
            confirmation::display_preview(&summary);
            println!();
        }
    }

    if args.dry_run {
        if args.json {
            let output = serde_json::json!({
                "status": "dry_run",
                "from": wallet.public_key,
                "to": args.destination,
                "amount": args.amount,
                "asset_code": args.asset_code,
                "asset_issuer": args.asset_issuer,
                "network": network,
                "estimated_fee": estimated_fee,
                "transaction_xdr": tx_xdr
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else {
            p::success("Dry-run completed. Transaction not submitted.");
        }
        return Ok(());
    }

    // Confirmation for actual submission
    let risk_level = if *network == "mainnet" {
        confirmation::RiskLevel::High
    } else {
        confirmation::RiskLevel::Medium
    };

    let summary = confirmation::OperationSummary::new(
        "Payment".to_string(),
        network.clone(),
        risk_level,
    )
    .add("From", &wallet.public_key)
    .add("To", &args.destination)
    .add("Amount", &format!("{} {}", args.amount, args.asset_code.as_deref().unwrap_or("XLM")))
    .add("Asset", &asset_display)
    .add("Estimated Fee", format!("{} stroops", estimated_fee));

    let confirm_config = confirmation::ConfirmationConfig {
        risk_level,
        network: network.clone(),
        skip_confirm: args.yes,
        dry_run: false,
        prompt: Some("Submit this payment?".to_string()),
        require_type_confirmation: *network == "mainnet",
        destructive_action: if *network == "mainnet" {
            Some(confirmation::DestructiveAction::Payment)
        } else {
            None
        },
        challenge_phrase: None,
    };

    if !args.json && !confirmation::confirm_operation(&summary, &confirm_config)? {
        p::info("Payment cancelled.");
        return Ok(());
    }

    // Submit transaction
    let signing_request = crate::utils::wallet_signer::SigningRequest::local_secret(
        zeroize::Zeroizing::new(wallet.secret_key.clone()),
        network,
    );
    let result = horizon::submit_payment_with_signing(&tx_xdr, &signing_request, network).await?;

    if args.json {
        let output = serde_json::json!({
            "status": "success",
            "transaction_hash": result.hash,
            "from": wallet.public_key,
            "to": args.destination,
            "amount": args.amount,
            "asset_code": args.asset_code,
            "asset_issuer": args.asset_issuer,
            "network": network
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        p::success("Payment submitted successfully!");
        p::separator();
        p::kv_accent("Transaction Hash", &result.hash);
        p::kv("Amount", &format!("{} {}", args.amount, args.asset_code.as_deref().unwrap_or("XLM")));
        p::kv("From", &wallet.public_key);
        p::kv("To", &args.destination);
        println!();
        p::info(&format!(
            "View on Stellar Expert: https://stellar.expert/explorer/{}/tx/{}",
            network, result.hash
        ));
    }

    Ok(())
}
