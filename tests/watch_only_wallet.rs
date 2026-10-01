//! Watch-only wallet add / list / signing rejection (#932).

use std::process::Command;
use tempfile::tempdir;

fn starforge_bin() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_starforge"));
    cmd.env_remove("STARFORGE_CONFIG_DIR");
    cmd
}

#[test]
fn watch_only_add_list_and_sign_rejection() {
    let dir = tempdir().expect("tempdir");
    let cfg = dir.path().join("cfg");
    std::fs::create_dir_all(&cfg).unwrap();

    // Well-formed G... (base32 alphabet). Validation is structural, not checksum.
    let address = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF";

    let add = starforge_bin()
        .env("STARFORGE_CONFIG_DIR", &cfg)
        .args([
            "wallet",
            "watch",
            "treasury",
            "--address",
            address,
            "--network",
            "testnet",
        ])
        .output()
        .expect("wallet watch");
    assert!(
        add.status.success(),
        "watch add failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );
    let stdout = String::from_utf8_lossy(&add.stdout);
    assert!(
        stdout.contains("watch-only") || stdout.contains("Watch-only"),
        "expected watch-only marker in output: {stdout}"
    );

    let list = starforge_bin()
        .env("STARFORGE_CONFIG_DIR", &cfg)
        .args(["wallet", "list", "--json"])
        .output()
        .expect("wallet list");
    assert!(
        list.status.success(),
        "{}",
        String::from_utf8_lossy(&list.stderr)
    );
    let body = String::from_utf8_lossy(&list.stdout);
    assert!(
        body.contains("\"watch_only\":true") || body.contains("\"watch_only\": true"),
        "JSON must mark watch_only: {body}"
    );

    let sign = starforge_bin()
        .env("STARFORGE_CONFIG_DIR", &cfg)
        .args(["wallet", "sign", "treasury", "--message", "hello"])
        .output()
        .expect("wallet sign");
    assert!(
        !sign.status.success(),
        "watch-only wallet must reject signing"
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&sign.stderr),
        String::from_utf8_lossy(&sign.stdout)
    );
    assert!(
        err.to_lowercase().contains("watch-only"),
        "expected clear watch-only error, got: {err}"
    );
}
