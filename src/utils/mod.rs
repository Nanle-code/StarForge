// AI Context Management (#487)
// AI Deployment Planner
// AI Rate Limiting (#489)
// AI Request Caching (#483)
// AI Response Validation (#486)
// AI Service Abstraction Layer (#479)
// AI Test Analytics (#570)
pub mod ai;
pub mod ai_accessibility;
pub mod ai_cache;
pub mod ai_context;
pub mod ai_conversation;
pub mod ai_debug_enhancement;
pub mod ai_debugger;
pub mod ai_deployment_planner;
pub mod ai_deployment_testing;
pub mod ai_doc_qa;
pub mod ai_docs;
pub mod ai_documentation_assistant;
pub mod ai_error_handler;
pub mod ai_feedback;
pub mod ai_gas_estimation;
pub mod ai_ide_integration;
pub mod ai_model_router;
pub mod ai_navigation;
pub mod ai_performance_profiler;
pub mod ai_project_planner;
pub mod ai_property_testing;
pub mod ai_quality_gates;
pub mod ai_rate_limiter;
pub mod ai_recommendations;
pub mod ai_refactor;
pub mod ai_search;
pub mod ai_security_training;
pub mod ai_telemetry;
pub mod ai_template_testing;
pub mod ai_test_analytics;
pub mod ai_test_assistant;
pub mod ai_test_generator;
pub mod ai_test_maintenance;
pub mod ai_tutorial;
pub mod ai_validation;
pub mod approval_engine;
pub mod audit;
pub mod audit_bundle;
pub mod backup;
pub mod benchmarking;
pub mod bindings;
pub mod bridge;
pub mod call_graph;
pub mod cargo_metadata;
pub mod completion;
pub mod compliance;
pub mod config;
pub mod confirmation;
pub mod context_help;
pub mod contract_assertions;
pub mod contract_deps;
pub mod contract_fixtures;
pub mod contract_mocks;
pub mod contract_profiler;
pub mod contract_suggestions;
pub mod contract_test_framework;
pub mod contract_test_runner;
pub mod contract_testing;
pub mod contract_versioning;
pub mod correlation;
pub mod cost_estimation;
pub mod cost_management;
pub mod crypto;
pub mod database;
pub mod debugger;
pub mod deploy_history;
pub mod deploy_orchestrator;
pub mod deployment_automation;
pub mod deployment_checkpoint;
pub mod deployment_monitor;
pub mod deployment_monitoring_service;
pub mod deployment_optimizer;
pub mod deployment_verify;
pub mod doc_api_ref;
pub mod doc_extractor;
pub mod doc_generator;
pub mod doc_html;
pub mod doc_publisher;
pub mod doc_templates;
pub mod docs;
pub mod documentation;
pub mod event_monitoring;
pub mod feature_flags;
pub mod gas_analyzer;
pub mod gas_report;
pub mod governance;
pub mod hardware_wallet;
pub mod help_metadata;
pub mod history;
pub mod history_search;
pub mod horizon;
pub mod http_client;
pub mod interactive;
pub mod latency_budget;
pub mod logging;
pub mod migration_ai;
pub mod migration_testing;
pub mod mnemonic;
pub mod mock_soroban;
pub mod multi_network_deploy;
pub mod multisig;
pub mod multisig_builder;
pub mod mutation;
pub mod network_sim;
pub mod network_simulator;
pub mod node;
pub mod notifications;
pub mod ollama;
pub mod optimizer;
pub mod orchestration;
pub mod output;
pub mod pattern_library;
pub mod performance;
pub mod pipeline_builder;
pub mod print;
pub mod privacy;
pub mod profiler;
pub mod prompt_manager;
pub mod quality_analysis;
pub mod redaction;
pub mod registry;
pub mod repl;
pub mod rollback_testing;
pub mod sandbox;
pub mod scheduler;
pub mod security;
pub mod security_scanner;
pub mod shamir;
pub mod simulation_resources;
pub mod social;
pub mod soroban;
pub mod state_diff;
pub mod state_transition;
pub mod stream;
pub mod telemetry;
pub mod template;
pub mod template_analytics;
pub mod template_customization_ai;
pub mod template_integration;
pub mod template_performance;
pub mod template_recommender;
pub mod template_security_scanner;
pub mod template_vcs;
pub mod template_version_ai;
pub mod templates;
pub mod test_automation;
pub mod test_coverage;
pub mod test_generator;
pub mod test_optimizer;
pub mod test_runner;
pub mod testnet_integration;
pub mod tutorial_engine;
pub mod tx_batch;
pub mod wallet_import;
pub mod wallet_signer;
pub mod wasm_hash;
pub mod workflow_guidance;

pub mod wasm_preflight;

// Contract monitoring and alerting (#374)
pub mod contract_health_monitor;

#[cfg(test)]
pub(crate) fn lock_home_env() -> std::sync::MutexGuard<'static, ()> {
    static HOME_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    HOME_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
