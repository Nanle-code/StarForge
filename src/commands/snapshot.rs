use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::Utc;
use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use stellar_strkey::Contract;
use stellar_xdr::curr::{
    ContractDataDurability, Hash, LedgerEntryData, LedgerKey, LedgerKeyContractCode,
    LedgerKeyContractData, ScAddress, ScVal,
};

use crate::utils::testnet_integration::{SorobanNetwork, TestnetClient, TestnetConfig};

#[derive(Debug, Subcommand)]
pub enum SnapshotCommands {
    /// Create a deterministic contract ledger snapshot
    Create(CreateSnapshotArgs),
}

#[derive(Debug, Args)]
pub struct CreateSnapshotArgs {
    /// Contract ID to snapshot
    #[arg(long)]
    pub contract: String,

    /// Network to snapshot from
    #[arg(long, value_parser = ["testnet", "mainnet"])]
    pub network: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ContractSnapshot {
    pub version: u32,
    pub contract_id: String,
    pub network: String,
    pub ledger: u32,
    pub captured_at: String,
    pub entries: Vec<SnapshotEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotEntry {
    pub kind: String,
    pub key: String,
    pub value: String,
    pub last_modified_ledger: u32,
    pub live_until_ledger: Option<u32>,
}

pub async fn handle(command: SnapshotCommands) -> Result<()> {
    match command {
        SnapshotCommands::Create(args) => create_snapshot(args),
    }
}

fn create_snapshot(args: CreateSnapshotArgs) -> Result<()> {
    let network = match args.network.as_str() {
        "mainnet" => SorobanNetwork::Mainnet,
        "testnet" => SorobanNetwork::Testnet,
        _ => anyhow::bail!("Unsupported network '{}'", args.network),
    };

    let client = TestnetClient::new(TestnetConfig::for_network(network));

    let ledger = client
        .latest_ledger()
        .context("Failed to fetch latest ledger")?;

    let contract = Contract::from_string(&args.contract)
        .context("Invalid Soroban contract ID")?;

    let contract_hash = contract.0;
    let contract_address = ScAddress::Contract(Hash(contract_hash));

    // The contract instance contains the WASM hash and instance storage.
    let instance_key = LedgerKey::ContractData(LedgerKeyContractData {
        contract: contract_address,
        key: ScVal::LedgerKeyContractInstance,
        durability: ContractDataDurability::Persistent,
    });

    let instance_key_xdr = encode_ledger_key(&instance_key)?;

    let instance_entries = client.get_ledger_entries(&[&instance_key_xdr])?;

    let instance_entry = instance_entries
        .into_iter()
        .next()
        .with_context(|| {
            format!(
                "Contract '{}' has no instance entry at ledger {}",
                args.contract, ledger
            )
        })?;

    let mut entries = vec![SnapshotEntry {
        kind: "instance".to_string(),
        key: instance_entry.key,
        value: instance_entry.value.clone(),
        last_modified_ledger: instance_entry.last_modified_ledger,
        live_until_ledger: instance_entry.live_until_ledger,
    }];

    // Decode the contract instance to obtain the actual WASM hash.
    let instance_xdr = BASE64
        .decode(&instance_entry.value)
        .context("Failed to decode contract instance XDR")?;

    let ledger_entry = LedgerEntryData::from_xdr(
        &instance_xdr,
        stellar_xdr::curr::Limits::none(),
    )
    .context("Failed to decode contract instance ledger entry")?;

    let wasm_hash = match ledger_entry {
        LedgerEntryData::ContractData(entry) => match entry.val {
            ScVal::ContractInstance(instance) => match instance.executable {
                stellar_xdr::curr::ContractExecutable::Wasm(hash) => hash,
                executable => {
                    anyhow::bail!(
                        "Contract uses unsupported executable type: {:?}",
                        executable
                    );
                }
            },
            value => {
                anyhow::bail!(
                    "Contract instance entry contains unexpected value: {:?}",
                    value
                );
            }
        },
        value => {
            anyhow::bail!(
                "Expected contract data ledger entry, got: {:?}",
                value
            );
        }
    };

    // Fetch the actual WASM ledger entry using the hash from the instance.
    let code_key = LedgerKey::ContractCode(LedgerKeyContractCode {
        hash: wasm_hash,
    });

    let code_key_xdr = encode_ledger_key(&code_key)?;
    let code_entries = client.get_ledger_entries(&[&code_key_xdr])?;

    let code_entry = code_entries
        .into_iter()
        .next()
        .context("Contract WASM code entry was not found")?;

    entries.push(SnapshotEntry {
        kind: "code".to_string(),
        key: code_entry.key,
        value: code_entry.value,
        last_modified_ledger: code_entry.last_modified_ledger,
        live_until_ledger: code_entry.live_until_ledger,
    });

    // Keep snapshots deterministic and diff-friendly.
    entries.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| a.key.cmp(&b.key))
            .then_with(|| a.value.cmp(&b.value))
    });

    let snapshot = ContractSnapshot {
        version: 1,
        contract_id: args.contract.clone(),
        network: args.network.clone(),
        ledger,
        captured_at: Utc::now().to_rfc3339(),
        entries,
    };

    let directory = PathBuf::from("test_snapshots");
    fs::create_dir_all(&directory)?;

    let path = directory.join(format!("{}-{}.json", args.contract, ledger));

    let contents = serde_json::to_string_pretty(&snapshot)?;
    fs::write(&path, contents)?;

    println!("Created contract snapshot:");
    println!("  Contract: {}", args.contract);
    println!("  Network:  {}", args.network);
    println!("  Ledger:   {}", ledger);
    println!("  Entries:  {}", snapshot.entries.len());
    println!("  File:     {}", path.display());

    Ok(())
}

fn encode_ledger_key(key: &LedgerKey) -> Result<String> {
    use stellar_xdr::curr::WriteXdr;

    let bytes = key
        .to_xdr(stellar_xdr::curr::Limits::none())
        .context("Failed to encode ledger key as XDR")?;

    Ok(BASE64.encode(bytes))
}