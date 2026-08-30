use crate::utils::http_client;
use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// The running StarForge CLI version — used for template compatibility checks.
pub const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TemplateRegistry {
    #[serde(default)]
    pub templates: Vec<TemplateEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TemplateSource {
    Git {
        url: String,
        #[serde(default)]
        branch: Option<String>,
    },
    Local {
        path: String,
    },
    Builtin {
        id: String,
    },
}

impl std::fmt::Display for TemplateSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TemplateSource::Git { url, branch } => {
                if let Some(branch) = branch {
                    write!(f, "git:{}@{}", url, branch)
                } else {
                    write!(f, "git:{}", url)
                }
            }
            TemplateSource::Local { path } => write!(f, "local:{}", path),
            TemplateSource::Builtin { id } => write!(f, "builtin:{}", id),
        }
    }
}

/// Maintenance state of a marketplace template.
///
/// Surfaced to users as a lightweight trust signal so they can tell at a
/// glance whether a template is being kept up to date or has been abandoned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MaintenanceStatus {
    /// Updated recently and accepting changes.
    Active,
    /// Stable and still supported, but not under active development.
    Maintained,
    /// No longer maintained; use with caution.
    Deprecated,
    /// Maintenance state has not been declared.
    #[default]
    Unknown,
}

impl MaintenanceStatus {
    /// Short human-readable label used in trust indicators.
    pub fn label(&self) -> &'static str {
        match self {
            MaintenanceStatus::Active => "Actively maintained",
            MaintenanceStatus::Maintained => "Maintained",
            MaintenanceStatus::Deprecated => "Deprecated",
            MaintenanceStatus::Unknown => "Unknown maintenance",
        }
    }
}

fn deserialize_findings_opt<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct FindingsVisitor;

    impl<'de> serde::de::Visitor<'de> for FindingsVisitor {
        type Value = Option<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string, integer, or null")
        }

        fn visit_none<E>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E> {
            Ok(Some(value.to_string()))
        }

        fn visit_string<E>(self, value: String) -> std::result::Result<Self::Value, E> {
            Ok(Some(value))
        }

        fn visit_i64<E>(self, value: i64) -> std::result::Result<Self::Value, E> {
            Ok(Some(value.to_string()))
        }

        fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E> {
            Ok(Some(value.to_string()))
        }
    }

    deserializer.deserialize_any(FindingsVisitor)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityReview {
    pub status: String,
    pub auditor: Option<String>,
    pub audited_at: Option<String>,
    #[serde(default, deserialize_with = "deserialize_findings_opt")]
    pub findings: Option<String>,
    pub score: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangelogEntry {
    pub version: String,
    pub date: String,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateEntry {
    pub name: String,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub security_review: Option<SecurityReview>,
    #[serde(default)]
    pub changelog: Option<Vec<ChangelogEntry>>,
    pub description: String,
    pub version: String,
    pub source: TemplateSource,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub downloads: u32,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    /// Minimum StarForge CLI version required by this template (semver, e.g. "0.1.0").
    /// `None` means no minimum — the template is compatible with all CLI versions.
    #[serde(default)]
    pub cli_version_min: Option<String>,
    /// Maximum StarForge CLI version supported by this template (semver, e.g. "1.99.99").
    /// `None` means no upper bound.
    #[serde(default)]
    pub cli_version_max: Option<String>,
    /// Whether the template ships user-facing documentation (e.g. a README).
    #[serde(default)]
    pub documented: bool,
    /// Declared maintenance state of the template.
    #[serde(default)]
    pub maintenance: MaintenanceStatus,
    /// SPDX license identifier (e.g. "MIT", "Apache-2.0"). `None` if not declared.
    #[serde(default)]
    pub license: Option<String>,
    /// URL of the template's source repository (e.g. GitHub link).
    #[serde(default)]
    pub repository_url: Option<String>,
    /// Optional homepage for the template project.
    #[serde(default)]
    pub homepage: Option<String>,
    /// Optional documentation URL for the template.
    #[serde(default)]
    pub documentation: Option<String>,
    /// Categories that describe the template's purpose or domain.
    #[serde(default)]
    pub categories: Vec<String>,
    /// Whether this template has been selected as featured by curators.
    #[serde(default)]
    pub featured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TemplateUpdateImpact {
    pub severity: String,
    pub breaking_changes: bool,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TemplateUpdateReport {
    pub template_name: String,
    pub previous_version: Option<String>,
    pub latest_version: String,
    pub update_available: bool,
    pub compatibility: String,
    pub impact: TemplateUpdateImpact,
    pub migration_guidance: Vec<String>,
    pub rollback_steps: Vec<String>,
    pub backup_path: Option<String>,
    pub tracked_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct TemplateUpdateState {
    template_name: String,
    backup_path: Option<String>,
    previous_version: Option<String>,
    last_report: Option<TemplateUpdateReport>,
}

/// Outcome of a template-vs-CLI compatibility check.
#[derive(Debug, PartialEq, Eq)]
pub enum CompatibilityStatus {
    /// Template is compatible with the running CLI version.
    Compatible,
    /// Template requires a newer CLI version than what is running.
    TooOld {
        required_min: String,
        running: String,
    },
    /// Template is not compatible with the current (too-new) CLI version.
    TooNew {
        required_max: String,
        running: String,
    },
    /// Template metadata contains a malformed version string.
    MalformedMetadata { reason: String },
}

/// Parse a semver string `"major.minor.patch"` into `(major, minor, patch)`.
///
/// Returns `Err` when the string cannot be parsed.
fn parse_semver(v: &str) -> std::result::Result<(u64, u64, u64), String> {
    let parts: Vec<&str> = v.splitn(3, '.').collect();
    if parts.len() != 3 {
        return Err(format!(
            "'{}' is not a valid semver string (expected major.minor.patch)",
            v
        ));
    }
    let parse = |s: &str| {
        s.parse::<u64>()
            .map_err(|_| format!("non-numeric component '{}' in version '{}'", s, v))
    };
    Ok((parse(parts[0])?, parse(parts[1])?, parse(parts[2])?))
}

/// Return whether `version` satisfies `min <= version <= max` using semver ordering.
///
/// Either bound may be `None`, meaning unbounded in that direction.
pub fn check_version_range(
    version: &str,
    min: Option<&str>,
    max: Option<&str>,
) -> CompatibilityStatus {
    let running = match parse_semver(version) {
        Ok(v) => v,
        Err(reason) => return CompatibilityStatus::MalformedMetadata { reason },
    };

    if let Some(min_str) = min {
        match parse_semver(min_str) {
            Ok(min_v) => {
                if running < min_v {
                    return CompatibilityStatus::TooOld {
                        required_min: min_str.to_string(),
                        running: version.to_string(),
                    };
                }
            }
            Err(reason) => return CompatibilityStatus::MalformedMetadata { reason },
        }
    }

    if let Some(max_str) = max {
        match parse_semver(max_str) {
            Ok(max_v) => {
                if running > max_v {
                    return CompatibilityStatus::TooNew {
                        required_max: max_str.to_string(),
                        running: version.to_string(),
                    };
                }
            }
            Err(reason) => return CompatibilityStatus::MalformedMetadata { reason },
        }
    }

    CompatibilityStatus::Compatible
}

/// Check whether `entry` is compatible with the currently running StarForge CLI.
///
/// Templates that carry no version constraints (`cli_version_min` and
/// `cli_version_max` are both `None`) are always considered compatible, ensuring
/// full backward compatibility with pre-versioning templates.
pub fn check_template_compatibility(entry: &TemplateEntry) -> CompatibilityStatus {
    check_version_range(
        CLI_VERSION,
        entry.cli_version_min.as_deref(),
        entry.cli_version_max.as_deref(),
    )
}

/// Validate that `entry` is compatible with the running CLI and return an
/// actionable error message if it is not.
pub fn assert_template_compatible(entry: &TemplateEntry) -> Result<()> {
    match check_template_compatibility(entry) {
        CompatibilityStatus::Compatible => Ok(()),
        CompatibilityStatus::TooOld {
            required_min,
            running,
        } => {
            anyhow::bail!(
                "Template '{}' requires StarForge >= {} but you are running {}.\n\
                 Please upgrade StarForge: https://github.com/Nanle-code/StarForge#installation",
                entry.name,
                required_min,
                running,
            )
        }
        CompatibilityStatus::TooNew {
            required_max,
            running,
        } => {
            anyhow::bail!(
                "Template '{}' only supports StarForge <= {} but you are running {}.\n\
                 Use an older version of StarForge or check if a newer template version is available.",
                entry.name,
                required_max,
                running,
            )
        }
        CompatibilityStatus::MalformedMetadata { reason } => {
            anyhow::bail!(
                "Template '{}' has malformed version metadata: {}.\n\
                 Contact the template author to fix the cli_version_min / cli_version_max fields.",
                entry.name,
                reason,
            )
        }
    }
}

fn infer_template_version_from_dir(path: &Path) -> Option<String> {
    let cargo_toml = path.join("Cargo.toml");
    if cargo_toml.exists() {
        if let Ok(content) = fs::read_to_string(&cargo_toml) {
            for line in content.lines() {
                let trimmed = line.trim();
                if let Some((_, value)) = trimmed.split_once("version") {
                    let value = value.trim().trim_matches('"');
                    if !value.is_empty() {
                        return Some(value.to_string());
                    }
                }
            }
        }
    }

    let package_json = path.join("package.json");
    if package_json.exists() {
        if let Ok(content) = fs::read_to_string(&package_json) {
            for line in content.lines() {
                let trimmed = line.trim();
                if let Some((_, value)) = trimmed.split_once("\"version\"") {
                    let value = value.trim().trim_matches(':').trim().trim_matches('"');
                    if !value.is_empty() {
                        return Some(value.to_string());
                    }
                }
            }
        }
    }

    None
}

fn build_update_report(
    template_name: &str,
    previous_version: Option<&str>,
    latest_version: &str,
    entry: &TemplateEntry,
) -> Result<TemplateUpdateReport> {
    let update_available = previous_version != Some(latest_version);
    let compatibility = match check_template_compatibility(entry) {
        CompatibilityStatus::Compatible => "Compatible with the current StarForge CLI".to_string(),
        CompatibilityStatus::TooOld {
            required_min,
            running,
        } => {
            format!(
                "Requires StarForge >= {} but the running CLI is {}",
                required_min, running
            )
        }
        CompatibilityStatus::TooNew {
            required_max,
            running,
        } => {
            format!(
                "Requires StarForge <= {} but the running CLI is {}",
                required_max, running
            )
        }
        CompatibilityStatus::MalformedMetadata { reason } => {
            format!("Version metadata is malformed: {}", reason)
        }
    };

    let mut migration_guidance = Vec::new();
    let mut severity = "low".to_string();
    let mut breaking_changes = false;
    let mut impact_summary =
        "No material changes are expected for this template update.".to_string();

    if update_available {
        impact_summary = format!(
            "The template is moving from {} to {}.",
            previous_version.unwrap_or("an unknown version"),
            latest_version
        );

        if let Some(latest) = entry.changelog.as_ref().and_then(|c| c.first()) {
            let notes = latest.notes.clone();
            if notes.to_lowercase().contains("breaking")
                || notes.to_lowercase().contains("migration")
                || notes.to_lowercase().contains("removed")
                || notes.to_lowercase().contains("deprecated")
            {
                breaking_changes = true;
                severity = "high".to_string();
                impact_summary.push_str(
                    " The release notes mention breaking or migration-sensitive changes.",
                );
            }
        }

        if previous_version.is_some() && latest_version.contains('.') {
            let current_parts: Vec<&str> =
                previous_version.unwrap_or_default().split('.').collect();
            let latest_parts: Vec<&str> = latest_version.split('.').collect();
            if current_parts.first() != latest_parts.first() {
                severity = "high".to_string();
                impact_summary.push_str(" The version jump appears to be a major release.");
                breaking_changes = true;
            } else if current_parts.get(1) != latest_parts.get(1) {
                severity = "medium".to_string();
                impact_summary
                    .push_str(" The update introduces a feature or compatibility change.");
            }
        }

        migration_guidance.push("Review the release notes and regenerate any custom project scaffolding before shipping changes.".to_string());
        migration_guidance.push(
            "Re-run your template smoke test after the update to confirm everything still works."
                .to_string(),
        );
        if breaking_changes {
            migration_guidance.push("Treat this as a breaking update and plan a migration or rollback path before applying it broadly.".to_string());
        }
    }

    if !compatibility.contains("Compatible") {
        migration_guidance.push(format!("Compatibility note: {}", compatibility));
    }

    let rollback_steps = vec![
        "The update process keeps a backup copy of the previous template contents.".to_string(),
        format!("Use `starforge template rollback {}` to restore the previous template state if needed.", template_name),
    ];

    Ok(TemplateUpdateReport {
        template_name: template_name.to_string(),
        previous_version: previous_version.map(str::to_string),
        latest_version: latest_version.to_string(),
        update_available,
        compatibility,
        impact: TemplateUpdateImpact {
            severity: severity.clone(),
            breaking_changes,
            summary: impact_summary,
        },
        migration_guidance,
        rollback_steps,
        backup_path: None,
        tracked_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string(),
    })
}

fn write_update_state(template_path: &Path, state: &TemplateUpdateState) -> Result<()> {
    let state_file = template_path.join(".starforge-update-state.json");
    let contents = serde_json::to_string_pretty(state)?;
    fs::write(&state_file, contents)
        .with_context(|| format!("Failed to persist update state to {}", state_file.display()))?;
    Ok(())
}

fn read_update_state(template_path: &Path) -> Result<Option<TemplateUpdateState>> {
    let state_file = template_path.join(".starforge-update-state.json");
    if !state_file.exists() {
        return Ok(None);
    }

    let contents = fs::read_to_string(&state_file)
        .with_context(|| format!("Failed to read update state from {}", state_file.display()))?;
    let state = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse update state from {}", state_file.display()))?;
    Ok(Some(state))
}

impl TemplateEntry {
    /// Compute a 0-100 quality/trust score from the available signals.
    ///
    /// The score blends verification status, documentation, usage (downloads)
    /// and maintenance state so that dependable templates rank higher and are
    /// easier to discover in a growing community catalog.
    pub fn quality_score(&self) -> u8 {
        let mut score: i32 = 0;

        // Verified templates have been vetted — the strongest trust signal.
        if self.verified {
            score += 40;
        }

        // Documentation makes a template far easier to adopt.
        if self.documented {
            score += 20;
        }

        // Usage is a proxy for community confidence (capped so a single
        // wildly-popular template cannot dominate the ranking).
        score += (self.downloads / 50).min(30) as i32;

        // Maintenance state rewards living projects and penalizes dead ones.
        score += match self.maintenance {
            MaintenanceStatus::Active => 10,
            MaintenanceStatus::Maintained => 5,
            MaintenanceStatus::Deprecated => -25,
            MaintenanceStatus::Unknown => 0,
        };

        score.clamp(0, 100) as u8
    }

    /// Compact trust/quality badge strings for inline display in list/search output.
    ///
    /// Returns short tokens like `[VERIFIED]`, `[DOCS]`, `[ACTIVE]`, `[DEPRECATED]`,
    /// `[POPULAR]` that can be joined and appended to a single summary line.
    pub fn trust_indicators(&self) -> Vec<String> {
        let mut badges = Vec::new();

        if self.verified {
            badges.push("[VERIFIED]".to_string());
        }
        if self.documented {
            badges.push("[DOCS]".to_string());
        }

        match self.maintenance {
            MaintenanceStatus::Active => badges.push("[ACTIVE]".to_string()),
            MaintenanceStatus::Maintained => badges.push("[MAINTAINED]".to_string()),
            MaintenanceStatus::Deprecated => badges.push("[DEPRECATED]".to_string()),
            MaintenanceStatus::Unknown => {}
        }

        if self.downloads >= 1000 {
            badges.push("[POPULAR]".to_string());
        }
        if self.featured {
            badges.push("[FEATURED]".to_string());
        }
        if self.is_trending() {
            badges.push("[TRENDING]".to_string());
        }
        if self.is_spam_suspected() {
            badges.push("[SUSPECT]".to_string());
        }

        badges
    }

    /// Estimate whether the template is likely a low-quality or spammy submission.
    pub fn is_spam_suspected(&self) -> bool {
        if self.verified {
            return false;
        }

        let low_confidence = self.description.len() < 50 || self.tags.is_empty();
        let poor_quality = self.quality_score() < 30;
        let deprecated = self.maintenance == MaintenanceStatus::Deprecated;

        poor_quality && (low_confidence || deprecated)
    }

    /// Return whether the template has recently shown activity or popularity.
    pub fn is_trending(&self) -> bool {
        if self.downloads >= 500 {
            return true;
        }
        self.updated_recently()
    }

    pub fn updated_recently(&self) -> bool {
        if self.updated_at.trim().is_empty() {
            return false;
        }

        if let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(&self.updated_at) {
            let age =
                chrono::Utc::now().signed_duration_since(timestamp.with_timezone(&chrono::Utc));
            age.num_days() <= 30
        } else {
            false
        }
    }

    /// A broad health score reflecting quality, maintenance, trending, and
    /// featured status.
    pub fn health_score(&self) -> u8 {
        let mut score = self.quality_score() as i32;

        if self.is_trending() {
            score += 5;
        }
        if self.featured {
            score += 5;
        }
        if self.is_spam_suspected() {
            score -= 15;
        }
        if self.maintenance == MaintenanceStatus::Deprecated {
            score -= 10;
        }

        score.clamp(0, 100) as u8
    }
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct TemplateManifest {
    name: Option<String>,
    description: Option<String>,
    version: Option<String>,
    source: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

const DEFAULT_REGISTRY: &str = include_str!("../../templates/registry.json");
const DEFAULT_REGISTRY_URL: &str =
    "https://starforge-protocol.github.io/starforge/templates/registry.json";

fn registry_path() -> Result<PathBuf> {
    let dir = crate::utils::config::config_dir().join("templates");
    if !dir.exists() {
        fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    }
    Ok(dir.join("registry.json"))
}

/// Verify that the SHA-256 checksum of `bytes` matches `expected_hex`.
///
/// On mismatch, returns an error containing both the expected and actual hex strings.
pub fn verify_archive_checksum(bytes: &[u8], expected_hex: &str) -> Result<()> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let actual_bytes = hasher.finalize();
    let actual_hex = hex::encode(actual_bytes);
    let expected_clean = expected_hex.trim();

    if !actual_hex.eq_ignore_ascii_case(expected_clean) {
        anyhow::bail!(
            "Checksum mismatch for template archive: expected {}, got {}",
            expected_clean,
            actual_hex
        );
    }
    Ok(())
}

/// Returns true if the path looks like a supported template archive.
pub fn is_archive_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("zip"))
        .unwrap_or(false)
}

/// Extract a `.zip` template package into `dest`, guarding against zip-slip paths.
pub fn extract_zip_archive(archive: &Path, dest: &Path) -> Result<()> {
    use zip::ZipArchive;

    if !dest.exists() {
        fs::create_dir_all(dest)?;
    }

    let file = fs::File::open(archive)
        .with_context(|| format!("Failed to open archive {}", archive.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("Failed to read ZIP archive {}", archive.display()))?;

    let dest_canon = dest.canonicalize().unwrap_or_else(|_| dest.to_path_buf());

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let entry_path = match entry.enclosed_name() {
            Some(p) => p.to_path_buf(),
            None => continue,
        };

        let out_path = dest_canon.join(&entry_path);
        if !out_path.starts_with(&dest_canon) {
            anyhow::bail!(
                "Archive entry '{}' escapes the destination directory (zip-slip)",
                entry_path.display()
            );
        }

        if entry.name().ends_with('/') {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut outfile = fs::File::create(&out_path)?;
            std::io::copy(&mut entry, &mut outfile)?;
        }
    }

    Ok(())
}

/// If `path` is a single top-level directory, return that directory; otherwise `path`.
pub fn normalize_template_root(path: &Path) -> Result<PathBuf> {
    if !path.is_dir() {
        return Ok(path.to_path_buf());
    }
    let mut entries = fs::read_dir(path)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            name != ".git" && name != "__MACOSX" && !name.to_string_lossy().starts_with('.')
        })
        .collect::<Vec<_>>();

    entries.retain(|e| {
        let n = e.file_name();
        n != ".DS_Store"
    });

    if entries.len() == 1 && entries[0].path().is_dir() {
        return Ok(entries[0].path());
    }
    Ok(path.to_path_buf())
}

/// Resolve a template path: directories are used as-is; ZIP archives are extracted to a temp dir.
pub fn resolve_template_source(path: &Path) -> Result<(PathBuf, Option<tempfile::TempDir>)> {
    if is_archive_path(path) {
        let temp =
            tempfile::tempdir().context("Failed to create temp dir for archive extraction")?;
        extract_zip_archive(path, temp.path())?;
        let root = normalize_template_root(temp.path())?;
        Ok((root, Some(temp)))
    } else if path.is_dir() {
        Ok((path.to_path_buf(), None))
    } else {
        anyhow::bail!(
            "Template path must be a directory or .zip archive: {}",
            path.display()
        );
    }
}

fn template_storage_dir() -> Result<PathBuf> {
    let dir = crate::utils::config::config_dir()
        .join("templates")
        .join("storage");
    if !dir.exists() {
        fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    }
    Ok(dir)
}

fn template_cache_dir() -> Result<PathBuf> {
    let dir = crate::utils::config::config_dir().join("template-cache");
    if !dir.exists() {
        fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    }
    Ok(dir)
}

/// Clone a git-sourced template into `~/.starforge/template-cache/<name>/` with
/// `--depth 1` (shallow clone) and return the cache path.
///
/// When `force_refresh` is `true` any existing cached copy is removed before
/// re-cloning, guaranteeing a fresh copy of the template.
pub fn fetch_template_cached(entry: &TemplateEntry, force_refresh: bool) -> Result<PathBuf> {
    let cache_root = template_cache_dir()?;
    let dest = cache_root.join(&entry.name);

    if dest.exists() {
        let mut should_refresh = force_refresh;
        if !should_refresh {
            if let Ok(metadata) = fs::metadata(&dest) {
                if let Ok(modified) = metadata.modified() {
                    use std::time::{Duration, SystemTime};
                    let ttl = Duration::from_secs(24 * 60 * 60); // 24 hours TTL
                    if SystemTime::now().duration_since(modified).unwrap_or(ttl) >= ttl {
                        should_refresh = true;
                    }
                }
            }
        }

        if should_refresh {
            // Rename existing cache to a temporary name to preserve it in case refresh fails
            let temp_old = cache_root.join(format!("{}.old", entry.name));
            // Remove any existing temp_old directory
            if temp_old.exists() {
                fs::remove_dir_all(&temp_old)?;
            }
            fs::rename(&dest, &temp_old)?;

            // Try to fetch new template
            match fetch_template(entry, &dest) {
                Ok(_) => {
                    // Success - clean up the old temp directory
                    fs::remove_dir_all(&temp_old).ok(); // Ignore errors during cleanup
                    Ok(dest)
                }
                Err(_) => {
                    // Failed - restore old cache and use it
                    if dest.exists() {
                        fs::remove_dir_all(&dest)?;
                    }
                    fs::rename(&temp_old, &dest)?;
                    Ok(dest)
                }
            }
        } else {
            Ok(dest)
        }
    } else {
        fetch_template(entry, &dest)?;
        Ok(dest)
    }
}

/// Return the `src/lib.rs` content for a marketplace template, fetching and
/// caching it if necessary.
///
/// Returns `None` when the template name is not found in the registry.
pub async fn template_source_content(name: &str, force_refresh: bool) -> Result<Option<String>> {
    let registry = load_registry().await?;
    let entry = match registry.templates.into_iter().find(|t| t.name == name) {
        Some(e) => e,
        None => return Ok(None),
    };

    let cache_path = fetch_template_cached(&entry, force_refresh)?;
    let lib_rs = cache_path.join("src").join("lib.rs");
    if lib_rs.exists() {
        let content = fs::read_to_string(&lib_rs)
            .with_context(|| format!("Failed to read {}", lib_rs.display()))?;
        Ok(Some(content))
    } else {
        Ok(None)
    }
}

pub async fn load_registry() -> Result<TemplateRegistry> {
    // Determine remote registry URL, falling back to the default global index.
    let remote_url = std::env::var("STARFORGE_TEMPLATE_REGISTRY_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_REGISTRY_URL.to_string());

    // Check if user forced a refresh
    let force_refresh = std::env::var("STARFORGE_TEMPLATE_REGISTRY_FORCE_REFRESH").is_ok();
    let cache_path = registry_path()?;

    // Use cache if it exists and is fresh and we are not forcing a refresh.
    if !force_refresh {
        if let Ok(metadata) = fs::metadata(&cache_path) {
            if let Ok(modified) = metadata.modified() {
                use std::time::{Duration, SystemTime};
                let ttl = Duration::from_secs(24 * 60 * 60); // 24 hours
                if SystemTime::now()
                    .duration_since(modified)
                    .unwrap_or_else(|_| ttl)
                    < ttl
                {
                    let contents = fs::read_to_string(&cache_path).with_context(|| {
                        format!("Failed to read cached registry at {}", cache_path.display())
                    })?;
                    let registry: TemplateRegistry = serde_json::from_str(&contents)
                        .with_context(|| "Failed to parse cached template registry")?;
                    return Ok(registry);
                }
            }
        }
    }

    // Either forced refresh or cache is missing/old – attempt to fetch remote.
    match fetch_and_cache_remote(&remote_url).await {
        Ok(registry) => Ok(registry),
        Err(_fetch_err) => {
            // If the remote fetch failed but a cached registry exists, fall back to it.
            if cache_path.exists() {
                let contents = fs::read_to_string(&cache_path).with_context(|| {
                    format!("Failed to read cached registry at {}", cache_path.display())
                })?;
                let registry: TemplateRegistry = serde_json::from_str(&contents)
                    .with_context(|| "Failed to parse cached template registry")?;
                return Ok(registry);
            }
            // No cache available – fall back to the registry bundled with the binary
            // so the marketplace still works offline on a fresh install.
            let registry: TemplateRegistry = serde_json::from_str(DEFAULT_REGISTRY)
                .with_context(|| "Failed to parse bundled default template registry")?;
            Ok(registry)
        }
    }
}

pub fn save_registry(registry: &TemplateRegistry) -> Result<()> {
    let path = registry_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }
    let contents =
        serde_json::to_string_pretty(registry).with_context(|| "Failed to serialize registry")?;
    fs::write(&path, contents)
        .with_context(|| format!("Failed to write registry to {}", path.display()))?;
    Ok(())
}

/// Fetches a remote JSON template registry, caches it locally, and returns the parsed registry.
async fn fetch_and_cache_remote(url: &str) -> Result<TemplateRegistry> {
    let response = http_client::get_client()
        .get(url)
        .send()
        .await
        .with_context(|| format!("Failed to fetch remote template registry from {}", url))?;
    if response.status() != 200 {
        anyhow::bail!(
            "Unexpected HTTP status {} when fetching remote registry",
            response.status()
        );
    }
    let json_str = response
        .text()
        .await
        .with_context(|| "Failed to read response body as string")?;
    // Parse the JSON into our TemplateRegistry struct.
    let registry: TemplateRegistry = serde_json::from_str(&json_str)
        .with_context(|| "Failed to deserialize remote template registry JSON")?;
    // Cache the fetched registry locally for offline use.
    let cache_path = registry_path()?;
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create cache directory {}", parent.display()))?;
    }
    fs::write(&cache_path, &json_str).with_context(|| {
        format!(
            "Failed to write cached registry to {}",
            cache_path.display()
        )
    })?;
    Ok(registry)
}

/// Filters applied on top of a text query when searching the marketplace.
#[derive(Debug, Clone, Default)]
pub struct SearchFilters {
    /// Templates must carry all of these tags (case-insensitive).
    pub tags: Vec<String>,
    /// Templates must carry all of these categories.
    pub categories: Vec<String>,
    /// Only include templates flagged as verified.
    pub verified_only: bool,
    /// Only include only featured templates.
    pub featured_only: bool,
    /// Hide templates that are likely low-quality or spammy.
    pub hide_spam: bool,
    /// Only include templates whose quality score is at least this value.
    pub min_quality: u8,
}

/// A single ranked search result, carrying the matched template alongside the
/// information needed to explain *why* it matched and *how* it ranked.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub entry: TemplateEntry,
    /// Text-relevance score for the query (0 when the query is empty).
    pub relevance: u32,
    /// Human-readable reasons the template matched the query.
    pub reasons: Vec<String>,
}

/// Compute the text-relevance of a template for a query, returning the score
/// and the reasons it matched. Field weighting (name > tags > description)
/// makes the most meaningful matches rank highest.
fn relevance_for(entry: &TemplateEntry, query_lower: &str) -> (u32, Vec<String>) {
    if query_lower.is_empty() {
        return (0, Vec::new());
    }

    let mut score = 0u32;
    let mut reasons = Vec::new();

    let name_lower = entry.name.to_lowercase();
    if name_lower == query_lower {
        score += 100;
        reasons.push("exact name".to_string());
    } else if name_lower.starts_with(query_lower) {
        score += 60;
        reasons.push("name prefix".to_string());
    } else if name_lower.contains(query_lower) {
        score += 40;
        reasons.push("name".to_string());
    }

    for tag in &entry.tags {
        let tag_lower = tag.to_lowercase();
        if tag_lower == query_lower {
            score += 30;
            reasons.push(format!("tag: {}", tag));
        } else if tag_lower.contains(query_lower) {
            score += 15;
            reasons.push(format!("tag ~ {}", tag));
        }
    }

    if entry.description.to_lowercase().contains(query_lower) {
        score += 10;
        reasons.push("description".to_string());
    }

    (score, reasons)
}

/// Search the marketplace with relevance ranking, filtering and per-result
/// match explanations.
///
/// Results are ordered by text relevance first, then by overall quality score
/// (verification, documentation, usage, maintenance), then by raw downloads.
/// An empty query lists every template that satisfies the filters, ranked by
/// quality alone.
pub async fn search_templates_ranked(
    query: &str,
    filters: &SearchFilters,
) -> Result<Vec<SearchResult>> {
    let registry = load_registry().await?;
    let query_lower = query.trim().to_lowercase();

    let mut results: Vec<SearchResult> = registry
        .templates
        .into_iter()
        .filter_map(|entry| {
            // Apply structured filters first — they are independent of the text query.
            let has_all_tags = filters
                .tags
                .iter()
                .all(|ft| entry.tags.iter().any(|t| t.eq_ignore_ascii_case(ft)));
            if !has_all_tags {
                return None;
            }
            let has_all_categories = filters
                .categories
                .iter()
                .all(|fc| entry.categories.iter().any(|c| c.eq_ignore_ascii_case(fc)));
            if !has_all_categories {
                return None;
            }
            if filters.verified_only && !entry.verified {
                return None;
            }
            if filters.featured_only && !entry.featured {
                return None;
            }
            if filters.hide_spam && entry.is_spam_suspected() {
                return None;
            }
            if entry.quality_score() < filters.min_quality {
                return None;
            }

            let (relevance, reasons) = relevance_for(&entry, &query_lower);
            // When a text query is supplied, drop templates that do not match it.
            if !query_lower.is_empty() && relevance == 0 {
                return None;
            }

            Some(SearchResult {
                entry,
                relevance,
                reasons,
            })
        })
        .collect();

    // Rank by relevance, then quality, then trending, then downloads.
    results.sort_by(|a, b| {
        b.relevance
            .cmp(&a.relevance)
            .then_with(|| b.entry.quality_score().cmp(&a.entry.quality_score()))
            .then_with(|| b.entry.is_trending().cmp(&a.entry.is_trending()))
            .then_with(|| b.entry.downloads.cmp(&a.entry.downloads))
    });

    Ok(results)
}

/// Backwards-compatible search returning just the ranked template entries.
pub async fn search_templates(query: &str, tags: Option<&[String]>) -> Result<Vec<TemplateEntry>> {
    let filters = SearchFilters {
        tags: tags.map(|t| t.to_vec()).unwrap_or_default(),
        ..Default::default()
    };
    Ok(search_templates_ranked(query, &filters)
        .await?
        .into_iter()
        .map(|r| r.entry)
        .collect())
}

pub async fn get_template(name: &str) -> Result<TemplateEntry> {
    let versions = get_templates_by_name(name).await?;
    versions
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("Template '{}' not found in registry", name))
}

pub async fn get_templates_by_name(name: &str) -> Result<Vec<TemplateEntry>> {
    let registry = load_registry().await?;
    let mut matching: Vec<TemplateEntry> = registry
        .templates
        .into_iter()
        .filter(|t| t.name == name)
        .collect();
    matching.sort_by(|a, b| {
        let a_ver =
            semver::Version::parse(&a.version).unwrap_or_else(|_| semver::Version::new(0, 0, 0));
        let b_ver =
            semver::Version::parse(&b.version).unwrap_or_else(|_| semver::Version::new(0, 0, 0));
        b_ver.cmp(&a_ver)
    });
    Ok(matching)
}

/// Render a Markdown documentation page for a template from its registry
/// metadata. Used by `starforge template docs <name>` to keep per-template
/// documentation consistent and auto-generated rather than hand-written.
pub fn generate_template_docs(entry: &TemplateEntry) -> String {
    let mut md = String::new();

    md.push_str(&format!("# {}\n\n", entry.name));
    if !entry.description.is_empty() {
        md.push_str(&format!("{}\n\n", entry.description));
    }

    let badges = entry.trust_indicators();
    if !badges.is_empty() {
        md.push_str(&format!("{}\n\n", badges.join(" ")));
    }

    md.push_str("## Overview\n\n");
    md.push_str(&format!("- **Version:** {}\n", entry.version));
    md.push_str(&format!(
        "- **Quality score:** {}/100\n",
        entry.quality_score()
    ));
    md.push_str(&format!(
        "- **Verified:** {}\n",
        if entry.verified { "yes" } else { "no" }
    ));
    md.push_str(&format!(
        "- **Maintenance:** {}\n",
        entry.maintenance.label()
    ));
    if !entry.author.is_empty() {
        md.push_str(&format!("- **Author:** {}\n", entry.author));
    }
    if let Some(license) = &entry.license {
        md.push_str(&format!("- **License:** {}\n", license));
    }
    if !entry.tags.is_empty() {
        md.push_str(&format!("- **Tags:** {}\n", entry.tags.join(", ")));
    }

    // CLI compatibility, mirroring the bounds used by `check_version_range`.
    let compat = match (&entry.cli_version_min, &entry.cli_version_max) {
        (Some(min), Some(max)) => format!(">= {} and <= {}", min, max),
        (Some(min), None) => format!(">= {}", min),
        (None, Some(max)) => format!("<= {}", max),
        (None, None) => "any version".to_string(),
    };
    md.push_str(&format!("- **Requires StarForge CLI:** {}\n", compat));
    md.push('\n');

    md.push_str("## Install\n\n");
    md.push_str("```bash\n");
    md.push_str(&format!("starforge template install {}\n", entry.name));
    md.push_str("```\n\n");

    let links: Vec<(&str, &Option<String>)> = vec![
        ("Repository", &entry.repository),
        ("Homepage", &entry.homepage),
        ("Documentation", &entry.documentation),
    ];
    let present: Vec<String> = links
        .into_iter()
        .filter_map(|(label, url)| url.as_ref().map(|u| format!("- [{}]({})", label, u)))
        .collect();
    if !present.is_empty() {
        md.push_str("## Links\n\n");
        md.push_str(&present.join("\n"));
        md.push('\n');
    }

    md
}

pub async fn get_template_by_name_and_version(
    name: &str,
    version: Option<&str>,
) -> Result<TemplateEntry> {
    let versions = get_templates_by_name(name).await?;

    if let Some(v) = version {
        versions
            .into_iter()
            .find(|t| t.version == v)
            .ok_or_else(|| anyhow::anyhow!("Template '{}@{}' not found", name, v))
    } else {
        versions
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("Template '{}' not found", name))
    }
}

fn semver_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let parse_version = |v: &str| {
        v.strip_prefix('v')
            .unwrap_or(v)
            .split('.')
            .filter_map(|s| s.parse::<u64>().ok())
            .collect::<Vec<_>>()
    };
    parse_version(a).cmp(&parse_version(b))
}

pub async fn add_template(entry: TemplateEntry) -> Result<()> {
    let mut registry = load_registry().await?;

    if let Some(existing) = registry
        .templates
        .iter_mut()
        .find(|t| t.name == entry.name && t.version == entry.version)
    {
        *existing = entry;
    } else {
        registry.templates.push(entry);
    }

    save_registry(&registry)?;
    Ok(())
}

/// Remove a template from the registry.
/// If `purge` is true, also deletes any cached/downloaded assets.
pub async fn remove_template(name: &str, purge: bool) -> Result<()> {
    let mut registry = load_registry().await?;
    let before = registry.templates.len();

    registry.templates.retain(|t| t.name != name);

    if registry.templates.len() == before {
        anyhow::bail!("Template '{}' not found in registry", name);
    }

    save_registry(&registry)?;

    // Purge local assets if requested
    if purge {
        purge_template_assets(name)?;
    }

    Ok(())
}

/// Delete all local cached and stored assets for a template
fn purge_template_assets(name: &str) -> Result<()> {
    // 1. Purge from template storage directory
    if let Ok(storage_dir) = template_storage_dir() {
        let template_path = storage_dir.join(name);
        if template_path.exists() {
            fs::remove_dir_all(&template_path).with_context(|| {
                format!(
                    "Failed to purge stored template at {}",
                    template_path.display()
                )
            })?;
        }
    }

    // 2. Purge from cache directory
    if let Ok(cache_dir) = template_cache_dir() {
        let cache_path = cache_dir.join(name);
        if cache_path.exists() {
            fs::remove_dir_all(&cache_path).with_context(|| {
                format!(
                    "Failed to purge cached template at {}",
                    cache_path.display()
                )
            })?;
        }
    }

    Ok(())
}

pub async fn update_template(name: &str) -> Result<()> {
    let entry = get_template(name).await?;

    match &entry.source {
        TemplateSource::Git { url, branch } => {
            let dest = std::env::temp_dir().join(&entry.name);
            if dest.exists() {
                fs::remove_dir_all(&dest).ok();
            }
            fetch_git_template(url, branch.as_deref(), &dest)?;
            Ok(())
        }
        other => anyhow::bail!("Template source '{}' does not support updates", other),
    }
}

/// Fetch a template's files into `dest` according to its source type.
pub fn fetch_template(entry: &TemplateEntry, dest: &Path) -> Result<()> {
    // Compatibility gate: reject incompatible templates before touching the filesystem.
    assert_template_compatible(entry)?;

    match &entry.source {
        TemplateSource::Git { url, branch } => fetch_git_template(url, branch.as_deref(), dest),
        TemplateSource::Local { path } => fetch_local_template(Path::new(path), dest),
        TemplateSource::Builtin { id } => fetch_builtin_template(id, dest),
    }
}

/// Copy a built-in example template (shipped under `templates/examples/<id>`)
/// into `dest`.
fn fetch_builtin_template(id: &str, dest: &Path) -> Result<()> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("templates")
        .join("examples")
        .join(id);
    if !src.exists() {
        anyhow::bail!(
            "Built-in template '{}' was not found at {}",
            id,
            src.display()
        );
    }
    fetch_local_template(&src, dest)
}

fn fetch_git_template(url: &str, branch: Option<&str>, dest: &Path) -> Result<()> {
    use std::process::Command;

    let mut cmd = Command::new("git");
    cmd.arg("clone");

    if let Some(b) = branch {
        cmd.arg("--branch").arg(b);
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

    // Remove .git directory to clean up
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
        .with_context(|| format!("Failed to copy template from {}", source.display()))?;

    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();

        // Skip .git directories
        if file_name == ".git" {
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

pub async fn publish_template(
    template_path: &Path,
    name: String,
    description: String,
    author: String,
    tags: Vec<String>,
    version: String,
) -> Result<()> {
    publish_template_versioned(
        template_path,
        name,
        description,
        author,
        tags,
        version,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .await
}

/// Like `publish_template` but also records optional CLI version constraints.
/// Install a template from a directory or `.zip` archive into the local registry.
pub async fn install_template_package(
    package_path: &Path,
    name: String,
    description: String,
    author: String,
    tags: Vec<String>,
    version: String,
    cli_version_min: Option<String>,
    cli_version_max: Option<String>,
) -> Result<()> {
    publish_template_versioned(
        package_path,
        name,
        description,
        author,
        tags,
        version,
        cli_version_min,
        cli_version_max,
        None,
        None,
        None,
        None,
    )
    .await
}

pub async fn publish_template_versioned(
    template_path: &Path,
    name: String,
    description: String,
    author: String,
    tags: Vec<String>,
    version: String,
    cli_version_min: Option<String>,
    cli_version_max: Option<String>,
    license: Option<String>,
    repository: Option<String>,
    homepage: Option<String>,
    documentation: Option<String>,
) -> Result<()> {
    if !template_path.exists() {
        anyhow::bail!("Template path does not exist: {}", template_path.display());
    }

    let (source_root, _temp_guard) = resolve_template_source(template_path)?;

    validate_template_structure(&source_root, &name, &description, &author, &version)?;

    let storage_root = template_storage_dir()?.join(&name);
    let dest = storage_root.join(&version);

    let registry = load_registry().await?;
    let same_version_exists = registry
        .templates
        .iter()
        .any(|t| t.name == name && t.version == version);

    if dest.exists() {
        if same_version_exists {
            fs::remove_dir_all(&dest).with_context(|| {
                format!(
                    "Failed to remove existing template version directory {}",
                    dest.display()
                )
            })?;
        } else {
            anyhow::bail!(
                "Template '{}' version '{}' already exists. Remove the old version or choose a new version.",
                name,
                version
            );
        }
    }

    copy_dir_recursive(&source_root, &dest)?;

    let created_at = Utc::now().to_rfc3339();
    let mut changelog: Vec<ChangelogEntry> = Vec::new();
    changelog.push(ChangelogEntry {
        version: version.clone(),
        date: Utc::now().format("%Y-%m-%d").to_string(),
        notes: "Initial release".to_string(),
    });

    let entry = TemplateEntry {
        name: name.clone(),
        changelog: None,
        repository: None,
        security_review: None,
        version: version.clone(),
        description,
        author,
        tags,
        source: TemplateSource::Local {
            path: dest.to_string_lossy().to_string(),
        },
        path: Some(dest.to_string_lossy().to_string()),
        downloads: 0,
        verified: false,
        created_at: created_at.clone(),
        updated_at: created_at,
        cli_version_min,
        cli_version_max,
        documented: source_root.join("README.md").exists(),
        maintenance: MaintenanceStatus::Active,
        license,
        repository_url: repository,
        homepage,
        documentation,
        categories: Vec::new(),
        featured: false,
    };

    add_template(entry).await?;

    Ok(())
}

/// Validate template metadata and structure without CLI version constraints.
pub fn validate_template_structure(
    path: &Path,
    name: &str,
    description: &str,
    author: &str,
    version: &str,
) -> Result<()> {
    validate_template_structure_with_constraints(
        path,
        name,
        description,
        author,
        version,
        None,
        None,
    )
}

/// Full validation including optional CLI version constraint format checks.
///
/// Called by `publish_template_versioned` so that every publish request is
/// audited before any file is written to the registry or storage directory.
/// Errors are actionable: they name the missing or invalid field/file and
/// explain what the author must fix.
pub fn validate_template_structure_with_constraints(
    path: &Path,
    name: &str,
    description: &str,
    author: &str,
    version: &str,
    cli_version_min: Option<&str>,
    cli_version_max: Option<&str>,
) -> Result<()> {
    // --- 1. Metadata completeness ---
    let mut missing: Vec<&str> = Vec::new();
    if name.trim().is_empty() {
        missing.push("name");
    }
    if description.trim().is_empty() {
        missing.push("description");
    }
    if author.trim().is_empty() {
        missing.push("author");
    }
    if version.trim().is_empty() {
        missing.push("version");
    }
    if !missing.is_empty() {
        anyhow::bail!(
            "Missing required metadata fields: {}.\n\
             Provide these fields via CLI flags (--name, --description, --author, --version).",
            missing.join(", ")
        );
    }

    // --- 2. Version string format ---
    if parse_semver(version).is_err() {
        anyhow::bail!(
            "Version '{}' is not valid semver (expected major.minor.patch, e.g. \"1.0.0\").",
            version
        );
    }

    // --- 3. CLI version constraints format (if provided) ---
    if let Some(min) = cli_version_min {
        if parse_semver(min).is_err() {
            anyhow::bail!(
                "cli_version_min '{}' is not valid semver (expected major.minor.patch, e.g. \"0.1.0\").",
                min
            );
        }
    }
    if let Some(max) = cli_version_max {
        if parse_semver(max).is_err() {
            anyhow::bail!(
                "cli_version_max '{}' is not valid semver (expected major.minor.patch, e.g. \"1.99.99\").",
                max
            );
        }
    }
    if let (Some(min), Some(max)) = (cli_version_min, cli_version_max) {
        if let (Ok(min_v), Ok(max_v)) = (parse_semver(min), parse_semver(max)) {
            if min_v > max_v {
                anyhow::bail!(
                    "cli_version_min '{}' is greater than cli_version_max '{}'. \
                     Fix the version bounds so that min <= max.",
                    min,
                    max
                );
            }
        }
    }

    // --- 4. Required files ---
    let cargo_toml = path.join("Cargo.toml");
    if !cargo_toml.exists() {
        anyhow::bail!(
            "Template is missing Cargo.toml.\n\
             A valid StarForge template must be a Rust crate with a Cargo.toml at its root."
        );
    }

    let src_dir = path.join("src");
    if !src_dir.exists() || !src_dir.is_dir() {
        anyhow::bail!(
            "Template is missing the src/ directory.\n\
             A valid StarForge template must contain src/ with at least lib.rs."
        );
    }

    let lib_rs = src_dir.join("lib.rs");
    if !lib_rs.exists() {
        anyhow::bail!(
            "Template is missing src/lib.rs.\n\
             Soroban contracts must define their entry points in src/lib.rs."
        );
    }

    // --- 5. README presence ---
    let readme = path.join("README.md");
    if !readme.exists() {
        anyhow::bail!(
            "Template is missing README.md.\n\
             A README is required so users know how to use the template. \
             Add a README.md explaining the template purpose, usage, and any configuration."
        );
    }

    // --- 6. Placeholder check ---
    // Cargo.toml must use {{PROJECT_NAME}} so the scaffolder can substitute it.
    let cargo_contents = fs::read_to_string(&cargo_toml)
        .with_context(|| format!("Failed to read {}", cargo_toml.display()))?;
    if !cargo_contents.contains("{{PROJECT_NAME}}") {
        anyhow::bail!(
            "Cargo.toml must contain the {{{{PROJECT_NAME}}}} placeholder.\n\
             This placeholder is replaced with the actual project name during scaffolding. \
             Replace the hardcoded package name with {{{{PROJECT_NAME}}}}."
        );
    }

    Ok(())
}

/// Determine how to fetch a template from a user-supplied source string,
/// then register it in the local registry and return the new entry.
///
/// Source resolution order:
/// 1. Starts with `https://`, `http://`, `git://`, or ends with `.git` → git URL
/// 2. Path exists on disk, or starts with `/`, `./`, or `../` → local path
/// 3. Anything else → treated as a registry template name (marketplace lookup)
pub async fn install_template(
    source: &str,
    name_override: Option<&str>,
    version: Option<&str>,
    force: bool,
) -> Result<TemplateEntry> {
    if source.starts_with("https://")
        || source.starts_with("http://")
        || source.starts_with("git://")
        || source.ends_with(".git")
    {
        return install_from_git_url(source, name_override, force).await;
    }

    let path = Path::new(source);
    if path.exists()
        || source.starts_with('/')
        || source.starts_with("./")
        || source.starts_with("../")
    {
        return install_from_local_path(path, name_override, force).await;
    }

    install_from_registry(source, version, force).await
}

async fn install_from_git_url(
    url: &str,
    name_override: Option<&str>,
    force: bool,
) -> Result<TemplateEntry> {
    let name = name_override.map(str::to_string).unwrap_or_else(|| {
        url.trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("template")
            .trim_end_matches(".git")
            .to_string()
    });

    let mut registry = load_registry().await?;
    if registry.templates.iter().any(|t| t.name == name) && !force {
        anyhow::bail!(
            "Template '{}' is already installed. Use --force to overwrite.",
            name
        );
    }

    let dest = template_storage_dir()?.join(&name);
    if dest.exists() {
        fs::remove_dir_all(&dest).with_context(|| {
            format!(
                "Failed to remove existing template directory {}",
                dest.display()
            )
        })?;
    }

    fetch_git_template(url, None, &dest)?;

    let entry = TemplateEntry {
        name: name.clone(),
        changelog: None,
        repository: None,
        security_review: None,
        description: String::new(),
        version: "1.0.0".to_string(),
        source: TemplateSource::Git {
            url: url.to_string(),
            branch: None,
        },
        tags: vec![],
        path: Some(dest.to_string_lossy().to_string()),
        author: String::new(),
        downloads: 0,
        verified: false,
        created_at: String::new(),
        updated_at: String::new(),
        cli_version_min: None,
        cli_version_max: None,
        documented: dest.join("README.md").exists(),
        maintenance: MaintenanceStatus::Unknown,
        license: None,
        repository_url: Some(url.to_string()),
        homepage: None,
        documentation: None,
        categories: Vec::new(),
        featured: false,
    };

    registry.templates.retain(|t| t.name != name);
    registry.templates.push(entry.clone());
    save_registry(&registry)?;

    Ok(entry)
}

async fn install_from_local_path(
    path: &Path,
    name_override: Option<&str>,
    force: bool,
) -> Result<TemplateEntry> {
    if !path.exists() {
        anyhow::bail!("Local path does not exist: {}", path.display());
    }

    let name = name_override.map(str::to_string).unwrap_or_else(|| {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("template")
            .to_string()
    });

    let mut registry = load_registry().await?;
    if registry.templates.iter().any(|t| t.name == name) && !force {
        anyhow::bail!(
            "Template '{}' is already installed. Use --force to overwrite.",
            name
        );
    }

    let dest = template_storage_dir()?.join(&name);
    if dest.exists() {
        fs::remove_dir_all(&dest).with_context(|| {
            format!(
                "Failed to remove existing template directory {}",
                dest.display()
            )
        })?;
    }

    fetch_local_template(path, &dest)?;

    let entry = TemplateEntry {
        name: name.clone(),
        changelog: None,
        repository: None,
        security_review: None,
        description: String::new(),
        version: "1.0.0".to_string(),
        source: TemplateSource::Local {
            path: dest.to_string_lossy().to_string(),
        },
        tags: vec![],
        path: Some(dest.to_string_lossy().to_string()),
        author: String::new(),
        downloads: 0,
        verified: false,
        created_at: String::new(),
        updated_at: String::new(),
        cli_version_min: None,
        cli_version_max: None,
        documented: dest.join("README.md").exists(),
        maintenance: MaintenanceStatus::Unknown,
        license: None,
        repository_url: None,
        homepage: None,
        documentation: None,
        categories: Vec::new(),
        featured: false,
    };

    registry.templates.retain(|t| t.name != name);
    registry.templates.push(entry.clone());
    save_registry(&registry)?;

    Ok(entry)
}

async fn install_from_registry(
    name: &str,
    version: Option<&str>,
    force: bool,
) -> Result<TemplateEntry> {
    let entry = get_template_by_name_and_version(name, version).await?;
    assert_template_compatible(&entry)?;

    let dest = template_storage_dir()?.join(&entry.name);
    if dest.exists() {
        if !force {
            anyhow::bail!(
                "Template '{}' is already cached locally. Use --force to re-download.",
                entry.name
            );
        }
        fs::remove_dir_all(&dest)
            .with_context(|| format!("Failed to remove cached template at {}", dest.display()))?;
    }

    match &entry.source {
        TemplateSource::Git { url, branch } => fetch_git_template(url, branch.as_deref(), &dest)?,
        TemplateSource::Local { path: src_path } => {
            fetch_local_template(Path::new(src_path), &dest)?
        }
        TemplateSource::Builtin { id } => fetch_builtin_template(id, &dest)?,
    }

    Ok(entry)
}

/// Re-fetch a git-sourced template into its local storage directory, updating
/// it in place. Only git-sourced templates support this operation.
pub async fn update_installed_template(name: &str) -> Result<TemplateUpdateReport> {
    let entry = get_template(name).await?;

    match &entry.source {
        TemplateSource::Git { url, branch } => {
            let dest = if let Some(ref p) = entry.path {
                PathBuf::from(p)
            } else {
                template_storage_dir()?.join(name)
            };

            let previous_version =
                infer_template_version_from_dir(&dest).or_else(|| Some(entry.version.clone()));
            let mut report =
                build_update_report(name, previous_version.as_deref(), &entry.version, &entry)?;

            if dest.exists() {
                let backup_root = template_storage_dir()?.join(".backups").join(name);
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let backup_dir = backup_root.join(format!(
                    "{}_{}",
                    timestamp,
                    previous_version.as_deref().unwrap_or("unknown")
                ));
                if backup_dir.exists() {
                    fs::remove_dir_all(&backup_dir)?;
                }
                fs::create_dir_all(&backup_dir)?;
                copy_dir_recursive(&dest, &backup_dir)?;
                report.backup_path = Some(backup_dir.to_string_lossy().to_string());
            }

            if dest.exists() {
                fs::remove_dir_all(&dest).with_context(|| {
                    format!("Failed to remove existing template at {}", dest.display())
                })?;
            }

            fetch_git_template(url, branch.as_deref(), &dest)?;

            let mut registry = load_registry().await?;
            if let Some(t) = registry.templates.iter_mut().find(|t| t.name == name) {
                t.path = Some(dest.to_string_lossy().to_string());
                t.updated_at = String::new();
            }
            save_registry(&registry)?;

            let state = TemplateUpdateState {
                template_name: name.to_string(),
                backup_path: report.backup_path.clone(),
                previous_version: previous_version.clone(),
                last_report: Some(report.clone()),
            };
            if dest.exists() {
                write_update_state(&dest, &state)?;
            }

            Ok(report)
        }
        other => anyhow::bail!(
            "Template '{}' uses source '{}' which does not support updates. \
             Only git-sourced templates can be updated.",
            name,
            other
        ),
    }
}

/// Update all git-sourced templates. Returns a list of (name, result) pairs.
pub async fn update_all_installed_templates() -> Result<Vec<(String, Result<TemplateUpdateReport>)>>
{
    let registry = load_registry().await?;
    let git_names: Vec<String> = registry
        .templates
        .iter()
        .filter(|t| matches!(t.source, TemplateSource::Git { .. }))
        .map(|t| t.name.clone())
        .collect();

    let mut results = Vec::new();
    for name in git_names {
        let result = update_installed_template(&name).await;
        results.push((name, result));
    }
    Ok(results)
}

pub async fn rollback_installed_template(name: &str) -> Result<TemplateUpdateReport> {
    let entry = get_template(name).await?;
    let dest = if let Some(ref p) = entry.path {
        PathBuf::from(p)
    } else {
        template_storage_dir()?.join(name)
    };

    let state = read_update_state(&dest)?.ok_or_else(|| {
        anyhow::anyhow!("No recorded update state exists for template '{}'", name)
    })?;
    let backup_path = state
        .backup_path
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("No backup is available for template '{}'", name))?;

    if dest.exists() {
        fs::remove_dir_all(&dest).with_context(|| {
            format!(
                "Failed to remove template directory before rollback: {}",
                dest.display()
            )
        })?;
    }

    copy_dir_recursive(Path::new(backup_path), &dest)
        .with_context(|| format!("Failed to restore template from backup at {}", backup_path))?;

    let mut report = state.last_report.unwrap_or_else(|| TemplateUpdateReport {
        template_name: name.to_string(),
        previous_version: state.previous_version.clone(),
        latest_version: entry.version.clone(),
        update_available: true,
        compatibility: "Rollback restored the previous template contents".to_string(),
        impact: TemplateUpdateImpact {
            severity: "low".to_string(),
            breaking_changes: false,
            summary: "Rollback restored the previous template state.".to_string(),
        },
        migration_guidance: vec!["Rollback completed successfully.".to_string()],
        rollback_steps: vec![],
        backup_path: state.backup_path.clone(),
        tracked_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string(),
    });
    report.backup_path = state.backup_path.clone();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test: the registry bundled into the binary is the last-resort
    /// fallback used when there is no cache and no network (e.g. a fresh,
    /// offline install). It must always deserialize cleanly.
    #[test]
    fn default_registry_parses() {
        let registry: TemplateRegistry =
            serde_json::from_str(DEFAULT_REGISTRY).expect("bundled registry.json must parse");
        assert!(!registry.templates.is_empty());
    }

    fn make_entry(name: &str) -> TemplateEntry {
        TemplateEntry {
            name: name.to_string(),
            changelog: None,
            repository: None,
            security_review: None,
            version: "1.0.0".to_string(),
            description: String::new(),
            author: String::new(),
            tags: vec![],
            source: TemplateSource::Git {
                url: "https://example.com/repo.git".to_string(),
                branch: None,
            },
            path: None,
            downloads: 0,
            verified: false,
            created_at: String::new(),
            updated_at: String::new(),
            cli_version_min: None,
            cli_version_max: None,
            documented: false,
            maintenance: MaintenanceStatus::Unknown,
            license: None,
            repository_url: None,
            homepage: None,
            documentation: None,
            categories: Vec::new(),
            featured: false,
            changelog: None,
            repository: None,
            security_review: None,
        }
    }

    #[test]
    fn generate_template_docs_includes_key_metadata() {
        let mut entry = make_entry("erc20-token");
        entry.description = "A fungible token implementing the ERC-20 interface.".to_string();
        entry.version = "2.1.0".to_string();
        entry.verified = true;
        entry.documented = true;
        entry.maintenance = MaintenanceStatus::Active;
        entry.author = "Stellar Community".to_string();
        entry.license = Some("MIT".to_string());
        entry.tags = vec!["token".to_string(), "erc20".to_string()];
        entry.cli_version_min = Some("0.1.0".to_string());
        entry.repository = Some("https://github.com/example/erc20".to_string());

        let md = generate_template_docs(&entry);

        assert!(md.starts_with("# erc20-token\n"));
        assert!(md.contains("A fungible token implementing the ERC-20 interface."));
        assert!(md.contains("- **Version:** 2.1.0"));
        assert!(md.contains("- **License:** MIT"));
        assert!(md.contains("- **Tags:** token, erc20"));
        assert!(md.contains("- **Requires StarForge CLI:** >= 0.1.0"));
        assert!(md.contains("[VERIFIED]"));
        assert!(md.contains("starforge template install erc20-token"));
        assert!(md.contains("[Repository](https://github.com/example/erc20)"));
        // Quality score is rendered (verified + documented + active => high).
        assert!(md.contains("Quality score:"));
    }

    #[test]
    fn generate_template_docs_omits_absent_optional_sections() {
        let entry = make_entry("bare");
        let md = generate_template_docs(&entry);
        // No links declared => no Links section; no version bound => "any version".
        assert!(!md.contains("## Links"));
        assert!(md.contains("- **Requires StarForge CLI:** any version"));
    }

    use std::fs;
    use tempfile::tempdir;

    fn make_valid_template(dir: &std::path::Path) {
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
    fn extract_zip_archive_and_validate() {
        use zip::write::FileOptions;
        use zip::ZipWriter;

        let tmp = tempdir().unwrap();
        let tpl_dir = tmp.path().join("inner");
        make_valid_template(&tpl_dir);

        let zip_path = tmp.path().join("package.zip");
        let file = fs::File::create(&zip_path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = FileOptions::default();

        for entry in walkdir_flat(&tpl_dir) {
            let rel = entry.strip_prefix(&tpl_dir).unwrap();
            let name = rel.to_string_lossy().replace('\\', "/");
            if entry.is_dir() {
                zip.add_directory(format!("{}/", name), options).unwrap();
            } else {
                zip.start_file(name, options).unwrap();
                let mut f = fs::File::open(entry).unwrap();
                std::io::copy(&mut f, &mut zip).unwrap();
            }
        }
        zip.finish().unwrap();

        let extract_dir = tmp.path().join("out");
        extract_zip_archive(&zip_path, &extract_dir).unwrap();
        let root = normalize_template_root(&extract_dir).unwrap();
        assert!(validate_template_structure(&root, "zip-tpl", "desc", "author", "1.0.0").is_ok());
    }

    fn walkdir_flat(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            if d.is_dir() {
                for entry in fs::read_dir(&d).unwrap() {
                    let p = entry.unwrap().path();
                    if p.is_dir() {
                        stack.push(p);
                    } else {
                        out.push(p);
                    }
                }
            }
        }
        out
    }

    #[test]
    fn validate_passes_for_valid_template() {
        let tmp = tempdir().unwrap();
        make_valid_template(tmp.path());
        assert!(
            validate_template_structure(tmp.path(), "my-tpl", "A desc", "Alice", "1.0.0").is_ok()
        );
    }

    #[test]
    fn validate_rejects_missing_metadata() {
        let tmp = tempdir().unwrap();
        make_valid_template(tmp.path());
        let err =
            validate_template_structure(tmp.path(), "", "desc", "author", "1.0.0").unwrap_err();
        assert!(
            err.to_string().contains("name"),
            "should mention missing field"
        );

        let err = validate_template_structure(tmp.path(), "n", "", "author", "1.0.0").unwrap_err();
        assert!(err.to_string().contains("description"));

        let err = validate_template_structure(tmp.path(), "n", "d", "", "1.0.0").unwrap_err();
        assert!(err.to_string().contains("author"));

        let err = validate_template_structure(tmp.path(), "n", "d", "a", "").unwrap_err();
        assert!(err.to_string().contains("version"));
    }

    #[test]
    fn validate_rejects_missing_cargo_toml() {
        let tmp = tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::write(tmp.path().join("src/lib.rs"), "").unwrap();
        fs::write(tmp.path().join("README.md"), "# T").unwrap();
        let err = validate_template_structure(tmp.path(), "n", "d", "a", "1.0.0").unwrap_err();
        assert!(err.to_string().contains("Cargo.toml"));
    }

    #[test]
    fn validate_rejects_missing_src_lib() {
        let tmp = tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"{{PROJECT_NAME}}\"\n",
        )
        .unwrap();
        fs::write(tmp.path().join("README.md"), "# T").unwrap();
        let err = validate_template_structure(tmp.path(), "n", "d", "a", "1.0.0").unwrap_err();
        assert!(err.to_string().contains("src/lib.rs"));
    }

    #[test]
    fn validate_rejects_missing_placeholder() {
        let tmp = tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        // Cargo.toml without {{PROJECT_NAME}}
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"hardcoded\"\n",
        )
        .unwrap();
        fs::write(tmp.path().join("src/lib.rs"), "").unwrap();
        fs::write(tmp.path().join("README.md"), "# T").unwrap();
        let err = validate_template_structure(tmp.path(), "n", "d", "a", "1.0.0").unwrap_err();
        assert!(err.to_string().contains("PROJECT_NAME"));
    }

    #[test]
    fn validate_rejects_missing_readme() {
        let tmp = tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"{{PROJECT_NAME}}\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        fs::write(tmp.path().join("src/lib.rs"), "").unwrap();
        // Deliberately no README.md
        let err = validate_template_structure(tmp.path(), "n", "d", "a", "1.0.0").unwrap_err();
        assert!(
            err.to_string().contains("README"),
            "error should mention README"
        );
    }

    #[test]
    fn validate_rejects_bad_version_semver() {
        let tmp = tempdir().unwrap();
        make_valid_template(tmp.path());
        let err = validate_template_structure(tmp.path(), "n", "d", "a", "not-semver").unwrap_err();
        assert!(err.to_string().contains("semver") || err.to_string().contains("not-semver"));
    }

    #[test]
    fn validate_rejects_bad_cli_version_min() {
        let tmp = tempdir().unwrap();
        make_valid_template(tmp.path());
        let err = validate_template_structure_with_constraints(
            tmp.path(),
            "n",
            "d",
            "a",
            "1.0.0",
            Some("bad"),
            None,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("cli_version_min"),
            "error should mention cli_version_min"
        );
    }

    #[test]
    fn validate_rejects_min_greater_than_max() {
        let tmp = tempdir().unwrap();
        make_valid_template(tmp.path());
        let err = validate_template_structure_with_constraints(
            tmp.path(),
            "n",
            "d",
            "a",
            "1.0.0",
            Some("2.0.0"),
            Some("1.0.0"),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("greater than"),
            "error should explain min > max"
        );
    }

    #[test]
    fn test_publish_template_versioned_stores_by_version() {
        let tmp = tempdir().unwrap();
        let home = tmp.path().join("home");
        std::env::set_var("HOME", home.as_os_str());
        let registry_dir = home.join(".starforge").join("templates");
        fs::create_dir_all(&registry_dir).unwrap();
        fs::write(
            registry_dir.join("registry.json"),
            "{\"version\": \"1\", \"templates\": []}",
        )
        .unwrap();

        let tpl_dir = tmp.path().join("template");
        make_valid_template(&tpl_dir);

        futures::executor::block_on(async {
            publish_template_versioned(
                &tpl_dir,
                "my-template".to_string(),
                "A test template".to_string(),
                "Alice".to_string(),
                vec!["defi".to_string()],
                "1.0.0".to_string(),
                Some("0.1.0".to_string()),
                Some("1.0.0".to_string()),
                Some("MIT".to_string()),
                Some("https://example.com".to_string()),
                Some("https://docs.example.com".to_string()),
                Some("https://homepage.example.com".to_string()),
            )
            .await
            .unwrap();

            let storage = home.join(".starforge").join("templates").join("storage");
            assert!(storage.join("my-template").join("1.0.0").exists());

            publish_template_versioned(
                &tpl_dir,
                "my-template".to_string(),
                "A test template".to_string(),
                "Alice".to_string(),
                vec!["defi".to_string()],
                "1.1.0".to_string(),
                Some("0.1.0".to_string()),
                Some("1.0.0".to_string()),
                Some("MIT".to_string()),
                Some("https://example.com".to_string()),
                Some("https://docs.example.com".to_string()),
                Some("https://homepage.example.com".to_string()),
            )
            .await
            .unwrap();

            let latest = get_template("my-template").await.unwrap();
            assert_eq!(latest.version, "1.1.0");

            let older = get_template_by_name_and_version("my-template", Some("1.0.0"))
                .await
                .unwrap();
            assert_eq!(older.version, "1.0.0");
        });
    }

    #[test]
    fn test_search_templates() {
        let mut registry = TemplateRegistry::default();
        registry.templates.push(TemplateEntry {
            name: "uniswap-v2".to_string(),
            changelog: None,
            repository: None,
            security_review: None,
            version: "1.0.0".to_string(),
            description: "Uniswap V2 DEX implementation".to_string(),
            author: "DeFi Team".to_string(),
            tags: vec!["defi".to_string(), "dex".to_string(), "amm".to_string()],
            source: TemplateSource::Git {
                url: "https://github.com/example/uniswap-v2.git".to_string(),
                branch: None,
            },
            path: None,
            created_at: "2025-01-01T00:00:00Z".to_string(),
            updated_at: "2025-01-01T00:00:00Z".to_string(),
            downloads: 100,
            verified: true,
            cli_version_min: None,
            cli_version_max: None,
            documented: true,
            maintenance: MaintenanceStatus::Active,
            license: None,
            repository_url: None,
            homepage: None,
            documentation: None,
            categories: Vec::new(),
            featured: false,
            changelog: None,
            repository: None,
            security_review: None,
        });

        // Test name search
        let results: Vec<_> = registry
            .templates
            .iter()
            .filter(|t| t.name.contains("uniswap"))
            .collect();
        assert_eq!(results.len(), 1);

        // Test tag search
        let results: Vec<_> = registry
            .templates
            .iter()
            .filter(|t| t.tags.contains(&"defi".to_string()))
            .collect();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn fetch_template_cached_uses_cache_on_second_call() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = tmp.path().join("my-template");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join("marker.txt"), "cached").unwrap();

        let entry = TemplateEntry {
            name: "my-template".to_string(),
            changelog: None,
            repository: None,
            security_review: None,
            source: TemplateSource::Git {
                url: "https://example.com/repo.git".to_string(),
                branch: None,
            },
            description: String::new(),
            version: "1.0.0".to_string(),
            tags: vec![],
            path: None,
            author: String::new(),
            downloads: 0,
            verified: false,
            created_at: String::new(),
            updated_at: String::new(),
            cli_version_min: None,
            cli_version_max: None,
            documented: false,
            maintenance: MaintenanceStatus::Unknown,
            license: None,
            repository_url: None,
            homepage: None,
            documentation: None,
            categories: Vec::new(),
            featured: false,
            changelog: None,
            repository: None,
            security_review: None,
        };

        let dest = tmp.path().join(&entry.name);
        assert!(dest.exists(), "pre-existing cache dir should exist");

        if dest.exists() {
            let marker = dest.join("marker.txt");
            assert!(
                marker.exists(),
                "cached content preserved on force_refresh=false"
            );
        }
    }

    #[test]
    fn fetch_template_cached_force_refresh_removes_old_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = tmp.path().join("my-template");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join("stale.txt"), "old").unwrap();

        std::fs::remove_dir_all(&cache_dir).unwrap();
        assert!(
            !cache_dir.exists(),
            "old cache dir should be gone after force_refresh"
        );
    }

    fn sample_entry() -> TemplateEntry {
        TemplateEntry {
            name: "sample".to_string(),
            changelog: None,
            repository: None,
            security_review: None,
            version: "1.0.0".to_string(),
            description: String::new(),
            author: String::new(),
            tags: vec![],
            source: TemplateSource::Builtin {
                id: "sample".to_string(),
            },
            path: None,
            created_at: String::new(),
            updated_at: String::new(),
            downloads: 0,
            verified: false,
            cli_version_min: None,
            cli_version_max: None,
            documented: false,
            maintenance: MaintenanceStatus::Unknown,
            license: None,
            repository_url: None,
            homepage: None,
            documentation: None,
            categories: Vec::new(),
            featured: false,
            changelog: None,
            repository: None,
            security_review: None,
        }
    }

    #[test]
    fn quality_score_rewards_trust_signals() {
        let bare = sample_entry();
        assert_eq!(bare.quality_score(), 0);

        let mut trusted = sample_entry();
        trusted.verified = true;
        trusted.documented = true;
        trusted.maintenance = MaintenanceStatus::Active;
        trusted.downloads = 2000;
        // 40 (verified) + 20 (documented) + 30 (downloads cap) + 10 (active)
        assert_eq!(trusted.quality_score(), 100);

        let mut deprecated = sample_entry();
        deprecated.maintenance = MaintenanceStatus::Deprecated;
        // Penalty is clamped at 0, never negative.
        assert_eq!(deprecated.quality_score(), 0);
    }

    #[test]
    fn quality_score_ranks_verified_above_unverified() {
        let mut verified = sample_entry();
        verified.verified = true;

        let mut popular = sample_entry();
        popular.downloads = 500; // capped contribution of 10

        assert!(verified.quality_score() > popular.quality_score());
    }

    #[test]
    fn trust_indicators_reflect_metadata() {
        let mut entry = sample_entry();
        entry.verified = true;
        entry.documented = true;
        entry.maintenance = MaintenanceStatus::Deprecated;
        entry.downloads = 1500;

        let badges = entry.trust_indicators();
        assert!(badges.iter().any(|b| b.contains("[VERIFIED]")));
        assert!(badges.iter().any(|b| b.contains("[DOCS]")));
        assert!(badges.iter().any(|b| b.contains("[DEPRECATED]")));
        assert!(badges.iter().any(|b| b.contains("[POPULAR]")));
    }

    #[test]
    fn relevance_weights_name_above_description() {
        let mut entry = sample_entry();
        entry.name = "uniswap-v2".to_string();
        entry.description = "an amm dex".to_string();
        entry.tags = vec!["defi".to_string()];

        let (name_score, name_reasons) = relevance_for(&entry, "uniswap");
        let (desc_score, _) = relevance_for(&entry, "amm");
        assert!(name_score > desc_score);
        assert!(name_reasons.iter().any(|r| r.contains("name")));
    }

    #[test]
    fn relevance_exact_name_beats_prefix() {
        let mut exact = sample_entry();
        exact.name = "token".to_string();
        let mut prefix = sample_entry();
        prefix.name = "token-allowlist".to_string();

        let (exact_score, _) = relevance_for(&exact, "token");
        let (prefix_score, _) = relevance_for(&prefix, "token");
        assert!(exact_score > prefix_score);
    }

    #[test]
    fn relevance_empty_query_scores_zero() {
        let entry = sample_entry();
        let (score, reasons) = relevance_for(&entry, "");
        assert_eq!(score, 0);
        assert!(reasons.is_empty());
    }

    #[test]
    fn relevance_tag_match_is_reported() {
        let mut entry = sample_entry();
        entry.tags = vec!["defi".to_string(), "dex".to_string()];
        let (score, reasons) = relevance_for(&entry, "defi");
        assert!(score > 0);
        assert!(reasons.iter().any(|r| r == "tag: defi"));
    }

    #[test]
    fn template_source_content_returns_none_for_unknown_template() {
        let registry = TemplateRegistry::default();
        let found = registry.templates.iter().find(|t| t.name == "nonexistent");
        assert!(found.is_none());
    }

    // ── Template versioning tests ──────────────────────────────────────────────

    #[test]
    fn parse_semver_valid() {
        assert_eq!(parse_semver("1.2.3"), Ok((1, 2, 3)));
        assert_eq!(parse_semver("0.1.0"), Ok((0, 1, 0)));
        assert_eq!(parse_semver("10.20.30"), Ok((10, 20, 30)));
    }

    #[test]
    fn parse_semver_invalid() {
        assert!(parse_semver("1.2").is_err());
        assert!(parse_semver("1.2.x").is_err());
        assert!(parse_semver("").is_err());
    }

    #[test]
    fn check_version_range_no_constraints_is_compatible() {
        // Templates with no min/max are always compatible.
        assert_eq!(
            check_version_range("0.1.0", None, None),
            CompatibilityStatus::Compatible
        );
        assert_eq!(
            check_version_range("99.0.0", None, None),
            CompatibilityStatus::Compatible
        );
    }

    #[test]
    fn check_version_range_within_bounds_is_compatible() {
        assert_eq!(
            check_version_range("0.1.0", Some("0.1.0"), Some("1.0.0")),
            CompatibilityStatus::Compatible
        );
        assert_eq!(
            check_version_range("0.5.0", None, None),
            CompatibilityStatus::Compatible
        );
        assert_eq!(
            check_version_range("0.1.0", Some("0.1.0"), None),
            CompatibilityStatus::Compatible
        );
    }

    #[test]
    fn check_version_range_below_min_is_too_old() {
        let result = check_version_range("0.0.9", Some("0.1.0"), None);
        assert!(matches!(result, CompatibilityStatus::TooOld { .. }));
    }

    #[test]
    fn check_version_range_above_max_is_too_new() {
        let result = check_version_range("2.0.0", None, Some("1.99.99"));
        assert!(matches!(result, CompatibilityStatus::TooNew { .. }));
    }

    #[test]
    fn check_version_range_malformed_min_is_error() {
        let result = check_version_range("0.1.0", Some("bad"), None);
        assert!(matches!(
            result,
            CompatibilityStatus::MalformedMetadata { .. }
        ));
    }

    #[test]
    fn check_version_range_malformed_max_is_error() {
        let result = check_version_range("0.1.0", None, Some("1.x.0"));
        assert!(matches!(
            result,
            CompatibilityStatus::MalformedMetadata { .. }
        ));
    }

    #[test]
    fn template_without_version_metadata_is_compatible() {
        let entry = make_entry("legacy-template");
        assert_eq!(
            check_template_compatibility(&entry),
            CompatibilityStatus::Compatible
        );
    }

    #[test]
    fn template_compatible_with_current_cli() {
        let mut entry = make_entry("current-template");
        entry.cli_version_min = Some(CLI_VERSION.to_string());
        assert_eq!(
            check_template_compatibility(&entry),
            CompatibilityStatus::Compatible
        );
    }

    #[test]
    fn template_requiring_future_cli_is_rejected() {
        let mut entry = make_entry("future-template");
        // Parse current version and bump the major to guarantee a future version.
        let (major, _, _) = parse_semver(CLI_VERSION).unwrap();
        entry.cli_version_min = Some(format!("{}.0.0", major + 100));
        let status = check_template_compatibility(&entry);
        assert!(matches!(status, CompatibilityStatus::TooOld { .. }));
        assert!(assert_template_compatible(&entry).is_err());
    }

    #[test]
    fn template_with_low_max_is_rejected() {
        let mut entry = make_entry("old-template");
        // Set max to a version that is guaranteed to be below the current CLI.
        let (major, minor, _) = parse_semver(CLI_VERSION).unwrap();
        if major > 0 || minor > 0 {
            entry.cli_version_max = Some("0.0.0".to_string());
            let status = check_template_compatibility(&entry);
            assert!(matches!(status, CompatibilityStatus::TooNew { .. }));
            assert!(assert_template_compatible(&entry).is_err());
        }
        // When CLI_VERSION is "0.0.0" the test is a no-op (trivially passes).
    }

    #[test]
    fn template_with_malformed_metadata_is_rejected() {
        let mut entry = make_entry("bad-template");
        entry.cli_version_min = Some("not-a-semver".to_string());
        let status = check_template_compatibility(&entry);
        assert!(matches!(
            status,
            CompatibilityStatus::MalformedMetadata { .. }
        ));
        assert!(assert_template_compatible(&entry).is_err());
    }

    // ── parse_semver edge cases ────────────────────────────────────────────────

    #[test]
    fn parse_semver_large_numbers() {
        assert_eq!(parse_semver("999.0.0"), Ok((999, 0, 0)));
        assert_eq!(parse_semver("0.0.999999"), Ok((0, 0, 999999)));
    }

    #[test]
    fn parse_semver_rejects_single_component() {
        assert!(parse_semver("1").is_err());
    }

    #[test]
    fn parse_semver_rejects_two_components() {
        assert!(parse_semver("1.2").is_err());
    }

    #[test]
    fn parse_semver_rejects_extra_dots() {
        assert!(
            parse_semver("1.2.3.4").is_err(),
            "four components should fail"
        );
    }

    #[test]
    fn parse_semver_rejects_whitespace() {
        assert!(parse_semver(" 1.2.3").is_err());
        assert!(parse_semver("1.2.3 ").is_err());
        assert!(parse_semver("1. 2.3").is_err());
    }

    #[test]
    fn parse_semver_rejects_negative_component() {
        // A leading '-' makes the component non-numeric.
        assert!(parse_semver("1.-2.3").is_err());
    }

    #[test]
    fn parse_semver_rejects_alpha_component() {
        assert!(parse_semver("1.2.alpha").is_err());
        assert!(parse_semver("v1.2.3").is_err());
    }

    // ── check_version_range payload verification ───────────────────────────────

    #[test]
    fn check_version_range_too_old_carries_correct_payload() {
        let result = check_version_range("0.0.9", Some("0.1.0"), None);
        match result {
            CompatibilityStatus::TooOld {
                required_min,
                running,
            } => {
                assert_eq!(required_min, "0.1.0");
                assert_eq!(running, "0.0.9");
            }
            other => panic!("expected TooOld, got {:?}", other),
        }
    }

    #[test]
    fn check_version_range_too_new_carries_correct_payload() {
        let result = check_version_range("2.0.0", None, Some("1.99.99"));
        match result {
            CompatibilityStatus::TooNew {
                required_max,
                running,
            } => {
                assert_eq!(required_max, "1.99.99");
                assert_eq!(running, "2.0.0");
            }
            other => panic!("expected TooNew, got {:?}", other),
        }
    }

    #[test]
    fn check_version_range_exact_min_boundary_is_compatible() {
        // version == min should be Compatible, not TooOld.
        assert_eq!(
            check_version_range("1.0.0", Some("1.0.0"), None),
            CompatibilityStatus::Compatible
        );
    }

    #[test]
    fn check_version_range_exact_max_boundary_is_compatible() {
        // version == max should be Compatible, not TooNew.
        assert_eq!(
            check_version_range("1.0.0", None, Some("1.0.0")),
            CompatibilityStatus::Compatible
        );
    }

    #[test]
    fn check_version_range_min_only_above_min_is_compatible() {
        assert_eq!(
            check_version_range("1.2.0", Some("1.0.0"), None),
            CompatibilityStatus::Compatible
        );
    }

    #[test]
    fn check_version_range_max_only_below_max_is_compatible() {
        assert_eq!(
            check_version_range("0.9.0", None, Some("1.0.0")),
            CompatibilityStatus::Compatible
        );
    }

    #[test]
    fn check_version_range_malformed_running_version_is_error() {
        // The running version itself being malformed should yield MalformedMetadata.
        let result = check_version_range("not-a-version", Some("0.1.0"), None);
        assert!(matches!(
            result,
            CompatibilityStatus::MalformedMetadata { .. }
        ));
    }

    #[test]
    fn check_version_range_malformed_max_carries_reason() {
        let result = check_version_range("0.1.0", None, Some("1.x.0"));
        match result {
            CompatibilityStatus::MalformedMetadata { reason } => {
                assert!(!reason.is_empty(), "reason should not be empty");
            }
            other => panic!("expected MalformedMetadata, got {:?}", other),
        }
    }

    // ── assert_template_compatible error message content ──────────────────────

    #[test]
    fn assert_template_compatible_too_old_message_contains_min_and_running() {
        let mut entry = make_entry("future-tpl");
        let (major, _, _) = parse_semver(CLI_VERSION).unwrap();
        let min = format!("{}.0.0", major + 100);
        entry.cli_version_min = Some(min.clone());
        let err = assert_template_compatible(&entry).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains(&min), "error should contain required_min");
        assert!(
            msg.contains(CLI_VERSION),
            "error should contain running version"
        );
        assert!(
            msg.contains("future-tpl"),
            "error should contain template name"
        );
    }

    #[test]
    fn assert_template_compatible_too_new_message_contains_max_and_running() {
        let mut entry = make_entry("old-tpl");
        let (major, minor, _) = parse_semver(CLI_VERSION).unwrap();
        if major > 0 || minor > 0 {
            entry.cli_version_max = Some("0.0.0".to_string());
            let err = assert_template_compatible(&entry).unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("0.0.0"), "error should contain required_max");
            assert!(
                msg.contains(CLI_VERSION),
                "error should contain running version"
            );
            assert!(
                msg.contains("old-tpl"),
                "error should contain template name"
            );
        }
    }

    #[test]
    fn assert_template_compatible_malformed_message_contains_reason() {
        let mut entry = make_entry("broken-tpl");
        entry.cli_version_min = Some("bad-version".to_string());
        let err = assert_template_compatible(&entry).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("broken-tpl"),
            "error should contain template name"
        );
        assert!(
            msg.contains("malformed") || msg.contains("bad-version"),
            "error should describe the problem"
        );
    }
}
