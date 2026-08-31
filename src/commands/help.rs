//! CLI surface for the AI Contextual Help System.
//!
//! Exposes a single top-level subcommand (`starforge help …`) with three
//! workflow modes plus a settings sub-mode:
//!
//!   * `starforge help`                      — overview of the system.
//!   * `starforge help <command>`            — command-specific help derived
//!     from the curated metadata table.
//!   * `starforge help --workflow [<slug>]`  — list or expand multi-step
//!     workflows.
//!   * `starforge help --why`                — run troubleshooting on the
//!     most recent error (favouring the AI debug findings).
//!   * `starforge help --settings`           — customise the surfaced help
//!     categories (--enable / --disable).
//!
//! All output flows through [[crate::utils::print]] so the look-and-feel
//! matches the rest of the CLI.

use crate::utils::{context_help, history, print as p};
use anyhow::Result;
use clap::Args;
use colored::*;
use std::collections::HashSet;

/// Subcommand arguments for `starforge help`.
#[derive(Args, Debug, Default)]
pub struct HelpArgs {
    /// The command you want help with (e.g. `deploy`, `wallet`).
    #[arg(value_name = "COMMAND", conflicts_with_all = ["workflow", "why", "settings"])]
    pub command: Option<String>,

    /// List workflows, or expand the named one when combined with a value.
    #[arg(long, short = 'w', conflicts_with_all = ["why", "settings"])]
    pub workflow: bool,

    /// Explain what went wrong in your most recent command (best-effort using
    /// the last telemetry event still on disk; otherwise prompts to wrap an
    /// error in `--error`).
    #[arg(long, conflicts_with_all = ["workflow", "settings"])]
    pub why: bool,

    /// Show or change help settings (categories enabled/disabled).
    #[arg(long, conflicts_with_all = ["workflow", "why"])]
    pub settings: bool,

    /// Enable a tip category for this invocation. Repeatable.
    #[arg(long, value_name = "CATEGORY", conflicts_with_all = ["disable"])]
    pub enable: Vec<String>,

    /// Disable a tip category for this invocation. Repeatable.
    #[arg(long, value_name = "CATEGORY", conflicts_with_all = ["enable"])]
    pub disable: Vec<String>,

    /// Raw error string to troubleshoot (used with `--why`).
    #[arg(long, value_name = "ERROR_TEXT", requires = "why")]
    pub error: Option<String>,

    /// Verbose output (more sections, more tips).
    #[arg(long, short = 'v')]
    pub verbose: bool,
}

/// Handle the help subcommand.
pub async fn handle(args: HelpArgs) -> Result<()> {
    if args.settings {
        return handle_settings(&args).await;
    }
    if args.why {
        return handle_why(&args).await;
    }
    if args.workflow {
        return handle_workflow(&args).await;
    }
    if let Some(cmd) = args.command.as_deref() {
        return handle_command(cmd, &args).await;
    }
    handle_overview().await
}

// ── Top-level overview ────────────────────────────────────────────────────────

async fn handle_overview() -> Result<()> {
    p::header("StarForge AI Contextual Help");
    println!();
    p::info(
        "Intelligent help that adapts to the command, your recent activity, and any error message.",
    );
    println!();

    p::separator();
    println!("  {}", "Usage".bright_white().bold());
    p::separator();
    println!(
        "  {} {}",
        "→".cyan(),
        "starforge help <command>            Show context-aware help for a command".dimmed()
    );
    println!(
        "  {} {}",
        "→".cyan(),
        "starforge help --workflow [<slug>]  List (or expand) multi-step workflows".dimmed()
    );
    println!(
        "  {} {}",
        "→".cyan(),
        "starforge help --why [--error <t>]  Explain a recent error and suggest next steps"
            .dimmed()
    );
    println!(
        "  {} {}",
        "→".cyan(),
        "starforge help --settings           Customise surfaced tip categories".dimmed()
    );
    println!();

    p::separator();
    println!(
        "  {} ({} total)",
        "Workflows".bright_white().bold(),
        context_help::workflow_count()
    );
    p::separator();
    for wf in workflows_iter() {
        println!(
            "  {} {} — {}\n      {} {}",
            "→".cyan(),
            wf.name.cyan().bold(),
            wf.description.dimmed(),
            "≈".dimmed(),
            wf.approx_duration.dimmed()
        );
    }
    println!();

    // Show available commands (subset that has dedicated metadata).
    p::separator();
    println!(
        "  {} (top {})",
        "Commands with rich help".bright_white().bold(),
        context_help::commands_with_help()
    );
    p::separator();
    for cmd in context_help::all_command_names().iter().take(12) {
        println!(
            "  {} {}    {}",
            "→".cyan(),
            format!("{:<14}", cmd).bright_white(),
            context_help::command_summary(cmd).unwrap_or("").dimmed()
        );
    }
    println!();
    p::info("Run `starforge help --enable tip --disable workflow` to customise what's shown.");
    Ok(())
}

// ── Command-specific help ─────────────────────────────────────────────────────

async fn handle_command(cmd: &str, args: &HelpArgs) -> Result<()> {
    let history_entries = load_history_safe().await?;
    let enabled: Vec<&'static str> = context_help::normalise_categories(args.enable.iter());
    let disabled: Vec<&'static str> = context_help::normalise_categories(args.disable.iter());

    let ctx = context_help::HelpContext {
        command: cmd,
        last_error: args.error.as_deref(),
        history: &history_entries,
        enabled_categories: &enabled,
        disabled_categories: &disabled,
    };
    let help = context_help::generate_help(&ctx);

    let canonical = cmd.trim().to_lowercase();
    p::header(&format!("Help: {}", canonical));
    let summary_line =
        context_help::command_summary(&canonical).unwrap_or(help.description.as_str());
    println!("  {}", summary_line.dimmed());
    println!();

    let expertise = context_help::expertise_level(&canonical, &history_entries);
    p::kv("Detected expertise", expertise.label());
    p::kv("History entries", &history_entries.len().to_string());
    if args.verbose {
        // Surface category settings so users can see what they're filtering.
        let en_str = enabled.join(", ");
        p::kv(
            "Enabled categories",
            if enabled.is_empty() { "(all)" } else { &en_str },
        );
        let dis_str = disabled.join(", ");
        p::kv(
            "Disabled categories",
            if disabled.is_empty() {
                "(none)"
            } else {
                &dis_str
            },
        );
    }
    println!();

    if !help.flags_and_examples.is_empty() {
        p::header("Examples & Flags");
        for line in &help.flags_and_examples {
            if line.trim_start().starts_with("--") || line.trim_start().starts_with('-') {
                println!("    {}", line.dimmed());
            } else {
                println!("  {} {}", "→".cyan(), line.bright_white());
            }
        }
        println!();
    }

    if !help.best_practice_tips.is_empty() {
        p::header("Best Practice Tips");
        for tip in &help.best_practice_tips {
            println!("  {} {}", "•".cyan(), tip.bright_white());
        }
        println!();
    }

    if !help.workflow_suggestions.is_empty() {
        p::header("Suggested Workflows");
        for line in &help.workflow_suggestions {
            println!("  {} {}", "→".cyan(), line.bright_white());
        }
        println!();
    }

    if !help.predicted_issues.is_empty() {
        p::header("Predicted Issues");
        for w in &help.predicted_issues {
            println!("  {} {}", "⚠".yellow().bold(), w.bright_white());
        }
        println!();
    }

    if !help.troubleshooting_steps.is_empty() {
        p::header("Troubleshooting");
        for s in &help.troubleshooting_steps {
            println!("  {} {}", "→".cyan(), s.bright_white());
        }
        println!();
    }

    if !help.related_commands.is_empty() {
        p::header("Related Commands");
        p::separator();
        for r in &help.related_commands {
            if let Some(about) = context_help::command_summary(r) {
                println!(
                    "  {} {}  {}",
                    "→".cyan(),
                    format!("{:<14}", r).bright_white(),
                    about.dimmed()
                );
            } else {
                println!(
                    "  {} {}",
                    "→".cyan(),
                    format!("starforge help {}", r).bright_white()
                );
            }
        }
        println!();
    }

    if help.has_no_command_metadata() && !context_help::is_known_command(&canonical) {
        p::info(&format!(
            "No specific contextual help is available for '{}'. Try `starforge {} --help` for the full flag list.",
            canonical, canonical
        ));
    }
    Ok(())
}

// ── Workflow listing / expansion ──────────────────────────────────────────────

async fn handle_workflow(args: &HelpArgs) -> Result<()> {
    match args.command.as_deref() {
        None => list_workflows(),
        Some(slug) => expand_workflow(slug),
    }
}

fn list_workflows() -> Result<()> {
    p::header("Multi-Step Workflows");
    p::separator();
    for wf in workflows_iter() {
        println!(
            "  {} {} — {}\n      {} {}",
            "→".cyan(),
            wf.name.cyan().bold(),
            wf.description.dimmed(),
            "≈".dimmed(),
            wf.approx_duration.dimmed()
        );
        println!();
    }
    p::info("Run `starforge help --workflow <slug>` to see every step.");
    Ok(())
}

fn expand_workflow(slug: &str) -> Result<()> {
    let steps = context_help::workflow_steps(slug);
    let desc = context_help::workflow_description(slug);
    let dur = context_help::workflow_duration(slug);

    match (steps, desc, dur) {
        (Some(steps), Some(desc), Some(dur)) => {
            p::header(&format!("Workflow: {}", slug));
            println!("  {}", desc.dimmed());
            p::kv("Approx duration", dur);
            println!();
            p::separator();
            for (i, step) in steps.iter().enumerate() {
                println!(
                    "  {} {}",
                    format!("step {} ›", i + 1).dimmed(),
                    step.bright_white()
                );
            }
            println!();
            p::info("Skip steps you've already completed.");
        }
        _ => {
            p::header(&format!("Workflow: {}", slug));
            p::info(&format!(
                "No workflow named '{}'. Run `starforge help --workflow` to list all.",
                slug
            ));
        }
    }
    Ok(())
}

// ── "Why did this fail?" ──────────────────────────────────────────────────────

async fn handle_why(args: &HelpArgs) -> Result<()> {
    let error_text: Option<String> = match args.error.clone() {
        Some(text) => Some(text),
        None => args.command.clone(),
    };

    let error_text = match error_text {
        Some(t) if !t.trim().is_empty() => t,
        _ => {
            p::header("Troubleshoot an error");
            p::separator();
            p::info("No error text supplied. Pass the error message as the command argument:");
            println!();
            println!(
                "    {}",
                "starforge help --why \"require_auth failed for caller\"".bright_white()
            );
            println!();
            return Ok(());
        }
    };

    p::header("Troubleshooting");
    p::separator();
    println!("  {}", format!("error: {}", error_text).dimmed());
    println!();

    // Use the smarter AI debugger first, then fall through to quick-fixes.
    let report = crate::utils::ai_debugger::analyse(&error_text, None, None, None);
    if !report.findings.is_empty() {
        println!("  {}", "Findings".bright_white().bold());
        for f in &report.findings {
            println!(
                "  {} {} [{}]  {}",
                f.severity.label(),
                f.id.cyan(),
                format!("{:?}", f.severity).dimmed(),
                f.title.bright_white()
            );
        }
        println!();
        for f in &report.findings {
            println!(
                "  {} {}\n      Root cause: {}\n      Fix:        {}",
                "→".cyan(),
                f.title.bright_white(),
                f.root_cause.dimmed(),
                f.fix_suggestion.dimmed()
            );
            println!();
        }
    }

    p::header("Quick fixes");
    for s in context_help::troubleshoot(&error_text) {
        println!("  {} {}", "→".cyan(), s.bright_white());
    }
    println!();

    // If we have an associated command hint for the context, surface it.
    if let Some(cmd) = error_text
        .split_whitespace()
        .next()
        .and_then(|w| w.strip_suffix(':'))
    {
        if let Some(meta) = context_help::command_summary(cmd) {
            println!("  {} {}", "💡".cyan(), meta.dimmed());
            println!();
        }
    }

    // Surface the workflow for try/fix flow when relevant.
    p::header("Next steps");
    println!(
        "  {} {}",
        "→".cyan(),
        "starforge help --workflow troubleshoot-error   # guided recovery flow".bright_white()
    );
    Ok(())
}

// ── Settings ──────────────────────────────────────────────────────────────────

async fn handle_settings(args: &HelpArgs) -> Result<()> {
    p::header("Help Settings");
    p::separator();
    println!(
        "  {} {}",
        "→".cyan(),
        "Categories control which sections appear in `starforge help <command>`.".dimmed()
    );
    println!();

    p::header("Available Categories");
    p::separator();
    for c in context_help::CATEGORIES {
        let enabled = if !args.enable.is_empty() {
            // Normalise through the engine so aliases (“tips”, “troubleshooting”)
            // are accepted on this command-line.
            let normalised: HashSet<&'static str> =
                context_help::normalise_categories(args.enable.iter())
                    .into_iter()
                    .collect();
            normalised.contains(c)
        } else if !args.disable.is_empty() {
            let normalised: HashSet<&'static str> =
                context_help::normalise_categories(args.disable.iter())
                    .into_iter()
                    .collect();
            !normalised.contains(c)
        } else {
            true
        };
        let marker = if enabled {
            "✓".green()
        } else {
            "·".dimmed()
        };
        println!("  {} {}", marker, c.bright_white());
    }
    println!();

    if !args.disable.is_empty() || !args.enable.is_empty() {
        p::header("Configuration");
        p::separator();
        for d in &args.disable {
            p::kv(
                &format!("Disabled: {}", d),
                "use --enable to re-enable for this run",
            );
        }
        for e in &args.enable {
            p::kv(&format!("Enabled:  {}", e), "for this invocation only");
        }
        println!();
    } else {
        p::header("Examples");
        p::separator();
        println!(
            "  {} {}",
            "→".cyan(),
            "starforge help deploy --disable workflow".dimmed()
        );
        println!(
            "  {} {}",
            "→".cyan(),
            "starforge help deploy --enable tip,related".dimmed()
        );
        println!(
            "  {} {}",
            "→".cyan(),
            "starforge help --workflow gas-debugging".dimmed()
        );
        println!();
    }
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

struct WorkflowEntry {
    name: &'static str,
    description: &'static str,
    approx_duration: &'static str,
}

fn workflows_iter() -> impl Iterator<Item = WorkflowEntry> {
    use crate::utils::help_metadata::WORKFLOWS;
    WORKFLOWS.iter().map(|w| WorkflowEntry {
        name: w.name,
        description: w.description,
        approx_duration: w.approx_duration,
    })
}

/// Load command history, swallowing I/O errors so help still works on
/// machines without history (e.g. CI).
async fn load_history_safe() -> Result<Vec<history::HistoryEntry>> {
    let config_dir = crate::utils::config::config_dir();
    Ok(history::load_history(&config_dir).unwrap_or_default())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::context_help::normalise_categories;

    #[test]
    fn normalise_accepts_plurals_and_case() {
        let raw = vec![
            "tip".to_string(),
            "TROUBLESHOOT".to_string(),
            "unknown".to_string(),
        ];
        let out = normalise_categories(raw.iter());
        assert_eq!(out, vec!["tip", "troubleshoot"]);
    }

    #[test]
    fn help_args_default_is_empty() {
        let a = HelpArgs::default();
        assert!(a.command.is_none());
        assert!(!a.workflow);
        assert!(!a.why);
        assert!(!a.settings);
        assert!(a.enable.is_empty());
        assert!(a.disable.is_empty());
        assert!(!a.verbose);
        assert!(a.error.is_none());
    }
}
