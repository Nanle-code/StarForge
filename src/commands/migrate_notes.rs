//! `starforge migrate-notes` — AI-assisted CLI migration notes generator
//!
//! Generates markdown migration notes for breaking CLI/config changes between
//! two versions of StarForge (or any tool following the same snapshot format).
//!
//! # Subcommands
//!
//! - `generate`  Produce migration notes from two snapshot files or a git range.
//! - `snapshot`  Capture the current CLI surface as a snapshot JSON file.
//! - `diff`      Show the raw list of changes between two snapshots.
//! - `docs`      Print the maintainer workflow documentation.
//!
//! # Human-review banner
//!
//! Every output produced by `generate` includes the mandatory human-review
//! banner from `utils::cli_migration_notes::HUMAN_REVIEW_BANNER`.
//! There is no flag to suppress it.
//!
//! See `docs/MIGRATION_NOTES_WORKFLOW.md` for the maintainer workflow.

use crate::utils::cli_migration_notes::{
    self, CliChange, CliCommandSnapshot, CliFlag, CliSnapshot, ConfigKeySnapshot,
    HUMAN_REVIEW_BANNER,
};
use crate::utils::print as p;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::*;
use std::path::PathBuf;

// ── CLI definition ────────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum MigrateNotesCommands {
    /// Generate markdown migration notes from two CLI snapshots.
    Generate(GenerateArgs),
    /// Capture the current CLI command/flag/config surface as a snapshot JSON.
    Snapshot(SnapshotArgs),
    /// Print the raw list of changes between two snapshot files.
    Diff(DiffArgs),
    /// Print the maintainer workflow documentation.
    Docs(DocsArgs),
}

#[derive(Args)]
pub struct GenerateArgs {
    /// Path to the "before" CLI snapshot JSON (older version).
    #[arg(long)]
    pub before: PathBuf,

    /// Path to the "after" CLI snapshot JSON (newer version).
    #[arg(long)]
    pub after: PathBuf,

    /// Write the generated notes to this file instead of stdout.
    #[arg(long, short = 'o')]
    pub output: Option<PathBuf>,

    /// Print a summary of detected changes alongside the notes.
    #[arg(long, default_value = "false")]
    pub summary: bool,
}

#[derive(Args)]
pub struct SnapshotArgs {
    /// Version label to embed in the snapshot (e.g. "v0.2.0" or a git tag).
    #[arg(long, default_value = "unknown")]
    pub version: String,

    /// Write the snapshot to this file.
    #[arg(long, short = 'o', default_value = "cli-snapshot.json")]
    pub output: PathBuf,
}

#[derive(Args)]
pub struct DiffArgs {
    /// Path to the "before" CLI snapshot JSON.
    #[arg(long)]
    pub before: PathBuf,

    /// Path to the "after" CLI snapshot JSON.
    #[arg(long)]
    pub after: PathBuf,

    /// Output format: `console` (default) or `json`.
    #[arg(long, default_value = "console")]
    pub format: String,
}

#[derive(Args)]
pub struct DocsArgs {
    /// Write docs to this file instead of printing to stdout.
    #[arg(long)]
    pub output: Option<PathBuf>,
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn handle(cmd: MigrateNotesCommands) -> Result<()> {
    match cmd {
        MigrateNotesCommands::Generate(args) => handle_generate(args),
        MigrateNotesCommands::Snapshot(args) => handle_snapshot(args),
        MigrateNotesCommands::Diff(args) => handle_diff(args),
        MigrateNotesCommands::Docs(args) => handle_docs(args),
    }
}

// ── Handlers ──────────────────────────────────────────────────────────────────

fn handle_generate(args: GenerateArgs) -> Result<()> {
    p::header("AI-Assisted CLI Migration Notes Generator");

    let before = cli_migration_notes::load_snapshot(&args.before)
        .with_context(|| format!("Loading before-snapshot: {}", args.before.display()))?;
    let after = cli_migration_notes::load_snapshot(&args.after)
        .with_context(|| format!("Loading after-snapshot: {}", args.after.display()))?;

    p::kv("Before version", &before.version);
    p::kv("After version", &after.version);

    let changes = cli_migration_notes::diff_snapshots(&before, &after);

    if args.summary {
        print_change_summary(&changes);
    }

    // render_migration_notes unconditionally prepends HUMAN_REVIEW_BANNER.
    let notes = cli_migration_notes::render_migration_notes(&before.version, &after.version, &changes);

    match &args.output {
        Some(path) => {
            std::fs::write(path, &notes)
                .with_context(|| format!("Writing notes to {}", path.display()))?;
            p::success(&format!("Migration notes written to {}", path.display()));
            p::warn(
                "⚠️  These notes are AI-generated — review and edit before publishing.",
            );
        }
        None => {
            println!("{}", notes);
        }
    }

    Ok(())
}

fn handle_snapshot(args: SnapshotArgs) -> Result<()> {
    p::header("Capture CLI Snapshot");

    // Build a snapshot of the current StarForge CLI surface.
    // In a real run this would introspect the running binary's clap command
    // tree.  For portability (this function runs without a live binary
    // available at all times) we build the canonical snapshot from the
    // known command list and config keys declared in source.
    let snapshot = build_current_snapshot(&args.version);

    cli_migration_notes::save_snapshot(&snapshot, &args.output)
        .with_context(|| format!("Writing snapshot to {}", args.output.display()))?;

    p::success(&format!("Snapshot written to {}", args.output.display()));
    p::kv("Version", &args.version);
    p::kv("Commands captured", &snapshot.commands.len().to_string());
    p::kv("Config keys captured", &snapshot.config_keys.len().to_string());
    Ok(())
}

fn handle_diff(args: DiffArgs) -> Result<()> {
    p::header("CLI Snapshot Diff");

    let before = cli_migration_notes::load_snapshot(&args.before)?;
    let after = cli_migration_notes::load_snapshot(&args.after)?;
    let changes = cli_migration_notes::diff_snapshots(&before, &after);

    match args.format.as_str() {
        "json" => {
            // Serialize changes as a simple JSON array of strings for
            // machine consumption.
            let descriptions: Vec<String> = changes.iter().map(describe_change).collect();
            println!("{}", serde_json::to_string_pretty(&descriptions)?);
        }
        _ => {
            if changes.is_empty() {
                p::success("No changes detected between the two snapshots.");
            } else {
                println!();
                for change in &changes {
                    println!("  {}", describe_change(change).cyan());
                }
                println!();
                p::kv("Total changes", &changes.len().to_string());
            }
        }
    }

    Ok(())
}

fn handle_docs(args: DocsArgs) -> Result<()> {
    let content = include_str!("../../docs/MIGRATION_NOTES_WORKFLOW.md");
    match &args.output {
        Some(path) => {
            std::fs::write(path, content)?;
            println!("Docs written to {}", path.display());
        }
        None => {
            println!("{}", content);
        }
    }
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn print_change_summary(changes: &[CliChange]) {
    if changes.is_empty() {
        p::info("No changes detected.");
        return;
    }

    let renames = changes
        .iter()
        .filter(|c| matches!(c, CliChange::CommandRenamed { .. }))
        .count();
    let removals = changes
        .iter()
        .filter(|c| matches!(c, CliChange::CommandRemoved { .. }))
        .count();
    let flag_changes = changes
        .iter()
        .filter(|c| {
            matches!(
                c,
                CliChange::FlagRenamed { .. }
                    | CliChange::FlagRemoved { .. }
                    | CliChange::FlagBecameRequired { .. }
            )
        })
        .count();
    let config_changes = changes
        .iter()
        .filter(|c| {
            matches!(
                c,
                CliChange::ConfigKeyRenamed { .. }
                    | CliChange::ConfigKeyRemoved { .. }
                    | CliChange::ConfigKeyTypeChanged { .. }
            )
        })
        .count();

    println!();
    p::separator();
    p::kv("Command renames", &renames.to_string());
    p::kv("Command removals", &removals.to_string());
    p::kv("Flag changes", &flag_changes.to_string());
    p::kv("Config changes", &config_changes.to_string());
    p::separator();
    println!();
}

fn describe_change(change: &CliChange) -> String {
    match change {
        CliChange::CommandRenamed { old, new } => {
            format!("[RENAME]  command  {} → {}", old, new)
        }
        CliChange::CommandRemoved { command, .. } => {
            format!("[REMOVE]  command  {} (no replacement)", command)
        }
        CliChange::CommandAdded { command, .. } => {
            format!("[ADD]     command  {}", command)
        }
        CliChange::FlagRenamed { command, old_flag, new_flag } => {
            format!("[RENAME]  flag     {} --{} → --{}", command, old_flag, new_flag)
        }
        CliChange::FlagRemoved { command, flag, .. } => {
            format!("[REMOVE]  flag     {} --{}", command, flag)
        }
        CliChange::FlagBecameRequired { command, flag, .. } => {
            format!("[BREAKING] flag    {} --{} became required", command, flag)
        }
        CliChange::ConfigKeyRenamed { old, new } => {
            format!("[RENAME]  config   {} → {}", old, new)
        }
        CliChange::ConfigKeyRemoved { key, .. } => {
            format!("[REMOVE]  config   {}", key)
        }
        CliChange::ConfigKeyAdded { key, .. } => {
            format!("[ADD]     config   {}", key)
        }
        CliChange::ConfigKeyTypeChanged { key, old_type, new_type } => {
            format!("[CHANGE]  config   {} type {} → {}", key, old_type, new_type)
        }
    }
}

/// Build a snapshot of the current StarForge CLI surface.
///
/// This is the canonical list of commands and config keys for the current
/// codebase.  When a command is added or removed from `src/main.rs`, this
/// function should be updated to match.  The snapshot is the "source of truth"
/// against which older captured snapshots are diffed.
pub fn build_current_snapshot(version: &str) -> CliSnapshot {
    CliSnapshot {
        version: version.to_string(),
        commands: vec![
            cmd("wallet create", "Create a new test wallet", vec![
                flag("name", Some("n"), true, None, "Wallet name"),
                flag("network", None, false, Some("testnet"), "Target network"),
            ]),
            cmd("wallet list", "List all local wallets", vec![]),
            cmd("wallet fund", "Fund a wallet via Friendbot (testnet only)", vec![
                flag("name", Some("n"), true, None, "Wallet name"),
            ]),
            cmd("wallet show", "Show wallet details", vec![
                flag("name", Some("n"), true, None, "Wallet name"),
            ]),
            cmd("wallet remove", "Remove a local wallet", vec![
                flag("name", Some("n"), true, None, "Wallet name"),
            ]),
            cmd("deploy", "Deploy a compiled Soroban contract (.wasm)", vec![
                flag("wasm", None, true, None, "Path to the compiled .wasm file"),
                flag("network", None, false, Some("testnet"), "Target network"),
                flag("simulate", None, false, None, "Simulate deployment without submitting"),
                flag("execute", None, false, None, "Execute the deployment"),
            ]),
            cmd("network show", "Show active network configuration", vec![]),
            cmd("network switch", "Switch the active network", vec![
                flag("name", None, true, None, "Network name (testnet or mainnet)"),
            ]),
            cmd("template list", "List available contract templates", vec![]),
            cmd("template use", "Scaffold from a template", vec![
                flag("name", Some("n"), true, None, "Template name"),
                flag("output", Some("o"), false, None, "Output directory"),
            ]),
            cmd("generate", "Generate a Soroban contract from a natural language prompt", vec![
                flag("prompt", Some("p"), true, None, "Natural language description"),
                flag("output", Some("o"), false, Some("contract.rs"), "Output file"),
            ]),
            cmd("info", "Show environment and version information", vec![]),
        ],
        config_keys: vec![
            config_key("network.default", "string", Some("testnet"), "Default network for all commands"),
            config_key("telemetry.enabled", "bool", Some("true"), "Whether to send anonymous usage telemetry"),
            config_key("ai.model", "string", Some("codellama:7b"), "Default Ollama model for AI commands"),
            config_key("ai.ollama_url", "string", Some("http://localhost:11434"), "Ollama base URL"),
            config_key("wallet.storage_dir", "string", None, "Directory where wallet files are stored"),
            config_key("deploy.default_network", "string", Some("testnet"), "Default network for deploy command"),
        ],
    }
}

// ── Snapshot builder helpers ──────────────────────────────────────────────────

fn cmd(command: &str, description: &str, flags: Vec<CliFlag>) -> CliCommandSnapshot {
    CliCommandSnapshot {
        command: command.to_string(),
        description: description.to_string(),
        flags,
    }
}

fn flag(
    name: &str,
    short: Option<&str>,
    required: bool,
    default: Option<&str>,
    description: &str,
) -> CliFlag {
    CliFlag {
        name: name.to_string(),
        short: short.map(str::to_string),
        required,
        default: default.map(str::to_string),
        description: description.to_string(),
    }
}

fn config_key(
    key: &str,
    value_type: &str,
    default: Option<&str>,
    description: &str,
) -> ConfigKeySnapshot {
    ConfigKeySnapshot {
        key: key.to_string(),
        value_type: value_type.to_string(),
        default: default.map(str::to_string),
        description: description.to_string(),
    }
}
