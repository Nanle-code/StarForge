//! AI-driven code refactoring for Soroban contracts.
//!
//! Uses a local Ollama LLM to automatically improve code quality,
//! maintainability, and adherence to best practices. Supports:
//!
//! - Extract functions
//! - Rename variables
//! - Simplify logic
//! - Improve structure
//! - Add documentation
//! - Optimize performance
//!
//! Every refactoring is tracked for before/after comparison and rollback.

use crate::utils::ollama;
use crate::utils::print as p;
use anyhow::{Context, Result};
use chrono::Utc;
use clap::Subcommand;
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// CLI sub-command enum

#[derive(Subcommand)]
pub enum RefactorCommands {
    /// Extract a function from selected code in a Soroban contract
    ExtractFunction {
        /// Path to the contract source file
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Function name for the extracted function
        #[arg(long)]
        name: Option<String>,

        /// Model to use
        #[arg(short, long, default_value = ollama::DEFAULT_MODEL)]
        model: String,

        /// Output file (default: overwrite source)
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Rename variables in a Soroban contract
    RenameVariables {
        /// Path to the contract source file
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Old variable name
        #[arg(long)]
        old: String,

        /// New variable name
        #[arg(long)]
        new: String,

        /// Model to use
        #[arg(short, long, default_value = ollama::DEFAULT_MODEL)]
        model: String,

        /// Output file (default: overwrite source)
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Simplify complex logic in a Soroban contract
    Simplify {
        /// Path to the contract source file
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Model to use
        #[arg(short, long, default_value = ollama::DEFAULT_MODEL)]
        model: String,

        /// Output file (default: overwrite source)
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Improve the overall structure of a Soroban contract
    ImproveStructure {
        /// Path to the contract source file
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Model to use
        #[arg(short, long, default_value = ollama::DEFAULT_MODEL)]
        model: String,

        /// Output file (default: overwrite source)
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Add documentation comments to a Soroban contract
    AddDocs {
        /// Path to the contract source file
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Model to use
        #[arg(short, long, default_value = ollama::DEFAULT_MODEL)]
        model: String,

        /// Output file (default: overwrite source)
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Optimize performance of a Soroban contract
    Optimize {
        /// Path to the contract source file
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Model to use
        #[arg(short, long, default_value = ollama::DEFAULT_MODEL)]
        model: String,

        /// Output file (default: overwrite source)
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Show before/after comparison of a previous refactoring
    Diff {
        /// Refactoring session ID
        session: String,
    },

    /// Rollback a previous refactoring
    Rollback {
        /// Refactoring session ID to rollback
        session: String,
    },

    /// List all refactoring sessions
    Sessions,
}

// Refactoring session for tracking and rollback

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefactorSession {
    pub id: String,
    pub timestamp: String,
    pub refactoring_type: String,
    pub file: String,
    pub original_content: String,
    pub refactored_content: String,
    pub model: String,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefactorReport {
    pub session_id: String,
    pub refactoring_type: String,
    pub file: String,
    pub model: String,
    pub timestamp: String,
    pub before_size: usize,
    pub after_size: usize,
    pub lines_before: usize,
    pub lines_after: usize,
    pub diff_summary: String,
    pub success: bool,
}

// Session storage helpers

fn sessions_dir() -> Result<PathBuf> {
    let dir = crate::utils::config::config_dir().join("refactor-sessions");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

fn session_path(id: &str) -> Result<PathBuf> {
    Ok(sessions_dir()?.join(format!("{}.json", id)))
}

fn save_session(session: &RefactorSession) -> Result<()> {
    let path = session_path(&session.id)?;
    let data = serde_json::to_string_pretty(session)?;
    fs::write(&path, data)?;
    Ok(())
}

fn load_session(id: &str) -> Result<RefactorSession> {
    let path = session_path(id)?;
    if !path.exists() {
        anyhow::bail!("Session '{}' not found. Run a refactoring first.", id);
    }
    let data = fs::read_to_string(&path)?;
    let session: RefactorSession = serde_json::from_str(&data)?;
    Ok(session)
}

fn list_sessions() -> Result<Vec<RefactorSession>> {
    let dir = sessions_dir()?;
    let mut sessions = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if let Some(ext) = entry.path().extension() {
                if ext == "json" {
                    if let Ok(data) = fs::read_to_string(entry.path()) {
                        if let Ok(session) = serde_json::from_str::<RefactorSession>(&data) {
                            sessions.push(session);
                        }
                    }
                }
            }
        }
    }
    sessions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(sessions)
}

// Prompt builders

fn extract_function_prompt(code: &str, desired_name: &str) -> String {
    format!(
        "{SYSTEM_CONTEXT}\
Identify one or more cohesive code blocks in the following Soroban contract \
that can be extracted into standalone functions. The extracted functions should \
have clear, descriptive names and proper Soroban SDK patterns.\
The new function name should be: {name}.

Return ONLY the refactored Rust source code with the extracted function(s) \
and the call site(s) updated to use them. Do not include any explanation or \
markdown formatting.

```rust
{code}
```",
        name = desired_name,
        code = code
    )
}

fn rename_variables_prompt(code: &str, old_name: &str, new_name: &str) -> String {
    format!(
        "{SYSTEM_CONTEXT}\
Rename all occurrences of the variable '{old}' to '{new}' in the following \
Soroban contract code. Preserve all semantics and do not change any other code.

Return ONLY the refactored Rust source code. Do not include any explanation or \
markdown formatting.

```rust
{code}
```",
        old = old_name,
        new = new_name,
        code = code
    )
}

fn simplify_logic_prompt(code: &str) -> String {
    format!(
        "{SYSTEM_CONTEXT}\
Simplify the complex logic in the following Soroban contract. Make it more \
readable, reduce nesting, use early returns where appropriate, and eliminate \
unnecessary complexity while preserving exact behavior.

Return ONLY the refactored Rust source code. Do not include any explanation or \
markdown formatting.

```rust
{code}
```",
        code = code
    )
}

fn improve_structure_prompt(code: &str) -> String {
    format!(
        "{SYSTEM_CONTEXT}\
Improve the overall structure of the following Soroban contract. Organize \
functions logically, group related functionality, add proper module-level \
documentation, and ensure consistent code style following Soroban best practices.

Return ONLY the refactored Rust source code. Do not include any explanation or \
markdown formatting.

```rust
{code}
```",
        code = code
    )
}

fn add_docs_prompt(code: &str) -> String {
    format!(
        "{SYSTEM_CONTEXT}\
Add comprehensive rustdoc comments to the following Soroban contract. \
Document every public function, struct, enum, and impl block. Include \
parameter descriptions, return values, and usage examples where appropriate. \
Use Soroban-specific documentation conventions.

Return ONLY the refactored Rust source code with added documentation comments. \
Do not include any explanation or markdown formatting.

```rust
{code}
```",
        code = code
    )
}

fn optimize_perf_prompt(code: &str) -> String {
    format!(
        "{SYSTEM_CONTEXT}\
Optimize the performance of the following Soroban contract. Focus on \
reducing gas consumption, minimizing storage reads/writes, using efficient \
data structures, and eliminating unnecessary computation. Preserve exact behavior.

Return ONLY the refactored Rust source code. Do not include any explanation or \
markdown formatting.

```rust
{code}
```",
        code = code
    )
}

// System context constant

const SYSTEM_CONTEXT: &str = "You are an expert Soroban smart contract developer integrated into the StarForge CLI. Refactor the provided contract code to improve quality while preserving exact behavior. Output ONLY valid Soroban Rust code.\n\n";

// Public entry point

pub async fn handle(cmd: RefactorCommands) -> Result<()> {
    match cmd {
        RefactorCommands::ExtractFunction {
            file,
            name,
            model,
            output,
        } => {
            handle_refactor(
                &file,
                &model,
                name.as_deref().unwrap_or("extracted"),
                TaskType::ExtractFunction,
                output,
            )
            .await
        }
        RefactorCommands::RenameVariables {
            file,
            old,
            new,
            model,
            output,
        } => {
            handle_refactor(
                &file,
                &model,
                &format!("{old}->{new}"),
                TaskType::RenameVariables,
                output,
            )
            .await
        }
        RefactorCommands::Simplify {
            file,
            model,
            output,
        } => handle_refactor(&file, &model, "simplify", TaskType::Simplify, output).await,
        RefactorCommands::ImproveStructure {
            file,
            model,
            output,
        } => {
            handle_refactor(
                &file,
                &model,
                "improve-structure",
                TaskType::ImproveStructure,
                output,
            )
            .await
        }
        RefactorCommands::AddDocs {
            file,
            model,
            output,
        } => handle_refactor(&file, &model, "add-docs", TaskType::AddDocs, output).await,
        RefactorCommands::Optimize {
            file,
            model,
            output,
        } => handle_refactor(&file, &model, "optimize", TaskType::Optimize, output).await,
        RefactorCommands::Diff { session } => handle_diff(session),
        RefactorCommands::Rollback { session } => handle_rollback(session),
        RefactorCommands::Sessions => handle_sessions(),
    }
}

enum TaskType {
    ExtractFunction,
    RenameVariables,
    Simplify,
    ImproveStructure,
    AddDocs,
    Optimize,
}

impl TaskType {
    fn label(&self) -> &'static str {
        match self {
            TaskType::ExtractFunction => "Extract Function",
            TaskType::RenameVariables => "Rename Variables",
            TaskType::Simplify => "Simplify Logic",
            TaskType::ImproveStructure => "Improve Structure",
            TaskType::AddDocs => "Add Documentation",
            TaskType::Optimize => "Optimize Performance",
        }
    }

    fn build_prompt(&self, code: &str, extra: &str) -> String {
        match self {
            TaskType::ExtractFunction => extract_function_prompt(code, extra),
            TaskType::RenameVariables => rename_variables_prompt(code, extra, ""),
            TaskType::Simplify => simplify_logic_prompt(code),
            TaskType::ImproveStructure => improve_structure_prompt(code),
            TaskType::AddDocs => add_docs_prompt(code),
            TaskType::Optimize => optimize_perf_prompt(code),
        }
    }
}

// Core refactoring handler

async fn handle_refactor(
    file: &PathBuf,
    model: &str,
    extra: &str,
    task_type: TaskType,
    output: Option<PathBuf>,
) -> Result<()> {
    let code = fs::read_to_string(file)
        .with_context(|| format!("Cannot read source file: {}", file.display()))?;

    ensure_ollama_running().await?;

    let task_label = task_type.label();
    p::header(&format!("AI {} Refactoring", task_label));
    p::separator();
    p::kv("Model", model);
    p::kv("File", &file.display().to_string());
    p::kv("Source lines", &code.lines().count().to_string());
    println!();

    let prompt = task_type.build_prompt(&code, extra);
    let opts = ollama::GenerateOptions {
        temperature: Some(0.1),
        num_predict: Some(4096),
        num_ctx: Some(8192),
    };

    let spinner = p::spinner(&format!(
        "Running {} refactoring...",
        task_label.to_lowercase()
    ));
    let response = ollama::generate(model, &prompt, Some(opts))
        .await
        .with_context(|| format!("LLM {} refactoring failed", task_label.to_lowercase()))?;
    spinner.finish_and_clear();

    let refactored = response.response.trim();

    if refactored.is_empty() {
        anyhow::bail!("LLM returned empty response. Try again or use a different model.");
    }

    // Generate session ID
    let session_id = format!(
        "refactor-{}-{}",
        Utc::now().format("%Y%m%d-%H%M%S"),
        sha256::hash(refactored).chars().take(8).collect::<String>()
    );

    // Save session for tracking/rollback
    let session = RefactorSession {
        id: session_id.clone(),
        timestamp: Utc::now().to_rfc3339(),
        refactoring_type: task_label.to_string(),
        file: file.display().to_string(),
        original_content: code.clone(),
        refactored_content: refactored.to_string(),
        model: model.to_string(),
        success: true,
    };
    save_session(&session)?;

    // Write output
    let out_path = output.as_ref().unwrap_or(file);

    // Back up original before overwriting
    if out_path == file {
        let backup_path = file.with_extension("rs.bak");
        fs::write(&backup_path, &code)?;
        p::kv("Backup", &backup_path.display().to_string());
    }

    fs::write(out_path, refactored)?;

    // Report
    let report = RefactorReport {
        session_id: session_id.clone(),
        refactoring_type: task_label.to_string(),
        file: file.display().to_string(),
        model: model.to_string(),
        timestamp: Utc::now().to_rfc3339(),
        before_size: code.len(),
        after_size: refactored.len(),
        lines_before: code.lines().count(),
        lines_after: refactored.lines().count(),
        diff_summary: generate_diff_summary(&code, refactored),
        success: true,
    };

    println!();
    p::separator();
    p::success(&format!("{} refactoring complete!", task_label));
    p::kv("Session ID", &session_id);
    p::kv("File", &out_path.display().to_string());
    p::kv("Before lines", &report.lines_before.to_string());
    p::kv("After lines", &report.lines_after.to_string());
    p::kv("Before size", &format!("{} bytes", report.before_size));
    p::kv("After size", &format!("{} bytes", report.after_size));
    println!();
    println!("{}", report.diff_summary);
    println!();
    p::info(&format!(
        "Use `starforge ai refactor diff {}` to see the full before/after.",
        session_id
    ));
    p::info(&format!(
        "Use `starforge ai refactor rollback {}` to undo this change.",
        session_id
    ));
    p::separator();
    Ok(())
}

fn handle_diff(session_id: String) -> Result<()> {
    let session = load_session(&session_id)?;

    p::header(&format!("Refactoring Diff - {}", session_id));
    p::separator();
    p::kv("Type", &session.refactoring_type);
    p::kv("File", &session.file);
    p::kv("Timestamp", &session.timestamp);
    p::kv("Model", &session.model);
    println!();

    p::step(1, 3, "Original content (first 20 lines)");
    for line in session.original_content.lines().take(20) {
        println!("  {} {}", "-".red(), line);
    }
    if session.original_content.lines().count() > 20 {
        println!(
            "  {} ... ({} more lines)",
            ".".dimmed(),
            session.original_content.lines().count() - 20
        );
    }
    println!();

    p::step(2, 3, "Refactored content (first 20 lines)");
    for line in session.refactored_content.lines().take(20) {
        println!("  {} {}", "+".green(), line);
    }
    if session.refactored_content.lines().count() > 20 {
        println!(
            "  {} ... ({} more lines)",
            ".".dimmed(),
            session.refactored_content.lines().count() - 20
        );
    }
    println!();

    p::step(3, 3, "Summary");
    p::kv(
        "Original lines",
        &session.original_content.lines().count().to_string(),
    );
    p::kv(
        "Refactored lines",
        &session.refactored_content.lines().count().to_string(),
    );
    let delta = session.refactored_content.lines().count() as i64
        - session.original_content.lines().count() as i64;
    let delta_str = if delta >= 0 {
        format!("+{}", delta)
    } else {
        delta.to_string()
    };
    p::kv("Line delta", &delta_str);
    p::separator();
    Ok(())
}

fn handle_rollback(session_id: String) -> Result<()> {
    let session = load_session(&session_id)?;

    if !session.success {
        anyhow::bail!("Cannot rollback a failed refactoring session.");
    }

    p::header(&format!("Rollback Refactoring - {}", session_id));
    p::separator();
    p::kv("Type", &session.refactoring_type);
    p::kv("File", &session.file);
    println!();

    let file_path = PathBuf::from(&session.file);
    if !file_path.exists() {
        anyhow::bail!("Source file no longer exists: {}", session.file);
    }

    // Save current content as a rollback backup
    let current = fs::read_to_string(&file_path)?;
    let rollback_backup = file_path.with_extension("rs.rollback-backup");
    fs::write(&rollback_backup, &current)?;

    // Restore original
    fs::write(&file_path, &session.original_content)?;

    // Mark session as rolled back
    let mut session = session;
    session.success = false;
    save_session(&session)?;

    p::success(&format!(
        "Rolled back {} to original state.",
        session.refactoring_type
    ));
    p::kv("Restored file", &file_path.display().to_string());
    p::kv("Rollback backup", &rollback_backup.display().to_string());
    p::info("The current refactored version is saved as a rollback backup.");
    p::separator();
    Ok(())
}

fn handle_sessions() -> Result<()> {
    p::header("Refactoring Sessions");
    p::separator();

    let sessions = list_sessions()?;

    if sessions.is_empty() {
        p::info("No refactoring sessions found.");
        p::separator();
        return Ok(());
    }

    println!(
        "  {:<30}  {:<20}  {:<15}  {}",
        "ID".dimmed(),
        "Type".dimmed(),
        "File".dimmed(),
        "Timestamp".dimmed()
    );
    println!("  {}", "-".repeat(100).dimmed());

    for session in &sessions {
        let file_short = PathBuf::from(&session.file)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| session.file.clone());
        println!(
            "  {:<30}  {:<20}  {:<15}  {}",
            session.id.chars().take(30).collect::<String>(),
            session.refactoring_type,
            file_short,
            session.timestamp.get(..19).unwrap_or(&session.timestamp)
        );
    }

    println!();
    p::kv("Total", &sessions.len().to_string());
    p::separator();
    Ok(())
}

fn generate_diff_summary(original: &str, refactored: &str) -> String {
    let orig_lines: Vec<&str> = original.lines().collect();
    let ref_lines: Vec<&str> = refactored.lines().collect();

    let added = ref_lines.len().saturating_sub(orig_lines.len());
    let removed = orig_lines.len().saturating_sub(ref_lines.len());

    let mut summary_parts = Vec::new();
    if added > 0 {
        summary_parts.push(format!("{} lines added", added));
    }
    if removed > 0 {
        summary_parts.push(format!("{} lines removed", removed));
    }
    if summary_parts.is_empty() {
        summary_parts.push("no change in line count".to_string());
    }

    summary_parts.join(", ")
}

async fn ensure_ollama_running() -> Result<()> {
    if !ollama::is_ollama_running().await {
        anyhow::bail!(
            "Ollama is not running.\n\n{}",
            ollama::cloud_fallback_message()
        );
    }
    Ok(())
}

// Compute a simple hash for session IDs
mod sha256 {
    use sha2::{Digest, Sha256};

    pub fn hash(input: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(input.as_bytes());
        hex::encode(hasher.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_function_prompt_contains_code() {
        let code = "pub fn hello() {}";
        let prompt = extract_function_prompt(code, "new_fn");
        assert!(prompt.contains(code));
        assert!(prompt.contains("new_fn"));
        assert!(prompt.contains("extracted"));
    }

    #[test]
    fn rename_variables_prompt_contains_names() {
        let code = "let old_name = 42;";
        let prompt = rename_variables_prompt(code, "old_name", "new_name");
        assert!(prompt.contains("old_name"));
        assert!(prompt.contains("new_name"));
    }

    #[test]
    fn simplify_prompt_contains_code() {
        let code = "fn complex() { if true { if false { 1 } else { 2 } } }";
        let prompt = simplify_logic_prompt(code);
        assert!(prompt.contains(code));
        assert!(prompt.contains("Simplify"));
    }

    #[test]
    fn add_docs_prompt_contains_code() {
        let code = "pub fn hello() {}";
        let prompt = add_docs_prompt(code);
        assert!(prompt.contains(code));
        assert!(prompt.contains("documentation"));
    }

    #[test]
    fn optimize_prompt_contains_code() {
        let code = "pub fn slow() { let mut x = 0; for _ in 0..100 { x += 1; } }";
        let prompt = optimize_perf_prompt(code);
        assert!(prompt.contains(code));
        // The instruction opens the sentence, so match without regard to case.
        assert!(prompt.to_lowercase().contains("optimize"), "got {}", prompt);
    }

    #[test]
    fn task_labels_are_non_empty() {
        for task in [
            TaskType::ExtractFunction,
            TaskType::RenameVariables,
            TaskType::Simplify,
            TaskType::ImproveStructure,
            TaskType::AddDocs,
            TaskType::Optimize,
        ] {
            assert!(!task.label().is_empty());
        }
    }

    #[test]
    fn generate_diff_summary_shows_additions() {
        let original = "fn a() {}\nfn b() {}\nfn c() {}\n";
        let refactored = "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}\n";
        let summary = generate_diff_summary(original, refactored);
        assert!(summary.contains("2 lines added"));
    }

    #[test]
    fn generate_diff_summary_shows_removals() {
        let original = "fn a() {}\nfn b() {}\nfn c() {}\n";
        let refactored = "fn a() {}\n";
        let summary = generate_diff_summary(original, refactored);
        assert!(summary.contains("2 lines removed"));
    }

    #[test]
    fn session_roundtrip() {
        let session = RefactorSession {
            id: "test-session-123".to_string(),
            timestamp: "2026-07-25T12:00:00Z".to_string(),
            refactoring_type: "Extract Function".to_string(),
            file: "/tmp/test.rs".to_string(),
            original_content: "pub fn hello() {}".to_string(),
            refactored_content: "pub fn hello_world() {}".to_string(),
            model: "codellama:7b".to_string(),
            success: true,
        };

        let json = serde_json::to_string_pretty(&session).unwrap();
        let deserialized: RefactorSession = serde_json::from_str(&json).unwrap();
        assert_eq!(session.id, deserialized.id);
        assert_eq!(session.refactoring_type, deserialized.refactoring_type);
        assert_eq!(session.original_content, deserialized.original_content);
        assert_eq!(session.refactored_content, deserialized.refactored_content);
    }
}
