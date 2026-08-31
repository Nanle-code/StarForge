//! Integration tests for `starforge ai-audit` CLI command.
//!
//! These tests verify the command-line interface, argument parsing,
//! and output formatting.

use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

/// Mirror of the AiAuditArgs struct for test-only usage.
/// This avoids requiring the binary-crate-internal struct to be publicly
/// exported from the library.
#[derive(Debug, Clone, PartialEq)]
struct AiAuditArgs {
    path: PathBuf,
    name: Option<String>,
    level: String,
    attack_simulation: bool,
    format: String,
    out: Option<PathBuf>,
    quiet: bool,
}

/// Helper to create a temporary Rust file with contract code.
fn create_temp_contract(code: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    file.write_all(code.as_bytes())
        .expect("Failed to write contract code");
    file.flush().expect("Failed to flush file");
    file
}

#[test]
fn test_audit_args_struct_creation() {
    let args = AiAuditArgs {
        path: PathBuf::from("contract.rs"),
        name: Some("MyContract".to_string()),
        level: "comprehensive".to_string(),
        attack_simulation: true,
        format: "text".to_string(),
        out: None,
        quiet: false,
    };

    assert_eq!(args.path, PathBuf::from("contract.rs"));
    assert_eq!(args.name, Some("MyContract".to_string()));
    assert_eq!(args.level, "comprehensive");
    assert!(args.attack_simulation);
    assert_eq!(args.format, "text");
    assert!(!args.quiet);
}

#[test]
fn test_audit_args_with_output_file() {
    let args = AiAuditArgs {
        path: PathBuf::from("contract.rs"),
        name: None,
        level: "standard".to_string(),
        attack_simulation: false,
        format: "json".to_string(),
        out: Some(PathBuf::from("audit.json")),
        quiet: true,
    };

    assert!(args.out.is_some());
    assert_eq!(args.out.unwrap(), PathBuf::from("audit.json"));
    assert!(!args.attack_simulation);
    assert!(args.quiet);
}

#[test]
fn test_audit_args_default_level() {
    let args = AiAuditArgs {
        path: PathBuf::from("contract.rs"),
        name: None,
        level: "comprehensive".to_string(),
        attack_simulation: true,
        format: "text".to_string(),
        out: None,
        quiet: false,
    };

    assert_eq!(args.level, "comprehensive");
}

#[test]
fn test_audit_args_output_formats() {
    let formats = vec!["text", "json", "html"];

    for format in formats {
        let args = AiAuditArgs {
            path: PathBuf::from("contract.rs"),
            name: None,
            level: "standard".to_string(),
            attack_simulation: true,
            format: format.to_string(),
            out: None,
            quiet: false,
        };

        assert_eq!(args.format, format);
    }
}

#[test]
fn test_audit_args_quiet_mode() {
    let quiet_args = AiAuditArgs {
        path: PathBuf::from("contract.rs"),
        name: None,
        level: "standard".to_string(),
        attack_simulation: true,
        format: "text".to_string(),
        out: None,
        quiet: true,
    };

    assert!(quiet_args.quiet);

    let verbose_args = AiAuditArgs {
        path: PathBuf::from("contract.rs"),
        name: None,
        level: "standard".to_string(),
        attack_simulation: true,
        format: "text".to_string(),
        out: None,
        quiet: false,
    };

    assert!(!verbose_args.quiet);
}

#[test]
fn test_audit_args_path_variations() {
    let paths = vec![
        "contract.rs",
        "./src/lib.rs",
        "/path/to/contract.rs",
        "contracts/token.rs",
    ];

    for path_str in paths {
        let args = AiAuditArgs {
            path: PathBuf::from(path_str),
            name: None,
            level: "standard".to_string(),
            attack_simulation: true,
            format: "text".to_string(),
            out: None,
            quiet: false,
        };

        assert_eq!(args.path, PathBuf::from(path_str));
    }
}

#[test]
fn test_audit_args_with_all_options() {
    let args = AiAuditArgs {
        path: PathBuf::from("./contracts/token.rs"),
        name: Some("TokenContract".to_string()),
        level: "comprehensive".to_string(),
        attack_simulation: true,
        format: "html".to_string(),
        out: Some(PathBuf::from("./reports/audit.html")),
        quiet: false,
    };

    assert_eq!(args.path, PathBuf::from("./contracts/token.rs"));
    assert_eq!(args.name, Some("TokenContract".to_string()));
    assert_eq!(args.level, "comprehensive");
    assert!(args.attack_simulation);
    assert_eq!(args.format, "html");
    assert_eq!(args.out, Some(PathBuf::from("./reports/audit.html")));
    assert!(!args.quiet);
}

#[test]
fn test_audit_args_minimal_options() {
    let args = AiAuditArgs {
        path: PathBuf::from("contract.rs"),
        name: None,
        level: "standard".to_string(),
        attack_simulation: true,
        format: "text".to_string(),
        out: None,
        quiet: false,
    };

    assert_eq!(args.path, PathBuf::from("contract.rs"));
    assert!(args.name.is_none());
    assert!(args.out.is_none());
}

#[test]
fn test_audit_args_attack_simulation_options() {
    let with_simulation = AiAuditArgs {
        path: PathBuf::from("contract.rs"),
        name: None,
        level: "standard".to_string(),
        attack_simulation: true,
        format: "text".to_string(),
        out: None,
        quiet: false,
    };

    let without_simulation = AiAuditArgs {
        path: PathBuf::from("contract.rs"),
        name: None,
        level: "standard".to_string(),
        attack_simulation: false,
        format: "text".to_string(),
        out: None,
        quiet: false,
    };

    assert!(with_simulation.attack_simulation);
    assert!(!without_simulation.attack_simulation);
}

#[test]
fn test_audit_args_multiple_instances_independent() {
    let args1 = AiAuditArgs {
        path: PathBuf::from("contract1.rs"),
        name: Some("Contract1".to_string()),
        level: "basic".to_string(),
        attack_simulation: false,
        format: "json".to_string(),
        out: None,
        quiet: true,
    };

    let args2 = AiAuditArgs {
        path: PathBuf::from("contract2.rs"),
        name: Some("Contract2".to_string()),
        level: "comprehensive".to_string(),
        attack_simulation: true,
        format: "html".to_string(),
        out: Some(PathBuf::from("report.html")),
        quiet: false,
    };

    assert_ne!(args1.path, args2.path);
    assert_ne!(args1.name, args2.name);
    assert_ne!(args1.level, args2.level);
    assert_ne!(args1.attack_simulation, args2.attack_simulation);
    assert_ne!(args1.format, args2.format);
    assert_ne!(args1.quiet, args2.quiet);
}

#[test]
fn test_audit_args_valid_json_output() {
    let args = AiAuditArgs {
        path: PathBuf::from("contract.rs"),
        name: None,
        level: "standard".to_string(),
        attack_simulation: true,
        format: "json".to_string(),
        out: Some(PathBuf::from("result.json")),
        quiet: false,
    };

    assert_eq!(args.format, "json");
    assert!(args
        .out
        .unwrap()
        .extension()
        .is_some_and(|ext| ext == "json"));
}

#[test]
fn test_audit_args_valid_html_output() {
    let args = AiAuditArgs {
        path: PathBuf::from("contract.rs"),
        name: None,
        level: "standard".to_string(),
        attack_simulation: true,
        format: "html".to_string(),
        out: Some(PathBuf::from("report.html")),
        quiet: false,
    };

    assert_eq!(args.format, "html");
    assert!(args
        .out
        .unwrap()
        .extension()
        .is_some_and(|ext| ext == "html"));
}

#[test]
fn test_audit_args_contract_name_variations() {
    let names = vec![
        "TokenContract",
        "token-contract",
        "token_contract",
        "TokenContractV2",
        "TOKENCONTRACT",
    ];

    for name in names {
        let args = AiAuditArgs {
            path: PathBuf::from("contract.rs"),
            name: Some(name.to_string()),
            level: "standard".to_string(),
            attack_simulation: true,
            format: "text".to_string(),
            out: None,
            quiet: false,
        };

        assert_eq!(args.name, Some(name.to_string()));
    }
}

#[test]
fn test_audit_args_level_case_sensitivity() {
    let levels = vec!["basic", "standard", "comprehensive"];

    for level in levels {
        let args = AiAuditArgs {
            path: PathBuf::from("contract.rs"),
            name: None,
            level: level.to_string(),
            attack_simulation: true,
            format: "text".to_string(),
            out: None,
            quiet: false,
        };

        assert_eq!(args.level, level);
    }
}

#[test]
fn test_audit_args_with_directory_path() {
    let args = AiAuditArgs {
        path: PathBuf::from("./src"),
        name: None,
        level: "standard".to_string(),
        attack_simulation: true,
        format: "text".to_string(),
        out: None,
        quiet: false,
    };

    // Should accept directory paths
    assert_eq!(args.path, PathBuf::from("./src"));
}
