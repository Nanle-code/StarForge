use crate::utils::migration_ai;
use crate::utils::migration_ai::{AnalysisConfig, MigrationPlan};
use crate::utils::print as p;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::*;
use serde::Serialize;
use std::fs;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum MigrateAiCommands {
    /// Analyze compatibility between two contract versions
    Analyze(AnalyzeArgs),
    /// Identify breaking changes between contract versions
    BreakingChanges(BreakingChangesArgs),
    /// Generate migration code/scripts
    Generate(GenerateArgs),
    /// Suggest upgrade paths
    Suggest(SuggestArgs),
    /// Generate a complete migration plan
    Plan(PlanArgs),
}

#[derive(Args)]
pub struct AnalyzeArgs {
    /// Path to the old WASM file
    #[arg(long)]
    pub old_wasm: Option<PathBuf>,
    /// Path to the new WASM file
    #[arg(long)]
    pub new_wasm: Option<PathBuf>,
    /// Old SDK version (e.g. 20.0.0)
    #[arg(long)]
    pub old_sdk_version: Option<String>,
    /// New SDK version (e.g. 21.0.0)
    #[arg(long)]
    pub new_sdk_version: Option<String>,
    /// Old Soroban protocol version
    #[arg(long)]
    pub old_protocol_version: Option<u32>,
    /// New Soroban protocol version
    #[arg(long)]
    pub new_protocol_version: Option<u32>,
    /// Contract name
    #[arg(long)]
    pub contract: Option<String>,
    /// Old spec entries file (one per line, for testing)
    #[arg(long)]
    pub old_spec_file: Option<PathBuf>,
    /// New spec entries file (one per line, for testing)
    #[arg(long)]
    pub new_spec_file: Option<PathBuf>,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
    /// Output plan to file
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Args)]
pub struct BreakingChangesArgs {
    /// Path to the old WASM file
    #[arg(long)]
    pub old_wasm: Option<PathBuf>,
    /// Path to the new WASM file
    #[arg(long)]
    pub new_wasm: Option<PathBuf>,
    /// Old SDK version
    #[arg(long)]
    pub old_sdk_version: Option<String>,
    /// New SDK version
    #[arg(long)]
    pub new_sdk_version: Option<String>,
    /// Old Soroban protocol version
    #[arg(long)]
    pub old_protocol_version: Option<u32>,
    /// New Soroban protocol version
    #[arg(long)]
    pub new_protocol_version: Option<u32>,
    /// Old spec entries file
    #[arg(long)]
    pub old_spec_file: Option<PathBuf>,
    /// New spec entries file
    #[arg(long)]
    pub new_spec_file: Option<PathBuf>,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
    /// Fail with exit code 1 if breaking changes found
    #[arg(long, default_value = "false")]
    pub fail_on_breaking: bool,
}

#[derive(Args)]
pub struct GenerateArgs {
    /// Path to the old WASM file
    #[arg(long)]
    pub old_wasm: Option<PathBuf>,
    /// Path to the new WASM file
    #[arg(long)]
    pub new_wasm: Option<PathBuf>,
    /// Old SDK version
    #[arg(long)]
    pub old_sdk_version: Option<String>,
    /// New SDK version
    #[arg(long)]
    pub new_sdk_version: Option<String>,
    /// Contract name
    #[arg(long)]
    pub contract: Option<String>,
    /// Old spec entries file
    #[arg(long)]
    pub old_spec_file: Option<PathBuf>,
    /// New spec entries file
    #[arg(long)]
    pub new_spec_file: Option<PathBuf>,
    /// Output file for generated migration code (.rs)
    #[arg(long, default_value = "migration.rs")]
    pub output: PathBuf,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct SuggestArgs {
    /// Path to the old WASM file
    #[arg(long)]
    pub old_wasm: Option<PathBuf>,
    /// Path to the new WASM file
    #[arg(long)]
    pub new_wasm: Option<PathBuf>,
    /// Old SDK version
    #[arg(long)]
    pub old_sdk_version: Option<String>,
    /// New SDK version
    #[arg(long)]
    pub new_sdk_version: Option<String>,
    /// Contract name
    #[arg(long)]
    pub contract: Option<String>,
    /// Old spec entries file
    #[arg(long)]
    pub old_spec_file: Option<PathBuf>,
    /// New spec entries file
    #[arg(long)]
    pub new_spec_file: Option<PathBuf>,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
    /// Output to file
    #[arg(long)]
    pub output: Option<PathBuf>,
}

#[derive(Args)]
pub struct PlanArgs {
    /// Path to the old WASM file
    #[arg(long)]
    pub old_wasm: Option<PathBuf>,
    /// Path to the new WASM file
    #[arg(long)]
    pub new_wasm: Option<PathBuf>,
    /// Old SDK version
    #[arg(long)]
    pub old_sdk_version: Option<String>,
    /// New SDK version
    #[arg(long)]
    pub new_sdk_version: Option<String>,
    /// Old Soroban protocol version
    #[arg(long)]
    pub old_protocol_version: Option<u32>,
    /// New Soroban protocol version
    #[arg(long)]
    pub new_protocol_version: Option<u32>,
    /// Contract name
    #[arg(long)]
    pub contract: Option<String>,
    /// Old spec entries file
    #[arg(long)]
    pub old_spec_file: Option<PathBuf>,
    /// New spec entries file
    #[arg(long)]
    pub new_spec_file: Option<PathBuf>,
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
    /// Output plan to file
    #[arg(long)]
    pub output: Option<PathBuf>,
}

pub fn handle(cmd: MigrateAiCommands) -> Result<()> {
    match cmd {
        MigrateAiCommands::Analyze(args) => handle_analyze(args),
        MigrateAiCommands::BreakingChanges(args) => handle_breaking_changes(args),
        MigrateAiCommands::Generate(args) => handle_generate(args),
        MigrateAiCommands::Suggest(args) => handle_suggest(args),
        MigrateAiCommands::Plan(args) => handle_plan(args),
    }
}

fn load_spec_entries(
    wasm_path: Option<&PathBuf>,
    spec_file: Option<&PathBuf>,
) -> Result<Vec<String>> {
    if let Some(path) = spec_file {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read spec file: {}", path.display()))?;
        return Ok(content.lines().map(|l| l.to_string()).collect());
    }

    if let Some(path) = wasm_path {
        let wasm_bytes = migration_ai::read_wasm_file(path)?;
        return migration_ai::extract_spec_entries(&wasm_bytes);
    }

    Ok(Vec::new())
}

// Each parameter is an independent, named input (CLI flags / distinct config
// values); bundling them into a struct here would add indirection without
// reducing real complexity.
#[allow(clippy::too_many_arguments)]
fn build_analysis_config(
    old_specs: &[String],
    new_specs: &[String],
    old_wasm: Option<&PathBuf>,
    new_wasm: Option<&PathBuf>,
    old_sdk_version: Option<String>,
    new_sdk_version: Option<String>,
    old_protocol_version: Option<u32>,
    new_protocol_version: Option<u32>,
    contract: Option<String>,
) -> Result<AnalysisConfig> {
    let old_sdk = old_sdk_version.or_else(|| migration_ai::extract_sdk_version(old_specs));
    let new_sdk = new_sdk_version.or_else(|| migration_ai::extract_sdk_version(new_specs));
    let old_proto =
        old_protocol_version.or_else(|| migration_ai::extract_protocol_version(old_specs));
    let new_proto =
        new_protocol_version.or_else(|| migration_ai::extract_protocol_version(new_specs));

    Ok(AnalysisConfig {
        old_wasm: old_wasm.map(|p| p.display().to_string()),
        new_wasm: new_wasm.map(|p| p.display().to_string()),
        old_source: None,
        new_source: None,
        old_sdk_version: old_sdk,
        new_sdk_version: new_sdk,
        old_protocol_version: old_proto,
        new_protocol_version: new_proto,
        contract_name: contract,
    })
}

// Each parameter is an independent, named input (CLI flags / distinct config
// values); bundling them into a struct here would add indirection without
// reducing real complexity.
#[allow(clippy::too_many_arguments)]
fn load_or_build_plan(
    old_wasm: Option<&PathBuf>,
    new_wasm: Option<&PathBuf>,
    old_spec_file: Option<&PathBuf>,
    new_spec_file: Option<&PathBuf>,
    old_sdk_version: Option<String>,
    new_sdk_version: Option<String>,
    old_protocol_version: Option<u32>,
    new_protocol_version: Option<u32>,
    contract: Option<String>,
) -> Result<MigrationPlan> {
    let old_specs = load_spec_entries(old_wasm, old_spec_file)?;
    let new_specs = load_spec_entries(new_wasm, new_spec_file)?;

    let config = build_analysis_config(
        &old_specs,
        &new_specs,
        old_wasm,
        new_wasm,
        old_sdk_version,
        new_sdk_version,
        old_protocol_version,
        new_protocol_version,
        contract,
    )?;

    let old_hash = match old_wasm {
        Some(path) => {
            let bytes = migration_ai::read_wasm_file(path)?;
            migration_ai::extract_wasm_hash(&bytes)
        }
        None => "old_hash".to_string(),
    };

    let new_hash = match new_wasm {
        Some(path) => {
            let bytes = migration_ai::read_wasm_file(path)?;
            migration_ai::extract_wasm_hash(&bytes)
        }
        None => "new_hash".to_string(),
    };

    migration_ai::analyze_contract_compatibility(
        &old_specs, &new_specs, &old_hash, &new_hash, &config,
    )
}

fn handle_analyze(args: AnalyzeArgs) -> Result<()> {
    p::header("AI-Assisted Migration Analysis");

    let plan = load_or_build_plan(
        args.old_wasm.as_ref(),
        args.new_wasm.as_ref(),
        args.old_spec_file.as_ref(),
        args.new_spec_file.as_ref(),
        args.old_sdk_version,
        args.new_sdk_version,
        args.old_protocol_version,
        args.new_protocol_version,
        args.contract,
    )?;

    if args.json {
        print_json(&plan)?;
        return Ok(());
    }

    print_plan_summary(&plan);

    if let Some(output) = &args.output {
        write_plan_output(&plan, output)?;
    }

    Ok(())
}

fn handle_breaking_changes(args: BreakingChangesArgs) -> Result<()> {
    p::header("Breaking Changes Analysis");

    let plan = load_or_build_plan(
        args.old_wasm.as_ref(),
        args.new_wasm.as_ref(),
        args.old_spec_file.as_ref(),
        args.new_spec_file.as_ref(),
        args.old_sdk_version,
        args.new_sdk_version,
        args.old_protocol_version,
        args.new_protocol_version,
        None,
    )?;

    if args.json {
        print_json(&plan.breaking_changes)?;
        return Ok(());
    }

    if plan.breaking_changes.is_empty() {
        p::success("No breaking changes detected between the two versions.");
        return Ok(());
    }

    for (i, change) in plan.breaking_changes.iter().enumerate() {
        let severity_color = match change.severity {
            crate::utils::migration_ai::Severity::Critical => "critical".red().bold(),
            crate::utils::migration_ai::Severity::Major => "major".yellow().bold(),
            crate::utils::migration_ai::Severity::Minor => "minor".cyan(),
            crate::utils::migration_ai::Severity::Info => "info".dimmed(),
        };
        println!(
            "\n  {}. [{}] {}",
            (i + 1).to_string().white().bold(),
            severity_color,
            change.title.white().bold(),
        );
        println!("     {}", change.description.dimmed());
        println!(
            "     {}",
            format!("Guide: {}", change.migration_guide).dimmed()
        );
    }

    if args.fail_on_breaking && !plan.breaking_changes.is_empty() {
        anyhow::bail!(
            "Found {} breaking change(s). Use --no-fail-on-breaking to suppress.",
            plan.breaking_changes.len()
        );
    }

    Ok(())
}

fn handle_generate(args: GenerateArgs) -> Result<()> {
    p::header("Generate Migration Code");

    let plan = load_or_build_plan(
        args.old_wasm.as_ref(),
        args.new_wasm.as_ref(),
        args.old_spec_file.as_ref(),
        args.new_spec_file.as_ref(),
        args.old_sdk_version,
        args.new_sdk_version,
        None,
        None,
        args.contract.clone(),
    )?;

    if args.json {
        print_json(&plan)?;
        return Ok(());
    }

    let storage_changes = &plan.storage_changes;
    let contract_name = args.contract.unwrap_or_else(|| "Contract".to_string());
    let _sdk_version = plan.to_version.clone();

    let mut code = String::new();
    code.push_str(&format!(
        "// Migration code generated by `starforge migrate-ai generate`\n\
         // From: {} → {}\n\
         // SDK: {} → {}\n\
         // Compatibility: {}\n\n",
        plan.from_version, plan.to_version, plan.from_version, plan.to_version, plan.compatibility,
    ));

    code.push_str("use soroban_sdk::{self, Address, Env};\n\n");
    code.push_str("/// Migrate contract storage from the old layout to the new layout.\n");
    code.push_str("/// Call this after upgrading the contract WASM.\n");
    code.push_str("#[allow(unused)]\n");
    code.push_str(&format!(
        "pub fn migrate_{}(env: &Env, admin: Address) {{\n",
        contract_name.to_lowercase()
    ));
    code.push_str("    admin.require_auth();\n\n");

    if !storage_changes.is_empty() {
        code.push_str("    // ── Storage migration steps ──\n");
        for sc in storage_changes {
            match sc.change_type.as_str() {
                "removed" => {
                    code.push_str(&format!(
                        "    // Remove deprecated key `{}`\n    env.storage().instance().remove(&\"{}\");\n\n",
                        sc.key, sc.key
                    ));
                }
                "added" => {
                    code.push_str(&format!(
                        "    // Initialize new storage key `{}`\n\
                         if !env.storage().instance().has(&\"{}\") {{\n\
                         env.storage().instance().set(&\"{}\", &soroban_sdk::Vec::<soroban_sdk::Val>::new(env));\n\
                         }}\n\n",
                        sc.key, sc.key, sc.key
                    ));
                }
                _ => {}
            }
        }
    } else {
        code.push_str("    // No storage layout changes detected.\n");
        code.push_str("    // Add any custom migration logic here.\n\n");
    }

    code.push_str("    // Emit migration event\n");
    code.push_str("    env.events().publish(\n");
    code.push_str("        (soroban_sdk::symbol_short!(\"migrated\"),),\n");
    code.push_str("        (&sdk_version,),\n");
    code.push_str("    );\n");
    code.push_str("}\n\n");

    code.push_str("#[cfg(test)]\n");
    code.push_str("mod tests {\n");
    code.push_str("    use super::*;\n");
    code.push_str("    use soroban_sdk::Env;\n\n");
    code.push_str("    #[test]\n");
    code.push_str(&format!(
        "    fn test_migrate_{}() {{\n",
        contract_name.to_lowercase()
    ));
    code.push_str("        let env = Env::default();\n");
    code.push_str("        let admin = Address::generate(&env);\n");
    code.push_str("        env.mock_all_auths();\n\n");
    code.push_str("        // Set up pre-migration storage state\n\n");
    code.push_str("        // Run migration\n");
    code.push_str(&format!(
        "        migrate_{}(&env, admin.clone());\n\n",
        contract_name.to_lowercase()
    ));
    code.push_str("        // Assert post-migration state\n\n");
    code.push_str("    }\n");
    code.push_str("}\n");

    fs::write(&args.output, &code).with_context(|| {
        format!(
            "Failed to write migration code to {}",
            args.output.display()
        )
    })?;

    p::success(&format!(
        "Wrote migration code to {}",
        args.output.display()
    ));
    p::info("Review and customize the generated migration function before deploying.");

    Ok(())
}

fn handle_suggest(args: SuggestArgs) -> Result<()> {
    p::header("Upgrade Path Suggestions");

    let plan = load_or_build_plan(
        args.old_wasm.as_ref(),
        args.new_wasm.as_ref(),
        args.old_spec_file.as_ref(),
        args.new_spec_file.as_ref(),
        args.old_sdk_version,
        args.new_sdk_version,
        None,
        None,
        args.contract,
    )?;

    if args.json {
        print_json(&plan.suggestions)?;
        return Ok(());
    }

    if plan.suggestions.is_empty() {
        p::success("No upgrade suggestions available. The contracts are fully compatible.");
        return Ok(());
    }

    println!();
    for (i, suggestion) in plan.suggestions.iter().enumerate() {
        let priority_color = match suggestion.priority.as_str() {
            "high" => "HIGH".red().bold(),
            "medium" => "MEDIUM".yellow().bold(),
            _ => "LOW".cyan(),
        };

        println!(
            "  {}. [{}] {}",
            (i + 1).to_string().white().bold(),
            priority_color,
            suggestion.title.white().bold(),
        );
        println!("     {}", suggestion.description.dimmed());
        println!(
            "     {} {:<10} {} {}",
            "Effort:".dimmed(),
            suggestion.effort.white(),
            "Risk:".dimmed(),
            suggestion.risk.white()
        );

        if let Some(snippet) = &suggestion.code_snippet {
            for line in snippet.lines() {
                println!("     {}", format!("  {}", line).bright_cyan());
            }
        }
        println!();
    }

    if let Some(output) = &args.output {
        let content = plan
            .suggestions
            .iter()
            .map(|s| format!("[{}] {}: {}", s.priority, s.title, s.description))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(output, &content)
            .with_context(|| format!("Failed to write suggestions to {}", output.display()))?;
        p::success(&format!("Wrote suggestions to {}", output.display()));
    }

    Ok(())
}

fn handle_plan(args: PlanArgs) -> Result<()> {
    p::header("Migration Plan");

    let plan = load_or_build_plan(
        args.old_wasm.as_ref(),
        args.new_wasm.as_ref(),
        args.old_spec_file.as_ref(),
        args.new_spec_file.as_ref(),
        args.old_sdk_version,
        args.new_sdk_version,
        args.old_protocol_version,
        args.new_protocol_version,
        args.contract,
    )?;

    if args.json {
        print_json(&plan)?;
        return Ok(());
    }

    print_plan_summary(&plan);

    println!(
        "\n{}",
        "Recommended Migration Steps:".white().bold().underline()
    );
    for step in &plan.steps {
        println!(
            "\n  {}. {}",
            step.order.to_string().cyan().bold(),
            step.action.replace('_', " ").to_uppercase().white().bold(),
        );
        println!("     {}", step.description.dimmed());
        if let Some(cmd) = &step.command {
            println!("     {}", format!("$ {}", cmd).bright_cyan());
        }
        if let Some(code) = &step.code_template {
            println!();
            for line in code.lines() {
                println!("     {}", format!("  {}", line).bright_cyan());
            }
        }
        if let Some(val) = &step.validation {
            println!("     {} {}", "✓".green(), val.dimmed());
        }
    }

    if let Some(strategy) = &plan.rollback_strategy {
        println!("\n{}", "Rollback Strategy:".white().bold().underline());
        for line in strategy.lines() {
            println!("  {}", line.dimmed());
        }
    }

    if let Some(output) = &args.output {
        write_plan_output(&plan, output)?;
    }

    Ok(())
}

fn print_plan_summary(plan: &MigrationPlan) {
    let _compat_color = match plan.compatibility {
        crate::utils::migration_ai::Compatibility::FullyCompatible => {
            "fully compatible".green().bold()
        }
        crate::utils::migration_ai::Compatibility::CompatibleWithMigration => {
            "compatible with migration".yellow().bold()
        }
        crate::utils::migration_ai::Compatibility::Incompatible => "incompatible".red().bold(),
    };

    println!();
    p::separator();
    p::kv("From version", &plan.from_version);
    p::kv("To version", &plan.to_version);
    p::kv("Compatibility", &plan.compatibility.to_string());
    p::kv("Breaking changes", &plan.breaking_changes.len().to_string());
    p::kv("Storage changes", &plan.storage_changes.len().to_string());
    p::kv("Estimated effort", &plan.estimated_effort);
    p::separator();

    if !plan.breaking_changes.is_empty() {
        println!("\n{}", "Breaking Changes:".red().bold());
        for (i, change) in plan.breaking_changes.iter().enumerate() {
            println!(
                "  {}. [{}] {}",
                (i + 1).to_string().dimmed(),
                change.severity.to_string().to_uppercase(),
                change.title.white().bold()
            );
            println!("     {}", change.description.dimmed());
        }
    }

    if !plan.storage_changes.is_empty() {
        println!("\n{}", "Storage Changes:".yellow().bold());
        for sc in &plan.storage_changes {
            let change_type_color = match sc.change_type.as_str() {
                "removed" => "-".red(),
                "added" => "+".green(),
                _ => "~".yellow(),
            };
            println!(
                "  {} {} ({})",
                change_type_color,
                sc.key.white(),
                sc.description.dimmed()
            );
        }
    }

    if !plan.suggestions.is_empty() {
        let high_count = plan
            .suggestions
            .iter()
            .filter(|s| s.priority == "high")
            .count();
        let medium_count = plan
            .suggestions
            .iter()
            .filter(|s| s.priority == "medium")
            .count();
        let low_count = plan
            .suggestions
            .iter()
            .filter(|s| s.priority == "low")
            .count();
        println!(
            "\n{} {} high, {} medium, {} low priority suggestions",
            "Suggestions:".cyan().bold(),
            high_count.to_string().red().bold(),
            medium_count.to_string().yellow().bold(),
            low_count.to_string().cyan()
        );
    }

    if plan.compatibility == crate::utils::migration_ai::Compatibility::Incompatible {
        println!();
        p::warn("Contract versions are incompatible. Migration is required and may involve significant rework.");
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn write_plan_output(plan: &MigrationPlan, path: &PathBuf) -> Result<()> {
    let json = serde_json::to_string_pretty(plan)?;
    fs::write(path, &json)
        .with_context(|| format!("Failed to write migration plan to {}", path.display()))?;
    p::success(&format!("Wrote migration plan to {}", path.display()));
    Ok(())
}
