//! Integration tests for man page generation and the `starforge man` subcommand.
//!
//! NOTE: The starforge binary has a pre-existing stack overflow issue (60+ deep
//! subcommand tree exceeds the default Windows stack). These tests validate
//! build-time output and content quality using in-process assertions.

use std::path::Path;

// ── Build-time man page existence ───────────────────────────────────────────

#[test]
fn build_script_generates_main_man_page() {
    let man_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("man");
    assert!(
        man_dir.join("starforge.1").exists(),
        "man/starforge.1 must exist after build"
    );
}

#[test]
fn build_script_generates_subcommand_man_pages() {
    let man_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("man");
    for name in &[
        "wallet",
        "deploy",
        "network",
        "config",
        "template",
        "plugin",
        "test",
        "gas",
        "benchmark",
        "tutorial",
        "debug",
        "inspect",
        "contract",
        "new",
        "info",
        "upgrade",
        "security",
        "perf",
        "docs",
        "analytics",
    ] {
        let page = man_dir.join(format!("starforge-{}.1", name));
        assert!(page.exists(), "man/starforge-{}.1 must exist", name);
    }
}

#[test]
fn man_pages_are_non_empty_roff() {
    let man_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("man");
    for name in &["starforge.1", "starforge-wallet.1", "starforge-deploy.1"] {
        let path = man_dir.join(name);
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
        assert!(
            content.len() > 100,
            "{} is suspiciously small ({} bytes)",
            name,
            content.len()
        );
        assert!(
            content.contains(".SH"),
            "{} must contain roff section headers",
            name
        );
    }
}

#[test]
fn main_man_page_has_required_sections() {
    let man_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("man");
    let content = std::fs::read_to_string(man_dir.join("starforge.1")).expect("read starforge.1");
    assert!(content.contains(".SH NAME"), "must contain NAME section");
    assert!(
        content.contains(".SH SYNOPSIS"),
        "must contain SYNOPSIS section"
    );
    assert!(
        content.contains(".SH DESCRIPTION"),
        "must contain DESCRIPTION section"
    );
    assert!(
        content.contains("starforge"),
        "must mention the binary name"
    );
}

#[test]
fn subcommand_man_pages_reference_full_name() {
    let man_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("man");
    for name in &["wallet", "deploy", "network", "config"] {
        let path = man_dir.join(format!("starforge-{}.1", name));
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
        assert!(
            content.contains(&format!("starforge-{}", name)),
            "starforge-{}.1 must reference 'starforge-{}' in NAME",
            name,
            name
        );
        assert!(
            content.contains(".SH NAME"),
            "starforge-{}.1 must contain .SH NAME",
            name
        );
    }
}

#[test]
fn man_pages_directory_has_expected_count() {
    let man_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("man");
    let count = std::fs::read_dir(&man_dir)
        .expect("read man/")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext == "1")
                .unwrap_or(false)
        })
        .count();
    assert!(
        count >= 50,
        "expected at least 50 man pages, found {}",
        count
    );
}

// ── Unit tests via the man module's public read_man_page API ────────────────
// These tests exercise the same code path as `starforge man generate` without
// spawning the full binary (which has a pre-existing stack overflow).

#[test]
fn unit_read_main_man_page() {
    let result = starforge::commands::man::read_man_page("starforge");
    assert!(
        result.is_ok(),
        "should read main man page: {:?}",
        result.err()
    );
    let contents = result.unwrap();
    assert!(contents.contains(".SH NAME"), "must contain NAME section");
    assert!(contents.contains("starforge"), "must mention binary name");
}

#[test]
fn unit_read_wallet_man_page() {
    let result = starforge::commands::man::read_man_page("wallet");
    assert!(
        result.is_ok(),
        "should read wallet man page: {:?}",
        result.err()
    );
    let contents = result.unwrap();
    assert!(
        contents.contains("starforge-wallet"),
        "must reference starforge-wallet"
    );
}

#[test]
fn unit_read_invalid_page_errors() {
    let result = starforge::commands::man::read_man_page("nonexistent-cmd-xyz");
    assert!(result.is_err(), "should fail for unknown page");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Unknown man page"),
        "error must mention unknown page, got: {}",
        err_msg
    );
}
