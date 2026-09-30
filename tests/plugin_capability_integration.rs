//! Plugin capability enforcement integration tests (issue #773).
//!
//! Executable proof that plugin filesystem/network capability boundaries hold:
//!
//! * the malicious fixture (`tests/fixtures/plugins/capabilities/malicious`)
//!   declares no capabilities and attempts filesystem and network access —
//!   every attempt must be blocked, both by the manifest gate and by the WASM
//!   sandbox;
//! * the benign fixture (`tests/fixtures/plugins/capabilities/benign`)
//!   declares exactly what it uses and must succeed.
//!
//! Run locally with:
//!
//! ```text
//! cargo test --test plugin_capability_integration --locked
//! ```

use starforge::plugins::manifest::{load_manifest_for_library, PluginManifest, MANIFEST_FILENAME};
use starforge::plugins::wasm::WasmSandboxPolicy;
use starforge::plugins::PluginManager;
use std::path::PathBuf;

fn fixture_dir(kind: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/plugins/capabilities")
        .join(kind)
}

fn fixture_manifest(kind: &str) -> PluginManifest {
    // `load_manifest_for_library` looks beside the plugin binary, which is the
    // same lookup the native loader performs at install/load time.
    let module = fixture_dir(kind).join("plugin.wasm");
    load_manifest_for_library(&module)
        .unwrap_or_else(|e| panic!("failed to parse {kind} fixture {MANIFEST_FILENAME}: {e:#}"))
        .unwrap_or_else(|| panic!("{kind} fixture is missing {MANIFEST_FILENAME}"))
}

/// Run a fixture module through the same entrypoint StarForge uses for WASM
/// plugins, returning the full error chain as a string on failure.
fn run_wasm_fixture(kind: &str, file: &str) -> Result<i32, String> {
    let path = fixture_dir(kind).join(file);
    let manager = PluginManager::new();
    let plugin = manager
        .load_wasm_plugin(&path, WasmSandboxPolicy::default())
        .map_err(|e| format!("{e:#}"))?;
    plugin.run().map_err(|e| format!("{e:#}"))
}

/// Assert that a malicious module is rejected before any of its code runs,
/// and that the error names the denied host import.
fn assert_wasm_import_denied(file: &str, denied_import: &str) {
    match run_wasm_fixture("malicious", file) {
        Ok(value) => panic!(
            "SECURITY: malicious fixture '{file}' importing '{denied_import}' executed and \
             returned {value}; the WASM sandbox must deny undeclared host imports"
        ),
        Err(err) => {
            assert!(
                err.contains("instantiate sandboxed plugin"),
                "malicious fixture '{file}' failed, but not at sandbox instantiation.\n  \
                 Expected the linker to reject the import before execution.\n  Got: {err}"
            );
            assert!(
                err.contains(denied_import),
                "sandbox rejected '{file}' but the error does not name the denied import \
                 '{denied_import}', so authors cannot tell what was blocked.\n  Got: {err}"
            );
        }
    }
}

// ── Malicious plugin: manifest gate ─────────────────────────────────────────

#[test]
fn malicious_manifest_is_denied_filesystem_read() {
    let manifest = fixture_manifest("malicious");
    let err = manifest
        .enforce_filesystem_access(false)
        .expect_err("SECURITY: plugin without 'fs:read' was granted filesystem read access");
    let msg = err.to_string();
    assert!(
        msg.contains("denied filesystem access") && msg.contains("'fs:read'"),
        "denial message should name the missing 'fs:read' capability, got: {msg}"
    );
    assert!(
        msg.contains(MANIFEST_FILENAME),
        "denial message should point authors at {MANIFEST_FILENAME}, got: {msg}"
    );
}

#[test]
fn malicious_manifest_is_denied_filesystem_write() {
    let manifest = fixture_manifest("malicious");
    let err = manifest
        .enforce_filesystem_access(true)
        .expect_err("SECURITY: plugin without 'fs:write' was granted filesystem write access");
    let msg = err.to_string();
    assert!(
        msg.contains("denied filesystem access") && msg.contains("'fs:write'"),
        "denial message should name the missing 'fs:write' capability, got: {msg}"
    );
}

#[test]
fn malicious_manifest_is_denied_network() {
    let manifest = fixture_manifest("malicious");
    let err = manifest
        .enforce_network_access()
        .expect_err("SECURITY: plugin without 'network' was granted network access");
    let msg = err.to_string();
    assert!(
        msg.contains("denied network access") && msg.contains("'network'"),
        "denial message should name the missing 'network' capability, got: {msg}"
    );
    assert!(
        msg.contains("capability-malicious"),
        "denial message should name the offending plugin, got: {msg}"
    );
}

// ── Malicious plugin: WASM sandbox ──────────────────────────────────────────

#[test]
fn malicious_wasm_filesystem_read_is_blocked() {
    assert_wasm_import_denied("fs_read.wat", "path_open");
}

#[test]
fn malicious_wasm_filesystem_write_is_blocked() {
    assert_wasm_import_denied("fs_write.wat", "fd_write");
}

#[test]
fn malicious_wasm_network_is_blocked() {
    assert_wasm_import_denied("network.wat", "sock_send");
}

// ── Benign plugin ───────────────────────────────────────────────────────────

#[test]
fn benign_manifest_is_valid_for_running_core() {
    let manifest = fixture_manifest("benign");
    manifest
        .validate()
        .unwrap_or_else(|e| panic!("benign fixture manifest should validate, got: {e:#}"));
}

#[test]
fn benign_manifest_declared_capabilities_are_granted() {
    let manifest = fixture_manifest("benign");
    manifest
        .enforce_filesystem_access(false)
        .unwrap_or_else(|e| panic!("declared 'fs:read' should be granted, got: {e:#}"));
    manifest
        .enforce_network_access()
        .unwrap_or_else(|e| panic!("declared 'network' should be granted, got: {e:#}"));
}

#[test]
fn benign_manifest_undeclared_capability_is_still_denied() {
    // Declaring fs:read must not implicitly grant fs:write.
    let manifest = fixture_manifest("benign");
    assert!(
        manifest.enforce_filesystem_access(true).is_err(),
        "SECURITY: plugin declaring only 'fs:read' was granted 'fs:write'"
    );
}

#[test]
fn benign_wasm_plugin_runs_in_sandbox() {
    let value = run_wasm_fixture("benign", "plugin.wat")
        .unwrap_or_else(|e| panic!("benign fixture should run in the sandbox, got: {e}"));
    assert_eq!(
        value, 42,
        "benign fixture returned an unexpected value from run()"
    );
}
