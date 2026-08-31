//! End-to-end tests for the AI developer-workflow commands.
//!
//! Covers the four features wired in this change:
//!
//! * `ai-profile`           — performance profiling (#559)
//! * `ai-ide`               — IDE integration (#554)
//! * `ai-test-maintain`     — test maintenance (#566)
//! * `ai-deployment-test`   — deployment testing (#547)
//!
//! These exercise the binary rather than the library, so they catch CLI wiring
//! mistakes (missing subcommand registration, bad flag names, JSON that does not
//! round-trip) that unit tests cannot see.

use std::path::Path;
use std::process::Command;

fn starforge(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_starforge"));
    cmd.arg("-q");
    cmd.env("HOME", home);
    cmd.env("USERPROFILE", home);
    cmd
}

fn assert_success(output: &std::process::Output, label: &str) {
    assert!(
        output.status.success(),
        "{label} failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Minimal but structurally valid WASM module with recognisable symbol names.
fn write_wasm(dir: &Path) -> std::path::PathBuf {
    let mut bytes = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    for name in ["transfer_tokens", "read_balance", "initialize_contract"] {
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(0);
    }
    bytes.extend(vec![0u8; 4096]);

    let path = dir.join("contract.wasm");
    std::fs::write(&path, bytes).expect("write wasm fixture");
    path
}

/// A tiny contract plus a partial test suite, for the maintenance commands.
fn write_project(dir: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let src = dir.join("src");
    let tests = dir.join("tests");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&tests).unwrap();

    std::fs::write(
        src.join("lib.rs"),
        r#"
#[contractimpl]
impl Counter {
    pub fn increment(env: Env, by: u32) -> u32 { 0 }
    pub fn reset(env: Env) {}
    pub fn balance_of(env: Env, owner: Address) -> i128 { 0 }
}
"#,
    )
    .unwrap();

    std::fs::write(
        tests.join("counter.rs"),
        r#"
#[test]
fn test_increment() {
    let env = Env::default();
    let result = client.increment(&env, 1);
    assert_eq!(result, 1);
}
"#,
    )
    .unwrap();

    (src, tests)
}

// ─────────────────────────────────────────────────────────────────────────────
// #559 — AI performance profiling
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn ai_profile_run_emits_a_complete_json_profile() {
    let home = tempfile::tempdir().unwrap();
    let wasm = write_wasm(home.path());

    let output = starforge(home.path())
        .args([
            "ai-profile",
            "run",
            "--wasm",
            wasm.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("spawn ai-profile run");
    assert_success(&output, "starforge ai-profile run");

    let profile: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("profile output must be valid JSON");

    assert!(profile["profile_id"].as_str().is_some());
    assert!(profile["summary"]["performance_score"].as_f64().is_some());
    assert!(
        !profile["functions"].as_array().unwrap().is_empty(),
        "expected at least one profiled function"
    );
}

#[test]
fn ai_profile_is_deterministic_for_the_same_artefact() {
    let home = tempfile::tempdir().unwrap();
    let wasm = write_wasm(home.path());

    let run = || {
        let output = starforge(home.path())
            .args([
                "ai-profile",
                "run",
                "--wasm",
                wasm.to_str().unwrap(),
                "--json",
            ])
            .output()
            .expect("spawn ai-profile run");
        assert_success(&output, "starforge ai-profile run");
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };

    assert_eq!(run()["profile_id"], run()["profile_id"]);
}

#[test]
fn ai_profile_compare_detects_no_regression_against_itself() {
    let home = tempfile::tempdir().unwrap();
    let wasm = write_wasm(home.path());
    let saved = home.path().join("baseline.json");

    let output = starforge(home.path())
        .args([
            "ai-profile",
            "run",
            "--wasm",
            wasm.to_str().unwrap(),
            "--save",
            saved.to_str().unwrap(),
        ])
        .output()
        .expect("spawn ai-profile run --save");
    assert_success(&output, "starforge ai-profile run --save");
    assert!(saved.exists(), "--save must write the profile");

    let output = starforge(home.path())
        .args([
            "ai-profile",
            "compare",
            "--baseline",
            saved.to_str().unwrap(),
            "--candidate",
            wasm.to_str().unwrap(),
            "--fail-on-regression",
            "--json",
        ])
        .output()
        .expect("spawn ai-profile compare");
    assert_success(&output, "starforge ai-profile compare");

    let comparison: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(comparison["is_regression"], serde_json::json!(false));
}

#[test]
fn ai_profile_rejects_a_missing_artefact() {
    let home = tempfile::tempdir().unwrap();
    let output = starforge(home.path())
        .args(["ai-profile", "run", "--wasm", "/nonexistent/contract.wasm"])
        .output()
        .expect("spawn ai-profile run");
    assert!(!output.status.success(), "missing artefact must fail");
}

// ─────────────────────────────────────────────────────────────────────────────
// #554 — AI IDE integration
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn ai_ide_setup_writes_editor_configuration() {
    let home = tempfile::tempdir().unwrap();
    let project = home.path().join("project");
    std::fs::create_dir_all(&project).unwrap();

    let output = starforge(home.path())
        .args([
            "ai-ide",
            "setup",
            "--ide",
            "vscode",
            "--path",
            project.to_str().unwrap(),
        ])
        .output()
        .expect("spawn ai-ide setup");
    assert_success(&output, "starforge ai-ide setup");

    for relative in [
        ".vscode/tasks.json",
        ".vscode/settings.json",
        ".vscode/starforge.code-snippets",
    ] {
        let path = project.join(relative);
        assert!(path.exists(), "expected {relative} to be written");
        let contents = std::fs::read_to_string(&path).unwrap();
        serde_json::from_str::<serde_json::Value>(&contents)
            .unwrap_or_else(|e| panic!("{relative} is not valid JSON: {e}"));
    }
}

#[test]
fn ai_ide_setup_dry_run_writes_nothing() {
    let home = tempfile::tempdir().unwrap();
    let project = home.path().join("project");
    std::fs::create_dir_all(&project).unwrap();

    let output = starforge(home.path())
        .args([
            "ai-ide",
            "setup",
            "--ide",
            "zed",
            "--path",
            project.to_str().unwrap(),
            "--dry-run",
        ])
        .output()
        .expect("spawn ai-ide setup --dry-run");
    assert_success(&output, "starforge ai-ide setup --dry-run");
    assert!(!project.join(".zed").exists(), "dry run must not write");
}

#[test]
fn ai_ide_setup_rejects_an_unknown_editor() {
    let home = tempfile::tempdir().unwrap();
    let output = starforge(home.path())
        .args(["ai-ide", "setup", "--ide", "notepad"])
        .output()
        .expect("spawn ai-ide setup");
    assert!(!output.status.success(), "unknown editor must fail");
}

#[test]
fn ai_ide_request_reports_diagnostics_as_json() {
    let home = tempfile::tempdir().unwrap();
    let source = home.path().join("contract.rs");
    std::fs::write(
        &source,
        "pub fn withdraw(env: Env, amount: i128) {\n    let b = store.get(&KEY).unwrap();\n}\n",
    )
    .unwrap();

    let output = starforge(home.path())
        .args([
            "ai-ide",
            "request",
            "--kind",
            "diagnostics",
            "--file",
            source.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("spawn ai-ide request");
    assert_success(&output, "starforge ai-ide request");

    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["type"], serde_json::json!("diagnostics"));

    let diagnostics = response["diagnostics"].as_array().unwrap();
    assert!(
        diagnostics
            .iter()
            .any(|d| d["code"] == serde_json::json!("unwrap_in_contract")),
        "expected the unwrap lint to fire: {diagnostics:?}"
    );
}

#[test]
fn ai_ide_list_reports_every_supported_editor() {
    let home = tempfile::tempdir().unwrap();
    let output = starforge(home.path())
        .args(["ai-ide", "list", "--json"])
        .output()
        .expect("spawn ai-ide list");
    assert_success(&output, "starforge ai-ide list");

    let listed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let keys: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["ide"].as_str().unwrap())
        .collect();

    for expected in ["vscode", "intellij", "neovim", "zed"] {
        assert!(keys.contains(&expected), "missing {expected} in {keys:?}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// #566 — AI test maintenance
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn ai_test_maintain_reports_coverage_gaps() {
    let home = tempfile::tempdir().unwrap();
    let (src, tests) = write_project(home.path());

    let output = starforge(home.path())
        .args([
            "ai-test-maintain",
            "analyze",
            "--source",
            src.to_str().unwrap(),
            "--tests",
            tests.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("spawn ai-test-maintain analyze");
    assert_success(&output, "starforge ai-test-maintain analyze");

    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["contract_functions"], serde_json::json!(3));
    assert_eq!(report["test_cases"], serde_json::json!(1));

    let gaps: Vec<&str> = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["kind"] == serde_json::json!("coverage_gap"))
        .map(|f| f["subject"].as_str().unwrap())
        .collect();
    assert!(gaps.contains(&"reset"), "got {gaps:?}");
    assert!(gaps.contains(&"balance_of"), "got {gaps:?}");
}

#[test]
fn ai_test_maintain_does_not_flag_a_correct_call_as_drift() {
    let home = tempfile::tempdir().unwrap();
    let (src, tests) = write_project(home.path());

    let output = starforge(home.path())
        .args([
            "ai-test-maintain",
            "analyze",
            "--source",
            src.to_str().unwrap(),
            "--tests",
            tests.to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("spawn ai-test-maintain analyze");
    assert_success(&output, "starforge ai-test-maintain analyze");

    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let drift: Vec<_> = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["kind"] == serde_json::json!("signature_drift"))
        .collect();
    assert!(
        drift.is_empty(),
        "client.increment(&env, 1) matches the declared arity: {drift:?}"
    );
}

#[test]
fn ai_test_maintain_generates_stubs_for_uncovered_functions() {
    let home = tempfile::tempdir().unwrap();
    let (src, tests) = write_project(home.path());

    let output = starforge(home.path())
        .args([
            "ai-test-maintain",
            "suggest",
            "--source",
            src.to_str().unwrap(),
            "--tests",
            tests.to_str().unwrap(),
        ])
        .output()
        .expect("spawn ai-test-maintain suggest");
    assert_success(&output, "starforge ai-test-maintain suggest");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("fn test_reset"), "got: {stdout}");
    assert!(stdout.contains("fn test_balance_of"), "got: {stdout}");
}

#[test]
fn ai_test_maintain_min_health_gate_fails_a_weak_suite() {
    let home = tempfile::tempdir().unwrap();
    let (src, tests) = write_project(home.path());

    let output = starforge(home.path())
        .args([
            "ai-test-maintain",
            "analyze",
            "--source",
            src.to_str().unwrap(),
            "--tests",
            tests.to_str().unwrap(),
            "--min-health",
            "95",
        ])
        .output()
        .expect("spawn ai-test-maintain analyze");
    assert!(
        !output.status.success(),
        "a suite at 33% coverage must not pass a 95 health gate"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// #547 — AI deployment testing
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn ai_deployment_test_passes_a_healthy_artefact() {
    let home = tempfile::tempdir().unwrap();
    let wasm = write_wasm(home.path());

    let output = starforge(home.path())
        .args([
            "ai-deployment-test",
            "run",
            "--wasm",
            wasm.to_str().unwrap(),
            "--phase",
            "pre",
            "--json",
        ])
        .output()
        .expect("spawn ai-deployment-test run");
    assert_success(&output, "starforge ai-deployment-test run");

    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["should_proceed"], serde_json::json!(true));
    assert_eq!(report["failed"], serde_json::json!(0));
}

#[test]
fn ai_deployment_test_gate_blocks_a_broken_artefact() {
    let home = tempfile::tempdir().unwrap();
    let wasm = home.path().join("broken.wasm");
    std::fs::write(&wasm, b"this is not a wasm module").unwrap();

    let output = starforge(home.path())
        .args([
            "ai-deployment-test",
            "run",
            "--wasm",
            wasm.to_str().unwrap(),
            "--phase",
            "pre",
            "--gate",
        ])
        .output()
        .expect("spawn ai-deployment-test run --gate");
    assert!(
        !output.status.success(),
        "--gate must exit non-zero when a blocking check fails"
    );
}

#[test]
fn ai_deployment_test_skips_post_checks_without_a_contract_id() {
    let home = tempfile::tempdir().unwrap();
    let wasm = write_wasm(home.path());

    let output = starforge(home.path())
        .args([
            "ai-deployment-test",
            "run",
            "--wasm",
            wasm.to_str().unwrap(),
            "--phase",
            "post",
            "--json",
        ])
        .output()
        .expect("spawn ai-deployment-test run");
    assert_success(&output, "starforge ai-deployment-test run");

    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["skipped"], serde_json::json!(1));
}

#[test]
fn ai_deployment_test_verifies_a_deployed_contract() {
    let home = tempfile::tempdir().unwrap();
    let wasm = write_wasm(home.path());
    let contract_id = format!("C{}", "A".repeat(55));

    let output = starforge(home.path())
        .args([
            "ai-deployment-test",
            "run",
            "--wasm",
            wasm.to_str().unwrap(),
            "--phase",
            "post",
            "--contract-id",
            &contract_id,
            "--verified",
            "--json",
        ])
        .output()
        .expect("spawn ai-deployment-test run");
    assert_success(&output, "starforge ai-deployment-test run");

    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["should_proceed"], serde_json::json!(true));
    assert_eq!(report["failed"], serde_json::json!(0));
}

#[test]
fn ai_deployment_test_arms_extra_triggers_on_mainnet() {
    let home = tempfile::tempdir().unwrap();

    let count_for = |network: &str| {
        let output = starforge(home.path())
            .args([
                "ai-deployment-test",
                "triggers",
                "--network",
                network,
                "--json",
            ])
            .output()
            .expect("spawn ai-deployment-test triggers");
        assert_success(&output, "starforge ai-deployment-test triggers");
        serde_json::from_slice::<serde_json::Value>(&output.stdout)
            .unwrap()
            .as_array()
            .unwrap()
            .len()
    };

    assert!(
        count_for("mainnet") > count_for("testnet"),
        "mainnet must arm at least one additional rollback trigger"
    );
}
