//! Integration tests for the AI template testing framework.
//!
//! These tests exercise the full test pipeline against real templates
//! shipped with StarForge, as well as synthetic edge-case templates.

use starforge::utils::ai_template_testing::{
    test_all_templates, test_template, FindingCategory, Severity, TemplateTestReport, TestConfig,
};
use std::fs;
use std::path::Path;

// ─── Helpers ────────────────────────────────────────────────────────────────

fn templates_dir() -> std::path::PathBuf {
    std::path::PathBuf::from("templates")
}

fn create_scaffold(dir: &Path, cargo_toml: &str, lib_rs: &str) {
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join("Cargo.toml"), cargo_toml).unwrap();
    fs::write(dir.join("src/lib.rs"), lib_rs).unwrap();
}

// ─── Registry Tests ─────────────────────────────────────────────────────────

#[test]
fn test_registry_json_is_valid() {
    let path = templates_dir().join("registry.json");
    assert!(path.exists(), "registry.json must exist");

    let content = fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

    assert_eq!(parsed["version"], "1");
    assert!(parsed["templates"].is_array());

    let templates = parsed["templates"].as_array().unwrap();
    assert!(
        !templates.is_empty(),
        "Registry must contain at least one template"
    );
}

#[test]
fn test_registry_schema_is_valid_json() {
    let path = templates_dir().join("registry.schema.json");
    assert!(path.exists(), "registry.schema.json must exist");

    let content = fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(parsed.is_object());
}

#[test]
fn test_all_registry_templates_have_required_fields() {
    let path = templates_dir().join("registry.json");
    let content = fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    let templates = parsed["templates"].as_array().unwrap();

    for template in templates {
        assert!(
            template["name"].is_string() && !template["name"].as_str().unwrap().is_empty(),
            "Every template must have a non-empty 'name'"
        );
        assert!(
            template["version"].is_string() && !template["version"].as_str().unwrap().is_empty(),
            "Template '{}' must have a non-empty 'version'",
            template["name"]
        );
        assert!(
            template["description"].is_string()
                && !template["description"].as_str().unwrap().is_empty(),
            "Template '{}' must have a non-empty 'description'",
            template["name"]
        );
        assert!(
            template["source"].is_object(),
            "Template '{}' must have a 'source' object",
            template["name"]
        );
    }
}

#[test]
fn test_registry_versions_are_semver() {
    let path = templates_dir().join("registry.json");
    let content = fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    let templates = parsed["templates"].as_array().unwrap();

    for template in templates {
        let version = template["version"].as_str().unwrap();
        let parts: Vec<&str> = version.split('.').collect();
        assert_eq!(
            parts.len(),
            3,
            "Template '{}' version '{}' is not semver X.Y.Z",
            template["name"],
            version
        );
        for part in &parts {
            assert!(
                part.parse::<u32>().is_ok(),
                "Template '{}' version part '{}' is not a number",
                template["name"],
                part
            );
        }
    }
}

// ─── Real Template Tests ────────────────────────────────────────────────────

#[test]
fn test_simple_counter_template() {
    let dir = templates_dir().join("examples").join("simple-counter");
    if !dir.exists() {
        eprintln!("Skipping simple-counter: template directory not found");
        return;
    }

    let config = TestConfig::default();
    let report = test_template(&dir, &config).unwrap();

    assert!(
        report.critical_count == 0,
        "simple-counter template should have no critical findings"
    );
    assert!(
        report.quality_score >= 40,
        "simple-counter score should be >= 40, got {}",
        report.quality_score
    );
    assert_eq!(
        report.template_name, "simple-counter",
        "Report template_name should match directory"
    );
}

#[test]
fn test_escrow_template() {
    let dir = templates_dir().join("examples").join("escrow");
    if !dir.exists() {
        eprintln!("Skipping escrow: template directory not found");
        return;
    }

    let config = TestConfig::default();
    let report = test_template(&dir, &config).unwrap();

    // Escrow has external token calls — may have reentrancy warning (medium)
    assert!(
        report.critical_count == 0,
        "Escrow template should have no critical findings. Report:\n{}",
        report.summary
    );
    assert!(
        report.quality_score >= 60,
        "Escrow score should be >= 60, got {}",
        report.quality_score
    );
}

#[test]
fn test_nft_template() {
    let dir = templates_dir().join("examples").join("nft");
    if !dir.exists() {
        eprintln!("Skipping nft: template directory not found");
        return;
    }

    let config = TestConfig::default();
    let report = test_template(&dir, &config).unwrap();

    assert!(
        report.critical_count == 0,
        "NFT template should have no critical findings"
    );
}

#[test]
fn test_staking_template() {
    let dir = templates_dir().join("examples").join("staking");
    if !dir.exists() {
        eprintln!("Skipping staking: template directory not found");
        return;
    }

    let config = TestConfig::default();
    let report = test_template(&dir, &config).unwrap();

    assert!(
        report.critical_count == 0,
        "Staking template should have no critical findings"
    );
}

#[test]
fn test_all_builtin_templates() {
    let dir = templates_dir();
    if !dir.join("examples").exists() {
        eprintln!("Skipping all_builtin_templates: examples directory not found");
        return;
    }

    let config = TestConfig::default();
    let reports = test_all_templates(&dir, &config).unwrap();

    assert!(!reports.is_empty(), "Should test at least one template");

    for report in &reports {
        assert!(
            report.critical_count == 0,
            "Template '{}' has {} critical findings",
            report.template_name,
            report.critical_count
        );
    }
}

// ─── Synthetic Edge-Case Template Tests ─────────────────────────────────────

#[test]
fn test_template_missing_all_files() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("empty");
    fs::create_dir_all(&dir).unwrap();

    let config = TestConfig::default();
    let report = test_template(&dir, &config).unwrap();

    assert!(!report.passed, "Empty template should fail");
    assert!(report.critical_count > 0);
}

#[test]
fn test_template_with_invalid_cargo_toml() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("bad-toml");
    create_scaffold(
        &dir,
        "this is not valid toml {{{{",
        "#![no_std]\nuse soroban_sdk::{contract, contractimpl};\n\n#[contract]\npub struct T;\n\n#[contractimpl]\nimpl T { pub fn f() {} }\n",
    );

    let config = TestConfig::default();
    let report = test_template(&dir, &config).unwrap();

    assert!(
        report.phases.iter().any(|p| p
            .findings
            .iter()
            .any(|f| f.title.contains("not valid TOML"))),
        "Should detect invalid TOML"
    );
}

#[test]
fn test_template_with_secret_in_source() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("secret-leak");
    create_scaffold(
        &dir,
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nsoroban-sdk = \"21.0.0\"\n",
        "#![no_std]\nuse soroban_sdk::{contract, contractimpl, Env};\n\n#[contract]\npub struct T;\n\n#[contractimpl]\nimpl T {\n    pub fn get_secret_key(env: Env) {\n        let secret_key = \"sk_live_abc123\";\n        let _ = secret_key;\n    }\n}\n",
    );

    let config = TestConfig {
        run_structure: false,
        run_docs: false,
        ..TestConfig::default()
    };
    let report = test_template(&dir, &config).unwrap();

    assert!(
        report.critical_count > 0,
        "Should detect potential secret in source"
    );
}

#[test]
fn test_template_with_unprotected_state_write() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("unprotected");
    create_scaffold(
        &dir,
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nsoroban-sdk = \"21.0.0\"\n",
        "#![no_std]\nuse soroban_sdk::{contract, contractimpl, Env, symbol_short, Symbol};\n\nconst KEY: Symbol = symbol_short!(\"K\");\n\n#[contract]\npub struct T;\n\n#[contractimpl]\nimpl T {\n    pub fn write(env: Env) {\n        env.storage().instance().set(&KEY, &42u32);\n    }\n}\n",
    );

    let config = TestConfig {
        run_structure: false,
        run_docs: false,
        ..TestConfig::default()
    };
    let report = test_template(&dir, &config).unwrap();

    assert!(
        report.high_count > 0,
        "Should detect unprotected state write"
    );
}

#[test]
fn test_template_with_old_rust_edition() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("old-edition");
    create_scaffold(
        &dir,
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2018\"\n\n[dependencies]\nsoroban-sdk = \"21.0.0\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n",
        "#![no_std]\nuse soroban_sdk::{contract, contractimpl};\n\n#[contract]\npub struct T;\n\n#[contractimpl]\nimpl T { pub fn f() {} }\n",
    );

    let config = TestConfig::default();
    let report = test_template(&dir, &config).unwrap();

    assert!(
        report.phases.iter().any(|p| p
            .findings
            .iter()
            .any(|f| f.title.contains("Outdated Rust edition"))),
        "Should detect old Rust edition"
    );
}

#[test]
fn test_config_selective_phases() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("selective");
    create_scaffold(
        &dir,
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nsoroban-sdk = \"21.0.0\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n",
        "#![no_std]\nuse soroban_sdk::{contract, contractimpl};\n\n#[contract]\npub struct T;\n\n#[contractimpl]\nimpl T { pub fn f() {} }\n",
    );

    let config = TestConfig {
        run_structure: true,
        run_security: false,
        run_performance: false,
        run_compatibility: false,
        run_docs: false,
        ..TestConfig::default()
    };

    let report = test_template(&dir, &config).unwrap();
    assert_eq!(report.phases.len(), 1, "Should only run structure phase");
    assert_eq!(report.phases[0].phase, "structure_validation");
}

// ─── Report Serialization Tests ─────────────────────────────────────────────

#[test]
fn test_report_serializes_to_json() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("serialize-test");
    create_scaffold(
        &dir,
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nsoroban-sdk = \"21.0.0\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[profile.release]\noverflow-checks = true\n",
        "#![no_std]\nuse soroban_sdk::{contract, contractimpl};\n\n#[contract]\npub struct {{PROJECT_NAME_PASCAL}};\n\n#[contractimpl]\nimpl {{PROJECT_NAME_PASCAL}} { pub fn f() {} }\n",
    );

    let config = TestConfig {
        run_docs: false,
        ..TestConfig::default()
    };
    let report = test_template(&dir, &config).unwrap();

    let json = serde_json::to_string_pretty(&report).unwrap();
    let deserialized: TemplateTestReport = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.template_name, report.template_name);
    assert_eq!(deserialized.quality_score, report.quality_score);
    assert_eq!(deserialized.passed, report.passed);
    assert_eq!(deserialized.phases.len(), report.phases.len());
}

#[test]
fn test_summary_includes_all_templates() {
    let tmp = tempfile::tempdir().unwrap();
    let dir1 = tmp.path().join("a");
    let dir2 = tmp.path().join("b");

    let good_cargo = "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nsoroban-sdk = \"21.0.0\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[profile.release]\noverflow-checks = true\n";
    let good_lib = "#![no_std]\nuse soroban_sdk::{contract, contractimpl};\n\n#[contract]\npub struct {{PROJECT_NAME_PASCAL}};\n\n#[contractimpl]\nimpl {{PROJECT_NAME_PASCAL}} { pub fn f() {} }\n";

    create_scaffold(&dir1, good_cargo, good_lib);
    create_scaffold(&dir2, good_cargo, good_lib);

    let config = TestConfig::default();
    let r1 = test_template(&dir1, &config).unwrap();
    let r2 = test_template(&dir2, &config).unwrap();

    let summary = starforge::utils::ai_template_testing::generate_summary(&[r1, r2]);
    assert!(summary.contains("Templates tested:  2"));
}

// ─── Category & Severity Tests ──────────────────────────────────────────────

#[test]
fn test_all_finding_categories_are_serializable() {
    let categories = [
        FindingCategory::Structure,
        FindingCategory::Placeholder,
        FindingCategory::Security,
        FindingCategory::Performance,
        FindingCategory::Compatibility,
        FindingCategory::Documentation,
        FindingCategory::BestPractice,
    ];

    for cat in &categories {
        let json = serde_json::to_string(cat).unwrap();
        let deserialized: FindingCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(format!("{:?}", cat), format!("{:?}", deserialized));
    }
}

#[test]
fn test_severity_ordering() {
    assert!(Severity::Critical.weight() > Severity::High.weight());
    assert!(Severity::High.weight() > Severity::Medium.weight());
    assert!(Severity::Medium.weight() > Severity::Low.weight());
    assert!(Severity::Low.weight() > Severity::Info.weight());
}
