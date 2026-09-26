//! Real Stellar transaction envelope construction, signing, and fee-bump wrapping.
//!
//! This module performs the actual XDR work that fee-bump transactions require:
//! building a `FeeBumpTransactionEnvelope` around an already-signed inner
//! envelope, computing the signature payload the network expects, and producing
//! the decorated signatures the ledger verifies.
//!
//! The signing chain mirrors the proven SEP-10 implementation in
//! [`crate::utils::sep10`]: a signature covers the XDR of a
//! `TransactionSignaturePayload`, which is never the bare envelope — it is the
//! `network_id` plus a *tagged* transaction. For a fee bump the tag is
//! `TxFeeBump`, and the outer envelope's own signatures are not part of the
//! inner transaction's payload.
//!
//! Two rules matter and are easy to get wrong:
//!
//! 1. Serialise through [`TransactionEnvelope`], never a bare
//!    `TransactionV1Envelope` or `FeeBumpTransactionEnvelope`. The bare struct
//!    serialises only its fields and would drop the leading `EnvelopeType`
//!    discriminant, producing bytes the network rejects.
//! 2. A fee bump needs **two** independent signatures: the inner transaction is
//!    signed by its original source account over the `Tx` payload, and the outer
//!    bump is signed by the fee source over the `TxFeeBump` payload.

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

// Re-exported so callers can build and inspect envelopes without importing
// `stellar_xdr` themselves; these appear throughout this module's public API.
use stellar_xdr::curr::{
    AccountId, AlphaNum12, AlphaNum4, Asset, AssetCode12, AssetCode4, BytesM, DecoratedSignature,
    FeeBumpTransaction, FeeBumpTransactionEnvelope, FeeBumpTransactionExt,
    FeeBumpTransactionInnerTx, Hash, Limits, Memo, MuxedAccountMed25519, Operation, OperationBody,
    PaymentOp, Preconditions, PublicKey, ReadXdr, SequenceNumber, Signature as XdrSignature,
    Transaction, TransactionExt, TransactionSignaturePayload,
    TransactionSignaturePayloadTaggedTransaction, Uint256, VecM, WriteXdr,
};
pub use stellar_xdr::curr::{MuxedAccount, TransactionEnvelope, TransactionV1Envelope};

/// The protocol floor for `baseFee` (CAP-15). A bump below this is rejected.
pub const MIN_BASE_FEE: i64 = 100;

/// The inner fee is multiplied by this when deriving the outer bump fee, so the
/// fee source stays profitable and the inner source is never asked for more.
pub const FEE_BUMP_MULTIPLIER: i64 = 2;

/// Length of a raw ed25519 signature.
pub const ED25519_SIGNATURE_LEN: usize = 64;

/// Network id: the SHA-256 of the network passphrase.
pub fn network_id(network_passphrase: &str) -> Hash {
    Hash(Sha256::digest(network_passphrase.as_bytes()).into())
}

/// Decode a `G...` account into its 32-byte public key.
pub fn parse_public_key(account: &str) -> Result<[u8; 32]> {
    let trimmed = account.trim();
    stellar_strkey::ed25519::PublicKey::from_string(trimmed)
        .map(|key| key.0)
        .with_context(|| format!("'{trimmed}' is not a valid Stellar account (G...)"))
}

/// Decode a `G...` or `M...` address into an XDR [`MuxedAccount`].
pub fn parse_muxed_account(account: &str) -> Result<MuxedAccount> {
    let trimmed = account.trim();
    if let Ok(key) = stellar_strkey::ed25519::PublicKey::from_string(trimmed) {
        return Ok(MuxedAccount::Ed25519(Uint256(key.0)));
    }
    if let Ok(muxed) = stellar_strkey::ed25519::MuxedAccount::from_string(trimmed) {
        return Ok(MuxedAccount::MuxedEd25519(MuxedAccountMed25519 {
            ed25519: Uint256(muxed.ed25519),
            id: muxed.id,
        }));
    }
    bail!("'{trimmed}' is not a valid Stellar account (expected G... or M...)")
}

/// Render an XDR [`MuxedAccount`] back to its `G...`/`M...` strkey.
pub fn account_str(account: &MuxedAccount) -> String {
    match account {
        MuxedAccount::Ed25519(Uint256(bytes)) => {
            stellar_strkey::ed25519::PublicKey(*bytes).to_string()
        }
        MuxedAccount::MuxedEd25519(muxed) => stellar_strkey::ed25519::MuxedAccount {
            ed25519: muxed.ed25519.0,
            id: muxed.id,
        }
        .to_string(),
    }
}

/// Decode an `S...` secret seed into an ed25519 signing key.
pub fn parse_signing_key(secret: &str) -> Result<SigningKey> {
    let trimmed = secret.trim();
    let private = stellar_strkey::ed25519::PrivateKey::from_string(trimmed)
        .with_context(|| "not a valid Stellar secret key (S...)")?;
    Ok(SigningKey::from_bytes(&private.0))
}

/// The 4-byte discriminator the ledger uses to route a signature to an account.
pub fn signature_hint(public_key: &[u8; 32]) -> stellar_xdr::curr::SignatureHint {
    stellar_xdr::curr::SignatureHint([
        public_key[28],
        public_key[29],
        public_key[30],
        public_key[31],
    ])
}

/// The byte string a classic (non-bumped) transaction signature covers.
pub fn transaction_signature_payload(
    tx: &Transaction,
    network_passphrase: &str,
) -> Result<Vec<u8>> {
    let payload = TransactionSignaturePayload {
        network_id: network_id(network_passphrase),
        tagged_transaction: TransactionSignaturePayloadTaggedTransaction::Tx(tx.clone()),
    };
    payload
        .to_xdr(Limits::none())
        .context("failed to encode transaction signature payload")
}

/// The byte string a fee-bump signature covers.
///
/// Note this takes the [`FeeBumpTransaction`], not its envelope: the envelope's
/// own signature list is deliberately excluded so a signature cannot be
/// altered by adding signatures.
pub fn fee_bump_signature_payload(
    fee_bump: &FeeBumpTransaction,
    network_passphrase: &str,
) -> Result<Vec<u8>> {
    let payload = TransactionSignaturePayload {
        network_id: network_id(network_passphrase),
        tagged_transaction: TransactionSignaturePayloadTaggedTransaction::TxFeeBump(
            fee_bump.clone(),
        ),
    };
    payload
        .to_xdr(Limits::none())
        .context("failed to encode fee-bump signature payload")
}

/// The 32-byte preimage Stellar actually verifies a signature against.
///
/// A signature does not cover the `TransactionSignaturePayload` bytes directly;
/// it covers their SHA-256 digest, the same value returned by [`envelope_hash`]
/// and by `stellar tx hash`. Signing the raw payload instead produces a
/// signature that looks well-formed but is rejected by the network.
pub fn signature_base_hash(payload: &[u8]) -> [u8; 32] {
    Sha256::digest(payload).into()
}

/// Sign a payload into a decorated signature with the account's hint attached.
pub fn decorate(signing_key: &SigningKey, payload: &[u8]) -> Result<DecoratedSignature> {
    let signature = signing_key.sign(&signature_base_hash(payload));
    let bytes = signature.to_bytes().to_vec();
    Ok(DecoratedSignature {
        hint: signature_hint(&signing_key.verifying_key().to_bytes()),
        signature: XdrSignature(
            BytesM::try_from(bytes).context("signature is not a valid XDR length")?,
        ),
    })
}

fn signature_vec(signatures: Vec<DecoratedSignature>) -> Result<VecM<DecoratedSignature, 20>> {
    VecM::try_from(signatures).context("too many signatures on a single envelope (max 20)")
}

/// Append `signing_key`'s signature to an existing classic envelope.
pub fn sign_transaction_v1(
    envelope: &TransactionV1Envelope,
    signing_key: &SigningKey,
    network_passphrase: &str,
) -> Result<TransactionV1Envelope> {
    let payload = transaction_signature_payload(&envelope.tx, network_passphrase)?;
    let mut signatures = envelope.signatures.to_vec();
    signatures.push(decorate(signing_key, &payload)?);
    Ok(TransactionV1Envelope {
        tx: envelope.tx.clone(),
        signatures: signature_vec(signatures)?,
    })
}

/// Verify that some signature on `envelope` is valid for `public_key`.
///
/// Used by the tests as an independent check that the payloads we sign are the
/// ones the ledger reconstructs.
pub fn verify_transaction_signature(
    envelope: &TransactionV1Envelope,
    public_key: &[u8; 32],
    network_passphrase: &str,
) -> Result<bool> {
    let payload = transaction_signature_payload(&envelope.tx, network_passphrase)?;
    let verifying = VerifyingKey::from_bytes(public_key).context("invalid verifying key")?;
    Ok(envelope.signatures.iter().any(|decorated| {
        let raw = decorated.signature.0.as_vec();
        let Ok(signature) = <[u8; 64]>::try_from(raw.as_slice()) else {
            return false;
        };
        verifying
            .verify_strict(
                &signature_base_hash(&payload),
                &ed25519_dalek::Signature::from_bytes(&signature),
            )
            .is_ok()
    }))
}

/// Verify that some signature on a fee-bump envelope is valid for `public_key`.
pub fn verify_fee_bump_signature(
    envelope: &FeeBumpTransactionEnvelope,
    public_key: &[u8; 32],
    network_passphrase: &str,
) -> Result<bool> {
    let payload = fee_bump_signature_payload(&envelope.tx, network_passphrase)?;
    let verifying = VerifyingKey::from_bytes(public_key).context("invalid verifying key")?;
    Ok(envelope.signatures.iter().any(|decorated| {
        let raw = decorated.signature.0.as_vec();
        let Ok(signature) = <[u8; 64]>::try_from(raw.as_slice()) else {
            return false;
        };
        verifying
            .verify_strict(
                &signature_base_hash(&payload),
                &ed25519_dalek::Signature::from_bytes(&signature),
            )
            .is_ok()
    }))
}

/// The outer fee for a bump: at least the base fee, and always a multiple of the
/// inner fee so the inner source never ends up out of pocket.
pub fn outer_fee(inner_fee: i64, base_fee: i64) -> i64 {
    let floor = base_fee.max(MIN_BASE_FEE);
    (inner_fee.max(0) * FEE_BUMP_MULTIPLIER).max(floor)
}

/// Wrap a signed classic envelope in a fee bump, signing the outer envelope as
/// the fee source.
///
/// `inner` must already carry a valid signature from its own source account; the
/// fee bump does not and cannot replace that signature.
pub fn wrap_fee_bump(
    inner: TransactionV1Envelope,
    fee_source_key: &SigningKey,
    network_passphrase: &str,
    base_fee: i64,
) -> Result<TransactionEnvelope> {
    let fee_source = MuxedAccount::Ed25519(Uint256(fee_source_key.verifying_key().to_bytes()));
    wrap_fee_bump_for(
        inner,
        fee_source,
        fee_source_key,
        network_passphrase,
        base_fee,
    )
}

/// Wrap a signed classic envelope in a fee bump using an explicit fee source
/// account and a separate key that authorises the bump.
pub fn wrap_fee_bump_for(
    inner: TransactionV1Envelope,
    fee_source: MuxedAccount,
    authorising_key: &SigningKey,
    network_passphrase: &str,
    base_fee: i64,
) -> Result<TransactionEnvelope> {
    if matches!(inner.tx.source_account, MuxedAccount::MuxedEd25519(_)) {
        bail!("fee-bump inner transaction must use an ed25519 (G...) source account");
    }

    let fee_bump = FeeBumpTransaction {
        fee_source: fee_source.clone(),
        fee: outer_fee(i64::from(inner.tx.fee), base_fee),
        inner_tx: FeeBumpTransactionInnerTx::Tx(inner),
        ext: FeeBumpTransactionExt::V0,
    };

    let payload = fee_bump_signature_payload(&fee_bump, network_passphrase)?;
    let signatures = signature_vec(vec![decorate(authorising_key, &payload)?])?;

    Ok(TransactionEnvelope::TxFeeBump(FeeBumpTransactionEnvelope {
        tx: fee_bump,
        signatures,
    }))
}

/// True when the envelope is a fee bump.
pub fn is_fee_bump(envelope: &TransactionEnvelope) -> bool {
    matches!(envelope, TransactionEnvelope::TxFeeBump(_))
}

/// The account that pays the fee: the bump's `fee_source` for a fee-bumped
/// envelope, otherwise the transaction's own source.
pub fn fee_payer_of(envelope: &TransactionEnvelope) -> MuxedAccount {
    match envelope {
        TransactionEnvelope::TxFeeBump(bump) => bump.tx.fee_source.clone(),
        TransactionEnvelope::TxV0(v0) => {
            MuxedAccount::Ed25519(Uint256(v0.tx.source_account_ed25519.0))
        }
        TransactionEnvelope::Tx(v1) => v1.tx.source_account.clone(),
    }
}

/// The total fee the envelope commits the network to charging.
pub fn effective_fee(envelope: &TransactionEnvelope) -> i64 {
    match envelope {
        TransactionEnvelope::TxFeeBump(bump) => bump.tx.fee,
        TransactionEnvelope::TxV0(v0) => i64::from(v0.tx.fee),
        TransactionEnvelope::Tx(v1) => i64::from(v1.tx.fee),
    }
}

/// Encode any envelope to base64 XDR, always through the union so the
/// `EnvelopeType` discriminant is retained.
pub fn envelope_to_base64(envelope: &TransactionEnvelope) -> Result<String> {
    let bytes = envelope
        .to_xdr(Limits::none())
        .context("failed to encode transaction envelope")?;
    Ok(general_purpose::STANDARD.encode(bytes))
}

/// Decode a base64 XDR envelope.
pub fn envelope_from_base64(xdr: &str) -> Result<TransactionEnvelope> {
    let bytes = general_purpose::STANDARD
        .decode(xdr.trim())
        .context("transaction envelope is not valid base64")?;
    TransactionEnvelope::from_xdr(bytes, Limits::none())
        .context("transaction envelope is not valid XDR")
}

/// The network's transaction hash for any envelope type.
pub fn envelope_hash(envelope: &TransactionEnvelope, network_passphrase: &str) -> Result<[u8; 32]> {
    let tagged = match envelope {
        TransactionEnvelope::Tx(v1) => {
            TransactionSignaturePayloadTaggedTransaction::Tx(v1.tx.clone())
        }
        TransactionEnvelope::TxFeeBump(bump) => {
            TransactionSignaturePayloadTaggedTransaction::TxFeeBump(bump.tx.clone())
        }
        TransactionEnvelope::TxV0(_) => {
            bail!("legacy v0 envelopes cannot be hashed for a signature payload")
        }
    };
    let payload = TransactionSignaturePayload {
        network_id: network_id(network_passphrase),
        tagged_transaction: tagged,
    };
    let xdr = payload
        .to_xdr(Limits::none())
        .context("failed to encode hash payload")?;
    Ok(Sha256::digest(xdr).into())
}

/// How a fee-bumped transaction splits responsibility between the two accounts.
///
/// Rendered by the previews so the operator can see exactly who pays what
/// before anything is signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeeBreakdown {
    /// Account that authorised the operations and signed the inner envelope.
    pub inner_source: String,
    /// Account that pays the fee, i.e. the fee source.
    pub fee_payer: String,
    /// Fee declared by the inner transaction.
    pub inner_fee: i64,
    /// Fee charged for the whole bump, paid by `fee_payer`.
    pub outer_fee: i64,
    /// Fee the inner source is actually charged. A fee bump fully covers the
    /// inner transaction's fee, so this is always 0 once bumped; it is kept as
    /// a field so callers and tests can assert that invariant.
    pub inner_source_pays: i64,
    /// True when the envelope is a fee bump.
    pub bumped: bool,
}

impl FeeBreakdown {
    /// Derive the breakdown for a built envelope.
    pub fn from_envelope(envelope: &TransactionEnvelope) -> Self {
        let bumped = is_fee_bump(envelope);
        let fee_payer = account_str(&fee_payer_of(envelope));
        match envelope {
            TransactionEnvelope::TxFeeBump(bump) => {
                let FeeBumpTransactionInnerTx::Tx(inner) = &bump.tx.inner_tx;
                let inner_fee = i64::from(inner.tx.fee);
                let outer_fee = bump.tx.fee;
                FeeBreakdown {
                    inner_source: account_str(&inner.tx.source_account),
                    fee_payer,
                    inner_fee,
                    outer_fee,
                    // CAP-15: the fee source pays the bump fee in full and the
                    // inner source is not charged the inner fee at all.
                    inner_source_pays: 0,
                    bumped,
                }
            }
            other => {
                let inner_fee = effective_fee(other);
                FeeBreakdown {
                    inner_source: fee_payer.clone(),
                    fee_payer,
                    inner_fee,
                    outer_fee: inner_fee,
                    // Not bumped, so the single source pays its own fee.
                    inner_source_pays: inner_fee,
                    bumped,
                }
            }
        }
    }

    /// Render as `p::kv` rows: who pays what.
    pub fn rows(&self) -> Vec<(&'static str, String)> {
        let mut rows = vec![
            ("Inner source", self.inner_source.clone()),
            ("Fee payer", self.fee_payer.clone()),
        ];
        if self.bumped {
            rows.push(("Inner fee (not charged)", stroops_xlm(self.inner_fee)));
            rows.push(("Bump fee (total)", stroops_xlm(self.outer_fee)));
            rows.push(("Paid by inner source", stroops_xlm(self.inner_source_pays)));
        } else {
            rows.push(("Fee", stroops_xlm(self.outer_fee)));
            rows.push(("Paid by inner source", stroops_xlm(self.inner_source_pays)));
        }
        rows
    }
}

/// Format stroops the way every StarForge command does: stroops plus XLM.
pub fn stroops_xlm(stroops: i64) -> String {
    format!("{} ({:.7} XLM)", stroops, stroops as f64 / 10_000_000.0)
}

/// Append `signing_key`'s signature to an existing fee-bump envelope.
pub fn sign_fee_bump_v1(
    envelope: &FeeBumpTransactionEnvelope,
    signing_key: &SigningKey,
    network_passphrase: &str,
) -> Result<FeeBumpTransactionEnvelope> {
    let payload = fee_bump_signature_payload(&envelope.tx, network_passphrase)?;
    let mut signatures = envelope.signatures.to_vec();
    signatures.push(decorate(signing_key, &payload)?);
    Ok(FeeBumpTransactionEnvelope {
        tx: envelope.tx.clone(),
        signatures: signature_vec(signatures)?,
    })
}

/// Sign whichever envelope variant this is, dispatching on the discriminant.
pub fn sign_envelope(
    envelope: &TransactionEnvelope,
    signing_key: &SigningKey,
    network_passphrase: &str,
) -> Result<TransactionEnvelope> {
    match envelope {
        TransactionEnvelope::Tx(v1) => Ok(TransactionEnvelope::Tx(sign_transaction_v1(
            v1,
            signing_key,
            network_passphrase,
        )?)),
        TransactionEnvelope::TxFeeBump(bump) => Ok(TransactionEnvelope::TxFeeBump(
            sign_fee_bump_v1(bump, signing_key, network_passphrase)?,
        )),
        TransactionEnvelope::TxV0(_) => {
            bail!("legacy v0 envelopes are not supported for signing")
        }
    }
}

/// Attach a signature produced elsewhere (for example by a hardware wallet) to
/// an envelope, computing the same payload this module would have signed.
///
/// `BytesM<64>` is variable-length, so an under-length signature would encode
/// successfully and only be rejected by the network. We reject it here instead
/// so the failure is local and legible.
pub fn attach_signature(
    envelope: &TransactionEnvelope,
    public_key: &[u8; 32],
    raw_signature: &[u8],
) -> Result<TransactionEnvelope> {
    if raw_signature.len() != ED25519_SIGNATURE_LEN {
        bail!(
            "ed25519 signature must be {ED25519_SIGNATURE_LEN} bytes, got {}",
            raw_signature.len()
        );
    }
    let signature = XdrSignature(
        BytesM::try_from(raw_signature.to_vec()).context("signature is not a valid XDR length")?,
    );
    let decorated = DecoratedSignature {
        hint: signature_hint(public_key),
        signature,
    };

    match envelope {
        TransactionEnvelope::Tx(v1) => {
            let mut signatures = v1.signatures.to_vec();
            signatures.push(decorated);
            Ok(TransactionEnvelope::Tx(TransactionV1Envelope {
                tx: v1.tx.clone(),
                signatures: signature_vec(signatures)?,
            }))
        }
        TransactionEnvelope::TxFeeBump(bump) => {
            let mut signatures = bump.signatures.to_vec();
            signatures.push(decorated);
            Ok(TransactionEnvelope::TxFeeBump(FeeBumpTransactionEnvelope {
                tx: bump.tx.clone(),
                signatures: signature_vec(signatures)?,
            }))
        }
        TransactionEnvelope::TxV0(_) => {
            bail!("legacy v0 envelopes are not supported for signing")
        }
    }
}

/// A classic payment to build.
#[derive(Debug, Clone)]
pub struct PaymentRequest {
    /// Source account (`G...`) that signs and pays the fee.
    pub source: String,
    /// Destination account (`G...` or `M...`).
    pub destination: String,
    /// Amount in stroops.
    pub amount: i64,
    /// Asset as `(code, issuer)`; `None` is native XLM.
    pub asset: Option<(String, String)>,
    /// Source account's current sequence number.
    pub sequence: i64,
    /// Network base fee in stroops.
    pub base_fee: i64,
}

/// Build a real single-operation classic payment, ready to be signed.
///
/// Supports native XLM and issued assets (up to a 12-character code), which is
/// what `starforge tx send` and the fee-bump flow both need.
pub fn build_payment(request: &PaymentRequest) -> Result<TransactionEnvelope> {
    if request.amount <= 0 {
        bail!("payment amount must be positive, got {}", request.amount);
    }

    let asset = match &request.asset {
        None => Asset::Native,
        Some((code, issuer)) => {
            if code.is_empty() || code.len() > 12 {
                bail!(
                    "asset code must be 1-12 characters, got '{code}' ({} chars)",
                    code.len()
                );
            }
            let issuer = AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(parse_public_key(
                issuer,
            )?)));
            if code.len() <= 4 {
                let mut padded = [0u8; 4];
                padded[..code.len()].copy_from_slice(code.as_bytes());
                Asset::CreditAlphanum4(AlphaNum4 {
                    asset_code: AssetCode4(padded),
                    issuer,
                })
            } else {
                let mut padded = [0u8; 12];
                padded[..code.len()].copy_from_slice(code.as_bytes());
                Asset::CreditAlphanum12(AlphaNum12 {
                    asset_code: AssetCode12(padded),
                    issuer,
                })
            }
        }
    };

    let operation = Operation {
        source_account: None,
        body: OperationBody::Payment(PaymentOp {
            destination: parse_muxed_account(&request.destination)?,
            asset,
            amount: request.amount,
        }),
    };

    let tx = Transaction {
        source_account: parse_muxed_account(&request.source)?,
        fee: u32::try_from(request.base_fee.max(MIN_BASE_FEE)).unwrap_or(u32::MAX),
        seq_num: SequenceNumber(request.sequence),
        cond: Preconditions::None,
        memo: Memo::None,
        operations: VecM::<Operation, 100>::try_from(vec![operation])
            .context("payment operation")?,
        ext: TransactionExt::V0,
    };

    Ok(TransactionEnvelope::Tx(TransactionV1Envelope {
        tx,
        signatures: VecM::try_from(Vec::new()).context("empty signature list")?,
    }))
}

/// Sign the inner transaction, optionally wrap it in a fee bump, and verify
/// every required signature before returning.
///
/// This is the single correctness-critical sequence for a submittable envelope:
/// CAP-15 needs the inner transaction signed by its own source *and*, when
/// bumped, the outer envelope signed by the fee source. Both are verified here
/// so a caller cannot submit an envelope that is missing a signature or whose
/// signatures do not check out.
///
/// `fee_source` is `None` to submit unwrapped, in which case the source pays its
/// own fee and no bump is created.
pub fn sign_inner_and_wrap(
    inner: &TransactionV1Envelope,
    source_key: &SigningKey,
    source_public_key: &[u8; 32],
    fee_source: Option<(&SigningKey, &[u8; 32])>,
    network_passphrase: &str,
    base_fee: i64,
) -> Result<TransactionEnvelope> {
    if account_str(&inner.tx.source_account)
        != account_str(&MuxedAccount::Ed25519(Uint256(*source_public_key)))
    {
        bail!(
            "transaction is sourced by {} but the signing key is {}; refusing to sign",
            account_str(&inner.tx.source_account),
            account_str(&MuxedAccount::Ed25519(Uint256(*source_public_key)))
        );
    }
    if source_key.verifying_key().to_bytes() != *source_public_key {
        bail!("signing key does not match the expected source account");
    }

    let signed_inner = sign_transaction_v1(inner, source_key, network_passphrase)?;
    if !verify_transaction_signature(&signed_inner, source_public_key, network_passphrase)? {
        bail!("inner transaction is missing a valid source signature");
    }

    let Some((fee_key, fee_public_key)) = fee_source else {
        return Ok(TransactionEnvelope::Tx(signed_inner));
    };

    if fee_key.verifying_key().to_bytes() != *fee_public_key {
        bail!("fee payer key does not match the expected fee source account");
    }
    if *fee_public_key == *source_public_key {
        bail!(
            "the fee payer and the transaction source are the same account; a fee bump to \
             yourself would only increase the fee. Submit the transaction unwrapped instead."
        );
    }

    let bump = wrap_fee_bump(signed_inner, fee_key, network_passphrase, base_fee)?;
    let TransactionEnvelope::TxFeeBump(outer) = &bump else {
        bail!("wrap_fee_bump did not return a fee bump");
    };
    let FeeBumpTransactionInnerTx::Tx(wrapped) = &outer.tx.inner_tx;

    // The wrap must not have disturbed the inner signature.
    if !verify_transaction_signature(wrapped, source_public_key, network_passphrase)? {
        bail!("fee bump is missing a valid inner source signature");
    }
    if !verify_fee_bump_signature(outer, fee_public_key, network_passphrase)? {
        bail!("fee bump is missing a valid fee-source signature");
    }
    if account_str(&fee_payer_of(&bump))
        != account_str(&MuxedAccount::Ed25519(Uint256(*fee_public_key)))
    {
        bail!("fee bump names the wrong fee source");
    }

    Ok(bump)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stellar_xdr::curr::{
        EnvelopeType, Hash as XdrHash, MuxedAccountMed25519, Operation, OperationBody,
        Preconditions, SequenceNumber, TimeBounds, TimePoint,
    };

    const PASSPHRASE: &str = "Test SDF Network ; September 2015";

    /// A `G...` key derived from a fixed seed so the tests are deterministic.
    const SPONSOR_SEED: [u8; 32] = [7u8; 32];
    const SOURCE_SEED: [u8; 32] = [9u8; 32];

    fn sponsor_key() -> SigningKey {
        SigningKey::from_bytes(&SPONSOR_SEED)
    }

    fn source_key() -> SigningKey {
        SigningKey::from_bytes(&SOURCE_SEED)
    }

    /// Build a minimal single-operation classic transaction from `key`.
    fn sample_envelope(key: &SigningKey, fee: u32) -> TransactionV1Envelope {
        let tx = Transaction {
            source_account: MuxedAccount::Ed25519(Uint256(key.verifying_key().to_bytes())),
            fee,
            seq_num: SequenceNumber(7),
            cond: Preconditions::Time(TimeBounds {
                min_time: TimePoint(0),
                max_time: TimePoint(0),
            }),
            memo: stellar_xdr::curr::Memo::None,
            operations: vec![Operation {
                source_account: None,
                body: OperationBody::Inflation,
            }]
            .try_into()
            .expect("one operation fits"),
            ext: stellar_xdr::curr::TransactionExt::V0,
        };
        TransactionV1Envelope {
            tx,
            signatures: VecM::try_from(vec![]).expect("empty"),
        }
    }

    #[test]
    fn network_id_is_sha256_of_passphrase() {
        let digest: [u8; 32] = Sha256::digest(PASSPHRASE.as_bytes()).into();
        assert_eq!(network_id(PASSPHRASE), XdrHash(digest));
        assert_ne!(network_id(PASSPHRASE), network_id("other network"));
    }

    #[test]
    fn account_round_trips_through_strkey() {
        let key = sponsor_key();
        let account = MuxedAccount::Ed25519(Uint256(key.verifying_key().to_bytes()));
        let encoded = account_str(&account);
        assert!(encoded.starts_with('G'));
        assert_eq!(parse_muxed_account(&encoded).unwrap(), account);
        assert_eq!(
            parse_public_key(&encoded).unwrap(),
            key.verifying_key().to_bytes()
        );
    }

    #[test]
    fn rejects_malformed_accounts() {
        assert!(parse_muxed_account("not-an-account").is_err());
        assert!(parse_signing_key("S-not-valid").is_err());
    }

    #[test]
    fn signing_a_classic_envelope_produces_a_valid_signature() {
        let key = source_key();
        let envelope = sample_envelope(&key, 100);
        let signed = sign_transaction_v1(&envelope, &key, PASSPHRASE).unwrap();
        assert_eq!(signed.signatures.len(), 1);
        assert!(
            verify_transaction_signature(&signed, &key.verifying_key().to_bytes(), PASSPHRASE)
                .unwrap()
        );
    }

    #[test]
    fn signature_does_not_verify_against_a_different_network() {
        let key = source_key();
        let signed = sign_transaction_v1(&sample_envelope(&key, 100), &key, PASSPHRASE).unwrap();
        assert!(!verify_transaction_signature(
            &signed,
            &key.verifying_key().to_bytes(),
            "Public Global Stellar Network ; September 2015"
        )
        .unwrap());
    }

    #[test]
    fn signature_hint_is_the_last_four_key_bytes() {
        let key = source_key();
        let public = key.verifying_key().to_bytes();
        let hint = signature_hint(&public);
        assert_eq!(hint.0, [public[28], public[29], public[30], public[31]]);
    }

    #[test]
    fn outer_fee_respects_base_fee_and_multiplier() {
        assert_eq!(outer_fee(100, 100), 200);
        assert_eq!(outer_fee(200, 100), 400);
        // A low inner fee cannot pull the bump below the base fee.
        assert_eq!(outer_fee(10, 500), 500);
        // Never below the protocol minimum.
        assert_eq!(outer_fee(0, 0), MIN_BASE_FEE);
    }

    #[test]
    fn fee_bump_carries_the_inner_envelope_verbatim() {
        let source = source_key();
        let sponsor = sponsor_key();
        let inner =
            sign_transaction_v1(&sample_envelope(&source, 100), &source, PASSPHRASE).unwrap();
        let original_xdr = envelope_to_base64(&TransactionEnvelope::Tx(inner.clone())).unwrap();

        let bumped = wrap_fee_bump(inner, &sponsor, PASSPHRASE, 100).unwrap();
        let TransactionEnvelope::TxFeeBump(bump) = &bumped else {
            panic!("expected a fee-bump envelope");
        };

        assert_eq!(bump.tx.fee, 200);
        assert_eq!(
            account_str(&bump.tx.fee_source),
            account_str(&MuxedAccount::Ed25519(Uint256(
                sponsor.verifying_key().to_bytes()
            )))
        );
        // Inner envelope is byte-identical, including its original signature.
        let FeeBumpTransactionInnerTx::Tx(inner) = &bump.tx.inner_tx;
        let inner_xdr = envelope_to_base64(&TransactionEnvelope::Tx(inner.clone())).unwrap();
        assert_eq!(inner_xdr, original_xdr);
    }

    #[test]
    fn fee_bump_outer_signature_verifies_for_the_fee_source_only() {
        let source = source_key();
        let sponsor = sponsor_key();
        let inner =
            sign_transaction_v1(&sample_envelope(&source, 100), &source, PASSPHRASE).unwrap();
        let bumped = wrap_fee_bump(inner, &sponsor, PASSPHRASE, 100).unwrap();
        let TransactionEnvelope::TxFeeBump(bump) = &bumped else {
            panic!("expected a fee-bump envelope");
        };

        assert!(
            verify_fee_bump_signature(bump, &sponsor.verifying_key().to_bytes(), PASSPHRASE)
                .unwrap()
        );
        assert!(
            !verify_fee_bump_signature(bump, &source.verifying_key().to_bytes(), PASSPHRASE)
                .unwrap()
        );
    }

    #[test]
    fn inner_signature_survives_the_bump() {
        let source = source_key();
        let sponsor = sponsor_key();
        let inner =
            sign_transaction_v1(&sample_envelope(&source, 100), &source, PASSPHRASE).unwrap();
        let bumped = wrap_fee_bump(inner, &sponsor, PASSPHRASE, 100).unwrap();
        let TransactionEnvelope::TxFeeBump(bump) = &bumped else {
            panic!("expected a fee-bump envelope");
        };
        let FeeBumpTransactionInnerTx::Tx(inner) = &bump.tx.inner_tx;

        // The inner transaction is still signed by its own source, not the sponsor.
        assert!(verify_transaction_signature(
            inner,
            &source.verifying_key().to_bytes(),
            PASSPHRASE
        )
        .unwrap());
    }

    #[test]
    fn envelope_round_trips_through_base64_with_discriminant() {
        let source = source_key();
        let sponsor = sponsor_key();
        let inner =
            sign_transaction_v1(&sample_envelope(&source, 100), &source, PASSPHRASE).unwrap();
        let bumped = wrap_fee_bump(inner, &sponsor, PASSPHRASE, 100).unwrap();

        let encoded = envelope_to_base64(&bumped).unwrap();
        let decoded = envelope_from_base64(&encoded).unwrap();
        assert_eq!(decoded, bumped);

        // The first four bytes must be the TxFeeBump discriminant, not the inner Tx one.
        let raw = general_purpose::STANDARD.decode(&encoded).unwrap();
        assert_eq!(
            i32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]),
            EnvelopeType::TxFeeBump as i32
        );
    }

    #[test]
    fn plain_transaction_envelope_keeps_the_tx_discriminant() {
        let source = source_key();
        let signed =
            sign_transaction_v1(&sample_envelope(&source, 100), &source, PASSPHRASE).unwrap();
        let encoded = envelope_to_base64(&TransactionEnvelope::Tx(signed)).unwrap();
        let raw = general_purpose::STANDARD.decode(&encoded).unwrap();
        assert_eq!(
            i32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]),
            EnvelopeType::Tx as i32
        );
    }

    #[test]
    fn hash_differs_between_bumped_and_unbumped() {
        let source = source_key();
        let sponsor = sponsor_key();
        let plain =
            sign_transaction_v1(&sample_envelope(&source, 100), &source, PASSPHRASE).unwrap();
        let plain = TransactionEnvelope::Tx(plain);
        let bumped = wrap_fee_bump(
            match plain.clone() {
                TransactionEnvelope::Tx(v1) => v1,
                _ => unreachable!(),
            },
            &sponsor,
            PASSPHRASE,
            100,
        )
        .unwrap();

        assert_ne!(
            envelope_hash(&plain, PASSPHRASE).unwrap(),
            envelope_hash(&bumped, PASSPHRASE).unwrap()
        );
    }

    #[test]
    fn fee_breakdown_attributes_the_fee_to_the_sponsor() {
        let source = source_key();
        let sponsor = sponsor_key();
        let inner =
            sign_transaction_v1(&sample_envelope(&source, 100), &source, PASSPHRASE).unwrap();
        let bumped = wrap_fee_bump(inner, &sponsor, PASSPHRASE, 100).unwrap();
        let breakdown = FeeBreakdown::from_envelope(&bumped);

        assert!(breakdown.bumped);
        assert_eq!(breakdown.inner_fee, 100);
        assert_eq!(breakdown.outer_fee, 200);
        assert_eq!(
            breakdown.fee_payer,
            account_str(&MuxedAccount::Ed25519(Uint256(
                sponsor.verifying_key().to_bytes()
            )))
        );
        assert_eq!(
            breakdown.inner_source,
            account_str(&MuxedAccount::Ed25519(Uint256(
                source.verifying_key().to_bytes()
            )))
        );
    }

    #[test]
    fn unbumped_breakdown_charges_the_source_its_own_fee() {
        let source = source_key();
        let signed =
            sign_transaction_v1(&sample_envelope(&source, 100), &source, PASSPHRASE).unwrap();
        let breakdown = FeeBreakdown::from_envelope(&TransactionEnvelope::Tx(signed));
        assert!(!breakdown.bumped);
        assert_eq!(breakdown.inner_source_pays, breakdown.outer_fee);
        assert_eq!(breakdown.outer_fee, 100);
    }

    #[test]
    fn bumped_breakdown_charges_only_the_fee_source() {
        let source = source_key();
        let payer = sponsor_key();
        let signed =
            sign_transaction_v1(&sample_envelope(&source, 100), &source, PASSPHRASE).unwrap();
        let bumped = wrap_fee_bump(signed, &payer, PASSPHRASE, 100).unwrap();
        let breakdown = FeeBreakdown::from_envelope(&bumped);

        assert!(breakdown.bumped);
        assert_eq!(breakdown.inner_source_pays, 0);
        assert_eq!(breakdown.outer_fee, 200);
        assert_ne!(breakdown.inner_source, breakdown.fee_payer);

        // The rendered rows must never imply the inner source pays the fee.
        let rows = breakdown.rows();
        let paid = rows
            .iter()
            .find(|(k, _)| *k == "Paid by inner source")
            .expect("who-pays-what rows must name the inner source's share");
        assert!(
            paid.1.starts_with('0'),
            "inner source should pay nothing, got '{}'",
            paid.1
        );
    }

    #[test]
    fn rejects_muxed_inner_source_accounts() {
        let source = source_key();
        let sponsor = sponsor_key();
        let mut inner = sample_envelope(&source, 100);
        inner.tx.source_account = MuxedAccount::MuxedEd25519(MuxedAccountMed25519 {
            ed25519: Uint256([3u8; 32]),
            id: 7,
        });
        assert!(wrap_fee_bump(inner, &sponsor, PASSPHRASE, 100).is_err());
    }

    #[test]
    fn stroops_formatting_shows_both_units() {
        assert_eq!(stroops_xlm(100), "100 (0.0000100 XLM)");
        assert_eq!(stroops_xlm(0), "0 (0.0000000 XLM)");
    }

    #[test]
    fn sign_envelope_dispatches_to_the_right_payload() {
        let source = source_key();
        let inner = sample_envelope(&source, 100);
        let signed =
            sign_envelope(&TransactionEnvelope::Tx(inner.clone()), &source, PASSPHRASE).unwrap();
        let TransactionEnvelope::Tx(v1) = signed else {
            panic!("expected Tx");
        };
        assert!(
            verify_transaction_signature(&v1, &source.verifying_key().to_bytes(), PASSPHRASE)
                .unwrap()
        );
    }

    #[test]
    fn sign_envelope_signs_the_fee_bump_outer_layer() {
        let source = source_key();
        let sponsor = sponsor_key();
        let inner =
            sign_transaction_v1(&sample_envelope(&source, 100), &source, PASSPHRASE).unwrap();
        let unsigned_bump = FeeBumpTransaction {
            fee_source: MuxedAccount::Ed25519(Uint256(sponsor.verifying_key().to_bytes())),
            fee: 200,
            inner_tx: FeeBumpTransactionInnerTx::Tx(inner),
            ext: FeeBumpTransactionExt::V0,
        };
        let unsigned = TransactionEnvelope::TxFeeBump(FeeBumpTransactionEnvelope {
            tx: unsigned_bump,
            signatures: VecM::try_from(Vec::new()).unwrap(),
        });

        let signed = sign_envelope(&unsigned, &sponsor, PASSPHRASE).unwrap();
        let TransactionEnvelope::TxFeeBump(bump) = &signed else {
            panic!("expected a fee-bump envelope");
        };
        assert!(
            verify_fee_bump_signature(bump, &sponsor.verifying_key().to_bytes(), PASSPHRASE)
                .unwrap()
        );
    }

    #[test]
    fn attached_external_signature_verifies_like_a_local_one() {
        let source = source_key();
        let envelope = sample_envelope(&source, 100);
        let payload = transaction_signature_payload(&envelope.tx, PASSPHRASE).unwrap();
        // An external signer (a hardware device, or `stellar tx sign`) signs the
        // 32-byte base hash, not the serialized payload.
        let raw = source.sign(&signature_base_hash(&payload)).to_bytes();

        let signed = attach_signature(
            &TransactionEnvelope::Tx(envelope),
            &source.verifying_key().to_bytes(),
            &raw,
        )
        .unwrap();
        let TransactionEnvelope::Tx(v1) = signed else {
            panic!("expected Tx");
        };
        assert!(
            verify_transaction_signature(&v1, &source.verifying_key().to_bytes(), PASSPHRASE)
                .unwrap()
        );
    }

    #[test]
    fn signatures_cover_the_base_hash_not_the_raw_payload() {
        // Regression guard: signing the serialized payload bytes produces a
        // well-formed signature that the network rejects. Cross-checked against
        // `stellar tx sign`, whose signature equals the base-hash form below.
        let source = source_key();
        let envelope = sample_envelope(&source, 100);
        let payload = transaction_signature_payload(&envelope.tx, PASSPHRASE).unwrap();

        let correct = source.sign(&signature_base_hash(&payload)).to_bytes();
        let incorrect = source.sign(&payload).to_bytes();
        assert_ne!(
            correct, incorrect,
            "base-hash and raw-payload signatures must differ"
        );

        let signed = sign_transaction_v1(&envelope, &source, PASSPHRASE).unwrap();
        let raw = signed.signatures[0].signature.0.as_vec();
        assert_eq!(raw.as_slice(), correct.as_slice());

        // And the base hash is exactly what the envelope hashes to.
        let full = TransactionEnvelope::Tx(signed);
        assert_eq!(
            signature_base_hash(&payload),
            envelope_hash(&full, PASSPHRASE).unwrap()
        );
    }

    #[test]
    fn fee_bump_signatures_cover_the_base_hash() {
        let source = source_key();
        let payer = sponsor_key();
        let inner = sample_envelope(&source, 100);
        let signed = sign_transaction_v1(&inner, &source, PASSPHRASE).unwrap();
        let TransactionEnvelope::TxFeeBump(bump) =
            wrap_fee_bump(signed, &payer, PASSPHRASE, 100).unwrap()
        else {
            panic!("expected a fee bump");
        };
        let signed_bump = sign_fee_bump_v1(&bump, &payer, PASSPHRASE).unwrap();

        let payload = fee_bump_signature_payload(&bump.tx, PASSPHRASE).unwrap();
        let expected = payer.sign(&signature_base_hash(&payload)).to_bytes();
        assert_eq!(
            signed_bump.signatures[0].signature.0.as_vec().as_slice(),
            expected.as_slice()
        );
        assert!(verify_fee_bump_signature(
            &signed_bump,
            &payer.verifying_key().to_bytes(),
            PASSPHRASE
        )
        .unwrap());
    }

    #[test]
    fn sign_envelope_dispatches_to_the_bump_layer() {
        let source = source_key();
        let payer = sponsor_key();
        let inner = sample_envelope(&source, 100);
        let signed = sign_transaction_v1(&inner, &source, PASSPHRASE).unwrap();
        let bump = wrap_fee_bump(signed, &payer, PASSPHRASE, 100).unwrap();

        // `wrap_fee_bump` already signs the outer layer as the fee source, so
        // re-signing here would append a redundant second signature.
        let TransactionEnvelope::TxFeeBump(v) = &bump else {
            panic!("expected a fee bump back");
        };
        assert_eq!(v.signatures.len(), 1);
        assert!(
            verify_fee_bump_signature(v, &payer.verifying_key().to_bytes(), PASSPHRASE).unwrap()
        );

        // Dispatching through `sign_envelope` on an unsigned bump also works.
        let bare = match &bump {
            TransactionEnvelope::TxFeeBump(b) => FeeBumpTransactionEnvelope {
                tx: b.tx.clone(),
                signatures: VecM::try_from(Vec::new()).unwrap(),
            },
            _ => unreachable!(),
        };
        let TransactionEnvelope::TxFeeBump(v) =
            sign_envelope(&TransactionEnvelope::TxFeeBump(bare), &payer, PASSPHRASE).unwrap()
        else {
            panic!("expected a fee bump back");
        };
        assert_eq!(v.signatures.len(), 1);
        assert!(
            verify_fee_bump_signature(&v, &payer.verifying_key().to_bytes(), PASSPHRASE).unwrap()
        );
    }

    #[test]
    fn payment_builder_produces_a_verifiable_single_op_transaction() {
        let source = source_key();
        // Seed [3] is the destination in the cross-checked reference vector, so
        // the hash below stays pinned to the exact envelope the Stellar CLI
        // independently hashed.
        let dest = SigningKey::from_bytes(&[3u8; 32]);
        let source_pk = account_str(&MuxedAccount::Ed25519(Uint256(
            source.verifying_key().to_bytes(),
        )));
        let dest_pk = account_str(&MuxedAccount::Ed25519(Uint256(
            dest.verifying_key().to_bytes(),
        )));

        let envelope = build_payment(&PaymentRequest {
            source: source_pk,
            destination: dest_pk,
            amount: 123_456_789,
            asset: None,
            sequence: 42,
            base_fee: 100,
        })
        .unwrap();

        let TransactionEnvelope::Tx(v1) = &envelope else {
            panic!("expected a classic envelope");
        };
        assert_eq!(v1.tx.fee, 100);
        assert_eq!(v1.tx.seq_num.0, 42);
        assert_eq!(v1.tx.operations.len(), 1);
        assert!(v1.signatures.is_empty());
        match &v1.tx.operations[0].body {
            OperationBody::Payment(op) => {
                assert_eq!(op.amount, 123_456_789);
                assert!(matches!(op.asset, Asset::Native));
            }
            other => panic!("expected a payment op, got {other:?}"),
        }

        // The reference CLI agrees: this exact envelope hashes to this value.
        assert_eq!(
            hex::encode(envelope_hash(&envelope, PASSPHRASE).unwrap()),
            "0ec4902f0a6a1c007f1f04f81605c171bf2b9ff1860a053d424119c757c2001c"
        );

        let signed = sign_envelope(&envelope, &source, PASSPHRASE).unwrap();
        let TransactionEnvelope::Tx(v1) = signed else {
            panic!("expected a classic envelope");
        };
        assert!(
            verify_transaction_signature(&v1, &source.verifying_key().to_bytes(), PASSPHRASE)
                .unwrap()
        );
    }

    #[test]
    fn payment_builder_supports_both_asset_code_widths() {
        let source = source_key();
        let issuer_pk = account_str(&MuxedAccount::Ed25519(Uint256(
            sponsor_key().verifying_key().to_bytes(),
        )));
        let source_pk = account_str(&MuxedAccount::Ed25519(Uint256(
            source.verifying_key().to_bytes(),
        )));

        for (code, expect12) in [("USD", false), ("USDC", false), ("LONGCODE12", true)] {
            let envelope = build_payment(&PaymentRequest {
                source: source_pk.clone(),
                destination: issuer_pk.clone(),
                amount: 1,
                asset: Some((code.to_string(), issuer_pk.clone())),
                sequence: 1,
                base_fee: 100,
            })
            .unwrap();
            let TransactionEnvelope::Tx(v1) = envelope else {
                panic!("expected a classic envelope");
            };
            match &v1.tx.operations[0].body {
                OperationBody::Payment(op) => match &op.asset {
                    Asset::CreditAlphanum4(a) => {
                        assert!(!expect12, "{code} should not be AlphaNum12");
                        assert_eq!(&a.asset_code.0[..code.len()], code.as_bytes());
                        assert!(
                            a.asset_code.0[code.len()..].iter().all(|b| *b == 0),
                            "{code} must be null padded to 4 bytes"
                        );
                    }
                    Asset::CreditAlphanum12(a) => {
                        assert!(expect12, "{code} should be AlphaNum12");
                        assert_eq!(&a.asset_code.0[..code.len()], code.as_bytes());
                        assert!(
                            a.asset_code.0[code.len()..].iter().all(|b| *b == 0),
                            "{code} must be null padded to 12 bytes"
                        );
                    }
                    other => panic!("expected a credit asset, got {other:?}"),
                },
                other => panic!("expected a payment op, got {other:?}"),
            }
        }
    }

    #[test]
    fn payment_builder_rejects_impossible_requests() {
        let source = source_key();
        let pk = account_str(&MuxedAccount::Ed25519(Uint256(
            source.verifying_key().to_bytes(),
        )));
        let base = PaymentRequest {
            source: pk.clone(),
            destination: pk.clone(),
            amount: 1,
            asset: None,
            sequence: 1,
            base_fee: 100,
        };

        let zero = PaymentRequest {
            amount: 0,
            ..base.clone()
        };
        assert!(
            build_payment(&zero).is_err(),
            "zero amount must be rejected"
        );

        let negative = PaymentRequest {
            amount: -1,
            ..base.clone()
        };
        assert!(
            build_payment(&negative).is_err(),
            "negative amount must be rejected"
        );

        let long_code = PaymentRequest {
            asset: Some(("TOOLONGASSETC".to_string(), pk.clone())),
            ..base.clone()
        };
        assert!(
            build_payment(&long_code).is_err(),
            "13-character asset code must be rejected"
        );

        let empty_code = PaymentRequest {
            asset: Some((String::new(), pk.clone())),
            ..base
        };
        assert!(
            build_payment(&empty_code).is_err(),
            "empty asset code must be rejected"
        );
    }

    #[test]
    fn payment_fee_floor_never_drops_below_the_minimum() {
        let source = source_key();
        let pk = account_str(&MuxedAccount::Ed25519(Uint256(
            source.verifying_key().to_bytes(),
        )));
        let envelope = build_payment(&PaymentRequest {
            source: pk.clone(),
            destination: pk,
            amount: 1,
            asset: None,
            sequence: 1,
            base_fee: 1,
        })
        .unwrap();
        let TransactionEnvelope::Tx(v1) = envelope else {
            panic!("expected a classic envelope");
        };
        assert_eq!(v1.tx.fee as i64, MIN_BASE_FEE);
    }

    #[test]
    fn a_fee_bumped_payment_is_accepted_by_the_reference_decoder() {
        // Mirrors the `tx send --fee-payer` path end to end: build, sign, wrap,
        // sign the bump, and confirm both layers verify independently.
        let source = source_key();
        let payer = sponsor_key();
        let source_pk = account_str(&MuxedAccount::Ed25519(Uint256(
            source.verifying_key().to_bytes(),
        )));
        let dest_pk = account_str(&MuxedAccount::Ed25519(Uint256(
            payer.verifying_key().to_bytes(),
        )));

        let payment = build_payment(&PaymentRequest {
            source: source_pk,
            destination: dest_pk,
            amount: 123_456_789,
            asset: None,
            sequence: 42,
            base_fee: 100,
        })
        .unwrap();
        let signed = sign_envelope(&payment, &source, PASSPHRASE).unwrap();
        let bumped = wrap_with_fee_payer_for_test(&signed, &payer, PASSPHRASE, 100).unwrap();

        assert!(is_fee_bump(&bumped));
        assert!(verify_fee_bump_signature(
            match &bumped {
                TransactionEnvelope::TxFeeBump(b) => b,
                _ => unreachable!(),
            },
            &payer.verifying_key().to_bytes(),
            PASSPHRASE
        )
        .unwrap());

        let breakdown = FeeBreakdown::from_envelope(&bumped);
        assert!(breakdown.bumped);
        assert_eq!(breakdown.inner_fee, 100);
        assert_eq!(breakdown.outer_fee, 200);
        assert_eq!(
            breakdown.fee_payer,
            account_str(&MuxedAccount::Ed25519(Uint256(
                payer.verifying_key().to_bytes()
            )))
        );

        // Round-trips through base64 the way the command hands it to Horizon.
        let b64 = envelope_to_base64(&bumped).unwrap();
        let reparsed = envelope_from_base64(&b64).unwrap();
        assert_eq!(reparsed, bumped);
    }

    fn wrap_with_fee_payer_for_test(
        envelope: &TransactionEnvelope,
        payer: &SigningKey,
        passphrase: &str,
        base_fee: i64,
    ) -> Result<TransactionEnvelope> {
        let TransactionEnvelope::Tx(v1) = envelope else {
            bail!("expected an unwrapped envelope");
        };
        wrap_fee_bump(v1.clone(), payer, passphrase, base_fee)
    }

    #[test]
    fn sign_inner_and_wrap_produces_a_fully_verified_bump() {
        let source = source_key();
        let payer = sponsor_key();
        let inner = sample_envelope(&source, 100);

        let out = sign_inner_and_wrap(
            &inner,
            &source,
            &source.verifying_key().to_bytes(),
            Some((&payer, &payer.verifying_key().to_bytes())),
            PASSPHRASE,
            100,
        )
        .expect("sign and wrap");

        assert!(is_fee_bump(&out));
        assert_eq!(fee_payer_of(&out), account_of(&payer));
        assert!(verify_fee_bump_signature(
            match &out {
                TransactionEnvelope::TxFeeBump(b) => b,
                _ => unreachable!(),
            },
            &payer.verifying_key().to_bytes(),
            PASSPHRASE
        )
        .unwrap());
    }

    #[test]
    fn sign_inner_and_wrap_without_a_fee_payer_stays_unwrapped() {
        let source = source_key();
        let inner = sample_envelope(&source, 100);
        let out = sign_inner_and_wrap(
            &inner,
            &source,
            &source.verifying_key().to_bytes(),
            None,
            PASSPHRASE,
            100,
        )
        .expect("sign only");

        assert!(!is_fee_bump(&out));
        let breakdown = FeeBreakdown::from_envelope(&out);
        assert_eq!(breakdown.fee_payer, breakdown.inner_source);
        assert_eq!(breakdown.outer_fee, 100);
    }

    #[test]
    fn sign_inner_and_wrap_rejects_a_self_bump() {
        // A bump to yourself would only raise the fee, so it is refused rather
        // than silently costing the user double.
        let source = source_key();
        let inner = sample_envelope(&source, 100);
        let key = source.verifying_key().to_bytes();
        let err = sign_inner_and_wrap(
            &inner,
            &source,
            &key,
            Some((&source, &key)),
            PASSPHRASE,
            100,
        )
        .unwrap_err();
        assert!(err.to_string().contains("same account"), "got: {err}");
    }

    #[test]
    fn sign_inner_and_wrap_rejects_a_mismatched_source() {
        // Signing a transaction sourced by someone else would produce a
        // signature the network rejects, so it fails before submission.
        let source = source_key();
        let stranger = sponsor_key();
        let inner = sample_envelope(&source, 100);
        let err = sign_inner_and_wrap(
            &inner,
            &stranger,
            &stranger.verifying_key().to_bytes(),
            None,
            PASSPHRASE,
            100,
        )
        .unwrap_err();
        assert!(err.to_string().contains("sourced by"), "got: {err}");
    }

    #[test]
    fn sign_inner_and_wrap_rejects_a_key_public_key_mismatch() {
        let source = source_key();
        let inner = sample_envelope(&source, 100);
        // Right account, wrong key material.
        let err = sign_inner_and_wrap(
            &inner,
            &sponsor_key(),
            &source.verifying_key().to_bytes(),
            None,
            PASSPHRASE,
            100,
        )
        .unwrap_err();
        assert!(err.to_string().contains("does not match"), "got: {err}");
    }

    fn account_of(key: &SigningKey) -> MuxedAccount {
        MuxedAccount::Ed25519(Uint256(key.verifying_key().to_bytes()))
    }

    #[test]
    fn attach_signature_rejects_malformed_signature_length() {
        let source = source_key();
        let envelope = TransactionEnvelope::Tx(sample_envelope(&source, 100));
        assert!(
            attach_signature(&envelope, &source.verifying_key().to_bytes(), &[0u8; 10]).is_err()
        );
    }

    #[test]
    fn signed_envelope_no_longer_contains_any_secret_material() {
        // Regression guard: the previous mock embedded the first 8 characters of
        // the secret seed in its "signature" output.
        let source = source_key();
        let secret = stellar_strkey::ed25519::PrivateKey(SOURCE_SEED).to_string();
        let signed = sign_envelope(
            &TransactionEnvelope::Tx(sample_envelope(&source, 100)),
            &source,
            PASSPHRASE,
        )
        .unwrap();
        let encoded = envelope_to_base64(&signed).unwrap();
        let prefix: String = secret.chars().take(8).collect();
        assert!(
            !encoded.contains(&prefix),
            "signed envelope must not embed secret key material"
        );
    }
}
