use anyhow::Result;
use clap::{Args, Subcommand};
use colored::*;
use std::io::Write;
use stellar_xdr::curr::TransactionEnvelope;

use crate::utils::confirmation;
use crate::utils::hardware_wallet::{self, HardwareWalletKind};
use crate::utils::horizon::FeeStats;
use crate::utils::{config, fee_payer, horizon, print as p, tx_batch, tx_builder, tx_xdr, wallet_signer};

#[derive(Args)]
pub struct TxArgs {
    #[command(subcommand)]
    pub command: TxCommands,
}

#[derive(Subcommand)]
pub enum TxCommands {
    /// Send a Stellar payment transaction
    Send(SendArgs),
    /// Submit multiple operations in one transaction from a JSON file
    Batch(BatchArgs),
    /// Fetch and display recent transactions for a Stellar account
    History(HistoryArgs),
    /// Show recommended fee levels based on Horizon fee stats
    Fees {
        /// Optional network (testnet/mainnet)
        #[arg(short, long)]
        network: Option<String>,
    },
    /// Decode a transaction envelope from XDR to JSON
    Decode(DecodeArgs),
    /// Encode a JSON transaction envelope back into XDR
    Encode(EncodeArgs),
    /// Sign a transaction envelope with a stored wallet
    Sign(SignArgs),
    /// Simulate an arbitrary transaction envelope on the network
    Simulate(SimulateArgs),
    /// Submit an arbitrary transaction envelope exactly as provided
    Submit(SubmitArgs),
}

/// An envelope supplied as an argument, a `--file`, or stdin.
#[derive(Args)]
pub struct XdrInputArgs {
    /// Base64 (or hex) transaction envelope XDR
    pub xdr: Option<String>,
    /// Read the XDR from a file instead of an argument or stdin
    #[arg(long)]
    pub file: Option<std::path::PathBuf>,
    /// Network the envelope is for; selects the signing hash
    #[arg(short, long)]
    pub network: Option<String>,
}

#[derive(Args)]
pub struct DecodeArgs {
    #[command(flatten)]
    pub input: XdrInputArgs,
    /// Emit the envelope JSON on a single line
    #[arg(long)]
    pub compact: bool,
    /// Emit only the transaction hash
    #[arg(long)]
    pub hash: bool,
    /// Emit the envelope, summary and hash as one JSON document
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct EncodeArgs {
    /// JSON transaction envelope, as produced by `tx decode`
    pub json: Option<String>,
    /// Read the JSON envelope from a file instead of an argument or stdin
    #[arg(long)]
    pub file: Option<std::path::PathBuf>,
    /// Output encoding for the XDR on stdout
    #[arg(long, default_value = "base64", value_parser = ["base64", "hex"])]
    pub format: String,
}

#[derive(Args)]
pub struct SignArgs {
    #[command(flatten)]
    pub input: XdrInputArgs,
    /// Wallet name to sign with
    #[arg(long)]
    pub wallet: String,
    /// Output encoding for the signed XDR on stdout
    #[arg(long, default_value = "base64", value_parser = ["base64", "hex"])]
    pub format: String,
}

#[derive(Args)]
pub struct SimulateArgs {
    #[command(flatten)]
    pub input: XdrInputArgs,
    /// Print the raw Soroban RPC response
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct SubmitArgs {
    #[command(flatten)]
    pub input: XdrInputArgs,
    /// Skip the confirmation prompt
    #[arg(long, default_value = "false")]
    pub yes: bool,
    /// Print the submission result as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct BatchArgs {
    /// Path to operations JSON file
    #[arg(long)]
    pub file: std::path::PathBuf,
    /// Wallet name to send from
    #[arg(long)]
    pub from: String,
    /// Network to use
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
    /// Skip confirmation prompt
    #[arg(long, default_value = "false")]
    pub yes: bool,
    /// Sign with a hardware wallet instead of a local secret key
    #[arg(long, value_enum)]
    pub hardware: Option<HardwareWalletKind>,
    /// HD derivation path for hardware wallet signing
    #[arg(long, default_value = crate::utils::hardware_wallet::STELLAR_HD_PATH)]
    pub hd_path: String,
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
    /// Wallet that pays the network fee via a fee bump (CAP-15)
    #[arg(long, value_name = "WALLET")]
    pub fee_payer: Option<String>,
    /// Skip confirmation prompt
    #[arg(long, default_value = "false")]
    pub yes: bool,
    /// Sign with a hardware wallet instead of a local secret key
    #[arg(long, value_enum)]
    pub hardware: Option<HardwareWalletKind>,
    /// HD derivation path for hardware wallet signing
    #[arg(long, default_value = crate::utils::hardware_wallet::STELLAR_HD_PATH)]
    pub hd_path: String,
}

#[derive(Args)]
pub struct HistoryArgs {
    /// Public key to fetch history for
    pub public_key: String,
    /// Limit number of transactions
    #[arg(long, default_value = "10")]
    pub limit: u8,
    /// Optional network (testnet/mainnet)
    #[arg(long)]
    pub network_override: Option<String>,
    /// Cursor for pagination
    #[arg(long)]
    pub cursor: Option<String>,
    /// Get transactions after cursor
    #[arg(long)]
    pub after: Option<String>,
    /// Get transactions before cursor
    #[arg(long)]
    pub before: Option<String>,
    /// Only successful transactions
    #[arg(long)]
    pub successful_only: bool,
    /// Include details
    #[arg(long)]
    pub details: bool,
}

pub async fn handle(args: TxArgs) -> Result<()> {
    match args.command {
        TxCommands::Fees { network } => handle_fees(network).await,
        TxCommands::Send(args) => handle_send(args).await,
        TxCommands::Batch(args) => handle_batch(args).await,
        TxCommands::History(args) => handle_history(args).await,
        TxCommands::Decode(args) => handle_decode(args),
        TxCommands::Encode(args) => handle_encode(args),
        TxCommands::Sign(args) => handle_sign(args),
        TxCommands::Simulate(args) => handle_simulate(args).await,
        TxCommands::Submit(args) => handle_submit(args).await,
    }
}

// ── XDR toolbox (#916) ────────────────────────────────────────────────────────
//
// `decode`, `encode` and `sign` write only the payload to stdout so they
// compose (`tx encode | tx sign | tx submit`); commentary goes to stderr.

/// Resolves `--network`, falling back to the configured default.
fn resolve_network(network: Option<&str>) -> Result<String> {
    let network = match network {
        Some(network) => network.to_string(),
        None => config::load()
            .map(|cfg| cfg.network)
            .unwrap_or_else(|_| "testnet".to_string()),
    };
    config::validate_network(&network)?;
    Ok(network)
}

fn emit_payload(payload: &str) -> Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    writeln!(handle, "{payload}")?;
    handle.flush()?;
    Ok(())
}

fn note(line: &str) {
    eprintln!("  {line}");
}

/// Reports what the envelope actually contains, and its hash, off the payload
/// stream so piping is unaffected.
fn report_envelope(envelope: &TransactionEnvelope, network: &str, passphrase: &str) {
    let summary = tx_xdr::summarize(envelope);
    note(&format!(
        "{} {}",
        "Envelope:".dimmed(),
        tx_xdr::envelope_kind(envelope).cyan()
    ));
    note(&format!(
        "{}  {}",
        "Source:".dimmed(),
        summary.source_account.yellow()
    ));
    note(&format!(
        "{}    {}  {}",
        "Seq:".dimmed(),
        summary.seq_num.white(),
        format!("(fee {} stroops)", summary.fee_stroops).dimmed()
    ));
    for (index, operation) in summary.operations.iter().enumerate() {
        note(&format!(
            "{}   {} {}",
            format!("Op {index}:").dimmed(),
            "→".cyan(),
            operation.white()
        ));
    }
    note(&format!(
        "{} {}",
        "Signatures:".dimmed(),
        if summary.signature_hints.is_empty() {
            "none yet".yellow().to_string()
        } else {
            format!(
                "{} (hints: {})",
                summary.signature_hints.len(),
                summary.signature_hints.join(", ")
            )
            .green()
            .to_string()
        }
    ));
    match tx_xdr::signature_hash(envelope, passphrase) {
        Ok(hash) => note(&format!(
            "{}      {}",
            "Hash:".dimmed(),
            hex::encode(hash).white().bold()
        )),
        Err(error) => note(&format!("{} {}", "Hash:".dimmed(), error.to_string().red())),
    }
    note(&format!("{}   {}", "Network:".dimmed(), network.cyan()));
}

fn handle_decode(args: DecodeArgs) -> Result<()> {
    let network = resolve_network(args.input.network.as_deref())?;
    let passphrase = config::get_network_passphrase(&network);
    let input = tx_xdr::read_payload(
        args.input.xdr.as_deref(),
        args.input.file.as_deref(),
        "xdr",
        "decode",
    )?;
    let envelope = tx_xdr::parse_envelope(&input)?;

    if args.hash {
        let hash = tx_xdr::signature_hash(&envelope, &passphrase)?;
        return emit_payload(&hex::encode(hash));
    }

    if args.json {
        let decoded = tx_xdr::decode(&input, Some(&passphrase))?;
        return emit_payload(&serde_json::to_string_pretty(&decoded)?);
    }

    emit_payload(&tx_xdr::to_json(&envelope, !args.compact)?)?;
    report_envelope(&envelope, &network, &passphrase);
    Ok(())
}

fn handle_encode(args: EncodeArgs) -> Result<()> {
    let json = tx_xdr::read_payload(args.json.as_deref(), args.file.as_deref(), "json", "encode")?;
    let envelope = tx_xdr::encode_json(&json)?;
    let format = tx_xdr::WireFormat::parse(&args.format)?;
    emit_payload(&tx_xdr::write_envelope(&envelope, format)?)
}

fn handle_sign(args: SignArgs) -> Result<()> {
    let network = resolve_network(args.input.network.as_deref())?;
    let passphrase = config::get_network_passphrase(&network);
    let input = tx_xdr::read_payload(
        args.input.xdr.as_deref(),
        args.input.file.as_deref(),
        "xdr",
        "sign",
    )?;
    let envelope = tx_xdr::parse_envelope(&input)?;
    let format = tx_xdr::WireFormat::parse(&args.format)?;

    config::validate_wallet_name(&args.wallet)?;
    let cfg = config::load()?;
    let wallet = cfg
        .wallets
        .iter()
        .find(|w| w.name == args.wallet)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Wallet '{}' not found. Run `starforge wallet list`",
                args.wallet
            )
        })?;

    let secret = wallet_signer::resolve_local_secret(wallet, &wallet.name)?;
    let signer = tx_xdr::public_key_for_secret(&secret)?;
    if signer != wallet.public_key {
        anyhow::bail!(
            "Wallet '{}' is configured for {} but its secret key belongs to {}. Refusing to sign.",
            wallet.name,
            wallet.public_key,
            signer
        );
    }

    let signed = tx_xdr::sign_envelope(&envelope, &secret, &passphrase)?;
    emit_payload(&tx_xdr::write_envelope(&signed, format)?)?;

    let summary = tx_xdr::summarize(&signed);
    note(&format!(
        "{} {}",
        "Signed:".dimmed(),
        format!("{signer} · {} signatures", summary.signature_hints.len()).green()
    ));
    if let Ok(hash) = tx_xdr::signature_hash(&signed, &passphrase) {
        note(&format!(
            "{} {}",
            "Hash:".dimmed(),
            hex::encode(hash).white()
        ));
    }
    Ok(())
}

async fn handle_simulate(args: SimulateArgs) -> Result<()> {
    let network = resolve_network(args.input.network.as_deref())?;
    let passphrase = config::get_network_passphrase(&network);
    let input = tx_xdr::read_payload(
        args.input.xdr.as_deref(),
        args.input.file.as_deref(),
        "xdr",
        "simulate",
    )?;
    // Validate locally so a malformed blob never reaches the RPC.
    let envelope = tx_xdr::parse_envelope(&input)?;
    let xdr = tx_xdr::write_envelope(&envelope, tx_xdr::WireFormat::Base64)?;

    if !args.json {
        p::header("Simulate Transaction");
        p::info("Simulating against Soroban RPC…");
    }
    report_envelope(&envelope, &network, &passphrase);

    let result = crate::utils::soroban::simulate_envelope(&xdr, &network).await?;

    if args.json {
        emit_payload(&serde_json::to_string_pretty(&result)?)?;
    }

    if let Some(error) = result.get("error").and_then(|v| v.as_str()) {
        if !args.json {
            p::error("Simulation failed");
        }
        anyhow::bail!("Simulation reported an error: {error}");
    }

    if args.json {
        return Ok(());
    }

    p::separator();
    if let Some(fee) = result.get("minResourceFee") {
        p::kv("Resource fee (stroops)", &fee.to_string());
    }
    if let Some(cost) = result.get("cost") {
        p::kv("Cost", &cost.to_string());
    }
    if let Some(ledger) = result.get("latestLedger") {
        p::kv("Latest ledger", &ledger.to_string());
    }
    if let Some(results) = result.get("results").and_then(|v| v.as_array()) {
        p::kv("Host function results", &results.len().to_string());
        for (index, entry) in results.iter().enumerate() {
            if let Some(xdr) = entry.get("xdr").and_then(|v| v.as_str()) {
                p::kv(&format!("Result {index}"), xdr);
            }
        }
    }
    if let Some(events) = result.get("events").and_then(|v| v.as_array()) {
        p::kv("Events", &events.len().to_string());
    }
    if let Some(preamble) = result.get("restorePreamble") {
        p::warn("Transaction needs state restored before submission:");
        println!("{}", preamble.to_string().yellow());
    }
    p::separator();
    Ok(())
}

async fn handle_submit(args: SubmitArgs) -> Result<()> {
    // `--json` is the scripted mode of a pipeline: stdout carries only the
    // result and nothing waits for a keystroke, so the caller has to opt into
    // the submission explicitly.
    if args.json && !args.yes {
        anyhow::bail!(
            "--json is for scripted pipelines and prints no prompt; pass --yes to submit."
        );
    }

    let network = resolve_network(args.input.network.as_deref())?;
    let passphrase = config::get_network_passphrase(&network);
    let input = tx_xdr::read_payload(
        args.input.xdr.as_deref(),
        args.input.file.as_deref(),
        "xdr",
        "submit",
    )?;
    let envelope = tx_xdr::parse_envelope(&input)?;
    let summary = tx_xdr::summarize(&envelope);
    let xdr = tx_xdr::write_envelope(&envelope, tx_xdr::WireFormat::Base64)?;

    if !args.json {
        p::header("Submit Transaction Envelope");
    }
    report_envelope(&envelope, &network, &passphrase);

    if summary.signature_hints.is_empty() {
        note(&format!(
            "{}",
            "This envelope carries no signatures; the network will reject it.".yellow()
        ));
    }
    if network == "mainnet" {
        note(&format!(
            "{}",
            "You are submitting on MAINNET. This will cost real XLM."
                .red()
                .bold()
        ));
    }

    if args.json {
        // `--json` never prompts, so it inherits the guardrail
        // `confirm_operation` enforces for a destructive action: a mainnet
        // submission still needs the explicit unsafe opt-in.
        if network == "mainnet" && !confirmation::unsafe_skip_confirmation_enabled() {
            anyhow::bail!(
                "Refusing to skip confirmation for destructive action without explicit unsafe opt-in. \
                 Set {}=1 only in controlled automation (see docs/CONFIRMATION_UX.md), \
                 or submit without --json.",
                confirmation::ENV_UNSAFE_SKIP_CONFIRMATION
            );
        }
    } else {
        let risk_level = if network == "mainnet" {
            confirmation::RiskLevel::High
        } else {
            confirmation::RiskLevel::Medium
        };
        let mut summary_builder = confirmation::OperationSummary::new(
            "Submit raw transaction envelope".to_string(),
            network.clone(),
            risk_level,
        )
        .add("Source", &summary.source_account)
        .add("Sequence", &summary.seq_num)
        .add("Fee (stroops)", summary.fee_stroops.to_string())
        .add("Operations", summary.operations.join(", "));
        if let Ok(hash) = tx_xdr::signature_hash(&envelope, &passphrase) {
            summary_builder = summary_builder.add("Hash", hex::encode(hash));
        }

        let confirm_config = confirmation::ConfirmationConfig {
            risk_level,
            network: network.clone(),
            skip_confirm: args.yes,
            dry_run: false,
            prompt: Some("Submit this transaction?".to_string()),
            require_type_confirmation: network == "mainnet",
            destructive_action: if network == "mainnet" {
                Some(confirmation::DestructiveAction::MainnetTransaction)
            } else {
                None
            },
            challenge_phrase: None,
        };

        if !confirmation::confirm_operation(&summary_builder, &confirm_config)? {
            return Ok(());
        }
    }

    crate::utils::network_guard::verify(&network).await?;
    if !args.json {
        p::info("Submitting envelope to Horizon…");
    }
    let outcome = horizon::submit_envelope(&xdr, &network).await?;

    if args.json {
        let payload = serde_json::json!({
            "hash": outcome.hash,
            "ledger": outcome.ledger,
            "successful": outcome.successful,
            "result_xdr": outcome.result_xdr,
        });
        return emit_payload(&serde_json::to_string_pretty(&payload)?);
    }

    println!();
    p::separator();
    println!(
        "  {} {}",
        "✓".green().bold(),
        "Transaction submitted successfully!".bright_white()
    );
    println!();
    p::kv_accent("Transaction Hash", &outcome.hash);
    if let Some(ledger) = outcome.ledger {
        p::kv("Ledger", &ledger.to_string());
    }
    p::kv(
        "Stellar Expert",
        &format!("{}/{}", explorer_base(&network), outcome.hash),
    );
    p::separator();
    Ok(())
}

fn explorer_base(network: &str) -> &'static str {
    if network == "mainnet" {
        "https://stellar.expert/explorer/public/tx"
    } else {
        "https://stellar.expert/explorer/testnet/tx"
    }
}

async fn handle_batch(args: BatchArgs) -> Result<()> {
    p::header("Batch Stellar Transaction");

    config::validate_wallet_name(&args.from)?;
    config::validate_network(&args.network)?;

    let doc = tx_batch::load_batch_file(&args.file)?;
    tx_batch::validate_batch_operations(&doc.operations)?;

    let cfg = config::load()?;
    let wallet = cfg
        .wallets
        .iter()
        .find(|w| w.name == args.from)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Wallet '{}' not found. Run `starforge wallet list`",
                args.from
            )
        })?;

    if wallet.secret_key.is_none() && args.hardware.is_none() {
        anyhow::bail!(
            "Wallet '{}' has no secret key stored. Use --hardware ledger or --hardware trezor.",
            args.from
        );
    }

    let payment_ops: Vec<horizon::BatchPaymentOp> = doc
        .operations
        .iter()
        .map(batch_operation_to_payment)
        .collect::<Result<Vec<_>>>()?;

    p::separator();
    p::kv("From Wallet", &wallet.name);
    p::kv("From Address", &wallet.public_key);
    p::kv("Operations", &doc.operations.len().to_string());
    p::kv("Batch File", &args.file.display().to_string());
    p::kv("Network", &args.network);

    if args.network == "mainnet" {
        p::warn("You are submitting on MAINNET. This will cost real XLM.");
    }

    for (i, op) in payment_ops.iter().enumerate() {
        let asset_label = match (&op.asset_code, &op.asset_issuer) {
            (None, None) => "XLM".to_string(),
            (Some(code), Some(issuer)) => format!("{}:{}", code, issuer),
            _ => "unknown".to_string(),
        };
        p::kv(
            &format!("Op {}", i + 1),
            &format!("payment → {} {} {}", op.destination, op.amount, asset_label),
        );
    }

    p::separator();

    println!();
    p::step(1, 2, "Fetching source account info…");
    let source_account = horizon::fetch_account(&wallet.public_key, &args.network)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Source account not found on {}: {}\nFund it with: starforge wallet fund {}",
                args.network,
                e,
                wallet.name
            )
        })?;

    p::step(2, 2, "Building batch transaction…");
    let tx_result = horizon::build_and_simulate_batch(
        &wallet.public_key,
        &payment_ops,
        &source_account.sequence,
        &args.network,
    )?;

    p::kv(
        "Estimated Fee",
        &format!("{:.7} XLM", tx_result.fee as f64 / 10_000_000.0),
    );
    p::kv(
        "Transaction XDR",
        &format!(
            "{}...",
            &tx_result.transaction_xdr[..tx_result.transaction_xdr.len().min(20)]
        ),
    );

    // Build operation summary for confirmation
    let risk_level = if args.network == "mainnet" {
        confirmation::RiskLevel::High
    } else {
        confirmation::RiskLevel::Medium
    };

    let mut summary = confirmation::OperationSummary::new(
        "Batch Stellar Transaction".to_string(),
        args.network.clone(),
        risk_level,
    )
    .add("From Wallet", &wallet.name)
    .add("From Address", &wallet.public_key)
    .add("Operations", doc.operations.len().to_string())
    .add("Batch File", args.file.display().to_string())
    .add(
        "Estimated Fee",
        format!("{:.7} XLM", tx_result.fee as f64 / 10_000_000.0),
    );

    // Add operation details to summary
    for (i, op) in payment_ops.iter().enumerate() {
        let asset_label = match (&op.asset_code, &op.asset_issuer) {
            (None, None) => "XLM".to_string(),
            (Some(code), Some(issuer)) => format!("{}:{}", code, issuer),
            _ => "unknown".to_string(),
        };
        summary = summary.add(
            format!("Op {}", i + 1),
            format!("payment → {} {} {}", op.destination, op.amount, asset_label),
        );
    }

    let confirm_config = confirmation::ConfirmationConfig {
        risk_level,
        network: args.network.clone(),
        skip_confirm: args.yes,
        dry_run: false,
        prompt: Some("Proceed with batch transaction?".to_string()),
        require_type_confirmation: args.network == "mainnet",
        destructive_action: if args.network == "mainnet" {
            Some(confirmation::DestructiveAction::MainnetTransaction)
        } else {
            None
        },
        challenge_phrase: None,
    };

    if !confirmation::confirm_operation(&summary, &confirm_config)? {
        return Ok(());
    }

    println!();

    let signing_request = wallet_signer::SigningRequest::from_options(
        Some(wallet),
        args.hardware,
        Some(&args.hd_path),
        &args.network,
        args.yes,
        "batch transaction",
    )?
    .with_fee_stroops(tx_result.fee)
    .for_non_contract();

    p::info("Submitting batch transaction…");
    let submit_result = horizon::submit_payment_with_signing(
        &tx_result.transaction_xdr,
        &signing_request,
        &args.network,
    )
    .await?;

    println!();
    p::separator();
    println!(
        "  {} {}",
        "✓".green().bold(),
        "Batch transaction submitted successfully!".bright_white()
    );
    println!();
    p::kv_accent("Transaction Hash", &submit_result.hash);

    let explorer_base = if args.network == "mainnet" {
        "https://stellar.expert/explorer/public/tx"
    } else {
        "https://stellar.expert/explorer/testnet/tx"
    };

    p::kv(
        "Stellar Expert",
        &format!("{}/{}", explorer_base, submit_result.hash),
    );
    p::separator();

    Ok(())
}

fn batch_operation_to_payment(op: &tx_batch::BatchOperation) -> Result<horizon::BatchPaymentOp> {
    match op {
        tx_batch::BatchOperation::Payment { to, amount, asset } => {
            let (asset_code, asset_issuer) = parse_asset(asset)?;
            Ok(horizon::BatchPaymentOp {
                destination: to.clone(),
                amount: amount.clone(),
                asset_code,
                asset_issuer,
            })
        }
    }
}

/// Convert the validated decimal amount to stroops without going through a
/// lossy `f64` round-trip where possible.
fn amount_f64_to_stroops(amount: &f64) -> Result<i64> {
    let stroops = (amount * 10_000_000.0).round();
    if !stroops.is_finite() || stroops < 0.0 {
        anyhow::bail!("amount '{}' is not a valid transfer amount", amount);
    }
    if stroops > i64::MAX as f64 {
        anyhow::bail!("amount '{}' exceeds the maximum i64 stroop value", amount);
    }
    Ok(stroops as i64)
}

async fn handle_send(args: SendArgs) -> Result<()> {
    p::header("Send Stellar Payment");

    config::validate_wallet_name(&args.from)?;
    config::validate_public_key(&args.to)?;
    config::validate_network(&args.network)?;
    config::validate_amount(&args.amount)?;

    // Load configuration and find wallet
    let cfg = config::load()?;
    let wallet = cfg
        .wallets
        .iter()
        .find(|w| w.name == args.from)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Wallet '{}' not found. Run `starforge wallet list`",
                args.from
            )
        })?;

    // Validate wallet has secret key
    if wallet.secret_key.is_none() && args.hardware.is_none() {
        anyhow::bail!(
            "Wallet '{}' has no secret key stored. Use --hardware ledger or --hardware trezor.",
            args.from
        );
    }

    // Parse asset
    let (asset_code, asset_issuer) = parse_asset(&args.asset)?;

    // Validate amount
    let amount_f64 = config::validate_amount(&args.amount)?;

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
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Source account not found on {}: {}\nFund it with: starforge wallet fund {}",
                args.network,
                e,
                wallet.name
            )
        })?;

    // Check balance if sending XLM
    if asset_code.is_none() {
        let xlm_balance = source_account
            .balances
            .iter()
            .find(|b| b.asset_type == "native")
            .map(|b| b.balance.parse::<f64>().unwrap_or(0.0))
            .unwrap_or(0.0);

        p::kv("XLM Balance", &format!("{:.7} XLM", xlm_balance));

        if xlm_balance < amount_f64 + 0.00001 {
            // Reserve for fees
            anyhow::bail!(
                "Insufficient XLM balance. Have: {:.7}, Need: {:.7} + fees",
                xlm_balance,
                amount_f64
            );
        }
    }

    // Step 2: Validate destination account
    p::step(2, 3, "Validating destination account…");
    match horizon::fetch_account(&args.to, &args.network).await {
        Ok(_) => p::kv("Destination", "✓ Account exists"),
        Err(_) => {
            if asset_code.is_none() {
                p::kv(
                    "Destination",
                    "⚠ Account will be created (requires 1 XLM minimum)",
                );
                if amount_f64 < 1.0 {
                    anyhow::bail!("Destination account doesn't exist. Minimum 1 XLM required to create account.");
                }
            } else {
                anyhow::bail!("Destination account doesn't exist and cannot be created with non-native assets");
            }
        }
    }

    // Step 3: Build the real payment envelope
    p::step(3, 3, "Building transaction…");
    let base_fee = fee_payer::resolve_base_fee(&args.network).await;

    let amount_stroops = amount_f64_to_stroops(&amount_f64)?;
    let payment_request = tx_builder::PaymentRequest {
        source: wallet.public_key.clone(),
        destination: args.to.clone(),
        amount: amount_stroops,
        asset: match (&asset_code, &asset_issuer) {
            (Some(code), Some(issuer)) => Some((code.clone(), issuer.clone())),
            _ => None,
        },
        sequence: source_account.sequence.trim().parse::<i64>().map_err(|_| {
            anyhow::anyhow!(
                "Horizon returned an unreadable sequence for {}: '{}'",
                wallet.public_key,
                source_account.sequence
            )
        })?,
        base_fee,
    };
    let plain_envelope = tx_builder::build_payment(&payment_request)?;

    // Sign the inner payment as the sender, then optionally wrap it in a fee
    // bump paid by a different wallet. Both layers are verified before this
    // returns, so nothing unverifiable can reach Horizon.
    let signed = fee_payer::sign_and_wrap(
        plain_envelope,
        &fee_payer::WrapOptions {
            network: &args.network,
            base_fee,
            source_wallet: wallet,
            hardware: args.hardware,
            hd_path: &args.hd_path,
            fee_payer: args.fee_payer.as_deref(),
        },
    )?;
    let breakdown = signed.breakdown.clone();
    let transaction_xdr = signed.xdr().to_string();

    if args.fee_payer.is_some() {
        p::header("Who Pays What");
        for (key, value) in breakdown.rows() {
            p::kv(key, &value);
        }
        p::separator();
    }

    p::kv("Network Fee", &tx_builder::stroops_xlm(breakdown.outer_fee));
    p::kv("Fee Payer", breakdown.fee_payer.as_str());
    p::kv(
        "Transaction XDR",
        &format!("{}...", &transaction_xdr[..transaction_xdr.len().min(20)]),
    );

    // Build operation summary for confirmation
    let risk_level = if args.network == "mainnet" {
        confirmation::RiskLevel::High
    } else {
        confirmation::RiskLevel::Medium
    };

    let mut summary = confirmation::OperationSummary::new(
        "Send Stellar Payment".to_string(),
        args.network.clone(),
        risk_level,
    )
    .add("From Wallet", &wallet.name)
    .add("From Address", &wallet.public_key)
    .add("To Address", &args.to)
    .add("Amount", format!("{} {}", args.amount, args.asset))
    .add("Network Fee", tx_builder::stroops_xlm(breakdown.outer_fee))
    .add("Fee Payer", breakdown.fee_payer.as_str());
    if let Some(payer) = &args.fee_payer {
        summary = summary.add("Fee Bump Signed By", payer);
    }

    let confirm_config = confirmation::ConfirmationConfig {
        risk_level,
        network: args.network.clone(),
        skip_confirm: args.yes,
        dry_run: false,
        prompt: Some("Proceed with payment?".to_string()),
        require_type_confirmation: args.network == "mainnet",
        destructive_action: if args.network == "mainnet" {
            Some(confirmation::DestructiveAction::MainnetTransaction)
        } else {
            None
        },
        challenge_phrase: None,
    };

    if !confirmation::confirm_operation(&summary, &confirm_config)? {
        return Ok(());
    }

    // Submit the already-signed envelope. Both the inner payment and the fee
    // bump were signed before confirmation so the preview showed a real,
    // verified fee split rather than an estimate.
    println!();

    p::info("Submitting transaction…");
    let submit_result = horizon::submit_signed_envelope(&transaction_xdr, &args.network).await?;

    println!();
    p::separator();
    println!(
        "  {} {}",
        "✓".green().bold(),
        "Payment sent successfully!".bright_white()
    );
    println!();
    p::kv_accent("Transaction Hash", &submit_result.hash);

    let explorer_base = if args.network == "mainnet" {
        "https://stellar.expert/explorer/public/tx"
    } else {
        "https://stellar.expert/explorer/testnet/tx"
    };

    p::kv(
        "Stellar Expert",
        &format!("{}/{}", explorer_base, submit_result.hash),
    );
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

async fn handle_history(args: HistoryArgs) -> Result<()> {
    let limit = args.limit.min(200);

    config::validate_public_key(&args.public_key)?;

    let network = args.network_override.unwrap_or_else(|| {
        config::load()
            .map(|c| c.network)
            .unwrap_or_else(|_| "testnet".to_string())
    });
    config::validate_network(&network)?;

    println!();
    println!(
        "  {} {}",
        "◆".cyan().bold(),
        "Transaction History".white().bold()
    );
    println!("  {} {}", "Account :".dimmed(), args.public_key.yellow());
    println!("  {} {}", "Network :".dimmed(), network.cyan());
    println!(
        "  {} {}",
        "Showing :".dimmed(),
        format!("up to {} txs", limit).white()
    );

    if args.after.is_some() || args.before.is_some() {
        let range = format!(
            "{} → {}",
            args.after.as_deref().unwrap_or("*"),
            args.before.as_deref().unwrap_or("*")
        );
        println!("  {} {}", "Range   :".dimmed(), range.white());
    }
    if args.successful_only {
        println!("  {} {}", "Filter  :".dimmed(), "successful only".white());
    }
    if args.cursor.is_some() {
        println!(
            "  {} {}",
            "Cursor  :".dimmed(),
            "paginating from cursor".white()
        );
    }

    println!("  {}", "─".repeat(72).dimmed());

    let filter = horizon::TxFilter {
        limit,
        cursor: args.cursor,
        after: args.after,
        before: args.before,
        order: None,
        type_filter: None,
        successful_only: if args.successful_only {
            Some(true)
        } else {
            None
        },
    };

    match horizon::fetch_transactions_filtered(&args.public_key, &network, filter).await {
        Err(e) => {
            println!("\n  {} {}\n", "✗".red().bold(), e.to_string().red());
        }
        Ok(txs) if txs.is_empty() => {
            println!(
                "\n  {} No transactions found for this account.\n",
                "!".yellow().bold()
            );
        }
        Ok(txs) => {
            print_transactions(&txs, &network, args.details);
        }
    }

    Ok(())
}

async fn handle_fees(network_opt: Option<String>) -> Result<()> {
    // Determine the network, default to config or testnet
    let network = match network_opt {
        Some(net) => net,
        None => config::load()?.network,
    };
    config::validate_network(&network)?;
    let stats: FeeStats = horizon::fetch_fee_stats(&network).await?;
    p::header("Recommended Fee Levels");
    p::kv("Network", &network);
    p::kv("Low Fee (stroops)", &stats.low_fee);
    p::kv("Medium Fee (stroops)", &stats.mode_fee);
    p::kv("High Fee (stroops)", &stats.high_fee);
    Ok(())
}

fn decode_memo(memo_type: Option<&str>, memo: Option<&str>) -> String {
    match (memo_type, memo) {
        (Some("text"), Some(m)) => format!("\"{}\"", m),
        (Some("id"), Some(m)) => format!("id:{}", m),
        (Some("hash"), Some(m)) => format!("hash:{}", &m[..m.len().min(16)]),
        (Some("return"), Some(m)) => format!("return:{}", &m[..m.len().min(16)]),
        (Some("none"), _) | (None, _) => "none".to_string(),
        _ => "—".to_string(),
    }
}

fn print_transactions(txs: &[horizon::TransactionRecord], network: &str, details: bool) {
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

        if details {
            if let Some(ref src) = tx.source_account {
                println!(
                    "  {:<14}  {}",
                    "".dimmed(),
                    format!("src: {}…", &src[..16]).dimmed()
                );
            }
            let memo = decode_memo(tx.memo_type.as_deref(), tx.memo.as_deref());
            println!(
                "  {:<14}  {}",
                "".dimmed(),
                format!("memo: {}", memo).dimmed()
            );
        }
    }

    println!("  {}", "─".repeat(72).dimmed());

    // Pagination hint
    if let Some(last) = txs.last() {
        if let Some(ref token) = last.paging_token {
            println!(
                "\n  {} {}",
                "Next page:".dimmed(),
                format!("--cursor {}", token).cyan()
            );
        }
    }

    // Explorer deep link
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
