use anyhow::{Context, Result};
use semver::Version;
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
pub struct ProjectTemplateMetadata {
    pub template: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateUpgradePlan {
    pub template: String,
    pub from_version: String,
    pub to_version: String,
    pub major_steps: Vec<String>,
    pub checklist: Vec<String>,
    pub patch_hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateBranch {
    pub name: String,
    pub current: bool,
    pub last_commit: String,
    pub last_message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationSession {
    pub id: String,
    pub template_name: String,
    pub participants: Vec<String>,
    pub created_at: String,
    pub activity_log: Vec<CollaborationActivity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationActivity {
    pub author: String,
    pub message: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSuggestion {
    pub title: String,
    pub summary: String,
    pub severity: String,
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictResolution {
    pub file_path: String,
    pub conflicts: Vec<String>,
    pub recommendation: String,
    pub resolved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    pub id: String,
    pub title: String,
    pub content: String,
    pub author: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamAnalytics {
    pub template_name: String,
    pub participant_count: usize,
    pub contribution_count: usize,
    pub review_suggestion_count: usize,
    pub knowledge_entry_count: usize,
    pub last_activity: String,
}

fn vcs_dir(template_path: &Path) -> PathBuf {
    template_path.join(".starforge-vcs")
}

fn versions_file(template_path: &Path) -> PathBuf {
    vcs_dir(template_path).join("versions.json")
}

fn project_metadata_file(project_path: &Path) -> PathBuf {
    project_path.join(".starforge-template.json")
}

fn changelog_file(template_path: &Path) -> PathBuf {
    vcs_dir(template_path).join("CHANGELOG.md")
}

fn collaboration_file(template_path: &Path) -> PathBuf {
    vcs_dir(template_path).join("collaboration.json")
}

fn knowledge_file(template_path: &Path) -> PathBuf {
    vcs_dir(template_path).join("knowledge.json")
}

pub fn plan_project_upgrade(
    project_path: &Path,
    target_version: &str,
    include_patch_hints: bool,
) -> Result<Option<TemplateUpgradePlan>> {
    let metadata_path = project_metadata_file(project_path);
    if !metadata_path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&metadata_path).with_context(|| {
        format!(
            "Failed to read template metadata at {}",
            metadata_path.display()
        )
    })?;
    let metadata: ProjectTemplateMetadata = serde_json::from_str(&content).with_context(|| {
        format!(
            "Failed to parse template metadata at {}",
            metadata_path.display()
        )
    })?;
    let from = Version::parse(&metadata.version).context("Invalid template version in metadata")?;
    let to = Version::parse(target_version).context("Invalid target template version")?;

    if to <= from {
        anyhow::bail!(
            "Target template version {} must be newer than project version {}",
            to,
            from
        );
    }

    let major_steps: Vec<String> = (from.major..to.major)
        .map(|major| format!("{}.x -> {}.x", major, major + 1))
        .collect();
    let mut checklist: Vec<String> = major_steps
        .iter()
        .map(|step| format!("Review release notes and breaking changes for {}", step))
        .collect();
    checklist.extend([
        "Compare the new template files with your project and merge changes manually".to_string(),
        "Run the project test suite and review the final diff".to_string(),
    ]);

    let patch_hints = if include_patch_hints {
        vec![
            "Check dependency and configuration changes before updating package manifests"
                .to_string(),
            "Apply suggested edits by hand; StarForge does not modify project files".to_string(),
        ]
    } else {
        Vec::new()
    };

    Ok(Some(TemplateUpgradePlan {
        template: metadata.template,
        from_version: from.to_string(),
        to_version: to.to_string(),
        major_steps,
        checklist,
        patch_hints,
    }))
}

fn is_git_repo(path: &Path) -> bool {
    path.join(".git").exists()
}

/// Per-invocation `-c` overrides supplying a committer identity.
///
/// Returns an empty list when git already has one configured, so a developer's
/// own identity is never overridden. Without this, `git commit` aborts with
/// "Author identity unknown" anywhere the global config is unset — CI runners
/// and containers, most notably.
fn committer_identity_args(template_path: &Path, author: &str) -> Vec<String> {
    let configured = Command::new("git")
        .current_dir(template_path)
        .args(["config", "--get", "user.email"])
        .output()
        .map(|out| out.status.success() && !out.stdout.is_empty())
        .unwrap_or(false);

    if configured {
        return Vec::new();
    }

    let handle = author
        .trim()
        .replace(char::is_whitespace, "-")
        .to_lowercase();
    let handle = if handle.is_empty() {
        "starforge".to_string()
    } else {
        handle
    };

    vec![
        "-c".to_string(),
        format!("user.name={}", author.trim()),
        "-c".to_string(),
        format!("user.email={handle}@templates.starforge.local"),
    ]
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

        let _ = Command::new("git")
            .current_dir(template_path)
            .args(["config", "user.name", "StarForge VCS"])
            .output();
        let _ = Command::new("git")
            .current_dir(template_path)
            .args(["config", "user.email", "vcs@starforge.test"])
            .output();
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
    fs::write(
        project_metadata_file(template_path),
        serde_json::to_string_pretty(&ProjectTemplateMetadata {
            template: versions.template_name.clone(),
            version: version.to_string(),
        })?,
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
        let output = Command::new("git")
            .current_dir(template_path)
            .args(committer_identity_args(template_path, author))
            .args(["commit", "-m", &commit_msg])
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

pub fn init_collaboration(
    template_path: &Path,
    template_name: &str,
    participants: &[String],
) -> Result<CollaborationSession> {
    let session = CollaborationSession {
        id: format!("collab_{}", uuid::Uuid::new_v4()),
        template_name: template_name.to_string(),
        participants: participants.to_vec(),
        created_at: chrono::Utc::now().to_rfc3339(),
        activity_log: vec![CollaborationActivity {
            author: "system".to_string(),
            message: "Collaboration session initialized".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        }],
    };

    fs::create_dir_all(vcs_dir(template_path))?;
    fs::write(
        collaboration_file(template_path),
        serde_json::to_string_pretty(&session)?,
    )?;
    Ok(session)
}

pub fn log_collaboration_activity(template_path: &Path, author: &str, message: &str) -> Result<()> {
    let mut session = load_collaboration(template_path)?;
    session.activity_log.push(CollaborationActivity {
        author: author.to_string(),
        message: message.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    });
    fs::write(
        collaboration_file(template_path),
        serde_json::to_string_pretty(&session)?,
    )?;
    Ok(())
}

pub fn generate_ai_review_suggestions(
    template_path: &Path,
    focus: Option<&str>,
) -> Result<Vec<ReviewSuggestion>> {
    let mut suggestions = Vec::new();
    let mut files = Vec::new();
    collect_template_files(template_path, &mut files)?;

    for path in files {
        if let Ok(content) = fs::read_to_string(&path) {
            let relative = path
                .strip_prefix(template_path)
                .unwrap_or(&path)
                .display()
                .to_string();
            let lowered = content.to_lowercase();

            if lowered.contains("todo") || lowered.contains("fixme") || lowered.contains("tbd") {
                suggestions.push(ReviewSuggestion {
                    title: format!("Address TODO/FIXME items in {}", relative),
                    summary: "The template still contains unresolved placeholders that should be clarified before sharing it with collaborators.".to_string(),
                    severity: "medium".to_string(),
                    file_path: Some(relative.clone()),
                });
            }

            if relative.ends_with("README.md")
                && !lowered.contains("usage")
                && !lowered.contains("customization")
            {
                suggestions.push(ReviewSuggestion {
                    title: format!("Add documentation guidance for {}", relative),
                    summary: "The documentation should explain how to install, customize, and test the template.".to_string(),
                    severity: "low".to_string(),
                    file_path: Some(relative.clone()),
                });
            }
        }
    }

    if suggestions.is_empty() {
        let prompt = focus.unwrap_or("template review");
        suggestions.push(ReviewSuggestion {
            title: format!("Review the {} workflow", prompt),
            summary: "No obvious blockers were found; consider improving onboarding notes and coverage for collaborative handoffs.".to_string(),
            severity: "info".to_string(),
            file_path: None,
        });
    }

    Ok(suggestions)
}

pub fn resolve_template_conflicts(template_path: &Path) -> Result<Vec<ConflictResolution>> {
    let mut resolutions = Vec::new();
    let mut files = Vec::new();
    collect_template_files(template_path, &mut files)?;

    for path in files {
        if let Ok(content) = fs::read_to_string(&path) {
            let relative = path
                .strip_prefix(template_path)
                .unwrap_or(&path)
                .display()
                .to_string();
            let conflict_lines: Vec<String> = content
                .lines()
                .filter(|line| {
                    line.contains("<<<<<<<") || line.contains("=======") || line.contains(">>>>>>")
                })
                .map(|line| line.trim().to_string())
                .collect();

            if !conflict_lines.is_empty() {
                resolutions.push(ConflictResolution {
                    file_path: relative,
                    conflicts: conflict_lines,
                    recommendation: "Review each conflicted block, retain the intended version, and remove the conflict markers before committing.".to_string(),
                    resolved: false,
                });
            }
        }
    }

    Ok(resolutions)
}

pub fn share_knowledge(
    template_path: &Path,
    title: &str,
    content: &str,
    author: &str,
) -> Result<KnowledgeEntry> {
    let mut entries = get_knowledge_entries(template_path)?;
    let entry = KnowledgeEntry {
        id: format!("knowledge_{}", uuid::Uuid::new_v4()),
        title: title.to_string(),
        content: content.to_string(),
        author: author.to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    entries.push(entry.clone());
    fs::write(
        knowledge_file(template_path),
        serde_json::to_string_pretty(&entries)?,
    )?;
    Ok(entry)
}

pub fn get_knowledge_entries(template_path: &Path) -> Result<Vec<KnowledgeEntry>> {
    if !knowledge_file(template_path).exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(knowledge_file(template_path))?;
    let entries: Vec<KnowledgeEntry> = serde_json::from_str(&content).unwrap_or_default();
    Ok(entries)
}

pub fn collect_team_analytics(template_path: &Path) -> Result<TeamAnalytics> {
    let session = load_collaboration(template_path).ok();
    let versions = load_versions(template_path)?.versions.len();
    let knowledge_entries = get_knowledge_entries(template_path)?.len();
    let review_suggestions = generate_ai_review_suggestions(template_path, None)?.len();
    let last_activity = session
        .as_ref()
        .and_then(|session| session.activity_log.last())
        .map(|entry| entry.timestamp.clone())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    Ok(TeamAnalytics {
        template_name: template_path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        participant_count: session
            .map(|session| session.participants.len())
            .unwrap_or(0),
        contribution_count: versions,
        review_suggestion_count: review_suggestions,
        knowledge_entry_count: knowledge_entries,
        last_activity,
    })
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

fn load_collaboration(template_path: &Path) -> Result<CollaborationSession> {
    let path = collaboration_file(template_path);
    if !path.exists() {
        return init_collaboration(template_path, "unknown", &[]);
    }

    let content = fs::read_to_string(&path)?;
    let session: CollaborationSession = serde_json::from_str(&content)?;
    Ok(session)
}

fn collect_template_files(template_path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(template_path)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if name == ".git" || name == ".starforge-vcs" || name == "target" {
                continue;
            }
            collect_template_files(&path, files)?;
        } else if path.is_file() {
            let extension = path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or_default();
            let supported = matches!(
                extension,
                "rs" | "md" | "toml" | "json" | "txt" | "yml" | "yaml" | "sh" | "sql" | "cfg"
            );
            if supported {
                files.push(path);
            }
        }
    }

    Ok(())
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
    fn upgrade_plan_reports_missing_metadata() {
        let tmp = tempdir().unwrap();
        assert!(plan_project_upgrade(tmp.path(), "3.0.0", false)
            .unwrap()
            .is_none());
    }

    #[test]
    fn upgrade_plan_lists_each_major_transition_without_modifying_project() {
        let tmp = tempdir().unwrap();
        let metadata_path = project_metadata_file(tmp.path());
        fs::write(
            &metadata_path,
            r#"{"template":"contract","version":"1.2.0"}"#,
        )
        .unwrap();

        let plan = plan_project_upgrade(tmp.path(), "4.0.0", true)
            .unwrap()
            .unwrap();

        assert_eq!(
            plan.major_steps,
            vec![
                "1.x -> 2.x".to_string(),
                "2.x -> 3.x".to_string(),
                "3.x -> 4.x".to_string(),
            ]
        );
        assert_eq!(plan.from_version, "1.2.0");
        assert_eq!(plan.to_version, "4.0.0");
        assert!(!plan.patch_hints.is_empty());
        assert_eq!(
            fs::read_to_string(&metadata_path).unwrap(),
            r#"{"template":"contract","version":"1.2.0"}"#
        );
        assert!(!tmp.path().join("src").exists());
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
        let metadata: ProjectTemplateMetadata =
            serde_json::from_str(&fs::read_to_string(project_metadata_file(tmp.path())).unwrap())
                .unwrap();
        assert_eq!(metadata.template, "test-template");
        assert_eq!(metadata.version, "1.0.0");
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

    #[test]
    fn init_collaboration_creates_session_state() {
        let tmp = tempdir().unwrap();
        make_valid_template(tmp.path());
        init_vcs(tmp.path(), "test-template").unwrap();

        let session =
            init_collaboration(tmp.path(), "test-template", &["alice".to_string()]).unwrap();

        assert_eq!(session.template_name, "test-template");
        assert_eq!(session.participants.len(), 1);
        assert!(session.id.starts_with("collab_"));
    }

    #[test]
    fn generate_ai_review_suggestions_detects_template_issues() {
        let tmp = tempdir().unwrap();
        make_valid_template(tmp.path());
        fs::write(
            tmp.path().join("README.md"),
            "# Template\n\nTODO: add docs\n",
        )
        .unwrap();
        fs::write(
            tmp.path().join("src/lib.rs"),
            "#![no_std]\n\n// TODO: improve defaults\n",
        )
        .unwrap();

        let suggestions = generate_ai_review_suggestions(tmp.path(), Some("improve docs")).unwrap();

        assert!(!suggestions.is_empty());
        assert!(suggestions
            .iter()
            .any(|suggestion| suggestion.title.contains("TODO")
                || suggestion.title.contains("documentation")
                || suggestion.title.contains("review")));
    }
}
