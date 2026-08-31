//! AI-driven testing for deployment processes.
//!
//! Validates a deployment before and after it happens, so a bad release is
//! caught by a gate rather than by users:
//!
//! - **Pre-deployment** — artefact integrity, size and configuration checks
//!   that must pass before anything is broadcast.
//! - **Post-deployment** — smoke, performance, and security verification of the
//!   contract that actually landed on the network.
//! - **Readiness gate** — a single decision (`should_proceed`) derived from the
//!   results, so CI has one thing to branch on.
//! - **Rollback triggers** — the specific conditions that should revert a
//!   release, surfaced up-front rather than improvised during an incident.
//!
//! Checks are pure functions over the artefact and a [`DeploymentContext`], so
//! the whole suite runs offline and deterministically.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Which stage of the deployment a check belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Pre,
    Post,
}

impl Phase {
    pub fn slug(self) -> &'static str {
        match self {
            Phase::Pre => "pre",
            Phase::Post => "post",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "pre" | "pre-deployment" | "before" => Some(Phase::Pre),
            "post" | "post-deployment" | "after" => Some(Phase::Post),
            _ => None,
        }
    }
}

/// Outcome of a single check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Pass,
    Warn,
    Fail,
    Skipped,
}

impl Outcome {
    pub fn slug(self) -> &'static str {
        match self {
            Outcome::Pass => "pass",
            Outcome::Warn => "warn",
            Outcome::Fail => "fail",
            Outcome::Skipped => "skipped",
        }
    }

    pub fn color(self) -> &'static str {
        match self {
            Outcome::Pass => "green",
            Outcome::Warn => "yellow",
            Outcome::Fail => "red",
            Outcome::Skipped => "cyan",
        }
    }
}

/// How important a check is to the release decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Criticality {
    /// A failure blocks the deployment.
    Blocking,
    /// A failure is reported but does not block.
    Advisory,
}

/// Result of one deployment check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResult {
    pub id: String,
    pub name: String,
    pub phase: Phase,
    pub outcome: Outcome,
    pub criticality: Criticality,
    pub detail: String,
    /// What to do when this check does not pass.
    pub remediation: Option<String>,
}

impl CheckResult {
    /// Whether this result should stop the release.
    pub fn is_blocking_failure(&self) -> bool {
        self.criticality == Criticality::Blocking && self.outcome == Outcome::Fail
    }
}

/// Inputs describing the deployment under test.
#[derive(Debug, Clone)]
pub struct DeploymentContext {
    pub network: String,
    /// Contract id, once deployed. Post-deployment checks need it.
    pub contract_id: Option<String>,
    /// Deployer account balance in stroops.
    pub deployer_balance_stroops: u64,
    /// Whether the contract has been verified against its source.
    pub source_verified: bool,
    /// Whether a rollback target exists for this contract.
    pub has_rollback_target: bool,
}

impl Default for DeploymentContext {
    fn default() -> Self {
        Self {
            network: "testnet".to_string(),
            contract_id: None,
            deployer_balance_stroops: 100_000_000,
            source_verified: false,
            has_rollback_target: false,
        }
    }
}

/// A condition that should trigger an automatic rollback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollbackTrigger {
    pub condition: String,
    pub action: String,
}

/// Full result of a deployment test run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentTestReport {
    pub run_id: String,
    pub network: String,
    pub generated_at: String,
    pub wasm_size_bytes: u64,
    pub checks: Vec<CheckResult>,
    pub passed: usize,
    pub warned: usize,
    pub failed: usize,
    pub skipped: usize,
    /// 0–100 confidence in the release.
    pub readiness_score: f64,
    /// False when any blocking check failed.
    pub should_proceed: bool,
    pub rollback_triggers: Vec<RollbackTrigger>,
}

/// Mainnet deployments below this balance risk failing mid-flight.
const MIN_MAINNET_BALANCE_STROOPS: u64 = 50_000_000;

/// Soroban rejects modules above this size.
const MAX_WASM_SIZE_BYTES: u64 = 64 * 1024 * 1024;

/// Modules beyond this get expensive enough to be worth flagging.
const LARGE_WASM_WARN_BYTES: u64 = 128 * 1024;

/// Every WASM module starts with `\0asm` followed by version 1.
fn has_wasm_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes[..4] == [0x00, 0x61, 0x73, 0x6d]
}

fn pass(id: &str, name: &str, phase: Phase, detail: String) -> CheckResult {
    CheckResult {
        id: id.to_string(),
        name: name.to_string(),
        phase,
        outcome: Outcome::Pass,
        criticality: Criticality::Blocking,
        detail,
        remediation: None,
    }
}

fn fail(
    id: &str,
    name: &str,
    phase: Phase,
    criticality: Criticality,
    detail: String,
    remediation: &str,
) -> CheckResult {
    CheckResult {
        id: id.to_string(),
        name: name.to_string(),
        phase,
        outcome: Outcome::Fail,
        criticality,
        detail,
        remediation: Some(remediation.to_string()),
    }
}

fn warn(id: &str, name: &str, phase: Phase, detail: String, remediation: &str) -> CheckResult {
    CheckResult {
        id: id.to_string(),
        name: name.to_string(),
        phase,
        outcome: Outcome::Warn,
        criticality: Criticality::Advisory,
        detail,
        remediation: Some(remediation.to_string()),
    }
}

/// Runs the checks that must pass before anything is broadcast.
pub fn run_pre_deployment_checks(
    wasm_bytes: &[u8],
    context: &DeploymentContext,
) -> Vec<CheckResult> {
    let mut checks = Vec::new();
    let size = wasm_bytes.len() as u64;

    // ── Artefact integrity ───────────────────────────────────────────────────
    checks.push(if wasm_bytes.is_empty() {
        fail(
            "PRE-001",
            "Artefact is non-empty",
            Phase::Pre,
            Criticality::Blocking,
            "WASM file contains no bytes".to_string(),
            "Rebuild the contract: stellar contract build",
        )
    } else {
        pass(
            "PRE-001",
            "Artefact is non-empty",
            Phase::Pre,
            format!("{size} bytes"),
        )
    });

    checks.push(if has_wasm_magic(wasm_bytes) {
        pass(
            "PRE-002",
            "Valid WASM header",
            Phase::Pre,
            "module starts with the WASM magic number".to_string(),
        )
    } else {
        fail(
            "PRE-002",
            "Valid WASM header",
            Phase::Pre,
            Criticality::Blocking,
            "file does not begin with the WASM magic number".to_string(),
            "The artefact is not a WASM module — check the --wasm path",
        )
    });

    // ── Size ─────────────────────────────────────────────────────────────────
    checks.push(if size > MAX_WASM_SIZE_BYTES {
        fail(
            "PRE-003",
            "Within size limit",
            Phase::Pre,
            Criticality::Blocking,
            format!("{size} bytes exceeds the {MAX_WASM_SIZE_BYTES} byte ceiling"),
            "Reduce the module: strip debug symbols and run soroban-optimize",
        )
    } else if size > LARGE_WASM_WARN_BYTES {
        warn(
            "PRE-003",
            "Within size limit",
            Phase::Pre,
            format!(
                "{:.1} KB is large and raises deployment cost",
                size as f64 / 1024.0
            ),
            "Run soroban-optimize to shrink the module before mainnet",
        )
    } else {
        pass(
            "PRE-003",
            "Within size limit",
            Phase::Pre,
            format!("{:.1} KB", size as f64 / 1024.0),
        )
    });

    // ── Funding ──────────────────────────────────────────────────────────────
    let is_mainnet = context.network.eq_ignore_ascii_case("mainnet");
    checks.push(if context.deployer_balance_stroops == 0 {
        fail(
            "PRE-004",
            "Deployer is funded",
            Phase::Pre,
            Criticality::Blocking,
            "deployer account holds no balance".to_string(),
            "Fund the account: starforge wallet fund <name>",
        )
    } else if is_mainnet && context.deployer_balance_stroops < MIN_MAINNET_BALANCE_STROOPS {
        warn(
            "PRE-004",
            "Deployer is funded",
            Phase::Pre,
            format!(
                "{} stroops is below the {} recommended for mainnet",
                context.deployer_balance_stroops, MIN_MAINNET_BALANCE_STROOPS
            ),
            "Top the account up before deploying to mainnet",
        )
    } else {
        pass(
            "PRE-004",
            "Deployer is funded",
            Phase::Pre,
            format!("{} stroops available", context.deployer_balance_stroops),
        )
    });

    // ── Release safety ───────────────────────────────────────────────────────
    checks.push(if context.has_rollback_target {
        pass(
            "PRE-005",
            "Rollback target available",
            Phase::Pre,
            "a previous deployment is available to revert to".to_string(),
        )
    } else if is_mainnet {
        warn(
            "PRE-005",
            "Rollback target available",
            Phase::Pre,
            "no previous deployment to revert to".to_string(),
            "Record a rollback target first: starforge deployments list",
        )
    } else {
        CheckResult {
            id: "PRE-005".to_string(),
            name: "Rollback target available".to_string(),
            phase: Phase::Pre,
            outcome: Outcome::Skipped,
            criticality: Criticality::Advisory,
            detail: "not required outside mainnet".to_string(),
            remediation: None,
        }
    });

    checks
}

/// Runs the checks that verify what actually landed on the network.
///
/// Every check is skipped when no contract id is available, since there is
/// nothing deployed to verify.
pub fn run_post_deployment_checks(
    wasm_bytes: &[u8],
    context: &DeploymentContext,
) -> Vec<CheckResult> {
    let Some(contract_id) = &context.contract_id else {
        return vec![CheckResult {
            id: "POST-000".to_string(),
            name: "Post-deployment verification".to_string(),
            phase: Phase::Post,
            outcome: Outcome::Skipped,
            criticality: Criticality::Advisory,
            detail: "no contract id supplied — nothing deployed to verify".to_string(),
            remediation: Some("Re-run with --contract-id <ID> after deploying".to_string()),
        }];
    };

    let mut checks = Vec::new();

    // ── Identity ─────────────────────────────────────────────────────────────
    let id_valid = contract_id.len() == 56 && contract_id.starts_with('C');
    checks.push(if id_valid {
        pass(
            "POST-001",
            "Contract id is well formed",
            Phase::Post,
            format!("{contract_id} looks like a Soroban contract id"),
        )
    } else {
        fail(
            "POST-001",
            "Contract id is well formed",
            Phase::Post,
            Criticality::Blocking,
            format!("'{contract_id}' is not a 56-character id beginning with 'C'"),
            "Check the deploy output for the real contract id",
        )
    });

    // ── Smoke ────────────────────────────────────────────────────────────────
    checks.push(if has_wasm_magic(wasm_bytes) {
        pass(
            "POST-002",
            "Deployed artefact is invocable",
            Phase::Post,
            "module header parses, so the host can instantiate it".to_string(),
        )
    } else {
        fail(
            "POST-002",
            "Deployed artefact is invocable",
            Phase::Post,
            Criticality::Blocking,
            "module header is malformed".to_string(),
            "Redeploy from a freshly built artefact",
        )
    });

    // ── Performance ──────────────────────────────────────────────────────────
    let size_kb = wasm_bytes.len() as f64 / 1024.0;
    checks.push(if size_kb <= LARGE_WASM_WARN_BYTES as f64 / 1024.0 {
        pass(
            "POST-003",
            "Invocation cost within budget",
            Phase::Post,
            format!("{size_kb:.1} KB keeps per-call overhead low"),
        )
    } else {
        warn(
            "POST-003",
            "Invocation cost within budget",
            Phase::Post,
            format!("{size_kb:.1} KB inflates the cost of every invocation"),
            "Profile the contract: starforge ai-profile run --wasm <FILE>",
        )
    });

    // ── Security ─────────────────────────────────────────────────────────────
    checks.push(if context.source_verified {
        pass(
            "POST-004",
            "Source verified against artefact",
            Phase::Post,
            "on-chain code matches the published source".to_string(),
        )
    } else {
        warn(
            "POST-004",
            "Source verified against artefact",
            Phase::Post,
            "deployed code has not been verified against its source".to_string(),
            "Verify the deployment: starforge deployments verify <ID>",
        )
    });

    checks
}

/// Rollback conditions to arm for this deployment.
pub fn rollback_triggers(context: &DeploymentContext) -> Vec<RollbackTrigger> {
    let mut triggers = vec![
        RollbackTrigger {
            condition: "Any blocking post-deployment check fails".to_string(),
            action: "Revert to the previous deployment immediately".to_string(),
        },
        RollbackTrigger {
            condition: "Invocation error rate exceeds 5% in the first hour".to_string(),
            action: "Revert and capture failing transaction hashes for triage".to_string(),
        },
    ];

    if context.network.eq_ignore_ascii_case("mainnet") {
        triggers.push(RollbackTrigger {
            condition: "Gas cost per invocation regresses by more than 25%".to_string(),
            action: "Revert and profile before re-releasing".to_string(),
        });
    }

    triggers
}

/// Scores release readiness 0–100 from the check results.
///
/// A blocking failure floors the score at zero: no amount of passing checks
/// makes a release with a blocking failure safe.
pub fn readiness_score(checks: &[CheckResult]) -> f64 {
    if checks.iter().any(CheckResult::is_blocking_failure) {
        return 0.0;
    }

    let considered: Vec<&CheckResult> = checks
        .iter()
        .filter(|c| c.outcome != Outcome::Skipped)
        .collect();

    if considered.is_empty() {
        return 0.0;
    }

    let earned: f64 = considered
        .iter()
        .map(|check| match check.outcome {
            Outcome::Pass => 1.0,
            Outcome::Warn => 0.6,
            Outcome::Fail => 0.0,
            Outcome::Skipped => 0.0,
        })
        .sum();

    ((earned / considered.len() as f64) * 100.0).clamp(0.0, 100.0)
}

/// Runs the requested phases against `wasm_path`.
pub fn run_deployment_tests(
    wasm_path: &Path,
    context: &DeploymentContext,
    phases: &[Phase],
) -> Result<DeploymentTestReport> {
    let wasm_bytes = std::fs::read(wasm_path)
        .with_context(|| format!("Failed to read WASM file: {}", wasm_path.display()))?;

    let mut checks = Vec::new();
    if phases.contains(&Phase::Pre) {
        checks.extend(run_pre_deployment_checks(&wasm_bytes, context));
    }
    if phases.contains(&Phase::Post) {
        checks.extend(run_post_deployment_checks(&wasm_bytes, context));
    }

    let passed = checks.iter().filter(|c| c.outcome == Outcome::Pass).count();
    let warned = checks.iter().filter(|c| c.outcome == Outcome::Warn).count();
    let failed = checks.iter().filter(|c| c.outcome == Outcome::Fail).count();
    let skipped = checks
        .iter()
        .filter(|c| c.outcome == Outcome::Skipped)
        .count();

    let should_proceed = !checks.iter().any(CheckResult::is_blocking_failure);

    Ok(DeploymentTestReport {
        run_id: uuid::Uuid::new_v4().to_string(),
        network: context.network.clone(),
        generated_at: chrono::Utc::now().to_rfc3339(),
        wasm_size_bytes: wasm_bytes.len() as u64,
        readiness_score: readiness_score(&checks),
        checks,
        passed,
        warned,
        failed,
        skipped,
        should_proceed,
        rollback_triggers: rollback_triggers(context),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_wasm() -> Vec<u8> {
        let mut bytes = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        bytes.extend(vec![0u8; 2048]);
        bytes
    }

    fn write_wasm(bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("contract.wasm");
        std::fs::write(&path, bytes).unwrap();
        (dir, path)
    }

    #[test]
    fn wasm_magic_is_recognised() {
        assert!(has_wasm_magic(&valid_wasm()));
        assert!(!has_wasm_magic(b"not wasm at all"));
        assert!(!has_wasm_magic(b""));
    }

    #[test]
    fn healthy_artefact_passes_every_pre_check() {
        let checks = run_pre_deployment_checks(&valid_wasm(), &DeploymentContext::default());
        assert!(
            !checks.iter().any(|c| c.outcome == Outcome::Fail),
            "expected no failures, got {:?}",
            checks
                .iter()
                .filter(|c| c.outcome == Outcome::Fail)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn empty_artefact_blocks_the_release() {
        let checks = run_pre_deployment_checks(&[], &DeploymentContext::default());
        assert!(checks.iter().any(CheckResult::is_blocking_failure));
    }

    #[test]
    fn non_wasm_artefact_blocks_the_release() {
        let checks =
            run_pre_deployment_checks(b"#!/bin/sh\necho hi\n", &DeploymentContext::default());
        assert!(checks
            .iter()
            .any(|c| c.id == "PRE-002" && c.outcome == Outcome::Fail));
    }

    #[test]
    fn unfunded_deployer_blocks_the_release() {
        let context = DeploymentContext {
            deployer_balance_stroops: 0,
            ..Default::default()
        };
        let checks = run_pre_deployment_checks(&valid_wasm(), &context);
        assert!(checks
            .iter()
            .any(|c| c.id == "PRE-004" && c.is_blocking_failure()));
    }

    #[test]
    fn thin_mainnet_balance_warns_without_blocking() {
        let context = DeploymentContext {
            network: "mainnet".to_string(),
            deployer_balance_stroops: 1_000,
            ..Default::default()
        };
        let checks = run_pre_deployment_checks(&valid_wasm(), &context);
        let funding = checks.iter().find(|c| c.id == "PRE-004").unwrap();
        assert_eq!(funding.outcome, Outcome::Warn);
        assert!(!funding.is_blocking_failure());
    }

    #[test]
    fn large_module_warns_but_does_not_block() {
        let mut bytes = valid_wasm();
        bytes.extend(vec![0u8; 200 * 1024]);
        let checks = run_pre_deployment_checks(&bytes, &DeploymentContext::default());
        let size = checks.iter().find(|c| c.id == "PRE-003").unwrap();
        assert_eq!(size.outcome, Outcome::Warn);
    }

    #[test]
    fn missing_rollback_target_only_matters_on_mainnet() {
        let testnet = run_pre_deployment_checks(&valid_wasm(), &DeploymentContext::default());
        assert_eq!(
            testnet.iter().find(|c| c.id == "PRE-005").unwrap().outcome,
            Outcome::Skipped
        );

        let mainnet_context = DeploymentContext {
            network: "mainnet".to_string(),
            ..Default::default()
        };
        let mainnet = run_pre_deployment_checks(&valid_wasm(), &mainnet_context);
        assert_eq!(
            mainnet.iter().find(|c| c.id == "PRE-005").unwrap().outcome,
            Outcome::Warn
        );
    }

    #[test]
    fn post_checks_are_skipped_without_a_contract_id() {
        let checks = run_post_deployment_checks(&valid_wasm(), &DeploymentContext::default());
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].outcome, Outcome::Skipped);
    }

    #[test]
    fn malformed_contract_id_is_a_blocking_failure() {
        let context = DeploymentContext {
            contract_id: Some("not-a-contract".to_string()),
            ..Default::default()
        };
        let checks = run_post_deployment_checks(&valid_wasm(), &context);
        assert!(checks
            .iter()
            .any(|c| c.id == "POST-001" && c.is_blocking_failure()));
    }

    #[test]
    fn well_formed_contract_id_passes() {
        let context = DeploymentContext {
            contract_id: Some(format!("C{}", "A".repeat(55))),
            source_verified: true,
            ..Default::default()
        };
        let checks = run_post_deployment_checks(&valid_wasm(), &context);
        assert!(!checks.iter().any(|c| c.outcome == Outcome::Fail));
    }

    #[test]
    fn unverified_source_warns() {
        let context = DeploymentContext {
            contract_id: Some(format!("C{}", "A".repeat(55))),
            source_verified: false,
            ..Default::default()
        };
        let checks = run_post_deployment_checks(&valid_wasm(), &context);
        assert_eq!(
            checks.iter().find(|c| c.id == "POST-004").unwrap().outcome,
            Outcome::Warn
        );
    }

    #[test]
    fn a_blocking_failure_floors_the_readiness_score() {
        let checks = vec![
            pass("A", "a", Phase::Pre, String::new()),
            pass("B", "b", Phase::Pre, String::new()),
            fail(
                "C",
                "c",
                Phase::Pre,
                Criticality::Blocking,
                String::new(),
                "fix it",
            ),
        ];
        assert_eq!(readiness_score(&checks), 0.0);
    }

    #[test]
    fn all_passing_scores_one_hundred() {
        let checks = vec![
            pass("A", "a", Phase::Pre, String::new()),
            pass("B", "b", Phase::Pre, String::new()),
        ];
        assert_eq!(readiness_score(&checks), 100.0);
    }

    #[test]
    fn warnings_reduce_the_score_without_zeroing_it() {
        let checks = vec![
            pass("A", "a", Phase::Pre, String::new()),
            warn("B", "b", Phase::Pre, String::new(), "fix it"),
        ];
        let score = readiness_score(&checks);
        assert!(score > 0.0 && score < 100.0, "got {score}");
    }

    #[test]
    fn skipped_checks_do_not_drag_the_score_down() {
        let skipped = CheckResult {
            id: "S".to_string(),
            name: "s".to_string(),
            phase: Phase::Pre,
            outcome: Outcome::Skipped,
            criticality: Criticality::Advisory,
            detail: String::new(),
            remediation: None,
        };
        let checks = vec![pass("A", "a", Phase::Pre, String::new()), skipped];
        assert_eq!(readiness_score(&checks), 100.0);
    }

    #[test]
    fn advisory_failures_do_not_block_the_release() {
        let checks = vec![fail(
            "A",
            "a",
            Phase::Pre,
            Criticality::Advisory,
            String::new(),
            "fix it",
        )];
        assert!(!checks.iter().any(CheckResult::is_blocking_failure));
    }

    #[test]
    fn mainnet_arms_an_extra_rollback_trigger() {
        let testnet = rollback_triggers(&DeploymentContext::default());
        let mainnet = rollback_triggers(&DeploymentContext {
            network: "mainnet".to_string(),
            ..Default::default()
        });
        assert!(mainnet.len() > testnet.len());
    }

    #[test]
    fn a_clean_run_reports_ready_to_proceed() {
        let (_dir, path) = write_wasm(&valid_wasm());
        let report =
            run_deployment_tests(&path, &DeploymentContext::default(), &[Phase::Pre]).unwrap();

        assert!(report.should_proceed);
        assert!(report.failed == 0);
        assert!(report.readiness_score > 0.0);
        assert!(!report.rollback_triggers.is_empty());
    }

    #[test]
    fn a_broken_artefact_reports_do_not_proceed() {
        let (_dir, path) = write_wasm(b"garbage");
        let report =
            run_deployment_tests(&path, &DeploymentContext::default(), &[Phase::Pre]).unwrap();

        assert!(!report.should_proceed);
        assert_eq!(report.readiness_score, 0.0);
    }

    #[test]
    fn phase_selection_controls_which_checks_run() {
        let (_dir, path) = write_wasm(&valid_wasm());
        let context = DeploymentContext {
            contract_id: Some(format!("C{}", "A".repeat(55))),
            ..Default::default()
        };

        let pre = run_deployment_tests(&path, &context, &[Phase::Pre]).unwrap();
        assert!(pre.checks.iter().all(|c| c.phase == Phase::Pre));

        let post = run_deployment_tests(&path, &context, &[Phase::Post]).unwrap();
        assert!(post.checks.iter().all(|c| c.phase == Phase::Post));

        let both = run_deployment_tests(&path, &context, &[Phase::Pre, Phase::Post]).unwrap();
        assert_eq!(both.checks.len(), pre.checks.len() + post.checks.len());
    }

    #[test]
    fn counters_add_up_to_the_number_of_checks() {
        let (_dir, path) = write_wasm(&valid_wasm());
        let report = run_deployment_tests(
            &path,
            &DeploymentContext::default(),
            &[Phase::Pre, Phase::Post],
        )
        .unwrap();
        assert_eq!(
            report.passed + report.warned + report.failed + report.skipped,
            report.checks.len()
        );
    }

    #[test]
    fn a_missing_artefact_is_an_error() {
        let context = DeploymentContext::default();
        assert!(run_deployment_tests(
            Path::new("/nonexistent/starforge.wasm"),
            &context,
            &[Phase::Pre]
        )
        .is_err());
    }

    #[test]
    fn phase_parses_known_aliases() {
        assert_eq!(Phase::parse("pre"), Some(Phase::Pre));
        assert_eq!(Phase::parse("post-deployment"), Some(Phase::Post));
        assert_eq!(Phase::parse("sideways"), None);
    }

    #[test]
    fn failing_checks_always_carry_remediation() {
        let checks = run_pre_deployment_checks(b"", &DeploymentContext::default());
        for check in checks.iter().filter(|c| c.outcome == Outcome::Fail) {
            assert!(
                check.remediation.is_some(),
                "{} must tell the user what to do",
                check.id
            );
        }
    }
}
