use crate::utils::ollama;
use crate::utils::template_vcs::{get_version_history, TemplateChangelog, TemplateVersion};
use anyhow::{Context, Result};
use semver::Version;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChangeAnalysis {
    pub changes: Vec<String>,
    pub breaking_changes: Vec<String>,
    pub impact_level: String,
    pub summary: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompatibilityCheck {
    pub compatible: bool,
    pub issues: Vec<String>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpdateSuggestion {
    pub suggested_version: String,
    pub reason: String,
    pub benefits: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MigrationGuide {
    pub from_version: String,
    pub to_version: String,
    pub steps: Vec<String>,
    pub warnings: Vec<String>,
}

pub async fn analyze_changes(template_path: &Path) -> Result<ChangeAnalysis> {
    let diff = get_template_diff(template_path)?;
    let prompt = build_analysis_prompt(&diff);

    let response = ollama::generate(
        ollama::DEFAULT_MODEL,
        &prompt,
        Some(ollama::GenerateOptions {
            temperature: Some(0.3),
            num_predict: Some(1024),
            num_ctx: Some(4096),
        }),
    )
    .await
    .context("Failed to analyze changes with AI")?;

    parse_change_analysis(&response.response)
}

pub async fn check_compatibility(
    template_path: &Path,
    from_version: &str,
    to_version: &str,
) -> Result<CompatibilityCheck> {
    let versions = get_version_history(template_path)?;
    let from_ver = find_version(&versions, from_version)?;
    let to_ver = find_version(&versions, to_version)?;

    let diff = get_version_diff(template_path, &from_ver.tag, &to_ver.tag)?;
    let prompt = build_compatibility_prompt(&diff, from_version, to_version);

    let response = ollama::generate(
        ollama::DEFAULT_MODEL,
        &prompt,
        Some(ollama::GenerateOptions {
            temperature: Some(0.3),
            num_predict: Some(1024),
            num_ctx: Some(4096),
        }),
    )
    .await
    .context("Failed to check compatibility with AI")?;

    parse_compatibility_check(&response.response)
}

pub async fn suggest_update(template_path: &Path) -> Result<UpdateSuggestion> {
    let versions = get_version_history(template_path)?;
    let latest = versions
        .versions
        .iter()
        .max_by(|a, b| {
            let a_v = Version::parse(&a.version).unwrap_or(Version::new(0, 0, 0));
            let b_v = Version::parse(&b.version).unwrap_or(Version::new(0, 0, 0));
            a_v.cmp(&b_v)
        })
        .context("No versions found")?;

    let diff = get_template_diff(template_path)?;
    let prompt = build_update_suggestion_prompt(&latest.version, &diff);

    let response = ollama::generate(
        ollama::DEFAULT_MODEL,
        &prompt,
        Some(ollama::GenerateOptions {
            temperature: Some(0.3),
            num_predict: Some(512),
            num_ctx: Some(4096),
        }),
    )
    .await
    .context("Failed to suggest update with AI")?;

    parse_update_suggestion(&response.response, &latest.version)
}

pub async fn generate_migration_guide(
    template_path: &Path,
    from_version: &str,
    to_version: &str,
) -> Result<MigrationGuide> {
    let versions = get_version_history(template_path)?;
    let from_ver = find_version(&versions, from_version)?;
    let to_ver = find_version(&versions, to_version)?;

    let diff = get_version_diff(template_path, &from_ver.tag, &to_ver.tag)?;
    let prompt = build_migration_prompt(&diff, from_version, to_version);

    let response = ollama::generate(
        ollama::DEFAULT_MODEL,
        &prompt,
        Some(ollama::GenerateOptions {
            temperature: Some(0.3),
            num_predict: Some(1536),
            num_ctx: Some(4096),
        }),
    )
    .await
    .context("Failed to generate migration guide with AI")?;

    parse_migration_guide(&response.response, from_version, to_version)
}

pub async fn rollback_to_version(template_path: &Path, version: &str) -> Result<()> {
    let versions = get_version_history(template_path)?;
    let target = find_version(&versions, version)?;

    if !is_git_repo(template_path) {
        anyhow::bail!("Not a git repository. Cannot rollback.");
    }

    let output = Command::new("git")
        .current_dir(template_path)
        .args(["checkout", &target.tag])
        .output()
        .context("Failed to checkout version")?;

    if !output.status.success() {
        anyhow::bail!(
            "git checkout failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

fn get_template_diff(template_path: &Path) -> Result<String> {
    if !is_git_repo(template_path) {
        return Ok(String::new());
    }

    let output = Command::new("git")
        .current_dir(template_path)
        .args(["diff"])
        .output()
        .context("Failed to get git diff")?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn get_version_diff(template_path: &Path, from_tag: &str, to_tag: &str) -> Result<String> {
    if !is_git_repo(template_path) {
        return Ok(String::new());
    }

    let output = Command::new("git")
        .current_dir(template_path)
        .args(["diff", from_tag, to_tag])
        .output()
        .context("Failed to get version diff")?;

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn is_git_repo(path: &Path) -> bool {
    path.join(".git").exists()
}

fn find_version<'a>(versions: &'a TemplateChangelog, version: &str) -> Result<&'a TemplateVersion> {
    versions
        .versions
        .iter()
        .find(|v| v.version == version)
        .context(format!("Version {} not found", version))
}

fn build_analysis_prompt(diff: &str) -> String {
    format!(
        "{}\
Analyze the following template changes and provide a structured analysis in this exact format:\n\
CHANGES:\n\
- [list of key changes]\n\
BREAKING_CHANGES:\n\
- [list of breaking changes or 'None']\n\
IMPACT_LEVEL: [High/Medium/Low]\n\
SUMMARY: [brief summary]\n\n\
Diff:\n\
{}",
        ollama::prompts::SYSTEM_CONTEXT,
        diff
    )
}

fn build_compatibility_prompt(diff: &str, from_ver: &str, to_ver: &str) -> String {
    format!(
        "{}\
Check compatibility between template version {} and {}. Provide:\n\
COMPATIBLE: [Yes/No]\n\
ISSUES:\n\
- [list of compatibility issues]\n\
RECOMMENDATIONS:\n\
- [list of recommendations]\n\n\
Diff:\n\
{}",
        ollama::prompts::SYSTEM_CONTEXT,
        from_ver,
        to_ver,
        diff
    )
}

fn build_update_suggestion_prompt(latest_version: &str, diff: &str) -> String {
    format!(
        "{}\
Current template version: {}\n\
Uncommitted changes available.\n\
Suggest a new version number using semantic versioning (MAJOR.MINOR.PATCH). Provide:\n\
SUGGESTED_VERSION: [version]\n\
REASON: [why this version]\n\
BENEFITS:\n\
- [list of benefits]\n\n\
Diff:\n\
{}",
        ollama::prompts::SYSTEM_CONTEXT,
        latest_version,
        diff
    )
}

fn build_migration_prompt(diff: &str, from_ver: &str, to_ver: &str) -> String {
    format!(
        "{}\
Create a migration guide from template version {} to {}. Provide:\n\
STEPS:\n\
- [step-by-step migration instructions]\n\
WARNINGS:\n\
- [list of warnings or things to watch out for]\n\n\
Diff:\n\
{}",
        ollama::prompts::SYSTEM_CONTEXT,
        from_ver,
        to_ver,
        diff
    )
}

fn parse_change_analysis(text: &str) -> Result<ChangeAnalysis> {
    let changes = extract_list(text, "CHANGES:");
    let breaking_changes = extract_list(text, "BREAKING_CHANGES:");
    let impact_level = extract_field(text, "IMPACT_LEVEL:").unwrap_or_else(|| "Medium".to_string());
    let summary =
        extract_field(text, "SUMMARY:").unwrap_or_else(|| "No summary available".to_string());

    Ok(ChangeAnalysis {
        changes,
        breaking_changes,
        impact_level,
        summary,
    })
}

fn parse_compatibility_check(text: &str) -> Result<CompatibilityCheck> {
    let compatible = extract_field(text, "COMPATIBLE:")
        .map(|s| s.eq_ignore_ascii_case("yes"))
        .unwrap_or(false);
    let issues = extract_list(text, "ISSUES:");
    let recommendations = extract_list(text, "RECOMMENDATIONS:");

    Ok(CompatibilityCheck {
        compatible,
        issues,
        recommendations,
    })
}

fn parse_update_suggestion(text: &str, current_version: &str) -> Result<UpdateSuggestion> {
    let suggested_version =
        extract_field(text, "SUGGESTED_VERSION:").unwrap_or_else(|| current_version.to_string());
    let reason = extract_field(text, "REASON:").unwrap_or_else(|| "No reason provided".to_string());
    let benefits = extract_list(text, "BENEFITS:");

    Ok(UpdateSuggestion {
        suggested_version,
        reason,
        benefits,
    })
}

fn parse_migration_guide(text: &str, from_ver: &str, to_ver: &str) -> Result<MigrationGuide> {
    let steps = extract_list(text, "STEPS:");
    let warnings = extract_list(text, "WARNINGS:");

    Ok(MigrationGuide {
        from_version: from_ver.to_string(),
        to_version: to_ver.to_string(),
        steps,
        warnings,
    })
}

fn extract_list(text: &str, section: &str) -> Vec<String> {
    let mut items = Vec::new();
    let Some(start) = text.find(section) else {
        return items;
    };

    let slice = &text[start + section.len()..];
    let lines = slice.lines().skip_while(|l| l.trim().is_empty());

    for line in lines {
        let line = line.trim();
        if line.is_empty() || line.contains(':') && !line.starts_with('-') {
            break;
        }
        if let Some(item) = line.strip_prefix("- ") {
            items.push(item.trim().to_string());
        }
    }

    items
}

fn extract_field(text: &str, field: &str) -> Option<String> {
    let start = text.find(field)?;
    let slice = &text[start + field.len()..];
    let end = slice.find('\n')?;
    Some(slice[..end].trim().to_string())
}
