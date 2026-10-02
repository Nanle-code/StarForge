//! Project manifest (`starforge.toml`) handling for StarForge.
//!
//! A project manifest serves as the single source of truth for a StarForge project.
//! It describes contracts, networks, deployment targets, init arguments, and custom task scripts.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Canonical filename for the StarForge project manifest.
pub const MANIFEST_FILENAME: &str = "starforge.toml";

/// Currently supported schema version.
pub const SUPPORTED_MANIFEST_VERSION: &str = "1";

/// StarForge Project Manifest root structure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectManifest {
    /// Schema version string (must be "1").
    pub version: String,

    /// Optional package metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<PackageConfig>,

    /// Soroban WASM target override; defaults according to the active Rust toolchain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wasm_target: Option<String>,

    /// Contract definitions mapped by contract name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub contracts: BTreeMap<String, ContractConfig>,

    /// Custom or overridden network definitions mapped by network name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub networks: BTreeMap<String, ManifestNetworkConfig>,

    /// Deployment configurations per environment (e.g. "testnet", "mainnet").
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub deploy: BTreeMap<String, DeployTargetConfig>,

    /// Custom task scripts (e.g. "build", "test", "smoke").
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub scripts: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PackageConfig {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authors: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ContractConfig {
    /// Path to contract source directory or crate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    /// Path to compiled WASM artifact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wasm: Option<String>,

    /// Initialization arguments for contract deployment/invocation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub init_args: Option<serde_json::Value>,

    /// Custom build command for this contract.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ManifestNetworkConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horizon_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soroban_rpc_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub friendbot_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_passphrase: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DeployTargetConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contracts: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fee: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_wallet: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub init_args: Option<serde_json::Value>,
}

impl ProjectManifest {
    /// Creates a default starter manifest for a given project name.
    pub fn default_starter(project_name: &str) -> Self {
        let mut contracts = BTreeMap::new();
        contracts.insert(
            project_name.to_string(),
            ContractConfig {
                path: Some(".".to_string()),
                wasm: None,
                init_args: None,
                build: Some("starforge contract build".to_string()),
            },
        );

        let mut networks = BTreeMap::new();
        networks.insert(
            "testnet".to_string(),
            ManifestNetworkConfig {
                horizon_url: Some("https://horizon-testnet.stellar.org".to_string()),
                soroban_rpc_url: Some("https://soroban-testnet.stellar.org".to_string()),
                friendbot_url: Some("https://friendbot-testnet.stellar.org".to_string()),
                network_passphrase: Some("Test SDF Network ; October 2015".to_string()),
            },
        );

        let mut deploy = BTreeMap::new();
        deploy.insert(
            "testnet".to_string(),
            DeployTargetConfig {
                network: Some("testnet".to_string()),
                contracts: Some(vec![project_name.to_string()]),
                fee: Some(100),
                source_wallet: None,
                init_args: None,
            },
        );

        let mut scripts = BTreeMap::new();
        scripts.insert(
            "build".to_string(),
            "starforge contract build".to_string(),
        );
        scripts.insert("test".to_string(), "cargo test".to_string());

        Self {
            version: SUPPORTED_MANIFEST_VERSION.to_string(),
            package: Some(PackageConfig {
                name: project_name.to_string(),
                version: Some("0.1.0".to_string()),
                description: Some(format!("Soroban contract project {}", project_name)),
                authors: Some(vec![]),
                license: Some("MIT".to_string()),
            }),
            wasm_target: None,
            contracts,
            networks,
            deploy,
            scripts,
        }
    }

    /// Validates the manifest fields for consistency and schema requirements.
    pub fn validate(&self) -> Result<()> {
        if self.version != SUPPORTED_MANIFEST_VERSION {
            bail!(
                "Unsupported manifest version '{}'. Currently supported version is '{}'.",
                self.version,
                SUPPORTED_MANIFEST_VERSION
            );
        }

        if let Some(target) = &self.wasm_target {
            crate::utils::wasm_target::resolve_target(Some(target))?;
        }

        if let Some(ref pkg) = self.package {
            if pkg.name.trim().is_empty() {
                bail!("Package name in starforge.toml cannot be empty.");
            }
        }

        for (c_name, c_config) in &self.contracts {
            if c_config.path.is_none() && c_config.wasm.is_none() && c_config.build.is_none() {
                bail!(
                    "Contract '{}' in starforge.toml must specify at least one of: 'path', 'wasm', or 'build'.",
                    c_name
                );
            }
        }

        for (n_name, n_config) in &self.networks {
            if n_config.horizon_url.is_none() && n_config.soroban_rpc_url.is_none() {
                bail!(
                    "Network '{}' in starforge.toml must specify at least 'horizon_url' or 'soroban_rpc_url'.",
                    n_name
                );
            }
        }

        for (d_name, d_config) in &self.deploy {
            if let Some(ref contracts) = d_config.contracts {
                for c in contracts {
                    if !self.contracts.contains_key(c) {
                        bail!(
                            "Deploy target '{}' references unknown contract '{}' not defined in [contracts].",
                            d_name,
                            c
                        );
                    }
                }
            }
        }

        for (s_name, s_cmd) in &self.scripts {
            if s_name.trim().is_empty() {
                bail!("Script name in starforge.toml cannot be empty.");
            }
            if s_cmd.trim().is_empty() {
                bail!(
                    "Script command for '{}' in starforge.toml cannot be empty.",
                    s_name
                );
            }
        }

        Ok(())
    }

    /// Resolve a contract's configured or toolchain-default WASM output path.
    pub fn wasm_path_for_contract(&self, name: &str, project_dir: &Path) -> Result<Option<PathBuf>> {
        let Some(contract) = self.contracts.get(name) else {
            return Ok(None);
        };
        if let Some(wasm) = &contract.wasm {
            return Ok(Some(project_dir.join(wasm)));
        }
        let target = crate::utils::wasm_target::resolve_target(self.wasm_target.as_deref())?;
        Ok(Some(crate::utils::wasm_target::artifact_path(
            project_dir,
            name,
            &target,
        )))
    }

    /// Generates the JSON Schema for `starforge.toml`.
    pub fn json_schema() -> serde_json::Value {
        serde_json::json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "title": "StarForge Project Manifest",
            "description": "Schema for starforge.toml project configuration",
            "type": "object",
            "required": ["version"],
            "properties": {
                "version": {
                    "type": "string",
                    "description": "Manifest schema version (must be \"1\")"
                },
                "package": {
                    "type": "object",
                    "description": "Package metadata",
                    "required": ["name"],
                    "properties": {
                        "name": { "type": "string", "description": "Project package name" },
                        "version": { "type": "string", "description": "Project version" },
                        "description": { "type": "string", "description": "Short description" },
                        "authors": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Package authors"
                        },
                        "license": { "type": "string", "description": "SPDX License identifier" }
                    },
                    "additionalProperties": false
                },
                "wasm_target": {
                    "type": "string",
                    "enum": [
                        crate::utils::wasm_target::LEGACY_WASM_TARGET,
                        crate::utils::wasm_target::V1_WASM_TARGET
                    ],
                    "description": "Optional Soroban WASM target override"
                },
                "contracts": {
                    "type": "object",
                    "description": "Smart contract target specifications",
                    "additionalProperties": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Path to contract source directory" },
                            "wasm": { "type": "string", "description": "Path to compiled WASM artifact" },
                            "init_args": { "description": "Contract constructor or init arguments" },
                            "build": { "type": "string", "description": "Contract build command" }
                        },
                        "additionalProperties": false
                    }
                },
                "networks": {
                    "type": "object",
                    "description": "Network definitions and overrides",
                    "additionalProperties": {
                        "type": "object",
                        "properties": {
                            "horizon_url": { "type": "string", "description": "Horizon RPC URL" },
                            "soroban_rpc_url": { "type": "string", "description": "Soroban RPC URL" },
                            "friendbot_url": { "type": "string", "description": "Friendbot funding URL" },
                            "network_passphrase": { "type": "string", "description": "Stellar network passphrase" }
                        },
                        "additionalProperties": false
                    }
                },
                "deploy": {
                    "type": "object",
                    "description": "Environment deployment targets",
                    "additionalProperties": {
                        "type": "object",
                        "properties": {
                            "network": { "type": "string", "description": "Target network name" },
                            "contracts": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "List of contracts to deploy"
                            },
                            "fee": { "type": "integer", "description": "Deployment transaction base fee" },
                            "source_wallet": { "type": "string", "description": "Deployment wallet name" },
                            "init_args": { "description": "Environment-specific init arguments" }
                        },
                        "additionalProperties": false
                    }
                },
                "scripts": {
                    "type": "object",
                    "description": "Custom task scripts",
                    "additionalProperties": {
                        "type": "string"
                    }
                }
            },
            "additionalProperties": false
        })
    }
}

/// Parses a [`ProjectManifest`] from a TOML string and validates it.
pub fn parse_manifest_str(contents: &str) -> Result<ProjectManifest> {
    let manifest: ProjectManifest =
        toml::from_str(contents).context("Invalid starforge.toml TOML syntax")?;
    manifest.validate()?;
    Ok(manifest)
}

/// Loads a [`ProjectManifest`] from a file path.
pub fn load_manifest(path: &Path) -> Result<ProjectManifest> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("Cannot read project manifest {}", path.display()))?;
    parse_manifest_str(&contents)
        .with_context(|| format!("Failed to parse project manifest {}", path.display()))
}

/// Searches `start` and its ancestor directories for `starforge.toml`.
pub fn discover_manifest(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(current) = dir {
        let candidate = current.join(MANIFEST_FILENAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = current.parent();
    }
    None
}

/// Discovers and loads `starforge.toml` starting from `start` directory walking upward.
pub fn find_and_load_manifest(start: &Path) -> Result<Option<(PathBuf, ProjectManifest)>> {
    match discover_manifest(start) {
        Some(path) => {
            let manifest = load_manifest(&path)?;
            Ok(Some((path, manifest)))
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_manifest_validates_cleanly() {
        let manifest = ProjectManifest::default_starter("my_contract");
        assert!(manifest.validate().is_ok());
        assert!(manifest
            .wasm_path_for_contract("my_contract", Path::new("project"))
            .unwrap()
            .unwrap()
            .to_string_lossy()
            .contains("/release/my_contract.wasm"));
    }

    #[test]
    fn manifest_target_override_controls_wasm_path() {
        let mut manifest = ProjectManifest::default_starter("my_contract");
        manifest.wasm_target = Some(crate::utils::wasm_target::LEGACY_WASM_TARGET.to_string());
        assert_eq!(
            manifest
                .wasm_path_for_contract("my_contract", Path::new("project"))
                .unwrap(),
            Some(
                PathBuf::from("project")
                    .join("target")
                    .join(crate::utils::wasm_target::LEGACY_WASM_TARGET)
                    .join("release")
                    .join("my_contract.wasm")
            )
        );
    }

    #[test]
    fn parse_and_validate_valid_toml() {
        let toml_data = r#"
version = "1"

[package]
name = "token_contract"
version = "0.1.0"

[contracts.token_contract]
path = "contracts/token"

[networks.testnet]
horizon_url = "https://horizon-testnet.stellar.org"

[deploy.testnet]
network = "testnet"
contracts = ["token_contract"]

[scripts]
build = "cargo build"
"#;
        let manifest = parse_manifest_str(toml_data).unwrap();
        assert_eq!(manifest.version, "1");
        assert_eq!(manifest.package.unwrap().name, "token_contract");
        assert!(manifest.contracts.contains_key("token_contract"));
    }

    #[test]
    fn invalid_version_fails_validation() {
        let toml_data = r#"
version = "99"

[package]
name = "foo"
"#;
        assert!(parse_manifest_str(toml_data).is_err());
    }

    #[test]
    fn invalid_contract_reference_in_deploy_fails() {
        let toml_data = r#"
version = "1"

[package]
name = "foo"

[deploy.testnet]
contracts = ["non_existent_contract"]
"#;
        assert!(parse_manifest_str(toml_data).is_err());
    }

    #[test]
    fn discovery_finds_manifest_upward() {
        let temp_dir = tempfile::tempdir().unwrap();
        let sub_dir = temp_dir.path().join("src").join("contracts");
        fs::create_dir_all(&sub_dir).unwrap();

        let manifest_path = temp_dir.path().join(MANIFEST_FILENAME);
        let starter = ProjectManifest::default_starter("test_proj");
        let toml_str = toml::to_string_pretty(&starter).unwrap();
        fs::write(&manifest_path, toml_str).unwrap();

        let found = discover_manifest(&sub_dir);
        assert_eq!(found, Some(manifest_path));
    }
}
