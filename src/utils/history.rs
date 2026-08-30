use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Represents a single entry in the command history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub command: String,
    pub timestamp: DateTime<Utc>,
    pub count: usize,
    pub last_used: DateTime<Utc>,
}

/// History file structure stored as JSON
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryFile {
    pub commands: Vec<HistoryEntry>,
}

/// Get the path to the history file: ~/.starforge/history.json
pub fn history_file_path(config_dir: &Path) -> PathBuf {
    config_dir.join("history.json")
}

/// Load command history from disk
pub fn load_history(config_dir: &Path) -> Result<Vec<HistoryEntry>> {
    let path = history_file_path(config_dir);

    if !path.exists() {
        return Ok(Vec::new());
    }

    match fs::read_to_string(&path) {
        Ok(content) => {
            let history_file: HistoryFile = serde_json::from_str(&content)?;
            Ok(history_file.commands)
        }
        Err(_) => Ok(Vec::new()), // Return empty on any read error
    }
}

/// Save command history to disk atomically
pub fn save_history(entries: &[HistoryEntry], config_dir: &Path) -> Result<()> {
    // Ensure the config directory exists
    fs::create_dir_all(config_dir)?;

    let history_file = HistoryFile {
        commands: entries.to_vec(),
    };

    let json = serde_json::to_string_pretty(&history_file)?;
    let path = history_file_path(config_dir);

    // Write to temp file first, then rename (atomic operation)
    let temp_path = path.with_extension("json.tmp");
    fs::write(&temp_path, json)?;
    fs::rename(&temp_path, &path)?;

    Ok(())
}

/// Prune history to keep only the most recent/frequently used entries
pub fn prune_history(entries: &mut Vec<HistoryEntry>, max: usize) {
    if entries.len() > max {
        // Sort by (last_used descending, count descending) to keep most recent and frequent
        entries.sort_by(|a, b| match b.last_used.cmp(&a.last_used) {
            std::cmp::Ordering::Equal => b.count.cmp(&a.count),
            other => other,
        });
        entries.truncate(max);
    }
}

/// Redact values that should never be persisted in command history.
pub fn redact_command(command: &str) -> String {
    let mut output = Vec::new();
    let mut redact_next = false;

    for token in command.split_whitespace() {
        if redact_next {
            output.push("[REDACTED]".to_string());
            redact_next = false;
            continue;
        }

        let lower = token.to_ascii_lowercase();
        if [
            "--secret",
            "--secret-key",
            "--token",
            "--api-key",
            "--secret-key-file",
        ]
        .iter()
        .any(|flag| lower == *flag)
        {
            output.push(token.to_string());
            redact_next = true;
        } else if lower.starts_with("--secret=")
            || lower.starts_with("--token=")
            || lower.starts_with("--api-key=")
            || lower.starts_with("secret_key=")
            || lower.starts_with("api_key=")
        {
            let key = token.split('=').next().unwrap_or("secret");
            output.push(format!("{}=[REDACTED]", key));
        } else {
            output.push(token.to_string());
        }
    }

    output.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_history_file_path() {
        let dir = PathBuf::from("/tmp");
        let path = history_file_path(&dir);
        assert!(path.ends_with("history.json"));
    }

    #[test]
    fn test_load_history_missing_file() {
        let temp_dir = TempDir::new().unwrap();
        let result = load_history(&temp_dir.path().to_path_buf());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[test]
    fn test_save_and_load_history() {
        let temp_dir = TempDir::new().unwrap();
        let config_dir = temp_dir.path().to_path_buf();

        let entries = vec![
            HistoryEntry {
                command: "wallet create".to_string(),
                timestamp: Utc::now(),
                count: 3,
                last_used: Utc::now(),
            },
            HistoryEntry {
                command: "deploy --wasm".to_string(),
                timestamp: Utc::now(),
                count: 1,
                last_used: Utc::now(),
            },
        ];

        save_history(&entries, &config_dir).unwrap();
        let loaded = load_history(&config_dir).unwrap();

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].command, "wallet create");
        assert_eq!(loaded[0].count, 3);
    }

    #[test]
    fn test_prune_history() {
        let mut entries = vec![];
        let now = Utc::now();

        for i in 0..10 {
            entries.push(HistoryEntry {
                command: format!("cmd{}", i),
                timestamp: now,
                count: i,
                last_used: now,
            });
        }

        prune_history(&mut entries, 5);
        assert_eq!(entries.len(), 5);
    }

    #[test]
    fn test_prune_history_keeps_oldest_if_less_than_max() {
        let mut entries = vec![
            HistoryEntry {
                command: "cmd1".to_string(),
                timestamp: Utc::now(),
                count: 1,
                last_used: Utc::now(),
            },
            HistoryEntry {
                command: "cmd2".to_string(),
                timestamp: Utc::now(),
                count: 1,
                last_used: Utc::now(),
            },
        ];

        prune_history(&mut entries, 5);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_redact_command_removes_secret_values() {
        let redacted = redact_command(
            "invoke transfer --token super-secret --api-key=also-secret --network testnet",
        );
        assert!(!redacted.contains("super-secret"));
        assert!(!redacted.contains("also-secret"));
        assert!(redacted.contains("[REDACTED]"));
    }
}
