use anyhow::Result;
use serde::Serialize;
use std::env;

#[derive(Serialize)]
pub struct JsonErrorEnvelope {
    pub code: String,
    pub message: String,
    pub cause: String,
    pub fix: String,
    pub docs: String,
    pub exit_code: i32,
}

#[derive(Serialize)]
pub struct JsonEnvelope<T> {
    version: u64,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonErrorEnvelope>,
}

pub fn set_json_mode(enabled: bool) {
    if enabled {
        env::set_var("STARFORGE_OUTPUT_JSON", "1");
        return;
    }

    // Respect an inherited environment setting. The JSON mode flag is a global
    // opt-in and should not silently wipe a caller-provided
    // `STARFORGE_OUTPUT_JSON` value that is already in effect.
    if env::var_os("STARFORGE_OUTPUT_JSON").is_none() {
        env::remove_var("STARFORGE_OUTPUT_JSON");
    }
}

/// Enable/leave plain output mode: no ANSI color, no decorative Unicode
/// symbols (`✓`/`✗`/`⚠`/`→`) in `utils::print`'s helpers. See
/// `is_plain_mode_enabled` for the full set of ways this can turn on.
pub fn set_plain_mode(enabled: bool) {
    if enabled {
        env::set_var("STARFORGE_PLAIN", "1");
        return;
    }
    if env::var_os("STARFORGE_PLAIN").is_none() {
        env::remove_var("STARFORGE_PLAIN");
    }
}

/// Whether output should avoid color and decorative Unicode symbols, for a
/// screen reader, a braille display, a log file, or a terminal that a user
/// has told every other tool to stop coloring for.
///
/// True when any of, checked in this order:
/// 1. `--plain` was passed (sets `STARFORGE_PLAIN`).
/// 2. `NO_COLOR` is set to anything non-empty — the cross-tool convention
///    <https://no-color.org>, respected here even though it only asks for
///    "no color": a screen reader gains nothing from `✓`/`✗`/`⚠` either, and
///    a caller who set `NO_COLOR` is exactly the caller this mode is for.
/// 3. `STARFORGE_NO_COLOR` is truthy, the project-namespaced equivalent.
pub fn is_plain_mode_enabled() -> bool {
    if truthy_env("STARFORGE_PLAIN") {
        return true;
    }
    if env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
        return true;
    }
    truthy_env("STARFORGE_NO_COLOR")
}

fn truthy_env(name: &str) -> bool {
    env::var(name)
        .ok()
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

pub fn is_json_mode_enabled() -> bool {
    truthy_env("STARFORGE_OUTPUT_JSON")
}

pub fn print_json<T: Serialize>(value: &T) -> Result<()> {
    let envelope = JsonEnvelope {
        version: 1,
        ok: true,
        data: Some(serde_json::to_value(value)?),
        error: None,
    };
    let rendered = serde_json::to_string_pretty(&envelope)?;
    let redacted = crate::utils::redaction::redact_secrets(&rendered);
    println!("{redacted}");
    Ok(())
}

fn error_json_envelope(
    code: crate::utils::errors::ErrorCode,
    message: &str,
) -> JsonEnvelope<()> {
    let explanation: crate::utils::errors::ErrorExplanation = code.into();
    JsonEnvelope::<()> {
        version: 1,
        ok: false,
        data: None,
        error: Some(JsonErrorEnvelope {
            code: explanation.code,
            message: message.to_string(),
            cause: explanation.cause.to_string(),
            fix: explanation.fix.to_string(),
            docs: explanation.docs,
            exit_code: explanation.exit_code,
        }),
    }
}

pub fn print_error_json(code: crate::utils::errors::ErrorCode, message: &str) -> Result<()> {
    let envelope = error_json_envelope(code, message);
    let rendered = serde_json::to_string_pretty(&envelope)?;
    let redacted = crate::utils::redaction::redact_secrets(&rendered);
    eprintln!("{redacted}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_error_contains_stable_code_and_remediation() {
        let envelope = error_json_envelope(
            crate::utils::errors::ErrorCode::DeploySubmissionFailed,
            "transaction rejected",
        );
        let value = serde_json::to_value(envelope).unwrap();
        assert_eq!(value["error"]["code"], "SF1203");
        assert!(value["error"]["cause"].is_string());
        assert!(value["error"]["fix"].is_string());
        assert!(value["error"]["docs"].as_str().unwrap().contains("ERRORS.md#sf1203"));
        assert_eq!(value["error"]["exit_code"], 6);
    }

    /// Every test in this module mutates process-wide env vars
    /// (`STARFORGE_OUTPUT_JSON`, `STARFORGE_PLAIN`, `STARFORGE_NO_COLOR`,
    /// `NO_COLOR`), and Rust's default test runner executes tests within a
    /// module concurrently by default. Without serialization, two tests
    /// setting/clearing different values race and produce spurious
    /// failures. Mirrors `crate::utils::lock_home_env`'s pattern for the
    /// same problem with `$HOME`.
    fn lock_output_mode_env() -> std::sync::MutexGuard<'static, ()> {
        static OUTPUT_MODE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        OUTPUT_MODE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Clears every env var any test in this module sets, so each test
    /// starts from a known "nothing set" baseline regardless of what an
    /// earlier test (or the invoking shell) left behind.
    fn clear_output_mode_env() {
        env::remove_var("STARFORGE_OUTPUT_JSON");
        env::remove_var("STARFORGE_PLAIN");
        env::remove_var("STARFORGE_NO_COLOR");
        env::remove_var("NO_COLOR");
    }

    #[test]
    fn truthy_values_enable_json_mode() {
        let _guard = lock_output_mode_env();
        clear_output_mode_env();
        for value in ["1", "true", "TRUE", "yes", "on"] {
            env::set_var("STARFORGE_OUTPUT_JSON", value);
            assert!(is_json_mode_enabled());
        }
        clear_output_mode_env();
    }

    #[test]
    fn falsy_values_disable_json_mode() {
        let _guard = lock_output_mode_env();
        clear_output_mode_env();
        for value in ["0", "false", "no", "off", ""] {
            env::set_var("STARFORGE_OUTPUT_JSON", value);
            assert!(!is_json_mode_enabled());
        }
        clear_output_mode_env();
    }

    #[test]
    fn inherited_json_env_is_preserved_when_flag_is_not_set() {
        let _guard = lock_output_mode_env();
        clear_output_mode_env();
        env::set_var("STARFORGE_OUTPUT_JSON", "1");
        set_json_mode(false);
        assert!(is_json_mode_enabled());
        clear_output_mode_env();
    }

    #[test]
    fn plain_mode_is_off_by_default() {
        let _guard = lock_output_mode_env();
        clear_output_mode_env();
        assert!(!is_plain_mode_enabled());
    }

    #[test]
    fn set_plain_mode_true_enables_it() {
        let _guard = lock_output_mode_env();
        clear_output_mode_env();
        set_plain_mode(true);
        assert!(is_plain_mode_enabled());
        clear_output_mode_env();
    }

    #[test]
    fn no_color_env_enables_plain_mode_even_when_flag_absent() {
        let _guard = lock_output_mode_env();
        clear_output_mode_env();
        // no-color.org only requires NO_COLOR be set to something, not "1"
        // or "true" specifically.
        env::set_var("NO_COLOR", "1");
        assert!(is_plain_mode_enabled());
        clear_output_mode_env();
    }

    #[test]
    fn empty_no_color_does_not_enable_plain_mode() {
        let _guard = lock_output_mode_env();
        clear_output_mode_env();
        env::set_var("NO_COLOR", "");
        assert!(!is_plain_mode_enabled());
        clear_output_mode_env();
    }

    #[test]
    fn starforge_no_color_env_enables_plain_mode() {
        let _guard = lock_output_mode_env();
        clear_output_mode_env();
        env::set_var("STARFORGE_NO_COLOR", "1");
        assert!(is_plain_mode_enabled());
        clear_output_mode_env();
    }

    #[test]
    fn inherited_plain_env_is_preserved_when_flag_is_not_set() {
        let _guard = lock_output_mode_env();
        clear_output_mode_env();
        env::set_var("STARFORGE_PLAIN", "1");
        set_plain_mode(false);
        assert!(is_plain_mode_enabled());
        clear_output_mode_env();
    }

    #[test]
    fn success_json_response_has_stable_envelope() {
        let payload = serde_json::json!({"name": "wallet", "count": 2});
        let rendered = serde_json::to_string(&JsonEnvelope {
            version: 1,
            ok: true,
            data: Some(payload.clone()),
            error: None,
        })
        .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(parsed["version"], 1);
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["data"]["name"], "wallet");
        assert!(parsed.get("error").map_or(true, serde_json::Value::is_null));
    }

    #[test]
    fn error_json_response_has_stable_envelope() {
        let rendered = serde_json::to_string(&JsonEnvelope::<serde_json::Value> {
            version: 1,
            ok: false,
            data: None,
            error: Some(JsonErrorEnvelope {
                code: "invalid_input".to_string(),
                message: "unsupported network".to_string(),
            }),
        })
        .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(parsed["version"], 1);
        assert_eq!(parsed["ok"], false);
        assert_eq!(parsed["error"]["code"], "invalid_input");
        assert_eq!(parsed["error"]["message"], "unsupported network");
    }
}
