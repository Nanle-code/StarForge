//! Unified `--dry-run` integration tests (#943).
//!
//! Every state-changing command must simulate fully, print a plan, and change
//! nothing — no filesystem writes and no network submissions. These tests run
//! the real `starforge` binary against an isolated configuration directory and
//! assert that dry runs leave persistent state untouched.

use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn isolated_home() -> tempfile::TempDir {
    tempfile::tempdir().expect("create isolated home")
}

fn starforge(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_starforge"));
    cmd.arg("-q");
    cmd.env("HOME", home);
    cmd.env("USERPROFILE", home);
    cmd.env("STARFORGE_CONFIG_DIR", home.join(".starforge"));
    // The CLI must never block on a prompt in tests.
    cmd.env("STARFORGE_NON_INTERACTIVE", "1");
    cmd
}

fn run(cmd: &mut Command, label: &str) -> std::process::Output {
    let output = cmd.output().unwrap_or_else(|e| panic!("spawn {label}: {e}"));
    assert!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn config_dir(home: &Path) -> PathBuf {
    home.join(".starforge")
}

/// Parse the first JSON value in the process output, ignoring any prefix.
fn first_json(output: &std::process::Output) -> serde_json::Value {
    let text = stdout(output);
    let start = text
        .find('{')
        .unwrap_or_else(|| panic!("no JSON object in output: {text}"));
    let mut de = serde_json::Deserializer::from_str(&text[start..]);
    serde_json::Value::deserialize(&mut de).expect("output must contain valid JSON")
}

/// Whether any file/directory under `root` has `needle` in its path.
fn dir_contains(root: &Path, needle: &str) -> bool {
    if !root.exists() {
        return false;
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.to_string_lossy().contains(needle) {
                return true;
            }
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    false
}

#[test]
fn config_set_dry_run_leaves_config_unchanged() {
    let home = isolated_home();
    let before = stdout(&run(
        starforge(home.path()).args(["config", "show"]),
        "config show (before)",
    ));

    let dry = run(
        starforge(home.path()).args(["config", "set", "telemetry.enabled", "true", "--dry-run"]),
        "config set --dry-run",
    );
    let dry_out = stdout(&dry);
    assert!(
        dry_out.contains("Dry-run plan: config set"),
        "dry-run must print a shared plan, got: {dry_out}"
    );
    assert!(
        dry_out.contains("No changes applied"),
        "dry-run must state that nothing changed, got: {dry_out}"
    );

    let after = stdout(&run(
        starforge(home.path()).args(["config", "show"]),
        "config show (after)",
    ));
    assert_eq!(
        before, after,
        "a dry run must not change the effective configuration"
    );
}

#[test]
fn config_set_dry_run_emits_json_plan() {
    let home = isolated_home();
    let output = run(
        starforge(home.path()).args([
            "--json",
            "config",
            "set",
            "telemetry.enabled",
            "true",
            "--dry-run",
        ]),
        "config set --dry-run --json",
    );

    let value = first_json(&output);
    assert_eq!(value["ok"], serde_json::Value::Bool(true));
    let data = &value["data"];
    assert_eq!(data["command"], "config set");
    assert_eq!(data["dry_run"], serde_json::Value::Bool(true));
    assert_eq!(data["writes_filesystem"], serde_json::Value::Bool(true));
    assert_eq!(data["operations"][0]["kind"], "config.write");
}

#[test]
fn network_add_dry_run_is_not_persisted() {
    let home = isolated_home();
    run(
        starforge(home.path()).args(["network", "show"]),
        "network show (before)",
    );

    let dry = run(
        starforge(home.path()).args([
            "network",
            "add",
            "dryrun-net",
            "--horizon-url",
            "https://dryrun-net.invalid",
            "--dry-run",
        ]),
        "network add --dry-run",
    );
    assert!(
        stdout(&dry).contains("Dry-run plan: network add"),
        "network add must print a plan under --dry-run"
    );

    let after = stdout(&run(
        starforge(home.path()).args(["network", "show"]),
        "network show (after)",
    ));
    assert!(
        !after.to_uppercase().contains("DRYRUN-NET"),
        "a dry run must not persist the network, got: {after}"
    );
}

#[test]
fn wallet_create_dry_run_creates_no_wallet() {
    let home = isolated_home();
    let dry = run(
        starforge(home.path()).args(["wallet", "create", "dryrunwallet", "--dry-run"]),
        "wallet create --dry-run",
    );
    assert!(
        stdout(&dry).contains("Dry-run plan: wallet create"),
        "wallet create must print a plan under --dry-run"
    );

    let listed = stdout(&run(
        starforge(home.path()).args(["wallet", "list"]),
        "wallet list (after)",
    ));
    assert!(
        !listed.contains("dryrunwallet"),
        "a dry run must not create a wallet, got: {listed}"
    );
}

#[test]
fn plugin_install_dry_run_registers_nothing() {
    let home = isolated_home();
    let dry = run(
        starforge(home.path()).args([
            "plugin",
            "install",
            "dryrun-plugin",
            "--path",
            "/nonexistent/dryrun-plugin.so",
            "--dry-run",
        ]),
        "plugin install --dry-run",
    );
    assert!(
        stdout(&dry).contains("Dry-run plan: plugin install"),
        "plugin install must print a plan under --dry-run"
    );

    let listed = stdout(&run(
        starforge(home.path()).args(["plugin", "list"]),
        "plugin list (after)",
    ));
    assert!(
        !listed.contains("dryrun-plugin"),
        "a dry run must not register a plugin, got: {listed}"
    );
}

#[test]
fn template_install_dry_run_writes_nothing() {
    let home = isolated_home();
    let dry = run(
        starforge(home.path()).args([
            "template",
            "install",
            "/nonexistent/dryrun-template",
            "--dry-run",
        ]),
        "template install --dry-run",
    );
    assert!(
        stdout(&dry).contains("Dry-run plan: template install"),
        "template install must print a plan under --dry-run"
    );
    assert!(
        !dir_contains(&config_dir(home.path()), "dryrun-template"),
        "a dry run must not write template files"
    );
}

#[test]
fn read_only_commands_still_run_under_dry_run() {
    let home = isolated_home();
    // A read-only command has nothing to simulate; it must still succeed.
    run(
        starforge(home.path()).args(["network", "show", "--dry-run"]),
        "network show --dry-run",
    );
}
