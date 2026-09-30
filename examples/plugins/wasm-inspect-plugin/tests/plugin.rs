//! Integration tests: exercise the plugin through the same `Plugin` trait and
//! manifest the StarForge host uses.

use starforge::plugins::manifest::{load_manifest_for_library, PluginManifest};
use starforge::plugins::{Plugin, PluginRegistrar};
use starforge_plugin_sdk::Capability;
use starforge_wasm_inspect::{inspect, register, render, InspectError, WasmInspectPlugin};
use std::path::{Path, PathBuf};

/// A module with one type section and a Soroban `contractspecv0` section.
fn soroban_like_module() -> Vec<u8> {
    let mut wasm = b"\0asm\x01\0\0\0".to_vec();
    // Type section: one `() -> ()` function type.
    wasm.extend_from_slice(&[1, 4, 1, 0x60, 0, 0]);
    // Custom section: name "contractspecv0" followed by a 3-byte payload.
    let name = b"contractspecv0";
    wasm.push(0);
    wasm.push((1 + name.len() + 3) as u8);
    wasm.push(name.len() as u8);
    wasm.extend_from_slice(name);
    wasm.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
    wasm
}

fn write_temp(file: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wasm-inspect-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(file);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn manifest() -> PluginManifest {
    let lib = Path::new(env!("CARGO_MANIFEST_DIR")).join("libstarforge_wasm_inspect.so");
    load_manifest_for_library(&lib)
        .expect("manifest parses")
        .expect("starforge-plugin.toml exists")
}

#[test]
fn detects_soroban_contract_sections() {
    let summary = inspect(&soroban_like_module()).unwrap();

    let names: Vec<&str> = summary.sections.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["type", "contractspecv0"]);
    assert!(summary.is_soroban_contract());
    assert!(render("c.wasm", &summary).contains("soroban:  yes"));
}

#[test]
fn reports_truncated_and_non_wasm_input() {
    let module = soroban_like_module();
    assert!(matches!(
        inspect(&module[..module.len() - 1]),
        Err(InspectError::Truncated { .. })
    ));
    assert_eq!(inspect(b"not wasm"), Err(InspectError::NotWasm));
}

#[test]
fn execute_reads_the_file_named_on_the_command_line() {
    let path = write_temp("contract.wasm", &soroban_like_module());
    let args = vec![path.to_string_lossy().into_owned()];
    assert_eq!(WasmInspectPlugin.execute(&args), Ok(()));
}

#[test]
fn execute_returns_errors_instead_of_panicking() {
    assert!(WasmInspectPlugin
        .execute(&[])
        .unwrap_err()
        .starts_with("usage:"));

    let not_wasm = write_temp("notes.txt", b"hello");
    let err = WasmInspectPlugin
        .execute(&[not_wasm.to_string_lossy().into_owned()])
        .unwrap_err();
    assert!(err.contains("not a WebAssembly module"), "{err}");
}

#[test]
fn registers_under_the_manifest_name() {
    #[derive(Default)]
    struct Registrar(Vec<Box<dyn Plugin>>);
    impl PluginRegistrar for Registrar {
        fn register_plugin(&mut self, plugin: Box<dyn Plugin>) {
            self.0.push(plugin);
        }
    }

    let mut registrar = Registrar::default();
    register(&mut registrar);
    let manifest = manifest();

    assert_eq!(registrar.0.len(), 1);
    assert_eq!(registrar.0[0].name(), manifest.name);
    assert_eq!(registrar.0[0].version(), manifest.version);
    manifest
        .validate()
        .expect("compatible with the running core");
}

#[test]
fn manifest_declares_least_privilege_capabilities() {
    let manifest = manifest();

    // Every declared capability uses a name the SDK knows about.
    let known = [
        Capability::FileSystemRead,
        Capability::FileSystemWrite,
        Capability::Network,
    ]
    .map(Capability::manifest_name);
    for declared in &manifest.required_capabilities {
        assert!(known.contains(&declared.as_str()), "unknown {declared}");
    }

    // Read access is declared; write and network access are not.
    assert!(manifest.has_capability(Capability::FileSystemRead.manifest_name()));
    assert!(manifest.enforce_filesystem_access(false).is_ok());
    assert!(manifest.enforce_filesystem_access(true).is_err());
    assert!(manifest.enforce_network_access().is_err());
}
