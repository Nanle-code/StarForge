//! End-to-end tests for keyless template provenance.
//!
//! Keyless Sigstore signing is performed by the `cosign` CLI. These tests hand
//! the CLI a tiny stand-in through `STARFORGE_COSIGN_BIN`: the stand-in records
//! the signed payload and the signing identity in the bundle at `sign-blob`
//! time and re-checks both at `verify-blob` time, which is the contract the real
//! CLI fulfils with a Fulcio certificate and a Rekor entry. That keeps the whole
//! publish -> registry -> install path, including tamper detection, exercisable
//! offline and deterministically.
//!
//! The fake signer is a POSIX shell script, so the signing tests are Unix-only.
//! The policy tests do not need a signer and run everywhere.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const IDENTITY: &str = "publisher@example.com";
const ISSUER: &str = "https://token.actions.githubusercontent.com";
const TEMPLATE_NAME: &str = "signed-demo";
const TEMPLATE_VERSION: &str = "1.0.0";

/// Minimal `cosign` stand-in. It only needs to be self-consistent: whatever it
/// records at signing time it must find again at verification time.
#[cfg(unix)]
const FAKE_COSIGN: &str = r#"#!/bin/sh
command="$1"
shift

bundle=""
identity=""
issuer=""
payload=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --bundle) bundle="$2"; shift 2 ;;
    --certificate-identity) identity="$2"; shift 2 ;;
    --certificate-oidc-issuer) issuer="$2"; shift 2 ;;
    --yes) shift ;;
    *) payload="$1"; shift ;;
  esac
done

case "$command" in
  sign-blob)
    if [ -z "$bundle" ] || [ -z "$payload" ]; then
      echo "fake cosign: sign-blob needs --bundle and a payload" >&2
      exit 2
    fi
    {
      echo "payload=$(cat "$payload")"
      echo "identity=$STARFORGE_OIDC_IDENTITY"
      echo "issuer=$STARFORGE_OIDC_ISSUER"
    } > "$bundle"
    exit 0
    ;;
  verify-blob)
    if [ ! -f "$bundle" ]; then
      echo "fake cosign: no bundle at $bundle" >&2
      exit 1
    fi
    if ! grep -qx "payload=$(cat "$payload")" "$bundle"; then
      echo "fake cosign: payload does not match the bundle" >&2
      exit 1
    fi
    if ! grep -qx "identity=$identity" "$bundle"; then
      echo "fake cosign: certificate identity does not match" >&2
      exit 1
    fi
    if ! grep -qx "issuer=$issuer" "$bundle"; then
      echo "fake cosign: certificate issuer does not match" >&2
      exit 1
    fi
    exit 0
    ;;
  *)
    echo "fake cosign: unsupported command $command" >&2
    exit 2
    ;;
esac
"#;

/// A `starforge` invocation whose config directory, template store and registry
/// all live under `home`, so the test never touches the real user's state.
fn starforge(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_starforge"));
    cmd.arg("-q");
    cmd.env("HOME", home);
    cmd.env("USERPROFILE", home);
    cmd
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{} failed\nstdout:\n{}\nstderr:\n{}",
        context,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn write_template(dir: &Path) {
    fs::create_dir_all(dir.join("src")).expect("create template src");
    fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"{{PROJECT_NAME}}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");
    fs::write(
        dir.join("src/lib.rs"),
        "#![no_std]\n\npub fn ping() -> u32 {\n    1\n}\n",
    )
    .expect("write lib.rs");
    fs::write(dir.join("README.md"), "# Signed demo template\n").expect("write README");
}

#[cfg(unix)]
fn install_fake_cosign(dir: &Path) -> PathBuf {
    let path = dir.join("cosign");
    fs::write(&path, FAKE_COSIGN).expect("write fake cosign");
    let mut permissions = fs::metadata(&path).expect("cosign metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("chmod fake cosign");
    path
}

fn registry_path(home: &Path) -> PathBuf {
    home.join(".starforge")
        .join("templates")
        .join("registry.json")
}

fn published_storage_path(home: &Path) -> PathBuf {
    home.join(".starforge")
        .join("templates")
        .join("storage")
        .join(TEMPLATE_NAME)
        .join(TEMPLATE_VERSION)
}

fn read_registry(home: &Path) -> serde_json::Value {
    let raw = fs::read_to_string(registry_path(home)).expect("read registry");
    serde_json::from_str(&raw).expect("parse registry")
}

/// The registry starts from the bundled index, so find the published entry by
/// name and version rather than assuming a position.
fn find_entry(registry: &serde_json::Value) -> &serde_json::Value {
    registry
        .get("templates")
        .and_then(|value| value.as_array())
        .expect("registry templates array")
        .iter()
        .find(|entry| {
            entry.get("name").and_then(|value| value.as_str()) == Some(TEMPLATE_NAME)
                && entry.get("version").and_then(|value| value.as_str()) == Some(TEMPLATE_VERSION)
        })
        .expect("published entry")
}

fn write_registry(home: &Path, registry: &serde_json::Value) {
    fs::write(
        registry_path(home),
        serde_json::to_string_pretty(registry).expect("serialize registry"),
    )
    .expect("write registry");
}

fn with_entry_mut<F>(home: &Path, mutate: F)
where
    F: FnOnce(&mut serde_json::Value),
{
    let mut registry = read_registry(home);
    let entries = registry
        .get_mut("templates")
        .and_then(|value| value.as_array_mut())
        .expect("registry templates array");
    let entry = entries
        .iter_mut()
        .find(|entry| {
            entry.get("name").and_then(|value| value.as_str()) == Some(TEMPLATE_NAME)
                && entry.get("version").and_then(|value| value.as_str()) == Some(TEMPLATE_VERSION)
        })
        .expect("published entry");
    mutate(entry);
    write_registry(home, &registry);
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create destination");
    for entry in fs::read_dir(from).expect("read source dir") {
        let entry = entry.expect("dir entry");
        let destination = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), &destination).expect("copy file");
        }
    }
}

/// Publish stores the package inside the template store and points the registry
/// entry at it. Copy the package somewhere else and re-point the entry so a
/// later install copies from a distinct source instead of onto itself.
fn relocate_published_package(home: &Path, destination: &Path) {
    copy_dir(&published_storage_path(home), destination);
    let destination = destination.display().to_string();
    with_entry_mut(home, |entry| {
        entry["source"]["path"] = serde_json::Value::String(destination);
    });
}

fn publish_signed(home: &Path, template_dir: &Path, cosign: &Path) -> Output {
    let mut cmd = starforge(home);
    cmd.env("STARFORGE_COSIGN_BIN", cosign);
    cmd.env("STARFORGE_OIDC_IDENTITY", IDENTITY);
    cmd.env("STARFORGE_OIDC_ISSUER", ISSUER);
    cmd.args(["template", "publish"]).arg(template_dir).args([
        "--name",
        TEMPLATE_NAME,
        "--description",
        "A template used by the provenance tests",
        "--author",
        "Test Author",
        "--sign",
        "--identity",
        IDENTITY,
        "--oidc-issuer",
        ISSUER,
    ]);
    cmd.output().expect("run template publish")
}

fn publish_unsigned(home: &Path, template_dir: &Path) -> Output {
    let mut cmd = starforge(home);
    // Point the signer lookup at a path that does not exist so the publish is
    // never signed, whatever is installed on the machine running the tests.
    cmd.env("STARFORGE_COSIGN_BIN", home.join("missing-cosign"));
    cmd.args(["template", "publish"]).arg(template_dir).args([
        "--name",
        TEMPLATE_NAME,
        "--description",
        "A template used by the provenance tests",
        "--author",
        "Test Author",
    ]);
    cmd.output().expect("run template publish")
}

fn fetch(home: &Path, require_signed: bool, cosign_env: Option<&Path>) -> Output {
    let mut cmd = starforge(home);
    cmd.args(["template", "fetch", TEMPLATE_NAME, "--force"]);
    if require_signed {
        cmd.arg("--require-signed");
    }
    if let Some(cosign) = cosign_env {
        cmd.env("STARFORGE_COSIGN_BIN", cosign);
    }
    cmd.output().expect("run template fetch")
}

#[cfg(unix)]
#[test]
fn signed_publish_records_provenance_and_install_verifies_it() {
    let home = tempfile::tempdir().expect("isolated home");
    let template_dir = home.path().join("source-template");
    write_template(&template_dir);
    let cosign = install_fake_cosign(home.path());

    let published = publish_signed(home.path(), &template_dir, &cosign);
    assert_success(&published, "signed publish");
    assert!(stdout(&published).contains("Signed by"));

    // The registry must carry the bundle, not just a checksum.
    let registry = read_registry(home.path());
    let entry = find_entry(&registry);
    assert_eq!(entry["provenance"]["identity"], IDENTITY);
    assert_eq!(entry["provenance"]["issuer"], ISSUER);
    assert_eq!(entry["provenance"]["signer"], "cosign");
    let digest = entry["provenance"]["digest"]
        .as_str()
        .expect("provenance digest");
    assert_eq!(digest.len(), 64);
    assert!(!entry["provenance"]["bundle"]
        .as_str()
        .expect("provenance bundle")
        .is_empty());

    let relocated = home.path().join("published-package");
    relocate_published_package(home.path(), &relocated);

    let installed = fetch(home.path(), false, Some(cosign.as_path()));
    assert_success(&installed, "verified install");
    assert!(stdout(&installed).contains("installed"));
}

#[cfg(unix)]
#[test]
fn install_rejects_a_package_modified_after_signing() {
    let home = tempfile::tempdir().expect("isolated home");
    let template_dir = home.path().join("source-template");
    write_template(&template_dir);
    let cosign = install_fake_cosign(home.path());

    assert_success(
        &publish_signed(home.path(), &template_dir, &cosign),
        "publish",
    );

    let relocated = home.path().join("published-package");
    relocate_published_package(home.path(), &relocated);

    // Tamper with the published bytes while leaving the registry record intact.
    fs::write(
        relocated.join("src/lib.rs"),
        "#![no_std]\n\npub fn ping() -> u32 {\n    1337\n}\n",
    )
    .expect("tamper with the published package");

    let installed = fetch(home.path(), false, Some(cosign.as_path()));
    assert!(
        !installed.status.success(),
        "a tampered package must not install"
    );
    assert!(
        stderr(&installed).contains("does not match its signed provenance"),
        "expected a digest mismatch, got:\n{}",
        stderr(&installed)
    );
}

#[cfg(unix)]
#[test]
fn install_rejects_a_tampered_bundle() {
    let home = tempfile::tempdir().expect("isolated home");
    let template_dir = home.path().join("source-template");
    write_template(&template_dir);
    let cosign = install_fake_cosign(home.path());

    assert_success(
        &publish_signed(home.path(), &template_dir, &cosign),
        "publish",
    );

    let relocated = home.path().join("published-package");
    relocate_published_package(home.path(), &relocated);

    // Replace the recorded bundle with unrelated bytes: the package still
    // matches its digest, so only the cryptographic layer can catch this.
    with_entry_mut(home.path(), |entry| {
        entry["provenance"]["bundle"] =
            serde_json::Value::String("bm90LXRoZS1yZWFsLWJ1bmRsZQ==".to_string());
    });

    let installed = fetch(home.path(), false, Some(cosign.as_path()));
    assert!(
        !installed.status.success(),
        "a tampered bundle must not install"
    );
    assert!(
        stderr(&installed).contains("Sigstore verification failed"),
        "expected a bundle verification failure, got:\n{}",
        stderr(&installed)
    );
}

#[test]
fn require_signed_refuses_an_unsigned_template() {
    let home = tempfile::tempdir().expect("isolated home");
    let template_dir = home.path().join("source-template");
    write_template(&template_dir);

    assert_success(
        &publish_unsigned(home.path(), &template_dir),
        "unsigned publish",
    );

    let registry = read_registry(home.path());
    assert!(
        find_entry(&registry).get("provenance").is_none(),
        "an unsigned publish must not invent provenance"
    );

    let relocated = home.path().join("published-package");
    relocate_published_package(home.path(), &relocated);

    // Without the policy the unsigned template still installs.
    let permissive = fetch(home.path(), false, None);
    assert_success(&permissive, "install without the signed policy");

    // With it, the same template is refused.
    let strict = fetch(home.path(), true, None);
    assert!(
        !strict.status.success(),
        "an unsigned template must not install under --require-signed"
    );
    assert!(
        stderr(&strict).contains("requires signed templates"),
        "expected the signed-templates policy to be named, got:\n{}",
        stderr(&strict)
    );
}
