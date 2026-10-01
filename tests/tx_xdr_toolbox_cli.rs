//! End-to-end piping for the transaction XDR toolbox (#916).
//!
//! The acceptance criterion is that the stages compose as real processes:
//! `tx encode | tx sign | tx submit`. These tests run the built binary with
//! stdin/stdout wired together and assert that stdout stays a pure payload —
//! no banner, no report lines — so a shell pipe carries valid XDR.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use starforge::utils::tx_xdr;
use stellar_strkey::ed25519 as strkey;
use stellar_xdr::curr::{MuxedAccount, Operation, OperationBody, Uint256};

const TESTNET_PASSPHRASE: &str = "Test SDF Network ; September 2015";
/// Seed for the wallet the CLI pipe test imports; never a real funded account.
const SIGNER_SEED: [u8; 32] = [7u8; 32];

fn inflation_envelope_xdr() -> String {
    let envelope = tx_xdr::unsigned_envelope(
        &MuxedAccount::Ed25519(Uint256([1u8; 32])),
        1_700_000_000,
        vec![Operation {
            source_account: None,
            body: OperationBody::Inflation,
        }],
    )
    .unwrap();
    tx_xdr::write_envelope(&envelope, tx_xdr::WireFormat::Base64).unwrap()
}

fn isolated_home() -> tempfile::TempDir {
    tempfile::tempdir().expect("create isolated home")
}

fn starforge(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_starforge"));
    cmd.env("HOME", home);
    cmd.env("USERPROFILE", home);
    cmd.env("STARFORGE_CONFIG_DIR", home.join(".starforge"));
    cmd
}

/// Runs one stage of the pipeline: `input` on stdin, stdout captured.
///
/// Deliberately spawned without `-q`: if the command succeeds while printing
/// anything but the payload to stdout, the pipe is broken.
fn pipe(home: &Path, args: &[&str], input: &str) -> String {
    let mut child = starforge(home)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn starforge");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait");
    assert!(
        output.status.success(),
        "starforge {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn decode_encode_sign_compose_over_stdin_and_stdout() {
    let home = isolated_home();
    let secret = strkey::PrivateKey(SIGNER_SEED).to_string();
    let signer = tx_xdr::public_key_for_secret(&secret).unwrap();

    // Import the signing wallet into the isolated config.
    let imported = starforge(home.path())
        .args([
            "-q",
            "wallet",
            "import",
            "toolbox",
            "--key",
            &secret,
            "--network",
            "testnet",
        ])
        .output()
        .expect("import wallet");
    assert!(
        imported.status.success(),
        "wallet import failed: {}",
        String::from_utf8_lossy(&imported.stderr)
    );

    let unsigned = inflation_envelope_xdr();

    // stage 1: decode → JSON, on a single line so the next stage can read it
    let json = pipe(home.path(), &["tx", "decode", "--compact"], &unsigned);
    // stage 2: JSON → XDR must reproduce the input byte for byte
    let encoded = pipe(home.path(), &["tx", "encode"], &json);
    assert_eq!(encoded, unsigned, "tx decode | tx encode changed the XDR");

    // stage 3: sign the piped envelope
    let signed = pipe(
        home.path(),
        &["tx", "sign", "--wallet", "toolbox", "--network", "testnet"],
        &encoded,
    );
    let signed_envelope = tx_xdr::parse_envelope(&signed).expect("signed XDR from the pipe");
    assert!(
        tx_xdr::verify_signature(&signed_envelope, TESTNET_PASSPHRASE, &signer).unwrap(),
        "the signature produced through the pipe does not verify"
    );

    // stage 4: the hash the network will key the transaction by
    let hash = pipe(
        home.path(),
        &["tx", "decode", "--hash", "--network", "testnet"],
        &signed,
    );
    assert_eq!(
        hash,
        hex::encode(tx_xdr::signature_hash(&signed_envelope, TESTNET_PASSPHRASE).unwrap())
    );
}

#[test]
fn submit_reports_a_local_parse_failure_without_submitting() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args(["-q", "tx", "submit", "--yes", "--network", "testnet"])
        .stdin(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            child
                .stdin
                .as_mut()
                .expect("stdin")
                .write_all(b"definitely-not-xdr")?;
            child.wait_with_output()
        })
        .expect("run tx submit");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr).to_lowercase();
    assert!(
        stderr.contains("envelope") || stderr.contains("xdr"),
        "expected a parse error, got: {stderr}"
    );
}
