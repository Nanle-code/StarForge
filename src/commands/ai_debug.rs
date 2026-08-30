//! `starforge ai-debug` — AI Contract Debugging Assistant.
//!
//! Analyses Soroban contract errors, stack traces, and variable state to
//! provide clear explanations, root-cause analysis, bug identification,
//! fix suggestions, and guided reproduction steps.
//!
//! ## Sub-commands
//! - `analyse`   — Analyse an error message and/or stack trace
//! - `explain`   — Explain a specific error code or category
//! - `inspect`   — Inspect variable state for suspicious values
//! - `test`      — Analyse test failure output and suggest fixes

use crate::utils::ai_debugger::{self, Severity};
use crate::utils::{ai_feedback, ai_telemetry, print as p};
use anyhow::Result;
use clap::{Args, Subcommand};
use colored::*;
use std::fs;
use std::path::PathBuf;

// ── Sub-command enum ──────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum AiDebugCommands {
    /// Analyse a contract error message, with optional stack trace and variables
    Analyse(AnalyseArgs),
    /// Explain a known error category (auth, arithmetic, storage, token, wasm, ttl, type, compilation, configuration)
    Explain(ExplainArgs),
    /// Inspect variable state for potential bugs (pass name=value pairs)
    Inspect(InspectArgs),
    /// Analyse test failure output and suggest fixes
    Test(TestArgs),
    /// Record whether a suggested fix helped (feeds the "learn from common errors" system)
    Feedback(FeedbackArgs),
    /// Show which fixes have historically been rated helpful, to prioritise common errors
    Learned(LearnedArgs),
    /// Predict bugs, suggest breakpoints, and visualize source execution paths
    Source(SourceArgs),
}

// ── Analyse sub-command ───────────────────────────────────────────────────────

#[derive(Args)]
pub struct AnalyseArgs {
    /// The error message to analyse (quote the full message)
    pub error: String,

    /// Raw stack trace string (optional; use quotes or --stack-trace-file)
    #[arg(long)]
    pub stack_trace: Option<String>,

    /// Path to a file containing the stack trace
    #[arg(long)]
    pub stack_trace_file: Option<PathBuf>,

    /// Variable name=value pairs for state inspection (e.g. amount=0 caller=None)
    #[arg(long = "var", value_name = "NAME=VALUE", num_args = 1..)]
    pub variables: Vec<String>,

    /// Output format: text | json
    #[arg(long, default_value = "text", value_parser = ["text", "json"])]
    pub format: String,

    /// When no local pattern matches, fall back to an AI provider for a plain-language
    /// explanation (requires STARFORGE_AI_API_KEY to be set)
    #[arg(long)]
    pub deep: bool,
}

// ── Feedback sub-command ──────────────────────────────────────────────────────

#[derive(Args)]
pub struct FeedbackArgs {
    /// The finding ID this feedback applies to (e.g. AUTH001, COMPILE001)
    pub finding_id: String,

    /// Mark the suggested fix as helpful
    #[arg(long, conflicts_with = "not_helpful")]
    pub helpful: bool,

    /// Mark the suggested fix as not helpful
    #[arg(long)]
    pub not_helpful: bool,

    /// Optional free-text comment
    #[arg(long)]
    pub comment: Option<String>,
}

// ── Learned sub-command ───────────────────────────────────────────────────────

#[derive(Args)]
pub struct LearnedArgs {
    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

// ── Explain sub-command ───────────────────────────────────────────────────────

#[derive(Args)]
pub struct ExplainArgs {
    /// Category to explain: auth | arithmetic | storage | token | panic | wasm | network | deployment | rollback | security | analytics | ttl | test | type
    pub category: String,
}

// ── Inspect sub-command ───────────────────────────────────────────────────────

#[derive(Args)]
pub struct InspectArgs {
    /// Variable name=value pairs to inspect (e.g. amount=0 balance=9999)
    #[arg(required = true, value_name = "NAME=VALUE", num_args = 1..)]
    pub variables: Vec<String>,
}

// ── Test sub-command ──────────────────────────────────────────────────────────

#[derive(Args)]
pub struct TestArgs {
    /// The test failure output to analyse (quote the full output)
    pub output: Option<String>,

    /// Path to a file containing test output (alternative to inline output)
    #[arg(long)]
    pub file: Option<PathBuf>,

    /// Also provide the originating error message for combined analysis
    #[arg(long)]
    pub error: Option<String>,
}

#[derive(Args)]
pub struct SourceArgs {
    /// Entry function used to build the execution path
    pub entry: String,
    /// Project directory to analyze
    #[arg(long, default_value = ".")]
    pub dir: PathBuf,
    /// Maximum internal call depth
    #[arg(long, default_value_t = 6)]
    pub depth: usize,
    /// Output the complete report as JSON
    #[arg(long)]
    pub json: bool,
}

// ── Top-level handler ─────────────────────────────────────────────────────────

pub async fn handle(cmd: AiDebugCommands) -> Result<()> {
    // Feature-flag gate (Stable category, default-on; admins can roll it back
    // for an entire fleet via `starforge feature-flags disable ai.debug`).
    crate::commands::feature_flags_cmd::require_feature("ai.debug")?;
    match cmd {
        AiDebugCommands::Analyse(args) => handle_analyse(args).await,
        AiDebugCommands::Explain(args) => handle_explain(args).await,
        AiDebugCommands::Inspect(args) => handle_inspect(args).await,
        AiDebugCommands::Test(args) => handle_test(args).await,
        AiDebugCommands::Feedback(args) => handle_feedback(args).await,
        AiDebugCommands::Learned(args) => handle_learned(args).await,
        AiDebugCommands::Source(args) => handle_source(args),
    }
}

fn handle_source(args: SourceArgs) -> Result<()> {
    let report =
        crate::utils::ai_debug_enhancement::analyze_project(&args.dir, &args.entry, args.depth)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    p::header("AI Source Debugging");
    println!(
        "{}",
        crate::utils::ai_debug_enhancement::render_execution_path(&report)
    );
    println!("  {}", "Suggested Breakpoints".cyan().bold());
    for breakpoint in &report.breakpoints {
        println!(
            "  {}:{} {} ({:.0}% confidence)",
            breakpoint.file.display(),
            breakpoint.line,
            breakpoint.reason,
            breakpoint.confidence * 100.0
        );
        println!("    inspect: {}", breakpoint.inspect.join(", "));
    }
    println!("\n  {}", "Bug Predictions".yellow().bold());
    for prediction in &report.predictions {
        println!(
            "  [{}] {}:{} {}",
            prediction.category,
            prediction.file.display(),
            prediction.line,
            prediction.evidence
        );
        println!("    root cause: {}", prediction.root_cause);
        println!("    fix: {}", prediction.fix);
    }
    for guidance in &report.guidance {
        println!("\n  {}", guidance);
    }
    Ok(())
}

// ── analyse handler ───────────────────────────────────────────────────────────

async fn handle_analyse(args: AnalyseArgs) -> Result<()> {
    // Resolve stack trace from inline string or file
    let stack_trace_owned: Option<String> = if let Some(file) = args.stack_trace_file {
        Some(fs::read_to_string(&file).map_err(|e| {
            anyhow::anyhow!("Could not read stack trace file {}: {}", file.display(), e)
        })?)
    } else {
        args.stack_trace
    };

    // Parse name=value variable pairs
    let variables = parse_variables(&args.variables)?;
    let vars_ref: Vec<(String, String)> = variables;

    let mut report = ai_debugger::analyse(
        &args.error,
        stack_trace_owned.as_deref(),
        if vars_ref.is_empty() {
            None
        } else {
            Some(&vars_ref)
        },
        None,
    );

    let mut deep_explanation: Option<String> = None;
    if args.deep && report.findings.is_empty() {
        match deep_explain_via_ai(&args.error).await {
            Ok(Some(explanation)) => deep_explanation = Some(explanation),
            Ok(None) => {
                report.overall_guidance.push_str(
                    "\n\nDeep explanation unavailable: set STARFORGE_AI_API_KEY to enable AI-powered fallback explanations.",
                );
            }
            Err(e) => {
                report
                    .overall_guidance
                    .push_str(&format!("\n\nDeep explanation failed: {}", e));
            }
        }
    }

    if args.format == "json" {
        if let Some(explanation) = &deep_explanation {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "report": report,
                    "deep_explanation": explanation,
                }))?
            );
        } else {
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        return Ok(());
    }

    print_report(&report);
    if let Some(explanation) = &deep_explanation {
        println!("  {}", "AI Deep Explanation:".magenta().bold());
        wrap_print(explanation, 80, "    ");
        println!();
        p::separator();
    }
    Ok(())
}

/// Falls back to an AI provider to explain an error in plain language when no
/// local rule-based pattern matched (issue #511). Mirrors the OpenAI-compatible
/// call pattern used by `ai_docs::try_llm_enrichment`, and records the call via
/// `ai_telemetry` under the "error-explain" feature.
async fn deep_explain_via_ai(error_message: &str) -> Result<Option<String>> {
    let api_key = match std::env::var("STARFORGE_AI_API_KEY") {
        Ok(key) if !key.trim().is_empty() => key,
        _ => return Ok(None),
    };
    let base_url = std::env::var("STARFORGE_AI_BASE_URL")
        .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
    let model = std::env::var("STARFORGE_AI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());

    let body = serde_json::json!({
        "model": model,
        "temperature": 0.2,
        "messages": [
            {
                "role": "system",
                "content": "You are a helpful assistant. The user encountered a Soroban smart \
                    contract error. Explain what it means in plain language, identify the most \
                    likely root cause, and suggest concrete troubleshooting steps. Keep it under \
                    200 words."
            },
            {
                "role": "user",
                "content": format!("Error: {}", error_message)
            }
        ]
    });

    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let start = std::time::Instant::now();

    let resp = crate::utils::http_client::get_client()
        .post(&url)
        .bearer_auth(&api_key)
        .json(&body)
        .send()
        .await;

    let elapsed_ms = start.elapsed().as_millis() as u64;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            ai_telemetry::record_call(
                "openai",
                &model,
                "error-explain",
                None,
                None,
                elapsed_ms,
                false,
                Some("network"),
            );
            return Err(anyhow::anyhow!("Deep-explain request failed: {}", e));
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        ai_telemetry::record_call(
            "openai",
            &model,
            "error-explain",
            None,
            None,
            elapsed_ms,
            false,
            Some(if status.as_u16() == 429 {
                "rate_limit"
            } else {
                "auth"
            }),
        );
        anyhow::bail!("AI provider returned error status {}", status);
    }

    let parsed: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse AI response: {}", e))?;

    let tokens_in = parsed
        .pointer("/usage/prompt_tokens")
        .and_then(|v| v.as_u64());
    let tokens_out = parsed
        .pointer("/usage/completion_tokens")
        .and_then(|v| v.as_u64());
    let content = parsed
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    ai_telemetry::record_call(
        "openai",
        &model,
        "error-explain",
        tokens_in,
        tokens_out,
        elapsed_ms,
        true,
        None,
    );

    if content.is_empty() {
        Ok(None)
    } else {
        Ok(Some(content))
    }
}

// ── feedback handler ──────────────────────────────────────────────────────────

async fn handle_feedback(args: FeedbackArgs) -> Result<()> {
    if !args.helpful && !args.not_helpful {
        anyhow::bail!("Specify either --helpful or --not-helpful");
    }
    let rating = if args.helpful {
        ai_feedback::FeedbackRating::Positive
    } else {
        ai_feedback::FeedbackRating::Negative
    };

    ai_feedback::record_feedback(
        "ai-debug",
        &args.finding_id,
        &format!("fix suggestion for {}", args.finding_id),
        rating,
        args.comment.clone(),
        vec![],
    )?;

    p::success(&format!(
        "Recorded feedback for finding {} — this helps prioritise common error fixes.",
        args.finding_id
    ));
    Ok(())
}

// ── learned handler ───────────────────────────────────────────────────────────

async fn handle_learned(args: LearnedArgs) -> Result<()> {
    let stats = ai_feedback::get_feature_stats("ai-debug")?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&stats)?);
        return Ok(());
    }

    p::header("AI Debugger — Learned From Common Errors");
    p::separator();
    p::kv("Total feedback entries", &stats.total_feedback.to_string());
    p::kv(
        "Positive rate",
        &format!("{:.1}%", stats.positive_rate * 100.0),
    );
    p::kv(
        "Negative rate",
        &format!("{:.1}%", stats.negative_rate * 100.0),
    );
    p::kv(
        "Avg quality score",
        &format!("{:.2}", stats.avg_quality_score),
    );
    p::separator();
    Ok(())
}

// ── explain handler ───────────────────────────────────────────────────────────

fn synthetic_error_for_category(category: &str) -> Result<&'static str> {
    match category.to_lowercase().as_str() {
        "auth" | "authorization" => Ok("require_auth failed"),
        "arithmetic" | "overflow" | "underflow" => Ok("attempt to add with overflow"),
        "storage" | "store" => Ok("storage key not found"),
        "token" | "balance" => Ok("insufficient balance for transfer"),
        "panic" => Ok("called `option::unwrap` on a `none` value"),
        "wasm" | "binary" => Ok("invalid wasm binary"),
        "network" | "contract" => Ok("contract not found on network"),
        "deployment" | "deploy" => Ok("deployment transaction failed on-chain"),
        "rollback" => Ok("rollback target missing or deployment reverted"),
        "security" => Ok("insufficient funds or unauthorized access"),
        "analytics" => Ok("deployment trend analysis shows repeated failures"),
        "ttl" | "archival" => Ok("entry expired ttl elapsed"),
        "test" | "assert" => Ok("assertion failed left right"),
        "type" | "abi" | "xdr" => Ok("xdr type conversion mismatch"),
        "compilation" | "compile" | "compiler" => Ok("error[E0308]: mismatched types, expected `u64`"),
        "configuration" | "config" => Ok("failed to load config.toml: unknown network"),
        other => anyhow::bail!(
            "Unknown category '{}'. Valid categories: auth, arithmetic, storage, token, panic, wasm, network, deployment, rollback, security, analytics, ttl, test, type, compilation, configuration",
            other
        ),
    }
}

async fn handle_explain(args: ExplainArgs) -> Result<()> {
    let synthetic_error = synthetic_error_for_category(&args.category)?;
    let report = ai_debugger::analyse(synthetic_error, None, None, None);

    p::header("AI Debugger — Category Explanation");
    p::kv("Category", &args.category);
    p::separator();

    if report.findings.is_empty() {
        p::warn("No detailed explanation available for this category.");
        return Ok(());
    }

    for finding in &report.findings {
        print_finding(finding, true);
    }
    Ok(())
}

// ── inspect handler ───────────────────────────────────────────────────────────

async fn handle_inspect(args: InspectArgs) -> Result<()> {
    let variables = parse_variables(&args.variables)?;

    p::header("AI Debugger — Variable State Inspection");
    p::separator();

    let insights = ai_debugger::inspect_variable_state(&variables);
    for insight in &insights {
        println!("  {}", insight.bright_white());
    }
    println!();
    Ok(())
}

// ── test handler ─────────────────────────────────────────────────────────────

async fn handle_test(args: TestArgs) -> Result<()> {
    // Resolve test output from inline or file
    let output: String = if let Some(file) = args.file {
        fs::read_to_string(&file)
            .map_err(|e| anyhow::anyhow!("Could not read file {}: {}", file.display(), e))?
    } else if let Some(out) = args.output {
        out
    } else {
        anyhow::bail!("Provide test output inline or via --file <path>");
    };

    let error_msg = args.error.as_deref().unwrap_or("test failure");

    let report = ai_debugger::analyse(error_msg, None, None, Some(&output));

    p::header("AI Debugger — Test Failure Analysis");
    p::separator();

    if let Some(ref analysis) = report.test_failure_analysis {
        println!("\n  {}", "Test Analysis:".yellow().bold());
        println!("  {}\n", analysis.bright_white());
    }

    if !report.findings.is_empty() {
        println!("  {}", "Related Findings:".yellow().bold());
        for finding in &report.findings {
            print_finding(finding, false);
        }
    }

    println!("  {}", "Guidance:".yellow().bold());
    println!("  {}\n", report.overall_guidance.bright_white());
    Ok(())
}

// ── Display helpers ───────────────────────────────────────────────────────────

fn print_report(report: &ai_debugger::DebugReport) {
    p::header("AI Contract Debugging Assistant");
    p::kv("Input", &report.input_summary);
    p::separator();

    if report.findings.is_empty() {
        p::warn("No specific issue pattern matched. See general guidance below.");
    } else {
        println!(
            "\n  {} {}\n",
            "Findings:".yellow().bold(),
            format!("({})", report.findings.len()).dimmed()
        );
        for finding in &report.findings {
            print_finding(finding, true);
        }
    }

    // Variable insights
    if !report.variable_insights.is_empty() {
        println!("  {}", "Variable State Insights:".yellow().bold());
        for insight in &report.variable_insights {
            println!("    {}", insight.bright_white());
        }
        println!();
    }

    // Suggested breakpoints
    if !report.suggested_breakpoints.is_empty() {
        println!("  {}", "Suggested Breakpoints:".cyan().bold());
        for bp in &report.suggested_breakpoints {
            println!("    {} {}", "→".cyan(), bp);
        }
        println!();
    }

    // Overall guidance
    println!("  {}", "Guidance:".yellow().bold());
    println!("  {}\n", report.overall_guidance.bright_white());
}

fn print_finding(finding: &ai_debugger::DebugFinding, verbose: bool) {
    let sev_color = match finding.severity {
        Severity::Critical => finding.severity.label().red().bold(),
        Severity::High => finding.severity.label().yellow().bold(),
        Severity::Medium => finding.severity.label().bright_yellow().bold(),
        Severity::Low => finding.severity.label().cyan().bold(),
        Severity::Info => finding.severity.label().white().bold(),
    };

    println!(
        "  [{}] {} — {}",
        sev_color,
        finding.id.bright_white().bold(),
        finding.title.bright_white()
    );
    println!("    {} {}", "Category:".dimmed(), finding.category);
    println!();

    println!("    {}", "Explanation:".bright_white().underline());
    wrap_print(&finding.explanation, 80, "    ");
    println!();

    println!("    {}", "Root Cause:".bright_white().underline());
    wrap_print(&finding.root_cause, 80, "    ");
    println!();

    println!("    {}", "Fix Suggestion:".green().underline());
    wrap_print(&finding.fix_suggestion, 80, "    ");
    println!();

    if verbose {
        if !finding.reproduction_steps.is_empty() {
            println!("    {}", "Reproduction Steps:".bright_white().underline());
            for (i, step) in finding.reproduction_steps.iter().enumerate() {
                println!("      {}. {}", i + 1, step);
            }
            println!();
        }

        if !finding.breakpoint_hints.is_empty() {
            println!("    {}", "Breakpoint Hints:".cyan().underline());
            for hint in &finding.breakpoint_hints {
                println!("      {} {}", "→".cyan(), hint);
            }
            println!();
        }

        if !finding.references.is_empty() {
            println!("    {}", "References:".dimmed().underline());
            for r in &finding.references {
                println!("      {}", r.dimmed());
            }
            println!();
        }
    }

    p::separator();
}

/// Naive word-wrap for long description strings.
fn wrap_print(text: &str, max_width: usize, indent: &str) {
    let mut line_len = 0;
    let mut current = String::new();
    for word in text.split_whitespace() {
        if line_len + word.len() + 1 > max_width && !current.is_empty() {
            println!("{}{}", indent, current);
            current = word.to_string();
            line_len = word.len();
        } else {
            if !current.is_empty() {
                current.push(' ');
                line_len += 1;
            }
            current.push_str(word);
            line_len += word.len();
        }
    }
    if !current.is_empty() {
        println!("{}{}", indent, current);
    }
}

/// Parse a list of "name=value" strings into tuples.
fn parse_variables(raw: &[String]) -> Result<Vec<(String, String)>> {
    raw.iter()
        .map(|s| {
            let mut parts = s.splitn(2, '=');
            let name = parts
                .next()
                .filter(|n| !n.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!("Invalid variable format '{}': expected NAME=VALUE", s)
                })?
                .to_string();
            let value = parts.next().unwrap_or("").to_string();
            Ok((name, value))
        })
        .collect()
}

#[cfg(test)]
mod explain_category_tests {
    use super::synthetic_error_for_category;

    #[test]
    fn deployment_category_maps_to_deployment_error() {
        let err = synthetic_error_for_category("deployment").unwrap();
        assert!(err.to_lowercase().contains("deployment"));
    }

    #[test]
    fn rollback_and_analytics_categories_are_supported() {
        assert!(synthetic_error_for_category("rollback").is_ok());
        assert!(synthetic_error_for_category("analytics").is_ok());
    }
}
