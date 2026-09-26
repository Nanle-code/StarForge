//! `starforge alias` — manage per-network contract and account aliases.
//!
//! Aliases let users type a short name (for example `token`) anywhere an
//! address is accepted. Resolution itself lives in [`crate::utils::aliases`];
//! this module only wires the `set`/`list`/`rm` subcommands to it.

use crate::utils::aliases::AliasStore;
use crate::utils::{config, print as p};
use anyhow::Result;
use clap::{Args, Subcommand};

/// Manage per-network contract and account aliases (set, list, rm).
#[derive(Subcommand)]
pub enum AliasCommands {
    /// Create or update an alias for a contract or account address
    Set(SetArgs),
    /// List the aliases defined on a network
    List(ListArgs),
    /// Remove an alias from a network
    Rm(RemoveArgs),
}

#[derive(Args)]
pub struct SetArgs {
    /// Alias name to create or update (for example, `token`)
    pub name: String,
    /// Stellar contract (`C...`) or account (`G...`) address to bind
    pub address: String,
    /// Network to scope the alias to (defaults to the active network)
    #[arg(long)]
    pub network: Option<String>,
}

#[derive(Args)]
pub struct ListArgs {
    /// Network to list aliases for (defaults to the active network)
    #[arg(long)]
    pub network: Option<String>,
}

#[derive(Args)]
pub struct RemoveArgs {
    /// Alias name to remove
    pub name: String,
    /// Network to remove the alias from (defaults to the active network)
    #[arg(long)]
    pub network: Option<String>,
}

pub async fn handle(cmd: AliasCommands) -> Result<()> {
    match cmd {
        AliasCommands::Set(args) => handle_set(args),
        AliasCommands::List(args) => handle_list(args),
        AliasCommands::Rm(args) => handle_remove(args),
    }
}

fn handle_set(args: SetArgs) -> Result<()> {
    let network = resolve_network(args.network);
    let mut store = AliasStore::load()?;
    let previous = store.set(&network, &args.name, &args.address)?;
    store.save()?;

    match previous {
        Some(previous) => p::info(&format!(
            "Updated alias '{}' on '{}': {} -> {}",
            args.name, network, previous, args.address
        )),
        None => p::success(&format!(
            "Alias '{}' saved on '{}': {}",
            args.name, network, args.address
        )),
    }
    p::info(&format!(
        "Use it anywhere an address is accepted: starforge contract invoke --id {} ...",
        args.name
    ));
    Ok(())
}

fn handle_list(args: ListArgs) -> Result<()> {
    let network = resolve_network(args.network);
    let store = AliasStore::load()?;
    let aliases = store.list(&network);

    if aliases.is_empty() {
        p::info(&format!("No aliases defined on '{}'.", network));
        p::info(&format!(
            "Add one with: starforge alias set token <ADDRESS> --network {}",
            network
        ));
        return Ok(());
    }

    p::header(&format!("Aliases on '{}'", network));
    let rows: Vec<Vec<String>> = aliases
        .into_iter()
        .map(|(name, address)| vec![name, address])
        .collect();
    p::table(&["Alias", "Address"], &rows);
    Ok(())
}

fn handle_remove(args: RemoveArgs) -> Result<()> {
    let network = resolve_network(args.network);
    let mut store = AliasStore::load()?;

    if store.remove(&network, &args.name) {
        store.save()?;
        p::success(&format!(
            "Removed alias '{}' from '{}'.",
            args.name, network
        ));
    } else {
        p::warn(&format!(
            "Alias '{}' is not defined on '{}'; nothing to remove.",
            args.name, network
        ));
    }
    Ok(())
}

/// Resolve the network for an alias command: the explicit `--network` when
/// given, otherwise the active network from config, falling back to `testnet`
/// when the config cannot be loaded.
fn resolve_network(explicit: Option<String>) -> String {
    if let Some(network) = explicit {
        let trimmed = network.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    config::load()
        .map(|cfg| cfg.network)
        .unwrap_or_else(|_| "testnet".to_string())
}
