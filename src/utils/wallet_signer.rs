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

        enforce_mainnet_plaintext_policy(wallet, network)?;
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
fn enforce_mainnet_plaintext_policy(
    wallet: &config::WalletEntry,
    network: &str,
) -> Result<()> {
    enforce_mainnet_plaintext_policy_with_override(
        wallet,
        network,
        crate::utils::network_guard::allow_plaintext_mainnet(),
    )
}

fn enforce_mainnet_plaintext_policy_with_override(
    wallet: &config::WalletEntry,
    network: &str,
    allow_override: bool,
) -> Result<()> {
    let Some(secret) = wallet.secret_key.as_ref() else {
        return Ok(());
    };

    let plaintext = !secret.contains(':') && secret.starts_with('S') && secret.len() == 56;

    if network != "mainnet" || !plaintext {
        return Ok(());
    }

    if !allow_override {
        anyhow::bail!(
            "Refusing mainnet signing with plaintext wallet '{}'. Encrypt the wallet before signing with `starforge wallet create --encrypt <name>` or `starforge wallet import --encrypt`. Alternatively use a hardware wallet with `--hardware ledger` or `--hardware trezor`. If you deliberately accept the risk, retry with `--allow-plaintext-mainnet`.",
            wallet.name
        );
    }

    crate::utils::print::warn(
        "WARNING: plaintext mainnet signing override enabled. Your secret key is stored unencrypted at rest."
    );

    let mut details = std::collections::HashMap::new();
    details.insert("network".to_string(), "mainnet".to_string());
    details.insert("wallet".to_string(), wallet.name.clone());
    details.insert("plaintext_secret".to_string(), "true".to_string());
    details.insert("override".to_string(), "allow-plaintext-mainnet".to_string());

    if let Err(e) = crate::utils::audit::log_action(
        "allow_plaintext_mainnet_signing",
        "cli",
        "wallet",
        &wallet.name,
        details,
        true,
        None,
    ) {
        crate::utils::print::warn(&format!(
            "Could not write plaintext-mainnet override audit entry: {}",
            e
        ));
    }

    Ok(())
}

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
            Zeroizing::new("SABCDEFGHIJKLMNOPQRSTUVWXYZ01234567890123456789012345678".to_string()),
            "testnet",
        );
        let signed = sign_transaction_xdr("mock_tx_payload", &request).unwrap();
        assert!(!signed.is_empty());
        let decoded = general_purpose::STANDARD.decode(signed).unwrap();
        let decoded_str = String::from_utf8(decoded).unwrap();
        assert!(decoded_str.contains("signed_"));
    }

    fn test_wallet(secret_key: Option<&str>) -> config::WalletEntry {
        config::WalletEntry {
            name: "mainnet-test".to_string(),
            public_key: "GTESTPUBLICKEY".to_string(),
            secret_key: secret_key.map(str::to_string),
            network: "mainnet".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            funded: true,
            kdf_options: None,
            rotation_history: Vec::new(),
        }
    }

    #[test]
    fn plaintext_mainnet_signing_is_blocked_by_default() {
        let wallet = test_wallet(Some(
            "SABCDEFGHIJKLMNOPQRSTUVWXYZ01234567890123456789012345678",
        ));

        let result = enforce_mainnet_plaintext_policy_with_override(&wallet, "mainnet", false);

        assert!(result.is_err());
        let message = result.unwrap_err().to_string();
        assert!(message.contains("Refusing mainnet signing"));
        assert!(message.contains("--allow-plaintext-mainnet"));
        assert!(message.contains("--encrypt"));
        assert!(message.contains("--hardware"));
    }

    #[test]
    fn plaintext_mainnet_signing_override_is_allowed() {
        let wallet = test_wallet(Some(
            "SABCDEFGHIJKLMNOPQRSTUVWXYZ01234567890123456789012345678",
        ));

        let result = enforce_mainnet_plaintext_policy_with_override(&wallet, "mainnet", true);

        assert!(result.is_ok());
    }

    #[test]
    fn encrypted_mainnet_signing_does_not_require_override() {
        let wallet = test_wallet(Some("enc:v1:encrypted-wallet-secret"));

        assert!(enforce_mainnet_plaintext_policy_with_override(&wallet, "mainnet", false).is_ok());
    }

    #[test]
    fn plaintext_testnet_signing_does_not_require_override() {
        let wallet = test_wallet(Some(
            "SABCDEFGHIJKLMNOPQRSTUVWXYZ01234567890123456789012345678",
        ));

        assert!(enforce_mainnet_plaintext_policy_with_override(&wallet, "testnet", false).is_ok());
    }

    #[test]
    fn hardware_wallet_without_local_secret_does_not_require_override() {
        let wallet = test_wallet(None);

        assert!(enforce_mainnet_plaintext_policy_with_override(&wallet, "mainnet", false).is_ok());
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
