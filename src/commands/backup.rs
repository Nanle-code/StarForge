use crate::utils::backup::{self, BackupStatus};
use crate::utils::crypto::prompt_password;
use crate::utils::print as p;
use crate::utils::soroban;
use anyhow::Result;
use clap::{Args, Subcommand};
use colored::Colorize;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum BackupCommands {
    /// Create a backup of contract code/state
    Create(CreateArgs),
    /// List recorded backups
    List(ListArgs),
    /// Show details of a backup
    Show(ShowArgs),
    /// Verify a backup's integrity (checksum)
    Verify(VerifyArgs),
    /// Restore a backup into a destination directory
    Restore(RestoreArgs),
    /// Restore the most recent backup at or before a given time (point-in-time recovery)
    RestorePointInTime(RestorePitArgs),
    /// Replicate an existing backup to another region
    Replicate(ReplicateArgs),
    /// Run a non-destructive recovery test (restore into a temp dir and verify)
    TestRecovery(VerifyArgs),
    /// Configure a recurring automated backup
    AutoConfigure(AutoConfigureArgs),
    /// Run any automated backups that are due
    AutoRun(AutoRunArgs),
    /// Capture contract state from Soroban RPC into a verified backup manifest
    ContractState(ContractStateArgs),
    /// Verify both archive integrity and contract state manifest integrity
    VerifyState(VerifyArgs),
    /// Generate a cross-network contract state restore plan
    RestoreState(RestoreStateArgs),
    /// Configure recurring contract state backups
    ScheduleState(ScheduleStateArgs),
    /// Show backup management dashboard
    Dashboard(DashboardArgs),
}

#[derive(Args)]
pub struct CreateArgs {
    /// Files or directories to back up (contract WASM/source, state dirs, etc.)
    #[arg(required = true)]
    pub sources: Vec<PathBuf>,
    /// Logical label for this backup set (used for point-in-time recovery lookups)
    #[arg(long)]
    pub label: String,
    /// Encrypt the backup archive with a passphrase
    #[arg(long, default_value_t = false)]
    pub encrypt: bool,
    /// Region label to replicate the backup to
    #[arg(long, default_value = "primary")]
    pub region: String,
}

#[derive(Args)]
pub struct ListArgs {
    #[arg(long)]
    pub label: Option<String>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct ShowArgs {
    pub id: String,
}

#[derive(Args)]
pub struct VerifyArgs {
    pub id: String,
}

#[derive(Args)]
pub struct RestoreArgs {
    pub id: String,
    /// Directory to restore the backup's files into
    #[arg(long)]
    pub dest: PathBuf,
}

#[derive(Args)]
pub struct RestorePitArgs {
    /// Backup label to search within
    #[arg(long)]
    pub label: String,
    /// Restore the most recent backup at or before this time (RFC3339)
    #[arg(long)]
    pub at: String,
    #[arg(long)]
    pub dest: PathBuf,
}

#[derive(Args)]
pub struct ReplicateArgs {
    pub id: String,
    #[arg(long)]
    pub region: String,
}

#[derive(Args)]
pub struct AutoConfigureArgs {
    #[arg(long)]
    pub label: String,
    #[arg(required = true)]
    pub sources: Vec<PathBuf>,
    /// How often the backup should run, in hours
    #[arg(long, default_value_t = 24)]
    pub interval_hours: u64,
    #[arg(long, default_value_t = false)]
    pub encrypt: bool,
    #[arg(long, default_value = "primary")]
    pub region: String,
}

#[derive(Args)]
pub struct AutoRunArgs {}

#[derive(Args)]
pub struct ContractStateArgs {
    /// Contract ID to capture
    #[arg(long)]
    pub contract: String,
    /// Source network configured in starforge
    #[arg(long)]
    pub network: String,
    /// Logical label for this backup set
    #[arg(long)]
    pub label: String,
    /// Encrypt the backup archive with a passphrase
    #[arg(long, default_value_t = false)]
    pub encrypt: bool,
    /// Region label to replicate the backup to
    #[arg(long, default_value = "primary")]
    pub region: String,
}

#[derive(Args)]
pub struct RestoreStateArgs {
    pub id: String,
    /// Target network for the restore/migration plan
    #[arg(long)]
    pub target_network: String,
    /// Optional target contract ID when restoring into an existing deployment
    #[arg(long)]
    pub target_contract: Option<String>,
    /// Directory to write the restore plan into
    #[arg(long)]
    pub output_dir: PathBuf,
}

#[derive(Args)]
pub struct ScheduleStateArgs {
    #[arg(long)]
    pub label: String,
    #[arg(long)]
    pub contract: String,
    #[arg(long)]
    pub network: String,
    /// How often the state backup should run, in hours
    #[arg(long, default_value_t = 24)]
    pub interval_hours: u64,
    #[arg(long, default_value_t = false)]
    pub encrypt: bool,
    #[arg(long, default_value = "primary")]
    pub region: String,
}

#[derive(Args)]
pub struct DashboardArgs {
    /// Print dashboard data as JSON
    #[arg(long)]
    pub json: bool,
}

pub async fn handle(cmd: BackupCommands) -> Result<()> {
    match cmd {
        BackupCommands::Create(args) => handle_create(args),
        BackupCommands::List(args) => handle_list(args),
        BackupCommands::Show(args) => handle_show(args),
        BackupCommands::Verify(args) => handle_verify(args),
        BackupCommands::Restore(args) => handle_restore(args),
        BackupCommands::RestorePointInTime(args) => handle_restore_pit(args),
        BackupCommands::Replicate(args) => handle_replicate(args),
        BackupCommands::TestRecovery(args) => handle_test_recovery(args),
        BackupCommands::AutoConfigure(args) => handle_auto_configure(args),
        BackupCommands::AutoRun(args) => handle_auto_run(args).await,
        BackupCommands::ContractState(args) => handle_contract_state(args).await,
        BackupCommands::VerifyState(args) => handle_verify_state(args),
        BackupCommands::RestoreState(args) => handle_restore_state(args),
        BackupCommands::ScheduleState(args) => handle_schedule_state(args),
        BackupCommands::Dashboard(args) => handle_dashboard(args),
    }
}

fn handle_create(args: CreateArgs) -> Result<()> {
    p::header("Create Backup");
    let passphrase = if args.encrypt {
        Some(prompt_password("Backup encryption passphrase", true)?)
    } else {
        None
    };

    let record = backup::create_backup(
        &args.sources,
        &args.label,
        args.encrypt,
        passphrase.as_deref().map(|s| s.as_str()),
        &args.region,
    )?;

    p::kv("Backup ID", &record.id);
    p::kv("Label", &record.label);
    p::kv("Size", &format!("{} bytes", record.size_bytes));
    p::kv("Encrypted", if record.encrypted { "yes" } else { "no" });
    p::kv("Checksum", &record.checksum);
    p::kv("Replicated to", &record.replicated_regions.join(", "));
    p::success("Backup created");
    Ok(())
}

fn handle_list(args: ListArgs) -> Result<()> {
    p::header("Backups");
    let mut records = backup::list_backups()?;
    if let Some(label) = &args.label {
        records.retain(|r| &r.label == label);
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&records)?);
        return Ok(());
    }

    if records.is_empty() {
        p::info("No backups recorded.");
        return Ok(());
    }

    p::separator();
    for r in &records {
        let status_str = match r.status {
            BackupStatus::Verified => r.status.to_string().green().to_string(),
            BackupStatus::VerificationFailed => r.status.to_string().red().to_string(),
            BackupStatus::Created => r.status.to_string().cyan().to_string(),
        };
        println!(
            "  {} {:<14} {:<20} {:<10} {}",
            r.id[..8.min(r.id.len())].cyan(),
            r.label,
            r.created_at.get(..19).unwrap_or(&r.created_at),
            status_str,
            r.replicated_regions.join(","),
        );
    }
    p::separator();
    Ok(())
}

fn handle_show(args: ShowArgs) -> Result<()> {
    p::header("Backup Details");
    let record = backup::load_backup(&args.id)?;
    println!("{}", serde_json::to_string_pretty(&record)?);
    Ok(())
}

fn handle_verify(args: VerifyArgs) -> Result<()> {
    p::header("Verify Backup");
    let record = backup::load_backup(&args.id)?;
    let passphrase = if record.encrypted {
        Some(prompt_password("Backup encryption passphrase", false)?)
    } else {
        None
    };
    let updated = backup::verify_backup(&args.id, passphrase.as_deref().map(|s| s.as_str()))?;
    p::kv("Backup ID", &updated.id);
    p::kv("Status", &updated.status.to_string());
    p::success("Backup verified successfully");
    Ok(())
}

fn handle_restore(args: RestoreArgs) -> Result<()> {
    p::header("Restore Backup");
    let record = backup::load_backup(&args.id)?;
    let passphrase = if record.encrypted {
        Some(prompt_password("Backup encryption passphrase", false)?)
    } else {
        None
    };
    let extracted = backup::restore_backup(
        &args.id,
        &args.dest,
        passphrase.as_deref().map(|s| s.as_str()),
    )?;
    p::kv("Files restored", &extracted.len().to_string());
    for f in &extracted {
        println!("  - {}", f);
    }
    p::success("Backup restored");
    Ok(())
}

fn handle_restore_pit(args: RestorePitArgs) -> Result<()> {
    p::header("Point-in-Time Recovery");
    let at = backup::find_point_in_time(&args.label, chrono_parse(&args.at)?)?;
    p::kv("Selected backup", &at.id);
    p::kv("Created at", &at.created_at);

    let passphrase = if at.encrypted {
        Some(prompt_password("Backup encryption passphrase", false)?)
    } else {
        None
    };
    let extracted = backup::restore_backup(
        &at.id,
        &args.dest,
        passphrase.as_deref().map(|s| s.as_str()),
    )?;
    p::kv("Files restored", &extracted.len().to_string());
    p::success("Point-in-time recovery complete");
    Ok(())
}

fn handle_replicate(args: ReplicateArgs) -> Result<()> {
    p::header("Replicate Backup");
    let record = backup::replicate_existing(&args.id, &args.region)?;
    p::kv("Backup ID", &record.id);
    p::kv("Replicated to", &record.replicated_regions.join(", "));
    p::success("Backup replicated");
    Ok(())
}

fn handle_test_recovery(args: VerifyArgs) -> Result<()> {
    p::header("Recovery Test");
    let record = backup::load_backup(&args.id)?;
    let passphrase = if record.encrypted {
        Some(prompt_password("Backup encryption passphrase", false)?)
    } else {
        None
    };
    let count = backup::test_restore(&args.id, passphrase.as_deref().map(|s| s.as_str()))?;
    p::kv("Files verified", &count.to_string());
    p::success("Recovery test passed — backup can be restored");
    Ok(())
}

fn handle_auto_configure(args: AutoConfigureArgs) -> Result<()> {
    p::header("Configure Automated Backup");
    let cfg = backup::configure_automation(
        &args.label,
        args.sources,
        args.interval_hours,
        args.encrypt,
        &args.region,
    )?;
    p::kv("Label", &cfg.label);
    p::kv("Interval", &format!("{}h", cfg.interval_hours));
    p::success("Automated backup configured. Run `starforge backup auto-run` periodically (e.g. via cron) to execute due backups.");
    Ok(())
}

async fn handle_auto_run(_args: AutoRunArgs) -> Result<()> {
    p::header("Run Automated Backups");
    let passphrase = if backup::list_automation()?.iter().any(|c| c.encrypt) {
        Some(prompt_password("Backup encryption passphrase", false)?)
    } else {
        None
    };
    let ran = backup::run_automation(passphrase.as_deref().map(|s| s.as_str()))?;
    let mut contract_ran = Vec::new();
    for cfg in backup::list_automation()? {
        let Some(contract) = cfg.contract.as_ref() else {
            continue;
        };
        if !automation_due(cfg.last_run.as_deref(), cfg.interval_hours) {
            continue;
        }
        let inspect = soroban::inspect_contract(&contract.contract_id, &contract.network).await?;
        let record = backup::create_contract_state_backup(
            &inspect,
            &contract.network,
            &cfg.label,
            cfg.encrypt,
            passphrase.as_deref().map(|s| s.as_str()),
            &cfg.region,
        )?;
        backup::mark_automation_ran(&cfg.label, chrono::Utc::now())?;
        contract_ran.push((cfg.label, record.id));
    }
    if ran.is_empty() {
        if contract_ran.is_empty() {
            p::info("No automated backups were due.");
        }
    } else {
        for label in &ran {
            p::success(&format!("Backed up '{}'", label));
        }
    }
    for (label, id) in &contract_ran {
        p::success(&format!("Backed up contract state '{}' ({})", label, id));
    }
    Ok(())
}

async fn handle_contract_state(args: ContractStateArgs) -> Result<()> {
    p::header("Contract State Backup");
    p::step(1, 3, "Inspecting contract state via Soroban RPC...");
    let inspect = soroban::inspect_contract(&args.contract, &args.network).await?;
    let passphrase = if args.encrypt {
        Some(prompt_password("Backup encryption passphrase", true)?)
    } else {
        None
    };

    p::step(2, 3, "Writing state manifest and archive...");
    let record = backup::create_contract_state_backup(
        &inspect,
        &args.network,
        &args.label,
        args.encrypt,
        passphrase.as_deref().map(|s| s.as_str()),
        &args.region,
    )?;
    p::step(3, 3, "Verifying backup manifest...");
    let manifest = backup::verify_contract_state_backup(
        &record.id,
        passphrase.as_deref().map(|s| s.as_str()),
    )?;

    p::kv("Backup ID", &record.id);
    p::kv("Contract", &manifest.contract_id);
    p::kv("Network", &manifest.source_network);
    p::kv("Latest ledger", &manifest.latest_ledger.to_string());
    p::kv(
        "State entries",
        &manifest.instance_storage.len().to_string(),
    );
    p::kv("Checksum", &manifest.checksum);
    p::success("Contract state backup created and verified");
    Ok(())
}

fn handle_verify_state(args: VerifyArgs) -> Result<()> {
    p::header("Verify Contract State Backup");
    let record = backup::load_backup(&args.id)?;
    let passphrase = if record.encrypted {
        Some(prompt_password("Backup encryption passphrase", false)?)
    } else {
        None
    };
    let manifest =
        backup::verify_contract_state_backup(&args.id, passphrase.as_deref().map(|s| s.as_str()))?;
    p::kv("Backup ID", &args.id);
    p::kv("Contract", &manifest.contract_id);
    p::kv("Manifest checksum", &manifest.checksum);
    p::success("Contract state backup verified");
    Ok(())
}

fn handle_restore_state(args: RestoreStateArgs) -> Result<()> {
    p::header("Cross-Network State Restore");
    let record = backup::load_backup(&args.id)?;
    let passphrase = if record.encrypted {
        Some(prompt_password("Backup encryption passphrase", false)?)
    } else {
        None
    };
    let restore = backup::restore_contract_state_backup(
        &args.id,
        &args.target_network,
        args.target_contract.as_deref(),
        &args.output_dir,
        passphrase.as_deref().map(|s| s.as_str()),
    )?;
    p::kv("Target network", &restore.target_network);
    if let Some(contract) = &restore.target_contract_id {
        p::kv("Target contract", contract);
    }
    p::kv("Restore plan", &restore.output_path.display().to_string());
    p::success("Cross-network restore plan generated and verified");
    Ok(())
}

fn handle_schedule_state(args: ScheduleStateArgs) -> Result<()> {
    p::header("Schedule Contract State Backup");
    let cfg = backup::configure_contract_automation(
        &args.label,
        &args.contract,
        &args.network,
        args.interval_hours,
        args.encrypt,
        &args.region,
    )?;
    p::kv("Label", &cfg.label);
    p::kv("Contract", &args.contract);
    p::kv("Network", &args.network);
    p::kv("Interval", &format!("{}h", cfg.interval_hours));
    p::success("Contract state backup schedule configured");
    Ok(())
}

fn handle_dashboard(args: DashboardArgs) -> Result<()> {
    let records = backup::list_backups()?;
    let schedules = backup::list_automation()?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "backups": records,
                "schedules": schedules,
            }))?
        );
        return Ok(());
    }

    p::header("Backup Management");
    p::kv("Total backups", &records.len().to_string());
    p::kv(
        "Contract state backups",
        &records
            .iter()
            .filter(|record| record.contract_state.is_some())
            .count()
            .to_string(),
    );
    p::kv("Schedules", &schedules.len().to_string());
    p::separator();
    for record in &records {
        let kind = if record.contract_state.is_some() {
            "contract-state"
        } else {
            "archive"
        };
        let status = match record.status {
            BackupStatus::Verified => record.status.to_string().green().to_string(),
            BackupStatus::VerificationFailed => record.status.to_string().red().to_string(),
            BackupStatus::Created => record.status.to_string().cyan().to_string(),
        };
        println!(
            "  {} {:<15} {:<20} {:<12} {}",
            record.id[..8.min(record.id.len())].cyan(),
            kind,
            record.label,
            status,
            record.created_at.get(..19).unwrap_or(&record.created_at)
        );
        if let Some(state) = &record.contract_state {
            println!(
                "      {} on {} | ledger {} | restores {}",
                state.contract_id,
                state.source_network,
                state.latest_ledger,
                state.restore_history.len()
            );
        }
    }
    p::separator();
    Ok(())
}

fn automation_due(last_run: Option<&str>, interval_hours: u64) -> bool {
    let Some(last_run) = last_run else {
        return true;
    };
    chrono::DateTime::parse_from_rfc3339(last_run)
        .map(|last| {
            chrono::Utc::now()
                .signed_duration_since(last.with_timezone(&chrono::Utc))
                .num_hours() as u64
                >= interval_hours
        })
        .unwrap_or(true)
}

fn chrono_parse(s: &str) -> Result<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&chrono::Utc))
        .map_err(|e| anyhow::anyhow!("Invalid RFC3339 timestamp '{}': {}", s, e))
}
