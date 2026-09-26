use crate::utils::{config, confirmation, horizon, print as p};
use anyhow::Result;
use clap::Args;
use stellar_xdr::curr::Asset;

#[derive(Args)]
pub struct PathPayArgs {
    /// Destination account (G...)
    #[arg(long)]
    destination: String,

    /// Destination asset code (optional, defaults to native XLM)
    #[arg(long)]
    dest_asset_code: Option<String>,

    /// Destination asset issuer (required if dest_asset_code is provided)
    #[arg(long)]
    dest_asset_issuer: Option<String>,

    /// Amount destination should receive
    #[arg(long)]
    dest_amount: String,

    /// Source asset code (optional, defaults to native XLM)
    #[arg(long)]
    send_asset_code: Option<String>,

    /// Source asset issuer (required if send_asset_code is provided)
    #[arg(long)]
    send_asset_issuer: Option<String>,

    /// Maximum amount willing to send
    #[arg(long)]
    send_max: String,

    /// Find best path automatically via Horizon
    #[arg(long)]
    auto_path: bool,

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

pub async fn handle(args: PathPayArgs) -> Result<()> {
    let cfg = config::load()?;
    let network = args.network.as_ref().unwrap_or(&cfg.network);

    // Validate destination
    if !args.destination.starts_with('G') {
        anyhow::bail!("Destination must be a valid Stellar public key (G...)");
    }

    // Validate asset parameters
    if args.dest_asset_code.is_some() != args.dest_asset_issuer.is_some() {
        anyhow::bail!("Both dest_asset_code and dest_asset_issuer must be provided together, or neither");
    }

    if args.send_asset_code.is_some() != args.send_asset_issuer.is_some() {
        anyhow::bail!("Both send_asset_code and send_asset_issuer must be provided together, or neither");
    }

    // Validate amounts
    if args.dest_amount.parse::<f64>().is_err() {
        anyhow::bail!("Invalid destination amount format");
    }
    if args.send_max.parse::<f64>().is_err() {
        anyhow::bail!("Invalid send_max amount format");
    }

    // Find wallet
    let wallet = cfg
        .wallets
        .iter()
        .find(|w| w.name == args.wallet)
        .ok_or_else(|| anyhow::anyhow!("Wallet '{}' not found", args.wallet))?;

    let send_asset_display = match (&args.send_asset_code, &args.send_asset_issuer) {
        (Some(code), Some(issuer)) => format!("{}:{}", code, issuer),
        _ => "XLM (native)".to_string(),
    };

    let dest_asset_display = match (&args.dest_asset_code, &args.dest_asset_issuer) {
        (Some(code), Some(issuer)) => format!("{}:{}", code, issuer),
        _ => "XLM (native)".to_string(),
    };

    // Find best path if requested
    let path: Vec<Asset> = if args.auto_path {
        if !args.json {
            p::step(1, 3, "Finding best payment path...");
        }
        
        // SAFETY: CodeQL flags this as "cleartext transmission of sensitive information"
        // because wallet struct contains secret_key (validated via validate_secret_key).
        // However, only wallet.public_key (a public Stellar address, G...) is transmitted
        // to Horizon here, never the secret key. The taint tracking is not field-sensitive.
        // Horizon URL is validated to use HTTPS for all built-in networks (testnet, mainnet)
        // in config::get_network_config(). See issue #934 security review.
        let paths = horizon::find_payment_paths(
            &wallet.public_key,
            &args.destination,
            args.dest_asset_code.as_deref(),
            args.dest_asset_issuer.as_deref(),
            &args.dest_amount,
            network,
        )
        .await?;

        if paths.is_empty() {
            anyhow::bail!("No payment paths found between the specified assets");
        }

        let best_path = &paths[0];
        
        if !args.json {
            p::success(&format!("Found {} possible path(s)", paths.len()));
            p::info(&format!(
                "Best path: send {} {}, receive {} {}",
                best_path.source_amount,
                args.send_asset_code.as_deref().unwrap_or("XLM"),
                best_path.destination_amount,
                args.dest_asset_code.as_deref().unwrap_or("XLM")
            ));
            println!();
        }

        // Convert path assets to stellar_xdr::Asset
        best_path
            .path
            .iter()
            .filter_map(|pa| {
                match (pa.asset_type.as_str(), &pa.asset_code, &pa.asset_issuer) {
                    ("native", _, _) => Some(Asset::Native),
                    (_, Some(code), Some(issuer)) => {
                        // Parse asset using horizon utility
                        match parse_path_asset(code, issuer) {
                            Ok(asset) => Some(asset),
                            Err(_) => None,
                        }
                    }
                    _ => None,
                }
            })
            .collect()
    } else {
        Vec::new() // Empty path means direct conversion
    };

    // Fetch account to get sequence number
    let account = horizon::fetch_account(&wallet.public_key, network).await?;
    let sequence = account.sequence.parse::<u64>()? + 1;

    if !args.json {
        if args.auto_path {
            p::step(2, 3, "Building path payment transaction...");
        } else {
            p::step(1, 2, "Building path payment transaction...");
        }
    }

    // Build transaction
    let tx_xdr = horizon::build_path_payment_transaction(
        &wallet.public_key,
        args.send_asset_code.as_deref(),
        args.send_asset_issuer.as_deref(),
        &args.send_max,
        &args.destination,
        args.dest_asset_code.as_deref(),
        args.dest_asset_issuer.as_deref(),
        &args.dest_amount,
        path.clone(),
        sequence,
        network,
    )?;

    let estimated_fee = 100_000u64; // Base fee for path payment operation

    if !args.json {
        println!();
        p::header("Path Payment Transaction");
        p::separator();
        p::kv("From", &wallet.public_key);
        p::kv("To", &args.destination);
        p::kv("Send (max)", &format!("{} {}", args.send_max, args.send_asset_code.as_deref().unwrap_or("XLM")));
        p::kv("Send Asset", &send_asset_display);
        p::kv("Receive (exact)", &format!("{} {}", args.dest_amount, args.dest_asset_code.as_deref().unwrap_or("XLM")));
        p::kv("Receive Asset", &dest_asset_display);
        p::kv("Path Hops", &format!("{}", path.len()));
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
            "Path Payment".to_string(),
            network.clone(),
            risk_level,
        )
        .add("From", &wallet.public_key)
        .add("To", &args.destination)
        .add("Send (max)", &format!("{} {}", args.send_max, args.send_asset_code.as_deref().unwrap_or("XLM")))
        .add("Send Asset", &send_asset_display)
        .add("Receive (exact)", &format!("{} {}", args.dest_amount, args.dest_asset_code.as_deref().unwrap_or("XLM")))
        .add("Receive Asset", &dest_asset_display)
        .add("Path Hops", format!("{}", path.len()))
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
                "send_max": args.send_max,
                "send_asset_code": args.send_asset_code,
                "send_asset_issuer": args.send_asset_issuer,
                "dest_amount": args.dest_amount,
                "dest_asset_code": args.dest_asset_code,
                "dest_asset_issuer": args.dest_asset_issuer,
                "path_hops": path.len(),
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
        "Path Payment".to_string(),
        network.clone(),
        risk_level,
    )
    .add("From", &wallet.public_key)
    .add("To", &args.destination)
    .add("Send (max)", &format!("{} {}", args.send_max, args.send_asset_code.as_deref().unwrap_or("XLM")))
    .add("Send Asset", &send_asset_display)
    .add("Receive (exact)", &format!("{} {}", args.dest_amount, args.dest_asset_code.as_deref().unwrap_or("XLM")))
    .add("Receive Asset", &dest_asset_display)
    .add("Path Hops", format!("{}", path.len()))
    .add("Estimated Fee", format!("{} stroops", estimated_fee));

    let confirm_config = confirmation::ConfirmationConfig {
        risk_level,
        network: network.clone(),
        skip_confirm: args.yes,
        dry_run: false,
        prompt: Some("Submit this path payment?".to_string()),
        require_type_confirmation: *network == "mainnet",
        destructive_action: if *network == "mainnet" {
            Some(confirmation::DestructiveAction::Payment)
        } else {
            None
        },
        challenge_phrase: None,
    };

    if !args.json && !confirmation::confirm_operation(&summary, &confirm_config)? {
        p::info("Path payment cancelled.");
        return Ok(());
    }

    // Submit transaction
    if !args.json {
        if args.auto_path {
            p::step(3, 3, "Submitting to network...");
        } else {
            p::step(2, 2, "Submitting to network...");
        }
    }

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
            "send_max": args.send_max,
            "send_asset_code": args.send_asset_code,
            "send_asset_issuer": args.send_asset_issuer,
            "dest_amount": args.dest_amount,
            "dest_asset_code": args.dest_asset_code,
            "dest_asset_issuer": args.dest_asset_issuer,
            "path_hops": path.len(),
            "network": network
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!();
        p::success("Path payment submitted successfully!");
        p::separator();
        p::kv_accent("Transaction Hash", &result.hash);
        p::kv("Send (max)", &format!("{} {}", args.send_max, args.send_asset_code.as_deref().unwrap_or("XLM")));
        p::kv("Receive (exact)", &format!("{} {}", args.dest_amount, args.dest_asset_code.as_deref().unwrap_or("XLM")));
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

// Helper to parse path assets
fn parse_path_asset(asset_code: &str, asset_issuer: &str) -> Result<Asset> {
    use stellar_strkey::ed25519;
    use stellar_xdr::curr::{
        AccountId, AlphaNum12, AlphaNum4, AssetCode12, AssetCode4, PublicKey, Uint256,
    };

    let issuer_pk = ed25519::PublicKey::from_string(asset_issuer)
        .with_context(|| format!("Invalid asset issuer: {}", asset_issuer))?;

    let issuer = AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(issuer_pk.0)));

    if asset_code.len() <= 4 {
        let mut code_bytes = [0u8; 4];
        let bytes = asset_code.as_bytes();
        code_bytes[..bytes.len()].copy_from_slice(bytes);

        Ok(Asset::CreditAlphanum4(AlphaNum4 {
            asset_code: AssetCode4(code_bytes),
            issuer,
        }))
    } else if asset_code.len() <= 12 {
        let mut code_bytes = [0u8; 12];
        let bytes = asset_code.as_bytes();
        code_bytes[..bytes.len()].copy_from_slice(bytes);

        Ok(Asset::CreditAlphanum12(AlphaNum12 {
            asset_code: AssetCode12(code_bytes),
            issuer,
        }))
    } else {
        anyhow::bail!("Asset code must be 1-12 characters")
    }
}
