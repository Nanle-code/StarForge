use crate::utils::{deploy_policy as policy, print as p};
use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum DeployPolicyCommands {
    /// Create a documented default deploy policy file
    Init {
        #[arg(default_value = "starforge-deploy-policy.toml")]
        output: PathBuf,
    },
    /// Validate a deploy policy file and optionally simulate a deploy context
    Check {
        #[arg(default_value = "starforge-deploy-policy.toml")]
        config: PathBuf,
        /// Network to simulate (defaults to first allowed network or testnet)
        #[arg(long, default_value = "testnet")]
        network: String,
        /// Simulate an executed deploy (passes require_execute_flag when set)
        #[arg(long, default_value = "false")]
        execute: bool,
        /// Comma-separated approvers for simulation (overrides env)
        #[arg(long, value_delimiter = ',')]
        approvers: Option<Vec<String>>,
        /// Comma-separated checklist ids for simulation (overrides env)
        #[arg(long, value_delimiter = ',')]
        checklist: Option<Vec<String>>,
        #[arg(long)]
        json: bool,
    },
}

pub fn handle(command: DeployPolicyCommands) -> Result<()> {
    match command {
        DeployPolicyCommands::Init { output } => {
            policy::write_default_policy(&output)?;
            p::success(&format!(
                "Deploy policy configuration written to {}",
                output.display()
            ));
        }
        DeployPolicyCommands::Check {
            config,
            network,
            execute,
            approvers,
            checklist,
            json,
        } => {
            let loaded = policy::load_policy(&config)?;
            let context = policy::DeployContext::from_env(&network, execute)
                .with_overrides(approvers, checklist);
            let report = policy::evaluate(&config, &loaded, &context);

            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                p::header("Deploy Policy Check");
                if let Some(org) = &report.organization {
                    p::kv("Organization", org);
                }
                p::kv("Policy file", &config.display().to_string());
                p::kv("Simulated network", &network);
                p::kv("Simulated execute", if execute { "yes" } else { "no" });
                println!();
                for violation in &report.violations {
                    println!(
                        "  [{}] {} — {}",
                        "FAIL".red().bold(),
                        violation.message,
                        violation.remediation.dimmed()
                    );
                }
                if report.passed {
                    p::success("All deploy policy rules satisfied");
                } else {
                    p::warn(&format!(
                        "{} violation(s) — deploy would be blocked",
                        report.violations.len()
                    ));
                }
            }

            if !report.passed {
                crate::utils::exit_codes::ExitCode::Usage.exit();
            }
        }
    }
    Ok(())
}
