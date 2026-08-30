use crate::utils::{
    audit, config, confirmation, horizon, print as p,
    wasm_hash::{compute_wasm_hash, BuildEnvironment},
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::{Args, Subcommand};
use colored::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// ── CLI definition ────────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum UpgradeCommands {
    /// Prepare and validate a contract upgrade
    Prepare(PrepareArgs),
    /// Automated compatibility checks, migration planning, and rollout helpers
    #[command(subcommand)]
    Auto(crate::commands::upgrade_auto::UpgradeAutoCommands),
    /// Create a governance proposal for a contract upgrade
    Propose(ProposeArgs),
    /// Create an emergency upgrade proposal (bypasses timelock)
    EmergencyPropose(EmergencyProposeArgs),
    /// List pending upgrade proposals
    List(ListArgs),
    /// Show status of upgrade proposals (alias for list)
    Status(ListArgs),
    /// Approve a pending upgrade proposal
    Approve(ApproveArgs),
    /// Manually unlock a proposal that has passed timelock
    Unlock(UnlockArgs),
    /// Execute an approved upgrade proposal
    Execute(ExecuteArgs),
    /// Roll back to a previous contract version
    Rollback(RollbackArgs),
    /// Show upgrade history for a contract
    History(HistoryArgs),
}

#[derive(Args)]
pub struct PrepareArgs {
    /// Contract ID to upgrade
    #[arg(long)]
    pub contract_id: String,
    /// Path to the new compiled .wasm file
    #[arg(long)]
    pub wasm: PathBuf,
    /// Network to use
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
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
    /// Number of approvals required before execution (default: 1)
    #[arg(long, default_value_t = 1)]
    pub threshold: u8,
    /// Timelock duration in seconds (default: 86400 = 24 hours)
    #[arg(long, default_value_t = 86400)]
    pub timelock_duration: u64,
}

#[derive(Args)]
pub struct EmergencyProposeArgs {
    /// Contract ID to upgrade
    #[arg(long)]
    pub contract_id: String,
    /// Path to the new compiled .wasm file
    #[arg(long)]
    pub wasm: PathBuf,
    /// Human-readable description of the emergency upgrade
    #[arg(long)]
    pub description: String,
    /// Wallet name to use for signing
    #[arg(long)]
    pub wallet: Option<String>,
    /// Network to use
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
    /// Number of approvals required before execution (default: 1)
    #[arg(long, default_value_t = 1)]
    pub threshold: u8,
}

#[derive(Args)]
pub struct UnlockArgs {
    /// Proposal ID to unlock
    #[arg(long)]
    pub proposal_id: String,
    /// Wallet name to use for signing
    #[arg(long)]
    pub wallet: Option<String>,
    /// Network to use
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
}

#[derive(Args)]
pub struct ListArgs {
    /// Filter by contract ID (optional)
    #[arg(long)]
    pub contract_id: Option<String>,
    /// Network to use
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
}

#[derive(Args)]
pub struct ApproveArgs {
    /// Proposal ID to approve
    #[arg(long)]
    pub proposal_id: String,
    /// Wallet name to use for signing
    #[arg(long)]
    pub wallet: Option<String>,
    /// Network to use
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
}

#[derive(Args)]
pub struct ExecuteArgs {
    /// Proposal ID to execute
    #[arg(long)]
    pub proposal_id: String,
    /// Wallet name to use for signing
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
pub struct RollbackArgs {
    /// Contract ID to roll back
    #[arg(long)]
    pub contract_id: String,
    /// Target version hash to roll back to
    #[arg(long)]
    pub to_hash: String,
    /// Wallet name to use for signing
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
pub struct HistoryArgs {
    /// Contract ID to show history for
    #[arg(long)]
    pub contract_id: String,
    /// Network to use
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
}

// ── Data structures ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProposalStatus {
    Pending,
    Approved,
    Timelocked,
    Unlocked,
    Executed,
    Rejected,
    Expired,
}

impl std::fmt::Display for ProposalStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProposalStatus::Pending => write!(f, "pending"),
            ProposalStatus::Approved => write!(f, "approved"),
            ProposalStatus::Timelocked => write!(f, "timelocked"),
            ProposalStatus::Unlocked => write!(f, "unlocked"),
            ProposalStatus::Executed => write!(f, "executed"),
            ProposalStatus::Rejected => write!(f, "rejected"),
            ProposalStatus::Expired => write!(f, "expired"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpgradeProposal {
    pub id: String,
    pub contract_id: String,
    pub new_wasm_hash: String,
    pub description: String,
    pub proposer: String,
    pub approvals: Vec<String>,
    pub threshold: u8,
    pub status: ProposalStatus,
    pub network: String,
    pub created_at: String,
    pub executed_at: Option<String>,
    pub timelock_start: Option<String>,
    pub timelock_duration_sec: Option<u64>,
    pub is_emergency: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpgradeRecord {
    pub contract_id: String,
    pub from_hash: String,
    pub to_hash: String,
    pub proposal_id: String,
    pub executed_by: String,
    pub network: String,
    pub timestamp: String,
}

// ── Storage helpers ───────────────────────────────────────────────────────────

fn upgrade_dir() -> Result<PathBuf> {
    let dir = config::config_dir().join("upgrades");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

fn proposals_path() -> Result<PathBuf> {
    Ok(upgrade_dir()?.join("proposals.json"))
}

fn history_path() -> Result<PathBuf> {
    Ok(upgrade_dir()?.join("history.json"))
}

fn load_proposals() -> Result<Vec<UpgradeProposal>> {
    let path = proposals_path()?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data).unwrap_or_default())
}

fn save_proposals(proposals: &[UpgradeProposal]) -> Result<()> {
    fs::write(proposals_path()?, serde_json::to_string_pretty(proposals)?)?;
    Ok(())
}

fn load_history() -> Result<Vec<UpgradeRecord>> {
    let path = history_path()?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data).unwrap_or_default())
}

fn save_history(history: &[UpgradeRecord]) -> Result<()> {
    fs::write(history_path()?, serde_json::to_string_pretty(history)?)?;
    Ok(())
}

// ── WASM utilities ────────────────────────────────────────────────────────────

/// Compute SHA-256 hash of WASM bytes, returned as a hex string.
pub fn wasm_hash(bytes: &[u8]) -> String {
    compute_wasm_hash(bytes, BuildEnvironment::current())
        .unwrap_or_else(|e| panic!("failed to compute upgrade WASM hash: {e}"))
}

fn validate_wasm(path: &PathBuf) -> Result<(Vec<u8>, String)> {
    if !path.exists() {
        anyhow::bail!(
            "WASM file not found: {}\nRun `stellar contract build` first.",
            path.display()
        );
    }
    let bytes = fs::read(path)?;
    // Basic WASM magic number check: \0asm
    if bytes.len() < 4 || &bytes[..4] != b"\0asm" {
        anyhow::bail!(
            "File does not appear to be a valid WASM binary: {}",
            path.display()
        );
    }
    let size_kb = bytes.len() as f64 / 1024.0;
    if size_kb > 128.0 {
        p::warn(&format!(
            "WASM is {:.1} KB — Soroban limit is 128 KB.",
            size_kb
        ));
    }
    let hash = wasm_hash(&bytes);
    Ok((bytes, hash))
}

fn short_id(id: &str) -> String {
    format!("{}…", &id[..id.len().min(12)])
}

// ── Command handlers ──────────────────────────────────────────────────────────

pub async fn handle(cmd: UpgradeCommands) -> Result<()> {
    match cmd {
        UpgradeCommands::Prepare(args) => handle_prepare(args).await,
        UpgradeCommands::Auto(cmd) => crate::commands::upgrade_auto::handle(cmd).await,
        UpgradeCommands::Propose(args) => handle_propose(args),
        UpgradeCommands::EmergencyPropose(args) => handle_emergency_propose(args),
        UpgradeCommands::List(args) => handle_list(args),
        UpgradeCommands::Status(args) => handle_list(args), // Alias for list
        UpgradeCommands::Approve(args) => handle_approve(args),
        UpgradeCommands::Unlock(args) => handle_unlock(args),
        UpgradeCommands::Execute(args) => handle_execute(args).await,
        UpgradeCommands::Rollback(args) => handle_rollback(args),
        UpgradeCommands::History(args) => handle_history(args),
    }
}

async fn handle_prepare(args: PrepareArgs) -> Result<()> {
    p::header("Prepare Contract Upgrade");

    config::validate_contract_id(&args.contract_id)?;
    config::validate_network(&args.network)?;

    p::step(1, 3, "Validating WASM file…");
    let (_, new_hash) = validate_wasm(&args.wasm)?;
    p::kv_accent("New WASM hash", &new_hash);

    p::step(2, 3, "Verifying contract exists on-chain…");
    // Verify the deployer account is reachable
    let cfg = config::load()?;
    let wallet = cfg.wallets.first().ok_or_else(|| {
        anyhow::anyhow!("No wallets found. Create one with `starforge wallet create`")
    })?;
    horizon::fetch_account(&wallet.public_key, &args.network)
        .await
        .map_err(|e| anyhow::anyhow!("Account not active on {}: {}", args.network, e))?;

    p::step(3, 3, "Generating upgrade command…");
    println!();
    p::separator();
    p::kv("Contract ID", &args.contract_id);
    p::kv("Network", &args.network);
    p::kv("WASM file", &args.wasm.display().to_string());
    p::kv_accent("New hash", &new_hash);
    println!();
    println!(
        "  {} {}",
        "Next step:".bright_white(),
        "create a proposal with:".dimmed()
    );
    println!(
        "  {}",
        format!(
            "starforge upgrade propose --contract-id {} --wasm {} --description \"<reason>\"",
            args.contract_id,
            args.wasm.display()
        )
        .cyan()
    );
    p::separator();
    Ok(())
}

fn handle_propose(args: ProposeArgs) -> Result<()> {
    p::header("Create Upgrade Proposal");

    config::validate_contract_id(&args.contract_id)?;
    config::validate_network(&args.network)?;

    p::step(1, 3, "Validating WASM…");
    let (_, new_hash) = validate_wasm(&args.wasm)?;

    p::step(2, 3, "Loading wallet…");
    let cfg = config::load()?;
    let wallet = resolve_wallet(&cfg, args.wallet.as_deref())?;

    p::step(3, 3, "Saving proposal…");
    let proposal_id = format!("prop-{}", &new_hash[..12]);

    // Check for duplicate
    let mut proposals = load_proposals()?;
    if proposals.iter().any(|p| p.id == proposal_id) {
        anyhow::bail!(
            "A proposal for this WASM hash already exists: {}",
            proposal_id
        );
    }

    let (status, timelock_start) = if args.threshold <= 1 {
        (ProposalStatus::Timelocked, Some(Utc::now().to_rfc3339()))
    } else {
        (ProposalStatus::Pending, None)
    };

    let proposal = UpgradeProposal {
        id: proposal_id.clone(),
        contract_id: args.contract_id.clone(),
        new_wasm_hash: new_hash.clone(),
        description: args.description.clone(),
        proposer: wallet.public_key.clone(),
        approvals: vec![wallet.public_key.clone()], // proposer auto-approves
        threshold: args.threshold,
        status,
        network: args.network.clone(),
        created_at: Utc::now().to_rfc3339(),
        executed_at: None,
        timelock_start,
        timelock_duration_sec: Some(args.timelock_duration),
        is_emergency: false,
    };

    proposals.push(proposal);
    save_proposals(&proposals)?;

    // Log audit action
    let mut details = std::collections::HashMap::new();
    details.insert("contract_id".to_string(), args.contract_id.clone());
    details.insert("new_wasm_hash".to_string(), new_hash.clone());
    details.insert("description".to_string(), args.description.clone());
    details.insert("threshold".to_string(), args.threshold.to_string());
    details.insert(
        "timelock_duration_sec".to_string(),
        args.timelock_duration.to_string(),
    );
    audit::log_action(
        "propose_upgrade",
        &wallet.public_key,
        "upgrade_proposal",
        &proposal_id,
        details,
        true,
        None,
    )?;

    println!();
    p::separator();
    p::kv_accent("Proposal ID", &proposal_id);
    p::kv("Contract ID", &args.contract_id);
    p::kv("New hash", &new_hash);
    p::kv("Description", &args.description);
    p::kv("Proposer", &wallet.public_key);
    p::kv("Threshold", &args.threshold.to_string());
    p::kv(
        "Timelock duration",
        &format!("{} seconds", args.timelock_duration),
    );
    p::kv(
        "Status",
        if args.threshold <= 1 {
            "timelocked (auto-approved)"
        } else {
            "pending"
        },
    );
    println!();
    if args.threshold <= 1 {
        let unlock_time = Utc::now() + chrono::Duration::seconds(args.timelock_duration as i64);
        p::info(&format!(
            "Proposal is timelocked until {}. Unlock with: starforge upgrade unlock --proposal-id {}",
            unlock_time, proposal_id
        ));
    } else {
        p::info(&format!(
            "Needs {} more approval(s): starforge upgrade approve --proposal-id {}",
            args.threshold - 1,
            proposal_id
        ));
    }
    p::separator();
    Ok(())
}

fn handle_emergency_propose(args: EmergencyProposeArgs) -> Result<()> {
    p::header("Create Emergency Upgrade Proposal");

    config::validate_contract_id(&args.contract_id)?;
    config::validate_network(&args.network)?;

    p::step(1, 3, "Validating WASM…");
    let (_, new_hash) = validate_wasm(&args.wasm)?;

    p::step(2, 3, "Loading wallet…");
    let cfg = config::load()?;
    let wallet = resolve_wallet(&cfg, args.wallet.as_deref())?;

    p::step(3, 3, "Saving proposal…");
    let proposal_id = format!("prop-{}", &new_hash[..12]);

    // Check for duplicate
    let mut proposals = load_proposals()?;
    if proposals.iter().any(|p| p.id == proposal_id) {
        anyhow::bail!(
            "A proposal for this WASM hash already exists: {}",
            proposal_id
        );
    }

    let status = if args.threshold <= 1 {
        ProposalStatus::Unlocked
    } else {
        ProposalStatus::Pending
    };

    let proposal = UpgradeProposal {
        id: proposal_id.clone(),
        contract_id: args.contract_id.clone(),
        new_wasm_hash: new_hash.clone(),
        description: args.description.clone(),
        proposer: wallet.public_key.clone(),
        approvals: vec![wallet.public_key.clone()], // proposer auto-approves
        threshold: args.threshold,
        status,
        network: args.network.clone(),
        created_at: Utc::now().to_rfc3339(),
        executed_at: None,
        timelock_start: None,
        timelock_duration_sec: None,
        is_emergency: true,
    };

    proposals.push(proposal);
    save_proposals(&proposals)?;

    // Log audit action
    let mut details = std::collections::HashMap::new();
    details.insert("contract_id".to_string(), args.contract_id.clone());
    details.insert("new_wasm_hash".to_string(), new_hash.clone());
    details.insert("description".to_string(), args.description.clone());
    details.insert("threshold".to_string(), args.threshold.to_string());
    audit::log_action(
        "propose_emergency_upgrade",
        &wallet.public_key,
        "upgrade_proposal",
        &proposal_id,
        details,
        true,
        None,
    )?;

    println!();
    p::separator();
    p::kv_accent("Proposal ID", &proposal_id);
    p::kv("Contract ID", &args.contract_id);
    p::kv("New hash", &new_hash);
    p::kv("Description", &args.description);
    p::kv("Proposer", &wallet.public_key);
    p::kv("Threshold", &args.threshold.to_string());
    p::kv("Emergency", "true");
    p::kv(
        "Status",
        if args.threshold <= 1 {
            "unlocked (ready to execute)"
        } else {
            "pending"
        },
    );
    println!();
    if args.threshold <= 1 {
        p::info(&format!(
            "Ready to execute: starforge upgrade execute --proposal-id {}",
            proposal_id
        ));
    } else {
        p::info(&format!(
            "Needs {} more approval(s): starforge upgrade approve --proposal-id {}",
            args.threshold - 1,
            proposal_id
        ));
    }
    p::separator();
    Ok(())
}

fn handle_list(args: ListArgs) -> Result<()> {
    p::header("Upgrade Proposals");
    config::validate_network(&args.network)?;

    let proposals = load_proposals()?;
    let filtered: Vec<_> = proposals
        .iter()
        .filter(|p| p.network == args.network)
        .filter(|p| {
            args.contract_id
                .as_deref()
                .map_or(true, |id| p.contract_id == id)
        })
        .collect();

    if filtered.is_empty() {
        p::info("No proposals found.");
        return Ok(());
    }

    p::separator();
    println!(
        "  {:<16}  {:<14}  {:<12}  {:<10}  {:<10}  {}",
        "Proposal ID".dimmed(),
        "Contract".dimmed(),
        "Status".dimmed(),
        "Emergency".dimmed(),
        "Approvals".dimmed(),
        "Created".dimmed(),
    );
    println!("  {}", "─".repeat(80).dimmed());

    for prop in &filtered {
        let status_colored = match prop.status {
            ProposalStatus::Pending => prop.status.to_string().yellow().to_string(),
            ProposalStatus::Approved => prop.status.to_string().cyan().to_string(),
            ProposalStatus::Timelocked => prop.status.to_string().magenta().to_string(),
            ProposalStatus::Unlocked => prop.status.to_string().cyan().to_string(),
            ProposalStatus::Executed => prop.status.to_string().green().to_string(),
            ProposalStatus::Rejected | ProposalStatus::Expired => {
                prop.status.to_string().red().to_string()
            }
        };
        let approvals = format!("{}/{}", prop.approvals.len(), prop.threshold);
        let created = prop.created_at.get(..10).unwrap_or(&prop.created_at);
        let emergency_flag = if prop.is_emergency {
            "yes".red().to_string()
        } else {
            "no".dimmed().to_string()
        };
        println!(
            "  {:<16}  {:<14}  {:<12}  {:<10}  {:<10}  {}",
            prop.id.white(),
            short_id(&prop.contract_id).cyan(),
            status_colored,
            emergency_flag,
            approvals.white(),
            created.dimmed(),
        );
    }
    p::separator();
    Ok(())
}

fn handle_approve(args: ApproveArgs) -> Result<()> {
    p::header("Approve Upgrade Proposal");
    config::validate_network(&args.network)?;

    let cfg = config::load()?;
    let wallet = resolve_wallet(&cfg, args.wallet.as_deref())?;

    let mut proposals = load_proposals()?;
    let proposal = proposals
        .iter_mut()
        .find(|p| p.id == args.proposal_id && p.network == args.network)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Proposal '{}' not found on {}",
                args.proposal_id,
                args.network
            )
        })?;

    if proposal.status != ProposalStatus::Pending {
        anyhow::bail!(
            "Proposal '{}' is not pending (status: {})",
            args.proposal_id,
            proposal.status
        );
    }
    if proposal.approvals.contains(&wallet.public_key) {
        anyhow::bail!(
            "Wallet '{}' has already approved this proposal",
            wallet.name
        );
    }

    proposal.approvals.push(wallet.public_key.clone());
    if proposal.approvals.len() >= proposal.threshold as usize {
        if proposal.is_emergency {
            proposal.status = ProposalStatus::Unlocked;
        } else {
            proposal.status = ProposalStatus::Timelocked;
            proposal.timelock_start = Some(Utc::now().to_rfc3339());
        }
    }

    let new_status = proposal.status.to_string();
    let approvals = format!("{}/{}", proposal.approvals.len(), proposal.threshold);
    let proposal_id = proposal.id.clone();
    let timelock_start = proposal.timelock_start.clone();
    let timelock_duration_sec = proposal.timelock_duration_sec;
    save_proposals(&proposals)?;

    // Log audit action
    let mut details = std::collections::HashMap::new();
    details.insert("approvals".to_string(), approvals.clone());
    details.insert("new_status".to_string(), new_status.clone());
    audit::log_action(
        "approve_upgrade",
        &wallet.public_key,
        "upgrade_proposal",
        &proposal_id,
        details,
        true,
        None,
    )?;

    println!();
    p::kv_accent("Proposal", &args.proposal_id);
    p::kv("Approved by", &wallet.public_key);
    p::kv("Approvals", &approvals);
    p::kv("Status", &new_status);
    println!();
    if new_status == "timelocked" {
        let unlock_time = DateTime::parse_from_rfc3339(timelock_start.as_ref().unwrap())
            .unwrap()
            .with_timezone(&Utc)
            + chrono::Duration::seconds(timelock_duration_sec.unwrap() as i64);
        p::success(&format!(
            "Threshold reached — proposal is timelocked until {}",
            unlock_time
        ));
        p::info(&format!(
            "Unlock after timelock: starforge upgrade unlock --proposal-id {}",
            args.proposal_id
        ));
    } else if new_status == "unlocked" {
        p::success("Threshold reached — emergency proposal is ready to execute.");
        p::info(&format!(
            "starforge upgrade execute --proposal-id {}",
            args.proposal_id
        ));
    }
    Ok(())
}

fn handle_unlock(args: UnlockArgs) -> Result<()> {
    p::header("Unlock Upgrade Proposal");
    config::validate_network(&args.network)?;

    let cfg = config::load()?;
    let wallet = resolve_wallet(&cfg, args.wallet.as_deref())?;

    let mut proposals = load_proposals()?;
    let proposal = proposals
        .iter_mut()
        .find(|p| p.id == args.proposal_id && p.network == args.network)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Proposal '{}' not found on {}",
                args.proposal_id,
                args.network
            )
        })?;

    if proposal.status != ProposalStatus::Timelocked {
        anyhow::bail!(
            "Proposal '{}' is not timelocked (status: {})",
            args.proposal_id,
            proposal.status
        );
    }

    // Check timelock has passed
    let timelock_start = DateTime::parse_from_rfc3339(
        proposal
            .timelock_start
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No timelock start time found"))?,
    )
    .unwrap()
    .with_timezone(&Utc);
    let timelock_duration = proposal
        .timelock_duration_sec
        .ok_or_else(|| anyhow::anyhow!("No timelock duration found"))?;
    let unlock_time = timelock_start + chrono::Duration::seconds(timelock_duration as i64);
    let now = Utc::now();

    if now < unlock_time {
        anyhow::bail!("Timelock has not passed yet. Unlock time: {}", unlock_time);
    }

    proposal.status = ProposalStatus::Unlocked;
    let timelock_start_str = proposal.timelock_start.clone();
    save_proposals(&proposals)?;

    // Log audit action
    let mut details = std::collections::HashMap::new();
    details.insert(
        "timelock_start".to_string(),
        timelock_start_str.unwrap_or_default(),
    );
    details.insert(
        "timelock_duration_sec".to_string(),
        timelock_duration.to_string(),
    );
    audit::log_action(
        "unlock_upgrade",
        &wallet.public_key,
        "upgrade_proposal",
        &args.proposal_id,
        details,
        true,
        None,
    )?;

    println!();
    p::kv_accent("Proposal", &args.proposal_id);
    p::kv("Status", "unlocked");
    println!();
    p::success("Proposal unlocked — ready to execute.");
    p::info(&format!(
        "starforge upgrade execute --proposal-id {}",
        args.proposal_id
    ));
    Ok(())
}

async fn handle_execute(args: ExecuteArgs) -> Result<()> {
    p::header("Execute Contract Upgrade");
    config::validate_network(&args.network)?;

    let cfg = config::load()?;
    let wallet = resolve_wallet(&cfg, args.wallet.as_deref())?;

    let mut proposals = load_proposals()?;
    let proposal = proposals
        .iter_mut()
        .find(|p| p.id == args.proposal_id && p.network == args.network)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Proposal '{}' not found on {}",
                args.proposal_id,
                args.network
            )
        })?;

    if proposal.status != ProposalStatus::Unlocked {
        anyhow::bail!(
            "Proposal '{}' is not unlocked (status: {}).",
            args.proposal_id,
            proposal.status
        );
    }

    p::separator();
    p::kv("Proposal ID", &proposal.id);
    p::kv("Contract ID", &proposal.contract_id);
    p::kv_accent("New WASM hash", &proposal.new_wasm_hash);
    p::kv("Network", &proposal.network);
    p::kv("Executor", &wallet.public_key);

    // Build operation summary for confirmation
    let risk_level = if args.network == "mainnet" {
        confirmation::RiskLevel::High
    } else {
        confirmation::RiskLevel::Medium
    };

    let summary = confirmation::OperationSummary::new(
        "Execute Contract Upgrade".to_string(),
        args.network.clone(),
        risk_level,
    )
    .add("Proposal ID", &proposal.id)
    .add("Contract ID", &proposal.contract_id)
    .add("New WASM hash", &proposal.new_wasm_hash)
    .add("Network", &proposal.network)
    .add("Executor", &wallet.public_key)
    .add(
        "Approvals",
        format!("{}/{}", proposal.approvals.len(), proposal.threshold),
    );

    let confirm_config = confirmation::ConfirmationConfig {
        risk_level,
        network: args.network.clone(),
        skip_confirm: args.yes,
        dry_run: false,
        prompt: Some("Execute this upgrade?".to_string()),
        require_type_confirmation: args.network == "mainnet",
    };

    if !confirmation::confirm_operation(&summary, &confirm_config)? {
        return Ok(());
    }

    println!();
    p::step(1, 2, "Verifying account on-chain…");
    horizon::fetch_account(&wallet.public_key, &args.network)
        .await
        .map_err(|e| anyhow::anyhow!("Account not active on {}: {}", args.network, e))?;

    p::step(2, 2, "Generating upgrade command…");

    // Clone fields needed after the mutable borrow ends
    let contract_id = proposal.contract_id.clone();
    let new_wasm_hash = proposal.new_wasm_hash.clone();
    let proposal_id = proposal.id.clone();
    let is_emergency = proposal.is_emergency;
    let network = proposal.network.clone();

    // Record in history
    let mut history = load_history()?;
    history.push(UpgradeRecord {
        contract_id: contract_id.clone(),
        from_hash: "unknown".to_string(),
        to_hash: new_wasm_hash.clone(),
        proposal_id: proposal_id.clone(),
        executed_by: wallet.public_key.clone(),
        network: network.clone(),
        timestamp: Utc::now().to_rfc3339(),
    });
    save_history(&history)?;

    proposal.status = ProposalStatus::Executed;
    proposal.executed_at = Some(Utc::now().to_rfc3339());
    save_proposals(&proposals)?;

    // Log audit action
    let mut details = std::collections::HashMap::new();
    details.insert("contract_id".to_string(), contract_id.clone());
    details.insert("new_wasm_hash".to_string(), new_wasm_hash.clone());
    details.insert("is_emergency".to_string(), is_emergency.to_string());
    audit::log_action(
        "execute_upgrade",
        &wallet.public_key,
        "upgrade_proposal",
        &proposal_id,
        details,
        true,
        None,
    )?;

    println!();
    p::separator();
    println!(
        "  {} {}",
        "✓".green().bold(),
        "Upgrade ready! Run this to apply on-chain:".bright_white()
    );
    println!();
    println!(
        "  {}",
        format!(
            "stellar contract upload --wasm <path-to-new.wasm> --source {} --network {}",
            wallet.public_key, args.network
        )
        .cyan()
    );
    println!(
        "  {}",
        format!(
            "stellar contract invoke --id {} --source {} --network {} -- upgrade --new-wasm-hash {}",
            contract_id, wallet.public_key, args.network, new_wasm_hash
        ).cyan()
    );
    p::separator();
    Ok(())
}

fn handle_rollback(args: RollbackArgs) -> Result<()> {
    p::header("Contract Rollback");
    config::validate_contract_id(&args.contract_id)?;
    config::validate_network(&args.network)?;

    let cfg = config::load()?;
    let wallet = resolve_wallet(&cfg, args.wallet.as_deref())?;

    // Verify the target hash exists in history
    let history = load_history()?;
    let target = history.iter()
        .find(|r| r.contract_id == args.contract_id && r.to_hash == args.to_hash && r.network == args.network)
        .ok_or_else(|| anyhow::anyhow!(
            "Hash '{}' not found in upgrade history for contract '{}' on {}.\nRun `starforge upgrade history --contract-id {}` to see available versions.",
            args.to_hash, args.contract_id, args.network, args.contract_id
        ))?;

    // Log audit action
    let mut details = std::collections::HashMap::new();
    details.insert("contract_id".to_string(), args.contract_id.clone());
    details.insert("rollback_to_hash".to_string(), args.to_hash.clone());
    details.insert(
        "original_proposal_id".to_string(),
        target.proposal_id.clone(),
    );
    audit::log_action(
        "rollback_upgrade",
        &wallet.public_key,
        "upgrade_proposal",
        &target.proposal_id,
        details,
        true,
        None,
    )?;

    p::separator();
    p::kv("Contract ID", &args.contract_id);
    p::kv_accent("Rollback to", &args.to_hash);
    p::kv("Originally from", &target.proposal_id);
    p::kv("Network", &args.network);

    // Build operation summary for confirmation
    let risk_level = if args.network == "mainnet" {
        confirmation::RiskLevel::High
    } else {
        confirmation::RiskLevel::Medium
    };

    let summary = confirmation::OperationSummary::new(
        "Contract Rollback".to_string(),
        args.network.clone(),
        risk_level,
    )
    .add("Contract ID", &args.contract_id)
    .add("Rollback to", &args.to_hash)
    .add("Originally from", &target.proposal_id)
    .add("Network", &args.network)
    .add("Executor", &wallet.public_key);

    let confirm_config = confirmation::ConfirmationConfig {
        risk_level,
        network: args.network.clone(),
        skip_confirm: args.yes,
        dry_run: false,
        prompt: Some("Proceed with rollback?".to_string()),
        require_type_confirmation: args.network == "mainnet",
    };

    if !confirmation::confirm_operation(&summary, &confirm_config)? {
        return Ok(());
    }

    println!();
    p::separator();
    println!(
        "  {} {}",
        "✓".green().bold(),
        "Rollback command:".bright_white()
    );
    println!();
    println!(
        "  {}",
        format!(
            "stellar contract invoke --id {} --source {} --network {} -- upgrade --new-wasm-hash {}",
            args.contract_id, wallet.public_key, args.network, args.to_hash
        ).cyan()
    );
    p::separator();
    Ok(())
}

fn handle_history(args: HistoryArgs) -> Result<()> {
    p::header("Contract Upgrade History");
    config::validate_contract_id(&args.contract_id)?;
    config::validate_network(&args.network)?;

    let history = load_history()?;
    let records: Vec<_> = history
        .iter()
        .filter(|r| r.contract_id == args.contract_id && r.network == args.network)
        .collect();

    if records.is_empty() {
        p::info("No upgrade history found for this contract.");
        return Ok(());
    }

    p::separator();
    p::kv("Contract ID", &args.contract_id);
    p::kv("Network", &args.network);
    println!();
    println!(
        "  {:<14}  {:<14}  {:<16}  {}",
        "From hash".dimmed(),
        "To hash".dimmed(),
        "Proposal".dimmed(),
        "Timestamp".dimmed(),
    );
    println!("  {}", "─".repeat(72).dimmed());

    for record in &records {
        println!(
            "  {:<14}  {:<14}  {:<16}  {}",
            short_id(&record.from_hash).dimmed(),
            short_id(&record.to_hash).cyan(),
            record.proposal_id.white(),
            record
                .timestamp
                .get(..16)
                .unwrap_or(&record.timestamp)
                .dimmed(),
        );
    }
    p::separator();
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_hash_is_deterministic() {
        let bytes = b"\0asmmock wasm content";
        assert_eq!(wasm_hash(bytes), wasm_hash(bytes));
    }

    #[test]
    fn wasm_hash_differs_for_different_input() {
        assert_ne!(wasm_hash(b"\0asmversion1"), wasm_hash(b"\0asmversion2"));
    }

    #[test]
    fn wasm_hash_is_64_hex_chars() {
        let hash = wasm_hash(b"\0asmtest");
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn proposal_status_display() {
        assert_eq!(ProposalStatus::Pending.to_string(), "pending");
        assert_eq!(ProposalStatus::Approved.to_string(), "approved");
        assert_eq!(ProposalStatus::Timelocked.to_string(), "timelocked");
        assert_eq!(ProposalStatus::Unlocked.to_string(), "unlocked");
        assert_eq!(ProposalStatus::Executed.to_string(), "executed");
        assert_eq!(ProposalStatus::Rejected.to_string(), "rejected");
        assert_eq!(ProposalStatus::Expired.to_string(), "expired");
    }
}
