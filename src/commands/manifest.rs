use crate::manifest::{
    discover_manifest, find_and_load_manifest, load_manifest, ProjectManifest, MANIFEST_FILENAME,
};
use crate::utils::print as p;
use anyhow::{Context, Result};
use clap::Subcommand;
use colored::*;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum ManifestCommands {
    /// Validate an existing starforge.toml project manifest
    Validate {
        /// Optional path to starforge.toml or project directory
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Output or save the JSON Schema for starforge.toml
    Schema {
        /// Optional path to save the JSON schema file
        #[arg(long, short)]
        out: Option<PathBuf>,
    },
    /// Initialize a new starforge.toml project manifest in the current directory
    Init {
        /// Project name (defaults to current directory name)
        #[arg(long)]
        name: Option<String>,
    },
    /// Display current project manifest summary
    Show {
        /// Emit machine-readable JSON output
        #[arg(long)]
        json: bool,
    },
}

pub async fn handle(cmd: ManifestCommands) -> Result<()> {
    match cmd {
        ManifestCommands::Validate { path } => validate_cmd(path),
        ManifestCommands::Schema { out } => schema_cmd(out),
        ManifestCommands::Init { name } => init_cmd(name),
        ManifestCommands::Show { json } => show_cmd(json),
    }
}

fn validate_cmd(path: Option<PathBuf>) -> Result<()> {
    let target = match path {
        Some(p) if p.is_file() => p,
        Some(p) => p.join(MANIFEST_FILENAME),
        None => {
            let cwd = std::env::current_dir().context("Failed to get current directory")?;
            discover_manifest(&cwd).ok_or_else(|| {
                anyhow::anyhow!(
                    "No starforge.toml found in current directory or parent directories."
                )
            })?
        }
    };

    p::header(&format!("Validating project manifest: {}", target.display()));
    let manifest = load_manifest(&target)?;
    manifest.validate()?;

    p::success("Manifest is valid!");
    p::kv("Version", &manifest.version);
    if let Some(pkg) = &manifest.package {
        p::kv("Package", &pkg.name);
    }
    p::kv("Contracts", &manifest.contracts.len().to_string());
    p::kv("Networks", &manifest.networks.len().to_string());
    p::kv("Deploy Targets", &manifest.deploy.len().to_string());
    p::kv("Scripts", &manifest.scripts.len().to_string());

    Ok(())
}

fn schema_cmd(out: Option<PathBuf>) -> Result<()> {
    let schema = ProjectManifest::json_schema();
    let schema_str = serde_json::to_string_pretty(&schema)?;

    if let Some(out_path) = out {
        fs::write(&out_path, &schema_str)
            .with_context(|| format!("Failed to write schema to {}", out_path.display()))?;
        p::success(&format!("Schema written to {}", out_path.display()));
    } else {
        println!("{}", schema_str);
    }

    Ok(())
}

fn init_cmd(name: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let manifest_path = cwd.join(MANIFEST_FILENAME);

    if manifest_path.exists() {
        anyhow::bail!("{} already exists in this directory.", MANIFEST_FILENAME);
    }

    let project_name = name.unwrap_or_else(|| {
        cwd.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("soroban_project")
            .to_string()
    });

    let manifest = ProjectManifest::default_starter(&project_name);
    let toml_str = toml::to_string_pretty(&manifest)
        .context("Failed to serialize default project manifest")?;

    fs::write(&manifest_path, toml_str)?;
    p::success(&format!(
        "Created {} for project '{}'",
        MANIFEST_FILENAME, project_name
    ));

    Ok(())
}

fn show_cmd(json: bool) -> Result<()> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    let (path, manifest) = find_and_load_manifest(&cwd)?
        .ok_or_else(|| anyhow::anyhow!("No starforge.toml found in current directory or ancestors."))?;

    if json {
        let json_val = serde_json::to_string_pretty(&manifest)?;
        println!("{}", json_val);
    } else {
        p::header(&format!("Project Manifest: {}", path.display()));
        p::kv("Version", &manifest.version);
        if let Some(pkg) = &manifest.package {
            p::kv("Package Name", &pkg.name);
            if let Some(v) = &pkg.version {
                p::kv("Package Version", v);
            }
        }
        println!("\n{}", "Contracts:".bold());
        for (name, c) in &manifest.contracts {
            println!("  • {}: path={:?}, wasm={:?}", name, c.path, c.wasm);
        }
        println!("\n{}", "Networks:".bold());
        for (name, n) in &manifest.networks {
            println!("  • {}: horizon={:?}", name, n.horizon_url);
        }
        println!("\n{}", "Deploy Targets:".bold());
        for (name, d) in &manifest.deploy {
            println!("  • {}: network={:?}, contracts={:?}", name, d.network, d.contracts);
        }
        println!("\n{}", "Scripts:".bold());
        for (name, script) in &manifest.scripts {
            println!("  • {}: {}", name, script);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_and_validate_in_tempdir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manifest_path = temp_dir.path().join(MANIFEST_FILENAME);
        let starter = ProjectManifest::default_starter("test_app");
        let toml_str = toml::to_string_pretty(&starter).unwrap();
        fs::write(&manifest_path, toml_str).unwrap();

        let loaded = load_manifest(&manifest_path).unwrap();
        assert_eq!(loaded.package.unwrap().name, "test_app");
        assert!(loaded.validate().is_ok());
    }

    #[test]
    fn schema_cmd_generates_json() {
        let schema = ProjectManifest::json_schema();
        assert!(schema.get("properties").is_some());
        assert_eq!(schema["required"][0], "version");
    }
}
