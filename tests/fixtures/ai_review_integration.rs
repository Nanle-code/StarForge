use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;

#[test]
#[ignore] // Requires a running Ollama instance with a model.
fn test_ai_review_markdown_output() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = tempfile::tempdir()?;
    let file_path = temp_dir.path().join("test_contract.rs");
    let code = r#"
#[contract]
pub struct HelloContract;

#[contractimpl]
impl HelloContract {
    pub fn hello(env: Env, to: Symbol) -> Symbol {
        symbol_short!("Hello")
    }
}
"#;
    fs::write(&file_path, code)?;

    let mut cmd = Command::cargo_bin("starforge")?;
    cmd.arg("review")
        .arg(file_path.to_str().unwrap())
        .arg("--output")
        .arg("markdown");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("# AI Code Review Report"))
        .stdout(predicate::str::contains("Overall Score:"))
        .stdout(predicate::str::contains("Summary:"));

    Ok(())
}

#[test]
fn test_ai_review_file_not_found() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("starforge")?;
    cmd.arg("review").arg("non_existent_file.rs");

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("Failed to read file"));

    Ok(())
}
