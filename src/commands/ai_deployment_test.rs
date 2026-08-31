//! AI-driven deployment testing commands.
//!
//! Exposes [`crate::utils::ai_deployment_testing`] through
//! `starforge ai-deployment-test …`.

use crate::utils::ai_deployment_testing::{
    rollback_triggers, run_deployment_tests, DeploymentContext, DeploymentTestReport, Outcome,
    Phase,
};
use crate::utils::print as p;
use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum AiDeploymentTestCommands {
    /// Run deployment tests before, after, or around a deployment
    Run {
        /// Path to the compiled WASM file
        #[arg(long, value_name = "FILE")]
        wasm: PathBuf,

        /// Phases to run: pre, post, or all
        #[arg(long, default_value = "all")]
        phase: String,

        /// Target network
        #[arg(long, default_value = "testnet")]
        network: String,

        /// Deployed contract id, required for post-deployment checks
        #[arg(long)]
        contract_id: Option<String>,

        /// Deployer balance in stroops, used by the funding check
        #[arg(long, default_value = "100000000")]
        balance: u64,

        /// Mark the deployment as source-verified
        #[arg(long)]
        verified: bool,

        /// Declare that a rollback target exists
        #[arg(long)]
        rollback_target: bool,

        /// Exit non-zero when the release should not proceed
        #[arg(long)]
        gate: bool,

        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },

    /// Show the rollback conditions armed for a network
    Triggers {
        /// Target network
        #[arg(long, default_value = "testnet")]
        network: String,

        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

pub async fn handle(cmd: AiDeploymentTestCommands) -> Result<()> {
    match cmd {
        AiDeploymentTestCommands::Run {
            wasm,
            phase,
            network,
            contract_id,
            balance,
            verified,
            rollback_target,
            gate,
            json,
        } => handle_run(
            wasm,
            phase,
            network,
            contract_id,
            balance,
            verified,
            rollback_target,
            gate,
            json,
        ),
        AiDeploymentTestCommands::Triggers { network, json } => handle_triggers(network, json),
    }
}

/// Resolves the `--phase` flag into the phases to execute.
fn resolve_phases(value: &str) -> Result<Vec<Phase>> {
    if value.trim().eq_ignore_ascii_case("all") {
        return Ok(vec![Phase::Pre, Phase::Post]);
    }

    let phases: Vec<Phase> = value.split(',').filter_map(Phase::parse).collect();

    if phases.is_empty() {
        anyhow::bail!("Unknown phase '{}'. Use pre, post, or all", value);
    }

    Ok(phases)
}

#[allow(clippy::too_many_arguments)]
fn handle_run(
    wasm: PathBuf,
    phase: String,
    network: String,
    contract_id: Option<String>,
    balance: u64,
    verified: bool,
    rollback_target: bool,
    gate: bool,
    json: bool,
) -> Result<()> {
    let phases = resolve_phases(&phase)?;

    let context = DeploymentContext {
        network: network.clone(),
        contract_id,
        deployer_balance_stroops: balance,
        source_verified: verified,
        has_rollback_target: rollback_target,
    };

    if !json {
        p::header("AI Deployment Testing");
        p::separator();
        p::kv("Artefact", &wasm.display().to_string());
        p::kv("Network", &network);
        p::kv(
            "Phases",
            &phases
                .iter()
                .map(|p| p.slug())
                .collect::<Vec<_>>()
                .join(", "),
        );
        println!();
    }

    let report = run_deployment_tests(&wasm, &context, &phases)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_report(&report);
    }

    if gate && !report.should_proceed {
        anyhow::bail!(
            "deployment gate failed: {} blocking check(s) did not pass",
            report
                .checks
                .iter()
                .filter(|c| c.is_blocking_failure())
                .count()
        );
    }

    Ok(())
}

fn print_report(report: &DeploymentTestReport) {
    for phase in [Phase::Pre, Phase::Post] {
        let checks: Vec<_> = report.checks.iter().filter(|c| c.phase == phase).collect();
        if checks.is_empty() {
            continue;
        }

        p::header(match phase {
            Phase::Pre => "Pre-deployment checks",
            Phase::Post => "Post-deployment checks",
        });

        for check in checks {
            println!(
                "  [{}] {} — {}",
                check
                    .outcome
                    .slug()
                    .to_uppercase()
                    .color(check.outcome.color()),
                check.id,
                check.name.bold()
            );
            println!("      {}", check.detail);
            if let Some(remediation) = &check.remediation {
                println!("      → {remediation}");
            }
        }
        println!();
    }

    p::header("Summary");
    p::kv("Passed", &report.passed.to_string());
    p::kv("Warnings", &report.warned.to_string());
    p::kv("Failures", &report.failed.to_string());
    p::kv("Skipped", &report.skipped.to_string());
    p::kv_accent("Readiness", &format!("{:.1}/100", report.readiness_score));
    println!();

    if report.should_proceed {
        p::success("Deployment may proceed");
    } else {
        p::error("Deployment blocked — resolve the failing checks above");
    }
    println!();

    p::header("Armed rollback triggers");
    for trigger in &report.rollback_triggers {
        println!("  {} {}", "•".cyan(), trigger.condition);
        println!("    → {}", trigger.action);
    }

    p::separator();
}

fn handle_triggers(network: String, json: bool) -> Result<()> {
    let context = DeploymentContext {
        network: network.clone(),
        ..Default::default()
    };
    let triggers = rollback_triggers(&context);

    if json {
        println!("{}", serde_json::to_string_pretty(&triggers)?);
        return Ok(());
    }

    p::header(&format!("Rollback Triggers — {network}"));
    p::separator();

    let rows: Vec<Vec<String>> = triggers
        .iter()
        .map(|trigger| vec![trigger.condition.clone(), trigger.action.clone()])
        .collect();
    p::table(&["Condition", "Action"], &rows);

    p::separator();
    Ok(())
}

/// Outcomes that count as a clean result, kept next to the renderer that uses them.
pub fn clean_outcomes() -> Vec<Outcome> {
    vec![Outcome::Pass, Outcome::Skipped]
}
