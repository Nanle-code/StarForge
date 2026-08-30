//! Central detection for non-interactive/CI environments.
//!
//! `starforge` has several prompting helpers (password/passphrase entry,
//! confirmation prompts, registry login) that read from stdin. Left
//! unguarded, those calls hang forever when run in CI or any other
//! non-interactive context (piped stdin, no controlling terminal). Every
//! prompting helper should call [`is_non_interactive`] first and, if true,
//! fail fast with [`ensure_interactive`] instead of blocking — pointing the
//! caller at the secure input alternative (an env var or CLI flag) that
//! lets automated pipelines supply the value headlessly.

use std::env;
use std::io::IsTerminal;

/// Set (to any truthy value) to force non-interactive mode, mirroring the
/// `--non-interactive` CLI flag. Also doubles as the storage for that flag,
/// following the same env-var-as-global-state convention as
/// `STARFORGE_OUTPUT_JSON` in [`crate::utils::output`].
pub const ENV_NON_INTERACTIVE: &str = "STARFORGE_NON_INTERACTIVE";

/// Record the `--non-interactive` flag resolved by the global CLI parser.
/// Called once from `main()` before any command runs. A `false` here does
/// **not** clear the env var — an already-set `STARFORGE_NON_INTERACTIVE=1`
/// (or a CI/non-tty environment, detected lazily in [`is_non_interactive`])
/// must still take effect even when the flag itself wasn't passed.
pub fn set_non_interactive(enabled: bool) {
    if enabled {
        env::set_var(ENV_NON_INTERACTIVE, "1");
    }
}

fn truthy_env(key: &str) -> bool {
    env::var(key)
        .ok()
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

/// True when prompting for input would block indefinitely or silently
/// consume piped data instead of a real answer:
/// - `--non-interactive` was passed, or `STARFORGE_NON_INTERACTIVE` is set
/// - `CI` is set (the de facto standard most CI providers export)
/// - stdin isn't attached to a terminal (piped, redirected, or closed)
pub fn is_non_interactive() -> bool {
    truthy_env(ENV_NON_INTERACTIVE) || env::var_os("CI").is_some() || !stdin_is_terminal()
}

fn stdin_is_terminal() -> bool {
    std::io::stdin().is_terminal()
}

/// Human-readable reason `is_non_interactive` returned true, for error
/// messages. Returns `None` when the environment is interactive.
pub fn non_interactive_reason() -> Option<&'static str> {
    if truthy_env(ENV_NON_INTERACTIVE) {
        Some("--non-interactive was set")
    } else if env::var_os("CI").is_some() {
        Some("the CI environment variable is set")
    } else if !stdin_is_terminal() {
        Some("stdin is not a terminal")
    } else {
        None
    }
}

/// Fail fast with a clear, actionable error if the environment is
/// non-interactive. `what` names the value that would otherwise be prompted
/// for (e.g. `"a wallet password"`); `alternative` tells the caller how to
/// supply it headlessly (e.g. `"Set STARFORGE_PASSWORD."`).
pub fn ensure_interactive(what: &str, alternative: &str) -> anyhow::Result<()> {
    if is_non_interactive() {
        let reason = non_interactive_reason().unwrap_or("running in a non-interactive environment");
        anyhow::bail!("Refusing to prompt for {what}: {reason}. {alternative}");
    }
    Ok(())
}

/// Serializes tests (in this module and elsewhere in the crate) that mutate
/// the process-wide `CI` / `STARFORGE_NON_INTERACTIVE` env vars, so parallel
/// test threads don't race on them.
#[cfg(test)]
pub(crate) fn env_test_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    &LOCK
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clear_env() {
        env::remove_var(ENV_NON_INTERACTIVE);
        env::remove_var("CI");
    }

    #[test]
    fn detects_explicit_non_interactive_flag() {
        let _guard = env_test_lock().lock().unwrap();
        clear_env();
        env::set_var(ENV_NON_INTERACTIVE, "1");
        assert!(is_non_interactive());
        assert_eq!(non_interactive_reason(), Some("--non-interactive was set"));
        clear_env();
    }

    #[test]
    fn detects_ci_env_var() {
        let _guard = env_test_lock().lock().unwrap();
        clear_env();
        env::set_var("CI", "true");
        assert!(is_non_interactive());
        assert_eq!(
            non_interactive_reason(),
            Some("the CI environment variable is set")
        );
        clear_env();
    }

    #[test]
    fn set_non_interactive_records_the_flag() {
        let _guard = env_test_lock().lock().unwrap();
        clear_env();
        set_non_interactive(true);
        assert!(is_non_interactive());
        clear_env();
    }

    #[test]
    fn set_non_interactive_false_does_not_clear_existing_state() {
        let _guard = env_test_lock().lock().unwrap();
        clear_env();
        env::set_var("CI", "1");
        set_non_interactive(false);
        assert!(
            is_non_interactive(),
            "an existing CI signal must survive a false flag"
        );
        clear_env();
    }

    #[test]
    fn ensure_interactive_fails_fast_with_a_clear_message() {
        let _guard = env_test_lock().lock().unwrap();
        clear_env();
        env::set_var(ENV_NON_INTERACTIVE, "1");
        let err = ensure_interactive("a wallet password", "Set STARFORGE_PASSWORD.")
            .unwrap_err()
            .to_string();
        assert!(err.contains("a wallet password"));
        assert!(err.contains("STARFORGE_PASSWORD"));
        assert!(err.contains("--non-interactive was set"));
        clear_env();
    }
}
