//! End-to-end tests for `starforge wallet import --from-stellar-cli`.
//!
//! Each test builds a fake stellar-cli config directory in a temp HOME and
//! runs the real binary against it, then checks the imported wallet through
//! `starforge --json wallet list`.
//!
//! Key material is the public SEP-0005 test vector 1 (index 0), so nothing
//! here is a real secret.

use std::path::Path;
use std::process::{Command, Output};

const PHRASE: &str = "illness spike retreat truth genius clock brain pass fit cave bargain toe";
const SECRET: &str = "SBGWSG6BTNCKCOB3DIFBGCVMUPQFYPA2G4O34RMTB343OYPXU5DJDVMN";
const PUBLIC: &str = "GDRXE2BQUC3AZNPVFSCEZ76NJ3WWL25FYFK6RGZGIEKWE4SOOHSUJUJ6";

fn starforge(home: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_starforge"))
        .arg("-q")
        .args(args)
        .current_dir(cwd)
        .env("STARFORGE_HOME", home)
        .env("USERPROFILE", home)
        .env_remove("STELLAR_CONFIG_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env("STARFORGE_NON_INTERACTIVE", "1")
        .output()
        .expect("run starforge")
}

fn write_identity(config_dir: &Path, name: &str, contents: &str) {
    let dir = config_dir.join("identity");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.toml")), contents).unwrap();
}

/// `(name, public_key)` of every saved wallet.
fn wallets(home: &Path, cwd: &Path) -> Vec<(String, String)> {
    let out = starforge(home, cwd, &["--json", "wallet", "list"]);
    assert_ok(&out);
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).expect("wallet list JSON");
    json["data"]["wallets"]
        .as_array()
        .map(|ws| {
            ws.iter()
                .map(|w| {
                    (
                        w["name"].as_str().unwrap_or_default().to_string(),
                        w["public_key"].as_str().unwrap_or_default().to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn assert_ok(out: &Output) {
    assert!(
        out.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn imports_secret_key_identity_from_global_config() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    write_identity(
        &home.path().join(".config").join("stellar"),
        "alice",
        &format!("secret_key = \"{SECRET}\"\n"),
    );

    let out = starforge(
        home.path(),
        cwd.path(),
        &["wallet", "import", "--from-stellar-cli", "alice"],
    );
    assert_ok(&out);

    assert_eq!(
        wallets(home.path(), cwd.path()),
        vec![("alice".to_string(), PUBLIC.to_string())]
    );
}

#[test]
fn imports_seed_phrase_identity_from_project_dir_under_new_name() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    write_identity(
        &cwd.path().join(".stellar"),
        "deployer",
        &format!("seed_phrase = \"{PHRASE}\"\n"),
    );

    let out = starforge(
        home.path(),
        cwd.path(),
        &["wallet", "import", "ops", "--from-stellar-cli", "deployer"],
    );
    assert_ok(&out);

    // Same key as the secret_key fixture: stellar-cli derives index 0.
    assert_eq!(
        wallets(home.path(), cwd.path()),
        vec![("ops".to_string(), PUBLIC.to_string())]
    );
}

#[test]
fn missing_identity_fails_without_creating_wallet() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();

    let out = starforge(
        home.path(),
        cwd.path(),
        &["wallet", "import", "--from-stellar-cli", "ghost"],
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not found"));
    assert!(wallets(home.path(), cwd.path()).is_empty());
}

#[test]
fn public_key_only_identity_is_rejected() {
    let home = tempfile::tempdir().unwrap();
    let cwd = tempfile::tempdir().unwrap();
    write_identity(
        &home.path().join(".config").join("stellar"),
        "watch",
        &format!("public_key = \"{PUBLIC}\"\n"),
    );

    let out = starforge(
        home.path(),
        cwd.path(),
        &["wallet", "import", "--from-stellar-cli", "watch"],
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("only stores a public key"));
}
