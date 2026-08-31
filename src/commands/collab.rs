//! `starforge collab` — AI-driven collaboration tools.
//!
//! Sub-commands
//! ────────────
//! - `review`            – AI code review assistance for a source file
//! - `resolve-conflict`  – AI-assisted merge-conflict resolution suggestions
//! - `kb-add`            – add an entry to the shared knowledge base
//! - `kb-search`         – search the knowledge base
//! - `kb-list`           – list all knowledge base entries
//! - `digest`            – generate a team activity digest (for stand-ups / chat)
//! - `contributions`     – contribution tracking, per author, over a time window
//! - `insights`          – team collaboration analytics dashboard

use crate::utils::{
    config,
    ollama::{self, GenerateOptions},
    print as p,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ─── Sub-command enum ──────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum CollabCommands {
    /// AI-assisted code review of a source file
    Review {
        /// Path to the Rust source file to review
        #[arg(value_name = "FILE")]
        file: PathBuf,
        /// Model to use
        #[arg(short, long, default_value = ollama::DEFAULT_MODEL)]
        model: String,
    },
    /// AI-assisted merge-conflict resolution suggestions
    ResolveConflict {
        /// Path to the file containing git conflict markers
        #[arg(value_name = "FILE")]
        file: PathBuf,
        /// Model to use
        #[arg(short, long, default_value = ollama::DEFAULT_MODEL)]
        model: String,
    },
    /// Add an entry to the shared knowledge base
    KbAdd {
        /// Short title for the entry
        title: String,
        /// Entry content
        #[arg(trailing_var_arg = true, num_args = 1..)]
        content: Vec<String>,
    },
    /// Search the knowledge base
    KbSearch {
        /// Search query
        query: String,
    },
    /// List all knowledge base entries
    KbList,
    /// Generate a team activity digest (stand-ups, chat updates)
    Digest {
        /// Number of days to include
        #[arg(long, default_value_t = 7)]
        days: i64,
    },
    /// Contribution tracking: commits per author over a time window
    Contributions {
        /// Number of days to include
        #[arg(long, default_value_t = 30)]
        days: i64,
    },
    /// Team collaboration analytics dashboard
    Insights,
}

// ─── Entry point ────────────────────────────────────────────────────────────

pub async fn handle(cmd: CollabCommands) -> Result<()> {
    match cmd {
        CollabCommands::Review { file, model } => handle_review(&file, &model).await,
        CollabCommands::ResolveConflict { file, model } => {
            handle_resolve_conflict(&file, &model).await
        }
        CollabCommands::KbAdd { title, content } => handle_kb_add(&title, &content.join(" ")),
        CollabCommands::KbSearch { query } => handle_kb_search(&query),
        CollabCommands::KbList => handle_kb_list(),
        CollabCommands::Digest { days } => handle_digest(days),
        CollabCommands::Contributions { days } => handle_contributions(days),
        CollabCommands::Insights => handle_insights(),
    }
}

// ─── Storage ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KbEntry {
    id: String,
    title: String,
    content: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReviewRecord {
    file: String,
    kind: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CollabStore {
    #[serde(default)]
    kb: Vec<KbEntry>,
    #[serde(default)]
    reviews: Vec<ReviewRecord>,
}

fn store_path() -> PathBuf {
    config::config_dir().join("collab.json")
}

fn load_store() -> Result<CollabStore> {
    let path = store_path();
    if !path.exists() {
        return Ok(CollabStore::default());
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("Cannot read collaboration store: {}", path.display()))?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

fn save_store(store: &CollabStore) -> Result<()> {
    let path = store_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Cannot create config dir: {}", parent.display()))?;
    }
    let raw = serde_json::to_string_pretty(store)?;
    fs::write(&path, raw)
        .with_context(|| format!("Cannot write collaboration store: {}", path.display()))?;
    Ok(())
}

fn record_review(file: &Path, kind: &str) {
    if let Ok(mut store) = load_store() {
        store.reviews.push(ReviewRecord {
            file: file.display().to_string(),
            kind: kind.to_string(),
            created_at: Utc::now(),
        });
        let _ = save_store(&store);
    }
}

// ─── AI helpers ─────────────────────────────────────────────────────────────

async fn ensure_ollama_running() -> Result<()> {
    if !ollama::is_ollama_running().await {
        anyhow::bail!(
            "Ollama is not running.\n\n{}",
            ollama::cloud_fallback_message()
        );
    }
    Ok(())
}

async fn ask_llm(model: &str, prompt: &str) -> Result<String> {
    ensure_ollama_running().await?;
    let opts = GenerateOptions {
        temperature: Some(0.2),
        num_predict: Some(2048),
        num_ctx: Some(8192),
    };
    let spinner = p::spinner("Thinking…");
    let response = ollama::generate(model, prompt, Some(opts))
        .await
        .context("LLM generation failed")?;
    spinner.finish_and_clear();
    Ok(response.response.trim().to_string())
}

// ─── Handler implementations ────────────────────────────────────────────────

async fn handle_review(file: &PathBuf, model: &str) -> Result<()> {
    let code = fs::read_to_string(file)
        .with_context(|| format!("Cannot read source file: {}", file.display()))?;

    let prompt = format!(
        "You are a senior Rust/Soroban smart-contract reviewer. Review the following code \
         for correctness, security issues, gas efficiency and style. Respond with a concise \
         bullet-point list of concrete suggestions.\n\n```rust\n{code}\n```"
    );

    p::header("AI Code Review");
    p::separator();
    let review = ask_llm(model, &prompt).await?;
    println!("{review}");
    p::separator();

    record_review(file, "review");
    p::success("Review complete.");
    Ok(())
}

async fn handle_resolve_conflict(file: &PathBuf, model: &str) -> Result<()> {
    let content = fs::read_to_string(file)
        .with_context(|| format!("Cannot read file: {}", file.display()))?;

    if !content.contains("<<<<<<<") {
        p::warn("No git conflict markers (<<<<<<<) found in this file.");
        return Ok(());
    }

    let prompt = format!(
        "The following file contains unresolved git merge conflict markers \
         (<<<<<<<, =======, >>>>>>>). Analyze both sides of each conflict, explain the \
         likely intent of each, and propose a single merged version that preserves the \
         correct behavior from both. Output the merged code followed by a short explanation.\n\n\
```\n{content}\n```"
    );

    p::header("AI Conflict Resolution");
    p::separator();
    let suggestion = ask_llm(model, &prompt).await?;
    println!("{suggestion}");
    p::separator();
    p::warn("This is a suggestion only — review carefully before applying.");

    record_review(file, "conflict-resolution");
    Ok(())
}

fn handle_kb_add(title: &str, content: &str) -> Result<()> {
    let mut store = load_store()?;
    store.kb.push(KbEntry {
        id: uuid::Uuid::new_v4().to_string(),
        title: title.to_string(),
        content: content.to_string(),
        created_at: Utc::now(),
    });
    save_store(&store)?;
    p::success(&format!("Added knowledge base entry: \"{title}\""));
    Ok(())
}

fn handle_kb_search(query: &str) -> Result<()> {
    let store = load_store()?;
    let q = query.to_lowercase();
    let matches: Vec<&KbEntry> = store
        .kb
        .iter()
        .filter(|e| e.title.to_lowercase().contains(&q) || e.content.to_lowercase().contains(&q))
        .collect();

    p::header(&format!("Knowledge Base — results for \"{query}\""));
    p::separator();
    if matches.is_empty() {
        p::warn("No matching entries found.");
        return Ok(());
    }
    for entry in matches {
        println!(
            "• {} ({})",
            entry.title,
            entry.created_at.format("%Y-%m-%d")
        );
        println!("  {}", entry.content);
    }
    p::separator();
    Ok(())
}

fn handle_kb_list() -> Result<()> {
    let store = load_store()?;
    p::header("Knowledge Base");
    p::separator();
    if store.kb.is_empty() {
        p::warn("Knowledge base is empty. Add entries with `starforge collab kb-add`.");
        return Ok(());
    }
    let headers = &["Title", "Created", "Preview"];
    let rows: Vec<Vec<String>> = store
        .kb
        .iter()
        .map(|e| {
            let preview: String = e.content.chars().take(60).collect();
            vec![
                e.title.clone(),
                e.created_at.format("%Y-%m-%d").to_string(),
                preview,
            ]
        })
        .collect();
    p::table(headers, &rows);
    Ok(())
}

/// Runs `git log` and returns one line per commit in the given format, over the last `days`.
fn git_log(days: i64, format: &str) -> Result<Vec<String>> {
    let since = format!("--since={days} days ago");
    let pretty = format!("--pretty=format:{format}");
    let output = Command::new("git")
        .args(["log", &since, &pretty])
        .output()
        .context("Failed to run `git log` — is this a git repository with git installed?")?;

    if !output.status.success() {
        anyhow::bail!(
            "git log failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.to_string())
        .collect())
}

fn handle_digest(days: i64) -> Result<()> {
    let lines = git_log(days, "%an|%ad|%s")?;

    p::header(&format!("Team Digest — last {days} day(s)"));
    p::separator();

    if lines.is_empty() {
        p::warn("No commits found in this window.");
        return Ok(());
    }

    use std::collections::BTreeMap;
    let mut by_author: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in &lines {
        let mut parts = line.splitn(3, '|');
        let author = parts.next().unwrap_or("unknown").to_string();
        let _date = parts.next().unwrap_or("");
        let subject = parts.next().unwrap_or("").to_string();
        by_author.entry(author).or_default().push(subject);
    }

    for (author, commits) in &by_author {
        println!("\n{author} ({} commit(s)):", commits.len());
        for subject in commits {
            println!("  - {subject}");
        }
    }
    p::separator();
    p::success("Digest ready to paste into your team channel.");
    Ok(())
}

fn handle_contributions(days: i64) -> Result<()> {
    let lines = git_log(days, "%an")?;

    p::header(&format!("Contribution Tracking — last {days} day(s)"));
    p::separator();

    if lines.is_empty() {
        p::warn("No commits found in this window.");
        return Ok(());
    }

    use std::collections::BTreeMap;
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for author in &lines {
        *counts.entry(author.clone()).or_insert(0) += 1;
    }
    let total: usize = counts.values().sum();

    let mut rows: Vec<(String, usize)> = counts.into_iter().collect();
    rows.sort_by_key(|a| std::cmp::Reverse(a.1));

    let headers = &["Author", "Commits", "Share"];
    let table_rows: Vec<Vec<String>> = rows
        .iter()
        .map(|(author, count)| {
            let pct = if total > 0 {
                (*count as f64 / total as f64) * 100.0
            } else {
                0.0
            };
            vec![author.clone(), count.to_string(), format!("{pct:.1}%")]
        })
        .collect();
    p::table(headers, &table_rows);
    p::separator();
    p::kv("Total commits", &total.to_string());
    Ok(())
}

fn handle_insights() -> Result<()> {
    let store = load_store()?;
    let recent_commits = git_log(30, "%an").unwrap_or_default();

    p::header("Team Collaboration Insights");
    p::separator();
    p::kv("Commits (last 30 days)", &recent_commits.len().to_string());
    p::kv("Knowledge base entries", &store.kb.len().to_string());
    p::kv("AI reviews performed", &store.reviews.len().to_string());

    use std::collections::BTreeSet;
    let contributors: BTreeSet<&String> = recent_commits.iter().collect();
    p::kv("Active contributors (30d)", &contributors.len().to_string());
    p::separator();
    Ok(())
}
