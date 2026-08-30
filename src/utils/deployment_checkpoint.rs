//! Deployment Checkpointing Module.
//!
//! Provides state checkpointing, resumability, staleness detection, schema versioning,
//! corruption recovery, and concurrency lock protection for deployment operations.

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Current schema version for deployment checkpoints.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Checkpoint status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CheckpointStatus {
    InProgress,
    Completed,
    Failed,
    RolledBack,
}

impl std::fmt::Display for CheckpointStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckpointStatus::InProgress => write!(f, "in_progress"),
            CheckpointStatus::Completed => write!(f, "completed"),
            CheckpointStatus::Failed => write!(f, "failed"),
            CheckpointStatus::RolledBack => write!(f, "rolled_back"),
        }
    }
}

/// Recorded completed step inside a deployment checkpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletedStep {
    pub name: String,
    pub timestamp: String,
    pub output: serde_json::Value,
}

/// Deployment checkpoint data structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentCheckpoint {
    pub schema_version: u32,
    pub id: String,
    pub session_key: String,
    pub wasm_hash: String,
    pub wasm_path: String,
    pub network: String,
    pub wallet: Option<String>,
    pub status: CheckpointStatus,
    pub completed_steps: Vec<CompletedStep>,
    pub failed_step: Option<(String, String)>,
    pub config_hash: String,
    pub created_at: String,
    pub updated_at: String,
}

impl DeploymentCheckpoint {
    pub fn new(
        session_key: &str,
        wasm_hash: &str,
        wasm_path: &str,
        network: &str,
        wallet: Option<&str>,
        config_hash: &str,
    ) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: uuid::Uuid::new_v4().to_string(),
            session_key: session_key.to_string(),
            wasm_hash: wasm_hash.to_string(),
            wasm_path: wasm_path.to_string(),
            network: network.to_string(),
            wallet: wallet.map(String::from),
            status: CheckpointStatus::InProgress,
            completed_steps: Vec::new(),
            failed_step: None,
            config_hash: config_hash.to_string(),
            created_at: now.clone(),
            updated_at: now,
        }
    }

    /// Check if a step has already been successfully completed.
    pub fn is_step_completed(&self, step_name: &str) -> bool {
        self.completed_steps.iter().any(|s| s.name == step_name)
    }

    /// Retrieve the deserialized output of a completed step if present.
    pub fn get_step_output<T: serde::de::DeserializeOwned>(
        &self,
        step_name: &str,
    ) -> Option<Result<T>> {
        self.completed_steps
            .iter()
            .find(|s| s.name == step_name)
            .map(|s| {
                serde_json::from_value(s.output.clone())
                    .context("Failed to deserialize step output")
            })
    }

    /// Record a step completion with output.
    pub fn record_step_completion(
        &mut self,
        step_name: &str,
        output: &impl Serialize,
    ) -> Result<()> {
        let val = serde_json::to_value(output).context("Failed to serialize step output")?;
        if let Some(existing) = self
            .completed_steps
            .iter_mut()
            .find(|s| s.name == step_name)
        {
            existing.timestamp = Utc::now().to_rfc3339();
            existing.output = val;
        } else {
            self.completed_steps.push(CompletedStep {
                name: step_name.to_string(),
                timestamp: Utc::now().to_rfc3339(),
                output: val,
            });
        }
        self.updated_at = Utc::now().to_rfc3339();
        self.failed_step = None;
        Ok(())
    }

    /// Record step failure without losing completed steps.
    pub fn record_step_failure(&mut self, step_name: &str, error_msg: &str) {
        self.status = CheckpointStatus::Failed;
        self.failed_step = Some((step_name.to_string(), error_msg.to_string()));
        self.updated_at = Utc::now().to_rfc3339();
    }

    /// Mark the checkpoint as fully completed.
    pub fn mark_completed(&mut self) {
        self.status = CheckpointStatus::Completed;
        self.updated_at = Utc::now().to_rfc3339();
        self.failed_step = None;
    }
}

/// Compute SHA-256 hash of WASM bytes for content verification.
pub fn compute_wasm_content_hash(wasm_bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(wasm_bytes);
    hex::encode(hasher.finalize())
}

/// Compute deterministic session key from WASM hash, network, and wallet.
pub fn compute_session_key(wasm_hash: &str, network: &str, wallet: Option<&str>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(wasm_hash.as_bytes());
    hasher.update(b":");
    hasher.update(network.as_bytes());
    hasher.update(b":");
    hasher.update(wallet.unwrap_or("default").as_bytes());
    hex::encode(hasher.finalize())
}

/// Compute config hash for detecting configuration drift.
pub fn compute_config_hash(network: &str, wallet: Option<&str>, flags: &[(&str, bool)]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(network.as_bytes());
    hasher.update(b"|");
    hasher.update(wallet.unwrap_or("").as_bytes());
    for (name, val) in flags {
        hasher.update(name.as_bytes());
        hasher.update(if *val { b"1" } else { b"0" });
    }
    hex::encode(hasher.finalize())
}

/// Directory for storing checkpoint files.
pub fn checkpoints_dir() -> Result<PathBuf> {
    let dir = crate::utils::config::config_dir().join("checkpoints");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
        set_restricted_dir_permissions(&dir)?;
    }
    Ok(dir)
}

/// Helper to set restricted directory permissions (0700 on Unix).
pub fn set_restricted_dir_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o700);
        fs::set_permissions(path, perms)?;
    }
    let _ = path;
    Ok(())
}

/// Helper to set restricted file permissions (0600 on Unix).
pub fn set_restricted_file_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(path, perms)?;
    }
    let _ = path;
    Ok(())
}

/// Checks if a process PID is alive.
pub fn is_pid_active(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        // Simple process check: if PID is current process it's alive, otherwise check via tasklist/sys
        if pid == std::process::id() {
            return true;
        }
        // Fallback file age check handled caller side
        true
    }
    #[cfg(unix)]
    {
        if pid == std::process::id() {
            return true;
        }
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
}

/// Concurrency file lock guard.
#[derive(Debug)]
pub struct DeploymentLock {
    lock_path: PathBuf,
}

impl DeploymentLock {
    /// Acquire exclusive lock for a session key.
    pub fn acquire(session_key: &str) -> Result<Self> {
        let lock_dir = checkpoints_dir()?;
        let lock_path = lock_dir.join(format!("{}.lock", session_key));

        if lock_path.exists() {
            let mut is_stale = false;
            let mut stale_pid: Option<u32> = None;

            if let Ok(content) = fs::read_to_string(&lock_path) {
                for line in content.lines() {
                    if line.starts_with("PID: ") {
                        if let Ok(pid) = line["PID: ".len()..].trim().parse::<u32>() {
                            stale_pid = Some(pid);
                            if !is_pid_active(pid) {
                                is_stale = true;
                            }
                        }
                    }
                }
            }

            // Also check lock file modification age (stale if > 10 minutes)
            if let Ok(meta) = fs::metadata(&lock_path) {
                if let Ok(elapsed) = meta.modified().and_then(|m| {
                    m.elapsed()
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
                }) {
                    if elapsed > std::time::Duration::from_secs(600) {
                        is_stale = true;
                    }
                }
            }

            if is_stale {
                crate::utils::print::warn(&format!(
                    "Detected stale lock file at '{}' (PID: {:?}). Purging stale lock.",
                    lock_path.display(),
                    stale_pid
                ));
                let _ = fs::remove_file(&lock_path);
            } else {
                anyhow::bail!(
                    "Deployment already in progress for session '{}' (lock file '{}' is held by active process PID {:?}).",
                    session_key,
                    lock_path.display(),
                    stale_pid
                );
            }
        }

        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .with_context(|| format!("Failed to acquire lock for session '{}'", session_key))?;

        let pid_info = format!(
            "PID: {}\nTime: {}\n",
            std::process::id(),
            Utc::now().to_rfc3339()
        );
        let _ = file.write_all(pid_info.as_bytes());
        set_restricted_file_permissions(&lock_path)?;

        Ok(Self { lock_path })
    }
}

impl Drop for DeploymentLock {
    fn drop(&mut self) {
        if self.lock_path.exists() {
            let _ = fs::remove_file(&self.lock_path);
        }
    }
}

/// Manager for checkpoint persistence, loading, staleness checking, and corruption recovery.
pub struct DeploymentCheckpointManager;

impl DeploymentCheckpointManager {
    /// Save a checkpoint atomically to disk (tmp file write + rename).
    pub fn save(checkpoint: &DeploymentCheckpoint) -> Result<PathBuf> {
        let dir = checkpoints_dir()?;
        let target_path = dir.join(format!("{}.json", checkpoint.session_key));
        let tmp_path = dir.join(format!("{}.json.tmp", checkpoint.session_key));

        let data = serde_json::to_string_pretty(checkpoint)
            .context("Failed to serialize deployment checkpoint")?;

        fs::write(&tmp_path, data).with_context(|| {
            format!(
                "Failed to write temporary checkpoint to {}",
                tmp_path.display()
            )
        })?;

        fs::rename(&tmp_path, &target_path).with_context(|| {
            format!(
                "Failed to perform atomic rename of checkpoint from {} to {}",
                tmp_path.display(),
                target_path.display()
            )
        })?;

        set_restricted_file_permissions(&target_path)?;

        Ok(target_path)
    }

    /// Load existing checkpoint or create a fresh one if missing, stale, or corrupted.
    pub fn load_or_create(
        session_key: &str,
        current_wasm_hash: &str,
        wasm_path: &str,
        network: &str,
        wallet: Option<&str>,
        current_config_hash: &str,
        fresh: bool,
    ) -> Result<(DeploymentCheckpoint, bool)> {
        let dir = checkpoints_dir()?;
        let checkpoint_path = dir.join(format!("{}.json", session_key));

        if fresh && checkpoint_path.exists() {
            tracing::info!(
                "Fresh flag set: purging existing checkpoint at {}",
                checkpoint_path.display()
            );
            let _ = fs::remove_file(&checkpoint_path);
        }

        if checkpoint_path.exists() {
            match fs::read_to_string(&checkpoint_path) {
                Ok(raw) => match serde_json::from_str::<DeploymentCheckpoint>(&raw) {
                    Ok(cp) => {
                        // Check schema version
                        if cp.schema_version != CURRENT_SCHEMA_VERSION {
                            crate::utils::print::warn(&format!(
                                "Checkpoint at {} has obsolete schema version {} (expected {}). Starting fresh deployment.",
                                checkpoint_path.display(),
                                cp.schema_version,
                                CURRENT_SCHEMA_VERSION
                            ));
                            let _ = fs::remove_file(&checkpoint_path);
                        } else if cp.wasm_hash != current_wasm_hash {
                            crate::utils::print::warn(&format!(
                                "WASM file content changed for session. Discarding stale checkpoint at {}.",
                                checkpoint_path.display()
                            ));
                            let _ = fs::remove_file(&checkpoint_path);
                        } else if cp.config_hash != current_config_hash {
                            crate::utils::print::warn(&format!(
                                "Deployment configuration changed for session. Discarding stale checkpoint at {}.",
                                checkpoint_path.display()
                            ));
                            let _ = fs::remove_file(&checkpoint_path);
                        } else {
                            // Checkpoint is valid and active!
                            return Ok((cp, true));
                        }
                    }
                    Err(e) => {
                        crate::utils::print::warn(&format!(
                            "Warning: Checkpoint file at '{}' is corrupt or invalid ({}). Discarding and starting fresh deployment.",
                            checkpoint_path.display(),
                            e
                        ));
                        let _ = fs::remove_file(&checkpoint_path);
                    }
                },
                Err(e) => {
                    crate::utils::print::warn(&format!(
                        "Warning: Could not read checkpoint file at '{}': {}. Discarding and starting fresh deployment.",
                        checkpoint_path.display(),
                        e
                    ));
                    let _ = fs::remove_file(&checkpoint_path);
                }
            }
        }

        let new_cp = DeploymentCheckpoint::new(
            session_key,
            current_wasm_hash,
            wasm_path,
            network,
            wallet,
            current_config_hash,
        );
        Self::save(&new_cp)?;
        Ok((new_cp, false))
    }

    /// Clear checkpoint file for a session.
    pub fn clear(session_key: &str) -> Result<()> {
        let dir = checkpoints_dir()?;
        let checkpoint_path = dir.join(format!("{}.json", session_key));
        if checkpoint_path.exists() {
            fs::remove_file(checkpoint_path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn set_temp_config_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        crate::utils::config::set_test_config_dir(dir.path().to_path_buf());
        dir
    }

    #[test]
    fn test_checkpoint_creation_and_serde() {
        let cp = DeploymentCheckpoint::new(
            "session1",
            "hash123",
            "a.wasm",
            "testnet",
            Some("alice"),
            "conf123",
        );
        assert_eq!(cp.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(cp.status, CheckpointStatus::InProgress);

        let json = serde_json::to_string(&cp).unwrap();
        let loaded: DeploymentCheckpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.session_key, "session1");
    }

    #[test]
    fn test_lock_acquire_and_release() {
        let _home = set_temp_config_dir();
        let session = "test_session_lock";
        let lock1 = DeploymentLock::acquire(session);
        assert!(lock1.is_ok());

        let lock2 = DeploymentLock::acquire(session);
        assert!(lock2.is_err());
        assert!(lock2
            .unwrap_err()
            .to_string()
            .contains("already in progress"));

        drop(lock1);
        let lock3 = DeploymentLock::acquire(session);
        assert!(lock3.is_ok());
    }

    #[test]
    fn test_staleness_and_corruption_handling() {
        let _home = set_temp_config_dir();
        let session = "test_session_stale";

        // Create initial checkpoint
        let (cp, resumed) = DeploymentCheckpointManager::load_or_create(
            session,
            "hash1",
            "a.wasm",
            "testnet",
            Some("alice"),
            "cfg1",
            false,
        )
        .unwrap();
        assert!(!resumed);
        assert_eq!(cp.wasm_hash, "hash1");

        // Reload with same parameters should resume
        let (_, resumed2) = DeploymentCheckpointManager::load_or_create(
            session,
            "hash1",
            "a.wasm",
            "testnet",
            Some("alice"),
            "cfg1",
            false,
        )
        .unwrap();
        assert!(resumed2);

        // Reload with changed WASM content hash should detect staleness and reset
        let (cp3, resumed3) = DeploymentCheckpointManager::load_or_create(
            session,
            "hash2_new",
            "a.wasm",
            "testnet",
            Some("alice"),
            "cfg1",
            false,
        )
        .unwrap();
        assert!(!resumed3);
        assert_eq!(cp3.wasm_hash, "hash2_new");
    }
}
