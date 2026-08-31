//! Fast CLI smoke tests for core user paths (no network required).

use std::process::Command;

fn isolated_home() -> tempfile::TempDir {
    tempfile::tempdir().expect("create isolated home")
}

fn starforge(home: &std::path::Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_starforge"));
    cmd.arg("-q");
    cmd.env("HOME", home);
    cmd.env("USERPROFILE", home);
    // HOME / USERPROFILE alone do not isolate the CLI on Windows, where
    // `dirs::home_dir()` resolves through SHGetKnownFolderPath(FOLDERID_Profile)
    // and ignores both. Without this, every test in this file shares the one
    // real config directory, and the tests that mutate networks concurrently
    // clobber each other. Set explicitly rather than inherited so a
    // STARFORGE_CONFIG_DIR already in the environment cannot deisolate the run.
    cmd.env("STARFORGE_CONFIG_DIR", home.join(".starforge"));
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

#[test]
fn info_exits_zero() {
    let home = isolated_home();
    let output = starforge(home.path())
        .arg("info")
        .output()
        .expect("spawn info");
    assert_success(&output, "starforge info");
}

#[test]
fn version_prints_release() {
    let home = isolated_home();
    let output = starforge(home.path())
        .arg("--version")
        .output()
        .expect("spawn version");
    assert_success(&output, "starforge --version");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("starforge"));
}

#[test]
fn help_lists_wallet_command() {
    let home = isolated_home();
    let output = starforge(home.path())
        .arg("--help")
        .output()
        .expect("spawn help");
    assert_success(&output, "starforge --help");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("wallet"));
}

#[test]
fn network_show_exits_zero() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args(["network", "show"])
        .output()
        .expect("spawn network show");
    assert_success(&output, "starforge network show");
}

#[test]
fn wallet_list_exits_zero() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args(["wallet", "list"])
        .output()
        .expect("spawn wallet list");
    assert_success(&output, "starforge wallet list");
}

#[test]
fn template_list_exits_zero() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args(["template", "list"])
        .output()
        .expect("spawn template list");
    assert_success(&output, "starforge template list");
}

#[test]
fn template_validate_accepts_the_bundled_registry() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args(["template", "validate", "templates/registry.json"])
        .output()
        .expect("spawn template validate");
    assert_success(&output, "starforge template validate");
}

#[test]
fn template_validate_reports_the_offending_field() {
    let home = isolated_home();
    let bad = home.path().join("bad-registry.json");
    std::fs::write(
        &bad,
        r#"{"templates":[{"name":"broken","version":"v1","description":"d",
            "author":"a","tags":[],"source":{"type":"builtin","id":"broken"}}]}"#,
    )
    .expect("write bad registry");

    let output = starforge(home.path())
        .args(["template", "validate"])
        .arg(&bad)
        .output()
        .expect("spawn template validate");

    assert!(
        !output.status.success(),
        "an invalid registry should exit non-zero"
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("templates[0].version"),
        "output should name the offending field, got: {}",
        combined
    );
}

#[test]
fn deploy_help_documents_flags() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args(["deploy", "--help"])
        .output()
        .expect("spawn deploy help");
    assert_success(&output, "starforge deploy --help");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--simulate"));
    assert!(stdout.contains("--execute"));
}

#[test]
fn upgrade_auto_help_lists_compatibility_commands() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args(["upgrade", "auto", "--help"])
        .output()
        .expect("spawn upgrade auto help");
    assert_success(&output, "starforge upgrade auto --help");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("compat"));
    assert!(stdout.contains("plan"));
    assert!(stdout.contains("migration"));
}

#[test]
fn network_add_custom_succeeds() {
    let home = isolated_home();
    let net_name = format!(
        "smoke-net-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros()
    );
    let output = starforge(home.path())
        .args([
            "network",
            "add",
            &net_name,
            "--horizon-url",
            "https://example.com/horizon",
            "--soroban-rpc-url",
            "https://example.com/rpc",
        ])
        .output()
        .expect("spawn network add");
    assert_success(&output, "starforge network add");

    // Verify it appears in the list
    let list_output = starforge(home.path())
        .args(["network", "show"])
        .output()
        .expect("spawn network show");
    assert_success(&list_output, "starforge network show");
    let stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(
        stdout.to_lowercase().contains(&net_name),
        "expected unique net_name in network show output"
    );
}

#[test]
fn network_add_rejects_empty_horizon_url() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args(["network", "add", "bad-net", "--horizon-url", ""])
        .output()
        .expect("spawn network add with empty url");
    assert!(
        !output.status.success(),
        "expected non-zero exit for empty horizon URL"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be empty") || stderr.contains("must start with"),
        "expected URL validation message in stderr, got: {}",
        stderr
    );
}

#[test]
fn network_switch_to_mainnet_succeeds() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args(["network", "switch", "mainnet"])
        .output()
        .expect("spawn network switch mainnet");
    assert_success(&output, "starforge network switch mainnet");
}

#[test]
fn network_switch_unknown_network_fails() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args(["network", "switch", "does-not-exist-xyz"])
        .output()
        .expect("spawn network switch unknown");
    assert!(
        !output.status.success(),
        "expected non-zero exit for unknown network"
    );
}

#[test]
fn network_remove_custom_succeeds() {
    let home = isolated_home();
    let net_name = format!(
        "remove-net-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros()
    );
    starforge(home.path())
        .args([
            "network",
            "add",
            &net_name,
            "--horizon-url",
            "https://example.com/horizon",
        ])
        .output()
        .expect("spawn network add");

    let output = starforge(home.path())
        .args(["network", "remove", &net_name])
        .output()
        .expect("spawn network remove");
    assert_success(&output, "starforge network remove");

    let show = starforge(home.path())
        .args(["network", "show"])
        .output()
        .expect("spawn network show");
    let stdout = String::from_utf8_lossy(&show.stdout);
    assert!(
        !stdout.to_lowercase().contains(&net_name),
        "removed network should not appear in show output"
    );
}

#[test]
fn network_remove_reserved_fails() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args(["network", "remove", "testnet"])
        .output()
        .expect("spawn network remove testnet");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("built-in") || stderr.contains("cannot be removed"));
}

#[test]
fn network_rename_custom_succeeds() {
    let home = isolated_home();
    let old_name = format!(
        "old-net-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros()
    );
    let new_name = format!(
        "new-net-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros()
            + 1
    );
    starforge(home.path())
        .args([
            "network",
            "add",
            &old_name,
            "--horizon-url",
            "https://example.com/horizon",
        ])
        .output()
        .expect("spawn network add");

    let output = starforge(home.path())
        .args(["network", "rename", &old_name, &new_name])
        .output()
        .expect("spawn network rename");
    assert_success(&output, "starforge network rename");

    let show = starforge(home.path())
        .args(["network", "show"])
        .output()
        .expect("spawn network show");
    let stdout = String::from_utf8_lossy(&show.stdout);
    assert!(stdout.to_lowercase().contains(&new_name));
}

#[test]
fn network_add_reserved_name_fails() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args([
            "network",
            "add",
            "testnet",
            "--horizon-url",
            "https://example.com",
        ])
        .output()
        .expect("spawn network add testnet");
    assert!(
        !output.status.success(),
        "expected non-zero exit when overwriting reserved network name"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("reserved"),
        "expected 'reserved' in stderr, got: {}",
        stderr
    );
}

#[test]
fn config_subcommand_sets_and_shows_telemetry() {
    let home = isolated_home();

    // Disable telemetry via config set
    let output2 = starforge(home.path())
        .args(["config", "set", "telemetry.enabled", "false"])
        .output()
        .expect("spawn config set");
    assert_success(&output2, "starforge config set telemetry.enabled false");
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout2.contains("set to 'false'"));

    // Check again: telemetry should show false
    let output3 = starforge(home.path())
        .args(["config", "show"])
        .output()
        .expect("spawn config show");
    assert_success(&output3, "starforge config show");
    let stdout3 = String::from_utf8_lossy(&output3.stdout);
    assert!(stdout3.contains("telemetry.enabled"));
    assert!(stdout3.contains("false"));

    // Enable telemetry via config set
    let output4 = starforge(home.path())
        .args(["config", "set", "telemetry.enabled", "true"])
        .output()
        .expect("spawn config set");
    assert_success(&output4, "starforge config set telemetry.enabled true");
    let stdout4 = String::from_utf8_lossy(&output4.stdout);
    assert!(stdout4.contains("set to 'true'"));

    // Check again: telemetry should show true
    let output5 = starforge(home.path())
        .args(["config", "show"])
        .output()
        .expect("spawn config show");
    assert_success(&output5, "starforge config show");
    let stdout5 = String::from_utf8_lossy(&output5.stdout);
    assert!(stdout5.contains("telemetry.enabled"));
    assert!(stdout5.contains("true"));
}

#[test]
fn telemetry_subcommand_toggles_status() {
    let home = isolated_home();

    // Disable telemetry
    let output2 = starforge(home.path())
        .args(["telemetry", "disable"])
        .output()
        .expect("spawn telemetry disable");
    assert_success(&output2, "starforge telemetry disable");
    let stdout2 = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout2.contains("disabled"));

    // Check disabled status
    let output3 = starforge(home.path())
        .args(["telemetry", "status"])
        .output()
        .expect("spawn telemetry status");
    assert_success(&output3, "starforge telemetry status");
    let stdout3 = String::from_utf8_lossy(&output3.stdout);
    assert!(stdout3.contains("false"));

    // Enable telemetry
    let output4 = starforge(home.path())
        .args(["telemetry", "enable"])
        .output()
        .expect("spawn telemetry enable");
    assert_success(&output4, "starforge telemetry enable");

    // Check enabled status
    let output5 = starforge(home.path())
        .args(["telemetry", "status"])
        .output()
        .expect("spawn telemetry status");
    assert_success(&output5, "starforge telemetry status");
    let stdout5 = String::from_utf8_lossy(&output5.stdout);
    assert!(stdout5.contains("true"));
}

#[test]
fn telemetry_respects_env_override() {
    let home = isolated_home();

    // status with env override False
    let mut cmd = starforge(home.path());
    cmd.args(["telemetry", "status"]);
    cmd.env("STARFORGE_TELEMETRY", "false");
    let output = cmd.output().expect("spawn telemetry status");
    assert_success(&output, "starforge telemetry status with env override");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Environment Override"));
    assert!(stdout.contains("false"));
}

fn write_config(home: &std::path::Path, contents: &str) {
    let dir = home.join(".starforge");
    std::fs::create_dir_all(&dir).expect("create config dir");
    std::fs::write(dir.join("config.toml"), contents).expect("write config");
}

#[test]
fn config_doctor_smoke_in_isolated_home() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args(["config", "doctor"])
        .output()
        .expect("spawn config doctor");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("StarForge Config Doctor"));
    assert!(stdout.contains("schema"));
    assert!(stdout.contains("Passed"));
    assert!(
        stdout.contains("no config.toml found") || stdout.contains("config version is"),
        "expected default schema finding, got: {stdout}"
    );
}

#[test]
fn config_doctor_fails_on_invalid_wallet_key() {
    let home = isolated_home();
    write_config(
        home.path(),
        r#"
version = "1"
network = "testnet"

[[wallets]]
name = "bad"
public_key = "not-a-key"
network = "testnet"
created_at = ""
funded = false
"#,
    );

    let output = starforge(home.path())
        .args(["config", "doctor"])
        .output()
        .expect("spawn config doctor");
    assert!(
        !output.status.success(),
        "expected non-zero exit for invalid wallet public key"
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("wallet") || combined.contains("public key"),
        "expected wallet validation failure, got: {combined}"
    );
}

#[test]
fn config_help_lists_doctor_subcommand() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args(["config", "--help"])
        .output()
        .expect("spawn config help");
    assert_success(&output, "starforge config --help");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("doctor"));
}

#[test]
fn multisig_templates_lists_scenarios() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args(["multisig", "templates"])
        .output()
        .expect("spawn multisig templates");
    assert_success(&output, "starforge multisig templates");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("escrow"));
    assert!(stdout.contains("dao"));
}

#[test]
fn multisig_create_and_sign_workflow() {
    let home = isolated_home();
    let dir = home.path().join("proposals");
    std::fs::create_dir_all(&dir).expect("create proposals dir");

    let create = starforge(home.path())
        .current_dir(&dir)
        .args([
            "multisig",
            "create",
            "--threshold",
            "2",
            "--signers",
            "alice,bob",
            "--network",
            "testnet",
        ])
        .output()
        .expect("spawn multisig create");
    assert_success(&create, "starforge multisig create");

    let entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("read proposals dir")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("proposal_") && n.ends_with(".json"))
                .unwrap_or(false)
        })
        .collect();
    assert_eq!(entries.len(), 1, "expected one proposal file");
    let created_path = entries[0].path();

    let sign_alice = starforge(home.path())
        .args([
            "multisig",
            "sign",
            "--wallet",
            "alice",
            created_path.to_str().unwrap(),
        ])
        .output()
        .expect("spawn multisig sign alice");
    assert_success(&sign_alice, "starforge multisig sign alice");

    let status = starforge(home.path())
        .args(["multisig", "status", created_path.to_str().unwrap()])
        .output()
        .expect("spawn multisig status");
    assert_success(&status, "starforge multisig status");
    let status_out = String::from_utf8_lossy(&status.stdout);
    assert!(status_out.contains("Progress: 1/2"));
    assert!(status_out.contains("50%"));

    let sign_bob = starforge(home.path())
        .args([
            "multisig",
            "sign",
            "--wallet",
            "bob",
            created_path.to_str().unwrap(),
        ])
        .output()
        .expect("spawn multisig sign bob");
    assert_success(&sign_bob, "starforge multisig sign bob");

    let is_ready = starforge(home.path())
        .args(["multisig", "is-ready", created_path.to_str().unwrap()])
        .output()
        .expect("spawn multisig is-ready");
    assert!(is_ready.status.success(), "expected ready proposal");
    assert_eq!(String::from_utf8_lossy(&is_ready.stdout).trim(), "ready");

    let export_path = dir.join("exported.json");
    let export = starforge(home.path())
        .args([
            "multisig",
            "export",
            "--output",
            export_path.to_str().unwrap(),
            created_path.to_str().unwrap(),
        ])
        .output()
        .expect("spawn multisig export");
    assert_success(&export, "starforge multisig export");

    let import_path = dir.join("imported.json");
    let import = starforge(home.path())
        .args([
            "multisig",
            "import",
            "--output",
            import_path.to_str().unwrap(),
            export_path.to_str().unwrap(),
        ])
        .output()
        .expect("spawn multisig import");
    assert_success(&import, "starforge multisig import");

    let notify = starforge(home.path())
        .args([
            "multisig",
            "notify",
            import_path.to_str().unwrap(),
            "--channel",
            "email",
        ])
        .output()
        .expect("spawn multisig notify");
    assert_success(&notify, "starforge multisig notify");

    let submit = starforge(home.path())
        .args([
            "multisig",
            "submit",
            import_path.to_str().unwrap(),
            "--network",
            "testnet",
        ])
        .output()
        .expect("spawn multisig submit");
    assert_success(&submit, "starforge multisig submit");
}

#[test]
fn multisig_from_template_creates_proposal() {
    let home = isolated_home();
    let dir = home.path().join("templates");
    std::fs::create_dir_all(&dir).expect("create templates dir");
    let output_path = dir.join("escrow.json");

    let output = starforge(home.path())
        .args([
            "multisig",
            "from-template",
            "--output",
            output_path.to_str().unwrap(),
            "escrow",
        ])
        .output()
        .expect("spawn multisig from-template");
    assert_success(&output, "starforge multisig from-template");
    assert!(output_path.exists(), "expected escrow proposal file");

    let contents = std::fs::read_to_string(&output_path).expect("read escrow proposal");
    assert!(contents.contains("buyer"));
    assert!(contents.contains("\"threshold\": 2"));
}
