use crate::utils::config::{self, WalletEntry};
use crate::utils::simulation_resources::{
    self, ResourceFeePlan, SimulationResourceError, SimulationResources,
};
use crate::utils::wallet_signer::{self, SigningRequest};
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use once_cell::sync::Lazy;
use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::time::Duration;
use stellar_strkey::{ed25519, Contract};
use stellar_xdr::curr::{
    AccountId, ContractDataDurability, ContractExecutable, Hash, LedgerEntryData, LedgerKey,
    LedgerKeyContractData, PublicKey, ScAddress, ScMap, ScString, ScSymbol, ScVal, Uint256,
};

fn build_http_client(timeout: Duration) -> Result<Client> {
    Client::builder()
        .timeout(timeout)
        .pool_max_idle_per_host(10)
        .build()
        .context("Failed to create Soroban HTTP client")
}

static HTTP_CLIENT: Lazy<Client> = Lazy::new(|| {
    build_http_client(Duration::from_secs(30)).expect("Failed to create shared Soroban HTTP client")
});

#[derive(Debug, Serialize, Deserialize)]
pub struct SimulationResult {
    pub return_value: String,
    /// Minimum resource fee (stroops) reported by simulation, falling back to
    /// [`FALLBACK_FEE_STROOPS`] when the RPC server did not report one.
    pub fee: u64,
    pub events: Vec<String>,
    #[serde(default)]
    pub errors: Vec<String>,
    /// Full resource accounting (CPU, memory, footprint, minimum resource fee)
    /// when the response carried it. `None` on servers that do not implement
    /// Soroban resource reporting, or when the payload could not be parsed —
    /// in that case the reason is appended to `errors`.
    #[serde(default)]
    pub resources: Option<SimulationResources>,
}

impl SimulationResult {
    /// Build a fee plan from the simulated resources.
    ///
    /// Returns `None` when the response carried no resource accounting, so
    /// callers can fall back to their own estimate instead of reporting a
    /// fabricated number.
    pub fn fee_plan(&self, margin_percent: u32) -> Option<ResourceFeePlan> {
        let resources = self.resources.as_ref()?;
        simulation_resources::plan_fee(
            resources,
            margin_percent,
            simulation_resources::DEFAULT_INCLUSION_FEE_STROOPS,
        )
        .ok()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionResult {
    pub hash: String,
    pub return_value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractInspectResult {
    pub contract_id: String,
    pub executable: String,
    pub wasm_hash: Option<String>,
    pub storage_durability: String,
    pub latest_ledger: u32,
    pub last_modified_ledger_seq: Option<u32>,
    pub live_until_ledger_seq: Option<u32>,
    pub instance_storage: Vec<ContractStorageEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractStorageEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Serialize)]
struct SorobanRpcRequest {
    jsonrpc: String,
    id: u64,
    method: String,
    params: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct SorobanRpcResponse<T> {
    #[serde(rename = "jsonrpc")]
    _jsonrpc: String,
    #[serde(rename = "id")]
    _id: u64,
    result: Option<T>,
    error: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct GetLedgerEntriesResult {
    #[serde(rename = "latestLedger")]
    latest_ledger: u32,
    entries: Vec<RpcLedgerEntry>,
}

#[derive(Debug, Deserialize)]
struct RpcLedgerEntry {
    #[allow(dead_code)]
    xdr: String,
    #[serde(rename = "lastModifiedLedgerSeq")]
    last_modified_ledger_seq: Option<u32>,
    #[serde(rename = "liveUntilLedgerSeq")]
    live_until_ledger_seq: Option<u32>,
}

/// Unified entry-point used by both `commands::contract` and `commands::invoke`.
///
/// When `wallet` is `None` the call simulates only; when `Some` it simulates
/// then submits and returns a `TransactionResult`.
pub struct InvokeOutcome {
    pub simulation: SimulationResult,
    pub transaction: Option<TransactionResult>,
}

pub async fn invoke_contract(
    contract_id: &str,
    function: &str,
    args: &[String],
    arg_types: &[String],
    network: &str,
    wallet: Option<&WalletEntry>,
    signing: Option<&SigningRequest>,
) -> Result<InvokeOutcome> {
    let simulation = simulate_transaction(contract_id, function, args, arg_types, network).await?;
    let transaction = match wallet {
        Some(w) => Some(
            submit_transaction(contract_id, function, args, arg_types, network, w, signing).await?,
        ),
        None => None,
    };
    Ok(InvokeOutcome {
        simulation,
        transaction,
    })
}

pub async fn simulate_transaction(
    contract_id: &str,
    function: &str,
    args: &[String],
    arg_types: &[String],
    network: &str,
) -> Result<SimulationResult> {
    let rpc_url = get_rpc_url(network)?;

    // Convert arguments to XDR ScVal format
    let xdr_args = encode_arguments(args, arg_types)?;

    // Build the simulation request
    let request = SorobanRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: "simulateTransaction".to_string(),
        params: serde_json::json!({
            "transaction": build_transaction_xdr(contract_id, function, &xdr_args)?,
        }),
    };

    // Make the RPC call
    let result: serde_json::Value = rpc_request_with_url(&rpc_url, request)
        .await
        .context("Simulation request failed")?;

    // Parse the simulation result (resources, fee, events, errors).
    build_simulation_result(&result)
}

pub async fn simulate_deploy_transaction(
    wasm_hash: &str,
    network: &str,
    wallet: &WalletEntry,
) -> Result<SimulationResult> {
    let rpc_url = get_rpc_url(network)?;
    let request = SorobanRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: "simulateTransaction".to_string(),
        params: serde_json::json!({
            "transaction": build_deploy_transaction_xdr(wasm_hash, wallet, network)?,
        }),
    };

    let result: serde_json::Value = rpc_request_with_url(&rpc_url, request)
        .await
        .context("Deploy simulation request failed")?;

    build_simulation_result(&result)
}

pub async fn submit_transaction(
    contract_id: &str,
    function: &str,
    args: &[String],
    arg_types: &[String],
    network: &str,
    wallet: &WalletEntry,
    signing: Option<&SigningRequest>,
) -> Result<TransactionResult> {
    crate::utils::network_guard::verify(network).await?;
    let rpc_url = get_rpc_url(network)?;

    // Convert arguments to XDR ScVal format
    let xdr_args = encode_arguments(args, arg_types)?;

    // Build and sign the transaction
    let signed_tx_xdr =
        build_and_sign_transaction(contract_id, function, &xdr_args, wallet, network, signing)?;

    // Build the submission request
    let request = SorobanRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: "sendTransaction".to_string(),
        params: serde_json::json!({
            "transaction": signed_tx_xdr,
        }),
    };

    // Make the RPC call
    let result: serde_json::Value = rpc_request_with_url(&rpc_url, request)
        .await
        .context("Transaction submission failed")?;

    // Parse the transaction result
    let hash = extract_transaction_hash(&result)?;
    let return_value = decode_return_value(&result)?;

    Ok(TransactionResult { hash, return_value })
}

pub fn upload_wasm(
    wasm_path: &str,
    network: &str,
    wallet: &crate::utils::config::WalletEntry,
) -> Result<String> {
    use std::process::Command;

    let rpc_url = get_rpc_url(network)?;
    let passphrase = config::get_network_passphrase(network);

    let output = Command::new("stellar")
        .args([
            "contract",
            "upload",
            "--wasm",
            wasm_path,
            "--rpc-url",
            &rpc_url,
            "--source",
            &wallet.name,
            "--network-passphrase",
            &passphrase,
        ])
        .output()
        .context("Failed to run `stellar contract upload`. Is the Stellar CLI installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("WASM upload failed: {}", stderr.trim());
    }

    let wasm_hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(wasm_hash)
}

pub async fn inspect_contract(contract_id: &str, network: &str) -> Result<ContractInspectResult> {
    let ledger_key = build_contract_instance_key(contract_id)?;
    let ledger_key_xdr = ledger_key_to_xdr_base64(&ledger_key)?;

    let request = SorobanRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: "getLedgerEntries".to_string(),
        params: serde_json::json!({
            "keys": [ledger_key_xdr],
            "xdrFormat": "base64",
        }),
    };

    let response: GetLedgerEntriesResult = rpc_request_with_url(&get_rpc_url(network)?, request)
        .await
        .with_context(|| {
            format!(
                "Failed to inspect contract '{}' on {}",
                contract_id, network
            )
        })?;

    parse_contract_inspect_result(contract_id, network, response)
}

fn get_rpc_url(network: &str) -> Result<String> {
    let cfg = config::load()?;
    match cfg.networks.get(network) {
        Some(net_cfg) => match &net_cfg.soroban_rpc_url {
            Some(url) => Ok(url.clone()),
            None => anyhow::bail!(
                "Network '{}' has no Soroban RPC URL configured. \
                 Use 'starforge network add --soroban-rpc-url <url>' to set one.",
                network
            ),
        },
        None => anyhow::bail!(
            "Network '{}' not found. Use 'starforge network add' to create it.",
            network
        ),
    }
}

pub fn rpc_url(network: &str) -> Result<String> {
    get_rpc_url(network)
}

/// Returns true when the Soroban RPC endpoint for `network` responds to `getHealth`.
pub async fn check_soroban_rpc(network: &str) -> bool {
    match get_rpc_url(network) {
        Ok(url) => check_soroban_rpc_url(&url).await,
        Err(_) => false,
    }
}

/// Returns true when a Soroban RPC URL responds to a `getHealth` JSON-RPC request.
pub async fn check_soroban_rpc_url(url: &str) -> bool {
    let request = SorobanRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: "getHealth".to_string(),
        params: serde_json::json!({}),
    };

    match HTTP_CLIENT.post(url).json(&request).send().await {
        Ok(response) => {
            if response.status() != 200 {
                return false;
            }
            match response
                .json::<SorobanRpcResponse<serde_json::Value>>()
                .await
            {
                Ok(parsed) => parsed.result.is_some(),
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

async fn rpc_request_with_url<T>(rpc_url: &str, request: SorobanRpcRequest) -> Result<T>
where
    T: DeserializeOwned,
{
    let response: SorobanRpcResponse<T> = HTTP_CLIENT
        .post(rpc_url)
        .json(&request)
        .send()
        .await
        .with_context(|| format!("Soroban RPC request to {} failed", rpc_url))?
        .json()
        .await
        .with_context(|| format!("Failed to decode Soroban RPC response from {}", rpc_url))?;

    if let Some(error) = response.error {
        anyhow::bail!(
            "Soroban RPC {} failed: {}",
            request.method,
            extract_rpc_error_message(&error)
        );
    }

    response
        .result
        .ok_or_else(|| anyhow::anyhow!("Soroban RPC {} returned no result", request.method))
}

fn build_contract_instance_key(contract_id: &str) -> Result<LedgerKey> {
    let contract = Contract::from_string(contract_id).map_err(|_| {
        anyhow::anyhow!(
            "Invalid contract ID '{}'. Expected a Stellar contract strkey starting with 'C'.",
            contract_id
        )
    })?;

    Ok(LedgerKey::ContractData(LedgerKeyContractData {
        contract: ScAddress::Contract(Hash(contract.0)),
        key: ScVal::LedgerKeyContractInstance,
        durability: ContractDataDurability::Persistent,
    }))
}

fn ledger_key_to_xdr_base64(key: &LedgerKey) -> Result<String> {
    use base64::{engine::general_purpose, Engine as _};
    // Simplified XDR encoding - in production use proper stellar-xdr encoding
    let mock_xdr = format!("ledger_key_{:?}", key);
    Ok(general_purpose::STANDARD.encode(mock_xdr))
}

#[allow(dead_code)]
fn ledger_entry_from_xdr_base64(xdr: &str) -> Result<LedgerEntryData> {
    use base64::{engine::general_purpose, Engine as _};
    // Simplified XDR decoding - in production use proper stellar-xdr decoding
    let _decoded = general_purpose::STANDARD.decode(xdr)?;

    // For now, return a mock contract data entry
    // In production, properly decode the XDR bytes
    anyhow::bail!("XDR decoding not fully implemented - this is a mock")
}

fn parse_contract_inspect_result(
    contract_id: &str,
    network: &str,
    response: GetLedgerEntriesResult,
) -> Result<ContractInspectResult> {
    let GetLedgerEntriesResult {
        latest_ledger,
        entries,
    } = response;

    let entry = entries.into_iter().next().ok_or_else(|| {
        anyhow::anyhow!("Contract '{}' was not found on {}.", contract_id, network)
    })?;

    // For now, return a mock result since we can't decode XDR properly yet
    // In production, use: LedgerEntryData::from_xdr(entry.xdr.as_bytes(), Limits::none())?

    Ok(ContractInspectResult {
        contract_id: contract_id.to_string(),
        executable: "Wasm".to_string(),
        wasm_hash: Some("mock_wasm_hash_placeholder".to_string()),
        storage_durability: "Persistent".to_string(),
        latest_ledger,
        last_modified_ledger_seq: entry.last_modified_ledger_seq,
        live_until_ledger_seq: entry.live_until_ledger_seq,
        instance_storage: vec![],
    })
}

fn encode_arguments(args: &[String], arg_types: &[String]) -> Result<Vec<String>> {
    let mut xdr_args = Vec::new();

    for (arg, arg_type) in args.iter().zip(arg_types.iter()) {
        let scval = match arg_type.as_str() {
            "string" => ScVal::String(ScString(arg.as_bytes().try_into()?)),
            "symbol" => ScVal::Symbol(ScSymbol(arg.as_bytes().try_into()?)),
            "int" => {
                let val: i64 = arg.parse()?;
                ScVal::I64(val)
            }
            "bool" => {
                let val: bool = arg.parse()?;
                ScVal::Bool(val)
            }
            "address" => {
                // Simplified address parsing - in production, use proper Stellar address validation
                ScVal::Address(ScAddress::Account(AccountId(
                    PublicKey::PublicKeyTypeEd25519(
                        Uint256([0; 32]), // Placeholder - proper implementation needed
                    ),
                )))
            }
            _ => anyhow::bail!("Unsupported argument type: {}", arg_type),
        };

        // Convert ScVal to XDR string (simplified - proper XDR encoding needed)
        xdr_args.push(format!("{:?}", scval));
    }

    Ok(xdr_args)
}

fn build_transaction_xdr(contract_id: &str, function: &str, args: &[String]) -> Result<String> {
    // This is a simplified mock implementation
    // In production, you'd use stellar-sdk to build proper transaction XDR
    Ok(format!(
        "mock_transaction_xdr_{}_{}_{}",
        contract_id,
        function,
        args.len()
    ))
}

fn build_and_sign_transaction(
    contract_id: &str,
    function: &str,
    args: &[String],
    wallet: &WalletEntry,
    network: &str,
    signing: Option<&SigningRequest>,
) -> Result<String> {
    let tx_xdr = build_transaction_xdr(contract_id, function, args)?;
    if let Some(request) = signing {
        return wallet_signer::sign_transaction_xdr(&tx_xdr, request);
    }

    Ok(format!(
        "signed_mock_transaction_xdr_{}_{}_{}_{}",
        contract_id,
        function,
        args.len(),
        wallet.name
    ))
}

pub fn sign_deploy_transaction(
    wasm_hash: &str,
    wallet: &WalletEntry,
    network: &str,
    signing: &SigningRequest,
) -> Result<String> {
    let tx_xdr = build_deploy_transaction_xdr(wasm_hash, wallet, network)?;
    wallet_signer::sign_transaction_xdr(&tx_xdr, signing)
}

fn build_deploy_transaction_xdr(
    wasm_hash: &str,
    wallet: &WalletEntry,
    network: &str,
) -> Result<String> {
    Ok(format!(
        "mock_deploy_transaction_xdr_{}_{}_{}",
        wasm_hash, wallet.public_key, network
    ))
}

fn decode_return_value(result: &serde_json::Value) -> Result<String> {
    // Simplified return value decoding
    // In production, decode actual XDR ScVal to human-readable format
    if let Some(return_val) = result.get("returnValue") {
        Ok(return_val.as_str().unwrap_or("null").to_string())
    } else {
        Ok("void".to_string())
    }
}

/// Fee used when the RPC server reports no resource accounting at all.
///
/// Only a last resort: a real `minResourceFee` from simulation always wins.
pub const FALLBACK_FEE_STROOPS: u64 = 100_000;

/// Parse the resource accounting out of a `simulateTransaction` response.
///
/// The parse failure is deliberately not fatal: a fee estimate is still more
/// useful than aborting, so the caller surfaces the reason through
/// `SimulationResult::errors` and falls back to [`FALLBACK_FEE_STROOPS`].
fn extract_resources(
    result: &serde_json::Value,
) -> std::result::Result<SimulationResources, SimulationResourceError> {
    simulation_resources::parse_simulation_resources(result)
}

/// Minimum resource fee in stroops for this simulation.
///
/// Prefers the `minResourceFee` the RPC server reported. Previous releases
/// mistakenly reported `cost.cpuInsns` (an instruction count, not a fee) here.
fn extract_fee(resources: Option<&SimulationResources>) -> u64 {
    resources
        .map(|r| r.min_resource_fee_stroops)
        .unwrap_or(FALLBACK_FEE_STROOPS)
}

/// Assemble a [`SimulationResult`] from a raw RPC response.
fn build_simulation_result(result: &serde_json::Value) -> Result<SimulationResult> {
    let mut errors = extract_simulation_errors(result);

    let resources = match extract_resources(result) {
        Ok(resources) => Some(resources),
        Err(e) => {
            // A host-level simulation failure is already reported through
            // `errors`; only add parse problems that are not duplicates.
            let message = format!("resource accounting unavailable: {}", e);
            if !errors.iter().any(|existing| existing.contains(&message)) {
                errors.push(message);
            }
            None
        }
    };

    Ok(SimulationResult {
        return_value: decode_return_value(result)?,
        fee: extract_fee(resources.as_ref()),
        events: extract_events(result)?,
        errors,
        resources,
    })
}

fn extract_events(result: &serde_json::Value) -> Result<Vec<String>> {
    // Extract events from simulation result
    if let Some(events) = result.get("events") {
        if let Some(events_array) = events.as_array() {
            return Ok(events_array
                .iter()
                .map(|event| {
                    event
                        .as_str()
                        .map(decode_event_string)
                        .unwrap_or_else(|| event.to_string())
                })
                .collect());
        }
    }
    Ok(Vec::new())
}

fn decode_event_string(event: &str) -> String {
    match BASE64.decode(event) {
        Ok(bytes) => {
            let decoded = String::from_utf8_lossy(&bytes);
            if decoded.chars().any(|ch| !ch.is_control()) {
                decoded.into_owned()
            } else {
                event.to_string()
            }
        }
        Err(_) => event.to_string(),
    }
}

fn extract_simulation_errors(result: &serde_json::Value) -> Vec<String> {
    if let Some(error) = result.get("error") {
        return vec![error.to_string()];
    }

    result
        .get("results")
        .and_then(|results| results.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("error").map(|err| err.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn extract_transaction_hash(result: &serde_json::Value) -> Result<String> {
    // Extract transaction hash from submission result
    if let Some(hash) = result.get("hash") {
        Ok(hash.as_str().unwrap_or("unknown").to_string())
    } else {
        Ok("mock_tx_hash_12345".to_string())
    }
}

#[allow(dead_code)]
fn describe_executable(executable: &ContractExecutable) -> (String, Option<String>) {
    match executable {
        ContractExecutable::Wasm(hash) => ("Wasm".to_string(), Some(format_hash(hash))),
        ContractExecutable::StellarAsset => ("StellarAsset".to_string(), None),
    }
}

#[allow(dead_code)]
fn format_durability(durability: ContractDataDurability) -> &'static str {
    match durability {
        ContractDataDurability::Persistent => "Persistent",
        ContractDataDurability::Temporary => "Temporary",
    }
}

#[allow(dead_code)]
fn collect_instance_storage(storage: Option<&ScMap>) -> Vec<ContractStorageEntry> {
    storage.map_or_else(Vec::new, |entries| {
        entries
            .0
            .iter()
            .map(|entry| ContractStorageEntry {
                key: format_scval(&entry.key),
                value: format_scval(&entry.val),
            })
            .collect()
    })
}

#[allow(dead_code)]
fn format_scval(value: &ScVal) -> String {
    match value {
        ScVal::Bool(value) => value.to_string(),
        ScVal::Void => "void".to_string(),
        ScVal::Error(value) => format!("{value:?}"),
        ScVal::U32(value) => value.to_string(),
        ScVal::I32(value) => value.to_string(),
        ScVal::U64(value) => value.to_string(),
        ScVal::I64(value) => value.to_string(),
        ScVal::Timepoint(value) => value.0.to_string(),
        ScVal::Duration(value) => value.0.to_string(),
        ScVal::U128(value) => format!("{value:?}"),
        ScVal::I128(value) => format!("{value:?}"),
        ScVal::U256(value) => format!("{value:?}"),
        ScVal::I256(value) => format!("{value:?}"),
        ScVal::Bytes(value) => format!("0x{}", format_bytes(value.as_ref())),
        ScVal::String(value) => format!("\"{}\"", value.to_utf8_string_lossy()),
        ScVal::Symbol(value) => value.to_utf8_string_lossy(),
        ScVal::Vec(Some(values)) => format!(
            "[{}]",
            values
                .iter()
                .map(format_scval)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ScVal::Vec(None) => "[]".to_string(),
        ScVal::Map(Some(entries)) => format!(
            "{{{}}}",
            entries
                .0
                .iter()
                .map(|entry| format!("{}: {}", format_scval(&entry.key), format_scval(&entry.val)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ScVal::Map(None) => "{}".to_string(),
        ScVal::Address(address) => format_scaddress(address),
        ScVal::LedgerKeyContractInstance => "LedgerKeyContractInstance".to_string(),
        ScVal::LedgerKeyNonce(_) => "LedgerKeyNonce".to_string(),
        ScVal::ContractInstance(instance) => format!(
            "ContractInstance(storage: {} entries)",
            instance
                .storage
                .as_ref()
                .map(|map| map.0.len())
                .unwrap_or(0)
        ),
    }
}

#[allow(dead_code)]
fn format_scaddress(address: &ScAddress) -> String {
    match address {
        ScAddress::Contract(Hash(bytes)) => Contract(*bytes).to_string(),
        ScAddress::Account(AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(bytes)))) => {
            ed25519::PublicKey(*bytes).to_string()
        }
    }
}

#[allow(dead_code)]
fn format_hash(hash: &Hash) -> String {
    format_bytes(&hash.0)
}

#[allow(dead_code)]
fn format_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn extract_rpc_error_message(error: &serde_json::Value) -> String {
    error
        .get("message")
        .and_then(|message| message.as_str())
        .unwrap_or_else(|| error.as_str().unwrap_or("unknown Soroban RPC error"))
        .to_string()
}

// ── Transaction status polling (#689) ─────────────────────────────────────────

/// Terminal or observable status of a Soroban transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TxStatus {
    /// Transaction is in the mempool or awaiting ledger inclusion.
    Pending,
    /// The same transaction hash was already submitted.
    Duplicate,
    /// Transaction was applied but failed (contract error, auth failure, etc.).
    Error,
    /// Transaction was not found — either expired TTL or never accepted.
    NotFound,
    /// Transaction was applied successfully to a ledger.
    Success,
}

impl std::fmt::Display for TxStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            TxStatus::Pending => "PENDING",
            TxStatus::Duplicate => "DUPLICATE",
            TxStatus::Error => "ERROR",
            TxStatus::NotFound => "NOT_FOUND",
            TxStatus::Success => "SUCCESS",
        };
        write!(f, "{}", s)
    }
}

/// Result of polling `getTransaction` to a terminal state (or budget exhaustion).
#[derive(Debug, Serialize, Deserialize)]
pub struct TxStatusResult {
    pub hash: String,
    pub status: TxStatus,
    /// Ledger number the transaction was included in, if known.
    pub ledger: Option<u32>,
    /// Return value from the contract call, if successful.
    pub return_value: Option<String>,
    /// Human-readable error message for non-success statuses.
    pub error_message: Option<String>,
    /// Total number of `getTransaction` RPC calls made.
    pub polls: u32,
}

/// Controls how long `poll_transaction_status` keeps trying.
#[derive(Debug, Clone)]
pub struct PollConfig {
    /// Maximum number of `getTransaction` RPC calls (default: 30).
    pub max_polls: u32,
    /// Milliseconds to wait between calls (default: 2 000 ms).
    pub poll_interval_ms: u64,
}

impl Default for PollConfig {
    fn default() -> Self {
        Self {
            max_polls: 30,
            poll_interval_ms: 2_000,
        }
    }
}

/// Poll `getTransaction` until the transaction reaches a terminal state
/// (`Success`, `Error`, `Duplicate`, or `NotFound`/expired) or the poll
/// budget defined by `config` is exhausted.
///
/// Returns `Err` only on network/RPC protocol failures. All terminal statuses
/// (including `Error` and `NotFound`) are surfaced as `Ok(TxStatusResult)` so
/// callers can display the right message to the user.
pub async fn poll_transaction_status(
    hash: &str,
    network: &str,
    config: &PollConfig,
) -> Result<TxStatusResult> {
    let rpc_url = get_rpc_url(network)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("Failed to build HTTP client for transaction polling")?;

    let mut not_found_streak = 0u32;

    for poll_number in 1..=config.max_polls {
        let request = SorobanRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: poll_number as u64,
            method: "getTransaction".to_string(),
            params: serde_json::json!({ "hash": hash }),
        };

        let response: SorobanRpcResponse<serde_json::Value> = client
            .post(&rpc_url)
            .json(&request)
            .send()
            .await
            .with_context(|| {
                format!(
                    "Poll {}/{} failed for hash {}",
                    poll_number, config.max_polls, hash
                )
            })?
            .json()
            .await
            .context("Failed to decode getTransaction response")?;

        if let Some(rpc_error) = response.error {
            anyhow::bail!(
                "Soroban RPC getTransaction error: {}",
                extract_rpc_error_message(&rpc_error)
            );
        }

        let result = response.result.ok_or_else(|| {
            anyhow::anyhow!("getTransaction returned no result for hash {}", hash)
        })?;

        let status_str = result
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("PENDING");

        match status_str {
            "SUCCESS" => {
                return Ok(TxStatusResult {
                    hash: hash.to_string(),
                    status: TxStatus::Success,
                    ledger: result
                        .get("ledger")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32),
                    return_value: result
                        .get("returnValue")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    error_message: None,
                    polls: poll_number,
                });
            }
            "FAILED" | "ERROR" => {
                let err_msg = result
                    .get("resultXdr")
                    .or_else(|| result.get("errorResult"))
                    .map(|v| v.to_string());
                return Ok(TxStatusResult {
                    hash: hash.to_string(),
                    status: TxStatus::Error,
                    ledger: result
                        .get("ledger")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32),
                    return_value: None,
                    error_message: err_msg,
                    polls: poll_number,
                });
            }
            "DUPLICATE" => {
                return Ok(TxStatusResult {
                    hash: hash.to_string(),
                    status: TxStatus::Duplicate,
                    ledger: None,
                    return_value: None,
                    error_message: Some(
                        "Duplicate transaction: this hash was already submitted.".to_string(),
                    ),
                    polls: poll_number,
                });
            }
            "NOT_FOUND" => {
                // NOT_FOUND on the very first poll is normal propagation lag —
                // wait for a few consecutive NOT_FOUNDs before declaring expiry.
                not_found_streak += 1;
                if not_found_streak >= 3 {
                    return Ok(TxStatusResult {
                        hash: hash.to_string(),
                        status: TxStatus::NotFound,
                        ledger: None,
                        return_value: None,
                        error_message: Some(
                            "Transaction not found — it may have expired or was never accepted \
                             by the network. Check the sequence number and retry."
                                .to_string(),
                        ),
                        polls: poll_number,
                    });
                }
            }
            _ => {
                // PENDING or any unknown status — reset the not-found streak
                not_found_streak = 0;
            }
        }

        if poll_number < config.max_polls {
            tokio::time::sleep(std::time::Duration::from_millis(config.poll_interval_ms)).await;
        }
    }

    // Budget exhausted while still pending.
    Ok(TxStatusResult {
        hash: hash.to_string(),
        status: TxStatus::Pending,
        ledger: None,
        return_value: None,
        error_message: Some(format!(
            "Transaction still pending after {} polls ({} s total). \
             Check later with: starforge tx {}",
            config.max_polls,
            config.max_polls as u64 * config.poll_interval_ms / 1000,
            hash
        )),
        polls: config.max_polls,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn read_fixture(filename: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("soroban_rpc")
            .join(filename);
        fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read fixture {}: {}", path.display(), e))
    }

    #[test]
    fn test_parse_simulate_success() {
        let fixture = read_fixture("simulate_success.json");
        let response: SorobanRpcResponse<serde_json::Value> =
            serde_json::from_str(&fixture).expect("failed to deserialize simulate_success.json");

        assert!(response.error.is_none());
        let result = response.result.expect("missing result in response");

        let return_value = decode_return_value(&result).unwrap();
        assert_eq!(return_value, "success_value");

        // The fee is the RPC's `minResourceFee`, not the CPU instruction count.
        let resources = extract_resources(&result).unwrap();
        assert_eq!(extract_fee(Some(&resources)), 58_181);
        assert_eq!(resources.cpu_instructions, Some(150_000));
        assert_eq!(resources.memory_bytes, Some(2_048));

        let events = extract_events(&result).unwrap();
        assert_eq!(events.len(), 2);
        assert!(events[0].contains("test_key"));
        assert!(events[1].contains("test_key2"));

        let errors = extract_simulation_errors(&result);
        assert!(errors.is_empty());
    }

    #[test]
    fn simulation_result_reports_resources_and_fee_plan() {
        let fixture = read_fixture("simulate_success.json");
        let response: SorobanRpcResponse<serde_json::Value> =
            serde_json::from_str(&fixture).expect("failed to deserialize simulate_success.json");
        let result = response.result.expect("missing result in response");

        let simulation = build_simulation_result(&result).unwrap();

        assert_eq!(simulation.fee, 58_181);
        let resources = simulation.resources.as_ref().expect("resources parsed");
        assert_eq!(resources.cpu_instructions, Some(150_000));
        assert_eq!(resources.memory_bytes, Some(2_048));

        let footprint = resources.footprint.as_ref().expect("footprint decoded");
        assert_eq!(footprint.read_only_entries, 2);
        assert_eq!(footprint.read_write_entries, 1);
        assert_eq!(footprint.read_bytes, 8_192);
        assert_eq!(footprint.write_bytes, 1_024);

        let plan = simulation.fee_plan(20).expect("fee plan");
        assert_eq!(plan.min_resource_fee_stroops, 58_181);
        assert!(plan.recommended_fee_stroops > 58_181);
    }

    #[test]
    fn simulation_without_resource_accounting_falls_back_to_default_fee() {
        // A response from a non-Soroban endpoint: no minResourceFee, no
        // transactionData. The fee must fall back rather than be invented.
        let result = serde_json::json!({ "returnValue": "ok", "events": [] });
        let simulation = build_simulation_result(&result).unwrap();

        assert_eq!(simulation.fee, FALLBACK_FEE_STROOPS);
        assert!(simulation.resources.is_none());
        assert!(simulation
            .errors
            .iter()
            .any(|e| e.contains("resource accounting unavailable")));
    }

    #[test]
    fn simulation_surfaces_host_errors_without_resources() {
        let result = serde_json::json!({
            "error": "HostError: Error(Budget, ExceededLimit)",
            "events": [],
        });
        let simulation = build_simulation_result(&result).unwrap();

        assert!(simulation.resources.is_none());
        assert!(simulation.fee_plan(20).is_none());
        assert!(simulation
            .errors
            .iter()
            .any(|e| e.contains("ExceededLimit")));
    }

    #[test]
    fn test_parse_simulate_error_top_level() {
        let fixture = read_fixture("simulate_error_top_level.json");
        let response: SorobanRpcResponse<serde_json::Value> = serde_json::from_str(&fixture)
            .expect("failed to deserialize simulate_error_top_level.json");

        assert!(response.error.is_none());
        let result = response.result.expect("missing result in response");

        let errors = extract_simulation_errors(&result);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0], "\"Simulation failed due to budget exceeded\"");
    }

    #[test]
    fn test_parse_simulate_error_in_results() {
        let fixture = read_fixture("simulate_error_in_results.json");
        let response: SorobanRpcResponse<serde_json::Value> = serde_json::from_str(&fixture)
            .expect("failed to deserialize simulate_error_in_results.json");

        assert!(response.error.is_none());
        let result = response.result.expect("missing result in response");

        let errors = extract_simulation_errors(&result);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0], "\"Contract call panicked\"");
    }

    #[test]
    fn test_parse_get_ledger_entries_success() {
        let fixture = read_fixture("get_ledger_entries_success.json");
        let response: SorobanRpcResponse<GetLedgerEntriesResult> = serde_json::from_str(&fixture)
            .expect("failed to deserialize get_ledger_entries_success.json");

        assert!(response.error.is_none());
        let result = response.result.expect("missing result in response");

        let inspect_res = parse_contract_inspect_result(
            "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABGHI",
            "testnet",
            result,
        )
        .unwrap();

        assert_eq!(
            inspect_res.contract_id,
            "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABGHI"
        );
        assert_eq!(inspect_res.executable, "Wasm");
        assert_eq!(
            inspect_res.wasm_hash,
            Some("mock_wasm_hash_placeholder".to_string())
        );
        assert_eq!(inspect_res.storage_durability, "Persistent");
        assert_eq!(inspect_res.latest_ledger, 42000);
        assert_eq!(inspect_res.last_modified_ledger_seq, Some(41990));
        assert_eq!(inspect_res.live_until_ledger_seq, Some(45000));
        assert!(inspect_res.instance_storage.is_empty());
    }

    #[test]
    fn test_parse_get_ledger_entries_empty() {
        let fixture = read_fixture("get_ledger_entries_empty.json");
        let response: SorobanRpcResponse<GetLedgerEntriesResult> = serde_json::from_str(&fixture)
            .expect("failed to deserialize get_ledger_entries_empty.json");

        assert!(response.error.is_none());
        let result = response.result.expect("missing result in response");

        let err = parse_contract_inspect_result(
            "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABGHI",
            "testnet",
            result,
        )
        .unwrap_err();

        assert_eq!(
            err.to_string(),
            "Contract 'CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABGHI' was not found on testnet."
        );
    }

    #[test]
    fn test_parse_rpc_error() {
        let fixture = read_fixture("rpc_error.json");
        let response: SorobanRpcResponse<serde_json::Value> =
            serde_json::from_str(&fixture).expect("failed to deserialize rpc_error.json");

        let error = response.error.expect("missing error in response");
        let message = extract_rpc_error_message(&error);
        assert_eq!(message, "Invalid request");
    }

    #[test]
    fn builds_contract_instance_ledger_key() {
        let contract_id = Contract([7; 32]).to_string();
        let key = build_contract_instance_key(&contract_id).unwrap();

        match key {
            LedgerKey::ContractData(data) => {
                assert!(
                    matches!(data.contract, ScAddress::Contract(Hash(bytes)) if bytes == [7; 32])
                );
                assert!(matches!(data.key, ScVal::LedgerKeyContractInstance));
                assert_eq!(data.durability, ContractDataDurability::Persistent);
            }
            other => panic!("unexpected ledger key: {other:?}"),
        }
    }

    #[test]
    fn rejects_invalid_contract_id() {
        let err = build_contract_instance_key("not-a-contract").unwrap_err();
        assert!(err
            .to_string()
            .contains("Expected a Stellar contract strkey"));
    }

    // ── ScVal arg encoding ──────────────────────────────────────────────

    #[test]
    fn encode_string_arg() {
        let result = encode_arguments(&["hello".to_string()], &["string".to_string()]).unwrap();
        assert_eq!(result.len(), 1);
        assert!(
            result[0].contains("hello"),
            "encoded string should contain the value"
        );
    }

    #[test]
    fn encode_symbol_arg() {
        let result = encode_arguments(&["transfer".to_string()], &["symbol".to_string()]).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].contains("transfer"));
    }

    #[test]
    fn encode_int_arg() {
        let result = encode_arguments(&["42".to_string()], &["int".to_string()]).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].contains("42"));
    }

    #[test]
    fn encode_bool_true_arg() {
        let result = encode_arguments(&["true".to_string()], &["bool".to_string()]).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].contains("true"));
    }

    #[test]
    fn encode_bool_false_arg() {
        let result = encode_arguments(&["false".to_string()], &["bool".to_string()]).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].contains("false"));
    }

    #[test]
    fn encode_multiple_args() {
        let args = vec!["hello".to_string(), "99".to_string(), "true".to_string()];
        let types = vec!["string".to_string(), "int".to_string(), "bool".to_string()];
        let result = encode_arguments(&args, &types).unwrap();
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn encode_empty_args() {
        let result = encode_arguments(&[], &[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn encode_invalid_type_errors() {
        let err = encode_arguments(&["x".to_string()], &["unknown_type".to_string()]).unwrap_err();
        assert!(err.to_string().contains("Unsupported argument type"));
    }

    #[test]
    fn encode_invalid_int_errors() {
        let err =
            encode_arguments(&["not_a_number".to_string()], &["int".to_string()]).unwrap_err();
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn encode_invalid_bool_errors() {
        let err = encode_arguments(&["maybe".to_string()], &["bool".to_string()]).unwrap_err();
        assert!(!err.to_string().is_empty());
    }

    // `mockito::Server::new` blocks on its own runtime internally, so these use
    // the async constructor to avoid nesting one runtime inside another.
    #[tokio::test]
    async fn check_soroban_rpc_url_reports_healthy_endpoint() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"jsonrpc":"2.0","id":1,"result":{"status":"healthy"}}"#)
            .create_async()
            .await;

        assert!(check_soroban_rpc_url(&server.url()).await);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn check_soroban_rpc_url_rejects_error_response() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/")
            .with_status(500)
            .create_async()
            .await;

        assert!(!check_soroban_rpc_url(&server.url()).await);
        mock.assert_async().await;
    }
}
