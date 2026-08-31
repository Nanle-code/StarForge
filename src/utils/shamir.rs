//! Shamir's Secret Sharing over GF(256) for encrypted wallet backups.
//!
//! This module provides an **opt-in** mechanism to split an encrypted wallet
//! backup into `N` recovery shares such that any `M` of them can reconstruct
//! the original data, but `M-1` shares reveal no information about it.
//!
//! # Threat model
//!
//! - An adversary who obtains fewer than `M` shares learns **nothing** about
//!   the encrypted backup (information-theoretic security of Shamir's scheme).
//! - An adversary who obtains `M` or more shares can reconstruct the backup.
//!   Share custody is therefore the critical trust boundary.
//! - This mechanism protects against **loss** of a single passphrase or share,
//!   not against a compromise of `M` custodians.
//!
//! # Design
//!
//! The input to the scheme is an encrypted backup bundle (the
//! `salt:nonce:ciphertext[...` string produced by [`crypto::encrypt_secret`]).
//! Each share is an independent JSON object carrying a share index, a
//! GF(256) polynomial evaluation, and integrity metadata. Shares are
//! written to separate files.
//!
//! The scheme uses a random polynomial of degree `M-1` over GF(256) where
//! the secret (the encrypted bundle bytes) is the constant term. Each share
//! is a point `(x, y)` on that polynomial.

use anyhow::{anyhow, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};

// ── GF(256) arithmetic ───────────────────────────────────────────────────────

/// Irreducible polynomial for GF(2^8): x^8 + x^4 + x^3 + x^2 + 1 = 0x11D.
const IRREDUCIBLE: u16 = 0x11D;

/// Lookup table for GF(256) logarithms (base 2 = primitive element).
/// Index 0 is unused (log(0) is undefined); we store log[i+1] = log of the
/// i-th power of the primitive element 2.
static LOG_TABLE: [u8; 256] = gen_log_table();

/// Inverse of `LOG_TABLE`: `EXP_TABLE[log(x)] = x` for x in GF(256)\{0}.
static EXP_TABLE: [u8; 256] = gen_exp_table();

const fn gen_log_table() -> [u8; 256] {
    // We need a mutable array; const fn can't use loops in older editions
    // but Rust 1.81+ (which this project uses per Cargo.toml) allows it.
    let mut log = [0u8; 256];
    let mut val: u16 = 1;
    let mut idx: u8 = 0;
    while idx < 255 {
        log[val as usize] = idx;
        val <<= 1;
        if val & 256 != 0 {
            val ^= IRREDUCIBLE;
        }
        idx += 1;
    }
    log
}

const fn gen_exp_table() -> [u8; 256] {
    let mut exp = [0u8; 256];
    let mut val: u16 = 1;
    let mut idx: u8 = 0;
    while idx < 255 {
        exp[idx as usize] = val as u8;
        val <<= 1;
        if val & 256 != 0 {
            val ^= IRREDUCIBLE;
        }
        idx += 1;
    }
    exp
}

/// Multiply two elements in GF(256).
fn gf_mul(a: u8, b: u8) -> u8 {
    if a == 0 || b == 0 {
        return 0;
    }
    let log_a = LOG_TABLE[a as usize] as u16;
    let log_b = LOG_TABLE[b as usize] as u16;
    let sum = (log_a + log_b) % 255;
    EXP_TABLE[sum as usize]
}

/// Divide two elements in GF(256). Panics if `b == 0`.
fn gf_div(a: u8, b: u8) -> u8 {
    assert!(b != 0, "division by zero in GF(256)");
    if a == 0 {
        return 0;
    }
    let log_a = LOG_TABLE[a as usize] as i16;
    let log_b = LOG_TABLE[b as usize] as i16;
    let diff = (log_a - log_b).rem_euclid(255);
    EXP_TABLE[diff as usize]
}

// ── Polynomial evaluation ────────────────────────────────────────────────────

/// Evaluate a polynomial (coefficients in ascending order) at point `x` in
/// GF(256) using Horner's method.
fn poly_eval(coeffs: &[u8], x: u8) -> u8 {
    // coeffs[0] + coeffs[1]*x + coeffs[2]*x^2 + ... + coeffs[n]*x^n
    let mut result = 0u8;
    for &c in coeffs.iter().rev() {
        result = gf_mul(result, x) ^ c;
    }
    result
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Maximum number of shares supported.
pub const MAX_SHARES: usize = 255;

/// Maximum threshold (degree + 1) supported.
pub const MAX_THRESHOLD: usize = 255;

/// A single recovery share.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryShare {
    /// 1-based share index (1..=N). Index 0 is not a valid share.
    pub index: u8,
    /// The share payload: evaluation of the polynomial at `index`, encoded as
    /// a hex string (one byte per character pair).
    pub payload: String,
    /// SHA-256 hash of the reconstructed secret (hex), so the importer can
    /// verify reconstruction integrity without the passphrase.
    pub secret_hash: String,
    /// Total number of shares created (N in M-of-N).
    pub total_shares: u8,
    /// Threshold required for reconstruction (M in M-of-N).
    pub threshold: u8,
}

/// Split `secret` bytes into `threshold`-of-`total_shares` recovery shares.
///
/// Returns exactly `total_shares` shares. Any `threshold` of them can
/// reconstruct the original `secret` via [`reconstruct`].
pub fn split(secret: &[u8], threshold: usize, total_shares: usize) -> Result<Vec<RecoveryShare>> {
    if threshold < 2 {
        return Err(anyhow!(
            "threshold must be at least 2 (got {}); use a single passphrase for threshold=1",
            threshold
        ));
    }
    if threshold > total_shares {
        return Err(anyhow!(
            "threshold ({}) cannot exceed total shares ({})",
            threshold,
            total_shares
        ));
    }
    if total_shares > MAX_SHARES {
        return Err(anyhow!(
            "total shares ({}) exceeds maximum ({})",
            total_shares,
            MAX_SHARES
        ));
    }

    // Secret hash for integrity verification after reconstruction.
    let secret_hash = {
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(secret);
        hex::encode(hash)
    };

    // Build a random polynomial of degree (threshold - 1) where the constant
    // term is secret[0] XORed with a random mask for each coefficient.
    // Actually, the standard Shamir scheme uses the secret bytes as the
    // polynomial coefficients directly, but we handle multi-byte secrets by
    // treating the secret as a sequence of bytes and applying the same
    // polynomial to each byte.
    let degree = threshold - 1;

    // Generate random coefficients for the polynomial (degree terms).
    // coeffs[0] is the secret (constant term), coeffs[1..=degree] are random.
    let mut rng = rand::thread_rng();

    // For each byte position in the secret, we need a polynomial.
    // To keep shares compact, we interleave: for share x, the share payload
    // is the concatenation of poly_i(x) for each byte position i.
    let secret_len = secret.len();

    // Generate coefficients for all polynomials: one polynomial per secret byte.
    // coeffs[0] is the secret (constant term), coeffs[1..=degree] are random.
    let mut all_coeffs = Vec::with_capacity(secret_len);
    for &secret_byte in secret {
        let mut coeffs = vec![0u8; degree + 1];
        coeffs[0] = secret_byte;
        for c in coeffs[1..].iter_mut() {
            *c = rng.next_u32() as u8;
        }
        all_coeffs.push(coeffs);
    }

    let mut shares: Vec<RecoveryShare> = Vec::with_capacity(total_shares);

    for share_idx in 1..=total_shares {
        let x = share_idx as u8;
        let mut payload_bytes = Vec::with_capacity(secret_len);

        for coeffs in &all_coeffs {
            payload_bytes.push(poly_eval(coeffs, x));
        }

        shares.push(RecoveryShare {
            index: share_idx as u8,
            payload: hex::encode(&payload_bytes),
            secret_hash: secret_hash.clone(),
            total_shares: total_shares as u8,
            threshold: threshold as u8,
        });
    }

    Ok(shares)
}

/// Reconstruct the original secret from a set of shares.
///
/// All provided shares must have the same `threshold` and `total_shares`.
/// At least `threshold` shares are required; fewer will produce an error.
/// If any share is corrupted (wrong index, wrong threshold, or tampered
/// payload), the reconstructed data will not match the `secret_hash` and
/// an error is returned.
pub fn reconstruct(shares: &[RecoveryShare]) -> Result<Vec<u8>> {
    if shares.is_empty() {
        return Err(anyhow!("no shares provided for reconstruction"));
    }

    let threshold = shares[0].threshold as usize;
    let total_shares = shares[0].total_shares as usize;
    let secret_hash = shares[0].secret_hash.clone();

    // Validate consistency.
    for (i, share) in shares.iter().enumerate() {
        if share.threshold as usize != threshold {
            return Err(anyhow!(
                "share {} has threshold {}, expected {}",
                i,
                share.threshold,
                threshold
            ));
        }
        if share.total_shares as usize != total_shares {
            return Err(anyhow!(
                "share {} has total_shares {}, expected {}",
                i,
                share.total_shares,
                total_shares
            ));
        }
    }

    if shares.len() < threshold {
        return Err(anyhow!(
            "need at least {} shares for reconstruction, but only {} were provided",
            threshold,
            shares.len()
        ));
    }

    // Check for duplicate share indices.
    let mut seen_indices = std::collections::HashSet::new();
    for share in shares {
        if !seen_indices.insert(share.index) {
            return Err(anyhow!(
                "duplicate share index {}; each share must have a unique index",
                share.index
            ));
        }
    }

    // Decode payloads.
    let decoded: Vec<(u8, Vec<u8>)> = shares
        .iter()
        .map(|s| {
            let bytes = hex::decode(&s.payload)
                .map_err(|e| anyhow!("share {}: invalid hex payload: {}", s.index, e))?;
            Ok((s.index, bytes))
        })
        .collect::<Result<Vec<_>>>()?;

    // Verify all payloads have the same length.
    let payload_len = decoded[0].1.len();
    for (idx, payload) in &decoded {
        if payload.len() != payload_len {
            return Err(anyhow!(
                "share {} has payload length {}, expected {}",
                idx,
                payload.len(),
                payload_len
            ));
        }
    }

    // Reconstruct each byte position using Lagrange interpolation.
    let mut secret = Vec::with_capacity(payload_len);

    for byte_idx in 0..payload_len {
        let mut result = 0u8;

        // Lagrange interpolation at x=0.
        for (i, (xi, yi)) in decoded.iter().enumerate() {
            let mut basis = 1u8;
            for (j, (xj, _)) in decoded.iter().enumerate() {
                if i == j {
                    continue;
                }
                // basis *= (0 - xj) / (xi - xj) = xj / (xi - xj) in GF(256)
                let numerator = *xj;
                let denominator = xi ^ xj; // subtraction = addition in GF(256)
                if denominator == 0 {
                    return Err(anyhow!(
                        "shares {} and {} have the same index — this should not happen",
                        xi,
                        xj
                    ));
                }
                basis = gf_mul(basis, gf_div(numerator, denominator));
            }
            result ^= gf_mul(basis, yi[byte_idx]);
        }

        secret.push(result);
    }

    // Verify integrity.
    let computed_hash = {
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(&secret);
        hex::encode(hash)
    };
    if computed_hash != secret_hash {
        return Err(anyhow!(
            "reconstructed data integrity check failed — shares may be corrupted or from different split operations"
        ));
    }

    Ok(secret)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_and_reconstruct_roundtrip() {
        let secret = b"hello, shamir secret sharing!";
        let shares = split(secret, 3, 5).unwrap();
        assert_eq!(shares.len(), 5);

        // Any 3 shares should reconstruct.
        let subset = vec![shares[0].clone(), shares[2].clone(), shares[4].clone()];
        let recovered = reconstruct(&subset).unwrap();
        assert_eq!(recovered, secret);
    }

    #[test]
    fn different_subsets_produce_same_result() {
        let secret = b"test secret data 12345";
        let shares = split(secret, 2, 3).unwrap();

        let r1 = reconstruct(&[shares[0].clone(), shares[1].clone()]).unwrap();
        let r2 = reconstruct(&[shares[1].clone(), shares[2].clone()]).unwrap();
        let r3 = reconstruct(&[shares[0].clone(), shares[2].clone()]).unwrap();

        assert_eq!(r1, secret);
        assert_eq!(r2, secret);
        assert_eq!(r3, secret);
    }

    #[test]
    fn insufficient_shares_fails() {
        let secret = b"need more shares";
        let shares = split(secret, 3, 5).unwrap();
        let result = reconstruct(&[shares[0].clone(), shares[1].clone()]);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("need at least 3 shares"));
    }

    #[test]
    fn corrupted_share_fails_integrity_check() {
        let secret = b"corruption detection";
        let shares = split(secret, 2, 3).unwrap();
        let mut tampered = shares[0].clone();
        // Flip a bit in the payload.
        let mut payload_bytes = hex::decode(&tampered.payload).unwrap();
        payload_bytes[0] ^= 0xFF;
        tampered.payload = hex::encode(&payload_bytes);

        let result = reconstruct(&[tampered, shares[1].clone()]);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("integrity check failed"));
    }

    #[test]
    fn wrong_threshold_errors() {
        let secret = b"threshold check";
        let shares = split(secret, 3, 5).unwrap();
        let result = reconstruct(&[shares[0].clone()]);
        assert!(result.is_err());
    }

    #[test]
    fn duplicate_share_index_errors() {
        let secret = b"duplicate check";
        let shares = split(secret, 2, 3).unwrap();
        let result = reconstruct(&[shares[0].clone(), shares[0].clone()]);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("duplicate share index"));
    }

    #[test]
    fn empty_shares_errors() {
        let result = reconstruct(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn threshold_must_be_at_least_two() {
        let result = split(b"secret", 1, 3);
        assert!(result.is_err());
    }

    #[test]
    fn threshold_cannot_exceed_total() {
        let result = split(b"secret", 5, 3);
        assert!(result.is_err());
    }

    #[test]
    fn total_shares_exceeds_max() {
        let result = split(b"secret", 3, 256);
        assert!(result.is_err());
    }

    #[test]
    fn empty_secret_works() {
        let shares = split(b"", 2, 3).unwrap();
        let recovered = reconstruct(&[shares[0].clone(), shares[1].clone()]).unwrap();
        assert_eq!(recovered, b"");
    }

    #[test]
    fn large_secret_works() {
        let secret = vec![0xABu8; 4096];
        let shares = split(&secret, 2, 3).unwrap();
        let recovered = reconstruct(&[shares[0].clone(), shares[2].clone()]).unwrap();
        assert_eq!(recovered, secret);
    }

    #[test]
    fn all_shares_are_different() {
        let shares = split(b"different check", 3, 5).unwrap();
        let payloads: Vec<&str> = shares.iter().map(|s| s.payload.as_str()).collect();
        let unique: std::collections::HashSet<&str> = payloads.into_iter().collect();
        assert_eq!(unique.len(), 5);
    }

    #[test]
    fn gf_arithmetic_basics() {
        // Multiplication by 1 is identity.
        assert_eq!(gf_mul(0, 0), 0);
        assert_eq!(gf_mul(1, 5), 5);
        assert_eq!(gf_mul(5, 1), 5);
        // Division is inverse of multiplication.
        assert_eq!(gf_div(gf_mul(3, 7), 7), 3);
    }

    #[test]
    fn single_byte_secret() {
        let shares = split(b"A", 2, 3).unwrap();
        let recovered = reconstruct(&[shares[1].clone(), shares[2].clone()]).unwrap();
        assert_eq!(recovered, b"A");
    }

    #[test]
    fn reconstruct_with_more_than_threshold_shares() {
        let secret = b"extra shares";
        let shares = split(secret, 2, 3).unwrap();
        let recovered = reconstruct(&shares).unwrap();
        assert_eq!(recovered, secret);
    }
}
