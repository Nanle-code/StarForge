//! Noun-verb command groups introduced by ADR 0007.
//!
//! Most of the consolidated tree lives in the modules that already owned the
//! command (`wallet`, `contract`, `network`, `template`, `config`, `project`):
//! their existing subcommand enum simply gained the moved commands as
//! additional verbs, so no argument definition is ever duplicated.
//!
//! This module holds the groups whose old top-level command was a leaf argument
//! struct or a single subtree, and therefore has no existing enum to extend:
//!
//! * [`AiTree`] — `ai` was one local-assistant subtree; the other 22 `ai-*`
//!   commands become its siblings, and the old subtree becomes `ai local`.
//! * [`DeployTree`] — `deploy` was the flat `deploy --wasm …` form; it is now
//!   the verb `deploy run` next to the deployment lifecycle commands.
//! * [`ToolTree`] — developer-environment utilities that do not belong to any
//!   single domain noun.
//!
//! See `docs/adr/0007-noun-verb-command-tree.md` and the migration table in
//! `docs/CLI_COMMAND_TREE.md`.

use crate::commands;
use anyhow::Result;
use clap::Subcommand;

/// `starforge deploy` — deploy a contract and manage the deployment lifecycle.
#[derive(Subcommand)]
pub enum DeployTree {
    /// Deploy a compiled Soroban contract (.wasm)
    Run {
        #[command(flatten)]
        args: commands::deploy::DeployArgs,
    },

    /// Deployment history, rollback, verification, and dashboard
    #[command(subcommand)]
    History(commands::deployments::DeploymentsCommands),

    /// Manage deployment environments (dev/staging/production)
    #[command(subcommand)]
    Env(commands::environment::EnvironmentCommands),

    /// Schedule deployments for future execution with approval workflows
    #[command(subcommand)]
    Schedule(commands::schedule::ScheduleCommands),

    /// Multi-contract deployment orchestration
    #[command(subcommand)]
    Orchestrate(commands::orchestrate::OrchestrateCommands),

    /// Visual pipeline builder for contract deployment workflows
    #[command(subcommand)]
    Pipeline(commands::pipeline_builder::PipelineCommands),

    /// Approval workflow for contract deployments (multi-level approvals, audit, compliance)
    #[command(subcommand)]
    Approval(commands::approval::ApprovalCommands),

    /// AI-assisted deployment cost management: budgets, forecasting, reporting
    #[command(subcommand)]
    Cost(commands::cost::CostCommands),

    /// Contract deployment analytics, dashboards, and reporting
    #[command(subcommand)]
    Analytics(commands::analytics::AnalyticsCommands),

    /// Backup and disaster recovery for contract state and code
    #[command(subcommand)]
    Backup(commands::backup::BackupCommands),
}

/// `starforge ai` — every AI feature, as a verb under one noun.
#[derive(Subcommand)]
pub enum AiTree {
    /// Local LLM assistant powered by Ollama (status, models, pull, ask, audit)
    #[command(subcommand)]
    Local(commands::ai::AiCommands),

    /// AI-powered contract debugging assistant (error analysis, fix suggestions)
    #[command(subcommand)]
    Debug(commands::ai_debug::AiDebugCommands),

    /// AI-driven definitions, references, code graphs, and contextual search
    #[command(subcommand)]
    Navigate(commands::ai_navigate::AiNavigateCommands),

    /// Configurable code quality, security, performance, coverage, docs, and license gates
    #[command(subcommand)]
    Gate(commands::ai_quality_gate::AiQualityGateCommands),

    /// AI-driven performance profiling
    #[command(subcommand)]
    Profiling(commands::ai_profile::AiProfileCommands),

    /// AI-powered IDE integration (editor snippets, task providers)
    #[command(subcommand)]
    Ide(commands::ai_ide::AiIdeCommands),

    /// AI-driven testing assistance (generate, optimize, analyze)
    #[command(subcommand)]
    Tests(commands::ai_test::AiTestCommands),

    /// AI-driven test maintenance
    #[command(subcommand)]
    TestMaintain(commands::ai_test_maintain::AiTestMaintainCommands),

    /// AI-driven deployment testing
    #[command(subcommand)]
    DeployTest(commands::ai_deployment_test::AiDeploymentTestCommands),

    /// AI property-based testing (discover properties, validate invariants)
    #[command(subcommand)]
    PropertyTest(commands::ai_property_test::AiPropertyTestCommands),

    /// AI feedback and learning system (record feedback, track quality)
    #[command(subcommand)]
    Feedback(commands::ai_feedback::AiFeedbackCommands),

    /// AI code search and discovery (search code, find patterns)
    #[command(subcommand)]
    Search(commands::ai_search::AiSearchCommands),

    /// AI best practice recommendations (analyze contracts, scan projects)
    #[command(subcommand)]
    Recommend(commands::ai_recommend::AiRecommendCommands),

    /// Intelligent AI model selection and routing by task complexity
    #[command(subcommand)]
    Route(commands::ai_model_router::AiModelRouterCommands),

    /// AI project planning assistant — requirements, architecture, timeline, risks
    #[command(subcommand)]
    Plan(commands::ai_plan::AiPlanCommands),

    /// AI accessibility features — screen reader, voice commands, text simplification
    #[command(subcommand)]
    Accessibility(commands::ai_accessibility::AiAccessibilityCommands),

    /// AI contract function suggestions based on contract type
    #[command(subcommand)]
    Suggest(commands::ai_contract_suggest::AiContractSuggestCommands),

    /// AI documentation Q&A with citations
    #[command(subcommand)]
    Docs(commands::ai_doc_qa::AiDocQaCommands),

    /// AI usage telemetry and analytics: calls, tokens, latency, cost, opt-out
    #[command(subcommand)]
    Telemetry(commands::ai_telemetry::AiTelemetryCommands),

    /// AI-driven security training: lessons, exercises, progress tracking
    #[command(subcommand)]
    Training(commands::ai_security_training::AiSecurityTrainingCommands),

    /// AI-powered security audit for Soroban contracts
    SecurityAudit {
        #[command(flatten)]
        args: commands::ai_audit::AiAuditArgs,
    },

    /// Manage AI prompt templates and versioning
    #[command(subcommand)]
    Prompts(commands::prompts::PromptsCommands),

    /// AI contextual help: command, workflow, error, and best-practice guidance
    Help {
        #[command(flatten)]
        args: commands::help::HelpArgs,
    },
}

/// `starforge tool` — developer-environment utilities with no domain noun.
#[derive(Subcommand)]
pub enum ToolTree {
    /// Interactive CLI tutorials
    #[command(subcommand)]
    Tutorial(commands::tutorial::TutorialCommands),

    /// Natural language command interface
    Nl {
        #[command(flatten)]
        args: commands::nl::NlArgs,
    },

    /// Check PR readiness (CI status and merge conflicts)
    #[command(subcommand)]
    Pr(commands::pr::PrCommands),

    /// Collect environment diagnostics and generate a prefilled bug report
    BugReport {
        #[command(flatten)]
        args: commands::bug_report::BugReportArgs,
    },
}

/// Dispatch `starforge deploy …` to the module that owns each verb.
pub async fn handle_deploy(cmd: DeployTree) -> Result<()> {
    match cmd {
        DeployTree::Run { args } => commands::deploy::handle(args).await,
        DeployTree::History(cmd) => commands::deployments::handle(cmd).await,
        DeployTree::Env(cmd) => commands::environment::handle(cmd),
        DeployTree::Schedule(cmd) => commands::schedule::handle(cmd).await,
        DeployTree::Orchestrate(cmd) => commands::orchestrate::handle(cmd).await,
        DeployTree::Pipeline(cmd) => commands::pipeline_builder::handle(cmd).await,
        DeployTree::Approval(cmd) => commands::approval::handle(cmd).await,
        DeployTree::Cost(cmd) => commands::cost::handle(cmd).await,
        DeployTree::Analytics(cmd) => commands::analytics::handle(cmd).await,
        DeployTree::Backup(cmd) => commands::backup::handle(cmd).await,
    }
}

/// Dispatch `starforge ai …` to the module that owns each verb.
pub async fn handle_ai(cmd: AiTree) -> Result<()> {
    match cmd {
        AiTree::Local(cmd) => commands::ai::handle(cmd).await,
        AiTree::Debug(cmd) => commands::ai_debug::handle(cmd).await,
        AiTree::Navigate(cmd) => commands::ai_navigate::handle(cmd),
        AiTree::Gate(cmd) => commands::ai_quality_gate::handle(cmd),
        AiTree::Profiling(cmd) => commands::ai_profile::handle(cmd).await,
        AiTree::Ide(cmd) => commands::ai_ide::handle(cmd).await,
        AiTree::Tests(cmd) => commands::ai_test::handle(cmd).await,
        AiTree::TestMaintain(cmd) => commands::ai_test_maintain::handle(cmd).await,
        AiTree::DeployTest(cmd) => commands::ai_deployment_test::handle(cmd).await,
        AiTree::PropertyTest(cmd) => commands::ai_property_test::handle(cmd).await,
        AiTree::Feedback(cmd) => commands::ai_feedback::handle(cmd).await,
        AiTree::Search(cmd) => commands::ai_search::handle(cmd).await,
        AiTree::Recommend(cmd) => commands::ai_recommend::handle(cmd).await,
        AiTree::Route(cmd) => commands::ai_model_router::handle(cmd).await,
        AiTree::Plan(cmd) => commands::ai_plan::handle(cmd).await,
        AiTree::Accessibility(cmd) => commands::ai_accessibility::handle(cmd).await,
        AiTree::Suggest(cmd) => commands::ai_contract_suggest::handle(cmd).await,
        AiTree::Docs(cmd) => commands::ai_doc_qa::handle(cmd).await,
        AiTree::Telemetry(cmd) => commands::ai_telemetry::handle(cmd).await,
        AiTree::Training(cmd) => commands::ai_security_training::handle(cmd).await,
        AiTree::SecurityAudit { args } => commands::ai_audit::handle(args).await,
        AiTree::Prompts(cmd) => commands::prompts::handle(&cmd).await,
        AiTree::Help { args } => commands::help::handle(args).await,
    }
}

/// Dispatch `starforge tool …` to the module that owns each verb.
pub async fn handle_tool(cmd: ToolTree) -> Result<()> {
    match cmd {
        ToolTree::Tutorial(cmd) => commands::tutorial::handle(cmd).await,
        ToolTree::Nl { args } => commands::nl::handle(args).await,
        ToolTree::Pr(cmd) => commands::pr::handle(cmd).await,
        ToolTree::BugReport { args } => commands::bug_report::handle(args).await,
    }
}
