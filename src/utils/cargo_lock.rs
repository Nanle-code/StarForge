use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Configuration options for Cargo.lock reproducibility verification.
#[derive(Debug, Clone)]
pub struct CargoLockVerificationConfig {
    /// Directory containing the `Cargo.toml` / `Cargo.lock` project or workspace.
    pub project_dir: PathBuf,
    /// Whether to treat non-critical warnings as failure violations.
    pub strict: bool,
}

impl Default for CargoLockVerificationConfig {
    fn default() -> Self {
        Self {
            project_dir: PathBuf::from("."),
            strict: false,
        }
    }
}

/// Detailed result report from running Cargo.lock reproducibility verification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CargoLockVerificationResult {
    /// Absolute path to the verified project directory.
    pub project_dir: String,
    /// `true` if the lockfile resolution is fully reproducible and unmodified.
    pub is_reproducible: bool,
    /// Whether `Cargo.toml` exists in the target directory.
    pub has_manifest: bool,
    /// Whether `Cargo.lock` exists in the target directory.
    pub has_lockfile: bool,
    /// `true` if locked build/check mutated the contents of `Cargo.lock`.
    pub mutated_lockfile: bool,
    /// Detailed resolution error message if `cargo check --locked` failed.
    pub resolution_error: Option<String>,
    /// Diff summary if `Cargo.lock` was modified during verification.
    pub diff_summary: Option<String>,
    /// List of validation violations/errors.
    pub violations: Vec<String>,
    /// List of non-fatal warnings.
    pub warnings: Vec<String>,
}

impl CargoLockVerificationResult {
    /// Returns `true` if the verification passed with zero violations.
    pub fn is_ok(&self) -> bool {
        self.is_reproducible && self.violations.is_empty()
    }
}

/// Helper function to check if a command executable is available in PATH.
fn is_tool_available(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Verifies Cargo.lock reproducibility for a given Rust project/workspace.
///
/// Ensures that locked builds (`cargo check --locked`) do not fail due to out-of-sync
/// dependencies or mutate `Cargo.lock` on any platform (Linux, macOS, Windows).
pub fn verify_cargo_lock_reproducibility(
    config: &CargoLockVerificationConfig,
) -> Result<CargoLockVerificationResult> {
    let raw_path = &config.project_dir;

    // ── 1. Invalid Input Handling ───────────────────────────────────────────
    if !raw_path.exists() {
        return Ok(CargoLockVerificationResult {
            project_dir: raw_path.to_string_lossy().to_string(),
            is_reproducible: false,
            has_manifest: false,
            has_lockfile: false,
            mutated_lockfile: false,
            resolution_error: None,
            diff_summary: None,
            violations: vec![format!("Project directory does not exist: {:?}", raw_path)],
            warnings: vec![],
        });
    }

    if !raw_path.is_dir() {
        return Ok(CargoLockVerificationResult {
            project_dir: raw_path.to_string_lossy().to_string(),
            is_reproducible: false,
            has_manifest: false,
            has_lockfile: false,
            mutated_lockfile: false,
            resolution_error: None,
            diff_summary: None,
            violations: vec![format!("Specified path is not a directory: {:?}", raw_path)],
            warnings: vec![],
        });
    }

    let canonical_dir = raw_path
        .canonicalize()
        .unwrap_or_else(|_| raw_path.to_path_buf());
    let manifest_path = canonical_dir.join("Cargo.toml");
    let lockfile_path = canonical_dir.join("Cargo.lock");

    let has_manifest = manifest_path.is_file();
    let has_lockfile = lockfile_path.is_file();

    if !has_manifest {
        return Ok(CargoLockVerificationResult {
            project_dir: canonical_dir.to_string_lossy().to_string(),
            is_reproducible: false,
            has_manifest: false,
            has_lockfile,
            mutated_lockfile: false,
            resolution_error: None,
            diff_summary: None,
            violations: vec![format!(
                "No Cargo.toml manifest found in directory: {:?}",
                canonical_dir
            )],
            warnings: vec![],
        });
    }

    if !has_lockfile {
        return Ok(CargoLockVerificationResult {
            project_dir: canonical_dir.to_string_lossy().to_string(),
            is_reproducible: false,
            has_manifest: true,
            has_lockfile: false,
            mutated_lockfile: false,
            resolution_error: None,
            diff_summary: None,
            violations: vec![format!(
                "No Cargo.lock file found in project directory: {:?}. Locked resolution requires a Cargo.lock file.",
                canonical_dir
            )],
            warnings: vec![],
        });
    }

    // ── 2. Unsupported Environment Handling ────────────────────────────────
    if !is_tool_available("cargo") {
        return Ok(CargoLockVerificationResult {
            project_dir: canonical_dir.to_string_lossy().to_string(),
            is_reproducible: false,
            has_manifest: true,
            has_lockfile: true,
            mutated_lockfile: false,
            resolution_error: Some("Cargo executable was not found in system PATH".to_string()),
            diff_summary: None,
            violations: vec![
                "Unsupported environment: Cargo toolchain is not available in PATH".to_string(),
            ],
            warnings: vec![],
        });
    }

    // ── 3. Read Original Cargo.lock State ────────────────────────────────────
    let initial_lock_content = fs::read_to_string(&lockfile_path)
        .with_context(|| format!("Failed to read initial Cargo.lock from {:?}", lockfile_path))?;

    // Basic validity check of Cargo.lock content
    if initial_lock_content.trim().is_empty() {
        return Ok(CargoLockVerificationResult {
            project_dir: canonical_dir.to_string_lossy().to_string(),
            is_reproducible: false,
            has_manifest: true,
            has_lockfile: true,
            mutated_lockfile: false,
            resolution_error: Some("Cargo.lock is empty".to_string()),
            diff_summary: None,
            violations: vec!["Cargo.lock file is empty (0 bytes)".to_string()],
            warnings: vec![],
        });
    }

    // ── 4. Perform Locked Resolution Verification ───────────────────────────
    let output = Command::new("cargo")
        .arg("check")
        .arg("--locked")
        .arg("--offline")
        .current_dir(&canonical_dir)
        .output();

    // Fallback: If offline check fails because dependencies are not pre-cached, run standard `cargo check --locked`
    let output = match output {
        Ok(out) if out.status.success() => Ok(out),
        _ => Command::new("cargo")
            .arg("check")
            .arg("--locked")
            .current_dir(&canonical_dir)
            .output(),
    };

    let mut violations = Vec::new();
    let mut warnings = Vec::new();
    let mut resolution_error = None;

    let check_success = match output {
        Ok(out) => {
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                resolution_error = Some(stderr.clone());
                violations.push(format!(
                    "Locked build check failed (cargo check --locked):\n{}",
                    stderr.trim()
                ));
                false
            } else {
                true
            }
        }
        Err(e) => {
            let err_msg = format!("Failed to execute cargo check --locked: {}", e);
            resolution_error = Some(err_msg.clone());
            violations.push(err_msg);
            false
        }
    };

    // ── 5. Check Cargo.lock Immutability ────────────────────────────────────
    let current_lock_content = fs::read_to_string(&lockfile_path).with_context(|| {
        format!(
            "Failed to read Cargo.lock after verification check from {:?}",
            lockfile_path
        )
    })?;

    let mutated_lockfile = initial_lock_content != current_lock_content;
    let mut diff_summary = None;

    if mutated_lockfile {
        diff_summary = Some(generate_diff_summary(
            &initial_lock_content,
            &current_lock_content,
        ));
        violations.push(
            "Cargo.lock was mutated during dependency resolution! Locked builds must be reproducible and strictly preserve Cargo.lock.".to_string()
        );

        // Restore original Cargo.lock to prevent lingering mutations in the workspace
        let _ = fs::write(&lockfile_path, &initial_lock_content);
    }

    if config.strict && !warnings.is_empty() {
        violations.push(format!(
            "Strict mode enabled: {} warning(s) treated as violations.",
            warnings.len()
        ));
    }

    let is_reproducible = check_success && !mutated_lockfile && violations.is_empty();

    Ok(CargoLockVerificationResult {
        project_dir: canonical_dir.to_string_lossy().to_string(),
        is_reproducible,
        has_manifest,
        has_lockfile,
        mutated_lockfile,
        resolution_error,
        diff_summary,
        violations,
        warnings,
    })
}

/// Generates a simple line-by-line diff summary between initial and mutated Cargo.lock strings.
fn generate_diff_summary(before: &str, after: &str) -> String {
    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();

    let mut diff = String::new();
    diff.push_str("--- Cargo.lock (original)\n+++ Cargo.lock (mutated)\n");

    let max_len = before_lines.len().max(after_lines.len());
    let mut diff_count = 0;

    for i in 0..max_len {
        let b = before_lines.get(i);
        let a = after_lines.get(i);

        if b != a {
            if let Some(old) = b {
                diff.push_str(&format!("- {}\n", old));
            }
            if let Some(new) = a {
                diff.push_str(&format!("+ {}\n", new));
            }
            diff_count += 1;
            if diff_count >= 20 {
                diff.push_str("... (diff truncated after 20 changes)\n");
                break;
            }
        }
    }

    if diff_count == 0 {
        diff.push_str("No line-by-line diff (whitespace or line ending mismatch).\n");
    }

    diff
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_cargo_lock_invalid_path_handling() {
        let non_existent = PathBuf::from("/path/that/does/not/exist/12345");
        let config = CargoLockVerificationConfig {
            project_dir: non_existent,
            strict: false,
        };
        let result = verify_cargo_lock_reproducibility(&config).unwrap();
        assert!(!result.is_reproducible);
        assert!(!result.has_manifest);
        assert!(!result.has_lockfile);
        assert!(!result.violations.is_empty());
        assert!(result.violations[0].contains("does not exist"));
    }

    #[test]
    fn test_cargo_lock_missing_manifest_and_lockfile() {
        let dir = tempdir().unwrap();
        let config = CargoLockVerificationConfig {
            project_dir: dir.path().to_path_buf(),
            strict: false,
        };
        let result = verify_cargo_lock_reproducibility(&config).unwrap();
        assert!(!result.is_reproducible);
        assert!(!result.has_manifest);
        assert!(!result.has_lockfile);
        assert!(result.violations[0].contains("No Cargo.toml manifest found"));
    }

    #[test]
    fn test_cargo_lock_missing_lockfile_only() {
        let dir = tempdir().unwrap();
        let cargo_toml = r#"
[package]
name = "dummy-pkg"
version = "0.1.0"
edition = "2021"
"#;
        fs::write(dir.path().join("Cargo.toml"), cargo_toml).unwrap();

        let config = CargoLockVerificationConfig {
            project_dir: dir.path().to_path_buf(),
            strict: false,
        };
        let result = verify_cargo_lock_reproducibility(&config).unwrap();
        assert!(!result.is_reproducible);
        assert!(result.has_manifest);
        assert!(!result.has_lockfile);
        assert!(result.violations[0].contains("No Cargo.lock file found"));
    }

    #[test]
    fn test_cargo_lock_empty_lockfile() {
        let dir = tempdir().unwrap();
        let cargo_toml = r#"
[package]
name = "dummy-pkg"
version = "0.1.0"
edition = "2021"
"#;
        fs::write(dir.path().join("Cargo.toml"), cargo_toml).unwrap();
        fs::write(dir.path().join("Cargo.lock"), "").unwrap();

        let config = CargoLockVerificationConfig {
            project_dir: dir.path().to_path_buf(),
            strict: false,
        };
        let result = verify_cargo_lock_reproducibility(&config).unwrap();
        assert!(!result.is_reproducible);
        assert!(result.has_manifest);
        assert!(result.has_lockfile);
        assert!(result.violations[0].contains("empty"));
    }

    #[test]
    fn test_cargo_lock_out_of_sync_failure() {
        let dir = tempdir().unwrap();
        let cargo_toml = r#"
[package]
name = "dummy-pkg"
version = "0.1.0"
edition = "2021"

[dependencies]
non_existent_crate_xyz_9999 = "9.9.9"
"#;
        let cargo_lock = r#"
# This file is automatically @generated by Cargo.
# It is not intended for manual editing.
version = 3

[[package]]
name = "dummy-pkg"
version = "0.1.0"
"#;
        fs::write(dir.path().join("Cargo.toml"), cargo_toml).unwrap();
        fs::write(dir.path().join("Cargo.lock"), cargo_lock).unwrap();

        let config = CargoLockVerificationConfig {
            project_dir: dir.path().to_path_buf(),
            strict: false,
        };
        let result = verify_cargo_lock_reproducibility(&config).unwrap();
        assert!(!result.is_reproducible);
        assert!(result.has_manifest);
        assert!(result.has_lockfile);
        assert!(!result.violations.is_empty());
        assert!(result.resolution_error.is_some());
    }

    #[test]
    fn test_diff_summary_generation() {
        let before = "line1\nline2\nline3\n";
        let after = "line1\nline2_changed\nline3\n";
        let diff = generate_diff_summary(before, after);
        assert!(diff.contains("- line2"));
        assert!(diff.contains("+ line2_changed"));
    }
}
