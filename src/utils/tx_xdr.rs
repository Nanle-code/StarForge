//! Raw transaction-envelope toolbox (#916).
//!
//! Composable, offline building blocks for XDR that arrives from wallets,
//! Stellar Lab, multisig coordinators or failed CI runs: decode to JSON,
//! encode from JSON, sign with a real ed25519 key, and surface the signature
//! hash so a signature can be checked against it.
//!
//! Nothing here touches the network, so an operator can inspect and re-sign a
//! blob on a machine with no connectivity. `stellar-xdr` is pinned in this
//! workspace without its `base64` feature, so base64 handling is explicit
//! (the same approach as `utils::sep10`).

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use ed25519_dalek::{Signature as Ed25519Signature, Signer, SigningKey, VerifyingKey};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::{IsTerminal, Read};
use stellar_strkey::ed25519 as strkey;
use stellar_xdr::curr::{
    AccountId, BytesM, DecoratedSignature, FeeBumpTransaction, FeeBumpTransactionEnvelope,
    FeeBumpTransactionInnerTx, Hash, Limits, Memo, MuxedAccount, Operation, OperationBody,
    Preconditions, PublicKey, ReadXdr, SequenceNumber, Signature, SignatureHint, Transaction,
    TransactionEnvelope, TransactionExt, TransactionSignaturePayload,
    TransactionSignaturePayloadTaggedTransaction, TransactionV1Envelope, VecM, WriteXdr,
};

/// How an envelope is rendered on the wire / on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireFormat {
    Base64,
    Hex,
}

impl WireFormat {
    pub fn parse(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "base64" | "b64" => Ok(Self::Base64),
            "hex" => Ok(Self::Hex),
            other => bail!("Unknown XDR format '{other}'. Use 'base64' or 'hex'."),
        }
    }
}

/// A decoded envelope: JSON body plus the summary and hash around it.
#[derive(Debug, Clone, Serialize)]
pub struct DecodedEnvelope {
    pub kind: &'static str,
    pub summary: EnvelopeSummary,
    /// Hex SHA-256 of the network passphrase, when a network was supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_id: Option<String>,
    /// Hex transaction hash; `None` for legacy v0 envelopes, whose signature
    /// payload this crate version cannot express.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    pub envelope: serde_json::Value,
}

/// Who signs, what the transaction does, and who has already signed.
#[derive(Debug, Clone, Serialize)]
pub struct EnvelopeSummary {
    pub source_account: String,
    pub fee_stroops: u64,
    pub seq_num: String,
    pub operations: Vec<String>,
    pub signature_hints: Vec<String>,
}

/// Reads a payload from an explicit argument, a `--file`, or stdin.
///
/// Piping is the point of this toolbox (`tx encode | tx sign | tx submit`),
/// so stdin is the fallback whenever no argument or file was given.
pub fn read_payload(
    value: Option<&str>,
    file: Option<&std::path::Path>,
    kind: &str,
    command: &str,
) -> Result<String> {
    if let Some(value) = value {
        return Ok(value.trim().to_string());
    }
    if let Some(path) = file {
        return std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {kind} file {}", path.display()))
            .map(|contents| contents.trim().to_string());
    }
    if std::io::stdin().is_terminal() {
        bail!(
            "No {kind} given. Pass it as an argument, use --file, or pipe it in:\n  \
             starforge tx encode … | starforge tx {command}"
        );
    }
    let mut buffer = String::new();
    std::io::stdin()
        .read_to_string(&mut buffer)
        .context("Failed to read from stdin")?;
    let trimmed = buffer.trim().to_string();
    if trimmed.is_empty() {
        bail!("Nothing received on stdin.");
    }
    Ok(trimmed)
}

/// Parses an envelope from base64 (what wallets and Horizon use) or hex.
///
/// Both are tried and the first that yields a valid envelope wins: a hex string
/// is made entirely of base64 alphabet characters, so guessing from the
/// character set alone would decode `--format hex` output into the wrong bytes.
pub fn parse_envelope(input: &str) -> Result<TransactionEnvelope> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        bail!("Empty XDR input.");
    }

    if let Ok(bytes) = general_purpose::STANDARD.decode(trimmed) {
        if let Ok(envelope) = TransactionEnvelope::from_xdr(bytes, Limits::none()) {
            return Ok(envelope);
        }
    }
    if let Ok(bytes) = hex::decode(trimmed) {
        if let Ok(envelope) = TransactionEnvelope::from_xdr(bytes, Limits::none()) {
            return Ok(envelope);
        }
    }

    bail!(
        "Input is not a valid transaction envelope: {} characters are neither base64 nor hex XDR.",
        trimmed.len()
    )
}

pub fn write_envelope(envelope: &TransactionEnvelope, format: WireFormat) -> Result<String> {
    let bytes = envelope
        .to_xdr(Limits::none())
        .context("Failed to serialize transaction envelope")?;
    Ok(match format {
        WireFormat::Base64 => general_purpose::STANDARD.encode(bytes),
        WireFormat::Hex => hex::encode(bytes),
    })
}

/// Parses a JSON envelope as produced by [`to_json`], rejecting anything that
/// cannot be re-encoded as XDR so failures surface at encode time.
pub fn encode_json(json: &str) -> Result<TransactionEnvelope> {
    let envelope: TransactionEnvelope = serde_json::from_str(json).context(
        "JSON is not a transaction envelope. Run `tx decode` on a real envelope to see the shape.",
    )?;
    envelope
        .to_xdr(Limits::none())
        .context("Decoded envelope cannot be re-encoded as XDR")?;
    Ok(envelope)
}

pub fn to_json(envelope: &TransactionEnvelope, pretty: bool) -> Result<String> {
    let value = serde_json::to_value(envelope).context("Failed to convert envelope to JSON")?;
    if pretty {
        serde_json::to_string_pretty(&value).context("Failed to render JSON")
    } else {
        serde_json::to_string(&value).context("Failed to render JSON")
    }
}

/// Decodes an envelope, optionally resolving the hash for `network_passphrase`.
pub fn decode(input: &str, network_passphrase: Option<&str>) -> Result<DecodedEnvelope> {
    let envelope = parse_envelope(input)?;
    let hash = match network_passphrase {
        Some(passphrase) => Some(hex::encode(signature_hash(&envelope, passphrase)?)),
        None => None,
    };
    let network_id =
        network_passphrase.map(|passphrase| hex::encode(sha256_passphrase(passphrase)));
    Ok(DecodedEnvelope {
        kind: envelope_kind(&envelope),
        summary: summarize(&envelope),
        network_id,
        hash,
        envelope: serde_json::to_value(&envelope).context("Failed to convert envelope to JSON")?,
    })
}

/// The byte string every Stellar signature covers:
/// `sha256(network_id || envelope_type || transaction)`.
pub fn signature_hash(
    envelope: &TransactionEnvelope,
    network_passphrase: &str,
) -> Result<[u8; 32]> {
    let tagged = match envelope {
        TransactionEnvelope::Tx(v1) => {
            TransactionSignaturePayloadTaggedTransaction::Tx(v1.tx.clone())
        }
        TransactionEnvelope::TxFeeBump(fb) => {
            TransactionSignaturePayloadTaggedTransaction::TxFeeBump(fb.tx.clone())
        }
        TransactionEnvelope::TxV0(_) => bail!(
            "Legacy v0 envelopes have no v1 signature payload. Rebuild the transaction as a v1 \
             envelope before hashing, signing, or submitting it."
        ),
    };
    let payload = TransactionSignaturePayload {
        network_id: Hash(sha256_passphrase(network_passphrase)),
        tagged_transaction: tagged,
    };
    let xdr = payload
        .to_xdr(Limits::none())
        .context("Failed to serialize signature payload")?;
    Ok(Sha256::digest(xdr).into())
}

fn sha256_passphrase(network_passphrase: &str) -> [u8; 32] {
    let digest = Sha256::digest(network_passphrase.as_bytes());
    let mut network_id = [0u8; 32];
    network_id.copy_from_slice(&digest);
    network_id
}

/// Appends an ed25519 signature for `secret_key` (an `S…` strkey).
///
/// The secret is used only to produce the signature; the returned envelope
/// carries the signature and its 4-byte hint. The signature is re-verified
/// before the envelope is handed back.
pub fn sign_envelope(
    envelope: &TransactionEnvelope,
    secret_key: &str,
    network_passphrase: &str,
) -> Result<TransactionEnvelope> {
    let secret = strkey::PrivateKey::from_string(secret_key.trim())
        .map_err(|_| anyhow::anyhow!("Invalid secret key: expected a Stellar 'S…' strkey."))?;
    let signing_key = SigningKey::from_bytes(&secret.0);
    let public_key = signing_key.verifying_key().to_bytes();

    let hash = signature_hash(envelope, network_passphrase)?;
    let signature = signing_key.sign(&hash).to_bytes();
    let decorated = DecoratedSignature {
        hint: SignatureHint([
            public_key[28],
            public_key[29],
            public_key[30],
            public_key[31],
        ]),
        signature: Signature(
            BytesM::try_from(signature.to_vec())
                .map_err(|e| anyhow::anyhow!("Invalid signature length: {e}"))?,
        ),
    };

    let signed = match envelope {
        TransactionEnvelope::Tx(v1) => {
            let mut signatures = v1.signatures.to_vec();
            signatures.push(decorated);
            TransactionEnvelope::Tx(TransactionV1Envelope {
                tx: v1.tx.clone(),
                signatures: vecm(signatures, "too many signatures")?,
            })
        }
        TransactionEnvelope::TxFeeBump(fb) => {
            let mut signatures = fb.signatures.to_vec();
            signatures.push(decorated);
            TransactionEnvelope::TxFeeBump(FeeBumpTransactionEnvelope {
                tx: fb.tx.clone(),
                signatures: vecm(signatures, "too many signatures")?,
            })
        }
        TransactionEnvelope::TxV0(_) => bail!(
            "Legacy v0 envelopes cannot be signed with this toolbox. Rebuild the transaction as a \
             v1 envelope."
        ),
    };

    let signer = strkey::PublicKey(public_key).to_string();
    if !verify_signature(&signed, network_passphrase, &signer)? {
        bail!("Internal error: the signature just produced does not verify.");
    }

    Ok(signed)
}

fn vecm<T, const MAX: u32>(values: Vec<T>, context: &str) -> Result<VecM<T, MAX>> {
    VecM::try_from(values).map_err(|e| anyhow::anyhow!("{context}: {e}"))
}

/// True when one of the envelope's signatures validates against `public_key`
/// (a `G…` strkey) over the hash for `network_passphrase`.
pub fn verify_signature(
    envelope: &TransactionEnvelope,
    network_passphrase: &str,
    public_key: &str,
) -> Result<bool> {
    let public = strkey::PublicKey::from_string(public_key.trim())
        .map_err(|_| anyhow::anyhow!("Invalid public key: expected a 'G…' strkey."))?;
    let verifier = VerifyingKey::from_bytes(&public.0)
        .map_err(|_| anyhow::anyhow!("Public key {public_key} is not a valid ed25519 key."))?;
    let hash = signature_hash(envelope, network_passphrase)?;

    for decorated in signatures(envelope) {
        let Ok(bytes): Result<[u8; 64], _> = decorated.signature.to_vec().try_into() else {
            continue;
        };
        if verifier
            .verify_strict(&hash, &Ed25519Signature::from_bytes(&bytes))
            .is_ok()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The public key (`G…`) an `S…` secret key corresponds to.
pub fn public_key_for_secret(secret_key: &str) -> Result<String> {
    let secret = strkey::PrivateKey::from_string(secret_key.trim())
        .map_err(|_| anyhow::anyhow!("Invalid secret key: expected a Stellar 'S…' strkey."))?;
    Ok(strkey::PublicKey(SigningKey::from_bytes(&secret.0).verifying_key().to_bytes()).to_string())
}

fn signatures(envelope: &TransactionEnvelope) -> &[DecoratedSignature] {
    match envelope {
        TransactionEnvelope::Tx(v1) => &v1.signatures,
        TransactionEnvelope::TxFeeBump(fb) => &fb.signatures,
        TransactionEnvelope::TxV0(v0) => &v0.signatures,
    }
}

pub fn envelope_kind(envelope: &TransactionEnvelope) -> &'static str {
    match envelope {
        TransactionEnvelope::Tx(_) => "transaction",
        TransactionEnvelope::TxV0(_) => "transaction (v0)",
        TransactionEnvelope::TxFeeBump(_) => "fee bump",
    }
}

/// Source account, fee, sequence, operation types and signature hints.
pub fn summarize(envelope: &TransactionEnvelope) -> EnvelopeSummary {
    let (source_account, fee_stroops, seq_num, operations) = match envelope {
        TransactionEnvelope::Tx(v1) => {
            let tx = &v1.tx;
            (
                tx.source_account.to_string(),
                tx.fee as u64,
                tx.seq_num.0.to_string(),
                tx.operations.as_slice(),
            )
        }
        TransactionEnvelope::TxFeeBump(fb) => {
            let inner = inner_transaction(&fb.tx);
            (
                fb.tx.fee_source.to_string(),
                fb.tx.fee.max(0) as u64,
                inner.seq_num.0.to_string(),
                inner.operations.as_slice(),
            )
        }
        TransactionEnvelope::TxV0(v0) => (
            AccountId(PublicKey::PublicKeyTypeEd25519(
                v0.tx.source_account_ed25519.clone(),
            ))
            .to_string(),
            v0.tx.fee as u64,
            v0.tx.seq_num.0.to_string(),
            v0.tx.operations.as_slice(),
        ),
    };

    EnvelopeSummary {
        source_account,
        fee_stroops,
        seq_num,
        operations: operations
            .iter()
            .map(|op| operation_kind(&op.body).to_string())
            .collect(),
        signature_hints: signatures(envelope)
            .iter()
            .map(|sig| hex::encode(sig.hint.0))
            .collect(),
    }
}

fn inner_transaction(fee_bump: &FeeBumpTransaction) -> &Transaction {
    match &fee_bump.inner_tx {
        FeeBumpTransactionInnerTx::Tx(v1) => &v1.tx,
    }
}

/// Operation type name, spelled the way Horizon and Stellar Lab spell it.
pub fn operation_kind(body: &OperationBody) -> &'static str {
    match body {
        OperationBody::CreateAccount(_) => "create_account",
        OperationBody::Payment(_) => "payment",
        OperationBody::PathPaymentStrictReceive(_) => "path_payment_strict_receive",
        OperationBody::ManageSellOffer(_) => "manage_sell_offer",
        OperationBody::CreatePassiveSellOffer(_) => "create_passive_sell_offer",
        OperationBody::SetOptions(_) => "set_options",
        OperationBody::ChangeTrust(_) => "change_trust",
        OperationBody::AllowTrust(_) => "allow_trust",
        OperationBody::AccountMerge(_) => "account_merge",
        OperationBody::Inflation => "inflation",
        OperationBody::ManageData(_) => "manage_data",
        OperationBody::BumpSequence(_) => "bump_sequence",
        OperationBody::ManageBuyOffer(_) => "manage_buy_offer",
        OperationBody::PathPaymentStrictSend(_) => "path_payment_strict_send",
        OperationBody::CreateClaimableBalance(_) => "create_claimable_balance",
        OperationBody::ClaimClaimableBalance(_) => "claim_claimable_balance",
        OperationBody::BeginSponsoringFutureReserves(_) => "begin_sponsoring_future_reserves",
        OperationBody::EndSponsoringFutureReserves => "end_sponsoring_future_reserves",
        OperationBody::RevokeSponsorship(_) => "revoke_sponsorship",
        OperationBody::Clawback(_) => "clawback",
        OperationBody::ClawbackClaimableBalance(_) => "clawback_claimable_balance",
        OperationBody::SetTrustLineFlags(_) => "set_trust_line_flags",
        OperationBody::LiquidityPoolDeposit(_) => "liquidity_pool_deposit",
        OperationBody::LiquidityPoolWithdraw(_) => "liquidity_pool_withdraw",
        OperationBody::InvokeHostFunction(_) => "invoke_host_function",
        OperationBody::ExtendFootprintTtl(_) => "extend_footprint_ttl",
        OperationBody::RestoreFootprint(_) => "restore_footprint",
    }
}

/// Builds an unsigned v1 envelope around `operations`.
///
/// A convenience for scripting and tests; the result is structurally valid but
/// has no signatures and must be signed before submission.
pub fn unsigned_envelope(
    source_account: &MuxedAccount,
    seq_num: i64,
    operations: Vec<Operation>,
) -> Result<TransactionEnvelope> {
    let tx = Transaction {
        source_account: source_account.clone(),
        fee: 100,
        seq_num: SequenceNumber(seq_num),
        cond: Preconditions::None,
        memo: Memo::None,
        operations: vecm(operations, "too many operations")?,
        ext: TransactionExt::V0,
    };
    Ok(TransactionEnvelope::Tx(TransactionV1Envelope {
        tx,
        signatures: VecM::default(),
    }))
}
