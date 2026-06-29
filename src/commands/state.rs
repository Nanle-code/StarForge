use crate::utils::print as p;
use crate::utils::{config, soroban, state_migration as sm};
use anyhow::Result;
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum StateCommands {
    /// Capture a contract's current state into a versioned snapshot
    Snapshot(SnapshotArgs),
    /// List stored state snapshots
    List(ListArgs),
    /// Diff two snapshots (added/removed/modified keys)
    Diff(DiffArgs),
    /// Generate a migration plan + soroban_sdk script between two snapshots
    Migrate(MigrateArgs),
    /// Validate a state transition against a safety policy
    Validate(ValidateArgs),
    /// Test a migration plan offline against an expected snapshot
    Test(TestArgs),
    /// Generate rollback operations to restore a previous snapshot
    Rollback(RollbackArgs),
}

#[derive(clap::Args)]
pub struct SnapshotArgs {
    /// Contract ID to snapshot (starts with C)
    #[arg(long)]
    pub contract: String,
    /// Network (testnet/mainnet)
    #[arg(long, default_value = "testnet")]
    pub network: String,
    /// Human-friendly label (e.g. v1, pre-upgrade)
    #[arg(long)]
    pub label: Option<String>,
    /// Load state from a JSON file instead of querying RPC (offline)
    #[arg(long)]
    pub from_file: Option<PathBuf>,
}

#[derive(clap::Args)]
pub struct ListArgs {
    /// Only show snapshots for this contract
    #[arg(long)]
    pub contract: Option<String>,
}

#[derive(clap::Args)]
pub struct DiffArgs {
    /// Source snapshot reference (id, label, or 'latest')
    pub from: String,
    /// Target snapshot reference (id, label, or 'latest')
    pub to: String,
    /// Restrict snapshot resolution to this contract
    #[arg(long)]
    pub contract: Option<String>,
    /// Emit JSON instead of formatted output
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::Args)]
pub struct MigrateArgs {
    pub from: String,
    pub to: String,
    #[arg(long)]
    pub contract: Option<String>,
    /// Write the generated migration script to this path
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Also write the machine-readable migration plan (JSON) here
    #[arg(long)]
    pub plan_out: Option<PathBuf>,
}

#[derive(clap::Args)]
pub struct ValidateArgs {
    pub from: String,
    pub to: String,
    #[arg(long)]
    pub contract: Option<String>,
    /// Permit removal of persistent keys (otherwise a critical finding)
    #[arg(long)]
    pub allow_persistent_removal: bool,
    /// Maximum number of changed entries before flagging scale
    #[arg(long, default_value = "100")]
    pub max_changes: usize,
}

#[derive(clap::Args)]
pub struct TestArgs {
    /// Base snapshot reference to apply the migration to
    #[arg(long)]
    pub base: String,
    /// Expected resulting snapshot reference
    #[arg(long)]
    pub expected: String,
    /// Migration plan JSON file (as produced by `state migrate --plan-out`)
    #[arg(long)]
    pub plan: PathBuf,
    #[arg(long)]
    pub contract: Option<String>,
}

#[derive(clap::Args)]
pub struct RollbackArgs {
    /// Current snapshot reference
    pub current: String,
    /// Target snapshot reference to roll back to
    pub target: String,
    #[arg(long)]
    pub contract: Option<String>,
    /// Write the rollback migration script to this path
    #[arg(long)]
    pub out: Option<PathBuf>,
}

pub async fn handle(cmd: StateCommands) -> Result<()> {
    match cmd {
        StateCommands::Snapshot(args) => handle_snapshot(args).await,
        StateCommands::List(args) => handle_list(args),
        StateCommands::Diff(args) => handle_diff(args),
        StateCommands::Migrate(args) => handle_migrate(args),
        StateCommands::Validate(args) => handle_validate(args),
        StateCommands::Test(args) => handle_test(args),
        StateCommands::Rollback(args) => handle_rollback(args),
    }
}

async fn handle_snapshot(args: SnapshotArgs) -> Result<()> {
    config::validate_contract_id(&args.contract)?;
    p::header("Contract State Snapshot");

    let snapshot = if let Some(path) = &args.from_file {
        p::kv("Source", &format!("file: {}", path.display()));
        // Accept either a StateSnapshot or a raw soroban inspection result.
        load_snapshot_or_inspect(path, &args.contract, &args.network, args.label.clone())?
    } else {
        p::kv("Source", &format!("RPC ({})", args.network));
        config::validate_network(&args.network)?;
        let inspect = soroban::inspect_contract(&args.contract, &args.network).await?;
        sm::StateSnapshot::from_inspect(&inspect, &args.network, args.label.clone())
    };

    let path = sm::save_snapshot(&snapshot)?;
    p::separator();
    p::kv_accent("Snapshot ID", &snapshot.id);
    p::kv("Contract", &snapshot.contract_id);
    if let Some(label) = &snapshot.label {
        p::kv("Label", label);
    }
    p::kv("Ledger", &snapshot.ledger_seq.to_string());
    p::kv("Entries", &snapshot.entries.len().to_string());
    p::kv("State hash", &snapshot.state_hash);
    p::kv("Saved to", &path.display().to_string());
    p::separator();
    p::success("State snapshot captured");
    Ok(())
}

fn load_snapshot_or_inspect(
    path: &std::path::Path,
    contract: &str,
    network: &str,
    label: Option<String>,
) -> Result<sm::StateSnapshot> {
    // First try a full StateSnapshot, then fall back to an inspection result.
    if let Ok(snap) = sm::load_snapshot_file(path) {
        return Ok(snap);
    }
    let contents = std::fs::read_to_string(path)?;
    let inspect: soroban::ContractInspectResult = serde_json::from_str(&contents).map_err(|e| {
        anyhow::anyhow!("File is neither a snapshot nor an inspection result: {}", e)
    })?;
    let _ = contract;
    Ok(sm::StateSnapshot::from_inspect(&inspect, network, label))
}

fn handle_list(args: ListArgs) -> Result<()> {
    p::header("State Snapshots");
    let snapshots = sm::list_snapshots()?;
    let filtered: Vec<_> = snapshots
        .into_iter()
        .filter(|s| {
            args.contract
                .as_deref()
                .map(|c| s.contract_id == c)
                .unwrap_or(true)
        })
        .collect();

    if filtered.is_empty() {
        p::info("No snapshots found. Capture one with: starforge state snapshot --contract <id>");
        return Ok(());
    }

    let rows: Vec<Vec<String>> = filtered
        .iter()
        .map(|s| {
            vec![
                s.id.chars().take(8).collect::<String>(),
                s.label.clone().unwrap_or_else(|| "-".to_string()),
                truncate(&s.contract_id, 12),
                s.network.clone(),
                s.ledger_seq.to_string(),
                s.entries.len().to_string(),
                s.timestamp.chars().take(19).collect::<String>(),
            ]
        })
        .collect();
    p::table(
        &[
            "ID", "Label", "Contract", "Network", "Ledger", "Keys", "Captured",
        ],
        &rows,
    );
    Ok(())
}

fn handle_diff(args: DiffArgs) -> Result<()> {
    let from = sm::resolve_snapshot(&args.from, args.contract.as_deref())?;
    let to = sm::resolve_snapshot(&args.to, args.contract.as_deref())?;
    let diff = sm::diff_snapshots(&from, &to);

    if args.json {
        println!("{}", serde_json::to_string_pretty(&diff)?);
        return Ok(());
    }

    p::header("State Diff");
    p::kv(
        "From",
        &format!(
            "{} ({})",
            short(&from.id),
            from.label.clone().unwrap_or_default()
        ),
    );
    p::kv(
        "To",
        &format!(
            "{} ({})",
            short(&to.id),
            to.label.clone().unwrap_or_default()
        ),
    );
    p::separator();
    p::kv_accent("Added", &diff.added.to_string());
    p::kv_accent("Removed", &diff.removed.to_string());
    p::kv_accent("Modified", &diff.modified.to_string());
    p::kv("Unchanged", &diff.unchanged.to_string());
    p::separator();

    if diff.is_identical() {
        p::success("States are identical");
        return Ok(());
    }

    for e in diff
        .entries
        .iter()
        .filter(|e| e.kind != sm::ChangeKind::Unchanged)
    {
        let line = match e.kind {
            sm::ChangeKind::Added => {
                format!("  + [{}] {} = {}", e.durability, e.key, val(&e.new_value))
            }
            sm::ChangeKind::Removed => format!(
                "  - [{}] {} (was {})",
                e.durability,
                e.key,
                val(&e.old_value)
            ),
            sm::ChangeKind::Modified => format!(
                "  ~ [{}] {}: {} -> {}",
                e.durability,
                e.key,
                val(&e.old_value),
                val(&e.new_value)
            ),
            sm::ChangeKind::Unchanged => continue,
        };
        println!("{}", line);
    }
    Ok(())
}

fn handle_migrate(args: MigrateArgs) -> Result<()> {
    let from = sm::resolve_snapshot(&args.from, args.contract.as_deref())?;
    let to = sm::resolve_snapshot(&args.to, args.contract.as_deref())?;
    let diff = sm::diff_snapshots(&from, &to);
    let plan = sm::generate_migration_plan(&diff);
    let script = sm::generate_migration_script(&plan);

    p::header("Migration Generation");
    p::kv("Contract", &plan.contract_id);
    p::kv("Operations", &plan.operations.len().to_string());
    p::separator();

    if let Some(out) = &args.out {
        std::fs::write(out, &script)?;
        p::success(&format!("Migration script written to {}", out.display()));
    } else {
        println!("{}", script);
    }

    if let Some(plan_out) = &args.plan_out {
        std::fs::write(plan_out, serde_json::to_string_pretty(&plan)?)?;
        p::success(&format!("Migration plan written to {}", plan_out.display()));
    }
    Ok(())
}

fn handle_validate(args: ValidateArgs) -> Result<()> {
    let from = sm::resolve_snapshot(&args.from, args.contract.as_deref())?;
    let to = sm::resolve_snapshot(&args.to, args.contract.as_deref())?;
    let diff = sm::diff_snapshots(&from, &to);
    let policy = sm::MigrationPolicy {
        allow_persistent_removal: args.allow_persistent_removal,
        allow_instance_removal: true,
        max_changes: args.max_changes,
    };
    let findings = sm::validate_transition(&diff, &policy);

    p::header("State Transition Validation");
    p::separator();
    for f in &findings {
        let line = format!("[{}] {}: {}", f.severity.label(), f.category, f.message);
        match f.severity {
            sm::Severity::Critical => p::error(&line),
            sm::Severity::Warning => p::warn(&line),
            sm::Severity::Info => p::info(&line),
        }
    }
    p::separator();

    if sm::has_blocking_findings(&findings) {
        anyhow::bail!("State transition is unsafe; resolve critical findings or pass --allow-persistent-removal");
    }
    p::success("State transition validated");
    Ok(())
}

fn handle_test(args: TestArgs) -> Result<()> {
    let base = sm::resolve_snapshot(&args.base, args.contract.as_deref())?;
    let expected = sm::resolve_snapshot(&args.expected, args.contract.as_deref())?;
    let plan_json = std::fs::read_to_string(&args.plan)?;
    let plan: sm::MigrationPlan = serde_json::from_str(&plan_json)
        .map_err(|e| anyhow::anyhow!("Failed to parse migration plan: {}", e))?;

    let result = sm::test_migration(&base, &plan.operations, &expected);

    p::header("Migration Test");
    p::kv("Base", &short(&base.id));
    p::kv("Expected", &short(&expected.id));
    p::kv("Operations", &plan.operations.len().to_string());
    p::kv("Expected hash", &result.expected_hash);
    p::kv("Actual hash", &result.actual_hash);
    p::separator();

    if result.passed {
        p::success("Migration produces the expected state");
        return Ok(());
    }

    p::warn(&format!(
        "{} mismatching entr(ies):",
        result.mismatches.len()
    ));
    for m in &result.mismatches {
        println!(
            "  {} [{}] {} (expected {}, got {})",
            m.kind.symbol(),
            m.durability,
            m.key,
            val(&m.old_value),
            val(&m.new_value)
        );
    }
    anyhow::bail!("Migration test failed");
}

fn handle_rollback(args: RollbackArgs) -> Result<()> {
    let current = sm::resolve_snapshot(&args.current, args.contract.as_deref())?;
    let target = sm::resolve_snapshot(&args.target, args.contract.as_deref())?;
    let ops = sm::rollback_operations(&current, &target);

    // The rollback is itself a migration from current -> target.
    let reverse_diff = sm::diff_snapshots(&current, &target);
    let plan = sm::generate_migration_plan(&reverse_diff);
    let script = sm::generate_migration_script(&plan);

    p::header("State Rollback Plan");
    p::kv("From (current)", &short(&current.id));
    p::kv("To (target)", &short(&target.id));
    p::kv("Rollback operations", &ops.len().to_string());
    p::separator();

    if let Some(out) = &args.out {
        std::fs::write(out, &script)?;
        p::success(&format!("Rollback script written to {}", out.display()));
    } else {
        println!("{}", script);
    }
    p::info("Apply the generated migrate() function to revert the contract's state.");
    Ok(())
}

// --- small display helpers ---

fn val(v: &Option<String>) -> String {
    v.clone()
        .map(|s| truncate(&s, 40))
        .unwrap_or_else(|| "∅".to_string())
}

fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}…", &s[..max])
    } else {
        s.to_string()
    }
}
