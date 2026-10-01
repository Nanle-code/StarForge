use crate::utils::{
    config,
    deploy_checklist::{self, ChecklistReport, DeploymentChecklistConfig},
    output, project_config,
};
use anyhow::{Context, Result};
use clap::Args;
use colored::Colorize;
use std::path::PathBuf;

#[derive(Args)]
pub struct DeployChecklistArgs {
    /// Path to the compiled .wasm artifact
    #[arg(long)]
    pub wasm: PathBuf,
    /// Target network to verify
    #[arg(long, default_value = "mainnet", value_parser = ["testnet", "mainnet"])]
    pub network: String,
    /// Wallet name (defaults to the first configured wallet)
    #[arg(long)]
    pub wallet: Option<String>,
    /// Select a hardware wallet as the signing method
    #[arg(long, value_enum)]
    pub hardware: Option<crate::utils::hardware_wallet::HardwareWalletKind>,
    /// Emit a machine-readable JSON report
    #[arg(long)]
    pub json: bool,
}

pub async fn handle(args: DeployChecklistArgs) -> Result<()> {
    let start = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project = project_config::find_and_load_project_lockfile(&start)?;
    let policy = project
        .and_then(|(_, lockfile)| lockfile.deployment_checklist)
        .unwrap_or_else(DeploymentChecklistConfig::default);
    policy.validate()?;

    let config = config::load_effective()?;
    let wallet = match args.wallet.as_deref() {
        Some(name) => config
            .wallets
            .iter()
            .find(|wallet| wallet.name == name)
            .with_context(|| format!("Wallet '{name}' not found"))?,
        None => config
            .wallets
            .first()
            .context("No wallets configured; create or import a deployer wallet first")?,
    };

    let report =
        deploy_checklist::run(&args.wasm, &args.network, wallet, args.hardware, &policy).await?;

    if args.json || output::is_json_mode_enabled() {
        output::print_json(&report)?;
    } else {
        print_report(&report);
    }

    if !report.passed {
        crate::utils::exit_codes::ExitCode::Usage.exit();
    }
    Ok(())
}

fn print_report(report: &ChecklistReport) {
    crate::utils::print::header("Pre-Mainnet Deployment Checklist");
    crate::utils::print::kv("Network", &report.network);
    crate::utils::print::kv("Wallet", &report.wallet);
    crate::utils::print::kv("WASM", &report.wasm);
    if let Some(hash) = &report.wasm_hash {
        crate::utils::print::kv("WASM SHA-256", hash);
    }
    println!();

    for check in &report.checks {
        let status = if check.passed {
            "PASS".green().bold()
        } else if check.required {
            "FAIL".red().bold()
        } else {
            "WARN".yellow().bold()
        };
        let required = if check.required {
            "required"
        } else {
            "advisory"
        };
        println!(
            "  [{status}] {} ({required}) — {}",
            check.name, check.detail
        );
        if let Some(remediation) = &check.remediation {
            println!("         Fix: {}", remediation.dimmed());
        }
    }
    println!();

    if report.passed {
        crate::utils::print::success("All required deployment checklist checks passed");
    } else {
        crate::utils::print::error(&format!(
            "{} required check(s) failed; mainnet deployment will be blocked",
            report.failed_required_checks().len()
        ));
    }
}
