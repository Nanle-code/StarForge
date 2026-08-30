use starforge::utils::cargo_metadata::{
    CargoMetadataError, CargoMetadataFixer, CargoMetadataValidator,
};
use std::fs;
use tempfile::tempdir;

#[test]
fn test_primary_flow_replace_placeholder_repository_url() {
    let dir = tempdir().unwrap();
    let cargo_path = dir.path().join("Cargo.toml");

    let initial_cargo_content = r#"[package]
name = "my-awesome-crate"
version = "0.1.0"
edition = "2021"
description = "A test crate"
license = "MIT"
repository = "https://github.com/YOUR_USERNAME/starforge"
"#;

    fs::write(&cargo_path, initial_cargo_content).unwrap();

    // Verify initial metadata has placeholder URL
    let meta_before = CargoMetadataValidator::parse_metadata(&cargo_path).unwrap();
    assert_eq!(
        meta_before.repository.as_deref(),
        Some("https://github.com/YOUR_USERNAME/starforge")
    );
    assert!(CargoMetadataFixer::is_placeholder_url(
        meta_before.repository.as_ref().unwrap()
    ));

    // Run fixer with target repository URL
    let target_repo = "https://github.com/onajidavid87-web/StarForge";
    let fix_report =
        CargoMetadataFixer::locate_and_update_cargo_tomls(&cargo_path, target_repo).unwrap();

    assert_eq!(fix_report.files_scanned, 1);
    assert_eq!(fix_report.files_updated.len(), 1);
    assert_eq!(fix_report.placeholders_corrected, 1);

    // Verify updated metadata
    let meta_after = CargoMetadataValidator::parse_metadata(&cargo_path).unwrap();
    assert_eq!(
        meta_after.repository.as_deref(),
        Some("https://github.com/onajidavid87-web/StarForge")
    );
    assert_eq!(
        meta_after.homepage.as_deref(),
        Some("https://github.com/onajidavid87-web/StarForge")
    );
    assert_eq!(
        meta_after.documentation.as_deref(),
        Some("https://docs.rs/my-awesome-crate")
    );

    // Validate links
    let val_report = CargoMetadataValidator::validate_package_links(&cargo_path).unwrap();
    assert!(val_report.is_valid);
    assert!(val_report.repository_valid);
    assert!(val_report.homepage_valid);
    assert!(val_report.documentation_valid);
}

#[test]
fn test_boundary_case_monorepo_workspace_members() {
    let dir = tempdir().unwrap();

    let root_cargo = dir.path().join("Cargo.toml");
    let crate_a_dir = dir.path().join("crates").join("crate-a");
    let crate_b_dir = dir.path().join("crates").join("crate-b");

    fs::create_dir_all(&crate_a_dir).unwrap();
    fs::create_dir_all(&crate_b_dir).unwrap();

    fs::write(
        &root_cargo,
        r#"[package]
name = "workspace-root"
version = "0.1.0"
repository = "https://github.com/TODO/monorepo"
"#,
    )
    .unwrap();

    fs::write(
        crate_a_dir.join("Cargo.toml"),
        r#"[package]
name = "crate-a"
version = "0.1.0"
repository = "https://github.com/YOUR_USERNAME/monorepo"
"#,
    )
    .unwrap();

    fs::write(
        crate_b_dir.join("Cargo.toml"),
        r#"[package]
name = "crate-b"
version = "0.2.0"
repository = "https://example.com/repo"
documentation = "https://docs.rs/crate-b"
"#,
    )
    .unwrap();

    let target_repo = "https://github.com/onajidavid87-web/StarForge";
    let fix_report =
        CargoMetadataFixer::locate_and_update_cargo_tomls(dir.path(), target_repo).unwrap();

    assert_eq!(fix_report.files_scanned, 3);
    assert_eq!(fix_report.files_updated.len(), 3);

    // Verify crate-a has crate-a docs and workspace repo URL
    let meta_a = CargoMetadataValidator::parse_metadata(&crate_a_dir.join("Cargo.toml")).unwrap();
    assert_eq!(meta_a.repository.as_deref(), Some(target_repo));
    assert_eq!(
        meta_a.documentation.as_deref(),
        Some("https://docs.rs/crate-a")
    );

    // Verify crate-b has crate-b docs and workspace repo URL
    let meta_b = CargoMetadataValidator::parse_metadata(&crate_b_dir.join("Cargo.toml")).unwrap();
    assert_eq!(meta_b.repository.as_deref(), Some(target_repo));
    assert_eq!(
        meta_b.documentation.as_deref(),
        Some("https://docs.rs/crate-b")
    );
}

#[test]
fn test_failure_case_invalid_url_rejection() {
    let dir = tempdir().unwrap();
    let cargo_path = dir.path().join("Cargo.toml");

    fs::write(
        &cargo_path,
        r#"[package]
name = "invalid-url-crate"
version = "0.1.0"
repository = "invalid-scheme-url"
"#,
    )
    .unwrap();

    // Direct URL validation
    let err = CargoMetadataFixer::validate_url_format("ftp://invalid-domain").unwrap_err();
    assert_eq!(
        err,
        CargoMetadataError::InvalidUrl("ftp://invalid-domain".to_string())
    );

    let incomplete_err =
        CargoMetadataFixer::validate_url_format("https://github.com/TODO").unwrap_err();
    assert_eq!(
        incomplete_err,
        CargoMetadataError::IncompleteUrlPattern("https://github.com/TODO".to_string())
    );

    // Package link validation fails for invalid scheme
    let val_report = CargoMetadataValidator::validate_package_links(&cargo_path).unwrap();
    assert!(!val_report.is_valid);
    assert!(!val_report.repository_valid);
    assert!(!val_report.errors.is_empty());
}
