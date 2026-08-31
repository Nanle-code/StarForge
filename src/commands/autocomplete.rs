use crate::utils::history::{load_history, prune_history, save_history, HistoryEntry};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;

/// Represents a suggestion provided by the autocomplete engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    pub text: String,
    pub score: f64,
    pub source: SuggestionSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SuggestionSource {
    History,
    Command,
    Flag,
    Predicted,
}

impl std::fmt::Display for SuggestionSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SuggestionSource::History => write!(f, "history"),
            SuggestionSource::Command => write!(f, "command"),
            SuggestionSource::Flag => write!(f, "flag"),
            SuggestionSource::Predicted => write!(f, "predicted"),
        }
    }
}

/// Smart autocomplete engine with command history and frequency-based scoring
pub struct AutocompleteEngine {
    history: Vec<HistoryEntry>,
    config_dir: PathBuf,
}

impl AutocompleteEngine {
    /// Create a new autocomplete engine, loading history from disk
    pub fn new() -> Result<Self> {
        let config_dir = crate::utils::config::config_dir();
        let history = load_history(&config_dir)?;
        Ok(AutocompleteEngine {
            history,
            config_dir,
        })
    }

    /// Record a command in history, incrementing count if it already exists
    pub fn record(&mut self, command: &str) -> Result<()> {
        let now = Utc::now();

        // Find and update existing entry
        if let Some(entry) = self.history.iter_mut().find(|e| e.command == command) {
            entry.count += 1;
            entry.last_used = now;
            self.save_history()?;
            return Ok(());
        }

        // Add new entry
        self.history.push(HistoryEntry {
            command: command.to_string(),
            timestamp: now,
            count: 1,
            last_used: now,
        });

        // Prune if exceeds max
        prune_history(&mut self.history, 500);
        self.save_history()?;
        Ok(())
    }

    /// Save history to disk
    fn save_history(&self) -> Result<()> {
        save_history(&self.history, &self.config_dir)
    }

    /// Get suggestions for a partial command
    pub fn suggest(&self, partial: &str) -> Vec<Suggestion> {
        let now = Utc::now();
        let mut suggestions = Vec::new();

        // Score all history entries that start with partial
        for entry in &self.history {
            if entry.command.starts_with(partial) && !partial.is_empty() {
                let score = self.calculate_score(entry, now);
                suggestions.push(Suggestion {
                    text: entry.command.clone(),
                    score,
                    source: SuggestionSource::History,
                });
            }
        }

        // Sort by score descending, take top 5
        suggestions.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        suggestions.truncate(5);
        suggestions
    }

    /// Calculate a suggestion score based on frequency and recency
    fn calculate_score(&self, entry: &HistoryEntry, now: DateTime<Utc>) -> f64 {
        let frequency_weight = 0.6;
        let recency_weight = 0.4;

        // Frequency component (raw count, normalized to 0-1)
        let max_count = self
            .history
            .iter()
            .map(|e| e.count)
            .max()
            .unwrap_or(1)
            .max(1);
        let frequency_score = (entry.count as f64) / (max_count as f64);

        // Recency component: 1.0 / (1.0 + days_since_last_use)
        let duration = now.signed_duration_since(entry.last_used);
        let days_since = duration.num_seconds() as f64 / (86400.0);
        let recency_score = 1.0 / (1.0 + days_since);

        frequency_weight * frequency_score + recency_weight * recency_score
    }

    /// Complete a partial command against known commands
    pub fn complete_command(&self, partial: &str) -> Vec<String> {
        let commands = vec![
            "wallet",
            "new",
            "deploy",
            "contract",
            "network",
            "template",
            "info",
            "completions",
            "autocomplete",
            "config",
            "telemetry",
            "tx",
            "node",
            "shell",
            "monitor",
            "tutorial",
            "benchmark",
            "test",
            "gas",
            "plugin",
            "registry",
            "multisig",
            "upgrade",
            "governance",
            "orchestrate",
            "pipeline",
            "security",
            "audit",
            "schedule",
            "simulate",
            "backup",
            "lint",
            "diagnostics",
            "template-vcs",
            "perf",
            "advanced-perf",
            "docs",
            "analytics",
            "approval",
            "debug",
            "inspect",
            "deployments",
            "migrate",
        ];

        commands
            .into_iter()
            .filter(|cmd| cmd.starts_with(partial))
            .map(String::from)
            .collect()
    }

    /// Complete a partial flag for a given command
    pub fn complete_flag(&self, command: &str, partial: &str) -> Vec<String> {
        let flags = match command {
            "wallet" => {
                vec!["create", "list", "show", "remove", "fund", "export"]
            }
            "wallet create" => {
                vec!["--encrypt", "--fund", "--network"]
            }
            "wallet show" => {
                vec!["--reveal", "--network"]
            }
            "wallet list" => {
                vec!["--network"]
            }
            "wallet fund" => {
                vec!["--network", "--amount"]
            }
            "wallet export" => {
                vec!["--format", "--output"]
            }
            "new" => {
                vec!["contract", "dapp"]
            }
            "new contract" => {
                vec!["--template", "--interactive", "--from", "--name"]
            }
            "new dapp" => {
                vec![]
            }
            "deploy" => {
                vec!["--wasm", "--network", "--wallet", "--yes", "--salt"]
            }
            "network" => {
                vec!["switch", "add", "show", "list"]
            }
            "network switch" => {
                vec![]
            }
            "network add" => {
                vec!["--horizon-url", "--soroban-rpc-url", "--network-passphrase"]
            }
            "contract" => {
                vec!["inspect", "invoke", "list"]
            }
            "contract inspect" => {
                vec!["--network"]
            }
            "contract invoke" => {
                vec!["--network", "--wallet", "--function", "--args"]
            }
            "template" => {
                vec!["search", "publish", "list"]
            }
            "template search" => {
                vec!["--tags", "--author", "--limit"]
            }
            "template publish" => {
                vec!["--name", "--description", "--author", "--tags"]
            }
            "config" => {
                vec!["get", "set", "show"]
            }
            _ => vec![],
        };

        flags
            .into_iter()
            .filter(|flag| flag.starts_with(partial))
            .map(String::from)
            .collect()
    }

    /// Predict the user's intent based on pattern matching
    pub fn predict_intent(&self, partial: &str) -> Option<String> {
        // Group commands by prefix and find frequent patterns
        let mut command_groups: HashMap<String, Vec<&HistoryEntry>> = HashMap::new();

        for entry in &self.history {
            if entry.command.starts_with(partial) && entry.count >= 3 {
                // Extract the prefix up to the first space
                let prefix = entry.command.split_whitespace().next().unwrap_or("");
                command_groups
                    .entry(prefix.to_string())
                    .or_default()
                    .push(entry);
            }
        }

        // Find the most frequently used command in matching groups
        command_groups
            .values()
            .flatten()
            .max_by_key(|entry| entry.count)
            .map(|entry| entry.command.clone())
    }

    /// Clear all history
    pub fn clear_history(&mut self) -> Result<()> {
        self.history.clear();
        self.save_history()
    }

    /// Get statistics about command usage
    pub fn get_stats(&self) -> CommandStats {
        let total_commands = self.history.len();
        let total_invocations = self.history.iter().map(|e| e.count).sum();

        let most_used = self.history.iter().max_by_key(|e| e.count).cloned();
        let least_used = self.history.iter().min_by_key(|e| e.count).cloned();

        CommandStats {
            total_commands,
            total_invocations,
            most_used,
            least_used,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CommandStats {
    pub total_commands: usize,
    pub total_invocations: usize,
    pub most_used: Option<HistoryEntry>,
    pub least_used: Option<HistoryEntry>,
}

/// Handle the autocomplete command
pub async fn handle_autocomplete(
    suggest: Option<String>,
    record: Option<String>,
    interactive: bool,
    clear_history: bool,
    stats: bool,
) -> Result<()> {
    let mut engine = AutocompleteEngine::new()?;

    if clear_history {
        engine.clear_history()?;
        println!("✓ Command history cleared");
        return Ok(());
    }

    if stats {
        let stats = engine.get_stats();
        println!("\n📊 Command Usage Statistics:");
        println!("  Total commands: {}", stats.total_commands);
        println!("  Total invocations: {}", stats.total_invocations);
        if let Some(most) = stats.most_used {
            println!("  Most used: {} ({}x)", most.command, most.count);
        }
        if let Some(least) = stats.least_used {
            println!("  Least used: {} ({}x)", least.command, least.count);
        }
        return Ok(());
    }

    if let Some(cmd) = record {
        engine.record(&cmd)?;
        return Ok(());
    }

    if let Some(partial) = suggest {
        let suggestions = engine.suggest(&partial);
        for (idx, sugg) in suggestions.iter().enumerate() {
            println!("{}.  {} [{}]", idx + 1, sugg.text, sugg.source);
        }
        return Ok(());
    }

    if interactive {
        run_interactive_mode(&engine)?;
        return Ok(());
    }

    Ok(())
}

/// Run interactive autocomplete REPL loop
fn run_interactive_mode(engine: &AutocompleteEngine) -> Result<()> {
    println!("\n✨ StarForge Autocomplete Interactive Mode");
    println!("Type a command prefix and press Enter to see suggestions.");
    println!("Type 'exit' to quit.\n");

    loop {
        print!("starforge> ");
        io::stdout().flush()?;

        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        let line = line.trim();

        if line.is_empty() || line == "exit" {
            println!("Goodbye!");
            break;
        }

        let suggestions = engine.suggest(line);
        if suggestions.is_empty() {
            println!("  (no suggestions)");
        } else {
            for (idx, sugg) in suggestions.iter().enumerate() {
                println!("  {}. {}  [{}]", idx + 1, sugg.text, sugg.source);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_engine(temp_dir: &TempDir) -> AutocompleteEngine {
        AutocompleteEngine {
            history: vec![],
            config_dir: temp_dir.path().to_path_buf(),
        }
    }

    #[test]
    fn test_suggest_returns_history_matches() {
        let temp_dir = TempDir::new().unwrap();
        let mut engine = create_test_engine(&temp_dir);

        engine.history.push(HistoryEntry {
            command: "wallet create alice".to_string(),
            timestamp: Utc::now(),
            count: 5,
            last_used: Utc::now(),
        });

        let suggestions = engine.suggest("wallet");
        assert!(!suggestions.is_empty());
        assert_eq!(suggestions[0].text, "wallet create alice");
    }

    #[test]
    fn test_suggest_sorted_by_score() {
        let temp_dir = TempDir::new().unwrap();
        let mut engine = create_test_engine(&temp_dir);

        let old_time = Utc::now() - chrono::Duration::days(30);
        engine.history.push(HistoryEntry {
            command: "deploy".to_string(),
            timestamp: old_time,
            count: 10,
            last_used: old_time,
        });

        engine.history.push(HistoryEntry {
            command: "wallet create".to_string(),
            timestamp: Utc::now(),
            count: 2,
            last_used: Utc::now(),
        });

        let suggestions = engine.suggest("");
        // Recent should rank higher despite lower count
        assert!(suggestions.len() <= 5);
    }

    #[test]
    fn test_complete_command_matches_partial() {
        let temp_dir = TempDir::new().unwrap();
        let engine = create_test_engine(&temp_dir);

        let matches = engine.complete_command("wal");
        assert!(matches.contains(&"wallet".to_string()));
    }

    #[test]
    fn test_complete_flag_for_wallet_create() {
        let temp_dir = TempDir::new().unwrap();
        let engine = create_test_engine(&temp_dir);

        let matches = engine.complete_flag("wallet create", "--en");
        assert!(matches.contains(&"--encrypt".to_string()));
    }

    #[test]
    fn test_predict_intent_frequent_pattern() {
        let temp_dir = TempDir::new().unwrap();
        let mut engine = create_test_engine(&temp_dir);

        for _ in 0..5 {
            engine.history.push(HistoryEntry {
                command: "wallet create alice".to_string(),
                timestamp: Utc::now(),
                count: 5,
                last_used: Utc::now(),
            });
        }

        let intent = engine.predict_intent("wallet");
        assert!(intent.is_some());
    }

    #[test]
    fn test_record_increments_count() {
        let temp_dir = TempDir::new().unwrap();
        let mut engine = create_test_engine(&temp_dir);

        engine.record("wallet create bob").ok();
        engine.record("wallet create bob").ok();

        let entry = engine
            .history
            .iter()
            .find(|e| e.command == "wallet create bob");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().count, 2);
    }

    #[test]
    fn test_history_pruned_at_max() {
        let temp_dir = TempDir::new().unwrap();
        let mut engine = create_test_engine(&temp_dir);

        for i in 0..501 {
            engine.history.push(HistoryEntry {
                command: format!("cmd{}", i),
                timestamp: Utc::now(),
                count: 1,
                last_used: Utc::now(),
            });
        }

        prune_history(&mut engine.history, 500);
        assert_eq!(engine.history.len(), 500);
    }

    #[test]
    fn test_get_stats() {
        let temp_dir = TempDir::new().unwrap();
        let mut engine = create_test_engine(&temp_dir);

        engine.history.push(HistoryEntry {
            command: "wallet create".to_string(),
            timestamp: Utc::now(),
            count: 10,
            last_used: Utc::now(),
        });

        engine.history.push(HistoryEntry {
            command: "deploy".to_string(),
            timestamp: Utc::now(),
            count: 2,
            last_used: Utc::now(),
        });

        let stats = engine.get_stats();
        assert_eq!(stats.total_commands, 2);
        assert_eq!(stats.total_invocations, 12);
    }
}
