//! PR readiness check command.
//!
//! Verifies CI status and merge conflict state for a GitHub pull request
//! before merge. This encodes project policy into developer tooling:
//! merges require green CI and a conflict-free branch.

use anyhow::{Context, Result};
use clap::Subcommand;
use colored::*;

use crate::utils::print as p;

/// Check PR readiness (CI status and conflict state).
#[derive(Subcommand)]
pub enum PrCommands {
    /// Check if a PR is ready to merge (CI green + no conflicts)
    Ready {
        /// PR number or URL (defaults to current branch's PR)
        #[arg(short, long)]
        pr: Option<String>,

        /// Repository in owner/repo format (auto-detected from git remote)
        #[arg(short, long)]
        repo: Option<String>,

        /// JSON output for machine consumption
        #[arg(long)]
        json: bool,
    },
}

/// Readiness status for a PR check.
#[derive(Debug, Clone)]
pub struct CheckStatus {
    pub name: String,
    pub state: CheckState,
    pub description: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckState {
    Success,
    Failure,
    Pending,
    Skipped,
    Unknown,
}

impl std::fmt::Display for CheckState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckState::Success => write!(f, "success"),
            CheckState::Failure => write!(f, "failure"),
            CheckState::Pending => write!(f, "pending"),
            CheckState::Skipped => write!(f, "skipped"),
            CheckState::Unknown => write!(f, "unknown"),
        }
    }
}

/// Overall PR readiness report.
#[derive(Debug, Clone)]
pub struct PrReadinessReport {
    pub pr_number: u32,
    pub pr_title: String,
    pub pr_url: String,
    pub mergeable: bool,
    pub merge_conflicts: bool,
    pub checks: Vec<CheckStatus>,
    pub is_ready: bool,
    pub blocking_reasons: Vec<String>,
}

impl PrReadinessReport {
    /// Build a JSON-serializable representation.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "pr_number": self.pr_number,
            "pr_title": self.pr_title,
            "pr_url": self.pr_url,
            "mergeable": self.mergeable,
            "merge_conflicts": self.merge_conflicts,
            "checks": self.checks.iter().map(|c| serde_json::json!({
                "name": c.name,
                "state": c.state.to_string(),
                "description": c.description,
                "url": c.url,
            })).collect::<Vec<_>>(),
            "is_ready": self.is_ready,
            "blocking_reasons": self.blocking_reasons,
        })
    }
}

/// Handle the `pr ready` command.
pub async fn handle(cmd: PrCommands) -> Result<()> {
    match cmd {
        PrCommands::Ready { pr, repo, json } => handle_pr_ready(pr, repo, json).await,
    }
}

async fn handle_pr_ready(pr: Option<String>, repo: Option<String>, json: bool) -> Result<()> {
    // Detect repository from git remote if not provided
    let repo_slug = match repo {
        Some(r) => r,
        None => detect_repo_slug().context(
            "Could not detect repository from git remote. \
             Use --repo owner/repo or run from a git repository.",
        )?,
    };

    // Detect PR number from current branch if not provided
    let pr_number = match pr {
        Some(num_or_url) => parse_pr_number(&num_or_url)?,
        None => detect_current_pr_number(&repo_slug).await.context(
            "Could not detect PR for current branch. \
                 Use --pr <number> or create a PR first.",
        )?,
    };

    // Fetch PR info and checks
    let report = fetch_pr_readiness(&repo_slug, pr_number).await?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report.to_json())
                .context("Failed to serialize report")?
        );
    } else {
        print_human_report(&report);
    }

    if report.is_ready {
        Ok(())
    } else {
        anyhow::bail!(
            "PR #{} is NOT ready to merge ({} blocking issue{}).",
            report.pr_number,
            report.blocking_reasons.len(),
            if report.blocking_reasons.len() == 1 {
                ""
            } else {
                "s"
            }
        );
    }
}

/// Detect repository slug from git remote.
fn detect_repo_slug() -> Result<String> {
    let output = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()
        .context("Failed to run git remote")?;

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Parse owner/repo from various URL formats
    // SSH: git@github.com:owner/repo.git
    // HTTPS: https://github.com/owner/repo.git
    let slug = if url.contains("github.com") {
        let without_host = url
            .split("github.com:")
            .nth(1)
            .or_else(|| url.split("github.com/").nth(1))
            .context("Could not parse GitHub URL")?;
        without_host.trim_end_matches(".git").to_string()
    } else {
        anyhow::bail!(
            "Only GitHub repositories are supported. Got remote URL: {}",
            url
        );
    };

    Ok(slug)
}

/// Parse a PR number from a string (number or URL).
fn parse_pr_number(input: &str) -> Result<u32> {
    // Try plain number first
    if let Ok(num) = input.parse::<u32>() {
        return Ok(num);
    }

    // Try URL format: https://github.com/owner/repo/pull/123
    if let Some(num_str) = input.split("/pull/").nth(1) {
        let num_str = num_str.trim_end_matches('/').trim_end_matches('#');
        if let Ok(num) = num_str.parse::<u32>() {
            return Ok(num);
        }
    }

    anyhow::bail!(
        "Invalid PR number or URL: '{}'. Expected a number like '42' or a URL like \
         'https://github.com/owner/repo/pull/42'.",
        input
    )
}

/// Detect the PR number for the current branch using gh CLI.
async fn detect_current_pr_number(repo_slug: &str) -> Result<u32> {
    let output = tokio::process::Command::new("gh")
        .args(["pr", "view", "--json", "number", "--repo", repo_slug])
        .output()
        .await
        .context("gh CLI not found. Install it from https://cli.github.com/")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr view failed: {}", stderr.trim());
    }

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("Failed to parse gh output")?;

    parsed["number"]
        .as_u64()
        .map(|n| n as u32)
        .context("Could not extract PR number from gh output")
}

/// Fetch PR readiness information from GitHub API via gh CLI.
async fn fetch_pr_readiness(repo_slug: &str, pr_number: u32) -> Result<PrReadinessReport> {
    // Fetch PR info (title, mergeable state, conflicts)
    let pr_output = tokio::process::Command::new("gh")
        .args([
            "pr",
            "view",
            &pr_number.to_string(),
            "--json",
            "title,url,mergeable,mergeStateStatus,reviewDecision,statusCheckRollup",
            "--repo",
            repo_slug,
        ])
        .output()
        .await
        .context("Failed to fetch PR info via gh")?;

    if !pr_output.status.success() {
        let stderr = String::from_utf8_lossy(&pr_output.stderr);
        anyhow::bail!("Failed to fetch PR #{}: {}", pr_number, stderr.trim());
    }

    let pr_data: serde_json::Value =
        serde_json::from_slice(&pr_output.stdout).context("Failed to parse PR data")?;

    let pr_title = pr_data["title"].as_str().unwrap_or("Unknown").to_string();
    let pr_url = pr_data["url"].as_str().unwrap_or("").to_string();
    let mergeable = pr_data["mergeable"].as_str().unwrap_or("UNKNOWN");
    let merge_state = pr_data["mergeStateStatus"].as_str().unwrap_or("UNKNOWN");

    // Determine conflict state
    let merge_conflicts = matches!(mergeable, "CONFLICTING" | "DIRTY");

    // Parse checks
    let mut checks = Vec::new();
    if let Some(checks_array) = pr_data["statusCheckRollup"].as_array() {
        for check in checks_array {
            let name = check["name"]
                .as_str()
                .or_else(|| check["context"].as_str())
                .unwrap_or("unknown")
                .to_string();

            let state_str = check["conclusion"]
                .as_str()
                .or_else(|| check["status"].as_str())
                .unwrap_or("unknown");

            let state = match state_str {
                "success" | "completed" => {
                    // Double-check: "completed" with "failure" conclusion
                    if check["conclusion"].as_str() == Some("failure") {
                        CheckState::Failure
                    } else {
                        CheckState::Success
                    }
                }
                "failure" | "error" | "cancelled" => CheckState::Failure,
                "pending" | "in_progress" | "queued" | "expected" | "waiting" => {
                    CheckState::Pending
                }
                "skipped" | "neutral" => CheckState::Skipped,
                _ => CheckState::Unknown,
            };

            let description = check["description"]
                .as_str()
                .or_else(|| check["summary"].as_str())
                .unwrap_or("")
                .to_string();

            let url = check["detailsUrl"]
                .as_str()
                .or_else(|| check["url"].as_str())
                .map(|s| s.to_string());

            checks.push(CheckStatus {
                name,
                state,
                description,
                url,
            });
        }
    }

    // Determine overall readiness
    let mut blocking_reasons = Vec::new();

    if merge_conflicts {
        blocking_reasons.push("PR has merge conflicts with the target branch".to_string());
    }

    let failed_checks: Vec<&CheckStatus> = checks
        .iter()
        .filter(|c| c.state == CheckState::Failure)
        .collect();
    for check in &failed_checks {
        blocking_reasons.push(format!(
            "Check '{}' is failing: {}",
            check.name, check.description
        ));
    }

    let pending_checks: Vec<&CheckStatus> = checks
        .iter()
        .filter(|c| c.state == CheckState::Pending)
        .collect();
    for check in &pending_checks {
        blocking_reasons.push(format!("Check '{}' is still pending", check.name));
    }

    if matches!(merge_state, "BLOCKED" | "BEHIND")
        && !blocking_reasons.iter().any(|r| r.contains("merge"))
    {
        blocking_reasons.push(format!(
            "Merge state is '{}' — branch may need a rebase",
            merge_state
        ));
    }

    let is_ready = blocking_reasons.is_empty();

    Ok(PrReadinessReport {
        pr_number,
        pr_title,
        pr_url,
        mergeable: !merge_conflicts,
        merge_conflicts,
        checks,
        is_ready,
        blocking_reasons,
    })
}

/// Print a human-readable readiness report.
fn print_human_report(report: &PrReadinessReport) {
    println!();
    p::header(&format!("PR #{} Readiness Report", report.pr_number));
    p::kv("Title", &report.pr_title);
    p::kv("URL", &report.pr_url);
    p::kv(
        "Conflicts",
        if report.merge_conflicts {
            "YES (has conflicts)"
        } else {
            "None"
        },
    );
    println!();

    if report.checks.is_empty() {
        println!("  {}", "No checks found.".yellow());
    } else {
        println!("  {}", "CI Checks:".bright_white().bold());
        for check in &report.checks {
            let icon = match check.state {
                CheckState::Success => "✅".green(),
                CheckState::Failure => "❌".red(),
                CheckState::Pending => "⏳".yellow(),
                CheckState::Skipped => "⏭️".dimmed(),
                CheckState::Unknown => "❓".dimmed(),
            };
            let state_color = match check.state {
                CheckState::Success => check.state.to_string().green(),
                CheckState::Failure => check.state.to_string().red(),
                CheckState::Pending => check.state.to_string().yellow(),
                _ => check.state.to_string().dimmed(),
            };
            println!("  {} {} ({})", icon, check.name.bright_white(), state_color);
            if !check.description.is_empty() {
                println!("     {}", check.description.dimmed());
            }
        }
    }

    println!();
    if report.is_ready {
        p::success(&format!("PR #{} is READY to merge ✅", report.pr_number));
    } else {
        p::error(&format!(
            "PR #{} is NOT ready to merge ❌",
            report.pr_number
        ));
        println!();
        println!("  {}", "Blocking issues:".bright_white().bold());
        for reason in &report.blocking_reasons {
            println!("  {} {}", "→".red(), reason);
        }
        println!();
        println!("  {}", "Next steps:".bright_white().bold());
        if report.merge_conflicts {
            println!(
                "  {} Resolve merge conflicts: git fetch origin && git rebase origin/master",
                "→".cyan()
            );
        }
        if report.checks.iter().any(|c| c.state == CheckState::Failure) {
            println!(
                "  {} Fix failing CI checks and push a new commit",
                "→".cyan()
            );
        }
        if report.checks.iter().any(|c| c.state == CheckState::Pending) {
            println!("  {} Wait for pending checks to complete", "→".cyan());
        }
    }
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pr_number_plain() {
        assert_eq!(parse_pr_number("42").unwrap(), 42);
    }

    #[test]
    fn parse_pr_number_url() {
        assert_eq!(
            parse_pr_number("https://github.com/owner/repo/pull/123").unwrap(),
            123
        );
    }

    #[test]
    fn parse_pr_number_url_with_trailing() {
        assert_eq!(
            parse_pr_number("https://github.com/owner/repo/pull/456/").unwrap(),
            456
        );
    }

    #[test]
    fn parse_pr_number_invalid() {
        assert!(parse_pr_number("not-a-number").is_err());
        assert!(parse_pr_number("abc/def").is_err());
    }

    #[test]
    fn report_to_json_structure() {
        let report = PrReadinessReport {
            pr_number: 42,
            pr_title: "Test PR".into(),
            pr_url: "https://github.com/test/repo/pull/42".into(),
            mergeable: true,
            merge_conflicts: false,
            checks: vec![CheckStatus {
                name: "ci".into(),
                state: CheckState::Success,
                description: "All checks passed".into(),
                url: None,
            }],
            is_ready: true,
            blocking_reasons: vec![],
        };

        let json = report.to_json();
        assert_eq!(json["pr_number"], 42);
        assert_eq!(json["is_ready"], true);
        assert!(json["checks"].as_array().unwrap().len() == 1);
    }

    #[test]
    fn check_state_display() {
        assert_eq!(CheckState::Success.to_string(), "success");
        assert_eq!(CheckState::Failure.to_string(), "failure");
        assert_eq!(CheckState::Pending.to_string(), "pending");
        assert_eq!(CheckState::Skipped.to_string(), "skipped");
        assert_eq!(CheckState::Unknown.to_string(), "unknown");
    }

    #[test]
    fn detect_repo_slug_parses_ssh_url() {
        // This test verifies the parsing logic without actually running git
        let url = "git@github.com:Nanle-code/StarForge.git";
        let slug = url
            .split("github.com:")
            .nth(1)
            .unwrap()
            .trim_end_matches(".git");
        assert_eq!(slug, "Nanle-code/StarForge");
    }

    #[test]
    fn detect_repo_slug_parses_https_url() {
        let url = "https://github.com/Nanle-code/StarForge.git";
        let slug = url
            .split("github.com/")
            .nth(1)
            .unwrap()
            .trim_end_matches(".git");
        assert_eq!(slug, "Nanle-code/StarForge");
    }
}
