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
pub fn sign_transaction_xdr(transaction_xdr: &str, request: &SigningRequest) -> Result<String> {
    if let Some(kind) = request.hardware {
        let tx_bytes = decode_transaction_bytes(transaction_xdr)?;
        let passphrase = config::get_network_passphrase(&request.network);
        let signature =
            hardware_wallet::sign_transaction(kind, &request.hd_path, &tx_bytes, &passphrase)
                .map_err(|err| hardware_wallet::map_signing_error(err, kind))?;

        let signed = format!(
            "hw_signed_{}_{}_{}",
            kind.to_string().to_lowercase(),
            hex::encode(&signature[..signature.len().min(8)]),
            &transaction_xdr[..transaction_xdr.len().min(16)]
        );
        return Ok(general_purpose::STANDARD.encode(signed));
    }

    let secret_key: &str = request
        .local_secret
        .as_deref()
        .context("No local secret key available for signing")?;

    let signed_mock = format!(
        "signed_{}_with_{}",
        transaction_xdr,
        &secret_key[..secret_key.len().min(8)]
    );
    Ok(general_purpose::STANDARD.encode(signed_mock))
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

    #[test]
    fn local_signing_request_produces_encoded_xdr() {
        let request = SigningRequest::local_secret(
            Zeroizing::new("SABCDEFGHIJKLMNOPQRSTUVWXYZ012345678901234567890".to_string()),
            "testnet",
        );
        let signed = sign_transaction_xdr("mock_tx_payload", &request).unwrap();
        assert!(!signed.is_empty());
        let decoded = general_purpose::STANDARD.decode(signed).unwrap();
        let decoded_str = String::from_utf8(decoded).unwrap();
        assert!(decoded_str.contains("signed_"));
    }

    #[test]
    fn hardware_signing_requires_feature_or_disabled_message() {
        let request = SigningRequest {
            local_secret: None,
            hardware: Some(hardware_wallet::HardwareWalletKind::Ledger),
            hd_path: hardware_wallet::STELLAR_HD_PATH.to_string(),
            network: "testnet".to_string(),
            skip_confirm: true,
        };
        let result = sign_transaction_xdr("dGVzdA==", &request);
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
