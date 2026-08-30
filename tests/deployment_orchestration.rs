use starforge::utils::deploy_orchestrator::{
    build_plan, execute_plan, load_manifest, resolve_order, rollback,
};
use std::io::Write;
use tempfile::TempDir;

fn use_temp_home() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
    let guard = home_lock();
    let home = TempDir::new().unwrap();
    std::env::set_var("HOME", home.path());
    (home, guard)
}

fn write_minimal_wasm(path: &std::path::Path) {
    let mut bytes = b"\0asm\x01\0\0\0".to_vec();
    bytes.extend(vec![0u8; 64]);
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn resolves_dependency_order() {
    let dir = TempDir::new().unwrap();
    let wasm_a = dir.path().join("a.wasm");
    let wasm_b = dir.path().join("b.wasm");
    write_minimal_wasm(&wasm_a);
    write_minimal_wasm(&wasm_b);

    let manifest_path = dir.path().join("manifest.json");
    let mut f = std::fs::File::create(&manifest_path).unwrap();
    write!(
        f,
        r#"{{
            "name": "test-stack",
            "network": "testnet",
            "contracts": [
                {{ "id": "b", "wasm": "{}", "depends_on": ["a"] }},
                {{ "id": "a", "wasm": "{}", "depends_on": [] }}
            ]
        }}"#,
        wasm_b.display(),
        wasm_a.display()
    )
    .unwrap();

    let manifest = load_manifest(&manifest_path).unwrap();
    let order = resolve_order(&manifest).unwrap();
    assert_eq!(order, vec!["a", "b"]);
}

#[test]
fn build_and_execute_plan_dry_run() {
    let _home = use_temp_home();
    let dir = TempDir::new().unwrap();
    let wasm = dir.path().join("c.wasm");
    write_minimal_wasm(&wasm);

    let manifest_path = dir.path().join("manifest.json");
    std::fs::write(
        &manifest_path,
        format!(
            r#"{{
                "name": "solo",
                "network": "testnet",
                "contracts": [{{ "id": "solo", "wasm": "{}", "depends_on": [] }}]
            }}"#,
            wasm.display()
        ),
    )
    .unwrap();

    let manifest = load_manifest(&manifest_path).unwrap();
    let mut state = build_plan(&manifest).unwrap();
    execute_plan(&mut state, true).unwrap();
    assert_eq!(
        state.steps[0].status,
        starforge::utils::deploy_orchestrator::DeployStepStatus::Deployed
    );

    let rolled = rollback(&mut state).unwrap();
    assert_eq!(rolled, vec!["solo"]);
}

#[test]
fn detects_circular_dependencies() {
    let dir = TempDir::new().unwrap();
    let wasm = dir.path().join("x.wasm");
    write_minimal_wasm(&wasm);

    let manifest_path = dir.path().join("bad.json");
    std::fs::write(
        &manifest_path,
        format!(
            r#"{{
                "name": "cycle",
                "network": "testnet",
                "contracts": [
                    {{ "id": "a", "wasm": "{}", "depends_on": ["b"] }},
                    {{ "id": "b", "wasm": "{}", "depends_on": ["a"] }}
                ]
            }}"#,
            wasm.display(),
            wasm.display()
        ),
    )
    .unwrap();

    let manifest = load_manifest(&manifest_path).unwrap();
    assert!(resolve_order(&manifest).is_err());
}

/// Serialises tests that replace the process-wide `HOME`.
///
/// `std::env::set_var` affects every thread in the binary while libtest runs
/// these tests in parallel, so without this two tests race and one reads back
/// paths under the other's temp home.
fn home_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
