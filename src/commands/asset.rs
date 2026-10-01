//! Classic Stellar asset helpers: deterministic SAC ids and wrap/deploy.

use crate::utils::{config, output, print as p, soroban_native, wallet_signer};
use anyhow::Result;
use clap::{Args, Subcommand};
use colored::*;
use serde::Serialize;

#[derive(Subcommand)]
pub enum AssetCommands {
    /// Compute the deterministic Stellar Asset Contract (SAC) id for an asset
    #[command(name = "contract-id")]
    ContractId(ContractIdArgs),
    /// Deploy / wrap the SAC for a classic asset if it is not already live
    Wrap(WrapArgs),
}

#[derive(Args)]
pub struct ContractIdArgs {
    /// Asset: `XLM` / `native`, or `CODE:ISSUER`
    pub asset: String,
    /// Network passphrase selector
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet", "docker-testnet"])]
    pub network: String,
    /// Emit JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct WrapArgs {
    /// Asset: `XLM` / `native`, or `CODE:ISSUER`
    pub asset: String,
    /// Network to deploy on
    #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet", "docker-testnet"])]
    pub network: String,
    /// Wallet that pays for the wrap transaction
    #[arg(long)]
    pub wallet: String,
    /// Skip confirmation
    #[arg(long, short = 'y')]
    pub yes: bool,
    /// Emit JSON
    #[arg(long)]
    pub json: bool,
}

pub async fn handle(cmd: AssetCommands) -> Result<()> {
    match cmd {
        AssetCommands::ContractId(args) => contract_id(args).await,
        AssetCommands::Wrap(args) => wrap(args).await,
    }
}

#[derive(Serialize)]
struct ContractIdResponse {
    asset: String,
    network: String,
    contract_id: String,
}

async fn contract_id(args: ContractIdArgs) -> Result<()> {
    config::validate_network(&args.network)?;
    let asset = soroban_native::parse_classic_asset(&args.asset)?;
    let contract_id = soroban_native::asset_contract_id(&asset, &args.network)?;

    if args.json || output::is_json_mode_enabled() {
        return output::print_json(&ContractIdResponse {
            asset: args.asset,
            network: args.network,
            contract_id,
        });
    }

    p::header("Stellar Asset Contract ID");
    p::kv("Asset", &args.asset);
    p::kv("Network", &args.network);
    p::kv_accent("Contract ID", &contract_id);
    Ok(())
}

#[derive(Serialize)]
struct WrapResponse {
    asset: String,
    network: String,
    contract_id: String,
    tx_hash: String,
    already_deployed: bool,
}

async fn wrap(args: WrapArgs) -> Result<()> {
    config::validate_network(&args.network)?;
    let asset = soroban_native::parse_classic_asset(&args.asset)?;
    let contract_id = soroban_native::asset_contract_id(&asset, &args.network)?;

    let cfg = config::load()?;
    let wallet = cfg
        .wallets
        .iter()
        .find(|w| w.name == args.wallet)
        .ok_or_else(|| anyhow::anyhow!("Wallet '{}' not found", args.wallet))?;

    if wallet.is_watch_only() {
        anyhow::bail!(
            "Wallet '{}' is watch-only and cannot sign. Import a secret key or use a signing wallet.",
            wallet.name
        );
    }

    if !args.yes {
        p::info(&format!(
            "Wrap SAC for {} on {} → {}",
            args.asset.cyan(),
            args.network.cyan(),
            contract_id.yellow()
        ));
    }

    let signing = wallet_signer::SigningRequest::from_options(
        Some(wallet),
        None,
        None,
        &args.network,
        args.yes,
        "SAC wrap",
    )?;

    let result = soroban_native::wrap_asset_native(asset, wallet, &args.network, &signing).await?;

    if args.json || output::is_json_mode_enabled() {
        return output::print_json(&WrapResponse {
            asset: args.asset,
            network: args.network,
            contract_id: result.contract_id,
            tx_hash: result.create_tx_hash,
            already_deployed: result.wasm_already_uploaded && result.create_tx_hash.is_empty(),
        });
    }

    if result.create_tx_hash.is_empty() {
        p::success(&format!(
            "SAC already deployed: {}",
            result.contract_id.green()
        ));
    } else {
        p::success(&format!("SAC wrapped: {}", result.contract_id.green()));
        p::kv("Tx", &result.create_tx_hash);
    }
    Ok(())
}
