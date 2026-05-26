use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateRegistry {
    #[serde(default = "default_registry_version")]
    pub version: String,
    #[serde(default)]
    pub templates: Vec<TemplateEntry>,
}

impl Default for TemplateRegistry {
    fn default() -> Self {
        Self {
            version: default_registry_version(),
            templates: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateEntry {
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub source: TemplateSource,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub downloads: u64,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TemplateSource {
    Git { url: String, branch: Option<String> },
    Local { path: String },
    Builtin { id: String },
}

impl fmt::Display for TemplateSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TemplateSource::Git { url, branch } => {
                if let Some(branch) = branch {
                    write!(f, "{}#{}", url, branch)
                } else {
                    write!(f, "{}", url)
                }
            }
            TemplateSource::Local { path } => write!(f, "{}", path),
            TemplateSource::Builtin { id } => write!(f, "builtin:{}", id),
        }
    }
}

const DEFAULT_REGISTRY: &str = include_str!("../../templates/registry.json");

fn default_registry_version() -> String {
    "1".to_string()
}

fn registry_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let dir = home.join(".starforge").join("templates");
    if !dir.exists() {
        fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    }
    Ok(dir.join("registry.json"))
}

fn templates_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let dir = home.join(".starforge").join("templates").join("storage");
    if !dir.exists() {
        fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    }
    Ok(dir)
}

pub fn initialize_registry() -> Result<TemplateRegistry> {
    let registry: TemplateRegistry = serde_json::from_str(DEFAULT_REGISTRY)
        .with_context(|| "Failed to parse bundled template registry")?;
    save_registry(&registry)?;
    Ok(registry)
}

pub fn load_registry() -> Result<TemplateRegistry> {
    let path = registry_path()?;
    if !path.exists() {
        return serde_json::from_str(DEFAULT_REGISTRY)
            .with_context(|| "Failed to parse bundled template registry");
    }

    let contents = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read registry at {}", path.display()))?;
    serde_json::from_str(&contents).with_context(|| "Failed to parse template registry")
}

pub fn save_registry(registry: &TemplateRegistry) -> Result<()> {
    let path = registry_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }
    let contents = serde_json::to_string_pretty(registry)
        .with_context(|| "Failed to serialize registry")?;
    fs::write(&path, contents)
        .with_context(|| format!("Failed to write registry to {}", path.display()))?;
    Ok(())
}

pub fn search_templates(query: &str, tags: Option<&[String]>) -> Result<Vec<TemplateEntry>> {
    let registry = load_registry()?;
    let query_lower = query.to_lowercase();

    let mut results: Vec<TemplateEntry> = registry
        .templates
        .into_iter()
        .filter(|entry| {
            let query_matches = query.is_empty()
                || entry.name.to_lowercase().contains(&query_lower)
                || entry.description.to_lowercase().contains(&query_lower)
                || entry
                    .tags
                    .iter()
                    .any(|tag| tag.to_lowercase().contains(&query_lower));

            let tags_match = match tags {
                Some(required) => required.iter().all(|required_tag| {
                    entry
                        .tags
                        .iter()
                        .any(|tag| tag.eq_ignore_ascii_case(required_tag))
                }),
                None => true,
            };

            query_matches && tags_match
        })
        .collect();

    results.sort_by(|a, b| {
        b.verified
            .cmp(&a.verified)
            .then_with(|| b.downloads.cmp(&a.downloads))
            .then_with(|| a.name.cmp(&b.name))
    });

    Ok(results)
}

pub fn get_template(name: &str) -> Result<TemplateEntry> {
    let registry = load_registry()?;
    registry
        .templates
        .into_iter()
        .find(|entry| entry.name == name)
        .ok_or_else(|| anyhow::anyhow!("Template '{}' not found in registry", name))
}

pub fn add_template(entry: TemplateEntry) -> Result<()> {
    let mut registry = load_registry()?;
    if let Some(existing) = registry.templates.iter_mut().find(|item| item.name == entry.name) {
        *existing = entry;
    } else {
        registry.templates.push(entry);
    }
    save_registry(&registry)
}

pub fn remove_template(name: &str) -> Result<()> {
    let mut registry = load_registry()?;
    let before = registry.templates.len();
    registry.templates.retain(|entry| entry.name != name);

    if registry.templates.len() == before {
        anyhow::bail!("Template '{}' not found in registry", name);
    }

    save_registry(&registry)
}

pub fn publish_template(template_path: &Path) -> Result<TemplateEntry> {
    validate_template_structure(template_path)?;

    let name = template_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow::anyhow!("Could not derive a template name from {}", template_path.display()))?
        .to_string();

    let storage_dir = templates_dir()?;
    let destination = storage_dir.join(&name);
    if destination.exists() {
        anyhow::bail!(
            "Template '{}' already exists. Remove it first or use a different directory name.",
            name
        );
    }

    copy_dir_recursive(template_path, &destination)?;

    let now = chrono::Utc::now().to_rfc3339();
    let entry = TemplateEntry {
        name: name.clone(),
        version: "1.0.0".to_string(),
        description: format!("Local template published from {}", template_path.display()),
        author: "local".to_string(),
        tags: Vec::new(),
        source: TemplateSource::Local {
            path: destination.to_string_lossy().to_string(),
        },
        created_at: now.clone(),
        updated_at: now,
        downloads: 0,
        verified: false,
        path: Some(destination.to_string_lossy().to_string()),
    };

    add_template(entry.clone())?;
    Ok(entry)
}

pub fn fetch_template(entry: &TemplateEntry, dest: &Path) -> Result<()> {
    match &entry.source {
        TemplateSource::Git { url, branch } => fetch_git_template(url, branch.as_deref(), dest),
        TemplateSource::Local { path } => fetch_local_template(Path::new(path), dest),
        TemplateSource::Builtin { id } => {
            let builtin_root = PathBuf::from("templates").join("examples").join(id);
            fetch_local_template(&builtin_root, dest)
        }
    }
}

pub fn template_source_content(name: &str) -> Result<Option<String>> {
    let entry = match get_template(name) {
        Ok(entry) => entry,
        Err(_) => return Ok(None),
    };

    let lib_rs = match entry.source {
        TemplateSource::Local { path } => PathBuf::from(path).join("src").join("lib.rs"),
        TemplateSource::Builtin { id } => PathBuf::from("templates")
            .join("examples")
            .join(id)
            .join("src")
            .join("lib.rs"),
        TemplateSource::Git { .. } => return Ok(None),
    };

    if !lib_rs.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&lib_rs)
        .with_context(|| format!("Failed to read template source from {}", lib_rs.display()))?;
    Ok(Some(contents))
}

pub fn validate_template_structure(path: &Path) -> Result<()> {
    let cargo_toml = path.join("Cargo.toml");
    if !cargo_toml.exists() {
        anyhow::bail!("Template must contain Cargo.toml");
    }

    let src_dir = path.join("src");
    if !src_dir.exists() || !src_dir.is_dir() {
        anyhow::bail!("Template must contain src/ directory");
    }

    let lib_rs = src_dir.join("lib.rs");
    if !lib_rs.exists() {
        anyhow::bail!("Template must contain src/lib.rs");
    }

    Ok(())
}

fn fetch_git_template(url: &str, branch: Option<&str>, dest: &Path) -> Result<()> {
    use std::process::Command;

    let mut cmd = Command::new("git");
    cmd.arg("clone");
    if let Some(branch) = branch {
        cmd.arg("--branch").arg(branch);
    }
    cmd.arg("--depth").arg("1");
    cmd.arg(url);
    cmd.arg(dest);

    let output = cmd
        .output()
        .with_context(|| "Failed to execute git clone. Is git installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Git clone failed: {}", stderr);
    }

    let git_dir = dest.join(".git");
    if git_dir.exists() {
        fs::remove_dir_all(&git_dir).ok();
    }

    Ok(())
}

fn fetch_local_template(source: &Path, dest: &Path) -> Result<()> {
    if !source.exists() {
        anyhow::bail!("Local template path does not exist: {}", source.display());
    }
    copy_dir_recursive(source, dest)
        .with_context(|| format!("Failed to copy template from {}", source.display()))
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();

        if file_name == ".git" || file_name == "target" {
            continue;
        }

        let dest_path = dst.join(&file_name);
        if path.is_dir() {
            copy_dir_recursive(&path, &dest_path)?;
        } else {
            fs::copy(&path, &dest_path)?;
        }
    }

    Ok(())
}
