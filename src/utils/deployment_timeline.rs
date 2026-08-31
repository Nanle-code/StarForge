//! Deployment timeline views for in-flight deployments.
//!
//! Operators frequently wait through slow finalization windows after a
//! transaction leaves the node. This module renders a *glanceable* phased
//! timeline that shows where a deployment sits — build, upload, submit, RPC
//! confirm, finalized — and how RPC polling is making progress toward
//! finalization.
//!
//! Design notes:
//!
//! - **Bounded retries.** [`poll_with_retries`] drives a caller-supplied poll
//!   function for at most `max_retries` attempts. Exceeding the budget returns
//!   an [`anyhow::Error`] with an *actionable* hint rather than a bare
//!   failure.
//! - **TTY vs non-TTY.** [`render`] produces a box-drawing ASCII timeline when
//!   the caller reports a TTY, and a stream of JSON events when it does not.
//!   Detection is left to the caller (`std::io::IsTerminal`) so tests can
//!   exercise both paths without a real terminal.
//! - **Correlation IDs.** Every status line carries the correlation ID so a
//!   human or an aggregator can join it to the rest of the invocation's log
//!   stream.

use std::fmt;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::utils::correlation::current_str;

/// A named stage in a deployment's lifecycle.
///
/// The ordering is fixed and used both to drive progression and to lay out the
/// timeline left-to-right. New phases may be inserted between existing ones
/// without breaking consumers because each phase carries its own ordinal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Build,
    Upload,
    Submit,
    ConfirmRpc,
    Finalize,
}

impl Phase {
    /// All phases in lifecycle order.
    pub const ALL: [Phase; 5] = [
        Phase::Build,
        Phase::Upload,
        Phase::Submit,
        Phase::ConfirmRpc,
        Phase::Finalize,
    ];

    /// Stable ordinal used for ordering and progress computation.
    pub fn ordinal(self) -> u8 {
        match self {
            Phase::Build => 0,
            Phase::Upload => 1,
            Phase::Submit => 2,
            Phase::ConfirmRpc => 3,
            Phase::Finalize => 4,
        }
    }

    /// A short, human readable label for the phase.
    pub fn label(self) -> &'static str {
        match self {
            Phase::Build => "build",
            Phase::Upload => "upload",
            Phase::Submit => "submit",
            Phase::ConfirmRpc => "confirm-rpc",
            Phase::Finalize => "finalize",
        }
    }
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The current state of a single phase on the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseState {
    /// Not reached yet — rendered dimmed/queued.
    Pending,
    /// Actively executing right now — rendered as the moving marker.
    Running,
    /// Completed successfully.
    Done,
    /// Hit a terminal error.
    Failed,
}

/// One row on the timeline: a phase plus its runtime status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelinePhase {
    pub phase: Phase,
    pub state: PhaseState,
    /// The current RPC `getTransaction` status, when known (e.g.
    /// `PENDING`, `SUCCESS`, `NOT_FOUND`, `DUPLICATE`).
    pub rpc_status: Option<String>,
    /// Which poll attempt produced this observation, if any.
    pub poll_attempt: Option<u32>,
    /// Human readable note shown beside the phase.
    pub note: Option<String>,
}

impl TimelinePhase {
    pub fn new(phase: Phase) -> Self {
        Self {
            phase,
            state: PhaseState::Pending,
            rpc_status: None,
            poll_attempt: None,
            note: None,
        }
    }

    pub fn with_state(mut self, state: PhaseState) -> Self {
        self.state = state;
        self
    }

    pub fn with_rpc_status(mut self, status: impl Into<String>) -> Self {
        self.rpc_status = Some(status.into());
        self
    }

    pub fn with_attempt(mut self, attempt: u32) -> Self {
        self.poll_attempt = Some(attempt);
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

/// A complete phased snapshot of an in-flight deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentTimeline {
    pub deployment_id: String,
    pub tx_hash: Option<String>,
    /// Number bound on RPC polling attempts.
    pub max_retries: u32,
    /// Polls actually performed so far.
    pub polls_done: u32,
    pub phases: Vec<TimelinePhase>,
}

impl Default for DeploymentTimeline {
    fn default() -> Self {
        let phases = Phase::ALL.iter().copied().map(TimelinePhase::new).collect();
        Self {
            deployment_id: String::new(),
            tx_hash: None,
            max_retries: 30,
            polls_done: 0,
            phases,
        }
    }
}

impl DeploymentTimeline {
    pub fn new(deployment_id: impl Into<String>) -> Self {
        Self {
            deployment_id: deployment_id.into(),
            ..Self::default()
        }
    }

    pub fn with_tx_hash(mut self, tx_hash: impl Into<String>) -> Self {
        self.tx_hash = Some(tx_hash.into());
        self
    }

    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries.max(1);
        self
    }

    /// Replace the state of a phase, keeping its order.
    pub fn set(&mut self, phase: Phase, state: PhaseState) -> &mut Self {
        if let Some(row) = self.phase_mut(phase) {
            row.state = state;
        }
        self
    }

    pub fn phase_mut(&mut self, phase: Phase) -> Option<&mut TimelinePhase> {
        self.phases.iter_mut().find(|row| row.phase == phase)
    }

    pub fn phase(&self, phase: Phase) -> Option<&TimelinePhase> {
        self.phases.iter().find(|row| row.phase == phase)
    }

    /// Overall progress as a percentage (0–100) based on the furthest phase
    /// that has finished.
    pub fn progress_pct(&self) -> u8 {
        let done_ordinal = self
            .phases
            .iter()
            .filter(|row| matches!(row.state, PhaseState::Done | PhaseState::Failed))
            .map(|row| row.phase.ordinal())
            .max();
        match done_ordinal {
            Some(ord) => (((ord as u16 + 1) * 100) / Phase::ALL.len() as u16) as u8,
            None => 0,
        }
    }

    /// True when any phase is in the failed state.
    pub fn failed(&self) -> bool {
        self.phases.iter().any(|row| row.state == PhaseState::Failed)
    }

    /// True when every phase is done.
    pub fn finalized(&self) -> bool {
        self.phases
            .iter()
            .all(|row| row.state == PhaseState::Done)
    }
}

/// A poll function that returns the current RPC status string for a hash.
///
/// Tests inject fixture-backed pollers here; the CLI uses a real
/// [`crate::utils::soroban::poll_transaction_status`] adapter.
pub type PollFn<'a> = Box<dyn FnMut(&str) -> Result<String> + Send + 'a>;

/// Poll `tx_hash` at most `max_retries` times, sleeping `interval_ms` between
/// attempts, until `is_terminal` returns true for the observed status.
///
/// When the budget is exhausted while still pending this returns an
/// *actionable* error, e.g. one that tells the operator to check the hash with
/// a follow-up command, rather than a bare "timed out".
pub fn poll_with_retries(
    tx_hash: &str,
    max_retries: u32,
    interval_ms: u64,
    mut poll: PollFn<'_>,
    is_terminal: impl Fn(&str) -> bool,
) -> Result<TxPollOutcome> {
    let max_retries = max_retries.max(1);
    for attempt in 1..=max_retries {
        let status = poll(tx_hash)
            .with_context(|| format!("RPC poll attempt {}/{} failed for hash {}", attempt, max_retries, tx_hash))?;

        if is_terminal(&status) {
            return Ok(TxPollOutcome {
                status,
                attempts: attempt,
                max_retries,
                timed_out: false,
            });
        }

        if attempt < max_retries {
            std::thread::sleep(std::time::Duration::from_millis(interval_ms));
        }
    }

    anyhow::bail!(
        "transaction {} still pending after {} RPC polls ({} s). \
         Re-check with a follow-up command or inspect network finalization status before resubmitting.",
        tx_hash,
        max_retries,
        max_retries as u64 * interval_ms / 1000,
    )
}

/// The result of a completed polling sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxPollOutcome {
    /// Terminal RPC status observed.
    pub status: String,
    /// How many attempts were made before reaching the terminal status.
    pub attempts: u32,
    pub max_retries: u32,
    /// False on success; true implies we exited via the budget path.
    pub timed_out: bool,
}

/// One JSON event on the non-TTY render path. Each event carries the phase and
/// the correlation ID so a log aggregator can reconstruct the timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub deployment_id: String,
    pub phase: Phase,
    pub state: PhaseState,
    pub rpc_status: Option<String>,
    pub poll_attempt: Option<u32>,
    pub correlation_id: String,
    pub note: Option<String>,
}

/// Options that shape how a timeline is rendered.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// When true render a box-drawing ASCII timeline; otherwise render JSON.
    pub tty: bool,
    /// The correlation ID to stamp onto every status line / event.
    pub correlation_id: String,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            tty: true,
            correlation_id: current_str().to_string(),
        }
    }
}

/// Render a phased timeline for `timeline`.
///
/// - `tty == true`: human friendly box-drawing timeline with phase markers,
///   RPC polling state, attempt counts, and the correlation ID in the header.
/// - `tty == false`: one JSON object per phase (and a summary object), each
///   stamped with the correlation ID — suitable for `| jq` pipelines.
pub fn render(timeline: &DeploymentTimeline, opts: &RenderOptions) -> String {
    if opts.tty {
        render_tty(timeline, opts)
    } else {
        render_json(timeline, opts)
    }
}

fn render_tty(timeline: &DeploymentTimeline, opts: &RenderOptions) -> String {
    use colored::*;

    let mut out = String::new();
    let marker = match (timeline.failed(), timeline.finalized()) {
        (true, _) => "✗".red().bold(),
        (_, true) => "✓".green().bold(),
        _ => "▶".cyan().bold(),
    };

    out.push_str(&format!(
        "\n  {} {}\n",
        marker,
        format!("Deployment timeline — {}", timeline.deployment_id).white().bold()
    ));
    if let Some(hash) = &timeline.tx_hash {
        out.push_str(&format!("  {}\n", format!("tx {}", hash).dimmed()));
    }
    out.push_str(&format!(
        "  {}\n",
        format!("correlation_id={}", opts.correlation_id).dimmed()
    ));

    let bar_width = 24usize;
    let filled = ((timeline.progress_pct() as usize) * bar_width / 100).min(bar_width);
    let filled_bar: String = "█".repeat(filled).cyan().to_string();
    let empty_bar: String = "-".repeat(bar_width - filled).dimmed().to_string();
    out.push_str(&format!(
        "  [{}{}] {:3}%\n",
        filled_bar, empty_bar, timeline.progress_pct()
    ));
    out.push_str(&format!("  {}\n", "─".repeat(48).dimmed()));

    for row in &timeline.phases {
        let mark = match row.state {
            PhaseState::Done => "●".to_string(),
            PhaseState::Running => "▶".to_string(),
            PhaseState::Failed => "✗".to_string(),
            PhaseState::Pending => "○".to_string(),
        };
        let label = row.phase.label();
        let label_color = match row.state {
            PhaseState::Done => label.green(),
            PhaseState::Running => label.cyan(),
            PhaseState::Failed => label.red(),
            PhaseState::Pending => label.dimmed(),
        };
        out.push_str(&format!("  {} {:<14}", mark, label_color));

        if let Some(rpc) = &row.rpc_status {
            out.push_str(&format!(" rpc={}", rpc.to_string().yellow()));
        }
        if let Some(a) = row.poll_attempt {
            out.push_str(&format!(" (poll {})", a));
        }
        if let Some(note) = &row.note {
            out.push_str(&format!(" — {}", note.dimmed()));
        }
        out.push('\n');
    }
    out.push_str(&format!("  {} {}\n", "─".repeat(48).dimmed(), opts.correlation_id.dimmed()));
    out.push('\n');
    out
}

fn render_json(timeline: &DeploymentTimeline, opts: &RenderOptions) -> String {
    let mut out = String::new();
    for row in &timeline.phases {
        let event = TimelineEvent {
            deployment_id: timeline.deployment_id.clone(),
            phase: row.phase,
            state: row.state,
            rpc_status: row.rpc_status.clone(),
            poll_attempt: row.poll_attempt,
            correlation_id: opts.correlation_id.clone(),
            note: row.note.clone(),
        };
        out.push_str(&serde_json::to_string(&event).unwrap_or_default());
        out.push('\n');
    }
    // Summary line so a consumer can distinguish a live vs timed-out run.
    let summary = serde_json::json!({
        "deployment_id": timeline.deployment_id,
        "tx_hash": timeline.tx_hash,
        "max_retries": timeline.max_retries,
        "polls_done": timeline.polls_done,
        "progress_pct": timeline.progress_pct(),
        "finalized": timeline.finalized(),
        "failed": timeline.failed(),
        "correlation_id": opts.correlation_id,
    });
    out.push_str(&summary.to_string());
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_terminal_success(s: &str) -> bool {
        s == "SUCCESS"
    }

    #[test]
    fn phases_are_ordered_and_display_cleanly() {
        let ordinals: Vec<u8> = Phase::ALL.iter().map(|p| p.ordinal()).collect();
        let mut sorted = ordinals.clone();
        sorted.sort_unstable();
        assert_eq!(ordinals, sorted);

        assert_eq!(Phase::ConfirmRpc.to_string(), "confirm-rpc");
        assert_eq!(Phase::Finalize.ordinal(), 4);
    }

    #[test]
    fn timeline_progress_and_finalized_flags() {
        let mut tl = DeploymentTimeline::new("dep-abcd1234");
        assert_eq!(tl.progress_pct(), 0);
        assert!(!tl.finalized());

        tl.set(Phase::Build, PhaseState::Done);
        tl.set(Phase::Upload, PhaseState::Done);
        assert!(!tl.finalized());
        assert!(tl.progress_pct() > 0);

        for p in Phase::ALL {
            tl.set(p, PhaseState::Done);
        }
        assert!(tl.finalized());
        assert_eq!(tl.progress_pct(), 100);
    }

    #[test]
    fn failed_phase_flags_the_timeline() {
        let mut tl = DeploymentTimeline::new("dep-ff00000000");
        tl.set(Phase::Submit, PhaseState::Failed);
        assert!(tl.failed());
        assert!(!tl.finalized());
    }

    // ── Polling fixtures ────────────────────────────────────────────────

    /// Fixture: the poller yields `PENDING` a fixed number of times before
    /// reporting the terminal status.
    fn pending_then(statuses: &[&str], times: usize) -> PollFn<'_> {
        let statuses = statuses.to_vec();
        let mut remaining = times;
        Box::new(move |_hash: &str| {
            let s = if remaining > 0 {
                remaining -= 1;
                "PENDING".to_string()
            } else {
                statuses[0].to_string()
            };
            Ok(s)
        })
    }

    #[test]
    fn poll_terminates_when_terminal_status_arrives() {
        let outcome = poll_with_retries(
            "aa11bb22",
            5,
            1,
            pending_then(&["SUCCESS"], 3),
            is_terminal_success,
        )
        .unwrap();

        assert_eq!(outcome.status, "SUCCESS");
        assert_eq!(outcome.attempts, 4);
        assert!(!outcome.timed_out);
    }

    #[test]
    fn poll_succeeds_on_first_attempt() {
        let outcome = poll_with_retries("z9", 5, 1, pending_then(&["SUCCESS"], 0), is_terminal_success)
            .unwrap();
        assert_eq!(outcome.attempts, 1);
        assert_eq!(outcome.status, "SUCCESS");
    }

    #[test]
    fn poll_budget_exhaustion_yields_actionable_error() {
        let err = poll_with_retries(
            "deadbeef",
            3,
            1,
            pending_then(&["SUCCESS"], 100),
            is_terminal_success,
        )
        .unwrap_err();

        let msg = err.to_string();
        assert!(msg.contains("still pending"), "got: {}", msg);
        assert!(msg.contains("deadbeef"), "got: {}", msg);
        assert!(msg.contains("3 RPC polls"), "got: {}", msg);
        assert!(msg.contains("follow-up command"), "msg should be actionable: {}", msg);
    }

    #[test]
    fn poll_propagates_polling_failures() {
        let mut poll = Box::new(|_hash: &str| anyhow::bail!("RPC unreachable"));
        let err = poll_with_retries("aa", 2, 1, poll, is_terminal_success).unwrap_err();
        assert!(err.to_string().contains("RPC poll attempt 1/2"));
    }

    #[test]
    fn poll_with_retries_clamps_max_retries_to_at_least_one() {
        let outcome = poll_with_retries("aa", 0, 1, pending_then(&["SUCCESS"], 0), is_terminal_success)
            .unwrap();
        assert_eq!(outcome.attempts, 1);
    }

    // ── Rendering ───────────────────────────────────────────────────────

    fn sample_timeline() -> DeploymentTimeline {
        let mut tl = DeploymentTimeline::new("dep-00aa11bb22");
        tl.set(Phase::Build, PhaseState::Done);
        tl.set(Phase::Upload, PhaseState::Done);
        tl.set(Phase::Submit, PhaseState::Done);
        {
            let row = tl.phase_mut(Phase::ConfirmRpc).unwrap();
            row.state = PhaseState::Running;
            row.rpc_status = Some("PENDING".to_string());
            row.poll_attempt = Some(2);
            row.note = Some("awaiting finalization".to_string());
        }
        tl.polls_done = 2;
        tl
    }

    #[test]
    fn tty_render_shows_phases_correlation_and_poll_state() {
        let tl = sample_timeline();
        let out = render(
            &tl,
            &RenderOptions {
                tty: true,
                correlation_id: "ci-12345678".to_string(),
            },
        );

        assert!(out.contains("correlation_id=ci-12345678"), "{}", out);
        assert!(out.contains("confirm-rpc"), "{}", out);
        assert!(out.contains("rpc=PENDING"), "{}", out);
        assert!(out.contains("(poll 2)"), "{}", out);
        assert!(out.contains("finalize"), "{}", out);
    }

    #[test]
    fn non_tty_render_emits_json_events_with_correlation_id() {
        let tl = sample_timeline();
        let out = render(
            &tl,
            &RenderOptions {
                tty: false,
                correlation_id: "ci-abcdef1234".to_string(),
            },
        );

        let parsed: Vec<serde_json::Value> = out
            .lines()
            .map(|line| serde_json::from_str(line).expect("each line must be JSON"))
            .collect();

        // 5 phase events + 1 summary object.
        assert_eq!(parsed.len(), 6, "{}", out);

        let confirm = parsed.iter().find(|v| {
            v["phase"] == "confirm_rpc" && v["state"] == "running"
        });
        assert!(confirm.is_some(), "missing running confirm-rpc event: {}", out);
        assert_eq!(confirm.unwrap()["correlation_id"], "ci-abcdef1234");
        assert_eq!(confirm.unwrap()["poll_attempt"], 2);

        let summary = parsed.last().unwrap();
        assert_eq!(summary["deployment_id"], "dep-00aa11bb22");
        assert_eq!(summary["failed"], false);
        assert_eq!(summary["correlation_id"], "ci-abcdef1234");
    }
}
