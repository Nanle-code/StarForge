//! Native Soroban deploy via RPC (no stellar CLI required).
//!
//! Flow for each host-function invocation:
//! 1. Build an unsigned `InvokeHostFunction` envelope
//! 2. `simulateTransaction` → footprint / resource fee / auth
//! 3. Assemble simulation data into the envelope
//! 4. Sign via [`wallet_signer`]
//! 5. `sendTransaction` → [`poll_transaction_status`]

use crate::utils::config::{self, WalletEntry};
use crate::utils::horizon;
use crate::utils::soroban::{
    self, poll_transaction_status, PollConfig, SorobanRpcRequest, TxStatus,
};
use crate::utils::tx_builder::{self, MIN_BASE_FEE};
use crate::utils::wallet_signer::{self, SigningRequest};
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use sha2::{Digest, Sha256};
use stellar_strkey::Contract;
use stellar_xdr::curr::{
    AccountId, Asset, BytesM, ContractExecutable, ContractIdPreimage,
    ContractIdPreimageFromAddress, CreateContractArgs, CreateContractArgsV2, Hash, HashIdPreimage,
    HashIdPreimageContractId, HostFunction, InvokeHostFunctionOp, LedgerKey, LedgerKeyContractCode,
    Limits, Memo, Operation, OperationBody, Preconditions, PublicKey, ReadXdr, ScAddress, ScVal,
    SequenceNumber, SorobanAuthorizationEntry, SorobanTransactionData, Transaction,
    TransactionEnvelope, TransactionExt, TransactionV1Envelope, Uint256, VecM, WriteXdr,
};

/// Outcome of a native WASM upload + contract create.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NativeDeployResult {
    pub wasm_hash: String,
    pub contract_id: String,
    pub upload_tx_hash: Option<String>,
    pub create_tx_hash: String,
    /// True when the WASM hash was already present on-chain (upload skipped).
    pub wasm_already_uploaded: bool,
}

/// Compute the SHA-256 hex digest of WASM bytes (Soroban code hash).
pub fn wasm_hash_hex(wasm: &[u8]) -> String {
    hex::encode(Sha256::digest(wasm))
}

fn hash_from_hex(hex_hash: &str) -> Result<Hash> {
    let bytes = hex::decode(hex_hash.trim())
        .with_context(|| format!("invalid WASM hash hex: {hex_hash}"))?;
    if bytes.len() != 32 {
        bail!("WASM hash must be 32 bytes, got {}", bytes.len());
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    Ok(Hash(arr))
}

/// Deterministic SAC / contract ID for a classic asset on `network`.
pub fn asset_contract_id(asset: &Asset, network: &str) -> Result<String> {
    let passphrase = config::get_network_passphrase(network);
    let preimage = HashIdPreimage::ContractId(HashIdPreimageContractId {
        network_id: tx_builder::network_id(&passphrase),
        contract_id_preimage: ContractIdPreimage::Asset(asset.clone()),
    });
    let bytes = preimage
        .to_xdr(Limits::none())
        .context("failed to encode HashIdPreimage for asset contract id")?;
    let digest = Sha256::digest(&bytes);
    Ok(Contract(digest.into()).to_string())
}

/// Parse `native`/`XLM` or `CODE:ISSUER` into an XDR [`Asset`].
pub fn parse_classic_asset(spec: &str) -> Result<Asset> {
    let trimmed = spec.trim();
    let upper = trimmed.to_uppercase();
    if upper == "XLM" || upper == "NATIVE" {
        return Ok(Asset::Native);
    }
    let (code, issuer) = trimmed
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("Invalid asset format. Use XLM, native, or CODE:ISSUER"))?;
    if code.is_empty() || code.len() > 12 {
        bail!(
            "asset code must be 1-12 characters, got '{code}' ({} chars)",
            code.len()
        );
    }
    config::validate_public_key(issuer)?;
    let issuer = AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(
        tx_builder::parse_public_key(issuer)?,
    )));
    if code.len() <= 4 {
        let mut padded = [0u8; 4];
        padded[..code.len()].copy_from_slice(code.as_bytes());
        Ok(Asset::CreditAlphanum4(stellar_xdr::curr::AlphaNum4 {
            asset_code: stellar_xdr::curr::AssetCode4(padded),
            issuer,
        }))
    } else {
        let mut padded = [0u8; 12];
        padded[..code.len()].copy_from_slice(code.as_bytes());
        Ok(Asset::CreditAlphanum12(stellar_xdr::curr::AlphaNum12 {
            asset_code: stellar_xdr::curr::AssetCode12(padded),
            issuer,
        }))
    }
}

/// True when the WASM hash is already present in contract code storage.
pub async fn wasm_already_uploaded(wasm_hash_hex: &str, network: &str) -> Result<bool> {
    let hash = hash_from_hex(wasm_hash_hex)?;
    let key = LedgerKey::ContractCode(LedgerKeyContractCode { hash });
    let key_xdr = key
        .to_xdr_base64(Limits::none())
        .context("encode LedgerKey::ContractCode")?;
    let rpc = soroban::rpc_url(network)?;
    let request = SorobanRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: "getLedgerEntries".to_string(),
        params: serde_json::json!({ "keys": [key_xdr] }),
    };
    let result: serde_json::Value = soroban::rpc_request_with_url(&rpc, request).await?;
    let entries = result
        .get("entries")
        .and_then(|e| e.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(!entries.is_empty())
}

fn build_host_fn_envelope(
    source: &str,
    sequence: i64,
    host_fn: HostFunction,
    auth: VecM<SorobanAuthorizationEntry>,
    fee: u32,
) -> Result<TransactionEnvelope> {
    let op = Operation {
        source_account: None,
        body: OperationBody::InvokeHostFunction(InvokeHostFunctionOp {
            host_function: host_fn,
            auth,
        }),
    };
    let tx = Transaction {
        source_account: tx_builder::parse_muxed_account(source)?,
        fee,
        seq_num: SequenceNumber(sequence),
        cond: Preconditions::None,
        memo: Memo::None,
        operations: VecM::try_from(vec![op]).context("host function operations")?,
        ext: TransactionExt::V0,
    };
    Ok(TransactionEnvelope::Tx(TransactionV1Envelope {
        tx,
        signatures: VecM::default(),
    }))
}

fn envelope_to_base64(env: &TransactionEnvelope) -> Result<String> {
    tx_builder::envelope_to_base64(env)
}

fn assemble_from_simulation(
    envelope: &TransactionEnvelope,
    simulation: &serde_json::Value,
) -> Result<TransactionEnvelope> {
    let result = simulation
        .get("result")
        .cloned()
        .unwrap_or_else(|| simulation.clone());

    if let Some(err) = result.get("error").and_then(|e| e.as_str()) {
        bail!("simulation failed: {err}");
    }

    let tx_data_b64 = result
        .get("transactionData")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("simulation missing transactionData"))?;

    let soroban_data = SorobanTransactionData::from_xdr_base64(tx_data_b64, Limits::none())
        .context("decode simulation transactionData")?;

    let min_resource_fee: i64 = result
        .get("minResourceFee")
        .and_then(|v| {
            v.as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| v.as_u64().map(|n| n as i64))
                .or_else(|| v.as_i64())
        })
        .unwrap_or(0);

    let mut auth_entries: Vec<SorobanAuthorizationEntry> = Vec::new();
    if let Some(results) = result.get("results").and_then(|r| r.as_array()) {
        for entry in results {
            if let Some(auths) = entry.get("auth").and_then(|a| a.as_array()) {
                for auth_b64 in auths.iter().filter_map(|a| a.as_str()) {
                    let decoded =
                        SorobanAuthorizationEntry::from_xdr_base64(auth_b64, Limits::none())
                            .context("decode simulation auth entry")?;
                    auth_entries.push(decoded);
                }
            }
        }
    }

    let TransactionEnvelope::Tx(TransactionV1Envelope { mut tx, signatures }) = envelope.clone()
    else {
        bail!("expected TransactionV1 envelope for Soroban assemble");
    };

    let mut ops = tx.operations.to_vec();
    if let Some(Operation {
        body: OperationBody::InvokeHostFunction(ref mut inv),
        ..
    }) = ops.first_mut()
    {
        inv.auth = VecM::try_from(auth_entries).context("auth entries")?;
    }
    tx.operations = VecM::try_from(ops).context("rebuilt operations")?;

    let inclusion = i64::from(tx.fee.max(MIN_BASE_FEE as u32));
    let total_fee = inclusion
        .saturating_add(min_resource_fee)
        .clamp(1, i64::from(u32::MAX));
    tx.fee = total_fee as u32;
    tx.ext = TransactionExt::V1(soroban_data);

    Ok(TransactionEnvelope::Tx(TransactionV1Envelope {
        tx,
        signatures,
    }))
}

async fn next_sequence(public_key: &str, network: &str) -> Result<i64> {
    let account = horizon::fetch_account(public_key, network).await?;
    let seq: i64 = account.sequence.parse().context("parse account sequence")?;
    Ok(seq + 1)
}

async fn simulate_and_assemble(
    envelope: &TransactionEnvelope,
    network: &str,
) -> Result<TransactionEnvelope> {
    let xdr = envelope_to_base64(envelope)?;
    let raw = soroban::simulate_envelope(&xdr, network).await?;
    assemble_from_simulation(envelope, &raw)
}

async fn sign_submit_poll(
    envelope: TransactionEnvelope,
    network: &str,
    signing: &SigningRequest,
    label: &str,
) -> Result<String> {
    let unsigned = envelope_to_base64(&envelope)?;
    let request = signing.clone().for_contract_deploy();
    let signed = wallet_signer::sign_transaction_xdr(&unsigned, &request)
        .with_context(|| format!("signing {label}"))?;

    let rpc = soroban::rpc_url(network)?;
    let send = SorobanRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: "sendTransaction".to_string(),
        params: serde_json::json!({ "transaction": signed }),
    };
    let sent: serde_json::Value = soroban::rpc_request_with_url(&rpc, send).await?;
    let hash = sent
        .get("hash")
        .and_then(|h| h.as_str())
        .ok_or_else(|| anyhow::anyhow!("sendTransaction missing hash: {sent}"))?
        .to_string();

    let status = poll_transaction_status(&hash, network, PollConfig::default()).await?;
    match status.status {
        TxStatus::Success | TxStatus::Duplicate => Ok(hash),
        other => bail!(
            "{label} ended with status {other}: {}",
            status.error_message.unwrap_or_default()
        ),
    }
}

fn upload_host_fn(wasm: &[u8]) -> Result<HostFunction> {
    let bytes = BytesM::try_from(wasm.to_vec()).context("WASM exceeds XDR BytesM limit")?;
    Ok(HostFunction::UploadContractWasm(bytes))
}

fn create_host_fn(
    deployer: &str,
    salt: [u8; 32],
    wasm_hash: &Hash,
    constructor_args: Vec<ScVal>,
) -> Result<HostFunction> {
    let address = ScAddress::Account(AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(
        tx_builder::parse_public_key(deployer)?,
    ))));
    let preimage = ContractIdPreimage::Address(ContractIdPreimageFromAddress {
        address,
        salt: Uint256(salt),
    });
    if constructor_args.is_empty() {
        Ok(HostFunction::CreateContract(CreateContractArgs {
            contract_id_preimage: preimage,
            executable: ContractExecutable::Wasm(wasm_hash.clone()),
        }))
    } else {
        Ok(HostFunction::CreateContractV2(CreateContractArgsV2 {
            contract_id_preimage: preimage,
            executable: ContractExecutable::Wasm(wasm_hash.clone()),
            constructor_args: VecM::try_from(constructor_args).context("constructor args")?,
        }))
    }
}

fn create_sac_host_fn(asset: Asset) -> HostFunction {
    HostFunction::CreateContract(CreateContractArgs {
        contract_id_preimage: ContractIdPreimage::Asset(asset),
        executable: ContractExecutable::StellarAsset,
    })
}

fn contract_id_from_address_preimage(
    deployer: &str,
    salt: [u8; 32],
    network: &str,
) -> Result<String> {
    let passphrase = config::get_network_passphrase(network);
    let address = ScAddress::Account(AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(
        tx_builder::parse_public_key(deployer)?,
    ))));
    let preimage = HashIdPreimage::ContractId(HashIdPreimageContractId {
        network_id: tx_builder::network_id(&passphrase),
        contract_id_preimage: ContractIdPreimage::Address(ContractIdPreimageFromAddress {
            address,
            salt: Uint256(salt),
        }),
    });
    let bytes = preimage
        .to_xdr(Limits::none())
        .context("encode contract id preimage")?;
    Ok(Contract(Sha256::digest(&bytes).into()).to_string())
}

/// Upload WASM (unless already present) and create a contract instance.
pub async fn deploy_wasm_native(
    wasm: &[u8],
    wallet: &WalletEntry,
    network: &str,
    signing: &SigningRequest,
    salt: [u8; 32],
    constructor_args: Vec<ScVal>,
) -> Result<NativeDeployResult> {
    crate::utils::network_guard::verify(network).await?;

    let wasm_hash_hex = wasm_hash_hex(wasm);
    let wasm_hash = hash_from_hex(&wasm_hash_hex)?;
    let already = wasm_already_uploaded(&wasm_hash_hex, network).await?;

    let mut upload_tx_hash = None;
    let mut seq = next_sequence(&wallet.public_key, network).await?;

    if !already {
        let unsigned = build_host_fn_envelope(
            &wallet.public_key,
            seq,
            upload_host_fn(wasm)?,
            VecM::default(),
            MIN_BASE_FEE as u32,
        )?;
        let assembled = simulate_and_assemble(&unsigned, network).await?;
        let hash = sign_submit_poll(assembled, network, signing, "WASM upload").await?;
        upload_tx_hash = Some(hash);
        seq += 1;
    }

    let unsigned = build_host_fn_envelope(
        &wallet.public_key,
        seq,
        create_host_fn(&wallet.public_key, salt, &wasm_hash, constructor_args)?,
        VecM::default(),
        MIN_BASE_FEE as u32,
    )?;
    let assembled = simulate_and_assemble(&unsigned, network).await?;
    let create_tx_hash = sign_submit_poll(assembled, network, signing, "contract create").await?;

    let contract_id = contract_id_from_address_preimage(&wallet.public_key, salt, network)?;

    Ok(NativeDeployResult {
        wasm_hash: wasm_hash_hex,
        contract_id,
        upload_tx_hash,
        create_tx_hash,
        wasm_already_uploaded: already,
    })
}

/// Deploy (wrap) a Stellar Asset Contract for a classic asset if missing.
pub async fn wrap_asset_native(
    asset: Asset,
    wallet: &WalletEntry,
    network: &str,
    signing: &SigningRequest,
) -> Result<NativeDeployResult> {
    crate::utils::network_guard::verify(network).await?;
    let contract_id = asset_contract_id(&asset, network)?;

    // If the SAC instance already exists, treat wrap as idempotent success.
    if soroban::inspect_contract(&contract_id, network)
        .await
        .is_ok()
    {
        return Ok(NativeDeployResult {
            wasm_hash: String::new(),
            contract_id,
            upload_tx_hash: None,
            create_tx_hash: String::new(),
            wasm_already_uploaded: true,
        });
    }

    let seq = next_sequence(&wallet.public_key, network).await?;
    let unsigned = build_host_fn_envelope(
        &wallet.public_key,
        seq,
        create_sac_host_fn(asset),
        VecM::default(),
        MIN_BASE_FEE as u32,
    )?;
    let assembled = simulate_and_assemble(&unsigned, network).await?;
    let create_tx_hash = sign_submit_poll(assembled, network, signing, "SAC wrap").await?;

    Ok(NativeDeployResult {
        wasm_hash: String::new(),
        contract_id,
        upload_tx_hash: None,
        create_tx_hash,
        wasm_already_uploaded: false,
    })
}

/// Encode simple constructor args (`type:value`) into [`ScVal`]s.
pub fn parse_constructor_args(raw: &[String]) -> Result<Vec<ScVal>> {
    let mut out = Vec::new();
    for item in raw {
        if let Some((ty, val)) = item.split_once(':') {
            let sc = match ty {
                "string" => ScVal::String(stellar_xdr::curr::ScString(
                    val.as_bytes()
                        .to_vec()
                        .try_into()
                        .context("constructor string")?,
                )),
                "symbol" => ScVal::Symbol(stellar_xdr::curr::ScSymbol(
                    val.as_bytes()
                        .to_vec()
                        .try_into()
                        .context("constructor symbol")?,
                )),
                "bool" => ScVal::Bool(val.parse().context("constructor bool")?),
                "i64" | "int" => ScVal::I64(val.parse().context("constructor i64")?),
                "u32" => ScVal::U32(val.parse().context("constructor u32")?),
                "address" => {
                    if val.starts_with('C') {
                        let c = Contract::from_string(val).context("constructor contract")?;
                        ScVal::Address(ScAddress::Contract(Hash(c.0)))
                    } else {
                        let pk = tx_builder::parse_public_key(val)?;
                        ScVal::Address(ScAddress::Account(AccountId(
                            PublicKey::PublicKeyTypeEd25519(Uint256(pk)),
                        )))
                    }
                }
                "xdr" => {
                    let bytes = BASE64.decode(val).context("constructor xdr base64")?;
                    ScVal::from_xdr(bytes, Limits::none()).context("decode constructor ScVal")?
                }
                other => bail!("unsupported constructor arg type '{other}' (use string,symbol,bool,i64,u32,address,xdr)"),
            };
            out.push(sc);
        } else {
            // bare string
            out.push(ScVal::String(stellar_xdr::curr::ScString(
                item.as_bytes()
                    .to_vec()
                    .try_into()
                    .context("constructor string")?,
            )));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_xlm_asset_parses() {
        assert!(matches!(parse_classic_asset("XLM").unwrap(), Asset::Native));
        assert!(matches!(
            parse_classic_asset("native").unwrap(),
            Asset::Native
        ));
    }

    #[test]
    fn issued_asset_contract_id_is_stable() {
        let issuer = "GBRPYHIL2CI3FNQ4BXLFMNDLFJUNPU2HY3ZMFSHONUCEOASW7QC7OX2H";
        let a = parse_classic_asset(&format!("USDC:{issuer}")).unwrap();
        let id1 = asset_contract_id(&a, "testnet").unwrap();
        let id2 = asset_contract_id(&a, "testnet").unwrap();
        assert_eq!(id1, id2);
        assert!(id1.starts_with('C'));
        assert_eq!(id1.len(), 56);
    }

    #[test]
    fn xlm_contract_id_differs_by_network() {
        let a = Asset::Native;
        let test = asset_contract_id(&a, "testnet").unwrap();
        let main = asset_contract_id(&a, "mainnet").unwrap();
        assert_ne!(test, main);
    }

    #[test]
    fn wasm_hash_hex_is_sha256() {
        let h = wasm_hash_hex(b"hello");
        assert_eq!(
            h,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }
}
