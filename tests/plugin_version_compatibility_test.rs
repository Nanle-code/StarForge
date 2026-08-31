use starforge::plugins::interface::CORE_VERSION;
use starforge::plugins::manifest::{
    load_manifest_for_library, require_compatible_manifest, PluginManifest, SupportedVersionPolicy,
    MANIFEST_FILENAME,
};

#[test]
fn test_primary_flow_compatible_manifest() {
    let manifest = PluginManifest {
        name: "test-plugin".to_string(),
        version: "1.0.0".to_string(),
        starforge_version: CORE_VERSION.to_string(),
        description: "A valid compatible test plugin".to_string(),
        starforge_version_min: None,
        starforge_version_max: None,
        required_capabilities: vec![],
        publisher: None,
        publisher_key: None,
        signature: None,
    };

    assert!(manifest.validate().is_ok());
}

#[test]
fn test_boundary_exact_min_max_version_bounds() {
    // Exact min bound matching current CORE_VERSION
    let min_manifest = PluginManifest {
        name: "min-bound-plugin".to_string(),
        version: "1.0.0".to_string(),
        starforge_version: CORE_VERSION.to_string(),
        description: "Exact min bound test".to_string(),
        starforge_version_min: Some(CORE_VERSION.to_string()),
        starforge_version_max: None,
        required_capabilities: vec![],
        publisher: None,
        publisher_key: None,
        signature: None,
    };
    assert!(min_manifest.validate().is_ok());

    // Exact max bound matching current CORE_VERSION
    let max_manifest = PluginManifest {
        name: "max-bound-plugin".to_string(),
        version: "1.0.0".to_string(),
        starforge_version: CORE_VERSION.to_string(),
        description: "Exact max bound test".to_string(),
        starforge_version_min: None,
        starforge_version_max: Some(CORE_VERSION.to_string()),
        required_capabilities: vec![],
        publisher: None,
        publisher_key: None,
        signature: None,
    };
    assert!(max_manifest.validate().is_ok());

    // Both min and max set to current CORE_VERSION
    let strict_manifest = PluginManifest {
        name: "strict-bound-plugin".to_string(),
        version: "1.0.0".to_string(),
        starforge_version: CORE_VERSION.to_string(),
        description: "Strict range test".to_string(),
        starforge_version_min: Some(CORE_VERSION.to_string()),
        starforge_version_max: Some(CORE_VERSION.to_string()),
        required_capabilities: vec![],
        publisher: None,
        publisher_key: None,
        signature: None,
    };
    assert!(strict_manifest.validate().is_ok());
}

#[test]
fn test_boundary_policy_summary_and_semver_parsing() {
    let policy = SupportedVersionPolicy::new(CORE_VERSION);
    let summary = policy.policy_summary();
    assert!(summary.contains("StarForge Supported-Version Policy"));
    assert!(summary.contains(CORE_VERSION));
}

#[test]
fn test_failure_incompatible_major_version() {
    let core_major: u64 = CORE_VERSION
        .split('.')
        .next()
        .unwrap_or("0")
        .parse()
        .unwrap_or(0);
    let incompatible_version = format!("{}.0.0", core_major + 10);

    let manifest = PluginManifest {
        name: "future-plugin".to_string(),
        version: "1.0.0".to_string(),
        starforge_version: incompatible_version,
        description: "Incompatible major version plugin".to_string(),
        starforge_version_min: None,
        starforge_version_max: None,
        required_capabilities: vec![],
        publisher: None,
        publisher_key: None,
        signature: None,
    };

    let err = manifest.validate().unwrap_err();
    assert!(err.to_string().contains("incompatible"));
    assert!(err.to_string().contains("Supported-Version Policy"));
}

#[test]
fn test_failure_min_version_violation() {
    let manifest = PluginManifest {
        name: "future-req-plugin".to_string(),
        version: "1.0.0".to_string(),
        starforge_version: CORE_VERSION.to_string(),
        description: "Requires higher CLI version".to_string(),
        starforge_version_min: Some("99.0.0".to_string()),
        starforge_version_max: None,
        required_capabilities: vec![],
        publisher: None,
        publisher_key: None,
        signature: None,
    };

    let err = manifest.validate().unwrap_err();
    assert!(err.to_string().contains("policy failure"));
    assert!(err.to_string().contains("99.0.0"));
}

#[test]
fn test_failure_max_version_violation() {
    let manifest = PluginManifest {
        name: "legacy-plugin".to_string(),
        version: "1.0.0".to_string(),
        starforge_version: CORE_VERSION.to_string(),
        description: "Legacy capped plugin".to_string(),
        starforge_version_min: None,
        starforge_version_max: Some("0.0.1".to_string()),
        required_capabilities: vec![],
        publisher: None,
        publisher_key: None,
        signature: None,
    };

    let err = manifest.validate().unwrap_err();
    assert!(err.to_string().contains("policy failure"));
    assert!(err.to_string().contains("0.0.1"));
}

#[test]
fn test_failure_missing_required_manifest_fields() {
    let empty_name = PluginManifest {
        name: "  ".to_string(),
        version: "1.0.0".to_string(),
        starforge_version: CORE_VERSION.to_string(),
        description: "".to_string(),
        starforge_version_min: None,
        starforge_version_max: None,
        required_capabilities: vec![],
        publisher: None,
        publisher_key: None,
        signature: None,
    };
    assert!(empty_name.validate().is_err());

    let empty_version = PluginManifest {
        name: "test".to_string(),
        version: "".to_string(),
        starforge_version: CORE_VERSION.to_string(),
        description: "".to_string(),
        starforge_version_min: None,
        starforge_version_max: None,
        required_capabilities: vec![],
        publisher: None,
        publisher_key: None,
        signature: None,
    };
    assert!(empty_version.validate().is_err());
}

#[test]
fn test_load_manifest_and_require_manifest_file_system() {
    let tmp = tempfile::TempDir::new().unwrap();
    let manifest_path = tmp.path().join(MANIFEST_FILENAME);
    let lib_path = tmp.path().join("libtest.so");

    std::fs::write(&lib_path, b"mock binary content").unwrap();

    // Absent manifest fails require_compatible_manifest
    let missing_err = require_compatible_manifest(&lib_path, "test-plugin").unwrap_err();
    assert!(missing_err
        .to_string()
        .contains("Plugin manifest not found"));

    // Write valid manifest
    std::fs::write(
        &manifest_path,
        format!(
            r#"
name = "test-plugin"
version = "1.0.0"
starforge_version = "{core}"
"#,
            core = CORE_VERSION
        ),
    )
    .unwrap();

    let loaded = load_manifest_for_library(&lib_path).unwrap().unwrap();
    assert_eq!(loaded.name, "test-plugin");

    let verified = require_compatible_manifest(&lib_path, "test-plugin").unwrap();
    assert_eq!(verified.version, "1.0.0");
}
