use crate::utils::{config, confirmation, crypto, hardware_wallet, print as p};
use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};
use zeroize::Zeroizing;

/// Describes how a transaction should be signed.
#[derive(Debug, Clone)]
pub struct SigningRequest {
    pub local_secret: Option<Zeroizing<String>>,
    pub hardware: Option<hardware_wallet::HardwareWalletKind>,
    pub hd_path: String,
    pub network: String,
    pub skip_confirm: bool,
}

impl SigningRequest {
    /// Build a signing request from CLI flags and an optional local wallet entry.
    pub fn from_options(
        wallet: Option<&config::WalletEntry>,
        hardware: Option<hardware_wallet::HardwareWalletKind>,
        hd_path: Option<&str>,
        network: &str,
        skip_confirm: bool,
        operation_label: &str,
    ) -> Result<Self> {
        let hd_path = hd_path
            .map(str::to_string)
            .unwrap_or_else(|| hardware_wallet::STELLAR_HD_PATH.to_string());

        if let Some(kind) = hardware {
            let public_key = wallet
                .map(|w| w.public_key.as_str())
                .unwrap_or("(derived from device)");
            prompt_hardware_confirmation(kind, public_key, network, skip_confirm, operation_label)?;
            return Ok(Self {
                local_secret: None,
                hardware: Some(kind),
                hd_path,
                network: network.to_string(),
                skip_confirm,
            });
        }

        let wallet = wallet.ok_or_else(|| {
            anyhow::anyhow!(
                "A wallet is required for local signing. Provide --from/--wallet or use --hardware."
            )
        })?;

        let secret = resolve_local_secret(wallet, &wallet.name)?;
        Ok(Self {
            local_secret: Some(secret),
            hardware: None,
            hd_path,
            network: network.to_string(),
            skip_confirm,
        })
    }

    pub fn local_secret(secret_key: Zeroizing<String>, network: &str) -> Self {
        Self {
            local_secret: Some(secret_key),
            hardware: None,
            hd_path: hardware_wallet::STELLAR_HD_PATH.to_string(),
            network: network.to_string(),
            skip_confirm: true,
        }
    }

    pub fn hardware(
        kind: hardware_wallet::HardwareWalletKind,
        hd_path: &str,
        network: &str,
        skip_confirm: bool,
        public_key: &str,
        operation_label: &str,
    ) -> Result<Self> {
        prompt_hardware_confirmation(kind, public_key, network, skip_confirm, operation_label)?;
        Ok(Self {
            local_secret: None,
            hardware: Some(kind),
            hd_path: hd_path.to_string(),
            network: network.to_string(),
            skip_confirm,
        })
    }
}

/// Prompt the user before initiating a hardware wallet signing session.
pub fn prompt_hardware_confirmation(
    kind: hardware_wallet::HardwareWalletKind,
    public_key: &str,
    network: &str,
    skip_confirm: bool,
    operation_label: &str,
) -> Result<()> {
    if skip_confirm {
        return Ok(());
    }

    let summary = confirmation::OperationSummary::new(
        format!("Hardware Wallet — {}", operation_label),
        network.to_string(),
        confirmation::RiskLevel::High,
    )
    .add("Device", kind.to_string())
    .add("Account", public_key)
    .add("Next step", "Review and approve on your device screen");

    let confirm_config = confirmation::ConfirmationConfig {
        risk_level: confirmation::RiskLevel::High,
        network: network.to_string(),
        skip_confirm: false,
        dry_run: false,
        prompt: Some("Proceed with hardware wallet signing?".to_string()),
        require_type_confirmation: network == "mainnet",
        ..Default::default()
    };

    if !confirmation::confirm_operation(&summary, &confirm_config)? {
        anyhow::bail!("Hardware wallet signing cancelled by user");
    }

    p::info(&format!(
        "Connect your {} and approve the {} on the device screen.",
        kind,
        operation_label.to_lowercase()
    ));
    Ok(())
}

/// Resolve a plaintext secret key from a wallet entry, decrypting when needed.
pub fn resolve_local_secret(
    wallet: &config::WalletEntry,
    wallet_name: &str,
) -> Result<Zeroizing<String>> {
    let sk = wallet.secret_key.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "Wallet '{}' has no local secret key. Use --hardware ledger or --hardware trezor.",
            wallet_name
        )
    })?;

    if !sk.contains(':') && sk.starts_with('S') && sk.len() == 56 {
        return Ok(Zeroizing::new(sk.clone()));
    }

    let pwd = crypto::prompt_password(
        &format!("Enter password to decrypt wallet '{}'", wallet_name),
        false,
    )?;
    Ok(Zeroizing::new(crypto::decrypt_secret(&pwd, sk).map_err(
        |_| {
            anyhow::anyhow!(
                "Incorrect password or unable to decrypt wallet '{}'.",
                wallet_name
            )
        },
    )?))
}

/// Sign a base64-encoded transaction XDR using local or hardware credentials.
///
/// Performs the real XDR work: decodes the envelope, computes the network's
/// signature payload for its variant, signs it, and re-encodes. Both classic
/// (`Tx`) and fee-bump (`TxFeeBump`) envelopes are supported; the payload is
/// chosen by the envelope's own discriminant, so a fee bump is signed as a fee
/// bump rather than as its inner transaction.
pub fn sign_transaction_xdr(transaction_xdr: &str, request: &SigningRequest) -> Result<String> {
    let envelope = crate::utils::tx_builder::envelope_from_base64(transaction_xdr)?;
    let passphrase = config::get_network_passphrase(&request.network);

    if let Some(kind) = request.hardware {
        // The device must sign the same 32-byte base hash we would have signed
        // locally, not the serialized payload bytes, or the network rejects it.
        let preimage =
            crate::utils::tx_builder::signature_base_hash(&payload_for(&envelope, &passphrase)?);
        let signature =
            hardware_wallet::sign_transaction(kind, &request.hd_path, &preimage, &passphrase)
                .map_err(|err| hardware_wallet::map_signing_error(err, kind))?;

        // The device signs the same preimage we would have signed locally; we
        // only attach the resulting bytes and the account's hint, then confirm
        // the signature actually verifies before returning it.
        let public_key = public_key_for_envelope(&envelope, kind)?;
        let signed =
            crate::utils::tx_builder::attach_signature(&envelope, &public_key, &signature)?;
        if !signature_present_and_valid(&signed, &public_key, &passphrase)? {
            anyhow::bail!(
                "hardware signature from {} did not verify; refusing to return an unverifiable \
                 envelope",
                kind.to_string().to_lowercase()
            );
        }
        return crate::utils::tx_builder::envelope_to_base64(&signed);
    }

    let secret_key: &str = request
        .local_secret
        .as_deref()
        .context("No local secret key available for signing")?;
    let signing_key = crate::utils::tx_builder::parse_signing_key(secret_key)?;
    let signed = crate::utils::tx_builder::sign_envelope(&envelope, &signing_key, &passphrase)?;
    crate::utils::tx_builder::envelope_to_base64(&signed)
}

/// The signature payload for whichever envelope variant this is.
fn payload_for(
    envelope: &stellar_xdr::curr::TransactionEnvelope,
    passphrase: &str,
) -> Result<Vec<u8>> {
    use crate::utils::tx_builder::{
        fee_bump_signature_payload, transaction_signature_payload, TransactionEnvelope as Te,
    };
    match envelope {
        Te::Tx(v1) => transaction_signature_payload(&v1.tx, passphrase),
        Te::TxFeeBump(bump) => fee_bump_signature_payload(&bump.tx, passphrase),
        Te::TxV0(_) => anyhow::bail!("legacy v0 envelopes cannot be signed for a fee bump"),
    }
}

/// Confirm at least one signature on the envelope verifies for `public_key`.
fn signature_present_and_valid(
    envelope: &stellar_xdr::curr::TransactionEnvelope,
    public_key: &[u8; 32],
    passphrase: &str,
) -> Result<bool> {
    use crate::utils::tx_builder::{
        verify_fee_bump_signature, verify_transaction_signature, TransactionEnvelope as Te,
    };
    match envelope {
        Te::Tx(v1) => verify_transaction_signature(v1, public_key, passphrase),
        Te::TxFeeBump(bump) => verify_fee_bump_signature(bump, public_key, passphrase),
        Te::TxV0(_) => Ok(false),
    }
}

/// Resolve the account a hardware device is expected to have derived.
///
/// The account is the envelope's own source (or, for a fee bump, its
/// `fee_source`), because that is the only account whose signature the
/// envelope is asking for at this layer.
fn public_key_for_envelope(
    envelope: &stellar_xdr::curr::TransactionEnvelope,
    kind: hardware_wallet::HardwareWalletKind,
) -> Result<[u8; 32]> {
    let account = crate::utils::tx_builder::fee_payer_of(envelope);
    let encoded = crate::utils::tx_builder::account_str(&account);
    crate::utils::tx_builder::parse_public_key(&encoded).with_context(|| {
        format!(
            "cannot determine the account a {} device should sign for ({encoded})",
            kind.to_string().to_lowercase()
        )
    })
}

/// Produce a partial signature for multi-sig collection flows.
pub fn sign_transaction_partial(
    transaction_xdr: &str,
    request: &SigningRequest,
    signer_label: &str,
) -> Result<String> {
    if request.hardware.is_some() {
        p::info(&format!(
            "Collecting partial signature from hardware wallet for signer '{}'.",
            signer_label
        ));
    }
    sign_transaction_xdr(transaction_xdr, request)
}

fn decode_transaction_bytes(transaction_xdr: &str) -> Result<Vec<u8>> {
    general_purpose::STANDARD
        .decode(transaction_xdr)
        .or_else(|_| Ok(transaction_xdr.as_bytes().to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use stellar_xdr::curr::{
        Memo, MuxedAccount, Operation, OperationBody, Preconditions, SequenceNumber, Transaction,
        TransactionEnvelope, TransactionExt, TransactionV1Envelope, Uint256, VecM,
    };

    /// Read the passphrase the same way production code does, so these tests stay
    /// correct if a network passphrase is overridden in configuration.
    fn passphrase() -> String {
        config::get_network_passphrase("testnet")
    }

    fn secret_for(seed: [u8; 32]) -> String {
        stellar_strkey::ed25519::PrivateKey(seed).to_string()
    }

    /// A real, decodable single-operation envelope signed by `seed`'s account.
    fn unsigned_envelope(seed: [u8; 32], fee: u32) -> TransactionEnvelope {
        let key = ed25519_dalek::SigningKey::from_bytes(&seed);
        let tx = Transaction {
            source_account: MuxedAccount::Ed25519(Uint256(key.verifying_key().to_bytes())),
            fee,
            seq_num: SequenceNumber(1),
            cond: Preconditions::None,
            memo: Memo::None,
            operations: VecM::<Operation, 100>::try_from(vec![Operation {
                source_account: None,
                body: OperationBody::Inflation,
            }])
            .unwrap(),
            ext: TransactionExt::V0,
        };
        TransactionEnvelope::Tx(TransactionV1Envelope {
            tx,
            signatures: VecM::try_from(Vec::new()).unwrap(),
        })
    }

    #[test]
    fn local_signing_produces_a_verifiable_signature() {
        let seed = [7u8; 32];
        let request = SigningRequest::local_secret(Zeroizing::new(secret_for(seed)), "testnet");
        let unsigned = unsigned_envelope(seed, 100);
        let xdr = crate::utils::tx_builder::envelope_to_base64(&unsigned).unwrap();

        let signed = sign_transaction_xdr(&xdr, &request).unwrap();
        let decoded = crate::utils::tx_builder::envelope_from_base64(&signed).unwrap();
        let TransactionEnvelope::Tx(v1) = &decoded else {
            panic!("expected a classic envelope");
        };
        assert_eq!(v1.signatures.len(), 1);

        let public = ed25519_dalek::SigningKey::from_bytes(&seed)
            .verifying_key()
            .to_bytes();
        assert!(crate::utils::tx_builder::verify_transaction_signature(
            &v1,
            &public,
            &passphrase()
        )
        .unwrap());
    }

    #[test]
    fn local_signing_never_embeds_secret_material() {
        // Regression guard: the previous mock returned base64 of
        // "signed_<xdr>_with_<first 8 chars of the secret>".
        let seed = [7u8; 32];
        let secret = secret_for(seed);
        let request = SigningRequest::local_secret(Zeroizing::new(secret.clone()), "testnet");
        let unsigned = unsigned_envelope(seed, 100);
        let xdr = crate::utils::tx_builder::envelope_to_base64(&unsigned).unwrap();

        let signed = sign_transaction_xdr(&xdr, &request).unwrap();
        let decoded = crate::utils::tx_builder::envelope_from_base64(&signed).unwrap();
        let revealed = format!("{decoded:?}");
        let prefix: String = secret.chars().take(8).collect();
        assert!(
            !revealed.contains(&prefix),
            "signed envelope must not contain secret key material"
        );
    }

    #[test]
    fn local_signing_rejects_a_non_xdr_payload() {
        let request =
            SigningRequest::local_secret(Zeroizing::new(secret_for([7u8; 32])), "testnet");
        assert!(sign_transaction_xdr("mock_tx_payload", &request).is_err());
    }

    #[test]
    fn signing_a_fee_bump_signs_the_outer_layer() {
        let source = [9u8; 32];
        let payer = [11u8; 32];
        let inner_key = ed25519_dalek::SigningKey::from_bytes(&source);
        let inner = unsigned_envelope(source, 200);
        let signed_inner =
            crate::utils::tx_builder::sign_envelope(&inner, &inner_key, &passphrase()).unwrap();
        let TransactionEnvelope::Tx(v1) = signed_inner else {
            panic!("expected a classic envelope");
        };
        let payer_key = ed25519_dalek::SigningKey::from_bytes(&payer);
        let bumped =
            crate::utils::tx_builder::wrap_fee_bump(v1, &payer_key, &passphrase(), 100).unwrap();
        let xdr = crate::utils::tx_builder::envelope_to_base64(&bumped).unwrap();

        let request = SigningRequest::local_secret(Zeroizing::new(secret_for(payer)), "testnet");
        let signed = sign_transaction_xdr(&xdr, &request).unwrap();
        let decoded = crate::utils::tx_builder::envelope_from_base64(&signed).unwrap();
        let TransactionEnvelope::TxFeeBump(bump) = &decoded else {
            panic!("expected a fee-bump envelope");
        };
        assert_eq!(bump.signatures.len(), 1);
        assert!(crate::utils::tx_builder::verify_fee_bump_signature(
            bump,
            &payer_key.verifying_key().to_bytes(),
            &passphrase()
        )
        .unwrap());
    }

    #[test]
    fn hardware_signing_reports_a_hardware_error() {
        let request = SigningRequest {
            local_secret: None,
            hardware: Some(hardware_wallet::HardwareWalletKind::Ledger),
            hd_path: hardware_wallet::STELLAR_HD_PATH.to_string(),
            network: "testnet".to_string(),
            skip_confirm: true,
        };
        let unsigned = unsigned_envelope([7u8; 32], 100);
        let xdr = crate::utils::tx_builder::envelope_to_base64(&unsigned).unwrap();
        let result = sign_transaction_xdr(&xdr, &request);
        assert!(result.is_err());
        let message = result.unwrap_err().to_string().to_lowercase();
        assert!(
            message.contains("hardware")
                || message.contains("ledger")
                || message.contains("disabled"),
            "unexpected error: {}",
            message
        );
    }
}
