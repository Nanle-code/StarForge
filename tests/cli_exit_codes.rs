//! Unit and integration tests for standardized CLI exit codes.
//!
//! Issue #672: Verifies that usage, configuration, network, signing,
//! execution, environment, and general failure errors map to stable,
//! documented exit codes.

use anyhow::anyhow;
use starforge::utils::exit_codes::{determine_exit_code, ExitCode};

// ── 1. Numeric value and specification verification ────────────────────────────

#[test]
fn test_exit_code_numeric_values() {
    assert_eq!(ExitCode::Success.code(), 0);
    assert_eq!(ExitCode::GeneralFailure.code(), 1);
    assert_eq!(ExitCode::Usage.code(), 2);
    assert_eq!(ExitCode::Config.code(), 3);
    assert_eq!(ExitCode::Network.code(), 4);
    assert_eq!(ExitCode::Signing.code(), 5);
    assert_eq!(ExitCode::Execution.code(), 6);
    assert_eq!(ExitCode::Environment.code(), 7);
}

#[test]
fn test_exit_code_names_and_descriptions() {
    assert_eq!(ExitCode::Success.name(), "SUCCESS");
    assert_eq!(ExitCode::Usage.name(), "USAGE_ERROR");
    assert_eq!(ExitCode::Config.name(), "CONFIG_ERROR");
    assert_eq!(ExitCode::Network.name(), "NETWORK_ERROR");
    assert_eq!(ExitCode::Signing.name(), "SIGNING_ERROR");
    assert_eq!(ExitCode::Execution.name(), "EXECUTION_ERROR");
    assert_eq!(ExitCode::Environment.name(), "ENVIRONMENT_ERROR");

    assert!(!ExitCode::Usage.description().is_empty());
    assert!(!ExitCode::Config.description().is_empty());
    assert!(!ExitCode::Network.description().is_empty());
    assert!(!ExitCode::Signing.description().is_empty());
    assert!(!ExitCode::Execution.description().is_empty());
    assert!(!ExitCode::Environment.description().is_empty());
}

// ── 2. Error classification tests (determine_exit_code) ───────────────────────

#[test]
fn test_classify_usage_errors() {
    let err1 = anyhow!("Invalid correlation ID: must be 8-64 chars");
    assert_eq!(determine_exit_code(&err1), ExitCode::Usage);

    let err2 = anyhow!("Invalid public key: expected 56 characters, got 10");
    assert_eq!(determine_exit_code(&err2), ExitCode::Usage);

    let err3 = anyhow!("Invalid argument '--foo'");
    assert_eq!(determine_exit_code(&err3), ExitCode::Usage);

    let err4 = anyhow!("Missing required argument '<name>'");
    assert_eq!(determine_exit_code(&err4), ExitCode::Usage);
}

#[test]
fn test_classify_signing_errors() {
    let err1 = anyhow!("Invalid secret key: must start with 'S'");
    assert_eq!(determine_exit_code(&err1), ExitCode::Signing);

    let err2 = anyhow!("Failed to decrypt secret bundle with passphrase");
    assert_eq!(determine_exit_code(&err2), ExitCode::Signing);

    let err3 = anyhow!("Wallet encryption keypair generation failed");
    assert_eq!(determine_exit_code(&err3), ExitCode::Signing);
}

#[test]
fn test_classify_network_errors() {
    let err1 = anyhow!("Failed to fetch Horizon endpoint: Connection refused");
    assert_eq!(determine_exit_code(&err1), ExitCode::Network);

    let err2 = anyhow!("Friendbot request timed out after 30s");
    assert_eq!(determine_exit_code(&err2), ExitCode::Network);

    let err3 = anyhow!("Soroban RPC node unreachable at http://localhost:8000/rpc");
    assert_eq!(determine_exit_code(&err3), ExitCode::Network);
}

#[test]
fn test_classify_config_errors() {
    let err1 = anyhow!("Failed to parse config.toml at ~/.starforge/config.toml");
    assert_eq!(determine_exit_code(&err1), ExitCode::Config);

    let err2 = anyhow!("Unsupported network 'devnet-custom'. Use 'testnet' or 'mainnet'");
    assert_eq!(determine_exit_code(&err2), ExitCode::Config);

    let err3 = anyhow!("Duplicate wallet name 'alice' in config");
    assert_eq!(determine_exit_code(&err3), ExitCode::Config);
}

#[test]
fn test_classify_execution_errors() {
    let err1 = anyhow!("WASM verification failed for contract bytecode");
    assert_eq!(determine_exit_code(&err1), ExitCode::Execution);

    let err2 = anyhow!("Soroban transaction simulation reverted with code -1");
    assert_eq!(determine_exit_code(&err2), ExitCode::Execution);

    let err3 = anyhow!("Contract compilation build failed: rustc exited 101");
    assert_eq!(determine_exit_code(&err3), ExitCode::Execution);
}

#[test]
fn test_classify_environment_errors() {
    let err1 = anyhow!("Docker daemon is not running or not installed");
    assert_eq!(determine_exit_code(&err1), ExitCode::Environment);

    let err2 = anyhow!("Permission denied (os error 13) when writing binary");
    assert_eq!(determine_exit_code(&err2), ExitCode::Environment);

    let err3 = anyhow!("Unsupported operating system: freebsd");
    assert_eq!(determine_exit_code(&err3), ExitCode::Environment);
}

#[test]
fn test_classify_general_failure_fallback() {
    let err = anyhow!("An unexpected internal computation state occurred");
    assert_eq!(determine_exit_code(&err), ExitCode::GeneralFailure);
}

// ── 3. Chained error context classification ───────────────────────────────────

#[test]
fn test_classify_chained_context_error() {
    let root = anyhow!("connection reset by peer");
    let chained = root.context("Failed to load active configuration");
    // Should classify as Network or Config based on chain content
    let code = determine_exit_code(&chained);
    assert!(code == ExitCode::Network || code == ExitCode::Config);
}
