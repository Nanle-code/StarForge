//! CLI integration tests for the AI Contract Completion Assistant
//! (`starforge complete …`). No network required.

use std::process::Command;

fn isolated_home() -> tempfile::TempDir {
    tempfile::tempdir().expect("create isolated home")
}

fn starforge(home: &std::path::Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_starforge"));
    cmd.arg("-q");
    cmd.env("STARFORGE_HOME", home);
    cmd.env("USERPROFILE", home);
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

fn write_file(home: &std::path::Path, name: &str, contents: &str) -> std::path::PathBuf {
    let path = home.join(name);
    std::fs::write(&path, contents).expect("write source file");
    path
}

#[test]
fn complete_help_lists_subcommands() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args(["complete", "--help"])
        .output()
        .expect("spawn complete help");
    assert_success(&output, "starforge complete --help");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("suggest"));
    assert!(stdout.contains("boilerplate"));
    assert!(stdout.contains("stub"));
    assert!(stdout.contains("imports"));
    assert!(stdout.contains("infer"));
}

#[test]
fn boilerplate_contract_emits_scaffold() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args(["complete", "boilerplate", "contract", "--name", "Vault"])
        .output()
        .expect("spawn boilerplate contract");
    assert_success(&output, "starforge complete boilerplate contract");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("pub struct Vault;"));
    assert!(stdout.contains("#[contractimpl]"));
}

#[test]
fn boilerplate_unknown_kind_fails() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args(["complete", "boilerplate", "notathing"])
        .output()
        .expect("spawn boilerplate bad");
    assert!(
        !output.status.success(),
        "expected non-zero exit for unknown boilerplate kind"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unknown boilerplate kind"));
}

#[test]
fn boilerplate_writes_output_file() {
    let home = isolated_home();
    let out = home.path().join("gen.rs");
    let output = starforge(home.path())
        .args([
            "complete",
            "boilerplate",
            "struct",
            "--name",
            "Account",
            "--output",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("spawn boilerplate output");
    assert_success(&output, "starforge complete boilerplate --output");
    assert!(out.exists());
    let contents = std::fs::read_to_string(&out).expect("read generated file");
    assert!(contents.contains("pub struct Account"));
    assert!(contents.contains("#[contracttype]"));
}

#[test]
fn suggest_empty_file_recommends_scaffold() {
    let home = isolated_home();
    let path = write_file(home.path(), "partial.rs", "");
    let output = starforge(home.path())
        .args(["complete", "suggest", path.to_str().unwrap(), "--json"])
        .output()
        .expect("spawn suggest empty");
    assert_success(&output, "starforge complete suggest empty");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("boilerplate"));
    assert!(stdout.contains("#[contract]"));
}

#[test]
fn suggest_reports_storage_context() {
    let home = isolated_home();
    let src = "\
#[contract]
pub struct C;
#[contractimpl]
impl C {
    pub fn f(env: Env) {
        env.storage()
";
    let path = write_file(home.path(), "storage.rs", src);
    let output = starforge(home.path())
        .args(["complete", "suggest", path.to_str().unwrap(), "--json"])
        .output()
        .expect("spawn suggest storage");
    assert_success(&output, "starforge complete suggest storage");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("storage"));
}

#[test]
fn imports_detects_missing_symbols() {
    let home = isolated_home();
    let src = "pub fn f(env: Env, a: Address) -> Symbol { symbol_short!(\"x\") }\n";
    let path = write_file(home.path(), "imports.rs", src);
    let output = starforge(home.path())
        .args(["complete", "imports", path.to_str().unwrap(), "--json"])
        .output()
        .expect("spawn imports");
    assert_success(&output, "starforge complete imports");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"Env\""));
    assert!(stdout.contains("\"Address\""));
    assert!(stdout.contains("use soroban_sdk::{"));
}

#[test]
fn infer_reports_binding_types() {
    let home = isolated_home();
    let src = "let a = true;\nlet b = 42;\nlet c = Address::from_string(&s);\n";
    let path = write_file(home.path(), "infer.rs", src);
    let output = starforge(home.path())
        .args(["complete", "infer", path.to_str().unwrap(), "--json"])
        .output()
        .expect("spawn infer");
    assert_success(&output, "starforge complete infer");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("bool"));
    assert!(stdout.contains("i128"));
    assert!(stdout.contains("Address"));
}

#[test]
fn stub_preview_lists_functions() {
    let home = isolated_home();
    let src = "\
pub fn a(env: Env) -> u32 {
}
pub fn b(env: Env) -> bool {
    todo!()
}
";
    let path = write_file(home.path(), "stub.rs", src);
    let output = starforge(home.path())
        .args(["complete", "stub", path.to_str().unwrap()])
        .output()
        .expect("spawn stub preview");
    assert_success(&output, "starforge complete stub");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("fn a"));
    assert!(stdout.contains("fn b"));
    // Preview must not modify the original file.
    let after = std::fs::read_to_string(&path).expect("read stub file");
    assert_eq!(after, src, "preview should not touch the source");
}

#[test]
fn stub_write_applies_bodies() {
    let home = isolated_home();
    let src = "pub fn flag(env: Env) -> bool {\n}\n";
    let path = write_file(home.path(), "apply.rs", src);
    let output = starforge(home.path())
        .args(["complete", "stub", path.to_str().unwrap(), "--write"])
        .output()
        .expect("spawn stub write");
    assert_success(&output, "starforge complete stub --write");
    let after = std::fs::read_to_string(&path).expect("read written file");
    assert!(
        after.contains("false"),
        "generated body should return false"
    );
    assert!(after.contains("// TODO: implement"));
    // Braces stay balanced.
    assert_eq!(after.matches('{').count(), after.matches('}').count());
}

#[test]
fn suggest_missing_file_fails() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args(["complete", "suggest", "does-not-exist.rs"])
        .output()
        .expect("spawn suggest missing");
    assert!(
        !output.status.success(),
        "expected non-zero exit for missing file"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("does not exist") || stderr.contains("File does not exist"));
}
