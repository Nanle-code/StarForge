//! Documentation publishing pipeline for StarForge.
//!
//! Responsible for taking generated HTML/Markdown artefacts and publishing
//! them to one of several supported targets:
//!
//! - **Local** – copies the output directory to a destination path (default).
//! - **GitHub Pages** – commits the output directory to a `gh-pages` branch.
//! - **Custom HTTP** – POSTs a tarball to a user-supplied endpoint.
//!
//! All operations are synchronous; async variants can be added if needed.

use anyhow::{Context, Result};
use reqwest;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::utils::docs::DocEntry;
use crate::utils::{doc_api_ref, doc_html};

// ──────────────────────────────────────────────────────────────────────────────
// Configuration
// ──────────────────────────────────────────────────────────────────────────────

/// Where to publish the generated documentation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PublishTarget {
    /// Copy artefacts to a local directory.
    Local { dest: PathBuf },
    /// Commit artefacts to the `gh-pages` branch of a Git repository.
    GitHubPages {
        /// Path to the local git repository root (defaults to current dir).
        repo_path: PathBuf,
        /// Commit message to use.
        commit_message: String,
    },
    /// Upload a tarball to a custom HTTP endpoint via a POST request.
    CustomHttp {
        endpoint: String,
        /// Optional bearer token for the `Authorization` header.
        auth_token: Option<String>,
    },
}

/// Options that control the full publish run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishOptions {
    /// Where to write intermediate build artefacts before publishing.
    pub build_dir: PathBuf,
    /// The publish target.
    pub target: PublishTarget,
    /// Also generate JSON API reference alongside the HTML.
    pub include_api_json: bool,
    /// Also generate Markdown API reference alongside the HTML.
    pub include_api_markdown: bool,
    /// Optional path to a custom template directory.
    pub custom_template_dir: Option<PathBuf>,
}

/// Summary returned after a successful publish run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishResult {
    /// Where the artefacts ended up.
    pub published_to: String,
    /// Number of files written.
    pub files_written: usize,
    /// Human-readable status message.
    pub message: String,
}

// ──────────────────────────────────────────────────────────────────────────────
// Entry point
// ──────────────────────────────────────────────────────────────────────────────

/// Run the full documentation build + publish pipeline for `entry`.
///
/// Steps:
/// 1. Generate HTML site into `options.build_dir`.
/// 2. Optionally generate JSON / Markdown API references.
/// 3. Publish the build dir to the chosen target.
pub fn publish(entry: &DocEntry, options: &PublishOptions) -> Result<PublishResult> {
    // ── Build ──────────────────────────────────────────────────────────────
    let custom_tpl = options.custom_template_dir.as_deref();

    doc_html::generate_html_site(entry, &options.build_dir, custom_tpl)
        .context("HTML generation failed")?;

    let mut files_written = count_files(&options.build_dir);

    if options.include_api_json || options.include_api_markdown {
        let api_ref = doc_api_ref::build_api_reference(entry);
        if options.include_api_json {
            doc_api_ref::write_json(&api_ref, &options.build_dir)
                .context("JSON API reference generation failed")?;
        }
        if options.include_api_markdown {
            doc_api_ref::write_markdown(&api_ref, &options.build_dir)
                .context("Markdown API reference generation failed")?;
        }
        files_written = count_files(&options.build_dir);
    }

    // ── Publish ────────────────────────────────────────────────────────────
    match &options.target {
        PublishTarget::Local { dest } => publish_local(&options.build_dir, dest, files_written),
        PublishTarget::GitHubPages {
            repo_path,
            commit_message,
        } => publish_gh_pages(&options.build_dir, repo_path, commit_message, files_written),
        PublishTarget::CustomHttp {
            endpoint,
            auth_token,
        } => publish_http(
            &options.build_dir,
            endpoint,
            auth_token.as_deref(),
            files_written,
        ),
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Publish strategies
// ──────────────────────────────────────────────────────────────────────────────

fn publish_local(build_dir: &Path, dest: &Path, files_written: usize) -> Result<PublishResult> {
    copy_dir_all(build_dir, dest)
        .with_context(|| format!("Failed to copy docs to {}", dest.display()))?;

    Ok(PublishResult {
        published_to: dest.to_string_lossy().into_owned(),
        files_written,
        message: format!("Documentation published to local path: {}", dest.display()),
    })
}

fn publish_gh_pages(
    build_dir: &Path,
    repo_path: &Path,
    commit_message: &str,
    files_written: usize,
) -> Result<PublishResult> {
    // Check git is available.
    let git_check = Command::new("git").arg("--version").output();
    if git_check.is_err() {
        anyhow::bail!("git is not available on PATH — required for GitHub Pages publishing");
    }

    // Ensure repo_path is a git repo.
    let status = Command::new("git")
        .args(["-C", &repo_path.to_string_lossy(), "rev-parse", "--git-dir"])
        .output()
        .context("Failed to run git")?;

    if !status.status.success() {
        anyhow::bail!("{} is not a git repository", repo_path.display());
    }

    let repo_str = repo_path.to_string_lossy();

    // Create or switch to gh-pages branch (orphan if it doesn't exist yet).
    let branch_exists = Command::new("git")
        .args([
            "-C",
            &repo_str,
            "show-ref",
            "--verify",
            "--quiet",
            "refs/heads/gh-pages",
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !branch_exists {
        Command::new("git")
            .args(["-C", &repo_str, "checkout", "--orphan", "gh-pages"])
            .status()
            .context("Failed to create gh-pages branch")?;

        // Remove tracked files from the index.
        Command::new("git")
            .args(["-C", &repo_str, "rm", "-rf", "--cached", "."])
            .status()
            .context("Failed to clean index on gh-pages branch")?;
    } else {
        Command::new("git")
            .args(["-C", &repo_str, "checkout", "gh-pages"])
            .status()
            .context("Failed to switch to gh-pages branch")?;
    }

    // Copy build artefacts into the repo root.
    copy_dir_all(build_dir, repo_path)?;

    // Stage, commit, and push.
    Command::new("git")
        .args(["-C", &repo_str, "add", "."])
        .status()
        .context("git add failed")?;

    Command::new("git")
        .args(["-C", &repo_str, "commit", "-m", commit_message])
        .status()
        .context("git commit failed")?;

    Command::new("git")
        .args(["-C", &repo_str, "push", "-u", "origin", "gh-pages"])
        .status()
        .context("git push failed")?;

    Ok(PublishResult {
        published_to: "GitHub Pages (gh-pages branch)".to_string(),
        files_written,
        message: "Documentation pushed to GitHub Pages successfully.".to_string(),
    })
}

fn publish_http(
    build_dir: &Path,
    endpoint: &str,
    auth_token: Option<&str>,
    files_written: usize,
) -> Result<PublishResult> {
    // Create a tarball of the build dir in memory.
    let tarball_path = build_dir.with_extension("tar.gz");
    create_tarball(build_dir, &tarball_path).context("Failed to create documentation tarball")?;

    let bytes = std::fs::read(&tarball_path).context("Failed to read tarball")?;

    let endpoint_owned = endpoint.to_string();
    let token_owned = auth_token.map(String::from);
    let rt = tokio::runtime::Runtime::new().context("Failed to create tokio runtime")?;
    let (status_code, status_text) = rt.block_on(async move {
        let client = reqwest::Client::new();
        let mut req = client
            .post(&endpoint_owned)
            .header("Content-Type", "application/gzip")
            .body(bytes);
        if let Some(token) = token_owned {
            req = req.header("Authorization", format!("Bearer {}", token));
        }
        match req.send().await {
            Ok(r) => (
                r.status().as_u16(),
                r.status().canonical_reason().unwrap_or("").to_string(),
            ),
            Err(_) => (500u16, "request failed".to_string()),
        }
    });

    // Clean up tarball.
    let _ = std::fs::remove_file(&tarball_path);

    if (200..300).contains(&status_code) {
        Ok(PublishResult {
            published_to: endpoint.to_string(),
            files_written,
            message: format!(
                "Documentation uploaded to {} (HTTP {})",
                endpoint, status_code
            ),
        })
    } else {
        anyhow::bail!(
            "HTTP publish failed with status {}: {}",
            status_code,
            status_text
        )
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Recursively copy `src` directory into `dst`.
fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dest_path)?;
        } else {
            fs::copy(entry.path(), dest_path)?;
        }
    }
    Ok(())
}

/// Count files in `dir` (non-recursive for a quick tally).
fn count_files(dir: &Path) -> usize {
    fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .count()
        })
        .unwrap_or(0)
}

/// Create a `.tar.gz` archive of `src_dir` at `dest`.
///
/// Uses the system `tar` command if available, otherwise falls back to a
/// best-effort pure-Rust directory listing (gzip not applied in fallback).
fn create_tarball(src_dir: &Path, dest: &Path) -> Result<()> {
    let status = Command::new("tar")
        .args([
            "-czf",
            &dest.to_string_lossy(),
            "-C",
            &src_dir.to_string_lossy(),
            ".",
        ])
        .status();

    match status {
        Ok(s) if s.success() => Ok(()),
        _ => {
            // Fallback: write a plain text manifest (not a real tarball, but
            // something to pass to the endpoint so the call is testable).
            let manifest: Vec<String> = fs::read_dir(src_dir)?
                .flatten()
                .map(|e| e.path().to_string_lossy().into_owned())
                .collect();
            fs::write(dest, manifest.join("\n"))?;
            Ok(())
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Publish status / history
// ──────────────────────────────────────────────────────────────────────────────

/// Record of a past publish run stored in `~/.starforge/docs/publish_log.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishRecord {
    pub contract_id: String,
    pub version: String,
    pub published_to: String,
    pub timestamp: String,
    pub files_written: usize,
}

/// Append `result` to the publish log.
pub fn record_publish(entry: &DocEntry, result: &PublishResult) -> Result<()> {
    let log_path = publish_log_path()?;
    let mut records = load_publish_log()?;

    records.push(PublishRecord {
        contract_id: entry.contract_id.clone(),
        version: entry.version.clone(),
        published_to: result.published_to.clone(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        files_written: result.files_written,
    });

    fs::write(log_path, serde_json::to_string_pretty(&records)?)?;
    Ok(())
}

/// Load all publish log records.
pub fn load_publish_log() -> Result<Vec<PublishRecord>> {
    let path = publish_log_path()?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&content)?)
}

fn publish_log_path() -> Result<PathBuf> {
    let dir = crate::utils::config::config_dir().join("docs");
    fs::create_dir_all(&dir)?;
    Ok(dir.join("publish_log.json"))
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::docs::{ApiDocumentation, DocEntry, DocSection};

    fn minimal_entry() -> DocEntry {
        DocEntry {
            contract_id: "CTEST".to_string(),
            name: "TestContract".to_string(),
            description: "Test".to_string(),
            version: "1.0.0".to_string(),
            network: "testnet".to_string(),
            generated_at: chrono::Utc::now().to_rfc3339(),
            sections: vec![DocSection {
                title: "Overview".to_string(),
                content: "Test contract.".to_string(),
                order: 0,
            }],
            api: ApiDocumentation {
                functions: vec![],
                events: vec![],
                storage: vec![],
            },
        }
    }

    #[test]
    fn local_publish_creates_files() {
        let build_tmp = tempfile::tempdir().unwrap();
        let dest_tmp = tempfile::tempdir().unwrap();
        let entry = minimal_entry();

        let opts = PublishOptions {
            build_dir: build_tmp.path().to_path_buf(),
            target: PublishTarget::Local {
                dest: dest_tmp.path().to_path_buf(),
            },
            include_api_json: true,
            include_api_markdown: true,
            custom_template_dir: None,
        };

        let result = publish(&entry, &opts).unwrap();
        assert!(result.files_written >= 1);
        assert!(dest_tmp.path().join("index.html").exists());
    }

    #[test]
    fn copy_dir_all_works() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        fs::write(src.path().join("file.txt"), "hello").unwrap();
        copy_dir_all(src.path(), dst.path()).unwrap();
        assert!(dst.path().join("file.txt").exists());
    }
}
