use anyhow::Result;
use serde::Serialize;
use std::env;

#[derive(Serialize)]
pub struct JsonErrorEnvelope {
    code: String,
    message: String,
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

pub fn is_json_mode_enabled() -> bool {
    env::var("STARFORGE_OUTPUT_JSON")
        .ok()
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
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

pub fn print_error_json(code: &str, message: &str) -> Result<()> {
    let envelope = JsonEnvelope {
        version: 1,
        ok: false,
        data: None,
        error: Some(JsonErrorEnvelope {
            code: code.to_string(),
            message: message.to_string(),
        }),
    };
    let rendered = serde_json::to_string_pretty(&envelope)?;
    let redacted = crate::utils::redaction::redact_secrets(&rendered);
    eprintln!("{redacted}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truthy_values_enable_json_mode() {
        for value in ["1", "true", "TRUE", "yes", "on"] {
            env::set_var("STARFORGE_OUTPUT_JSON", value);
            assert!(is_json_mode_enabled());
        }
        env::remove_var("STARFORGE_OUTPUT_JSON");
    }

    #[test]
    fn falsy_values_disable_json_mode() {
        for value in ["0", "false", "no", "off", ""] {
            env::set_var("STARFORGE_OUTPUT_JSON", value);
            assert!(!is_json_mode_enabled());
        }
        env::remove_var("STARFORGE_OUTPUT_JSON");
    }

    #[test]
    fn inherited_json_env_is_preserved_when_flag_is_not_set() {
        env::set_var("STARFORGE_OUTPUT_JSON", "1");
        set_json_mode(false);
        assert!(is_json_mode_enabled());
        env::remove_var("STARFORGE_OUTPUT_JSON");
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
        assert!(parsed["error"].is_null() || parsed["error"].is_missing());
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
