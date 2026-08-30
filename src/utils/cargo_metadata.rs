//! Cargo Metadata Correction and Validation Utility
//!
//! Provides utilities to scan, detect placeholder repository/homepage/documentation metadata
//! in `Cargo.toml` files, validate URLs, enforce package link requirements, and correct metadata across workspaces.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// Metadata extracted from a `Cargo.toml` package section.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CargoPackageMetadata {
    pub name: String,
    pub version: Option<String>,
    pub edition: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub license_file: Option<String>,
    pub repository: Option<String>,
    pub homepage: Option<String>,
    pub documentation: Option<String>,
    pub publish: Option<bool>,
}

/// Errors encountered during metadata operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CargoMetadataError {
    MissingCargoToml(PathBuf),
    IoError(String),
    TomlParseError(String),
    InvalidUrl(String),
    IncompleteUrlPattern(String),
    UndeterminedRepositoryUrl(PathBuf),
    LicenseFileNotFound(PathBuf),
    InvalidPackageName(String),
}

impl fmt::Display for CargoMetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCargoToml(path) => {
                write!(f, "Cargo.toml not found at path: {}", path.display())
            }
            Self::IoError(err) => write!(f, "I/O error during Cargo.toml operation: {err}"),
            Self::TomlParseError(err) => write!(f, "Failed to parse Cargo.toml: {err}"),
            Self::InvalidUrl(url) => write!(f, "Invalid HTTP/HTTPS URL format: '{url}'"),
            Self::IncompleteUrlPattern(url) => {
                write!(f, "Incomplete or placeholder URL detected: '{url}'")
            }
            Self::UndeterminedRepositoryUrl(path) => write!(
                f,
                "Repository URL could not be determined for: {}",
                path.display()
            ),
            Self::LicenseFileNotFound(path) => {
                write!(f, "License file not found at path: {}", path.display())
            }
            Self::InvalidPackageName(name) => {
                write!(f, "Invalid crate package name format: '{name}'")
            }
        }
    }
}

impl std::error::Error for CargoMetadataError {}

/// Report summarizing changes made by `CargoMetadataFixer`.
#[derive(Debug, Clone, Default)]
pub struct FixReport {
    pub files_scanned: usize,
    pub files_updated: Vec<PathBuf>,
    pub placeholders_corrected: usize,
    pub warnings: Vec<String>,
}

/// Report summarizing package link validation results.
#[derive(Debug, Clone, Default)]
pub struct ValidationReport {
    pub path: PathBuf,
    pub package_name: String,
    pub is_valid: bool,
    pub repository_valid: bool,
    pub homepage_valid: bool,
    pub documentation_valid: bool,
    pub license_valid: bool,
    pub is_private: bool,
    pub errors: Vec<CargoMetadataError>,
    pub warnings: Vec<String>,
}

/// Fixer for detecting and updating Cargo repository metadata.
pub struct CargoMetadataFixer;

impl CargoMetadataFixer {
    /// Detects if a URL string contains placeholder or incomplete patterns.
    pub fn is_placeholder_url(url: &str) -> bool {
        let trimmed = url.trim();

        if trimmed.is_empty() {
            return true;
        }

        let lower = trimmed.to_lowercase();

        // Placeholder domain / path patterns
        let placeholder_patterns = [
            "github.com/todo",
            "github.com/your_username",
            "github.com/user/repo",
            "github.com/example",
            "example.com",
            "placeholder",
            "todo.com",
        ];

        for pattern in &placeholder_patterns {
            if lower.contains(pattern) {
                return true;
            }
        }

        // Incomplete repository URLs (e.g., https://github.com or https://github.com/)
        if lower == "https://github.com"
            || lower == "https://github.com/"
            || lower == "http://github.com"
            || lower == "http://github.com/"
        {
            return true;
        }

        // Must start with valid HTTP/HTTPS scheme
        if !lower.starts_with("http://") && !lower.starts_with("https://") {
            return true;
        }

        false
    }

    /// Validates basic HTTP/HTTPS URL structure.
    pub fn validate_url_format(url: &str) -> Result<(), CargoMetadataError> {
        let trimmed = url.trim();

        if Self::is_placeholder_url(trimmed) {
            return Err(CargoMetadataError::IncompleteUrlPattern(
                trimmed.to_string(),
            ));
        }

        if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
            return Err(CargoMetadataError::InvalidUrl(trimmed.to_string()));
        }

        let after_scheme = if let Some(s) = trimmed.strip_prefix("https://") {
            s
        } else if let Some(s) = trimmed.strip_prefix("http://") {
            s
        } else {
            return Err(CargoMetadataError::InvalidUrl(trimmed.to_string()));
        };

        if after_scheme.trim().is_empty() || !after_scheme.contains('.') {
            return Err(CargoMetadataError::InvalidUrl(trimmed.to_string()));
        }

        Ok(())
    }

    /// Locates all `Cargo.toml` files under `root_path` and updates placeholder metadata with `default_repo_url`.
    pub fn locate_and_update_cargo_tomls(
        root_path: &Path,
        default_repo_url: &str,
    ) -> Result<FixReport, CargoMetadataError> {
        Self::validate_url_format(default_repo_url)?;

        if !root_path.exists() {
            return Err(CargoMetadataError::MissingCargoToml(
                root_path.to_path_buf(),
            ));
        }

        let cargo_tomls = find_cargo_toml_files(root_path)?;
        if cargo_tomls.is_empty() {
            if root_path.file_name().and_then(|n| n.to_str()) == Some("Cargo.toml") {
                // Single file target
            } else {
                return Err(CargoMetadataError::MissingCargoToml(
                    root_path.to_path_buf(),
                ));
            }
        }

        let target_files = if root_path.is_file() {
            vec![root_path.to_path_buf()]
        } else {
            cargo_tomls
        };

        let mut report = FixReport::default();

        for cargo_file in target_files {
            report.files_scanned += 1;
            let content = match fs::read_to_string(&cargo_file) {
                Ok(c) => c,
                Err(e) => return Err(CargoMetadataError::IoError(e.to_string())),
            };

            // Skip template files containing mustache variables like {{PROJECT_NAME}}
            if content.contains("{{") && content.contains("}}") {
                report.warnings.push(format!(
                    "Skipping template file containing unrendered placeholders: {}",
                    cargo_file.display()
                ));
                continue;
            }

            let mut modified = false;
            let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            let mut in_package_section = false;
            let mut has_repository = false;
            let mut has_homepage = false;
            let mut has_documentation = false;
            let mut package_name = String::new();
            let mut package_end_idx = None;

            for (idx, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                if trimmed.starts_with('[') && trimmed.ends_with(']') {
                    if trimmed == "[package]" {
                        in_package_section = true;
                    } else if in_package_section {
                        in_package_section = false;
                        if package_end_idx.is_none() {
                            package_end_idx = Some(idx);
                        }
                    }
                    continue;
                }

                if in_package_section {
                    if let Some((key, val)) = parse_kv_line(trimmed) {
                        match key.as_str() {
                            "name" => {
                                package_name = val.trim_matches('"').to_string();
                            }
                            "repository" => {
                                has_repository = true;
                                let clean_val = val.trim_matches('"');
                                if Self::is_placeholder_url(clean_val) {
                                    report.placeholders_corrected += 1;
                                    modified = true;
                                }
                            }
                            "homepage" => {
                                has_homepage = true;
                                let clean_val = val.trim_matches('"');
                                if Self::is_placeholder_url(clean_val) {
                                    report.placeholders_corrected += 1;
                                    modified = true;
                                }
                            }
                            "documentation" => {
                                has_documentation = true;
                                let clean_val = val.trim_matches('"');
                                if Self::is_placeholder_url(clean_val) {
                                    report.placeholders_corrected += 1;
                                    modified = true;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            if package_name.is_empty() {
                // Not a crate package Cargo.toml or missing name
                continue;
            }

            let doc_url = format!("https://docs.rs/{package_name}");
            let repo_url = default_repo_url.trim_end_matches('/');

            // Re-build lines for [package] section
            let mut new_lines = Vec::new();
            in_package_section = false;

            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed == "[package]" {
                    in_package_section = true;
                    new_lines.push(line.to_string());
                    continue;
                }

                if trimmed.starts_with('[') && trimmed.ends_with(']') {
                    if in_package_section {
                        // Append missing fields before leaving [package] section
                        if !has_repository {
                            new_lines.push(format!("repository = \"{repo_url}\""));
                            modified = true;
                        }
                        if !has_homepage {
                            new_lines.push(format!("homepage = \"{repo_url}\""));
                            modified = true;
                        }
                        if !has_documentation {
                            new_lines.push(format!("documentation = \"{doc_url}\""));
                            modified = true;
                        }
                        in_package_section = false;
                    }
                    new_lines.push(line.to_string());
                    continue;
                }

                if in_package_section {
                    if let Some((key, val)) = parse_kv_line(trimmed) {
                        match key.as_str() {
                            "repository" => {
                                let clean_val = val.trim_matches('"');
                                if Self::is_placeholder_url(clean_val) {
                                    new_lines.push(format!("repository = \"{repo_url}\""));
                                } else {
                                    new_lines.push(line.to_string());
                                }
                            }
                            "homepage" => {
                                let clean_val = val.trim_matches('"');
                                if Self::is_placeholder_url(clean_val) {
                                    new_lines.push(format!("homepage = \"{repo_url}\""));
                                } else {
                                    new_lines.push(line.to_string());
                                }
                            }
                            "documentation" => {
                                let clean_val = val.trim_matches('"');
                                if Self::is_placeholder_url(clean_val) {
                                    new_lines.push(format!("documentation = \"{doc_url}\""));
                                } else {
                                    new_lines.push(line.to_string());
                                }
                            }
                            _ => {
                                new_lines.push(line.to_string());
                            }
                        }
                    } else {
                        new_lines.push(line.to_string());
                    }
                } else {
                    new_lines.push(line.to_string());
                }
            }

            if in_package_section {
                if !has_repository {
                    new_lines.push(format!("repository = \"{repo_url}\""));
                    modified = true;
                }
                if !has_homepage {
                    new_lines.push(format!("homepage = \"{repo_url}\""));
                    modified = true;
                }
                if !has_documentation {
                    new_lines.push(format!("documentation = \"{doc_url}\""));
                    modified = true;
                }
            }

            if modified {
                let updated_content = new_lines.join("\n") + "\n";
                if let Err(e) = fs::write(&cargo_file, updated_content) {
                    return Err(CargoMetadataError::IoError(e.to_string()));
                }
                report.files_updated.push(cargo_file);
            }
        }

        Ok(report)
    }
}

/// Validator for Cargo package metadata and links.
pub struct CargoMetadataValidator;

impl CargoMetadataValidator {
    /// Parses package metadata from a `Cargo.toml` file.
    pub fn parse_metadata(
        cargo_toml_path: &Path,
    ) -> Result<CargoPackageMetadata, CargoMetadataError> {
        if !cargo_toml_path.exists() {
            return Err(CargoMetadataError::MissingCargoToml(
                cargo_toml_path.to_path_buf(),
            ));
        }

        let content = fs::read_to_string(cargo_toml_path)
            .map_err(|e| CargoMetadataError::IoError(e.to_string()))?;

        let mut meta = CargoPackageMetadata::default();
        let mut in_package = false;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                in_package = trimmed == "[package]";
                continue;
            }

            if in_package {
                if let Some((key, val)) = parse_kv_line(trimmed) {
                    let clean_val = val.trim_matches('"').to_string();
                    match key.as_str() {
                        "name" => meta.name = clean_val,
                        "version" => meta.version = Some(clean_val),
                        "edition" => meta.edition = Some(clean_val),
                        "description" => meta.description = Some(clean_val),
                        "license" => meta.license = Some(clean_val),
                        "license-file" => meta.license_file = Some(clean_val),
                        "repository" => meta.repository = Some(clean_val),
                        "homepage" => meta.homepage = Some(clean_val),
                        "documentation" => meta.documentation = Some(clean_val),
                        "publish" => meta.publish = val.parse::<bool>().ok(),
                        _ => {}
                    }
                }
            }
        }

        if meta.name.is_empty() {
            return Err(CargoMetadataError::TomlParseError(
                "Missing package name in [package] section".to_string(),
            ));
        }

        Ok(meta)
    }

    /// Validates package metadata fields and package links for a `Cargo.toml` file.
    pub fn validate_package_links(
        cargo_toml_path: &Path,
    ) -> Result<ValidationReport, CargoMetadataError> {
        let meta = Self::parse_metadata(cargo_toml_path)?;
        let repo_root = cargo_toml_path.parent().unwrap_or_else(|| Path::new("."));

        let mut report = ValidationReport {
            path: cargo_toml_path.to_path_buf(),
            package_name: meta.name.clone(),
            is_valid: true,
            repository_valid: true,
            homepage_valid: true,
            documentation_valid: true,
            license_valid: true,
            is_private: meta.publish == Some(false),
            errors: Vec::new(),
            warnings: Vec::new(),
        };

        // Package name format validation
        if !is_valid_crate_name(&meta.name) {
            report.is_valid = false;
            report
                .errors
                .push(CargoMetadataError::InvalidPackageName(meta.name.clone()));
        }

        // Repository URL check
        match &meta.repository {
            Some(repo) => {
                if let Err(e) = CargoMetadataFixer::validate_url_format(repo) {
                    report.repository_valid = false;
                    report.is_valid = false;
                    report.errors.push(e);
                }
            }
            None => {
                report.repository_valid = false;
                if !report.is_private {
                    report
                        .warnings
                        .push("Missing 'repository' field in published crate".to_string());
                }
            }
        }

        // Homepage URL check
        if let Some(homepage) = &meta.homepage {
            if let Err(e) = CargoMetadataFixer::validate_url_format(homepage) {
                report.homepage_valid = false;
                report.is_valid = false;
                report.errors.push(e);
            }
        }

        // Documentation URL check
        if let Some(doc) = &meta.documentation {
            if let Err(e) = CargoMetadataFixer::validate_url_format(doc) {
                report.documentation_valid = false;
                report.is_valid = false;
                report.errors.push(e);
            }
        }

        // License file existence check if specified
        if let Some(lic_file) = &meta.license_file {
            let lic_path = repo_root.join(lic_file);
            if !lic_path.exists() {
                report.license_valid = false;
                report.is_valid = false;
                report
                    .errors
                    .push(CargoMetadataError::LicenseFileNotFound(lic_path));
            }
        } else if meta.license.is_none() {
            report.license_valid = false;
            if !report.is_private {
                report
                    .warnings
                    .push("No license or license-file specified".to_string());
            }
        }

        Ok(report)
    }
}

/// Helper function to check if crate package name follows valid crates.io naming rules.
fn is_valid_crate_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let first = name.chars().next().unwrap();
    if !first.is_ascii_alphabetic() {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Recursively finds all `Cargo.toml` files, excluding target, .git, node_modules, etc.
fn find_cargo_toml_files(dir: &Path) -> Result<Vec<PathBuf>, CargoMetadataError> {
    let mut results = Vec::new();
    if !dir.is_dir() {
        if dir.file_name().and_then(|n| n.to_str()) == Some("Cargo.toml") {
            return Ok(vec![dir.to_path_buf()]);
        }
        return Ok(results);
    }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) => return Err(CargoMetadataError::IoError(err.to_string())),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if file_name.starts_with('.')
            || file_name == "target"
            || file_name == "node_modules"
            || file_name == "vendor"
        {
            continue;
        }

        if path.is_dir() {
            let mut sub = find_cargo_toml_files(&path)?;
            results.append(&mut sub);
        } else if file_name == "Cargo.toml" {
            results.push(path);
        }
    }

    Ok(results)
}

/// Helper to parse simple key = "value" lines.
fn parse_kv_line(line: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = line.splitn(2, '=').collect();
    if parts.len() == 2 {
        let key = parts[0].trim().to_string();
        let val = parts[1].trim().to_string();
        Some((key, val))
    } else {
        None
    }
}
