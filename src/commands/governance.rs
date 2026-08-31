use crate::utils::governance::{self, DashboardSummary, GovernanceProposal, VoteChoice};
use crate::utils::{config, confirmation, horizon, print as p};
use anyhow::Result;
use clap::{Args, Subcommand};
use colored::*;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum GovernanceCommands {
    /// Create a contract upgrade governance proposal
    Propose(ProposeArgs),
    /// List governance proposals
    List(ListArgs),
    /// Show details of a governance proposal
    Show(ShowArgs),
    /// Cast a vote on an active proposal
    Vote(VoteArgs),
    /// Reject a governance proposal
    Reject(RejectArgs),
    /// Execute a timelock-ready proposal
    Execute(ExecuteArgs),
    /// Emergency upgrade (bypasses timelock, requires guardian authorization)
    Emergency(EmergencyArgs),
    /// Show governance audit trail
    Audit(AuditArgs),
    /// Governance dashboard summary
    Dashboard(DashboardArgs),
    /// View or update governance configuration
    #[command(subcommand)]
    Config(ConfigCommands),
}

#[derive(Args)]
pub struct ProposeArgs {
    /// Contract ID to upgrade
    #[arg(long)]
    pub contract_id: String,
    /// Path to the new compiled .wasm file
    #[arg(long)]
    pub wasm: PathBuf,
    /// Human-readable description of the upgrade
    #[arg(long)]
    pub description: String,
    /// Wallet name to use for signing
    #[arg(long)]
    pub wallet: Option<String>,
    /// Network to use
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
    /// Number of votes required to pass
    #[arg(long)]
    pub threshold: Option<u8>,
    /// Timelock delay in seconds before execution is allowed
    #[arg(long)]
    pub timelock: Option<u64>,
}

#[derive(Args)]
pub struct ListArgs {
    /// Filter by contract ID
    #[arg(long)]
    pub contract_id: Option<String>,
    /// Filter by status
    #[arg(long)]
    pub status: Option<String>,
    /// Network to use
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct ShowArgs {
    /// Proposal ID
    #[arg(long)]
    pub proposal_id: String,
    /// Network to use
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
}

#[derive(Args)]
pub struct VoteArgs {
    /// Proposal ID
    #[arg(long)]
    pub proposal_id: String,
    /// Vote for the proposal
    #[arg(long, group = "choice")]
    pub r#for: bool,
    /// Vote against the proposal
    #[arg(long, group = "choice")]
    pub against: bool,
    /// Wallet name to use
    #[arg(long)]
    pub wallet: Option<String>,
    /// Network to use
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
}

#[derive(Args)]
pub struct RejectArgs {
    /// Proposal ID
    #[arg(long)]
    pub proposal_id: String,
    /// Optional rejection reason
    #[arg(long)]
    pub reason: Option<String>,
    /// Wallet name to use
    #[arg(long)]
    pub wallet: Option<String>,
    /// Network to use
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
}

#[derive(Args)]
pub struct ExecuteArgs {
    /// Proposal ID
    #[arg(long)]
    pub proposal_id: String,
    /// Wallet name to use
    #[arg(long)]
    pub wallet: Option<String>,
    /// Network to use
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
    /// Skip confirmation prompt
    #[arg(long, default_value = "false")]
    pub yes: bool,
}

#[derive(Args)]
pub struct EmergencyArgs {
    /// Contract ID to upgrade
    #[arg(long)]
    pub contract_id: String,
    /// Path to the new compiled .wasm file
    #[arg(long)]
    pub wasm: PathBuf,
    /// Human-readable description
    #[arg(long)]
    pub description: String,
    /// Guardian wallet name
    #[arg(long)]
    pub wallet: Option<String>,
    /// Network to use
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
    /// Skip confirmation prompt
    #[arg(long, default_value = "false")]
    pub yes: bool,
}

#[derive(Args)]
pub struct AuditArgs {
    /// Filter by proposal ID
    #[arg(long)]
    pub proposal_id: Option<String>,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct DashboardArgs {
    /// Network to filter by
    #[arg(long)]
    pub network: Option<String>,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Show current governance configuration
    Show,
    /// Update governance configuration
    Set(SetConfigArgs),
}

#[derive(Args)]
pub struct SetConfigArgs {
    /// Default timelock delay in seconds
    #[arg(long)]
    pub timelock: Option<u64>,
    /// Default approval threshold
    #[arg(long)]
    pub threshold: Option<u8>,
    /// Emergency quorum (guardian votes required)
    #[arg(long)]
    pub emergency_quorum: Option<u8>,
    /// Add an emergency guardian public key
    #[arg(long)]
    pub guardian: Option<String>,
}

pub async fn handle(cmd: GovernanceCommands) -> Result<()> {
    match cmd {
        GovernanceCommands::Propose(args) => handle_propose(args),
        GovernanceCommands::List(args) => handle_list(args),
        GovernanceCommands::Show(args) => handle_show(args),
        GovernanceCommands::Vote(args) => handle_vote(args),
        GovernanceCommands::Reject(args) => handle_reject(args),
        GovernanceCommands::Execute(args) => handle_execute(args).await,
        GovernanceCommands::Emergency(args) => handle_emergency(args),
        GovernanceCommands::Audit(args) => handle_audit(args),
        GovernanceCommands::Dashboard(args) => handle_dashboard(args),
        GovernanceCommands::Config(sub) => handle_config(sub),
    }
}

fn handle_propose(args: ProposeArgs) -> Result<()> {
    p::header("Create Governance Proposal");

    let cfg = config::load()?;
    let wallet = resolve_wallet(&cfg, args.wallet.as_deref())?;

    let proposal = governance::create_proposal(
        args.contract_id.clone(),
        args.wasm,
        args.description.clone(),
        wallet.public_key.clone(),
        args.network.clone(),
        args.threshold,
        args.timelock,
    )?;

    print_proposal_summary(&proposal);
    p::info(&format!(
        "Vote with: starforge governance vote --proposal-id {} --for",
        proposal.id
    ));
    Ok(())
}

fn handle_list(args: ListArgs) -> Result<()> {
    p::header("Governance Proposals");

    let proposals = governance::list_proposals(
        Some(&args.network),
        args.contract_id.as_deref(),
        args.status.as_deref(),
    )?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&proposals)?);
        return Ok(());
    }

    if proposals.is_empty() {
        p::info("No governance proposals found.");
        return Ok(());
    }

    p::separator();
    println!(
        "  {:<18}  {:<14}  {:<16}  {:<8}  {}",
        "Proposal ID".dimmed(),
        "Contract".dimmed(),
        "Status".dimmed(),
        "Votes".dimmed(),
        "Created".dimmed(),
    );
    println!("  {}", "─".repeat(72).dimmed());

    for prop in &proposals {
        let votes = format!(
            "{}/{}",
            governance::votes_for(prop),
            prop.approval_threshold
        );
        let created = prop.created_at.get(..10).unwrap_or(&prop.created_at);
        let status_colored = color_status(&prop.status.to_string());
        println!(
            "  {:<18}  {:<14}  {:<16}  {:<8}  {}",
            prop.id.white(),
            short_id(&prop.contract_id).cyan(),
            status_colored,
            votes.white(),
            created.dimmed(),
        );
    }
    p::separator();
    Ok(())
}

fn handle_show(args: ShowArgs) -> Result<()> {
    p::header("Governance Proposal Details");
    let proposal = governance::get_proposal(&args.proposal_id, &args.network)?;
    print_proposal_detail(&proposal);
    Ok(())
}

fn handle_vote(args: VoteArgs) -> Result<()> {
    p::header("Cast Governance Vote");

    let choice = if args.r#for {
        VoteChoice::For
    } else if args.against {
        VoteChoice::Against
    } else {
        anyhow::bail!("Specify --for or --against");
    };
    let choice_label = choice.to_string();

    let cfg = config::load()?;
    let wallet = resolve_wallet(&cfg, args.wallet.as_deref())?;

    let proposal = governance::cast_vote(
        &args.proposal_id,
        wallet.public_key.clone(),
        choice,
        &args.network,
    )?;

    p::kv_accent("Proposal", &args.proposal_id);
    p::kv("Vote", &choice_label);
    p::kv(
        "Tally",
        &format!(
            "{} for / {} against (threshold: {})",
            governance::votes_for(&proposal),
            governance::votes_against(&proposal),
            proposal.approval_threshold
        ),
    );
    p::kv("Status", &proposal.status.to_string());

    if proposal.status.to_string() == "passed" {
        if let Some(expires) = &proposal.timelock_expires_at {
            p::info(&format!("Timelock expires at {}", expires));
        }
    }
    Ok(())
}

fn handle_reject(args: RejectArgs) -> Result<()> {
    p::header("Reject Governance Proposal");

    let cfg = config::load()?;
    let wallet = resolve_wallet(&cfg, args.wallet.as_deref())?;

    let proposal = governance::reject_proposal(
        &args.proposal_id,
        &wallet.public_key,
        &args.network,
        args.reason.as_deref(),
    )?;

    p::success(&format!("Proposal '{}' rejected", proposal.id));
    Ok(())
}

async fn handle_execute(args: ExecuteArgs) -> Result<()> {
    p::header("Execute Governance Upgrade");

    let cfg = config::load()?;
    let wallet = resolve_wallet(&cfg, args.wallet.as_deref())?;

    let preview = governance::get_proposal(&args.proposal_id, &args.network)?;

    let risk_level = if args.network == "mainnet" {
        confirmation::RiskLevel::High
    } else {
        confirmation::RiskLevel::Medium
    };

    let summary = confirmation::OperationSummary::new(
        "Execute Governance Upgrade".to_string(),
        args.network.clone(),
        risk_level,
    )
    .add("Proposal ID", &preview.id)
    .add("Contract ID", &preview.contract_id)
    .add("New WASM hash", &preview.new_wasm_hash)
    .add("Network", &preview.network)
    .add("Executor", &wallet.public_key);

    let confirm_config = confirmation::ConfirmationConfig {
        risk_level,
        network: args.network.clone(),
        skip_confirm: args.yes,
        dry_run: false,
        prompt: Some("Execute this governance upgrade?".to_string()),
        require_type_confirmation: args.network == "mainnet",
        ..Default::default()
    };

    if !confirmation::confirm_operation(&summary, &confirm_config)? {
        return Ok(());
    }

    horizon::fetch_account(&wallet.public_key, &args.network)
        .await
        .map_err(|e| anyhow::anyhow!("Account not active on {}: {}", args.network, e))?;

    let proposal =
        governance::execute_proposal(&args.proposal_id, &wallet.public_key, &args.network)?;

    println!();
    p::separator();
    println!(
        "  {} {}",
        "✓".green().bold(),
        "Governance upgrade approved — apply on-chain:".bright_white()
    );
    println!();
    println!(
        "  {}",
        format!(
            "stellar contract upload --wasm <path> --source {} --network {}",
            wallet.public_key, args.network
        )
        .cyan()
    );
    println!(
        "  {}",
        format!(
            "stellar contract invoke --id {} --source {} --network {} -- upgrade --new-wasm-hash {}",
            proposal.contract_id, wallet.public_key, args.network, proposal.new_wasm_hash
        )
        .cyan()
    );
    p::separator();
    Ok(())
}

fn handle_emergency(args: EmergencyArgs) -> Result<()> {
    p::header("Emergency Contract Upgrade");

    let cfg = config::load()?;
    let wallet = resolve_wallet(&cfg, args.wallet.as_deref())?;

    let risk_level = confirmation::RiskLevel::High;
    let summary = confirmation::OperationSummary::new(
        "Emergency Governance Upgrade".to_string(),
        args.network.clone(),
        risk_level,
    )
    .add("Contract ID", &args.contract_id)
    .add("Description", &args.description)
    .add("Guardian", &wallet.public_key)
    .add("Network", &args.network);

    let confirm_config = confirmation::ConfirmationConfig {
        risk_level,
        network: args.network.clone(),
        skip_confirm: args.yes,
        dry_run: false,
        prompt: Some("Proceed with emergency upgrade?".to_string()),
        require_type_confirmation: true,
        ..Default::default()
    };

    if !confirmation::confirm_operation(&summary, &confirm_config)? {
        return Ok(());
    }

    let proposal = governance::emergency_upgrade(
        args.contract_id.clone(),
        args.wasm,
        args.description,
        wallet.public_key.clone(),
        args.network,
    )?;

    p::warn("Emergency upgrade initiated — timelock bypassed");
    print_proposal_summary(&proposal);
    Ok(())
}

fn handle_audit(args: AuditArgs) -> Result<()> {
    p::header("Governance Audit Trail");

    let entries = if let Some(id) = &args.proposal_id {
        governance::audit_for_proposal(id)?
    } else {
        governance::load_audit_log()?
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(());
    }

    if entries.is_empty() {
        p::info("No governance audit entries found.");
        return Ok(());
    }

    p::separator();
    println!(
        "  {:<22}  {:<14}  {:<12}  {}",
        "Timestamp".dimmed(),
        "Action".dimmed(),
        "Actor".dimmed(),
        "Proposal".dimmed(),
    );
    println!("  {}", "─".repeat(72).dimmed());

    for entry in &entries {
        let ts = entry.timestamp.get(..19).unwrap_or(&entry.timestamp);
        println!(
            "  {:<22}  {:<14}  {:<12}  {}",
            ts.dimmed(),
            entry.action.cyan(),
            short_id(&entry.actor).white(),
            entry.proposal_id.white(),
        );
    }
    p::separator();
    Ok(())
}

fn handle_dashboard(args: DashboardArgs) -> Result<()> {
    p::header("Governance Dashboard");

    let summary = governance::dashboard(args.network.as_deref())?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
        return Ok(());
    }

    print_dashboard(&summary);
    Ok(())
}

fn handle_config(cmd: ConfigCommands) -> Result<()> {
    match cmd {
        ConfigCommands::Show => {
            p::header("Governance Configuration");
            let cfg = governance::load_config()?;
            p::kv(
                "Default timelock (seconds)",
                &cfg.default_timelock_seconds.to_string(),
            );
            p::kv(
                "Default threshold",
                &cfg.default_approval_threshold.to_string(),
            );
            p::kv("Emergency quorum", &cfg.emergency_quorum.to_string());
            if cfg.emergency_guardians.is_empty() {
                p::kv("Emergency guardians", "(none configured)");
            } else {
                for (i, g) in cfg.emergency_guardians.iter().enumerate() {
                    p::kv(&format!("Guardian {}", i + 1), g);
                }
            }
            Ok(())
        }
        ConfigCommands::Set(args) => {
            p::header("Update Governance Configuration");
            let mut cfg = governance::load_config()?;

            if let Some(timelock) = args.timelock {
                cfg.default_timelock_seconds = timelock;
            }
            if let Some(threshold) = args.threshold {
                cfg.default_approval_threshold = threshold;
            }
            if let Some(quorum) = args.emergency_quorum {
                cfg.emergency_quorum = quorum;
            }
            if let Some(guardian) = args.guardian {
                config::validate_public_key(&guardian)?;
                if !cfg.emergency_guardians.contains(&guardian) {
                    cfg.emergency_guardians.push(guardian);
                }
            }

            governance::save_config(&cfg)?;
            p::success("Governance configuration updated");
            handle_config(ConfigCommands::Show)
        }
    }
}

fn print_proposal_summary(proposal: &GovernanceProposal) {
    p::separator();
    p::kv_accent("Proposal ID", &proposal.id);
    p::kv("Contract ID", &proposal.contract_id);
    p::kv("New WASM hash", &proposal.new_wasm_hash);
    p::kv("Description", &proposal.description);
    p::kv("Threshold", &proposal.approval_threshold.to_string());
    p::kv("Timelock (seconds)", &proposal.timelock_seconds.to_string());
    p::kv("Status", &proposal.status.to_string());
    p::separator();
}

fn print_proposal_detail(proposal: &GovernanceProposal) {
    print_proposal_summary(proposal);
    p::kv("Proposer", &proposal.proposer);
    p::kv("Network", &proposal.network);
    p::kv("Created", &proposal.created_at);
    if let Some(expires) = &proposal.timelock_expires_at {
        p::kv("Timelock expires", expires);
        if let Some(remaining) = governance::timelock_remaining(proposal) {
            p::kv(
                "Time remaining",
                &format!("{} hours", remaining.num_hours()),
            );
        }
    }
    if proposal.is_emergency {
        p::kv("Emergency", "yes");
    }
    if !proposal.votes.is_empty() {
        println!();
        println!("  {}", "Votes:".bright_white());
        for vote in &proposal.votes {
            println!(
                "    {} {} ({})",
                short_id(&vote.voter).cyan(),
                vote.choice.to_string().white(),
                vote.voted_at.dimmed()
            );
        }
    }
}

fn print_dashboard(summary: &DashboardSummary) {
    p::separator();
    p::kv("Total proposals", &summary.total_proposals.to_string());
    p::kv("Active", &summary.active.to_string());
    p::kv("Passed (timelock)", &summary.passed.to_string());
    p::kv("Timelock ready", &summary.timelock_ready.to_string());
    p::kv("Executed", &summary.executed.to_string());
    p::kv("Rejected", &summary.rejected.to_string());
    p::kv(
        "Emergency executed",
        &summary.emergency_executed.to_string(),
    );
    println!();

    if !summary.recent_audit_entries.is_empty() {
        println!("  {}", "Recent audit events:".bright_white());
        for entry in &summary.recent_audit_entries {
            let ts = entry.timestamp.get(..19).unwrap_or(&entry.timestamp);
            println!(
                "    {} {} {} → {}",
                ts.dimmed(),
                entry.action.cyan(),
                short_id(&entry.actor).white(),
                entry.proposal_id.white(),
            );
        }
    }
    p::separator();
}

fn short_id(id: &str) -> String {
    if id.len() <= 12 {
        id.to_string()
    } else {
        format!("{}…", &id[..12])
    }
}

fn color_status(status: &str) -> String {
    match status {
        "active" => status.yellow().to_string(),
        "passed" => status.cyan().to_string(),
        "timelock-ready" => status.green().to_string(),
        "executed" | "emergency-executed" => status.green().bold().to_string(),
        "rejected" => status.red().to_string(),
        _ => status.to_string(),
    }
}

fn resolve_wallet<'a>(
    cfg: &'a config::Config,
    name: Option<&str>,
) -> Result<&'a config::WalletEntry> {
    if let Some(wallet_name) = name {
        cfg.wallets
            .iter()
            .find(|w| w.name == wallet_name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Wallet '{}' not found. Run `starforge wallet list`",
                    wallet_name
                )
            })
    } else if !cfg.wallets.is_empty() {
        p::info(&format!(
            "No --wallet specified. Using: {}",
            cfg.wallets[0].name.cyan()
        ));
        Ok(&cfg.wallets[0])
    } else {
        anyhow::bail!("No wallets found. Create one with `starforge wallet create <name> --fund`")
    }
}
