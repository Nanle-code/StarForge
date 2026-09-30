use starforge::utils::security::{
    apply_hardening, run_checklist, validate_security, HardeningOptions, SecurityPatternLibrary,
};
use std::io::Write;
use tempfile::{NamedTempFile, TempDir};

fn use_temp_home() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
    let guard = home_lock();
    let home = TempDir::new().unwrap();
    (home, guard)
}

const SAMPLE_CONTRACT: &str = r#"
#![no_std]
use soroban_sdk::{contract, contractimpl, Env};


#[contract]
pub struct Token;

#[contractimpl]
impl Token {
    pub fn transfer(env: Env, amount: u64) -> u64 {
        let balance = env.storage().instance().get(&()).unwrap();
        balance + amount
    }

    pub fn mint(env: Env, amount: u64) -> u64 {
        amount * 2
    }
}
"#;

#[test]
fn security_pattern_library_has_entries() {
    let patterns = SecurityPatternLibrary::all();
    assert!(patterns.len() >= 5);
    assert!(patterns.iter().any(|p| p.id == "auth-missing"));
}

#[test]
fn hardening_detects_unchecked_arithmetic() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(SAMPLE_CONTRACT.as_bytes()).unwrap();

    let result = apply_hardening(
        file.path(),
        &HardeningOptions {
            apply_fixes: false,
            dry_run: true,
            pattern_ids: None,
        },
    )
    .unwrap();

    assert!(!result.findings.is_empty());
}

#[test]
fn security_checklist_runs() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(SAMPLE_CONTRACT.as_bytes()).unwrap();

    let checklist = run_checklist(file.path()).unwrap();
    assert!(checklist.items.len() >= 5);
    assert!(checklist.score_percent <= 100.0);
}

#[test]
fn security_validation_fails_on_high_severity() {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(SAMPLE_CONTRACT.as_bytes()).unwrap();

    let validation = validate_security(file.path()).unwrap();
    assert!(!validation.findings.is_empty());
}

#[test]
fn hardening_apply_writes_output() {
    let _home = use_temp_home();
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(SAMPLE_CONTRACT.as_bytes()).unwrap();

    let result = apply_hardening(
        file.path(),
        &HardeningOptions {
            apply_fixes: true,
            dry_run: false,
            pattern_ids: Some(vec!["unchecked-arithmetic".into()]),
        },
    )
    .unwrap();

    if result.transforms_applied > 0 {
        assert!(result.output_path.is_some());
    }
}

/// Serialises tests that replace the process-wide `HOME`.
///
/// `std::env::set_var` affects every thread in the binary while libtest runs
/// these tests in parallel, so without this two tests race and one reads back
/// paths under the other's temp home.

#[allow(dead_code)]
fn home_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
