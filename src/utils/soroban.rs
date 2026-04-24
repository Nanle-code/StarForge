use crate::utils::config::WalletEntry;
use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use stellar_strkey::{ed25519, Contract};
use stellar_xdr::curr::{
    AccountId, ContractDataDurability, ContractExecutable, Hash, LedgerEntryData, LedgerKey,
    LedgerKeyContractData, Limits, PublicKey, ReadXdr, ScAddress, ScMap, ScString, ScSymbol, ScVal,
    Uint256, WriteXdr,
};

// Helpers to replace missing to_xdr_base64 / from_xdr_base64 methods
fn to_xdr_base64<T: WriteXdr>(val: &T, limits: Limits) -> Result<String> {
    let bytes = val
        .to_xdr(limits)
        .map_err(|e| anyhow::anyhow!("XDR serialization failed: {:?}", e))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
}

fn from_xdr_base64<T: ReadXdr>(s: &str, limits: Limits) -> Result<T> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| anyhow::anyhow!("Base64 decode failed: {}", e))?;
    T::from_xdr(&bytes, limits).map_err(|e| anyhow::anyhow!("XDR deserialization failed: {:?}", e))
}

fn from_xdr_base64<T: ReadXdr>(s: &str, limits: Limits) -> Result<T> {
    let bytes = base64::decode(s).map_err(|e| anyhow::anyhow!("Base64 decode failed: {}", e))?;
    T::from_xdr(&bytes, limits).map_err(|e| anyhow::anyhow!("XDR deserialization failed: {:?}", e))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SimulationResult {
    pub return_value: String,
    pub fee: u64,
    pub events: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionResult {
    pub hash: String,
    pub return_value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
    xdr: String,
    #[serde(rename = "lastModifiedLedgerSeq")]
    last_modified_ledger_seq: Option<u32>,
    #[serde(rename = "liveUntilLedgerSeq")]
    live_until_ledger_seq: Option<u32>,
}

pub fn simulate_transaction(
    contract_id: &str,
    function: &str,
    args: &[String],
    arg_types: &[String],
    network: &str,
) -> Result<SimulationResult> {
    let rpc_url = get_rpc_url(network);

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
    let result: serde_json::Value =
        rpc_request_with_url(&rpc_url, request).context("Simulation request failed")?;

    // Parse the simulation result
    let return_value = decode_return_value(&result)?;
    let fee = extract_fee(&result)?;
    let events = extract_events(&result)?;

    Ok(SimulationResult {
        return_value,
        fee,
        events,
    })
}

pub fn submit_transaction(
    contract_id: &str,
    function: &str,
    args: &[String],
    arg_types: &[String],
    network: &str,
    wallet: &WalletEntry,
) -> Result<TransactionResult> {
    let rpc_url = get_rpc_url(network);

    // Convert arguments to XDR ScVal format
    let xdr_args = encode_arguments(args, arg_types)?;

    // Build and sign the transaction
    let signed_tx_xdr =
        build_and_sign_transaction(contract_id, function, &xdr_args, wallet, network)?;

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
    let result: serde_json::Value =
        rpc_request_with_url(&rpc_url, request).context("Transaction submission failed")?;

    // Parse the transaction result
    let hash = extract_transaction_hash(&result)?;
    let return_value = decode_return_value(&result)?;

    Ok(TransactionResult { hash, return_value })
}

pub fn inspect_contract(contract_id: &str, network: &str) -> Result<ContractInspectResult> {
    let request = SorobanRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: "getLedgerEntries".to_string(),
        params: serde_json::json!({
            "keys": [to_xdr_base64(&build_contract_instance_key(contract_id)?, Limits::none())?],
            "xdrFormat": "base64",
        }),
    };

    let response: GetLedgerEntriesResult = rpc_request_with_url(&get_rpc_url(network), request)
        .with_context(|| {
            format!(
                "Failed to inspect contract '{}' on {}",
                contract_id, network
            )
        })?;

    parse_contract_inspect_result(contract_id, network, response)
}

fn get_rpc_url(network: &str) -> String {
    match network {
        "mainnet" => "https://mainnet.sorobanrpc.com".to_string(),
        _ => "https://soroban-testnet.stellar.org".to_string(),
    }
}

fn rpc_request_with_url<T>(rpc_url: &str, request: SorobanRpcRequest) -> Result<T>
where
    T: DeserializeOwned,
{
    let response: SorobanRpcResponse<T> = ureq::post(rpc_url)
        .set("Content-Type", "application/json")
        .send_json(&request)
        .with_context(|| format!("Soroban RPC request to {} failed", rpc_url))?
        .into_json()
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

    let ledger_entry = from_xdr_base64::<LedgerEntryData>(&entry.xdr, Limits::none())
        .context("Failed to decode contract ledger entry from Soroban RPC")?;

    let contract_data = match ledger_entry {
        LedgerEntryData::ContractData(contract_data) => contract_data,
        other => {
            anyhow::bail!(
                "Unexpected ledger entry returned for '{}': {}",
                contract_id,
                other.name()
            );
        }
    };

    let instance = match &contract_data.val {
        ScVal::ContractInstance(instance) => instance,
        _ => {
            anyhow::bail!(
                "Contract '{}' did not return a contract instance entry.",
                contract_id
            );
        }
    };

    let (executable, wasm_hash) = describe_executable(&instance.executable);

    Ok(ContractInspectResult {
        contract_id: contract_id.to_string(),
        executable,
        wasm_hash,
        storage_durability: format_durability(contract_data.durability).to_string(),
        latest_ledger,
        last_modified_ledger_seq: entry.last_modified_ledger_seq,
        live_until_ledger_seq: entry.live_until_ledger_seq,
        instance_storage: collect_instance_storage(instance.storage.as_ref()),
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
    _network: &str,
) -> Result<String> {
    // This is a simplified mock implementation
    // In production, you'd use stellar-sdk to build and sign proper transaction XDR
    Ok(format!(
        "signed_mock_transaction_xdr_{}_{}_{}_{}",
        contract_id,
        function,
        args.len(),
        wallet.name
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

fn extract_fee(result: &serde_json::Value) -> Result<u64> {
    // Extract fee from simulation result
    if let Some(cost) = result.get("cost") {
        if let Some(fee) = cost.get("cpuInsns") {
            return Ok(fee.as_u64().unwrap_or(100000)); // Default fee
        }
    }
    Ok(100000) // Default fee in stroops
}

fn extract_events(result: &serde_json::Value) -> Result<Vec<String>> {
    // Extract events from simulation result
    if let Some(events) = result.get("events") {
        if let Some(events_array) = events.as_array() {
            return Ok(events_array.iter().map(|e| e.to_string()).collect());
        }
    }
    Ok(Vec::new())
}

fn extract_transaction_hash(result: &serde_json::Value) -> Result<String> {
    // Extract transaction hash from submission result
    if let Some(hash) = result.get("hash") {
        Ok(hash.as_str().unwrap_or("unknown").to_string())
    } else {
        Ok("mock_tx_hash_12345".to_string())
    }
}

fn describe_executable(executable: &ContractExecutable) -> (String, Option<String>) {
    match executable {
        ContractExecutable::Wasm(hash) => ("Wasm".to_string(), Some(format_hash(hash))),
        ContractExecutable::StellarAsset => ("StellarAsset".to_string(), None),
    }
}

fn format_durability(durability: ContractDataDurability) -> &'static str {
    match durability {
        ContractDataDurability::Persistent => "Persistent",
        ContractDataDurability::Temporary => "Temporary",
    }
}

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

fn format_scaddress(address: &ScAddress) -> String {
    match address {
        ScAddress::Contract(Hash(bytes)) => Contract(*bytes).to_string(),
        ScAddress::Account(AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(bytes)))) => {
            ed25519::PublicKey(*bytes).to_string()
        }
    }
}

fn format_hash(hash: &Hash) -> String {
    format_bytes(&hash.0)
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use stellar_xdr::curr::{ContractDataEntry, ExtensionPoint, ScContractInstance, ScMapEntry};

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
    fn parses_contract_inspection_response() {
        let contract_id = Contract([9; 32]).to_string();
        let response = GetLedgerEntriesResult {
            latest_ledger: 912345,
            entries: vec![RpcLedgerEntry {
                xdr: LedgerEntryData::ContractData(ContractDataEntry {
                    ext: ExtensionPoint::V0,
                    contract: ScAddress::Contract(Hash([9; 32])),
                    key: ScVal::LedgerKeyContractInstance,
                    durability: ContractDataDurability::Persistent,
                    val: ScVal::ContractInstance(ScContractInstance {
                        executable: ContractExecutable::Wasm(Hash([0xab; 32])),
                        storage: Some(
                            vec![
                                ScMapEntry {
                                    key: ScVal::Symbol(ScSymbol(
                                        "COUNTER".as_bytes().try_into().unwrap(),
                                    )),
                                    val: ScVal::I64(42),
                                },
                                ScMapEntry {
                                    key: ScVal::Symbol(ScSymbol(
                                        "OWNER".as_bytes().try_into().unwrap(),
                                    )),
                                    val: ScVal::Address(ScAddress::Contract(Hash([3; 32]))),
                                },
                            ]
                            .try_into()
                            .map(ScMap)
                            .unwrap(),
                        ),
                    }),
                })
                .to_xdr_base64(Limits::none())
                .unwrap(),
                last_modified_ledger_seq: Some(912300),
                live_until_ledger_seq: Some(912999),
            }],
        };

        let result = parse_contract_inspect_result(&contract_id, "testnet", response).unwrap();

        assert_eq!(result.contract_id, contract_id);
        assert_eq!(result.executable, "Wasm");
        assert_eq!(
            result.wasm_hash,
            Some("abababababababababababababababababababababababababababababababab".to_string())
        );
        assert_eq!(result.storage_durability, "Persistent");
        assert_eq!(result.latest_ledger, 912345);
        assert_eq!(result.last_modified_ledger_seq, Some(912300));
        assert_eq!(result.live_until_ledger_seq, Some(912999));
        assert_eq!(result.instance_storage.len(), 2);
        assert_eq!(result.instance_storage[0].key, "COUNTER");
        assert_eq!(result.instance_storage[0].value, "42");
        assert_eq!(result.instance_storage[1].key, "OWNER");
        assert_eq!(
            result.instance_storage[1].value,
            Contract([3; 32]).to_string()
        );
    }

    #[test]
    fn rejects_invalid_contract_id() {
        let err = build_contract_instance_key("not-a-contract").unwrap_err();
        assert!(err
            .to_string()
            .contains("Expected a Stellar contract strkey"));
    }
}
