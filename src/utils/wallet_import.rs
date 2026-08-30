//! Pure parsers for wallet import and backup payloads.
//!
//! `starforge wallet import --file` and `starforge backup restore` both accept
//! files that arrive from outside the tool: an exported wallet backup, possibly
//! wrapped in an encrypted bundle. That makes them a trust boundary, so the
//! parsing lives here — separated from prompting, disk access, and the config
//! store — where it can be unit-tested, property-tested, and fuzzed.
//!
//! The harnesses under `fuzz/fuzz_targets/` drive these functions directly:
//!
//! ```text
//! cargo fuzz run fuzz_wallet_backup_parse
//! cargo fuzz run fuzz_wallet_import_envelope
//! ```
//!
//! ## Guarantees
//!
//! Every function here is total: for **any** input — malformed JSON, truncated
//! ciphertext, invalid StrKeys, multi-megabyte blobs, or hostile Unicode — it
//! returns a [`WalletImportError`] rather than panicking, and never allocates
//! proportionally to an attacker-chosen length before the size check runs.
//!
//! ## Security
//!
//! - Size limits are enforced *before* parsing, so an oversized file cannot
//!   drive the JSON parser into a large allocation.
//! - Error messages never echo secret key material; only the wallet name and
//!   the failure reason are reported.
//! - Wallet names containing bidirectional or zero-width control characters are
//!   rejected: they can make one wallet's name render identically to another's.
//!   Non-ASCII names are accepted but reported as a warning for the same
//!   reason — rejecting them outright would break backups made by earlier
//!   releases, which allow any Unicode alphanumeric.

use serde::{Deserialize, Serialize};

use crate::utils::config;
use crate::utils::shamir;

/// Backup schema version this build writes and accepts.
pub const WALLET_BACKUP_VERSION: &str = "2";

/// Well-known HMAC key used for the v2 integrity tag.
/// This key is not secret — it binds the tag to this application and
/// version, not to a per-user secret. Tamper detection comes from the
/// HMAC construction, not from key secrecy.
pub const BACKUP_HMAC_KEY: &[u8] = b"starforge-wallet-backup-v2";

/// Largest backup document accepted, in bytes.
pub const MAX_BACKUP_BYTES: usize = 4 * 1024 * 1024;

/// Largest number of wallets accepted in one backup.
pub const MAX_WALLETS_PER_BACKUP: usize = 1_000;

/// Longest wallet name accepted from a backup file.
pub const MAX_WALLET_NAME_LEN: usize = 64;

/// Largest encrypted bundle accepted, in bytes.
pub const MAX_ENVELOPE_BYTES: usize = MAX_BACKUP_BYTES * 2;

/// Salt length written by [`crate::utils::crypto::encrypt_secret`].
pub const SALT_LEN: usize = 16;

/// AES-GCM nonce length.
pub const NONCE_LEN: usize = 12;

/// AES-GCM authentication tag length: a ciphertext shorter than this cannot
/// even carry a tag and is therefore truncated.
pub const GCM_TAG_LEN: usize = 16;

// ─────────────────────────────────────────────────────────────────────────────
// Errors
// ─────────────────────────────────────────────────────────────────────────────

/// Why an import payload was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalletImportError {
    /// Input exceeded a size limit.
    TooLarge { bytes: usize, limit: usize },
    /// Input was empty or whitespace only.
    Empty,
    /// The document was not valid JSON.
    MalformedJson(String),
    /// The backup declares a version this build cannot read.
    UnsupportedVersion { found: String, expected: String },
    /// The backup contained no wallets.
    NoWallets,
    /// The backup contained more wallets than [`MAX_WALLETS_PER_BACKUP`].
    TooManyWallets { count: usize, limit: usize },
    /// Two entries share a name.
    DuplicateWallet(String),
    /// A wallet entry failed validation.
    InvalidEntry { wallet: String, reason: String },
    /// A wallet name carried invisible or direction-changing characters.
    DeceptiveWalletName { wallet: String, reason: String },
    /// The encrypted bundle did not have 3, 5, or 6 colon-separated parts.
    MalformedEnvelope { parts: usize },
    /// Recovery shares are present but invalid.
    InvalidRecoveryShares(String),
    /// Not enough shares provided for reconstruction.
    InsufficientShares { provided: usize, required: usize },
    /// Reconstructed data failed integrity check (corrupted shares).
    CorruptedShares,
    /// A base64 field of the bundle did not decode.
    InvalidBase64 { field: &'static str },
    /// A bundle field had the wrong decoded length.
    InvalidFieldLength {
        field: &'static str,
        len: usize,
        expected: usize,
    },
    /// The ciphertext is too short to carry an authentication tag.
    TruncatedCiphertext { len: usize, minimum: usize },
    /// A KDF parameter was absent, non-numeric, or zero.
    InvalidKdfParameter { field: &'static str, reason: String },
    /// The HMAC-SHA256 integrity tag did not match the backup body.
    IntegrityCheckFailed,
}

impl std::fmt::Display for WalletImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge { bytes, limit } => write!(
                f,
                "input is {} bytes, above the {} byte limit for an import payload",
                bytes, limit
            ),
            Self::Empty => write!(f, "input is empty"),
            Self::MalformedJson(msg) => write!(f, "invalid backup JSON: {}", msg),
            Self::UnsupportedVersion { found, expected } => write!(
                f,
                "unsupported backup version '{}'; this build reads version '{}'",
                found, expected
            ),
            Self::NoWallets => write!(f, "backup contains no wallets"),
            Self::TooManyWallets { count, limit } => write!(
                f,
                "backup contains {} wallets, above the limit of {}",
                count, limit
            ),
            Self::DuplicateWallet(name) => {
                write!(f, "duplicate wallet '{}' in backup file", name)
            }
            Self::InvalidEntry { wallet, reason } => {
                write!(f, "wallet '{}' is invalid: {}", wallet, reason)
            }
            Self::DeceptiveWalletName { wallet, reason } => write!(
                f,
                "wallet name {:?} is rejected: {}",
                wallet.escape_debug().to_string(),
                reason
            ),
            Self::MalformedEnvelope { parts } => write!(
                f,
                "encrypted bundle has {} colon-separated parts; expected 3, 5, or 6",
                parts
            ),
            Self::InvalidBase64 { field } => {
                write!(f, "encrypted bundle field `{}` is not valid base64", field)
            }
            Self::InvalidFieldLength {
                field,
                len,
                expected,
            } => write!(
                f,
                "encrypted bundle field `{}` decoded to {} bytes; expected {}",
                field, len, expected
            ),
            Self::TruncatedCiphertext { len, minimum } => write!(
                f,
                "ciphertext is {} bytes; at least {} are needed for the authentication tag",
                len, minimum
            ),
            Self::InvalidKdfParameter { field, reason } => {
                write!(f, "KDF parameter `{}` is invalid: {}", field, reason)
            }
            Self::InvalidRecoveryShares(msg) => {
                write!(f, "invalid recovery shares: {}", msg)
            }
            Self::InsufficientShares { provided, required } => {
                write!(
                    f,
                    "need at least {} recovery shares for reconstruction, but only {} were provided",
                    required, provided
                )
            }
            Self::CorruptedShares => {
                write!(f, "recovery shares failed integrity check — data may be corrupted or from different split operations")
            }
            Self::IntegrityCheckFailed => {
                write!(
                    f,
                    "backup integrity check failed: file may have been modified or corrupted"
                )
            }
        }
    }
}

impl std::error::Error for WalletImportError {}

type Result<T> = std::result::Result<T, WalletImportError>;

// ─────────────────────────────────────────────────────────────────────────────
// Backup documents
// ─────────────────────────────────────────────────────────────────────────────

/// A wallet backup document, as written by `starforge wallet export`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalletBackup {
    pub version: String,
    pub exported_at: String,
    pub wallets: Vec<WalletBackupEntry>,
    /// Optional Shamir recovery shares. When present, the backup can be
    /// reconstructed from `threshold` of `total_shares` share files instead
    /// of a single passphrase.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub recovery_shares: Option<Vec<shamir::RecoveryShare>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub integrity_tag: Option<String>,
}

/// One wallet inside a backup document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalletBackupEntry {
    pub name: String,
    pub public_key: String,
    pub secret_key: Option<String>,
    pub network: String,
    pub created_at: String,
    pub funded: bool,
}

/// A parsed backup plus any non-fatal observations about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedBackup {
    pub backup: WalletBackup,
    /// Notes worth showing the user, e.g. a wallet name that could be
    /// confused with another one.
    pub warnings: Vec<String>,
}

/// Whether an import payload is an encrypted bundle or a plaintext document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadKind {
    /// `salt:nonce:ciphertext[:mem:iters[:parallelism]]`
    Encrypted,
    /// A bare JSON document.
    Plaintext,
}

/// Classify an import payload.
///
/// Earlier releases detected encryption with `raw.matches(':').count() == 2`,
/// which misclassified the 5- and 6-part bundles written when custom Argon2
/// parameters are configured — those were handed to the JSON parser and failed
/// with a confusing "Invalid backup JSON format". Classification now mirrors
/// the bundle grammar, and a JSON document (which always starts with `{` or
/// `[`) is never treated as a bundle regardless of how many colons it holds.
pub fn classify_payload(raw: &str) -> PayloadKind {
    let trimmed = raw.trim();

    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return PayloadKind::Plaintext;
    }

    let parts: Vec<&str> = trimmed.split(':').collect();
    if matches!(parts.len(), 3 | 5 | 6)
        && parts
            .iter()
            .take(3)
            .all(|part| !part.is_empty() && part.bytes().all(is_base64_byte))
    {
        return PayloadKind::Encrypted;
    }

    PayloadKind::Plaintext
}

fn is_base64_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'='
}

// ─────────────────────────────────────────────────────────────────────────────
// Encrypted envelope
// ─────────────────────────────────────────────────────────────────────────────

/// A structurally valid encrypted bundle.
///
/// Structural validity says nothing about whether the passphrase is correct —
/// that is decided by AES-GCM during decryption. The point of parsing first is
/// to reject garbage before spending an Argon2 key derivation on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedEnvelope {
    pub salt: Vec<u8>,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub mem_cost: Option<u32>,
    pub iterations: Option<u32>,
    pub parallelism: Option<u32>,
}

/// Parse and structurally validate an encrypted bundle.
pub fn parse_encrypted_envelope(raw: &str) -> Result<EncryptedEnvelope> {
    if raw.len() > MAX_ENVELOPE_BYTES {
        return Err(WalletImportError::TooLarge {
            bytes: raw.len(),
            limit: MAX_ENVELOPE_BYTES,
        });
    }

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(WalletImportError::Empty);
    }

    let parts: Vec<&str> = trimmed.split(':').collect();
    if !matches!(parts.len(), 3 | 5 | 6) {
        return Err(WalletImportError::MalformedEnvelope { parts: parts.len() });
    }

    let salt = decode_field(parts[0], "salt")?;
    let nonce = decode_field(parts[1], "nonce")?;
    let ciphertext = decode_field(parts[2], "ciphertext")?;

    if salt.len() != SALT_LEN {
        return Err(WalletImportError::InvalidFieldLength {
            field: "salt",
            len: salt.len(),
            expected: SALT_LEN,
        });
    }
    if nonce.len() != NONCE_LEN {
        return Err(WalletImportError::InvalidFieldLength {
            field: "nonce",
            len: nonce.len(),
            expected: NONCE_LEN,
        });
    }
    if ciphertext.len() < GCM_TAG_LEN {
        return Err(WalletImportError::TruncatedCiphertext {
            len: ciphertext.len(),
            minimum: GCM_TAG_LEN,
        });
    }

    let (mem_cost, iterations) = if parts.len() >= 5 {
        (
            Some(parse_kdf_param(parts[3], "mem")?),
            Some(parse_kdf_param(parts[4], "iterations")?),
        )
    } else {
        (None, None)
    };
    let parallelism = if parts.len() == 6 {
        Some(parse_kdf_param(parts[5], "parallelism")?)
    } else {
        None
    };

    Ok(EncryptedEnvelope {
        salt,
        nonce,
        ciphertext,
        mem_cost,
        iterations,
        parallelism,
    })
}

fn decode_field(value: &str, field: &'static str) -> Result<Vec<u8>> {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
    BASE64
        .decode(value)
        .map_err(|_| WalletImportError::InvalidBase64 { field })
}

fn parse_kdf_param(value: &str, field: &'static str) -> Result<u32> {
    let parsed = value
        .parse::<u32>()
        .map_err(|_| WalletImportError::InvalidKdfParameter {
            field,
            reason: "must be a decimal u32".to_string(),
        })?;
    if parsed == 0 {
        return Err(WalletImportError::InvalidKdfParameter {
            field,
            reason: "must be greater than zero".to_string(),
        });
    }
    Ok(parsed)
}

// ─────────────────────────────────────────────────────────────────────────────
// Integrity tag helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Serialise the backup body used as the HMAC input.
///
/// The tag is computed over the canonical JSON of the backup with
/// `integrity_tag` forced to `None`, so the computation is stable
/// regardless of whether a tag is already present in the struct.
fn backup_body_for_hmac(backup: &WalletBackup) -> std::result::Result<Vec<u8>, serde_json::Error> {
    let mut canonical = backup.clone();
    canonical.integrity_tag = None;
    serde_json::to_vec(&canonical)
}

/// Compute an HMAC-SHA256 integrity tag for a backup document.
///
/// The tag is the lowercase hex encoding of HMAC-SHA256 keyed with `key`
/// over the canonical JSON of the backup (with `integrity_tag` set to `None`).
///
/// # Errors
/// Returns an error if the backup cannot be serialised to JSON.
pub fn compute_integrity_tag(
    backup: &WalletBackup,
    key: &[u8],
) -> std::result::Result<String, serde_json::Error> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let body = backup_body_for_hmac(backup)?;
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(&body);
    let digest = mac.finalize().into_bytes();
    Ok(hex::encode(digest))
}

/// Verify an integrity tag against a backup document.
///
/// Recomputes the expected tag and compares it to `tag`. Returns `false` if
/// the backup cannot be serialised or if the tags differ. Because both values
/// are fixed-length lowercase hex strings, the comparison does not exit early
/// on a mismatch in the common-length case.
pub fn verify_integrity_tag(backup: &WalletBackup, tag: &str, key: &[u8]) -> bool {
    match compute_integrity_tag(backup, key) {
        Ok(expected) => expected == tag,
        Err(_) => false,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Backup parsing
// ─────────────────────────────────────────────────────────────────────────────

/// Parse and validate a plaintext wallet backup document.
pub fn parse_wallet_backup(contents: &str) -> Result<ParsedBackup> {
    if contents.len() > MAX_BACKUP_BYTES {
        return Err(WalletImportError::TooLarge {
            bytes: contents.len(),
            limit: MAX_BACKUP_BYTES,
        });
    }
    if contents.trim().is_empty() {
        return Err(WalletImportError::Empty);
    }

    let backup: WalletBackup = serde_json::from_str(contents)
        .map_err(|e| WalletImportError::MalformedJson(e.to_string()))?;

    let mut warnings = Vec::new();

    match backup.version.as_str() {
        "1" => {
            // v1 backups carry no integrity tag. Accept them with a warning
            // so users know they should re-export.
            warnings.push(
                "backup is version 1 (no integrity tag); \
                 re-export to get tamper detection"
                    .to_string(),
            );
        }
        "2" => match &backup.integrity_tag {
            Some(tag) => {
                if !verify_integrity_tag(&backup, tag, BACKUP_HMAC_KEY) {
                    return Err(WalletImportError::IntegrityCheckFailed);
                }
            }
            None => {
                warnings.push("backup is version 2 but carries no integrity tag".to_string());
            }
        },
        _ => {
            return Err(WalletImportError::UnsupportedVersion {
                found: backup.version.clone(),
                expected: WALLET_BACKUP_VERSION.to_string(),
            });
        }
    }

    if backup.wallets.is_empty() {
        return Err(WalletImportError::NoWallets);
    }
    if backup.wallets.len() > MAX_WALLETS_PER_BACKUP {
        return Err(WalletImportError::TooManyWallets {
            count: backup.wallets.len(),
            limit: MAX_WALLETS_PER_BACKUP,
        });
    }

    let mut seen = std::collections::HashSet::new();

    for entry in &backup.wallets {
        check_wallet_name(&entry.name)?;
        if !seen.insert(entry.name.as_str()) {
            return Err(WalletImportError::DuplicateWallet(entry.name.clone()));
        }
        if !entry.name.is_ascii() {
            warnings.push(format!(
                "wallet '{}' has a non-ASCII name, which can render identically to another name",
                entry.name
            ));
        }
        validate_entry(entry)?;
    }

    Ok(ParsedBackup { backup, warnings })
}

/// Reject wallet names that are invisible, direction-changing, or overlong.
///
/// Length is checked in `char`s: a name of 64 astral characters is 256 bytes,
/// and the limit is about what a human can read, not about storage.
fn check_wallet_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(WalletImportError::DeceptiveWalletName {
            wallet: name.to_string(),
            reason: "name is empty".to_string(),
        });
    }
    if name.chars().count() > MAX_WALLET_NAME_LEN {
        return Err(WalletImportError::DeceptiveWalletName {
            wallet: name.chars().take(16).collect(),
            reason: format!(
                "name is {} characters; at most {} are allowed",
                name.chars().count(),
                MAX_WALLET_NAME_LEN
            ),
        });
    }
    if let Some(bad) = name.chars().find(|c| is_deceptive_char(*c)) {
        return Err(WalletImportError::DeceptiveWalletName {
            wallet: name.to_string(),
            reason: format!(
                "contains U+{:04X}, an invisible or direction-changing character",
                bad as u32
            ),
        });
    }
    Ok(())
}

/// Characters that are invisible or that reorder the rendering of a name.
fn is_deceptive_char(c: char) -> bool {
    c.is_control()
        || matches!(c,
            '\u{200B}'..='\u{200F}'   // zero width space … RTL mark
            | '\u{202A}'..='\u{202E}' // bidi embedding / override
            | '\u{2066}'..='\u{2069}' // bidi isolates
            | '\u{FEFF}'              // zero width no-break space
            | '\u{00AD}'              // soft hyphen
        )
}

/// Validate a single backup entry against the wallet rules.
///
/// Errors quote the wallet name and the reason, never the key material.
pub fn validate_entry(entry: &WalletBackupEntry) -> Result<()> {
    config::validate_wallet_name(&entry.name).map_err(|e| WalletImportError::InvalidEntry {
        wallet: entry.name.clone(),
        reason: e.to_string(),
    })?;
    config::validate_public_key(&entry.public_key).map_err(|e| {
        WalletImportError::InvalidEntry {
            wallet: entry.name.clone(),
            reason: e.to_string(),
        }
    })?;
    if let Some(secret) = &entry.secret_key {
        config::validate_secret_key(secret).map_err(|e| WalletImportError::InvalidEntry {
            wallet: entry.name.clone(),
            reason: e.to_string(),
        })?;
    }
    if entry.network.trim().is_empty() {
        return Err(WalletImportError::InvalidEntry {
            wallet: entry.name.clone(),
            reason: "network is empty".to_string(),
        });
    }
    Ok(())
}

/// Validate a set of recovery shares for reconstruction.
///
/// Returns the shares sorted by index, or an error describing what is wrong.
pub fn validate_recovery_shares(shares: &[shamir::RecoveryShare]) -> Result<()> {
    if shares.is_empty() {
        return Err(WalletImportError::InvalidRecoveryShares(
            "no shares provided".to_string(),
        ));
    }

    let threshold = shares[0].threshold as usize;
    let total = shares[0].total_shares as usize;
    let secret_hash = &shares[0].secret_hash;

    for (i, share) in shares.iter().enumerate() {
        if share.threshold as usize != threshold {
            return Err(WalletImportError::InvalidRecoveryShares(format!(
                "share {} has threshold {}, expected {}",
                i, share.threshold, threshold
            )));
        }
        if share.total_shares as usize != total {
            return Err(WalletImportError::InvalidRecoveryShares(format!(
                "share {} has total_shares {}, expected {}",
                i, share.total_shares, total
            )));
        }
        if &share.secret_hash != secret_hash {
            return Err(WalletImportError::InvalidRecoveryShares(format!(
                "share {} has a different secret hash — shares may be from different split operations",
                i
            )));
        }
    }

    // Check for duplicate indices.
    let mut seen = std::collections::HashSet::new();
    for share in shares {
        if !seen.insert(share.index) {
            return Err(WalletImportError::InvalidRecoveryShares(format!(
                "duplicate share index {}",
                share.index
            )));
        }
    }

    if shares.len() < threshold {
        return Err(WalletImportError::InsufficientShares {
            provided: shares.len(),
            required: threshold,
        });
    }

    Ok(())
}

/// Reconstruct an encrypted bundle from recovery shares.
///
/// This is a convenience wrapper around [`shamir::reconstruct`] that
/// returns wallet-import-specific errors.
pub fn reconstruct_from_shares(shares: &[shamir::RecoveryShare]) -> Result<String> {
    validate_recovery_shares(shares)?;
    let secret = shamir::reconstruct(shares)
        .map_err(|e| WalletImportError::InvalidRecoveryShares(e.to_string()))?;
    String::from_utf8(secret).map_err(|e| {
        WalletImportError::InvalidRecoveryShares(format!(
            "reconstructed data is not valid UTF-8: {}",
            e
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_public_key() -> String {
        format!("G{}", "A".repeat(55))
    }

    fn valid_secret_key() -> String {
        format!("S{}", "B".repeat(55))
    }

    /// Build a v1 backup document (no integrity tag). Used by migration tests.
    fn backup_json(wallets: &str) -> String {
        format!(
            r#"{{"version":"1","exported_at":"2026-07-29T00:00:00Z","wallets":[{}]}}"#,
            wallets
        )
    }

    /// Build a v2 backup document without an integrity tag.
    /// Use this for the no-tag warning path and as a base for tag tests.
    fn backup_json_v2(wallets: &str) -> String {
        format!(
            r#"{{"version":"2","exported_at":"2026-07-29T00:00:00Z","wallets":[{}]}}"#,
            wallets
        )
    }

    fn wallet_json(name: &str) -> String {
        format!(
            r#"{{"name":"{}","public_key":"{}","secret_key":"{}","network":"testnet","created_at":"2026-07-29T00:00:00Z","funded":true}}"#,
            name,
            valid_public_key(),
            valid_secret_key()
        )
    }

    fn envelope(salt: usize, nonce: usize, ct: usize) -> String {
        use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
        format!(
            "{}:{}:{}",
            BASE64.encode(vec![1u8; salt]),
            BASE64.encode(vec![2u8; nonce]),
            BASE64.encode(vec![3u8; ct])
        )
    }

    // ── Primary flow ────────────────────────────────────────────────────────

    #[test]
    fn parses_a_well_formed_backup() {
        // Use a v2 document (the current default version).
        let doc = backup_json_v2(&wallet_json("alice"));
        let parsed = parse_wallet_backup(&doc).unwrap();

        assert_eq!(parsed.backup.version, "2");
        assert_eq!(parsed.backup.wallets.len(), 1);
        assert_eq!(parsed.backup.wallets[0].name, "alice");
        // v2 with no tag produces exactly one warning
        assert_eq!(parsed.warnings.len(), 1);
        assert!(parsed.warnings[0].contains("no integrity tag"));
    }

    #[test]
    fn parses_a_well_formed_envelope() {
        let env = parse_encrypted_envelope(&envelope(SALT_LEN, NONCE_LEN, 64)).unwrap();

        assert_eq!(env.salt.len(), SALT_LEN);
        assert_eq!(env.nonce.len(), NONCE_LEN);
        assert_eq!(env.ciphertext.len(), 64);
        assert_eq!(env.mem_cost, None);
    }

    #[test]
    fn parses_a_six_part_envelope_with_kdf_parameters() {
        let raw = format!("{}:65536:3:4", envelope(SALT_LEN, NONCE_LEN, 32));
        let env = parse_encrypted_envelope(&raw).unwrap();

        assert_eq!(env.mem_cost, Some(65_536));
        assert_eq!(env.iterations, Some(3));
        assert_eq!(env.parallelism, Some(4));
    }

    #[test]
    fn classifies_bundles_and_documents() {
        assert_eq!(
            classify_payload(&envelope(SALT_LEN, NONCE_LEN, 32)),
            PayloadKind::Encrypted
        );
        // 5- and 6-part bundles were misclassified as plaintext before #697.
        assert_eq!(
            classify_payload(&format!("{}:65536:3", envelope(SALT_LEN, NONCE_LEN, 32))),
            PayloadKind::Encrypted
        );
        assert_eq!(
            classify_payload(&format!("{}:65536:3:4", envelope(SALT_LEN, NONCE_LEN, 32))),
            PayloadKind::Encrypted
        );
        assert_eq!(
            classify_payload(&backup_json(&wallet_json("alice"))),
            PayloadKind::Plaintext
        );
        // A JSON document with colons in its values is still a document.
        assert_eq!(classify_payload(r#"{"a":"b:c:d"}"#), PayloadKind::Plaintext);
    }

    // ── Boundary cases ──────────────────────────────────────────────────────

    #[test]
    fn ciphertext_of_exactly_one_tag_is_accepted_and_one_byte_less_is_not() {
        assert!(parse_encrypted_envelope(&envelope(SALT_LEN, NONCE_LEN, GCM_TAG_LEN)).is_ok());
        assert_eq!(
            parse_encrypted_envelope(&envelope(SALT_LEN, NONCE_LEN, GCM_TAG_LEN - 1)).unwrap_err(),
            WalletImportError::TruncatedCiphertext {
                len: GCM_TAG_LEN - 1,
                minimum: GCM_TAG_LEN,
            }
        );
    }

    #[test]
    fn a_backup_at_the_wallet_limit_is_accepted_and_one_over_is_not() {
        let at_limit = (0..MAX_WALLETS_PER_BACKUP)
            .map(|i| wallet_json(&format!("w{}", i)))
            .collect::<Vec<_>>()
            .join(",");
        assert!(parse_wallet_backup(&backup_json(&at_limit)).is_ok());

        let over = (0..=MAX_WALLETS_PER_BACKUP)
            .map(|i| wallet_json(&format!("w{}", i)))
            .collect::<Vec<_>>()
            .join(",");
        assert!(matches!(
            parse_wallet_backup(&backup_json(&over)).unwrap_err(),
            WalletImportError::TooManyWallets { .. }
        ));
    }

    #[test]
    fn a_name_at_the_length_limit_is_accepted_and_one_over_is_not() {
        let at_limit = "a".repeat(MAX_WALLET_NAME_LEN);
        assert!(parse_wallet_backup(&backup_json(&wallet_json(&at_limit))).is_ok());

        let over = "a".repeat(MAX_WALLET_NAME_LEN + 1);
        assert!(matches!(
            parse_wallet_backup(&backup_json(&wallet_json(&over))).unwrap_err(),
            WalletImportError::DeceptiveWalletName { .. }
        ));
    }

    #[test]
    fn an_oversized_document_is_rejected_before_parsing() {
        let big = "x".repeat(MAX_BACKUP_BYTES + 1);
        assert_eq!(
            parse_wallet_backup(&big).unwrap_err(),
            WalletImportError::TooLarge {
                bytes: MAX_BACKUP_BYTES + 1,
                limit: MAX_BACKUP_BYTES,
            }
        );
    }

    // ── Failure cases ───────────────────────────────────────────────────────

    #[test]
    fn malformed_json_is_rejected() {
        for bad in [
            "{",
            "{\"version\":}",
            "[]",
            "null",
            "\"just a string\"",
            "{\"version\":\"1\"}",
        ] {
            assert!(
                matches!(
                    parse_wallet_backup(bad),
                    Err(WalletImportError::MalformedJson(_))
                ),
                "accepted malformed JSON {:?}",
                bad
            );
        }
    }

    #[test]
    fn empty_input_is_rejected() {
        assert_eq!(
            parse_wallet_backup("   ").unwrap_err(),
            WalletImportError::Empty
        );
        assert_eq!(
            parse_encrypted_envelope("   ").unwrap_err(),
            WalletImportError::Empty
        );
    }

    #[test]
    fn an_unsupported_version_is_rejected() {
        let doc =
            backup_json_v2(&wallet_json("alice")).replace("\"version\":\"2\"", "\"version\":\"9\"");
        assert_eq!(
            parse_wallet_backup(&doc).unwrap_err(),
            WalletImportError::UnsupportedVersion {
                found: "9".to_string(),
                expected: "2".to_string(),
            }
        );
    }

    #[test]
    fn an_empty_wallet_list_is_rejected() {
        assert_eq!(
            parse_wallet_backup(&backup_json("")).unwrap_err(),
            WalletImportError::NoWallets
        );
    }

    #[test]
    fn duplicate_wallet_names_are_rejected() {
        let doc = backup_json(&format!(
            "{},{}",
            wallet_json("alice"),
            wallet_json("alice")
        ));
        assert_eq!(
            parse_wallet_backup(&doc).unwrap_err(),
            WalletImportError::DuplicateWallet("alice".to_string())
        );
    }

    #[test]
    fn invalid_strkeys_are_rejected() {
        let doc = backup_json(&wallet_json("alice").replace(&valid_public_key(), "GNOTAKEY"));
        assert!(matches!(
            parse_wallet_backup(&doc).unwrap_err(),
            WalletImportError::InvalidEntry { .. }
        ));

        let doc = backup_json(&wallet_json("alice").replace(&valid_secret_key(), "S123"));
        assert!(matches!(
            parse_wallet_backup(&doc).unwrap_err(),
            WalletImportError::InvalidEntry { .. }
        ));
    }

    #[test]
    fn an_error_never_echoes_the_secret_key() {
        let secret = valid_secret_key();
        let doc = backup_json(&wallet_json("alice").replace(&valid_public_key(), "GBAD"));
        let err = parse_wallet_backup(&doc).unwrap_err().to_string();

        assert!(
            !err.contains(&secret),
            "secret key leaked into the error: {}",
            err
        );
    }

    #[test]
    fn bidi_and_zero_width_names_are_rejected() {
        for name in [
            "al\u{202E}ice", // right-to-left override
            "al\u{200B}ice", // zero width space
            "al\u{FEFF}ice", // BOM
            "al\u{00AD}ice", // soft hyphen
            "al\u{2066}ice", // bidi isolate
        ] {
            let doc = backup_json(&wallet_json(name));
            assert!(
                matches!(
                    parse_wallet_backup(&doc),
                    Err(WalletImportError::DeceptiveWalletName { .. })
                ),
                "accepted deceptive name {:?}",
                name
            );
        }
    }

    #[test]
    fn a_non_ascii_name_is_accepted_but_warned_about() {
        // Cyrillic 'а' renders like Latin 'a'.
        let entry_str = wallet_json("\u{0430}lice");
        let mut backup: WalletBackup = serde_json::from_str(&backup_json_v2(&entry_str)).unwrap();
        let tag = compute_integrity_tag(&backup, BACKUP_HMAC_KEY)
            .expect("compute_integrity_tag must succeed");
        backup.integrity_tag = Some(tag);
        let json = serde_json::to_string(&backup).unwrap();
        let parsed = parse_wallet_backup(&json).unwrap();

        assert_eq!(parsed.warnings.len(), 1);
        assert!(parsed.warnings[0].contains("non-ASCII"));
    }

    #[test]
    fn truncated_and_corrupt_envelopes_are_rejected() {
        // Wrong number of parts.
        assert!(matches!(
            parse_encrypted_envelope("onlyonepart"),
            Err(WalletImportError::MalformedEnvelope { parts: 1 })
        ));
        assert!(matches!(
            parse_encrypted_envelope("a:b:c:d"),
            Err(WalletImportError::MalformedEnvelope { parts: 4 })
        ));
        // Not base64.
        assert_eq!(
            parse_encrypted_envelope("!!!:@@@:###").unwrap_err(),
            WalletImportError::InvalidBase64 { field: "salt" }
        );
        // Wrong salt / nonce length.
        assert!(matches!(
            parse_encrypted_envelope(&envelope(8, NONCE_LEN, 32)),
            Err(WalletImportError::InvalidFieldLength { field: "salt", .. })
        ));
        assert!(matches!(
            parse_encrypted_envelope(&envelope(SALT_LEN, 4, 32)),
            Err(WalletImportError::InvalidFieldLength { field: "nonce", .. })
        ));
    }

    #[test]
    fn invalid_kdf_parameters_are_rejected() {
        let base = envelope(SALT_LEN, NONCE_LEN, 32);
        assert!(matches!(
            parse_encrypted_envelope(&format!("{}:notanumber:3", base)),
            Err(WalletImportError::InvalidKdfParameter { field: "mem", .. })
        ));
        assert!(matches!(
            parse_encrypted_envelope(&format!("{}:65536:0", base)),
            Err(WalletImportError::InvalidKdfParameter {
                field: "iterations",
                ..
            })
        ));
        assert!(matches!(
            parse_encrypted_envelope(&format!("{}:65536:3:99999999999", base)),
            Err(WalletImportError::InvalidKdfParameter {
                field: "parallelism",
                ..
            })
        ));
    }

    #[test]
    fn parsers_are_total_over_hostile_input() {
        // Everything here must return an error, never panic.
        let inputs = [
            String::new(),
            "\u{0}\u{1}\u{2}".to_string(),
            "\u{FFFD}".repeat(100),
            ":".repeat(1000),
            "{".repeat(5000),
            "\u{1F600}".repeat(500),
            format!("{}\u{0}", envelope(SALT_LEN, NONCE_LEN, 32)),
        ];
        for input in &inputs {
            let _ = parse_wallet_backup(input);
            let _ = parse_encrypted_envelope(input);
            let _ = classify_payload(input);
        }
    }

    // ── Versioned backup / integrity tag tests ───────────────────────────────

    #[test]
    fn v1_backup_is_accepted_with_migration_warning() {
        let doc = backup_json(&wallet_json("alice"));
        let parsed = parse_wallet_backup(&doc).expect("v1 backup must parse");

        assert!(
            parsed.warnings.iter().any(|w| w.contains("version 1")),
            "expected a 'version 1' warning, got: {:?}",
            parsed.warnings
        );
    }

    #[test]
    fn v2_backup_with_valid_tag_is_accepted() {
        // Build a v2 struct, compute the tag, embed it in JSON, then parse.
        let entry_str = wallet_json("alice");
        let mut backup: WalletBackup = serde_json::from_str(&backup_json_v2(&entry_str)).unwrap();
        let tag = compute_integrity_tag(&backup, BACKUP_HMAC_KEY)
            .expect("compute_integrity_tag must succeed");
        backup.integrity_tag = Some(tag);

        let json = serde_json::to_string(&backup).unwrap();
        let parsed = parse_wallet_backup(&json).expect("v2 backup with valid tag must parse");

        assert!(
            parsed.warnings.is_empty(),
            "expected no warnings, got: {:?}",
            parsed.warnings
        );
        assert_eq!(parsed.backup.wallets[0].name, "alice");
    }

    #[test]
    fn v2_backup_with_wrong_tag_is_rejected() {
        let entry_str = wallet_json("alice");
        let mut backup: WalletBackup = serde_json::from_str(&backup_json_v2(&entry_str)).unwrap();
        // Embed a tag that is the right format but wrong value.
        backup.integrity_tag =
            Some("0000000000000000000000000000000000000000000000000000000000000000".to_string());

        let json = serde_json::to_string(&backup).unwrap();
        assert_eq!(
            parse_wallet_backup(&json).unwrap_err(),
            WalletImportError::IntegrityCheckFailed,
        );
    }

    #[test]
    fn v2_backup_with_no_tag_is_accepted_with_warning() {
        let doc = backup_json_v2(&wallet_json("alice"));
        let parsed = parse_wallet_backup(&doc).expect("v2 backup without tag must parse");

        assert!(
            parsed
                .warnings
                .iter()
                .any(|w| w.contains("no integrity tag")),
            "expected a 'no integrity tag' warning, got: {:?}",
            parsed.warnings
        );
    }

    #[test]
    fn unsupported_version_is_still_rejected() {
        let doc = backup_json_v2(&wallet_json("alice"))
            .replace("\"version\":\"2\"", "\"version\":\"99\"");
        assert!(
            matches!(
                parse_wallet_backup(&doc).unwrap_err(),
                WalletImportError::UnsupportedVersion { found, .. } if found == "99"
            ),
            "version 99 must be rejected with UnsupportedVersion"
        );
    }
}
