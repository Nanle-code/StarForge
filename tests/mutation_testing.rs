//! CLI integration tests for `starforge mutate` (AI mutation testing).
//!
//! The `run` subcommand is exercised with a trivial shell command instead of a
//! real Rust test suite, which keeps these tests fast and network-free while
//! still covering the execution + scoring + CI-gating paths end to end.

use std::process::Command;

fn isolated_home() -> tempfile::TempDir {
    tempfile::tempdir().expect("create isolated home")
}

fn starforge(home: &std::path::Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_starforge"));
    cmd.arg("-q");
    cmd.env("STARFORGE_HOME", home);
    cmd.env("USERPROFILE", home);
    cmd
}

fn assert_success(output: &std::process::Output, cmd: &str) {
    assert!(
        output.status.success(),
        "{} failed: {}",
        cmd,
        String::from_utf8_lossy(&output.stderr)
    );
}

const CONTRACT: &str = r#"pub fn withdraw(caller: Address, amount: i128, balance: i128) -> i128 {
    caller.require_auth();
    if amount > balance {
        panic!("insufficient");
    }
    let remaining = balance - amount;
    remaining
}
"#;

fn write_contract(home: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
    let path = home.join(name);
    std::fs::write(&path, body).expect("write contract");
    path
}

// The runner already wraps --test-command in `cmd /C` (Windows) or `sh -c`
// (elsewhere), so these are bare shell commands.

/// A command that always succeeds — every mutant "survives".
fn always_pass() -> &'static str {
    if cfg!(windows) {
        "exit 0"
    } else {
        "true"
    }
}

/// A command that always fails — every mutant is "killed".
fn always_fail() -> &'static str {
    if cfg!(windows) {
        "exit 1"
    } else {
        "false"
    }
}

#[test]
fn mutate_help_lists_subcommands() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args(["mutate", "--help"])
        .output()
        .expect("spawn mutate help");
    assert_success(&output, "starforge mutate --help");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("generate"));
    assert!(stdout.contains("run"));
    assert!(stdout.contains("operators"));
    assert!(stdout.contains("ci-workflow"));
}

#[test]
fn operators_lists_strategies() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args(["mutate", "operators"])
        .output()
        .expect("spawn operators");
    assert_success(&output, "starforge mutate operators");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("comparison"));
    assert!(stdout.contains("require-auth"));
    assert!(stdout.contains("storage-durability"));
}

#[test]
fn generate_produces_mutants_json() {
    let home = isolated_home();
    let path = write_contract(home.path(), "contract.rs", CONTRACT);
    let output = starforge(home.path())
        .args(["mutate", "generate", path.to_str().unwrap(), "--json"])
        .output()
        .expect("spawn generate");
    assert_success(&output, "starforge mutate generate");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"mutants\""));
    assert!(stdout.contains("require-auth"));
    assert!(stdout.contains("comparison"));
}

#[test]
fn generate_respects_operator_filter() {
    let home = isolated_home();
    let path = write_contract(home.path(), "contract.rs", CONTRACT);
    let output = starforge(home.path())
        .args([
            "mutate",
            "generate",
            path.to_str().unwrap(),
            "--operators",
            "require-auth",
            "--json",
        ])
        .output()
        .expect("spawn generate filtered");
    assert_success(&output, "starforge mutate generate --operators");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("require-auth"));
    assert!(
        !stdout.contains("\"comparison\""),
        "filter should exclude other operators"
    );
}

#[test]
fn generate_rejects_unknown_operator() {
    let home = isolated_home();
    let path = write_contract(home.path(), "contract.rs", CONTRACT);
    let output = starforge(home.path())
        .args([
            "mutate",
            "generate",
            path.to_str().unwrap(),
            "--operators",
            "nonsense",
        ])
        .output()
        .expect("spawn generate bad operator");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unknown mutation operator"));
}

#[test]
fn generate_respects_max_mutants() {
    let home = isolated_home();
    let path = write_contract(home.path(), "contract.rs", CONTRACT);
    let output = starforge(home.path())
        .args([
            "mutate",
            "generate",
            path.to_str().unwrap(),
            "--max-mutants",
            "2",
            "--json",
        ])
        .output()
        .expect("spawn generate capped");
    assert_success(&output, "starforge mutate generate --max-mutants");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"count\": 2"));
}

#[test]
fn generate_missing_file_fails() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args(["mutate", "generate", "nope.rs"])
        .output()
        .expect("spawn generate missing");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("does not exist"));
}

#[test]
fn run_with_failing_tests_kills_all_mutants() {
    let home = isolated_home();
    let path = write_contract(home.path(), "contract.rs", CONTRACT);
    let output = starforge(home.path())
        .args([
            "mutate",
            "run",
            path.to_str().unwrap(),
            "--test-command",
            always_fail(),
            "--skip-baseline",
            "--max-mutants",
            "3",
            "--format",
            "json",
        ])
        .output()
        .expect("spawn run");
    assert_success(&output, "starforge mutate run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"survived\": 0"));
    assert!(stdout.contains("\"score\": 100"));
}

#[test]
fn run_with_passing_tests_reports_survivors_and_suggestions() {
    let home = isolated_home();
    let path = write_contract(home.path(), "contract.rs", CONTRACT);
    let output = starforge(home.path())
        .args([
            "mutate",
            "run",
            path.to_str().unwrap(),
            "--test-command",
            always_pass(),
            "--skip-baseline",
            "--max-mutants",
            "3",
            "--format",
            "json",
        ])
        .output()
        .expect("spawn run survivors");
    assert_success(&output, "starforge mutate run survivors");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"killed\": 0"));
    assert!(stdout.contains("\"suggestions\""));
    assert!(stdout.contains("\"weak_spots\""));
}

#[test]
fn run_restores_the_original_source() {
    let home = isolated_home();
    let path = write_contract(home.path(), "contract.rs", CONTRACT);
    let output = starforge(home.path())
        .args([
            "mutate",
            "run",
            path.to_str().unwrap(),
            "--test-command",
            always_pass(),
            "--skip-baseline",
            "--max-mutants",
            "3",
        ])
        .output()
        .expect("spawn run restore");
    assert_success(&output, "starforge mutate run restore");
    let after = std::fs::read_to_string(&path).expect("read contract");
    assert_eq!(
        after, CONTRACT,
        "the contract source must be restored after the run"
    );
}

#[test]
fn run_ci_gate_fails_below_threshold() {
    let home = isolated_home();
    let path = write_contract(home.path(), "contract.rs", CONTRACT);
    let output = starforge(home.path())
        .args([
            "mutate",
            "run",
            path.to_str().unwrap(),
            "--test-command",
            always_pass(),
            "--skip-baseline",
            "--max-mutants",
            "2",
            "--min-score",
            "90",
            "--ci",
        ])
        .output()
        .expect("spawn run ci gate");
    assert!(
        !output.status.success(),
        "CI gate must fail when every mutant survives"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("below the required"));
}

#[test]
fn run_ci_gate_passes_above_threshold() {
    let home = isolated_home();
    let path = write_contract(home.path(), "contract.rs", CONTRACT);
    let output = starforge(home.path())
        .args([
            "mutate",
            "run",
            path.to_str().unwrap(),
            "--test-command",
            always_fail(),
            "--skip-baseline",
            "--max-mutants",
            "2",
            "--min-score",
            "90",
            "--ci",
        ])
        .output()
        .expect("spawn run ci pass");
    assert_success(&output, "starforge mutate run ci pass");
}

#[test]
fn run_writes_markdown_report() {
    let home = isolated_home();
    let path = write_contract(home.path(), "contract.rs", CONTRACT);
    let report = home.path().join("out").join("mutation.md");
    let output = starforge(home.path())
        .args([
            "mutate",
            "run",
            path.to_str().unwrap(),
            "--test-command",
            always_pass(),
            "--skip-baseline",
            "--max-mutants",
            "2",
            "--format",
            "markdown",
            "--output",
            report.to_str().unwrap(),
        ])
        .output()
        .expect("spawn run markdown");
    assert_success(&output, "starforge mutate run --output");
    assert!(report.exists(), "report file should be created");
    let contents = std::fs::read_to_string(&report).expect("read report");
    assert!(contents.contains("# Mutation Testing Report"));
    assert!(contents.contains("Mutation score"));
}

#[test]
fn run_fails_when_baseline_is_red() {
    let home = isolated_home();
    let path = write_contract(home.path(), "contract.rs", CONTRACT);
    // Baseline enabled (no --skip-baseline) with a failing suite.
    let output = starforge(home.path())
        .args([
            "mutate",
            "run",
            path.to_str().unwrap(),
            "--test-command",
            always_fail(),
        ])
        .output()
        .expect("spawn run baseline");
    assert!(
        !output.status.success(),
        "a red baseline must abort the run"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Baseline"));
}

#[test]
fn ci_workflow_is_written() {
    let home = isolated_home();
    let out = home.path().join("mutation.yml");
    let output = starforge(home.path())
        .args([
            "mutate",
            "ci-workflow",
            "--source",
            "src/lib.rs",
            "--min-score",
            "75",
            "--output",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("spawn ci-workflow");
    assert_success(&output, "starforge mutate ci-workflow");
    let yaml = std::fs::read_to_string(&out).expect("read workflow");
    assert!(yaml.contains("name: StarForge Mutation Testing"));
    assert!(yaml.contains("--min-score 75.0"));
    assert!(yaml.contains("--ci"));
}
