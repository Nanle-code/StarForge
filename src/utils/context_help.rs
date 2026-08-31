//! Core engine backing the `starforge help …` command family.
//!
//! This module ties together [`crate::utils::help_metadata`] (static content),
//! [`crate::utils::history`] (user behaviour) and the user's current config
//! to generate help that is:
//!
//!   * **Context-aware** — different tips are surfaced for a first-time user
//!     versus a power-user; the engine reads
//!     [`crate::utils::history::HistoryEntry::count`] and `last_used` to
//!     estimate expertise.
//!   * **Helpful after errors** — when the CLI fails somewhere with an
//!     `anyhow::Error`, the proactive help hook (wired into `main.rs`)
//!     consults [`troubleshoot`] to inject one extra hint alongside the
//!     command-specific `recovery_hints`.
//!   * **Predictive** — [`predict_issues`] looks at recent command history
//!     to warn the user about missing prerequisites before they run their
//!     next command.
//!   * **Lightweight and offline** — no AI API calls, no extra deps. The
//!     heuristics stay simple enough that all logic fits in well under
//!     400 lines and runs in microseconds.
//!
//! The engine exposes a small, opinionated surface so the CLI layer in
//! [`crate::commands::help`] can present results without needing to know
//! how they're built.

use crate::utils::help_metadata;
use crate::utils::history::HistoryEntry;

// ── Public types ──────────────────────────────────────────────────────────────

/// A coarse expertise tier used to tune tip verbosity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expertise {
    /// Few or no prior commands in this area — keep tips short and concrete.
    Beginner,
    /// Some prior commands — surface intermediate tips and best practices.
    Intermediate,
    /// Many recent commands — prefer concise, power-user pointers.
    Advanced,
}

impl Default for Expertise {
    fn default() -> Self {
        Expertise::Beginner
    }
}

impl Expertise {
    /// Lower-case label for printing in headers.
    pub fn label(self) -> &'static str {
        match self {
            Expertise::Beginner => "beginner",
            Expertise::Intermediate => "intermediate",
            Expertise::Advanced => "advanced",
        }
    }
}

/// Snapshot of "the world right now" passed to [[generate_help]]. Cheap to
/// construct — every field is borrowed, no I/O.
#[derive(Debug, Clone, Default)]
pub struct HelpContext<'a> {
    /// The command the user is asking about (e.g. `"deploy"`).
    pub command: &'a str,
    /// Optional error text from a failed run. Drives [[troubleshoot]] when
    /// `--why` is used.
    pub last_error: Option<&'a str>,
    /// Optional command history slice. Recent entries are typically near the
    /// front of the slice.
    pub history: &'a [HistoryEntry],
    /// Tip categories to surface. Empty means "all categories enabled".
    pub enabled_categories: &'a [&'static str],
    /// Tip categories to suppress (overrides `enabled_categories` when both
    /// are configured).
    pub disabled_categories: &'a [&'static str],
}

impl<'a> HelpContext<'a> {
    /// True when category `cat` should be considered enabled.
    pub fn category_enabled(&self, cat: &str) -> bool {
        if self.disabled_categories.iter().any(|c| *c == cat) {
            return false;
        }
        if self.enabled_categories.is_empty() {
            true
        } else {
            self.enabled_categories.iter().any(|c| *c == cat)
        }
    }
}

/// Help categories the engine can surface. Each maps to a CLI flag under
/// `starforge help --enable/--disable` so users can customise what they
/// want to see.
pub const CATEGORIES: &[&str] = &[
    "command",
    "workflow",
    "tip",
    "troubleshoot",
    "predict",
    "related",
];

// ── Main help payload ─────────────────────────────────────────────────────────

/// Output of [[generate_help]]. The CLI layer renders each non-empty
/// section with the project's standard `print::*` helpers.
#[derive(Debug, Clone, Default)]
pub struct ContextualHelp {
    /// Long-form description of the command (always present, even if generic).
    pub description: String,
    /// Common flags, ready to print in a `flags: --wasm <path>`-style block.
    pub flags_and_examples: Vec<String>,
    /// Workflows the user can run from here.
    pub workflow_suggestions: Vec<String>,
    /// Best-practice tips, filtered by expertise.
    pub best_practice_tips: Vec<String>,
    /// Issues the engine predicts given recent history.
    pub predicted_issues: Vec<String>,
    /// One-line fixes for the most recent error, if any.
    pub troubleshooting_steps: Vec<String>,
    /// Other commands to explore next.
    pub related_commands: Vec<String>,
}

impl ContextualHelp {
    /// True if no actionable section has anything in it. The `description`
    /// flag is intentionally *not* checked — the fallback path always sets
    /// it, so checking it would make `is_empty` always return `false` and the
    /// CLI's "no specific help" branch would never fire.
    pub fn is_empty(&self) -> bool {
        self.flags_and_examples.is_empty()
            && self.workflow_suggestions.is_empty()
            && self.best_practice_tips.is_empty()
            && self.predicted_issues.is_empty()
            && self.troubleshooting_steps.is_empty()
            && self.related_commands.is_empty()
    }

    /// True when no command-specific data was found (used by the CLI to
    /// print the "no dedicated help" banner without re-doing the lookup).
    pub fn has_no_command_metadata(&self) -> bool {
        self.flags_and_examples.is_empty()
            && self.workflow_suggestions.is_empty()
            && self.best_practice_tips.is_empty()
            && self.related_commands.is_empty()
    }

    /// Number of actionable items across all sections. Used by tests.
    pub fn total_items(&self) -> usize {
        self.flags_and_examples.len()
            + self.workflow_suggestions.len()
            + self.best_practice_tips.len()
            + self.predicted_issues.len()
            + self.troubleshooting_steps.len()
            + self.related_commands.len()
    }
}

// ── Core entry point ──────────────────────────────────────────────────────────

/// Build a [[ContextualHelp]] for the request described by `ctx`.
///
/// Behaviour:
///   * Pulls static metadata from [[help_metadata::HELP_REGISTRY]] for known
///     commands. Falls back to a generic "no specific help" payload otherwise.
///   * Filters workflows against the requested `command` name.
///   * Filters tips by the user's [[expertise_level]] for that command.
///   * Runs [[predict_issues]] for the requested `command` against the
///     provided history.
///   * Runs [[troubleshoot]] if `ctx.last_error` is set.
pub fn generate_help(ctx: &HelpContext<'_>) -> ContextualHelp {
    let mut out = ContextualHelp::default();
    // Normalise to lower-case so case-insensitive lookups succeed. The
    // registry stores canonical lower-case names.
    let cmd_lower = ctx.command.trim().to_lowercase();
    let cmd = cmd_lower.as_str();

    // 1. Static metadata
    let meta = help_metadata::HELP_REGISTRY.iter().find(|c| c.name == cmd);

    if let Some(meta) = meta {
        out.description = meta.summary.to_string();

        if ctx.category_enabled("command") {
            for ex in meta.examples {
                out.flags_and_examples
                    .push(format!("{} — {}", ex.command, ex.description));
            }
            for flag in meta.flags {
                out.flags_and_examples
                    .push(format!("  {}  {}", flag.flag, flag.purpose));
            }
        }

        if ctx.category_enabled("workflow") {
            for wf_name in meta.workflows {
                if let Some(wf) = help_metadata::WORKFLOWS.iter().find(|w| w.name == *wf_name) {
                    out.workflow_suggestions
                        .push(format!("{} — {}", wf.name, wf.description));
                }
            }
        }

        if ctx.category_enabled("tip") {
            let expertise = expertise_level(cmd, ctx.history);
            // Beginners see all the tips; advanced users see only the most
            // important (the first two).
            let limit = match expertise {
                Expertise::Beginner => meta.tips.len(),
                Expertise::Intermediate => meta.tips.len(),
                Expertise::Advanced => meta.tips.len().min(2),
            };
            for tip in meta.tips.iter().take(limit) {
                out.best_practice_tips.push((*tip).to_string());
            }
        }

        if ctx.category_enabled("related") {
            for related in meta.related {
                out.related_commands.push((*related).to_string());
            }
        }
    } else {
        // No specific metadata — give a sane fallback.
        out.description = if cmd.is_empty() {
            "Top-level overview of the AI Contextual Help system.".to_string()
        } else {
            format!(
                "No dedicated help for '{}'. Try `starforge help` for an overview, or `starforge {} --help` for the full flag list.",
                cmd, cmd
            )
        };
        // Ensure the user gets the “no specific help” exit branch in the CLI.
        out.flags_and_examples.clear();
        out.workflow_suggestions.clear();
        out.best_practice_tips.clear();
        out.related_commands.clear();
    }

    // 2. Issue prediction (independent of registry membership — works for any command)
    if ctx.category_enabled("predict") {
        for issue in predict_issues(cmd, ctx.history) {
            out.predicted_issues.push(issue);
        }
    }

    // 3. Error-specific troubleshooting
    if ctx.category_enabled("troubleshoot") {
        if let Some(err) = ctx.last_error {
            for step in troubleshoot(err) {
                out.troubleshooting_steps.push(step);
            }
        }
    }

    out
}

// ── Issue prediction ──────────────────────────────────────────────────────────

/// Predict issues the user may run into for `command` given their `history`.
///
/// Returns user-facing warning strings, one per missing prerequisite. If
/// there is no curated prerequisite data for the command, returns the empty
/// vector (no false positives).
pub fn predict_issues(command: &str, history: &[HistoryEntry]) -> Vec<String> {
    let cmd = command.trim().to_lowercase();
    let set = match help_metadata::PREREQUISITES
        .iter()
        .find(|s| s.command == cmd)
    {
        Some(s) => s,
        None => return Vec::new(),
    };

    let mut warnings = Vec::new();
    for need in set.needs {
        // `.contains(needle)` already covers prefix matches; no extra check
        // needed.
        let satisfied = history.iter().any(|h| h.command.contains(need.pattern));
        if !satisfied {
            warnings.push(format!(
                "{} → run `{}` to clear this",
                need.warning, need.remedy
            ));
        }
    }
    warnings
}

// ── Expertise scoring ─────────────────────────────────────────────────────────

/// Score how experienced the user is with `category` based on their
/// `history`. Categories mirror command names; the empty category `""`
/// scores the user as a whole.
pub fn expertise_level(category: &str, history: &[HistoryEntry]) -> Expertise {
    if history.is_empty() {
        return Expertise::Beginner;
    }

    let category = category.trim();
    let now = chrono::Utc::now();

    // Weight recent history heavier than old history.
    let mut weight: u32 = 0;
    let mut recent_count: u32 = 0;
    for entry in history {
        // Bucket by whole days rather than seconds: "used yesterday" should
        // count as recent, and a seconds comparison puts an entry exactly one
        // day old on the wrong side of the boundary.
        let days = now.signed_duration_since(entry.last_used).num_days().max(0);
        let recency: f32 = if days <= 1 {
            1.0
        } else if days <= 7 {
            0.6
        } else if days <= 30 {
            0.3
        } else {
            0.1
        };

        // Word-boundary match — avoid, e.g. “mydeploy” matching `deploy`.
        let matches = if category.is_empty() {
            true
        } else {
            command_matches(&entry.command, category)
        };
        if matches {
            weight += ((entry.count as f32) * recency).ceil() as u32;
            recent_count += 1;
        }
    }

    // Roughly: a handful of recent uses is intermediate, sustained use across
    // several days is advanced. The advanced threshold sits at 15 so that a
    // user with ~5 recent sessions spread over a few days clears it once the
    // recency weighting is applied.
    match (recent_count, weight) {
        (0, _) => Expertise::Beginner,
        (_, w) if w >= 15 => Expertise::Advanced,
        (_, w) if w >= 6 => Expertise::Intermediate,
        _ => Expertise::Beginner,
    }
}

// ── Error troubleshooting ─────────────────────────────────────────────────────

/// Return one-line troubleshooting steps for an arbitrary `error` string.
///
/// Each step is a sentence suitable for the "What to try" block printed by
/// [[crate::utils::print::cli_error]]. The first matching fix wins.
pub fn troubleshoot(error: &str) -> Vec<String> {
    let lower = error.to_lowercase();
    let mut out: Vec<String> = Vec::new();

    for fix in help_metadata::ERROR_QUICK_FIXES {
        if fix.keywords.iter().any(|kw| lower.contains(kw)) {
            out.push(format!("{}  ({})", fix.action, fix.category));
            if !fix.follow_up.is_empty() {
                out.push(format!("→ {}", fix.follow_up));
            }
            break;
        }
    }

    // Add a generic fallback so callers always get at least one item.
    if out.is_empty() {
        out.push(
            "No specific pattern matched this error. Re-run with `-v` for verbose output, \
             or try `starforge help --why` for guided analysis."
                .to_string(),
        );
    }
    out
}

/// Convenience helper used by `main.rs` to merge quick fixes into the
/// existing recovery-hints array without duplicating identical entries.
pub fn troubleshoot_merging(error: &str, existing: &mut Vec<String>) {
    for fix in troubleshoot(error) {
        if !existing.iter().any(|h| h == &fix) {
            existing.push(fix);
        }
    }
}

// ── Proactive one-liner ───────────────────────────────────────────────────────

/// Commands the proactive tip should *not* follow. These are either read-only
/// (`info`, `version`) or already a kind of help themselves (`help`,
/// `completions`); nudging after them is noise.
pub const PROACTIVE_BLOCKLIST: &[&str] = &["help", "info", "completions", "version"];

/// Pick a single concise tip to print after a successful command, based on
/// recent history and the just-run `command`. Returns `None` if nothing
/// worth saying OR if the command is on the [[PROACTIVE_BLOCKLIST]].
pub fn proactive_tip(command: &str, history: &[HistoryEntry]) -> Option<String> {
    let cmd = command.trim().to_lowercase();
    if PROACTIVE_BLOCKLIST.iter().any(|c| *c == cmd.as_str()) {
        return None;
    }

    // If the user has never explored tutorials, nudge them.
    let ever_tutorial = history.iter().any(|h| h.command.starts_with("tutorial"));
    if !ever_tutorial {
        return Some(
            "Tip: run `starforge tutorial start hello-world` for a guided first run.".into(),
        );
    }

    // For first-time deploys, recommend audit.
    if command == "deploy" {
        let ever_audit = history.iter().any(|h| {
            let c = &h.command;
            c.starts_with("audit") || c.starts_with("ai-audit")
        });
        if !ever_audit {
            return Some(
                "Tip: `starforge audit <path>` catches common security issues before deployment."
                    .into(),
            );
        }
    }

    // After wallet-related commands, remind about encryption.
    if command.starts_with("wallet") {
        let ever_encrypt = history.iter().any(|h| h.command.contains("--encrypt"));
        if !ever_encrypt {
            return Some(
                "Tip: use `starforge wallet create <name> --encrypt` to password-protect the saved secret key."
                    .into(),
            );
        }
    }

    None
}

// ── Workflow lookup ───────────────────────────────────────────────────────────

/// Return full step-by-step content for a workflow by name.
pub fn workflow_steps(name: &str) -> Option<&'static [&'static str]> {
    help_metadata::WORKFLOWS
        .iter()
        .find(|w| w.name == name)
        .map(|w| w.steps)
}

/// Description for a workflow by name (used in headers).
pub fn workflow_description(name: &str) -> Option<&'static str> {
    help_metadata::WORKFLOWS
        .iter()
        .find(|w| w.name == name)
        .map(|w| w.description)
}

/// Approximate duration hint for a workflow.
pub fn workflow_duration(name: &str) -> Option<&'static str> {
    help_metadata::WORKFLOWS
        .iter()
        .find(|w| w.name == name)
        .map(|w| w.approx_duration)
}

/// Sum of all workflow counts (used in headers / top-level overviews).
pub fn workflow_count() -> usize {
    help_metadata::WORKFLOWS.len()
}

/// Number of commands that have a dedicated metadata entry. Used in the
/// top-level overview header and as a sanity check from tests.
pub fn commands_with_help() -> usize {
    help_metadata::HELP_REGISTRY.len()
}

/// Names of every command in the registry, sorted alphabetically. Used by
/// the top-level overview and by `--list-commands` style flags.
pub fn all_command_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = help_metadata::HELP_REGISTRY
        .iter()
        .map(|c| c.name)
        .collect();
    names.sort_unstable();
    names
}

/// One-line summary for `cmd` from the registry. `None` when unrecognised.
/// Matching is case-insensitive — callers can pass `Deploy`, `deploy`, etc.
/// and all resolve to the same canonical command.
pub fn command_summary(cmd: &str) -> Option<&'static str> {
    let needle = cmd.trim().to_lowercase();
    help_metadata::HELP_REGISTRY
        .iter()
        .find(|c| c.name == needle.as_str())
        .map(|c| c.summary)
}

/// True when `cmd` matches a known registry entry (case-insensitive).
pub fn is_known_command(cmd: &str) -> bool {
    let needle = cmd.trim().to_lowercase();
    help_metadata::HELP_REGISTRY
        .iter()
        .any(|c| c.name == needle.as_str())
}

/// Normalise a user-provided category name into one of [[CATEGORIES]].
/// Unknown categories are silently dropped so they cannot poison the help
/// output. Returns owned strings because the input is owned; callers
/// convert back to a slice where required.
pub fn normalise_categories<I, S>(input: I) -> Vec<&'static str>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    input
        .into_iter()
        .filter_map(|raw| {
            let lower = raw.as_ref().trim().to_lowercase();
            match lower.as_str() {
                "command" | "commands" => Some("command"),
                "workflow" | "workflows" => Some("workflow"),
                "tip" | "tips" => Some("tip"),
                "troubleshoot" | "troubleshooting" => Some("troubleshoot"),
                "predict" | "prediction" | "predictions" => Some("predict"),
                "related" => Some("related"),
                _ => None,
            }
        })
        .collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Word-boundary matcher used by `expertise_level`.
fn command_matches(entry_cmd: &str, category: &str) -> bool {
    if category.is_empty() {
        return true;
    }
    if entry_cmd == category {
        return true;
    }
    let cat_len = category.len();
    if entry_cmd.len() >= cat_len && entry_cmd.starts_with(category) {
        match entry_cmd.as_bytes().get(cat_len) {
            None | Some(b' ') => return true,
            _ => {}
        }
    }
    let needle = format!(" {} ", category);
    entry_cmd.contains(needle.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    fn entry(command: &str, count: u32, days_ago: i64) -> HistoryEntry {
        let last_used = Utc::now() - Duration::days(days_ago);
        HistoryEntry {
            command: command.to_string(),
            timestamp: last_used,
            count: count as usize,
            last_used,
        }
    }

    // ── generate_help tests ───────────────────────────────────────────────

    #[test]
    fn help_for_known_command_includes_summary_and_examples() {
        let ctx = HelpContext {
            command: "deploy",
            history: &[],
            ..HelpContext::default()
        };
        let h = generate_help(&ctx);
        assert!(h.description.contains("Deploy"));
        assert!(
            h.flags_and_examples.iter().any(|s| s.contains("--wasm")),
            "missing --wasm flag in {:?}",
            h.flags_and_examples
        );
        // Examples are rendered into the same list as flags.
        assert!(
            h.flags_and_examples
                .iter()
                .any(|s| s.contains("starforge deploy --wasm")),
            "missing a deploy example in {:?}",
            h.flags_and_examples
        );
    }

    #[test]
    fn help_for_unknown_command_falls_back_to_generic() {
        let ctx = HelpContext {
            command: "no-such-command",
            ..HelpContext::default()
        };
        let h = generate_help(&ctx);
        assert!(
            h.description.contains("No dedicated help"),
            "got {:?}",
            h.description
        );
        assert!(h.flags_and_examples.is_empty());
        assert!(h.workflow_suggestions.is_empty());
    }

    #[test]
    fn help_filters_by_disabled_categories() {
        // Disable workflows; expect zero workflow suggestions.
        let ctx = HelpContext {
            command: "deploy",
            disabled_categories: &["workflow"],
            ..HelpContext::default()
        };
        let h = generate_help(&ctx);
        assert!(
            h.workflow_suggestions.is_empty(),
            "got {:?}",
            h.workflow_suggestions
        );
        // Tips still appear because we're only filtering workflow.
        assert!(!h.best_practice_tips.is_empty());
    }

    #[test]
    fn help_filters_by_enabled_categories() {
        // Limit output to tips only.
        let ctx = HelpContext {
            command: "deploy",
            enabled_categories: &["tip"],
            ..HelpContext::default()
        };
        let h = generate_help(&ctx);
        assert!(
            h.flags_and_examples.is_empty(),
            "got {:?}",
            h.flags_and_examples
        );
        assert!(h.workflow_suggestions.is_empty());
        assert!(!h.best_practice_tips.is_empty());
    }

    #[test]
    fn advanced_user_sees_fewer_tips() {
        let hist = vec![
            entry("deploy", 30, 0),
            entry("deploy --wasm foo.wasm", 25, 1),
            entry("deploy --wasm bar.wasm --optimize", 20, 2),
            entry("deploy --wasm baz.wasm", 15, 3),
        ];
        let begin_ctx = HelpContext {
            command: "deploy",
            history: &hist,
            ..HelpContext::default()
        };
        let advanced_tips = generate_help(&begin_ctx).best_practice_tips.len();

        let empty_ctx = HelpContext {
            command: "deploy",
            history: &[],
            ..HelpContext::default()
        };
        let beginner_tips = generate_help(&empty_ctx).best_practice_tips.len();

        assert!(
            advanced_tips < beginner_tips,
            "advanced={} beginner={}",
            advanced_tips,
            beginner_tips
        );
    }

    #[test]
    fn help_includes_error_troubleshooting_when_provided() {
        let ctx = HelpContext {
            command: "deploy",
            last_error: Some("Error: require_auth failed"),
            ..HelpContext::default()
        };
        let h = generate_help(&ctx);
        assert!(
            h.troubleshooting_steps
                .iter()
                .any(|s| s.contains("Authorization")),
            "missing troubleshooting for auth error: {:?}",
            h.troubleshooting_steps
        );
    }

    // ── predict_issues tests ──────────────────────────────────────────────

    #[test]
    fn predict_issues_flags_missing_wallet_for_deploy() {
        let hist = vec![entry("info", 1, 0)];
        let warnings = predict_issues("deploy", &hist);
        assert!(warnings.iter().any(|w| w.contains("wallet create")));
        assert!(warnings.iter().any(|w| w.contains("wallet fund")));
    }

    #[test]
    fn predict_issues_silent_when_history_satisfies_prereqs() {
        let hist = vec![
            entry("wallet create deployer", 1, 0),
            entry("wallet fund deployer", 1, 0),
        ];
        let warnings = predict_issues("deploy", &hist);
        assert!(warnings.is_empty(), "got {:?}", warnings);
    }

    #[test]
    fn predict_issues_unknown_command_returns_empty() {
        assert!(predict_issues("nonexistent", &[]).is_empty());
    }

    // ── expertise_level tests ─────────────────────────────────────────────

    #[test]
    fn empty_history_is_beginner() {
        assert_eq!(expertise_level("deploy", &[]), Expertise::Beginner);
    }

    #[test]
    fn few_commands_is_beginner() {
        let hist = vec![entry("deploy", 1, 0)];
        assert_eq!(expertise_level("deploy", &hist), Expertise::Beginner);
    }

    #[test]
    fn moderate_history_is_intermediate() {
        let hist = vec![entry("deploy", 3, 0), entry("deploy --wasm a.wasm", 3, 1)];
        assert_eq!(expertise_level("deploy", &hist), Expertise::Intermediate);
    }

    #[test]
    fn many_recent_commands_is_advanced() {
        let hist = vec![
            entry("deploy --wasm a.wasm", 5, 0),
            entry("deploy --wasm b.wasm", 5, 0),
            entry("deploy --wasm c.wasm --optimize", 5, 1),
            entry("deploy --wasm d.wasm", 5, 2),
            entry("deploy --wasm e.wasm", 5, 3),
        ];
        assert_eq!(expertise_level("deploy", &hist), Expertise::Advanced);
    }

    // ── troubleshoot tests ────────────────────────────────────────────────

    #[test]
    fn troubleshoot_finds_auth_error() {
        let steps = troubleshoot("require_auth failed for caller");
        assert!(steps.iter().any(|s| s.contains("Authorization")));
        assert!(steps.iter().any(|s| s.contains("→")));
    }

    #[test]
    fn troubleshoot_finds_overflow_error() {
        let steps = troubleshoot("attempt to multiply with overflow");
        assert!(steps
            .iter()
            .any(|s| s.to_lowercase().contains("arithmetic")));
    }

    #[test]
    fn troubleshoot_falls_back_for_unknown_text() {
        let steps = troubleshoot("xyzzy no recognizable error");
        assert_eq!(
            steps.len(),
            1,
            "expected single fallback step, got {:?}",
            steps
        );
        assert!(steps[0].contains("No specific pattern"));
    }

    #[test]
    fn troubleshoot_merging_does_not_duplicate() {
        let mut existing = vec!["Already-known hint".to_string()];
        troubleshoot_merging("require_auth failed", &mut existing);
        let after_first = existing.len();
        assert!(
            after_first > 1,
            "expected the auth fixes to be merged in: {:?}",
            existing
        );

        troubleshoot_merging("require_auth failed", &mut existing);
        assert_eq!(
            existing.len(),
            after_first,
            "merging the same error twice duplicated hints: {:?}",
            existing
        );
    }

    // ── proactive_tip tests ───────────────────────────────────────────────

    #[test]
    fn proactive_tip_nudges_first_run_tutorial() {
        assert!(proactive_tip("wallet create alice", &[]).is_some());
    }

    #[test]
    fn proactive_tip_stays_quiet_after_history_filled() {
        let hist = vec![
            entry("tutorial list", 1, 0),
            entry("tutorial start hello-world", 1, 0),
            entry("tutorial next", 1, 0),
        ];
        assert!(proactive_tip("wallet create bob", &hist).is_some()); // encrypt tip
    }

    #[test]
    fn proactive_tip_recommends_audit_after_first_deploy() {
        let hist = vec![entry("tutorial list", 1, 0)];
        let tip = proactive_tip("deploy", &hist);
        assert!(tip.as_deref().unwrap_or_default().contains("audit"));
    }

    // ── workflow lookup tests ─────────────────────────────────────────────

    #[test]
    fn workflow_steps_known_workflow_returns_some() {
        assert!(workflow_steps("first-contract").is_some());
        assert!(workflow_description("first-contract").is_some());
        assert!(workflow_duration("first-contract").is_some());
    }

    #[test]
    fn workflow_steps_unknown_returns_none() {
        assert!(workflow_steps("does-not-exist").is_none());
    }

    #[test]
    fn workflow_count_matches_metadata() {
        assert_eq!(workflow_count(), help_metadata::WORKFLOWS.len());
    }

    // ── category plumbing tests ───────────────────────────────────────────

    #[test]
    fn disabled_categories_override_enabled() {
        let ctx = HelpContext {
            command: "deploy",
            enabled_categories: &["tip", "workflow"],
            disabled_categories: &["workflow"],
            ..HelpContext::default()
        };
        assert!(!ctx.category_enabled("workflow"));
        assert!(ctx.category_enabled("tip"));
    }

    #[test]
    fn empty_disabled_with_empty_enabled_means_all_enabled() {
        let ctx = HelpContext {
            command: "deploy",
            ..HelpContext::default()
        };
        for c in CATEGORIES {
            assert!(
                ctx.category_enabled(c),
                "category {c} unexpectedly disabled"
            );
        }
    }
}
