//! Offline analysis and guidance for integrating a template into a Rust project.
use anyhow::{Context, Result};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub area: String,
    pub message: String,
    pub action: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct DependencyChange {
    pub name: String,
    pub template_requirement: String,
    pub project_requirement: Option<String>,
    pub compatible: bool,
    pub kind: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct TestResult {
    pub attempted: bool,
    pub passed: bool,
    pub command: String,
    pub summary: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct IntegrationReport {
    pub template: PathBuf,
    pub project: PathBuf,
    pub dependencies: Vec<DependencyChange>,
    pub configuration: Vec<Finding>,
    pub customization: Vec<String>,
    pub testing: Vec<String>,
    pub deployment: Vec<String>,
    pub troubleshooting: Vec<Finding>,
    pub test_result: Option<TestResult>,
}

impl IntegrationReport {
    pub fn has_errors(&self) -> bool {
        self.configuration
            .iter()
            .chain(&self.troubleshooting)
            .any(|f| f.severity == Severity::Error)
            || self.dependencies.iter().any(|d| !d.compatible)
    }
    pub fn to_markdown(&self) -> String {
        let status = if self.has_errors() {
            "Needs attention"
        } else {
            "Ready to integrate"
        };
        let mut out = format!("# Template integration report\n\n**Status:** {status}\n\n**Template:** `{}`  \n**Project:** `{}`\n\n", self.template.display(), self.project.display());
        out.push_str("## Dependencies\n\n");
        if self.dependencies.is_empty() {
            out.push_str("No template dependencies were found.\n\n");
        } else {
            out.push_str(
                "| Dependency | Kind | Template | Project | Result |\n|---|---|---|---|---|\n",
            );
            for d in &self.dependencies {
                out.push_str(&format!(
                    "| {} | {} | `{}` | {} | {} |\n",
                    d.name,
                    d.kind,
                    d.template_requirement,
                    d.project_requirement
                        .as_deref()
                        .map(|v| format!("`{v}`"))
                        .unwrap_or_else(|| "add".into()),
                    if d.compatible {
                        "compatible"
                    } else {
                        "conflict"
                    }
                ));
            }
            out.push('\n');
        }
        push_findings(&mut out, "Configuration", &self.configuration);
        push_list(&mut out, "Customization guidance", &self.customization);
        push_list(&mut out, "Integration testing", &self.testing);
        push_list(&mut out, "Deployment preparation", &self.deployment);
        push_findings(&mut out, "Troubleshooting", &self.troubleshooting);
        if let Some(test) = &self.test_result {
            out.push_str(&format!(
                "## Test execution\n\n- Command: `{}`\n- Result: {}\n- Summary: {}\n\n",
                test.command,
                if test.passed { "passed" } else { "failed" },
                test.summary
            ));
        }
        out
    }
}
fn push_list(out: &mut String, heading: &str, values: &[String]) {
    out.push_str(&format!("## {heading}\n\n"));
    if values.is_empty() {
        out.push_str("No additional steps detected.\n\n");
    } else {
        for v in values {
            out.push_str(&format!("- {v}\n"));
        }
        out.push('\n');
    }
}
fn push_findings(out: &mut String, heading: &str, values: &[Finding]) {
    out.push_str(&format!("## {heading}\n\n"));
    if values.is_empty() {
        out.push_str("No issues detected.\n\n");
    } else {
        for f in values {
            out.push_str(&format!(
                "- **{:?} / {}:** {} _Action: {}_\n",
                f.severity, f.area, f.message, f.action
            ));
        }
        out.push('\n');
    }
}

/// Analyze a template and existing project without modifying either.
pub fn analyze(template: &Path, project: &Path) -> Result<IntegrationReport> {
    validate_directory(template, "Template")?;
    validate_directory(project, "Project")?;
    let tm = read_manifest(template)?;
    let pm = read_manifest(project)?;
    let dependencies = compare_dependencies(&tm, &pm);
    let placeholders = collect_placeholders(template)?;
    let mut configuration = placeholders
        .iter()
        .map(|name| Finding {
            severity: Severity::Warning,
            area: "placeholder".into(),
            message: format!("Template value `{{{{{name}}}}}` must be resolved."),
            action: placeholder_action(name),
        })
        .collect::<Vec<_>>();
    if let Some(name) = [".env.example", ".env.sample"]
        .iter()
        .find(|name| template.join(name).is_file())
    {
        configuration.push(Finding {
            severity: Severity::Info,
            area: "environment".into(),
            message: format!("Template provides `{name}`."),
            action: format!("Copy `{name}` to `.env` and fill values; do not commit secrets."),
        });
    }
    let mut troubleshooting = structural_findings(template, project);
    for d in &dependencies {
        if !d.compatible {
            troubleshooting.push(Finding { severity: Severity::Error, area: "dependency".into(), message: format!("`{}` requires `{}` in the template but `{}` in the project.", d.name, d.template_requirement, d.project_requirement.as_deref().unwrap_or("nothing")), action: "Choose one compatible version and run `cargo update`; review breaking changes before merging.".into() });
        }
    }
    let crate_name = package_name(&tm).unwrap_or_else(|| "template crate".into());
    let has_tests = template.join("tests").is_dir()
        || source_files(template)?.iter().any(|p| {
            fs::read_to_string(p)
                .map(|s| s.contains("#[test]"))
                .unwrap_or(false)
        });
    Ok(IntegrationReport {
        template: canonical_or_original(template), project: canonical_or_original(project), dependencies, configuration,
        customization: vec!["Replace project-name placeholders, then rename contract types and public methods to match your domain.".into(), format!("Review `{crate_name}` public entry points before copying source modules; preserve existing project APIs where possible."), "Merge source and configuration files deliberately; do not overwrite the target Cargo.toml or existing tests wholesale.".into(), "Review authorization, storage keys, TTL behavior, and emitted events for application-specific security requirements.".into()],
        testing: vec!["Run `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`.".into(), "Run `cargo test` after merging dependencies and source files.".into(), if has_tests { "Port the template's unit/integration tests and add cases for the target project's existing behavior.".into() } else { "The template has no detected tests; add happy-path, authorization, boundary, and failure tests before deployment.".into() }, "Build the WASM artifact and test it on a local network or testnet before production.".into()],
        deployment: vec!["Confirm the selected Rust WASM target and Stellar CLI are installed.".into(), "Run `starforge contract build` and record the resulting WASM hash.".into(), "Set network/account configuration explicitly and keep secrets outside source control.".into(), "Deploy to testnet, execute a smoke invocation, then document contract ID and rollback/upgrade steps.".into()],
        troubleshooting, test_result: None,
    })
}

pub fn run_integration_tests(project: &Path) -> TestResult {
    match Command::new("cargo")
        .arg("test")
        .arg("--all-targets")
        .current_dir(project)
        .output()
    {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let summary = stderr
                .lines()
                .chain(stdout.lines())
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("cargo test completed")
                .trim()
                .to_string();
            TestResult {
                attempted: true,
                passed: output.status.success(),
                command: "cargo test --all-targets".into(),
                summary,
            }
        }
        Err(error) => TestResult {
            attempted: true,
            passed: false,
            command: "cargo test --all-targets".into(),
            summary: format!("Could not start cargo: {error}"),
        },
    }
}
fn validate_directory(path: &Path, label: &str) -> Result<()> {
    if !path.is_dir() {
        anyhow::bail!("{label} directory does not exist: {}", path.display());
    }
    if !path.join("Cargo.toml").is_file() {
        anyhow::bail!("{label} is missing Cargo.toml: {}", path.display());
    }
    Ok(())
}
fn read_manifest(root: &Path) -> Result<toml::Value> {
    let path = root.join("Cargo.toml");
    let source =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    toml::from_str(&source).with_context(|| format!("Invalid TOML in {}", path.display()))
}
fn package_name(m: &toml::Value) -> Option<String> {
    m.get("package")?.get("name")?.as_str().map(str::to_owned)
}
fn dependency_tables(m: &toml::Value) -> BTreeMap<(String, String), String> {
    let mut out = BTreeMap::new();
    for kind in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = m.get(kind).and_then(toml::Value::as_table) {
            for (name, value) in table {
                let req = value
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| value.get("version")?.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "path/git".into());
                out.insert((kind.into(), name.clone()), req);
            }
        }
    }
    out
}
fn compare_dependencies(template: &toml::Value, project: &toml::Value) -> Vec<DependencyChange> {
    let t = dependency_tables(template);
    let p = dependency_tables(project);
    t.into_iter()
        .map(|((kind, name), req)| {
            let current = p.get(&(kind.clone(), name.clone())).cloned().or_else(|| {
                p.iter()
                    .find(|((_, n), _)| n == &name)
                    .map(|(_, v)| v.clone())
            });
            let compatible = current
                .as_deref()
                .map(|v| requirements_compatible(&req, v))
                .unwrap_or(true);
            DependencyChange {
                name,
                template_requirement: req,
                project_requirement: current,
                compatible,
                kind,
            }
        })
        .collect()
}
fn requirements_compatible(a: &str, b: &str) -> bool {
    if a == b || a == "path/git" || b == "path/git" {
        return true;
    }
    let norm = |v: &str| {
        v.trim()
            .trim_start_matches(['^', '~', '=', '>', '<'])
            .to_string()
    };
    let req = |v: &str| {
        semver::VersionReq::parse(v)
            .ok()
            .or_else(|| semver::VersionReq::parse(&format!("^{}", norm(v))).ok())
    };
    let ver = |v: &str| {
        semver::Version::parse(&norm(v))
            .ok()
            .or_else(|| semver::Version::parse(&format!("{}.0", norm(v))).ok())
    };
    matches!((req(a), ver(b)), (Some(r), Some(v)) if r.matches(&v))
        || matches!((req(b), ver(a)), (Some(r), Some(v)) if r.matches(&v))
}
fn source_files(root: &Path) -> Result<Vec<PathBuf>> {
    fn walk(path: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let p = entry.path();
            let n = entry.file_name();
            if p.is_dir() && n != "target" && n != ".git" {
                walk(&p, files)?;
            } else if p.is_file() {
                files.push(p);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(root, &mut files)?;
    Ok(files)
}
fn collect_placeholders(root: &Path) -> Result<Vec<String>> {
    let mut values = BTreeSet::new();
    for path in source_files(root)? {
        if fs::metadata(&path)?.len() > 1_000_000 {
            continue;
        }
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        let mut rest = content.as_str();
        while let Some(start) = rest.find("{{") {
            rest = &rest[start + 2..];
            let Some(end) = rest.find("}}") else { break };
            let value = rest[..end].trim();
            if !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                values.insert(value.to_string());
            }
            rest = &rest[end + 2..];
        }
    }
    Ok(values.into_iter().collect())
}
fn placeholder_action(name: &str) -> String {
    match name {
        "PROJECT_NAME" => "Use the target package name from Cargo.toml.".into(),
        "PROJECT_NAME_SNAKE" => "Use the project name with hyphens replaced by underscores.".into(),
        "PROJECT_NAME_PASCAL" => "Use the project name converted to PascalCase.".into(),
        _ => format!("Choose a project-specific value for `{name}` and replace every occurrence."),
    }
}
fn structural_findings(template: &Path, project: &Path) -> Vec<Finding> {
    let mut out = Vec::new();
    for (root, label) in [(template, "template"), (project, "project")] {
        if !root.join("src/lib.rs").is_file() && !root.join("src/main.rs").is_file() {
            out.push(Finding {
                severity: Severity::Error,
                area: "structure".into(),
                message: format!("The {label} has no `src/lib.rs` or `src/main.rs`."),
                action: "Restore a Rust crate entry point before integration.".into(),
            });
        }
    }
    if !template.join("README.md").is_file() {
        out.push(Finding {
            severity: Severity::Warning,
            area: "documentation".into(),
            message: "The template has no README.md.".into(),
            action:
                "Inspect its public API and configuration manually; ask the author to document it."
                    .into(),
        });
    }
    if project.join(".env").is_file() && !project.join(".gitignore").is_file() {
        out.push(Finding {
            severity: Severity::Warning,
            area: "secrets".into(),
            message: "The project has a `.env` file but no `.gitignore`.".into(),
            action: "Add `.env` to `.gitignore` before adding credentials.".into(),
        });
    }
    out
}
fn canonical_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn crate_at(root: &Path, version: &str, placeholder: bool) {
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "#[cfg(test)] mod tests { #[test] fn ok() {} }",
        )
        .unwrap();
        fs::write(root.join("Cargo.toml"), format!("[package]\nname=\"demo\"\nversion=\"0.1.0\"\n[dependencies]\nsoroban-sdk=\"{version}\"\n")).unwrap();
        if placeholder {
            fs::write(root.join("README.md"), "{{PROJECT_NAME}} {{NETWORK}}").unwrap();
        }
    }
    #[test]
    fn reports_conflicts_and_placeholders() {
        let tmp = tempfile::tempdir().unwrap();
        let t = tmp.path().join("template");
        let p = tmp.path().join("project");
        crate_at(&t, "21.0.0", true);
        crate_at(&p, "20.0.0", false);
        let report = analyze(&t, &p).unwrap();
        assert!(!report.dependencies[0].compatible);
        assert_eq!(report.configuration.len(), 2);
        assert!(report.to_markdown().contains("Needs attention"));
    }
    #[test]
    fn missing_manifest_is_actionable() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(analyze(tmp.path(), tmp.path())
            .unwrap_err()
            .to_string()
            .contains("Cargo.toml"));
    }
}
