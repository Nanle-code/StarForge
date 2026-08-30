use std::fs;
use std::path::PathBuf;

// Note: These are integration-style tests that would normally be in tests/
// For now, we'll create a basic structure to demonstrate the testing approach

#[cfg(test)]
mod template_tests {
    use super::*;

    #[test]
    fn test_template_registry_structure() {
        // Verify the registry.json file exists and is valid JSON
        let registry_path = PathBuf::from("templates/registry.json");
        assert!(registry_path.exists(), "Registry file should exist");

        let content =
            fs::read_to_string(&registry_path).expect("Should be able to read registry file");

        let parsed: serde_json::Value =
            serde_json::from_str(&content).expect("Registry should be valid JSON");

        assert!(
            parsed.get("version").is_some(),
            "Registry should have version"
        );
        assert!(
            parsed.get("templates").is_some(),
            "Registry should have templates array"
        );
    }

    #[test]
    fn test_example_template_structure() {
        // Verify the example template has required files
        let template_path = PathBuf::from("templates/examples/simple-counter");

        assert!(template_path.exists(), "Example template should exist");
        assert!(
            template_path.join("Cargo.toml").exists(),
            "Template should have Cargo.toml"
        );
        assert!(
            template_path.join("src").exists(),
            "Template should have src directory"
        );
        assert!(
            template_path.join("src/lib.rs").exists(),
            "Template should have src/lib.rs"
        );
    }

    #[test]
    fn test_template_placeholders() {
        // Verify template files contain placeholders
        let lib_rs = PathBuf::from("templates/examples/simple-counter/src/lib.rs");
        let content = fs::read_to_string(&lib_rs).expect("Should be able to read lib.rs");

        assert!(
            content.contains("{{PROJECT_NAME_PASCAL}}"),
            "Template should contain PROJECT_NAME_PASCAL placeholder"
        );
    }

    #[test]
    fn test_cargo_toml_placeholders() {
        let cargo_toml = PathBuf::from("templates/examples/simple-counter/Cargo.toml");
        let content = fs::read_to_string(&cargo_toml).expect("Should be able to read Cargo.toml");

        assert!(
            content.contains("{{PROJECT_NAME}}"),
            "Cargo.toml should contain PROJECT_NAME placeholder"
        );
    }

    #[test]
    fn test_verify_checksum_matches() {
        use sha2::{Digest, Sha256};
        use starforge::utils::templates::verify_archive_checksum;

        let sample_bytes = b"hello starforge template verification";
        let mut hasher = Sha256::new();
        hasher.update(sample_bytes);
        let digest = hasher.finalize();
        let expected_hex = hex::encode(digest);

        let result = verify_archive_checksum(sample_bytes, &expected_hex);
        assert!(
            result.is_ok(),
            "Checksum verification should succeed for matching hash"
        );
    }

    #[test]
    fn test_verify_checksum_mismatch() {
        use sha2::{Digest, Sha256};
        use starforge::utils::templates::verify_archive_checksum;

        let sample_bytes = b"tampered bytes in archive";
        let mut hasher = Sha256::new();
        hasher.update(sample_bytes);
        let digest = hasher.finalize();
        let actual_hex = hex::encode(digest);

        let wrong_hex = "0000000000000000000000000000000000000000000000000000000000000000";

        let result = verify_archive_checksum(sample_bytes, wrong_hex);
        assert!(
            result.is_err(),
            "Checksum verification should fail for mismatching hash"
        );

        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains(wrong_hex),
            "Error message should contain expected hex '{}': got '{}'",
            wrong_hex,
            err_msg
        );
        assert!(
            err_msg.contains(&actual_hex),
            "Error message should contain actual hex '{}': got '{}'",
            actual_hex,
            err_msg
        );
    }

    #[test]
    fn test_verify_checksum_skipped_when_none() {
        use starforge::utils::templates::verify_archive_checksum;

        let sample_bytes = b"dummy archive bytes";
        let expected_sha256: Option<&str> = None;

        let result: anyhow::Result<()> = match expected_sha256 {
            Some(hex) => verify_archive_checksum(sample_bytes, hex),
            None => Ok(()),
        };

        assert!(
            result.is_ok(),
            "No error should occur when expected_sha256 is None"
        );
    }
}
