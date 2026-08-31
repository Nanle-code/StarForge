//! `starforge environment` — deployment environment management (#381 / D-44).
//!
//! See `crate::utils::environment` for the underlying model (tiers,
//! validation, isolation, promotion) — this module is CLI plumbing only.

use crate::utils::deploy_history;
use crate::utils::environment::{
    self, check_isolation, validate_environment, EnvironmentConfig, EnvironmentTier,
};
use crate::utils::{config, print as p};
use anyhow::Result;
use clap::{Args, Subcommand};
use colored::*;

#[derive(Subcommand)]
pub enum EnvironmentCommands {
    /// Register a new deployment environment
    Add(AddArgs),
    /// List all registered environments
    List,
    /// Show one environment's configuration and last deployment
    Show(ShowArgs),
    /// Remove a registered environment
    Remove(RemoveArgs),
    /// Validate one (or all) environments' configuration and isolation
    Validate(ValidateArgs),
    /// Promote the last successful deployment from one environment to the next
    Promote(PromoteArgs),
    /// Show an overview dashboard of all environments
    Dashboard,
}

#[derive(Args)]
pub struct AddArgs {
    /// Environment name, e.g. "dev", "staging", "production"
    #[arg(long)]
    pub name: String,
    /// Pipeline tier: dev, staging, or production
    #[arg(long)]
    pub tier: String,
    /// Network this environment deploys to
    #[arg(long)]
    pub network: String,
    /// Wallet name used to sign deployments in this environment
    #[arg(long)]
    pub wallet: Option<String>,
    /// Human-readable description
    #[arg(long)]
    pub description: Option<String>,
}

#[derive(Args)]
pub struct ShowArgs {
    /// Environment name
    #[arg(long)]
    pub name: String,
}

#[derive(Args)]
pub struct RemoveArgs {
    /// Environment name
    #[arg(long)]
    pub name: String,
}

#[derive(Args)]
pub struct ValidateArgs {
    /// Environment name. Validates every registered environment if omitted.
    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Args)]
pub struct PromoteArgs {
    /// Source environment name
    #[arg(long)]
    pub from: String,
    /// Target environment name
    #[arg(long)]
    pub to: String,
    /// Skip the confirmation prompt (still honored for environments with
    /// `require_confirmation` set — see `environment add --tier production`)
    #[arg(long)]
    pub yes: bool,
}

pub fn handle(cmd: EnvironmentCommands) -> Result<()> {
    match cmd {
        EnvironmentCommands::Add(args) => handle_add(args),
        EnvironmentCommands::List => handle_list(),
        EnvironmentCommands::Show(args) => handle_show(args),
        EnvironmentCommands::Remove(args) => handle_remove(args),
        EnvironmentCommands::Validate(args) => handle_validate(args),
        EnvironmentCommands::Promote(args) => handle_promote(args),
        EnvironmentCommands::Dashboard => handle_dashboard(),
    }
}

fn handle_add(args: AddArgs) -> Result<()> {
    p::header("Add Deployment Environment");

    let tier: EnvironmentTier = args.tier.parse()?;
    let env = EnvironmentConfig::new(&args.name, tier, &args.network, args.wallet.clone());

    // Configuration validation (#381 acceptance criterion): reject the
    // environment up front rather than letting a dangling network/wallet
    // reference surface later as a confusing deploy-time failure.
    let cfg = config::load()?;
    validate_environment(&cfg, &env)?;

    let existing = environment::load_environments()?;
    let violations = check_isolation(&env, &existing);
    if !violations.is_empty() {
        p::warn(&format!(
            "Isolation warning: {}",
            violations[0].reason
        ));
        p::warn("Registering anyway — rerun `starforge environment validate` to review.");
    }

    environment::register_environment(env)?;

    p::separator();
    p::success(&format!("Environment '{}' registered.", args.name));
    p::kv("Tier", &tier.to_string());
    p::kv("Network", &args.network);
    if let Some(wallet) = &args.wallet {
        p::kv("Wallet", wallet);
    }
    p::separator();
    Ok(())
}

fn handle_list() -> Result<()> {
    p::header("Deployment Environments");
    let envs = environment::load_environments()?;

    if envs.is_empty() {
        p::info("No environments registered yet.");
        p::info("Run `starforge environment add --name dev --tier dev --network testnet`.");
        return Ok(());
    }

    p::separator();
    for env in &envs {
        println!(
            "  {} {} {} {}",
            "▶".cyan(),
            env.name.bright_white().bold(),
            format!("[{}]", env.tier).dimmed(),
            format!("→ {}", env.network).white(),
        );
        if let Some(wallet) = &env.wallet {
            println!("      wallet: {}", wallet.dimmed());
        }
    }
    p::separator();
    Ok(())
}

fn handle_show(args: ShowArgs) -> Result<()> {
    p::header(&format!("Environment: {}", args.name));

    let env = environment::get_environment(&args.name)?
        .ok_or_else(|| anyhow::anyhow!("Environment '{}' not found.", args.name))?;

    p::separator();
    p::kv("Name", &env.name);
    p::kv("Tier", &env.tier.to_string());
    p::kv("Network", &env.network);
    p::kv("Wallet", env.wallet.as_deref().unwrap_or("(none configured)"));
    p::kv(
        "Requires confirmation to promote into",
        if env.require_confirmation { "yes" } else { "no" },
    );
    if let Some(desc) = &env.description {
        p::kv("Description", desc);
    }
    p::kv("Created", &env.created_at);

    if let Ok(Some(last)) = deploy_history::last_successful(&env.network) {
        println!();
        p::kv(
            "Last successful deployment",
            &format!(
                "{} ({})",
                &last.id[..8.min(last.id.len())],
                last.timestamp.get(..16).unwrap_or(&last.timestamp)
            ),
        );
        p::kv(
            "Contract",
            last.contract_id.as_deref().unwrap_or("(not recorded)"),
        );
    } else {
        println!();
        p::info("No successful deployment recorded on this network yet.");
    }
    p::separator();
    Ok(())
}

fn handle_remove(args: RemoveArgs) -> Result<()> {
    let removed = environment::remove_environment(&args.name)?;
    if removed {
        p::success(&format!("Environment '{}' removed.", args.name));
    } else {
        p::warn(&format!("Environment '{}' was not registered.", args.name));
    }
    Ok(())
}

fn handle_validate(args: ValidateArgs) -> Result<()> {
    p::header("Validate Deployment Environments");
    let envs = environment::load_environments()?;
    let cfg = config::load()?;

    let targets: Vec<&EnvironmentConfig> = match &args.name {
        Some(name) => {
            let found = envs
                .iter()
                .find(|e| &e.name == name)
                .ok_or_else(|| anyhow::anyhow!("Environment '{}' not found.", name))?;
            vec![found]
        }
        None => envs.iter().collect(),
    };

    if targets.is_empty() {
        p::info("No environments registered yet.");
        return Ok(());
    }

    p::separator();
    let mut any_failed = false;
    for env in targets {
        let others: Vec<EnvironmentConfig> = envs
            .iter()
            .filter(|e| e.name != env.name)
            .cloned()
            .collect();
        let isolation_violations = check_isolation(env, &others);

        match validate_environment(&cfg, env) {
            Ok(()) if isolation_violations.is_empty() => {
                p::success(&format!("{}: configuration valid, isolated.", env.name));
            }
            Ok(()) => {
                any_failed = true;
                p::error(&format!(
                    "{}: configuration valid, but isolation violated — {}",
                    env.name, isolation_violations[0].reason
                ));
            }
            Err(e) => {
                any_failed = true;
                p::error(&format!("{}: {}", env.name, e));
            }
        }
    }
    p::separator();

    if any_failed {
        anyhow::bail!("One or more environments failed validation.");
    }
    Ok(())
}

fn handle_promote(args: PromoteArgs) -> Result<()> {
    p::header(&format!("Promote: {} -> {}", args.from, args.to));

    let to_env = environment::get_environment(&args.to)?
        .ok_or_else(|| anyhow::anyhow!("Environment '{}' not found.", args.to))?;

    if to_env.require_confirmation && !args.yes {
        use dialoguer::Confirm;
        let ok = Confirm::new()
            .with_prompt(format!(
                "Promote the last successful deployment from '{}' into '{}' ({})?",
                args.from, args.to, to_env.tier
            ))
            .default(false)
            .interact()
            .unwrap_or(false);
        if !ok {
            p::info("Promotion cancelled.");
            return Ok(());
        }
    }

    let record = environment::promote(&args.from, &args.to)?;

    p::separator();
    p::success("Promotion recorded.");
    p::kv("From", &record.from_environment);
    p::kv("To", &record.to_environment);
    p::kv("WASM hash", &record.wasm_hash);
    p::kv(
        "New deployment record",
        &record.promoted_deployment_id[..8.min(record.promoted_deployment_id.len())],
    );
    println!();
    p::info(
        "This records intent only. Run `starforge deploy --execute` against the target \
         network to actually submit the transaction.",
    );
    p::separator();
    Ok(())
}

fn handle_dashboard() -> Result<()> {
    p::header("Environment Dashboard");
    let envs = environment::load_environments()?;

    if envs.is_empty() {
        p::info("No environments registered yet.");
        p::info("Run `starforge environment add --name dev --tier dev --network testnet`.");
        return Ok(());
    }

    p::separator();
    for env in &envs {
        println!(
            "  {} {} {}",
            "▶".cyan(),
            env.name.bright_white().bold(),
            format!("[{}] → {}", env.tier, env.network).dimmed(),
        );

        match deploy_history::last_successful(&env.network) {
            Ok(Some(last)) => {
                println!(
                    "      {} {} | {} | {}",
                    "✓".green(),
                    last.id[..8.min(last.id.len())].dimmed(),
                    last.timestamp.get(..16).unwrap_or(&last.timestamp).dimmed(),
                    last.contract_id.as_deref().unwrap_or(&last.wasm_path).white(),
                );
            }
            Ok(None) => {
                println!("      {} no successful deployment yet", "…".dimmed());
            }
            Err(e) => {
                println!("      {} could not read deployment history: {}", "!".yellow(), e);
            }
        }

        let others: Vec<EnvironmentConfig> =
            envs.iter().filter(|e| e.name != env.name).cloned().collect();
        let violations = check_isolation(env, &others);
        if !violations.is_empty() {
            println!(
                "      {} {}",
                "!".red(),
                violations[0].reason.yellow()
            );
        }
        println!();
    }
    p::separator();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_args_tier_parses_into_environment_tier() {
        let args = AddArgs {
            name: "dev".to_string(),
            tier: "dev".to_string(),
            network: "testnet".to_string(),
            wallet: None,
            description: None,
        };
        let tier: EnvironmentTier = args.tier.parse().unwrap();
        assert_eq!(tier, EnvironmentTier::Dev);
    }

    #[test]
    fn add_args_rejects_an_unknown_tier() {
        let args = AddArgs {
            name: "x".to_string(),
            tier: "quality-assurance".to_string(),
            network: "testnet".to_string(),
            wallet: None,
            description: None,
        };
        assert!(args.tier.parse::<EnvironmentTier>().is_err());
    }
}
