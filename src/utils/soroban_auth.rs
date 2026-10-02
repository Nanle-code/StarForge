use crate::utils::hardware_wallet::{self, HardwareWalletKind};
use crate::utils::{config, wallet_signer};
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;
use stellar_strkey::ed25519::PrivateKey;
use stellar_xdr::curr::{
    Hash, HashIdPreimage, HashIdPreimageSorobanAuthorization, Limits, ReadXdr, ScAddress, ScVal,
    SorobanAuthorizationEntry, SorobanCredentials, Uint256, WriteXdr,
};

pub const DEFAULT_SIGNATURE_TTL_LEDGERS: u32 = 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthEntryRecord {
    pub address: String,
    pub entry_xdr: String,
    pub payload_hash: String,
    pub signature_expiration_ledger: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthEntryBundle {
    pub network: String,
    pub latest_ledger: u32,
    pub entries: Vec<AuthEntryRecord>,
}

/// Extract Soroban authorization entries returned by `simulateTransaction`.
pub fn parse_simulation_auth_entries(result: &Value, network: &str) -> Result<AuthEntryBundle> {
    let latest_ledger = result
        .get("latestLedger")
        .and_then(Value::as_u64)
        .and_then(|ledger| u32::try_from(ledger).ok())
        .context("Simulation response omitted a valid latestLedger")?;
    let results = result
        .get("results")
        .and_then(Value::as_array)
        .context("Simulation response omitted results")?;
    let expiration = latest_ledger
        .checked_add(DEFAULT_SIGNATURE_TTL_LEDGERS)
        .context("Simulation ledger is too large to set an authorization expiration")?;
    let network_id = network_id(network);
    let mut entries = Vec::new();

    for (result_index, operation_result) in results.iter().enumerate() {
        let Some(auth_entries) = operation_result.get("auth").and_then(Value::as_array) else {
            continue;
        };
        for (auth_index, auth_value) in auth_entries.iter().enumerate() {
            let encoded = auth_value.as_str().with_context(|| {
                format!("Simulation auth entry {result_index}:{auth_index} is not XDR text")
            })?;
            let bytes = BASE64.decode(encoded).with_context(|| {
                format!("Simulation auth entry {result_index}:{auth_index} is not valid base64")
            })?;
            let mut entry = SorobanAuthorizationEntry::from_xdr(bytes.as_slice(), Limits::none())
                .with_context(|| {
                format!("Unable to decode simulation auth entry {result_index}:{auth_index}")
            })?;

            let (address, nonce) = match &entry.credentials {
                SorobanCredentials::Address(credentials) => {
                    (account_address(&credentials.address)?, credentials.nonce)
                }
                SorobanCredentials::SourceAccount => continue,
            };

            if let SorobanCredentials::Address(credentials) = &mut entry.credentials {
                credentials.signature_expiration_ledger = expiration;
            }
            let payload_hash =
                authorization_payload_hash(&network_id, nonce, expiration, &entry.root_invocation)?;
            let entry_xdr = BASE64.encode(entry.to_xdr(Limits::none())?);

            entries.push(AuthEntryRecord {
                address,
                entry_xdr,
                payload_hash,
                signature_expiration_ledger: expiration,
                signature: None,
            });
        }
    }

    Ok(AuthEntryBundle {
        network: network.to_string(),
        latest_ledger,
        entries,
    })
}

/// Sign entries for wallets explicitly selected by name. Wallet secrets are
/// resolved through the shared wallet signer, including encrypted entries.
pub fn sign_bundle_with_wallets(
    bundle: &mut AuthEntryBundle,
    wallets: &[config::WalletEntry],
    signer_names: &[String],
    hardware: Option<HardwareWalletKind>,
    hd_path: &str,
) -> Result<()> {
    for entry in &mut bundle.entries {
        if entry.signature.is_some() {
            continue;
        }
        let wallet = signer_names
            .iter()
            .filter_map(|name| wallets.iter().find(|wallet| wallet.name == *name))
            .find(|wallet| wallet.public_key == entry.address);
        let payload_hash = hex::decode(&entry.payload_hash)
            .context("Authorization payload hash is not valid hexadecimal")?;
        let signature = if let Some(wallet) = wallet {
            let secret = wallet_signer::resolve_local_secret(wallet, &wallet.name)?;
            let private_key = PrivateKey::from_string(secret.as_str()).with_context(|| {
                format!("Wallet '{}' contains an invalid secret key", wallet.name)
            })?;
            SigningKey::from_bytes(&private_key.0)
                .sign(&payload_hash)
                .to_bytes()
                .to_vec()
        } else if let Some(kind) = hardware {
            let device_address = hardware_wallet::get_stellar_address(kind, hd_path)
                .map_err(|err| hardware_wallet::map_signing_error(err, kind))?;
            if device_address != entry.address {
                anyhow::bail!(
                    "Hardware wallet address {} does not match required Soroban authorization address {}",
                    device_address,
                    entry.address
                );
            }
            hardware_wallet::sign_transaction(
                kind,
                hd_path,
                &payload_hash,
                &config::get_network_passphrase(&bundle.network),
            )
            .map_err(|err| hardware_wallet::map_signing_error(err, kind))?
        } else {
            anyhow::bail!(
                "No --auth-signer wallet matches required Soroban authorization address {}",
                entry.address
            );
        };
        entry.signature = Some(hex::encode(signature));
        attach_signature(entry)?;
    }
    Ok(())
}

/// Import detached signatures from a JSON bundle and attach them to auth XDR.
pub fn import_signatures(bundle: &mut AuthEntryBundle, input: &Path) -> Result<()> {
    let imported: AuthEntryBundle = serde_json::from_slice(&std::fs::read(input)?)?;
    if imported.network != bundle.network {
        anyhow::bail!(
            "Auth signature bundle network mismatch: expected '{}', got '{}'",
            bundle.network,
            imported.network
        );
    }
    let latest_ledger = bundle.latest_ledger;
    let expected_addresses: std::collections::HashSet<_> = bundle
        .entries
        .iter()
        .map(|entry| entry.address.as_str())
        .collect();
    let imported_addresses: std::collections::HashSet<_> = imported
        .entries
        .iter()
        .map(|entry| entry.address.as_str())
        .collect();
    if expected_addresses != imported_addresses {
        anyhow::bail!("Imported auth bundle signer addresses do not match the active simulation");
    }
    let network_id = network_id(&bundle.network);
    for entry in &imported.entries {
        if entry.signature_expiration_ledger <= latest_ledger {
            anyhow::bail!(
                "Imported Soroban authorization for {} expired at ledger {}; active simulation is at ledger {}",
                entry.address,
                entry.signature_expiration_ledger,
                latest_ledger
            );
        }
        let signature = entry.signature.as_deref().with_context(|| {
            format!(
                "Imported auth bundle has no signature for {}",
                entry.address
            )
        })?;
        let bytes = BASE64
            .decode(&entry.entry_xdr)
            .context("Imported auth entry XDR is not valid base64")?;
        let auth_xdr = SorobanAuthorizationEntry::from_xdr(bytes.as_slice(), Limits::none())?;
        let SorobanCredentials::Address(credentials) = &auth_xdr.credentials else {
            anyhow::bail!(
                "Imported auth entry for {} is not address credentials",
                entry.address
            );
        };
        if credentials.signature_expiration_ledger != entry.signature_expiration_ledger
            || account_address(&credentials.address)? != entry.address
        {
            anyhow::bail!(
                "Imported auth entry metadata does not match its XDR for {}",
                entry.address
            );
        }
        let payload_hash = authorization_payload_hash(
            &network_id,
            credentials.nonce,
            credentials.signature_expiration_ledger,
            &auth_xdr.root_invocation,
        )?;
        if payload_hash != entry.payload_hash {
            anyhow::bail!(
                "Imported auth payload does not match its XDR for {}",
                entry.address
            );
        }
        let public_key = stellar_strkey::ed25519::PublicKey::from_string(&entry.address)
            .context("Auth entry contains an invalid signer address")?;
        let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&public_key.0)?;
        let signature = ed25519_dalek::Signature::from_slice(&hex::decode(signature)?)
            .context("Imported auth signature must be 64 bytes")?;
        verifying_key
            .verify_strict(&hex::decode(payload_hash)?, &signature)
            .context("Imported auth signature does not match the required address and payload")?;
    }
    bundle.entries = imported.entries;
    Ok(())
}

fn network_id(network: &str) -> Hash {
    Hash(Sha256::digest(config::get_network_passphrase(network).as_bytes()).into())
}

fn authorization_payload_hash(
    network_id: &Hash,
    nonce: i64,
    expiration: u32,
    invocation: &stellar_xdr::curr::SorobanAuthorizedInvocation,
) -> Result<String> {
    let preimage = HashIdPreimage::SorobanAuthorization(HashIdPreimageSorobanAuthorization {
        network_id: network_id.clone(),
        nonce,
        signature_expiration_ledger: expiration,
        invocation: invocation.clone(),
    });
    Ok(hex::encode(Sha256::digest(
        preimage.to_xdr(Limits::none())?,
    )))
}

pub fn export_bundle(bundle: &AuthEntryBundle, output: &Path) -> Result<()> {
    std::fs::write(output, serde_json::to_vec_pretty(bundle)?)?;
    Ok(())
}

pub fn same_authorization_call(left: &AuthEntryRecord, right: &AuthEntryRecord) -> Result<bool> {
    let left_bytes = BASE64.decode(&left.entry_xdr)?;
    let right_bytes = BASE64.decode(&right.entry_xdr)?;
    let left_entry = SorobanAuthorizationEntry::from_xdr(left_bytes.as_slice(), Limits::none())?;
    let right_entry = SorobanAuthorizationEntry::from_xdr(right_bytes.as_slice(), Limits::none())?;
    let (
        SorobanCredentials::Address(left_credentials),
        SorobanCredentials::Address(right_credentials),
    ) = (&left_entry.credentials, &right_entry.credentials)
    else {
        return Ok(false);
    };
    Ok(left.address == right.address
        && left_credentials.nonce == right_credentials.nonce
        && left_entry.root_invocation == right_entry.root_invocation)
}

fn attach_signature(record: &mut AuthEntryRecord) -> Result<()> {
    let Some(signature) = record.signature.as_deref() else {
        return Ok(());
    };
    let signature_bytes = hex::decode(signature).context("Auth signature is not valid hex")?;
    let bytes = BASE64
        .decode(&record.entry_xdr)
        .context("Auth entry XDR is not valid base64")?;
    let mut entry = SorobanAuthorizationEntry::from_xdr(bytes.as_slice(), Limits::none())?;
    let SorobanCredentials::Address(credentials) = &mut entry.credentials else {
        anyhow::bail!("Cannot attach a signature to source-account credentials")
    };
    if credentials.signature_expiration_ledger != record.signature_expiration_ledger {
        anyhow::bail!("Auth entry expiration ledger does not match the exported bundle")
    }
    credentials.signature = ScVal::Vec(Some(
        vec![ScVal::Bytes(signature_bytes.try_into()?)].try_into()?,
    ));
    record.entry_xdr = BASE64.encode(entry.to_xdr(Limits::none())?);
    Ok(())
}

fn account_address(address: &ScAddress) -> Result<String> {
    match address {
        ScAddress::Account(account) => match &account.0 {
            stellar_xdr::curr::PublicKey::PublicKeyTypeEd25519(Uint256(bytes)) => {
                Ok(stellar_strkey::ed25519::PublicKey(*bytes).to_string())
            }
        },
        ScAddress::Contract(_) => anyhow::bail!(
            "Soroban authorization address is a contract account; this signer supports Ed25519 G-addresses only"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_auth_results_produce_empty_bundle() {
        let response = serde_json::json!({"latestLedger": 1234, "results": [{"auth": []}]});
        let bundle = parse_simulation_auth_entries(&response, "testnet").unwrap();
        assert_eq!(bundle.latest_ledger, 1234);
        assert!(bundle.entries.is_empty());
    }

    #[test]
    fn missing_latest_ledger_is_rejected() {
        let response = serde_json::json!({"results": []});
        let error = parse_simulation_auth_entries(&response, "testnet").unwrap_err();
        assert!(error.to_string().contains("latestLedger"));
    }

    #[test]
    fn missing_signer_error_names_the_required_address() {
        let address = stellar_strkey::ed25519::PublicKey([7; 32]).to_string();
        let mut bundle = AuthEntryBundle {
            network: "testnet".to_string(),
            latest_ledger: 100,
            entries: vec![AuthEntryRecord {
                address: address.clone(),
                entry_xdr: String::new(),
                payload_hash: "00".repeat(32),
                signature_expiration_ledger: 110,
                signature: None,
            }],
        };
        let error =
            sign_bundle_with_wallets(&mut bundle, &[], &[], None, "m/44'/148'/0'").unwrap_err();
        assert!(error.to_string().contains(&address));
    }

    #[test]
    fn exported_bundle_signature_round_trips_and_verifies() {
        let signing_key = SigningKey::from_bytes(&[19; 32]);
        let address =
            stellar_strkey::ed25519::PublicKey(signing_key.verifying_key().to_bytes()).to_string();
        let payload = [3; 32];
        let signature = signing_key.sign(&payload);
        let bundle = AuthEntryBundle {
            network: "testnet".to_string(),
            latest_ledger: 100,
            entries: vec![AuthEntryRecord {
                address,
                entry_xdr: String::new(),
                payload_hash: hex::encode(payload),
                signature_expiration_ledger: 110,
                signature: Some(hex::encode(signature.to_bytes())),
            }],
        };
        let path =
            std::env::temp_dir().join(format!("starforge-auth-{}.json", uuid::Uuid::new_v4()));
        export_bundle(&bundle, &path).unwrap();
        let imported: AuthEntryBundle =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(imported, bundle);
    }
}
