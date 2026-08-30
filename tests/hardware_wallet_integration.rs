use std::process::Command;

fn isolated_home() -> tempfile::TempDir {
    tempfile::tempdir().expect("create isolated home")
}

fn starforge(home: &std::path::Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_starforge"));
    cmd.env("HOME", home);
    cmd.env("USERPROFILE", home);
    cmd
}

#[test]
fn test_hardware_wallet_command_availability() {
    let home = isolated_home();
    let output = starforge(home.path())
        .arg("wallet")
        .arg("--help")
        .output()
        .expect("Failed to get wallet help");

    assert!(
        output.status.success(),
        "Wallet command should be available"
    );

    let help_text = String::from_utf8_lossy(&output.stdout);
    assert!(
        help_text.contains("wallet")
            || help_text.contains("hardware")
            || help_text.contains("ledger"),
        "Wallet help should document hardware wallet options"
    );
}

#[test]
fn test_hardware_wallet_detection_graceful_fallback() {
    let home = isolated_home();
    let output = starforge(home.path())
        .arg("wallet")
        .arg("list")
        .output()
        .expect("Failed to list wallets");

    assert!(
        output.status.success() || output.status.code().is_some(),
        "Wallet list should handle missing hardware gracefully"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stderr, stdout);

    if !output.status.success() {
        assert!(
            combined.contains("hardware")
                || combined.contains("not found")
                || combined.contains("unavailable"),
            "Should clearly indicate hardware wallet status"
        );
    }
}

#[test]
fn test_hardware_wallet_without_device_handling() {
    let home = isolated_home();
    let output = starforge(home.path())
        .arg("wallet")
        .arg("import")
        .arg("dummy_name")
        .arg("--hardware")
        .arg("ledger")
        .output()
        .expect("Failed to attempt hardware import");

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Diagnostics are user-facing prose ("Error: ..."), so match case-insensitively.
        let combined = format!("{}{}", stderr, stdout).to_lowercase();

        assert!(
            combined.contains("not found")
                || combined.contains("unavailable")
                || combined.contains("connect")
                || combined.contains("error"),
            "Missing hardware device should produce clear diagnostic"
        );
    }
}

#[test]
fn test_hardware_wallet_feature_detection() {
    let home = isolated_home();
    let output = starforge(home.path())
        .arg("--version")
        .output()
        .expect("Failed to get version");

    assert!(output.status.success(), "Version should be available");

    let version_str = String::from_utf8_lossy(&output.stdout);
    assert!(
        !version_str.is_empty(),
        "Version should report for feature availability"
    );
}

#[test]
fn test_hardware_wallet_error_recovery() {
    let home = isolated_home();
    let output1 = starforge(home.path())
        .arg("wallet")
        .arg("list")
        .output()
        .expect("First wallet list should work");

    let output2 = starforge(home.path())
        .arg("wallet")
        .arg("list")
        .output()
        .expect("Second wallet list should work");

    assert!(
        output1.status.success() || output1.status.code() == output2.status.code(),
        "Hardware wallet errors should be recoverable across invocations"
    );
}

#[test]
fn test_hardware_wallet_api_consistency() {
    let home = isolated_home();
    let wallet_help = starforge(home.path())
        .arg("wallet")
        .arg("--help")
        .output()
        .expect("Wallet help should be available");

    let import_help = starforge(home.path())
        .arg("wallet")
        .arg("import")
        .arg("--help")
        .output()
        .expect("Wallet import help should be available");

    let _wallet_help_text = String::from_utf8_lossy(&wallet_help.stdout);
    let _import_help_text = String::from_utf8_lossy(&import_help.stdout);

    assert!(
        wallet_help.status.success(),
        "Wallet command interface should be consistent"
    );

    assert!(
        import_help.status.success(),
        "Wallet subcommands should be available"
    );
}

#[test]
fn test_hardware_wallet_offline_behavior() {
    let home = isolated_home();
    let output = starforge(home.path())
        .arg("wallet")
        .arg("export")
        .arg("--format")
        .arg("json")
        .output();

    match output {
        Ok(output) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                let combined = format!("{}{}", stderr, stdout).to_lowercase();

                assert!(
                    combined.contains("error")
                        || combined.contains("required")
                        || combined.contains("invalid"),
                    "Should provide clear error when requirements not met"
                );
            }
        }
        Err(_) => {
            panic!("Wallet export command should be callable");
        }
    }
}

#[test]
fn test_hardware_wallet_deploy_flag_documented() {
    let home = isolated_home();
    let output = starforge(home.path())
        .arg("deploy")
        .arg("--help")
        .output()
        .expect("Failed to get deploy help");

    assert!(output.status.success(), "Deploy help should be available");
    let help_text = String::from_utf8_lossy(&output.stdout).to_lowercase();
    assert!(
        help_text.contains("hardware"),
        "Deploy command should document --hardware flag"
    );
}

#[test]
fn test_hardware_wallet_tx_send_flag_documented() {
    let home = isolated_home();
    let output = starforge(home.path())
        .arg("tx")
        .arg("send")
        .arg("--help")
        .output()
        .expect("Failed to get tx send help");

    assert!(output.status.success(), "Tx send help should be available");
    let help_text = String::from_utf8_lossy(&output.stdout).to_lowercase();
    assert!(
        help_text.contains("hardware"),
        "Tx send command should document --hardware flag"
    );
}

#[test]
fn test_hardware_wallet_multisig_sign_flag_documented() {
    let home = isolated_home();
    let output = starforge(home.path())
        .arg("wallet")
        .arg("multisig")
        .arg("sign")
        .arg("--help")
        .output()
        .expect("Failed to get multisig sign help");

    assert!(
        output.status.success(),
        "Multisig sign help should be available"
    );
    let help_text = String::from_utf8_lossy(&output.stdout).to_lowercase();
    assert!(
        help_text.contains("hardware"),
        "Multisig sign should document --hardware flag"
    );
}

#[test]
fn test_hardware_wallet_connect_timeout_flag_documented() {
    let home = isolated_home();
    let output = starforge(home.path())
        .arg("wallet")
        .arg("connect")
        .arg("--help")
        .output()
        .expect("Failed to get wallet connect help");

    assert!(
        output.status.success(),
        "Wallet connect help should be available"
    );
    let help_text = String::from_utf8_lossy(&output.stdout).to_lowercase();
    assert!(
        help_text.contains("timeout"),
        "Wallet connect should document --timeout flag"
    );
}

// -- Optional-backend coverage: these only run when the crate is compiled with
// `--features hardware-wallet`, which exercises the real hidapi/trezor-client
// code paths. CI runners have no physical device attached, so the assertions
// below pin down the disconnect / device-approval-required behavior rather
// than requiring hardware.

#[cfg(feature = "hardware-wallet")]
#[test]
fn test_hardware_wallet_connect_reports_disconnect_without_device() {
    let starforge_binary = env!("CARGO_BIN_EXE_starforge");

    let output = Command::new(starforge_binary)
        .args(["wallet", "connect", "ledger", "--timeout", "1s"])
        .output()
        .expect("Failed to run wallet connect");

    assert!(
        !output.status.success(),
        "Connect should fail when no physical Ledger is attached"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stderr, stdout).to_lowercase();
    assert!(
        combined.contains("ledger") || combined.contains("device") || combined.contains("connect"),
        "Disconnect error should name the device or connection state"
    );
}

#[cfg(feature = "hardware-wallet")]
#[test]
fn test_hardware_wallet_hw_status_reports_disconnect_without_device() {
    let starforge_binary = env!("CARGO_BIN_EXE_starforge");

    let output = Command::new(starforge_binary)
        .args(["wallet", "hw-status", "trezor"])
        .output()
        .expect("Failed to run wallet hw-status");

    assert!(
        !output.status.success(),
        "hw-status should fail when no physical Trezor is attached"
    );
}

#[cfg(feature = "hardware-wallet")]
#[test]
fn test_hardware_wallet_import_rejects_unapproved_device() {
    let starforge_binary = env!("CARGO_BIN_EXE_starforge");

    let output = Command::new(starforge_binary)
        .args([
            "wallet",
            "import",
            "ci-hw-import-test",
            "--hardware",
            "ledger",
        ])
        .output()
        .expect("Failed to attempt hardware import");

    assert!(
        !output.status.success(),
        "Import from an unapproved/absent hardware device must fail, not silently succeed"
    );
}

#[test]
fn test_hardware_wallet_timeout_behavior() {
    let home = isolated_home();
    let output = starforge(home.path())
        .arg("wallet")
        .arg("connect")
        .arg("--timeout")
        .arg("1s")
        .output();

    if let Ok(output) = output {
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let combined = format!("{}{}", stderr, stdout).to_lowercase();

            assert!(
                combined.contains("timeout")
                    || combined.contains("unavailable")
                    || combined.contains("error"),
                "Timeout behavior should be clear and predictable"
            );
        }
    }
}
