use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateVersion {
    pub version: String,
    pub tag: String,
    pub message: String,
    pub author: String,
    pub timestamp: String,
    pub changes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateChangelog {
    pub template_name: String,
    pub versions: Vec<TemplateVersion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateBranch {
    pub name: String,
    pub current: bool,
    pub last_commit: String,
    pub last_message: String,
}

fn vcs_dir(template_path: &Path) -> PathBuf {
    template_path.join(".starforge-vcs")
}

fn versions_file(template_path: &Path) -> PathBuf {
    vcs_dir(template_path).join("versions.json")
}

fn changelog_file(template_path: &Path) -> PathBuf {
    vcs_dir(template_path).join("CHANGELOG.md")
}

fn is_git_repo(path: &Path) -> bool {
    path.join(".git").exists()
}

pub fn init_vcs(template_path: &Path, template_name: &str) -> Result<()> {
    let vcs = vcs_dir(template_path);
    if vcs.exists() {
        anyhow::bail!(
            "VCS already initialized for '{}'. Use `starforge template vcs status` to check.",
            template_name
        );
    }

    fs::create_dir_all(&vcs)?;

    let versions = TemplateChangelog {
        template_name: template_name.to_string(),
        versions: Vec::new(),
    };
    fs::write(
        versions_file(template_path),
        serde_json::to_string_pretty(&versions)?,
    )?;

    if !is_git_repo(template_path) {
        let output = Command::new("git")
            .arg("init")
            .arg(template_path)
            .output()
            .context("Failed to initialize git repo. Is git installed?")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git init failed: {}", stderr);
        }
    }

    Ok(())
}

pub fn commit_version(
    template_path: &Path,
    version: &str,
    message: &str,
    author: &str,
) -> Result<TemplateVersion> {
    let mut versions = load_versions(template_path)?;

    let tag = format!("v{}", version);

    if versions.versions.iter().any(|v| v.version == version) {
        anyhow::bail!(
            "Version '{}' already exists. Bump the version number.",
            version
        );
    }

    let all_changes: Vec<String> = message
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    let entry = TemplateVersion {
        version: version.to_string(),
        tag: tag.clone(),
        message: message.to_string(),
        author: author.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        changes: all_changes,
    };

    versions.versions.push(entry.clone());
    versions.versions.sort_by(|a, b| b.version.cmp(&a.version));

    fs::write(
        versions_file(template_path),
        serde_json::to_string_pretty(&versions)?,
    )?;

    if is_git_repo(template_path) {
        let output = Command::new("git")
            .current_dir(template_path)
            .args(["add", "-A"])
            .output()
            .context("Failed to stage files")?;

        if !output.status.success() {
            anyhow::bail!(
                "git add failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let commit_msg = format!("{}: {}", tag, message.lines().next().unwrap_or(message));
        // Supply a per-invocation identity so commits succeed even when the
        // user (or a CI runner) has no global git user.name/user.email set.
        let output = Command::new("git")
            .current_dir(template_path)
            .args([
                "-c",
                "user.name=StarForge",
                "-c",
                "user.email=starforge@localhost",
                "commit",
                "-m",
                &commit_msg,
            ])
            .output()
            .context("Failed to commit")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains("nothing to commit") {
                anyhow::bail!("git commit failed: {}", stderr);
            }
        }

        let output = Command::new("git")
            .current_dir(template_path)
            .args(["tag", &tag])
            .output()
            .context("Failed to create tag")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains("already exists") {
                anyhow::bail!("git tag failed: {}", stderr);
            }
        }
    }

    update_changelog(template_path, &versions)?;

    Ok(entry)
}

pub fn list_branches(template_path: &Path) -> Result<Vec<TemplateBranch>> {
    if !is_git_repo(template_path) {
        anyhow::bail!("Not a git repository. Run `starforge template vcs init` first.");
    }

    let output = Command::new("git")
        .current_dir(template_path)
        .args(["branch", "-v"])
        .output()
        .context("Failed to list branches")?;

    if !output.status.success() {
        anyhow::bail!(
            "git branch failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut branches = Vec::new();

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let (current, name) = if line.starts_with('*') {
            (true, line.strip_prefix("* ").unwrap_or(line).trim())
        } else {
            (false, line.trim())
        };

        let parts: Vec<&str> = name.split_whitespace().collect();
        let branch_name = parts.first().unwrap_or(&name).to_string();
        let last_commit = parts.get(1).unwrap_or(&"").to_string();
        let last_message = parts[2..].join(" ");

        branches.push(TemplateBranch {
            name: branch_name,
            current,
            last_commit,
            last_message,
        });
    }

    Ok(branches)
}

pub fn create_branch(template_path: &Path, branch_name: &str) -> Result<()> {
    if !is_git_repo(template_path) {
        anyhow::bail!("Not a git repository. Run `starforge template vcs init` first.");
    }

    let output = Command::new("git")
        .current_dir(template_path)
        .args(["checkout", "-b", branch_name])
        .output()
        .context("Failed to create branch")?;

    if !output.status.success() {
        anyhow::bail!(
            "git checkout -b failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

pub fn switch_branch(template_path: &Path, branch_name: &str) -> Result<()> {
    if !is_git_repo(template_path) {
        anyhow::bail!("Not a git repository. Run `starforge template vcs init` first.");
    }

    let output = Command::new("git")
        .current_dir(template_path)
        .args(["checkout", branch_name])
        .output()
        .context("Failed to switch branch")?;

    if !output.status.success() {
        anyhow::bail!(
            "git checkout failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

pub fn view_log(template_path: &Path, limit: usize) -> Result<Vec<TemplateVersion>> {
    let versions = load_versions(template_path)?;
    let mut sorted = versions.versions;
    sorted.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(sorted.into_iter().take(limit).collect())
}

pub fn show_diff(template_path: &Path) -> Result<String> {
    if !is_git_repo(template_path) {
        anyhow::bail!("Not a git repository. Run `starforge template vcs init` first.");
    }

    let output = Command::new("git")
        .current_dir(template_path)
        .args(["diff", "--stat"])
        .output()
        .context("Failed to run git diff")?;

    if !output.status.success() {
        anyhow::bail!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(stdout)
}

pub fn create_release(
    template_path: &Path,
    version: &str,
    message: &str,
    author: &str,
) -> Result<TemplateVersion> {
    commit_version(template_path, version, message, author)
}

pub fn generate_changelog(template_path: &Path) -> Result<String> {
    let versions = load_versions(template_path)?;

    let mut output = String::new();
    output.push_str(&format!("# Changelog — {}\n\n", versions.template_name));

    for version in &versions.versions {
        output.push_str(&format!(
            "## {} ({})\n\n",
            version.tag,
            &version.timestamp[..10]
        ));
        output.push_str(&format!("**Author:** {}\n\n", version.author));

        if !version.changes.is_empty() {
            for change in &version.changes {
                output.push_str(&format!("- {}\n", change));
            }
        } else {
            output.push_str(&format!("- {}\n", version.message));
        }
        output.push('\n');
    }

    if versions.versions.is_empty() {
        output.push_str("_No versions recorded yet._\n");
    }

    fs::write(changelog_file(template_path), &output)?;
    Ok(output)
}

pub fn get_version_history(template_path: &Path) -> Result<TemplateChangelog> {
    load_versions(template_path)
}

pub fn create_release_with_notes(
    template_path: &Path,
    version: &str,
    message: &str,
    author: &str,
    notes: &str,
) -> Result<TemplateVersion> {
    let mut changes: Vec<String> = message
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    for line in notes.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            changes.push(trimmed.to_string());
        }
    }

    let combined = if changes.is_empty() {
        message.to_string()
    } else {
        changes.join("\n")
    };

    commit_version(template_path, version, &combined, author)
}

fn load_versions(template_path: &Path) -> Result<TemplateChangelog> {
    let vf = versions_file(template_path);
    if !vf.exists() {
        return Ok(TemplateChangelog {
            template_name: template_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            versions: Vec::new(),
        });
    }

    let content = fs::read_to_string(&vf)
        .with_context(|| format!("Failed to read versions file at {}", vf.display()))?;
    let versions: TemplateChangelog =
        serde_json::from_str(&content).context("Failed to parse versions file")?;
    Ok(versions)
}

fn update_changelog(template_path: &Path, versions: &TemplateChangelog) -> Result<()> {
    let mut output = String::new();
    output.push_str(&format!("# Changelog — {}\n\n", versions.template_name));

    for version in &versions.versions {
        output.push_str(&format!(
            "## {} ({})\n\n",
            version.tag,
            &version.timestamp[..10]
        ));
        output.push_str(&format!("**Author:** {}\n\n", version.author));

        if !version.changes.is_empty() {
            for change in &version.changes {
                output.push_str(&format!("- {}\n", change));
            }
        } else {
            output.push_str(&format!("- {}\n", version.message));
        }
        output.push('\n');
    }

    if versions.versions.is_empty() {
        output.push_str("_No versions recorded yet._\n");
    }

    fs::write(changelog_file(template_path), &output)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_valid_template(dir: &Path) {
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"{{PROJECT_NAME}}\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(dir.join("src/lib.rs"), "#![no_std]\n").unwrap();
        fs::write(dir.join("README.md"), "# Template\n").unwrap();
    }

    #[test]
    fn init_vcs_creates_directory_and_versions() {
        let tmp = tempdir().unwrap();
        make_valid_template(tmp.path());
        init_vcs(tmp.path(), "test-template").unwrap();
        assert!(vcs_dir(tmp.path()).exists());
        assert!(versions_file(tmp.path()).exists());
    }

    #[test]
    fn commit_version_adds_entry() {
        let tmp = tempdir().unwrap();
        make_valid_template(tmp.path());
        init_vcs(tmp.path(), "test-template").unwrap();

        let entry = commit_version(tmp.path(), "1.0.0", "Initial release", "Author").unwrap();
        assert_eq!(entry.version, "1.0.0");
        assert_eq!(entry.tag, "v1.0.0");

        let versions = load_versions(tmp.path()).unwrap();
        assert_eq!(versions.versions.len(), 1);
    }

    #[test]
    fn commit_version_rejects_duplicate() {
        let tmp = tempdir().unwrap();
        make_valid_template(tmp.path());
        init_vcs(tmp.path(), "test-template").unwrap();

        commit_version(tmp.path(), "1.0.0", "Initial release", "Author").unwrap();
        let result = commit_version(tmp.path(), "1.0.0", "Duplicate", "Author");
        assert!(result.is_err());
    }

    #[test]
    fn generate_changelog_empty() {
        let tmp = tempdir().unwrap();
        make_valid_template(tmp.path());
        init_vcs(tmp.path(), "test-template").unwrap();

        let changelog = generate_changelog(tmp.path()).unwrap();
        assert!(changelog.contains("No versions recorded yet"));
    }

    #[test]
    fn generate_changelog_with_versions() {
        let tmp = tempdir().unwrap();
        make_valid_template(tmp.path());
        init_vcs(tmp.path(), "test-template").unwrap();

        commit_version(tmp.path(), "1.0.0", "Initial", "Alice").unwrap();
        commit_version(tmp.path(), "1.1.0", "New feature", "Bob").unwrap();

        let changelog = generate_changelog(tmp.path()).unwrap();
        assert!(changelog.contains("v1.0.0"));
        assert!(changelog.contains("v1.1.0"));
        assert!(changelog.contains("Alice"));
        assert!(changelog.contains("Bob"));
    }

    #[test]
    fn view_log_returns_versions_in_reverse_chronological_order() {
        let tmp = tempdir().unwrap();
        make_valid_template(tmp.path());
        init_vcs(tmp.path(), "test-template").unwrap();

        commit_version(tmp.path(), "1.0.0", "First", "A").unwrap();
        commit_version(tmp.path(), "2.0.0", "Second", "B").unwrap();

        let log = view_log(tmp.path(), 10).unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].version, "2.0.0");
        assert_eq!(log[1].version, "1.0.0");
    }
}
