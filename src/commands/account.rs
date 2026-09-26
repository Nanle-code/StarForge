//! `starforge account` — on-chain account lifecycle with sponsored reserves.
//!
//! Focus is CAP-33 sponsored reserves, which let a sponsor pay a new account's
//! base reserve so a user can onboard without holding any XLM of their own.
//!
//! * `create --sponsor` submits `BeginSponsoringFutureReserves` +
//!   `CreateAccount` atomically in one transaction.
//! * `end-sponsorship` submits `EndSponsoringFutureReserves`, signed by the
//!   sponsored account, to release the sponsor's reserve.
//!
//! Either subcommand accepts `--fee-payer` to have a third wallet pay the
//! network fee through a fee bump, so even the sponsoring transaction can be
//! gasless for the sponsor.

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use serde::Serialize;

use crate::utils::{config, confirmation, fee_payer, horizon, print as p, sponsorship, tx_builder};

/// Base fee used when Horizon cannot be reached; also the protocol floor.
const FALLBACK_BASE_FEE: i64 = tx_builder::MIN_BASE_FEE;

#[derive(Subcommand)]
pub enum AccountCommands {
    /// Create a Stellar account, optionally with a sponsor paying the reserve
    Create(CreateAccountArgs),
    /// End an existing sponsorship (releases the sponsor's reserve)
    EndSponsorship(EndSponsorshipArgs),
}

#[derive(Args)]
pub struct CreateAccountArgs {
    /// Sponsor wallet name that pays the new account's base reserve
    #[arg(long, value_name = "WALLET")]
    pub sponsor: String,

    /// Public key (G...) of the account being created
    #[arg(long, value_name = "G...")]
    pub to: String,

    /// Starting balance in stroops (1 XLM = 10000000). Defaults to one base reserve
    #[arg(long, default_value_t = sponsorship::BASE_RESERVE)]
    pub starting_balance: i64,

    /// Network to use
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet", "futurenet", "local"])]
    pub network: String,

    /// Wallet that pays the network fee via a fee bump (defaults to the sponsor)
    #[arg(long, value_name = "WALLET", conflicts_with = "no_fee_payer")]
    pub fee_payer: Option<String>,

    /// Create the account without a separate fee payer (the sponsor pays the fee)
    #[arg(long)]
    pub no_fee_payer: bool,

    /// Sign with a hardware wallet instead of a local secret key
    #[arg(long, value_enum)]
    pub hardware: Option<crate::utils::hardware_wallet::HardwareWalletKind>,

    /// HD derivation path for hardware wallet signing
    #[arg(long, default_value = crate::utils::hardware_wallet::STELLAR_HD_PATH)]
    pub hd_path: String,

    /// Skip the interactive confirmation
    #[arg(long)]
    pub yes: bool,

    /// Emit machine-readable JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct EndSponsorshipArgs {
    /// Local wallet whose account is ending its sponsorship
    #[arg(long, value_name = "WALLET")]
    pub wallet: String,

    /// Network to use
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet", "futurenet", "local"])]
    pub network: String,

    /// Wallet that pays the network fee via a fee bump
    #[arg(long, value_name = "WALLET")]
    pub fee_payer: Option<String>,

    /// Skip the interactive confirmation
    #[arg(long)]
    pub yes: bool,

    /// Emit machine-readable JSON
    #[arg(long)]
    pub json: bool,
}

/// JSON payload for a completed sponsored account creation.
#[derive(Serialize)]
struct CreateJson {
    contract_version: &'static str,
    network: String,
    sponsor: String,
    account: String,
    starting_balance: String,
    base_reserve: String,
    transaction_hash: String,
    sponsor_pays: bool,
    fee_payer: Option<String>,
    inner_source_fee: String,
    bump_fee: String,
    sponsored: bool,
}

/// JSON payload for a completed end-sponsorship.
#[derive(Serialize)]
struct EndSponsorshipJson {
    contract_version: &'static str,
    network: String,
    account: String,
    transaction_hash: String,
    sponsorship_ended: bool,
    fee_payer: Option<String>,
}

pub async fn handle(cmd: AccountCommands) -> Result<()> {
    match cmd {
        AccountCommands::Create(args) => handle_create(args).await,
        AccountCommands::EndSponsorship(args) => handle_end_sponsorship(args).await,
    }
}

async fn handle_create(args: CreateAccountArgs) -> Result<()> {
    if args.json {
        crate::utils::output::set_json_mode(true);
    }

    config::validate_network(&args.network)?;

    let plan = sponsorship::SponsoredCreationPlan {
        sponsor: args.sponsor.clone(),
        new_account: args.to.clone(),
        starting_balance: args.starting_balance,
        network_passphrase: config::get_network_passphrase(&args.network),
    };
    let (sponsor_account, new_account) = plan.validated()?;

    // Fail early if the destination already exists: CAP-33 creation is for
    // brand-new accounts and an existing one would be a wasted fee.
    if let Ok(_) = horizon::fetch_account(&args.to, &args.network).await {
        bail!(
            "account {} already exists on {}; nothing to sponsor",
            args.to,
            args.network
        );
    }

    let base_fee = fee_payer::resolve_base_fee(&args.network).await;
    let sponsor_wallet = fee_payer::find_wallet(&args.sponsor)?;

    // Sequence numbers must be read fresh or the network returns txBAD_SEQ.
    let sponsor_account_info = horizon::fetch_account(&sponsor_wallet.public_key, &args.network)
        .await
        .with_context_sponsor()?;
    let sequence = parse_sequence(&sponsor_account_info.sequence)?;

    let tx = sponsorship::build_sponsored_creation(
        &sponsor_account,
        &new_account,
        args.starting_balance,
        sequence,
        base_fee,
    )?;
    let envelope = sponsorship::sponsored_creation_envelope(&tx)?;
    let plain = tx_builder::TransactionEnvelope::Tx(envelope.clone());

    // Sign the inner transaction as the sponsor, and only wrap in a fee bump when
    // a *different* wallet is named to pay. Wrapping the sponsor's own
    // transaction in a bump to itself would double the fee for no benefit.
    let fee_payer_name: Option<String> = match (&args.fee_payer, args.no_fee_payer) {
        (_, true) => None,
        (Some(payer), false) => Some(payer.clone()),
        (None, false) => None,
    };
    if let Some(payer) = &fee_payer_name {
        if payer == &args.sponsor {
            anyhow::bail!(
                "--fee-payer '{payer}' is the sponsor, which already signs this transaction. \
                 Omit the flag to let the sponsor pay the fee directly."
            );
        }
    }

    let signed = fee_payer::sign_and_wrap(
        plain,
        &fee_payer::WrapOptions {
            network: &args.network,
            base_fee,
            source_wallet: &sponsor_wallet,
            hardware: args.hardware,
            hd_path: &args.hd_path,
            fee_payer: fee_payer_name.as_deref(),
        },
    )?;
    let breakdown = signed.breakdown.clone();

    if args.json {
        p::separator();
    } else {
        p::header("Create Account (Sponsored Reserves)");
        p::kv("Network", &args.network);
        p::separator();
        for (key, value) in plan.preview_rows(base_fee) {
            p::kv(key, &value);
        }
        p::separator();
        p::header("Who Pays What");
        for (key, value) in breakdown.rows() {
            p::kv(key, &value);
        }
        p::separator();
    }

    if args.network == "mainnet" {
        p::warn("You are creating a sponsored account on MAINNET.");
    }

    if args.dry_run_preview() {
        p::info("Dry run: transaction built but not submitted.");
        return Ok(());
    }

    let risk_level = if args.network == "mainnet" {
        confirmation::RiskLevel::High
    } else {
        confirmation::RiskLevel::Medium
    };
    let mut summary = confirmation::OperationSummary::new(
        "Create Sponsored Account".to_string(),
        args.network.clone(),
        risk_level,
    )
    .add("Sponsor", &args.sponsor)
    .add("Sponsor address", &sponsor_wallet.public_key)
    .add("New account", &args.to)
    .add(
        "Starting balance",
        &tx_builder::stroops_xlm(args.starting_balance),
    )
    .add("Reserve paid by", &args.sponsor)
    .add(
        "Network fee payer",
        fee_payer_name.as_deref().unwrap_or("sponsor"),
    )
    .add("Network fee", &tx_builder::stroops_xlm(breakdown.outer_fee));

    if let Some(wallet_name) = &fee_payer_name {
        if wallet_name != &args.sponsor {
            summary = summary.add("Fee bump signed by", wallet_name);
        }
    }

    let confirm_config = confirmation::ConfirmationConfig {
        risk_level,
        network: args.network.clone(),
        skip_confirm: args.yes,
        dry_run: false,
        prompt: Some("Proceed with sponsored account creation?".to_string()),
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

    fee_payer::preflight(&args.network).await?;

    p::info("Submitting sponsored account creation…");
    // Both layers are already signed and verified; submitting must not re-sign,
    // which would append a duplicate signature and use the wrong key for the
    // outer fee-bump layer.
    let result = horizon::submit_signed_envelope(signed.xdr(), &args.network).await?;

    p::separator();
    p::success("Sponsored account created");
    p::kv_accent("Transaction Hash", &result.hash);
    p::kv("Sponsor", &sponsor_wallet.public_key);
    p::kv("Account", &args.to);
    p::kv(
        "Starting balance",
        &tx_builder::stroops_xlm(args.starting_balance),
    );
    p::separator();
    p::info(
        "The account's base reserve is sponsored. End the sponsorship when it is no longer needed:",
    );
    p::kv(
        "",
        &format!(
            "starforge account end-sponsorship --wallet <wallet> --network {}",
            args.network
        ),
    );

    if args.json {
        let payload = CreateJson {
            contract_version: "1.0.0",
            network: args.network.clone(),
            sponsor: sponsor_wallet.public_key.clone(),
            account: args.to.clone(),
            starting_balance: args.starting_balance.to_string(),
            base_reserve: sponsorship::BASE_RESERVE.to_string(),
            transaction_hash: result.hash.clone(),
            sponsor_pays: true,
            fee_payer: fee_payer_name.clone(),
            inner_source_fee: breakdown.inner_fee.to_string(),
            bump_fee: breakdown.outer_fee.to_string(),
            sponsored: true,
        };
        println!("{}", serde_json::to_string_pretty(&payload)?);
    }

    Ok(())
}

async fn handle_end_sponsorship(args: EndSponsorshipArgs) -> Result<()> {
    if args.json {
        crate::utils::output::set_json_mode(true);
    }

    config::validate_network(&args.network)?;

    let plan = sponsorship::EndSponsoringPlan {
        sponsored_account: args.wallet.clone(),
        network_passphrase: config::get_network_passphrase(&args.network),
    };
    let account = plan.validated()?;
    let wallet = fee_payer::find_wallet(&args.wallet)?;

    let base_fee = fee_payer::resolve_base_fee(&args.network).await;
    let info = horizon::fetch_account(&wallet.public_key, &args.network)
        .await
        .with_context_sponsor()?;
    let sequence = parse_sequence(&info.sequence)?;

    let tx = sponsorship::build_end_sponsoring(&account, sequence, base_fee)?;
    let envelope = sponsorship::sponsored_creation_envelope(&tx)?;
    let plain = tx_builder::TransactionEnvelope::Tx(envelope);

    // The sponsored account signs its own release; an optional third wallet
    // covers the fee.
    let signed = fee_payer::sign_and_wrap(
        plain,
        &fee_payer::WrapOptions {
            network: &args.network,
            base_fee,
            source_wallet: &wallet,
            hardware: None,
            hd_path: crate::utils::hardware_wallet::STELLAR_HD_PATH,
            fee_payer: args.fee_payer.as_deref(),
        },
    )?;
    let breakdown = signed.breakdown.clone();

    if args.json {
        p::separator();
    } else {
        p::header("End Sponsored Reserves");
        p::kv("Network", &args.network);
        p::separator();
        for (key, value) in plan.preview_rows(base_fee) {
            p::kv(key, &value);
        }
        p::separator();
        p::header("Who Pays What");
        for (key, value) in breakdown.rows() {
            p::kv(key, &value);
        }
        p::separator();
    }

    if args.network == "mainnet" {
        p::warn("You are ending a sponsorship on MAINNET.");
    }

    let risk_level = if args.network == "mainnet" {
        confirmation::RiskLevel::High
    } else {
        confirmation::RiskLevel::Medium
    };
    let mut summary = confirmation::OperationSummary::new(
        "End Sponsored Reserves".to_string(),
        args.network.clone(),
        risk_level,
    )
    .add("Account", &wallet.public_key)
    .add(
        "Reserve released",
        &tx_builder::stroops_xlm(sponsorship::BASE_RESERVE),
    )
    .add("Network fee", &tx_builder::stroops_xlm(breakdown.outer_fee));
    if let Some(payer) = &args.fee_payer {
        summary = summary.add("Fee bump signed by", payer);
    }

    let confirm_config = confirmation::ConfirmationConfig {
        risk_level,
        network: args.network.clone(),
        skip_confirm: args.yes,
        dry_run: false,
        prompt: Some("Proceed with ending this sponsorship?".to_string()),
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

    fee_payer::preflight(&args.network).await?;

    p::info("Submitting end-sponsorship…");
    // Already signed and verified; re-signing would append a duplicate.
    let result = horizon::submit_signed_envelope(signed.xdr(), &args.network).await?;

    p::separator();
    p::success("Sponsorship ended");
    p::kv_accent("Transaction Hash", &result.hash);
    p::kv("Account", &wallet.public_key);
    p::separator();
    p::info("The account must now maintain its own base reserve.");

    if args.json {
        let payload = EndSponsorshipJson {
            contract_version: "1.0.0",
            network: args.network.clone(),
            account: wallet.public_key.clone(),
            transaction_hash: result.hash.clone(),
            sponsorship_ended: true,
            fee_payer: args.fee_payer.clone(),
        };
        println!("{}", serde_json::to_string_pretty(&payload)?);
    }

    Ok(())
}

/// Horizon returns `sequence` as a JSON string; a stale or missing sequence is
/// the most common cause of `txBAD_SEQ`, so parse it explicitly and fail loudly
/// rather than defaulting to zero.
fn parse_sequence(raw: &str) -> Result<i64> {
    raw.trim()
        .parse::<i64>()
        .with_context(|| format!("Horizon returned an unreadable sequence number: '{raw}'"))
}

impl CreateAccountArgs {
    /// A sponsored creation is always a preview-only operation when the user
    /// asks for JSON without confirming; kept as a named predicate so the
    /// intent reads clearly at the call site.
    fn dry_run_preview(&self) -> bool {
        self.yes && std::env::var("STARFORGE_DRY_RUN").is_ok()
    }
}

/// Attach a sponsor-specific hint when the sponsor account cannot be loaded.
trait SponsorContext<T> {
    fn with_context_sponsor(self) -> Result<T>;
}

impl<T, E> SponsorContext<T> for Result<T, E>
where
    E: std::fmt::Display,
{
    fn with_context_sponsor(self) -> Result<T> {
        self.map_err(|err| {
            anyhow::anyhow!(
                "could not load the sponsor account from Horizon ({}). \
                 Is the network reachable and the sponsor funded?",
                err
            )
        })
    }
}
