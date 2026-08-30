//! Integration tests for Shamir Secret Sharing recovery shares.
//!
//! These tests verify the full lifecycle: splitting an encrypted wallet backup
//! into recovery shares, validating shares, reconstructing the backup, and
//! handling error cases (insufficient shares, corrupted data).

use starforge::utils::shamir::{self, RecoveryShare};
use starforge::utils::wallet_import;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Simulate an encrypted backup bundle (the output of `crypto::encrypt_secret`).
fn fake_encrypted_bundle() -> String {
    "AAAA:BBBB:CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC".to_string()
}

fn fake_large_bundle() -> String {
    // Simulate a larger encrypted bundle with KDF params.
    format!(
        "{}:{}:{}:{}:{}:{}",
        "AAAA",
        "BBBB",
        "D".repeat(128), // long ciphertext
        65536,
        4,
        2
    )
}

// ── Shamir split/reconstruct roundtrip ───────────────────────────────────────

#[test]
fn split_and_reconstruct_small_secret() {
    let secret = b"hello world";
    let shares = shamir::split(secret, 2, 3).unwrap();
    assert_eq!(shares.len(), 3);

    let recovered = shamir::reconstruct(&[shares[0].clone(), shares[1].clone()]).unwrap();
    assert_eq!(recovered, secret);
}

#[test]
fn split_and_reconstruct_large_secret() {
    let secret = vec![0xABu8; 4096];
    let shares = shamir::split(&secret, 3, 5).unwrap();
    assert_eq!(shares.len(), 5);

    // Any 3 of 5 should work.
    let recovered =
        shamir::reconstruct(&[shares[0].clone(), shares[2].clone(), shares[4].clone()]).unwrap();
    assert_eq!(recovered, secret);
}

#[test]
fn split_and_reconstruct_all_shares() {
    let secret = b"all shares used";
    let shares = shamir::split(secret, 2, 3).unwrap();
    let recovered = shamir::reconstruct(&shares).unwrap();
    assert_eq!(recovered, secret);
}

#[test]
fn different_share_subsets_produce_same_result() {
    let secret = b"different subsets";
    let shares = shamir::split(secret, 2, 4).unwrap();

    let r1 = shamir::reconstruct(&[shares[0].clone(), shares[1].clone()]).unwrap();
    let r2 = shamir::reconstruct(&[shares[2].clone(), shares[3].clone()]).unwrap();
    let r3 = shamir::reconstruct(&[shares[0].clone(), shares[3].clone()]).unwrap();

    assert_eq!(r1, secret);
    assert_eq!(r2, secret);
    assert_eq!(r3, secret);
}

// ── Insufficient shares ──────────────────────────────────────────────────────

#[test]
fn insufficient_shares_returns_error() {
    let secret = b"not enough shares";
    let shares = shamir::split(secret, 3, 5).unwrap();

    let result = shamir::reconstruct(&[shares[0].clone(), shares[1].clone()]);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("need at least 3 shares"),
        "unexpected error: {}",
        msg
    );
}

#[test]
fn single_share_fails_for_threshold_2() {
    let secret = b"single share";
    let shares = shamir::split(secret, 2, 3).unwrap();

    let result = shamir::reconstruct(&[shares[0].clone()]);
    assert!(result.is_err());
}

// ── Corrupted shares ─────────────────────────────────────────────────────────

#[test]
fn tampered_share_fails_integrity_check() {
    let secret = b"corruption test";
    let shares = shamir::split(secret, 2, 3).unwrap();

    let mut tampered = shares[0].clone();
    let mut payload_bytes = hex::decode(&tampered.payload).unwrap();
    payload_bytes[0] ^= 0xFF; // flip bits
    tampered.payload = hex::encode(&payload_bytes);

    let result = shamir::reconstruct(&[tampered, shares[1].clone()]);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("integrity check failed"),
        "unexpected error: {}",
        msg
    );
}

#[test]
fn swapped_payloads_fails_integrity_check() {
    let secret = b"swapped payloads";
    let shares = shamir::split(secret, 2, 3).unwrap();

    let mut s0 = shares[0].clone();
    let mut s1 = shares[1].clone();
    // Swap payloads but keep indices
    std::mem::swap(&mut s0.payload, &mut s1.payload);

    let result = shamir::reconstruct(&[s0, s1]);
    assert!(result.is_err());
}

// ── Duplicate and empty shares ───────────────────────────────────────────────

#[test]
fn duplicate_share_index_fails() {
    let secret = b"duplicate index";
    let shares = shamir::split(secret, 2, 3).unwrap();

    let result = shamir::reconstruct(&[shares[0].clone(), shares[0].clone()]);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("duplicate"), "unexpected error: {}", msg);
}

#[test]
fn empty_shares_fails() {
    let result = shamir::reconstruct(&[]);
    assert!(result.is_err());
}

// ── Share metadata consistency ───────────────────────────────────────────────

#[test]
fn shares_carry_correct_metadata() {
    let secret = b"metadata check";
    let shares = shamir::split(secret, 3, 5).unwrap();

    for (i, share) in shares.iter().enumerate() {
        assert_eq!(share.index, (i + 1) as u8);
        assert_eq!(share.threshold, 3);
        assert_eq!(share.total_shares, 5);
        assert!(!share.secret_hash.is_empty());
        assert!(!share.payload.is_empty());
    }
}

#[test]
fn all_shares_have_same_secret_hash() {
    let secret = b"hash consistency";
    let shares = shamir::split(secret, 2, 4).unwrap();

    let first_hash = &shares[0].secret_hash;
    for share in &shares[1..] {
        assert_eq!(&share.secret_hash, first_hash);
    }
}

#[test]
fn all_shares_are_unique() {
    let secret = b"uniqueness check";
    let shares = shamir::split(secret, 3, 6).unwrap();

    let payloads: Vec<&str> = shares.iter().map(|s| s.payload.as_str()).collect();
    let unique: std::collections::HashSet<&str> = payloads.into_iter().collect();
    assert_eq!(unique.len(), 6);
}

// ── Share JSON serialization roundtrip ────────────────────────────────────────

#[test]
fn shares_serialize_and_deserialize() {
    let secret = b"serialization test";
    let shares = shamir::split(secret, 2, 3).unwrap();

    for share in &shares {
        let json = serde_json::to_string(share).unwrap();
        let deserialized: RecoveryShare = serde_json::from_str(&json).unwrap();
        assert_eq!(*share, deserialized);
    }
}

// ── wallet_import validation ─────────────────────────────────────────────────

#[test]
fn validate_shares_happy_path() {
    let secret = b"validation test";
    let shares = shamir::split(secret, 2, 3).unwrap();
    assert!(wallet_import::validate_recovery_shares(&shares).is_ok());
}

#[test]
fn validate_shares_empty_fails() {
    let result = wallet_import::validate_recovery_shares(&[]);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("no shares provided"));
}

#[test]
fn validate_shares_insufficient_count() {
    let secret = b"insufficient validation";
    let shares = shamir::split(secret, 3, 5).unwrap();
    // Only provide 2 out of 3 required
    let result = wallet_import::validate_recovery_shares(&shares[..2]);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("InsufficientShares") || err.contains("at least"));
}

#[test]
fn validate_shares_duplicate_index() {
    let secret = b"dup validation";
    let shares = shamir::split(secret, 2, 3).unwrap();
    let duped = vec![shares[0].clone(), shares[0].clone()];
    let result = wallet_import::validate_recovery_shares(&duped);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("duplicate"));
}

#[test]
fn validate_shares_inconsistent_threshold() {
    let secret = b"mismatch validation";
    let shares = shamir::split(secret, 2, 3).unwrap();
    let mut modified = shares[0].clone();
    modified.threshold = 99;
    let result = wallet_import::validate_recovery_shares(&[modified, shares[1].clone()]);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("threshold"));
}

#[test]
fn validate_shares_inconsistent_secret_hash() {
    let shares_a = shamir::split(b"secret_a", 2, 3).unwrap();
    let shares_b = shamir::split(b"secret_b", 2, 3).unwrap();
    // Mix shares from different splits
    let mixed = vec![shares_a[0].clone(), shares_b[1].clone()];
    let result = wallet_import::validate_recovery_shares(&mixed);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("different secret hash") || err.contains("secret_hash"));
}

// ── reconstruct_from_shares ──────────────────────────────────────────────────

#[test]
fn reconstruct_from_shares_roundtrip() {
    let bundle = fake_encrypted_bundle();
    let shares = shamir::split(bundle.as_bytes(), 2, 3).unwrap();

    let recovered = wallet_import::reconstruct_from_shares(&shares).unwrap();
    assert_eq!(recovered, bundle);
}

#[test]
fn reconstruct_from_shares_insufficient_fails() {
    let bundle = fake_encrypted_bundle();
    let shares = shamir::split(bundle.as_bytes(), 3, 5).unwrap();

    let result = wallet_import::reconstruct_from_shares(&shares[..2]);
    assert!(result.is_err());
}

// ── Edge cases ───────────────────────────────────────────────────────────────

#[test]
fn empty_secret_splits_and_reconstructs() {
    let shares = shamir::split(b"", 2, 3).unwrap();
    let recovered = shamir::reconstruct(&[shares[0].clone(), shares[2].clone()]).unwrap();
    assert_eq!(recovered, b"");
}

#[test]
fn single_byte_secret() {
    let shares = shamir::split(b"X", 2, 3).unwrap();
    let recovered = shamir::reconstruct(&[shares[1].clone(), shares[2].clone()]).unwrap();
    assert_eq!(recovered, b"X");
}

#[test]
fn maximum_threshold() {
    let secret = b"max threshold";
    let shares = shamir::split(secret, 255, 255).unwrap();
    assert_eq!(shares.len(), 255);

    // Need all 255 shares to reconstruct.
    let recovered = shamir::reconstruct(&shares).unwrap();
    assert_eq!(recovered, secret);
}

#[test]
fn split_rejects_threshold_below_two() {
    let result = shamir::split(b"secret", 1, 3);
    assert!(result.is_err());
}

#[test]
fn split_rejects_threshold_exceeding_total() {
    let result = shamir::split(b"secret", 5, 3);
    assert!(result.is_err());
}

#[test]
fn split_rejects_total_exceeding_max() {
    let result = shamir::split(b"secret", 3, 256);
    assert!(result.is_err());
}
