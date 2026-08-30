//! Generated-input tests for the wallet import and backup parsers (issue #697).
//!
//! The `fuzz/fuzz_targets/fuzz_wallet_*` harnesses explore these same functions
//! far more deeply, but `cargo fuzz` needs a nightly toolchain and runs for
//! minutes. These proptest cases run on stable in the normal `cargo test`
//! sweep, so the invariants the fuzzers assert are also checked on every PR.
//!
//! Run with:
//!   cargo test --test wallet_import_property_tests
//!
//! Deeper coverage:
//!   PROPTEST_CASES=10000 cargo test --test wallet_import_property_tests

#![allow(dead_code)]

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use proptest::prelude::*;
use starforge::utils::wallet_import::{
    classify_payload, compute_integrity_tag, parse_encrypted_envelope, parse_wallet_backup,
    verify_integrity_tag, PayloadKind, WalletBackup, WalletImportError, BACKUP_HMAC_KEY,
    GCM_TAG_LEN, MAX_BACKUP_BYTES, MAX_WALLETS_PER_BACKUP, MAX_WALLET_NAME_LEN, NONCE_LEN,
    SALT_LEN,
};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

const STELLAR_CHARSET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn stellar_chars(len: usize) -> impl Strategy<Value = String> {
    proptest::collection::vec(proptest::sample::select(STELLAR_CHARSET.as_bytes()), len)
        .prop_map(|v| String::from_utf8(v).unwrap())
}

fn public_key() -> impl Strategy<Value = String> {
    stellar_chars(55).prop_map(|s| format!("G{}", s))
}

fn secret_key() -> impl Strategy<Value = String> {
    stellar_chars(55).prop_map(|s| format!("S{}", s))
}

fn wallet_name() -> impl Strategy<Value = String> {
    "[A-Za-z0-9_-]{1,32}"
}

/// Build a v2 backup document (current default version).
fn backup_document(entries: &[String]) -> String {
    format!(
        r#"{{"version":"2","exported_at":"2026-07-29T00:00:00Z","wallets":[{}]}}"#,
        entries.join(",")
    )
}

/// Build a v1 backup document (no integrity tag) for migration tests.
fn backup_document_v1(entries: &[String]) -> String {
    format!(
        r#"{{"version":"1","exported_at":"2026-07-29T00:00:00Z","wallets":[{}]}}"#,
        entries.join(",")
    )
}

fn entry_json(name: &str, public_key: &str, secret_key: Option<&str>, network: &str) -> String {
    let secret = match secret_key {
        Some(s) => format!("\"{}\"", s),
        None => "null".to_string(),
    };
    format!(
        r#"{{"name":"{}","public_key":"{}","secret_key":{},"network":"{}","created_at":"2026-07-29T00:00:00Z","funded":false}}"#,
        name, public_key, secret, network
    )
}

fn envelope(salt_len: usize, nonce_len: usize, ct_len: usize) -> String {
    format!(
        "{}:{}:{}",
        BASE64.encode(vec![1u8; salt_len]),
        BASE64.encode(vec![2u8; nonce_len]),
        BASE64.encode(vec![3u8; ct_len])
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Primary flow
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    /// Any generated well-formed backup parses, and the parsed values are the
    /// ones that went in.
    #[test]
    fn well_formed_backups_parse_and_preserve_their_values(
        entries in proptest::collection::vec(
            (wallet_name(), public_key(), proptest::option::of(secret_key())),
            1..6,
        )
    ) {
        // Names must be unique for the document to be valid at all.
        let mut seen = std::collections::HashSet::new();
        let entries: Vec<_> = entries
            .into_iter()
            .filter(|(name, _, _)| seen.insert(name.clone()))
            .collect();

        let doc = backup_document(
            &entries
                .iter()
                .map(|(name, key, secret)| entry_json(name, key, secret.as_deref(), "testnet"))
                .collect::<Vec<_>>(),
        );

        let parsed = parse_wallet_backup(&doc).expect("well-formed backup must parse");
        prop_assert_eq!(parsed.backup.wallets.len(), entries.len());
        for (wallet, (name, key, secret)) in parsed.backup.wallets.iter().zip(&entries) {
            prop_assert_eq!(&wallet.name, name);
            prop_assert_eq!(&wallet.public_key, key);
            prop_assert_eq!(&wallet.secret_key, secret);
        }
        // v2 backups without an integrity tag produce a "no integrity tag" warning —
        // that is expected and tested separately. We only check for unexpected
        // non-integrity-tag warnings here.
        let unexpected: Vec<_> = parsed
            .warnings
            .iter()
            .filter(|w| !w.contains("integrity tag"))
            .collect();
        prop_assert!(unexpected.is_empty(), "unexpected warnings: {:?}", unexpected);
    }

    /// Any structurally valid envelope parses back to the exact field lengths.
    #[test]
    fn well_formed_envelopes_parse(ct_len in GCM_TAG_LEN..512usize) {
        let env = parse_encrypted_envelope(&envelope(SALT_LEN, NONCE_LEN, ct_len))
            .expect("structurally valid envelope must parse");
        prop_assert_eq!(env.salt.len(), SALT_LEN);
        prop_assert_eq!(env.nonce.len(), NONCE_LEN);
        prop_assert_eq!(env.ciphertext.len(), ct_len);
    }

    /// Classification is deterministic and never mistakes JSON for a bundle.
    #[test]
    fn classification_is_deterministic_and_never_misreads_json(text in ".{0,300}") {
        let first = classify_payload(&text);
        prop_assert_eq!(first, classify_payload(&text));

        let trimmed = text.trim();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            prop_assert_eq!(first, PayloadKind::Plaintext);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Boundary cases
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    /// Ciphertexts shorter than one GCM tag are always rejected; anything at or
    /// above the tag length is always accepted.
    #[test]
    fn the_ciphertext_length_boundary_is_exact(len in 0usize..64) {
        let result = parse_encrypted_envelope(&envelope(SALT_LEN, NONCE_LEN, len));
        if len < GCM_TAG_LEN {
            prop_assert!(
                matches!(result, Err(WalletImportError::TruncatedCiphertext { .. })),
                "accepted a {}-byte ciphertext", len
            );
        } else {
            prop_assert!(result.is_ok(), "rejected a {}-byte ciphertext", len);
        }
    }

    /// Salt and nonce lengths other than the expected ones are rejected.
    #[test]
    fn salt_and_nonce_lengths_are_enforced(len in 0usize..40) {
        let salt_result = parse_encrypted_envelope(&envelope(len, NONCE_LEN, 32));
        prop_assert_eq!(salt_result.is_ok(), len == SALT_LEN);

        let nonce_result = parse_encrypted_envelope(&envelope(SALT_LEN, len, 32));
        prop_assert_eq!(nonce_result.is_ok(), len == NONCE_LEN);
    }

    /// Wallet names are accepted up to the character limit and rejected beyond.
    #[test]
    fn the_wallet_name_length_boundary_is_exact(
        len in 1usize..(MAX_WALLET_NAME_LEN + 8),
        key in public_key(),
    ) {
        let name = "a".repeat(len);
        let doc = backup_document(&[entry_json(&name, &key, None, "testnet")]);
        let result = parse_wallet_backup(&doc);

        if len <= MAX_WALLET_NAME_LEN {
            prop_assert!(result.is_ok(), "rejected a {}-character name", len);
        } else {
            prop_assert!(
                matches!(result, Err(WalletImportError::DeceptiveWalletName { .. })),
                "accepted a {}-character name", len
            );
        }
    }
}

#[test]
fn boundary_oversized_input_is_rejected_by_size_not_by_content() {
    // Valid JSON, just too big: the size gate must fire before the parser.
    let padding = "a".repeat(MAX_BACKUP_BYTES);
    let doc = format!(
        r#"{{"version":"1","exported_at":"{}","wallets":[]}}"#,
        padding
    );

    assert!(matches!(
        parse_wallet_backup(&doc),
        Err(WalletImportError::TooLarge { .. })
    ));
}

#[test]
fn boundary_exactly_one_wallet_is_the_minimum_accepted() {
    let key = format!("G{}", "A".repeat(55));
    assert!(parse_wallet_backup(&backup_document(&[])).is_err());
    assert!(parse_wallet_backup(&backup_document(&[entry_json(
        "solo", &key, None, "testnet"
    )]))
    .is_ok());
}

// ─────────────────────────────────────────────────────────────────────────────
// Failure cases
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    /// No input, however hostile, may panic either parser.
    #[test]
    fn parsers_never_panic(text in ".{0,600}") {
        let _ = parse_wallet_backup(&text);
        let _ = parse_encrypted_envelope(&text);
        let _ = classify_payload(&text);
    }

    /// Arbitrary bytes reinterpreted as UTF-8 must not panic either.
    #[test]
    fn parsers_never_panic_on_lossy_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..600)) {
        let text = String::from_utf8_lossy(&bytes);
        let _ = parse_wallet_backup(&text);
        let _ = parse_encrypted_envelope(&text);
        let _ = classify_payload(&text);
    }

    /// A public key that is not a well-formed StrKey is always rejected.
    #[test]
    fn malformed_public_keys_are_always_rejected(bad in "[a-zA-Z0-9]{0,60}") {
        prop_assume!(!(bad.len() == 55 && bad.chars().all(|c| matches!(c, 'A'..='Z' | '2'..='7'))));

        let doc = backup_document(&[entry_json("w", &format!("G{}", bad), None, "testnet")]);
        prop_assert!(
            matches!(parse_wallet_backup(&doc), Err(WalletImportError::InvalidEntry { .. })),
            "accepted a malformed public key of length {}", bad.len() + 1
        );
    }

    /// A secret key that is neither a StrKey nor a valid bundle is rejected.
    #[test]
    fn malformed_secret_keys_are_always_rejected(bad in "[A-Z2-7]{0,54}", key in public_key()) {
        let secret = format!("S{}", bad);
        prop_assume!(secret.len() != 56);

        let doc = backup_document(&[entry_json("w", &key, Some(&secret), "testnet")]);
        prop_assert!(
            matches!(
                parse_wallet_backup(&doc),
                Err(WalletImportError::InvalidEntry { .. })
            ),
            "accepted a malformed secret key of length {}",
            secret.len()
        );
    }

    /// Invisible and direction-changing characters in a name are always
    /// rejected, whichever position they occupy.
    #[test]
    fn deceptive_names_are_always_rejected(
        prefix in "[a-z]{0,8}",
        suffix in "[a-z]{0,8}",
        bad in proptest::sample::select(vec![
            '\u{200B}', '\u{200E}', '\u{202A}', '\u{202E}', '\u{2066}', '\u{FEFF}', '\u{00AD}',
        ]),
        key in public_key(),
    ) {
        let name = format!("{}{}{}", prefix, bad, suffix);
        let doc = backup_document(&[entry_json(&name, &key, None, "testnet")]);

        prop_assert!(
            matches!(parse_wallet_backup(&doc), Err(WalletImportError::DeceptiveWalletName { .. })),
            "accepted a name containing U+{:04X}", bad as u32
        );
    }

    /// A rejection must never quote the secret key back at the user, because
    /// error text lands in terminals, CI logs, and bug reports.
    #[test]
    fn errors_never_echo_secret_material(secret in secret_key()) {
        // Well-formed secret, malformed public key: the entry is rejected and
        // the message must mention the wallet, not the key.
        let doc = backup_document(&[entry_json("w", "GBAD", Some(&secret), "testnet")]);
        let err = parse_wallet_backup(&doc).unwrap_err().to_string();

        prop_assert!(!err.contains(&secret), "secret leaked into: {}", err);
    }
}

#[test]
fn failure_duplicate_names_are_rejected_even_when_entries_differ() {
    let a = format!("G{}", "A".repeat(55));
    let b = format!("G{}", "B".repeat(55));
    let doc = backup_document(&[
        entry_json("same", &a, None, "testnet"),
        entry_json("same", &b, None, "mainnet"),
    ]);

    assert_eq!(
        parse_wallet_backup(&doc).unwrap_err(),
        WalletImportError::DuplicateWallet("same".to_string())
    );
}

#[test]
fn failure_too_many_wallets_is_rejected() {
    let key = format!("G{}", "A".repeat(55));
    let entries: Vec<String> = (0..=MAX_WALLETS_PER_BACKUP)
        .map(|i| entry_json(&format!("w{}", i), &key, None, "testnet"))
        .collect();

    assert!(matches!(
        parse_wallet_backup(&backup_document(&entries)),
        Err(WalletImportError::TooManyWallets { .. })
    ));
}

// ─────────────────────────────────────────────────────────────────────────────
// Integrity-tag property tests
// ─────────────────────────────────────────────────────────────────────────────

proptest! {
    /// Any mutation of the wallet name or public key in a v2 backup must cause
    /// verify_integrity_tag to return false, demonstrating the tag binds the
    /// content of the document.
    #[test]
    fn tampered_v2_body_always_fails_tag_verification(
        name in wallet_name(),
        key in public_key(),
        tampered_name in wallet_name(),
    ) {
        // Only interesting when the tampered name differs from the original.
        prop_assume!(name != tampered_name);

        // Build a valid v2 backup and compute its tag.
        let entry = entry_json(&name, &key, None, "testnet");
        let backup: WalletBackup =
            serde_json::from_str(&backup_document(&[entry])).unwrap();
        let tag = compute_integrity_tag(&backup, BACKUP_HMAC_KEY)
            .expect("compute_integrity_tag must not fail");

        // Build a tampered backup (different wallet name) and verify the
        // original tag is no longer valid.
        let tampered_entry = entry_json(&tampered_name, &key, None, "testnet");
        let tampered: WalletBackup =
            serde_json::from_str(&backup_document(&[tampered_entry])).unwrap();

        prop_assert!(
            !verify_integrity_tag(&tampered, &tag, BACKUP_HMAC_KEY),
            "tag must not verify against a tampered backup"
        );
    }

    /// A v1 backup — however its wallet entries vary — must always parse as Ok
    /// and include at least one warning mentioning "version 1".
    #[test]
    fn v1_backup_always_parses_with_version_warning(
        entries in proptest::collection::vec(
            (wallet_name(), public_key()),
            1..4,
        )
    ) {
        // Deduplicate names so the backup is structurally valid.
        let mut seen = std::collections::HashSet::new();
        let entries: Vec<_> = entries
            .into_iter()
            .filter(|(name, _)| seen.insert(name.clone()))
            .collect();
        prop_assume!(!entries.is_empty());

        let entry_strings: Vec<String> = entries
            .iter()
            .map(|(name, key)| entry_json(name, key, None, "testnet"))
            .collect();
        let doc = backup_document_v1(&entry_strings);

        let parsed = parse_wallet_backup(&doc).expect("v1 backup must parse");
        prop_assert!(
            parsed.warnings.iter().any(|w| w.contains("version 1")),
            "v1 backup must warn about version 1, got: {:?}", parsed.warnings
        );
    }
}
