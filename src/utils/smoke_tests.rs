//! Post-deploy smoke tests declared in the project manifest (#753).
//!
//! A project can list `[[smoke_tests]]` entries in `starforge-project.toml`.
//! `starforge deploy --execute` runs them automatically, and only, after the
//! Stellar CLI has confirmed the deployment and returned a contract ID. This
//! closes the loop between deploy and verification: a contract whose
//! initialisation or configuration is broken is caught immediately instead of
//! by the first user.
//!
//! Classification guarantees:
//! - Invalid smoke test declarations fail manifest validation *before* the
//!   deploy starts, never after a contract is already live.
//! - A smoke failure is reported as [`SmokeTestFailure`], which maps to its
//!   own exit code ([`crate::utils::exit_codes::ExitCode::SmokeTestFailure`])
//!   so CI can tell "the deploy broke" apart from "the deploy worked but the
//!   deployed contract misbehaves". The deployment record stays `Success`:
//!   the contract is on-chain either way.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Per-test timeout used when an entry does not set `timeout_secs`.
pub const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// Upper bound on `timeout_secs`, so a typo cannot hang a pipeline for days.
pub const MAX_TIMEOUT_SECS: u64 = 3600;

/// One `[[smoke_tests]]` entry from the project manifest.
///
/// Exactly one of `command` or `invoke` must be set. Assertions are optional;
/// with none set the test passes when the check exits with code 0.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SmokeTest {
    /// Human-readable name, unique within the manifest.
    pub name: String,
    /// Shell command, run through `sh -c` (or `cmd /C` on Windows) from the
    /// directory containing the manifest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Contract function to invoke on the freshly deployed contract through
    /// `stellar contract invoke`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invoke: Option<InvokeCheck>,
    /// Expected exit code (default `0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_exit: Option<i32>,
    /// Expected stdout, compared after trimming surrounding whitespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_output: Option<String>,
    /// Substring that stdout must contain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_contains: Option<String>,
    /// Timeout in seconds (default [`DEFAULT_TIMEOUT_SECS`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
}

/// A contract function call used as a smoke check.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InvokeCheck {
    /// Contract function name.
    pub function: String,
    /// Arguments passed after the function name, e.g. `["--to", "world"]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
}

impl SmokeTest {
    /// Effective timeout for this test.
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS))
    }

    /// Effective expected exit code for this test.
    pub fn expected_exit(&self) -> i32 {
        self.expect_exit.unwrap_or(0)
    }
}

/// Validate smoke test declarations.
///
/// Runs as part of manifest parsing, so every rule here is enforced before
/// a deploy begins.
pub fn validate(tests: &[SmokeTest]) -> Result<()> {
    let mut seen = HashSet::new();
    for (index, test) in tests.iter().enumerate() {
        let label = if test.name.trim().is_empty() {
            format!("smoke_tests[{}]", index)
        } else {
            format!("smoke test '{}'", test.name)
        };

        if test.name.trim().is_empty() {
            bail!("{}: `name` must not be empty", label);
        }
        if !seen.insert(test.name.as_str()) {
            bail!("{}: duplicate smoke test name", label);
        }
        match (&test.command, &test.invoke) {
            (Some(_), Some(_)) => bail!("{}: set only one of `command` or `invoke`", label),
            (None, None) => bail!("{}: one of `command` or `invoke` is required", label),
            (Some(command), None) if command.trim().is_empty() => {
                bail!("{}: `command` must not be empty", label)
            }
            (None, Some(invoke)) if invoke.function.trim().is_empty() => {
                bail!("{}: `invoke.function` must not be empty", label)
            }
            _ => {}
        }
        if let Some(timeout) = test.timeout_secs {
            if timeout == 0 || timeout > MAX_TIMEOUT_SECS {
                bail!(
                    "{}: `timeout_secs` must be between 1 and {}",
                    label,
                    MAX_TIMEOUT_SECS
                );
            }
        }
    }
    Ok(())
}

/// What the smoke tests run against: the confirmed deployment.
#[derive(Debug, Clone)]
pub struct SmokeContext {
    /// Contract ID returned by the confirmed deploy.
    pub contract_id: String,
    /// Network the contract was deployed to.
    pub network: String,
    /// Stellar CLI identity or public key used as the invoke source.
    pub source: String,
    /// Working directory for `command` checks (the manifest's directory).
    pub workdir: std::path::PathBuf,
}

/// Outcome of a single smoke test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmokeOutcome {
    Passed,
    /// An assertion did not hold; the string says which one and why.
    Failed(String),
    /// The check did not finish within its timeout.
    TimedOut(Duration),
    /// The check could not be started at all (e.g. `stellar` not installed).
    Error(String),
}

impl SmokeOutcome {
    pub fn is_pass(&self) -> bool {
        matches!(self, Self::Passed)
    }

    /// One-line reason for a non-passing outcome.
    pub fn reason(&self) -> Option<String> {
        match self {
            Self::Passed => None,
            Self::Failed(reason) => Some(reason.clone()),
            Self::TimedOut(timeout) => Some(format!("timed out after {}s", timeout.as_secs())),
            Self::Error(reason) => Some(format!("could not run: {}", reason)),
        }
    }
}

/// Result of one smoke test, for reporting.
#[derive(Debug, Clone)]
pub struct SmokeResult {
    pub name: String,
    pub outcome: SmokeOutcome,
    pub duration: Duration,
}

/// Results of a full smoke run.
#[derive(Debug, Clone, Default)]
pub struct SmokeReport {
    pub results: Vec<SmokeResult>,
}

impl SmokeReport {
    pub fn passed(&self) -> usize {
        self.results.iter().filter(|r| r.outcome.is_pass()).count()
    }

    pub fn failed(&self) -> usize {
        self.results.len() - self.passed()
    }

    pub fn all_passed(&self) -> bool {
        self.failed() == 0
    }

    /// Plain-text report lines, each labelled `SMOKE PASS` or `SMOKE FAIL`.
    pub fn lines(&self) -> Vec<String> {
        self.results
            .iter()
            .map(|r| {
                let ms = r.duration.as_millis();
                match r.outcome.reason() {
                    None => format!("SMOKE PASS  {} ({} ms)", r.name, ms),
                    Some(reason) => format!("SMOKE FAIL  {} ({} ms): {}", r.name, ms, reason),
                }
            })
            .collect()
    }
}

/// Error returned when at least one smoke test failed after a confirmed deploy.
///
/// Kept as a distinct type so exit-code classification can recognise it by
/// downcasting instead of guessing from message text.
#[derive(Debug, Clone)]
pub struct SmokeTestFailure {
    pub failed: usize,
    pub total: usize,
    pub contract_id: String,
    pub network: String,
}

impl fmt::Display for SmokeTestFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Post-deploy smoke tests failed ({} of {}). The deployment itself succeeded: \
             contract {} is live on {}.",
            self.failed, self.total, self.contract_id, self.network
        )
    }
}

impl std::error::Error for SmokeTestFailure {}

/// Run every smoke test in order and collect the results.
///
/// All tests run even after a failure, so one report shows everything that is
/// broken.
pub fn run_all(tests: &[SmokeTest], ctx: &SmokeContext) -> SmokeReport {
    let results = tests
        .iter()
        .map(|test| {
            let started = Instant::now();
            let outcome = run_one(test, ctx);
            SmokeResult {
                name: test.name.clone(),
                outcome,
                duration: started.elapsed(),
            }
        })
        .collect();
    SmokeReport { results }
}

/// Turn a report into the command result: `Ok` when everything passed,
/// otherwise a [`SmokeTestFailure`].
pub fn into_result(report: &SmokeReport, ctx: &SmokeContext) -> Result<()> {
    if report.all_passed() {
        return Ok(());
    }
    Err(SmokeTestFailure {
        failed: report.failed(),
        total: report.results.len(),
        contract_id: ctx.contract_id.clone(),
        network: ctx.network.clone(),
    }
    .into())
}

fn run_one(test: &SmokeTest, ctx: &SmokeContext) -> SmokeOutcome {
    let mut cmd = build_command(test, ctx);
    match run_with_timeout(&mut cmd, test.timeout()) {
        Ok(Some(output)) => check_assertions(test, &output),
        Ok(None) => SmokeOutcome::TimedOut(test.timeout()),
        Err(e) => SmokeOutcome::Error(e.to_string()),
    }
}

/// Arguments for `stellar contract invoke` against the deployed contract.
///
/// Built as an argument vector (never a shell string) so manifest values
/// cannot inject extra shell commands.
pub fn build_invoke_args(invoke: &InvokeCheck, ctx: &SmokeContext) -> Vec<String> {
    let mut args = vec![
        "contract".to_string(),
        "invoke".to_string(),
        "--id".to_string(),
        ctx.contract_id.clone(),
        "--source".to_string(),
        ctx.source.clone(),
        "--network".to_string(),
        ctx.network.clone(),
        "--".to_string(),
        invoke.function.clone(),
    ];
    args.extend(invoke.args.iter().cloned());
    args
}

fn build_command(test: &SmokeTest, ctx: &SmokeContext) -> Command {
    let mut cmd = match (&test.command, &test.invoke) {
        (Some(command), _) => shell(command),
        (None, Some(invoke)) => {
            let mut c = Command::new("stellar");
            c.args(build_invoke_args(invoke, ctx));
            c
        }
        // Unreachable after `validate`; fall back to a check that fails loudly.
        (None, None) => shell("exit 127"),
    };
    cmd.current_dir(&ctx.workdir)
        .env("STARFORGE_CONTRACT_ID", &ctx.contract_id)
        .env("STARFORGE_NETWORK", &ctx.network)
        .env("STARFORGE_SOURCE", &ctx.source);
    cmd
}

fn shell(command: &str) -> Command {
    if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(command);
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(command);
        c
    }
}

/// Captured output of a finished check.
#[derive(Debug, Clone)]
struct CheckOutput {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Run `cmd`, returning `Ok(None)` if it exceeded `timeout`.
fn run_with_timeout(cmd: &mut Command, timeout: Duration) -> Result<Option<CheckOutput>> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().context("failed to start smoke test process")?;

    // Drain both pipes on background threads so a chatty check cannot fill
    // the OS pipe buffer and block while we poll for exit.
    let mut child_stdout = child.stdout.take();
    let mut child_stderr = child.stderr.take();
    let stdout_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = child_stdout.as_mut() {
            let _ = s.read_to_end(&mut buf);
        }
        buf
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = child_stderr.as_mut() {
            let _ = s.read_to_end(&mut buf);
        }
        buf
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    // Grandchildren may keep the pipes open; detach the
                    // readers rather than block on them.
                    drop(stdout_reader);
                    drop(stderr_reader);
                    return Ok(None);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(anyhow::anyhow!("failed waiting on smoke test: {}", e)),
        }
    };

    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    Ok(Some(CheckOutput {
        code: status.code(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    }))
}

fn check_assertions(test: &SmokeTest, output: &CheckOutput) -> SmokeOutcome {
    let expected = test.expected_exit();
    if output.code != Some(expected) {
        let got = output
            .code
            .map(|c| c.to_string())
            .unwrap_or_else(|| "terminated by signal".to_string());
        let mut reason = format!("expected exit code {}, got {}", expected, got);
        let stderr = output.stderr.trim();
        if !stderr.is_empty() {
            reason.push_str(&format!(" (stderr: {})", truncate(stderr, 200)));
        }
        return SmokeOutcome::Failed(reason);
    }

    let stdout = output.stdout.trim();
    if let Some(want) = &test.expect_output {
        if stdout != want.trim() {
            return SmokeOutcome::Failed(format!(
                "expected output {:?}, got {:?}",
                want.trim(),
                truncate(stdout, 200)
            ));
        }
    }
    if let Some(needle) = &test.expect_contains {
        if !stdout.contains(needle.as_str()) {
            return SmokeOutcome::Failed(format!(
                "expected output to contain {:?}, got {:?}",
                needle,
                truncate(stdout, 200)
            ));
        }
    }
    SmokeOutcome::Passed
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let head: String = s.chars().take(max_chars).collect();
        format!("{}…", head)
    }
}

/// Directory that `command` checks run from: the manifest's own directory.
pub fn workdir_for_manifest(manifest_path: &Path) -> std::path::PathBuf {
    manifest_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> SmokeContext {
        SmokeContext {
            contract_id: format!("C{}", "A".repeat(55)),
            network: "testnet".to_string(),
            source: "deployer".to_string(),
            workdir: std::env::temp_dir(),
        }
    }

    fn cmd_test(name: &str, command: &str) -> SmokeTest {
        SmokeTest {
            name: name.to_string(),
            command: Some(command.to_string()),
            ..Default::default()
        }
    }

    /// A command that sleeps for a few seconds on either platform.
    fn sleep_command() -> &'static str {
        if cfg!(windows) {
            "ping -n 6 127.0.0.1 >NUL"
        } else {
            "sleep 5"
        }
    }

    // ── Schema validation ─────────────────────────────────────────────────────

    #[test]
    fn valid_declarations_pass_validation() {
        let tests = vec![
            cmd_test("health", "echo ok"),
            SmokeTest {
                name: "hello".to_string(),
                invoke: Some(InvokeCheck {
                    function: "hello".to_string(),
                    args: vec!["--to".to_string(), "world".to_string()],
                }),
                expect_contains: Some("world".to_string()),
                timeout_secs: Some(30),
                ..Default::default()
            },
        ];
        validate(&tests).unwrap();
    }

    #[test]
    fn missing_check_is_rejected() {
        let tests = vec![SmokeTest {
            name: "nothing".to_string(),
            ..Default::default()
        }];
        let err = validate(&tests).unwrap_err().to_string();
        assert!(
            err.contains("one of `command` or `invoke` is required"),
            "{err}"
        );
    }

    #[test]
    fn both_command_and_invoke_is_rejected() {
        let mut test = cmd_test("both", "echo ok");
        test.invoke = Some(InvokeCheck {
            function: "f".to_string(),
            args: vec![],
        });
        let err = validate(&[test]).unwrap_err().to_string();
        assert!(err.contains("only one of"), "{err}");
    }

    #[test]
    fn duplicate_and_empty_names_are_rejected() {
        let err = validate(&[cmd_test("a", "echo"), cmd_test("a", "echo")])
            .unwrap_err()
            .to_string();
        assert!(err.contains("duplicate"), "{err}");

        let err = validate(&[cmd_test("  ", "echo")]).unwrap_err().to_string();
        assert!(err.contains("`name` must not be empty"), "{err}");
    }

    #[test]
    fn out_of_range_timeout_is_rejected() {
        for bad in [0, MAX_TIMEOUT_SECS + 1] {
            let mut test = cmd_test("t", "echo");
            test.timeout_secs = Some(bad);
            assert!(validate(&[test]).is_err(), "timeout {bad} must be rejected");
        }
    }

    #[test]
    fn unknown_fields_are_rejected_by_serde() {
        let result: std::result::Result<SmokeTest, _> =
            toml::from_str("name = \"x\"\ncommand = \"echo\"\nexpect_exitcode = 0\n");
        assert!(
            result.is_err(),
            "typos in assertion keys must not be ignored"
        );
    }

    // ── Invoke argument construction ──────────────────────────────────────────

    #[test]
    fn invoke_args_target_the_deployed_contract() {
        let invoke = InvokeCheck {
            function: "hello".to_string(),
            args: vec!["--to".to_string(), "world; rm -rf /".to_string()],
        };
        let c = ctx();
        let args = build_invoke_args(&invoke, &c);
        let id = args.iter().position(|a| a == "--id").unwrap();
        assert_eq!(args[id + 1], c.contract_id);
        let sep = args.iter().position(|a| a == "--").unwrap();
        assert_eq!(args[sep + 1], "hello");
        // Arguments are passed verbatim as argv entries, never via a shell.
        assert_eq!(args.last().unwrap(), "world; rm -rf /");
    }

    // ── Execution and classification ──────────────────────────────────────────

    #[test]
    fn passing_command_passes() {
        let mut test = cmd_test("echo", "echo hello-smoke");
        test.expect_contains = Some("hello-smoke".to_string());
        let report = run_all(&[test], &ctx());
        assert!(report.all_passed(), "{:?}", report.lines());
        assert!(report.lines()[0].starts_with("SMOKE PASS"));
        into_result(&report, &ctx()).unwrap();
    }

    #[test]
    fn contract_id_is_exported_to_commands() {
        let command = if cfg!(windows) {
            "echo %STARFORGE_CONTRACT_ID%"
        } else {
            "echo $STARFORGE_CONTRACT_ID"
        };
        let mut test = cmd_test("env", command);
        test.expect_output = Some(ctx().contract_id);
        assert!(run_all(&[test], &ctx()).all_passed());
    }

    #[test]
    fn wrong_exit_code_fails_with_reason() {
        let report = run_all(&[cmd_test("exit", "exit 3")], &ctx());
        assert_eq!(report.failed(), 1);
        let line = &report.lines()[0];
        assert!(line.starts_with("SMOKE FAIL"), "{line}");
        assert!(line.contains("expected exit code 0, got 3"), "{line}");
    }

    #[test]
    fn expected_nonzero_exit_can_pass() {
        let mut test = cmd_test("exit", "exit 3");
        test.expect_exit = Some(3);
        assert!(run_all(&[test], &ctx()).all_passed());
    }

    #[test]
    fn output_mismatch_fails() {
        let mut test = cmd_test("out", "echo actual");
        test.expect_output = Some("expected".to_string());
        let report = run_all(&[test], &ctx());
        assert!(report.lines()[0].contains("expected output \"expected\""));
    }

    #[test]
    fn timeout_is_a_smoke_failure() {
        let mut test = cmd_test("slow", sleep_command());
        test.timeout_secs = Some(1);
        let started = Instant::now();
        let report = run_all(&[test], &ctx());
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "timeout not enforced"
        );
        assert_eq!(
            report.results[0].outcome,
            SmokeOutcome::TimedOut(Duration::from_secs(1))
        );
        assert!(report.lines()[0].contains("timed out after 1s"));
    }

    #[test]
    fn all_tests_run_after_a_failure() {
        let report = run_all(
            &[cmd_test("bad", "exit 1"), cmd_test("good", "echo ok")],
            &ctx(),
        );
        assert_eq!(report.results.len(), 2);
        assert_eq!((report.passed(), report.failed()), (1, 1));
    }

    #[test]
    fn failure_error_is_typed_and_says_deploy_succeeded() {
        let report = run_all(&[cmd_test("bad", "exit 1")], &ctx());
        let err = into_result(&report, &ctx()).unwrap_err();
        let failure = err
            .downcast_ref::<SmokeTestFailure>()
            .expect("smoke failures must be a typed error");
        assert_eq!((failure.failed, failure.total), (1, 1));
        let msg = err.to_string();
        assert!(msg.contains("smoke tests failed"), "{msg}");
        assert!(msg.contains("deployment itself succeeded"), "{msg}");
    }

    #[test]
    fn manifest_workdir_is_its_parent() {
        assert_eq!(
            workdir_for_manifest(Path::new("proj/starforge-project.toml")),
            std::path::PathBuf::from("proj")
        );
        assert_eq!(
            workdir_for_manifest(Path::new("starforge-project.toml")),
            std::path::PathBuf::from(".")
        );
    }
}
