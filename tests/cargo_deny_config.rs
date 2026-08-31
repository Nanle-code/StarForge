//! Integration tests for cargo-deny supply-chain policy configuration.
//!
//! These tests validate that `deny.toml` exists, is well-formed, covers
//! the four required policy areas (advisories, licenses, bans, sources),
//! and that the CI workflow correctly invokes cargo-deny.
//!
//! Run with: `cargo test --test cargo_deny_config`

use std::fs;
use std::path::Path;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read the project-root `deny.toml` and panic with a clear message when
/// the file is missing.
fn read_deny_toml() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("deny.toml");
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read deny.toml at {}: {e}", path.display()))
}

/// Read an arbitrary file relative to the project root, returning `None`
/// when the file does not exist.
fn read_project_file(relative: &str) -> Option<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    fs::read_to_string(path).ok()
}

// ---------------------------------------------------------------------------
// Primary flow – deny.toml is valid and covers all required sections
// ---------------------------------------------------------------------------

#[test]
fn deny_toml_exists_and_is_valid_toml() {
    let content = read_deny_toml();
    // A bare parse check: every TOML document must parse without panicking.
    // We use toml::Value because the `toml` crate is already a dependency.
    let parsed: toml::Value =
        toml::from_str(&content).expect("deny.toml is not valid TOML – fix syntax errors");
    assert!(
        parsed.is_table(),
        "deny.toml must be a TOML table at the top level"
    );
}

#[test]
fn deny_toml_has_advisories_section() {
    let content = read_deny_toml();
    let parsed: toml::Value = toml::from_str(&content).expect("invalid TOML");
    let table = parsed.as_table().expect("top-level must be a table");
    assert!(
        table.contains_key("advisories"),
        "deny.toml must contain an [advisories] section"
    );
    let advisories = table["advisories"]
        .as_table()
        .expect("[advisories] must be a table");
    assert!(
        advisories.contains_key("version"),
        "[advisories] must specify a version"
    );
    // Verify ignore list exists and is documented
    if let Some(ignore) = advisories.get("ignore") {
        assert!(
            ignore.is_array(),
            "[advisories].ignore must be an array of RUSTSEC IDs"
        );
    }
}

#[test]
fn deny_toml_has_licenses_section() {
    let content = read_deny_toml();
    let parsed: toml::Value = toml::from_str(&content).expect("invalid TOML");
    let table = parsed.as_table().expect("top-level must be a table");
    assert!(
        table.contains_key("licenses"),
        "deny.toml must contain a [licenses] section"
    );
    let licenses = table["licenses"]
        .as_table()
        .expect("[licenses] must be a table");
    assert!(
        licenses.contains_key("allow"),
        "[licenses] must define an allow-list of accepted licenses"
    );
    let allow = licenses["allow"]
        .as_array()
        .expect("[licenses].allow must be an array");
    assert!(
        !allow.is_empty(),
        "license allow-list must not be empty – at least MIT should be present"
    );
    // Sanity: MIT is the project's own license
    let license_strs: Vec<String> = allow
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    assert!(
        license_strs.contains(&"MIT".to_string()),
        "license allow-list must include MIT (the project's own license)"
    );
}

#[test]
fn deny_toml_has_bans_section() {
    let content = read_deny_toml();
    let parsed: toml::Value = toml::from_str(&content).expect("invalid TOML");
    let table = parsed.as_table().expect("top-level must be a table");
    assert!(
        table.contains_key("bans"),
        "deny.toml must contain a [bans] section"
    );
    let bans = table["bans"].as_table().expect("[bans] must be a table");
    assert!(
        bans.contains_key("multiple-versions"),
        "[bans] must configure multiple-versions policy"
    );
}

#[test]
fn deny_toml_has_sources_section() {
    let content = read_deny_toml();
    let parsed: toml::Value = toml::from_str(&content).expect("invalid TOML");
    let table = parsed.as_table().expect("top-level must be a table");
    assert!(
        table.contains_key("sources"),
        "deny.toml must contain a [sources] section"
    );
    let sources = table["sources"]
        .as_table()
        .expect("[sources] must be a table");
    assert!(
        sources.contains_key("unknown-registry"),
        "[sources] must deny unknown registries"
    );
    assert!(
        sources.contains_key("unknown-git"),
        "[sources] must deny unknown git sources"
    );
    // The deny-level should be "deny" (not "warn" or "allow")
    assert_eq!(
        sources["unknown-registry"].as_str(),
        Some("deny"),
        "unknown-registry must be set to \"deny\""
    );
    assert_eq!(
        sources["unknown-git"].as_str(),
        Some("deny"),
        "unknown-git must be set to \"deny\""
    );
}

// ---------------------------------------------------------------------------
// Boundary case – crates.io is explicitly allowed as the only registry
// ---------------------------------------------------------------------------

#[test]
fn sources_allow_list_includes_crates_io() {
    let content = read_deny_toml();
    let parsed: toml::Value = toml::from_str(&content).expect("invalid TOML");
    let sources = parsed["sources"].as_table().expect("[sources] must exist");
    if let Some(allow_reg) = sources.get("allow-registry") {
        let regs: Vec<String> = allow_reg
            .as_array()
            .expect("allow-registry must be an array")
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        assert!(
            regs.iter().any(|r| r.contains("crates.io")),
            "allow-registry must include the crates.io index"
        );
    }
    // No unknown git sources should be allowed
    if let Some(allow_git) = sources.get("allow-git") {
        let git: Vec<String> = allow_git
            .as_array()
            .expect("allow-git must be an array")
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        assert!(
            git.is_empty(),
            "allow-git should be empty – no git dependencies are used"
        );
    }
}

// ---------------------------------------------------------------------------
// Boundary case – advisory ignores are documented with rationale comments
// ---------------------------------------------------------------------------

#[test]
fn advisory_ignores_are_documented() {
    let content = read_deny_toml();
    let parsed: toml::Value = toml::from_str(&content).expect("invalid TOML");
    let advisories = parsed["advisories"]
        .as_table()
        .expect("[advisories] must exist");
    if let Some(ignore) = advisories.get("ignore") {
        let entries = ignore.as_array().expect("ignore must be an array").len();
        // If there are ignored advisories, ensure there are corresponding
        // comment lines explaining each one.  We check that the raw text
        // contains at least as many "#" comment lines before the ignore
        // block as there are entries.
        if entries > 0 {
            let comment_count = content
                .lines()
                .filter(|l| l.trim_start().starts_with('#'))
                .count();
            assert!(
                comment_count >= entries,
                "each ignored advisory should have a rationale comment above it; \
                 found {entries} ignore entries but only {comment_count} comment lines"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Failure case – a malformed deny.toml must be rejected
// ---------------------------------------------------------------------------

#[test]
fn malformed_toml_fails_parsing() {
    let bad_toml = r#"
[advisories]
version = 2
ignore = [
    # missing closing bracket
    "RUSTSEC-2024-0388"
"#;
    let result = toml::from_str::<toml::Value>(bad_toml);
    assert!(result.is_err(), "malformed deny.toml must fail to parse");
}

#[test]
fn deny_toml_licenses_must_not_be_empty() {
    let content = read_deny_toml();
    let parsed: toml::Value = toml::from_str(&content).expect("invalid TOML");
    let licenses = &parsed["licenses"];
    let allow = licenses["allow"]
        .as_array()
        .expect("[licenses].allow must be an array");
    assert!(!allow.is_empty(), "license allow-list must not be empty");
}

// ---------------------------------------------------------------------------
// CI workflow validation
// ---------------------------------------------------------------------------

#[test]
fn ci_workflow_contains_cargo_deny_job() {
    let ci = read_project_file(".github/workflows/ci.yml")
        .expect("CI workflow .github/workflows/ci.yml must exist");
    assert!(
        ci.contains("cargo-deny") || ci.contains("cargo deny") || ci.contains("Cargo Deny"),
        "CI workflow must reference cargo-deny"
    );
}

#[test]
fn ci_workflow_does_not_swallow_deny_failures() {
    let ci = read_project_file(".github/workflows/ci.yml")
        .expect("CI workflow .github/workflows/ci.yml must exist");
    // Find the deny job section
    let deny_section_start = ci.find("Cargo Deny").expect("deny job not found in CI");
    // Extract until the next job (next top-level key after "  name:")
    let deny_section = &ci[deny_section_start..];
    assert!(
        !deny_section.contains("continue-on-error: true"),
        "cargo-deny CI job must not use continue-on-error: true"
    );
}

#[test]
fn ci_workflow_deny_job_uses_official_action() {
    let ci = read_project_file(".github/workflows/ci.yml")
        .expect("CI workflow .github/workflows/ci.yml must exist");
    assert!(
        ci.contains("EmbarkStudios/cargo-deny-action"),
        "CI should use the official EmbarkStudios/cargo-deny-action"
    );
}
