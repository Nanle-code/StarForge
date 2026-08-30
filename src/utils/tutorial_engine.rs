use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TutorialStep {
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub command: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TutorialDefinition {
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    pub steps: Vec<TutorialStep>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct TutorialStatus {
    pub active: Option<String>,
    pub started_at: Option<String>,
    pub current_step: usize,
    pub completed_steps: Vec<usize>,
}

fn status_path() -> Result<PathBuf> {
    Ok(crate::utils::config::config_dir().join("tutorial_status.json"))
}

pub fn load_status() -> Result<TutorialStatus> {
    let path = status_path()?;
    if !path.exists() {
        return Ok(TutorialStatus::default());
    }
    let bytes = fs::read(&path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn save_status(status: &TutorialStatus) -> Result<()> {
    let path = status_path()?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let bytes = serde_json::to_vec_pretty(status)?;
    fs::write(&path, bytes)?;
    Ok(())
}

pub fn tutorials_dir(repo_root: &Path) -> PathBuf {
    repo_root.join("tutorials")
}

pub fn tutorial_manifest_path(repo_root: &Path, slug: &str) -> PathBuf {
    tutorials_dir(repo_root).join(slug).join("tutorial.json")
}

pub fn load_tutorial(repo_root: &Path, slug: &str) -> Result<TutorialDefinition> {
    let path = tutorial_manifest_path(repo_root, slug);
    if !path.exists() {
        anyhow::bail!(
            "Tutorial manifest missing at {}. Add tutorial.json for structured steps.",
            path.display()
        );
    }
    let bytes = fs::read(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let mut definition: TutorialDefinition = serde_json::from_slice(&bytes)?;
    if definition.slug.is_empty() {
        definition.slug = slug.to_string();
    }
    if definition.steps.is_empty() {
        anyhow::bail!("Tutorial '{}' has no steps defined", slug);
    }
    Ok(definition)
}

pub fn list_tutorial_slugs(repo_root: &Path) -> Result<Vec<String>> {
    let dir = tutorials_dir(repo_root);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut slugs = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.path().is_dir() {
            slugs.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    slugs.sort();
    Ok(slugs)
}

pub fn render_step(step: &TutorialStep, index: usize, total: usize) -> String {
    let mut lines = vec![
        format!("Step {}/{}: {}", index + 1, total, step.title),
        step.description.clone(),
    ];
    if let Some(cmd) = &step.command {
        lines.push(format!("Run: {}", cmd));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_step_includes_command_hint() {
        let step = TutorialStep {
            title: "Check environment".into(),
            description: "Run info".into(),
            command: Some("starforge info".into()),
        };
        let rendered = render_step(&step, 0, 3);
        assert!(rendered.contains("Step 1/3"));
        assert!(rendered.contains("starforge info"));
    }
}
