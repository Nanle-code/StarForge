use crate::plugins::interface::CORE_VERSION;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Filename searched beside the plugin library or in the plugin install directory.
pub const MANIFEST_FILENAME: &str = "starforge-plugin.toml";

/// Formally defines StarForge's supported-version policy rules for plugins.
#[derive(Debug, Clone)]
pub struct SupportedVersionPolicy {
    /// Running StarForge CLI version.
    pub running_core_version: String,
    /// Extracted running major version component.
    pub supported_major: u64,
}

impl Default for SupportedVersionPolicy {
    fn default() -> Self {
        Self::new(CORE_VERSION)
    }
}

impl SupportedVersionPolicy {
    pub fn new(running_core_version: &str) -> Self {
        let supported_major = parse_version_parts(running_core_version)
            .map(|(m, _, _)| m)
            .unwrap_or(0);
        Self {
            running_core_version: running_core_version.to_string(),
            supported_major,
        }
    }

    /// Evaluates compatibility of a plugin manifest against this supported-version policy.
    pub fn evaluate(&self, manifest: &PluginManifest) -> Result<()> {
        let (plugin_major, _, _) =
            parse_version_parts(&manifest.starforge_version).ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid 'starforge_version' format in manifest for '{}': '{}'",
                    manifest.name,
                    manifest.starforge_version
                )
            })?;

        if plugin_major != self.supported_major {
            anyhow::bail!(
                "Plugin '{}' targets StarForge major version {}, which is incompatible with running StarForge major version {}.\n\n  \
                 Supported-Version Policy: Major versions must match exactly to guarantee ABI safety.\n  \
                 Rebuild the plugin targeting StarForge {} or update your StarForge CLI.",
                manifest.name,
                plugin_major,
                self.supported_major,
                self.running_core_version,
            );
        }

        if let Some(ref min) = manifest.starforge_version_min {
            if !version_at_least(&self.running_core_version, min) {
                anyhow::bail!(
                    "Plugin '{}' policy failure: requires StarForge >= {} (running {})",
                    manifest.name,
                    min,
                    self.running_core_version
                );
            }
        }

        if let Some(ref max) = manifest.starforge_version_max {
            if !version_at_most(&self.running_core_version, max) {
                anyhow::bail!(
                    "Plugin '{}' policy failure: requires StarForge <= {} (running {})",
                    manifest.name,
                    max,
                    self.running_core_version
                );
            }
        }

        Ok(())
    }

    /// Returns policy guidance text summarizing supported CLI version constraints.
    pub fn policy_summary(&self) -> String {
        format!(
            "StarForge Supported-Version Policy:\n  \
             - Current CLI Version: {}\n  \
             - Target Major Version: {}\n  \
             - Major Version Match: Required (ABI stability enforcement)\n  \
             - Range Constraints: Enforced via `starforge_version_min` / `starforge_version_max`",
            self.running_core_version, self.supported_major
        )
    }
}

/// Plugin manifest schema — required for distribution; enforces CLI compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Plugin command name (must match install name).
    pub name: String,
    /// Plugin semver.
    pub version: String,
    /// StarForge CLI version this plugin was built for (e.g. "0.1.0").
    pub starforge_version: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// Optional minimum StarForge version (semver).
    #[serde(default)]
    pub starforge_version_min: Option<String>,
    /// Optional maximum StarForge version (semver).
    #[serde(default)]
    pub starforge_version_max: Option<String>,
    /// Capabilities this AI plugin requires.
    #[serde(default)]
    pub required_capabilities: Vec<String>,
}

impl PluginManifest {
    /// Validate manifest fields and CLI compatibility with the running StarForge.
    pub fn validate(&self) -> Result<()> {
        self.validate_for_core(CORE_VERSION)
    }

    /// Validate manifest fields against a specific target StarForge core version.
    pub fn validate_for_core(&self, core_version: &str) -> Result<()> {
        if self.name.trim().is_empty() {
            anyhow::bail!("Plugin manifest: 'name' is required");
        }
        if self.version.trim().is_empty() {
            anyhow::bail!("Plugin manifest: 'version' is required");
        }
        if self.starforge_version.trim().is_empty() {
            anyhow::bail!(
                "Plugin manifest: 'starforge_version' is required (the StarForge CLI version this plugin targets)"
            );
        }
        let policy = SupportedVersionPolicy::new(core_version);
        policy.evaluate(self)?;

        Ok(())
    }

    /// Check if the manifest explicitly requests a specific capability.
    pub fn has_capability(&self, capability: &str) -> bool {
        let cap_lower = capability.to_lowercase();
        self.required_capabilities.iter().any(|c| {
            let existing = c.to_lowercase();
            existing == cap_lower
                || existing == "*"
                || match (existing.as_str(), cap_lower.as_str()) {
                    ("fs", "fs:read")
                    | ("fs", "fs:write")
                    | ("filesystem", "fs:read")
                    | ("filesystem", "fs:write")
                    | ("filesystemaccess", "fs:read")
                    | ("filesystemaccess", "fs:write") => true,
                    ("net", "network")
                    | ("net", "net:http")
                    | ("network", "net:http")
                    | ("network", "net:ws")
                    | ("networkaccess", "network")
                    | ("networkaccess", "net:http") => true,
                    _ => false,
                }
        })
    }

    /// Enforces that the plugin manifest has declared filesystem access capabilities.
    pub fn enforce_filesystem_access(&self, write: bool) -> Result<()> {
        let required = if write { "fs:write" } else { "fs:read" };
        if !self.has_capability(required) {
            anyhow::bail!(
                "Plugin '{}' denied filesystem access: capability '{}' is not declared in {}",
                self.name,
                required,
                MANIFEST_FILENAME
            );
        }
        Ok(())
    }

    /// Enforces that the plugin manifest has declared network access capabilities.
    pub fn enforce_network_access(&self) -> Result<()> {
        if !self.has_capability("network") {
            anyhow::bail!(
                "Plugin '{}' denied network access: capability 'network' is not declared in {}",
                self.name,
                MANIFEST_FILENAME
            );
        }
        Ok(())
    }
}

/// Locate and parse `starforge-plugin.toml` beside the library or in its parent directory.
pub fn load_manifest_for_library(library_path: &Path) -> Result<Option<PluginManifest>> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(parent) = library_path.parent() {
        candidates.push(parent.join(MANIFEST_FILENAME));
        // Plugin install layout: ~/.starforge/plugins/<name>/lib*.so + manifest
        if let Some(grandparent) = parent.parent() {
            candidates.push(grandparent.join(MANIFEST_FILENAME));
        }
    }

    for path in candidates {
        if path.is_file() {
            let contents = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read plugin manifest {}", path.display()))?;
            let manifest: PluginManifest = toml::from_str(&contents).with_context(|| {
                format!(
                    "Failed to parse plugin manifest {}. \
                     Required fields: name, version, starforge_version",
                    path.display()
                )
            })?;
            return Ok(Some(manifest));
        }
    }

    Ok(None)
}

/// Require a manifest when installing; returns a clear error if missing.
pub fn require_compatible_manifest(
    library_path: &Path,
    install_name: &str,
) -> Result<PluginManifest> {
    match load_manifest_for_library(library_path)? {
        Some(manifest) => {
            if manifest.name != install_name {
                anyhow::bail!(
                    "Plugin manifest name '{}' does not match install name '{}'",
                    manifest.name,
                    install_name
                );
            }
            manifest.validate()?;
            Ok(manifest)
        }
        None => {
            anyhow::bail!(
                "Plugin manifest not found. Place '{}' next to the plugin library with:\n\n  \
                 name = \"{}\"\n  \
                 version = \"1.0.0\"\n  \
                 starforge_version = \"{}\"\n\n  \
                 This declares which StarForge CLI version the plugin is compatible with.",
                MANIFEST_FILENAME,
                install_name,
                CORE_VERSION
            );
        }
    }
}

/// User-friendly compatibility message when binary declaration fails (no manifest).
pub fn format_binary_incompatibility(plugin_core: &str, path: &str) -> String {
    format!(
        "Plugin version incompatibility in '{path}':\n  \
         Plugin was built for StarForge {plugin_core}\n  \
         Running StarForge {core}\n\n  \
         The major version must match. Add a '{manifest}' with starforge_version = \"{core}\" \
         and rebuild the plugin, or install a compatible StarForge version.",
        path = path,
        plugin_core = plugin_core,
        core = CORE_VERSION,
        manifest = MANIFEST_FILENAME,
    )
}

fn parse_version_parts(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    Some((major, minor, patch))
}

fn version_at_least(running: &str, min: &str) -> bool {
    match (parse_version_parts(running), parse_version_parts(min)) {
        (Some(a), Some(b)) => a >= b,
        _ => true,
    }
}

fn version_at_most(running: &str, max: &str) -> bool {
    match (parse_version_parts(running), parse_version_parts(max)) {
        (Some(a), Some(b)) => a <= b,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn manifest_validates_starforge_version_major() {
        let manifest = PluginManifest {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            starforge_version: CORE_VERSION.to_string(),
            description: String::new(),
            starforge_version_min: None,
            starforge_version_max: None,
            required_capabilities: vec![],
        };
        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn manifest_rejects_incompatible_major() {
        let core_major: u64 = CORE_VERSION
            .split('.')
            .next()
            .unwrap_or("0")
            .parse()
            .unwrap_or(0);
        let other = format!("{}.0.0", core_major + 1);
        let manifest = PluginManifest {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            starforge_version: other,
            description: String::new(),
            starforge_version_min: None,
            starforge_version_max: None,
            required_capabilities: vec![],
        };
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn manifest_enforces_filesystem_and_network_capabilities() {
        let no_caps = PluginManifest {
            name: "sandboxed".to_string(),
            version: "1.0.0".to_string(),
            starforge_version: CORE_VERSION.to_string(),
            description: String::new(),
            starforge_version_min: None,
            starforge_version_max: None,
            required_capabilities: vec![],
        };

        assert!(no_caps.enforce_filesystem_access(false).is_err());
        assert!(no_caps.enforce_filesystem_access(true).is_err());
        assert!(no_caps.enforce_network_access().is_err());

        let with_caps = PluginManifest {
            name: "privileged".to_string(),
            version: "1.0.0".to_string(),
            starforge_version: CORE_VERSION.to_string(),
            description: String::new(),
            starforge_version_min: None,
            starforge_version_max: None,
            required_capabilities: vec!["fs:read".to_string(), "network".to_string()],
        };

        assert!(with_caps.enforce_filesystem_access(false).is_ok());
        assert!(with_caps.enforce_filesystem_access(true).is_err());
        assert!(with_caps.enforce_network_access().is_ok());
    }

    #[test]
    fn load_manifest_from_plugin_dir() {
        let tmp = TempDir::new().unwrap();
        let manifest_path = tmp.path().join(MANIFEST_FILENAME);
        fs::write(
            &manifest_path,
            format!(
                r#"
name = "myplugin"
version = "1.0.0"
starforge_version = "{core}"
required_capabilities = ["fs:read", "network"]
"#,
                core = CORE_VERSION
            ),
        )
        .unwrap();
        let lib = tmp.path().join("libstarforge_myplugin.so");
        fs::write(&lib, b"dummy").unwrap();
        let loaded = load_manifest_for_library(&lib).unwrap().unwrap();
        assert_eq!(loaded.name, "myplugin");
        assert!(loaded.has_capability("fs:read"));
        assert!(loaded.has_capability("network"));
    }
}
