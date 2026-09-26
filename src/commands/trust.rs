use crate::utils::{config, confirmation, horizon, print as p};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct TrustArgs {
    /// Asset code (e.g., USDC, EUR)
    #[arg(long)]
    asset_code: String,

    /// Asset issuer's public key (G...)
    #[arg(long)]
    asset_issuer: String,

    /// Trust limit (optional, defaults to maximum)
    #[arg(long)]
    limit: Option<String>,

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

pub async fn handle(args: TrustArgs) -> Result<()> {
    let cfg = config::load()?;
    let network = args.network.as_ref().unwrap_or(&cfg.network);

    // Validate asset code
    if args.asset_code.is_empty() || args.asset_code.len() > 12 {
        anyhow::bail!("Asset code must be 1-12 characters");
    }

    // Validate asset issuer
    if !args.asset_issuer.starts_with('G') {
        anyhow::bail!("Asset issuer must be a valid Stellar public key (G...)");
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
    let tx_xdr = horizon::build_change_trust_transaction(
        &wallet.public_key,
        &args.asset_code,
        &args.asset_issuer,
        args.limit.as_deref(),
        sequence,
        network,
    )?;

    let estimated_fee = 100_000u64; // Base fee for trustline operation

    if !args.json {
        p::header("Establishing Trustline");
        p::separator();
        p::kv("Asset", &format!("{}:{}", args.asset_code, args.asset_issuer));
        if let Some(ref limit) = args.limit {
            p::kv("Trust Limit", limit);
        } else {
            p::kv("Trust Limit", "Maximum");
        }
        p::kv("Wallet", &args.wallet);
        p::kv("Account", &wallet.public_key);
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
            "Establish Trustline".to_string(),
            network.clone(),
            risk_level,
        )
        .add("Asset Code", &args.asset_code)
        .add("Asset Issuer", &args.asset_issuer)
        .add(
            "Trust Limit",
            args.limit.as_deref().unwrap_or("Maximum"),
        )
        .add("Wallet", &args.wallet)
        .add("Account", &wallet.public_key)
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
                "asset_code": args.asset_code,
                "asset_issuer": args.asset_issuer,
                "limit": args.limit.unwrap_or_else(|| "max".to_string()),
                "account": wallet.public_key,
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
        "Establish Trustline".to_string(),
        network.clone(),
        risk_level,
    )
    .add("Asset Code", &args.asset_code)
    .add("Asset Issuer", &args.asset_issuer)
    .add(
        "Trust Limit",
        args.limit.as_deref().unwrap_or("Maximum"),
    )
    .add("Wallet", &args.wallet)
    .add("Account", &wallet.public_key)
    .add("Estimated Fee", format!("{} stroops", estimated_fee));

    let confirm_config = confirmation::ConfirmationConfig {
        risk_level,
        network: network.clone(),
        skip_confirm: args.yes,
        dry_run: false,
        prompt: Some("Establish this trustline?".to_string()),
        require_type_confirmation: *network == "mainnet",
        destructive_action: if *network == "mainnet" {
            Some(confirmation::DestructiveAction::TrustlineModification)
        } else {
            None
        },
        challenge_phrase: None,
    };

    if !args.json && !confirmation::confirm_operation(&summary, &confirm_config)? {
        p::info("Trustline operation cancelled.");
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
            "asset_code": args.asset_code,
            "asset_issuer": args.asset_issuer,
            "limit": args.limit.unwrap_or_else(|| "max".to_string()),
            "account": wallet.public_key,
            "network": network
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        p::success("Trustline established successfully!");
        p::separator();
        p::kv_accent("Transaction Hash", &result.hash);
        p::kv("Asset", &format!("{}:{}", args.asset_code, args.asset_issuer));
        p::kv("Account", &wallet.public_key);
        println!();
        p::info(&format!(
            "View on Stellar Expert: https://stellar.expert/explorer/{}/tx/{}",
            network, result.hash
        ));
    }

    Ok(())
}
