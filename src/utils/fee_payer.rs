//! Resolve `--fee-payer` into a real fee-bumped transaction.
//!
//! This is the glue between the pure XDR work in
//! [`crate::utils::tx_builder`] and the CLI. It exists as a single function on
//! purpose: building, signing the inner transaction, optionally wrapping it in a
//! fee bump, signing the bump, and verifying both layers is an easy sequence to
//! get subtly wrong, and getting it wrong produces transactions that look
//! valid but are rejected by the network.
//!
//! CAP-15 requires two distinct signatures:
//!
//! * the **inner** transaction is signed by its own source account (the wallet
//!   sending the payment, or the sponsor creating the account);
//! * the **outer** fee bump is signed by the **fee source**, which may be a
//!   completely different account.
//!
//! A device holding the source key therefore cannot authorise a bump paid by
//! someone else, and the source key must never be used for the outer layer.

use anyhow::{bail, Context, Result};
use zeroize::Zeroizing;

use crate::utils::{
    config,
    hardware_wallet::{self, HardwareWalletKind},
    wallet_signer,
};

use super::tx_builder::{self, FeeBreakdown, TransactionEnvelope};

/// Everything needed to turn a built envelope into a submittable one.
#[derive(Debug, Clone)]
pub struct WrapOptions<'a> {
    /// Network passphrase selector (`testnet`, `mainnet`, ...).
    pub network: &'a str,
    /// Current base fee in stroops.
    pub base_fee: i64,
    /// Wallet that owns the inner transaction and signs it.
    pub source_wallet: &'a config::WalletEntry,
    /// Sign the inner transaction with this device instead of a local secret.
    pub hardware: Option<HardwareWalletKind>,
    /// HD derivation path used when `hardware` is set.
    pub hd_path: &'a str,
    /// Wallet that pays the network fee. `None` submits the transaction
    /// unwrapped, so the source pays its own fee at no extra cost.
    pub fee_payer: Option<&'a str>,
}

/// A fully signed, verified, submittable envelope.
#[derive(Debug, Clone)]
pub struct SignedOutcome {
    /// The envelope to submit.
    pub envelope: TransactionEnvelope,
    /// Base64 XDR of `envelope`, ready for Horizon.
    pub xdr: String,
    /// Who pays what, for the confirmation preview.
    pub breakdown: FeeBreakdown,
    /// Name of the wallet that paid the fee, if a bump was applied.
    pub fee_payer_wallet: Option<String>,
}

impl SignedOutcome {
    /// Base64 XDR of the envelope to submit.
    pub fn xdr(&self) -> &str {
        &self.xdr
    }
}

/// The network's current base fee in stroops.
///
/// Falls back to the protocol minimum when Horizon cannot be reached, so a
/// preview still renders offline. A too-low bump fee would be rejected by the
/// network, so falling back upwards is the safe direction.
pub async fn resolve_base_fee(network: &str) -> i64 {
    match crate::utils::horizon::fetch_fee_stats(network).await {
        Ok(stats) => stats
            .mode_fee
            .parse::<i64>()
            .unwrap_or(tx_builder::MIN_BASE_FEE)
            .max(tx_builder::MIN_BASE_FEE),
        Err(_) => tx_builder::MIN_BASE_FEE,
    }
}

/// Look up a wallet by name.
pub fn find_wallet(name: &str) -> Result<config::WalletEntry> {
    let cfg = config::load()?;
    cfg.wallets
        .iter()
        .find(|w| w.name == name)
        .cloned()
        .with_context(|| {
            format!(
                "fee payer wallet '{}' not found. Create it with: starforge wallet create {}",
                name, name
            )
        })
}

/// Sign `plain` and, when a fee payer is named, wrap and sign the bump.
///
/// Both signature layers are verified before returning, so a caller cannot
/// submit an envelope that was built or signed incorrectly.
pub fn sign_and_wrap(plain: TransactionEnvelope, opts: &WrapOptions<'_>) -> Result<SignedOutcome> {
    let inner_v1 = match &plain {
        TransactionEnvelope::Tx(v1) => v1.clone(),
        TransactionEnvelope::TxFeeBump(_) => {
            bail!("--fee-payer expects an un-wrapped transaction, got an existing fee bump")
        }
        TransactionEnvelope::TxV0(_) => {
            bail!("--fee-payer does not support legacy v0 envelopes")
        }
    };

    // Confirm the transaction is actually sourced by the wallet that is about to
    // sign it, so a mismatched --from cannot produce a bad signature.
    let declared_source = tx_builder::account_str(&inner_v1.tx.source_account);
    if declared_source != opts.source_wallet.public_key.trim() {
        bail!(
            "transaction is sourced by {declared_source} but wallet '{}' is {}",
            opts.source_wallet.name,
            opts.source_wallet.public_key
        );
    }

    let passphrase = config::get_network_passphrase(opts.network);
    let inner_signed = sign_inner(
        &plain,
        opts.source_wallet,
        opts.hardware,
        opts.hd_path,
        &passphrase,
    )?;

    let (envelope, fee_payer_wallet) = match opts.fee_payer {
        None => (inner_signed, None),
        Some(payer_name) => {
            if payer_name == opts.source_wallet.name {
                bail!(
                    "--fee-payer '{}' is the transaction's own source wallet; the sender already \
                     pays the fee. Omit the flag to pay the fee directly.",
                    payer_name
                );
            }
            let payer = find_wallet(payer_name)?;
            let payer_secret = wallet_signer::resolve_local_secret(&payer, &payer.name)?;
            let payer_key = tx_builder::parse_signing_key(&payer_secret)?;
            let public_key = tx_builder::parse_public_key(&payer.public_key)?;
            if public_key != payer_key.verifying_key().to_bytes() {
                bail!(
                    "fee payer wallet '{}' secret does not match its stored public key ({})",
                    payer.name,
                    payer.public_key
                );
            }

            // `wrap_fee_bump` signs the outer layer with the fee source.
            let bumped = tx_builder::wrap_fee_bump(
                inner_signed_v1(&inner_signed)?,
                &payer_key,
                &passphrase,
                opts.base_fee,
            )?;
            (bumped, Some(payer.name))
        }
    };

    verify_layers(&envelope, opts, &passphrase)?;

    let breakdown = FeeBreakdown::from_envelope(&envelope);
    let xdr = tx_builder::envelope_to_base64(&envelope)?;
    Ok(SignedOutcome {
        envelope,
        xdr,
        breakdown,
        fee_payer_wallet,
    })
}

fn inner_signed_v1(envelope: &TransactionEnvelope) -> Result<tx_builder::TransactionV1Envelope> {
    match envelope {
        TransactionEnvelope::Tx(v1) => Ok(v1.clone()),
        _ => bail!("expected an unwrapped transaction"),
    }
}

/// Sign the inner transaction, locally or with a device, and verify the result.
fn sign_inner(
    plain: &TransactionEnvelope,
    wallet: &config::WalletEntry,
    hardware: Option<HardwareWalletKind>,
    hd_path: &str,
    passphrase: &str,
) -> Result<TransactionEnvelope> {
    let TransactionEnvelope::Tx(v1) = plain else {
        bail!("expected an unwrapped transaction");
    };

    match hardware {
        Some(kind) => {
            let payload = tx_builder::transaction_signature_payload(&v1.tx, passphrase)?;
            // The device must sign the 32-byte base hash, not the payload bytes.
            let preimage = tx_builder::signature_base_hash(&payload);
            let raw = hardware_wallet::sign_transaction(kind, hd_path, &preimage, passphrase)
                .map_err(|err| hardware_wallet::map_signing_error(err, kind))
                .with_context(|| {
                    format!(
                        "hardware signing failed for '{}'. If the device is not connected, sign \
                         with a local wallet secret instead.",
                        wallet.name
                    )
                })?;
            let public_key = tx_builder::parse_public_key(&wallet.public_key)?;
            let signed = tx_builder::attach_signature(plain, &public_key, &raw)?;
            let TransactionEnvelope::Tx(signed_v1) = &signed else {
                bail!("hardware signing produced an unexpected envelope");
            };
            if !tx_builder::verify_transaction_signature(signed_v1, &public_key, passphrase)? {
                bail!(
                    "hardware signature did not verify against {}; refusing to submit an \
                     unverifiable transaction",
                    wallet.public_key
                );
            }
            Ok(signed)
        }
        None => {
            let secret: Zeroizing<String> =
                wallet_signer::resolve_local_secret(wallet, &wallet.name)?;
            let key = tx_builder::parse_signing_key(&secret)?;
            let public_key = tx_builder::parse_public_key(&wallet.public_key)?;
            if public_key != key.verifying_key().to_bytes() {
                bail!(
                    "wallet '{}' secret does not match its stored public key ({})",
                    wallet.name,
                    wallet.public_key
                );
            }
            let signed = tx_builder::sign_envelope(plain, &key, passphrase)?;
            let TransactionEnvelope::Tx(signed_v1) = &signed else {
                bail!("signing produced an unexpected envelope");
            };
            if !tx_builder::verify_transaction_signature(signed_v1, &public_key, passphrase)? {
                bail!(
                    "signature for {} did not verify; refusing to submit an unverifiable \
                     transaction",
                    wallet.public_key
                );
            }
            Ok(signed)
        }
    }
}

/// Assert every required signature is present and valid before submission.
fn verify_layers(
    envelope: &TransactionEnvelope,
    opts: &WrapOptions<'_>,
    passphrase: &str,
) -> Result<()> {
    let source_key = tx_builder::parse_public_key(&opts.source_wallet.public_key)?;
    match envelope {
        TransactionEnvelope::Tx(v1) => {
            if !tx_builder::verify_transaction_signature(v1, &source_key, passphrase)? {
                bail!("inner transaction is missing a valid source signature");
            }
        }
        TransactionEnvelope::TxFeeBump(bump) => {
            // The inner signature must survive the wrap; a bump cannot supply it.
            let stellar_xdr::curr::FeeBumpTransactionInnerTx::Tx(inner) = &bump.tx.inner_tx;
            if !tx_builder::verify_transaction_signature(inner, &source_key, passphrase)? {
                bail!("fee bump is missing a valid inner source signature");
            }
            let payer_name = opts
                .fee_payer
                .ok_or_else(|| anyhow::anyhow!("unexpected fee bump with no fee payer"))?;
            let payer = find_wallet(payer_name)?;
            let payer_key = tx_builder::parse_public_key(&payer.public_key)?;
            if !tx_builder::verify_fee_bump_signature(bump, &payer_key, passphrase)? {
                bail!("fee bump is missing a valid fee-source signature");
            }
            if tx_builder::account_str(&tx_builder::fee_payer_of(envelope))
                != payer.public_key.trim()
            {
                bail!("fee bump names the wrong fee source");
            }
        }
        TransactionEnvelope::TxV0(_) => bail!("legacy v0 envelopes are not supported"),
    }
    Ok(())
}

/// Verify the target network before anything is signed or submitted.
pub async fn preflight(network: &str) -> Result<()> {
    crate::utils::network_guard::verify(network).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_fee_constant_is_the_protocol_floor() {
        assert_eq!(tx_builder::MIN_BASE_FEE, 100);
    }

    #[test]
    fn unknown_wallet_is_a_clear_error() {
        let err = find_wallet("definitely-not-a-wallet-12345")
            .err()
            .expect("should not find a wallet with this name");
        let message = err.to_string();
        assert!(
            message.contains("not found") || message.contains("load"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn inner_signed_v1_rejects_a_fee_bump() {
        let payer = tx_builder::parse_signing_key(
            &stellar_strkey::ed25519::PrivateKey([7u8; 32]).to_string(),
        )
        .unwrap();
        let inner = tx_builder::build_payment(&tx_builder::PaymentRequest {
            source: tx_builder::account_str(&tx_builder::MuxedAccount::Ed25519(
                stellar_xdr::curr::Uint256([9u8; 32]),
            )),
            destination: tx_builder::account_str(&tx_builder::MuxedAccount::Ed25519(
                stellar_xdr::curr::Uint256([3u8; 32]),
            )),
            amount: 1,
            asset: None,
            sequence: 1,
            base_fee: 100,
        })
        .unwrap();
        let TransactionEnvelope::Tx(v1) = inner else {
            panic!("expected classic");
        };
        let bumped = tx_builder::wrap_fee_bump(v1, &payer, "Test", 100).unwrap();
        let err = inner_signed_v1(&bumped).unwrap_err();
        assert!(err.to_string().contains("un-wrapped"));
    }
}
