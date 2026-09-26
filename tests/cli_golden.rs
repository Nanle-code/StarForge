//! Golden-output CLI tests for `starforge`.
//!
//! These tests pin the *observable* output of the built binary — stdout,
//! stderr, and the exit code — against committed snapshots under `tests/cmd/`.
//! Any change to help text, success output, or error output therefore shows up
//! as an explicit diff in the pull request instead of slipping through review.
//!
//! ## Why not `trycmd` / `snapbox`?
//!
//! `Cargo.lock` is checked in and CI asserts `git diff --exit-code Cargo.lock`,
//! so the test-only crates that normally drive this workflow (`trycmd`,
//! `snapbox`, `assert_cmd`, `insta`) cannot be added without failing that gate.
//! This module is a small, dependency-free re-implementation that mirrors the
//! `trycmd` workflow (`TRYCMD=overwrite`) using only `std` plus dependencies
//! the crate already has (`toml`, `serde`, `tempfile`).
//!
//! ## Corpus layout
//!
//! ```text
//! tests/cmd/<case>.toml     # case definition: args, optional stdin/env/timeout
//! tests/cmd/<case>.stdout   # expected stdout (raw)
//! tests/cmd/<case>.stderr   # expected stderr (raw)
//! tests/cmd/<case>.code     # expected exit code, e.g. "0\n"
//! ```
//!
//! A `<case>.toml` file looks like:
//!
//! ```toml
//! description = "wallet --help"
//! args = ["wallet", "--help"]
//! # Optional keys:
//! # stdin = "y\n"
//! # timeout_secs = 30
//! # [env]
//! # NO_COLOR = "0"
//! ```
//!
//! ## Generating / refreshing snapshots
//!
//! A case whose snapshot file is missing is *generated and then passed* (with a
//! notice on stderr), so a freshly added case never turns CI red before its
//! snapshot has been committed. When an intentional output change makes a
//! committed snapshot stale, refresh the whole corpus with:
//!
//! ```bash
//! TRYCMD=overwrite cargo test --test cli_golden
//! # equivalents: GOLDEN_OVERWRITE=1 ... / UPDATE_SNAPSHOTS=1 ...
//! ```
//!
//! Review the resulting diff (`git diff -- tests/cmd`) before committing it.
//!
//! ## Determinism
//!
//! Every invocation runs with an isolated, empty HOME/config tree, a scrubbed
//! environment (`STARFORGE_*`, `XDG_*`, `COLUMNS`, colour toggles), no attached
//! terminal, and a hard timeout. Captured output is normalized (CRLF -> LF,
//! ANSI stripped, isolated paths -> `$HOME`) so snapshots stay stable across
//! operating systems and CI runners.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

/// Exit code reported when the child is killed by a signal (or on timeout).
const SIGNAL_EXIT_CODE: i32 = -1;

/// Default per-invocation timeout, in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Environment variables that would otherwise leak the Developer's real
/// machine / terminal into the snapshot.
const SCRUBBED_ENV: [&str; 10] = [
    "COLUMNS",
    "TERM",
    "NO_COLOR",
    "CLICOLOR",
    "CLICOLOR_FORCE",
    "FORCE_COLOR",
    "XDG_CONFIG_HOME",
    "XDG_CACHE_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
];

/// One golden case, deserialized from `tests/cmd/<case>.toml`.
#[derive(Debug, Deserialize)]
struct Case {
    /// Human-readable description (used only in failure messages).
    #[serde(default)]
    description: String,
    /// Arguments passed to the `starforge` binary.
    args: Vec<String>,
    /// Optional stdin payload; the pipe is closed after it is written.
    #[serde(default)]
    stdin: Option<String>,
    /// Extra environment variables layered on top of the deterministic baseline.
    #[serde(default)]
    env: BTreeMap<String, String>,
    /// Optional per-case timeout override.
    #[serde(default)]
    timeout_secs: Option<u64>,
}

/// Captured, normalized output of one CLI invocation.
struct RunResult {
    stdout: String,
    stderr: String,
    code: i32,
    timed_out: bool,
}

/// Directory holding the `*.toml` cases and their sidecar snapshots.
fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cmd")
}

/// True when the caller asked to (re)write snapshots instead of comparing.
fn overwrite_mode() -> bool {
    if let Ok(value) = std::env::var("TRYCMD") {
        let value = value.trim().to_ascii_lowercase();
        if matches!(value.as_str(), "overwrite" | "1" | "true" | "yes") {
            return true;
        }
    }
    ["GOLDEN_OVERWRITE", "UPDATE_SNAPSHOTS"].iter().any(|key| {
        std::env::var(key).is_ok_and(|value| {
            let value = value.trim().to_ascii_lowercase();
            !(value.is_empty() || value == "0" || value == "false" || value == "no")
        })
    })
}

/// Remove ANSI CSI escape sequences so colour never leaks into a snapshot.
fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                // Consume until the final byte of the CSI sequence.
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(ch);
    }
    out
}

/// Normalize captured output so snapshots are stable across machines.
fn normalize(bytes: &[u8], home: &Path) -> String {
    let text = String::from_utf8_lossy(bytes);
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = strip_ansi(&text);

    let mut text = text;
    // Replace the canonical (symlink-resolved) path first, then the literal one,
    // so both `/var/folders/...` and `/private/var/folders/...` (macOS) collapse
    // to the same placeholder.
    if let Ok(canonical) = std::fs::canonicalize(home) {
        let canonical = canonical.to_string_lossy().to_string();
        if !canonical.is_empty() {
            text = text.replace(&canonical, "$HOME");
            text = text.replace(&canonical.replace('\\', "/"), "$HOME");
        }
    }
    let literal = home.to_string_lossy().to_string();
    if !literal.is_empty() {
        text = text.replace(&literal, "$HOME");
        text = text.replace(&literal.replace('\\', "/"), "$HOME");
    }
    text
}

/// Build a deterministic, isolated environment for one invocation.
fn isolate_env(cmd: &mut Command, home: &Path) {
    for key in SCRUBBED_ENV {
        cmd.env_remove(key);
    }
    // Drop every inherited `STARFORGE_*` override before setting our own.
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("STARFORGE_") {
            cmd.env_remove(&key);
        }
    }

    let tmp = home.join("tmp");
    let _ = std::fs::create_dir_all(&tmp);

    cmd.env("HOME", home);
    cmd.env("USERPROFILE", home);
    cmd.env("XDG_CONFIG_HOME", home.join(".config"));
    cmd.env("XDG_CACHE_HOME", home.join(".cache"));
    cmd.env("XDG_DATA_HOME", home.join(".local/share"));
    cmd.env("XDG_STATE_HOME", home.join(".local/state"));
    cmd.env("APPDATA", home.join("AppData/Roaming"));
    cmd.env("LOCALAPPDATA", home.join("AppData/Local"));
    cmd.env("TMPDIR", &tmp);
    cmd.env("TEMP", &tmp);
    cmd.env("TMP", &tmp);

    // StarForge-specific isolation: `STARFORGE_CONFIG_DIR` is what actually
    // isolates the CLI on Windows, where `dirs::home_dir()` ignores HOME.
    cmd.env("STARFORGE_CONFIG_DIR", home.join(".starforge"));
    cmd.env("STARFORGE_NON_INTERACTIVE", "1");

    // Deterministic, colour-free, locale-independent output.
    cmd.env("NO_COLOR", "1");
    cmd.env("TERM", "dumb");
    cmd.env("LANG", "C");
    cmd.env("LC_ALL", "C");
}

/// Run one case with a hard timeout and capture its normalized output.
fn run_case(binary: &Path, case: &Case) -> RunResult {
    let home = tempfile::tempdir().expect("create isolated home for case");

    let mut cmd = Command::new(binary);
    cmd.args(&case.args);
    isolate_env(&mut cmd, home.path());
    for (key, value) in &case.env {
        cmd.env(key, value);
    }
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"));
    if case.stdin.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .unwrap_or_else(|err| panic!("failed to spawn {}: {err}", binary.display()));

    // Feed stdin from a helper thread so a large payload cannot deadlock
    // against the stdout/stderr readers below.
    if let Some(input) = case.stdin.clone() {
        if let Some(mut sink) = child.stdin.take() {
            std::thread::spawn(move || {
                let _ = sink.write_all(input.as_bytes());
                let _ = sink.flush();
            });
        }
    }

    let mut stdout_pipe = child.stdout.take().expect("child stdout is piped");
    let mut stderr_pipe = child.stderr.take().expect("child stderr is piped");
    let stdout_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout_pipe.read_to_end(&mut buf);
        buf
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut buf);
        buf
    });

    let timeout = Duration::from_secs(case.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS));
    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let code = loop {
        match child.try_wait().expect("try_wait on child") {
            Some(status) => break status.code().unwrap_or(SIGNAL_EXIT_CODE),
            None => {
                if Instant::now() >= deadline {
                    timed_out = true;
                    let _ = child.kill();
                    let _ = child.wait();
                    break SIGNAL_EXIT_CODE;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    };

    let raw_stdout = stdout_reader.join().unwrap_or_default();
    let raw_stderr = stderr_reader.join().unwrap_or_default();

    RunResult {
        stdout: normalize(&raw_stdout, home.path()),
        stderr: normalize(&raw_stderr, home.path()),
        code,
        timed_out,
    }
}

/// Render a compact line-level diff for a failing snapshot comparison.
fn render_diff(expected: &str, actual: &str) -> String {
    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();
    let max = expected_lines.len().max(actual_lines.len());
    let mut lines = Vec::new();
    for index in 0..max {
        let expected_line = expected_lines.get(index).copied();
        let actual_line = actual_lines.get(index).copied();
        if expected_line != actual_line {
            lines.push(format!(
                "  line {}:\n    expected: {:?}\n    actual:   {:?}",
                index + 1,
                expected_line,
                actual_line
            ));
            if lines.len() >= 20 {
                lines.push("  ... (further differences truncated)".to_string());
                break;
            }
        }
    }
    if lines.is_empty() {
        lines.push("  (no line-level differences; check trailing whitespace/newlines)".to_string());
    }
    lines.join("\n")
}

#[test]
fn golden_cli_corpus() {
    let dir = corpus_dir();
    let binary = Path::new(env!("CARGO_BIN_EXE_starforge"));
    let overwrite = overwrite_mode();

    let mut cases: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("cannot read golden corpus dir {}: {err}", dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
        .collect();
    cases.sort();

    assert!(
        !cases.is_empty(),
        "no golden cases (*.toml) found in {}",
        dir.display()
    );

    let mut failures: Vec<String> = Vec::new();
    let mut generated = 0usize;
    let mut compared = 0usize;

    for path in &cases {
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("case")
            .to_string();
        let source = std::fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
        let case: Case =
            toml::from_str(&source).unwrap_or_else(|err| panic!("parse {}: {err}", path.display()));
        let label = if case.description.is_empty() {
            name.clone()
        } else {
            format!("{name}: {}", case.description)
        };

        let RunResult {
            stdout,
            stderr,
            code,
            timed_out,
        } = run_case(binary, &case);

        if timed_out {
            failures.push(format!(
                "[{label}] timed out after {}s (args: {:?})",
                case.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS),
                case.args
            ));
            continue;
        }

        let outputs = [
            ("stdout", stdout),
            ("stderr", stderr),
            ("code", format!("{code}\n")),
        ];

        for (extension, actual) in outputs {
            let snapshot = dir.join(format!("{name}.{extension}"));
            let exists = snapshot.exists();

            if overwrite || !exists {
                std::fs::write(&snapshot, actual.as_bytes())
                    .unwrap_or_else(|err| panic!("write snapshot {}: {err}", snapshot.display()));
                if overwrite {
                    eprintln!("[cli_golden] updated {}", snapshot.display());
                } else {
                    eprintln!(
                        "[cli_golden] generated missing snapshot {} \
                         (review and commit it, or run `TRYCMD=overwrite cargo test --test cli_golden`)",
                        snapshot.display()
                    );
                    generated += 1;
                }
                continue;
            }

            let expected = std::fs::read_to_string(&snapshot)
                .unwrap_or_else(|err| panic!("read snapshot {}: {err}", snapshot.display()));
            if expected == actual {
                compared += 1;
            } else {
                failures.push(format!(
                    "[{label}] {extension} differs from {}\n{}",
                    snapshot.display(),
                    render_diff(&expected, &actual)
                ));
            }
        }
    }

    eprintln!(
        "[cli_golden] {} case(s): {} snapshot comparison(s) matched, {} missing snapshot(s) generated",
        cases.len(),
        compared,
        generated
    );

    if !failures.is_empty() {
        panic!(
            "cli golden snapshot mismatch(es):\n\n{}\n\n\
             If the change is intentional, refresh the corpus with \
             `TRYCMD=overwrite cargo test --test cli_golden`, review `git diff -- tests/cmd`, \
             and commit the result.",
            failures.join("\n\n")
        );
    }
}
