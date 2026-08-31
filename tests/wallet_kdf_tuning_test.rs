use starforge::utils::config::{self, WalletEntry};
use starforge::utils::crypto::{
    self, KdfMetadata, KdfOptions, KDF_VERSION_1, MAX_KDF_MEM, MIN_KDF_MEM,
};

#[test]
fn test_kdf_metadata_default_and_validation() {
    let defaults = argon2::Params::default();
    let meta = KdfMetadata::default_v1();
    assert_eq!(meta.version, KDF_VERSION_1);
    assert_eq!(meta.mem, defaults.m_cost());
    assert_eq!(meta.iterations, defaults.t_cost());
    assert_eq!(meta.parallelism, defaults.p_cost());
    assert!(meta.validate().is_ok());
}

#[test]
fn test_kdf_parameter_boundary_checks() {
    // Valid minimum boundary
    let min_kdf = KdfOptions {
        mem: Some(MIN_KDF_MEM),
        iterations: Some(1),
        parallelism: Some(1),
    };
    assert!(min_kdf.validate().is_ok());

    // Invalid memory cost below minimum
    let low_mem = KdfOptions {
        mem: Some(1024),
        iterations: Some(3),
        parallelism: Some(1),
    };
    assert!(low_mem.validate().is_err());

    // Invalid memory cost above maximum
    let high_mem = KdfOptions {
        mem: Some(MAX_KDF_MEM + 1),
        iterations: Some(3),
        parallelism: Some(1),
    };
    assert!(high_mem.validate().is_err());

    // Invalid zero iteration count
    let zero_iter = KdfOptions {
        mem: Some(32768),
        iterations: Some(0),
        parallelism: Some(1),
    };
    assert!(zero_iter.validate().is_err());

    // Invalid zero parallelism
    let zero_p = KdfOptions {
        mem: Some(32768),
        iterations: Some(3),
        parallelism: Some(0),
    };
    assert!(zero_p.validate().is_err());
}

#[test]
fn test_primary_kdf_upgrade_flow() {
    let defaults = argon2::Params::default();
    let password = "correct-horse-battery-staple";
    let secret = "SXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX";

    // Initial encryption with default parameters
    let bundle_v1 = crypto::encrypt_secret(password, secret, None).unwrap();
    assert_eq!(crypto::decrypt_secret(password, &bundle_v1).unwrap(), secret);

    // Initial metadata extraction
    let initial_meta = crypto::extract_kdf_metadata(&bundle_v1).unwrap();
    assert_eq!(initial_meta.version, KDF_VERSION_1);
    assert_eq!(initial_meta.mem, defaults.m_cost());

    // Upgrade to higher security parameters
    let upgraded_kdf = KdfOptions {
        mem: Some(65536),
        iterations: Some(4),
        parallelism: Some(2),
    };

    let bundle_v2 =
        crypto::upgrade_wallet_kdf_secret(password, &bundle_v1, Some(&upgraded_kdf)).unwrap();

    // Verify bundle format carries v1 version tag
    assert!(bundle_v2.starts_with("v1:"));

    // Verify upgraded decryption works seamlessly
    let decrypted = crypto::decrypt_secret(password, &bundle_v2).unwrap();
    assert_eq!(decrypted, secret);

    // Verify updated metadata
    let new_meta = crypto::extract_kdf_metadata(&bundle_v2).unwrap();
    assert_eq!(new_meta.version, KDF_VERSION_1);
    assert_eq!(new_meta.mem, 65536);
    assert_eq!(new_meta.iterations, 4);
    assert_eq!(new_meta.parallelism, 2);
}

#[test]
fn test_legacy_three_part_bundle_upgrade_boundary() {
    let password = "correct-horse-battery-staple";
    let secret = "SXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX";

    // Simulate legacy 3-part bundle (no explicit KDF parameters)
    let legacy_bundle = crypto::encrypt_secret(password, secret, None).unwrap();
    assert_eq!(legacy_bundle.split(':').count(), 3);

    let kdf_opts = KdfOptions {
        mem: Some(40960),
        iterations: Some(5),
        parallelism: Some(2),
    };

    // Upgrade legacy bundle to versioned tuned bundle
    let upgraded_bundle =
        crypto::upgrade_wallet_kdf_secret(password, &legacy_bundle, Some(&kdf_opts)).unwrap();

    assert!(upgraded_bundle.starts_with("v1:"));
    assert_eq!(upgraded_bundle.split(':').count(), 7);
    assert_eq!(
        crypto::decrypt_secret(password, &upgraded_bundle).unwrap(),
        secret
    );

    let meta = crypto::extract_kdf_metadata(&upgraded_bundle).unwrap();
    assert_eq!(meta.mem, 40960);
    assert_eq!(meta.iterations, 5);
    assert_eq!(meta.parallelism, 2);
}

#[test]
fn test_failure_path_wrong_password_during_upgrade() {
    let password = "correct-horse-battery-staple";
    let secret = "SXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX";
    let bundle = crypto::encrypt_secret(password, secret, None).unwrap();

    let upgraded_kdf = KdfOptions {
        mem: Some(65536),
        iterations: Some(4),
        parallelism: Some(1),
    };

    // Attempting upgrade with wrong password fails fast
    let result = crypto::upgrade_wallet_kdf_secret("wrong-password", &bundle, Some(&upgraded_kdf));
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Decryption failed"));

    // Original bundle remains intact and decryptable with original password
    assert_eq!(crypto::decrypt_secret(password, &bundle).unwrap(), secret);
}

#[test]
fn test_failure_path_invalid_kdf_parameters_during_upgrade() {
    let password = "correct-horse-battery-staple";
    let secret = "SXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX";
    let bundle = crypto::encrypt_secret(password, secret, None).unwrap();

    // Invalid parameters (mem = 0)
    let invalid_kdf = KdfOptions {
        mem: Some(0),
        iterations: Some(3),
        parallelism: Some(1),
    };

    let result = crypto::upgrade_wallet_kdf_secret(password, &bundle, Some(&invalid_kdf));
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Memory cost must be between"));

    // Original bundle remains untouched
    assert_eq!(crypto::decrypt_secret(password, &bundle).unwrap(), secret);
}

#[test]
fn test_wallet_entry_kdf_metadata_extraction() {
    let entry = WalletEntry {
        name: "alice".to_string(),
        public_key: "GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX".to_string(),
        secret_key: Some(
            "v1:YWJjZGVmZ2hpamtsbW5vcA==:cXdlcnR5dWlvcGFzZGZnaA==:aGprbGFzZGZnaGprbGFzZGY=:65536:4:2".to_string(),
        ),
        network: "testnet".to_string(),
        created_at: "2026-08-29T10:00:00Z".to_string(),
        funded: false,
        kdf_options: Some(KdfOptions {
            mem: Some(65536),
            iterations: Some(4),
            parallelism: Some(2),
        }),
        rotation_history: vec![],
    };

    let meta = entry.kdf_metadata().expect("metadata should extract");
    assert_eq!(meta.version, KDF_VERSION_1);
    assert_eq!(meta.mem, 65536);
    assert_eq!(meta.iterations, 4);
    assert_eq!(meta.parallelism, 2);
}
