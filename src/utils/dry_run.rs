//! Unified `--dry-run` semantics for every state-changing command (#943).
//!
//! `--dry-run` must mean exactly the same thing everywhere: simulate the
//! command fully, print a plan of every operation (and cost) that *would* be
//! performed, and then change nothing — no filesystem writes, no network
//! submissions.
//!
//! Rather than each command inventing its own dry-run output, mutating command
//! families build a [`DryRunPlan`] and call [`DryRunPlan::emit`]. The same plan
//! is rendered either as a human-readable table or, with `--json` (or the
//! global `STARFORGE_OUTPUT_JSON` mode), as the CLI's standard JSON envelope.
//!
//! The global `--dry-run` flag is wired up in `main.rs` through
//! [`set_enabled`]; command handlers call [`is_enabled`] and, when it is set,
//! emit their plan and return *before* performing any mutation.

use crate::utils::output;
use anyhow::Result;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};

static DRY_RUN_ENABLED: AtomicBool = AtomicBool::new(false);

/// Enable or disable dry-run mode for this process.
///
/// Called once from `main` with the value of the global `--dry-run` flag. It
/// is process-global because the flag is global (it can be passed before or
/// after the subcommand).
pub fn set_enabled(enabled: bool) {
    DRY_RUN_ENABLED.store(enabled, Ordering::SeqCst);
}

/// Whether the process is running under `--dry-run`.
pub fn is_enabled() -> bool {
    DRY_RUN_ENABLED.load(Ordering::SeqCst)
}

/// Render a boolean as a short human label for plan output.
pub fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

/// A single key/value row attached to a plan or operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanField {
    pub label: String,
    pub value: String,
}

impl PlanField {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }
}

/// One state-changing action that a command *would* perform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlannedOperation {
    /// Machine-friendly category, e.g. `config.write` or `wallet.write`.
    pub kind: String,
    /// What the operation acts on (file, wallet name, network, …).
    pub target: String,
    /// Human-readable description of the action.
    pub description: String,
    /// Extra key/value rows rendered under the operation.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<PlanField>,
}

impl PlannedOperation {
    pub fn new(
        kind: impl Into<String>,
        target: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            kind: kind.into(),
            target: target.into(),
            description: description.into(),
            details: Vec::new(),
        }
    }

    pub fn detail(mut self, label: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.push(PlanField::new(label, value));
        self
    }
}

/// The complete plan for a single command invocation under `--dry-run`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DryRunPlan {
    /// Command name, e.g. `config set`.
    pub command: String,
    /// One-line summary of what the command would do.
    pub summary: String,
    /// Target network, when the command operates on one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    /// Always `true`; present so JSON consumers can assert on it.
    pub dry_run: bool,
    pub operations: Vec<PlannedOperation>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub costs: Vec<PlanField>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// Whether applying the command would write to the filesystem.
    pub writes_filesystem: bool,
    /// Whether applying the command would submit a transaction.
    pub submits_transactions: bool,
}

impl DryRunPlan {
    pub fn new(command: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            summary: summary.into(),
            network: None,
            dry_run: true,
            operations: Vec::new(),
            costs: Vec::new(),
            warnings: Vec::new(),
            writes_filesystem: false,
            submits_transactions: false,
        }
    }

    pub fn network(mut self, network: impl Into<String>) -> Self {
        self.network = Some(network.into());
        self
    }

    pub fn maybe_network(mut self, network: Option<String>) -> Self {
        self.network = network;
        self
    }

    pub fn operation(mut self, operation: PlannedOperation) -> Self {
        self.operations.push(operation);
        self
    }

    /// Record an estimated cost or impact row.
    pub fn cost(mut self, label: impl Into<String>, value: impl Into<String>) -> Self {
        self.costs.push(PlanField::new(label, value));
        self
    }

    pub fn warn(mut self, message: impl Into<String>) -> Self {
        self.warnings.push(message.into());
        self
    }

    pub fn writes_filesystem(mut self) -> Self {
        self.writes_filesystem = true;
        self
    }

    pub fn submits_transactions(mut self) -> Self {
        self.submits_transactions = true;
        self
    }

    /// Render the plan as a human-readable table.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("Dry-run plan: {}\n", self.command));
        out.push_str(&format!("{}\n", self.summary));
        if let Some(network) = &self.network {
            out.push_str(&format!("Network: {network}\n"));
        }
        out.push('\n');

        out.push_str("Planned operations:\n");
        if self.operations.is_empty() {
            out.push_str("  (none)\n");
        } else {
            out.push_str(&format!(
                "  {:<4}{:<18}{:<32}{}\n",
                "#", "KIND", "TARGET", "DESCRIPTION"
            ));
            for (index, operation) in self.operations.iter().enumerate() {
                out.push_str(&format!(
                    "  {:<4}{:<18}{:<32}{}\n",
                    index + 1,
                    operation.kind,
                    operation.target,
                    operation.description
                ));
                for detail in &operation.details {
                    out.push_str(&format!("      {}: {}\n", detail.label, detail.value));
                }
            }
        }

        if !self.costs.is_empty() {
            out.push('\n');
            out.push_str("Estimated cost / impact:\n");
            for cost in &self.costs {
                out.push_str(&format!("  {}: {}\n", cost.label, cost.value));
            }
        }

        if !self.warnings.is_empty() {
            out.push('\n');
            out.push_str("Warnings:\n");
            for warning in &self.warnings {
                out.push_str(&format!("  ! {warning}\n"));
            }
        }

        out.push('\n');
        out.push_str(
            "No changes applied — this was a dry run. \
             Re-run without --dry-run to apply the plan.\n",
        );
        out
    }

    /// Print the plan, either as the CLI's JSON envelope or as plain text.
    pub fn emit(&self, json: bool) -> Result<()> {
        if json {
            output::print_json(self)
        } else {
            print!("{}", self.render());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_plan() -> DryRunPlan {
        DryRunPlan::new("config set", "Set configuration key 'telemetry.enabled'")
            .operation(
                PlannedOperation::new("config.write", "configuration store", "would persist a key")
                    .detail("Config file", "/tmp/config.toml"),
            )
            .cost("Filesystem writes", "1")
            .warn("example warning")
            .writes_filesystem()
    }

    #[test]
    fn render_includes_command_operations_and_no_change_notice() {
        let rendered = sample_plan().render();
        assert!(rendered.contains("Dry-run plan: config set"));
        assert!(rendered.contains("config.write"));
        assert!(rendered.contains("configuration store"));
        assert!(rendered.contains("would persist a key"));
        assert!(rendered.contains("Config file: /tmp/config.toml"));
        assert!(rendered.contains("No changes applied"));
    }

    #[test]
    fn render_lists_warnings_and_costs() {
        let rendered = sample_plan().render();
        assert!(rendered.contains("Estimated cost / impact"));
        assert!(rendered.contains("Filesystem writes: 1"));
        assert!(rendered.contains("example warning"));
    }

    #[test]
    fn plan_serializes_as_self_describing_json() {
        let value = serde_json::to_value(sample_plan()).unwrap();
        assert_eq!(value["command"], "config set");
        assert_eq!(value["dry_run"], serde_json::Value::Bool(true));
        assert_eq!(value["writes_filesystem"], serde_json::Value::Bool(true));
        assert_eq!(value["operations"][0]["kind"], "config.write");
        assert_eq!(value["operations"][0]["details"][0]["label"], "Config file");
    }

    #[test]
    fn empty_operations_are_serialized_as_an_empty_array() {
        let value = serde_json::to_value(DryRunPlan::new("noop", "nothing to do")).unwrap();
        assert_eq!(value["operations"], serde_json::json!([]));
        assert_eq!(value["dry_run"], serde_json::Value::Bool(true));
    }

    #[test]
    fn optional_fields_are_omitted_when_unset() {
        let value = serde_json::to_value(DryRunPlan::new("noop", "nothing")).unwrap();
        assert!(value.get("network").is_none());
        assert!(value.get("costs").is_none());
        assert!(value.get("warnings").is_none());
    }

    #[test]
    fn yes_no_maps_booleans() {
        assert_eq!(yes_no(true), "yes");
        assert_eq!(yes_no(false), "no");
    }
}
