//! Integration tests for offline-first AI mode and the cloud provider
//! feature-parity matrix (see `docs/OFFLINE_AI.md`).
//!
//! These tests are deterministic and network-free: they exercise the public
//! `ai_offline` API and the cloud-only guards that fire *before* any network
//! or file I/O, so they are safe to run in CI even without Ollama or API keys.

use starforge::commands;
use starforge::utils::ai_offline::{self, AiMode, OfflineSupport, ResolvedMode};

#[test]
fn parity_matrix_is_nonempty_and_well_formed() {
    assert!(!ai_offline::AI_PARITY.is_empty());
    for e in ai_offline::AI_PARITY {
        assert!(!e.command.is_empty());
        assert!(!e.description.is_empty());
        assert!(!e.providers.is_empty());
        assert!(!e.providers.iter().any(|p| p.is_empty()));
    }
}

#[test]
fn parity_matrix_includes_each_support_tier() {
    let kinds: Vec<OfflineSupport> = ai_offline::AI_PARITY.iter().map(|e| e.support).collect();
    assert!(kinds.contains(&OfflineSupport::Local));
    assert!(kinds.contains(&OfflineSupport::Hybrid));
    assert!(kinds.contains(&OfflineSupport::CloudOnly));
}

#[test]
fn generated_and_explain_are_cloud_only() {
    assert!(ai_offline::is_cloud_only("generate"));
    assert!(ai_offline::is_cloud_only("explain"));
    assert!(!ai_offline::works_offline("generate"));
    // And the local assistant is explicitly not cloud-only.
    assert!(!ai_offline::is_cloud_only("ai ask"));
    assert!(ai_offline::works_offline("ai ask"));
}

#[test]
fn offline_subset_contains_the_documented_local_commands() {
    let offline: Vec<&str> = ai_offline::offline_commands().map(|e| e.command).collect();
    for cmd in [
        "ai status",
        "ai models",
        "ai audit",
        "ai explain",
        "ai test",
        "ai optimise",
        "ai profile",
        "ai patterns",
    ] {
        assert!(offline.contains(&cmd), "expected '{cmd}' in offline subset");
    }
}

#[test]
fn cloud_only_feature_fails_clearly_in_offline_mode() {
    let resolved = ResolvedMode::Offline;
    let err = ai_offline::require_offline_compatible("generate", resolved).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("cloud-only"), "message: {msg}");
    assert!(msg.contains("generate"), "message: {msg}");
    assert!(msg.contains("offline"), "message: {msg}");

    // Same command is permitted online.
    assert!(ai_offline::require_offline_compatible("generate", ResolvedMode::Online).is_ok());
}

#[test]
fn cloud_only_feature_also_blocks_offline_degraded() {
    // Degraded (offline requested but no local model) must still block cloud.
    let err = ai_offline::require_offline_compatible("explain", ResolvedMode::OfflineDegraded);
    assert!(err.is_err());
}

#[test]
fn local_command_is_never_cloud_blocked() {
    assert!(ai_offline::require_offline_compatible("ai audit", ResolvedMode::Offline).is_ok());
    assert!(
        ai_offline::require_offline_compatible("ai audit", ResolvedMode::OfflineDegraded).is_ok()
    );
}

#[test]
fn unavailable_model_error_is_actionable() {
    let models = vec![starforge::utils::ollama::OllamaModel {
        name: "codellama:7b".into(),
        size: 0,
        modified_at: "".into(),
        digest: "".into(),
    }];

    // Present: bare and exact.
    assert!(ai_offline::model_present(&models, "codellama"));
    assert!(ai_offline::model_present(&models, "codellama:7b"));
    assert!(ai_offline::ensure_model_available(&models, "codellama:7b").is_ok());

    // Missing: clear error tells the user how to pull it.
    let err = ai_offline::ensure_model_available(&models, "mistral").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("mistral"));
    assert!(msg.contains("starforge ai pull mistral"));
    assert!(msg.contains("not available"));

    // A different tag is NOT the same model.
    assert!(!ai_offline::model_present(&models, "codellama:13b"));
}

#[test]
fn mode_parsing_and_resolution() {
    assert_eq!(
        ai_offline::parse_ai_mode("offline").unwrap(),
        AiMode::Offline
    );
    assert_eq!(ai_offline::parse_ai_mode("cloud").unwrap(), AiMode::Online);
    assert!(ai_offline::parse_ai_mode("nope").is_err());

    // Offline is always honoured, even without a local backend.
    assert_eq!(
        ai_offline::resolve_mode(AiMode::Offline, false),
        ResolvedMode::OfflineDegraded
    );
    assert!(ResolvedMode::OfflineDegraded.is_offline());
    assert!(!ResolvedMode::OfflineDegraded.allows_cloud());
}

// ── Enforcement wired into the real command handlers ─────────────────────────

/// In offline mode, `starforge generate` (cloud-only) fails clearly before
/// making any network call or even requiring an API key.
#[tokio::test]
async fn generate_handler_is_cloud_blocked_in_offline_mode() {
    std::env::set_var("STARFORGE_AI_MODE", "offline");
    let cmd = commands::generate::GenerateCommands::Contract {
        prompt: "NFT contract".into(),
        out: std::path::PathBuf::from("nft.rs"),
    };
    let err = commands::generate::handle(&cmd).await.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("cloud-only"), "message: {msg}");
    assert!(msg.contains("generate"), "message: {msg}");
    std::env::remove_var("STARFORGE_AI_MODE");
}

/// In offline mode, `starforge explain` (cloud-only) fails clearly before
/// reading the file or contacting OpenAI.
#[tokio::test]
async fn explain_handler_is_cloud_blocked_in_offline_mode() {
    std::env::set_var("STARFORGE_AI_MODE", "offline");
    let cmd = commands::explain::ExplainCommands::Contract {
        file: std::path::PathBuf::from("does-not-exist.rs"),
        level: "intermediate".into(),
        lang: "English".into(),
    };
    let err = commands::explain::handle(&cmd).await.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("cloud-only"), "message: {msg}");
    assert!(msg.contains("explain"), "message: {msg}");
    std::env::remove_var("STARFORGE_AI_MODE");
}

/// `starforge ai offline check <cloud-only>` surfaces the same clear error
/// through the CLI dispatch path.
#[tokio::test]
async fn offline_check_reports_cloud_only_feature() {
    std::env::set_var("STARFORGE_AI_MODE", "offline");
    let cmd = commands::ai::AiCommands::Offline(commands::ai::AiOfflineCommands::Check {
        command: "generate".into(),
    });
    let err = commands::ai::handle(cmd).await.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("cloud-only"), "message: {msg}");
    std::env::remove_var("STARFORGE_AI_MODE");
}

/// `starforge ai offline check <local>` reports success in offline mode.
#[tokio::test]
async fn offline_check_accepts_local_command() {
    std::env::set_var("STARFORGE_AI_MODE", "offline");
    let cmd = commands::ai::AiCommands::Offline(commands::ai::AiOfflineCommands::Check {
        command: "ai audit".into(),
    });
    let res = commands::ai::handle(cmd).await;
    assert!(res.is_ok(), "expected local command to pass, got: {res:?}");
    std::env::remove_var("STARFORGE_AI_MODE");
}
