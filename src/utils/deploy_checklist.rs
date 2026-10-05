//! Pre-mainnet deployment checklist evaluation and gating.

use crate::utils::{
    config::{self, WalletEntry},
    hardware_wallet::HardwareWalletKind,
    horizon, soroban, wasm_hash,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const CHECK_WASM_HASH: &str = "wasm_hash_reproducibility";
pub const CHECK_SIMULATION: &str = "simulation_success";
pub const CHECK_BALANCE: &str = "balance";
pub const CHECK_AUTH: &str = "auth_setup";
pub const CHECK_NETWORK_IDENTITY: &str = "network_identity";

/// Project-level settings for the pre-mainnet checklist. A reproducible
/// build's reviewed SHA-256 must be pinned before the hash check can pass.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct DeploymentChecklistConfig {
    pub required_checks: Vec<String>,
    pub expected_wasm_hash: Option<String>,
    pub minimum_balance_xlm: f64,
}

impl Default for DeploymentChecklistConfig {
    fn default() -> Self {
        Self {
            required_checks: vec![
                CHECK_WASM_HASH.to_string(),
                CHECK_SIMULATION.to_string(),
                CHECK_BALANCE.to_string(),
                CHECK_AUTH.to_string(),
                CHECK_NETWORK_IDENTITY.to_string(),
            ],
            expected_wasm_hash: None,
            minimum_balance_xlm: 1.0,
        }
    }
}

impl DeploymentChecklistConfig {
    pub fn validate(&self) -> Result<()> {
        if self.required_checks.is_empty() {
            anyhow::bail!("at least one deployment checklist check must be required");
        }
        let known = [
            CHECK_WASM_HASH,
            CHECK_SIMULATION,
            CHECK_BALANCE,
            CHECK_AUTH,
            CHECK_NETWORK_IDENTITY,
        ];
        let mut seen = std::collections::HashSet::new();
        for check_id in &self.required_checks {
            if !known.contains(&check_id.as_str()) {
                anyhow::bail!("unknown deployment checklist check '{check_id}'");
            }
            if !seen.insert(check_id) {
                anyhow::bail!("duplicate deployment checklist check '{check_id}'");
            }
        }
        if !self.minimum_balance_xlm.is_finite() || self.minimum_balance_xlm < 0.0 {
            anyhow::bail!(
                "deployment checklist minimum_balance_xlm must be finite and non-negative"
            );
        }
        if let Some(hash) = &self.expected_wasm_hash {
            if hash.len() != 64 || !hash.chars().all(|character| character.is_ascii_hexdigit()) {
                anyhow::bail!(
                    "deployment checklist expected_wasm_hash must be 64 hexadecimal characters"
                );
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChecklistCheck {
    pub id: String,
    pub name: String,
    pub required: bool,
    pub passed: bool,
    pub detail: String,
    pub remediation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChecklistReport {
    pub network: String,
    pub wallet: String,
    pub wasm: String,
    pub wasm_hash: Option<String>,
    pub passed: bool,
    pub checks: Vec<ChecklistCheck>,
}

impl ChecklistReport {
    pub fn new(
        network: &str,
        wallet: &str,
        wasm: &Path,
        wasm_hash: Option<String>,
        checks: Vec<ChecklistCheck>,
    ) -> Self {
        let passed = checks.iter().all(|check| !check.required || check.passed);
        Self {
            network: network.to_string(),
            wallet: wallet.to_string(),
            wasm: wasm.display().to_string(),
            wasm_hash,
            passed,
            checks,
        }
    }

    pub fn failed_required_checks(&self) -> Vec<&ChecklistCheck> {
        self.checks
            .iter()
            .filter(|check| check.required && !check.passed)
            .collect()
    }
}

fn check(
    id: &str,
    name: &str,
    config: &DeploymentChecklistConfig,
    passed: bool,
    detail: impl Into<String>,
    remediation: &str,
) -> ChecklistCheck {
    ChecklistCheck {
        id: id.to_string(),
        name: name.to_string(),
        required: config.required_checks.iter().any(|required| required == id),
        passed,
        detail: detail.into(),
        remediation: (!passed).then(|| remediation.to_string()),
    }
}

fn reproducible_hash(bytes: &[u8], expected: Option<&str>) -> Result<String> {
    let actual = wasm_hash::compute_wasm_hash(bytes, wasm_hash::BuildEnvironment::current())?;
    let expected = expected.ok_or_else(|| {
        anyhow::anyhow!("No expected_wasm_hash is pinned in starforge-project.toml")
    })?;
    if !actual.eq_ignore_ascii_case(expected) {
        anyhow::bail!("WASM hash {actual} does not match the pinned reproducible hash {expected}");
    }
    Ok(actual)
}

fn auth_setup(
    wallet: &WalletEntry,
    network: &str,
    hardware: Option<HardwareWalletKind>,
) -> Result<String> {
    if !wallet.usage_policy.allowed_networks.is_empty()
        && !wallet
            .usage_policy
            .allowed_networks
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(network))
    {
        anyhow::bail!("Wallet policy does not allow signing on {network}");
    }

    match hardware {
        Some(device) => {
            #[cfg(feature = "hardware-wallet")]
            {
                crate::utils::hardware_wallet::device_status(device)
                    .with_context(|| format!("{device} signer is not ready"))?;
                Ok(format!("{device} signer is connected and ready"))
            }
            #[cfg(not(feature = "hardware-wallet"))]
            {
                let _ = device;
                anyhow::bail!("hardware-wallet support is disabled in this build")
            }
        }
        None if wallet.secret_key.is_some() => Ok(format!(
            "wallet '{}' has a local signing key configured",
            wallet.name
        )),
        None => anyhow::bail!(
            "wallet '{}' is watch-only and no hardware signer was selected",
            wallet.name
        ),
    }
}

/// Run all five checks using the same configured endpoints and signer as deploy.
pub async fn run(
    wasm_path: &Path,
    network: &str,
    wallet: &WalletEntry,
    hardware: Option<HardwareWalletKind>,
    policy: &DeploymentChecklistConfig,
) -> Result<ChecklistReport> {
    policy.validate()?;
    let bytes = std::fs::read(wasm_path)
        .with_context(|| format!("Failed to read WASM file {}", wasm_path.display()))?;
    let wasm_hash =
        wasm_hash::compute_wasm_hash(&bytes, wasm_hash::BuildEnvironment::current()).ok();
    let mut checks = Vec::with_capacity(5);

    match reproducible_hash(&bytes, policy.expected_wasm_hash.as_deref()) {
        Ok(hash) => checks.push(check(
            CHECK_WASM_HASH,
            "WASM hash reproducibility",
            policy,
            true,
            format!("SHA-256 {hash} matches the reviewed reproducible-build pin"),
            "Build reproducibly and update deployment_checklist.expected_wasm_hash after review.",
        )),
        Err(error) => checks.push(check(
            CHECK_WASM_HASH,
            "WASM hash reproducibility",
            policy,
            false,
            error.to_string(),
            "Build reproducibly and pin its SHA-256 in starforge-project.toml.",
        )),
    }

    match wasm_hash.as_deref() {
        Some(hash) => match soroban::simulate_deploy_transaction(hash, network, wallet).await {
            Ok(simulation) if simulation.errors.is_empty() => checks.push(check(
                CHECK_SIMULATION,
                "Soroban deploy simulation",
                policy,
                true,
                format!(
                    "Simulation succeeded; minimum resource fee {} stroops",
                    simulation.fee
                ),
                "Resolve the RPC simulation errors before deploying.",
            )),
            Ok(simulation) => checks.push(check(
                CHECK_SIMULATION,
                "Soroban deploy simulation",
                policy,
                false,
                simulation.errors.join("; "),
                "Resolve the RPC simulation errors before deploying.",
            )),
            Err(error) => checks.push(check(
                CHECK_SIMULATION,
                "Soroban deploy simulation",
                policy,
                false,
                error.to_string(),
                "Verify Soroban RPC connectivity and retry the simulation.",
            )),
        },
        None => checks.push(check(
            CHECK_SIMULATION,
            "Soroban deploy simulation",
            policy,
            false,
            "Cannot simulate because the WASM hash could not be computed".to_string(),
            "Provide a valid WASM module and retry.",
        )),
    }

    match horizon::fetch_account(&wallet.public_key, network).await {
        Ok(account) => {
            let balance = account
                .balances
                .iter()
                .find(|balance| balance.asset_type == "native")
                .and_then(|balance| balance.balance.parse::<f64>().ok());
            match balance {
                Some(balance) if balance.is_finite() && balance >= policy.minimum_balance_xlm => {
                    checks.push(check(
                        CHECK_BALANCE,
                        "Deployer balance",
                        policy,
                        true,
                        format!(
                            "{balance:.7} XLM available (minimum {:.7} XLM)",
                            policy.minimum_balance_xlm
                        ),
                        "Fund the deployer account or adjust the project minimum after review.",
                    ));
                }
                Some(balance) => checks.push(check(
                    CHECK_BALANCE,
                    "Deployer balance",
                    policy,
                    false,
                    format!(
                        "{balance:.7} XLM is below the {:.7} XLM minimum",
                        policy.minimum_balance_xlm
                    ),
                    "Fund the deployer account or adjust the project minimum after review.",
                )),
                None => checks.push(check(
                    CHECK_BALANCE,
                    "Deployer balance",
                    policy,
                    false,
                    "Horizon account response has no parseable native XLM balance".to_string(),
                    "Verify the account and Horizon response, then retry.",
                )),
            }
        }
        Err(error) => checks.push(check(
            CHECK_BALANCE,
            "Deployer balance",
            policy,
            false,
            error.to_string(),
            "Verify the account is funded and Horizon is reachable.",
        )),
    }

    match auth_setup(wallet, network, hardware) {
        Ok(detail) => checks.push(check(
            CHECK_AUTH,
            "Authorization setup",
            policy,
            true,
            detail,
            "Configure a local signing key or select a supported, connected hardware signer.",
        )),
        Err(error) => checks.push(check(
            CHECK_AUTH,
            "Authorization setup",
            policy,
            false,
            error.to_string(),
            "Configure a permitted signing method for this wallet and network.",
        )),
    }

    let configured_identity = config::get_network_passphrase(network);
    match horizon::fetch_network_passphrase(network).await {
        Ok(observed) if observed == configured_identity => checks.push(check(
            CHECK_NETWORK_IDENTITY,
            "Network identity",
            policy,
            true,
            format!("Horizon identity matches configured network '{network}'"),
            "Correct the network endpoint or passphrase before deploying.",
        )),
        Ok(_) => checks.push(check(
            CHECK_NETWORK_IDENTITY,
            "Network identity",
            policy,
            false,
            format!("Horizon identity does not match the configured passphrase for '{network}'"),
            "Correct the network endpoint or passphrase before deploying.",
        )),
        Err(error) => checks.push(check(
            CHECK_NETWORK_IDENTITY,
            "Network identity",
            policy,
            false,
            error.to_string(),
            "Verify Horizon connectivity and the selected network configuration.",
        )),
    }

    Ok(ChecklistReport::new(
        network,
        &wallet.name,
        wasm_path,
        wasm_hash,
        checks,
    ))
}

/// Stop a mainnet deploy when any configured required check failed unless the
/// operator explicitly acknowledges the failures with the override flag.
pub fn enforce_mainnet_gate(
    network: &str,
    report: &ChecklistReport,
    override_failures: bool,
) -> Result<()> {
    let failures = report.failed_required_checks();
    if network.eq_ignore_ascii_case("mainnet") && !failures.is_empty() && !override_failures {
        anyhow::bail!(
            "Mainnet deployment blocked by {} required checklist check(s): {}. Review `starforge deploy checklist --wasm <FILE>` or explicitly acknowledge with --override-checklist.",
            failures.len(),
            failures.iter().map(|check| check.id.as_str()).collect::<Vec<_>>().join(", ")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(required_check_passed: bool) -> ChecklistReport {
        ChecklistReport::new(
            "mainnet",
            "deployer",
            Path::new("contract.wasm"),
            Some("a".repeat(64)),
            vec![ChecklistCheck {
                id: CHECK_SIMULATION.to_string(),
                name: "Simulation".to_string(),
                required: true,
                passed: required_check_passed,
                detail: String::new(),
                remediation: None,
            }],
        )
    }

    #[test]
    fn reproducible_hash_requires_the_reviewed_pin() {
        let bytes = b"\0asm\x01\0\0\0";
        let expected =
            wasm_hash::compute_wasm_hash(bytes, wasm_hash::BuildEnvironment::Linux).unwrap();
        assert_eq!(reproducible_hash(bytes, Some(&expected)).unwrap(), expected);
        assert!(reproducible_hash(bytes, None).is_err());
        assert!(reproducible_hash(bytes, Some(&"0".repeat(64))).is_err());
    }

    #[test]
    fn all_required_checks_passing_allows_mainnet() {
        assert!(enforce_mainnet_gate("mainnet", &report(true), false).is_ok());
    }

    #[test]
    fn failed_required_check_blocks_mainnet_without_override() {
        let result = enforce_mainnet_gate("mainnet", &report(false), false);
        assert!(result.unwrap_err().to_string().contains("blocked"));
    }

    #[test]
    fn explicit_override_allows_mainnet_with_failed_required_check() {
        assert!(enforce_mainnet_gate("mainnet", &report(false), true).is_ok());
    }

    #[test]
    fn failed_required_check_does_not_gate_testnet() {
        assert!(enforce_mainnet_gate("testnet", &report(false), false).is_ok());
    }

    #[test]
    fn failed_advisory_check_does_not_gate_mainnet() {
        let advisory_report = ChecklistReport::new(
            "mainnet",
            "deployer",
            Path::new("contract.wasm"),
            None,
            vec![ChecklistCheck {
                id: CHECK_SIMULATION.to_string(),
                name: "Simulation".to_string(),
                required: false,
                passed: false,
                detail: "advisory only".to_string(),
                remediation: None,
            }],
        );
        assert!(advisory_report.passed);
        assert!(enforce_mainnet_gate("mainnet", &advisory_report, false).is_ok());
    }

    #[test]
    fn checklist_report_serializes_for_json_output() {
        let json = serde_json::to_value(report(false)).unwrap();
        assert_eq!(json["passed"], false);
        assert_eq!(json["checks"][0]["id"], CHECK_SIMULATION);
    }
}
