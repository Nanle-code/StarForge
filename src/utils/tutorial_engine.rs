use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const ONBOARDING_TUTORIAL_SLUG: &str = "onboarding-15-minute";
const ONBOARDING_TUTORIAL_JSON: &str =
    include_str!("../../tutorials/onboarding-15-minute/tutorial.json");

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TutorialStep {
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub checkpoint: Option<TutorialCheckpoint>,
    #[serde(default)]
    pub repair_hint: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TutorialCheckpoint {
    CliInstalled,
    WalletExists { name: String },
    ContractProjectExists { path: PathBuf },
    SimulationRuns,
}

pub fn verify_checkpoint(checkpoint: &TutorialCheckpoint, working_dir: &Path) -> Result<()> {
    match checkpoint {
        TutorialCheckpoint::CliInstalled => {
            let executable = std::env::current_exe()?;
            if !executable.is_file() {
                anyhow::bail!("StarForge executable was not found at {}", executable.display());
            }
        }
        TutorialCheckpoint::WalletExists { name } => {
            let config = crate::utils::config::load()?;
            if !config.wallets.iter().any(|wallet| wallet.name == *name) {
                anyhow::bail!("Wallet '{}' is not saved in the local StarForge configuration", name);
            }
        }
        TutorialCheckpoint::ContractProjectExists { path } => {
            let project_dir = working_dir.join(path);
            if !project_dir.join("Cargo.toml").is_file()
                || !project_dir.join("src/lib.rs").is_file()
            {
                anyhow::bail!(
                    "Contract project is incomplete at {} (expected Cargo.toml and src/lib.rs)",
                    project_dir.display()
                );
            }
        }
        TutorialCheckpoint::SimulationRuns => {
            use crate::utils::network_simulator::scenarios::{
                BuiltInScenario, ScenarioRunner,
            };

            let scenario = ScenarioRunner::load_built_in(BuiltInScenario::SimpleCounter, 42);
            let (_, result) = ScenarioRunner::run(scenario);
            if result.accounts.is_empty() || result.contracts.is_empty() {
                anyhow::bail!("The local simple-counter simulation returned no accounts or contracts");
            }
        }
    }
    Ok(())
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
    #[serde(default)]
    pub demo_mode: bool,
}

pub const DEMO_MODE_BANNER: &str = "[DEMO MODE: NON-PRODUCTION - OFFLINE STUBS ACTIVE]";

/// Check if demo/offline mode is currently active (via saved status or environment).
pub fn is_demo_mode_active() -> bool {
    if std::env::var("STARFORGE_DEMO_MODE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        return true;
    }
    load_status().map(|s| s.demo_mode).unwrap_or(false)
}

/// Guard against dangerous operations when operating in demo mode.
/// Rejects mainnet transactions, live funding, or real broadcast operations.
pub fn assert_demo_safe(operation: &str, network: Option<&str>) -> Result<()> {
    if !is_demo_mode_active() {
        return Ok(());
    }

    let net = network.unwrap_or("testnet").to_lowercase();
    if net == "mainnet" || net == "pubnet" {
        anyhow::bail!(
            "Dangerous operation '{}' blocked in demo mode: demo mode uses offline network stubs \
             and is strictly forbidden from touching live/mainnet networks.",
            operation
        );
    }

    let op_lower = operation.to_lowercase();
    if op_lower.contains("live_fund")
        || op_lower.contains("real_payment")
        || op_lower.contains("mainnet")
    {
        anyhow::bail!(
            "Dangerous operation '{}' blocked in demo mode: demo mode uses offline stubs \
             and cannot execute real on-chain fund transfers.",
            operation
        );
    }

    Ok(())
}

/// Deterministic mock keypair for offline demo/tutorial mode (non-funded).
pub fn mock_demo_wallet() -> (String, String) {
    (
        "GBDEMOTUTORIALOFFLINESTUBPUBLICKEYFORSTELLAR42".to_string(),
        "SDEMOTUTORIALOFFLINESTUBSECRETKEYFORSTELLAR42".to_string(),
    )
}

/// Deterministic mock simulation response JSON for offline tutorial execution.
pub fn mock_demo_simulation_json() -> &'static str {
    r#"{
        "latestLedger": 100000,
        "minResourceFee": "25000",
        "cost": {
            "cpuInsns": "450000",
            "memBytes": "256000"
        },
        "results": [{
            "auth": [],
            "xdr": "AAAAAQ=="
        }]
    }"#
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
    let mut definition: TutorialDefinition = if path.exists() {
        let bytes =
            fs::read(&path).with_context(|| format!("Failed to read {}", path.display()))?;
        serde_json::from_slice(&bytes)?
    } else if slug == ONBOARDING_TUTORIAL_SLUG {
        serde_json::from_str(ONBOARDING_TUTORIAL_JSON)?
    } else {
        anyhow::bail!(
            "Tutorial manifest missing at {}. Add tutorial.json for structured steps.",
            path.display()
        );
    };
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
    let mut slugs = Vec::new();
    if dir.exists() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            if entry.path().is_dir() {
                slugs.push(entry.file_name().to_string_lossy().to_string());
            }
        }
    }
    if !slugs.iter().any(|slug| slug == ONBOARDING_TUTORIAL_SLUG) {
        slugs.push(ONBOARDING_TUTORIAL_SLUG.to_string());
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
            checkpoint: None,
            repair_hint: None,
        };
        let rendered = render_step(&step, 0, 3);
        assert!(rendered.contains("Step 1/3"));
        assert!(rendered.contains("starforge info"));
    }

    #[test]
    fn old_tutorial_steps_default_checkpoint_fields() {
        let step: TutorialStep = serde_json::from_str(
            r#"{"title":"Old step","description":"Legacy","command":"starforge info"}"#,
        )
        .unwrap();

        assert!(step.checkpoint.is_none());
        assert!(step.repair_hint.is_none());
    }

    #[test]
    fn embedded_onboarding_is_listed_and_loads_outside_checkout() {
        let directory = tempfile::tempdir().unwrap();
        let slugs = list_tutorial_slugs(directory.path()).unwrap();
        assert!(slugs.iter().any(|slug| slug == ONBOARDING_TUTORIAL_SLUG));

        let tutorial = load_tutorial(directory.path(), ONBOARDING_TUTORIAL_SLUG).unwrap();
        assert_eq!(tutorial.steps.len(), 4);
    }

    #[test]
    fn cli_installed_checkpoint_accepts_running_executable() {
        assert!(verify_checkpoint(&TutorialCheckpoint::CliInstalled, Path::new(".")).is_ok());
    }

    #[test]
    fn contract_project_checkpoint_requires_expected_files() {
        let directory = tempfile::tempdir().unwrap();
        let checkpoint = TutorialCheckpoint::ContractProjectExists {
            path: PathBuf::from("contract"),
        };
        assert!(verify_checkpoint(&checkpoint, directory.path()).is_err());

        let project = directory.path().join("contract");
        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(project.join("Cargo.toml"), "[package]").unwrap();
        fs::write(project.join("src/lib.rs"), "").unwrap();
        assert!(verify_checkpoint(&checkpoint, directory.path()).is_ok());
    }

    #[test]
    fn local_simulation_checkpoint_runs_without_network() {
        assert!(verify_checkpoint(
            &TutorialCheckpoint::SimulationRuns,
            Path::new(".")
        )
        .is_ok());
    }

    #[test]
    fn demo_safe_guard_blocks_mainnet_and_dangerous_ops() {
        // Force demo mode active via env var
        std::env::set_var("STARFORGE_DEMO_MODE", "1");
        assert!(is_demo_mode_active());

        // Safe testnet operations pass
        assert!(assert_demo_safe("create_wallet", Some("testnet")).is_ok());
        assert!(assert_demo_safe("simulate_contract", Some("testnet")).is_ok());

        // Mainnet operations fail loudly
        let err = assert_demo_safe("deploy_contract", Some("mainnet")).unwrap_err();
        assert!(err.to_string().contains("blocked in demo mode"));
        assert!(err.to_string().contains("live/mainnet"));

        // Dangerous fund operations fail
        let err2 = assert_demo_safe("live_fund_real_money", Some("testnet")).unwrap_err();
        assert!(err2.to_string().contains("blocked in demo mode"));

        std::env::remove_var("STARFORGE_DEMO_MODE");
    }

    #[test]
    fn demo_stubs_provide_valid_mocks() {
        let (pubkey, secret) = mock_demo_wallet();
        assert!(pubkey.starts_with("GBDEMO"));
        assert!(secret.starts_with("SDEMO"));

        let raw = mock_demo_simulation_json();
        let val: serde_json::Value = serde_json::from_str(raw).unwrap();
        assert_eq!(val["minResourceFee"], "25000");
    }
}
